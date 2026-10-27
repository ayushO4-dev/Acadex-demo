//! Re-evaluation and re-total requests (architecture.md §9.3): a student
//! appeals a published result inside the appeal window and pays per subject;
//! the exam cell decides, and a changed mark is published as a new attempt in
//! a re-evaluation result set, with the usual two-person approval.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

/// A published result the student may still appeal.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Appealable {
    pub offering_id: Uuid,
    pub attempt: i16,
    pub code: String,
    pub name: String,
    pub semester: i16,
    /// regular | backlog: the result set it came from.
    pub result_kind: String,
    pub external: f64,
    pub external_max: i16,
    pub published_at: DateTime<Utc>,
    pub closes_at: DateTime<Utc>,
}

/// The caller's published, latest, not-yet-appealed results whose window is
/// open. Read as the student, so RLS limits it to their own.
pub async fn appealable(
    conn: &mut PgConnection,
    student: Uuid,
    window_days: i16,
) -> Result<Vec<Appealable>, sqlx::Error> {
    sqlx::query_as(
        "SELECT em.offering_id, em.attempt, s.code, s.name, s.semester, rp.kind AS result_kind,
                em.marks::float8 AS external, em.max_marks AS external_max, rp.published_at,
                rp.published_at + make_interval(days => $2::int) AS closes_at
         FROM exams.external_marks em
         JOIN exams.result_publications rp ON rp.id = em.publication_id
         JOIN academic.offerings o ON o.id = em.offering_id
         JOIN academic.subjects s ON s.id = o.subject_id
         WHERE em.student_id = $1
           AND rp.kind IN ('regular', 'backlog') AND rp.published_at IS NOT NULL
           AND rp.published_at + make_interval(days => $2::int) >= now()
           AND NOT EXISTS (SELECT 1 FROM exams.external_marks later
                           WHERE later.student_id = em.student_id AND later.offering_id = em.offering_id
                             AND later.attempt > em.attempt)
           AND NOT EXISTS (SELECT 1 FROM exams.revaluation_requests r
                           WHERE r.student_id = em.student_id AND r.offering_id = em.offering_id
                             AND r.attempt = em.attempt)
         ORDER BY s.semester, s.code",
    )
    .bind(student)
    .bind(i32::from(window_days))
    .fetch_all(conn)
    .await
}

#[derive(Debug, sqlx::FromRow)]
pub struct Created {
    pub request_id: Uuid,
    pub status: String,
    pub charge_id: Option<i64>,
}

/// Files a request as the acting student; the function enforces the rules.
pub async fn create(
    conn: &mut PgConnection,
    offering: Uuid,
    attempt: i16,
    kind: &str,
    reason: &str,
) -> Result<Created, sqlx::Error> {
    sqlx::query_as("SELECT * FROM exams.request_revaluation($1, $2, $3, $4)")
        .bind(offering)
        .bind(attempt)
        .bind(kind)
        .bind(reason)
        .fetch_one(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Request {
    pub id: Uuid,
    pub student_id: Uuid,
    pub student_name: String,
    pub roll_no: String,
    pub program_id: i16,
    pub program_name: String,
    pub offering_id: Uuid,
    pub attempt: i16,
    pub code: String,
    pub name: String,
    pub semester: i16,
    /// The academic year of the result appealed.
    pub academic_year: i16,
    /// revaluation | retotal
    pub kind: String,
    pub reason: String,
    pub fee_paise: i64,
    pub charge_id: Option<i64>,
    /// Still owed on the fee: payment_pending requests can be paid.
    pub pending_paise: i64,
    pub status: String,
    pub old_marks: f64,
    pub external_max: i16,
    pub new_marks: Option<f64>,
    pub remarks: Option<String>,
    pub publication_id: Option<Uuid>,
    /// When the changed mark was published; `None` while staged or unchanged.
    pub outcome_published_at: Option<DateTime<Utc>>,
    pub decided_by: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

macro_rules! request_sql {
    () => {
        "SELECT r.id, r.student_id, u.display_name AS student_name, st.roll_no, p.id AS program_id,
                p.name AS program_name, r.offering_id, r.attempt, s.code, s.name, s.semester,
                rp.academic_year, r.kind, r.reason, r.fee_paise, r.charge_id,
                CASE WHEN r.charge_id IS NULL THEN 0 ELSE greatest(fees.pending_of(r.charge_id), 0) END AS pending_paise,
                r.status, r.old_marks::float8 AS old_marks, em.max_marks AS external_max,
                r.new_marks::float8 AS new_marks, r.remarks, r.publication_id,
                out.published_at AS outcome_published_at, d.display_name AS decided_by, r.decided_at,
                r.submitted_at, r.created_at
         FROM exams.revaluation_requests r
         JOIN exams.external_marks em ON em.student_id = r.student_id AND em.offering_id = r.offering_id
                                     AND em.attempt = r.attempt
         JOIN exams.result_publications rp ON rp.id = em.publication_id
         JOIN people.students st ON st.user_id = r.student_id
         JOIN identity.users u ON u.id = r.student_id
         JOIN org.programs p ON p.id = st.program_id
         JOIN academic.offerings o ON o.id = r.offering_id
         JOIN academic.subjects s ON s.id = o.subject_id
         LEFT JOIN exams.result_publications out ON out.id = r.publication_id
         LEFT JOIN identity.users d ON d.id = r.decided_by"
    };
}

/// Requests the caller may see (RLS: a student's own, or all for the exam
/// cell): undecided first, oldest submission first, then the latest decided.
pub async fn list(conn: &mut PgConnection, limit: i64) -> Result<Vec<Request>, sqlx::Error> {
    sqlx::query_as(concat!(
        request_sql!(),
        " ORDER BY r.status = 'decided', r.status = 'payment_pending', r.submitted_at, r.decided_at DESC NULLS LAST
          LIMIT $1"
    ))
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn get(conn: &mut PgConnection, id: Uuid) -> Result<Option<Request>, sqlx::Error> {
    sqlx::query_as(concat!(request_sql!(), " WHERE r.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// As the gateway: the student's requests whose fee is now paid.
pub async fn complete_paid(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT exams.complete_paid_revaluations($1)")
        .bind(student)
        .fetch_all(conn)
        .await
}

/// The staged, not-yet-requested re-evaluation set for a program's semester
/// and year, opened if there isn't one. A set already asked to be published
/// is left alone; later decisions go in a new one.
pub async fn open_set(
    conn: &mut PgConnection,
    program: i16,
    semester: i16,
    academic_year: i16,
) -> Result<Uuid, sqlx::Error> {
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM exams.result_publications
         WHERE program_id = $1 AND semester = $2 AND academic_year = $3 AND kind = 'revaluation'
           AND published_at IS NULL AND requested_by IS NULL
         ORDER BY staged_at LIMIT 1",
    )
    .bind(program)
    .bind(semester)
    .bind(academic_year)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(id) = existing {
        return Ok(id);
    }
    sqlx::query_scalar(
        "INSERT INTO exams.result_publications (program_id, semester, academic_year, kind)
         VALUES ($1, $2, $3, 'revaluation') RETURNING id",
    )
    .bind(program)
    .bind(semester)
    .bind(academic_year)
    .fetch_one(conn)
    .await
}

/// Puts a changed mark into a staged set as the subject's next attempt.
pub async fn stage_mark(
    conn: &mut PgConnection,
    set: Uuid,
    request: &Request,
    marks: f64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO exams.external_marks (student_id, offering_id, publication_id, marks, max_marks, attempt)
         SELECT $1, $2, $3, $4::numeric, $5,
                (SELECT max(attempt) + 1 FROM exams.external_marks WHERE student_id = $1 AND offering_id = $2)",
    )
    .bind(request.student_id)
    .bind(request.offering_id)
    .bind(set)
    .bind(marks)
    .bind(request.external_max)
    .execute(conn)
    .await?;
    Ok(())
}

/// Records the decision as the acting exam-cell user. `false` if it was
/// already decided (RLS only lets submitted requests change).
pub async fn decide(
    conn: &mut PgConnection,
    id: Uuid,
    new_marks: f64,
    publication: Option<Uuid>,
    remarks: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE exams.revaluation_requests
         SET status = 'decided', new_marks = $2::numeric, publication_id = $3, remarks = $4,
             decided_by = identity.current_user_id(), decided_at = now()
         WHERE id = $1 AND status = 'submitted'",
    )
    .bind(id)
    .bind(new_marks)
    .bind(publication)
    .bind(remarks)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}
