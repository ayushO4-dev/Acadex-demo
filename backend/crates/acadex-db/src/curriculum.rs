//! Curriculum management: a program's subjects per semester, and who teaches
//! each in an academic year. Row-level security limits changes to the
//! program's head of department and administration.

use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CurriculumSubject {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub credits: i16,
    pub semester: i16,
    pub kind: String,
    pub has_practical: bool,
    /// Electives: the group a student picks one subject from.
    pub elective_group: Option<String>,
    pub active: bool,
    /// This academic year's offering, if any.
    pub offering_id: Option<Uuid>,
    pub faculty_id: Option<Uuid>,
    pub faculty: Option<String>,
    /// Attendance or marks exist: the subject can be retired, but its
    /// semester, kind and practical part are fixed, and this year's offering
    /// can't be removed.
    pub in_use: bool,
    pub offering_in_use: bool,
}

/// A program's subjects (one semester, or all), with this year's teacher.
pub async fn subjects(
    conn: &mut PgConnection,
    program_id: i16,
    semester: Option<i16>,
    academic_year: i16,
) -> Result<Vec<CurriculumSubject>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id, s.code, s.name, s.credits, s.semester, s.kind::text AS kind, s.has_practical,
                s.elective_group, s.active,
                o.id AS offering_id, o.faculty_id, u.display_name AS faculty,
                EXISTS (SELECT 1 FROM academic.offerings x
                        WHERE x.subject_id = s.id AND academic.offering_has_records(x.id)) AS in_use,
                coalesce(academic.offering_has_records(o.id), false) AS offering_in_use
         FROM academic.subjects s
         LEFT JOIN academic.offerings o ON o.subject_id = s.id AND o.academic_year = $3
         LEFT JOIN identity.users u ON u.id = o.faculty_id
         WHERE s.program_id = $1 AND ($2::smallint IS NULL OR s.semester = $2)
         ORDER BY s.semester, s.active DESC, s.code",
    )
    .bind(program_id)
    .bind(semester)
    .bind(academic_year)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Teacher {
    pub id: Uuid,
    pub name: String,
    pub designation: String,
    pub department_program: String,
}

/// Faculty who may be assigned: the program's department first, then everyone.
pub async fn teachers(
    conn: &mut PgConnection,
    program_id: i16,
) -> Result<Vec<Teacher>, sqlx::Error> {
    sqlx::query_as(
        "SELECT f.user_id AS id, u.display_name AS name, f.designation, p.code AS department_program
         FROM people.faculty f
         JOIN identity.users u ON u.id = f.user_id
         JOIN org.programs p ON p.id = f.program_id
         WHERE u.active
         ORDER BY (p.department_id = (SELECT department_id FROM org.programs WHERE id = $1)) DESC, u.display_name",
    )
    .bind(program_id)
    .fetch_all(conn)
    .await
}

pub struct SubjectFields<'a> {
    pub name: &'a str,
    pub credits: i16,
    pub semester: i16,
    pub kind: &'a str,
    pub has_practical: bool,
    pub elective_group: Option<&'a str>,
}

/// `None` if the code is taken or the caller may not manage the program
/// (row-level security refuses the insert).
pub async fn add_subject(
    conn: &mut PgConnection,
    program_id: i16,
    code: &str,
    fields: &SubjectFields<'_>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO academic.subjects (program_id, code, name, credits, semester, kind, has_practical, elective_group)
         VALUES ($1, $2, $3, $4, $5, $6::academic.subject_kind, $7, $8) RETURNING id",
    )
    .bind(program_id)
    .bind(code)
    .bind(fields.name)
    .bind(fields.credits)
    .bind(fields.semester)
    .bind(fields.kind)
    .bind(fields.has_practical)
    .bind(fields.elective_group)
    .fetch_one(conn)
    .await
}

pub enum Edit {
    Done,
    NotFound,
    /// Records exist, so the semester, kind and practical part can't change.
    InUse,
}

pub async fn edit_subject(
    conn: &mut PgConnection,
    id: Uuid,
    fields: &SubjectFields<'_>,
) -> Result<Edit, sqlx::Error> {
    let current: Option<(i16, String, bool, bool, Option<String>, bool)> = sqlx::query_as(
        "SELECT s.semester, s.kind::text, s.has_practical,
                EXISTS (SELECT 1 FROM academic.offerings x
                        WHERE x.subject_id = s.id AND academic.offering_has_records(x.id)),
                s.elective_group,
                EXISTS (SELECT 1 FROM academic.elective_choices c WHERE c.subject_id = s.id)
         FROM academic.subjects s WHERE s.id = $1 AND academic.may_manage_program(s.program_id)",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((semester, kind, has_practical, in_use, group, chosen)) = current else {
        return Ok(Edit::NotFound);
    };
    // Students' choices name the group and semester: those stay once anyone chose.
    if chosen
        && (group.as_deref() != fields.elective_group
            || semester != fields.semester
            || kind != fields.kind)
    {
        return Ok(Edit::InUse);
    }
    // Attendance is kept per lecture and practical: its shape can't change under it.
    if in_use
        && (semester != fields.semester
            || kind != fields.kind
            || has_practical != fields.has_practical)
    {
        return Ok(Edit::InUse);
    }
    sqlx::query(
        "UPDATE academic.subjects
         SET name = $2, credits = $3, semester = $4, kind = $5::academic.subject_kind, has_practical = $6,
             elective_group = $7, updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(fields.name)
    .bind(fields.credits)
    .bind(fields.semester)
    .bind(fields.kind)
    .bind(fields.has_practical)
    .bind(fields.elective_group)
    .execute(conn)
    .await?;
    Ok(Edit::Done)
}

/// Retires (or brings back) a subject; `false` if not found or not allowed.
pub async fn set_active(
    conn: &mut PgConnection,
    id: Uuid,
    active: bool,
) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("UPDATE academic.subjects SET active = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(active)
            .execute(conn)
            .await?;
    Ok(result.rows_affected() == 1)
}

/// Assigns (or reassigns) the subject's teacher for a year. Returns the
/// offering's id, or `None` if the subject isn't the caller's to manage.
pub async fn assign(
    conn: &mut PgConnection,
    subject_id: Uuid,
    academic_year: i16,
    faculty_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO academic.offerings (subject_id, academic_year, faculty_id)
         SELECT $1, $2, $3 FROM academic.subjects s
         WHERE s.id = $1 AND s.active AND academic.may_manage_program(s.program_id)
         ON CONFLICT (subject_id, academic_year) DO UPDATE SET faculty_id = EXCLUDED.faculty_id
         RETURNING id",
    )
    .bind(subject_id)
    .bind(academic_year)
    .bind(faculty_id)
    .fetch_optional(conn)
    .await
}

pub enum Unassign {
    Done,
    NotFound,
    /// Attendance or marks were recorded against it.
    InUse,
}

pub async fn unassign(conn: &mut PgConnection, offering_id: Uuid) -> Result<Unassign, sqlx::Error> {
    let in_use: Option<bool> = sqlx::query_scalar(
        "SELECT academic.offering_has_records(o.id) FROM academic.offerings o
         JOIN academic.subjects s ON s.id = o.subject_id
         WHERE o.id = $1 AND academic.may_manage_program(s.program_id)",
    )
    .bind(offering_id)
    .fetch_optional(&mut *conn)
    .await?;
    match in_use {
        None => Ok(Unassign::NotFound),
        Some(true) => Ok(Unassign::InUse),
        Some(false) => {
            sqlx::query("DELETE FROM academic.offerings WHERE id = $1")
                .bind(offering_id)
                .execute(conn)
                .await?;
            Ok(Unassign::Done)
        }
    }
}

/// The programs the caller may manage (an HOD's department; everything for
/// administration).
pub async fn manageable_programs(
    conn: &mut PgConnection,
) -> Result<Vec<(i16, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, code, name FROM org.programs WHERE academic.may_manage_program(id) ORDER BY name",
    )
    .fetch_all(conn)
    .await
}
