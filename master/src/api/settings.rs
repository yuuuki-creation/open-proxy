//! 设置：主控域名、时区、Cloudflare Token（只写，读取时只返回是否已设置）。

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use super::auth::Admin;
use super::{ApiError, ApiResult};
use crate::VERSION;
use crate::app::AppState;
use crate::db::settings;

pub async fn get(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Value>> {
    let domain: Option<String> = settings::get(&state.db, settings::DOMAIN).await?;
    let token: Option<String> = settings::get(&state.db, settings::CLOUDFLARE_API_TOKEN).await?;
    let timezone = settings::timezone(&state.db).await;
    Ok(Json(json!({
        "domain": domain.unwrap_or_default(),
        "timezone": timezone.name(),
        "cloudflare_api_token_set": token.is_some_and(|t| !t.is_empty()),
        "public_url": state.base_url().await?,
        "version": VERSION,
    })))
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    domain: Option<String>,
    timezone: Option<String>,
    /// 空字符串表示清除
    cloudflare_api_token: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<UpdateSettings>,
) -> ApiResult<Json<Value>> {
    if let Some(domain) = &req.domain {
        let domain = domain.trim().to_lowercase();
        if !domain.is_empty() && !super::is_hostname(&domain) {
            return Err(ApiError::bad_request("invalid_domain", "主控域名格式不对"));
        }
        settings::set(&state.db, settings::DOMAIN, &domain).await?;
    }
    if let Some(tz) = &req.timezone {
        let tz = settings::check_timezone(tz.trim())
            .ok_or_else(|| ApiError::bad_request("invalid_timezone", "不认识这个时区"))?;
        settings::set(&state.db, settings::TIMEZONE, &tz).await?;
    }
    if let Some(token) = &req.cloudflare_api_token {
        let token = token.trim();
        if token.is_empty() {
            settings::delete(&state.db, settings::CLOUDFLARE_API_TOKEN).await?;
        } else {
            settings::set(&state.db, settings::CLOUDFLARE_API_TOKEN, &token).await?;
        }
    }
    state.config_changed();
    if req.domain.is_some() || req.cloudflare_api_token.is_some() {
        state.check_certificates();
    }
    get(State(state), Admin).await
}
