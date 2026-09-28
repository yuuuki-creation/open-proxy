//! REALITY 伪装目标的检测和扫描，由 Agent 在服务器上做（nodes.md「REALITY 伪装目标」）。
//! 扫描同一台服务器同时只跑一个，结果只放内存。

use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::auth::Admin;
use super::{ApiError, ApiResult};
use crate::agent::ScanState;
use crate::app::AppState;
use crate::db;
use crate::pb::agentv1 as pb;

#[derive(Deserialize)]
pub struct CheckRequest {
    targets: Vec<String>,
}

#[derive(Serialize)]
struct CheckResult {
    target: String,
    tls13: bool,
    h2: bool,
    latency_ms: u32,
    certificate_valid: bool,
    error: String,
}

/// 检测选中的目标，等 Agent 回复再返回（最多 1 分钟）。
pub async fn check(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
    Json(req): Json<CheckRequest>,
) -> ApiResult<Json<Value>> {
    let targets: Vec<String> = req
        .targets
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if targets.is_empty() || targets.len() > 20 {
        return Err(ApiError::bad_request(
            "invalid_targets",
            "一次检测 1–20 个目标",
        ));
    }
    let agent = state
        .hub
        .get(id)
        .ok_or_else(|| ApiError::conflict("offline", "Agent 不在线，没法检测"))?;
    let reply = agent
        .request(
            pb::master_message::Body::CheckRealityTargets(pb::CheckRealityTargets { targets }),
            Duration::from_secs(60),
        )
        .await
        .map_err(|e| ApiError::conflict("agent_error", format!("检测失败: {e}")))?;
    match reply {
        pb::agent_message::Body::CheckRealityTargetsResult(r) => {
            let results: Vec<CheckResult> = r
                .results
                .into_iter()
                .map(|c| CheckResult {
                    target: c.target,
                    tls13: c.tls13,
                    h2: c.h2,
                    latency_ms: c.latency_ms,
                    certificate_valid: c.certificate_valid,
                    error: c.error,
                })
                .collect();
            Ok(Json(json!({ "results": results })))
        }
        pb::agent_message::Body::ErrorReply(e) => Err(ApiError::conflict(
            "agent_error",
            format!("Agent 回复失败: {}", e.message),
        )),
        _ => Err(ApiError::internal("Agent 的回复不对")),
    }
}

#[derive(Deserialize, Default)]
pub struct ScanRequest {
    cidr: Option<String>,
    concurrency: Option<u32>,
    max_per_second: Option<u32>,
}

/// 发起扫描：默认扫服务器地址所在的 /24，慢速、低并发。
pub async fn start_scan(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
    Json(req): Json<ScanRequest>,
) -> ApiResult<Json<Value>> {
    let server = db::servers::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("服务器不存在"))?;
    let agent = state
        .hub
        .get(id)
        .ok_or_else(|| ApiError::conflict("offline", "Agent 不在线，没法扫描"))?;
    let cidr = match req.cidr.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => c.to_string(),
        None => default_cidr(&server.address).await.ok_or_else(|| {
            ApiError::bad_request("invalid_cidr", "解析不出服务器的 IPv4 地址，请手动填网段")
        })?,
    };
    if !valid_cidr(&cidr) {
        return Err(ApiError::bad_request(
            "invalid_cidr",
            "网段要写成 IPv4 的 a.b.c.d/n，n 在 20–32 之间",
        ));
    }
    let concurrency = req.concurrency.unwrap_or(4).clamp(1, 32);
    let max_per_second = req.max_per_second.unwrap_or(10).clamp(1, 100);
    if !state.hub.begin_scan(id) {
        return Err(ApiError::conflict(
            "scan_running",
            "这台服务器正在扫描，等它结束",
        ));
    }
    tracing::info!(server_id = id, %cidr, concurrency, max_per_second, "发起 REALITY 目标扫描");
    let hub = state.hub.clone();
    tokio::spawn(async move {
        let result = agent
            .request(
                pb::master_message::Body::ScanRealityTargets(pb::ScanRealityTargets {
                    cidr,
                    concurrency,
                    max_per_second,
                }),
                Duration::from_secs(20 * 60),
            )
            .await;
        let outcome = match result {
            Ok(pb::agent_message::Body::ScanRealityTargetsResult(r)) => Ok(r.candidates),
            Ok(pb::agent_message::Body::ErrorReply(e)) => Err(e.message),
            Ok(_) => Err("Agent 的回复不对".to_string()),
            Err(err) => Err(err.to_string()),
        };
        hub.finish_scan(id, outcome);
    });
    Ok(Json(json!({ "status": "running" })))
}

/// 扫描进度和结果。
pub async fn scan_status(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<ScanState>> {
    Ok(Json(state.hub.scan(id)))
}

async fn default_cidr(address: &str) -> Option<String> {
    let ip = match address.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => ip,
        Err(_) => tokio::net::lookup_host((address, 0))
            .await
            .ok()?
            .find_map(|a| match a.ip() {
                std::net::IpAddr::V4(v4) => Some(v4),
                _ => None,
            })?,
    };
    let [a, b, c, _] = ip.octets();
    Some(format!("{a}.{b}.{c}.0/24"))
}

/// 只允许 IPv4、/20 到 /32（最多 4096 个地址，避免扫太大的网段）。
fn valid_cidr(cidr: &str) -> bool {
    let Some((ip, bits)) = cidr.split_once('/') else {
        return false;
    };
    ip.parse::<std::net::Ipv4Addr>().is_ok()
        && bits.parse::<u8>().is_ok_and(|b| (20..=32).contains(&b))
}
