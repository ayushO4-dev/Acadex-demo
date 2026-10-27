//! Fees for the signed-in student (Payable, Pay, Receipts) and the payment
//! gateway's webhook.

use acadex_auth::Permission;
use acadex_db::{
    audit::{self, Outcome},
    fees::{self, Attempt, ChargeLine, GatewayCapture, Receipt, Settlement},
    rls::{self, System},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthUser, ClientInfo},
    error::ApiError,
    payments::{self, SIGNATURE_HEADER},
};

const MAX_CHARGES_PER_PAYMENT: usize = 20;

#[derive(Serialize)]
pub struct Payable {
    academic_year: i16,
    lines: Vec<ChargeLine>,
    totals: Totals,
}

#[derive(Default, Serialize)]
struct Totals {
    receivable: i64,
    concession: i64,
    paid: i64,
    pending: i64,
}

pub async fn payable(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Payable>, ApiError> {
    user.require(Permission::FeesRead)?;
    let academic_year = state
        .cache
        .settings(&state.pool)
        .await?
        .academic_year_of(Utc::now().date_naive());
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let lines = fees::payable(&mut tx, user.id, academic_year).await?;
    let totals = lines.iter().fold(Totals::default(), |mut t, line| {
        t.receivable += line.receivable;
        t.concession += line.concession;
        t.paid += line.paid;
        t.pending += line.pending;
        t
    });
    Ok(Json(Payable {
        academic_year,
        lines,
        totals,
    }))
}

#[derive(Deserialize)]
pub struct PayRequest {
    charge_ids: Vec<i64>,
}

#[derive(Serialize)]
pub struct Checkout {
    attempt_id: Uuid,
    amount_paise: i64,
    pub(crate) checkout_url: String,
}

/// Starts paying for some charges: records an attempt for what they owe now,
/// opens a gateway order, and returns where to send the browser.
///
/// Requires an `Idempotency-Key` header. Repeating a request with the same key
/// returns the same attempt — a double click or a retry never opens a second
/// order. The amount is always worked out here, never taken from the client.
pub async fn start_payment(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    headers: HeaderMap,
    Json(request): Json<PayRequest>,
) -> Result<(StatusCode, Json<Checkout>), ApiError> {
    user.require(Permission::FeesPay)?;
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .filter(|key| (8..=100).contains(&key.len()))
        .ok_or_else(|| {
            ApiError::invalid(
                "idempotency-key-required",
                "An Idempotency-Key header is required",
                "8 to 100 characters, unique per payment",
            )
        })?
        .to_owned();

    checkout(&state, &user, &client, &key, &request.charge_ids).await
}

/// Opens (or, for the same idempotency key, returns) a gateway checkout for
/// some of the acting student's charges. Shared by Pay and by exam forms.
pub(crate) async fn checkout(
    state: &AppState,
    user: &AuthUser,
    client: &ClientInfo,
    key: &str,
    charge_ids: &[i64],
) -> Result<(StatusCode, Json<Checkout>), ApiError> {
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let (attempt_id, amount) = match fees::attempt_by_key(&mut tx, user.id, key).await? {
        Some(Attempt {
            id,
            amount_paise,
            gateway_order_id: Some(order_id),
            ..
        }) => {
            let checkout_url = state.payments.checkout_link(&order_id, amount_paise, id);
            return Ok((
                StatusCode::OK,
                Json(Checkout {
                    attempt_id: id,
                    amount_paise,
                    checkout_url,
                }),
            ));
        }
        // Recorded, but the gateway call failed last time: try it again.
        Some(attempt) => (attempt.id, attempt.amount_paise),
        None => {
            let items = payable_items(&mut tx, charge_ids).await?;
            let id = fees::create_attempt(&mut tx, key, &items).await?;
            let amount = items.iter().map(|(_, amount)| amount).sum();
            let mut event =
                client.event("payment.initiated", "payment_attempt", Some(id.to_string()));
            event.metadata = json!({ "amount_paise": amount, "charges": charge_ids });
            audit::record(&mut tx, &event).await?;
            (id, amount)
        }
    };
    tx.commit().await?;

    // No transaction is held open across the network call.
    let order_id = state
        .payments
        .gateway
        .create_order(amount, &attempt_id.to_string())
        .await
        .map_err(|err| {
            tracing::warn!(error = %err, "creating the gateway order failed");
            ApiError::upstream(
                "gateway-unavailable",
                "The payment gateway is unavailable. Please try again.",
            )
        })?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    fees::set_order(&mut tx, attempt_id, &order_id).await?;
    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(Checkout {
            attempt_id,
            amount_paise: amount,
            checkout_url: state.payments.checkout_link(&order_id, amount, attempt_id),
        }),
    ))
}

/// The `(charge, pending amount)` pairs to pay, or why they can't be.
async fn payable_items(
    conn: &mut PgConnection,
    charge_ids: &[i64],
) -> Result<Vec<(i64, i64)>, ApiError> {
    let mut unique = charge_ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if unique.is_empty() || unique.len() > MAX_CHARGES_PER_PAYMENT {
        return Err(ApiError::invalid(
            "invalid-charges",
            "Choose what to pay for",
            format!("1 to {MAX_CHARGES_PER_PAYMENT} distinct charges"),
        ));
    }
    // RLS: someone else's charge is simply not found.
    let items = fees::pending_of(conn, &unique).await?;
    if items.len() != unique.len() {
        return Err(ApiError::invalid(
            "unknown-charge",
            "No such charge",
            "a charge isn't yours or doesn't exist",
        ));
    }
    if items.iter().any(|(_, pending)| *pending <= 0) {
        return Err(ApiError::invalid(
            "nothing-due",
            "Nothing is due on one of these",
            "already paid",
        ));
    }
    Ok(items)
}

/// The browser comes back here from the gateway. It only reports what the
/// webhook has recorded: returning from checkout never confirms a payment.
pub async fn payment_status(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Attempt>, ApiError> {
    user.require(Permission::FeesRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let attempt = fees::attempt(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::not_found("payment"))?;
    Ok(Json(attempt))
}

pub async fn receipts(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<Receipt>>, ApiError> {
    user.require(Permission::FeesRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    Ok(Json(fees::receipts(&mut tx, user.id).await?))
}

/// Streams the student's own receipt PDF. RLS means another student's
/// receipt is simply not found.
pub async fn receipt_pdf(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    user.require(Permission::FeesRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let (receipt_no, object) = fees::receipt_object(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::not_found("receipt"))?;
    tx.commit().await?;
    let object = object.ok_or_else(|| {
        ApiError::conflict("receipt-not-ready", "The receipt PDF is still being made")
    })?;

    let pdf = state
        .files
        .storage
        .get(&state.files.receipts_bucket, &object)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, %object, "reading the receipt PDF failed");
            ApiError::upstream(
                "storage-unavailable",
                "The receipt couldn't be fetched. Please try again.",
            )
        })?;
    let filename = format!("{}.pdf", receipt_no.replace('/', "-"));
    let mut response = pdf.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

// ── Webhook ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GatewayEvent {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    data: PaymentData,
}

#[derive(Deserialize)]
struct PaymentData {
    payment_id: String,
    order_id: String,
    amount: i64,
    currency: String,
    method: Option<String>,
}

/// `POST /webhooks/payments`: the gateway's word on a payment, and the only
/// thing that confirms one. Signed, replay-protected, and idempotent: every
/// event is processed at most once, however often it is delivered.
pub async fn webhook(
    State(state): State<AppState>,
    client: ClientInfo,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let signature = headers
        .get(SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("missing webhook signature"))?;
    payments::verify(
        &state.payments.webhook_secret,
        signature,
        &body,
        Utc::now().timestamp(),
    )
    .map_err(ApiError::unauthorized)?;
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::invalid("malformed-event", "Malformed event", "not JSON"))?;
    let event: GatewayEvent = serde_json::from_value(payload.clone())
        .map_err(|err| ApiError::invalid("malformed-event", "Malformed event", err.to_string()))?;

    let mut tx = rls::begin_system(&state.pool, System::Gateway).await?;
    let status = if !fees::record_event(&mut tx, &event.id, &event.kind, &payload).await? {
        "duplicate"
    } else {
        match event.kind.as_str() {
            "payment.captured" => captured(&mut tx, &client, &event.data).await?,
            "payment.failed" => failed(&mut tx, &client, &event.data).await?,
            _ => "ignored",
        }
    };
    tx.commit().await?;
    if status == "processed" {
        state.outbox_wakeup.notify_one();
    }
    tracing::info!(event = %event.id, kind = %event.kind, status, "payment webhook");
    Ok(Json(json!({ "status": status })))
}

async fn captured(
    tx: &mut PgConnection,
    client: &ClientInfo,
    data: &PaymentData,
) -> Result<&'static str, ApiError> {
    let settlement = fees::settle_captured(
        tx,
        &GatewayCapture {
            order_id: &data.order_id,
            payment_id: &data.payment_id,
            amount_paise: data.amount,
            currency: &data.currency,
            method: data.method.as_deref(),
        },
        &client.origin(),
        "webhook",
    )
    .await?;
    Ok(match settlement {
        Settlement::UnknownOrder => "ignored",
        Settlement::Duplicate => "duplicate",
        Settlement::DuplicateCapture => "duplicate_capture",
        Settlement::Review => "review",
        Settlement::Processed { .. } => "processed",
    })
}

async fn failed(
    tx: &mut PgConnection,
    client: &ClientInfo,
    data: &PaymentData,
) -> Result<&'static str, ApiError> {
    let Some(attempt) = fees::lock_attempt_for_order(tx, &data.order_id).await? else {
        return Ok("ignored");
    };
    if attempt.status != "created" {
        return Ok("ignored");
    }
    fees::set_status(tx, attempt.id, "failed").await?;
    let mut event = client.event(
        "payment.failed",
        "payment_attempt",
        Some(attempt.id.to_string()),
    );
    event.outcome = Outcome::Error;
    event.metadata =
        json!({ "student_id": attempt.student_id, "amount_paise": attempt.amount_paise });
    audit::record(tx, &event).await?;
    Ok("processed")
}
