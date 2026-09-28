//! 服务器：装了 Agent 的机器。创建时返回安装命令，Agent Token 只显示这一次（库里只存哈希）。

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::auth::Admin;
use super::{ApiError, ApiResult, check_address, check_name, check_port, nullable, time, time_opt};
use crate::app::AppState;
use crate::db::is_unique_violation;
use crate::db::servers::{self, NewCertificate, Server, ServerFields};
use crate::{secret, tls};

/// 自签证书的有效期：订阅里固定了指纹，不需要续期，给长一点
const SELF_SIGNED_DAYS: i64 = 3650;

#[derive(Serialize)]
pub struct ServerView {
    id: i64,
    name: String,
    address: String,
    port_range_start: i64,
    port_range_end: i64,
    cert_mode: String,
    cert_domain: Option<String>,
    traffic_quota_bytes: Option<i64>,
    traffic_reset_day: Option<i64>,
    online: bool,
    /// 在线但和主控版本不一致：按本地状态继续服务，暂停同步，等管理员升级
    version_mismatch: bool,
    /// 实时网速（字节/秒，10 秒平均）
    rx_speed: u64,
    tx_speed: u64,
    /// 从上一个重置日起的网卡收发（字节）
    month_rx: i64,
    month_tx: i64,
    agent_version: String,
    agent_arch: String,
    /// 上次升级失败、换回旧版本时要升级到的版本
    rolled_back_from: String,
    /// 最近一次升级指令失败的原因
    upgrade_error: Option<String>,
    last_seen_at: Option<String>,
    state_version: i64,
    applied_version: i64,
    /// Agent 已经应用了最新的期望状态
    synced: bool,
    apply_failures: Value,
    certificate: Option<CertificateView>,
    created_at: String,
}

#[derive(Serialize)]
pub struct CertificateView {
    kind: String,
    domain: Option<String>,
    sha256: String,
    not_after: String,
    renewed_at: String,
    last_error: String,
}

pub async fn view(state: &AppState, s: Server) -> ApiResult<ServerView> {
    let certificate = servers::certificate(&state.db, Some(s.id))
        .await?
        .map(|c| CertificateView {
            kind: c.kind,
            domain: c.domain,
            sha256: c.sha256,
            not_after: time(c.not_after),
            renewed_at: time(c.renewed_at),
            last_error: c.last_error,
        });
    let agent = state.hub.get(s.id);
    let (rx_speed, tx_speed) = state.hub.speed(s.id);
    let (month_rx, month_tx) =
        super::stats::server_month_usage(state, s.id, s.traffic_reset_day).await?;
    Ok(ServerView {
        id: s.id,
        name: s.name,
        address: s.address,
        port_range_start: s.port_range_start,
        port_range_end: s.port_range_end,
        cert_mode: s.cert_mode,
        cert_domain: s.cert_domain,
        traffic_quota_bytes: s.traffic_quota_bytes,
        traffic_reset_day: s.traffic_reset_day,
        online: agent.is_some(),
        version_mismatch: agent.is_some_and(|a| !a.sync),
        rx_speed,
        tx_speed,
        month_rx,
        month_tx,
        agent_version: s.agent_version,
        agent_arch: s.agent_arch,
        rolled_back_from: s.rolled_back_from,
        upgrade_error: state.hub.upgrade_error(s.id),
        last_seen_at: time_opt(s.last_seen_at),
        state_version: s.state_version,
        applied_version: s.applied_version,
        synced: s.applied_version == s.state_version,
        apply_failures: serde_json::from_str(&s.apply_failures).unwrap_or(json!([])),
        certificate,
        created_at: time(s.created_at),
    })
}

pub async fn list(
    State(state): State<AppState>,
    _admin: Admin,
) -> ApiResult<Json<Vec<ServerView>>> {
    let mut views = Vec::new();
    for s in servers::list(&state.db).await? {
        views.push(view(&state, s).await?);
    }
    Ok(Json(views))
}

pub async fn get(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<ServerView>> {
    let s = find(&state, id).await?;
    Ok(Json(view(&state, s).await?))
}

async fn find(state: &AppState, id: i64) -> ApiResult<Server> {
    servers::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("服务器不存在"))
}

#[derive(Deserialize)]
pub struct CreateServer {
    name: String,
    address: String,
    #[serde(default)]
    port_range_start: Option<i64>,
    #[serde(default)]
    port_range_end: Option<i64>,
    #[serde(default = "default_cert_mode")]
    cert_mode: String,
    #[serde(default)]
    cert_domain: Option<String>,
    #[serde(default)]
    traffic_quota_bytes: Option<i64>,
    #[serde(default)]
    traffic_reset_day: Option<i64>,
}

fn default_cert_mode() -> String {
    "self_signed".to_string()
}

pub async fn create(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<CreateServer>,
) -> ApiResult<Json<Value>> {
    let fields = check_fields(ServerFields {
        name: req.name,
        address: req.address,
        port_range_start: req.port_range_start.unwrap_or(10000),
        port_range_end: req.port_range_end.unwrap_or(60000),
        cert_mode: req.cert_mode,
        cert_domain: req.cert_domain,
        traffic_quota_bytes: req.traffic_quota_bytes,
        traffic_reset_day: req.traffic_reset_day,
    })?;
    let base_url = state.base_url().await?.ok_or_else(no_domain)?;

    let token = secret::new_token();
    let id = servers::create(&state.db, &fields, &secret::sha256_hex(&token))
        .await
        .map_err(|e| {
            if is_unique_violation(&e) {
                ApiError::conflict("name_taken", "已经有同名的服务器")
            } else {
                e.into()
            }
        })?;
    if fields.cert_mode == "self_signed" {
        issue_self_signed(&state, id).await?;
    }
    tracing::info!(server_id = id, name = %fields.name, "创建服务器");
    state.config_changed();
    if fields.cert_mode == "acme" {
        state.check_certificates();
    }

    let server = view(&state, find(&state, id).await?).await?;
    Ok(Json(json!({
        "server": server,
        "install_command": command_line(&base_url, &token),
    })))
}

#[derive(Deserialize)]
pub struct UpdateServer {
    name: Option<String>,
    address: Option<String>,
    port_range_start: Option<i64>,
    port_range_end: Option<i64>,
    cert_mode: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    cert_domain: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    traffic_quota_bytes: Option<Option<i64>>,
    #[serde(default, deserialize_with = "nullable")]
    traffic_reset_day: Option<Option<i64>>,
}

pub async fn update(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
    Json(req): Json<UpdateServer>,
) -> ApiResult<Json<ServerView>> {
    let old = find(&state, id).await?;
    let fields = check_fields(ServerFields {
        name: req.name.unwrap_or(old.name.clone()),
        address: req.address.unwrap_or(old.address.clone()),
        port_range_start: req.port_range_start.unwrap_or(old.port_range_start),
        port_range_end: req.port_range_end.unwrap_or(old.port_range_end),
        cert_mode: req.cert_mode.unwrap_or(old.cert_mode.clone()),
        cert_domain: req.cert_domain.unwrap_or(old.cert_domain.clone()),
        traffic_quota_bytes: req.traffic_quota_bytes.unwrap_or(old.traffic_quota_bytes),
        traffic_reset_day: req.traffic_reset_day.unwrap_or(old.traffic_reset_day),
    })?;
    servers::update(&state.db, id, &fields).await.map_err(|e| {
        if is_unique_violation(&e) {
            ApiError::conflict("name_taken", "已经有同名的服务器")
        } else {
            e.into()
        }
    })?;
    // 证书方式或域名变了：自签的立即重新生成；自动申请的删掉旧证书，等证书任务重新申请
    if fields.cert_mode != old.cert_mode || fields.cert_domain != old.cert_domain {
        if fields.cert_mode == "self_signed" {
            if old.cert_mode != "self_signed" {
                issue_self_signed(&state, id).await?;
            }
        } else {
            servers::delete_certificate(&state.db, id).await?;
            state.check_certificates();
        }
    }
    state.config_changed();
    Ok(Json(view(&state, find(&state, id).await?).await?))
}

pub async fn delete(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let server = find(&state, id).await?;
    if let Some((_, exit_name)) = servers::landing_exit_of(&state.db, id).await? {
        return Err(ApiError::conflict(
            "in_use",
            format!("这台服务器是落地出口「{exit_name}」的落地机，先删除这个出口"),
        ));
    }
    servers::delete(&state.db, id).await?;
    // 在线的 Agent 收到卸载指令；不在线的下次连上时收到「服务器已删除」
    super::hosting::uninstall(&state, id);
    tracing::info!(server_id = id, name = %server.name, "删除服务器");
    state.config_changed();
    Ok(Json(json!({})))
}

/// 重新生成安装命令：换一个新 Token，旧 Token 立即失效。
pub async fn install_command(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    find(&state, id).await?;
    let base_url = state.base_url().await?.ok_or_else(no_domain)?;
    let token = secret::new_token();
    servers::set_token_hash(&state.db, id, &secret::sha256_hex(&token)).await?;
    state.hub.disconnect(id);
    tracing::info!(server_id = id, "重新生成安装命令，旧 Token 失效");
    Ok(Json(
        json!({ "install_command": command_line(&base_url, &token) }),
    ))
}

fn command_line(base_url: &str, token: &str) -> String {
    format!("curl -fsSL {base_url}/api/agent/install.sh | bash -s -- {base_url} {token}")
}

fn no_domain() -> ApiError {
    ApiError::bad_request("no_domain", "先在设置里填写主控域名")
}

/// 给服务器生成自签证书（主控生成，指纹写进订阅；重装 Agent 后不变）。
async fn issue_self_signed(state: &AppState, server_id: i64) -> ApiResult<()> {
    let cert = tls::generate_self_signed(vec!["open-proxy".to_string()], SELF_SIGNED_DAYS)?;
    servers::save_certificate(
        &state.db,
        &NewCertificate {
            server_id: Some(server_id),
            kind: "self_signed",
            domain: None,
            cert_pem: &cert.cert_pem,
            key_pem: &cert.key_pem,
            sha256: &cert.sha256,
            not_after: cert.not_after,
        },
    )
    .await?;
    Ok(())
}

fn check_fields(mut f: ServerFields) -> ApiResult<ServerFields> {
    f.name = check_name(&f.name, "服务器名")?;
    f.address = check_address(&f.address)?;
    check_port(f.port_range_start, "端口范围")?;
    check_port(f.port_range_end, "端口范围")?;
    if f.port_range_start > f.port_range_end {
        return Err(ApiError::bad_request(
            "invalid_port_range",
            "端口范围的起点不能大于终点",
        ));
    }
    match f.cert_mode.as_str() {
        "self_signed" => f.cert_domain = None,
        "acme" => {
            let domain = f
                .cert_domain
                .as_deref()
                .map(|d| d.trim().to_lowercase())
                .filter(|d| super::is_hostname(d))
                .ok_or_else(|| {
                    ApiError::bad_request("invalid_domain", "自动申请证书要填这台服务器的域名")
                })?;
            f.cert_domain = Some(domain);
        }
        _ => {
            return Err(ApiError::bad_request(
                "invalid_cert_mode",
                "证书方式只能是 acme 或 self_signed",
            ));
        }
    }
    if let Some(quota) = f.traffic_quota_bytes
        && quota < 0
    {
        return Err(ApiError::bad_request(
            "invalid_quota",
            "整机月额度不能是负数",
        ));
    }
    if let Some(day) = f.traffic_reset_day
        && !(1..=31).contains(&day)
    {
        return Err(ApiError::bad_request(
            "invalid_reset_day",
            "重置日要在 1–31 之间",
        ));
    }
    Ok(f)
}
