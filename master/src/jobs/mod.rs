//! 定时任务：期望状态推送、每分钟的停用检查和每月重置、定期清理。

use std::time::Duration;

use crate::app::AppState;
use crate::{db, state, traffic};

pub fn spawn(app: &AppState) {
    tokio::spawn(state::run(app.clone()));
    tokio::spawn(every_minute(app.clone()));
    tokio::spawn(housekeeping(app.clone()));
    tokio::spawn(remote_template(app.clone()));
    tokio::spawn(certificates(app.clone()));
}

/// 每 10 分钟检查一次需要自动申请或续期的证书（启动时马上检查一次）。
async fn certificates(app: AppState) {
    let mut tick = tokio::time::interval(Duration::from_secs(600));
    loop {
        tick.tick().await;
        crate::acme::check_all(&app, app.acme_staging).await;
    }
}

/// 每 10 分钟看一次：远程模板到了拉取周期就拉一次，失败保留上一份。
async fn remote_template(app: AppState) {
    let mut tick = tokio::time::interval(Duration::from_secs(600));
    loop {
        tick.tick().await;
        let due = match crate::subscription::template::load(&app.db).await {
            Ok(t) => {
                t.source == "remote"
                    && t.last_fetch_at.is_none_or(|at| {
                        db::now_ms() - at >= i64::from(t.refresh_hours) * 3_600_000
                    })
            }
            Err(err) => {
                tracing::warn!("读取订阅模板设置出错: {err:#}");
                false
            }
        };
        if due {
            let _ = crate::api::template::refresh_remote(&app).await;
        }
    }
}

/// 每分钟：每月重置、到期检查。停用原因变了就重算期望状态。
async fn every_minute(app: AppState) {
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    loop {
        tick.tick().await;
        let mut changed = false;
        match traffic::monthly_reset(&app.db).await {
            Ok(c) => changed |= c,
            Err(err) => tracing::error!("每月重置出错: {err:#}"),
        }
        match traffic::update_blocks(&app.db).await {
            Ok(c) => changed |= c,
            Err(err) => tracing::error!("检查停用出错: {err:#}"),
        }
        if changed {
            app.config_changed();
        }
    }
}

/// 每 10 分钟：WAL checkpoint（写流量用短事务，定期收缩 WAL，architecture.md「数据库」）、删过期会话。
async fn housekeeping(app: AppState) {
    let mut tick = tokio::time::interval(Duration::from_secs(600));
    loop {
        tick.tick().await;
        if let Err(err) = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&app.db)
            .await
        {
            tracing::warn!("WAL checkpoint 出错: {err}");
        }
        if let Err(err) = db::admin::delete_expired_sessions(&app.db).await {
            tracing::warn!("清理过期会话出错: {err}");
        }
    }
}
