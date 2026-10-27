//! Publishes outbox events to Pub/Sub (architecture.md §11.1).
//!
//! Woken right after a commit that queued an event, and also sweeps on a
//! timer, so an event left behind by a crash or a Pub/Sub outage is sent on
//! the next pass. Delivery is at least once: if marking a row as sent fails
//! after publishing, it is published again, which is why consumers are
//! idempotent.

use std::{sync::Arc, time::Duration};

use acadex_cloud::PubSub;
use acadex_db::outbox;
use sqlx::PgPool;
use tokio::sync::Notify;

const BATCH: i64 = 50;
const SWEEP_EVERY: Duration = Duration::from_secs(30);

pub fn spawn(pool: PgPool, pubsub: PubSub, wakeup: Arc<Notify>) {
    tokio::spawn(async move {
        loop {
            match dispatch_once(&pool, &pubsub).await {
                // A full batch means there may be more: go again at once.
                Ok(sent) if sent as i64 == BATCH => continue,
                Ok(_) => {}
                Err(err) => tracing::warn!(error = %err, "outbox dispatch failed; will retry"),
            }
            tokio::select! {
                () = wakeup.notified() => {}
                () = tokio::time::sleep(SWEEP_EVERY) => {}
            }
        }
    });
}

/// Sends one batch of pending events; returns how many were sent.
#[tracing::instrument(name = "outbox.dispatch", skip_all)]
pub async fn dispatch_once(pool: &PgPool, pubsub: &PubSub) -> anyhow::Result<usize> {
    let batch = outbox::begin_batch(pool, BATCH).await?;
    let sent = publish_all(pubsub, &batch.events).await;
    let count = sent.len();
    outbox::finish(batch, &sent).await?;
    Ok(count)
}

/// Publishes in order until one fails (order holds; the rest go next time).
pub async fn publish_all(pubsub: &PubSub, events: &[outbox::Pending]) -> Vec<i64> {
    let mut sent = Vec::with_capacity(events.len());
    for event in events {
        match pubsub
            .publish_event(&event.topic, &event.payload, event.id)
            .await
        {
            Ok(_) => sent.push(event.id),
            Err(err) => {
                tracing::warn!(error = %err, outbox_id = event.id, topic = %event.topic, "publish failed");
                break;
            }
        }
    }
    sent
}
