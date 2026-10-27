//! MOOCs (architecture.md §9.1): a catalogue, enrolment, certificate upload,
//! and credit transfer approved by the student's HOD.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Course {
    pub id: Uuid,
    pub provider: String,
    pub code: String,
    pub title: String,
    pub weeks: i16,
    pub credits: i16,
    pub url: Option<String>,
    pub starts_on: Option<NaiveDate>,
    /// Retired courses are listed for managers only.
    pub active: bool,
    /// Anyone has enrolled: its weeks and credits are fixed.
    pub enrolled: i64,
    /// The acting student's enrolment, if any.
    pub enrolment_id: Option<Uuid>,
    pub enrolment_status: Option<String>,
}

/// The catalogue; with `retired`, also courses no longer offered (managers).
pub async fn catalogue(conn: &mut PgConnection, retired: bool) -> Result<Vec<Course>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.id, c.provider, c.code, c.title, c.weeks, c.credits, c.url, c.starts_on, c.active,
                academic.mooc_enrolled(c.id) AS enrolled,
                e.id AS enrolment_id, e.status AS enrolment_status
         FROM academic.mooc_courses c
         LEFT JOIN academic.mooc_enrolments e
                ON e.course_id = c.id AND e.student_id = identity.current_user_id()
         WHERE c.active OR $1
         ORDER BY c.active DESC, c.provider, c.title",
    )
    .bind(retired)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Deserialize)]
pub struct CourseEdit {
    pub title: String,
    pub weeks: i16,
    pub credits: i16,
    pub url: Option<String>,
    pub starts_on: Option<NaiveDate>,
}

pub enum EditCourse {
    Saved,
    NotFound,
    /// Students have enrolled: weeks and credits stay.
    Enrolled,
}

pub async fn edit_course(
    conn: &mut PgConnection,
    id: Uuid,
    edit: &CourseEdit,
) -> Result<EditCourse, sqlx::Error> {
    let current: Option<(i16, i16, i64)> = sqlx::query_as(
        "SELECT weeks, credits, academic.mooc_enrolled(id) FROM academic.mooc_courses WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((weeks, credits, enrolled)) = current else {
        return Ok(EditCourse::NotFound);
    };
    if enrolled > 0 && (weeks != edit.weeks || credits != edit.credits) {
        return Ok(EditCourse::Enrolled);
    }
    sqlx::query(
        "UPDATE academic.mooc_courses SET title = $2, weeks = $3, credits = $4, url = $5, starts_on = $6 WHERE id = $1",
    )
    .bind(id)
    .bind(&edit.title)
    .bind(edit.weeks)
    .bind(edit.credits)
    .bind(edit.url.as_deref())
    .bind(edit.starts_on)
    .execute(conn)
    .await?;
    Ok(EditCourse::Saved)
}

/// Retires (no new enrolments; existing ones carry on) or restores a course.
pub async fn set_course_active(
    conn: &mut PgConnection,
    id: Uuid,
    active: bool,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE academic.mooc_courses SET active = $2 WHERE id = $1")
        .bind(id)
        .bind(active)
        .execute(conn)
        .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Deserialize)]
pub struct NewCourse {
    pub provider: String,
    pub code: String,
    pub title: String,
    pub weeks: i16,
    pub credits: i16,
    pub url: Option<String>,
    pub starts_on: Option<NaiveDate>,
}

pub async fn add_course(conn: &mut PgConnection, course: &NewCourse) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO academic.mooc_courses (institution_id, provider, code, title, weeks, credits, url, starts_on)
         SELECT u.institution_id, $1, $2, $3, $4, $5, $6, $7
         FROM identity.users u WHERE u.id = identity.current_user_id()
         RETURNING id",
    )
    .bind(&course.provider)
    .bind(&course.code)
    .bind(&course.title)
    .bind(course.weeks)
    .bind(course.credits)
    .bind(course.url.as_deref())
    .bind(course.starts_on)
    .fetch_one(conn)
    .await
}

/// Enrols the acting student. `None` if already enrolled.
pub async fn enrol(conn: &mut PgConnection, course_id: Uuid) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO academic.mooc_enrolments (course_id, student_id)
         SELECT id, identity.current_user_id() FROM academic.mooc_courses WHERE id = $1 AND active
         ON CONFLICT (course_id, student_id) DO NOTHING
         RETURNING id",
    )
    .bind(course_id)
    .fetch_optional(conn)
    .await
}

/// Attaches the certificate and asks for credit, on the acting student's own
/// enrolment that hasn't been decided. False otherwise.
pub async fn request_credit(
    conn: &mut PgConnection,
    enrolment_id: Uuid,
    object: &str,
    file_name: &str,
    score_pct: Option<f64>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE academic.mooc_enrolments
         SET certificate_object = $2, certificate_name = $3, score_pct = $4,
             status = 'credit_requested', requested_at = now()
         WHERE id = $1 AND student_id = identity.current_user_id() AND status IN ('enrolled', 'credit_requested')",
    )
    .bind(enrolment_id)
    .bind(object)
    .bind(file_name)
    .bind(score_pct)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Enrolment {
    pub id: Uuid,
    pub student_id: Uuid,
    pub student_name: String,
    pub roll_no: String,
    pub provider: String,
    pub course_code: String,
    pub course_title: String,
    pub credits: i16,
    pub status: String,
    pub certificate_name: Option<String>,
    pub score_pct: Option<f64>,
    pub enrolled_at: DateTime<Utc>,
    pub requested_at: Option<DateTime<Utc>>,
    pub decided_by: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
}

macro_rules! enrolment_sql {
    () => {
        "SELECT e.id, e.student_id, u.display_name AS student_name, st.roll_no, c.provider,
                c.code AS course_code, c.title AS course_title, c.credits, e.status, e.certificate_name,
                e.score_pct::float8 AS score_pct, e.enrolled_at, e.requested_at,
                d.display_name AS decided_by, e.decided_at, e.decision_note
         FROM academic.mooc_enrolments e
         JOIN academic.mooc_courses c ON c.id = e.course_id
         JOIN identity.users u ON u.id = e.student_id
         JOIN people.students st ON st.user_id = e.student_id
         LEFT JOIN identity.users d ON d.id = e.decided_by"
    };
}

/// Enrolments the caller may see: their own (student), their department's
/// (HOD), all (exam cell, admin). Requests awaiting a decision first.
pub async fn enrolments(conn: &mut PgConnection) -> Result<Vec<Enrolment>, sqlx::Error> {
    sqlx::query_as(concat!(
        enrolment_sql!(),
        " ORDER BY e.status <> 'credit_requested', coalesce(e.requested_at, e.enrolled_at) DESC LIMIT 200"
    ))
    .fetch_all(conn)
    .await
}

/// Approved MOOC credits for one student (they count towards their record).
pub async fn approved_for(
    conn: &mut PgConnection,
    student_id: Uuid,
) -> Result<Vec<Enrolment>, sqlx::Error> {
    sqlx::query_as(concat!(
        enrolment_sql!(),
        " WHERE e.student_id = $1 AND e.status = 'approved' ORDER BY e.decided_at"
    ))
    .bind(student_id)
    .fetch_all(conn)
    .await
}

/// The stored certificate of an enrolment the caller may see.
pub async fn certificate(
    conn: &mut PgConnection,
    enrolment_id: Uuid,
) -> Result<Option<(String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT certificate_object, coalesce(certificate_name, 'certificate')
         FROM academic.mooc_enrolments WHERE id = $1 AND certificate_object IS NOT NULL",
    )
    .bind(enrolment_id)
    .fetch_optional(conn)
    .await
}

/// Approves or rejects a credit request as the acting HOD. Returns the
/// student, or `None` if it isn't awaiting a decision in their department.
pub async fn decide(
    conn: &mut PgConnection,
    enrolment_id: Uuid,
    approve: bool,
    note: Option<&str>,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE academic.mooc_enrolments
         SET status = $2, decided_by = identity.current_user_id(), decided_at = now(), decision_note = $3
         WHERE id = $1 AND status = 'credit_requested'
         RETURNING student_id",
    )
    .bind(enrolment_id)
    .bind(if approve { "approved" } else { "rejected" })
    .bind(note)
    .fetch_optional(conn)
    .await
}
