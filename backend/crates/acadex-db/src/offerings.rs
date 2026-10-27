//! Offerings and attendance marking. Run inside an RLS transaction, so a
//! faculty member only ever reaches their own offerings.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Offering {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub academic_year: i16,
    pub program_id: i16,
    pub program_name: String,
    pub semester: i16,
    pub student_count: i64,
    /// theory | lab | elective
    pub kind: String,
    /// Theory subjects with a practical component, marked separately.
    pub has_practical: bool,
}

/// The acting faculty member's offerings in one academic year.
pub async fn taught_by_me(
    conn: &mut PgConnection,
    academic_year: i16,
) -> Result<Vec<Offering>, sqlx::Error> {
    sqlx::query_as(
        "SELECT o.id, s.code, s.name, o.academic_year, p.id AS program_id, p.name AS program_name, s.semester, s.kind::text AS kind, s.has_practical,
                (SELECT count(*) FROM people.students st
                 WHERE st.program_id = s.program_id AND st.current_semester = s.semester AND st.graduated_on IS NULL AND academic.takes(st.user_id, s.id)) AS student_count
         FROM academic.offerings o
         JOIN academic.subjects s ON s.id = o.subject_id
         JOIN org.programs p ON p.id = s.program_id
         WHERE o.faculty_id = identity.current_user_id() AND o.academic_year = $1
         ORDER BY s.code",
    )
    .bind(academic_year)
    .fetch_all(conn)
    .await
}

/// The kind a session was marked as (theory or practical), if it has been.
pub async fn session_kind(
    conn: &mut PgConnection,
    offering_id: Uuid,
    session_date: NaiveDate,
    slot: i16,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT a.session_kind FROM academic.attendance a
         JOIN academic.offerings o ON o.id = a.offering_id
         JOIN academic.subjects s ON s.id = o.subject_id
         WHERE a.program_id = s.program_id AND a.academic_year = o.academic_year
           AND a.offering_id = $1 AND a.session_date = $2 AND a.slot = $3
         LIMIT 1",
    )
    .bind(offering_id)
    .bind(session_date)
    .bind(slot)
    .fetch_optional(conn)
    .await
}

/// One of the acting faculty member's offerings, or `None`.
pub async fn mine(conn: &mut PgConnection, id: Uuid) -> Result<Option<Offering>, sqlx::Error> {
    sqlx::query_as(
        "SELECT o.id, s.code, s.name, o.academic_year, p.id AS program_id, p.name AS program_name, s.semester, s.kind::text AS kind, s.has_practical,
                (SELECT count(*) FROM people.students st
                 WHERE st.program_id = s.program_id AND st.current_semester = s.semester AND st.graduated_on IS NULL AND academic.takes(st.user_id, s.id)) AS student_count
         FROM academic.offerings o
         JOIN academic.subjects s ON s.id = o.subject_id
         JOIN org.programs p ON p.id = s.program_id
         WHERE o.id = $1 AND o.faculty_id = identity.current_user_id()",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttendanceStatus {
    Present,
    Absent,
    Late,
    Excused,
}

impl AttendanceStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
            Self::Late => "late",
            Self::Excused => "excused",
        }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RosterEntry {
    pub student_id: Uuid,
    pub roll_no: String,
    pub display_name: String,
    /// `None` until marked for this session.
    pub status: Option<String>,
}

/// The students who take an offering — its program, in the subject's
/// semester — with whatever is already marked for one session.
pub async fn roster(
    conn: &mut PgConnection,
    offering_id: Uuid,
    session_date: NaiveDate,
    slot: i16,
) -> Result<Vec<RosterEntry>, sqlx::Error> {
    sqlx::query_as(
        "SELECT st.user_id AS student_id, st.roll_no, u.display_name, a.status::text AS status
         FROM academic.offerings o
         JOIN academic.subjects s ON s.id = o.subject_id
         JOIN people.students st ON st.program_id = s.program_id AND st.current_semester = s.semester AND st.graduated_on IS NULL AND academic.takes(st.user_id, s.id)
         JOIN identity.users u ON u.id = st.user_id
         LEFT JOIN academic.attendance a
                ON a.program_id = s.program_id AND a.academic_year = o.academic_year
               AND a.student_id = st.user_id AND a.offering_id = o.id
               AND a.session_date = $2 AND a.slot = $3
         WHERE o.id = $1 AND o.faculty_id = identity.current_user_id()
         ORDER BY st.roll_no",
    )
    .bind(offering_id)
    .bind(session_date)
    .bind(slot)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Deserialize)]
pub struct Mark {
    pub student_id: Uuid,
    pub status: AttendanceStatus,
}

/// Records one session's attendance, overwriting earlier marks for the same
/// session (so saving twice is harmless). Students not on the roster are
/// skipped; the returned count lets the caller notice and roll back.
pub async fn mark(
    conn: &mut PgConnection,
    offering_id: Uuid,
    session_date: NaiveDate,
    slot: i16,
    session_kind: &str,
    marks: &[Mark],
) -> Result<u64, sqlx::Error> {
    let students: Vec<Uuid> = marks.iter().map(|m| m.student_id).collect();
    let statuses: Vec<&str> = marks.iter().map(|m| m.status.as_str()).collect();
    let result = sqlx::query(
        "INSERT INTO academic.attendance
             (program_id, academic_year, student_id, offering_id, session_date, slot, status, marked_by, session_kind)
         SELECT s.program_id, o.academic_year, m.student_id, o.id, $2, $3,
                m.status::academic.attendance_status, identity.current_user_id(), $6
         FROM academic.offerings o
         JOIN academic.subjects s ON s.id = o.subject_id
         CROSS JOIN unnest($4::uuid[], $5::text[]) AS m(student_id, status)
         JOIN people.students st
           ON st.user_id = m.student_id AND st.program_id = s.program_id AND st.current_semester = s.semester AND st.graduated_on IS NULL AND academic.takes(st.user_id, s.id)
         WHERE o.id = $1
         ON CONFLICT (program_id, academic_year, student_id, offering_id, session_date, slot)
         DO UPDATE SET status = EXCLUDED.status, marked_by = EXCLUDED.marked_by, marked_at = now(),
                       session_kind = EXCLUDED.session_kind",
    )
    .bind(offering_id)
    .bind(session_date)
    .bind(slot)
    .bind(&students)
    .bind(&statuses)
    .bind(session_kind)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}
