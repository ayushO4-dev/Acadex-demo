//! The API end to end: real token verification (test signing key), real Postgres
//! connected as the app role, so RBAC and row-level security both apply.
//!
//! Needs DATABASE_URL pointing at a superuser (`just test-backend` sets it).

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use acadex_api::{
    AppState, Files,
    payments::{self, Gateway, Payments},
    router,
};
use acadex_auth::testing;
use acadex_cloud::{PubSub, Storage};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use serde_json::Value;
use sqlx::{
    Executor, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tower::ServiceExt;

const STUDENT_A: &str = "a0000000-0000-0000-0000-00000000000a";
const STUDENT_B: &str = "b0000000-0000-0000-0000-00000000000b";
const STUDENT_C: &str = "c0000000-0000-0000-0000-00000000000c";

const WEBHOOK_SECRET: &str = "test-webhook-secret";
const FORWARDING_KEY: &str = "test-forwarding-key";

struct TestApp {
    router: Router,
    /// The app role's pool, as the API uses it.
    pool: PgPool,
    admin: PgPool,
    cloud: FakeCloud,
}

/// What the fake services have seen, and hold.
#[derive(Clone, Default)]
struct FakeCloud {
    url: String,
    /// Orders the gateway has created.
    orders: Arc<AtomicUsize>,
    /// Refunds it has made.
    refunds: Arc<AtomicUsize>,
    /// (topic, message) delivered to the worker's push endpoint.
    published: Arc<Mutex<Vec<(String, Value)>>>,
    /// Where stored files go: a fresh folder per test.
    files: std::path::PathBuf,
}

/// The payment gateway's order API and the worker's push endpoint, on a
/// random local port, so tests exercise the real HTTP clients. Order ids
/// count up: order_test_1, _2, …
async fn fake_cloud() -> FakeCloud {
    use axum::routing;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cloud = FakeCloud {
        url: format!("http://{}", listener.local_addr().unwrap()),
        files: std::env::temp_dir().join(format!("acadex-test-{}", uuid::Uuid::new_v4())),
        ..FakeCloud::default()
    };
    let refunds = cloud.refunds.clone();
    let (orders, published) = (cloud.orders.clone(), cloud.published.clone());
    let fake = Router::new()
        .route(
            "/v1/orders",
            routing::post(move || {
                let n = orders.fetch_add(1, Ordering::SeqCst) + 1;
                async move { axum::Json(serde_json::json!({ "id": format!("order_test_{n}") })) }
            }),
        )
        .route(
            "/v1/payments/{id}/refund",
            routing::post(move || {
                let n = refunds.fetch_add(1, Ordering::SeqCst) + 1;
                async move {
                    axum::Json(serde_json::json!({ "id": format!("rfnd_test_{n}"), "status": "processed" }))
                }
            }),
        )
        .route(
            // As the worker's push endpoint receives a Pub/Sub push.
            "/push",
            routing::post(move |axum::Json(body): axum::Json<Value>| {
                let topic = body["message"]["attributes"]["event"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                published.lock().unwrap().push((topic, body["message"].clone()));
                async { StatusCode::NO_CONTENT }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });
    cloud
}

async fn app(pool_options: PgPoolOptions, connect_options: PgConnectOptions) -> TestApp {
    app_limited(pool_options, connect_options, None).await
}

async fn app_limited(
    pool_options: PgPoolOptions,
    connect_options: PgConnectOptions,
    rate: Option<acadex_api::limits::PerMinute>,
) -> TestApp {
    let admin = PgPool::connect_with(connect_options.clone()).await.unwrap();
    sqlx::raw_sql(include_str!("../../acadex-db/fixtures/test_data.sql"))
        .execute(&admin)
        .await
        .unwrap();

    // Connect the way production does: as the app role, where RLS applies.
    let pool = pool_options
        .after_connect(|conn, _| {
            Box::pin(async move {
                conn.execute("SET ROLE acadex_app").await?;
                Ok(())
            })
        })
        .connect_with(connect_options)
        .await
        .unwrap();

    let cloud = fake_cloud().await;
    let state = AppState {
        pool: pool.clone(),
        verifier: Arc::new(testing::verifier()),
        link_unverified_emails: false,
        payments: Payments {
            gateway: Gateway::new(&cloud.url, "test_key"),
            webhook_secret: WEBHOOK_SECRET.into(),
            checkout_url: "http://checkout.test/pay".into(),
        },
        files: Files {
            storage: Storage::new(&cloud.files),
            receipts_bucket: "receipts-test".into(),
            uploads_bucket: "uploads-test".into(),
        },
        outbox_wakeup: Arc::default(),
        notices_live: tokio::sync::broadcast::channel(16).0,
        cache: acadex_api::cache::Cache::in_process(),
        // Rate limits are tested on their own; these tests make many requests.
        limits: Arc::new(acadex_api::limits::Limits::new(rate, None)),
        // As on Cloud Run: one proxy in front, and the web app's key.
        forwarding: Arc::new(acadex_api::client_addr::Forwarding::new(
            1,
            Some(FORWARDING_KEY),
        )),
        app_base_url: "http://localhost:3000".into(),
    };
    TestApp {
        router: router(state),
        pool,
        admin,
        cloud,
    }
}

impl TestApp {
    async fn call(&self, method: &str, path: &str, subject: Option<&str>) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            // The web app calling for a browser at 203.0.113.7.
            .header("x-forwarded-for", "203.0.113.7, 10.0.0.1")
            .header(acadex_api::client_addr::CLIENT_IP_HEADER, "203.0.113.7")
            .header(
                acadex_api::client_addr::FORWARDING_KEY_HEADER,
                FORWARDING_KEY,
            )
            .header(header::USER_AGENT, "integration-test");
        if let Some(sub) = subject {
            request = request.header(
                header::AUTHORIZATION,
                format!("Bearer {}", testing::token(sub)),
            );
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn health_and_ready_need_no_token(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    assert_eq!(app.call("GET", "/health", None).await.0, StatusCode::OK);
    let (status, body) = app.call("GET", "/ready", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["database"], "ok");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn requests_without_a_valid_token_are_rejected(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app.call("GET", "/me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["type"], "https://acadex.edu/errors/unauthenticated");

    let forged = app
        .router
        .clone()
        .oneshot(
            Request::get("/me")
                .header(header::AUTHORIZATION, "Bearer not.a.token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn a_login_without_an_acadex_account_is_forbidden(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let (status, body) = app.call("GET", "/me", Some("auth0|stranger")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["type"], "https://acadex.edu/errors/no-account");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn me_returns_profile_and_permissions(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app.call("GET", "/me", Some("auth0|a")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], STUDENT_A);
    assert_eq!(body["role"], "student");
    assert_eq!(body["student"]["roll_no"], "ECE24001");
    assert_eq!(
        body["permissions"],
        // Whatever the matrix grants (rbac.rs pins the matrix itself).
        serde_json::json!(
            acadex_auth::rbac::permissions(acadex_auth::Role::Student)
                .map(acadex_auth::Permission::as_str)
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(
        body["email"], "a@acadex.test",
        "their own email, and only theirs"
    );
}

/// The Phase 1 exit criterion, over HTTP: A asks for B's attendance by id.
#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn student_cannot_fetch_another_students_attendance(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;

    let (status, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_B}/attendance?academic_year=2026"),
            Some("auth0|a"),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/attendance?academic_year=2026"),
            Some("auth0|a"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["subjects"].as_array().unwrap().len(), 2);
    assert_eq!(body["overall"]["percentage"], 100.0);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn subject_entries_have_no_duplicate_keys(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let raw = app
        .router
        .clone()
        .oneshot(
            Request::get(format!(
                "/students/{STUDENT_A}/attendance?academic_year=2026"
            ))
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", testing::token("auth0|a")),
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(raw.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    // The overall totals, and for each of the two subjects its totals and its
    // theory figures (no practicals here): five, no more.
    assert_eq!(text.matches("\"conducted\"").count(), 5, "{text}");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn reports_flag_shortage(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_B}/attendance?academic_year=2026"),
            Some("auth0|b"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["overall"]["percentage"], 50.0);
    assert_eq!(body["overall"]["shortage"], true);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn faculty_see_only_their_own_subjects(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_B}/attendance?academic_year=2026"),
            Some("auth0|f"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let codes: Vec<_> = body["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["code"].clone())
        .collect();
    assert_eq!(codes, ["EC501"]);

    // A CSE student is outside F's programs.
    let (status, _) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_C}/attendance"),
            Some("auth0|f"),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn exam_cell_reads_any_student(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_C}/attendance?academic_year=2026"),
            Some("auth0|e"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["student"]["program_name"], "B.Tech CSE");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn sign_in_is_audited_with_client_details(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app.call("POST", "/sessions", Some("auth0|a")).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["display_name"], "Student A");

    let (actor, action, ip, agent): (String, String, String, String) = sqlx::query_as(
        "SELECT actor_id::text, action, host(ip), user_agent FROM audit.action_log ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&app.admin)
    .await
    .unwrap();
    assert_eq!(
        (actor.as_str(), action.as_str(), ip.as_str(), agent.as_str()),
        (STUDENT_A, "auth.login", "203.0.113.7", "integration-test")
    );
}

// ── Attendance marking ─────────────────────────────────────────────────────

const EC501: &str = "0f000000-0000-0000-0000-000000000501";
const EC502: &str = "0f000000-0000-0000-0000-000000000502";
const STUDENT_D: &str = "d0000000-0000-0000-0000-00000000000d";

impl TestApp {
    async fn put_json(&self, path: &str, subject: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::put(path)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", testing::token(subject)),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::USER_AGENT, "integration-test")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

fn marks(entries: &[(&str, &str)]) -> Value {
    let marks: Vec<Value> = entries
        .iter()
        .map(|(student, status)| serde_json::json!({ "student_id": student, "status": status }))
        .collect();
    serde_json::json!({ "session_date": "2026-09-28", "slot": 1, "marks": marks })
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn faculty_list_only_their_offerings(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app
        .call("GET", "/offerings?academic_year=2026", Some("auth0|f"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let codes: Vec<_> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["code"].clone())
        .collect();
    assert_eq!(codes, ["EC501"]);
    assert_eq!(body[0]["student_count"], 3, "A, B and D are ECE semester 5");

    let (status, _) = app.call("GET", "/offerings", Some("auth0|a")).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "students can't mark attendance"
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn attendance_follows_the_calendar_and_settles(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let path = format!("/offerings/{EC501}/attendance");
    let on = |date: &str| serde_json::json!({ "session_date": date, "slot": 1, "marks": [{ "student_id": STUDENT_A, "status": "present" }] });

    // A holiday for ECE on 28 September: no classes.
    let (status, event) = app
        .post_json(
            "/academic/calendar",
            "auth0|e",
            serde_json::json!({ "kind": "holiday", "title": "Ganesh Visarjan", "starts_on": "2026-09-28",
                                "ends_on": "2026-09-28", "program_id": 11 }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{event}");
    let (status, body) = app.put_json(&path, "auth0|f", on("2026-09-28")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["detail"].as_str().unwrap().contains("Ganesh Visarjan"),
        "{body}"
    );
    // Moved to the 29th: the 28th is a working day again.
    let (status, _) = app
        .put_json(
            &format!("/academic/calendar/{}", event["id"].as_str().unwrap()),
            "auth0|e",
            serde_json::json!({ "kind": "holiday", "title": "Ganesh Visarjan", "starts_on": "2026-09-29",
                                "ends_on": "2026-09-29", "program_id": 11 }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app.put_json(&path, "auth0|f", on("2026-09-28")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = app.put_json(&path, "auth0|f", on("2026-09-29")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn old_attendance_is_locked(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    sqlx::query("UPDATE org.institution_settings SET attendance_lock_days = 7")
        .execute(&app.admin)
        .await
        .unwrap();
    let path = format!("/offerings/{EC501}/attendance");
    let on = |date: chrono::NaiveDate| serde_json::json!({ "session_date": date, "slot": 1, "marks": [{ "student_id": STUDENT_A, "status": "present" }] });
    let today = chrono::Utc::now().date_naive();
    let (status, body) = app
        .put_json(&path, "auth0|f", on(today - chrono::Duration::days(8)))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["type"].as_str().unwrap().rsplit('/').next(),
        Some("attendance-locked")
    );
    // Within the week: fine, on any day the calendar doesn't rule out
    // (weekends and holidays such as the Diwali break answer "no classes").
    let mut marked = false;
    for days in (1..=6).rev() {
        let (status, body) = app
            .put_json(&path, "auth0|f", on(today - chrono::Duration::days(days)))
            .await;
        if status == StatusCode::OK {
            marked = true;
            break;
        }
        assert_eq!(
            body["type"].as_str().unwrap().rsplit('/').next(),
            Some("no-classes"),
            "{body}"
        );
    }
    assert!(marked, "no teaching day in the last six");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn marking_saves_overwrites_and_audits(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let path = format!("/offerings/{EC501}/attendance");

    let (status, body) = app
        .put_json(
            &path,
            "auth0|f",
            marks(&[(STUDENT_A, "present"), (STUDENT_B, "absent")]),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["saved"], 2);

    // A correction overwrites rather than duplicating.
    let (status, _) = app
        .put_json(&path, "auth0|f", marks(&[(STUDENT_B, "late")]))
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, roster) = app
        .call(
            "GET",
            &format!("/offerings/{EC501}/roster?date=2026-09-28&slot=1"),
            Some("auth0|f"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let statuses: Vec<_> = roster["students"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["roll_no"].as_str().unwrap().to_owned(),
                s["status"].clone(),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        [
            ("ECE24001".to_owned(), Value::from("present")),
            ("ECE24002".to_owned(), Value::from("late")),
            ("ECE24003".to_owned(), Value::Null),
        ]
    );

    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit.action_log WHERE action = 'attendance.marked'",
    )
    .fetch_one(&app.admin)
    .await
    .unwrap();
    assert_eq!(audited, 2);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn faculty_cannot_mark_someone_elses_offering(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, _) = app
        .put_json(
            &format!("/offerings/{EC502}/attendance"),
            "auth0|f",
            marks(&[(STUDENT_A, "present")]),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn off_roster_students_reject_the_whole_request(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app
        .put_json(
            &format!("/offerings/{EC501}/attendance"),
            "auth0|f",
            marks(&[(STUDENT_D, "present"), (STUDENT_C, "present")]),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["type"], "https://acadex.edu/errors/not-on-roster");

    let saved: i64 =
        sqlx::query_scalar("SELECT count(*) FROM academic.attendance WHERE student_id = $1::uuid")
            .bind(STUDENT_D)
            .fetch_one(&app.admin)
            .await
            .unwrap();
    assert_eq!(saved, 0, "nothing from the rejected request was kept");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn marking_rejects_future_sessions_and_bad_periods(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let path = format!("/offerings/{EC501}/attendance");

    let mut future = marks(&[(STUDENT_A, "present")]);
    future["session_date"] = "2099-01-01".into();
    let (status, body) = app.put_json(&path, "auth0|f", future).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["type"], "https://acadex.edu/errors/future-session");

    let mut bad_slot = marks(&[(STUDENT_A, "present")]);
    bad_slot["slot"] = 9.into();
    let (status, _) = app.put_json(&path, "auth0|f", bad_slot).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = app
        .put_json(&path, "auth0|a", marks(&[(STUDENT_A, "present")]))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "students can't mark attendance"
    );
}

// ── Logs ─────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn logs_are_for_administration_alone(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    app.call("POST", "/sessions", Some("auth0|a")).await;
    app.call("POST", "/sessions", Some("auth0|f")).await;
    app.put_json(
        &format!("/offerings/{EC501}/attendance"),
        "auth0|f",
        marks(&[(STUDENT_A, "present")]),
    )
    .await;

    // Nobody else reads the log, not even their own entries.
    for subject in [
        "auth0|a", "auth0|f", "auth0|e", "auth0|k", "auth0|p", "auth0|h",
    ] {
        let (status, _) = app.call("GET", "/audit/logs", Some(subject)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{subject}");
        let (status, _) = app.call("GET", "/audit/logs/export", Some(subject)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{subject}");
    }

    let (status, body) = app.call("GET", "/audit/logs", Some("auth0|m")).await;
    assert_eq!(status, StatusCode::OK);
    let actions: Vec<_> = body["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].clone())
        .collect();
    assert_eq!(
        actions,
        ["attendance.marked", "auth.login", "auth.login"],
        "newest first, everyone's"
    );
    assert_eq!(body["entries"][0]["metadata"]["subject"], "EC501");
    assert_eq!(body["entries"][0]["actor_name"], "Faculty F");
    assert_eq!(body["entries"][2]["ip"], "203.0.113.7");
    assert!(body["next_before"].is_null());

    // Keyset paging: everything older than the newest entry.
    let newest = body["entries"][0]["id"].as_i64().unwrap();
    let (_, older) = app
        .call(
            "GET",
            &format!("/audit/logs?before={newest}"),
            Some("auth0|m"),
        )
        .await;
    assert_eq!(older["entries"].as_array().unwrap().len(), 2);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn logs_filter_export_and_record_sign_out(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    app.call("POST", "/sessions", Some("auth0|a")).await;
    app.call("POST", "/sessions", Some("auth0|f")).await;
    app.put_json(
        &format!("/offerings/{EC501}/attendance"),
        "auth0|f",
        marks(&[(STUDENT_A, "present")]),
    )
    .await;
    let (status, _) = app.call("DELETE", "/sessions", Some("auth0|a")).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let actions = |body: &Value| -> Vec<String> {
        body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["action"].as_str().unwrap().to_owned())
            .collect()
    };
    let (_, body) = app
        .call("GET", "/audit/logs?who=Student%20A", Some("auth0|m"))
        .await;
    assert_eq!(actions(&body), ["auth.logout", "auth.login"]);

    // Category and outcome, role and name.
    let (_, body) = app
        .call("GET", "/audit/logs?category=auth", Some("auth0|m"))
        .await;
    assert_eq!(actions(&body), ["auth.logout", "auth.login", "auth.login"]);
    let (_, body) = app
        .call(
            "GET",
            "/audit/logs?category=academic&role=faculty",
            Some("auth0|m"),
        )
        .await;
    assert_eq!(actions(&body), ["attendance.marked"]);
    let (_, body) = app
        .call("GET", "/audit/logs?who=faculty", Some("auth0|m"))
        .await;
    assert_eq!(actions(&body), ["attendance.marked", "auth.login"]);
    let (_, body) = app
        .call("GET", "/audit/logs?outcome=denied", Some("auth0|m"))
        .await;
    assert!(actions(&body).is_empty());

    // Dates are Indian days, inclusive; a range ending yesterday is empty.
    let today = chrono::Utc::now()
        .with_timezone(&chrono_tz::Asia::Kolkata)
        .date_naive();
    let (_, body) = app
        .call(
            "GET",
            &format!("/audit/logs?from={today}&to={today}"),
            Some("auth0|m"),
        )
        .await;
    assert_eq!(actions(&body).len(), 4);
    let yesterday = today.pred_opt().unwrap();
    let (_, body) = app
        .call(
            "GET",
            &format!("/audit/logs?to={yesterday}"),
            Some("auth0|m"),
        )
        .await;
    assert!(actions(&body).is_empty());
    let (status, _) = app
        .call(
            "GET",
            "/audit/logs?from=2026-10-05&to=2026-10-01",
            Some("auth0|m"),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = app
        .call("GET", "/audit/logs?category=nope", Some("auth0|m"))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // CSV export, which is itself logged.
    let request = Request::get("/audit/logs/export?category=auth&who=Student%20A")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", testing::token("auth0|m")),
        )
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/csv")
    );
    let csv = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    let lines: Vec<_> = csv.lines().collect();
    assert!(lines[0].starts_with("time_ist,actor,"));
    assert_eq!(
        lines.len(),
        3,
        "header and Student A's two sign-in events: {csv}"
    );
    assert!(lines[1].contains(",auth.logout,"));
    let (_, body) = app.call("GET", "/audit/logs", Some("auth0|m")).await;
    assert_eq!(actions(&body)[0], "audit.exported");
    assert_eq!(body["entries"][0]["metadata"]["rows"], 2);
}

// ── Operations ───────────────────────────────────────────────────────────────

// ── Rate limits ──────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn callers_are_rate_limited_per_kind(opts: PgPoolOptions, conn: PgConnectOptions) {
    let rate = acadex_api::limits::PerMinute {
        read: 3,
        mutation: 20,
        auth: 2,
    };
    let app = app_limited(opts, conn, Some(rate)).await;
    // Sign-ins: two a minute.
    for _ in 0..2 {
        let (status, _) = app.call("POST", "/sessions", Some("auth0|a")).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let request = Request::post("/sessions")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", testing::token("auth0|a")),
        )
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key(header::RETRY_AFTER));

    // Reads have their own bucket, and so does every other caller.
    for _ in 0..3 {
        assert_eq!(
            app.call("GET", "/me", Some("auth0|a")).await.0,
            StatusCode::OK
        );
    }
    assert_eq!(
        app.call("GET", "/me", Some("auth0|a")).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        app.call("GET", "/me", Some("auth0|b")).await.0,
        StatusCode::OK
    );
    // Health checks are never limited.
    for _ in 0..5 {
        assert_eq!(app.call("GET", "/health", None).await.0, StatusCode::OK);
    }
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn anonymous_callers_cannot_forge_their_address(opts: PgPoolOptions, conn: PgConnectOptions) {
    let rate = acadex_api::limits::PerMinute {
        read: 3,
        mutation: 20,
        auth: 2,
    };
    let app = app_limited(opts, conn, Some(rate)).await;
    let get = |headers: &[(&str, &str)]| {
        let mut request = Request::get("/institution");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let router = app.router.clone();
        let request = request.body(Body::empty()).unwrap();
        async move { router.oneshot(request).await.unwrap().status() }
    };
    // A caller at 198.51.100.9 inventing a new first entry each time still
    // spends one bucket: the address Cloud Run appended is the last.
    for forged in ["1.1.1.1", "2.2.2.2", "3.3.3.3"] {
        let chain = format!("{forged}, 198.51.100.9");
        assert_eq!(get(&[("x-forwarded-for", &chain)]).await, StatusCode::OK);
    }
    assert_eq!(
        get(&[("x-forwarded-for", "4.4.4.4, 198.51.100.9")]).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Claiming to be the web app takes its key.
    assert_eq!(
        get(&[
            ("x-forwarded-for", "198.51.100.9"),
            (acadex_api::client_addr::CLIENT_IP_HEADER, "5.5.5.5"),
            (acadex_api::client_addr::FORWARDING_KEY_HEADER, "a guess"),
        ])
        .await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // The web app's calls for two browsers come from one address, yet each
    // browser has its own bucket.
    for browser in ["203.0.113.20", "203.0.113.21"] {
        assert_eq!(
            get(&[
                ("x-forwarded-for", "10.8.0.1"),
                (acadex_api::client_addr::CLIENT_IP_HEADER, browser),
                (
                    acadex_api::client_addr::FORWARDING_KEY_HEADER,
                    FORWARDING_KEY
                ),
            ])
            .await,
            StatusCode::OK
        );
    }
}

// ── Privacy (DPDP) ───────────────────────────────────────────────────────────

// ── Curriculum ───────────────────────────────────────────────────────────────

// ── Platform and university ──────────────────────────────────────────────────

// ── Student lifecycle ────────────────────────────────────────────────────────

// ── Search ───────────────────────────────────────────────────────────────────

// ── Accounts office ──────────────────────────────────────────────────────────

// ── Notices ──────────────────────────────────────────────────────────────────

const NOTICE_GENERAL: &str = "70000000-0000-0000-0000-000000000001";
const NOTICE_CSE: &str = "70000000-0000-0000-0000-000000000003";

impl TestApp {
    async fn post_json(&self, path: &str, subject: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::post(path)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", testing::token(subject)),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::USER_AGENT, "integration-test")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn notice_titles(&self, subject: &str) -> Vec<String> {
        let (status, body) = self.call("GET", "/notices", Some(subject)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["notices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["title"].as_str().unwrap().to_owned())
            .collect()
    }
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn public_board_shows_current_general_notices_and_is_cacheable(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let response = app
        .router
        .clone()
        .oneshot(Request::get("/notices/public").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "public, max-age=60, stale-while-revalidate=300"
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let titles: Vec<_> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["title"].clone())
        .collect();
    assert_eq!(
        titles,
        ["Campus open day"],
        "no program, expired or scheduled notices"
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn each_role_sees_the_notices_meant_for_it(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    assert_eq!(
        app.notice_titles("auth0|a").await,
        ["ECE lab moved", "Campus open day"]
    );
    assert_eq!(
        app.notice_titles("auth0|f").await,
        ["ECE lab moved", "Campus open day"]
    );
    assert_eq!(
        app.notice_titles("auth0|e").await,
        [
            "Next week",
            // Published together: the later id first.
            "CSE exam moved",
            "ECE lab moved",
            "Campus open day",
            "Old notice"
        ],
        "the exam cell also sees scheduled and expired notices"
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn read_state_is_per_user_and_limited_to_visible_notices(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let (_, me) = app.call("GET", "/me", Some("auth0|a")).await;
    assert_eq!(me["unread_notices"], 2);

    // The CSE notice isn't visible to A, so it can't be marked read.
    let (status, _) = app
        .post_json(
            "/notices/read",
            "auth0|a",
            serde_json::json!({ "ids": [NOTICE_GENERAL, NOTICE_CSE] }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let reads: i64 = sqlx::query_scalar("SELECT count(*) FROM comms.notice_reads")
        .fetch_one(&app.admin)
        .await
        .unwrap();
    assert_eq!(reads, 1);

    let (_, list) = app.call("GET", "/notices", Some("auth0|a")).await;
    assert_eq!(list["unread"], 1);
    assert_eq!(list["notices"][1]["read"], true);
    let (_, me) = app.call("GET", "/me", Some("auth0|b")).await;
    assert_eq!(me["unread_notices"], 2, "B's read state is separate");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn only_the_exam_cell_publishes_notices(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let notice = serde_json::json!({
        "title": "  Fee deadline  ",
        "body": "Pay by Friday.",
        "severity": "important",
        "program_id": 11,
    });

    let (status, _) = app.post_json("/notices", "auth0|a", notice.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = app.post_json("/notices", "auth0|e", notice.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        app.notice_titles("auth0|a").await[0],
        "Fee deadline",
        "trimmed, and visible to ECE"
    );

    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit.action_log WHERE action = 'notice.published'",
    )
    .fetch_one(&app.admin)
    .await
    .unwrap();
    assert_eq!(audited, 1);

    let mut blank = notice.clone();
    blank["title"] = " ".into();
    let (status, _) = app.post_json("/notices", "auth0|e", blank).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let mut unknown = notice;
    unknown["program_id"] = 99.into();
    let (status, body) = app.post_json("/notices", "auth0|e", unknown).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["type"], "https://acadex.edu/errors/unknown-program");
}

// ── Results ──────────────────────────────────────────────────────────────────

const SEMESTER_4: &str = "7e000000-0000-0000-0000-000000000004";

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn published_results_are_withdrawn_by_two_people(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let semester_3 = "7e000000-0000-0000-0000-000000000003";
    let path = format!("/results/publications/{semester_3}/withdrawal");
    let semesters_a_sees = || async {
        let (_, body) = app
            .call(
                "GET",
                &format!("/students/{STUDENT_A}/results"),
                Some("auth0|a"),
            )
            .await;
        body["semesters"].as_array().map(|s| s.len()).unwrap_or(0)
    };
    assert_eq!(semesters_a_sees().await, 1);

    let (status, _) = app
        .post_json(
            &path,
            "auth0|a",
            serde_json::json!({ "reason": "My marks look wrong to me" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = app
        .post_json(&path, "auth0|e", serde_json::json!({ "reason": "short" }))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a reason is needed"
    );
    let (status, body) = app
        .post_json(
            &path,
            "auth0|e",
            serde_json::json!({ "reason": "EC301 externals were entered for the wrong paper" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        semesters_a_sees().await,
        1,
        "still published until approved"
    );
    let (status, _) = app
        .call("POST", &format!("{path}/approve"), Some("auth0|e"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "not by the person who asked");
    let (status, body) = app
        .call("POST", &format!("{path}/approve"), Some("auth0|e2"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["published_at"].is_null());
    assert_eq!(body["withdrawals"], 1);
    assert_eq!(semesters_a_sees().await, 0, "students no longer see it");
    assert_eq!(
        app.count("SELECT count(*) FROM comms.notices WHERE title = 'Semester 3 results withdrawn for correction'").await,
        1
    );

    // Corrected, it goes out again through the usual two people.
    let (status, _) = app
        .call(
            "POST",
            &format!("/results/publications/{semester_3}/request"),
            Some("auth0|e"),
        )
        .await;
    assert!(status.is_success());
    let (status, _) = app
        .call(
            "POST",
            &format!("/results/publications/{semester_3}/publish"),
            Some("auth0|e2"),
        )
        .await;
    assert!(status.is_success());
    assert_eq!(semesters_a_sees().await, 1);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn students_see_only_their_published_results(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/results"),
            Some("auth0|a"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let semesters: Vec<_> = body["semesters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["semester"].clone())
        .collect();
    assert_eq!(semesters, [3], "semester 4 is staged, so invisible");

    let selected = &body["selected"];
    assert_eq!(selected["semester"], 3);
    assert_eq!(
        (selected["sgpa"].clone(), selected["cgpa"].clone()),
        (10.0.into(), 10.0.into())
    );
    let ec301 = &selected["subjects"][0];
    assert_eq!(ec301["code"], "EC301");
    assert_eq!(
        ec301["components"][1],
        serde_json::json!({ "code": "MID", "max": 30.0, "marks": 27.0 })
    );
    assert_eq!(
        (ec301["internal"].clone(), ec301["external"].clone()),
        (36.0.into(), 54.0.into())
    );
    assert_eq!(
        (ec301["grade"].clone(), ec301["points"].clone()),
        ("O".into(), 10.into())
    );

    let (_, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/results?semester=4"),
            Some("auth0|a"),
        )
        .await;
    assert!(
        body["selected"].is_null(),
        "asking for the staged semester finds nothing"
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn below_the_external_minimum_is_a_fail(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (_, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_B}/results"),
            Some("auth0|b"),
        )
        .await;
    let selected = &body["selected"];
    assert_eq!(selected["subjects"][0]["grade"], "F");
    assert_eq!(selected["subjects"][0]["passed"], false);
    assert_eq!(
        (
            selected["credits_earned"].clone(),
            selected["passed"].clone()
        ),
        (0.into(), false.into())
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn results_are_private(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, _) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_B}/results"),
            Some("auth0|a"),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another student's results don't exist for A"
    );
    let (status, _) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/results"),
            Some("auth0|f"),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "faculty don't read results");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn publishing_reveals_results_once_with_notice_audit_and_event(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let publish = format!("/results/publications/{SEMESTER_4}/publish");

    let (_, list) = app
        .call("GET", "/results/publications", Some("auth0|e"))
        .await;
    assert_eq!(list[0]["semester"], 4, "staged sets come first");
    assert!(list[0]["published_at"].is_null());
    assert_eq!(
        (list[0]["students"].clone(), list[0]["subjects"].clone()),
        (2.into(), 1.into())
    );

    // The exam cell can review staged results before publishing.
    let (_, preview) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/results?semester=4"),
            Some("auth0|e"),
        )
        .await;
    assert_eq!(preview["selected"]["subjects"][0]["code"], "EC401");

    let (status, _) = app.call("POST", &publish, Some("auth0|a")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Two people: one asks, another approves.
    let request = format!("/results/publications/{SEMESTER_4}/request");
    let (status, body) = app.call("POST", &publish, Some("auth0|e")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["type"], "https://acadex.edu/errors/not-requested");
    let (status, body) = app.call("POST", &request, Some("auth0|e")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["requested_by"], "Exam Cell");
    let (status, _) = app.call("POST", &request, Some("auth0|e2")).await;
    assert_eq!(status, StatusCode::CONFLICT, "already requested");
    let (status, body) = app.call("POST", &publish, Some("auth0|e")).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "can't approve one's own request"
    );
    assert_eq!(body["type"], "https://acadex.edu/errors/same-person");

    let (status, body) = app.call("POST", &publish, Some("auth0|e2")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["published_by"], "Controller E2");
    assert_eq!(body["requested_by"], "Exam Cell");
    let (status, body) = app.call("POST", &publish, Some("auth0|e2")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["type"], "https://acadex.edu/errors/already-published");

    let (_, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/results"),
            Some("auth0|a"),
        )
        .await;
    let selected = &body["selected"];
    assert_eq!(selected["semester"], 4, "now the latest");
    assert_eq!(selected["subjects"][0]["grade"], "A");
    assert_eq!(selected["sgpa"], 8.0);
    assert_eq!(selected["cgpa"], 9.14, "(10 × 4 + 8 × 3) / 7");

    assert_eq!(
        app.notice_titles("auth0|a").await[0],
        "Semester 4 results published"
    );
    let (topic, semester): (String, i64) =
        sqlx::query_as("SELECT topic, (payload->>'semester')::bigint FROM ops.outbox")
            .fetch_one(&app.admin)
            .await
            .unwrap();
    assert_eq!((topic.as_str(), semester), ("results.published", 4));
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit.action_log WHERE action = 'results.published'",
    )
    .fetch_one(&app.admin)
    .await
    .unwrap();
    assert_eq!(audited, 1, "the rejected second publish left no trace");

    let (status, _) = app
        .call(
            "POST",
            "/results/publications/00000000-0000-0000-0000-000000000000/publish",
            Some("auth0|e"),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn teachers_enter_internal_marks_for_their_own_unpublished_subjects(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let sheet = format!("/offerings/{EC501}/internal");

    let (status, body) = app
        .call(
            "GET",
            &format!("/offerings/{EC502}/internal"),
            Some("auth0|f"),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "G teaches EC502");
    assert_eq!(body["type"], "https://acadex.edu/errors/not-your-subject");
    let (status, _) = app.call("GET", &sheet, Some("auth0|a")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "students don't enter marks");

    // EC301's result is published: its marks are locked.
    let (status, body) = app
        .put_json(
            &format!("/offerings/{EC301}/components"),
            "auth0|f",
            serde_json::json!([{ "code": "CCA1", "max_marks": 10 }]),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["type"], "https://acadex.edu/errors/results-published");

    let (status, body) = app
        .put_json(&format!("/offerings/{EC501}/components"), "auth0|f", serde_json::json!([{ "code": "cca1", "max_marks": 70 }, { "code": "MID", "max_marks": 40 }]))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "over 100 marks: {body}"
    );
    let (status, body) = app
        .put_json(&format!("/offerings/{EC501}/components"), "auth0|f", serde_json::json!([{ "code": "cca1", "max_marks": 10 }, { "code": "MID", "max_marks": 30 }]))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body[0]["code"], "CCA1");
    let cca1 = body[0]["id"].as_str().unwrap().to_owned();

    let (_, body) = app.call("GET", &sheet, Some("auth0|f")).await;
    let roster: Vec<_> = body["students"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["roll_no"].clone())
        .collect();
    assert_eq!(
        roster,
        ["ECE24001", "ECE24002", "ECE24003"],
        "the ECE 2024 batch"
    );

    let entry = |student: &str, marks: f64| serde_json::json!([{ "student_id": student, "component_id": cca1, "marks": marks }]);
    let (status, _) = app
        .put_json(&sheet, "auth0|f", entry(STUDENT_A, 10.5))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "over the component's maximum"
    );
    let (status, _) = app.put_json(&sheet, "auth0|f", entry(STUDENT_C, 5.0)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "C is in CSE");
    let (status, body) = app.put_json(&sheet, "auth0|f", entry(STUDENT_A, 8.5)).await;
    assert_eq!((status, body["saved"].clone()), (StatusCode::OK, 1.into()));
    let (status, _) = app.put_json(&sheet, "auth0|f", entry(STUDENT_A, 9.0)).await;
    assert_eq!(status, StatusCode::OK, "corrections overwrite");

    let (_, body) = app.call("GET", &sheet, Some("auth0|f")).await;
    assert_eq!(body["students"][0]["marks"][&cca1], 9.0);

    // A component with marks can't be dropped.
    let (status, body) = app
        .put_json(
            &format!("/offerings/{EC501}/components"),
            "auth0|f",
            serde_json::json!([{ "code": "MID", "max_marks": 30 }]),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body["type"],
        "https://acadex.edu/errors/component-has-marks"
    );
}

const EC301: &str = "0f000000-0000-0000-0000-000000000301";

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn a_published_backlog_pass_replaces_the_fail(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    // B registered EC301 (failed last year) on a backlog form.
    sqlx::raw_sql(
        "INSERT INTO exams.exam_forms (id, session_id, student_id, kind, fee_paise, status, submitted_at)
         VALUES ('f0f00000-0000-0000-0000-00000000000b', 'e5000000-0000-0000-0000-000000000001',
                 'b0000000-0000-0000-0000-00000000000b', 'backlog', 100000, 'submitted', now());
         INSERT INTO exams.exam_form_subjects (form_id, subject_id, offering_id)
         VALUES ('f0f00000-0000-0000-0000-00000000000b', '50000000-0000-0000-0000-000000000301',
                 '0f000000-0000-0000-0000-000000000301');",
    )
    .execute(&app.admin)
    .await
    .unwrap();

    let new_set = serde_json::json!({ "program_id": 11, "semester": 3, "academic_year": 2026, "kind": "backlog" });
    let (status, _) = app
        .post_json("/results/publications", "auth0|f", new_set.clone())
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, set) = app
        .post_json("/results/publications", "auth0|e", new_set.clone())
        .await;
    assert_eq!(status, StatusCode::CREATED, "{set}");
    let (status, _) = app
        .post_json("/results/publications", "auth0|e", new_set)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let set = set["id"].as_str().unwrap().to_owned();

    let (_, subjects) = app
        .call(
            "GET",
            &format!("/results/publications/{set}/subjects"),
            Some("auth0|e"),
        )
        .await;
    assert_eq!(subjects[0]["code"], "EC301");
    let marks = format!("/results/publications/{set}/subjects/{EC301}/marks");
    let (_, sheet) = app.call("GET", &marks, Some("auth0|e")).await;
    let students = sheet["students"].as_array().unwrap();
    assert_eq!(students.len(), 1, "only who registered: {sheet}");
    assert_eq!(
        (
            students[0]["roll_no"].clone(),
            students[0]["attempt"].clone()
        ),
        ("ECE24002".into(), 2.into())
    );

    let (status, _) = app
        .post_json(
            &format!("/results/publications/{set}/request"),
            "auth0|e",
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "no marks yet");
    let entry = |student: &str, marks: f64| serde_json::json!([{ "student_id": student, "attempt": 2, "marks": marks }]);
    let (status, _) = app
        .put_json(&marks, "auth0|e", entry(STUDENT_A, 50.0))
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "A didn't sit it");
    let (status, body) = app
        .put_json(&marks, "auth0|e", entry(STUDENT_B, 42.0))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Staged: B still sees the fail.
    let results = format!("/students/{STUDENT_B}/results?semester=3");
    let (_, body) = app.call("GET", &results, Some("auth0|b")).await;
    assert_eq!(body["selected"]["subjects"][0]["grade"], "F");
    assert_eq!(body["selected"]["history"].as_array().unwrap().len(), 1);

    app.post_json(
        &format!("/results/publications/{set}/request"),
        "auth0|e",
        Value::Null,
    )
    .await;
    let (status, _) = app
        .put_json(&marks, "auth0|e", entry(STUDENT_B, 45.0))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "frozen once requested");
    let (status, body) = app
        .call(
            "POST",
            &format!("/results/publications/{set}/publish"),
            Some("auth0|e2"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, body) = app.call("GET", &results, Some("auth0|b")).await;
    let selected = &body["selected"];
    // 5 + 15 internal + 42 external = 62: B+.
    assert_eq!(selected["subjects"][0]["grade"], "B+");
    assert_eq!(selected["subjects"][0]["kind"], "backlog");
    assert_eq!(
        selected["history"].as_array().unwrap().len(),
        2,
        "the fail stays on record"
    );
    assert_eq!(
        (selected["sgpa"].clone(), selected["sgpa_regular"].clone()),
        (7.0.into(), 0.0.into())
    );
    assert_eq!(selected["passed"], true);
    assert_eq!(
        app.notice_titles("auth0|b").await[0],
        "Semester 3 backlog results published"
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn grade_cards_wait_for_the_worker(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/grade-cards/3/pdf"),
            Some("auth0|a"),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["type"], "https://acadex.edu/errors/grade-card-pending");
    let (status, _) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/grade-cards/3/pdf"),
            Some("auth0|b"),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "B can't see A's card either way"
    );
}

// ── Fees ─────────────────────────────────────────────────────────────────────

const TUITION_A: i64 = 1001;
const EXAM_FEE_A: i64 = 1002;
const TUITION_B: i64 = 1003;

impl TestApp {
    async fn start_payment(
        &self,
        subject: &str,
        key: Option<&str>,
        charges: &[i64],
    ) -> (StatusCode, Value) {
        let mut request = Request::post("/fees/payments")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", testing::token(subject)),
            )
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(key) = key {
            request = request.header("idempotency-key", key);
        }
        let body = serde_json::json!({ "charge_ids": charges }).to_string();
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Delivers a webhook the way the gateway does, signed with `secret` at `timestamp`.
    async fn webhook_signed(
        &self,
        event: &Value,
        secret: &str,
        timestamp: i64,
    ) -> (StatusCode, Value) {
        let body = event.to_string();
        let signature = format!(
            "t={timestamp},v1={}",
            payments::sign(secret, timestamp, body.as_bytes())
        );
        let request = Request::post("/webhooks/payments")
            .header(header::CONTENT_TYPE, "application/json")
            .header(payments::SIGNATURE_HEADER, signature)
            .body(Body::from(body))
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn webhook(&self, event: &Value) -> (StatusCode, Value) {
        self.webhook_signed(event, WEBHOOK_SECRET, chrono::Utc::now().timestamp())
            .await
    }

    async fn count(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&self.admin)
            .await
            .unwrap()
    }
}

fn payment_event(event_id: &str, kind: &str, order_id: &str, amount: i64) -> Value {
    serde_json::json!({
        "id": event_id,
        "type": kind,
        "data": {
            "payment_id": format!("pay_{order_id}"),
            "order_id": order_id,
            "amount": amount,
            "currency": "INR",
            "method": "upi",
        },
    })
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn payable_derives_pending_from_the_ledger(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app.call("GET", "/fees/payable", Some("auth0|a")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["lines"].as_array().unwrap().len(), 2);
    assert_eq!(
        body["totals"]["pending"], 6_490_000,
        "₹62,500 + ₹2,400 in paise"
    );

    let (_, body) = app.call("GET", "/fees/payable", Some("auth0|b")).await;
    let tuition = &body["lines"][0];
    assert_eq!(
        (tuition["concession"].clone(), tuition["pending"].clone()),
        (1_562_500.into(), 4_687_500.into())
    );

    let (status, _) = app.call("GET", "/fees/payable", Some("auth0|f")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn starting_a_payment_is_idempotent_per_key(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app.start_payment("auth0|a", None, &[TUITION_A]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        body["type"],
        "https://acadex.edu/errors/idempotency-key-required"
    );

    let (status, first) = app
        .start_payment("auth0|a", Some("click-0001"), &[TUITION_A, EXAM_FEE_A])
        .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["amount_paise"], 6_490_000, "worked out by the server");
    let url = first["checkout_url"].as_str().unwrap();
    assert!(
        url.starts_with("http://checkout.test/pay?order_id=order_test_1&amount=6490000"),
        "{url}"
    );

    let (status, again) = app
        .start_payment("auth0|a", Some("click-0001"), &[TUITION_A, EXAM_FEE_A])
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["attempt_id"], first["attempt_id"]);
    assert_eq!(
        app.cloud.orders.load(Ordering::SeqCst),
        1,
        "the retry opened no second order"
    );

    let (status, body) = app
        .start_payment("auth0|a", Some("click-0002"), &[TUITION_B])
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "B's charge doesn't exist for A"
    );
    assert_eq!(body["type"], "https://acadex.edu/errors/unknown-charge");
    let (status, _) = app.start_payment("auth0|a", Some("click-0003"), &[]).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn webhooks_must_be_signed_and_fresh(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let event = payment_event("evt_1", "payment.captured", "order_test_1", 100);
    let now = chrono::Utc::now().timestamp();

    let (status, _) = app.webhook_signed(&event, "wrong-secret", now).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = app.webhook_signed(&event, WEBHOOK_SECRET, now - 3600).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "an hour-old signature is a replay"
    );
    let response = app
        .router
        .clone()
        .oneshot(
            Request::post("/webhooks/payments")
                .body(Body::from(event.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "unsigned");
    assert_eq!(
        app.count("SELECT count(*) FROM fees.gateway_events").await,
        0
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn a_captured_payment_settles_exactly_once(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (_, checkout) = app
        .start_payment("auth0|a", Some("click-0001"), &[TUITION_A, EXAM_FEE_A])
        .await;
    let attempt = checkout["attempt_id"].as_str().unwrap().to_owned();
    let event = payment_event("evt_1", "payment.captured", "order_test_1", 6_490_000);

    let (status, body) = app.webhook(&event).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "processed");

    // Redelivery of the same event, and the same payment under a new event id.
    let (_, body) = app.webhook(&event).await;
    assert_eq!(body["status"], "duplicate");
    let (_, body) = app
        .webhook(&payment_event(
            "evt_2",
            "payment.captured",
            "order_test_1",
            6_490_000,
        ))
        .await;
    assert_eq!(body["status"], "duplicate");

    assert_eq!(app.count("SELECT count(*) FROM fees.payments").await, 1);
    assert_eq!(app.count("SELECT count(*) FROM fees.receipts").await, 1);
    assert_eq!(
        app.count("SELECT count(*) FROM fees.ledger WHERE kind = 'payment'")
            .await,
        2
    );
    assert_eq!(
        app.count("SELECT count(*) FROM ops.outbox WHERE topic = 'receipt.requested'")
            .await,
        1
    );

    let (_, payable) = app.call("GET", "/fees/payable", Some("auth0|a")).await;
    assert_eq!(payable["totals"]["pending"], 0);
    let (_, status) = app
        .call("GET", &format!("/fees/payments/{attempt}"), Some("auth0|a"))
        .await;
    assert_eq!(status["status"], "captured");
    let receipt_no = status["receipt_no"].as_str().unwrap();
    assert!(
        receipt_no.starts_with("RCPT/") && receipt_no.ends_with("/000001"),
        "{receipt_no}"
    );

    let (_, receipts) = app.call("GET", "/fees/receipts", Some("auth0|a")).await;
    assert_eq!(
        receipts[0]["remarks"],
        "Tuition fee — Semester 5, Examination fee — Semester 5"
    );
    assert_eq!(
        receipts[0]["pdf_ready"], false,
        "the worker hasn't made it yet"
    );

    // On administration's Logs page: the student's start, the gateway's confirmation.
    let (_, logs) = app
        .call("GET", "/audit/logs?category=fees", Some("auth0|m"))
        .await;
    let actions: Vec<_> = logs["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].clone())
        .collect();
    assert_eq!(actions, ["payment.captured", "payment.initiated"]);
    assert_eq!(logs["entries"][0]["actor_role"], "gateway");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn a_wrong_amount_is_held_for_review(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (_, checkout) = app
        .start_payment("auth0|a", Some("click-0001"), &[EXAM_FEE_A])
        .await;
    let (_, body) = app
        .webhook(&payment_event(
            "evt_1",
            "payment.captured",
            "order_test_1",
            100,
        ))
        .await;
    assert_eq!(body["status"], "review");
    assert_eq!(app.count("SELECT count(*) FROM fees.payments").await, 0);
    let (_, status) = app
        .call(
            "GET",
            &format!(
                "/fees/payments/{}",
                checkout["attempt_id"].as_str().unwrap()
            ),
            Some("auth0|a"),
        )
        .await;
    assert_eq!(status["status"], "review");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn a_failed_payment_leaves_the_charge_payable(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    app.start_payment("auth0|a", Some("click-0001"), &[EXAM_FEE_A])
        .await;
    let (_, body) = app
        .webhook(&payment_event(
            "evt_1",
            "payment.failed",
            "order_test_1",
            240_000,
        ))
        .await;
    assert_eq!(body["status"], "processed");
    let (_, payable) = app.call("GET", "/fees/payable", Some("auth0|a")).await;
    assert_eq!(payable["totals"]["pending"], 6_490_000);
    let (status, _) = app
        .start_payment("auth0|a", Some("click-0002"), &[EXAM_FEE_A])
        .await;
    assert_eq!(status, StatusCode::CREATED, "a new attempt can follow");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn receipt_numbers_are_gapless(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    app.start_payment("auth0|a", Some("click-0001"), &[EXAM_FEE_A])
        .await;
    app.webhook(&payment_event(
        "evt_1",
        "payment.captured",
        "order_test_1",
        240_000,
    ))
    .await;
    app.start_payment("auth0|b", Some("click-0002"), &[TUITION_B])
        .await;
    app.webhook(&payment_event(
        "evt_2",
        "payment.captured",
        "order_test_2",
        4_687_500,
    ))
    .await;
    let seqs: Vec<i32> = sqlx::query_scalar("SELECT seq FROM fees.receipts ORDER BY seq")
        .fetch_all(&app.admin)
        .await
        .unwrap();
    assert_eq!(seqs, [1, 2]);
}

// ── Outbox and receipt PDFs ──────────────────────────────────────────────────

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn the_dispatcher_publishes_each_event_once(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    app.start_payment("auth0|a", Some("click-0001"), &[EXAM_FEE_A])
        .await;
    app.webhook(&payment_event(
        "evt_1",
        "payment.captured",
        "order_test_1",
        240_000,
    ))
    .await;

    let pubsub = PubSub::new(format!("{}/push", app.cloud.url));
    let sent = acadex_api::dispatch::dispatch_once(&app.pool, &pubsub)
        .await
        .unwrap();
    assert_eq!(sent, 1);
    assert_eq!(
        acadex_api::dispatch::dispatch_once(&app.pool, &pubsub)
            .await
            .unwrap(),
        0,
        "already sent"
    );

    let published = app.cloud.published.lock().unwrap().clone();
    let (topic, message) = &published[0];
    assert_eq!(topic, "receipt.requested");
    assert_eq!(message["attributes"]["event"], "receipt.requested");
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD
        .decode(message["data"].as_str().unwrap())
        .unwrap();
    let body: Value = serde_json::from_slice(&data).unwrap();
    assert!(body["receipt_id"].is_string());
    assert_eq!(
        app.count("SELECT count(*) FROM ops.outbox WHERE published_at IS NULL")
            .await,
        0
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn students_download_only_their_own_receipts(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    app.start_payment("auth0|a", Some("click-0001"), &[EXAM_FEE_A])
        .await;
    app.webhook(&payment_event(
        "evt_1",
        "payment.captured",
        "order_test_1",
        240_000,
    ))
    .await;
    let receipt: uuid::Uuid = sqlx::query_scalar("SELECT id FROM fees.receipts")
        .fetch_one(&app.admin)
        .await
        .unwrap();
    let path = format!("/fees/receipts/{receipt}/pdf");

    let (status, body) = app.call("GET", &path, Some("auth0|a")).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the worker hasn't made it yet"
    );
    assert_eq!(body["type"], "https://acadex.edu/errors/receipt-not-ready");

    // As the worker would: store the PDF and record where.
    Storage::new(&app.cloud.files)
        .put(
            "receipts-test",
            "receipts/r1.pdf",
            "application/pdf",
            b"%PDF-1.7 test".to_vec(),
        )
        .await
        .unwrap();
    sqlx::query("UPDATE fees.receipts SET pdf_object = 'receipts/r1.pdf'")
        .execute(&app.admin)
        .await
        .unwrap();

    let request = Request::get(&path)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", testing::token("auth0|a")),
        )
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .ends_with("-000001.pdf\"")
    );
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&bytes[..], b"%PDF-1.7 test");

    let (status, _) = app.call("GET", &path, Some("auth0|b")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "someone else's receipt doesn't exist"
    );
}

// ── Caching ──────────────────────────────────────────────────────────────────

impl TestApp {
    /// GET as `subject`; returns status, x-cache, cache-control and body.
    async fn get_cached(&self, path: &str, subject: &str) -> (StatusCode, String, String, Value) {
        let request = Request::get(path)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", testing::token(subject)),
            )
            .body(Body::empty())
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let header_of = |name: &str| {
            response
                .headers()
                .get(name)
                .map(|v| v.to_str().unwrap().to_owned())
                .unwrap_or_default()
        };
        let (status, x_cache, cache_control) = (
            response.status(),
            header_of("x-cache"),
            header_of("cache-control"),
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            x_cache,
            cache_control,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn results_are_cached_until_publication(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let path = format!("/students/{STUDENT_A}/results");

    let (status, x_cache, cache_control, body) = app.get_cached(&path, "auth0|a").await;
    assert_eq!((status, x_cache.as_str()), (StatusCode::OK, "miss"));
    assert_eq!(
        cache_control, "private, no-store",
        "per-user: never in a shared cache"
    );
    assert_eq!(body["selected"]["semester"], 3);
    let (_, x_cache, _, _) = app.get_cached(&path, "auth0|a").await;
    assert_eq!(x_cache, "hit");

    // Publishing invalidates at once: no waiting out the TTL.
    app.call(
        "POST",
        &format!("/results/publications/{SEMESTER_4}/request"),
        Some("auth0|e"),
    )
    .await;
    app.call(
        "POST",
        &format!("/results/publications/{SEMESTER_4}/publish"),
        Some("auth0|e2"),
    )
    .await;
    let (_, x_cache, _, body) = app.get_cached(&path, "auth0|a").await;
    assert_eq!(x_cache, "miss");
    assert_eq!(body["selected"]["semester"], 4);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn cached_responses_never_cross_users(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let path = format!("/students/{STUDENT_A}/attendance?academic_year=2026");
    let (status, _, _, _) = app.get_cached(&path, "auth0|a").await;
    assert_eq!(status, StatusCode::OK);
    // B asks for the same URL: A's cached copy must not be served.
    let (status, x_cache, _, _) = app.get_cached(&path, "auth0|b").await;
    assert_eq!((status, x_cache.as_str()), (StatusCode::NOT_FOUND, ""));
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn marking_attendance_invalidates_the_students_summary(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let path = format!("/students/{STUDENT_A}/attendance?academic_year=2026");
    let (_, _, _, before) = app.get_cached(&path, "auth0|a").await;
    app.get_cached(&path, "auth0|a").await;

    app.put_json(
        &format!("/offerings/{EC501}/attendance"),
        "auth0|f",
        marks(&[(STUDENT_A, "absent")]),
    )
    .await;
    let (_, x_cache, _, after) = app.get_cached(&path, "auth0|a").await;
    assert_eq!(x_cache, "miss");
    assert_eq!(
        after["overall"]["conducted"].as_i64().unwrap(),
        before["overall"]["conducted"].as_i64().unwrap() + 1
    );
}

// ── Roles and administration (F1) ────────────────────────────────────────────

// ── Academic (F2) ────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn the_calendar_shows_each_person_their_events(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let range = "/academic/calendar?from=2026-10-01&to=2026-10-31";
    let titles = |body: &Value| -> Vec<String> {
        body.as_array()
            .unwrap()
            .iter()
            .map(|e| e["title"].as_str().unwrap().to_owned())
            .collect()
    };
    let (_, body) = app.call("GET", range, Some("auth0|a")).await;
    assert_eq!(
        titles(&body),
        ["ECE project expo", "Diwali break"],
        "everyone's plus ECE's"
    );
    let (_, body) = app.call("GET", range, Some("auth0|e")).await;
    assert_eq!(titles(&body).len(), 3, "the exam cell manages all of them");

    let event = serde_json::json!({ "kind": "exam", "title": "Semester 5 exams", "starts_on": "2026-11-16", "ends_on": "2026-11-28" });
    assert_eq!(
        app.post_json("/academic/calendar", "auth0|a", event.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status, body) = app.post_json("/academic/calendar", "auth0|e", event).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let request = Request::get("/academic/calendar.ics")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", testing::token("auth0|a")),
        )
        .body(Body::empty())
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/calendar; charset=utf-8"
    );
    let ics = String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        ics.contains("SUMMARY:Semester 5 exams") && ics.contains("SUMMARY:Diwali break"),
        "{ics}"
    );
    assert!(!ics.contains("CSE lab exam"));

    let id = body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        app.call(
            "DELETE",
            &format!("/academic/calendar/{id}"),
            Some("auth0|e")
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn attendance_by_day_and_by_session(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (status, body) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_B}/attendance/days?month=2026-07"),
            Some("auth0|b"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let days = body["days"].as_array().unwrap();
    assert_eq!(days.len(), 4, "13–16 July");
    assert_eq!(
        (days[1]["sessions"].clone(), days[1]["absent"].clone()),
        (2.into(), 2.into()),
        "B missed both on the 14th"
    );

    let (_, sessions) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_B}/attendance/sessions?subject=EC501&academic_year=2026"),
            Some("auth0|b"),
        )
        .await;
    assert_eq!(sessions.as_array().unwrap().len(), 4);
    assert_eq!(sessions[0]["date"], "2026-07-16", "newest first");

    let (status, _) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_B}/attendance/days?month=2026-07"),
            Some("auth0|a"),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "not A's to see");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn an_hod_approves_a_correction_and_the_mark_changes(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let summary = format!("/students/{STUDENT_B}/attendance?academic_year=2026");
    let (_, before) = app.call("GET", &summary, Some("auth0|b")).await;

    let request = serde_json::json!({
        "offering_id": EC501, "session_date": "2026-07-14", "slot": 1,
        "requested_status": "excused", "reason": "Represented the college at the state quiz",
    });
    let (status, body) = app
        .post_json("/attendance/corrections", "auth0|b", request.clone())
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().unwrap().to_owned();
    assert_eq!(
        app.post_json("/attendance/corrections", "auth0|b", request)
            .await
            .0,
        StatusCode::CONFLICT,
        "one open request per session"
    );
    let wrong = serde_json::json!({ "offering_id": EC501, "session_date": "2026-07-13", "slot": 1, "requested_status": "present", "reason": "Was present" });
    assert_eq!(
        app.post_json("/attendance/corrections", "auth0|b", wrong)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY,
        "already present"
    );

    let (_, queue) = app
        .call("GET", "/attendance/corrections", Some("auth0|f"))
        .await;
    assert_eq!(queue[0]["status"], "pending", "EC501's teacher sees it");
    let decision = format!("/attendance/corrections/{id}/decision");
    let approve =
        serde_json::json!({ "approve": true, "note": "Letter from the quiz organisers on file" });
    assert_eq!(
        app.post_json(&decision, "auth0|f", approve.clone()).await.0,
        StatusCode::FORBIDDEN,
        "faculty can't decide"
    );
    assert_eq!(
        app.post_json(&decision, "auth0|h", approve.clone()).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.post_json(&decision, "auth0|h", approve).await.0,
        StatusCode::NOT_FOUND,
        "decided once"
    );

    let (_, sessions) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_B}/attendance/sessions?subject=EC501&academic_year=2026"),
            Some("auth0|b"),
        )
        .await;
    let fourteenth = sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["date"] == "2026-07-14")
        .unwrap();
    assert_eq!(
        (
            fourteenth["status"].clone(),
            fourteenth["marked_by"].clone()
        ),
        ("excused".into(), "HOD H".into())
    );
    assert_eq!(fourteenth["correction"], "approved");

    // Excused still isn't attended, but the summary is recomputed, not stale.
    let (_, after) = app.call("GET", &summary, Some("auth0|b")).await;
    assert_eq!(
        after["overall"]["conducted"],
        before["overall"]["conducted"]
    );
    assert_eq!(
        app.count(
            "SELECT count(*) FROM audit.action_log WHERE action LIKE 'attendance.correction_%'"
        )
        .await,
        2
    );
}

// ── Examination: sessions and forms (F3a) ────────────────────────────────────

// ── Hall tickets (F3b) ───────────────────────────────────────────────────────

// ── Activities (F4) ──────────────────────────────────────────────────────────

// ── Profile (F4) ─────────────────────────────────────────────────────────────

// ── Support (F5) ─────────────────────────────────────────────────────────────

// ── Notices (F5) ─────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn notices_reach_their_batch_section_or_named_people(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let base = |title: &str| serde_json::json!({ "title": title, "body": "Details", "severity": "general" });

    let mut section_b = base("Section B lab moved");
    section_b["program_id"] = 11.into();
    section_b["batch_year"] = 2024.into();
    section_b["section"] = "b".into();
    let (status, body) = app.post_json("/notices", "auth0|e", section_b).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let mut named = base("Please see the HOD");
    named["recipient_ids"] = serde_json::json!([STUDENT_A]);
    app.post_json("/notices", "auth0|e", named).await;
    let mut later = base("Next week's holiday");
    later["publish_at"] = (chrono::Utc::now() + chrono::Duration::days(2))
        .to_rfc3339()
        .into();
    let (_, body) = app.post_json("/notices", "auth0|e", later).await;
    assert_eq!(body["scheduled"], true);

    let a = app.notice_titles("auth0|a").await;
    let b = app.notice_titles("auth0|b").await;
    assert!(
        a.contains(&"Please see the HOD".to_owned())
            && !a.contains(&"Section B lab moved".to_owned()),
        "{a:?}"
    );
    assert!(
        b.contains(&"Section B lab moved".to_owned())
            && !b.contains(&"Please see the HOD".to_owned()),
        "{b:?}"
    );
    assert!(
        !a.contains(&"Next week's holiday".to_owned()),
        "scheduled notices wait"
    );
    assert!(
        !app.notice_titles("auth0|f")
            .await
            .contains(&"Section B lab moved".to_owned()),
        "batch notices are for students"
    );
    assert_eq!(
        app.count("SELECT count(*) FROM ops.outbox WHERE topic = 'notice.published'")
            .await,
        2,
        "the scheduled one fans out later"
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn critical_notices_wait_for_acknowledgement(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    let (_, body) = app
        .post_json("/notices", "auth0|e", serde_json::json!({ "title": "Exam postponed", "body": "See the new date", "severity": "critical", "program_id": 11 }))
        .await;
    let id = body["id"].as_str().unwrap().to_owned();
    let (_, pending) = app
        .call("GET", "/notices/awaiting-acknowledgement", Some("auth0|a"))
        .await;
    assert_eq!(pending[0]["title"], "Exam postponed");
    let (_, pending) = app
        .call("GET", "/notices/awaiting-acknowledgement", Some("auth0|e"))
        .await;
    assert_eq!(
        pending,
        serde_json::json!([]),
        "publishers aren't its audience"
    );

    let (status, _) = app
        .post_json(
            &format!("/notices/{id}/acknowledge"),
            "auth0|a",
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, pending) = app
        .call("GET", "/notices/awaiting-acknowledgement", Some("auth0|a"))
        .await;
    assert_eq!(pending, serde_json::json!([]));
    let (status, _) = app
        .post_json(
            &format!("/notices/{id}/acknowledge"),
            "auth0|c",
            Value::Null,
        )
        .await;
    assert_ne!(
        status,
        StatusCode::NO_CONTENT,
        "a CSE student can't acknowledge an ECE notice"
    );

    let (status, receipts) = app
        .call("GET", &format!("/notices/{id}/receipts"), Some("auth0|e"))
        .await;
    assert_eq!(status, StatusCode::OK, "{receipts}");
    assert_eq!(
        (receipts["read"].clone(), receipts["acknowledged"].clone()),
        (1.into(), 1.into())
    );
    let (status, _) = app
        .call("GET", &format!("/notices/{id}/receipts"), Some("auth0|a"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (_, prefs) = app
        .call("GET", "/notices/preferences", Some("auth0|a"))
        .await;
    assert_eq!(prefs["email_level"], "important");
    let (status, _) = app
        .put_json(
            "/notices/preferences",
            "auth0|a",
            serde_json::json!({ "email_level": "loud" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, prefs) = app
        .put_json(
            "/notices/preferences",
            "auth0|a",
            serde_json::json!({ "email_level": "critical" }),
        )
        .await;
    assert_eq!(prefs["email_level"], "critical");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn notices_page_from_newest_to_oldest(opts: PgPoolOptions, conn: PgConnectOptions) {
    let app = app(opts, conn).await;
    sqlx::raw_sql(
        "INSERT INTO comms.notices (institution_id, title, body, published_at)
         SELECT 1, 'Notice ' || n, 'Body', now() - n * interval '1 hour' FROM generate_series(1, 70) n",
    )
    .execute(&app.admin)
    .await
    .unwrap();
    let (_, first) = app.call("GET", "/notices", Some("auth0|a")).await;
    let page = first["notices"].as_array().unwrap();
    assert_eq!(page.len(), 50);
    let next = first["next"].as_array().unwrap();
    let query = format!(
        "/notices?before={}&before_id={}",
        next[0].as_str().unwrap().replace('+', "%2B"),
        next[1].as_str().unwrap()
    );
    let (status, second) = app.call("GET", &query, Some("auth0|a")).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let older = second["notices"].as_array().unwrap();
    assert!(!older.is_empty() && older.len() < 50, "{}", older.len());
    assert!(second["next"].is_null());
    assert!(
        older
            .iter()
            .all(|n| !page.iter().any(|p| p["id"] == n["id"])),
        "no overlap"
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn notices_are_corrected_withdrawn_or_unscheduled(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let (status, created) = app
        .post_json(
            "/notices",
            "auth0|e",
            serde_json::json!({ "title": "Lab moved", "body": "To Block C.", "severity": "critical" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_str().unwrap().to_owned();
    let (status, _) = app
        .post_json(
            &format!("/notices/{id}/acknowledge"),
            "auth0|a",
            serde_json::json!({}),
        )
        .await;
    assert!(status.is_success());
    let title_seen_by_a = || async {
        let (_, list) = app.call("GET", "/notices", Some("auth0|a")).await;
        list["notices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id.as_str())
            .map(|n| {
                (
                    n["title"].as_str().unwrap().to_owned(),
                    n["acknowledged"].as_bool().unwrap(),
                    !n["edited_at"].is_null(),
                )
            })
    };
    assert_eq!(
        title_seen_by_a().await,
        Some(("Lab moved".to_owned(), true, false))
    );

    // A correction: students can't, the exam cell can, and A acknowledges the new text.
    let fix = serde_json::json!({ "title": "Lab moved to Block D", "body": "To Block D, room 12.", "severity": "critical" });
    let (status, _) = app
        .put_json(&format!("/notices/{id}"), "auth0|a", fix.clone())
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = app
        .put_json(&format!("/notices/{id}"), "auth0|e", fix)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        title_seen_by_a().await,
        Some(("Lab moved to Block D".to_owned(), false, true))
    );
    let before: Value = sqlx::query_scalar(
        "SELECT metadata->'before'->'title' FROM audit.action_log WHERE action = 'notice.edited'",
    )
    .fetch_one(&app.admin)
    .await
    .unwrap();
    assert_eq!(before, "Lab moved", "the log keeps what it replaced");

    // Withdrawn: gone for the audience; it needs a reason, and can't be deleted.
    let (status, _) = app
        .post_json(
            &format!("/notices/{id}/withdraw"),
            "auth0|e",
            serde_json::json!({ "reason": "" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = app
        .call("DELETE", &format!("/notices/{id}"), Some("auth0|e"))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = app
        .post_json(
            &format!("/notices/{id}/withdraw"),
            "auth0|e",
            serde_json::json!({ "reason": "Sent by mistake" }),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(title_seen_by_a().await, None);
    let (_, list) = app.call("GET", "/notices", Some("auth0|e")).await;
    let mine = list["notices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id.as_str())
        .unwrap()
        .clone();
    assert_eq!(mine["withdraw_reason"], "Sent by mistake");

    // A scheduled notice nobody has seen can simply be deleted.
    let (_, scheduled) = app
        .post_json(
            "/notices",
            "auth0|m",
            serde_json::json!({ "title": "Holiday", "body": "Friday off.", "severity": "general",
                                "publish_at": (chrono::Utc::now() + chrono::Duration::days(2)).to_rfc3339() }),
        )
        .await;
    let (status, _) = app
        .call(
            "DELETE",
            &format!("/notices/{}", scheduled["id"].as_str().unwrap()),
            Some("auth0|m"),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ── Theory and practical attendance ──────────────────────────────────────────

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn practicals_are_marked_and_held_to_the_threshold_separately(
    opts: PgPoolOptions,
    conn: PgConnectOptions,
) {
    let app = app(opts, conn).await;
    let path = format!("/offerings/{EC501}/attendance");
    let practical = |date: &str, slot: i16, status: &str| {
        serde_json::json!({ "session_date": date, "slot": slot, "session_kind": "practical",
                            "marks": [{ "student_id": STUDENT_A, "status": status }] })
    };
    let (status, body) = app
        .put_json(&path, "auth0|f", practical("2026-09-29", 5, "absent"))
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "EC501 has no practicals yet"
    );
    assert_eq!(body["type"], "https://acadex.edu/errors/wrong-session-kind");

    sqlx::query("UPDATE academic.subjects SET has_practical = true WHERE code = 'EC501'")
        .execute(&app.admin)
        .await
        .unwrap();
    for (date, status) in [
        ("2026-09-22", "present"),
        ("2026-09-29", "absent"),
        ("2026-09-30", "absent"),
    ] {
        let (code, body) = app
            .put_json(&path, "auth0|f", practical(date, 5, status))
            .await;
        assert_eq!(code, StatusCode::OK, "{body}");
    }
    let (_, roster) = app
        .call(
            "GET",
            &format!("/offerings/{EC501}/roster?date=2026-09-29&slot=5"),
            Some("auth0|f"),
        )
        .await;
    assert_eq!(roster["session_kind"], "practical");

    let (_, report) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/attendance"),
            Some("auth0|a"),
        )
        .await;
    let ec501 = report["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["code"] == "EC501")
        .unwrap()
        .clone();
    assert_eq!(
        (
            ec501["theory"]["conducted"].clone(),
            ec501["theory"]["shortage"].clone()
        ),
        (4.into(), false.into())
    );
    assert_eq!(
        (
            ec501["practical"]["conducted"].clone(),
            ec501["practical"]["attended"].clone()
        ),
        (3.into(), 1.into())
    );
    assert_eq!(
        ec501["shortage"], true,
        "4/4 theory but 1/3 practical: short"
    );

    let (_, sessions) = app
        .call(
            "GET",
            &format!("/students/{STUDENT_A}/attendance/sessions?subject=EC501"),
            Some("auth0|a"),
        )
        .await;
    let kinds: Vec<_> = sessions
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["session_kind"].clone())
        .collect();
    assert!(
        kinds.contains(&"practical".into()) && kinds.contains(&"theory".into()),
        "{kinds:?}"
    );
}
