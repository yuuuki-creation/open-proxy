//! 数据库：SQLite（WAL、外键），带版本号的迁移。表结构见 migrations/ 和 main 分支 database.md。
//! 一张表或一组相关的表一个文件；这里只放读写，不放业务规则。

pub mod admin;
pub mod exits;
pub mod nodes;
pub mod plans;
pub mod servers;
pub mod settings;
pub mod users;

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

/// 打开数据库并执行迁移。
pub async fn open(path: &Path) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .with_context(|| format!("打开数据库 {}", path.display()))?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("执行数据库迁移")?;
    Ok(pool)
}

/// 这个主控认识的最新迁移版本（恢复备份时检查备份是不是来自更新的主控）。
pub fn latest_migration() -> i64 {
    sqlx::migrate!("./migrations")
        .migrations
        .iter()
        .map(|m| m.version)
        .max()
        .unwrap_or(0)
}

/// 当前时间，Unix 毫秒。
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 是不是违反唯一约束（名字重复、端口重复等）。
pub fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(e) if e.is_unique_violation())
}

/// 是不是违反外键约束（还在被引用）。
pub fn is_foreign_key_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(e) if e.is_foreign_key_violation())
}
