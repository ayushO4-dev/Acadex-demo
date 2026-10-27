//! Academic (architecture.md §9.1): subjects, the calendar, attendance detail
//! and corrections, course status, and finding students. Every query runs
//! under the caller's row-level security.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

// ── Subjects ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Subject {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub credits: i16,
    pub kind: String,
    /// Lectures with a practical component, attended and counted separately.
    pub has_practical: bool,
    pub semester: i16,
    /// The offering shown: the one in `preferred_year` if it exists, else the latest.
    pub academic_year: Option<i16>,
    pub faculty: Option<String>,
}

pub async fn subjects(
    conn: &mut PgConnection,
    program_id: i16,
    semester: i16,
    preferred_year: Option<i16>,
) -> Result<Vec<Subject>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id, s.code, s.name, s.credits, s.kind::text AS kind, s.has_practical, s.semester,
                o.academic_year, u.display_name AS faculty
         FROM academic.subjects s
         LEFT JOIN LATERAL (SELECT academic_year, faculty_id FROM academic.offerings o
                            WHERE o.subject_id = s.id
                            ORDER BY (o.academic_year = $3) DESC, o.academic_year DESC LIMIT 1) o ON true
         LEFT JOIN identity.users u ON u.id = o.faculty_id
         WHERE s.program_id = $1 AND s.semester = $2
           -- A retired subject still shows for the year it was taught.
           AND (s.active OR o.academic_year = $3)
         ORDER BY s.code",
    )
    .bind(program_id)
    .bind(semester)
    .bind(preferred_year)
    .fetch_all(conn)
    .await
}

// ── Calendar ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CalendarEvent {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub description: Option<String>,
    pub starts_on: NaiveDate,
    pub ends_on: NaiveDate,
    /// `None` for institution-wide events.
    pub program_id: Option<i16>,
    pub program_name: Option<String>,
}

/// Events overlapping `[from, to]` that the caller may see.
pub async fn calendar(
    conn: &mut PgConnection,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<CalendarEvent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT e.id, e.kind::text AS kind, e.title, e.description, e.starts_on, e.ends_on,
                e.program_id, p.name AS program_name
         FROM academic.calendar_events e
         LEFT JOIN org.programs p ON p.id = e.program_id
         WHERE e.ends_on >= $1 AND e.starts_on <= $2
         ORDER BY e.starts_on, e.title",
    )
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Deserialize)]
pub struct NewEvent {
    pub kind: String,
    pub title: String,
    pub description: Option<String>,
    pub starts_on: NaiveDate,
    pub ends_on: NaiveDate,
    pub program_id: Option<i16>,
}

pub async fn create_event(conn: &mut PgConnection, event: &NewEvent) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO academic.calendar_events
             (institution_id, program_id, kind, title, description, starts_on, ends_on, created_by)
         SELECT u.institution_id, $1, $2::academic.event_kind, $3, $4, $5, $6, u.id
         FROM identity.users u WHERE u.id = identity.current_user_id()
         RETURNING id",
    )
    .bind(event.program_id)
    .bind(&event.kind)
    .bind(&event.title)
    .bind(event.description.as_deref())
    .bind(event.starts_on)
    .bind(event.ends_on)
    .fetch_one(conn)
    .await
}

pub async fn update_event(
    conn: &mut PgConnection,
    id: Uuid,
    event: &NewEvent,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE academic.calendar_events
         SET program_id = $2, kind = $3::academic.event_kind, title = $4, description = $5,
             starts_on = $6, ends_on = $7
         WHERE id = $1",
    )
    .bind(id)
    .bind(event.program_id)
    .bind(&event.kind)
    .bind(&event.title)
    .bind(event.description.as_deref())
    .bind(event.starts_on)
    .bind(event.ends_on)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// The holiday or class suspension that rules out classes for a program on a
/// day, if any.
pub async fn no_classes_on(
    conn: &mut PgConnection,
    program_id: i16,
    day: NaiveDate,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT title FROM academic.calendar_events
         WHERE kind IN ('holiday', 'class_suspension') AND $2 BETWEEN starts_on AND ends_on
           AND (program_id IS NULL OR program_id = $1)
         ORDER BY program_id NULLS FIRST LIMIT 1",
    )
    .bind(program_id)
    .bind(day)
    .fetch_optional(conn)
    .await
}

pub async fn delete_event(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM academic.calendar_events WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(result.rows_affected() == 1)
}

// ── Attendance detail ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AttendanceDay {
    pub date: NaiveDate,
    pub sessions: i64,
    pub present: i64,
    pub late: i64,
    pub absent: i64,
    pub excused: i64,
}

/// One student's sessions per day between two dates (one academic year's
/// partition: `program_id` and `academic_year` let the planner prune).
pub async fn attendance_days(
    conn: &mut PgConnection,
    program_id: i16,
    academic_year: i16,
    student_id: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<AttendanceDay>, sqlx::Error> {
    sqlx::query_as(
        "SELECT session_date AS date, count(*) AS sessions,
                count(*) FILTER (WHERE status = 'present') AS present,
                count(*) FILTER (WHERE status = 'late') AS late,
                count(*) FILTER (WHERE status = 'absent') AS absent,
                count(*) FILTER (WHERE status = 'excused') AS excused
         FROM academic.attendance
         WHERE program_id = $1 AND academic_year = $2 AND student_id = $3
           AND session_date BETWEEN $4 AND $5
         GROUP BY session_date ORDER BY session_date",
    )
    .bind(program_id)
    .bind(academic_year)
    .bind(student_id)
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Session {
    pub offering_id: Uuid,
    pub date: NaiveDate,
    pub slot: i16,
    pub status: String,
    /// theory | practical
    pub session_kind: String,
    pub marked_by: Option<String>,
    pub marked_at: DateTime<Utc>,
    /// The latest correction request for this session, if any.
    pub correction: Option<String>,
}

/// Every session of one subject for one student in an academic year, newest first.
pub async fn sessions(
    conn: &mut PgConnection,
    program_id: i16,
    academic_year: i16,
    student_id: Uuid,
    subject_code: &str,
) -> Result<Vec<Session>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.offering_id, a.session_date AS date, a.slot, a.status::text AS status, a.session_kind,
                u.display_name AS marked_by, a.marked_at,
                (SELECT c.status FROM academic.attendance_corrections c
                 WHERE c.student_id = a.student_id AND c.offering_id = a.offering_id
                   AND c.session_date = a.session_date AND c.slot = a.slot
                 ORDER BY c.created_at DESC LIMIT 1) AS correction
         FROM academic.attendance a
         JOIN academic.offerings o ON o.id = a.offering_id
         JOIN academic.subjects s ON s.id = o.subject_id
         LEFT JOIN identity.users u ON u.id = a.marked_by
         WHERE a.program_id = $1 AND a.academic_year = $2 AND a.student_id = $3 AND s.code = $4
         ORDER BY a.session_date DESC, a.slot",
    )
    .bind(program_id)
    .bind(academic_year)
    .bind(student_id)
    .bind(subject_code)
    .fetch_all(conn)
    .await
}

// ── Attendance corrections ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CorrectionRequest {
    pub offering_id: Uuid,
    pub session_date: NaiveDate,
    pub slot: i16,
    pub requested_status: String,
    pub reason: String,
}

/// Files a correction for one of the acting student's own sessions. `None`
/// if there's no such session, or it already has the requested status.
pub async fn request_correction(
    conn: &mut PgConnection,
    request: &CorrectionRequest,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO academic.attendance_corrections
             (program_id, academic_year, student_id, offering_id, session_date, slot,
              current_status, requested_status, reason)
         SELECT a.program_id, a.academic_year, a.student_id, a.offering_id, a.session_date, a.slot,
                a.status, $4::academic.attendance_status, $5
         FROM academic.attendance a
         WHERE a.student_id = identity.current_user_id() AND a.offering_id = $1
           AND a.session_date = $2 AND a.slot = $3 AND a.status <> $4::academic.attendance_status
         RETURNING id",
    )
    .bind(request.offering_id)
    .bind(request.session_date)
    .bind(request.slot)
    .bind(&request.requested_status)
    .bind(&request.reason)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Correction {
    pub id: Uuid,
    pub student_id: Uuid,
    pub student_name: String,
    pub roll_no: String,
    pub subject_code: String,
    pub subject_name: String,
    pub session_date: NaiveDate,
    pub slot: i16,
    pub current_status: String,
    pub requested_status: String,
    pub reason: String,
    pub status: String,
    pub decided_by: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// The corrections the caller may see: their own (student), their
/// offerings' (faculty), their department's (HOD). Pending first.
pub async fn corrections(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<Correction>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.id, c.student_id, u.display_name AS student_name, st.roll_no,
                s.code AS subject_code, s.name AS subject_name, c.session_date, c.slot,
                c.current_status::text AS current_status, c.requested_status::text AS requested_status,
                c.reason, c.status, d.display_name AS decided_by, c.decided_at, c.decision_note, c.created_at
         FROM academic.attendance_corrections c
         JOIN identity.users u ON u.id = c.student_id
         JOIN people.students st ON st.user_id = c.student_id
         JOIN academic.offerings o ON o.id = c.offering_id
         JOIN academic.subjects s ON s.id = o.subject_id
         LEFT JOIN identity.users d ON d.id = c.decided_by
         ORDER BY c.status <> 'pending', c.created_at DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub enum Decision {
    Decided {
        student_id: Uuid,
    },
    /// Not found, not pending, or not in the HOD's department.
    NotPending,
    /// The mark changed since the request; nothing was rewritten.
    MarkChanged,
}

/// Approves or rejects a pending correction as the acting HOD; approving
/// rewrites the attendance mark in the same transaction.
pub async fn decide_correction(
    conn: &mut PgConnection,
    id: Uuid,
    approve: bool,
    note: Option<&str>,
) -> Result<Decision, sqlx::Error> {
    #[derive(sqlx::FromRow)]
    struct Pending {
        program_id: i16,
        academic_year: i16,
        student_id: Uuid,
        offering_id: Uuid,
        session_date: NaiveDate,
        slot: i16,
        current_status: String,
        requested_status: String,
    }
    let pending: Option<Pending> = sqlx::query_as(
        "SELECT program_id, academic_year, student_id, offering_id, session_date, slot,
                current_status::text AS current_status, requested_status::text AS requested_status
         FROM academic.attendance_corrections WHERE id = $1 AND status = 'pending' FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(p) = pending else {
        return Ok(Decision::NotPending);
    };
    if approve {
        let rewritten = sqlx::query(
            "UPDATE academic.attendance
             SET status = $7::academic.attendance_status, marked_by = identity.current_user_id(), marked_at = now()
             WHERE program_id = $1 AND academic_year = $2 AND student_id = $3 AND offering_id = $4
               AND session_date = $5 AND slot = $6 AND status = $8::academic.attendance_status",
        )
        .bind(p.program_id)
        .bind(p.academic_year)
        .bind(p.student_id)
        .bind(p.offering_id)
        .bind(p.session_date)
        .bind(p.slot)
        .bind(&p.requested_status)
        .bind(&p.current_status)
        .execute(&mut *conn)
        .await?;
        if rewritten.rows_affected() != 1 {
            return Ok(Decision::MarkChanged);
        }
    }
    sqlx::query(
        "UPDATE academic.attendance_corrections
         SET status = $2, decided_by = identity.current_user_id(), decided_at = now(), decision_note = $3
         WHERE id = $1",
    )
    .bind(id)
    .bind(if approve { "approved" } else { "rejected" })
    .bind(note)
    .execute(conn)
    .await?;
    Ok(Decision::Decided {
        student_id: p.student_id,
    })
}

// ── Course status ───────────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
pub struct CourseRow {
    pub subject_id: Uuid,
    /// The offering the student took it in (their batch's year for that semester).
    pub offering_id: Option<Uuid>,
    pub semester: i16,
    pub code: String,
    pub name: String,
    pub credits: i16,
    pub kind: String,
    pub internal: Option<f64>,
    pub internal_max: Option<f64>,
    pub internal_entered: i64,
    pub external: Option<f64>,
    pub external_max: Option<i16>,
    pub published_at: Option<DateTime<Utc>>,
}

/// Every subject of the student's program up to their current semester, with
/// whatever marks the caller may see (a student: only published ones).
pub async fn course_rows(
    conn: &mut PgConnection,
    student_id: Uuid,
) -> Result<Vec<CourseRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id AS subject_id, o.id AS offering_id, s.semester, s.code, s.name, s.credits, s.kind::text AS kind,
                i.internal, i.internal_max, i.entered AS internal_entered,
                em.marks::float8 AS external, em.max_marks AS external_max, rp.published_at
         FROM people.students st
         JOIN academic.subjects s ON s.program_id = st.program_id AND s.semester <= st.current_semester
                                  AND academic.takes(st.user_id, s.id)
         LEFT JOIN academic.offerings o
                ON o.subject_id = s.id AND o.academic_year = st.batch_year + (s.semester - 1) / 2
         -- The latest attempt the caller may see (a backlog pass replaces a fail).
         LEFT JOIN LATERAL (SELECT * FROM exams.external_marks em
                            WHERE em.offering_id = o.id AND em.student_id = st.user_id
                            ORDER BY em.attempt DESC LIMIT 1) em ON true
         LEFT JOIN exams.result_publications rp ON rp.id = em.publication_id
         LEFT JOIN LATERAL (
             SELECT sum(im.marks)::float8 AS internal, sum(c.max_marks)::float8 AS internal_max,
                    count(im.marks) AS entered
             FROM exams.internal_components c
             LEFT JOIN exams.internal_marks im ON im.component_id = c.id AND im.student_id = st.user_id
             WHERE c.offering_id = o.id) i ON true
         WHERE st.user_id = $1
         ORDER BY s.semester, s.code",
    )
    .bind(student_id)
    .fetch_all(conn)
    .await
}

// ── Finding students (staff) ────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StudentRow {
    pub id: Uuid,
    pub display_name: String,
    pub roll_no: String,
    pub program_name: String,
    pub current_semester: i16,
    pub section: String,
    pub graduated_on: Option<NaiveDate>,
}

/// Students the caller may see (RLS: faculty their programs, an HOD their
/// department, other staff everyone), by roll number, keyset-paged.
pub async fn find_students(
    conn: &mut PgConnection,
    search: Option<&str>,
    after_roll: Option<&str>,
    limit: i64,
) -> Result<Vec<StudentRow>, sqlx::Error> {
    let search = search.map(|s| {
        s.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    });
    sqlx::query_as(
        "SELECT s.user_id AS id, u.display_name, s.roll_no, p.name AS program_name, s.current_semester, s.section,
                s.graduated_on
         FROM people.students s
         JOIN identity.users u ON u.id = s.user_id
         JOIN org.programs p ON p.id = s.program_id
         WHERE ($1::text IS NULL OR s.roll_no ILIKE $1 || '%' OR u.display_name ILIKE '%' || $1 || '%')
           AND ($2::text IS NULL OR s.roll_no > $2)
         ORDER BY s.roll_no
         LIMIT $3",
    )
    .bind(search)
    .bind(after_roll)
    .bind(limit)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SubjectRef {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub semester: i16,
    pub program_name: String,
}

/// Every subject, for pickers (the timetable).
pub async fn all_subjects(conn: &mut PgConnection) -> Result<Vec<SubjectRef>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id, s.code, s.name, s.semester, p.name AS program_name
         FROM academic.subjects s JOIN org.programs p ON p.id = s.program_id
         WHERE s.active
         ORDER BY p.name, s.semester, s.code",
    )
    .fetch_all(conn)
    .await
}
