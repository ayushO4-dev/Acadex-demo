//! The transactional outbox (architecture.md §11.1).

use serde_json::Value;
use sqlx::PgConnection;

/// Queues an event inside the caller's transaction, so it exists if and only
/// if the business write it announces commits.
pub async fn enqueue(
    conn: &mut PgConnection,
    topic: &str,
    ordering_key: Option<&str>,
    payload: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO ops.outbox (topic, ordering_key, payload) VALUES ($1, $2, $3)")
        .bind(topic)
        .bind(ordering_key)
        .bind(payload)
        .execute(conn)
        .await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
pub struct Pending {
    pub id: i64,
    pub topic: String,
    pub payload: Value,
}

/// The oldest unsent events, locked for this transaction. SKIP LOCKED lets
/// several dispatchers (instances) run at once without sending one twice.
pub async fn lock_pending(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<Pending>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, topic, payload FROM ops.outbox
         WHERE published_at IS NULL ORDER BY id LIMIT $1
         FOR UPDATE SKIP LOCKED",
    )
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn mark_published(conn: &mut PgConnection, ids: &[i64]) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE ops.outbox SET published_at = now() WHERE id = ANY($1)")
        .bind(ids)
        .execute(conn)
        .await?;
    Ok(())
}

/// A batch of pending events locked for publishing. The caller publishes
/// them in order, stopping at the first failure, then calls [`finish`] with
/// the ids that went. Shared by the API's in-process dispatcher and the
/// worker's `outbox.sweep` job.
pub struct Batch {
    tx: sqlx::Transaction<'static, sqlx::Postgres>,
    pub events: Vec<Pending>,
}

pub async fn begin_batch(pool: &sqlx::PgPool, limit: i64) -> Result<Batch, sqlx::Error> {
    use crate::rls::{self, System};
    let mut tx = rls::begin_system(pool, System::Dispatcher).await?;
    let events = lock_pending(&mut tx, limit).await?;
    Ok(Batch { tx, events })
}

/// Marks what was sent and releases the rest for the next pass.
pub async fn finish(mut batch: Batch, sent: &[i64]) -> Result<(), sqlx::Error> {
    mark_published(&mut batch.tx, sent).await?;
    batch.tx.commit().await?;
    if !sent.is_empty() {
        tracing::info!(count = sent.len(), "outbox events published");
    }
    Ok(())
}
