//! 节点：一台服务器上的一个入站。端口不填就在服务器的端口范围里随机分配，
//! 避开已用的端口、Hysteria2 端口跳跃范围和自建落地的端口（nodes.md「端口」）。

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::auth::Admin;
use super::{ApiError, ApiResult, check_address, check_name, check_port, nullable, time};
use crate::app::AppState;
use crate::db::nodes::{self, Node, Params, SS_METHOD};
use crate::db::{exits, is_unique_violation, servers};
use crate::secret;

pub const PROTOCOLS: [&str; 5] = [
    "vless_reality",
    "hysteria2",
    "anytls",
    "shadowsocks2022",
    "mieru",
];

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct PortRange {
    start: i64,
    end: i64,
}

#[derive(Serialize)]
pub struct NodeView {
    id: i64,
    server_id: i64,
    server_name: String,
    name: String,
    protocol: String,
    port: i64,
    hop_ports: Option<PortRange>,
    /// 实际使用的地址：节点单独设置的，或者服务器的
    address: String,
    /// 节点单独设置的地址，没有时为 null
    address_override: Option<String>,
    exit_id: Option<i64>,
    exit_name: Option<String>,
    enabled: bool,
    sort_order: i64,
    reality: Option<RealityView>,
    /// Hysteria2 是否开了 salamander 混淆
    obfs: bool,
    /// 在哪些订阅格式里看不到（客户端 × 协议能力表）
    hidden_in: Vec<&'static str>,
    created_at: String,
}

#[derive(Serialize)]
pub struct RealityView {
    public_key: String,
    short_id: String,
    /// host:port
    target: String,
}

struct Names {
    servers: HashMap<i64, (String, String)>,
    exits: HashMap<i64, String>,
}

async fn names(state: &AppState) -> ApiResult<Names> {
    let servers = servers::list(&state.db)
        .await?
        .into_iter()
        .map(|s| (s.id, (s.name, s.address)))
        .collect();
    let exits = exits::list(&state.db)
        .await?
        .into_iter()
        .map(|e| (e.id, e.name))
        .collect();
    Ok(Names { servers, exits })
}

fn view(n: Node, names: &Names) -> NodeView {
    let params = n.params();
    let hidden_in = crate::subscription::node::hidden_in(&n);
    let (server_name, server_address) =
        names.servers.get(&n.server_id).cloned().unwrap_or_default();
    let reality = (n.protocol == "vless_reality").then(|| RealityView {
        public_key: params.public_key.clone(),
        short_id: params.short_ids.first().cloned().unwrap_or_default(),
        target: format!("{}:{}", params.target_host, params.target_port),
    });
    NodeView {
        id: n.id,
        server_id: n.server_id,
        server_name,
        address: n.address.clone().unwrap_or(server_address),
        address_override: n.address,
        name: n.name,
        protocol: n.protocol,
        port: n.port,
        hop_ports: match (n.hop_port_start, n.hop_port_end) {
            (Some(start), Some(end)) => Some(PortRange { start, end }),
            _ => None,
        },
        exit_name: n.exit_id.and_then(|id| names.exits.get(&id).cloned()),
        exit_id: n.exit_id,
        enabled: n.enabled,
        sort_order: n.sort_order,
        reality,
        obfs: !params.obfs_password.is_empty(),
        hidden_in,
        created_at: time(n.created_at),
    }
}

pub async fn list(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<NodeView>>> {
    let names = names(&state).await?;
    let nodes = nodes::list(&state.db).await?;
    Ok(Json(nodes.into_iter().map(|n| view(n, &names)).collect()))
}

pub async fn get(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<NodeView>> {
    let node = find(&state, id).await?;
    Ok(Json(view(node, &names(&state).await?)))
}

async fn find(state: &AppState, id: i64) -> ApiResult<Node> {
    nodes::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("节点不存在"))
}

#[derive(Deserialize)]
pub struct CreateNode {
    server_id: i64,
    name: String,
    protocol: String,
    #[serde(default)]
    port: Option<i64>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    exit_id: Option<i64>,
    #[serde(default = "yes")]
    enabled: bool,
    /// VLESS + REALITY 必填：伪装目标，host 或 host:port
    #[serde(default)]
    reality_target: Option<String>,
    /// Hysteria2：开 salamander 混淆
    #[serde(default)]
    obfs: bool,
    /// Hysteria2：端口跳跃范围
    #[serde(default)]
    hop_ports: Option<PortRange>,
}

fn yes() -> bool {
    true
}

pub async fn create(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<CreateNode>,
) -> ApiResult<Json<NodeView>> {
    let server = servers::get(&state.db, req.server_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("server_not_found", "服务器不存在"))?;
    let name = check_name(&req.name, "节点名")?;
    if !PROTOCOLS.contains(&req.protocol.as_str()) {
        return Err(ApiError::bad_request("invalid_protocol", "不支持这个协议"));
    }
    let address = check_optional_address(req.address.as_deref())?;
    let exit_id = check_exit(&state, req.exit_id).await?;

    let mut params = Params::default();
    match req.protocol.as_str() {
        "vless_reality" => {
            let target = req.reality_target.as_deref().unwrap_or("");
            let (host, port) = parse_target(target)?;
            let (private_key, public_key) = secret::reality_keypair();
            params.private_key = private_key;
            params.public_key = public_key;
            params.short_ids = vec![secret::reality_short_id()];
            params.target_host = host;
            params.target_port = port;
        }
        "hysteria2" => {
            if req.obfs {
                params.obfs_password = secret::random_password(24);
            }
        }
        "shadowsocks2022" => {
            params.method = SS_METHOD.to_string();
            params.server_key = secret::ss2022_key();
        }
        _ => {}
    }

    let hop = match (req.protocol.as_str(), req.hop_ports) {
        ("hysteria2", Some(range)) => Some(check_hop_range(range)?),
        _ => None,
    };
    let mut occupied = nodes::occupied_ports(&state.db, server.id, None, None).await?;
    if let Some(h) = hop {
        if conflicts((h.start, h.end), &occupied) {
            return Err(ApiError::conflict(
                "port_taken",
                "端口跳跃范围和这台服务器上已用的端口冲突",
            ));
        }
        occupied.push((h.start, h.end));
    }
    let port = match req.port {
        Some(p) => {
            check_port(p, "端口")?;
            if conflicts((p, p), &occupied) {
                return Err(ApiError::conflict(
                    "port_taken",
                    format!("端口 {p} 已被占用"),
                ));
            }
            p
        }
        None => pick_port((server.port_range_start, server.port_range_end), &occupied).ok_or_else(
            || ApiError::conflict("no_free_port", "这台服务器的端口范围里没有空闲端口"),
        )?,
    };

    let node = Node {
        id: 0,
        server_id: server.id,
        name,
        protocol: req.protocol,
        port,
        hop_port_start: hop.map(|h| h.start),
        hop_port_end: hop.map(|h| h.end),
        params: params.to_json(),
        address,
        exit_id,
        enabled: req.enabled,
        sort_order: 0,
        created_at: 0,
    };
    let id = nodes::create(&state.db, &node)
        .await
        .map_err(port_conflict)?;
    tracing::info!(node_id = id, server_id = server.id, protocol = %node.protocol, port, "创建节点");
    state.config_changed();
    Ok(Json(view(find(&state, id).await?, &names(&state).await?)))
}

#[derive(Deserialize)]
pub struct UpdateNode {
    name: Option<String>,
    port: Option<i64>,
    #[serde(default, deserialize_with = "nullable")]
    address: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    exit_id: Option<Option<i64>>,
    enabled: Option<bool>,
    reality_target: Option<String>,
    obfs: Option<bool>,
    #[serde(default, deserialize_with = "nullable")]
    hop_ports: Option<Option<PortRange>>,
}

pub async fn update(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
    Json(req): Json<UpdateNode>,
) -> ApiResult<Json<NodeView>> {
    let mut node = find(&state, id).await?;
    let mut params = node.params();
    if let Some(name) = &req.name {
        node.name = check_name(name, "节点名")?;
    }
    if let Some(address) = &req.address {
        node.address = check_optional_address(address.as_deref())?;
    }
    if let Some(exit_id) = req.exit_id {
        node.exit_id = check_exit(&state, exit_id).await?;
    }
    if let Some(enabled) = req.enabled {
        node.enabled = enabled;
    }
    if let Some(target) = &req.reality_target {
        if node.protocol != "vless_reality" {
            return Err(ApiError::bad_request(
                "invalid_field",
                "只有 VLESS + REALITY 节点有伪装目标",
            ));
        }
        let (host, port) = parse_target(target)?;
        params.target_host = host;
        params.target_port = port;
    }
    if let Some(obfs) = req.obfs {
        if node.protocol != "hysteria2" {
            return Err(ApiError::bad_request(
                "invalid_field",
                "只有 Hysteria2 节点有混淆",
            ));
        }
        if !obfs {
            params.obfs_password.clear();
        } else if params.obfs_password.is_empty() {
            params.obfs_password = secret::random_password(24);
        }
    }
    if let Some(hop) = req.hop_ports {
        if node.protocol != "hysteria2" && hop.is_some() {
            return Err(ApiError::bad_request(
                "invalid_field",
                "只有 Hysteria2 节点有端口跳跃",
            ));
        }
        let hop = hop.map(check_hop_range).transpose()?;
        node.hop_port_start = hop.map(|h| h.start);
        node.hop_port_end = hop.map(|h| h.end);
    }
    if let Some(port) = req.port {
        node.port = check_port(port, "端口")?;
    }

    // 端口和端口跳跃范围不能和这台服务器上的其他占用冲突
    let occupied = nodes::occupied_ports(&state.db, node.server_id, Some(node.id), None).await?;
    if conflicts((node.port, node.port), &occupied) {
        return Err(ApiError::conflict(
            "port_taken",
            format!("端口 {} 已被占用", node.port),
        ));
    }
    if let (Some(start), Some(end)) = (node.hop_port_start, node.hop_port_end)
        && (conflicts((start, end), &occupied) || (start..=end).contains(&node.port))
    {
        return Err(ApiError::conflict(
            "port_taken",
            "端口跳跃范围和这台服务器上已用的端口冲突",
        ));
    }

    node.params = params.to_json();
    nodes::update(&state.db, &node)
        .await
        .map_err(port_conflict)?;
    state.config_changed();
    Ok(Json(view(find(&state, id).await?, &names(&state).await?)))
}

pub async fn delete(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    find(&state, id).await?;
    nodes::delete(&state.db, id).await?;
    tracing::info!(node_id = id, "删除节点");
    state.config_changed();
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
pub struct Order {
    ids: Vec<i64>,
}

/// 拖拽调整顺序后提交整个顺序。
pub async fn set_order(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<Order>,
) -> ApiResult<Json<Value>> {
    nodes::set_order(&state.db, &req.ids).await?;
    state.config_changed();
    Ok(Json(json!({})))
}

fn check_optional_address(address: Option<&str>) -> ApiResult<Option<String>> {
    match address.map(str::trim) {
        Some(a) if !a.is_empty() => Ok(Some(check_address(a)?)),
        _ => Ok(None),
    }
}

async fn check_exit(state: &AppState, exit_id: Option<i64>) -> ApiResult<Option<i64>> {
    match exit_id {
        Some(id) => {
            exits::get(&state.db, id)
                .await?
                .ok_or_else(|| ApiError::bad_request("exit_not_found", "落地出口不存在"))?;
            Ok(Some(id))
        }
        None => Ok(None),
    }
}

/// 解析 REALITY 伪装目标：`host` 或 `host:port`，host 必须是域名，端口默认 443。
fn parse_target(target: &str) -> ApiResult<(String, u16)> {
    let target = target.trim().to_lowercase();
    let (host, port) = match target.rsplit_once(':') {
        Some((host, port)) => (
            host.to_string(),
            port.parse::<u16>().ok().filter(|p| *p != 0),
        ),
        None => (target.clone(), Some(443)),
    };
    match port {
        Some(port) if super::is_hostname(&host) => Ok((host, port)),
        _ => Err(ApiError::bad_request(
            "invalid_reality_target",
            "伪装目标要写成 域名 或 域名:端口，例如 www.example.com:443",
        )),
    }
}

fn check_hop_range(range: PortRange) -> ApiResult<PortRange> {
    check_port(range.start, "端口跳跃范围")?;
    check_port(range.end, "端口跳跃范围")?;
    if range.start >= range.end {
        return Err(ApiError::bad_request(
            "invalid_port_range",
            "端口跳跃范围的起点要小于终点",
        ));
    }
    Ok(range)
}

fn overlaps(a: (i64, i64), b: (i64, i64)) -> bool {
    a.0 <= b.1 && b.0 <= a.1
}

fn conflicts(range: (i64, i64), occupied: &[(i64, i64)]) -> bool {
    occupied.iter().any(|r| overlaps(range, *r))
}

/// 在范围内随机选一个没被占用的端口：先随机试，都撞上了再顺序找。
pub fn pick_port(range: (i64, i64), occupied: &[(i64, i64)]) -> Option<i64> {
    let (lo, hi) = range;
    if lo > hi {
        return None;
    }
    let mut rng = rand::thread_rng();
    for _ in 0..200 {
        let port = rng.gen_range(lo..=hi);
        if !conflicts((port, port), occupied) {
            return Some(port);
        }
    }
    (lo..=hi).find(|p| !conflicts((*p, *p), occupied))
}

fn port_conflict(err: sqlx::Error) -> ApiError {
    if is_unique_violation(&err) {
        ApiError::conflict("port_taken", "这个端口已被这台服务器上的其他节点使用")
    } else {
        err.into()
    }
}
