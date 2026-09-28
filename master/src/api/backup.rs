//! 数据备份：下载时用 SQLite 的在线备份（VACUUM INTO），不停服务；上传的备份校验后放进数据目录，
//! 主控退出，由 systemd 或 Docker 重启时换上（panel.md「数据备份」）。
//! 备份文件等同于全部凭据（database.md「约定」），面板下载前会提示妥善保管。

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use sqlx::ConnectOptions;
use sqlx::sqlite::SqliteConnectOptions;

use super::auth::Admin;
use super::{ApiError, ApiResult};
use crate::app::AppState;
use crate::db;
use crate::secret;

/// 等待恢复的备份文件名：下次启动时换上
pub const PENDING_RESTORE: &str = "restore-pending.db";
/// 主控退出码：让 systemd（Restart=always 或 on-failure）和 Docker 都会重启
const RESTART_EXIT_CODE: i32 = 75;

pub async fn download(State(state): State<AppState>, _admin: Admin) -> ApiResult<Response> {
    let tmp = state
        .data_dir
        .join(format!(".backup-{}.db", secret::random_token(8)));
    let result = async {
        sqlx::query("VACUUM INTO ?")
            .bind(tmp.to_string_lossy().to_string())
            .execute(&state.db)
            .await?;
        Ok::<_, ApiError>(tokio::fs::read(&tmp).await.map_err(anyhow::Error::from)?)
    }
    .await;
    let _ = tokio::fs::remove_file(&tmp).await;
    let data = result?;
    let name = format!(
        "op-master-{}.db",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    tracing::info!(bytes = data.len(), "下载了数据库备份");
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        data,
    )
        .into_response())
}

/// 恢复：请求体是备份文件的原始字节（Content-Type: application/octet-stream）。
pub async fn restore(
    State(state): State<AppState>,
    _admin: Admin,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if !body.starts_with(b"SQLite format 3\0") {
        return Err(ApiError::bad_request(
            "invalid_backup",
            "这不是 SQLite 数据库文件",
        ));
    }
    let check_path = state.data_dir.join("restore-check.db");
    tokio::fs::write(&check_path, &body)
        .await
        .map_err(anyhow::Error::from)?;
    if let Err(err) = check_backup(&check_path).await {
        let _ = tokio::fs::remove_file(&check_path).await;
        return Err(ApiError::bad_request("invalid_backup", format!("{err:#}")));
    }
    tokio::fs::rename(&check_path, state.data_dir.join(PENDING_RESTORE))
        .await
        .map_err(anyhow::Error::from)?;
    tracing::warn!("收到备份文件，主控马上退出，重启后换上");
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        std::process::exit(RESTART_EXIT_CODE);
    });
    Ok(Json(json!({ "restarting": true })))
}

/// 确认是我们的数据库：有管理员，迁移版本不比这个主控新（否则启动时迁移会失败）。
async fn check_backup(path: &std::path::Path) -> anyhow::Result<()> {
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .connect()
        .await?;
    let admins: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM admin")
        .fetch_one(&mut conn)
        .await
        .map_err(|_| anyhow::anyhow!("文件里没有 op-master 的表，不是 op-master 的备份"))?;
    if admins.0 == 0 {
        anyhow::bail!("备份里没有管理员账号");
    }
    let (version,): (i64,) = sqlx::query_as("SELECT IFNULL(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(&mut conn)
        .await?;
    let latest = db::latest_migration();
    if version > latest {
        anyhow::bail!(
            "备份来自更新版本的主控（数据库版本 {version}，这个主控只到 {latest}），先升级主控"
        );
    }
    Ok(())
}

/// 启动时：有等待恢复的备份就换上，旧库改名留作备份。
pub fn apply_pending(data_dir: &std::path::Path) -> anyhow::Result<()> {
    let pending = data_dir.join(PENDING_RESTORE);
    if !pending.exists() {
        return Ok(());
    }
    let db_path = data_dir.join("op-master.db");
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    for suffix in ["", "-wal", "-shm"] {
        let from = data_dir.join(format!("op-master.db{suffix}"));
        if from.exists() {
            let to = data_dir.join(format!("op-master.db{suffix}.before-restore-{stamp}"));
            std::fs::rename(&from, &to)?;
        }
    }
    std::fs::rename(&pending, &db_path)?;
    tracing::warn!(backup = %format!("op-master.db.before-restore-{stamp}"), "已换上恢复的数据库，原来的库改名保留");
    Ok(())
}
