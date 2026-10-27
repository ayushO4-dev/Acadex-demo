//! The weekly class timetable. Row-level security lets heads of department
//! change their programs' (administration, any); `clash` finds what a new
//! entry would collide with.

use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Entry {
    pub id: Uuid,
    pub offering_id: Uuid,
    pub code: String,
    pub subject: String,
    pub program_id: i16,
    pub program: String,
    pub semester: i16,
    pub section: Option<String>,
    pub weekday: i16,
    pub slot: i16,
    pub session_kind: String,
    pub room: Option<String>,
    pub faculty: Option<String>,
}

macro_rules! entry_sql {
    () => {
        "SELECT t.id, t.offering_id, s.code, s.name AS subject, s.program_id, p.code AS program, s.semester,
                t.section, t.weekday, t.slot, t.session_kind, t.room, u.display_name AS faculty
         FROM academic.timetable t
         JOIN academic.offerings o ON o.id = t.offering_id
         JOIN academic.subjects s ON s.id = o.subject_id
         JOIN org.programs p ON p.id = s.program_id
         LEFT JOIN identity.users u ON u.id = o.faculty_id"
    };
}

/// A class's week: one program and semester (and section, or all of them).
/// A student's view passes `student` to see only the electives they take.
pub async fn for_class(
    conn: &mut PgConnection,
    program_id: i16,
    semester: i16,
    section: Option<&str>,
    academic_year: i16,
    student: Option<Uuid>,
) -> Result<Vec<Entry>, sqlx::Error> {
    sqlx::query_as(concat!(
        entry_sql!(),
        " WHERE s.program_id = $1 AND s.semester = $2 AND o.academic_year = $4
            AND ($3::text IS NULL OR t.section IS NULL OR t.section = $3)
            AND ($5::uuid IS NULL OR academic.takes($5, s.id))
          ORDER BY t.weekday, t.slot, s.code"
    ))
    .bind(program_id)
    .bind(semester)
    .bind(section)
    .bind(academic_year)
    .bind(student)
    .fetch_all(conn)
    .await
}

/// What the acting faculty member teaches in a year.
pub async fn for_teacher(
    conn: &mut PgConnection,
    academic_year: i16,
) -> Result<Vec<Entry>, sqlx::Error> {
    sqlx::query_as(concat!(
        entry_sql!(),
        " WHERE o.faculty_id = identity.current_user_id() AND o.academic_year = $1
          ORDER BY t.weekday, t.slot"
    ))
    .bind(academic_year)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Deserialize)]
pub struct NewEntry {
    pub offering_id: Uuid,
    #[serde(default)]
    pub section: Option<String>,
    pub weekday: i16,
    pub slot: i16,
    #[serde(default = "theory")]
    pub session_kind: String,
    #[serde(default)]
    pub room: Option<String>,
}

fn theory() -> String {
    "theory".to_owned()
}

/// What a new entry would collide with, described for the person adding it:
/// the same students (that class and section), the same teacher, or the
/// same room in the same period. `None` if it fits.
pub async fn clash(
    conn: &mut PgConnection,
    entry: &NewEntry,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "WITH new AS (
             SELECT s.program_id, s.semester, o.academic_year, o.faculty_id, s.kind, s.elective_group
             FROM academic.offerings o JOIN academic.subjects s ON s.id = o.subject_id WHERE o.id = $1
         )
         SELECT CASE
                  WHEN s.program_id = new.program_id AND s.semester = new.semester
                       AND ($2::text IS NULL OR t.section IS NULL OR t.section = $2)
                       AND NOT (s.kind = 'elective' AND new.kind = 'elective' AND s.elective_group = new.elective_group)
                    THEN 'the class already has ' || s.code || ' then'
                  WHEN o.faculty_id = new.faculty_id
                    THEN coalesce(u.display_name, 'the teacher') || ' already teaches ' || s.code || ' then'
                  ELSE 'room ' || t.room || ' already has ' || s.code || ' then'
                END
         FROM academic.timetable t
         JOIN academic.offerings o ON o.id = t.offering_id
         JOIN academic.subjects s ON s.id = o.subject_id
         LEFT JOIN identity.users u ON u.id = o.faculty_id
         CROSS JOIN new
         WHERE t.weekday = $3 AND t.slot = $4 AND o.academic_year = new.academic_year
           AND ((s.program_id = new.program_id AND s.semester = new.semester
                 AND ($2::text IS NULL OR t.section IS NULL OR t.section = $2)
                 -- Options of one elective group meet together: their students differ.
                 AND NOT (s.kind = 'elective' AND new.kind = 'elective' AND s.elective_group = new.elective_group))
                OR o.faculty_id = new.faculty_id
                OR ($5::text IS NOT NULL AND lower(t.room) = lower($5)))
         LIMIT 1",
    )
    .bind(entry.offering_id)
    .bind(entry.section.as_deref())
    .bind(entry.weekday)
    .bind(entry.slot)
    .bind(entry.room.as_deref())
    .fetch_optional(conn)
    .await
}

/// One timetable change at a time (until the transaction ends), so two
/// clashing entries can't both pass `clash`.
pub async fn lock(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('academic.timetable'))")
        .execute(conn)
        .await?;
    Ok(())
}

/// Adds an entry; `None` if the offering isn't the caller's to timetable.
pub async fn add(conn: &mut PgConnection, entry: &NewEntry) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO academic.timetable (offering_id, section, weekday, slot, session_kind, room, created_by)
         SELECT o.id, $2, $3, $4, $5, $6, identity.current_user_id()
         FROM academic.offerings o JOIN academic.subjects s ON s.id = o.subject_id
         WHERE o.id = $1 AND academic.may_manage_program(s.program_id)
         RETURNING id",
    )
    .bind(entry.offering_id)
    .bind(entry.section.as_deref())
    .bind(entry.weekday)
    .bind(entry.slot)
    .bind(&entry.session_kind)
    .bind(entry.room.as_deref())
    .fetch_optional(conn)
    .await
}

pub async fn remove(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM academic.timetable WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ClassOffering {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub kind: String,
    pub has_practical: bool,
    pub faculty: Option<String>,
}

/// A class's subjects taught in a year, for placing on the timetable.
pub async fn class_offerings(
    conn: &mut PgConnection,
    program_id: i16,
    semester: i16,
    academic_year: i16,
) -> Result<Vec<ClassOffering>, sqlx::Error> {
    sqlx::query_as(
        "SELECT o.id, s.code, s.name, s.kind::text AS kind, s.has_practical, u.display_name AS faculty
         FROM academic.offerings o
         JOIN academic.subjects s ON s.id = o.subject_id
         LEFT JOIN identity.users u ON u.id = o.faculty_id
         WHERE s.program_id = $1 AND s.semester = $2 AND o.academic_year = $3 AND s.active
         ORDER BY s.code",
    )
    .bind(program_id)
    .bind(semester)
    .bind(academic_year)
    .fetch_all(conn)
    .await
}

/// The acting student's class: program, semester and section.
pub async fn my_class(conn: &mut PgConnection) -> Result<Option<(i16, i16, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT program_id, current_semester, section FROM people.students
         WHERE user_id = identity.current_user_id() AND graduated_on IS NULL",
    )
    .fetch_optional(conn)
    .await
}
