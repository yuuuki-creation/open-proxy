//! admin 和 sessions 表：唯一的管理员和他的登录会话。

use sqlx::SqlitePool;

use super::now_ms;

#[derive(Debug, sqlx::FromRow)]
pub struct Admin {
    pub username: String,
    pub password_hash: String,
}

pub async fn get(db: &SqlitePool) -> sqlx::Result<Option<Admin>> {
    sqlx::query_as("SELECT username, password_hash FROM admin WHERE id = 1")
        .fetch_optional(db)
        .await
}

/// 创建管理员；已经有了就返回 false（首次初始化只能做一次）。
pub async fn create(db: &SqlitePool, username: &str, password_hash: &str) -> sqlx::Result<bool> {
    let result = sqlx::query(
        "INSERT INTO admin (id, username, password_hash, updated_at) VALUES (1, ?, ?, ?)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(username)
    .bind(password_hash)
    .bind(now_ms())
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 改密码，同时删除全部会话。
pub async fn set_password(db: &SqlitePool, password_hash: &str) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("UPDATE admin SET password_hash = ?, updated_at = ? WHERE id = 1")
        .bind(password_hash)
        .bind(now_ms())
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions")
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

pub async fn create_session(
    db: &SqlitePool,
    token_hash: &str,
    expires_at: i64,
    ip: &str,
    user_agent: &str,
) -> sqlx::Result<()> {
    let now = now_ms();
    sqlx::query(
        "INSERT INTO sessions (token_hash, created_at, expires_at, last_seen_at, ip, user_agent)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(token_hash)
    .bind(now)
    .bind(expires_at)
    .bind(now)
    .bind(ip)
    .bind(user_agent)
    .execute(db)
    .await?;
    Ok(())
}

/// 找到没过期的会话，返回上次活动时间。
pub async fn find_session(db: &SqlitePool, token_hash: &str) -> sqlx::Result<Option<i64>> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT last_seen_at FROM sessions WHERE token_hash = ? AND expires_at > ?")
            .bind(token_hash)
            .bind(now_ms())
            .fetch_optional(db)
            .await?;
    Ok(row.map(|(t,)| t))
}

pub async fn touch_session(db: &SqlitePool, token_hash: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE token_hash = ?")
        .bind(now_ms())
        .bind(token_hash)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn delete_session(db: &SqlitePool, token_hash: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(token_hash)
        .execute(db)
        .await?;
    Ok(())
}

/// 删掉过期的会话（定时任务里调用）。
pub async fn delete_expired_sessions(db: &SqlitePool) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
        .bind(now_ms())
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}
