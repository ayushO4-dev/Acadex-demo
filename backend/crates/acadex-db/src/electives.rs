//! Elective choices: one subject per elective group per semester.
//! `academic.choose_elective` checks who may choose what, and refuses leaving
//! a class that already has records.

use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Choice {
    pub subject_id: Uuid,
    pub code: String,
    pub name: String,
    pub credits: i16,
    pub elective_group: String,
    pub faculty: Option<String>,
    /// The acting student chose it.
    pub chosen: bool,
}

/// The electives of the acting student's current semester, with their choices.
pub async fn mine(conn: &mut PgConnection) -> Result<Vec<Choice>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id AS subject_id, s.code, s.name, s.credits, s.elective_group, u.display_name AS faculty,
                EXISTS (SELECT 1 FROM academic.elective_choices c
                        WHERE c.student_id = st.user_id AND c.subject_id = s.id) AS chosen
         FROM people.students st
         JOIN academic.subjects s ON s.program_id = st.program_id AND s.semester = st.current_semester
         LEFT JOIN academic.offerings o
                ON o.subject_id = s.id AND o.academic_year = st.batch_year + (s.semester - 1) / 2
         LEFT JOIN identity.users u ON u.id = o.faculty_id
         WHERE st.user_id = identity.current_user_id() AND s.kind = 'elective' AND s.active
         ORDER BY s.elective_group, s.code",
    )
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Tally {
    pub subject_id: Uuid,
    pub code: String,
    pub name: String,
    pub elective_group: String,
    pub faculty: Option<String>,
    pub students: i64,
}

/// How many of a class chose each elective (managers).
pub async fn tally(
    conn: &mut PgConnection,
    program_id: i16,
    semester: i16,
    academic_year: i16,
) -> Result<Vec<Tally>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id AS subject_id, s.code, s.name, s.elective_group, u.display_name AS faculty,
                (SELECT count(*) FROM academic.elective_choices c
                 JOIN people.students st ON st.user_id = c.student_id
                 WHERE c.subject_id = s.id AND st.current_semester = s.semester AND st.graduated_on IS NULL) AS students
         FROM academic.subjects s
         LEFT JOIN academic.offerings o ON o.subject_id = s.id AND o.academic_year = $3
         LEFT JOIN identity.users u ON u.id = o.faculty_id
         WHERE s.program_id = $1 AND s.semester = $2 AND s.kind = 'elective' AND s.active
         ORDER BY s.elective_group, s.code",
    )
    .bind(program_id)
    .bind(semester)
    .bind(academic_year)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Undecided {
    pub elective_group: String,
    pub student_id: Uuid,
    pub name: String,
    pub roll_no: String,
}

/// Students of a class who haven't chosen in a group yet (managers).
pub async fn undecided(
    conn: &mut PgConnection,
    program_id: i16,
    semester: i16,
) -> Result<Vec<Undecided>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.elective_group, st.user_id AS student_id, u.display_name AS name, st.roll_no
         FROM (SELECT DISTINCT elective_group FROM academic.subjects
               WHERE program_id = $1 AND semester = $2 AND kind = 'elective' AND active) g
         CROSS JOIN people.students st
         JOIN identity.users u ON u.id = st.user_id AND u.active
         WHERE st.program_id = $1 AND st.current_semester = $2 AND st.graduated_on IS NULL
           AND NOT EXISTS (SELECT 1 FROM academic.elective_choices c
                           WHERE c.student_id = st.user_id AND c.semester = $2 AND c.elective_group = g.elective_group)
         ORDER BY g.elective_group, st.roll_no
         LIMIT 500",
    )
    .bind(program_id)
    .bind(semester)
    .fetch_all(conn)
    .await
}

/// "chosen", "changed" or "unchanged".
pub async fn choose(
    conn: &mut PgConnection,
    student: Uuid,
    subject: Uuid,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT academic.choose_elective($1, $2)")
        .bind(student)
        .bind(subject)
        .fetch_one(conn)
        .await
}
