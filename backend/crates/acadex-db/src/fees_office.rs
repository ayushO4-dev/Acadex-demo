//! The accounts office (role `accounts`): fee structures, ledgers,
//! concessions, refunds, and the reconciliation queue. Amounts are paise.
//! Row-level security lets only `accounts` (and, for reconciliation, the
//! worker) do any of this.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

use crate::fees::{ChargeLine, charge_lines};

// ── Overview ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProgramDues {
    pub program_id: i16,
    pub program: String,
    pub name: String,
    pub students: i64,
    pub receivable: i64,
    pub concession: i64,
    pub paid: i64,
    pub pending: i64,
    /// Students who still owe something.
    pub with_dues: i64,
}

/// Each program's charges for an academic year, and what's been settled.
pub async fn dues_by_program(
    conn: &mut PgConnection,
    academic_year: i16,
) -> Result<Vec<ProgramDues>, sqlx::Error> {
    sqlx::query_as(concat!(
        "WITH l AS (",
        charge_lines!(),
        "), per_student AS (
             SELECT l.student_id, sum(l.receivable) AS receivable, sum(l.concession) AS concession,
                    sum(l.paid) AS paid
             FROM l WHERE l.academic_year = $1 GROUP BY l.student_id
         )
         SELECT p.id AS program_id, p.code AS program, p.name,
                count(ps.student_id) AS students,
                coalesce(sum(ps.receivable), 0)::bigint AS receivable,
                coalesce(sum(ps.concession), 0)::bigint AS concession,
                coalesce(sum(ps.paid), 0)::bigint AS paid,
                coalesce(sum(ps.receivable - ps.concession - ps.paid), 0)::bigint AS pending,
                count(*) FILTER (WHERE ps.receivable - ps.concession - ps.paid > 0) AS with_dues
         FROM org.programs p
         LEFT JOIN people.students st ON st.program_id = p.id
         LEFT JOIN per_student ps ON ps.student_id = st.user_id
         GROUP BY p.id ORDER BY p.code"
    ))
    .bind(academic_year)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Default, Serialize, sqlx::FromRow)]
pub struct Collections {
    /// Net of refunds, in the financial year.
    pub financial_year: i64,
    pub last_30_days: i64,
    pub today: i64,
    pub payments_today: i64,
    pub refunded_financial_year: i64,
}

pub async fn collections(
    conn: &mut PgConnection,
    fy_start: NaiveDate,
) -> Result<Collections, sqlx::Error> {
    sqlx::query_as(
        "WITH ist AS (SELECT (now() AT TIME ZONE 'Asia/Kolkata')::date AS today)
         SELECT
           (coalesce((SELECT sum(amount_paise) FROM fees.payments
                      WHERE captured_at >= $1::timestamp AT TIME ZONE 'Asia/Kolkata'), 0)
            - coalesce((SELECT sum(amount_paise) FROM fees.refunds WHERE status = 'processed'
                        AND payment_id IS NOT NULL
                        AND processed_at >= $1::timestamp AT TIME ZONE 'Asia/Kolkata'), 0))::bigint
             AS financial_year,
           coalesce((SELECT sum(amount_paise) FROM fees.payments
                     WHERE captured_at >= now() - interval '30 days'), 0)::bigint AS last_30_days,
           coalesce((SELECT sum(amount_paise) FROM fees.payments, ist
                     WHERE captured_at >= ist.today::timestamp AT TIME ZONE 'Asia/Kolkata'), 0)::bigint AS today,
           (SELECT count(*) FROM fees.payments, ist
            WHERE captured_at >= ist.today::timestamp AT TIME ZONE 'Asia/Kolkata') AS payments_today,
           coalesce((SELECT sum(amount_paise) FROM fees.refunds WHERE status = 'processed'
                     AND processed_at >= $1::timestamp AT TIME ZONE 'Asia/Kolkata'), 0)::bigint
             AS refunded_financial_year",
    )
    .bind(fy_start)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RecentPayment {
    pub id: Uuid,
    pub student_id: Uuid,
    pub student: String,
    pub roll_no: String,
    pub amount_paise: i64,
    pub method: Option<String>,
    pub captured_at: DateTime<Utc>,
    pub receipt_no: Option<String>,
}

pub async fn recent_payments(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<RecentPayment>, sqlx::Error> {
    sqlx::query_as(
        "SELECT p.id, p.student_id, u.display_name AS student, st.roll_no, p.amount_paise, p.method,
                p.captured_at, r.receipt_no
         FROM fees.payments p
         JOIN people.students st ON st.user_id = p.student_id
         JOIN identity.users u ON u.id = p.student_id
         LEFT JOIN fees.receipts r ON r.payment_id = p.id
         ORDER BY p.captured_at DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(conn)
    .await
}

// ── Fee structures ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, serde::Deserialize, sqlx::FromRow)]
pub struct StructureLine {
    pub id: i32,
    pub fee_head: String,
    pub particulars: String,
    pub amount_paise: i64,
    pub due_on: NaiveDate,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Structure {
    pub id: i32,
    pub program_id: i16,
    pub program: String,
    pub academic_year: i16,
    pub semester: i16,
    pub applied_at: Option<DateTime<Utc>>,
    pub lines: Json<Vec<StructureLine>>,
    /// Students now in that program and semester.
    pub students: i64,
    /// Of them, charged every line.
    pub fully_charged: i64,
}

pub async fn structures(
    conn: &mut PgConnection,
    academic_year: i16,
) -> Result<Vec<Structure>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id, s.program_id, p.code AS program, s.academic_year, s.semester, s.applied_at,
                coalesce((SELECT jsonb_agg(jsonb_build_object('id', l.id, 'fee_head', l.fee_head,
                                  'particulars', l.particulars, 'amount_paise', l.amount_paise,
                                  'due_on', l.due_on) ORDER BY l.due_on, l.id)
                          FROM fees.fee_structure_lines l WHERE l.structure_id = s.id), '[]') AS lines,
                (SELECT count(*) FROM people.students st
                 WHERE st.program_id = s.program_id AND st.current_semester = s.semester AND st.graduated_on IS NULL) AS students,
                (SELECT count(*) FROM people.students st
                 WHERE st.program_id = s.program_id AND st.current_semester = s.semester AND st.graduated_on IS NULL
                   AND EXISTS (SELECT 1 FROM fees.fee_structure_lines l WHERE l.structure_id = s.id)
                   AND NOT EXISTS (SELECT 1 FROM fees.fee_structure_lines l
                                   WHERE l.structure_id = s.id
                                     AND NOT EXISTS (SELECT 1 FROM fees.ledger c
                                                     WHERE c.student_id = st.user_id
                                                       AND c.structure_line_id = l.id))) AS fully_charged
         FROM fees.fee_structures s JOIN org.programs p ON p.id = s.program_id
         WHERE s.academic_year = $1
         ORDER BY p.code, s.semester",
    )
    .bind(academic_year)
    .fetch_all(conn)
    .await
}

/// `None` if that program already has a structure for the semester.
pub async fn create_structure(
    conn: &mut PgConnection,
    program_id: i16,
    academic_year: i16,
    semester: i16,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO fees.fee_structures (program_id, academic_year, semester, created_by)
         VALUES ($1, $2, $3, identity.current_user_id())
         ON CONFLICT (program_id, academic_year, semester) DO NOTHING RETURNING id",
    )
    .bind(program_id)
    .bind(academic_year)
    .bind(semester)
    .fetch_optional(conn)
    .await
}

pub struct NewLine<'a> {
    pub fee_head: &'a str,
    pub particulars: &'a str,
    pub amount_paise: i64,
    pub due_on: NaiveDate,
}

/// `None` if the structure doesn't exist or already has that line.
pub async fn add_line(
    conn: &mut PgConnection,
    structure_id: i32,
    line: &NewLine<'_>,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO fees.fee_structure_lines (structure_id, fee_head, particulars, amount_paise, due_on)
         SELECT id, $2, $3, $4, $5 FROM fees.fee_structures WHERE id = $1
         ON CONFLICT (structure_id, particulars) DO NOTHING RETURNING id",
    )
    .bind(structure_id)
    .bind(line.fee_head)
    .bind(line.particulars)
    .bind(line.amount_paise)
    .bind(line.due_on)
    .fetch_optional(conn)
    .await
}

pub enum RemoveLine {
    Removed,
    NotFound,
    /// Students were already charged for it; the ledger is permanent.
    Charged,
}

pub async fn remove_line(
    conn: &mut PgConnection,
    structure_id: i32,
    line_id: i32,
) -> Result<RemoveLine, sqlx::Error> {
    let charged: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM fees.ledger WHERE structure_line_id = $1)",
    )
    .bind(line_id)
    .fetch_one(&mut *conn)
    .await?;
    if charged {
        return Ok(RemoveLine::Charged);
    }
    let result =
        sqlx::query("DELETE FROM fees.fee_structure_lines WHERE id = $1 AND structure_id = $2")
            .bind(line_id)
            .bind(structure_id)
            .execute(conn)
            .await?;
    Ok(if result.rows_affected() == 1 {
        RemoveLine::Removed
    } else {
        RemoveLine::NotFound
    })
}

pub enum EditLine {
    Saved,
    NotFound,
    /// Students were charged for it; the ledger is permanent.
    Charged,
    /// Another line already has those particulars.
    Duplicate,
}

pub async fn update_line(
    conn: &mut PgConnection,
    structure_id: i32,
    line_id: i32,
    line: &NewLine<'_>,
) -> Result<EditLine, sqlx::Error> {
    let charged: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM fees.ledger WHERE structure_line_id = $1)",
    )
    .bind(line_id)
    .fetch_one(&mut *conn)
    .await?;
    if charged {
        return Ok(EditLine::Charged);
    }
    let result = sqlx::query(
        "UPDATE fees.fee_structure_lines SET fee_head = $3, particulars = $4, amount_paise = $5, due_on = $6
         WHERE id = $1 AND structure_id = $2",
    )
    .bind(line_id)
    .bind(structure_id)
    .bind(line.fee_head)
    .bind(line.particulars)
    .bind(line.amount_paise)
    .bind(line.due_on)
    .execute(conn)
    .await;
    match result {
        Ok(r) if r.rows_affected() == 1 => Ok(EditLine::Saved),
        Ok(_) => Ok(EditLine::NotFound),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => Ok(EditLine::Duplicate),
        Err(err) => Err(err),
    }
}

pub enum DeleteStructure {
    Deleted,
    NotFound,
    /// Some of its lines are charged.
    Charged,
}

/// Deletes a structure nobody has been charged from (its lines go with it).
pub async fn delete_structure(
    conn: &mut PgConnection,
    structure_id: i32,
) -> Result<DeleteStructure, sqlx::Error> {
    let charged: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM fees.ledger c JOIN fees.fee_structure_lines l ON l.id = c.structure_line_id
                        WHERE l.structure_id = $1)",
    )
    .bind(structure_id)
    .fetch_one(&mut *conn)
    .await?;
    if charged {
        return Ok(DeleteStructure::Charged);
    }
    let result = sqlx::query("DELETE FROM fees.fee_structures WHERE id = $1")
        .bind(structure_id)
        .execute(conn)
        .await?;
    Ok(if result.rows_affected() == 1 {
        DeleteStructure::Deleted
    } else {
        DeleteStructure::NotFound
    })
}

/// Charges every student now in the structure's program and semester for
/// each line they haven't been charged yet. Returns `None` if there's no such
/// structure, else how many charges were made.
pub async fn apply_structure(
    conn: &mut PgConnection,
    structure_id: i32,
) -> Result<Option<u64>, sqlx::Error> {
    let exists: Option<i32> =
        sqlx::query_scalar("SELECT id FROM fees.fee_structures WHERE id = $1 FOR UPDATE")
            .bind(structure_id)
            .fetch_optional(&mut *conn)
            .await?;
    if exists.is_none() {
        return Ok(None);
    }
    let result = sqlx::query(
        "INSERT INTO fees.ledger (student_id, academic_year, kind, particulars, amount_paise, fee_head,
                                  due_on, structure_line_id, recorded_by)
         SELECT st.user_id, s.academic_year, 'charge', l.particulars, l.amount_paise, l.fee_head,
                l.due_on, l.id, identity.current_user_id()
         FROM fees.fee_structures s
         JOIN fees.fee_structure_lines l ON l.structure_id = s.id
         JOIN people.students st ON st.program_id = s.program_id AND st.current_semester = s.semester AND st.graduated_on IS NULL
         WHERE s.id = $1
         ON CONFLICT (student_id, structure_line_id) WHERE structure_line_id IS NOT NULL DO NOTHING",
    )
    .bind(structure_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE fees.fee_structures SET applied_at = now(), applied_by = identity.current_user_id()
         WHERE id = $1",
    )
    .bind(structure_id)
    .execute(conn)
    .await?;
    Ok(Some(result.rows_affected()))
}

// ── Ledgers ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StudentDues {
    pub id: Uuid,
    pub name: String,
    pub roll_no: String,
    pub program: String,
    pub semester: i16,
    pub receivable: i64,
    pub concession: i64,
    pub paid: i64,
    pub pending: i64,
}

#[derive(Debug, Default)]
pub struct LedgerFilter {
    pub search: Option<String>,
    pub program_id: Option<i16>,
    /// Only students who owe something (overdue or not).
    pub with_dues: bool,
    /// Only students with something past its due date.
    pub overdue: bool,
}

/// Students with their totals over every charge, by roll number, `limit`
/// after `after_roll` (keyset).
pub async fn ledgers(
    conn: &mut PgConnection,
    filter: &LedgerFilter,
    after_roll: Option<&str>,
    limit: i64,
) -> Result<Vec<StudentDues>, sqlx::Error> {
    let pattern = filter.search.as_deref().map(crate::search::contains);
    sqlx::query_as(concat!(
        "WITH l AS (",
        charge_lines!(),
        "), per_student AS (
             SELECT l.student_id, sum(l.receivable) AS receivable, sum(l.concession) AS concession,
                    sum(l.paid) AS paid,
                    bool_or(l.due_on < (now() AT TIME ZONE 'Asia/Kolkata')::date
                            AND l.receivable - l.concession - l.paid > 0) AS overdue
             FROM l GROUP BY l.student_id
         )
         SELECT st.user_id AS id, u.display_name AS name, st.roll_no, p.code AS program,
                st.current_semester AS semester,
                coalesce(ps.receivable, 0)::bigint AS receivable,
                coalesce(ps.concession, 0)::bigint AS concession,
                coalesce(ps.paid, 0)::bigint AS paid,
                coalesce(ps.receivable - ps.concession - ps.paid, 0)::bigint AS pending
         FROM people.students st
         JOIN identity.users u ON u.id = st.user_id
         JOIN org.programs p ON p.id = st.program_id
         LEFT JOIN per_student ps ON ps.student_id = st.user_id
         WHERE ($1::text IS NULL OR u.display_name ILIKE $1 OR st.roll_no ILIKE $1)
           AND ($2::smallint IS NULL OR st.program_id = $2)
           AND (NOT $3 OR coalesce(ps.receivable - ps.concession - ps.paid, 0) > 0)
           AND (NOT $4 OR coalesce(ps.overdue, false))
           AND ($5::text IS NULL OR st.roll_no > $5)
         ORDER BY st.roll_no
         LIMIT $6"
    ))
    .bind(pattern)
    .bind(filter.program_id)
    .bind(filter.with_dues)
    .bind(filter.overdue)
    .bind(after_roll)
    .bind(limit)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StudentHeader {
    pub id: Uuid,
    pub name: String,
    pub roll_no: String,
    pub program: String,
    pub semester: i16,
    pub batch_year: i16,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Entry {
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub kind: String,
    pub academic_year: i16,
    /// For a charge, what it's for; otherwise the charge it settles.
    pub particulars: String,
    pub charge_id: Option<i64>,
    pub amount_paise: i64,
    pub receipt_no: Option<String>,
    pub remarks: Option<String>,
    pub recorded_by: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PaymentRow {
    pub id: Uuid,
    pub gateway_payment_id: String,
    pub amount_paise: i64,
    pub method: Option<String>,
    pub captured_at: DateTime<Utc>,
    pub receipt_no: Option<String>,
    /// Refunded or being refunded.
    pub refunded_paise: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RefundRow {
    pub id: Uuid,
    pub student_id: Uuid,
    pub student: String,
    pub roll_no: String,
    pub payment_id: Option<Uuid>,
    pub gateway_payment_id: String,
    pub amount_paise: i64,
    pub reason_category: String,
    pub reason: String,
    pub status: String,
    pub gateway_refund_id: Option<String>,
    pub failure: Option<String>,
    pub requested_by: String,
    pub requested_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
    pub issue_id: Option<i32>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ConcessionRow {
    pub ledger_id: i64,
    pub charge_id: i64,
    pub particulars: String,
    pub amount_paise: i64,
    pub category: String,
    pub reason: String,
    pub granted_by: Option<String>,
    pub granted_at: DateTime<Utc>,
}

pub async fn student_header(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Option<StudentHeader>, sqlx::Error> {
    sqlx::query_as(
        "SELECT st.user_id AS id, u.display_name AS name, st.roll_no, p.code AS program,
                st.current_semester AS semester, st.batch_year
         FROM people.students st JOIN identity.users u ON u.id = st.user_id
         JOIN org.programs p ON p.id = st.program_id WHERE st.user_id = $1",
    )
    .bind(student)
    .fetch_optional(conn)
    .await
}

/// Every charge the student has had, with what settled each.
pub async fn charge_lines(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Vec<ChargeLine>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT l.charge_id, l.fee_head, l.academic_year, l.particulars, l.due_on, l.remarks, l.receivable,
                l.concession, l.paid, l.receivable - l.concession - l.paid AS pending
         FROM (",
        charge_lines!(),
        ") l WHERE l.student_id = $1
         ORDER BY l.academic_year DESC, l.due_on, l.charge_id"
    ))
    .bind(student)
    .fetch_all(conn)
    .await
}

/// The ledger as it was written, oldest first.
pub async fn entries(conn: &mut PgConnection, student: Uuid) -> Result<Vec<Entry>, sqlx::Error> {
    sqlx::query_as(
        "SELECT e.id, e.created_at, e.kind::text AS kind, e.academic_year,
                coalesce(e.particulars, c.particulars) AS particulars, e.charge_id, e.amount_paise,
                r.receipt_no, e.remarks, u.display_name AS recorded_by
         FROM fees.ledger e
         LEFT JOIN fees.ledger c ON c.id = e.charge_id
         LEFT JOIN fees.receipts r ON r.payment_id = e.payment_id AND e.kind = 'payment'
         LEFT JOIN identity.users u ON u.id = e.recorded_by
         WHERE e.student_id = $1
         ORDER BY e.created_at, e.id",
    )
    .bind(student)
    .fetch_all(conn)
    .await
}

pub async fn payments(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Vec<PaymentRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT p.id, p.gateway_payment_id, p.amount_paise, p.method, p.captured_at, r.receipt_no,
                coalesce((SELECT sum(f.amount_paise) FROM fees.refunds f
                          WHERE f.payment_id = p.id AND f.status <> 'failed'), 0)::bigint AS refunded_paise
         FROM fees.payments p LEFT JOIN fees.receipts r ON r.payment_id = p.id
         WHERE p.student_id = $1 ORDER BY p.captured_at DESC",
    )
    .bind(student)
    .fetch_all(conn)
    .await
}

macro_rules! refund_rows {
    () => {
        "SELECT f.id, f.student_id, u.display_name AS student, st.roll_no, f.payment_id, f.gateway_payment_id,
                f.amount_paise, f.reason_category, f.reason, f.status, f.gateway_refund_id, f.failure,
                rb.display_name AS requested_by, f.requested_at, f.processed_at, f.issue_id
         FROM fees.refunds f
         JOIN people.students st ON st.user_id = f.student_id
         JOIN identity.users u ON u.id = f.student_id
         JOIN identity.users rb ON rb.id = f.requested_by"
    };
}

pub async fn refunds_of(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Vec<RefundRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        refund_rows!(),
        " WHERE f.student_id = $1 ORDER BY f.requested_at DESC"
    ))
    .bind(student)
    .fetch_all(conn)
    .await
}

pub async fn recent_refunds(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<RefundRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        refund_rows!(),
        " ORDER BY f.requested_at DESC LIMIT $1"
    ))
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn concessions(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Vec<ConcessionRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT k.ledger_id, e.charge_id, c.particulars, e.amount_paise, k.category, k.reason,
                u.display_name AS granted_by, k.granted_at
         FROM fees.concessions k
         JOIN fees.ledger e ON e.id = k.ledger_id
         JOIN fees.ledger c ON c.id = e.charge_id
         LEFT JOIN identity.users u ON u.id = k.granted_by
         WHERE k.student_id = $1 ORDER BY k.granted_at DESC",
    )
    .bind(student)
    .fetch_all(conn)
    .await
}

// ── Concessions ──────────────────────────────────────────────────────────────

pub enum Grant {
    Granted(i64),
    NoSuchCharge,
    /// More than is still owed on the charge.
    TooMuch {
        pending: i64,
    },
}

/// Concedes part of what a student still owes on one charge. The charge row
/// is locked, so two concessions at once can't together exceed it.
pub async fn grant_concession(
    conn: &mut PgConnection,
    student: Uuid,
    charge_id: i64,
    amount_paise: i64,
    category: &str,
    reason: &str,
) -> Result<Grant, sqlx::Error> {
    // The ledger is append-only (no UPDATE grant, so no FOR UPDATE): an
    // advisory lock per charge serialises concessions on it instead.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('fees.charge'), $1::int)")
        .bind((charge_id % i64::from(i32::MAX)) as i32)
        .execute(&mut *conn)
        .await?;
    let charge: Option<i16> = sqlx::query_scalar(
        "SELECT academic_year FROM fees.ledger
         WHERE id = $1 AND student_id = $2 AND kind = 'charge'",
    )
    .bind(charge_id)
    .bind(student)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(academic_year) = charge else {
        return Ok(Grant::NoSuchCharge);
    };
    let pending: i64 = sqlx::query_scalar("SELECT fees.pending_of($1)")
        .bind(charge_id)
        .fetch_one(&mut *conn)
        .await?;
    if amount_paise > pending {
        return Ok(Grant::TooMuch { pending });
    }
    let ledger_id: i64 = sqlx::query_scalar(
        "INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, remarks, recorded_by)
         VALUES ($1, $2, 'concession', $3, $4, $5, identity.current_user_id()) RETURNING id",
    )
    .bind(student)
    .bind(academic_year)
    .bind(charge_id)
    .bind(amount_paise)
    .bind(reason)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO fees.concessions (ledger_id, student_id, category, reason, granted_by)
         VALUES ($1, $2, $3, $4, identity.current_user_id())",
    )
    .bind(ledger_id)
    .bind(student)
    .bind(category)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(Grant::Granted(ledger_id))
}

// ── Refunds ──────────────────────────────────────────────────────────────────

/// What a refund is against: one of our payments, or (for a duplicate
/// capture) only the gateway's payment.
#[derive(Debug, sqlx::FromRow)]
pub struct Refundable {
    pub student_id: Uuid,
    pub payment_id: Option<Uuid>,
    pub gateway_payment_id: String,
    pub amount_paise: i64,
    /// Refunded already, or being refunded.
    pub refunded_paise: i64,
}

/// Serialises refunds of one thing until the transaction ends, so two at
/// once can't together refund more than was paid.
async fn lock_refunds_of(conn: &mut PgConnection, what: &str) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('fees.refund'), hashtext($1))")
        .bind(what)
        .execute(conn)
        .await?;
    Ok(())
}

/// One of our payments, locked so concurrent refunds can't over-refund it.
pub async fn refundable_payment(
    conn: &mut PgConnection,
    payment_id: Uuid,
) -> Result<Option<Refundable>, sqlx::Error> {
    lock_refunds_of(conn, &payment_id.to_string()).await?;
    sqlx::query_as(
        "SELECT p.student_id, p.id AS payment_id, p.gateway_payment_id, p.amount_paise,
                coalesce((SELECT sum(f.amount_paise) FROM fees.refunds f
                          WHERE f.payment_id = p.id AND f.status <> 'failed'), 0)::bigint AS refunded_paise
         FROM fees.payments p WHERE p.id = $1",
    )
    .bind(payment_id)
    .fetch_optional(conn)
    .await
}

/// A duplicate capture's money, by the issue that reported it, locked.
pub async fn refundable_duplicate(
    conn: &mut PgConnection,
    issue_id: i32,
) -> Result<Option<Refundable>, sqlx::Error> {
    lock_refunds_of(conn, &format!("issue:{issue_id}")).await?;
    sqlx::query_as(
        "SELECT i.student_id, NULL::uuid AS payment_id, i.gateway_payment_id, i.amount_paise,
                coalesce((SELECT sum(f.amount_paise) FROM fees.refunds f
                          WHERE f.gateway_payment_id = i.gateway_payment_id AND f.status <> 'failed'), 0)::bigint
                  AS refunded_paise
         FROM fees.reconciliation_issues i
         WHERE i.id = $1 AND i.kind = 'duplicate_capture' AND i.student_id IS NOT NULL
           AND i.gateway_payment_id IS NOT NULL AND i.amount_paise IS NOT NULL",
    )
    .bind(issue_id)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RefundState {
    pub id: Uuid,
    pub status: String,
    pub amount_paise: i64,
    pub gateway_payment_id: String,
    pub gateway_refund_id: Option<String>,
}

pub async fn refund_by_key(
    conn: &mut PgConnection,
    key: &str,
) -> Result<Option<RefundState>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, status, amount_paise, gateway_payment_id, gateway_refund_id
         FROM fees.refunds WHERE idempotency_key = $1",
    )
    .bind(key)
    .fetch_optional(conn)
    .await
}

pub struct NewRefund<'a> {
    pub key: &'a str,
    pub of: &'a Refundable,
    pub amount_paise: i64,
    pub reason_category: &'a str,
    pub reason: &'a str,
    pub issue_id: Option<i32>,
}

pub async fn create_refund(
    conn: &mut PgConnection,
    refund: &NewRefund<'_>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO fees.refunds (student_id, payment_id, gateway_payment_id, amount_paise, reason_category,
                                   reason, idempotency_key, requested_by, issue_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, identity.current_user_id(), $8) RETURNING id",
    )
    .bind(refund.of.student_id)
    .bind(refund.of.payment_id)
    .bind(&refund.of.gateway_payment_id)
    .bind(refund.amount_paise)
    .bind(refund.reason_category)
    .bind(refund.reason)
    .bind(refund.key)
    .bind(refund.issue_id)
    .fetch_one(conn)
    .await
}

/// The gateway confirmed the refund: record it, and put the money back on
/// the ledger against the charges the payment settled (latest first), so
/// those charges are owed again by exactly that much. Resolves the issue it
/// was made for. `false` if it was already recorded.
pub async fn complete_refund(
    conn: &mut PgConnection,
    refund_id: Uuid,
    gateway_refund_id: &str,
) -> Result<bool, sqlx::Error> {
    let refund: Option<(Option<Uuid>, i64, Option<i32>)> = sqlx::query_as(
        "UPDATE fees.refunds SET status = 'processed', gateway_refund_id = $2, processed_at = now(), failure = NULL
         WHERE id = $1 AND status <> 'processed' RETURNING payment_id, amount_paise, issue_id",
    )
    .bind(refund_id)
    .bind(gateway_refund_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((payment_id, amount, issue_id)) = refund else {
        return Ok(false);
    };
    if let Some(payment_id) = payment_id {
        // What the payment credited to each charge, less refunds already on it.
        let credits: Vec<(i64, i64, i16, Uuid)> = sqlx::query_as(
            "SELECT e.charge_id,
                    (e.amount_paise - coalesce((SELECT sum(r.amount_paise) FROM fees.ledger r
                                                WHERE r.kind = 'refund' AND r.payment_id = e.payment_id
                                                  AND r.charge_id = e.charge_id), 0))::bigint,
                    e.academic_year, e.student_id
             FROM fees.ledger e
             WHERE e.kind = 'payment' AND e.payment_id = $1
             -- An overpaid charge's issue: that charge first.
             ORDER BY e.charge_id = (SELECT charge_id FROM fees.reconciliation_issues WHERE id = $2) DESC NULLS LAST,
                      e.charge_id DESC",
        )
        .bind(payment_id)
        .bind(issue_id)
        .fetch_all(&mut *conn)
        .await?;
        let mut left = amount;
        for (charge_id, available, academic_year, student) in credits {
            if left == 0 {
                break;
            }
            let take = left.min(available);
            if take <= 0 {
                continue;
            }
            sqlx::query(
                "INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, payment_id,
                                          remarks, recorded_by)
                 VALUES ($1, $2, 'refund', $3, $4, $5, $6, identity.current_user_id())",
            )
            .bind(student)
            .bind(academic_year)
            .bind(charge_id)
            .bind(take)
            .bind(payment_id)
            .bind(format!("Refund {gateway_refund_id}"))
            .execute(&mut *conn)
            .await?;
            left -= take;
        }
    }
    if let Some(issue_id) = issue_id {
        sqlx::query(
            "UPDATE fees.reconciliation_issues
             SET status = 'resolved', resolution = $2, resolved_by = identity.current_user_id(), resolved_at = now()
             WHERE id = $1 AND status = 'open'",
        )
        .bind(issue_id)
        .bind(format!("Refunded ₹{} ({gateway_refund_id})", rupees(amount)))
        .execute(conn)
        .await?;
    }
    Ok(true)
}

pub async fn fail_refund(
    conn: &mut PgConnection,
    refund_id: Uuid,
    failure: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE fees.refunds SET status = 'failed', failure = $2 WHERE id = $1 AND status = 'pending'")
        .bind(refund_id)
        .bind(failure)
        .execute(conn)
        .await?;
    Ok(())
}

/// ₹1,234.50 style, for remarks.
fn rupees(paise: i64) -> String {
    let rupees = paise / 100;
    let digits = rupees.to_string();
    // Indian grouping: last three digits, then pairs.
    let (head, tail) = digits.split_at(digits.len().saturating_sub(3));
    let mut groups = Vec::new();
    let mut head = head.to_owned();
    while head.len() > 2 {
        let split = head.len() - 2;
        groups.insert(0, head[split..].to_owned());
        head.truncate(split);
    }
    if !head.is_empty() {
        groups.insert(0, head);
    }
    groups.push(tail.to_owned());
    let whole = groups.join(",");
    match paise % 100 {
        0 => whole,
        p => format!("{whole}.{p:02}"),
    }
}

// ── Reconciliation ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Run {
    pub id: i32,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub window_from: DateTime<Utc>,
    pub window_to: DateTime<Utc>,
    pub gateway_payments: i32,
    pub matched: i32,
    pub recovered: i32,
    pub issues_opened: i32,
    pub error: Option<String>,
}

pub async fn runs(conn: &mut PgConnection, limit: i64) -> Result<Vec<Run>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM fees.reconciliation_runs ORDER BY id DESC LIMIT $1")
        .bind(limit)
        .fetch_all(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Issue {
    pub id: i32,
    pub kind: String,
    pub reference: String,
    pub student_id: Option<Uuid>,
    pub student: Option<String>,
    pub roll_no: Option<String>,
    pub payment_id: Option<Uuid>,
    pub gateway_payment_id: Option<String>,
    pub charge_id: Option<i64>,
    pub particulars: Option<String>,
    pub amount_paise: Option<i64>,
    pub details: Value,
    pub found_at: DateTime<Utc>,
    pub status: String,
    pub resolution: Option<String>,
    pub resolved_by: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    /// A refund for it is waiting on the gateway.
    pub refund_pending: bool,
}

/// Open ones first, newest first; the latest resolved ones after.
pub async fn issues(conn: &mut PgConnection, limit: i64) -> Result<Vec<Issue>, sqlx::Error> {
    sqlx::query_as(
        "SELECT i.id, i.kind, i.reference, i.student_id, u.display_name AS student, st.roll_no,
                i.payment_id, i.gateway_payment_id, i.charge_id, c.particulars, i.amount_paise, i.details,
                i.found_at, i.status, i.resolution, rb.display_name AS resolved_by, i.resolved_at,
                EXISTS (SELECT 1 FROM fees.refunds f WHERE f.issue_id = i.id AND f.status = 'pending')
                  AS refund_pending
         FROM fees.reconciliation_issues i
         LEFT JOIN people.students st ON st.user_id = i.student_id
         LEFT JOIN identity.users u ON u.id = i.student_id
         LEFT JOIN fees.ledger c ON c.id = i.charge_id
         LEFT JOIN identity.users rb ON rb.id = i.resolved_by
         ORDER BY i.status = 'open' DESC, i.found_at DESC, i.id DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn open_issue_count(conn: &mut PgConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM fees.reconciliation_issues WHERE status = 'open'")
        .fetch_one(conn)
        .await
}

#[derive(Debug, sqlx::FromRow)]
pub struct IssueRef {
    pub kind: String,
    pub student_id: Option<Uuid>,
    pub payment_id: Option<Uuid>,
    pub amount_paise: Option<i64>,
    pub status: String,
}

pub async fn issue(conn: &mut PgConnection, id: i32) -> Result<Option<IssueRef>, sqlx::Error> {
    sqlx::query_as(
        "SELECT kind, student_id, payment_id, amount_paise, status FROM fees.reconciliation_issues WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

/// `false` if there's no open issue with that id.
pub async fn resolve_issue(
    conn: &mut PgConnection,
    id: i32,
    resolution: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE fees.reconciliation_issues
         SET status = 'resolved', resolution = $2, resolved_by = identity.current_user_id(), resolved_at = now()
         WHERE id = $1 AND status = 'open'",
    )
    .bind(id)
    .bind(resolution)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

// For the reconciliation job (worker context).

/// Where the last finished run's window ended.
pub async fn last_window_end(
    conn: &mut PgConnection,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT max(window_to) FROM fees.reconciliation_runs WHERE finished_at IS NOT NULL AND error IS NULL",
    )
    .fetch_one(conn)
    .await
}

pub async fn start_run(
    conn: &mut PgConnection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO fees.reconciliation_runs (window_from, window_to) VALUES ($1, $2) RETURNING id",
    )
    .bind(from)
    .bind(to)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Default)]
pub struct RunTotals {
    pub gateway_payments: i32,
    pub matched: i32,
    pub recovered: i32,
    pub issues_opened: i32,
}

pub async fn finish_run(
    conn: &mut PgConnection,
    id: i32,
    totals: &RunTotals,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE fees.reconciliation_runs
         SET finished_at = now(), gateway_payments = $2, matched = $3, recovered = $4, issues_opened = $5, error = $6
         WHERE id = $1",
    )
    .bind(id)
    .bind(totals.gateway_payments)
    .bind(totals.matched)
    .bind(totals.recovered)
    .bind(totals.issues_opened)
    .bind(error)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
pub struct RecordedPayment {
    pub id: Uuid,
    pub student_id: Uuid,
    pub gateway_payment_id: String,
    pub amount_paise: i64,
}

/// Our payments captured in a window, to check the gateway lists each.
pub async fn payments_between(
    conn: &mut PgConnection,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<RecordedPayment>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, student_id, gateway_payment_id, amount_paise FROM fees.payments
         WHERE captured_at >= $1 AND captured_at < $2",
    )
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await
}

pub async fn payment_by_gateway_id(
    conn: &mut PgConnection,
    gateway_payment_id: &str,
) -> Result<Option<RecordedPayment>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, student_id, gateway_payment_id, amount_paise FROM fees.payments WHERE gateway_payment_id = $1",
    )
    .bind(gateway_payment_id)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, sqlx::FromRow)]
pub struct Overpaid {
    pub charge_id: i64,
    pub student_id: Uuid,
    pub excess: i64,
    pub payment_id: Option<Uuid>,
    pub gateway_payment_id: Option<String>,
}

/// Charges with more paid than owed, anywhere, with the latest payment on each.
pub async fn overpaid_charges(conn: &mut PgConnection) -> Result<Vec<Overpaid>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT l.charge_id, l.student_id, (l.paid + l.concession - l.receivable)::bigint AS excess,
                p.id AS payment_id, p.gateway_payment_id
         FROM (",
        charge_lines!(),
        ") l
         LEFT JOIN LATERAL (SELECT pay.id, pay.gateway_payment_id FROM fees.ledger e
                            JOIN fees.payments pay ON pay.id = e.payment_id
                            WHERE e.charge_id = l.charge_id AND e.kind = 'payment'
                            ORDER BY pay.captured_at DESC LIMIT 1) p ON true
         WHERE l.paid + l.concession > l.receivable"
    ))
    .fetch_all(conn)
    .await
}

#[cfg(test)]
mod tests {
    use super::rupees;

    #[test]
    fn rupees_use_indian_grouping() {
        assert_eq!(rupees(6_250_000), "62,500");
        assert_eq!(rupees(1_234_567_850), "1,23,45,678.50");
        assert_eq!(rupees(99), "0.99");
        assert_eq!(rupees(100_000), "1,000");
    }
}
