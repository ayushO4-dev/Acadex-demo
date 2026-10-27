//! Term-end promotion: a program's students move up a semester together, its
//! final-semester students graduate, and detained students are held back.
//! `academic.promote_program` checks the caller manages the program.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Cohort {
    pub semester: i16,
    pub students: i64,
}

/// Enrolled, active students per semester.
pub async fn cohorts(conn: &mut PgConnection, program_id: i16) -> Result<Vec<Cohort>, sqlx::Error> {
    sqlx::query_as(
        "SELECT st.current_semester AS semester, count(*) AS students
         FROM people.students st JOIN identity.users u ON u.id = st.user_id
         WHERE st.program_id = $1 AND st.graduated_on IS NULL AND u.active
         GROUP BY st.current_semester ORDER BY st.current_semester",
    )
    .bind(program_id)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Candidate {
    pub id: Uuid,
    pub name: String,
    pub roll_no: String,
    pub semester: i16,
    /// Subjects (lectures or practicals) below the attendance threshold.
    pub shortages: i64,
}

/// Students with an attendance shortage this academic year: the usual
/// reason to hold someone back.
pub async fn hold_candidates(
    conn: &mut PgConnection,
    program_id: i16,
    academic_year: i16,
    threshold: f64,
) -> Result<Vec<Candidate>, sqlx::Error> {
    sqlx::query_as(
        "SELECT st.user_id AS id, u.display_name AS name, st.roll_no, st.current_semester AS semester,
                count(DISTINCT r.offering_id) AS shortages
         FROM academic.attendance_rollup r
         JOIN people.students st ON st.user_id = r.student_id
         JOIN identity.users u ON u.id = st.user_id
         JOIN academic.offerings o ON o.id = r.offering_id
         JOIN academic.subjects s ON s.id = o.subject_id AND s.semester = st.current_semester
         WHERE r.program_id = $1 AND r.academic_year = $2 AND r.conducted > 0
           AND 100.0 * r.attended / r.conducted < $3
           AND st.graduated_on IS NULL AND u.active
         GROUP BY st.user_id, u.display_name, st.roll_no, st.current_semester
         ORDER BY st.current_semester, st.roll_no",
    )
    .bind(program_id)
    .bind(academic_year)
    .bind(threshold)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Run {
    pub promoted: i32,
    pub graduated: i32,
    pub held: i32,
    pub promoted_by: Option<String>,
    pub promoted_at: DateTime<Utc>,
}

pub async fn last_run(
    conn: &mut PgConnection,
    program_id: i16,
) -> Result<Option<Run>, sqlx::Error> {
    sqlx::query_as(
        "SELECT p.promoted, p.graduated, p.held, u.display_name AS promoted_by, p.promoted_at
         FROM academic.promotions p LEFT JOIN identity.users u ON u.id = p.promoted_by
         WHERE p.program_id = $1 ORDER BY p.promoted_at DESC LIMIT 1",
    )
    .bind(program_id)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Outcome {
    pub promoted: i32,
    pub graduated: i32,
    pub held: i32,
}

pub async fn promote(
    conn: &mut PgConnection,
    program_id: i16,
    hold: &[Uuid],
) -> Result<Outcome, sqlx::Error> {
    sqlx::query_as("SELECT * FROM academic.promote_program($1, $2)")
        .bind(program_id)
        .bind(hold)
        .fetch_one(conn)
        .await
}

/// A program's semesters, if the caller manages it.
pub async fn programs(conn: &mut PgConnection) -> Result<Vec<(i16, String, i16)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, semesters FROM org.programs WHERE academic.may_manage_program(id) ORDER BY name",
    )
    .fetch_all(conn)
    .await
}
