//! Examination: sessions, timetable, venues, forms and seating
//! (architecture.md §9.3). Forms are created only through `exams.create_form`.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub academic_year: i16,
    pub form_opens_on: NaiveDate,
    pub form_closes_on: NaiveDate,
    pub regular_fee_paise: i64,
    pub backlog_fee_paise: i64,
    pub hall_tickets_generated_at: Option<DateTime<Utc>>,
    pub hall_tickets_released_at: Option<DateTime<Utc>>,
    /// Forms the caller can see, by status.
    pub forms_submitted: i64,
    pub forms_pending: i64,
    /// Cancelled before anyone registered: kept on record, closed to everything.
    pub cancelled_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
}

macro_rules! session_sql {
    () => {
        "SELECT s.id, s.name, s.academic_year, s.form_opens_on, s.form_closes_on, s.regular_fee_paise,
                s.backlog_fee_paise, s.hall_tickets_generated_at, s.hall_tickets_released_at,
                (SELECT count(*) FROM exams.exam_forms f WHERE f.session_id = s.id AND f.status = 'submitted') AS forms_submitted,
                (SELECT count(*) FROM exams.exam_forms f WHERE f.session_id = s.id AND f.status = 'payment_pending') AS forms_pending,
                s.cancelled_at, s.cancel_reason
         FROM exams.exam_sessions s"
    };
}

pub async fn sessions(conn: &mut PgConnection) -> Result<Vec<Session>, sqlx::Error> {
    sqlx::query_as(concat!(session_sql!(), " ORDER BY s.form_opens_on DESC"))
        .fetch_all(conn)
        .await
}

pub async fn session(conn: &mut PgConnection, id: Uuid) -> Result<Option<Session>, sqlx::Error> {
    sqlx::query_as(concat!(session_sql!(), " WHERE s.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Deserialize)]
pub struct SessionInput {
    pub name: String,
    pub academic_year: i16,
    pub form_opens_on: NaiveDate,
    pub form_closes_on: NaiveDate,
    pub regular_fee_paise: i64,
    pub backlog_fee_paise: i64,
}

pub async fn create_session(
    conn: &mut PgConnection,
    s: &SessionInput,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO exams.exam_sessions
             (institution_id, name, academic_year, form_opens_on, form_closes_on, regular_fee_paise, backlog_fee_paise, created_by)
         SELECT u.institution_id, $1, $2, $3, $4, $5, $6, u.id
         FROM identity.users u WHERE u.id = identity.current_user_id()
         RETURNING id",
    )
    .bind(&s.name)
    .bind(s.academic_year)
    .bind(s.form_opens_on)
    .bind(s.form_closes_on)
    .bind(s.regular_fee_paise)
    .bind(s.backlog_fee_paise)
    .fetch_one(conn)
    .await
}

pub async fn update_session(
    conn: &mut PgConnection,
    id: Uuid,
    s: &SessionInput,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE exams.exam_sessions
         SET name = $2, academic_year = $3, form_opens_on = $4, form_closes_on = $5,
             regular_fee_paise = $6, backlog_fee_paise = $7
         WHERE id = $1",
    )
    .bind(id)
    .bind(&s.name)
    .bind(s.academic_year)
    .bind(s.form_opens_on)
    .bind(s.form_closes_on)
    .bind(s.regular_fee_paise)
    .bind(s.backlog_fee_paise)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Paper {
    pub subject_id: Uuid,
    #[sqlx(default)]
    #[serde(default)]
    pub code: String,
    #[sqlx(default)]
    #[serde(default)]
    pub name: String,
    #[sqlx(default)]
    #[serde(default)]
    pub semester: i16,
    #[sqlx(default)]
    #[serde(default)]
    pub program_name: String,
    pub exam_date: NaiveDate,
    pub starts_at: NaiveTime,
    pub duration_minutes: i16,
}

pub async fn schedule(conn: &mut PgConnection, session: Uuid) -> Result<Vec<Paper>, sqlx::Error> {
    sqlx::query_as(
        "SELECT p.subject_id, s.code, s.name, s.semester, pr.name AS program_name,
                p.exam_date, p.starts_at, p.duration_minutes
         FROM exams.schedule p
         JOIN academic.subjects s ON s.id = p.subject_id
         JOIN org.programs pr ON pr.id = s.program_id
         WHERE p.session_id = $1
         ORDER BY p.exam_date, p.starts_at, s.code",
    )
    .bind(session)
    .fetch_all(conn)
    .await
}

/// Replaces the timetable in one go.
pub async fn replace_schedule(
    conn: &mut PgConnection,
    session: Uuid,
    papers: &[Paper],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM exams.schedule WHERE session_id = $1")
        .bind(session)
        .execute(&mut *conn)
        .await?;
    let subjects: Vec<Uuid> = papers.iter().map(|p| p.subject_id).collect();
    let dates: Vec<NaiveDate> = papers.iter().map(|p| p.exam_date).collect();
    let starts: Vec<NaiveTime> = papers.iter().map(|p| p.starts_at).collect();
    let durations: Vec<i16> = papers.iter().map(|p| p.duration_minutes).collect();
    sqlx::query(
        "INSERT INTO exams.schedule (session_id, subject_id, exam_date, starts_at, duration_minutes)
         SELECT $1, s, d, t, m FROM unnest($2::uuid[], $3::date[], $4::time[], $5::int2[]) AS p(s, d, t, m)",
    )
    .bind(session)
    .bind(&subjects)
    .bind(&dates)
    .bind(&starts)
    .bind(&durations)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Venue {
    #[serde(default)]
    pub id: i16,
    pub name: String,
    pub capacity: i16,
    /// The highest seat number allocated in it (it can't shrink below that).
    #[serde(default)]
    pub seats_used: Option<i16>,
}

pub async fn venues(conn: &mut PgConnection) -> Result<Vec<Venue>, sqlx::Error> {
    sqlx::query_as(
        "SELECT v.id, v.name, v.capacity, (SELECT max(seat_no) FROM exams.seating se WHERE se.venue_id = v.id) AS seats_used
         FROM exams.venues v ORDER BY v.name",
    )
        .fetch_all(conn)
        .await
}

pub async fn add_venue(conn: &mut PgConnection, venue: &Venue) -> Result<i16, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO exams.venues (id, institution_id, name, capacity)
         SELECT coalesce((SELECT max(id) FROM exams.venues), 0) + 1, u.institution_id, $1, $2
         FROM identity.users u WHERE u.id = identity.current_user_id()
         RETURNING id",
    )
    .bind(&venue.name)
    .bind(venue.capacity)
    .fetch_one(conn)
    .await
}

pub enum Change {
    Done,
    NotFound,
    /// Seating already uses more seats, or uses the venue at all (to delete).
    InUse(i16),
}

pub async fn update_venue(
    conn: &mut PgConnection,
    id: i16,
    name: &str,
    capacity: i16,
) -> Result<Change, sqlx::Error> {
    let used: Option<Option<i16>> = sqlx::query_scalar(
        "SELECT (SELECT max(seat_no) FROM exams.seating WHERE venue_id = v.id) FROM exams.venues v WHERE v.id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(used) = used else {
        return Ok(Change::NotFound);
    };
    if let Some(max) = used.filter(|max| *max > capacity) {
        return Ok(Change::InUse(max));
    }
    sqlx::query("UPDATE exams.venues SET name = $2, capacity = $3 WHERE id = $1")
        .bind(id)
        .bind(name)
        .bind(capacity)
        .execute(conn)
        .await?;
    Ok(Change::Done)
}

pub async fn delete_venue(conn: &mut PgConnection, id: i16) -> Result<Change, sqlx::Error> {
    let seats: Option<i64> = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM exams.seating WHERE venue_id = v.id) FROM exams.venues v WHERE v.id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    match seats {
        None => Ok(Change::NotFound),
        Some(n) if n > 0 => Ok(Change::InUse(i16::try_from(n).unwrap_or(i16::MAX))),
        Some(_) => {
            sqlx::query("DELETE FROM exams.venues WHERE id = $1")
                .bind(id)
                .execute(conn)
                .await?;
            Ok(Change::Done)
        }
    }
}

pub enum Cancel {
    Done,
    NotFound,
    AlreadyCancelled,
    /// Students have registered (and been charged): settle those first.
    HasForms(i64),
}

pub async fn cancel_session(
    conn: &mut PgConnection,
    id: Uuid,
    reason: &str,
) -> Result<Cancel, sqlx::Error> {
    let row: Option<(bool, i64)> = sqlx::query_as(
        "SELECT s.cancelled_at IS NOT NULL, (SELECT count(*) FROM exams.exam_forms f WHERE f.session_id = s.id)
         FROM exams.exam_sessions s WHERE s.id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        None => Ok(Cancel::NotFound),
        Some((true, _)) => Ok(Cancel::AlreadyCancelled),
        Some((false, forms)) if forms > 0 => Ok(Cancel::HasForms(forms)),
        Some(_) => {
            sqlx::query(
                "UPDATE exams.exam_sessions
                 SET cancelled_at = now(), cancelled_by = identity.current_user_id(), cancel_reason = $2
                 WHERE id = $1",
            )
            .bind(id)
            .bind(reason)
            .execute(conn)
            .await?;
            Ok(Cancel::Done)
        }
    }
}

/// `None` if there's no such session.
pub async fn is_cancelled(conn: &mut PgConnection, id: Uuid) -> Result<Option<bool>, sqlx::Error> {
    sqlx::query_scalar("SELECT cancelled_at IS NOT NULL FROM exams.exam_sessions WHERE id = $1")
        .bind(id)
        .fetch_optional(conn)
        .await
}

// ── Forms ───────────────────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
pub struct CreatedForm {
    pub form_id: Uuid,
    pub status: String,
    pub pending_charges: Vec<i64>,
}

pub async fn create_form(
    conn: &mut PgConnection,
    session: Uuid,
    kind: &str,
    subjects: &[Uuid],
    offerings: &[Uuid],
) -> Result<CreatedForm, sqlx::Error> {
    sqlx::query_as("SELECT * FROM exams.create_form($1, $2, $3, $4)")
        .bind(session)
        .bind(kind)
        .bind(subjects)
        .bind(offerings)
        .fetch_one(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Form {
    pub id: Uuid,
    pub session_id: Uuid,
    pub session_name: String,
    pub student_id: Uuid,
    pub student_name: String,
    pub roll_no: String,
    pub program_name: String,
    pub kind: String,
    pub fee_paise: i64,
    pub status: String,
    pub submitted_at: Option<DateTime<Utc>>,
    pub ack_ready: bool,
    pub subjects: sqlx::types::Json<Vec<FormSubject>>,
    /// Charges still to pay, for resuming payment.
    pub pending_charges: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormSubject {
    pub code: String,
    pub name: String,
    pub semester: i16,
}

macro_rules! form_sql {
    () => {
        "SELECT f.id, f.session_id, x.name AS session_name, f.student_id, u.display_name AS student_name,
                st.roll_no, p.name AS program_name, f.kind, f.fee_paise, f.status, f.submitted_at,
                f.ack_pdf_object IS NOT NULL AS ack_ready,
                coalesce((SELECT jsonb_agg(jsonb_build_object('code', s.code, 'name', s.name, 'semester', s.semester) ORDER BY s.semester, s.code)
                          FROM exams.exam_form_subjects fs JOIN academic.subjects s ON s.id = fs.subject_id
                          WHERE fs.form_id = f.id), '[]'::jsonb) AS subjects,
                coalesce((SELECT array_agg(fc.charge_id ORDER BY fc.charge_id) FROM exams.exam_form_charges fc
                          WHERE fc.form_id = f.id AND fees.pending_of(fc.charge_id) > 0), '{}') AS pending_charges
         FROM exams.exam_forms f
         JOIN exams.exam_sessions x ON x.id = f.session_id
         JOIN people.students st ON st.user_id = f.student_id
         JOIN identity.users u ON u.id = f.student_id
         JOIN org.programs p ON p.id = st.program_id"
    };
}

/// Forms the caller may see, optionally for one session. Newest first.
pub async fn forms(
    conn: &mut PgConnection,
    session: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Form>, sqlx::Error> {
    sqlx::query_as(concat!(
        form_sql!(),
        " WHERE ($1::uuid IS NULL OR f.session_id = $1) ORDER BY f.created_at DESC LIMIT $2"
    ))
    .bind(session)
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn form(conn: &mut PgConnection, id: Uuid) -> Result<Option<Form>, sqlx::Error> {
    sqlx::query_as(concat!(form_sql!(), " WHERE f.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// After a payment: forms of the student with nothing left to pay become
/// submitted (as `gateway`). Returns them.
pub async fn complete_paid_forms(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT exams.complete_paid_forms($1)")
        .bind(student)
        .fetch_all(conn)
        .await
}

pub async fn allocate_seats(conn: &mut PgConnection, session: Uuid) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT exams.allocate_seats($1)")
        .bind(session)
        .fetch_one(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Seat {
    pub venue: String,
    pub seat_no: i16,
}

pub async fn seat(
    conn: &mut PgConnection,
    session: Uuid,
    student: Uuid,
) -> Result<Option<Seat>, sqlx::Error> {
    sqlx::query_as(
        "SELECT v.name AS venue, s.seat_no FROM exams.seating s JOIN exams.venues v ON v.id = s.venue_id
         WHERE s.session_id = $1 AND s.student_id = $2",
    )
    .bind(session)
    .bind(student)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SeatingSummary {
    pub venue: String,
    pub capacity: i16,
    pub seated: i64,
}

pub async fn seating_summary(
    conn: &mut PgConnection,
    session: Uuid,
) -> Result<Vec<SeatingSummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT v.name AS venue, v.capacity, count(s.student_id) AS seated
         FROM exams.venues v LEFT JOIN exams.seating s ON s.venue_id = v.id AND s.session_id = $1
         GROUP BY v.id ORDER BY v.name",
    )
    .bind(session)
    .fetch_all(conn)
    .await
}

/// Where a submitted form's acknowledgement PDF is (the worker sets it).
pub async fn attach_ack(
    conn: &mut PgConnection,
    form: Uuid,
    object: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE exams.exam_forms SET ack_pdf_object = $2 WHERE id = $1 AND ack_pdf_object IS NULL",
    )
    .bind(form)
    .bind(object)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn ack_object(
    conn: &mut PgConnection,
    form: Uuid,
) -> Result<Option<Option<String>>, sqlx::Error> {
    sqlx::query_scalar("SELECT ack_pdf_object FROM exams.exam_forms WHERE id = $1")
        .bind(form)
        .fetch_optional(conn)
        .await
}

// ── For the worker ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckSubject {
    pub code: String,
    pub name: String,
    pub semester: i16,
    pub exam_date: Option<NaiveDate>,
    pub starts_at: Option<NaiveTime>,
}

/// What an exam-form acknowledgement prints.
#[derive(Debug, sqlx::FromRow)]
pub struct AckDocument {
    pub form_id: Uuid,
    pub kind: String,
    pub fee_paise: i64,
    pub submitted_at: Option<DateTime<Utc>>,
    pub ack_pdf_object: Option<String>,
    pub session_name: String,
    pub institution: String,
    pub student_name: String,
    pub roll_no: String,
    pub program: String,
    pub semester: i16,
    pub subjects: sqlx::types::Json<Vec<AckSubject>>,
}

pub async fn ack_document(
    conn: &mut PgConnection,
    form: Uuid,
) -> Result<Option<AckDocument>, sqlx::Error> {
    sqlx::query_as(
        "SELECT f.id AS form_id, f.kind, f.fee_paise, f.submitted_at, f.ack_pdf_object, x.name AS session_name,
                i.name AS institution, u.display_name AS student_name, st.roll_no, p.name AS program,
                st.current_semester AS semester,
                coalesce((SELECT jsonb_agg(jsonb_build_object('code', s.code, 'name', s.name, 'semester', s.semester,
                                                              'exam_date', sc.exam_date, 'starts_at', sc.starts_at)
                                           ORDER BY sc.exam_date NULLS LAST, s.code)
                          FROM exams.exam_form_subjects fs
                          JOIN academic.subjects s ON s.id = fs.subject_id
                          LEFT JOIN exams.schedule sc ON sc.session_id = f.session_id AND sc.subject_id = s.id
                          WHERE fs.form_id = f.id), '[]'::jsonb) AS subjects
         FROM exams.exam_forms f
         JOIN exams.exam_sessions x ON x.id = f.session_id
         JOIN people.students st ON st.user_id = f.student_id
         JOIN identity.users u ON u.id = f.student_id
         JOIN org.programs p ON p.id = st.program_id
         JOIN org.institutions i ON i.id = p.institution_id
         WHERE f.id = $1 AND f.status = 'submitted'",
    )
    .bind(form)
    .fetch_optional(conn)
    .await
}

// ── Hall tickets ────────────────────────────────────────────────────────────

/// What a hall ticket prints.
#[derive(Debug, sqlx::FromRow)]
pub struct TicketDocument {
    pub student_id: Uuid,
    pub student_name: String,
    pub roll_no: String,
    pub program: String,
    pub semester: i16,
    pub venue: String,
    pub seat_no: i16,
    pub papers: sqlx::types::Json<Vec<AckSubject>>,
}

/// Seated students with a submitted form and no ticket yet (worker).
pub async fn tickets_to_make(
    conn: &mut PgConnection,
    session: Uuid,
) -> Result<Vec<TicketDocument>, sqlx::Error> {
    sqlx::query_as(
        "SELECT st.user_id AS student_id, u.display_name AS student_name, st.roll_no, p.name AS program,
                st.current_semester AS semester, v.name AS venue, se.seat_no,
                coalesce((SELECT jsonb_agg(jsonb_build_object('code', s.code, 'name', s.name, 'semester', s.semester,
                                                              'exam_date', sc.exam_date, 'starts_at', sc.starts_at)
                                           ORDER BY sc.exam_date, sc.starts_at, s.code)
                          FROM exams.exam_forms f
                          JOIN exams.exam_form_subjects fs ON fs.form_id = f.id
                          JOIN academic.subjects s ON s.id = fs.subject_id
                          LEFT JOIN exams.schedule sc ON sc.session_id = f.session_id AND sc.subject_id = s.id
                          WHERE f.session_id = se.session_id AND f.student_id = se.student_id AND f.status = 'submitted'),
                         '[]'::jsonb) AS papers
         FROM exams.seating se
         JOIN exams.venues v ON v.id = se.venue_id
         JOIN people.students st ON st.user_id = se.student_id
         JOIN identity.users u ON u.id = se.student_id
         JOIN org.programs p ON p.id = st.program_id
         WHERE se.session_id = $1
           AND NOT EXISTS (SELECT 1 FROM exams.hall_tickets t WHERE t.session_id = se.session_id AND t.student_id = se.student_id)
         ORDER BY st.roll_no",
    )
    .bind(session)
    .fetch_all(conn)
    .await
}

pub async fn record_ticket(
    conn: &mut PgConnection,
    id: Uuid,
    session: Uuid,
    student: Uuid,
    object: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO exams.hall_tickets (id, session_id, student_id, pdf_object) VALUES ($1, $2, $3, $4)
         ON CONFLICT (session_id, student_id) DO NOTHING",
    )
    .bind(id)
    .bind(session)
    .bind(student)
    .bind(object)
    .execute(conn)
    .await?;
    Ok(())
}

/// Marks the session generated once every seated student has a ticket.
pub async fn finish_generation(
    conn: &mut PgConnection,
    session: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE exams.exam_sessions x SET hall_tickets_generated_at = now()
         WHERE x.id = $1 AND x.hall_tickets_generated_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM exams.seating se WHERE se.session_id = x.id
                           AND NOT EXISTS (SELECT 1 FROM exams.hall_tickets t
                                           WHERE t.session_id = se.session_id AND t.student_id = se.student_id))",
    )
    .bind(session)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn ticket_count(conn: &mut PgConnection, session: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM exams.hall_tickets WHERE session_id = $1")
        .bind(session)
        .fetch_one(conn)
        .await
}

/// Releases a generated session's tickets as the acting exam cell user.
pub async fn release(conn: &mut PgConnection, session: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE exams.exam_sessions SET hall_tickets_released_at = now(), released_by = identity.current_user_id()
         WHERE id = $1 AND hall_tickets_generated_at IS NOT NULL AND hall_tickets_released_at IS NULL",
    )
    .bind(session)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Ticket {
    pub id: Uuid,
    pub session_id: Uuid,
    pub student_id: Uuid,
    pub pdf_object: String,
}

/// A student's ticket for a session, if the caller may see it (a student:
/// only once released).
pub async fn ticket(
    conn: &mut PgConnection,
    session: Uuid,
    student: Uuid,
) -> Result<Option<Ticket>, sqlx::Error> {
    sqlx::query_as("SELECT id, session_id, student_id, pdf_object FROM exams.hall_tickets WHERE session_id = $1 AND student_id = $2")
        .bind(session)
        .bind(student)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Verification {
    pub student_name: String,
    pub roll_no: String,
    pub program: String,
    pub session_name: String,
    pub venue: Option<String>,
    pub seat_no: Option<i16>,
    pub papers: serde_json::Value,
}

pub async fn verify(
    conn: &mut PgConnection,
    ticket: Uuid,
) -> Result<Option<Verification>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM exams.verify_ticket($1)")
        .bind(ticket)
        .fetch_optional(conn)
        .await
}
