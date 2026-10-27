//! Academic pages (architecture.md §9.1): the calendar and its iCal export,
//! attendance detail and corrections.

use acadex_auth::Permission;
use acadex_db::{
    academic::{self, CalendarEvent, Correction, CorrectionRequest, Decision, NewEvent, Session},
    audit, people, rls,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthUser, ClientInfo},
    error::ApiError,
    routes::attendance_scope,
};

// ── Subjects ────────────────────────────────────────────────────────────────

// ── Calendar ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct Range {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

pub async fn calendar(
    State(state): State<AppState>,
    user: AuthUser,
    Query(range): Query<Range>,
) -> Result<Json<Vec<CalendarEvent>>, ApiError> {
    let today = Utc::now().date_naive();
    let from = range.from.unwrap_or(today - Duration::days(31));
    let to = range.to.unwrap_or(today + Duration::days(92));
    if to < from || (to - from).num_days() > 400 {
        return Err(ApiError::invalid(
            "invalid-range",
            "Choose a range of up to a year",
            format!("{from}..{to}"),
        ));
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    Ok(Json(academic::calendar(&mut tx, from, to).await?))
}

/// The current academic year's calendar as iCalendar (RFC 5545), for
/// importing into Google Calendar, Outlook or a phone.
pub async fn calendar_ics(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Response, ApiError> {
    let settings = state.cache.settings(&state.pool).await?;
    let year = settings.academic_year_of(Utc::now().date_naive());
    let start_month = settings.academic_year_start_month as u32;
    let from =
        NaiveDate::from_ymd_opt(i32::from(year), start_month, 1).ok_or_else(ApiError::internal)?;
    let to = from + Duration::days(366);
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let events = academic::calendar(&mut tx, from, to).await?;

    let mut response = ical(&events, year).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/calendar; charset=utf-8"),
    );
    if let Ok(value) = HeaderValue::from_str(&format!(
        "attachment; filename=\"acadex-calendar-{year}-{:02}.ics\"",
        (year + 1) % 100
    )) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

fn ical(events: &[CalendarEvent], year: i16) -> String {
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let mut out = String::from(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Acadex//Academic Calendar//EN\r\nCALSCALE:GREGORIAN\r\n",
    );
    out += &fold(&format!(
        "X-WR-CALNAME:Acadex academic calendar {year}-{:02}",
        (year + 1) % 100
    ));
    for event in events {
        out += "BEGIN:VEVENT\r\n";
        out += &fold(&format!("UID:{}@acadex", event.id));
        out += &fold(&format!("DTSTAMP:{stamp}"));
        // All-day events: DTEND is the day after the last day.
        out += &fold(&format!(
            "DTSTART;VALUE=DATE:{}",
            event.starts_on.format("%Y%m%d")
        ));
        out += &fold(&format!(
            "DTEND;VALUE=DATE:{}",
            (event.ends_on + Duration::days(1)).format("%Y%m%d")
        ));
        out += &fold(&format!("SUMMARY:{}", escape(&event.title)));
        out += &fold(&format!(
            "CATEGORIES:{}",
            event.kind.replace('_', " ").to_uppercase()
        ));
        if let Some(description) = &event.description {
            out += &fold(&format!("DESCRIPTION:{}", escape(description)));
        }
        out += "END:VEVENT\r\n";
    }
    out + "END:VCALENDAR\r\n"
}

/// RFC 5545 §3.3.11 text escaping.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
}

/// RFC 5545 §3.1: lines longer than 75 octets continue on the next line after
/// a space, never splitting a UTF-8 character.
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut width = 0;
    for ch in line.chars() {
        let len = ch.len_utf8();
        if width + len > 75 {
            out += "\r\n ";
            width = 1;
        }
        out.push(ch);
        width += len;
    }
    out + "\r\n"
}

pub async fn create_event(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Json(mut event): Json<NewEvent>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    user.require(Permission::CalendarManage)?;
    validate_event(&mut event)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let id = academic::create_event(&mut tx, &event).await?;
    let mut audit_event = client.event(
        "calendar.event_added",
        "calendar_event",
        Some(id.to_string()),
    );
    audit_event.metadata =
        json!({ "title": event.title, "kind": event.kind, "starts_on": event.starts_on });
    audit::record(&mut tx, &audit_event).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

fn validate_event(event: &mut NewEvent) -> Result<(), ApiError> {
    event.title = event.title.trim().to_owned();
    if event.title.is_empty() || event.title.chars().count() > 200 {
        return Err(ApiError::invalid(
            "invalid-title",
            "Title must be 1–200 characters",
            "title",
        ));
    }
    if !["holiday", "exam", "deadline", "event", "class_suspension"].contains(&event.kind.as_str())
    {
        return Err(ApiError::invalid(
            "invalid-kind",
            "No such event type",
            event.kind.clone(),
        ));
    }
    if event.ends_on < event.starts_on {
        return Err(ApiError::invalid(
            "invalid-dates",
            "The event ends before it starts",
            "ends_on",
        ));
    }
    Ok(())
}

pub async fn update_event(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
    Json(mut event): Json<NewEvent>,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::CalendarManage)?;
    validate_event(&mut event)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    if !academic::update_event(&mut tx, id, &event).await? {
        return Err(ApiError::not_found("event"));
    }
    let mut audit_event = client.event(
        "calendar.event_changed",
        "calendar_event",
        Some(id.to_string()),
    );
    audit_event.metadata = json!({
        "title": event.title, "kind": event.kind, "starts_on": event.starts_on, "ends_on": event.ends_on,
    });
    audit::record(&mut tx, &audit_event).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_event(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::CalendarManage)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    if !academic::delete_event(&mut tx, id).await? {
        return Err(ApiError::not_found("event"));
    }
    audit::record(
        &mut tx,
        &client.event(
            "calendar.event_removed",
            "calendar_event",
            Some(id.to_string()),
        ),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Attendance detail ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct MonthQuery {
    /// YYYY-MM; defaults to this month.
    month: Option<String>,
}

#[derive(Serialize)]
pub struct AttendanceMonth {
    month: String,
    academic_year: i16,
    days: Vec<academic::AttendanceDay>,
}

pub async fn attendance_month(
    State(state): State<AppState>,
    user: AuthUser,
    Path(student_id): Path<Uuid>,
    Query(query): Query<MonthQuery>,
) -> Result<Json<AttendanceMonth>, ApiError> {
    user.require(Permission::AttendanceRead)?;
    let first = match &query.month {
        Some(month) => {
            NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").map_err(|_| {
                ApiError::invalid("invalid-month", "Month must be YYYY-MM", month.clone())
            })?
        }
        None => {
            let today = Utc::now().date_naive();
            NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
                .ok_or_else(ApiError::internal)?
        }
    };
    let last = first
        .checked_add_months(chrono::Months::new(1))
        .ok_or_else(ApiError::internal)?
        - Duration::days(1);
    let academic_year = state
        .cache
        .settings(&state.pool)
        .await?
        .academic_year_of(first);

    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let student = people::student(&mut tx, student_id)
        .await?
        .ok_or_else(|| ApiError::not_found("student"))?;
    let days = academic::attendance_days(
        &mut tx,
        student.program_id,
        academic_year,
        student_id,
        first,
        last,
    )
    .await?;
    Ok(Json(AttendanceMonth {
        month: first.format("%Y-%m").to_string(),
        academic_year,
        days,
    }))
}

#[derive(Deserialize)]
pub struct SessionsQuery {
    subject: String,
    academic_year: Option<i16>,
}

pub async fn attendance_sessions(
    State(state): State<AppState>,
    user: AuthUser,
    Path(student_id): Path<Uuid>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<Vec<SessionView>>, ApiError> {
    user.require(Permission::AttendanceRead)?;
    let settings = state.cache.settings(&state.pool).await?;
    let academic_year = query
        .academic_year
        .unwrap_or_else(|| settings.academic_year_of(Utc::now().date_naive()));
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let student = people::student(&mut tx, student_id)
        .await?
        .ok_or_else(|| ApiError::not_found("student"))?;
    let sessions = academic::sessions(
        &mut tx,
        student.program_id,
        academic_year,
        student_id,
        &query.subject,
    )
    .await?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|session| {
                let times = settings.period_times(session.slot);
                SessionView {
                    starts_at: times.as_ref().map(|t| t.0.clone()),
                    ends_at: times.map(|t| t.1),
                    session,
                }
            })
            .collect(),
    ))
}

/// A session with its period's clock times, from the institution's settings.
#[derive(Serialize)]
pub struct SessionView {
    #[serde(flatten)]
    session: Session,
    starts_at: Option<String>,
    ends_at: Option<String>,
}

// ── Corrections ─────────────────────────────────────────────────────────────

const STATUSES: [&str; 4] = ["present", "absent", "late", "excused"];

pub async fn request_correction(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Json(mut request): Json<CorrectionRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    user.require(Permission::CorrectionsRequest)?;
    request.reason = request.reason.trim().to_owned();
    if !STATUSES.contains(&request.requested_status.as_str()) {
        return Err(ApiError::invalid(
            "invalid-status",
            "No such attendance status",
            request.requested_status.clone(),
        ));
    }
    if !(5..=1000).contains(&request.reason.chars().count()) {
        return Err(ApiError::invalid(
            "invalid-reason",
            "Give a reason of 5–1,000 characters",
            "reason",
        ));
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let id = match academic::request_correction(&mut tx, &request).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return Err(ApiError::invalid(
                "no-such-session",
                "There's no such session of yours, or it already has that status",
                request.session_date.to_string(),
            ));
        }
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return Err(ApiError::conflict(
                "already-requested",
                "A correction for this session is already waiting",
            ));
        }
        Err(err) => return Err(err.into()),
    };
    let mut event = client.event(
        "attendance.correction_requested",
        "attendance_correction",
        Some(id.to_string()),
    );
    event.metadata = json!({ "session_date": request.session_date, "slot": request.slot, "requested": request.requested_status });
    audit::record(&mut tx, &event).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn corrections(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<Correction>>, ApiError> {
    if user.require(Permission::CorrectionsDecide).is_err() {
        user.require(Permission::AttendanceRead)?;
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    Ok(Json(academic::corrections(&mut tx, 200).await?))
}

#[derive(Deserialize)]
pub struct DecisionRequest {
    approve: bool,
    note: Option<String>,
}

pub async fn decide_correction(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
    Json(decision): Json<DecisionRequest>,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::CorrectionsDecide)?;
    let note = decision
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let student = match academic::decide_correction(&mut tx, id, decision.approve, note).await? {
        Decision::Decided { student_id } => student_id,
        Decision::NotPending => return Err(ApiError::not_found("pending correction")),
        Decision::MarkChanged => {
            return Err(ApiError::conflict(
                "mark-changed",
                "The session's mark has changed since this was requested; reject it or ask the student to request again",
            ));
        }
    };
    let action = if decision.approve {
        "attendance.correction_approved"
    } else {
        "attendance.correction_rejected"
    };
    let mut event = client.event(action, "attendance_correction", Some(id.to_string()));
    event.metadata = json!({ "student_id": student, "note": note });
    audit::record(&mut tx, &event).await?;
    tx.commit().await?;
    state.cache.bump(&[attendance_scope(student)]).await;
    Ok(StatusCode::NO_CONTENT)
}

// ── Course status ───────────────────────────────────────────────────────────

// ── Student lookup ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ical_text_is_escaped_and_long_lines_folded() {
        assert_eq!(
            escape("Exams; rooms A, B\nbring ID"),
            "Exams\\; rooms A\\, B\\nbring ID"
        );
        let folded = fold(&format!("SUMMARY:{}", "é".repeat(60)));
        for line in folded.trim_end().split("\r\n") {
            assert!(line.len() <= 75, "{} octets", line.len());
        }
        assert!(
            folded.contains("\r\n "),
            "continuation lines start with a space"
        );
    }

    #[test]
    fn all_day_events_end_the_day_after() {
        let event = CalendarEvent {
            id: Uuid::nil(),
            kind: "holiday".to_owned(),
            title: "Diwali break".to_owned(),
            description: None,
            starts_on: NaiveDate::from_ymd_opt(2026, 10, 18).unwrap(),
            ends_on: NaiveDate::from_ymd_opt(2026, 10, 22).unwrap(),
            program_id: None,
            program_name: None,
        };
        let ics = ical(&[event], 2026);
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n") && ics.ends_with("END:VCALENDAR\r\n"));
        assert!(ics.contains("DTSTART;VALUE=DATE:20261018\r\n"));
        assert!(ics.contains("DTEND;VALUE=DATE:20261023\r\n"));
        assert!(ics.contains("CATEGORIES:HOLIDAY\r\n"));
    }
}
