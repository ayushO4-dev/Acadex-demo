//! Fees: the ledger, payment attempts, and what the payment webhook records.
//! Amounts are integer paise.

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

// ── For the student ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ChargeLine {
    pub charge_id: i64,
    pub fee_head: Option<String>,
    pub academic_year: i16,
    pub particulars: String,
    pub due_on: Option<NaiveDate>,
    pub remarks: Option<String>,
    pub receivable: i64,
    pub concession: i64,
    pub paid: i64,
    /// Derived: receivable − concession − paid. Negative is an advance.
    pub pending: i64,
}

macro_rules! charge_lines {
    () => {
        "SELECT c.id AS charge_id, c.fee_head, c.student_id, c.academic_year, c.particulars, c.due_on, c.remarks,
                c.amount_paise AS receivable,
                coalesce(sum(e.amount_paise) FILTER (WHERE e.kind = 'concession'), 0)::bigint AS concession,
                (coalesce(sum(e.amount_paise) FILTER (WHERE e.kind = 'payment'), 0)
                 - coalesce(sum(e.amount_paise) FILTER (WHERE e.kind = 'refund'), 0))::bigint AS paid
         FROM fees.ledger c
         LEFT JOIN fees.ledger e ON e.charge_id = c.id
         WHERE c.kind = 'charge'
         GROUP BY c.id"
    };
}
pub(crate) use charge_lines;

/// The Payable table: this year's charges, plus any older one still owed.
#[tracing::instrument(name = "db.fees_payable", skip_all, fields(otel.kind = "client"))]
pub async fn payable(
    conn: &mut PgConnection,
    student_id: Uuid,
    academic_year: i16,
) -> Result<Vec<ChargeLine>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT l.charge_id, l.fee_head, l.academic_year, l.particulars, l.due_on, l.remarks, l.receivable,
                l.concession, l.paid, l.receivable - l.concession - l.paid AS pending
         FROM (",
        charge_lines!(),
        ") l
         WHERE l.student_id = $1
           AND (l.academic_year = $2 OR l.receivable - l.concession - l.paid > 0)
         ORDER BY l.academic_year, l.due_on, l.charge_id"
    ))
    .bind(student_id)
    .bind(academic_year)
    .fetch_all(conn)
    .await
}

/// Pending amounts of the given charges, for a new payment attempt. Charges
/// the caller can't see are simply absent.
pub async fn pending_of(
    conn: &mut PgConnection,
    charge_ids: &[i64],
) -> Result<Vec<(i64, i64)>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT l.charge_id, l.receivable - l.concession - l.paid FROM (",
        charge_lines!(),
        ") l WHERE l.charge_id = ANY($1)"
    ))
    .bind(charge_ids)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Attempt {
    pub id: Uuid,
    pub student_id: Uuid,
    pub amount_paise: i64,
    pub gateway_order_id: Option<String>,
    pub status: String,
    /// Set once the payment is confirmed.
    pub receipt_no: Option<String>,
}

macro_rules! attempt_sql {
    () => {
        "SELECT a.id, a.student_id, a.amount_paise, a.gateway_order_id, a.status, r.receipt_no
         FROM fees.payment_attempts a
         LEFT JOIN fees.payments p ON p.attempt_id = a.id
         LEFT JOIN fees.receipts r ON r.payment_id = p.id"
    };
}

pub async fn attempt(conn: &mut PgConnection, id: Uuid) -> Result<Option<Attempt>, sqlx::Error> {
    sqlx::query_as(concat!(attempt_sql!(), " WHERE a.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub async fn attempt_by_key(
    conn: &mut PgConnection,
    student_id: Uuid,
    idempotency_key: &str,
) -> Result<Option<Attempt>, sqlx::Error> {
    sqlx::query_as(concat!(
        attempt_sql!(),
        " WHERE a.student_id = $1 AND a.idempotency_key = $2"
    ))
    .bind(student_id)
    .bind(idempotency_key)
    .fetch_optional(conn)
    .await
}

/// Opens an attempt for the acting student over `(charge_id, amount)` items.
pub async fn create_attempt(
    conn: &mut PgConnection,
    idempotency_key: &str,
    items: &[(i64, i64)],
) -> Result<Uuid, sqlx::Error> {
    let total: i64 = items.iter().map(|(_, amount)| amount).sum();
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO fees.payment_attempts (student_id, idempotency_key, amount_paise)
         VALUES (identity.current_user_id(), $1, $2) RETURNING id",
    )
    .bind(idempotency_key)
    .bind(total)
    .fetch_one(&mut *conn)
    .await?;
    let (charges, amounts): (Vec<i64>, Vec<i64>) = items.iter().copied().unzip();
    sqlx::query(
        "INSERT INTO fees.payment_attempt_items (attempt_id, charge_id, amount_paise)
         SELECT $1, charge_id, amount FROM unnest($2::bigint[], $3::bigint[]) AS i(charge_id, amount)",
    )
    .bind(id)
    .bind(&charges)
    .bind(&amounts)
    .execute(conn)
    .await?;
    Ok(id)
}

pub async fn set_order(
    conn: &mut PgConnection,
    attempt_id: Uuid,
    order_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE fees.payment_attempts SET gateway_order_id = $2, updated_at = now() WHERE id = $1",
    )
    .bind(attempt_id)
    .bind(order_id)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Receipt {
    pub id: Uuid,
    pub receipt_no: String,
    pub issued_at: DateTime<Utc>,
    pub remarks: String,
    pub amount_paise: i64,
    pub pdf_ready: bool,
}

pub async fn receipts(
    conn: &mut PgConnection,
    student_id: Uuid,
) -> Result<Vec<Receipt>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, receipt_no, issued_at, remarks, amount_paise, pdf_object IS NOT NULL AS pdf_ready
         FROM fees.receipts WHERE student_id = $1 ORDER BY issued_at DESC",
    )
    .bind(student_id)
    .fetch_all(conn)
    .await
}

/// A receipt's number and stored PDF, if the acting user may see it.
pub async fn receipt_object(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<(String, Option<String>)>, sqlx::Error> {
    sqlx::query_as("SELECT receipt_no, pdf_object FROM fees.receipts WHERE id = $1")
        .bind(id)
        .fetch_optional(conn)
        .await
}

// ── For the payment webhook (system context `gateway`) ───────────────────────

/// Records a webhook event by its id. `false` if it was seen before: the
/// caller then does nothing, which is what makes redelivery harmless.
pub async fn record_event(
    conn: &mut PgConnection,
    event_id: &str,
    event_type: &str,
    payload: &Value,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO fees.gateway_events (event_id, event_type, payload) VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(payload)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// The attempt behind a gateway order, locked until the transaction ends so
/// two deliveries for one order can't both settle it.
pub async fn lock_attempt_for_order(
    conn: &mut PgConnection,
    order_id: &str,
) -> Result<Option<Attempt>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.id, a.student_id, a.amount_paise, a.gateway_order_id, a.status, NULL::text AS receipt_no
         FROM fees.payment_attempts a WHERE a.gateway_order_id = $1 FOR UPDATE",
    )
    .bind(order_id)
    .fetch_optional(conn)
    .await
}

pub async fn set_status(
    conn: &mut PgConnection,
    attempt_id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE fees.payment_attempts SET status = $2, updated_at = now() WHERE id = $1")
        .bind(attempt_id)
        .bind(status)
        .execute(conn)
        .await?;
    Ok(())
}

pub struct Capture<'a> {
    pub attempt: &'a Attempt,
    pub gateway_payment_id: &'a str,
    pub method: Option<&'a str>,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct Captured {
    pub payment_id: Uuid,
    pub receipt_id: Uuid,
    pub receipt_no: String,
}

/// Settles a captured payment: the payment, one ledger credit per charge it
/// covers, the attempt's status, and a receipt with the next gapless number
/// for the financial year. One transaction, so all or nothing.
#[tracing::instrument(name = "db.fees_capture", skip_all, fields(otel.kind = "client"))]
pub async fn capture(
    conn: &mut PgConnection,
    capture: &Capture<'_>,
) -> Result<Captured, sqlx::Error> {
    let attempt = capture.attempt;
    let payment_id: Uuid = sqlx::query_scalar(
        "INSERT INTO fees.payments (attempt_id, student_id, gateway_payment_id, amount_paise, method, captured_at)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(attempt.id)
    .bind(attempt.student_id)
    .bind(capture.gateway_payment_id)
    .bind(attempt.amount_paise)
    .bind(capture.method)
    .bind(capture.captured_at)
    .fetch_one(&mut *conn)
    .await?;

    let particulars: Vec<String> = sqlx::query_scalar(
        "WITH credited AS (
             INSERT INTO fees.ledger (student_id, academic_year, kind, charge_id, amount_paise, payment_id)
             SELECT c.student_id, c.academic_year, 'payment'::fees.entry_kind, c.id, i.amount_paise, $2
             FROM fees.payment_attempt_items i
             JOIN fees.ledger c ON c.id = i.charge_id
             WHERE i.attempt_id = $1
             RETURNING charge_id
         )
         SELECT c.particulars FROM credited JOIN fees.ledger c ON c.id = credited.charge_id ORDER BY c.id",
    )
    .bind(attempt.id)
    .bind(payment_id)
    .fetch_all(&mut *conn)
    .await?;

    set_status(conn, attempt.id, "captured").await?;

    let financial_year = financial_year_of(capture.captured_at.date_naive());
    // The row lock serialises receipts within a year; a rollback returns the number.
    let seq: i32 = sqlx::query_scalar(
        "INSERT INTO fees.receipt_seq (financial_year, last_no) VALUES ($1, 1)
         ON CONFLICT (financial_year) DO UPDATE SET last_no = fees.receipt_seq.last_no + 1
         RETURNING last_no",
    )
    .bind(financial_year)
    .fetch_one(&mut *conn)
    .await?;
    let receipt_no = format!(
        "RCPT/{financial_year}-{:02}/{seq:06}",
        (financial_year + 1) % 100
    );
    let receipt_id: Uuid = sqlx::query_scalar(
        "INSERT INTO fees.receipts (receipt_no, financial_year, seq, payment_id, student_id, amount_paise, remarks)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(&receipt_no)
    .bind(financial_year)
    .bind(seq)
    .bind(payment_id)
    .bind(attempt.student_id)
    .bind(attempt.amount_paise)
    .bind(particulars.join(", "))
    .fetch_one(&mut *conn)
    .await?;

    Ok(Captured {
        payment_id,
        receipt_id,
        receipt_no,
    })
}

/// What the gateway says it captured.
#[derive(Debug)]
pub struct GatewayCapture<'a> {
    pub order_id: &'a str,
    pub payment_id: &'a str,
    pub amount_paise: i64,
    pub currency: &'a str,
    pub method: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Settlement {
    /// No attempt of ours has that order.
    UnknownOrder,
    /// The same payment, announced again.
    Duplicate,
    /// A second payment for an order already paid: money taken twice. Not
    /// credited; an issue is opened for accounts to refund it.
    DuplicateCapture,
    /// The amount or currency doesn't match: held for review.
    Review,
    Processed {
        payment_id: Uuid,
        receipt_no: String,
    },
}

/// Settles a captured payment the gateway has vouched for, through its
/// webhook or through reconciliation (`via`): records the payment, credits
/// the charges, issues the receipt, completes exam forms and re-evaluation
/// requests waiting on it, and audits it all. Runs in the gateway context.
/// Also opens a reconciliation issue for anything paid twice.
pub async fn settle_captured(
    conn: &mut PgConnection,
    data: &GatewayCapture<'_>,
    origin: &crate::audit::Origin,
    via: &str,
) -> Result<Settlement, sqlx::Error> {
    use crate::audit::{self, Outcome};
    use serde_json::json;

    let Some(attempt) = lock_attempt_for_order(conn, data.order_id).await? else {
        let mut event = origin.event(
            "payment.unknown_order",
            "gateway_order",
            Some(data.order_id.to_owned()),
        );
        event.outcome = Outcome::Error;
        event.metadata = json!({ "gateway_payment_id": data.payment_id, "via": via });
        audit::record(conn, &event).await?;
        return Ok(Settlement::UnknownOrder);
    };
    if attempt.status == "captured" {
        let recorded: Option<String> = sqlx::query_scalar(
            "SELECT gateway_payment_id FROM fees.payments WHERE attempt_id = $1",
        )
        .bind(attempt.id)
        .fetch_optional(&mut *conn)
        .await?;
        if recorded.as_deref() == Some(data.payment_id) {
            return Ok(Settlement::Duplicate);
        }
        // The gateway took a second payment for this order.
        let opened = open_issue(
            conn,
            &NewIssue {
                kind: "duplicate_capture",
                reference: data.payment_id.to_owned(),
                student_id: Some(attempt.student_id),
                payment_id: None,
                gateway_payment_id: Some(data.payment_id),
                charge_id: None,
                amount_paise: Some(data.amount_paise),
                details: json!({ "order_id": data.order_id, "first_payment": recorded, "via": via }),
                run_id: None,
            },
        )
        .await?;
        if opened {
            let mut event = origin.event(
                "payment.duplicate_capture",
                "payment_attempt",
                Some(attempt.id.to_string()),
            );
            event.outcome = Outcome::Error;
            event.metadata = json!({
                "student_id": attempt.student_id,
                "gateway_payment_id": data.payment_id,
                "amount_paise": data.amount_paise,
                "via": via,
            });
            audit::record(conn, &event).await?;
        }
        return Ok(Settlement::DuplicateCapture);
    }
    if attempt.status == "review"
        || data.amount_paise != attempt.amount_paise
        || data.currency != "INR"
    {
        set_status(conn, attempt.id, "review").await?;
        let mut event = origin.event(
            "payment.mismatch",
            "payment_attempt",
            Some(attempt.id.to_string()),
        );
        event.outcome = Outcome::Error;
        event.metadata = json!({
            "student_id": attempt.student_id,
            "expected_paise": attempt.amount_paise,
            "reported_paise": data.amount_paise,
            "currency": data.currency,
            "via": via,
        });
        audit::record(conn, &event).await?;
        return Ok(Settlement::Review);
    }

    let captured = capture(
        conn,
        &Capture {
            attempt: &attempt,
            gateway_payment_id: data.payment_id,
            method: data.method,
            captured_at: Utc::now(),
        },
    )
    .await?;
    // The receipt PDF is made by the worker, after commit.
    crate::outbox::enqueue(
        conn,
        "receipt.requested",
        Some(&attempt.student_id.to_string()),
        &json!({ "receipt_id": captured.receipt_id }),
    )
    .await?;
    // Exam forms waiting on this payment are now submitted. (The full product
    // also has the worker make each form's acknowledgement PDF.)
    for form in crate::exams::complete_paid_forms(conn, attempt.student_id).await? {
        let mut event = origin.event("examform.submitted", "exam_form", Some(form.to_string()));
        event.metadata = json!({ "student_id": attempt.student_id });
        audit::record(conn, &event).await?;
    }
    // So are re-evaluation requests.
    for request in crate::revaluation::complete_paid(conn, attempt.student_id).await? {
        let mut event = origin.event(
            "revaluation.submitted",
            "revaluation_request",
            Some(request.to_string()),
        );
        event.metadata = json!({ "student_id": attempt.student_id });
        audit::record(conn, &event).await?;
    }
    let mut event = origin.event(
        "payment.captured",
        "payment",
        Some(captured.payment_id.to_string()),
    );
    event.metadata = json!({
        "student_id": attempt.student_id,
        "amount_paise": attempt.amount_paise,
        "receipt_no": captured.receipt_no,
        "gateway_payment_id": data.payment_id,
        "via": via,
    });
    audit::record(conn, &event).await?;

    // Two attempts for the same charge, both paid: more paid than owed.
    let overpaid: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT i.charge_id, -fees.pending_of(i.charge_id)
         FROM fees.payment_attempt_items i
         WHERE i.attempt_id = $1 AND fees.pending_of(i.charge_id) < 0",
    )
    .bind(attempt.id)
    .fetch_all(&mut *conn)
    .await?;
    for (charge_id, excess) in overpaid {
        open_issue(
            conn,
            &NewIssue {
                kind: "overpaid",
                reference: format!("charge:{charge_id}"),
                student_id: Some(attempt.student_id),
                payment_id: Some(captured.payment_id),
                gateway_payment_id: Some(data.payment_id),
                charge_id: Some(charge_id),
                amount_paise: Some(excess),
                details: json!({ "receipt_no": captured.receipt_no, "via": via }),
                run_id: None,
            },
        )
        .await?;
    }

    Ok(Settlement::Processed {
        payment_id: captured.payment_id,
        receipt_no: captured.receipt_no,
    })
}

/// A reconciliation issue to open.
#[derive(Debug)]
pub struct NewIssue<'a> {
    pub kind: &'a str,
    pub reference: String,
    pub student_id: Option<Uuid>,
    pub payment_id: Option<Uuid>,
    pub gateway_payment_id: Option<&'a str>,
    pub charge_id: Option<i64>,
    pub amount_paise: Option<i64>,
    pub details: Value,
    pub run_id: Option<i32>,
}

/// Opens an issue unless the same one (kind and reference) exists; `true`
/// if it is new.
pub async fn open_issue(
    conn: &mut PgConnection,
    issue: &NewIssue<'_>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO fees.reconciliation_issues
             (kind, reference, student_id, payment_id, gateway_payment_id, charge_id, amount_paise, details, run_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (kind, reference) DO NOTHING",
    )
    .bind(issue.kind)
    .bind(&issue.reference)
    .bind(issue.student_id)
    .bind(issue.payment_id)
    .bind(issue.gateway_payment_id)
    .bind(issue.charge_id)
    .bind(issue.amount_paise)
    .bind(&issue.details)
    .bind(issue.run_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

// ── For the receipt worker (system context `worker`) ─────────────────────────

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct ReceiptItem {
    pub particulars: String,
    pub amount_paise: i64,
}

/// Everything printed on a receipt.
#[derive(Debug, sqlx::FromRow)]
pub struct ReceiptDocument {
    pub receipt_no: String,
    pub financial_year: i16,
    pub issued_at: DateTime<Utc>,
    pub amount_paise: i64,
    pub pdf_object: Option<String>,
    pub institution: String,
    pub student_name: String,
    pub roll_no: String,
    pub program: String,
    pub method: Option<String>,
    pub gateway_payment_id: String,
    pub items: sqlx::types::Json<Vec<ReceiptItem>>,
}

pub async fn receipt_document(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<ReceiptDocument>, sqlx::Error> {
    sqlx::query_as(
        "SELECT r.receipt_no, r.financial_year, r.issued_at, r.amount_paise, r.pdf_object,
                i.name AS institution, u.display_name AS student_name, st.roll_no, p.name AS program,
                pay.method, pay.gateway_payment_id,
                coalesce((SELECT jsonb_agg(jsonb_build_object('particulars', c.particulars,
                                                              'amount_paise', e.amount_paise)
                                           ORDER BY c.id)
                          FROM fees.ledger e JOIN fees.ledger c ON c.id = e.charge_id
                          WHERE e.payment_id = r.payment_id), '[]'::jsonb) AS items
         FROM fees.receipts r
         JOIN fees.payments pay ON pay.id = r.payment_id
         JOIN people.students st ON st.user_id = r.student_id
         JOIN identity.users u ON u.id = r.student_id
         JOIN org.programs p ON p.id = st.program_id
         JOIN org.institutions i ON i.id = p.institution_id
         WHERE r.id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

/// Records where a receipt's PDF is stored. `false` if one was recorded
/// already, by an earlier delivery of the same event.
pub async fn attach_pdf(
    conn: &mut PgConnection,
    id: Uuid,
    object: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE fees.receipts SET pdf_object = $2 WHERE id = $1 AND pdf_object IS NULL",
    )
    .bind(id)
    .bind(object)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Indian financial years run April to March: 3 October 2026 is in FY
/// 2026-27, stored as 2026.
pub fn financial_year_of(date: NaiveDate) -> i16 {
    use chrono::Datelike;
    let year = date.year() as i16;
    if date.month() >= 4 { year } else { year - 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn financial_years_start_in_april() {
        let fy = |y, m, d| financial_year_of(NaiveDate::from_ymd_opt(y, m, d).unwrap());
        assert_eq!(fy(2026, 3, 31), 2025);
        assert_eq!(fy(2026, 4, 1), 2026);
        assert_eq!(fy(2026, 10, 3), 2026);
    }
}
