//! 流量入账、停用规则、每月重置（database.md「流量怎么算」「停用规则」）。

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use sqlx::SqlitePool;

use crate::app::AppState;
use crate::db::{self, settings};
use crate::pb::agentv1 as pb;

/// Agent 的实例 ID 变了（进程重启过）：删掉这台服务器的计数器，之后的上报从零算增量。
pub async fn check_instance(
    pool: &SqlitePool,
    server_id: i64,
    instance_id: u64,
) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;
    check_instance_in(&mut tx, server_id, instance_id).await?;
    tx.commit().await
}

async fn check_instance_in(
    tx: &mut sqlx::SqliteConnection,
    server_id: i64,
    instance_id: u64,
) -> sqlx::Result<()> {
    // u64 按位存成 i64
    let id = instance_id as i64;
    let row: Option<(i64,)> = sqlx::query_as("SELECT agent_instance_id FROM servers WHERE id = ?")
        .bind(server_id)
        .fetch_optional(&mut *tx)
        .await?;
    if row.is_some_and(|(old,)| old != id) {
        sqlx::query("DELETE FROM traffic_counters WHERE server_id = ?")
            .bind(server_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE servers SET agent_instance_id = ? WHERE id = ?")
            .bind(id)
            .bind(server_id)
            .execute(&mut *tx)
            .await?;
    }
    Ok(())
}

/// 处理一条流量上报，在一个短事务里入账。有用户流量时顺带检查超额。
pub async fn ingest(
    state: &AppState,
    server_id: i64,
    report: pb::TrafficReport,
) -> anyhow::Result<()> {
    let day = settings::today(&state.db).await;
    let mut tx = state.db.begin().await?;
    check_instance_in(&mut tx, server_id, report.instance_id).await?;

    let mut had_user_traffic = false;
    for row in &report.users {
        let (node_id, user_id) = (row.node_id as i64, row.user_id as i64);
        let (up, down) = (row.uplink as i64, row.downlink as i64);
        let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
        if exists.is_none() {
            continue; // 用户已删除
        }
        let last: Option<(i64, i64)> = sqlx::query_as(
            "SELECT last_up, last_down FROM traffic_counters WHERE server_id = ? AND node_id = ? AND user_id = ?",
        )
        .bind(server_id)
        .bind(node_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        // 没有上次的值（新实例或新出现的行）时增量就是本次：Agent 的计数从零开始
        let (d_up, d_down) = match last {
            Some((lu, ld)) if up >= lu && down >= ld => (up - lu, down - ld),
            _ => (up, down),
        };
        sqlx::query(
            "INSERT INTO traffic_counters (server_id, node_id, user_id, last_up, last_down) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (server_id, node_id, user_id) DO UPDATE SET last_up = excluded.last_up, last_down = excluded.last_down",
        )
        .bind(server_id)
        .bind(node_id)
        .bind(user_id)
        .bind(up)
        .bind(down)
        .execute(&mut *tx)
        .await?;
        if d_up == 0 && d_down == 0 {
            continue;
        }
        had_user_traffic = true;
        sqlx::query(
            "UPDATE users SET up_total = up_total + ?, down_total = down_total + ? WHERE id = ?",
        )
        .bind(d_up)
        .bind(d_down)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO traffic_daily (day, user_id, node_id, server_id, up, down) VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (day, user_id, node_id) DO UPDATE SET up = up + excluded.up, down = down + excluded.down",
        )
        .bind(&day)
        .bind(user_id)
        .bind(node_id)
        .bind(server_id)
        .bind(d_up)
        .bind(d_down)
        .execute(&mut *tx)
        .await?;
    }

    if let Some(nic) = &report.network {
        let (rx, tx_bytes) = (nic.rx_bytes as i64, nic.tx_bytes as i64);
        // 服务器刚被删除时（卸载前补发的最后一次上报）没有这一行，网卡计数不记
        let row: Option<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT nic_boot_id, nic_interface, nic_last_rx, nic_last_tx FROM servers WHERE id = ?",
        )
        .bind(server_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((boot_id, iface, last_rx, last_tx)) = row else {
            tx.commit().await?;
            return Ok(());
        };
        // 第一次见到这台服务器或换了网卡：只记为起点（开机以来的累计不是今天的流量）；
        // 服务器重启过：增量就是本次；否则是差值
        let (d_rx, d_tx, reset) = if boot_id.is_empty() || iface != nic.interface {
            (0, 0, true)
        } else if boot_id != nic.boot_id {
            (rx, tx_bytes, true)
        } else if rx >= last_rx && tx_bytes >= last_tx {
            (rx - last_rx, tx_bytes - last_tx, false)
        } else {
            (rx, tx_bytes, true)
        };
        sqlx::query(
            "UPDATE servers SET nic_boot_id = ?, nic_interface = ?, nic_last_rx = ?, nic_last_tx = ? WHERE id = ?",
        )
        .bind(&nic.boot_id)
        .bind(&nic.interface)
        .bind(rx)
        .bind(tx_bytes)
        .bind(server_id)
        .execute(&mut *tx)
        .await?;
        if d_rx > 0 || d_tx > 0 {
            sqlx::query(
                "INSERT INTO server_traffic_daily (day, server_id, rx, tx) VALUES (?, ?, ?, ?)
                 ON CONFLICT (day, server_id) DO UPDATE SET rx = rx + excluded.rx, tx = tx + excluded.tx",
            )
            .bind(&day)
            .bind(server_id)
            .bind(d_rx)
            .bind(d_tx)
            .execute(&mut *tx)
            .await?;
        }
        state
            .hub
            .record_nic(server_id, nic.rx_bytes, nic.tx_bytes, reset);
    }

    sqlx::query("UPDATE servers SET last_seen_at = ? WHERE id = ?")
        .bind(db::now_ms())
        .bind(server_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    // 收到流量时检查超额；停用原因变了就重算期望状态
    if had_user_traffic && update_blocks(&state.db).await? {
        state.config_changed();
    }
    Ok(())
}

/// 按规则重算每个用户的停用原因：手动 > 到期 > 超额 > 不停用。返回是否有变化。
pub async fn update_blocks(pool: &SqlitePool) -> anyhow::Result<bool> {
    let tz = settings::timezone(pool).await;
    let today = Utc::now().with_timezone(&tz).date_naive();
    let quotas: HashMap<i64, Option<i64>> = db::plans::list(pool)
        .await?
        .into_iter()
        .map(|p| (p.id, p.traffic_quota_bytes))
        .collect();
    let mut changed = false;
    for user in db::users::list(pool).await? {
        let expired = user
            .expires_on
            .as_deref()
            .and_then(parse_date)
            // 到期日当天结束（管理员时区的 24 点）时停用
            .is_some_and(|d| today > d);
        let over_quota = quotas
            .get(&user.plan_id)
            .copied()
            .flatten()
            .is_some_and(|quota| user.period_used() >= quota);
        let reason = if !user.enabled {
            "manual"
        } else if expired {
            "expired"
        } else if over_quota {
            "over_quota"
        } else {
            ""
        };
        if reason != user.blocked_reason {
            db::users::set_blocked(pool, user.id, reason).await?;
            tracing::info!(
                user_id = user.id,
                name = %user.name,
                from = %user.blocked_reason,
                to = reason,
                "用户的停用原因变了"
            );
            changed = true;
        }
    }
    Ok(changed)
}

/// 每月重置：到了重置日（开通日的「日」，短月夹到月末）在管理员时区的 0 点，
/// 令本周期起点 = 当前累计；同一周期只重置一次。返回是否有人被重置。
pub async fn monthly_reset(pool: &SqlitePool) -> anyhow::Result<bool> {
    let tz = settings::timezone(pool).await;
    let today = Utc::now().with_timezone(&tz).date_naive();
    let mut changed = false;
    for user in db::users::list(pool).await? {
        let Some(started) = parse_date(&user.started_on) else {
            continue;
        };
        let Some(boundary) = last_reset_date(started, today) else {
            continue;
        };
        let boundary_ms = tz
            .from_local_datetime(&boundary.and_time(chrono::NaiveTime::MIN))
            .earliest()
            .map(|t| t.timestamp_millis())
            .unwrap_or_else(|| {
                boundary
                    .and_time(chrono::NaiveTime::MIN)
                    .and_utc()
                    .timestamp_millis()
            });
        if user.last_reset_at.is_none_or(|t| t < boundary_ms) {
            db::users::reset_period_at(pool, user.id, db::now_ms()).await?;
            tracing::info!(user_id = user.id, name = %user.name, "到了重置日，本周期用量清零");
            changed = true;
        }
    }
    Ok(changed)
}

/// 某年某月的重置日：开通日的「日」，这个月没有这一天时用月末最后一天。
pub fn reset_date_in(year: i32, month: u32, day: u32) -> NaiveDate {
    let mut d = day;
    loop {
        if let Some(date) = NaiveDate::from_ymd_opt(year, month, d) {
            return date;
        }
        d -= 1;
    }
}

/// 不晚于今天、又晚于开通日的最近一个重置日；还没到第一个重置日时为 None。
pub fn last_reset_date(started: NaiveDate, today: NaiveDate) -> Option<NaiveDate> {
    let day = started.day();
    let this_month = reset_date_in(today.year(), today.month(), day);
    let candidate = if this_month <= today {
        this_month
    } else if today.month() == 1 {
        reset_date_in(today.year() - 1, 12, day)
    } else {
        reset_date_in(today.year(), today.month() - 1, day)
    };
    (candidate > started).then_some(candidate)
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}
