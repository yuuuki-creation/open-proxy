//! 首次初始化、登录、会话。会话 Token 放在 Cookie `op_session` 里（HttpOnly、Secure、SameSite=Strict），
//! 库里只存哈希；写操作只接受 JSON，配合 SameSite 防跨站请求伪造（api.md「约定」）。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE, USER_AGENT};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, ApiResult, client_ip};
use crate::app::{AppState, PeerAddr};
use crate::db::{self, settings};
use crate::secret;

pub const SESSION_COOKIE: &str = "op_session";
const SESSION_DAYS: i64 = 30;
const MIN_PASSWORD_LEN: usize = 8;

/// 已登录的管理员。放在处理函数的参数里，没登录时回 401。
pub struct Admin;

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        if !matches!(parts.method, Method::GET | Method::HEAD) {
            let content_type = parts
                .headers
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            // 恢复备份上传的是文件本身
            let upload = parts.uri.path().ends_with("/backup/restore")
                && content_type.starts_with("application/octet-stream");
            if !content_type.starts_with("application/json") && !upload {
                return Err(ApiError::new(
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "unsupported_media_type",
                    "写操作要用 Content-Type: application/json",
                ));
            }
        }
        let token = session_token(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let hash = secret::sha256_hex(&token);
        let last_seen = db::admin::find_session(&state.db, &hash)
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        if db::now_ms() - last_seen > 60_000 {
            db::admin::touch_session(&state.db, &hash).await?;
        }
        Ok(Admin)
    }
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    for value in headers.get_all(COOKIE) {
        let Ok(value) = value.to_str() else { continue };
        for part in value.split(';') {
            if let Some((name, token)) = part.trim().split_once('=')
                && name == SESSION_COOKIE
                && !token.is_empty()
            {
                return Some(token.to_string());
            }
        }
    }
    None
}

fn session_cookie(token: &str) -> HeaderValue {
    let cookie = format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age={}; HttpOnly; Secure; SameSite=Strict",
        SESSION_DAYS * 86_400
    );
    HeaderValue::from_str(&cookie).unwrap_or_else(|_| HeaderValue::from_static(""))
}

fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("op_session=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Strict")
}

/// 建一个新会话，返回 Set-Cookie 的值。
async fn new_session(state: &AppState, ip: &str, headers: &HeaderMap) -> ApiResult<HeaderValue> {
    let token = secret::new_token();
    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();
    let expires_at = db::now_ms() + SESSION_DAYS * 86_400_000;
    db::admin::create_session(
        &state.db,
        &secret::sha256_hex(&token),
        expires_at,
        ip,
        &user_agent,
    )
    .await?;
    Ok(session_cookie(&token))
}

/// 登录失败限流：同一 IP 或同一用户名连续失败 5 次，锁 15 分钟（api.md「约定」）。只放内存。
#[derive(Default)]
pub struct LoginLimiter {
    failures: Mutex<HashMap<String, (u32, Instant)>>,
}

const MAX_FAILURES: u32 = 5;
const LOCK_TIME: Duration = Duration::from_secs(15 * 60);

impl LoginLimiter {
    fn locked(&self, keys: &[String]) -> bool {
        let map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        keys.iter().any(|k| {
            map.get(k)
                .is_some_and(|(count, last)| *count >= MAX_FAILURES && last.elapsed() < LOCK_TIME)
        })
    }

    fn fail(&self, keys: &[String]) {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        // 顺手清理早就过期的记录，防止被刷满
        map.retain(|_, (_, last)| last.elapsed() < LOCK_TIME);
        for k in keys {
            let entry = map.entry(k.clone()).or_insert((0, Instant::now()));
            entry.0 += 1;
            entry.1 = Instant::now();
        }
    }

    fn succeed(&self, keys: &[String]) {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        for k in keys {
            map.remove(k);
        }
    }
}

/// 是否已经初始化。
pub async fn setup_status(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let initialized = db::admin::get(&state.db).await?.is_some();
    Ok(Json(json!({ "initialized": initialized })))
}

#[derive(Deserialize)]
pub struct SetupRequest {
    username: String,
    password: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    cloudflare_api_token: String,
    #[serde(default)]
    timezone: Option<String>,
}

/// 首次初始化：谁先打开谁初始化（api.md 决策记录），之后这个接口失效。
pub async fn setup(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<PeerAddr>,
    headers: HeaderMap,
    Json(req): Json<SetupRequest>,
) -> ApiResult<impl IntoResponse> {
    let username = super::check_name(&req.username, "用户名")?;
    check_password(&req.password)?;
    let domain = req.domain.trim().to_lowercase();
    if !domain.is_empty() && !super::is_hostname(&domain) {
        return Err(ApiError::bad_request("invalid_domain", "主控域名格式不对"));
    }
    let timezone = match req.timezone.as_deref().map(str::trim) {
        Some(tz) if !tz.is_empty() => settings::check_timezone(tz)
            .ok_or_else(|| ApiError::bad_request("invalid_timezone", "不认识这个时区"))?,
        _ => settings::DEFAULT_TIMEZONE.to_string(),
    };

    let hash = secret::hash_password(&req.password)?;
    if !db::admin::create(&state.db, &username, &hash).await? {
        return Err(ApiError::forbidden("already_initialized", "已经初始化过了"));
    }
    if !domain.is_empty() {
        settings::set(&state.db, settings::DOMAIN, &domain).await?;
    }
    let token = req.cloudflare_api_token.trim();
    if !token.is_empty() {
        settings::set(&state.db, settings::CLOUDFLARE_API_TOKEN, &token).await?;
    }
    settings::set(&state.db, settings::TIMEZONE, &timezone).await?;
    tracing::info!(%username, "完成首次初始化");
    state.config_changed();

    let cookie = new_session(&state, &client_ip(peer.0, &headers), &headers).await?;
    Ok((
        [(SET_COOKIE, cookie)],
        Json(json!({ "username": username })),
    ))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<PeerAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> ApiResult<impl IntoResponse> {
    let ip = client_ip(peer.0, &headers);
    let keys = [format!("ip:{ip}"), format!("user:{}", req.username.trim())];
    if state.login_limiter.locked(&keys) {
        return Err(ApiError::too_many("登录失败次数太多，请 15 分钟后再试"));
    }
    let admin = db::admin::get(&state.db).await?;
    let ok = match &admin {
        Some(a) => {
            a.username == req.username.trim()
                && secret::verify_password(&req.password, &a.password_hash)
        }
        None => false,
    };
    if !ok {
        state.login_limiter.fail(&keys);
        tracing::warn!(%ip, "登录失败");
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "用户名或密码不对",
        ));
    }
    state.login_limiter.succeed(&keys);
    let cookie = new_session(&state, &ip, &headers).await?;
    let username = admin.map(|a| a.username).unwrap_or_default();
    Ok((
        [(SET_COOKIE, cookie)],
        Json(json!({ "username": username })),
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    _admin: Admin,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    if let Some(token) = session_token(&headers) {
        db::admin::delete_session(&state.db, &secret::sha256_hex(&token)).await?;
    }
    Ok(([(SET_COOKIE, clear_cookie())], Json(json!({}))))
}

pub async fn me(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Value>> {
    let admin = db::admin::get(&state.db)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    Ok(Json(json!({ "username": admin.username })))
}

#[derive(Deserialize)]
pub struct PasswordRequest {
    old_password: String,
    new_password: String,
}

/// 改密码：所有会话失效，要重新登录。
pub async fn change_password(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<PasswordRequest>,
) -> ApiResult<impl IntoResponse> {
    let admin = db::admin::get(&state.db)
        .await?
        .ok_or_else(ApiError::unauthorized)?;
    if !secret::verify_password(&req.old_password, &admin.password_hash) {
        return Err(ApiError::bad_request("wrong_password", "原密码不对"));
    }
    check_password(&req.new_password)?;
    let hash = secret::hash_password(&req.new_password)?;
    db::admin::set_password(&state.db, &hash).await?;
    tracing::info!("管理员密码已修改，全部会话失效");
    Ok(([(SET_COOKIE, clear_cookie())], Json(json!({}))))
}

fn check_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ApiError::bad_request(
            "weak_password",
            format!("密码至少 {MIN_PASSWORD_LEN} 位"),
        ));
    }
    Ok(())
}
