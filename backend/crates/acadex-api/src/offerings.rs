//! Faculty attendance marking: list my offerings, load a session's roster,
//! save the marks.

use acadex_auth::Permission;
use acadex_db::{
    academic, audit,
    offerings::{self, Mark, Offering, RosterEntry},
    rls,
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthUser, ClientInfo},
    error::ApiError,
    routes::attendance_scope,
};

/// Periods in a teaching day.
const SLOTS: std::ops::RangeInclusive<i16> = 1..=8;

#[derive(Deserialize)]
pub struct YearQuery {
    academic_year: Option<i16>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<YearQuery>,
) -> Result<Json<Vec<Offering>>, ApiError> {
    user.require(Permission::AttendanceMark)?;
    let settings = state.cache.settings(&state.pool).await?;
    let year = query
        .academic_year
        .unwrap_or_else(|| settings.academic_year_of(today()));
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    Ok(Json(offerings::taught_by_me(&mut tx, year).await?))
}

#[derive(Deserialize)]
pub struct SessionQuery {
    date: NaiveDate,
    slot: i16,
}

#[derive(Serialize)]
pub struct Roster {
    offering: Offering,
    session_date: NaiveDate,
    slot: i16,
    /// How the session was marked, if it has been: theory or practical.
    session_kind: Option<String>,
    students: Vec<RosterEntry>,
}

pub async fn roster(
    State(state): State<AppState>,
    user: AuthUser,
    Path(offering_id): Path<Uuid>,
    Query(session): Query<SessionQuery>,
) -> Result<Json<Roster>, ApiError> {
    user.require(Permission::AttendanceMark)?;
    check_slot(session.slot)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let offering = offerings::mine(&mut tx, offering_id)
        .await?
        .ok_or_else(|| ApiError::not_found("offering"))?;
    let students = offerings::roster(&mut tx, offering_id, session.date, session.slot).await?;
    let session_kind =
        offerings::session_kind(&mut tx, offering_id, session.date, session.slot).await?;
    Ok(Json(Roster {
        offering,
        session_date: session.date,
        slot: session.slot,
        session_kind,
        students,
    }))
}

#[derive(Deserialize)]
pub struct MarkRequest {
    session_date: NaiveDate,
    slot: i16,
    /// theory | practical. Default: practical for a lab, theory otherwise.
    session_kind: Option<String>,
    marks: Vec<Mark>,
}

#[derive(Serialize)]
pub struct MarkResponse {
    saved: u64,
}

/// Saves a session's attendance. A PUT: sending the same marks again changes
/// nothing, and corrections simply overwrite.
pub async fn mark(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(offering_id): Path<Uuid>,
    Json(request): Json<MarkRequest>,
) -> Result<Json<MarkResponse>, ApiError> {
    user.require(Permission::AttendanceMark)?;
    check_slot(request.slot)?;
    if request.session_date > today() {
        return Err(ApiError::invalid(
            "future-session",
            "Can't mark a future session",
            format!("{} hasn't happened yet", request.session_date),
        ));
    }
    if request.marks.is_empty() {
        return Err(ApiError::invalid(
            "no-marks",
            "Nothing to save",
            "marks is empty",
        ));
    }

    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let offering = offerings::mine(&mut tx, offering_id)
        .await?
        .ok_or_else(|| ApiError::not_found("offering"))?;
    let lab = offering.kind == "lab";
    let session_kind = request
        .session_kind
        .clone()
        .unwrap_or_else(|| if lab { "practical" } else { "theory" }.to_owned());
    let allowed = match session_kind.as_str() {
        "practical" => lab || offering.has_practical,
        "theory" => !lab,
        _ => false,
    };
    if !allowed {
        return Err(ApiError::invalid(
            "wrong-session-kind",
            "That kind of session isn't taught in this subject",
            format!(
                "{} has {}",
                offering.code,
                if lab {
                    "practicals only"
                } else if offering.has_practical {
                    "theory and practicals"
                } else {
                    "theory only"
                }
            ),
        ));
    }
    let settings = state.cache.settings(&state.pool).await?;
    if settings.academic_year_of(request.session_date) != offering.academic_year {
        return Err(ApiError::invalid(
            "outside-academic-year",
            "Date is outside this offering's academic year",
            format!("{} is taught in {}", offering.code, offering.academic_year),
        ));
    }
    if !settings.attendance_open(request.session_date, today()) {
        return Err(ApiError::invalid(
            "attendance-locked",
            "Attendance this old is locked",
            format!(
                "Sessions can be changed for {} days; students can request a correction, which the HOD decides",
                settings.attendance_lock_days
            ),
        ));
    }
    if let Some(reason) =
        academic::no_classes_on(&mut tx, offering.program_id, request.session_date).await?
    {
        return Err(ApiError::invalid(
            "no-classes",
            "There are no classes that day",
            format!("{}: {reason}", request.session_date),
        ));
    }

    let saved = offerings::mark(
        &mut tx,
        offering_id,
        request.session_date,
        request.slot,
        &session_kind,
        &request.marks,
    )
    .await?;
    // Every mark must land; a student off the roster means the request is wrong.
    if saved != request.marks.len() as u64 {
        return Err(ApiError::invalid(
            "not-on-roster",
            "Some students aren't on this offering's roster",
            format!(
                "{} of {} marks matched the roster",
                saved,
                request.marks.len()
            ),
        ));
    }

    let present = request
        .marks
        .iter()
        .filter(|m| {
            matches!(
                m.status,
                offerings::AttendanceStatus::Present | offerings::AttendanceStatus::Late
            )
        })
        .count();
    let mut event = client.event(
        "attendance.marked",
        "offering",
        Some(offering_id.to_string()),
    );
    event.metadata = json!({
        "subject": offering.code,
        "session_date": request.session_date,
        "slot": request.slot,
        "session_kind": session_kind,
        "students": saved,
        "present": present,
    });
    audit::record(&mut tx, &event).await?;
    // The worker refreshes this program's attendance rollup (shortage lists).
    let program_id: i16 = sqlx::query_scalar(
        "SELECT s.program_id FROM academic.offerings o JOIN academic.subjects s ON s.id = o.subject_id WHERE o.id = $1",
    )
    .bind(offering_id)
    .fetch_one(&mut *tx)
    .await?;
    acadex_db::outbox::enqueue(
        &mut tx,
        "attendance.marked",
        Some(&program_id.to_string()),
        &json!({ "program_id": program_id }),
    )
    .await?;
    tx.commit().await?;
    state.outbox_wakeup.notify_one();
    let scopes: Vec<String> = request
        .marks
        .iter()
        .map(|m| attendance_scope(m.student_id))
        .collect();
    state.cache.bump(&scopes).await;
    Ok(Json(MarkResponse { saved }))
}

fn check_slot(slot: i16) -> Result<(), ApiError> {
    if SLOTS.contains(&slot) {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "invalid-slot",
            "No such period",
            format!("slot must be between {} and {}", SLOTS.start(), SLOTS.end()),
        ))
    }
}

fn today() -> NaiveDate {
    Utc::now().date_naive()
}
