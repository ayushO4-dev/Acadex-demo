//! Universal search: a few matches of each kind, newest or closest first.
//! Row-level security scopes every query; the API decides which kinds a
//! role searches at all.

use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

/// Per kind; the dialog shows a handful, not a results page.
const LIMIT: i64 = 5;

/// `term` as a LIKE pattern matching it anywhere, with its own wildcards escaped.
pub fn contains(term: &str) -> String {
    let escaped = term
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SubjectHit {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub semester: i16,
    pub program_id: i16,
    pub program: String,
}

/// A student finds their own program's subjects; staff, any.
pub async fn subjects(
    conn: &mut PgConnection,
    pattern: &str,
    term: &str,
    own_program_only: bool,
) -> Result<Vec<SubjectHit>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id, s.code, s.name, s.semester, s.program_id, p.code AS program
         FROM academic.subjects s JOIN org.programs p ON p.id = s.program_id
         WHERE s.active AND (s.name ILIKE $1 OR s.code ILIKE $1)
           AND (NOT $3 OR s.program_id = (SELECT program_id FROM people.students
                                          WHERE user_id = identity.current_user_id()))
         ORDER BY s.code ILIKE $2 || '%' DESC, similarity(s.name, $2) DESC, s.code
         LIMIT $4",
    )
    .bind(pattern)
    .bind(term)
    .bind(own_program_only)
    .bind(LIMIT)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct NoticeHit {
    pub id: Uuid,
    pub title: String,
    pub severity: String,
    pub published_at: chrono::DateTime<chrono::Utc>,
}

pub async fn notices(
    conn: &mut PgConnection,
    pattern: &str,
) -> Result<Vec<NoticeHit>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, title, severity::text AS severity, published_at FROM comms.notices
         WHERE title ILIKE $1 AND published_at <= now()
         ORDER BY published_at DESC LIMIT $2",
    )
    .bind(pattern)
    .bind(LIMIT)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ReceiptHit {
    pub id: Uuid,
    pub receipt_no: String,
    pub student_id: Uuid,
    pub amount_paise: i64,
    pub issued_at: chrono::DateTime<chrono::Utc>,
}

pub async fn receipts(
    conn: &mut PgConnection,
    pattern: &str,
) -> Result<Vec<ReceiptHit>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, receipt_no, student_id, amount_paise, issued_at FROM fees.receipts
         WHERE receipt_no ILIKE $1 ORDER BY issued_at DESC LIMIT $2",
    )
    .bind(pattern)
    .bind(LIMIT)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TicketHit {
    pub id: Uuid,
    pub number: i64,
    pub subject: String,
    pub status: String,
}

/// By subject, or by number (`4801` or `#4801`).
pub async fn tickets(
    conn: &mut PgConnection,
    pattern: &str,
    number: Option<i64>,
) -> Result<Vec<TicketHit>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, number, subject, status FROM support.tickets
         WHERE subject ILIKE $1 OR number = $2
         ORDER BY number = $2 DESC, created_at DESC LIMIT $3",
    )
    .bind(pattern)
    .bind(number)
    .bind(LIMIT)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PersonHit {
    pub id: Uuid,
    pub name: String,
    pub roll_no: String,
    pub program: String,
}

/// Students, by name or roll number: whoever the caller may look up.
pub async fn students(
    conn: &mut PgConnection,
    pattern: &str,
    term: &str,
) -> Result<Vec<PersonHit>, sqlx::Error> {
    sqlx::query_as(
        "SELECT st.user_id AS id, u.display_name AS name, st.roll_no, p.code AS program
         FROM people.students st
         JOIN identity.users u ON u.id = st.user_id
         JOIN org.programs p ON p.id = st.program_id
         WHERE u.display_name ILIKE $1 OR st.roll_no ILIKE $1
         ORDER BY st.roll_no ILIKE $2 DESC, similarity(u.display_name, $2) DESC, st.roll_no
         LIMIT $3",
    )
    .bind(pattern)
    .bind(term)
    .bind(LIMIT)
    .fetch_all(conn)
    .await
}
