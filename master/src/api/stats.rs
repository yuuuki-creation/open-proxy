//! 流量统计：概览、按用户 / 节点 / 服务器的每日流量、单个用户和服务器的明细。
//! 按天的数据只有日账本（traffic_daily、server_traffic_daily），日期按管理员时区。

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::auth::Admin;
use super::{ApiError, ApiResult, check_date};
use crate::app::AppState;
use crate::db::{self, settings};

#[derive(Deserialize)]
pub struct Range {
    from: Option<String>,
    to: Option<String>,
}

/// 日期范围，默认最近 30 天（含今天），最长一年。
async fn range(state: &AppState, q: &Range) -> ApiResult<(String, String)> {
    let today = settings::today(&state.db).await;
    let to = match &q.to {
        Some(d) => check_date(d, "结束日期")?,
        None => today,
    };
    let to_date = NaiveDate::parse_from_str(&to, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("invalid_date", "结束日期不对"))?;
    let from = match &q.from {
        Some(d) => check_date(d, "开始日期")?,
        None => (to_date - Duration::days(29))
            .format("%Y-%m-%d")
            .to_string(),
    };
    let from_date = NaiveDate::parse_from_str(&from, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("invalid_date", "开始日期不对"))?;
    if from_date > to_date || (to_date - from_date).num_days() > 366 {
        return Err(ApiError::bad_request(
            "invalid_range",
            "日期范围不对：开始不能晚于结束，最长一年",
        ));
    }
    Ok((from, to))
}

#[derive(Serialize)]
struct DayPoint {
    day: String,
    up: i64,
    down: i64,
}

/// 概览：今天、本月（自然月）总流量，用户用量排行（本周期），最近 30 天每日趋势，服务器和用户数。
pub async fn overview(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Value>> {
    let pool = &state.db;
    let today = settings::today(pool).await;
    let today_date = NaiveDate::parse_from_str(&today, "%Y-%m-%d")
        .map_err(|_| ApiError::internal("日期出错"))?;
    let month_start = today_date
        .with_day(1)
        .unwrap_or(today_date)
        .format("%Y-%m-%d")
        .to_string();
    let since = (today_date - Duration::days(29))
        .format("%Y-%m-%d")
        .to_string();

    let sum = |since: String| async move {
        sqlx::query_as::<_, (i64, i64)>(
            "SELECT IFNULL(SUM(up), 0), IFNULL(SUM(down), 0) FROM traffic_daily WHERE day >= ?",
        )
        .bind(since)
        .fetch_one(pool)
        .await
    };
    let (today_up, today_down) = sum(today.clone()).await?;
    let (month_up, month_down) = sum(month_start).await?;
    let daily: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT day, SUM(up), SUM(down) FROM traffic_daily WHERE day >= ? GROUP BY day ORDER BY day",
    )
    .bind(&since)
    .fetch_all(pool)
    .await?;
    // 没有流量的日子也补 0，图表连续
    let by_day: HashMap<String, (i64, i64)> =
        daily.into_iter().map(|(d, u, dn)| (d, (u, dn))).collect();
    let daily: Vec<DayPoint> = (0..30)
        .map(|i| {
            let day = (today_date - Duration::days(29 - i))
                .format("%Y-%m-%d")
                .to_string();
            let (up, down) = by_day.get(&day).copied().unwrap_or((0, 0));
            DayPoint { day, up, down }
        })
        .collect();

    let quotas: HashMap<i64, Option<i64>> = db::plans::list(pool)
        .await?
        .into_iter()
        .map(|p| (p.id, p.traffic_quota_bytes))
        .collect();
    let users = db::users::list(pool).await?;
    let mut top: Vec<Value> = users
        .iter()
        .map(|u| {
            json!({
                "id": u.id,
                "name": u.name,
                "used_bytes": u.period_used(),
                "quota_bytes": quotas.get(&u.plan_id).copied().flatten(),
            })
        })
        .collect();
    top.sort_by_key(|v| std::cmp::Reverse(v["used_bytes"].as_i64().unwrap_or(0)));
    top.truncate(10);

    let servers = db::servers::list(pool).await?;
    let online = servers.iter().filter(|s| state.hub.is_online(s.id)).count();
    let blocked = users
        .iter()
        .filter(|u| !u.blocked_reason.is_empty())
        .count();
    Ok(Json(json!({
        "today": { "up": today_up, "down": today_down },
        "month": { "up": month_up, "down": month_down },
        "daily": daily,
        "top_users": top,
        "servers": { "total": servers.len(), "online": online },
        "users": { "total": users.len(), "blocked": blocked },
    })))
}

#[derive(Deserialize)]
pub struct TrafficQuery {
    group_by: String,
    from: Option<String>,
    to: Option<String>,
}

#[derive(Serialize)]
struct Series {
    id: i64,
    name: String,
    points: Vec<DayPoint>,
}

/// 每日流量，按用户、节点或服务器分组（服务器维度是经过这台服务器的代理流量）。
pub async fn traffic(
    State(state): State<AppState>,
    _admin: Admin,
    Query(q): Query<TrafficQuery>,
) -> ApiResult<Json<Value>> {
    let (from, to) = range(
        &state,
        &Range {
            from: q.from.clone(),
            to: q.to.clone(),
        },
    )
    .await?;
    let (column, names): (&str, HashMap<i64, String>) = match q.group_by.as_str() {
        "user" => (
            "user_id",
            db::users::list(&state.db)
                .await?
                .into_iter()
                .map(|u| (u.id, u.name))
                .collect(),
        ),
        "node" => (
            "node_id",
            db::nodes::list(&state.db)
                .await?
                .into_iter()
                .map(|n| (n.id, n.name))
                .collect(),
        ),
        "server" => (
            "server_id",
            db::servers::list(&state.db)
                .await?
                .into_iter()
                .map(|s| (s.id, s.name))
                .collect(),
        ),
        _ => {
            return Err(ApiError::bad_request(
                "invalid_group_by",
                "group_by 只能是 user、node 或 server",
            ));
        }
    };
    let rows: Vec<(i64, String, i64, i64)> = sqlx::query_as(&format!(
        "SELECT {column}, day, SUM(up), SUM(down) FROM traffic_daily
         WHERE day BETWEEN ? AND ? GROUP BY {column}, day ORDER BY {column}, day"
    ))
    .bind(&from)
    .bind(&to)
    .fetch_all(&state.db)
    .await?;
    let mut grouped: BTreeMap<i64, Vec<DayPoint>> = BTreeMap::new();
    for (id, day, up, down) in rows {
        grouped
            .entry(id)
            .or_default()
            .push(DayPoint { day, up, down });
    }
    let series: Vec<Series> = grouped
        .into_iter()
        .map(|(id, points)| Series {
            name: names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| format!("已删除（#{id}）")),
            id,
            points,
        })
        .collect();
    Ok(Json(json!({ "from": from, "to": to, "series": series })))
}

/// 一个用户按天、按节点的明细。
pub async fn user_traffic(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
    Query(q): Query<Range>,
) -> ApiResult<Json<Value>> {
    db::users::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("用户不存在"))?;
    let (from, to) = range(&state, &q).await?;
    let nodes: HashMap<i64, String> = db::nodes::list(&state.db)
        .await?
        .into_iter()
        .map(|n| (n.id, n.name))
        .collect();
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        "SELECT day, node_id, up, down FROM traffic_daily
         WHERE user_id = ? AND day BETWEEN ? AND ? ORDER BY day, node_id",
    )
    .bind(id)
    .bind(&from)
    .bind(&to)
    .fetch_all(&state.db)
    .await?;
    let rows: Vec<Value> = rows
        .into_iter()
        .map(|(day, node_id, up, down)| {
            json!({
                "day": day,
                "node_id": node_id,
                "node_name": nodes.get(&node_id).cloned().unwrap_or_else(|| format!("已删除（#{node_id}）")),
                "up": up,
                "down": down,
            })
        })
        .collect();
    Ok(Json(json!({ "from": from, "to": to, "rows": rows })))
}

/// 一台服务器网卡的每日收发。
pub async fn server_traffic(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
    Query(q): Query<Range>,
) -> ApiResult<Json<Value>> {
    let (from, to) = range(&state, &q).await?;
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT day, rx, tx FROM server_traffic_daily
         WHERE server_id = ? AND day BETWEEN ? AND ? ORDER BY day",
    )
    .bind(id)
    .bind(&from)
    .bind(&to)
    .fetch_all(&state.db)
    .await?;
    let rows: Vec<Value> = rows
        .into_iter()
        .map(|(day, rx, tx)| json!({ "day": day, "rx": rx, "tx": tx }))
        .collect();
    Ok(Json(json!({ "from": from, "to": to, "rows": rows })))
}

/// 服务器本月（从上一个重置日起）的网卡收发。重置日没设置时按每月 1 号。
pub async fn server_month_usage(
    state: &AppState,
    server_id: i64,
    reset_day: Option<i64>,
) -> ApiResult<(i64, i64)> {
    let today = settings::today(&state.db).await;
    let today_date = NaiveDate::parse_from_str(&today, "%Y-%m-%d")
        .map_err(|_| ApiError::internal("日期出错"))?;
    let day = reset_day.unwrap_or(1).clamp(1, 31) as u32;
    let this_month = crate::traffic::reset_date_in(today_date.year(), today_date.month(), day);
    let since = if this_month <= today_date {
        this_month
    } else if today_date.month() == 1 {
        crate::traffic::reset_date_in(today_date.year() - 1, 12, day)
    } else {
        crate::traffic::reset_date_in(today_date.year(), today_date.month() - 1, day)
    };
    let (rx, tx): (i64, i64) = sqlx::query_as(
        "SELECT IFNULL(SUM(rx), 0), IFNULL(SUM(tx), 0) FROM server_traffic_daily WHERE server_id = ? AND day >= ?",
    )
    .bind(server_id)
    .bind(since.format("%Y-%m-%d").to_string())
    .fetch_one(&state.db)
    .await?;
    Ok((rx, tx))
}
