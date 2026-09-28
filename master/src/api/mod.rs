//! 面板用的 REST API，都在 /api/ 下。约定见 main 分支 docs/design-docs/api.md。

pub mod auth;
pub mod backup;
mod error;
mod exits;
mod hosting;
mod nodes;
mod plans;
mod reality;
mod servers;
mod settings;
mod stats;
pub mod template;
mod users;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::DefaultBodyLimit;
use axum::http::HeaderMap;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::SecondsFormat;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::VERSION;
use crate::app::AppState;

pub use error::{ApiError, ApiResult};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/setup", get(auth::setup_status).post(auth::setup))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/password", put(auth::change_password))
        .route("/settings", get(settings::get).patch(settings::update))
        .route("/overview", get(stats::overview))
        .route("/backup", get(backup::download))
        .route(
            "/backup/restore",
            post(backup::restore).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .route("/template", get(template::get).put(template::update))
        .route("/template/refresh", post(template::refresh))
        .route("/template/report", get(template::report))
        .route("/template/preview", get(template::preview))
        .route("/traffic", get(stats::traffic))
        .route("/agent/ws", get(crate::agent::ws_handler))
        .route("/agent/install.sh", get(hosting::install_script))
        .route("/agent/binary/{version}/{arch}", get(hosting::binary))
        .route("/servers/upgrade-all", post(hosting::upgrade_all))
        .route("/servers/{id}/upgrade", post(hosting::upgrade))
        .route("/servers/{id}/reality/check", post(reality::check))
        .route(
            "/servers/{id}/reality/scan",
            get(reality::scan_status).post(reality::start_scan),
        )
        .route("/servers", get(servers::list).post(servers::create))
        .route("/servers/{id}/traffic", get(stats::server_traffic))
        .route(
            "/servers/{id}",
            get(servers::get)
                .patch(servers::update)
                .delete(servers::delete),
        )
        .route(
            "/servers/{id}/install-command",
            post(servers::install_command),
        )
        .route("/nodes", get(nodes::list).post(nodes::create))
        .route("/nodes/order", put(nodes::set_order))
        .route(
            "/nodes/{id}",
            get(nodes::get).patch(nodes::update).delete(nodes::delete),
        )
        .route("/exits", get(exits::list).post(exits::create))
        .route(
            "/exits/{id}",
            get(exits::get).patch(exits::update).delete(exits::delete),
        )
        .route("/plans", get(plans::list).post(plans::create))
        .route(
            "/plans/{id}",
            get(plans::get).patch(plans::update).delete(plans::delete),
        )
        .route("/users", get(users::list).post(users::create))
        .route(
            "/users/{id}",
            get(users::get).patch(users::update).delete(users::delete),
        )
        .route("/users/{id}/reset-period", post(users::reset_period))
        .route("/users/{id}/traffic", get(stats::user_traffic))
        .route(
            "/users/{id}/reset-credentials",
            post(users::reset_credentials),
        )
        .fallback(not_found)
}

/// 健康检查：返回版本号，部署和测试时确认主控起来了。
async fn health() -> Json<Value> {
    Json(json!({ "version": VERSION }))
}

async fn not_found() -> ApiError {
    ApiError::not_found("接口不存在")
}

/// Unix 毫秒转成 RFC 3339（UTC）。
pub fn time(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_default()
}

pub fn time_opt(ms: Option<i64>) -> Option<String> {
    ms.map(time)
}

/// PATCH 里可以设成 null 的字段：没写是 None，写了 null 是 Some(None)，写了值是 Some(Some(v))。
/// 用法：`#[serde(default, deserialize_with = "nullable")]`。
pub fn nullable<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

/// 名字：去掉首尾空白后不能为空，最长 64 个字符。
pub fn check_name(name: &str, what: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad_request(
            "invalid_name",
            format!("{what}不能为空，最长 64 个字"),
        ));
    }
    Ok(name.to_string())
}

/// 地址：IPv4，或者域名（只支持 IPv4，nodes.md）。返回去掉空白后的地址。
pub fn check_address(address: &str) -> ApiResult<String> {
    let address = address.trim();
    if address.parse::<Ipv4Addr>().is_ok() || is_hostname(address) {
        return Ok(address.to_string());
    }
    Err(ApiError::bad_request(
        "invalid_address",
        format!("地址「{address}」要写 IPv4 地址或域名（不支持 IPv6）"),
    ))
}

/// 是不是合法的域名（字母、数字、连字符，至少两段）。
pub fn is_hostname(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 || s.parse::<IpAddr>().is_ok() {
        return false;
    }
    let labels: Vec<&str> = s.trim_end_matches('.').split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// 端口：1–65535。
pub fn check_port(port: i64, what: &str) -> ApiResult<i64> {
    if (1..=65535).contains(&port) {
        Ok(port)
    } else {
        Err(ApiError::bad_request(
            "invalid_port",
            format!("{what}要在 1–65535 之间"),
        ))
    }
}

/// 日期：YYYY-MM-DD。
pub fn check_date(s: &str, what: &str) -> ApiResult<String> {
    chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d")
        .map(|d| d.format("%Y-%m-%d").to_string())
        .map_err(|_| ApiError::bad_request("invalid_date", format!("{what}要写成 YYYY-MM-DD")))
}

/// 客户端的真实 IP。对端是本机（反向代理在同一台机器上）时取 X-Forwarded-For 的最后一个地址：
/// 那是反向代理自己加的，前面的可能是客户端伪造的（否则换着伪造地址就能绕过登录限制）。
pub fn client_ip(peer: SocketAddr, headers: &HeaderMap) -> String {
    if peer.ip().is_loopback()
        && let Some(forwarded) = headers
            .get_all("x-forwarded-for")
            .iter()
            .next_back()
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .map(str::trim)
            .filter(|v| v.parse::<IpAddr>().is_ok())
    {
        return forwarded.to_string();
    }
    peer.ip().to_string()
}
