//! The worker end to end: a Pub/Sub-style push in, a PDF in file storage and
//! its object name in Postgres out, with the app role so RLS applies.
//!
//! Needs DATABASE_URL pointing at a superuser (`just test-backend` sets it).

use std::sync::Arc;

use acadex_cloud::Storage;
use acadex_worker::{WorkerState, receipt::Renderer, router};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tower::ServiceExt;
use uuid::Uuid;

/// The files the worker stored: a fresh folder per test.
#[derive(Clone)]
struct Uploads {
    dir: std::path::PathBuf,
}

impl Uploads {
    fn new() -> Self {
        Self {
            dir: std::env::temp_dir().join(format!("acadex-worker-test-{}", Uuid::new_v4())),
        }
    }

    fn storage(&self) -> Storage {
        Storage::new(&self.dir)
    }

    /// An object's bytes, from whichever bucket holds it.
    fn read(&self, name: &str) -> Vec<u8> {
        let file: String = name
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect();
        ["receipts-test", "uploads-test"]
            .iter()
            .find_map(|bucket| std::fs::read(self.dir.join(bucket).join(&file)).ok())
            .unwrap_or_else(|| panic!("{name} wasn't stored"))
    }

    fn is_empty(&self) -> bool {
        walk(&self.dir) == 0
    }

    fn clear(&self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn walk(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| {
                if e.path().is_dir() {
                    walk(&e.path())
                } else {
                    1
                }
            })
            .sum()
    })
}

/// A captured payment for student A with its receipt, as the webhook leaves it.
const RECEIPT_SQL: &str = "
    WITH a AS (
        INSERT INTO fees.payment_attempts (student_id, idempotency_key, amount_paise, gateway_order_id, status)
        VALUES ('a0000000-0000-0000-0000-00000000000a', 'test-key-0001', 240000, 'order_1', 'captured')
        RETURNING id),
    i AS (INSERT INTO fees.payment_attempt_items SELECT a.id, 1002, 240000 FROM a),
    p AS (
        INSERT INTO fees.payments (attempt_id, student_id, gateway_payment_id, amount_paise, method, captured_at)
        SELECT a.id, 'a0000000-0000-0000-0000-00000000000a', 'pay_1', 240000, 'upi', now() FROM a
        RETURNING id),
    l AS (
        INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, payment_id)
        SELECT 'a0000000-0000-0000-0000-00000000000a', 2026, 'payment', 1002, 240000, p.id FROM p)
    INSERT INTO fees.receipts (receipt_no, financial_year, seq, payment_id, student_id, amount_paise, remarks)
    SELECT 'RCPT/2026-27/000001', 2026, 1, p.id, 'a0000000-0000-0000-0000-00000000000a', 240000,
           'Examination fee — Semester 5'
    FROM p RETURNING id";

async fn setup(opts: PgPoolOptions, conn: PgConnectOptions) -> (Router, PgPool, Uploads, Uuid) {
    let admin = PgPool::connect_with(conn.clone()).await.unwrap();
    sqlx::raw_sql(include_str!("../../acadex-db/fixtures/test_data.sql"))
        .execute(&admin)
        .await
        .unwrap();
    let receipt: Uuid = sqlx::query_scalar(RECEIPT_SQL)
        .fetch_one(&admin)
        .await
        .unwrap();

    let pool = opts
        .after_connect(|conn, _| {
            Box::pin(async move { conn.execute("SET ROLE acadex_app").await.map(|_| ()) })
        })
        .connect_with(conn)
        .await
        .unwrap();
    let uploads = Uploads::new();
    let state = WorkerState {
        pool,
        storage: uploads.storage(),
        receipts_bucket: "receipts-test".into(),
        uploads_bucket: "uploads-test".into(),
        renderer: Arc::new(Renderer::default()),
        documents: Arc::new(acadex_worker::documents::Documents::default()),
        public_url: "http://acadex.test".into(),
        mailer: None,
        gateway: None,
        pubsub: None,
    };
    (router(state), admin, uploads, receipt)
}

async fn push(app: &Router, event: &str, payload: serde_json::Value) -> StatusCode {
    let envelope = serde_json::json!({
        "message": {
            "data": BASE64.encode(payload.to_string()),
            "attributes": { "event": event },
            "messageId": "1",
        },
        "subscription": "projects/test/subscriptions/worker",
    });
    let request = Request::post("/pubsub/push")
        .header("content-type", "application/json")
        .body(Body::from(envelope.to_string()))
        .unwrap();
    app.clone().oneshot(request).await.unwrap().status()
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn a_receipt_event_produces_its_pdf_once(opts: PgPoolOptions, conn: PgConnectOptions) {
    let (app, admin, uploads, receipt) = setup(opts, conn).await;

    let status = push(
        &app,
        "receipt.requested",
        serde_json::json!({ "receipt_id": receipt }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let object: Option<String> =
        sqlx::query_scalar("SELECT pdf_object FROM fees.receipts WHERE id = $1")
            .bind(receipt)
            .fetch_one(&admin)
            .await
            .unwrap();
    assert_eq!(
        object.as_deref(),
        Some("receipts/2026-27/RCPT-2026-27-000001.pdf")
    );
    let pdf = uploads.read("receipts/2026-27/RCPT-2026-27-000001.pdf");
    assert!(pdf.starts_with(b"%PDF-"));

    // Redelivery: acknowledged, nothing written again.
    uploads.clear();
    let status = push(
        &app,
        "receipt.requested",
        serde_json::json!({ "receipt_id": receipt }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(uploads.is_empty());
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn impossible_events_are_acknowledged_and_unknown_ones_retried(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let (app, _, uploads, _) = setup(opts, conn).await;
    let missing = serde_json::json!({ "receipt_id": Uuid::nil() });
    assert_eq!(
        push(&app, "receipt.requested", missing).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        push(&app, "something.else", serde_json::json!({})).await,
        StatusCode::SERVICE_UNAVAILABLE,
        "an unknown event type is left for a newer worker, not dropped"
    );
    assert!(uploads.is_empty());
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn published_results_get_grade_cards_once(opts: PgPoolOptions, conn: PgConnectOptions) {
    let (app, admin, uploads, _) = setup(opts, conn).await;
    let semester_3 =
        serde_json::json!({ "publication_id": "7e000000-0000-0000-0000-000000000003" });
    assert_eq!(
        push(&app, "results.published", semester_3.clone()).await,
        StatusCode::NO_CONTENT
    );
    let cards: Vec<(String, String)> = sqlx::query_as(
        "SELECT st.roll_no, gc.pdf_object FROM exams.grade_cards gc
         JOIN people.students st ON st.user_id = gc.student_id ORDER BY 1",
    )
    .fetch_all(&admin)
    .await
    .unwrap();
    assert_eq!(cards.len(), 2, "A and B: {cards:?}");
    assert!(uploads.read(&cards[0].1).starts_with(b"%PDF-"));

    // Again: both cards already reflect it.
    uploads.clear();
    push(&app, "results.published", semester_3).await;
    assert!(uploads.is_empty());

    // A staged set makes nothing: its marks aren't public yet.
    let staged = serde_json::json!({ "publication_id": "7e000000-0000-0000-0000-000000000004" });
    assert_eq!(
        push(&app, "results.published", staged).await,
        StatusCode::NO_CONTENT
    );
    assert!(uploads.is_empty());
}

/// The gateway's payment list, as reconciliation reads it: one payment whose
/// webhook never arrived, and one for an order that isn't ours.
async fn fake_gateway() -> String {
    use axum::routing;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let created = chrono::Utc::now().timestamp() - 600;
    let fake = axum::Router::new().route(
        "/v1/payments",
        routing::get(move || async move {
            axum::Json(serde_json::json!({ "entity": "collection", "items": [
                { "id": "pay_lost", "order_id": "order_lost", "amount": 240000, "currency": "INR",
                  "status": "captured", "method": "upi", "created_at": created },
                { "id": "pay_stray", "order_id": "order_nobody", "amount": 5000, "currency": "INR",
                  "status": "captured", "method": "card", "created_at": created },
            ]}))
        }),
    );
    tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });
    url
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn reconciliation_recovers_a_lost_webhook_and_flags_the_rest(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let admin = PgPool::connect_with(conn.clone()).await.unwrap();
    sqlx::raw_sql(include_str!("../../acadex-db/fixtures/test_data.sql"))
        .execute(&admin)
        .await
        .unwrap();
    // Student A paid the exam fee; the gateway's webhook was lost.
    sqlx::raw_sql(
        "INSERT INTO fees.payment_attempts (id, student_id, idempotency_key, amount_paise, gateway_order_id)
         VALUES ('99999999-0000-0000-0000-000000000001', 'a0000000-0000-0000-0000-00000000000a', 'lost-0001', 240000, 'order_lost');
         INSERT INTO fees.payment_attempt_items VALUES ('99999999-0000-0000-0000-000000000001', 1002, 240000);",
    )
    .execute(&admin)
    .await
    .unwrap();
    let pool = opts
        .after_connect(|conn, _| {
            Box::pin(async move { conn.execute("SET ROLE acadex_app").await.map(|_| ()) })
        })
        .connect_with(conn)
        .await
        .unwrap();
    let state = WorkerState {
        pool,
        storage: Uploads::new().storage(),
        receipts_bucket: "receipts-test".into(),
        uploads_bucket: "uploads-test".into(),
        renderer: Arc::new(Renderer::default()),
        documents: Arc::new(acadex_worker::documents::Documents::default()),
        public_url: "http://acadex.test".into(),
        mailer: None,
        gateway: Some(acadex_cloud::gateway::Gateway::new(
            &fake_gateway().await,
            "test_key",
        )),
        pubsub: None,
    };

    let done = acadex_worker::reconcile_payments(&state).await.unwrap();
    assert_eq!(
        done,
        acadex_worker::Done::Generated(
            "2 gateway payments, 0 matched, 1 recovered, 1 issues".to_owned()
        )
    );
    let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM fees.receipts")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(receipts, 1, "settled as the webhook would have");
    let issues: Vec<(String, String)> =
        sqlx::query_as("SELECT kind, status FROM fees.reconciliation_issues ORDER BY kind")
            .fetch_all(&admin)
            .await
            .unwrap();
    assert_eq!(
        issues,
        [
            ("missed_webhook".to_owned(), "resolved".to_owned()),
            ("unknown_payment".to_owned(), "open".to_owned()),
        ]
    );

    // Again: the recovered payment now matches, and nothing is reported twice.
    let done = acadex_worker::reconcile_payments(&state).await.unwrap();
    assert_eq!(
        done,
        acadex_worker::Done::Generated(
            "2 gateway payments, 1 matched, 0 recovered, 0 issues".to_owned()
        )
    );
}

/// A worker state for calling jobs directly, and an admin pool to look.
async fn job_state(opts: PgPoolOptions, conn: PgConnectOptions) -> (WorkerState, PgPool) {
    let admin = PgPool::connect_with(conn.clone()).await.unwrap();
    sqlx::raw_sql(include_str!("../../acadex-db/fixtures/test_data.sql"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = opts
        .after_connect(|conn, _| {
            Box::pin(async move { conn.execute("SET ROLE acadex_app").await.map(|_| ()) })
        })
        .connect_with(conn)
        .await
        .unwrap();
    let state = WorkerState {
        pool,
        storage: Uploads::new().storage(),
        receipts_bucket: "receipts-test".into(),
        uploads_bucket: "uploads-test".into(),
        renderer: Arc::new(Renderer::default()),
        documents: Arc::new(acadex_worker::documents::Documents::default()),
        public_url: "http://acadex.test".into(),
        mailer: None,
        gateway: None,
        pubsub: None,
    };
    (state, admin)
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn a_message_that_keeps_failing_is_dead_lettered(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let (app, admin, _, _) = setup(opts, conn).await;
    let deliver = || {
        let envelope = serde_json::json!({
            "message": {
                "data": BASE64.encode(r#"{"x":1}"#),
                "attributes": { "event": "no.such.event" },
                "messageId": "poison-1",
            },
            "subscription": "projects/test/subscriptions/worker",
        });
        Request::post("/pubsub/push")
            .header("content-type", "application/json")
            .body(Body::from(envelope.to_string()))
            .unwrap()
    };
    for attempt in 1..5 {
        let status = app.clone().oneshot(deliver()).await.unwrap().status();
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "attempt {attempt} is retried"
        );
    }
    let status = app.clone().oneshot(deliver()).await.unwrap().status();
    assert_eq!(status, StatusCode::NO_CONTENT, "the fifth failure parks it");
    let parked: (String, i32, serde_json::Value) = sqlx::query_as(
        "SELECT event, attempts, payload FROM ops.dead_letters WHERE message_id = 'poison-1'",
    )
    .fetch_one(&admin)
    .await
    .unwrap();
    assert_eq!(parked.0, "no.such.event");
    assert_eq!(parked.1, 5);
    assert_eq!(parked.2["x"], 1);
    let counting: i64 = sqlx::query_scalar("SELECT count(*) FROM ops.delivery_failures")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(counting, 0);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn the_audit_chain_check_notices_a_removed_newest_row(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    use acadex_db::{audit, rls};
    let (state, admin) = job_state(opts, conn).await;
    for action in ["test.one", "test.two", "test.three"] {
        let mut tx = rls::begin_system(&state.pool, rls::System::Worker)
            .await
            .unwrap();
        audit::record(
            &mut tx,
            &audit::Event {
                action,
                resource_type: "test",
                resource_id: None,
                outcome: audit::Outcome::Success,
                ip: None,
                user_agent: None,
                trace_id: None,
                metadata: serde_json::json!({}),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let done = acadex_worker::verify_audit_chain(&state).await.unwrap();
    assert_eq!(
        done,
        acadex_worker::Done::Generated("3 rows verified".to_owned())
    );

    // Remove the newest row: the walk alone still passes, the checkpoint doesn't.
    sqlx::query("DELETE FROM audit.action_log WHERE id = (SELECT max(id) FROM audit.action_log)")
        .execute(&admin)
        .await
        .unwrap();
    let done = acadex_worker::verify_audit_chain(&state).await.unwrap();
    let acadex_worker::Done::Generated(text) = done else {
        panic!("{done:?}")
    };
    assert!(text.starts_with("chain broken at row"), "{text}");
    let alerts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit.action_log WHERE action = 'audit.chain_broken'",
    )
    .fetch_one(&admin)
    .await
    .unwrap();
    assert_eq!(alerts, 1);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn partitions_are_made_ahead_and_the_rollup_matches_live_counts(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let (state, admin) = job_state(opts, conn).await;
    let acadex_worker::Done::Generated(created) =
        acadex_worker::maintain_partitions(&state).await.unwrap()
    else {
        panic!("expected next year's partitions");
    };
    assert!(created.contains("attendance_btech_ece_"), "{created}");
    assert_eq!(
        acadex_worker::maintain_partitions(&state).await.unwrap(),
        acadex_worker::Done::AlreadyDone
    );

    acadex_worker::rollup_attendance(&state, None)
        .await
        .unwrap();
    let mismatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM academic.attendance_rollup r
         JOIN (SELECT student_id, offering_id, session_kind, count(*)::int AS conducted,
                      (count(*) FILTER (WHERE status IN ('present', 'late')))::int AS attended
               FROM academic.attendance GROUP BY 1, 2, 3) l USING (student_id, offering_id, session_kind)
         WHERE (r.conducted, r.attended) IS DISTINCT FROM (l.conducted, l.attended)",
    )
    .fetch_one(&admin)
    .await
    .unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM academic.attendance_rollup")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert!(rows > 0);
    assert_eq!(mismatches, 0);
}
