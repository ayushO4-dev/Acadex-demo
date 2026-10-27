use acadex_auth::{Permission, Role, rbac};
use acadex_db::{attendance, attendance::SubjectAttendance, audit, people, rls};
use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
    routing::{get, post, put},
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgConnection;
use tower_http::{set_header::SetResponseHeaderLayer, trace::TraceLayer};
use uuid::Uuid;

use crate::{
    AppState, academic, admin,
    auth::{AuthUser, ClientInfo},
    cache,
    error::ApiError,
    fees, logs, notices, offerings, platform, results, timetable, trace,
};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/me", get(me))
        .route("/sessions", post(create_session).delete(end_session))
        .route("/institution", get(platform::public_institution))
        .route("/institution/logo", get(platform::public_logo))
        .route("/students/{id}/attendance", get(student_attendance))
        .route(
            "/academic/calendar",
            get(academic::calendar).post(academic::create_event),
        )
        .route("/academic/calendar.ics", get(academic::calendar_ics))
        .route(
            "/academic/calendar/{id}",
            put(academic::update_event).delete(academic::delete_event),
        )
        .route(
            "/attendance/corrections",
            get(academic::corrections).post(academic::request_correction),
        )
        .route(
            "/attendance/corrections/{id}/decision",
            post(academic::decide_correction),
        )
        // Read-only in the demo: faculty start marking attendance from it.
        .route("/academic/timetable", get(timetable::week))
        .route("/audit/logs", get(logs::list))
        .route("/audit/logs/export", get(logs::export))
        .route("/configuration", get(admin::configuration))
        .route("/fees/payable", get(fees::payable))
        .route("/fees/payments", post(fees::start_payment))
        .route("/fees/payments/{id}", get(fees::payment_status))
        .route("/fees/receipts", get(fees::receipts))
        .route("/fees/receipts/{id}/pdf", get(fees::receipt_pdf))
        .route("/notices", get(notices::list).post(notices::publish))
        .route("/notices/public", get(notices::public))
        .route("/notices/read", post(notices::mark_read))
        .route(
            "/notices/awaiting-acknowledgement",
            get(notices::awaiting_acknowledgement),
        )
        .route(
            "/notices/preferences",
            get(notices::preferences).put(notices::set_preferences),
        )
        .route("/notices/stream", get(notices::stream))
        .route(
            "/notices/{id}",
            put(notices::edit).delete(notices::delete_scheduled),
        )
        .route("/notices/{id}/withdraw", post(notices::withdraw))
        .route("/notices/{id}/acknowledge", post(notices::acknowledge))
        .route("/notices/{id}/receipts", get(notices::receipts))
        .route(
            "/notices/{id}/attachments",
            post(notices::attach)
                .layer(DefaultBodyLimit::max(notices::MAX_UPLOAD_BYTES + 64 * 1024)),
        )
        .route(
            "/notices/{id}/attachments/{attachment}",
            get(notices::attachment),
        )
        .route("/programs", get(programs))
        .route("/offerings", get(offerings::list))
        .route(
            "/results/publications",
            get(results::publications).post(results::create_set),
        )
        .route(
            "/results/publications/{id}/request",
            post(results::request_publication).delete(results::withdraw_request),
        )
        .route("/results/publications/{id}/publish", post(results::publish))
        .route(
            "/results/publications/{id}/withdrawal",
            post(results::request_withdrawal).delete(results::cancel_withdrawal),
        )
        .route(
            "/results/publications/{id}/withdrawal/approve",
            post(results::approve_withdrawal),
        )
        .route(
            "/results/publications/{id}/subjects",
            get(results::set_subjects),
        )
        .route(
            "/results/publications/{id}/subjects/{offering}/marks",
            get(results::external_sheet).put(results::save_external),
        )
        .route(
            "/students/{id}/attendance/days",
            get(academic::attendance_month),
        )
        .route(
            "/students/{id}/attendance/sessions",
            get(academic::attendance_sessions),
        )
        .route("/students/{id}/results", get(results::student_results))
        .route(
            "/students/{id}/grade-cards/{semester}/pdf",
            get(results::grade_card),
        )
        // Reached from the internet through the load balancer, behind Cloud
        // Armor (architecture.md §3.1); authenticated by signature, not token.
        .route("/webhooks/payments", post(fees::webhook))
        .route("/offerings/{id}/roster", get(offerings::roster))
        .route(
            "/offerings/{id}/internal",
            get(results::internal_sheet).put(results::save_internal),
        )
        .route("/offerings/{id}/components", put(results::set_components))
        .route("/offerings/{id}/attendance", put(offerings::mark))
        // Every API response is per-user unless a handler says otherwise (the
        // public notice board does): never stored by a shared cache.
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        ))
        // Shed or limit before any work is done; inside tracing, so a refused
        // request still shows in traces and logs.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::limits::guard,
        ))
        // Before the limiter: it keys anonymous callers on this address.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::client_addr::resolve,
        ))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::request_span)
                .on_response(trace::record_response),
        )
        .with_state(state)
}

/// Liveness: the process is up. Never touches dependencies.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Readiness: the API can serve traffic, which needs the database.
async fn ready(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    match acadex_db::ping(&state.pool).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "status": "ready", "database": "ok" })),
        ),
        Err(err) => {
            tracing::warn!(error = %err, "database ping failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "not_ready", "database": "unreachable" })),
            )
        }
    }
}

#[derive(Serialize)]
struct Me {
    #[serde(flatten)]
    user: people::User,
    permissions: Vec<&'static str>,
    student: Option<people::Student>,
    /// The department an HOD heads.
    department: Option<acadex_db::org::Department>,
    /// For the notifications bell.
    unread_notices: i64,
    /// Their own, for the sidebar.
    email: Option<String>,
}

async fn load_me(conn: &mut PgConnection, user: &AuthUser) -> Result<Me, ApiError> {
    let profile = people::user(conn, user.id)
        .await?
        .ok_or_else(|| ApiError::not_found("user"))?;
    let student = match user.role {
        Role::Student => people::student(conn, user.id).await?,
        _ => None,
    };
    let department = match user.role {
        Role::Hod => acadex_db::org::headed_by_me(conn).await?,
        _ => None,
    };
    Ok(Me {
        user: profile,
        permissions: rbac::permissions(user.role)
            .map(Permission::as_str)
            .collect(),
        student,
        department,
        unread_notices: acadex_db::notices::unread_count(conn).await?,
        email: sqlx::query_scalar("SELECT identity.my_email()")
            .fetch_one(conn)
            .await?,
    })
}

async fn me(State(state): State<AppState>, user: AuthUser) -> Result<Json<Me>, ApiError> {
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    Ok(Json(load_me(&mut tx, &user).await?))
}

async fn create_session(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
) -> Result<(StatusCode, Json<Me>), ApiError> {
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    audit::record(&mut tx, &client.event("auth.login", "session", None)).await?;
    let me = load_me(&mut tx, &user).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(me)))
}

/// Called by the BFF as the user signs out, before Auth0 ends the session.
async fn end_session(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
) -> Result<StatusCode, ApiError> {
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    audit::record(&mut tx, &client.event("auth.logout", "session", None)).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, sqlx::FromRow)]
struct Program {
    id: i16,
    code: String,
    name: String,
    semesters: i16,
    department_id: Option<i16>,
}

/// Reference data for pickers, e.g. a notice's audience.
async fn programs(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<Program>>, ApiError> {
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let programs = sqlx::query_as(
        "SELECT id, code, name, semesters, department_id FROM org.programs ORDER BY name",
    )
    .fetch_all(&mut *tx)
    .await?;
    Ok(Json(programs))
}

#[derive(Deserialize)]
struct AttendanceQuery {
    /// Default: the student's current semester.
    semester: Option<i16>,
    /// A whole academic year instead (both its semesters).
    academic_year: Option<i16>,
}

#[derive(Serialize)]
struct AttendanceReport {
    student: people::Student,
    academic_year: i16,
    /// The semester shown; `None` for a whole academic year.
    semester: Option<i16>,
    /// Semesters so far, newest first, for the selector.
    semesters: Vec<i16>,
    /// Where the semester's attendance ends; `None` if it has none.
    last_session_on: Option<NaiveDate>,
    shortage_threshold_pct: f64,
    overall: Totals,
    subjects: Vec<SubjectReport>,
}

#[derive(Serialize)]
struct SubjectReport {
    code: String,
    name: String,
    credits: i16,
    /// Both components together; `shortage` is set if either component is short.
    #[serde(flatten)]
    totals: Totals,
    /// Each component on its own, when the subject has sessions of it.
    theory: Option<Totals>,
    practical: Option<Totals>,
}

impl SubjectReport {
    /// Theory and practical are each held to the threshold: a subject is in
    /// shortage if either is, even when the combined figure clears it.
    fn new(row: SubjectAttendance, threshold_pct: f64) -> Self {
        let component = |conducted: i64, attended: i64| {
            (conducted > 0).then(|| Totals::new(conducted, attended, threshold_pct))
        };
        let theory = component(row.theory_conducted, row.theory_attended);
        let practical = component(row.practical_conducted, row.practical_attended);
        let mut totals = Totals::new(row.conducted, row.attended, threshold_pct);
        totals.shortage = theory.as_ref().is_some_and(|t| t.shortage)
            || practical.as_ref().is_some_and(|t| t.shortage);
        Self {
            totals,
            theory,
            practical,
            code: row.code,
            name: row.name,
            credits: row.credits,
        }
    }
}

#[derive(Serialize)]
struct Totals {
    conducted: i64,
    attended: i64,
    percentage: f64,
    shortage: bool,
}

impl Totals {
    fn new(conducted: i64, attended: i64, threshold_pct: f64) -> Self {
        let percentage = if conducted == 0 {
            0.0
        } else {
            (attended as f64 * 1000.0 / conducted as f64).round() / 10.0
        };
        let shortage = conducted > 0 && percentage < threshold_pct;
        Self {
            conducted,
            attended,
            percentage,
            shortage,
        }
    }
}

async fn student_attendance(
    State(state): State<AppState>,
    user: AuthUser,
    Path(student_id): Path<Uuid>,
    Query(query): Query<AttendanceQuery>,
) -> Result<Response, ApiError> {
    user.require(Permission::AttendanceRead)?;
    let settings = state.cache.settings(&state.pool).await?;
    let key = format!(
        "v{}:s{student_id}:sem{}:y{}",
        user.id,
        query.semester.map_or(String::new(), |s| s.to_string()),
        query.academic_year.map_or(String::new(), |y| y.to_string()),
    );
    let cached = state
        .cache
        .json("attendance", &attendance_scope(student_id), &key, || {
            attendance_report(
                &state,
                user,
                student_id,
                query.semester,
                query.academic_year,
                settings.attendance_threshold_pct,
            )
        })
        .await?;
    Ok(cache::json_response(cached))
}

/// Bumped whenever the student's attendance is marked.
pub(crate) fn attendance_scope(student_id: Uuid) -> String {
    format!("attendance:{student_id}")
}

async fn attendance_report(
    state: &AppState,
    user: AuthUser,
    student_id: Uuid,
    semester: Option<i16>,
    academic_year: Option<i16>,
    threshold_pct: f64,
) -> Result<AttendanceReport, ApiError> {
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    // Whose records are visible is RLS's job: asking for a student you may not
    // see finds nothing, exactly as if they didn't exist.
    let student = people::student(&mut tx, student_id)
        .await?
        .ok_or_else(|| ApiError::not_found("student"))?;
    // A semester's year follows from the batch: semesters 1-2 in the first year.
    let (semester, academic_year) = match (semester, academic_year) {
        (Some(sem), _) => {
            if !(1..=student.current_semester).contains(&sem) {
                return Err(ApiError::invalid(
                    "invalid-semester",
                    "No such semester yet",
                    format!("Semesters run from 1 to {}.", student.current_semester),
                ));
            }
            (Some(sem), student.batch_year + (sem - 1) / 2)
        }
        (None, Some(year)) => (None, year),
        (None, None) => (
            Some(student.current_semester),
            student.batch_year + (student.current_semester - 1) / 2,
        ),
    };
    let rows = attendance::summary(
        &mut tx,
        student.program_id,
        academic_year,
        student_id,
        semester,
    )
    .await?;
    let last_session_on = attendance::last_session_on(
        &mut tx,
        student.program_id,
        academic_year,
        student_id,
        semester,
    )
    .await?;
    let semesters = (1..=student.current_semester).rev().collect();

    let overall = Totals::new(
        rows.iter().map(|row| row.conducted).sum(),
        rows.iter().map(|row| row.attended).sum(),
        threshold_pct,
    );
    let subjects = rows
        .into_iter()
        .map(|row| SubjectReport::new(row, threshold_pct))
        .collect();

    Ok(AttendanceReport {
        student,
        academic_year,
        semester,
        semesters,
        last_session_on,
        shortage_threshold_pct: threshold_pct,
        overall,
        subjects,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortage_is_below_the_threshold_and_needs_sessions() {
        assert!(Totals::new(4, 2, 75.0).shortage);
        assert!(
            !Totals::new(4, 3, 75.0).shortage,
            "exactly 75% is not a shortage"
        );
        assert!(
            !Totals::new(0, 0, 75.0).shortage,
            "no sessions yet is not a shortage"
        );
        assert_eq!(Totals::new(3, 2, 75.0).percentage, 66.7);
    }

    #[test]
    fn theory_and_practical_are_each_held_to_the_threshold() {
        let row = |tc, ta, pc, pa| SubjectAttendance {
            code: "EC501".to_owned(),
            name: "Digital Communication".to_owned(),
            credits: 4,
            conducted: tc + pc,
            attended: ta + pa,
            theory_conducted: tc,
            theory_attended: ta,
            practical_conducted: pc,
            practical_attended: pa,
        };
        // 36/40 overall is 90%, but the practical's 6/10 is short.
        let short = SubjectReport::new(row(30, 30, 10, 6), 75.0);
        assert!(!short.theory.as_ref().unwrap().shortage);
        assert!(short.practical.as_ref().unwrap().shortage);
        assert!(short.totals.shortage, "short in one component is short");
        let fine = SubjectReport::new(row(30, 24, 0, 0), 75.0);
        assert!(
            fine.practical.is_none(),
            "no practicals, no practical figure"
        );
        assert!(!fine.totals.shortage);
    }
}
