//! settings 表：全局设置，值存 JSON。用到的键见 database.md「settings」。

#![allow(dead_code)] // P2 起用

use anyhow::Context;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::SqlitePool;

use super::now_ms;

pub const DOMAIN: &str = "domain";
pub const CLOUDFLARE_API_TOKEN: &str = "cloudflare_api_token";
pub const TIMEZONE: &str = "timezone";
pub const ACME_ACCOUNT: &str = "acme_account";
pub const TEMPLATE: &str = "template";

/// 没设置时区时用的默认值。
pub const DEFAULT_TIMEZONE: &str = "Asia/Shanghai";

/// 校验时区名（IANA，例如 Asia/Shanghai），返回规范写法。
pub fn check_timezone(name: &str) -> Option<String> {
    name.parse::<chrono_tz::Tz>()
        .ok()
        .map(|tz| tz.name().to_string())
}

/// 管理员时区：日账本分日、每月重置、到期都按它。没设置或设置坏了时用默认值。
pub async fn timezone(db: &SqlitePool) -> chrono_tz::Tz {
    match get::<String>(db, TIMEZONE).await {
        Ok(Some(name)) => name.parse().unwrap_or(chrono_tz::Asia::Shanghai),
        _ => chrono_tz::Asia::Shanghai,
    }
}

/// 管理员时区的今天，YYYY-MM-DD。
pub async fn today(db: &SqlitePool) -> String {
    let tz = timezone(db).await;
    chrono::Utc::now()
        .with_timezone(&tz)
        .format("%Y-%m-%d")
        .to_string()
}

/// 读一项设置；没有时返回 None。
pub async fn get<T: DeserializeOwned>(db: &SqlitePool, key: &str) -> anyhow::Result<Option<T>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(db)
        .await
        .with_context(|| format!("读取设置 {key}"))?;
    match row {
        Some((value,)) => Ok(Some(
            serde_json::from_str(&value).with_context(|| format!("解析设置 {key}"))?,
        )),
        None => Ok(None),
    }
}

/// 写一项设置（有则覆盖）。
pub async fn set<T: Serialize>(db: &SqlitePool, key: &str, value: &T) -> anyhow::Result<()> {
    let value = serde_json::to_string(value).with_context(|| format!("编码设置 {key}"))?;
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(now_ms())
    .execute(db)
    .await
    .with_context(|| format!("写入设置 {key}"))?;
    Ok(())
}

/// 删除一项设置。
pub async fn delete(db: &SqlitePool, key: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(db)
        .await
        .with_context(|| format!("删除设置 {key}"))?;
    Ok(())
}
