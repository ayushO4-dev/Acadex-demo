//! Examination results. RLS shows a student only their own published marks;
//! the exam cell sees everything, staged results included.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct GradeBand {
    pub min_pct: f64,
    pub grade: String,
    pub points: i16,
}

#[derive(Clone, Debug)]
pub struct Grading {
    /// Highest band first.
    pub scale: Vec<GradeBand>,
    pub external_min_pct: f64,
    /// A subject also needs this share of its internal marks.
    pub internal_min_pct: f64,
}

// SQL fragments are macros so queries are built with `concat!` at compile
// time: sqlx only accepts static SQL text.
macro_rules! my_institution {
    () => {
        "(SELECT institution_id FROM identity.users WHERE id = identity.current_user_id())"
    };
}

/// The acting user's institution's grading configuration.
pub async fn grading(conn: &mut PgConnection) -> Result<Grading, sqlx::Error> {
    let scale = sqlx::query_as(concat!(
        "SELECT min_pct::float8 AS min_pct, grade, points FROM exams.grade_scale WHERE institution_id = ",
        my_institution!(),
        " ORDER BY min_pct DESC"
    ))
    .fetch_all(&mut *conn)
    .await?;
    let (external_min_pct, internal_min_pct): (f64, f64) = sqlx::query_as(concat!(
        "SELECT external_min_pct::float8, internal_min_pct::float8 FROM exams.grading_policy WHERE institution_id = ",
        my_institution!()
    ))
    .fetch_one(&mut *conn)
    .await?;
    Ok(Grading {
        scale,
        external_min_pct,
        internal_min_pct,
    })
}

/// The grading configuration for system actors, which have no user (one
/// institution per deployment, architecture.md A2).
pub async fn institution_grading(conn: &mut PgConnection) -> Result<Grading, sqlx::Error> {
    let scale = sqlx::query_as(
        "SELECT min_pct::float8 AS min_pct, grade, points FROM exams.grade_scale
         WHERE institution_id = (SELECT min(id) FROM org.institutions) ORDER BY min_pct DESC",
    )
    .fetch_all(&mut *conn)
    .await?;
    let (external_min_pct, internal_min_pct): (f64, f64) = sqlx::query_as(
        "SELECT external_min_pct::float8, internal_min_pct::float8 FROM exams.grading_policy
         WHERE institution_id = (SELECT min(id) FROM org.institutions)",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(Grading {
        scale,
        external_min_pct,
        internal_min_pct,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentMark {
    pub code: String,
    pub max: f64,
    /// `None` if not entered.
    pub marks: Option<f64>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SubjectMarks {
    /// 1 for the regular attempt, then 2, 3… for backlog attempts.
    pub attempt: i16,
    /// The result set's kind: regular, backlog or revaluation.
    pub kind: String,
    pub semester: i16,
    pub academic_year: i16,
    pub published_at: Option<DateTime<Utc>>,
    pub code: String,
    pub name: String,
    pub credits: i16,
    pub external: f64,
    pub external_max: i16,
    pub components: Json<Vec<ComponentMark>>,
}

macro_rules! marks_sql {
    () => {
        "SELECT em.attempt, rp.kind, sub.semester, o.academic_year, rp.published_at, sub.code, sub.name, sub.credits,
                em.marks::float8 AS external, em.max_marks AS external_max,
                coalesce((SELECT jsonb_agg(jsonb_build_object('code', c.code, 'max', c.max_marks,
                                                              'marks', im.marks)
                                           ORDER BY c.position)
                          FROM exams.internal_components c
                          LEFT JOIN exams.internal_marks im
                                 ON im.component_id = c.id AND im.student_id = em.student_id
                          WHERE c.offering_id = o.id), '[]'::jsonb) AS components
         FROM exams.external_marks em
         JOIN exams.result_publications rp ON rp.id = em.publication_id
         JOIN academic.offerings o ON o.id = em.offering_id
         JOIN academic.subjects sub ON sub.id = o.subject_id
         WHERE em.student_id = $1"
    };
}

/// Every subject result the acting user may see for one student, in
/// semester, code and attempt order (RLS: students and HODs see published
/// results only; the exam cell sees staged ones too).
#[tracing::instrument(name = "db.student_marks", skip_all, fields(otel.kind = "client"))]
pub async fn student_marks(
    conn: &mut PgConnection,
    student_id: Uuid,
) -> Result<Vec<SubjectMarks>, sqlx::Error> {
    sqlx::query_as(concat!(
        marks_sql!(),
        " ORDER BY sub.semester, sub.code, em.attempt"
    ))
    .bind(student_id)
    .fetch_all(conn)
    .await
}

/// Published marks only, whoever asks: what a grade card shows.
pub async fn published_marks(
    conn: &mut PgConnection,
    student_id: Uuid,
) -> Result<Vec<SubjectMarks>, sqlx::Error> {
    sqlx::query_as(concat!(
        marks_sql!(),
        " AND rp.published_at IS NOT NULL ORDER BY sub.semester, sub.code, em.attempt"
    ))
    .bind(student_id)
    .fetch_all(conn)
    .await
}

#[derive(Debug, sqlx::FromRow)]
pub struct CardStudent {
    pub student_id: Uuid,
    pub student_name: String,
    pub roll_no: String,
    pub program: String,
}

/// The students with a mark in a published result set, for their grade cards,
/// skipping those whose card already reflects this set or a later one.
pub async fn card_students(
    conn: &mut PgConnection,
    publication: Uuid,
) -> Result<Vec<CardStudent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT DISTINCT st.user_id AS student_id, u.display_name AS student_name, st.roll_no, p.name AS program
         FROM exams.external_marks em
         JOIN exams.result_publications rp ON rp.id = em.publication_id AND rp.published_at IS NOT NULL
         JOIN people.students st ON st.user_id = em.student_id
         JOIN identity.users u ON u.id = st.user_id
         JOIN org.programs p ON p.id = st.program_id
         WHERE em.publication_id = $1
           AND NOT EXISTS (SELECT 1 FROM exams.grade_cards gc
                           JOIN exams.result_publications done ON done.id = gc.publication_id
                           WHERE gc.student_id = em.student_id AND gc.semester = rp.semester
                             AND done.published_at >= rp.published_at)
         ORDER BY st.roll_no",
    )
    .bind(publication)
    .fetch_all(conn)
    .await
}

/// Records a student's grade card for a semester, unless a card from a
/// later publication got there first (events can arrive out of order).
pub async fn record_card(
    conn: &mut PgConnection,
    student: Uuid,
    semester: i16,
    publication: Uuid,
    object: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO exams.grade_cards AS gc (student_id, semester, publication_id, pdf_object)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (student_id, semester) DO UPDATE
         SET publication_id = EXCLUDED.publication_id, pdf_object = EXCLUDED.pdf_object, generated_at = now()
         WHERE (SELECT published_at FROM exams.result_publications WHERE id = gc.publication_id)
               <= (SELECT published_at FROM exams.result_publications WHERE id = EXCLUDED.publication_id)",
    )
    .bind(student)
    .bind(semester)
    .bind(publication)
    .bind(object)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Where a student's grade card for a semester is, if they have one (RLS:
/// their own, or staff).
pub async fn card_object(
    conn: &mut PgConnection,
    student: Uuid,
    semester: i16,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT pdf_object FROM exams.grade_cards WHERE student_id = $1 AND semester = $2",
    )
    .bind(student)
    .bind(semester)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Publication {
    pub id: Uuid,
    pub program_id: i16,
    pub program_name: String,
    pub semester: i16,
    pub academic_year: i16,
    pub kind: String,
    pub staged_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub published_by: Option<String>,
    /// Who asked to publish: a different person must approve.
    pub requested_by_id: Option<Uuid>,
    pub requested_by: Option<String>,
    pub requested_at: Option<DateTime<Utc>>,
    pub students: i64,
    pub subjects: i64,
    /// A withdrawal asked for (published sets): a different person approves.
    pub withdrawal_requested_by_id: Option<Uuid>,
    pub withdrawal_requested_by: Option<String>,
    pub withdrawal_requested_at: Option<DateTime<Utc>>,
    pub withdrawal_reason: Option<String>,
    /// Times it was withdrawn before (and republished).
    pub withdrawals: i64,
}

macro_rules! publication_sql {
    () => {
        "SELECT rp.id, rp.program_id, p.name AS program_name, rp.semester, rp.academic_year, rp.kind,
           rp.staged_at, rp.published_at, u.display_name AS published_by,
           rp.requested_by AS requested_by_id, r.display_name AS requested_by, rp.requested_at,
           (SELECT count(DISTINCT student_id) FROM exams.external_marks WHERE publication_id = rp.id) AS students,
           (SELECT count(DISTINCT offering_id) FROM exams.external_marks WHERE publication_id = rp.id) AS subjects,
           rp.withdrawal_requested_by AS withdrawal_requested_by_id, w.display_name AS withdrawal_requested_by,
           rp.withdrawal_requested_at, rp.withdrawal_reason,
           (SELECT count(*) FROM exams.publication_withdrawals pw WHERE pw.publication_id = rp.id) AS withdrawals
    FROM exams.result_publications rp
    JOIN org.programs p ON p.id = rp.program_id
    LEFT JOIN identity.users u ON u.id = rp.published_by
    LEFT JOIN identity.users r ON r.id = rp.requested_by
    LEFT JOIN identity.users w ON w.id = rp.withdrawal_requested_by"
    };
}

/// Staged first (they need action), then newest published.
pub async fn publications(conn: &mut PgConnection) -> Result<Vec<Publication>, sqlx::Error> {
    sqlx::query_as(concat!(
        publication_sql!(),
        " ORDER BY rp.published_at IS NOT NULL, rp.published_at DESC, p.name, rp.semester"
    ))
    .fetch_all(conn)
    .await
}

pub async fn publication(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<Publication>, sqlx::Error> {
    sqlx::query_as(concat!(publication_sql!(), " WHERE rp.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub enum PublishOutcome {
    Published,
    /// Already published (a double click, or two people at once).
    AlreadyPublished,
    /// Nobody has asked to publish it yet.
    NotRequested,
    /// The caller asked to publish it, and can't also approve it.
    SamePerson,
}

/// Publishes a requested result set as the acting user, who must not be the
/// one who requested it. The checks are in the UPDATE itself, and the
/// table's constraint backs them, so two people at once can't both win.
pub async fn publish(conn: &mut PgConnection, id: Uuid) -> Result<PublishOutcome, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE exams.result_publications
         SET published_at = now(), published_by = identity.current_user_id()
         WHERE id = $1 AND published_at IS NULL
           AND requested_by IS NOT NULL AND requested_by <> identity.current_user_id()",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    if result.rows_affected() == 1 {
        return Ok(PublishOutcome::Published);
    }
    let state: Option<(bool, Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        "SELECT published_at IS NOT NULL, requested_by, identity.current_user_id()
         FROM exams.result_publications WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?;
    Ok(match state {
        Some((true, _, _)) => PublishOutcome::AlreadyPublished,
        Some((false, None, _)) | None => PublishOutcome::NotRequested,
        Some((false, Some(_), _)) => PublishOutcome::SamePerson,
    })
}

/// Asks for a staged set to be published (step one of two). `false` if it
/// isn't staged, or was already requested.
pub async fn request_publication(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE exams.result_publications
         SET requested_by = identity.current_user_id(), requested_at = now()
         WHERE id = $1 AND published_at IS NULL AND requested_by IS NULL",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Withdraws a request (to correct marks before anyone approves).
pub async fn withdraw_request(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE exams.result_publications SET requested_by = NULL, requested_at = NULL
         WHERE id = $1 AND published_at IS NULL AND requested_by = identity.current_user_id()",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Deserialize)]
pub struct NewResultSet {
    pub program_id: i16,
    pub semester: i16,
    pub academic_year: i16,
    pub kind: String,
}

pub async fn create_set(conn: &mut PgConnection, set: &NewResultSet) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO exams.result_publications (program_id, semester, academic_year, kind)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(set.program_id)
    .bind(set.semester)
    .bind(set.academic_year)
    .bind(&set.kind)
    .fetch_one(conn)
    .await
}

// ── Marks entry ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Component {
    #[serde(default)]
    pub id: Option<Uuid>,
    pub code: String,
    pub max_marks: i16,
    #[serde(default)]
    pub position: i16,
}

pub async fn components(
    conn: &mut PgConnection,
    offering: Uuid,
) -> Result<Vec<Component>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, code, max_marks, position FROM exams.internal_components WHERE offering_id = $1 ORDER BY position",
    )
    .bind(offering)
    .fetch_all(conn)
    .await
}

/// Replaces an offering's components. Fails (by RLS or foreign key) if the
/// caller doesn't teach it, it's published, or a removed component has marks.
pub async fn replace_components(
    conn: &mut PgConnection,
    offering: Uuid,
    components: &[Component],
) -> Result<(), sqlx::Error> {
    let keep: Vec<Uuid> = components.iter().filter_map(|c| c.id).collect();
    sqlx::query(
        "DELETE FROM exams.internal_components WHERE offering_id = $1 AND NOT (id = ANY($2))",
    )
    .bind(offering)
    .bind(&keep)
    .execute(&mut *conn)
    .await?;
    for (position, c) in components.iter().enumerate() {
        match c.id {
            Some(id) => {
                sqlx::query("UPDATE exams.internal_components SET code = $3, max_marks = $4, position = $5 WHERE id = $1 AND offering_id = $2")
                    .bind(id)
                    .bind(offering)
                    .bind(&c.code)
                    .bind(c.max_marks)
                    .bind(position as i16 + 1)
                    .execute(&mut *conn)
                    .await?;
            }
            None => {
                sqlx::query("INSERT INTO exams.internal_components (offering_id, code, max_marks, position) VALUES ($1, $2, $3, $4)")
                    .bind(offering)
                    .bind(&c.code)
                    .bind(c.max_marks)
                    .bind(position as i16 + 1)
                    .execute(&mut *conn)
                    .await?;
            }
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct MarksRow {
    pub student_id: Uuid,
    pub roll_no: String,
    pub display_name: String,
    /// Component id → marks, as entered so far.
    pub marks: Json<std::collections::HashMap<Uuid, f64>>,
}

/// The students who take an offering (its program, in the subject's
/// semester, in the offering's year) with their internal marks so far.
pub async fn internal_roster(
    conn: &mut PgConnection,
    offering: Uuid,
) -> Result<Vec<MarksRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT st.user_id AS student_id, st.roll_no, u.display_name,
                coalesce((SELECT jsonb_object_agg(im.component_id, im.marks)
                          FROM exams.internal_marks im JOIN exams.internal_components c ON c.id = im.component_id
                          WHERE c.offering_id = o.id AND im.student_id = st.user_id), '{}'::jsonb) AS marks
         FROM academic.offerings o
         JOIN academic.subjects s ON s.id = o.subject_id
         JOIN people.students st ON st.program_id = s.program_id
                                AND st.batch_year + (s.semester - 1) / 2 = o.academic_year
         JOIN identity.users u ON u.id = st.user_id
         WHERE o.id = $1
         ORDER BY st.roll_no",
    )
    .bind(offering)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Deserialize)]
pub struct MarkEntry {
    pub student_id: Uuid,
    pub component_id: Uuid,
    pub marks: f64,
}

/// Upserts internal marks; RLS refuses them once the result is published.
pub async fn save_internal(
    conn: &mut PgConnection,
    entries: &[MarkEntry],
) -> Result<u64, sqlx::Error> {
    let students: Vec<Uuid> = entries.iter().map(|e| e.student_id).collect();
    let components: Vec<Uuid> = entries.iter().map(|e| e.component_id).collect();
    let marks: Vec<f64> = entries.iter().map(|e| e.marks).collect();
    let result = sqlx::query(
        "INSERT INTO exams.internal_marks (student_id, component_id, marks)
         SELECT s, c, m::numeric FROM unnest($1::uuid[], $2::uuid[], $3::float8[]) AS e(s, c, m)
         ON CONFLICT (student_id, component_id) DO UPDATE SET marks = EXCLUDED.marks",
    )
    .bind(&students)
    .bind(&components)
    .bind(&marks)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExternalRow {
    pub student_id: Uuid,
    pub roll_no: String,
    pub display_name: String,
    pub attempt: i16,
    pub marks: Option<f64>,
}

/// Who sits a result set's paper for one subject, with marks entered so far.
/// Regular: the batch that takes it that year. Backlog: students who
/// registered the subject on a backlog form that year; their attempt is one
/// more than their last. Re-evaluation: whose changed mark is in the set.
pub async fn external_roster(
    conn: &mut PgConnection,
    set: Uuid,
    offering: Uuid,
) -> Result<Vec<ExternalRow>, sqlx::Error> {
    sqlx::query_as(
        "WITH set AS (SELECT * FROM exams.result_publications WHERE id = $1),
         candidates AS (
             SELECT st.user_id
             FROM set JOIN academic.offerings o ON o.id = $2
             JOIN academic.subjects s ON s.id = o.subject_id AND s.program_id = set.program_id AND s.semester = set.semester
             JOIN people.students st ON st.program_id = s.program_id
             WHERE (set.kind = 'regular' AND st.batch_year + (s.semester - 1) / 2 = o.academic_year)
                OR (set.kind = 'backlog' AND EXISTS (
                       SELECT 1 FROM exams.exam_forms f
                       JOIN exams.exam_form_subjects fs ON fs.form_id = f.id
                       JOIN exams.exam_sessions x ON x.id = f.session_id
                       WHERE f.student_id = st.user_id AND f.kind = 'backlog' AND f.status = 'submitted'
                         AND fs.offering_id = o.id AND x.academic_year = set.academic_year))
                OR (set.kind = 'revaluation' AND EXISTS (
                       SELECT 1 FROM exams.revaluation_requests r
                       WHERE r.publication_id = set.id AND r.student_id = st.user_id AND r.offering_id = o.id))
         )
         SELECT c.user_id AS student_id, st.roll_no, u.display_name,
                coalesce(mine.attempt, (SELECT coalesce(max(em.attempt), 0) + 1 FROM exams.external_marks em
                                        WHERE em.student_id = c.user_id AND em.offering_id = $2))::smallint AS attempt,
                mine.marks::float8 AS marks
         FROM candidates c
         JOIN people.students st ON st.user_id = c.user_id
         JOIN identity.users u ON u.id = c.user_id
         LEFT JOIN exams.external_marks mine ON mine.student_id = c.user_id AND mine.offering_id = $2 AND mine.publication_id = $1
         ORDER BY st.roll_no",
    )
    .bind(set)
    .bind(offering)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Deserialize)]
pub struct ExternalEntry {
    pub student_id: Uuid,
    pub attempt: i16,
    pub marks: f64,
}

/// Upserts external marks into a staged set; RLS refuses a published one.
pub async fn save_external(
    conn: &mut PgConnection,
    set: Uuid,
    offering: Uuid,
    max_marks: i16,
    entries: &[ExternalEntry],
) -> Result<u64, sqlx::Error> {
    let students: Vec<Uuid> = entries.iter().map(|e| e.student_id).collect();
    let attempts: Vec<i16> = entries.iter().map(|e| e.attempt).collect();
    let marks: Vec<f64> = entries.iter().map(|e| e.marks).collect();
    let result = sqlx::query(
        "INSERT INTO exams.external_marks (student_id, offering_id, publication_id, marks, max_marks, attempt)
         SELECT s, $2, $1, m::numeric, $3, a FROM unnest($4::uuid[], $5::int2[], $6::float8[]) AS e(s, a, m)
         ON CONFLICT (student_id, offering_id, attempt) DO UPDATE SET marks = EXCLUDED.marks
         WHERE exams.external_marks.publication_id = $1",
    )
    .bind(set)
    .bind(offering)
    .bind(max_marks)
    .bind(&students)
    .bind(&attempts)
    .bind(&marks)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}

/// The offerings a result set covers: the program's subjects of that
/// semester, in the set's year (regular) or the year they were taught
/// (backlog); for re-evaluation, only those with a changed mark.
pub async fn set_offerings(
    conn: &mut PgConnection,
    set: Uuid,
) -> Result<Vec<(Uuid, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT o.id, s.code, s.name
         FROM exams.result_publications rp
         JOIN academic.subjects s ON s.program_id = rp.program_id AND s.semester = rp.semester
         JOIN academic.offerings o ON o.subject_id = s.id
              AND (rp.kind <> 'regular' OR o.academic_year = rp.academic_year)
         WHERE rp.id = $1
           AND (rp.kind <> 'revaluation'
                OR EXISTS (SELECT 1 FROM exams.external_marks em WHERE em.publication_id = rp.id AND em.offering_id = o.id))
         ORDER BY s.code, o.academic_year DESC",
    )
    .bind(set)
    .fetch_all(conn)
    .await
}

/// Whether the caller teaches an offering, and whether its result is
/// published (marks then lock). `None` if there's no such offering.
pub async fn offering_access(
    conn: &mut PgConnection,
    offering: Uuid,
) -> Result<Option<(bool, bool)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT academic.teaches(id), exams.offering_published(id) FROM academic.offerings WHERE id = $1",
    )
    .bind(offering)
    .fetch_optional(conn)
    .await
}

/// Asks to withdraw a published set (step one of two); false if it isn't
/// published or a withdrawal is already asked for.
pub async fn request_withdrawal(
    conn: &mut PgConnection,
    id: Uuid,
    reason: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT exams.request_withdrawal($1, $2)")
        .bind(id)
        .bind(reason)
        .fetch_one(conn)
        .await
}

pub async fn cancel_withdrawal(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT exams.cancel_withdrawal($1)")
        .bind(id)
        .fetch_one(conn)
        .await
}

/// "withdrawn", "not-requested", "same-person" or "revaluations-open".
pub async fn approve_withdrawal(conn: &mut PgConnection, id: Uuid) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT exams.approve_withdrawal($1)")
        .bind(id)
        .fetch_one(conn)
        .await
}
