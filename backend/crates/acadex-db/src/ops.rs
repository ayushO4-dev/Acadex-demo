//! Operations: failed deliveries and the dead-letter store (architecture.md
//! §11.3), and the scheduled jobs that need more than the app role may do.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

/// Failures after which a message is parked instead of retried.
pub const MAX_ATTEMPTS: i32 = 5;

/// Counts a failed delivery; returns the attempts so far, this one included.
pub async fn record_failure(
    conn: &mut PgConnection,
    message_id: &str,
    error: &str,
) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO ops.delivery_failures (message_id, attempts, last_error) VALUES ($1, 1, $2)
         ON CONFLICT (message_id) DO UPDATE
             SET attempts = ops.delivery_failures.attempts + 1, last_error = $2, last_at = now()
         RETURNING attempts",
    )
    .bind(message_id)
    .bind(error.chars().take(1000).collect::<String>())
    .fetch_one(conn)
    .await
}

/// A delivery succeeded: forget its earlier failures.
pub async fn clear_failures(conn: &mut PgConnection, message_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM ops.delivery_failures WHERE message_id = $1")
        .bind(message_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Parks a message that has failed too often; `false` if already parked.
pub async fn park(
    conn: &mut PgConnection,
    message_id: &str,
    event: &str,
    payload: &Value,
    attributes: &Value,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO ops.dead_letters (message_id, event, payload, attributes, attempts, last_error, first_failed_at)
         SELECT $1, $2, $3, $4, f.attempts, f.last_error, f.first_at
         FROM ops.delivery_failures f WHERE f.message_id = $1
         ON CONFLICT (message_id) DO NOTHING",
    )
    .bind(message_id)
    .bind(event)
    .bind(payload)
    .bind(attributes)
    .execute(&mut *conn)
    .await?;
    clear_failures(conn, message_id).await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DeadLetter {
    pub id: i64,
    pub message_id: String,
    pub event: String,
    pub payload: Value,
    pub attempts: i32,
    pub last_error: String,
    pub first_failed_at: DateTime<Utc>,
    pub dead_at: DateTime<Utc>,
    pub status: String,
    pub handled_by: Option<String>,
    pub handled_at: Option<DateTime<Utc>>,
    pub note: Option<String>,
}

/// Waiting ones first, then the latest handled.
pub async fn dead_letters(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<DeadLetter>, sqlx::Error> {
    sqlx::query_as(
        "SELECT d.id, d.message_id, d.event, d.payload, d.attempts, d.last_error, d.first_failed_at, d.dead_at,
                d.status, u.display_name AS handled_by, d.handled_at, d.note
         FROM ops.dead_letters d LEFT JOIN identity.users u ON u.id = d.handled_by
         ORDER BY d.status = 'dead' DESC, d.dead_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// Puts a parked message's event back in the outbox; `None` if there's no
/// such message waiting.
pub async fn replay(
    conn: &mut PgConnection,
    id: i64,
    note: Option<&str>,
) -> Result<Option<String>, sqlx::Error> {
    let event: Option<(String, Value)> = sqlx::query_as(
        "UPDATE ops.dead_letters
         SET status = 'replayed', handled_by = identity.current_user_id(), handled_at = now(), note = $2
         WHERE id = $1 AND status = 'dead' RETURNING event, payload",
    )
    .bind(id)
    .bind(note)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((event, payload)) = event else {
        return Ok(None);
    };
    crate::outbox::enqueue(conn, &event, None, &payload).await?;
    Ok(Some(event))
}

pub async fn discard(conn: &mut PgConnection, id: i64, note: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE ops.dead_letters
         SET status = 'discarded', handled_by = identity.current_user_id(), handled_at = now(), note = $2
         WHERE id = $1 AND status = 'dead'",
    )
    .bind(id)
    .bind(note)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn waiting_dead_letters(conn: &mut PgConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM ops.dead_letters WHERE status = 'dead'")
        .fetch_one(conn)
        .await
}

// ── audit.verify_chain ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ChainVerification {
    pub id: i64,
    pub verified_at: DateTime<Utc>,
    pub rows_checked: i64,
    pub head_id: Option<i64>,
    pub first_broken_id: Option<i64>,
    pub detail: Option<String>,
}

/// Walks the hash chain and records the result (worker only).
pub async fn verify_audit_chain(conn: &mut PgConnection) -> Result<ChainVerification, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, verified_at, rows_checked, head_id, first_broken_id, detail FROM audit.verify_and_checkpoint()",
    )
    .fetch_one(conn)
    .await
}

pub async fn last_verification(
    conn: &mut PgConnection,
) -> Result<Option<ChainVerification>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, verified_at, rows_checked, head_id, first_broken_id, detail
         FROM audit.chain_verifications ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(conn)
    .await
}

// ── partition.maintain, attendance.rollup ────────────────────────────────────

/// Creates any missing attendance partitions for these academic years;
/// returns the ones created (worker only).
pub async fn maintain_partitions(
    conn: &mut PgConnection,
    years: &[i32],
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT academic.maintain_partitions($1)")
        .bind(years)
        .fetch_one(conn)
        .await
}

/// Every (program, academic year) with attendance in these years.
pub async fn attendance_partitions(
    conn: &mut PgConnection,
    years: &[i16],
) -> Result<Vec<(i16, i16)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, y FROM org.programs CROSS JOIN unnest($1::smallint[]) AS y ORDER BY id, y",
    )
    .bind(years)
    .fetch_all(conn)
    .await
}

/// Recomputes the rollup for one program's academic year (worker only).
pub async fn refresh_rollup(
    conn: &mut PgConnection,
    program_id: i16,
    academic_year: i16,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT academic.refresh_attendance_rollup($1, $2)")
        .bind(i32::from(program_id))
        .bind(i32::from(academic_year))
        .fetch_one(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Shortage {
    pub student_id: Uuid,
    pub student: String,
    pub roll_no: String,
    pub program: String,
    pub semester: i16,
    pub subject_code: String,
    pub subject: String,
    pub session_kind: String,
    pub conducted: i32,
    pub attended: i32,
    pub pct: f64,
    pub refreshed_at: DateTime<Utc>,
}

/// Students below `threshold` percent in a subject component, from the
/// rollup; row-level security narrows an HOD to their department.
pub async fn shortages(
    conn: &mut PgConnection,
    academic_year: i16,
    threshold: f64,
    program_id: Option<i16>,
    semester: Option<i16>,
) -> Result<Vec<Shortage>, sqlx::Error> {
    sqlx::query_as(
        "SELECT r.student_id, u.display_name AS student, st.roll_no, p.code AS program, s.semester,
                s.code AS subject_code, s.name AS subject, r.session_kind, r.conducted, r.attended,
                round(100.0 * r.attended / r.conducted, 1)::float8 AS pct, r.refreshed_at
         FROM academic.attendance_rollup r
         JOIN people.students st ON st.user_id = r.student_id
         JOIN identity.users u ON u.id = r.student_id
         JOIN org.programs p ON p.id = r.program_id
         JOIN academic.offerings o ON o.id = r.offering_id
         JOIN academic.subjects s ON s.id = o.subject_id
         WHERE r.academic_year = $1 AND r.conducted > 0
           AND 100.0 * r.attended / r.conducted < $2
           AND ($3::smallint IS NULL OR r.program_id = $3)
           AND ($4::smallint IS NULL OR s.semester = $4)
         ORDER BY p.code, st.roll_no, s.code, r.session_kind",
    )
    .bind(academic_year)
    .bind(threshold)
    .bind(program_id)
    .bind(semester)
    .fetch_all(conn)
    .await
}

/// Pending outbox events and the oldest one's age in seconds, read in the
/// dispatcher's context (the only one that sees pending rows).
pub async fn outbox_backlog(pool: &sqlx::PgPool) -> Result<(i64, Option<f64>), sqlx::Error> {
    use crate::rls::{self, System};
    let mut tx = rls::begin_system(pool, System::Dispatcher).await?;
    let backlog = sqlx::query_as(
        "SELECT count(*), extract(epoch FROM now() - min(created_at))::float8
         FROM ops.outbox WHERE published_at IS NULL",
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(backlog)
}
