//! 订阅模板：来源（内置、粘贴或上传的正文、远程地址和拉取周期）、兼容性报告、预览。

use axum::Json;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::auth::Admin;
use super::{ApiError, ApiResult, time_opt};
use crate::app::AppState;
use crate::db;
use crate::subscription::template::{self, TemplateSettings};
use crate::subscription::{self, Format};

#[derive(Serialize)]
pub struct TemplateView {
    source: String,
    /// 当前生效的正文（内置时是内置模板）
    content: String,
    remote_url: Option<String>,
    refresh_hours: u32,
    last_fetch_at: Option<String>,
    last_error: String,
}

fn view(t: &TemplateSettings) -> TemplateView {
    TemplateView {
        source: t.source.clone(),
        content: t.text().to_string(),
        remote_url: t.remote_url.clone(),
        refresh_hours: t.refresh_hours,
        last_fetch_at: time_opt(t.last_fetch_at),
        last_error: t.last_error.clone(),
    }
}

pub async fn get(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<TemplateView>> {
    Ok(Json(view(&template::load(&state.db).await?)))
}

#[derive(Deserialize)]
pub struct UpdateTemplate {
    source: String,
    content: Option<String>,
    remote_url: Option<String>,
    refresh_hours: Option<u32>,
}

pub async fn update(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<UpdateTemplate>,
) -> ApiResult<Json<TemplateView>> {
    let mut t = template::load(&state.db).await?;
    if let Some(hours) = req.refresh_hours {
        if !(1..=720).contains(&hours) {
            return Err(ApiError::bad_request(
                "invalid_refresh_hours",
                "拉取周期要在 1–720 小时之间",
            ));
        }
        t.refresh_hours = hours;
    }
    match req.source.as_str() {
        "builtin" => {
            t.source = "builtin".to_string();
            t.content.clear();
            t.last_error.clear();
        }
        "custom" => {
            let content = req.content.unwrap_or_default();
            template::parse(&content)
                .map_err(|e| ApiError::bad_request("invalid_template", format!("{e:#}")))?;
            t.source = "custom".to_string();
            t.content = content;
            t.last_error.clear();
        }
        "remote" => {
            let url = req.remote_url.unwrap_or_default().trim().to_string();
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err(ApiError::bad_request(
                    "invalid_url",
                    "远程地址要以 http:// 或 https:// 开头",
                ));
            }
            // 先拉一次，拉不到就不切换
            let content = template::fetch_remote(&url)
                .await
                .map_err(|e| ApiError::bad_request("fetch_failed", format!("拉取失败: {e:#}")))?;
            t.source = "remote".to_string();
            t.remote_url = Some(url);
            t.content = content;
            t.last_fetch_at = Some(db::now_ms());
            t.last_error.clear();
        }
        _ => {
            return Err(ApiError::bad_request(
                "invalid_source",
                "来源只能是 builtin、custom 或 remote",
            ));
        }
    }
    template::save(&state.db, &t).await?;
    tracing::info!(source = %t.source, "订阅模板已更新");
    Ok(Json(view(&t)))
}

/// 立即拉取远程模板。
pub async fn refresh(
    State(state): State<AppState>,
    _admin: Admin,
) -> ApiResult<Json<TemplateView>> {
    let t = refresh_remote(&state).await?;
    Ok(Json(view(&t)))
}

/// 拉取远程模板；失败时保留上一份，记下错误。定时任务也用它。
pub async fn refresh_remote(state: &AppState) -> ApiResult<TemplateSettings> {
    let mut t = template::load(&state.db).await?;
    let Some(url) = t.remote_url.clone().filter(|_| t.source == "remote") else {
        return Err(ApiError::bad_request("not_remote", "当前不是远程模板"));
    };
    match template::fetch_remote(&url).await {
        Ok(content) => {
            t.content = content;
            t.last_error.clear();
        }
        Err(err) => {
            tracing::warn!("拉取远程模板失败，保留上一份: {err:#}");
            t.last_error = format!("{err:#}");
        }
    }
    t.last_fetch_at = Some(db::now_ms());
    template::save(&state.db, &t).await?;
    Ok(t)
}

/// 兼容性报告：每种格式丢掉了哪些东西。
pub async fn report(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Value>> {
    let t = template::load(&state.db).await?;
    let tpl = template::parse(t.text())
        .map_err(|e| ApiError::bad_request("invalid_template", format!("{e:#}")))?;
    let formats: Vec<Value> = Format::ALL
        .into_iter()
        .map(|f| json!({ "format": f.name(), "dropped": tpl.report(f) }))
        .collect();
    Ok(Json(json!({ "formats": formats })))
}

#[derive(Deserialize)]
pub struct PreviewQuery {
    format: String,
    user_id: i64,
}

/// 按某个用户预览某种格式的输出（纯文本）。
pub async fn preview(
    State(state): State<AppState>,
    _admin: Admin,
    Query(q): Query<PreviewQuery>,
) -> ApiResult<impl IntoResponse> {
    let format = Format::parse(&q.format)
        .ok_or_else(|| ApiError::bad_request("invalid_format", "不认识这个格式"))?;
    let user = db::users::get(&state.db, q.user_id)
        .await?
        .ok_or_else(|| ApiError::not_found("用户不存在"))?;
    let body = subscription::preview(&state, &user, format).await?;
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body))
}
