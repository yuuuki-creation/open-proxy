//! users 表：拼车的朋友。凭据所有节点通用；流量只存全部累计和本周期起点（database.md）。

use sqlx::SqlitePool;

use super::now_ms;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub remark: String,
    pub plan_id: i64,
    pub enabled: bool,
    pub started_on: String,
    pub expires_on: Option<String>,
    pub uuid: String,
    pub password: String,
    pub ss_key: String,
    pub sub_token: String,
    pub up_total: i64,
    pub down_total: i64,
    pub period_base: i64,
    pub last_reset_at: Option<i64>,
    /// 空 / `manual` / `expired` / `over_quota`
    pub blocked_reason: String,
    pub blocked_since: Option<i64>,
    pub created_at: i64,
}

impl User {
    /// 本周期用量（字节）。
    pub fn period_used(&self) -> i64 {
        (self.up_total + self.down_total - self.period_base).max(0)
    }
}

const COLUMNS: &str = "id, name, remark, plan_id, enabled, started_on, expires_on, uuid, password,
    ss_key, sub_token, up_total, down_total, period_base, last_reset_at, blocked_reason,
    blocked_since, created_at";

pub async fn list(db: &SqlitePool) -> sqlx::Result<Vec<User>> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM users ORDER BY id"))
        .fetch_all(db)
        .await
}

pub async fn get(db: &SqlitePool, id: i64) -> sqlx::Result<Option<User>> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM users WHERE id = ?"))
        .bind(id)
        .fetch_optional(db)
        .await
}

pub async fn by_sub_token(db: &SqlitePool, token: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM users WHERE sub_token = ?"))
        .bind(token)
        .fetch_optional(db)
        .await
}

/// 管理员能改的字段。
pub struct UserFields {
    pub name: String,
    pub remark: String,
    pub plan_id: i64,
    pub enabled: bool,
    pub started_on: String,
    pub expires_on: Option<String>,
}

/// 一套新凭据：UUID、密码、Shadowsocks 密钥、订阅 Token。
pub struct Credentials {
    pub uuid: String,
    pub password: String,
    pub ss_key: String,
    pub sub_token: String,
}

pub async fn create(db: &SqlitePool, f: &UserFields, c: &Credentials) -> sqlx::Result<i64> {
    let result = sqlx::query(
        "INSERT INTO users (name, remark, plan_id, enabled, started_on, expires_on,
             uuid, password, ss_key, sub_token, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&f.name)
    .bind(&f.remark)
    .bind(f.plan_id)
    .bind(f.enabled)
    .bind(&f.started_on)
    .bind(&f.expires_on)
    .bind(&c.uuid)
    .bind(&c.password)
    .bind(&c.ss_key)
    .bind(&c.sub_token)
    .bind(now_ms())
    .execute(db)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn update(db: &SqlitePool, id: i64, f: &UserFields) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE users SET name = ?, remark = ?, plan_id = ?, enabled = ?, started_on = ?, expires_on = ?
         WHERE id = ?",
    )
    .bind(&f.name)
    .bind(&f.remark)
    .bind(f.plan_id)
    .bind(f.enabled)
    .bind(&f.started_on)
    .bind(&f.expires_on)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// 删除用户：日账本和计数器由外键级联删除。
pub async fn delete(db: &SqlitePool, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 清零本周期用量：本周期起点设成当前的全部累计，重置日不变。
pub async fn reset_period(db: &SqlitePool, id: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET period_base = up_total + down_total WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 同时换订阅链接和全部凭据。
pub async fn set_credentials(db: &SqlitePool, id: i64, c: &Credentials) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET uuid = ?, password = ?, ss_key = ?, sub_token = ? WHERE id = ?")
        .bind(&c.uuid)
        .bind(&c.password)
        .bind(&c.ss_key)
        .bind(&c.sub_token)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// 写停用原因：空表示不停用（blocked_since 清空），否则记下开始时间。
pub async fn set_blocked(db: &SqlitePool, id: i64, reason: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE users SET blocked_reason = ?, blocked_since = CASE WHEN ? = '' THEN NULL ELSE ? END
         WHERE id = ?",
    )
    .bind(reason)
    .bind(reason)
    .bind(now_ms())
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// 每月自动重置：本周期起点设成当前累计，记下重置时间。
pub async fn reset_period_at(db: &SqlitePool, id: i64, at: i64) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE users SET period_base = up_total + down_total, last_reset_at = ? WHERE id = ?",
    )
    .bind(at)
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}
