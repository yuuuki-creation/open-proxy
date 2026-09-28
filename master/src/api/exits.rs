//! 落地出口：登记第三方 SOCKS5，或选一台服务器建成自建落地机（账号密码由主控生成）。

use std::collections::HashMap;

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::auth::Admin;
use super::nodes::pick_port;
use super::{ApiError, ApiResult, check_address, check_name, check_port, time};
use crate::app::AppState;
use crate::db::exits::{self, Exit};
use crate::db::{is_foreign_key_violation, is_unique_violation, nodes, servers};
use crate::secret;

#[derive(Serialize)]
pub struct ExitView {
    id: i64,
    name: String,
    kind: String,
    /// 连接地址：第三方的是登记的地址，自建的是落地机的地址
    host: String,
    port: i64,
    username: String,
    password: String,
    landing_server_id: Option<i64>,
    landing_server_name: Option<String>,
    /// 有多少个节点在用
    node_count: i64,
    created_at: String,
}

async fn views(state: &AppState, list: Vec<Exit>) -> ApiResult<Vec<ExitView>> {
    let servers: HashMap<i64, (String, String)> = servers::list(&state.db)
        .await?
        .into_iter()
        .map(|s| (s.id, (s.name, s.address)))
        .collect();
    let counts: HashMap<i64, i64> = exits::node_counts(&state.db).await?.into_iter().collect();
    Ok(list
        .into_iter()
        .map(|e| {
            let landing = e.landing_server_id.and_then(|id| servers.get(&id).cloned());
            ExitView {
                host: match &landing {
                    Some((_, address)) => address.clone(),
                    None => e.host.clone(),
                },
                landing_server_name: landing.map(|(name, _)| name),
                node_count: counts.get(&e.id).copied().unwrap_or(0),
                id: e.id,
                name: e.name,
                kind: e.kind,
                port: e.port,
                username: e.username,
                password: e.password,
                landing_server_id: e.landing_server_id,
                created_at: time(e.created_at),
            }
        })
        .collect())
}

pub async fn list(State(state): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<ExitView>>> {
    let list = exits::list(&state.db).await?;
    Ok(Json(views(&state, list).await?))
}

pub async fn get(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<ExitView>> {
    let exit = find(&state, id).await?;
    one(&state, exit).await
}

async fn one(state: &AppState, exit: Exit) -> ApiResult<Json<ExitView>> {
    let mut list = views(state, vec![exit]).await?;
    list.pop()
        .map(Json)
        .ok_or_else(|| ApiError::internal("生成出口信息失败"))
}

async fn find(state: &AppState, id: i64) -> ApiResult<Exit> {
    exits::get(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found("落地出口不存在"))
}

#[derive(Deserialize)]
pub struct CreateExit {
    name: String,
    kind: String,
    /// 第三方：地址、端口、账号、密码
    #[serde(default)]
    host: String,
    #[serde(default)]
    port: Option<i64>,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    /// 自建：哪台服务器当落地机
    #[serde(default)]
    landing_server_id: Option<i64>,
}

pub async fn create(
    State(state): State<AppState>,
    _admin: Admin,
    Json(req): Json<CreateExit>,
) -> ApiResult<Json<ExitView>> {
    let name = check_name(&req.name, "出口名")?;
    let exit = match req.kind.as_str() {
        "third_party" => Exit {
            id: 0,
            name,
            kind: req.kind,
            host: check_address(&req.host)?,
            port: check_port(req.port.unwrap_or(0), "端口")?,
            username: check_credential(&req.username, "账号")?,
            password: check_credential(&req.password, "密码")?,
            landing_server_id: None,
            created_at: 0,
        },
        "self_built" => {
            let server_id = req.landing_server_id.ok_or_else(|| {
                ApiError::bad_request("invalid_landing", "自建出口要选一台服务器当落地机")
            })?;
            let server = servers::get(&state.db, server_id)
                .await?
                .ok_or_else(|| ApiError::bad_request("server_not_found", "服务器不存在"))?;
            let occupied = nodes::occupied_ports(&state.db, server_id, None, None).await?;
            let port = match req.port {
                Some(p) => {
                    check_port(p, "端口")?;
                    if occupied.iter().any(|(s, e)| *s <= p && p <= *e) {
                        return Err(ApiError::conflict(
                            "port_taken",
                            format!("端口 {p} 已被占用"),
                        ));
                    }
                    p
                }
                None => pick_port((server.port_range_start, server.port_range_end), &occupied)
                    .ok_or_else(|| {
                        ApiError::conflict("no_free_port", "落地机的端口范围里没有空闲端口")
                    })?,
            };
            Exit {
                id: 0,
                name,
                kind: req.kind,
                host: String::new(),
                port,
                username: secret::random_password(12),
                password: secret::random_password(24),
                landing_server_id: Some(server_id),
                created_at: 0,
            }
        }
        _ => {
            return Err(ApiError::bad_request(
                "invalid_kind",
                "出口种类只能是 third_party 或 self_built",
            ));
        }
    };
    let id = exits::create(&state.db, &exit).await.map_err(|e| {
        if is_unique_violation(&e) {
            ApiError::conflict(
                "name_taken",
                "已经有同名的出口，或者这台服务器已经是别的出口的落地机",
            )
        } else {
            e.into()
        }
    })?;
    tracing::info!(exit_id = id, kind = %exit.kind, "创建落地出口");
    state.config_changed();
    let exit = find(&state, id).await?;
    one(&state, exit).await
}

#[derive(Deserialize)]
pub struct UpdateExit {
    name: Option<String>,
    host: Option<String>,
    port: Option<i64>,
    username: Option<String>,
    password: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
    Json(req): Json<UpdateExit>,
) -> ApiResult<Json<ExitView>> {
    let mut exit = find(&state, id).await?;
    if let Some(name) = &req.name {
        exit.name = check_name(name, "出口名")?;
    }
    if let Some(port) = req.port {
        exit.port = check_port(port, "端口")?;
    }
    if exit.kind == "third_party" {
        if let Some(host) = &req.host {
            exit.host = check_address(host)?;
        }
        if let Some(username) = &req.username {
            exit.username = check_credential(username, "账号")?;
        }
        if let Some(password) = &req.password {
            exit.password = check_credential(password, "密码")?;
        }
    } else if let Some(server_id) = exit.landing_server_id {
        let occupied = nodes::occupied_ports(&state.db, server_id, None, Some(exit.id)).await?;
        if occupied
            .iter()
            .any(|(s, e)| *s <= exit.port && exit.port <= *e)
        {
            return Err(ApiError::conflict(
                "port_taken",
                format!("端口 {} 已被占用", exit.port),
            ));
        }
    }
    exits::update(&state.db, &exit).await.map_err(|e| {
        if is_unique_violation(&e) {
            ApiError::conflict("name_taken", "已经有同名的出口")
        } else {
            e.into()
        }
    })?;
    state.config_changed();
    one(&state, exit).await
}

pub async fn delete(
    State(state): State<AppState>,
    _admin: Admin,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    find(&state, id).await?;
    let counts: HashMap<i64, i64> = exits::node_counts(&state.db).await?.into_iter().collect();
    if let Some(n) = counts.get(&id).filter(|n| **n > 0) {
        return Err(ApiError::conflict(
            "in_use",
            format!("有 {n} 个节点在用这个出口，先给它们换出口"),
        ));
    }
    exits::delete(&state.db, id).await.map_err(|e| {
        if is_foreign_key_violation(&e) {
            ApiError::conflict("in_use", "还有节点在用这个出口")
        } else {
            e.into()
        }
    })?;
    tracing::info!(exit_id = id, "删除落地出口");
    state.config_changed();
    Ok(Json(json!({})))
}

fn check_credential(s: &str, what: &str) -> ApiResult<String> {
    if s.len() > 255 {
        return Err(ApiError::bad_request(
            "invalid_credential",
            format!("{what}最长 255 个字符"),
        ));
    }
    Ok(s.to_string())
}
