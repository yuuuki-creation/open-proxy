//! 用户：拼车的朋友。续期、换套餐、手动停用或启用都是 PATCH（api.md）。
//! 停用原因（手动 > 到期 > 超额）由检查任务按规则算（P3），这里只改字段并通知「配置变了」。

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::auth::Admin;
use super::{ApiError, ApiResult, check_date, check_name, nullable, time, time_opt};
use crate::app::AppState;
use crate::db::users::{self, Credentials, User, UserFields};
use crate::db::{is_unique_violation, plans, settings};
use crate::secret;

#[derive(Serialize)]
pub struct UserView {
    id: i64,
    name: String,
    remark: String,
    plan_id: i64,
    plan_name: String,
    /// 套餐的流量额度，null 表示不限
    quota_bytes: Option<i64>,
    enabled: bool,
    started_on: String,
    expires_on: Option<String>,
    /// 本周期用量
    used_bytes: i64,
    up_total: i64,
    down_total: i64,
    /// 空 / manual / expired / over_quota
    blocked_reason: String,
    blocked_since: Option<String>,
    /// 订阅链接；主控还没有域名时为 null
    sub_url: Option<String>,
    /// 这个用户涉及的服务器里，还没同步最新期望状态的台数（停用、恢复是否已生效）
    pending_servers: i64,
    created_at: String,
}

async fn views(state: &AppState, list: Vec<User>) -> ApiResult<Vec<UserView>> {
    let plans: HashMap<i64, (String, Option<i64>)> = plans::list(&state.db)
        .await?
        .into_iter()
        .map(|p| (p.id, (p.name, p.traffic_quota_bytes)))
        .collect();
    let base_url = state.base_url().await?;
    let pending = crate::state::pending_servers(state).await?;
    Ok(list
        .into_iter()
        .map(|u| {
            let (plan_name, quota_bytes) = plans.get(&u.plan_id).cloned().unwrap_or_default();
            UserView {
                used_bytes: u.period_used(),
                pending_servers: pending.get(&u.id).copied().unwrap_or(0),
                sub_url: base_url.as_ref().map(|b| format!("{b}/s/{}", u.sub_token)),
                id: u.id,
                name: u.name,
                remark: u.remark,
                plan_id: u.plan_id,
                plan_name,
                quota_bytes,
                enabled: u.enabled,
                started_on: u.started_on,
                expires_on: u.expires_on,
                up_total: u.up_total,
                down_total: u.down_total,
                blocked_reason: u.blocked_reason,
                blocked_since: time_opt(u.blocked_since),
                created_at: time(u.created_at),
            }
        })
        .collect())
}

pub async fn list(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<UserView>>> {
    let list = users::list(&state.db).await?;
    Ok(Json(views(&state, list).await?))
}

pub async fn get(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<UserView>> {
    one(&state, id).await
}

async fn one(state: &AppState, id: i64) -> ApiResult<Json<UserView>> {
    let user = find(state, id).await?;
    let mut list = views(state, vec![user]).await?;
    list.pop()
        .map(Json)
        .ok_or_else(|| ApiError::internal("生成用户信息失败"))
}

async fn find(state: &AppState, id: i64) -> ApiResult<User> {
    users::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("用户不存在"))
}

#[derive(Deserialize)]
pub struct CreateUser {
    name: String,
    #[serde(default)]
    remark: String,
    plan_id: i64,
    /// 开通日，默认今天（管理员时区）
    #[serde(default)]
    started_on: Option<String>,
    /// 到期日，不填表示永久
    #[serde(default)]
    expires_on: Option<String>,
    #[serde(default = "yes")]
    enabled: bool,
}

fn yes() -> bool {
    true
}

pub async fn create(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<CreateUser>,
) -> ApiResult<Json<UserView>> {
    let started_on = match req.started_on.as_deref() {
        Some(d) if !d.trim().is_empty() => check_date(d, "开通日")?,
        _ => settings::today(&state.db).await,
    };
    let fields = UserFields {
        name: check_name(&req.name, "用户名")?,
        remark: check_remark(&req.remark)?,
        plan_id: check_plan(&state, req.plan_id).await?,
        enabled: req.enabled,
        started_on,
        expires_on: check_expires(req.expires_on.as_deref())?,
    };
    let id = users::create(&state.db, &fields, &new_credentials())
        .await
        .map_err(name_conflict)?;
    tracing::info!(user_id = id, name = %fields.name, "创建用户");
    state.config_changed();
    one(&state, id).await
}

#[derive(Deserialize)]
pub struct UpdateUser {
    name: Option<String>,
    remark: Option<String>,
    plan_id: Option<i64>,
    enabled: Option<bool>,
    started_on: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    expires_on: Option<Option<String>>,
}

pub async fn update(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUser>,
) -> ApiResult<Json<UserView>> {
    let user = find(&state, id).await?;
    let fields = UserFields {
        name: match &req.name {
            Some(n) => check_name(n, "用户名")?,
            None => user.name,
        },
        remark: match &req.remark {
            Some(r) => check_remark(r)?,
            None => user.remark,
        },
        plan_id: match req.plan_id {
            Some(p) => check_plan(&state, p).await?,
            None => user.plan_id,
        },
        enabled: req.enabled.unwrap_or(user.enabled),
        started_on: match &req.started_on {
            Some(d) => check_date(d, "开通日")?,
            None => user.started_on,
        },
        expires_on: match &req.expires_on {
            Some(d) => check_expires(d.as_deref())?,
            None => user.expires_on,
        },
    };
    users::update(&state.db, id, &fields)
        .await
        .map_err(name_conflict)?;
    state.config_changed();
    one(&state, id).await
}

pub async fn delete(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    let user = find(&state, id).await?;
    users::delete(&state.db, id).await?;
    tracing::info!(user_id = id, name = %user.name, "删除用户，流量记录一起删除");
    state.config_changed();
    Ok(Json(json!({})))
}

/// 清零本周期用量：重置日不变，历史流量和日账本不变。
pub async fn reset_period(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<UserView>> {
    find(&state, id).await?;
    users::reset_period(&state.db, id).await?;
    tracing::info!(user_id = id, "清零本周期用量");
    state.config_changed();
    one(&state, id).await
}

/// 同时换订阅链接和全部凭据，推送到所有相关服务器。
pub async fn reset_credentials(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<UserView>> {
    find(&state, id).await?;
    users::set_credentials(&state.db, id, &new_credentials()).await?;
    tracing::info!(user_id = id, "重置订阅链接和凭据");
    state.config_changed();
    one(&state, id).await
}

fn new_credentials() -> Credentials {
    Credentials {
        uuid: secret::new_uuid(),
        password: secret::random_password(24),
        ss_key: secret::ss2022_key(),
        // 256 位随机数（subscription.md 要求 128 位以上）
        sub_token: secret::new_token(),
    }
}

async fn check_plan(state: &AppState, plan_id: i64) -> ApiResult<i64> {
    plans::get(&state.db, plan_id)
        .await?
        .ok_or_else(|| ApiError::bad_request("plan_not_found", "套餐不存在"))?;
    Ok(plan_id)
}

fn check_expires(date: Option<&str>) -> ApiResult<Option<String>> {
    match date.map(str::trim) {
        Some(d) if !d.is_empty() => Ok(Some(check_date(d, "到期日")?)),
        _ => Ok(None),
    }
}

fn check_remark(remark: &str) -> ApiResult<String> {
    let remark = remark.trim();
    if remark.chars().count() > 200 {
        return Err(ApiError::bad_request("invalid_remark", "备注最长 200 个字"));
    }
    Ok(remark.to_string())
}

fn name_conflict(err: sqlx::Error) -> ApiError {
    if is_unique_violation(&err) {
        ApiError::conflict("name_taken", "已经有同名的用户")
    } else {
        err.into()
    }
}
