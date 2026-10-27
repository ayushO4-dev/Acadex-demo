//! DPDP Act rights: consent for optional purposes, a copy of one's data,
//! and erasure that keeps statutory records (architecture.md §13).

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

/// The privacy notice consents are given under; bump when it changes.
pub const NOTICE_VERSION: &str = "2026-10";

pub const PURPOSES: [&str; 4] = [
    "email_notices",
    "placement_profile",
    "alumni_contact",
    "research_analytics",
];

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Consent {
    pub purpose: String,
    pub granted: bool,
    pub notice_version: String,
    pub recorded_at: DateTime<Utc>,
}

/// The acting user's current choice per purpose (`None` where never asked).
pub async fn current(conn: &mut PgConnection) -> Result<Vec<Consent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT DISTINCT ON (purpose) purpose, granted, notice_version, recorded_at
         FROM privacy.consents WHERE user_id = identity.current_user_id()
         ORDER BY purpose, id DESC",
    )
    .fetch_all(conn)
    .await
}

/// Every change, newest first: the record DPDP asks a data fiduciary to keep.
pub async fn history(conn: &mut PgConnection) -> Result<Vec<Consent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT purpose, granted, notice_version, recorded_at FROM privacy.consents
         WHERE user_id = identity.current_user_id() ORDER BY id DESC LIMIT 100",
    )
    .fetch_all(conn)
    .await
}

pub async fn record(
    conn: &mut PgConnection,
    purpose: &str,
    granted: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO privacy.consents (user_id, purpose, granted, notice_version)
         VALUES (identity.current_user_id(), $1, $2, $3)",
    )
    .bind(purpose)
    .bind(granted)
    .bind(NOTICE_VERSION)
    .execute(conn)
    .await?;
    Ok(())
}

// ── Erasure ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ErasureRequest {
    pub id: Uuid,
    pub student_id: Uuid,
    pub student: String,
    pub roll_no: String,
    pub reason: String,
    pub status: String,
    pub requested_at: DateTime<Utc>,
    pub decided_by: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub summary: Option<Value>,
}

macro_rules! erasure_sql {
    () => {
        "SELECT r.id, r.student_id, u.display_name AS student, st.roll_no, r.reason, r.status, r.requested_at,
                d.display_name AS decided_by, r.decided_at, r.decision_note, r.summary
         FROM privacy.erasure_requests r
         JOIN people.students st ON st.user_id = r.student_id
         JOIN identity.users u ON u.id = r.student_id
         LEFT JOIN identity.users d ON d.id = r.decided_by"
    };
}

/// The acting student's requests, or (administration) everyone's: open first.
pub async fn erasure_requests(conn: &mut PgConnection) -> Result<Vec<ErasureRequest>, sqlx::Error> {
    sqlx::query_as(concat!(
        erasure_sql!(),
        " ORDER BY r.status = 'requested' DESC, r.requested_at DESC LIMIT 200"
    ))
    .fetch_all(conn)
    .await
}

/// `None` if the student already has one open.
pub async fn request_erasure(
    conn: &mut PgConnection,
    reason: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO privacy.erasure_requests (student_id, reason) VALUES (identity.current_user_id(), $1)
         ON CONFLICT (student_id) WHERE status = 'requested' DO NOTHING RETURNING id",
    )
    .bind(reason)
    .fetch_optional(conn)
    .await
}

pub async fn withdraw_erasure(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE privacy.erasure_requests SET status = 'withdrawn'
         WHERE id = $1 AND student_id = identity.current_user_id() AND status = 'requested'",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// The open request's student, locked while it's decided.
pub async fn open_request(conn: &mut PgConnection, id: Uuid) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT student_id FROM privacy.erasure_requests WHERE id = $1 AND status = 'requested' FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

/// Erases the student's personal data (administration); returns the summary.
pub async fn erase(conn: &mut PgConnection, student: Uuid) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT privacy.erase_student($1, $2)")
        .bind(student)
        .bind(NOTICE_VERSION)
        .fetch_one(conn)
        .await
}

pub async fn close_request(
    conn: &mut PgConnection,
    id: Uuid,
    status: &str,
    note: Option<&str>,
    summary: Option<&Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE privacy.erasure_requests
         SET status = $2, decided_by = identity.current_user_id(), decided_at = now(), decision_note = $3, summary = $4
         WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(note)
    .bind(summary)
    .execute(conn)
    .await?;
    Ok(())
}

// ── Export ───────────────────────────────────────────────────────────────────

/// Everything Acadex holds about the acting student, as one JSON document,
/// read through row-level security as the student themselves. Sealed numbers
/// appear masked (their last four digits), as everywhere else.
pub async fn export(conn: &mut PgConnection) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "WITH me AS (SELECT identity.current_user_id() AS id)
         SELECT jsonb_build_object(
           'generated_at', now(),
           'privacy_notice_version', $1::text,
           'account', (SELECT jsonb_build_object('name', u.display_name, 'email', identity.my_email(), 'role', u.role)
                       FROM identity.users u, me WHERE u.id = me.id),
           'student', (SELECT jsonb_build_object('roll_no', s.roll_no, 'program', p.name, 'batch', s.batch_year,
                                                 'section', s.section, 'current_semester', s.current_semester)
                       FROM people.students s JOIN org.programs p ON p.id = s.program_id, me WHERE s.user_id = me.id),
           'admission', (SELECT to_jsonb(a) - 'student_id' FROM profile.admission a, me WHERE a.student_id = me.id),
           'personal', (SELECT to_jsonb(pi) - 'student_id' - 'aadhaar_sealed' - 'bank_account_sealed'
                        FROM profile.personal_info pi, me WHERE pi.student_id = me.id),
           'parents', (SELECT to_jsonb(x) - 'student_id' FROM profile.parent_info x, me WHERE x.student_id = me.id),
           'contact', (SELECT to_jsonb(x) - 'student_id' FROM profile.contact_info x, me WHERE x.student_id = me.id),
           'education', (SELECT coalesce(jsonb_agg(to_jsonb(e) - 'student_id' ORDER BY e.year), '[]')
                         FROM profile.education e, me WHERE e.student_id = me.id),
           'documents', (SELECT coalesce(jsonb_agg(jsonb_build_object('type', d.doc_type, 'description', d.description,
                                     'file', d.name, 'uploaded_at', d.uploaded_at, 'verification', d.verification)
                                     ORDER BY d.uploaded_at), '[]')
                         FROM profile.documents d, me WHERE d.student_id = me.id),
           'attendance', (SELECT coalesce(jsonb_agg(jsonb_build_object('academic_year', x.academic_year, 'subject', x.code,
                                     'part', x.session_kind, 'held', x.held, 'attended', x.attended)
                                     ORDER BY x.academic_year, x.code, x.session_kind), '[]')
                          FROM (SELECT a.academic_year, s.code, a.session_kind, count(*) AS held,
                                       count(*) FILTER (WHERE a.status IN ('present', 'late')) AS attended
                                FROM academic.attendance a
                                JOIN academic.offerings o ON o.id = a.offering_id
                                JOIN academic.subjects s ON s.id = o.subject_id, me
                                WHERE a.student_id = me.id
                                GROUP BY a.academic_year, s.code, a.session_kind) x),
           'results', (SELECT coalesce(jsonb_agg(jsonb_build_object('subject', s.code, 'name', s.name, 'semester', s.semester,
                                  'marks', m.marks, 'max_marks', m.max_marks) ORDER BY s.semester, s.code), '[]')
                       FROM exams.external_marks m
                       JOIN academic.offerings o ON o.id = m.offering_id
                       JOIN academic.subjects s ON s.id = o.subject_id, me
                       WHERE m.student_id = me.id),
           'fees', (SELECT coalesce(jsonb_agg(jsonb_build_object('date', e.created_at, 'kind', e.kind,
                               'particulars', coalesce(e.particulars, c.particulars), 'amount_paise', e.amount_paise,
                               'remarks', e.remarks) ORDER BY e.created_at, e.id), '[]')
                    FROM fees.ledger e LEFT JOIN fees.ledger c ON c.id = e.charge_id, me WHERE e.student_id = me.id),
           'receipts', (SELECT coalesce(jsonb_agg(jsonb_build_object('receipt_no', r.receipt_no, 'issued_at', r.issued_at,
                                   'amount_paise', r.amount_paise, 'remarks', r.remarks) ORDER BY r.issued_at), '[]')
                        FROM fees.receipts r, me WHERE r.student_id = me.id),
           'activities', (SELECT coalesce(jsonb_agg(jsonb_build_object('kind', a.kind, 'title', a.title,
                                     'description', a.description, 'date', a.occurred_on, 'status', a.status,
                                     'points', a.points) ORDER BY a.occurred_on), '[]')
                          FROM activities.activities a, me WHERE a.student_id = me.id),
           'support_tickets', (SELECT coalesce(jsonb_agg(jsonb_build_object('number', t.number, 'subject', t.subject,
                                     'status', t.status, 'created_at', t.created_at,
                                     'messages', (SELECT coalesce(jsonb_agg(jsonb_build_object('at', m.created_at,
                                                         'from_me', m.author_id = t.requester_id, 'body', m.body)
                                                         ORDER BY m.id), '[]')
                                                  FROM support.messages m WHERE m.ticket_id = t.id AND NOT m.internal))
                                     ORDER BY t.created_at), '[]')
                               FROM support.tickets t, me WHERE t.requester_id = me.id),
           'consents', (SELECT coalesce(jsonb_agg(jsonb_build_object('purpose', c.purpose, 'granted', c.granted,
                                   'notice_version', c.notice_version, 'at', c.recorded_at) ORDER BY c.id), '[]')
                        FROM privacy.consents c, me WHERE c.user_id = me.id)
         )",
    )
    .bind(NOTICE_VERSION)
    .fetch_one(conn)
    .await
}
