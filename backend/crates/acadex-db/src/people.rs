use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub display_name: String,
    pub role: String,
}

pub async fn user(conn: &mut PgConnection, id: Uuid) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as("SELECT id, display_name, role::text AS role FROM identity.users WHERE id = $1")
        .bind(id)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Student {
    pub id: Uuid,
    pub display_name: String,
    pub roll_no: String,
    pub program_id: i16,
    pub program_name: String,
    pub batch_year: i16,
    pub current_semester: i16,
}

/// `None` both when the student doesn't exist and when RLS hides them from the
/// acting user — callers can't tell the two apart, which is the point.
#[tracing::instrument(name = "db.student", skip_all, fields(otel.kind = "client"))]
pub async fn student(conn: &mut PgConnection, id: Uuid) -> Result<Option<Student>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.user_id AS id, u.display_name, s.roll_no, s.program_id,
                p.name AS program_name, s.batch_year, s.current_semester
         FROM people.students s
         JOIN identity.users u ON u.id = s.user_id
         JOIN org.programs p ON p.id = s.program_id
         WHERE s.user_id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}
