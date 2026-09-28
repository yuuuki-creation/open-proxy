//! servers、deleted_servers、certificates 表：装了 Agent 的服务器和它的证书。

use sqlx::SqlitePool;

use super::now_ms;

/// servers 表的一行（不含 Token 哈希）。
#[allow(dead_code)] // Agent 和网卡相关的字段在 P3 的 Agent 网关里用
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Server {
    pub id: i64,
    pub name: String,
    pub address: String,
    pub port_range_start: i64,
    pub port_range_end: i64,
    pub cert_mode: String,
    pub cert_domain: Option<String>,
    pub traffic_quota_bytes: Option<i64>,
    pub traffic_reset_day: Option<i64>,
    pub state_version: i64,
    pub state_hash: String,
    pub applied_version: i64,
    pub apply_failures: String,
    pub agent_version: String,
    pub agent_arch: String,
    pub agent_instance_id: i64,
    pub rolled_back_from: String,
    pub last_seen_at: Option<i64>,
    pub nic_boot_id: String,
    pub nic_interface: String,
    pub nic_last_rx: i64,
    pub nic_last_tx: i64,
    pub created_at: i64,
}

const COLUMNS: &str = "id, name, address, port_range_start, port_range_end, cert_mode, cert_domain,
    traffic_quota_bytes, traffic_reset_day, state_version, state_hash, applied_version, apply_failures,
    agent_version, agent_arch, agent_instance_id, rolled_back_from, last_seen_at,
    nic_boot_id, nic_interface, nic_last_rx, nic_last_tx, created_at";

pub async fn list(db: &SqlitePool) -> sqlx::Result<Vec<Server>> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM servers ORDER BY id"))
        .fetch_all(db)
        .await
}

pub async fn get(db: &SqlitePool, id: i64) -> sqlx::Result<Option<Server>> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM servers WHERE id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// 管理员能改的字段。
pub struct ServerFields {
    pub name: String,
    pub address: String,
    pub port_range_start: i64,
    pub port_range_end: i64,
    pub cert_mode: String,
    pub cert_domain: Option<String>,
    pub traffic_quota_bytes: Option<i64>,
    pub traffic_reset_day: Option<i64>,
}

pub async fn create(db: &SqlitePool, f: &ServerFields, token_hash: &str) -> sqlx::Result<i64> {
    let result = sqlx::query(
        "INSERT INTO servers (name, address, port_range_start, port_range_end, cert_mode, cert_domain,
             traffic_quota_bytes, traffic_reset_day, token_hash, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&f.name)
    .bind(&f.address)
    .bind(f.port_range_start)
    .bind(f.port_range_end)
    .bind(&f.cert_mode)
    .bind(&f.cert_domain)
    .bind(f.traffic_quota_bytes)
    .bind(f.traffic_reset_day)
    .bind(token_hash)
    .bind(now_ms())
    .execute(db)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn update(db: &SqlitePool, id: i64, f: &ServerFields) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE servers SET name = ?, address = ?, port_range_start = ?, port_range_end = ?,
             cert_mode = ?, cert_domain = ?, traffic_quota_bytes = ?, traffic_reset_day = ?
         WHERE id = ?",
    )
    .bind(&f.name)
    .bind(&f.address)
    .bind(f.port_range_start)
    .bind(f.port_range_end)
    .bind(&f.cert_mode)
    .bind(&f.cert_domain)
    .bind(f.traffic_quota_bytes)
    .bind(f.traffic_reset_day)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// 换 Agent Token（重新生成安装命令）：旧 Token 立即失效。
pub async fn set_token_hash(db: &SqlitePool, id: i64, token_hash: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE servers SET token_hash = ? WHERE id = ?")
        .bind(token_hash)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 删除服务器：节点、证书跟着删；Token 哈希移到 deleted_servers，通知当时不在线的 Agent 卸载。
pub async fn delete(db: &SqlitePool, id: i64) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT token_hash, name FROM servers WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((token_hash, name)) = row {
        sqlx::query(
            "INSERT INTO deleted_servers (token_hash, name, deleted_at) VALUES (?, ?, ?)
             ON CONFLICT (token_hash) DO NOTHING",
        )
        .bind(&token_hash)
        .bind(&name)
        .bind(now_ms())
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM servers WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// 这台服务器是不是某个自建出口的落地机。
pub async fn landing_exit_of(db: &SqlitePool, id: i64) -> sqlx::Result<Option<(i64, String)>> {
    sqlx::query_as("SELECT id, name FROM exits WHERE landing_server_id = ?")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// 一张证书。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Certificate {
    pub kind: String,
    pub domain: Option<String>,
    pub cert_pem: String,
    pub key_pem: String,
    pub sha256: String,
    pub not_after: i64,
    pub renewed_at: i64,
    pub last_error: String,
}

/// 服务器的证书；`server_id` 为 None 时是主控自己的。
pub async fn certificate(
    db: &SqlitePool,
    server_id: Option<i64>,
) -> sqlx::Result<Option<Certificate>> {
    sqlx::query_as(
        "SELECT kind, domain, cert_pem, key_pem, sha256, not_after, renewed_at, last_error
         FROM certificates WHERE IFNULL(server_id, 0) = ?",
    )
    .bind(server_id.unwrap_or(0))
    .fetch_optional(db)
    .await
}

/// 写入（或替换）一张证书。
pub struct NewCertificate<'a> {
    pub server_id: Option<i64>,
    pub kind: &'a str,
    pub domain: Option<&'a str>,
    pub cert_pem: &'a str,
    pub key_pem: &'a str,
    pub sha256: &'a str,
    pub not_after: i64,
}

pub async fn save_certificate(db: &SqlitePool, c: &NewCertificate<'_>) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM certificates WHERE IFNULL(server_id, 0) = ?")
        .bind(c.server_id.unwrap_or(0))
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO certificates (server_id, kind, domain, cert_pem, key_pem, sha256, not_after, renewed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(c.server_id)
    .bind(c.kind)
    .bind(c.domain)
    .bind(c.cert_pem)
    .bind(c.key_pem)
    .bind(c.sha256)
    .bind(c.not_after)
    .bind(now_ms())
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// 删除服务器的证书（换证书方式时）。
pub async fn delete_certificate(db: &SqlitePool, server_id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM certificates WHERE server_id = ?")
        .bind(server_id)
        .execute(db)
        .await?;
    Ok(())
}

/// 用 Agent Token 的哈希找服务器。
pub async fn by_token_hash(db: &SqlitePool, token_hash: &str) -> sqlx::Result<Option<Server>> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM servers WHERE token_hash = ?"
    ))
    .bind(token_hash)
    .fetch_optional(db)
    .await
}

/// 这个 Token 是不是已删除服务器的。
pub async fn is_deleted_token(db: &SqlitePool, token_hash: &str) -> sqlx::Result<bool> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM deleted_servers WHERE token_hash = ?")
        .bind(token_hash)
        .fetch_optional(db)
        .await?;
    Ok(row.is_some())
}

/// 记下 Agent 最近一次 Hello 的内容。
pub async fn record_hello(
    db: &SqlitePool,
    id: i64,
    version: &str,
    arch: &str,
    rolled_back_from: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE servers SET agent_version = ?, agent_arch = ?, rolled_back_from = ?, last_seen_at = ?
         WHERE id = ?",
    )
    .bind(version)
    .bind(arch)
    .bind(rolled_back_from)
    .bind(now_ms())
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// 记下 Agent 最近一次 StateReport：已应用的版本和失败项（JSON）。
pub async fn record_state_report(
    db: &SqlitePool,
    id: i64,
    applied_version: i64,
    failures: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE servers SET applied_version = ?, apply_failures = ?, last_seen_at = ? WHERE id = ?",
    )
    .bind(applied_version)
    .bind(failures)
    .bind(now_ms())
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// 更新最近一次收到消息的时间。
pub async fn touch(db: &SqlitePool, id: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE servers SET last_seen_at = ? WHERE id = ?")
        .bind(now_ms())
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 期望状态的内容变了：写新版本和内容哈希。
pub async fn set_state(db: &SqlitePool, id: i64, version: i64, hash: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE servers SET state_version = ?, state_hash = ? WHERE id = ?")
        .bind(version)
        .bind(hash)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
