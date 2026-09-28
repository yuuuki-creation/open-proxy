//! 期望状态：每台服务器一份，从数据库现算（database.md「期望状态怎么生成」）。
//! 内容哈希变了才升版本；任何配置变更后等 3 秒合并，再重算所有服务器并推给在线的 Agent。
//! 服务器只有几台，每次都全部重算，不用判断每次改动影响了哪些服务器。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use prost::Message;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::app::AppState;
use crate::db::{self, nodes::Node, servers::Server};
use crate::pb::agentv1 as pb;
use crate::traffic;

/// 变更后等这么久再推送，把连续的变更合并成一次（Hysteria2 每次加人都会断会话）
const DEBOUNCE: Duration = Duration::from_secs(3);

/// 各服务器当前的期望状态（带版本号），Agent 连上时直接拿来推送。
#[derive(Default)]
pub struct Engine {
    cache: RwLock<HashMap<i64, Arc<pb::DesiredState>>>,
    /// 同一时间只做一次重算，免得两次重算交错着升版本
    lock: tokio::sync::Mutex<()>,
}

impl Engine {
    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<i64, Arc<pb::DesiredState>>> {
        self.cache.read().unwrap_or_else(|e| e.into_inner())
    }

    /// 这台服务器当前的期望状态；还没算过（刚创建）时先重算一遍。
    pub async fn current(
        &self,
        state: &AppState,
        server_id: i64,
    ) -> anyhow::Result<Option<Arc<pb::DesiredState>>> {
        if let Some(s) = self.read().get(&server_id) {
            return Ok(Some(s.clone()));
        }
        recompute(state).await?;
        Ok(self.read().get(&server_id).cloned())
    }
}

/// 后台任务：启动时算一遍，之后每次收到「配置变了」就等 3 秒合并，再重算停用状态和期望状态。
pub async fn run(state: AppState) {
    if let Err(err) = refresh(&state).await {
        tracing::error!("计算期望状态出错: {err:#}");
    }
    loop {
        state.changes.notified().await;
        tokio::time::sleep(DEBOUNCE).await;
        if let Err(err) = refresh(&state).await {
            tracing::error!("计算期望状态出错: {err:#}");
        }
    }
}

async fn refresh(state: &AppState) -> anyhow::Result<()> {
    // 停用原因先按规则算好，停用和恢复走的都是「用户在不在期望状态里」
    traffic::update_blocks(&state.db).await?;
    recompute(state).await
}

/// 重算所有服务器的期望状态：内容变了就升版本，推给在线且版本一致的 Agent。
pub async fn recompute(state: &AppState) -> anyhow::Result<()> {
    let _guard = state.states.lock.lock().await;
    let built = build_all(state).await?;
    let mut cache = HashMap::new();
    for (server, mut desired) in built {
        let hash = hex::encode(Sha256::digest(desired.encode_to_vec()));
        let changed = hash != server.state_hash || server.state_version == 0;
        let version = if changed {
            let version = (server.state_version + 1).max(db::now_ms());
            db::servers::set_state(&state.db, server.id, version, &hash).await?;
            tracing::info!(server_id = server.id, version, "期望状态有变化，升版本");
            version
        } else {
            server.state_version
        };
        desired.version = version as u64;
        let desired = Arc::new(desired);
        if changed
            && let Some(agent) = state.hub.get(server.id)
            && agent.sync
            && let Err(err) = agent
                .push(pb::master_message::Body::DesiredState((*desired).clone()))
                .await
        {
            tracing::warn!(
                server_id = server.id,
                "推送期望状态失败，下次连上时补推: {err}"
            );
        }
        cache.insert(server.id, desired);
    }
    *state
        .states
        .cache
        .write()
        .unwrap_or_else(|e| e.into_inner()) = cache;
    Ok(())
}

/// 从数据库算出每台服务器的期望状态（版本号先填 0，用来算哈希）。
async fn build_all(state: &AppState) -> anyhow::Result<Vec<(Server, pb::DesiredState)>> {
    let pool = &state.db;
    let servers = db::servers::list(pool).await?;
    let nodes = db::nodes::list(pool).await?;
    let users = db::users::list(pool).await?;
    let plan_nodes = db::plans::node_ids(pool).await?;
    let exits = db::exits::list(pool).await?;

    // 每个节点上放行的用户：套餐包含这个节点，且没有停用
    let mut node_users: HashMap<i64, BTreeSet<i64>> = HashMap::new();
    for u in users.iter().filter(|u| u.blocked_reason.is_empty()) {
        for node_id in plan_nodes.get(&u.plan_id).into_iter().flatten() {
            node_users.entry(*node_id).or_default().insert(u.id);
        }
    }
    let users_by_id: HashMap<i64, &db::users::User> = users.iter().map(|u| (u.id, u)).collect();
    let servers_by_id: HashMap<i64, &Server> = servers.iter().map(|s| (s.id, s)).collect();

    // 自建落地要放行的来源 IP：用这个出口的节点所在服务器的地址，解析成 IPv4
    let mut resolved: HashMap<String, Vec<String>> = HashMap::new();
    for s in &servers {
        if !resolved.contains_key(&s.address) {
            resolved.insert(s.address.clone(), resolve_ipv4(&s.address).await);
        }
    }

    let mut result = Vec::new();
    for server in &servers {
        let mut my_nodes: Vec<&Node> = nodes
            .iter()
            .filter(|n| n.server_id == server.id && n.enabled)
            .collect();
        my_nodes.sort_by_key(|n| n.id);

        let mut desired = pb::DesiredState::default();
        let mut used_users = BTreeSet::new();
        let mut used_exits = BTreeSet::new();
        let mut needs_cert = false;
        for node in &my_nodes {
            let user_ids: Vec<u64> = node_users
                .get(&node.id)
                .map(|ids| ids.iter().map(|id| *id as u64).collect())
                .unwrap_or_default();
            used_users.extend(user_ids.iter().copied());
            if let Some(exit_id) = node.exit_id {
                used_exits.insert(exit_id);
            }
            needs_cert |= matches!(node.protocol.as_str(), "hysteria2" | "anytls");
            desired.nodes.push(pb::Node {
                id: node.id as u64,
                port: node.port as u32,
                user_ids,
                exit_id: node.exit_id.unwrap_or(0) as u64,
                protocol: node_protocol(node),
            });
        }
        for id in used_users {
            if let Some(u) = users_by_id.get(&(id as i64)) {
                desired.users.push(pb::User {
                    id,
                    uuid: u.uuid.clone(),
                    password: u.password.clone(),
                    ss_key: u.ss_key.clone(),
                });
            }
        }
        for exit in exits.iter().filter(|e| used_exits.contains(&e.id)) {
            let host = match exit.landing_server_id {
                Some(landing) => servers_by_id
                    .get(&landing)
                    .map(|s| s.address.clone())
                    .unwrap_or_default(),
                None => exit.host.clone(),
            };
            desired.exits.push(pb::Exit {
                id: exit.id as u64,
                host,
                port: exit.port as u32,
                username: exit.username.clone(),
                password: exit.password.clone(),
            });
        }
        if let Some(exit) = exits
            .iter()
            .find(|e| e.landing_server_id == Some(server.id))
        {
            // 用这个出口的节点所在的服务器
            let sources: BTreeSet<String> = nodes
                .iter()
                .filter(|n| n.enabled && n.exit_id == Some(exit.id))
                .filter_map(|n| servers_by_id.get(&n.server_id))
                .flat_map(|s| resolved.get(&s.address).cloned().unwrap_or_default())
                .collect();
            desired.landing = Some(pb::Landing {
                port: exit.port as u32,
                username: exit.username.clone(),
                password: exit.password.clone(),
                allowed_source_ips: sources.into_iter().collect(),
            });
        }
        if needs_cert && let Some(cert) = db::servers::certificate(pool, Some(server.id)).await? {
            desired.certificate = Some(pb::Certificate {
                cert_pem: cert.cert_pem,
                key_pem: cert.key_pem,
            });
        }
        result.push((server.clone(), desired));
    }
    Ok(result)
}

fn node_protocol(node: &Node) -> Option<pb::node::Protocol> {
    use pb::node::Protocol;
    let params = node.params();
    Some(match node.protocol.as_str() {
        "vless_reality" => Protocol::VlessReality(pb::VlessReality {
            private_key: params.private_key,
            short_ids: params.short_ids,
            target_host: params.target_host,
            target_port: u32::from(params.target_port),
        }),
        "hysteria2" => Protocol::Hysteria2(pb::Hysteria2 {
            obfs_password: params.obfs_password,
            port_hopping: match (node.hop_port_start, node.hop_port_end) {
                (Some(start), Some(end)) => Some(pb::PortRange {
                    start: start as u32,
                    end: end as u32,
                }),
                _ => None,
            },
        }),
        "anytls" => Protocol::Anytls(pb::AnyTls {}),
        "shadowsocks2022" => Protocol::Shadowsocks2022(pb::Shadowsocks2022 {
            method: params.method,
            server_key: params.server_key,
        }),
        "mieru" => Protocol::Mieru(pb::Mieru {}),
        _ => return None,
    })
}

/// 把地址解析成 IPv4 列表；本身就是 IPv4 时原样返回。解析失败返回空列表并打日志。
async fn resolve_ipv4(address: &str) -> Vec<String> {
    if address.parse::<std::net::Ipv4Addr>().is_ok() {
        return vec![address.to_string()];
    }
    let lookup = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host((address, 0)),
    )
    .await;
    match lookup {
        Ok(Ok(addrs)) => {
            let ips: BTreeSet<String> = addrs
                .filter(|a| a.is_ipv4())
                .map(|a| a.ip().to_string())
                .collect();
            ips.into_iter().collect()
        }
        _ => {
            tracing::warn!(%address, "解析服务器地址失败，自建落地暂时不放行它");
            Vec::new()
        }
    }
}

/// 记下 Agent 的 StateReport：已应用的版本和失败项。
pub async fn record_report(
    state: &AppState,
    server_id: i64,
    report: pb::StateReport,
) -> anyhow::Result<()> {
    let failures: Vec<serde_json::Value> = report
        .failures
        .iter()
        .map(|f| json!({ "item": item_name(f.item), "id": f.id, "reason": f.reason }))
        .collect();
    let level_warn = !failures.is_empty();
    db::servers::record_state_report(
        &state.db,
        server_id,
        report.applied_version as i64,
        &serde_json::to_string(&failures)?,
    )
    .await?;
    if level_warn {
        tracing::warn!(
            server_id,
            version = report.applied_version,
            ?failures,
            "Agent 有应用失败的项"
        );
    } else {
        tracing::info!(
            server_id,
            version = report.applied_version,
            "Agent 已应用期望状态"
        );
    }
    Ok(())
}

fn item_name(item: i32) -> &'static str {
    use pb::apply_failure::Item;
    match Item::try_from(item) {
        Ok(Item::Node) => "node",
        Ok(Item::Exit) => "exit",
        Ok(Item::Landing) => "landing",
        Ok(Item::Certificate) => "certificate",
        Ok(Item::PortHopping) => "port_hopping",
        _ => "unknown",
    }
}

/// 每个用户涉及的服务器里，还没同步到最新期望状态的有几台（面板显示「停用中，还有 N 台没同步」）。
pub async fn pending_servers(state: &AppState) -> anyhow::Result<HashMap<i64, i64>> {
    let servers: BTreeMap<i64, bool> = db::servers::list(&state.db)
        .await?
        .into_iter()
        .map(|s| (s.id, s.applied_version < s.state_version))
        .collect();
    let node_server: HashMap<i64, i64> = db::nodes::list(&state.db)
        .await?
        .into_iter()
        .map(|n| (n.id, n.server_id))
        .collect();
    let plan_nodes = db::plans::node_ids(&state.db).await?;
    let mut result = HashMap::new();
    for u in db::users::list(&state.db).await? {
        let pending: BTreeSet<i64> = plan_nodes
            .get(&u.plan_id)
            .into_iter()
            .flatten()
            .filter_map(|n| node_server.get(n))
            .filter(|s| servers.get(s).copied().unwrap_or(false))
            .copied()
            .collect();
        result.insert(u.id, pending.len() as i64);
    }
    Ok(result)
}
