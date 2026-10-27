use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SubjectAttendance {
    pub code: String,
    pub name: String,
    pub credits: i16,
    pub conducted: i64,
    /// Present or late. Excused sessions count as missed for the shortage rule
    /// until the condonation policy is decided (plan.md §13.3).
    pub attended: i64,
    /// The same, for lectures and for practicals (logged separately).
    pub theory_conducted: i64,
    pub theory_attended: i64,
    pub practical_conducted: i64,
    pub practical_attended: i64,
}

/// The query behind "my attendance this semester" (or year, with no semester).
/// Filtering on `program_id` and `academic_year` lets the planner prune to one
/// partition; RLS decides which of that partition's rows the acting user may see.
pub const SUMMARY_SQL: &str = "
    SELECT sub.code, sub.name, sub.credits,
           count(*) AS conducted,
           count(*) FILTER (WHERE a.status IN ('present', 'late')) AS attended,
           count(*) FILTER (WHERE a.session_kind = 'theory') AS theory_conducted,
           count(*) FILTER (WHERE a.session_kind = 'theory' AND a.status IN ('present', 'late')) AS theory_attended,
           count(*) FILTER (WHERE a.session_kind = 'practical') AS practical_conducted,
           count(*) FILTER (WHERE a.session_kind = 'practical' AND a.status IN ('present', 'late')) AS practical_attended
    FROM academic.attendance a
    JOIN academic.offerings o ON o.id = a.offering_id
    JOIN academic.subjects sub ON sub.id = o.subject_id
    WHERE a.program_id = $1 AND a.academic_year = $2 AND a.student_id = $3
      AND ($4::smallint IS NULL OR sub.semester = $4)
    GROUP BY sub.code, sub.name, sub.credits
    ORDER BY sub.code";

#[tracing::instrument(name = "db.attendance_summary", skip_all, fields(otel.kind = "client"))]
pub async fn summary(
    conn: &mut PgConnection,
    program_id: i16,
    academic_year: i16,
    student_id: Uuid,
    semester: Option<i16>,
) -> Result<Vec<SubjectAttendance>, sqlx::Error> {
    sqlx::query_as(SUMMARY_SQL)
        .bind(program_id)
        .bind(academic_year)
        .bind(student_id)
        .bind(semester)
        .fetch_all(conn)
        .await
}

/// The day of a student's last recorded session in a semester (or year), so
/// a past semester's calendar opens where its attendance ends.
pub async fn last_session_on(
    conn: &mut PgConnection,
    program_id: i16,
    academic_year: i16,
    student_id: Uuid,
    semester: Option<i16>,
) -> Result<Option<chrono::NaiveDate>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT max(a.session_date) FROM academic.attendance a
         JOIN academic.offerings o ON o.id = a.offering_id
         JOIN academic.subjects sub ON sub.id = o.subject_id
         WHERE a.program_id = $1 AND a.academic_year = $2 AND a.student_id = $3
           AND ($4::smallint IS NULL OR sub.semester = $4)",
    )
    .bind(program_id)
    .bind(academic_year)
    .bind(student_id)
    .bind(semester)
    .fetch_one(conn)
    .await
}
