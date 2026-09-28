//! Agent 托管：安装脚本、二进制下载、升级（architecture.md「发布、安装与升级」，protocol.md「升级」）。

use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::auth::Admin;
use super::{ApiError, ApiResult};
use crate::VERSION;
use crate::agent::RequestError;
use crate::app::AppState;
use crate::db;
use crate::pb::agentv1 as pb;
use crate::secret;

const INSTALL_SCRIPT: &str = include_str!("../../assets/install.sh");
/// 升级请求的超时（protocol.md「消息一览」）
const UPGRADE_TIMEOUT: Duration = Duration::from_secs(300);

/// 安装脚本：填好版本号和各架构二进制的 SHA-256。不含秘密，不需要登录。
pub async fn install_script(State(state): State<AppState>) -> Response {
    let hash = |arch: &str| {
        state
            .agent_dist
            .binary(arch)
            .map(|b| hex::encode(b.sha256))
            .unwrap_or_default()
    };
    let script = INSTALL_SCRIPT
        .replace("__VERSION__", VERSION)
        .replace("__SHA256_AMD64__", &hash("amd64"))
        .replace("__SHA256_ARM64__", &hash("arm64"));
    (
        [(header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8")],
        script,
    )
        .into_response()
}

/// 下载 Agent 二进制：请求头 `Authorization: Bearer <Agent Token>`；只托管和主控同版本的。
pub async fn binary(
    State(state): State<AppState>,
    Path((version, arch)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let authorized = !token.is_empty()
        && matches!(
            db::servers::by_token_hash(&state.db, &secret::sha256_hex(token)).await,
            Ok(Some(_))
        );
    if !authorized {
        return (StatusCode::UNAUTHORIZED, "Token 无效").into_response();
    }
    if version != VERSION {
        return (StatusCode::NOT_FOUND, "主控只托管和自己同版本的 Agent").into_response();
    }
    match state.agent_dist.binary(&arch) {
        Some(bin) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            bin.data,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "没有这个架构的 Agent 二进制").into_response(),
    }
}

/// 给一台服务器发升级指令，发出就返回；结果看服务器列表里的版本号和升级错误。
pub async fn upgrade(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    db::servers::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("服务器不存在"))?;
    start_upgrade(&state, id)?;
    Ok(Json(json!({})))
}

/// 全部升级：给所有在线、版本和主控不同的服务器发升级指令。
pub async fn upgrade_all(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Value>> {
    let mut started = 0;
    for server in db::servers::list(&state.db).await? {
        let outdated = state
            .hub
            .get(server.id)
            .is_some_and(|a| a.version != VERSION);
        if outdated && start_upgrade(&state, server.id).is_ok() {
            started += 1;
        }
    }
    Ok(Json(json!({ "started": started })))
}

fn start_upgrade(state: &AppState, server_id: i64) -> ApiResult<()> {
    let agent = state
        .hub
        .get(server_id)
        .ok_or_else(|| ApiError::conflict("offline", "Agent 不在线，没法升级"))?;
    let arch = match agent.arch {
        pb::Arch::Amd64 => "amd64",
        pb::Arch::Arm64 => "arm64",
        pb::Arch::Unspecified => {
            return Err(ApiError::conflict("unknown_arch", "不知道这台服务器的架构"));
        }
    };
    let bin = state.agent_dist.binary(arch).ok_or_else(|| {
        ApiError::conflict("no_binary", format!("这个版本的主控没有带 {arch} 的 Agent"))
    })?;
    let signature = bin.signature.clone().ok_or_else(|| {
        ApiError::conflict("unsigned", "这个版本的 Agent 没有签名，Agent 会拒绝升级")
    })?;
    let msg = pb::master_message::Body::Upgrade(pb::Upgrade {
        version: VERSION.to_string(),
        download_path: format!("/api/agent/binary/{VERSION}/{arch}"),
        sha256: bin.sha256.to_vec(),
        signature,
    });
    state.hub.set_upgrade_error(server_id, None);
    let hub = state.hub.clone();
    tracing::info!(server_id, from = %agent.version, to = VERSION, "发出升级指令");
    tokio::spawn(async move {
        let error = match agent.request(msg, UPGRADE_TIMEOUT).await {
            Ok(pb::agent_message::Body::UpgradeResult(r)) if r.ok => None,
            Ok(pb::agent_message::Body::UpgradeResult(r)) => Some(r.error),
            Ok(pb::agent_message::Body::ErrorReply(e)) => Some(e.message),
            Ok(_) => Some("Agent 的回复不对".to_string()),
            // 升级成功时 Agent 回复后会退出，连接断开不算失败
            Err(RequestError::Disconnected) => None,
            Err(err) => Some(err.to_string()),
        };
        match &error {
            Some(e) => tracing::warn!(server_id, "升级失败: {e}"),
            None => tracing::info!(server_id, "Agent 已接受升级，等它重启后重新连上"),
        }
        hub.set_upgrade_error(server_id, error);
    });
    Ok(())
}

/// 删除服务器时让在线的 Agent 卸载自己；不在线的下次连上时收到「服务器已删除」。
pub fn uninstall(state: &AppState, server_id: i64) {
    let Some(agent) = state.hub.get(server_id) else {
        return;
    };
    tokio::spawn(async move {
        let result = agent
            .request(
                pb::master_message::Body::Uninstall(pb::Uninstall {}),
                Duration::from_secs(60),
            )
            .await;
        match result {
            Ok(pb::agent_message::Body::UninstallResult(r)) if r.ok => {
                tracing::info!(server_id, "Agent 开始卸载")
            }
            Ok(pb::agent_message::Body::UninstallResult(r)) => {
                tracing::warn!(server_id, "Agent 卸载失败: {}", r.error)
            }
            Ok(pb::agent_message::Body::ErrorReply(e)) => tracing::warn!(
                server_id,
                "Agent 不能卸载（下次连上时会收到「服务器已删除」）: {}",
                e.message
            ),
            Ok(_) => tracing::warn!(server_id, "Agent 对卸载指令的回复不对"),
            Err(err) => tracing::warn!(
                server_id,
                "卸载指令没有得到确认（下次连上时会收到「服务器已删除」）: {err}"
            ),
        }
        agent.close();
    });
}
