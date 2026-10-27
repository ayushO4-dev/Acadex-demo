//! Row-level security, partition pruning, identity linking and the audit chain,
//! tested against a real Postgres. Each test gets a fresh database (sqlx::test)
//! with migrations and `fixtures/test_data.sql` applied.
//!
//! Needs DATABASE_URL pointing at a superuser (`just test` sets it).

use acadex_db::{attendance, audit, identity, people};
use serde_json::json;
use sqlx::{AssertSqlSafe, PgPool, Postgres, SqlSafeStr, Transaction};
use uuid::{Uuid, uuid};

const STUDENT_A: Uuid = uuid!("a0000000-0000-0000-0000-00000000000a");
const STUDENT_B: Uuid = uuid!("b0000000-0000-0000-0000-00000000000b");
const STUDENT_C: Uuid = uuid!("c0000000-0000-0000-0000-00000000000c");
const FACULTY_F: Uuid = uuid!("f0000000-0000-0000-0000-00000000000f");
const EXAM_CELL: Uuid = uuid!("e0000000-0000-0000-0000-00000000000e");
const ECE: i16 = 11;

async fn load_fixture(pool: &PgPool) {
    sqlx::raw_sql(include_str!("../fixtures/test_data.sql"))
        .execute(pool)
        .await
        .expect("fixture loads");
}

/// A transaction that behaves exactly like an API request: the app role (so RLS
/// applies, even though the test connection is a superuser) plus the user binding.
async fn as_user(pool: &PgPool, user: Uuid, role: &str) -> Transaction<'static, Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE acadex_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('app.user_id', $1, true), set_config('app.role', $2, true)")
        .bind(user.to_string())
        .bind(role)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

async fn count(tx: &mut Transaction<'static, Postgres>, sql: impl SqlSafeStr) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&mut **tx).await.unwrap()
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn student_cannot_read_another_students_attendance(pool: PgPool) {
    load_fixture(&pool).await;
    let mut tx = as_user(&pool, STUDENT_A, "student").await;

    // The same query the API runs, aimed at B — a hand-crafted request, in effect.
    let others = attendance::summary(&mut tx, ECE, 2026, STUDENT_B, None)
        .await
        .unwrap();
    assert!(
        others.is_empty(),
        "A must not see B's attendance: {others:?}"
    );
    assert!(people::student(&mut tx, STUDENT_B).await.unwrap().is_none());

    let own = attendance::summary(&mut tx, ECE, 2026, STUDENT_A, None)
        .await
        .unwrap();
    assert_eq!(own.len(), 2, "A sees their own EC501 and EC502");
    assert!(people::student(&mut tx, STUDENT_A).await.unwrap().is_some());

    // Not even a filter-free query leaks anything.
    let foreign = count(
        &mut tx,
        AssertSqlSafe(format!(
            "SELECT count(*) FROM academic.attendance WHERE student_id <> '{STUDENT_A}'"
        )),
    )
    .await;
    assert_eq!(foreign, 0);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn faculty_sees_only_their_offerings(pool: PgPool) {
    load_fixture(&pool).await;
    let mut tx = as_user(&pool, FACULTY_F, "faculty").await;

    // F teaches EC501 (2026) but not EC502, so B's report shows one subject.
    let report = attendance::summary(&mut tx, ECE, 2026, STUDENT_B, None)
        .await
        .unwrap();
    let codes: Vec<_> = report.iter().map(|s| s.code.as_str()).collect();
    assert_eq!(codes, ["EC501"]);

    // ECE students are visible; the CSE student is not.
    assert!(people::student(&mut tx, STUDENT_B).await.unwrap().is_some());
    assert!(people::student(&mut tx, STUDENT_C).await.unwrap().is_none());
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn faculty_cannot_mark_attendance_for_someone_elses_offering(pool: PgPool) {
    load_fixture(&pool).await;
    let mut tx = as_user(&pool, FACULTY_F, "faculty").await;
    let result = sqlx::query(
        "INSERT INTO academic.attendance
             (program_id, academic_year, student_id, offering_id, session_date, slot, status, marked_by)
         VALUES (11, 2026, $1, '0f000000-0000-0000-0000-000000000502', '2026-08-03', 2, 'present', $2)",
    )
    .bind(STUDENT_A)
    .bind(FACULTY_F)
    .execute(&mut *tx)
    .await;
    let err = result.expect_err("EC502 belongs to faculty G");
    assert!(err.to_string().contains("row-level security"), "{err}");
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn exam_cell_sees_everyone(pool: PgPool) {
    load_fixture(&pool).await;
    let mut tx = as_user(&pool, EXAM_CELL, "exam_cell").await;
    assert_eq!(
        count(&mut tx, "SELECT count(*) FROM people.students").await,
        4
    );
    let report = attendance::summary(&mut tx, 12, 2026, STUDENT_C, None)
        .await
        .unwrap();
    assert_eq!(report.len(), 1);
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn no_bound_user_sees_nothing(pool: PgPool) {
    load_fixture(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE acadex_app")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        count(&mut tx, "SELECT count(*) FROM academic.attendance").await,
        0
    );
    assert_eq!(
        count(&mut tx, "SELECT count(*) FROM people.students").await,
        0
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn app_role_cannot_bypass_the_parent_table_or_read_emails(pool: PgPool) {
    load_fixture(&pool).await;
    for sql in [
        "SELECT count(*) FROM academic.attendance_btech_ece_2026",
        "SELECT count(*) FROM identity.users WHERE email LIKE '%'",
        "SELECT count(*) FROM identity.user_identities",
    ] {
        let mut tx = as_user(&pool, STUDENT_A, "student").await;
        let err = sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(&mut *tx)
            .await
            .expect_err(sql);
        assert!(
            err.to_string().contains("permission denied"),
            "{sql}: {err}"
        );
    }
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn my_attendance_scans_one_partition(pool: PgPool) {
    load_fixture(&pool).await;
    let mut tx = as_user(&pool, STUDENT_A, "student").await;
    let sql = attendance::SUMMARY_SQL
        .replace("$1", &ECE.to_string())
        .replace("$2", "2026")
        .replace("$3", &format!("'{STUDENT_A}'"))
        .replace("$4", "5");
    let plan: serde_json::Value =
        sqlx::query_scalar(AssertSqlSafe(format!("EXPLAIN (FORMAT JSON) {sql}")))
            .fetch_one(&mut *tx)
            .await
            .unwrap();

    let mut scanned = Vec::new();
    collect_relations(&plan, &mut scanned);
    scanned.retain(|name| name.starts_with("attendance"));
    assert_eq!(scanned, ["attendance_btech_ece_2026"], "plan: {plan:#}");
}

fn collect_relations(node: &serde_json::Value, out: &mut Vec<String>) {
    match node {
        serde_json::Value::Object(map) => {
            if let Some(name) = map.get("Relation Name").and_then(|v| v.as_str()) {
                out.push(name.to_owned());
            }
            map.values().for_each(|v| collect_relations(v, out));
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| collect_relations(v, out)),
        _ => {}
    }
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn linking_requires_a_verified_email_and_happens_once(pool: PgPool) {
    load_fixture(&pool).await;

    let unverified = identity::resolve_subject(
        &pool,
        "auth0|new",
        Some("unlinked@acadex.test"),
        false,
        false,
    )
    .await
    .unwrap();
    assert!(unverified.is_none(), "unverified email must not link");

    let linked = identity::resolve_subject(
        &pool,
        "auth0|new",
        Some("Unlinked@Acadex.test"),
        true,
        false,
    )
    .await
    .unwrap()
    .expect("verified email links, case-insensitively");
    assert!(linked.newly_linked);
    assert_eq!(linked.role, "student");

    let again = identity::resolve_subject(&pool, "auth0|new", None, false, false)
        .await
        .unwrap()
        .expect("linked subject resolves without an email");
    assert!(!again.newly_linked);

    let takeover = identity::resolve_subject(
        &pool,
        "auth0|attacker",
        Some("unlinked@acadex.test"),
        true,
        false,
    )
    .await
    .unwrap();
    assert!(
        takeover.is_none(),
        "an already-linked user can't be claimed by a second login"
    );
}

#[sqlx::test(migrator = "acadex_db::MIGRATOR")]
async fn audit_chain_verifies_and_detects_tampering(pool: PgPool) {
    load_fixture(&pool).await;
    for (user, role, action) in [
        (STUDENT_A, "student", "auth.login"),
        (FACULTY_F, "faculty", "auth.login"),
        (STUDENT_A, "student", "profile.viewed"),
    ] {
        let mut tx = as_user(&pool, user, role).await;
        let event = audit::Event {
            action,
            resource_type: "session",
            resource_id: None,
            outcome: audit::Outcome::Success,
            ip: Some("203.0.113.7".parse().unwrap()),
            user_agent: Some("test"),
            trace_id: None,
            metadata: json!({ "n": 1 }),
        };
        audit::record(&mut tx, &event).await.unwrap();
        tx.commit().await.unwrap();
    }

    let (checked, broken): (i64, Option<i64>) =
        sqlx::query_as("SELECT * FROM audit.verify_chain()")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((checked, broken), (3, None));

    // The actor came from the session, and the app can't rewrite history.
    let mut tx = as_user(&pool, STUDENT_A, "student").await;
    assert_eq!(
        count(&mut tx, "SELECT count(*) FROM audit.action_log").await,
        2,
        "A sees only A's rows"
    );
    let err = sqlx::query("UPDATE audit.action_log SET outcome = 'denied'")
        .execute(&mut *tx)
        .await
        .expect_err("app role has no UPDATE");
    assert!(err.to_string().contains("permission denied"), "{err}");
    drop(tx);

    // A superuser edit is still caught by the chain.
    sqlx::query("UPDATE audit.action_log SET action = 'auth.logout' WHERE id = 2")
        .execute(&pool)
        .await
        .unwrap();
    let (_, broken): (i64, Option<i64>) = sqlx::query_as("SELECT * FROM audit.verify_chain()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(broken, Some(2));
}
