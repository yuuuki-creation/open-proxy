//! 订阅服务：`GET /s/{token}`（subscription.md）。按 User-Agent 识别格式，也可以用 `?format=` 指定。
//! 超额、到期、停用、Token 无效时返回 200 和只含提示节点的配置，朋友更新订阅时直接在客户端里看到原因。

mod clash;
mod conf;
mod links;
pub mod node;
pub mod template;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{NaiveDate, TimeZone};
use serde::Deserialize;

use crate::api::client_ip;
use crate::app::{AppState, PeerAddr};
use crate::db::{self, settings, users::User};
pub use node::Format;
use node::ProxyNode;

#[derive(Deserialize)]
pub struct SubQuery {
    format: Option<String>,
}

/// 频率限制：同一 IP 每分钟最多 60 次（Token 有 256 位，不需要防猜测的封禁，只防刷）。
static LIMITER: LazyLock<Mutex<HashMap<IpAddr, (u32, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const PER_MINUTE: u32 = 60;

fn rate_limited(ip: IpAddr) -> bool {
    let mut map = LIMITER.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    map.retain(|_, (_, start)| now.duration_since(*start) < Duration::from_secs(60));
    let entry = map.entry(ip).or_insert((0, now));
    entry.0 += 1;
    entry.0 > PER_MINUTE
}

pub async fn serve(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<PeerAddr>,
    Path(token): Path<String>,
    Query(q): Query<SubQuery>,
    headers: HeaderMap,
) -> Response {
    let ip = client_ip(peer.0, &headers).parse().unwrap_or(peer.0.ip());
    if rate_limited(ip) {
        return (StatusCode::TOO_MANY_REQUESTS, "请求太频繁，稍后再试").into_response();
    }
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let format = q
        .format
        .as_deref()
        .and_then(Format::parse)
        .unwrap_or_else(|| Format::detect(user_agent));
    match build(&state, &token, format).await {
        Ok(resp) => resp,
        Err(err) => {
            tracing::error!("生成订阅出错: {err:#}");
            (StatusCode::INTERNAL_SERVER_ERROR, "生成订阅出错").into_response()
        }
    }
}

/// 用量信息：响应头和提示节点都用它。
struct Usage {
    used: i64,
    quota: Option<i64>,
    expires_on: Option<String>,
    /// 到期日结束时刻（管理员时区的 24 点）的 Unix 秒；没有到期日时是一个很远的日期
    expire_unix: i64,
}

async fn usage(state: &AppState, user: &User) -> anyhow::Result<Usage> {
    let quota = db::plans::get(&state.db, user.plan_id)
        .await?
        .and_then(|p| p.traffic_quota_bytes);
    let tz = settings::timezone(&state.db).await;
    let expire_unix = user
        .expires_on
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .and_then(|d| d.succ_opt())
        .and_then(|d| {
            tz.from_local_datetime(&d.and_time(chrono::NaiveTime::MIN))
                .earliest()
        })
        .map(|t| t.timestamp())
        // Shadowrocket 把缺失或 0 显示成 1970，给一个很远的日期
        .unwrap_or(4_102_444_799);
    Ok(Usage {
        used: user.period_used(),
        quota,
        expires_on: user.expires_on.clone(),
        expire_unix,
    })
}

async fn build(state: &AppState, token: &str, format: Format) -> anyhow::Result<Response> {
    let Some(user) = db::users::by_sub_token(&state.db, token).await? else {
        return Ok(warning(
            format,
            &["⚠️ 订阅链接无效", "⚠️ 请联系管理员"],
            None,
        ));
    };
    let usage = usage(state, &user).await?;
    if !user.blocked_reason.is_empty() {
        let message = match user.blocked_reason.as_str() {
            "over_quota" => "⚠️ 流量已用完",
            "expired" => "⚠️ 已到期",
            _ => "⚠️ 已停用",
        };
        return Ok(warning(format, &[message, "⚠️ 请联系管理员"], Some(&usage)));
    }
    let body = render_for(state, &user, &usage, format).await?;
    Ok(respond(format, body, Some(&usage)))
}

/// 按用户生成某种格式的订阅正文（预览也用它，不看停用状态）。
async fn render_for(
    state: &AppState,
    user: &User,
    usage: &Usage,
    format: Format,
) -> anyhow::Result<String> {
    let settings = template::load(&state.db).await?;
    let tpl = match template::parse(settings.text()) {
        Ok(t) => t,
        Err(err) => {
            tracing::warn!("订阅模板解析失败，改用内置模板: {err:#}");
            template::parse(template::BUILTIN)?
        }
    };
    let nodes = node::user_nodes(&state.db, user).await?;
    let remaining = match usage.quota {
        Some(q) => format_bytes((q - usage.used).max(0)),
        None => "不限".to_string(),
    };
    let expires = usage
        .expires_on
        .clone()
        .unwrap_or_else(|| "永久".to_string());
    let hints = vec![
        node::hint_node(&format!("剩余流量 {remaining}")),
        node::hint_node(&format!("到期 {expires}")),
    ];
    Ok(render(&tpl, &hints, &nodes, format))
}

/// 按格式输出：提示节点放在最前面，不进代理组。
pub fn render(
    tpl: &template::Template,
    hints: &[ProxyNode],
    nodes: &[ProxyNode],
    format: Format,
) -> String {
    match format {
        Format::Mihomo => clash::mihomo(tpl, hints, nodes),
        Format::Stash | Format::Shadowrocket => clash::translated(tpl, hints, nodes, format),
        Format::Surge | Format::Loon => conf::surge_like(tpl, hints, nodes, format),
        Format::QuantumultX => conf::quantumultx(tpl, hints, nodes),
        Format::V2ray => links::render(hints, nodes),
    }
}

/// 停用和无效时的配置：只有提示节点，一个选择组，规则全部直连。
const WARNING_TEMPLATE: &str = "mode: rule
proxy-groups:
  - name: ⚠️ 提示
    type: select
    include-all-proxies: true
rules:
  - MATCH,DIRECT
";

fn warning(format: Format, messages: &[&str], usage: Option<&Usage>) -> Response {
    let body = match template::parse(WARNING_TEMPLATE) {
        Ok(tpl) => {
            let nodes: Vec<ProxyNode> = messages.iter().map(|m| node::hint_node(m)).collect();
            render(&tpl, &[], &nodes, format)
        }
        Err(_) => String::new(),
    };
    respond(format, body, usage)
}

fn respond(format: Format, body: String, usage: Option<&Usage>) -> Response {
    let (content_type, ext) = match format {
        Format::Mihomo | Format::Stash | Format::Shadowrocket => {
            ("text/yaml; charset=utf-8", "yaml")
        }
        Format::Surge | Format::Loon | Format::QuantumultX => ("text/plain; charset=utf-8", "conf"),
        Format::V2ray => ("text/plain; charset=utf-8", "txt"),
    };
    let mut resp = (StatusCode::OK, body).into_response();
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert("profile-update-interval", HeaderValue::from_static("24"));
    if let Ok(v) = HeaderValue::from_str(&format!("base64:{}", STANDARD.encode("open-proxy"))) {
        h.insert("profile-title", v);
    }
    // RFC 5987 写法，文件名只有 ASCII
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''open-proxy.{ext}"))
    {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    // 额度不限时不发（subscription.md「用量显示」）
    if let Some(u) = usage
        && let Some(quota) = u.quota
        && let Ok(v) = HeaderValue::from_str(&format!(
            "upload=0; download={}; total={quota}; expire={}",
            u.used, u.expire_unix
        ))
    {
        h.insert("subscription-userinfo", v);
    }
    resp
}

/// 流量按 1024 进制换算，保留两位小数。
pub fn format_bytes(bytes: i64) -> String {
    let b = bytes as f64;
    const K: f64 = 1024.0;
    if b >= K * K * K {
        format!("{:.2} GB", b / (K * K * K))
    } else if b >= K * K {
        format!("{:.2} MB", b / (K * K))
    } else if b >= K {
        format!("{:.2} KB", b / K)
    } else {
        format!("{bytes} B")
    }
}

/// 预览：按某个用户生成某种格式（面板「订阅模板」页用）。
pub async fn preview(state: &AppState, user: &User, format: Format) -> anyhow::Result<String> {
    let usage = usage(state, user).await?;
    render_for(state, user, &usage, format).await
}
