//! exits 表：落地出口（第三方 SOCKS5，或选一台服务器当自建落地机）。

use sqlx::SqlitePool;

use super::now_ms;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Exit {
    pub id: i64,
    pub name: String,
    /// `third_party` 或 `self_built`
    pub kind: String,
    /// 第三方出口的地址；自建的为空，用落地机的地址
    pub host: String,
    pub port: i64,
    pub username: String,
    pub password: String,
    pub landing_server_id: Option<i64>,
    pub created_at: i64,
}

const COLUMNS: &str =
    "id, name, kind, host, port, username, password, landing_server_id, created_at";

pub async fn list(db: &SqlitePool) -> sqlx::Result<Vec<Exit>> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM exits ORDER BY id"))
        .fetch_all(db)
        .await
}

pub async fn get(db: &SqlitePool, id: i64) -> sqlx::Result<Option<Exit>> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM exits WHERE id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await
}

pub async fn create(db: &SqlitePool, e: &Exit) -> sqlx::Result<i64> {
    let result = sqlx::query(
        "INSERT INTO exits (name, kind, host, port, username, password, landing_server_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&e.name)
    .bind(&e.kind)
    .bind(&e.host)
    .bind(e.port)
    .bind(&e.username)
    .bind(&e.password)
    .bind(e.landing_server_id)
    .bind(now_ms())
    .execute(db)
    .await?;
    Ok(result.last_insert_rowid())
}

/// 改出口（种类和落地机不在这里改）。
pub async fn update(db: &SqlitePool, e: &Exit) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE exits SET name = ?, host = ?, port = ?, username = ?, password = ? WHERE id = ?",
    )
    .bind(&e.name)
    .bind(&e.host)
    .bind(e.port)
    .bind(&e.username)
    .bind(&e.password)
    .bind(e.id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete(db: &SqlitePool, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM exits WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 每个出口被多少个节点使用：(出口 ID, 节点数)。
pub async fn node_counts(db: &SqlitePool) -> sqlx::Result<Vec<(i64, i64)>> {
    sqlx::query_as("SELECT exit_id, COUNT(*) FROM nodes WHERE exit_id IS NOT NULL GROUP BY exit_id")
        .fetch_all(db)
        .await
}
