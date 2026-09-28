//! 套餐：流量额度 + 可用节点。改套餐对绑定的人全部生效。

use std::collections::BTreeSet;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::auth::Admin;
use super::{ApiError, ApiResult, check_name, nullable, time};
use crate::app::AppState;
use crate::db::plans::{self, Plan};
use crate::db::{is_foreign_key_violation, is_unique_violation, nodes};

#[derive(Serialize)]
pub struct PlanView {
    id: i64,
    name: String,
    /// 每个周期的流量额度（字节），null 表示不限
    traffic_quota_bytes: Option<i64>,
    node_ids: Vec<i64>,
    user_count: i64,
    created_at: String,
}

async fn views(state: &AppState, list: Vec<Plan>) -> ApiResult<Vec<PlanView>> {
    let mut node_ids = plans::node_ids(&state.db).await?;
    let user_counts = plans::user_counts(&state.db).await?;
    Ok(list
        .into_iter()
        .map(|p| PlanView {
            node_ids: node_ids.remove(&p.id).unwrap_or_default(),
            user_count: user_counts.get(&p.id).copied().unwrap_or(0),
            id: p.id,
            name: p.name,
            traffic_quota_bytes: p.traffic_quota_bytes,
            created_at: time(p.created_at),
        })
        .collect())
}

pub async fn list(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<PlanView>>> {
    let list = plans::list(&state.db).await?;
    Ok(Json(views(&state, list).await?))
}

pub async fn get(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<PlanView>> {
    one(&state, id).await
}

async fn one(state: &AppState, id: i64) -> ApiResult<Json<PlanView>> {
    let plan = plans::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("套餐不存在"))?;
    let mut list = views(state, vec![plan]).await?;
    list.pop()
        .map(Json)
        .ok_or_else(|| ApiError::internal("生成套餐信息失败"))
}

#[derive(Deserialize)]
pub struct CreatePlan {
    name: String,
    #[serde(default)]
    traffic_quota_bytes: Option<i64>,
    #[serde(default)]
    node_ids: Vec<i64>,
}

pub async fn create(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<CreatePlan>,
) -> ApiResult<Json<PlanView>> {
    let name = check_name(&req.name, "套餐名")?;
    check_quota(req.traffic_quota_bytes)?;
    let node_ids = check_nodes(&state, &req.node_ids).await?;
    let id = plans::create(&state.db, &name, req.traffic_quota_bytes, &node_ids)
        .await
        .map_err(name_conflict)?;
    tracing::info!(plan_id = id, %name, "创建套餐");
    state.config_changed();
    one(&state, id).await
}

#[derive(Deserialize)]
pub struct UpdatePlan {
    name: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    traffic_quota_bytes: Option<Option<i64>>,
    node_ids: Option<Vec<i64>>,
}

pub async fn update(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
    Json(req): Json<UpdatePlan>,
) -> ApiResult<Json<PlanView>> {
    let plan = plans::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("套餐不存在"))?;
    let name = match &req.name {
        Some(n) => check_name(n, "套餐名")?,
        None => plan.name,
    };
    let quota = req.traffic_quota_bytes.unwrap_or(plan.traffic_quota_bytes);
    check_quota(quota)?;
    let node_ids = match &req.node_ids {
        Some(ids) => check_nodes(&state, ids).await?,
        None => plans::node_ids(&state.db)
            .await?
            .remove(&id)
            .unwrap_or_default(),
    };
    plans::update(&state.db, id, &name, quota, &node_ids)
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
    plans::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("套餐不存在"))?;
    if let Some(n) = plans::user_counts(&state.db)
        .await?
        .get(&id)
        .filter(|n| **n > 0)
    {
        return Err(ApiError::conflict(
            "in_use",
            format!("有 {n} 个用户绑定了这个套餐，先给他们换套餐"),
        ));
    }
    plans::delete(&state.db, id).await.map_err(|e| {
        if is_foreign_key_violation(&e) {
            ApiError::conflict("in_use", "还有用户绑定了这个套餐")
        } else {
            e.into()
        }
    })?;
    tracing::info!(plan_id = id, "删除套餐");
    state.config_changed();
    Ok(Json(json!({})))
}

fn check_quota(quota: Option<i64>) -> ApiResult<()> {
    if quota.is_some_and(|q| q < 0) {
        return Err(ApiError::bad_request("invalid_quota", "流量额度不能是负数"));
    }
    Ok(())
}

/// 节点 ID 去重并确认都存在。
async fn check_nodes(state: &AppState, ids: &[i64]) -> ApiResult<Vec<i64>> {
    let ids: BTreeSet<i64> = ids.iter().copied().collect();
    let existing: BTreeSet<i64> = nodes::list(&state.db).await?.iter().map(|n| n.id).collect();
    if let Some(missing) = ids.iter().find(|id| !existing.contains(id)) {
        return Err(ApiError::bad_request(
            "node_not_found",
            format!("节点 {missing} 不存在"),
        ));
    }
    Ok(ids.into_iter().collect())
}

fn name_conflict(err: sqlx::Error) -> ApiError {
    if is_unique_violation(&err) {
        ApiError::conflict("name_taken", "已经有同名的套餐")
    } else {
        err.into()
    }
}
