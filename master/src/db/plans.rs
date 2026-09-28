//! plans、plan_nodes 表：套餐（流量额度 + 可用节点）。

use std::collections::HashMap;

use sqlx::SqlitePool;

use super::now_ms;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Plan {
    pub id: i64,
    pub name: String,
    pub traffic_quota_bytes: Option<i64>,
    pub created_at: i64,
}

pub async fn list(db: &SqlitePool) -> sqlx::Result<Vec<Plan>> {
    sqlx::query_as("SELECT id, name, traffic_quota_bytes, created_at FROM plans ORDER BY id")
        .fetch_all(db)
        .await
}

pub async fn get(db: &SqlitePool, id: i64) -> sqlx::Result<Option<Plan>> {
    sqlx::query_as("SELECT id, name, traffic_quota_bytes, created_at FROM plans WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// 每个套餐包含的节点 ID。
pub async fn node_ids(db: &SqlitePool) -> sqlx::Result<HashMap<i64, Vec<i64>>> {
    let rows: Vec<(i64, i64)> =
        sqlx::query_as("SELECT plan_id, node_id FROM plan_nodes ORDER BY plan_id, node_id")
            .fetch_all(db)
            .await?;
    let mut map: HashMap<i64, Vec<i64>> = HashMap::new();
    for (plan_id, node_id) in rows {
        map.entry(plan_id).or_default().push(node_id);
    }
    Ok(map)
}

/// 每个套餐绑定的用户数。
pub async fn user_counts(db: &SqlitePool) -> sqlx::Result<HashMap<i64, i64>> {
    let rows: Vec<(i64, i64)> =
        sqlx::query_as("SELECT plan_id, COUNT(*) FROM users GROUP BY plan_id")
            .fetch_all(db)
            .await?;
    Ok(rows.into_iter().collect())
}

pub async fn create(
    db: &SqlitePool,
    name: &str,
    quota: Option<i64>,
    node_ids: &[i64],
) -> sqlx::Result<i64> {
    let mut tx = db.begin().await?;
    let result =
        sqlx::query("INSERT INTO plans (name, traffic_quota_bytes, created_at) VALUES (?, ?, ?)")
            .bind(name)
            .bind(quota)
            .bind(now_ms())
            .execute(&mut *tx)
            .await?;
    let id = result.last_insert_rowid();
    for node_id in node_ids {
        sqlx::query("INSERT INTO plan_nodes (plan_id, node_id) VALUES (?, ?)")
            .bind(id)
            .bind(node_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(id)
}

/// 改名字、额度，并整体替换节点列表。
pub async fn update(
    db: &SqlitePool,
    id: i64,
    name: &str,
    quota: Option<i64>,
    node_ids: &[i64],
) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("UPDATE plans SET name = ?, traffic_quota_bytes = ? WHERE id = ?")
        .bind(name)
        .bind(quota)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM plan_nodes WHERE plan_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    for node_id in node_ids {
        sqlx::query("INSERT INTO plan_nodes (plan_id, node_id) VALUES (?, ?)")
            .bind(id)
            .bind(node_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

pub async fn delete(db: &SqlitePool, id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM plans WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}
