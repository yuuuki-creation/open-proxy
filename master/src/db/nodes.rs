//! nodes 表：一个节点就是服务器上的一个入站。

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::now_ms;

/// Shadowsocks 2022 的加密方式，固定（subscription.md：iOS 客户端只支持 aes-128/256-gcm）。
pub const SS_METHOD: &str = "2022-blake3-aes-128-gcm";

/// 节点的协议参数（nodes.params 的 JSON）。所有协议共用一个结构，用不到的字段为空。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Params {
    /// VLESS + REALITY：X25519 私钥和公钥，base64 raw url
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub private_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub public_key: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub short_ids: Vec<String>,
    /// VLESS + REALITY：伪装目标，同时作为 server_name
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_host: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub target_port: u16,
    /// Hysteria2：salamander 混淆密码，空表示不开
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub obfs_password: String,
    /// Shadowsocks 2022：加密方式和服务端主密钥
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub method: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub server_key: String,
}

fn is_zero(v: &u16) -> bool {
    *v == 0
}

impl Params {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Node {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    pub protocol: String,
    pub port: i64,
    pub hop_port_start: Option<i64>,
    pub hop_port_end: Option<i64>,
    /// 各协议自己的参数，JSON，见 `api::nodes::Params`
    pub params: String,
    pub address: Option<String>,
    pub exit_id: Option<i64>,
    pub enabled: bool,
    pub sort_order: i64,
    pub created_at: i64,
}

impl Node {
    /// 解析协议参数；格式坏了时返回空参数（生成期望状态时这个节点会报错）。
    pub fn params(&self) -> Params {
        serde_json::from_str(&self.params).unwrap_or_default()
    }
}

const COLUMNS: &str = "id, server_id, name, protocol, port, hop_port_start, hop_port_end, params,
    address, exit_id, enabled, sort_order, created_at";

/// 全部节点，按订阅里的顺序。
pub async fn list(db: &SqlitePool) -> sqlx::Result<Vec<Node>> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM nodes ORDER BY sort_order, id"
    ))
    .fetch_all(db)
    .await
}

pub async fn get(db: &SqlitePool, id: i64) -> sqlx::Result<Option<Node>> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM nodes WHERE id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// 新节点排在最后。
pub async fn create(db: &SqlitePool, n: &Node) -> sqlx::Result<i64> {
    let result = sqlx::query(
        "INSERT INTO nodes (server_id, name, protocol, port, hop_port_start, hop_port_end, params,
             address, exit_id, enabled, sort_order, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, (SELECT IFNULL(MAX(sort_order), 0) + 1 FROM nodes), ?)",
    )
    .bind(n.server_id)
    .bind(&n.name)
    .bind(&n.protocol)
    .bind(n.port)
    .bind(n.hop_port_start)
    .bind(n.hop_port_end)
    .bind(&n.params)
    .bind(&n.address)
    .bind(n.exit_id)
    .bind(n.enabled)
    .bind(now_ms())
    .execute(db)
    .await?;
    Ok(result.last_insert_rowid())
}

/// 改节点（所属服务器、协议、顺序不在这里改）。
pub async fn update(db: &SqlitePool, n: &Node) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE nodes SET name = ?, port = ?, hop_port_start = ?, hop_port_end = ?, params = ?,
             address = ?, exit_id = ?, enabled = ?
         WHERE id = ?",
    )
    .bind(&n.name)
    .bind(n.port)
    .bind(n.hop_port_start)
    .bind(n.hop_port_end)
    .bind(&n.params)
    .bind(&n.address)
    .bind(n.exit_id)
    .bind(n.enabled)
    .bind(n.id)
    .execute(db)
    .await?;
    Ok(())
}

/// 删除节点；plan_nodes 里的引用由外键级联删除。
pub async fn delete(db: &SqlitePool, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM nodes WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 按给出的 ID 顺序重排；没列出的节点排在后面，保持原来的相对顺序。
pub async fn set_order(db: &SqlitePool, ids: &[i64]) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("UPDATE nodes SET sort_order = sort_order + ?")
        .bind(ids.len() as i64 + 1)
        .execute(&mut *tx)
        .await?;
    for (i, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE nodes SET sort_order = ? WHERE id = ?")
            .bind(i as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// 一台服务器上已经占用的端口段（闭区间）：节点端口、端口跳跃范围、自建落地的 SOCKS5 端口。
/// `except_node`、`except_exit` 是正在修改的节点或自建出口，不算它自己。
pub async fn occupied_ports(
    db: &SqlitePool,
    server_id: i64,
    except_node: Option<i64>,
    except_exit: Option<i64>,
) -> sqlx::Result<Vec<(i64, i64)>> {
    let nodes: Vec<(i64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT port, hop_port_start, hop_port_end FROM nodes WHERE server_id = ? AND id != ?",
    )
    .bind(server_id)
    .bind(except_node.unwrap_or(0))
    .fetch_all(db)
    .await?;
    let mut ranges = Vec::new();
    for (port, hop_start, hop_end) in nodes {
        ranges.push((port, port));
        if let (Some(s), Some(e)) = (hop_start, hop_end) {
            ranges.push((s, e));
        }
    }
    let landing: Vec<(i64,)> =
        sqlx::query_as("SELECT port FROM exits WHERE landing_server_id = ? AND id != ?")
            .bind(server_id)
            .bind(except_exit.unwrap_or(0))
            .fetch_all(db)
            .await?;
    ranges.extend(landing.into_iter().map(|(p,)| (p, p)));
    Ok(ranges)
}
