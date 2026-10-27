//! acadex-worker: Pub/Sub push subscriptions deliver events here
//! (architecture.md §11.2). Every handler is idempotent, because delivery is
//! at least once: the outbox may publish twice, and Pub/Sub may redeliver.

pub mod documents;
pub mod notify;
pub mod receipt;

use std::{collections::HashMap, sync::Arc};

use acadex_cloud::Storage;
use acadex_db::{
    fees,
    rls::{self, System},
};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

#[derive(Clone)]
pub struct WorkerState {
    pub pool: PgPool,
    pub storage: Storage,
    pub receipts_bucket: Arc<str>,
    /// Generated documents students download (acknowledgements, hall tickets, grade cards).
    pub uploads_bucket: Arc<str>,
    pub renderer: Arc<receipt::Renderer>,
    pub documents: Arc<documents::Documents>,
    /// The web app's public address, for links printed on documents (QR codes).
    pub public_url: Arc<str>,
    /// Email for notices; without it, notices go out in-app only.
    pub mailer: Option<notify::Mailer>,
    /// The payment gateway's API, for reconciliation.
    pub gateway: Option<acadex_cloud::gateway::Gateway>,
    /// Pub/Sub, for the outbox sweep.
    pub pubsub: Option<acadex_cloud::PubSub>,
}

pub fn router(state: WorkerState) -> Router {
    Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/pubsub/push", post(push))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Pub/Sub's push envelope.
#[derive(Deserialize)]
struct Push {
    message: PushMessage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushMessage {
    #[serde(default)]
    data: String,
    #[serde(default)]
    attributes: HashMap<String, String>,
    #[serde(default)]
    message_id: String,
}

/// A 2xx acknowledges the message; anything else makes Pub/Sub retry with
/// backoff. Events that can never succeed (malformed, or for a record that
/// doesn't exist) are acknowledged and logged rather than retried forever.
/// Event types this version doesn't know are retried instead: a newer worker
/// may be on its way, and in production the dead-letter topic catches the rest.
async fn push(State(state): State<WorkerState>, Json(push): Json<Push>) -> StatusCode {
    let message = push.message;
    // Our own events carry `event`; Cloud Storage notifications, `eventType`.
    let event = message
        .attributes
        .get("event")
        .or_else(|| message.attributes.get("eventType"))
        .map(String::as_str)
        .unwrap_or("");
    let payload: Value = match BASE64
        .decode(&message.data)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(payload) => payload,
        None => {
            tracing::error!(message_id = %message.message_id, event, "undecodable message; dropping");
            return StatusCode::NO_CONTENT;
        }
    };
    let span = tracing::info_span!("worker.event", event, message_id = %message.message_id);
    let _enter = span.enter();

    let result = match event {
        "fees.reconcile_requested" => reconcile_payments(&state).await,
        // Marking changed a program's numbers: refresh its rollup now.
        "attendance.marked" => {
            rollup_attendance(&state, payload["program_id"].as_i64().map(|p| p as i16)).await
        }
        "receipt.requested" => match payload["receipt_id"]
            .as_str()
            .and_then(|id| id.parse().ok())
        {
            Some(id) => generate_receipt(&state, id).await,
            None => Ok(Done::Skipped("no receipt_id")),
        },
        // The API already bumped the results cache; the worker makes grade cards.
        "results.published" => match payload["publication_id"]
            .as_str()
            .and_then(|id| id.parse().ok())
        {
            Some(id) => generate_grade_cards(&state, id).await,
            None => Ok(Done::Skipped("no publication_id")),
        },
        "notice.published" => match payload["notice_id"].as_str().and_then(|id| id.parse().ok()) {
            Some(id) => fanout_notice(&state, id).await,
            None => Ok(Done::Skipped("no notice_id")),
        },
        // A new object in the uploads bucket: profile documents get scanned.
        // An event this version doesn't know may be for a newer worker that's
        // being deployed: refuse it, so Pub/Sub redelivers it (and, in
        // production, dead-letters it if no version ever takes it).
        _ => Err(anyhow::anyhow!("unknown event type")),
    };
    match result {
        Ok(done) => {
            tracing::info!(event, outcome = ?done, "event handled");
            if let Err(err) = forget_failures(&state, &message.message_id).await {
                tracing::warn!(error = %err, "clearing earlier failures failed");
            }
            StatusCode::NO_CONTENT
        }
        Err(err) => match count_failure(&state, &message, event, &payload, &err).await {
            Ok(true) => {
                tracing::error!(event, message_id = %message.message_id, error = %err, "event dead-lettered after repeated failures");
                StatusCode::NO_CONTENT
            }
            Ok(false) => {
                tracing::warn!(event, error = %err, "event failed; Pub/Sub will retry");
                StatusCode::SERVICE_UNAVAILABLE
            }
            Err(db) => {
                tracing::warn!(event, error = %err, counting_error = %db, "event failed; Pub/Sub will retry");
                StatusCode::SERVICE_UNAVAILABLE
            }
        },
    }
}

/// Counts a failed delivery and, at the limit, parks the message in the
/// dead-letter store (`true`: acknowledge it, so Pub/Sub stops retrying).
async fn count_failure(
    state: &WorkerState,
    message: &PushMessage,
    event: &str,
    payload: &Value,
    err: &anyhow::Error,
) -> Result<bool, sqlx::Error> {
    use acadex_db::ops;
    if message.message_id.is_empty() {
        return Ok(false);
    }
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let attempts = ops::record_failure(&mut tx, &message.message_id, &format!("{err:#}")).await?;
    let parked = attempts >= ops::MAX_ATTEMPTS && {
        let attributes = json!(message.attributes);
        ops::park(&mut tx, &message.message_id, event, payload, &attributes).await?;
        true
    };
    tx.commit().await?;
    Ok(parked)
}

async fn forget_failures(state: &WorkerState, message_id: &str) -> Result<(), sqlx::Error> {
    if message_id.is_empty() {
        return Ok(());
    }
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    acadex_db::ops::clear_failures(&mut tx, message_id).await?;
    tx.commit().await
}

#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    Generated(String),
    AlreadyDone,
    Skipped(&'static str),
}

/// Renders a receipt's PDF, stores it, and records where. Safe to repeat:
/// a receipt that already has its PDF is left alone, and the object name is
/// fixed per receipt, so a concurrent duplicate writes the same file.
pub async fn generate_receipt(state: &WorkerState, receipt_id: Uuid) -> anyhow::Result<Done> {
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let document = fees::receipt_document(&mut tx, receipt_id).await?;
    tx.commit().await?;
    let Some(document) = document else {
        return Ok(Done::Skipped("no such receipt"));
    };
    if document.pdf_object.is_some() {
        return Ok(Done::AlreadyDone);
    }

    let fy = document.financial_year;
    let object = format!(
        "receipts/{fy}-{:02}/{}.pdf",
        (fy + 1) % 100,
        document.receipt_no.replace('/', "-")
    );
    let renderer = state.renderer.clone();
    let pdf = tokio::task::spawn_blocking(move || renderer.render(&document)).await??;
    state
        .storage
        .put(&state.receipts_bucket, &object, "application/pdf", pdf)
        .await?;

    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let attached = fees::attach_pdf(&mut tx, receipt_id, &object).await?;
    tx.commit().await?;
    Ok(if attached {
        Done::Generated(object)
    } else {
        Done::AlreadyDone
    })
}

/// Makes the grade cards a published result set changes: for each of its
/// students, the semester at every subject's latest published result. Safe
/// to repeat: students whose card already reflects this set (or a later
/// one) are skipped, and the object name is fixed per set.
pub async fn generate_grade_cards(
    state: &WorkerState,
    publication_id: Uuid,
) -> anyhow::Result<Done> {
    use acadex_db::{grading, results};
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let Some(publication) = results::publication(&mut tx, publication_id).await? else {
        return Ok(Done::Skipped("no such result set"));
    };
    if publication.published_at.is_none() {
        return Ok(Done::Skipped("not published"));
    }
    let institution: String = sqlx::query_scalar("SELECT name FROM org.institutions LIMIT 1")
        .fetch_one(&mut *tx)
        .await?;
    let rules = results::institution_grading(&mut tx).await?;
    let students = results::card_students(&mut tx, publication_id).await?;
    tx.commit().await?;

    let issued_on = chrono::Utc::now()
        .with_timezone(&chrono_tz::Asia::Kolkata)
        .format("%-d %b %Y")
        .to_string();
    let mut made = 0;
    for student in students {
        let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
        let rows = results::published_marks(&mut tx, student.student_id).await?;
        tx.commit().await?;
        let Some(semester) = grading::by_semester(rows, &rules)
            .into_iter()
            .find(|s| s.summary.semester == publication.semester)
        else {
            continue;
        };
        let summary = &semester.summary;
        let mut notes = Vec::new();
        let subjects: Vec<Value> = semester
            .subjects
            .iter()
            .map(|s| {
                let note = match s.kind.as_str() {
                    "backlog" => "B",
                    "revaluation" => "R",
                    _ => "",
                };
                if !note.is_empty() {
                    let what = if note == "B" {
                        "cleared in a backlog exam"
                    } else {
                        "after re-evaluation"
                    };
                    let line = format!("{note}: {what}");
                    if !notes.contains(&line) {
                        notes.push(line);
                    }
                }
                json!({
                    "code": s.code,
                    "name": s.name,
                    "credits": s.credits.to_string(),
                    "internal": format!("{}/{}", s.internal, s.internal_max),
                    "external": format!("{}/{}", s.external, s.external_max),
                    "grade": s.grade,
                    "points": s.points.to_string(),
                    "result": if s.passed { "Pass" } else { "Fail" },
                    "note": note,
                })
            })
            .collect();
        let year = summary.academic_year;
        let data = json!({
            "institution": institution,
            "student_name": student.student_name,
            "roll_no": student.roll_no,
            "program": student.program,
            "semester": summary.semester.to_string(),
            "academic_year": format!("{year}–{:02}", (year + 1) % 100),
            "issued_on": issued_on,
            "subjects": subjects,
            "sgpa": format!("{:.2}", summary.sgpa),
            "cgpa": format!("{:.2}", summary.cgpa),
            "credits": format!("{} / {}", summary.credits_earned, summary.credits_attempted),
            "result": if summary.passed { "Pass" } else { "Backlog" },
            "notes": notes,
            "reference": format!("{}-S{}-{}", student.roll_no, summary.semester, &publication_id.simple().to_string()[..8]),
        });
        let documents = state.documents.clone();
        let pdf =
            tokio::task::spawn_blocking(move || documents.grade_card.render(&data, Vec::new()))
                .await??;
        let object = format!(
            "grade-cards/{}/semester-{}-{publication_id}.pdf",
            student.roll_no, publication.semester
        );
        state
            .storage
            .put(&state.uploads_bucket, &object, "application/pdf", pdf)
            .await?;
        let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
        if results::record_card(
            &mut tx,
            student.student_id,
            publication.semester,
            publication_id,
            &object,
        )
        .await?
        {
            made += 1;
        }
        tx.commit().await?;
    }
    tracing::info!(%publication_id, made, "grade cards");
    Ok(if made == 0 {
        Done::AlreadyDone
    } else {
        Done::Generated(format!("{made} grade cards"))
    })
}

/// Emails a live notice to its audience by their preference, once each, then
/// marks it fanned out (which also tells listening browsers).
pub async fn fanout_notice(state: &WorkerState, notice_id: Uuid) -> anyhow::Result<Done> {
    use acadex_db::notices;
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let notice = notices::outgoing(&mut tx, notice_id).await?;
    tx.commit().await?;
    // Not live yet (scheduled: the release job picks it up), or already done.
    let Some(notice) = notice else {
        return Ok(Done::AlreadyDone);
    };
    let mut sent = 0;
    if let Some(mailer) = &state.mailer {
        let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
        let recipients = notices::email_recipients(&mut tx, notice_id, &notice.severity).await?;
        tx.commit().await?;
        let (subject, body) = notify::compose(
            &notice.severity,
            &notice.title,
            &notice.body,
            notice.author.as_deref(),
            &state.public_url,
        );
        for r in recipients {
            mailer
                .send(&r.display_name, &r.email, &subject, body.clone())
                .await?;
            let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
            notices::record_delivery(&mut tx, notice_id, r.user_id).await?;
            tx.commit().await?;
            sent += 1;
        }
    }
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    notices::mark_fanned_out(&mut tx, notice_id).await?;
    tx.commit().await?;
    tracing::info!(%notice_id, sent, "notice fanned out");
    Ok(Done::Generated(format!("{sent} emails")))
}

/// The scheduled side of fan-out: notices whose publish time has come
/// (and any whose event was lost). Runs every minute.
pub async fn release_notices(state: &WorkerState) -> anyhow::Result<Done> {
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let due = acadex_db::notices::due_for_fanout(&mut tx, 20).await?;
    tx.commit().await?;
    let count = due.len();
    for notice in due {
        fanout_notice(state, notice.id).await?;
    }
    Ok(if count == 0 {
        Done::AlreadyDone
    } else {
        Done::Generated(format!("{count} notices released"))
    })
}

/// The `fees.reconcile` job (architecture.md §11.2): compares the gateway's
/// payments since the last run with what Acadex recorded.
///
/// - A payment the gateway took that we never heard of (a lost webhook) is
///   settled now, exactly as the webhook would have, and noted as recovered.
/// - A second payment on an order already paid, an amount that disagrees, a
///   payment for no order of ours, or one of ours the gateway doesn't list
///   opens an issue for the accounts office.
/// - Any charge with more paid than owed (paid twice) opens an issue to refund.
///
/// Runs every 5 minutes, and on request from the reconciliation page. Each
/// issue is opened once, so overlapping windows and repeated runs are harmless.
pub async fn reconcile_payments(state: &WorkerState) -> anyhow::Result<Done> {
    use acadex_db::{audit, fees_office};
    let Some(gateway) = &state.gateway else {
        return Ok(Done::Skipped("no payment gateway configured"));
    };
    // One run at a time (the 5-minute tick and a Run now can coincide): the
    // lock lives as long as this transaction, which the run holds open.
    let mut guard = state.pool.begin().await?;
    let ours: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtext('fees.reconcile'))")
            .fetch_one(&mut *guard)
            .await?;
    if !ours {
        return Ok(Done::Skipped("another reconciliation is running"));
    }
    let now = chrono::Utc::now();
    // Webhooks normally land within seconds; leave the last two minutes to them.
    let to = now - chrono::Duration::minutes(2);
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let from = match fees_office::last_window_end(&mut tx).await? {
        // A little overlap: a payment created at the edge is checked twice, harmlessly.
        Some(end) => end - chrono::Duration::minutes(10),
        None => now - chrono::Duration::hours(48),
    };
    let run = fees_office::start_run(&mut tx, from, to).await?;
    tx.commit().await?;

    let mut totals = fees_office::RunTotals::default();
    let result = reconcile_window(state, gateway, run, from, to, &mut totals).await;
    let error = result.as_ref().err().map(|err| format!("{err:#}"));

    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    totals.issues_opened = sqlx::query_scalar(
        "SELECT count(*)::int FROM fees.reconciliation_issues
         WHERE found_at >= (SELECT started_at FROM fees.reconciliation_runs WHERE id = $1)
           AND kind <> 'missed_webhook'",
    )
    .bind(run)
    .fetch_one(&mut *tx)
    .await?;
    fees_office::finish_run(&mut tx, run, &totals, error.as_deref()).await?;
    audit::record(
        &mut tx,
        &audit::Event {
            action: "reconcile.completed",
            resource_type: "reconciliation_run",
            resource_id: Some(run.to_string()),
            outcome: if error.is_some() {
                audit::Outcome::Error
            } else {
                audit::Outcome::Success
            },
            ip: None,
            user_agent: None,
            trace_id: None,
            metadata: json!({
                "gateway_payments": totals.gateway_payments,
                "matched": totals.matched,
                "recovered": totals.recovered,
                "issues_opened": totals.issues_opened,
                "error": error,
            }),
        },
    )
    .await?;
    tx.commit().await?;
    guard.rollback().await?;
    result?;
    if totals.recovered > 0 || totals.issues_opened > 0 {
        tracing::warn!(
            run,
            recovered = totals.recovered,
            issues = totals.issues_opened,
            "reconciliation found differences"
        );
    }
    Ok(Done::Generated(format!(
        "{} gateway payments, {} matched, {} recovered, {} issues",
        totals.gateway_payments, totals.matched, totals.recovered, totals.issues_opened
    )))
}

async fn reconcile_window(
    state: &WorkerState,
    gateway: &acadex_cloud::gateway::Gateway,
    run: i32,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    totals: &mut acadex_db::fees_office::RunTotals,
) -> anyhow::Result<()> {
    use acadex_db::{
        audit::Origin,
        fees::{self, GatewayCapture, NewIssue, Settlement},
        fees_office,
    };
    use std::collections::HashSet;

    let listed = gateway.payments(from.timestamp(), to.timestamp()).await?;
    let mut seen = HashSet::new();
    let origin = Origin::default();
    for p in listed
        .iter()
        .filter(|p| matches!(p.status.as_str(), "captured" | "refunded"))
    {
        totals.gateway_payments += 1;
        seen.insert(p.id.clone());
        let issue = |kind: &'static str, details: Value| NewIssue {
            kind,
            reference: p.id.clone(),
            student_id: None,
            payment_id: None,
            gateway_payment_id: Some(&p.id),
            charge_id: None,
            amount_paise: Some(p.amount),
            details,
            run_id: Some(run),
        };
        // The gateway's own context: we act on its word, read from its API.
        let mut tx = rls::begin_system(&state.pool, System::Gateway).await?;
        match fees_office::payment_by_gateway_id(&mut tx, &p.id).await? {
            Some(ours) if ours.amount_paise == p.amount => totals.matched += 1,
            Some(ours) => {
                let mut issue = issue(
                    "amount_mismatch",
                    json!({ "recorded_paise": ours.amount_paise, "gateway_paise": p.amount }),
                );
                issue.student_id = Some(ours.student_id);
                issue.payment_id = Some(ours.id);
                fees::open_issue(&mut tx, &issue).await?;
            }
            None => match &p.order_id {
                None => {
                    fees::open_issue(&mut tx, &issue("unknown_payment", json!({}))).await?;
                }
                Some(order_id) => {
                    let settled = fees::settle_captured(
                        &mut tx,
                        &GatewayCapture {
                            order_id,
                            payment_id: &p.id,
                            amount_paise: p.amount,
                            currency: &p.currency,
                            method: p.method.as_deref(),
                        },
                        &origin,
                        "reconciliation",
                    )
                    .await?;
                    match settled {
                        Settlement::Processed {
                            payment_id,
                            receipt_no,
                        } => {
                            totals.recovered += 1;
                            let mut found = issue(
                                "missed_webhook",
                                json!({ "order_id": order_id, "receipt_no": receipt_no }),
                            );
                            found.payment_id = Some(payment_id);
                            fees::open_issue(&mut tx, &found).await?;
                            sqlx::query(
                                "UPDATE fees.reconciliation_issues
                                 SET status = 'resolved', resolved_at = now(),
                                     resolution = 'Recovered automatically: payment recorded, receipt ' || $2,
                                     student_id = (SELECT student_id FROM fees.payments WHERE id = $3)
                                 WHERE kind = 'missed_webhook' AND reference = $1",
                            )
                            .bind(&p.id)
                            .bind(&receipt_no)
                            .bind(payment_id)
                            .execute(&mut *tx)
                            .await?;
                        }
                        Settlement::UnknownOrder => {
                            fees::open_issue(
                                &mut tx,
                                &issue("unknown_payment", json!({ "order_id": order_id })),
                            )
                            .await?;
                        }
                        Settlement::Review => {
                            fees::open_issue(
                                &mut tx,
                                &issue(
                                    "amount_mismatch",
                                    json!({ "order_id": order_id, "gateway_paise": p.amount }),
                                ),
                            )
                            .await?;
                        }
                        // Settled a moment ago (by a late webhook).
                        Settlement::Duplicate => totals.matched += 1,
                        // Opened by settle_captured itself.
                        Settlement::DuplicateCapture => {}
                    }
                }
            },
        }
        tx.commit().await?;
    }

    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    for ours in fees_office::payments_between(&mut tx, from, to).await? {
        if !seen.contains(&ours.gateway_payment_id) {
            fees::open_issue(
                &mut tx,
                &NewIssue {
                    kind: "missing_at_gateway",
                    reference: ours.gateway_payment_id.clone(),
                    student_id: Some(ours.student_id),
                    payment_id: Some(ours.id),
                    gateway_payment_id: Some(&ours.gateway_payment_id),
                    charge_id: None,
                    amount_paise: Some(ours.amount_paise),
                    details: json!({}),
                    run_id: Some(run),
                },
            )
            .await?;
        }
    }
    for o in fees_office::overpaid_charges(&mut tx).await? {
        fees::open_issue(
            &mut tx,
            &NewIssue {
                kind: "overpaid",
                reference: format!("charge:{}", o.charge_id),
                student_id: Some(o.student_id),
                payment_id: o.payment_id,
                gateway_payment_id: o.gateway_payment_id.as_deref(),
                charge_id: Some(o.charge_id),
                amount_paise: Some(o.excess),
                details: json!({}),
                run_id: Some(run),
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ── Scheduled jobs (architecture.md §11.2) ──────────────────────────────────

/// `outbox.sweep`, every minute: publishes events the API's in-process
/// dispatcher left behind (an instance scaled to zero, a Pub/Sub outage).
pub async fn sweep_outbox(state: &WorkerState) -> anyhow::Result<Done> {
    use acadex_db::outbox;
    let Some(pubsub) = &state.pubsub else {
        return Ok(Done::Skipped("no Pub/Sub configured"));
    };
    let mut total = 0;
    loop {
        let batch = outbox::begin_batch(&state.pool, 100).await?;
        let mut sent = Vec::with_capacity(batch.events.len());
        for event in &batch.events {
            match pubsub
                .publish_event(&event.topic, &event.payload, event.id)
                .await
            {
                Ok(_) => sent.push(event.id),
                Err(err) => {
                    tracing::warn!(error = %err, outbox_id = event.id, topic = %event.topic, "publish failed");
                    break;
                }
            }
        }
        let (count, full) = (
            sent.len(),
            sent.len() == batch.events.len() && batch.events.len() == 100,
        );
        outbox::finish(batch, &sent).await?;
        total += count;
        if !full {
            break;
        }
    }
    Ok(if total == 0 {
        Done::AlreadyDone
    } else {
        Done::Generated(format!("{total} events published"))
    })
}

/// `audit.verify_chain`, nightly: walks the hash chain. A break is logged
/// as an error (the alert) and as an audit event of its own.
pub async fn verify_audit_chain(state: &WorkerState) -> anyhow::Result<Done> {
    use acadex_db::{audit, ops};
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let result = ops::verify_audit_chain(&mut tx).await?;
    if let Some(broken) = result.first_broken_id {
        tracing::error!(broken, detail = ?result.detail, "audit hash chain is broken");
        audit::record(
            &mut tx,
            &audit::Event {
                action: "audit.chain_broken",
                resource_type: "action_log",
                resource_id: Some(broken.to_string()),
                outcome: audit::Outcome::Error,
                ip: None,
                user_agent: None,
                trace_id: None,
                metadata: json!({ "detail": result.detail, "rows_checked": result.rows_checked }),
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Done::Generated(match result.first_broken_id {
        None => format!("{} rows verified", result.rows_checked),
        Some(id) => format!("chain broken at row {id}"),
    }))
}

/// The academic year a date falls in, from the institution's settings.
async fn current_academic_year(state: &WorkerState) -> anyhow::Result<i16> {
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let settings = acadex_db::settings::load(&mut tx).await?;
    tx.commit().await?;
    Ok(settings.academic_year_of(chrono::Utc::now().date_naive()))
}

/// `partition.maintain`, weekly: attendance partitions for this academic
/// year and the next, created ahead of need.
pub async fn maintain_partitions(state: &WorkerState) -> anyhow::Result<Done> {
    let year = i32::from(current_academic_year(state).await?);
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let created = acadex_db::ops::maintain_partitions(&mut tx, &[year, year + 1]).await?;
    tx.commit().await?;
    Ok(if created.is_empty() {
        Done::AlreadyDone
    } else {
        tracing::info!(?created, "attendance partitions created");
        Done::Generated(format!("created {}", created.join(", ")))
    })
}

/// `attendance.rollup`, hourly and after marking: recomputes held and
/// attended sessions per student and subject for this academic year (or one
/// program's, when marking announced which).
pub async fn rollup_attendance(
    state: &WorkerState,
    program_id: Option<i16>,
) -> anyhow::Result<Done> {
    let year = current_academic_year(state).await?;
    let mut tx = rls::begin_system(&state.pool, System::Worker).await?;
    let mut rows = 0;
    for (program, academic_year) in acadex_db::ops::attendance_partitions(&mut tx, &[year]).await? {
        if program_id.is_some_and(|only| only != program) {
            continue;
        }
        rows += acadex_db::ops::refresh_rollup(&mut tx, program, academic_year).await?;
    }
    tx.commit().await?;
    Ok(Done::Generated(format!("{rows} rollup rows refreshed")))
}
