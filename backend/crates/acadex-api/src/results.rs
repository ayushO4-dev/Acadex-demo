//! Results: a student's result sheet, and the exam cell's publication flow.

use acadex_auth::{Permission, Role};
use acadex_db::{
    audit,
    grading::{self, SemesterResult, SemesterSummary},
    notices::{self, NewNotice, Severity},
    outbox, people,
    results::{
        self, Component, ExternalEntry, ExternalRow, MarkEntry, MarksRow, NewResultSet,
        Publication, PublishOutcome,
    },
    rls,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthUser, ClientInfo},
    cache,
    error::ApiError,
};

#[derive(Deserialize)]
pub struct ResultsQuery {
    semester: Option<i16>,
}

#[derive(Serialize)]
pub struct ResultsReport {
    student: people::Student,
    /// Every semester with results the caller may see, for the selector.
    semesters: Vec<SemesterSummary>,
    /// The requested semester (default: the latest). `None` if it has no
    /// visible results, e.g. not yet published.
    selected: Option<SemesterResult>,
}

pub async fn student_results(
    State(state): State<AppState>,
    user: AuthUser,
    Path(student_id): Path<Uuid>,
    Query(query): Query<ResultsQuery>,
) -> Result<Response, ApiError> {
    user.require(Permission::ResultsRead)?;
    let semester = query
        .semester
        .map_or("latest".to_owned(), |s| s.to_string());
    let cached = state
        .cache
        .json(
            "results",
            RESULTS_SCOPE,
            &format!("v{}:s{student_id}:{semester}", user.id),
            || results_report(&state, user, student_id, query.semester),
        )
        .await?;
    Ok(cache::json_response(cached))
}

/// One epoch for every result: publishing is rare, and a bump makes the new
/// results visible at once instead of after the TTL.
const RESULTS_SCOPE: &str = "results";

async fn results_report(
    state: &AppState,
    user: AuthUser,
    student_id: Uuid,
    semester: Option<i16>,
) -> Result<ResultsReport, ApiError> {
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let student = people::student(&mut tx, student_id)
        .await?
        .ok_or_else(|| ApiError::not_found("student"))?;
    let grading = results::grading(&mut tx).await?;
    let rows = results::student_marks(&mut tx, student_id).await?;

    let mut semesters = grading::by_semester(rows, &grading);
    let wanted = semester.or_else(|| semesters.last().map(|s| s.summary.semester));
    let summaries = semesters.iter().map(|s| s.summary.clone()).collect();
    let selected = semesters
        .iter()
        .rposition(|s| Some(s.summary.semester) == wanted)
        .map(|index| semesters.swap_remove(index));

    Ok(ResultsReport {
        student,
        semesters: summaries,
        selected,
    })
}

pub async fn publications(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<Publication>>, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    Ok(Json(results::publications(&mut tx).await?))
}

/// Opens an empty result set for marks entry: a semester's regular results,
/// or a year's backlog results for it.
pub async fn create_set(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Json(set): Json<NewResultSet>,
) -> Result<(StatusCode, Json<Publication>), ApiError> {
    user.require(Permission::ResultsPublish)?;
    if !matches!(set.kind.as_str(), "regular" | "backlog") {
        return Err(ApiError::invalid(
            "invalid-kind",
            "Unknown kind of result set",
            "A result set is regular or backlog; re-evaluation sets come from requests.",
        ));
    }
    if !(1..=8).contains(&set.semester) {
        return Err(ApiError::invalid(
            "invalid-semester",
            "No such semester",
            "Semesters run from 1 to 8.",
        ));
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let exists = results::publications(&mut tx).await?.into_iter().any(|p| {
        (p.program_id, p.semester, p.academic_year, p.kind.as_str())
            == (
                set.program_id,
                set.semester,
                set.academic_year,
                set.kind.as_str(),
            )
    });
    if exists {
        return Err(ApiError::conflict(
            "set-exists",
            "That result set already exists",
        ));
    }
    let id = results::create_set(&mut tx, &set).await?;
    let mut event = client.event(
        "results.set_created",
        "result_publication",
        Some(id.to_string()),
    );
    event.metadata = json!({ "program_id": set.program_id, "semester": set.semester,
                             "academic_year": set.academic_year, "kind": set.kind });
    audit::record(&mut tx, &event).await?;
    let created = results::publication(&mut tx, id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Step one of two: asks for a staged set to be published. Someone else in
/// the exam cell then approves it (architecture.md §9.3).
pub async fn request_publication(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
) -> Result<Json<Publication>, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let staged = results::publication(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::not_found("result set"))?;
    if staged.students == 0 {
        return Err(ApiError::invalid(
            "no-marks",
            "No marks entered yet",
            "Enter the external marks before asking to publish.",
        ));
    }
    if !results::request_publication(&mut tx, id).await? {
        return Err(ApiError::conflict(
            "already-requested",
            "Publishing these results was already requested",
        ));
    }
    let mut event = client.event(
        "results.publish_requested",
        "result_publication",
        Some(id.to_string()),
    );
    event.metadata = json!({ "program": staged.program_name, "semester": staged.semester,
                             "academic_year": staged.academic_year, "kind": staged.kind });
    audit::record(&mut tx, &event).await?;
    let requested = results::publication(&mut tx, id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await?;
    Ok(Json(requested))
}

/// Withdraws one's own request, e.g. to correct a mark first.
pub async fn withdraw_request(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
) -> Result<Json<Publication>, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    if !results::withdraw_request(&mut tx, id).await? {
        return Err(ApiError::conflict(
            "not-your-request",
            "There's no request of yours to withdraw",
        ));
    }
    audit::record(
        &mut tx,
        &client.event(
            "results.request_withdrawn",
            "result_publication",
            Some(id.to_string()),
        ),
    )
    .await?;
    let staged = results::publication(&mut tx, id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await?;
    Ok(Json(staged))
}

/// Makes a requested result set visible to its students, in one transaction
/// with everything that follows from it: a notice to the program, the audit
/// entry, and a `results.published` event for the worker (grade cards). The
/// approver must not be the requester.
#[derive(Deserialize)]
pub struct WithdrawalBody {
    reason: String,
}

/// Step one of withdrawing published results: ask, with a reason.
pub async fn request_withdrawal(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
    Json(body): Json<WithdrawalBody>,
) -> Result<Json<Publication>, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let reason = body.reason.trim();
    if !(10..=500).contains(&reason.chars().count()) {
        return Err(ApiError::invalid(
            "reason-required",
            "Say what's wrong with these results",
            "10 to 500 characters",
        ));
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    if !results::request_withdrawal(&mut tx, id, reason).await? {
        return Err(ApiError::conflict(
            "not-withdrawable",
            "Only published results can be withdrawn, and a withdrawal is asked for once",
        ));
    }
    let mut event = client.event(
        "results.withdrawal_requested",
        "result_publication",
        Some(id.to_string()),
    );
    event.metadata = json!({ "reason": reason });
    audit::record(&mut tx, &event).await?;
    let publication = results::publication(&mut tx, id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await?;
    Ok(Json(publication))
}

pub async fn cancel_withdrawal(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    if !results::cancel_withdrawal(&mut tx, id).await? {
        return Err(ApiError::conflict(
            "not-yours",
            "Only the person who asked can take the request back",
        ));
    }
    audit::record(
        &mut tx,
        &client.event(
            "results.withdrawal_cancelled",
            "result_publication",
            Some(id.to_string()),
        ),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Step two: a different exam-cell member approves. The set goes back to
/// staged, its grade cards come down, and the program's students are told.
pub async fn approve_withdrawal(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
) -> Result<Json<Publication>, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let before = results::publication(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::not_found("result set"))?;
    match results::approve_withdrawal(&mut tx, id).await?.as_str() {
        "withdrawn" => {}
        "same-person" => {
            return Err(ApiError::forbidden(
                "same-person",
                "Someone else in the exam cell must approve your request",
            ));
        }
        "revaluations-open" => {
            return Err(ApiError::conflict(
                "revaluations-open",
                "Re-evaluations on these results are still open; decide them first",
            ));
        }
        _ => {
            return Err(ApiError::conflict(
                "not-requested",
                "Nobody has asked to withdraw these results",
            ));
        }
    }
    notices::publish(
        &mut tx,
        &NewNotice {
            title: format!("Semester {} results withdrawn for correction", before.semester),
            body: format!(
                "The published results for {} semester {} have been withdrawn while an error is corrected. \
                 Corrected results will be published again; you will be notified.",
                before.program_name, before.semester
            ),
            severity: Severity::Important,
            program_id: Some(before.program_id),
            ..Default::default()
        },
    )
    .await?;
    let mut event = client.event(
        "results.withdrawn",
        "result_publication",
        Some(id.to_string()),
    );
    event.metadata = json!({
        "program": before.program_name, "semester": before.semester, "academic_year": before.academic_year,
        "kind": before.kind, "reason": before.withdrawal_reason, "requested_by": before.withdrawal_requested_by,
    });
    audit::record(&mut tx, &event).await?;
    let after = results::publication(&mut tx, id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await?;
    state.cache.bump(&[RESULTS_SCOPE.to_owned()]).await;
    Ok(Json(after))
}

pub async fn publish(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
) -> Result<Json<Publication>, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let staged = results::publication(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::not_found("result set"))?;
    match results::publish(&mut tx, id).await? {
        PublishOutcome::Published => {}
        PublishOutcome::AlreadyPublished => {
            return Err(ApiError::conflict(
                "already-published",
                "These results are already published",
            ));
        }
        PublishOutcome::NotRequested => {
            return Err(ApiError::conflict(
                "not-requested",
                "Nobody has asked to publish these results yet",
            ));
        }
        PublishOutcome::SamePerson => {
            return Err(ApiError::forbidden(
                "same-person",
                "Someone else in the exam cell must approve your request",
            ));
        }
    }

    let year = format!(
        "{}–{:02}",
        staged.academic_year,
        (staged.academic_year + 1) % 100
    );
    notices::publish(
        &mut tx,
        &NewNotice {
            title: match staged.kind.as_str() {
                "backlog" => format!("Semester {} backlog results published", staged.semester),
                "revaluation" => format!("Semester {} re-evaluation results published", staged.semester),
                _ => format!("Semester {} results published", staged.semester),
            },
            body: format!(
                "Results for {} semester {} (academic year {year}) are now available under Examination → Results.",
                staged.program_name, staged.semester
            ),
            severity: Severity::Important,
            program_id: Some(staged.program_id),
            ..Default::default()
        },
    )
    .await?;

    let event = json!({
        "publication_id": id,
        "program_id": staged.program_id,
        "semester": staged.semester,
        "academic_year": staged.academic_year,
        "kind": staged.kind,
    });
    outbox::enqueue(&mut tx, "results.published", Some(&id.to_string()), &event).await?;

    let mut audit_event = client.event(
        "results.published",
        "result_publication",
        Some(id.to_string()),
    );
    audit_event.metadata = json!({
        "program": staged.program_name,
        "semester": staged.semester,
        "academic_year": staged.academic_year,
        "students": staged.students,
        "kind": staged.kind,
        "requested_by": staged.requested_by,
    });
    audit::record(&mut tx, &audit_event).await?;

    let published = results::publication(&mut tx, id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await?;
    state.cache.bump(&[RESULTS_SCOPE.to_owned()]).await;
    state.outbox_wakeup.notify_one();
    Ok(Json(published))
}

/// A student's grade card for a semester, made by the worker once results
/// are published. Students get their own; staff per RLS.
pub async fn grade_card(
    State(state): State<AppState>,
    user: AuthUser,
    Path((student_id, semester)): Path<(Uuid, i16)>,
) -> Result<Response, ApiError> {
    user.require(Permission::ResultsRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let object = results::card_object(&mut tx, student_id, semester).await?;
    tx.commit().await?;
    let object = object.ok_or_else(|| {
        ApiError::conflict(
            "grade-card-pending",
            "The grade card isn't ready yet; it's made shortly after results are published",
        )
    })?;
    pdf(
        &state,
        &object,
        &format!("grade-card-semester-{semester}.pdf"),
    )
    .await
}

// ── Marks entry ─────────────────────────────────────────────────────────────

/// Internal marks are the subject teacher's (the exam cell may step in);
/// both lock once the subject's result is published.
async fn internal_access(
    tx: &mut sqlx::PgConnection,
    user: &AuthUser,
    offering: Uuid,
    writing: bool,
) -> Result<(), ApiError> {
    user.require(Permission::MarksEnter)?;
    let (teaches, published) = results::offering_access(tx, offering)
        .await?
        .ok_or_else(|| ApiError::not_found("offering"))?;
    if !teaches && !matches!(user.role, Role::ExamCell | Role::SuperAdmin) {
        return Err(ApiError::forbidden(
            "not-your-subject",
            "You don't teach this subject",
        ));
    }
    if writing && published {
        return Err(ApiError::conflict(
            "results-published",
            "This subject's results are published; marks can't change",
        ));
    }
    Ok(())
}

#[derive(Serialize)]
pub struct InternalSheet {
    components: Vec<Component>,
    published: bool,
    students: Vec<MarksRow>,
}

pub async fn internal_sheet(
    State(state): State<AppState>,
    user: AuthUser,
    Path(offering): Path<Uuid>,
) -> Result<Json<InternalSheet>, ApiError> {
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    internal_access(&mut tx, &user, offering, false).await?;
    let published = results::offering_access(&mut tx, offering)
        .await?
        .is_some_and(|(_, published)| published);
    Ok(Json(InternalSheet {
        components: results::components(&mut tx, offering).await?,
        published,
        students: results::internal_roster(&mut tx, offering).await?,
    }))
}

/// Sets an offering's assessment components (e.g. CCA1 10, CCA2 10, MID 20).
pub async fn set_components(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(offering): Path<Uuid>,
    Json(components): Json<Vec<Component>>,
) -> Result<Json<Vec<Component>>, ApiError> {
    let mut codes: Vec<&str> = components.iter().map(|c| c.code.trim()).collect();
    codes.sort_unstable();
    codes.dedup();
    if components.is_empty()
        || components.len() > 8
        || codes.len() != components.len()
        || components.iter().any(|c| {
            c.code.trim().is_empty() || c.code.len() > 12 || !(1..=100).contains(&c.max_marks)
        })
    {
        return Err(ApiError::invalid(
            "invalid-components",
            "Check the components",
            "One to eight components, each with a unique short code and 1–100 marks.",
        ));
    }
    let total: i32 = components.iter().map(|c| i32::from(c.max_marks)).sum();
    if total > 100 {
        return Err(ApiError::invalid(
            "invalid-components",
            "Check the components",
            "Internal assessment can't add up to more than 100 marks.",
        ));
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    internal_access(&mut tx, &user, offering, true).await?;
    let trimmed: Vec<Component> = components
        .into_iter()
        .map(|c| Component {
            code: c.code.trim().to_uppercase(),
            ..c
        })
        .collect();
    if let Err(err) = results::replace_components(&mut tx, offering, &trimmed).await {
        // A removed component that already has marks.
        if err
            .as_database_error()
            .is_some_and(|e| e.is_foreign_key_violation())
        {
            return Err(ApiError::conflict(
                "component-has-marks",
                "A component you removed already has marks",
            ));
        }
        return Err(err.into());
    }
    let mut event = client.event(
        "marks.components_set",
        "offering",
        Some(offering.to_string()),
    );
    event.metadata = json!({ "components": trimmed.iter().map(|c| format!("{} /{}", c.code, c.max_marks)).collect::<Vec<_>>() });
    audit::record(&mut tx, &event).await?;
    let saved = results::components(&mut tx, offering).await?;
    tx.commit().await?;
    Ok(Json(saved))
}

#[derive(Serialize)]
pub struct Saved {
    saved: u64,
}

pub async fn save_internal(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(offering): Path<Uuid>,
    Json(entries): Json<Vec<MarkEntry>>,
) -> Result<Json<Saved>, ApiError> {
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    internal_access(&mut tx, &user, offering, true).await?;
    let components = results::components(&mut tx, offering).await?;
    let roster = results::internal_roster(&mut tx, offering).await?;
    for entry in &entries {
        let Some(component) = components.iter().find(|c| c.id == Some(entry.component_id)) else {
            return Err(ApiError::invalid(
                "unknown-component",
                "Unknown component",
                "Reload the sheet and try again.",
            ));
        };
        if !roster.iter().any(|r| r.student_id == entry.student_id) {
            return Err(ApiError::invalid(
                "not-on-roster",
                "Unknown student",
                "A student isn't on this subject's roster.",
            ));
        }
        if !(0.0..=f64::from(component.max_marks)).contains(&entry.marks)
            || (entry.marks * 2.0).fract() != 0.0
        {
            return Err(ApiError::invalid(
                "marks-out-of-range",
                "Marks out of range",
                format!(
                    "{} marks run from 0 to {}, in halves.",
                    component.code, component.max_marks
                ),
            ));
        }
    }
    let saved = results::save_internal(&mut tx, &entries).await?;
    let mut event = client.event(
        "marks.internal_saved",
        "offering",
        Some(offering.to_string()),
    );
    event.metadata = json!({ "entries": entries.len() });
    audit::record(&mut tx, &event).await?;
    tx.commit().await?;
    Ok(Json(Saved { saved }))
}

#[derive(Serialize)]
pub struct SetSubject {
    offering_id: Uuid,
    code: String,
    name: String,
}

/// The subjects a result set covers, for its marks entry.
pub async fn set_subjects(
    State(state): State<AppState>,
    user: AuthUser,
    Path(set): Path<Uuid>,
) -> Result<Json<Vec<SetSubject>>, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let subjects = results::set_offerings(&mut tx, set).await?;
    Ok(Json(
        subjects
            .into_iter()
            .map(|(offering_id, code, name)| SetSubject {
                offering_id,
                code,
                name,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct ExternalSheet {
    publication: Publication,
    max_marks: i16,
    students: Vec<ExternalRow>,
}

/// External exams are out of 60 here (with 40 internal), as in the seed.
const EXTERNAL_MAX: i16 = 60;

async fn staged_set(tx: &mut sqlx::PgConnection, set: Uuid) -> Result<Publication, ApiError> {
    results::publication(tx, set)
        .await?
        .ok_or_else(|| ApiError::not_found("result set"))
}

pub async fn external_sheet(
    State(state): State<AppState>,
    user: AuthUser,
    Path((set, offering)): Path<(Uuid, Uuid)>,
) -> Result<Json<ExternalSheet>, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let publication = staged_set(&mut tx, set).await?;
    Ok(Json(ExternalSheet {
        publication,
        max_marks: EXTERNAL_MAX,
        students: results::external_roster(&mut tx, set, offering).await?,
    }))
}

/// Enters external marks into a staged set. Once publishing is requested
/// the marks are frozen: withdraw the request to correct one.
pub async fn save_external(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path((set, offering)): Path<(Uuid, Uuid)>,
    Json(entries): Json<Vec<ExternalEntry>>,
) -> Result<Json<Saved>, ApiError> {
    user.require(Permission::ResultsPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let publication = staged_set(&mut tx, set).await?;
    if publication.published_at.is_some() {
        return Err(ApiError::conflict(
            "results-published",
            "These results are published; marks can't change",
        ));
    }
    if publication.requested_at.is_some() {
        return Err(ApiError::conflict(
            "publish-requested",
            "Publishing was requested; withdraw the request to change marks",
        ));
    }
    if publication.kind == "revaluation" {
        return Err(ApiError::conflict(
            "decided-marks",
            "Re-evaluation marks come from the decisions on requests",
        ));
    }
    let roster = results::external_roster(&mut tx, set, offering).await?;
    for entry in &entries {
        if !roster
            .iter()
            .any(|r| r.student_id == entry.student_id && r.attempt == entry.attempt)
        {
            return Err(ApiError::invalid(
                "not-on-roster",
                "Unknown student",
                "A student doesn't sit this paper in this result set.",
            ));
        }
        if !(0.0..=f64::from(EXTERNAL_MAX)).contains(&entry.marks)
            || (entry.marks * 2.0).fract() != 0.0
        {
            return Err(ApiError::invalid(
                "marks-out-of-range",
                "Marks out of range",
                format!("External marks run from 0 to {EXTERNAL_MAX}, in halves."),
            ));
        }
    }
    let saved = results::save_external(&mut tx, set, offering, EXTERNAL_MAX, &entries).await?;
    let mut event = client.event(
        "marks.external_saved",
        "result_publication",
        Some(set.to_string()),
    );
    event.metadata = json!({ "offering_id": offering, "entries": entries.len() });
    audit::record(&mut tx, &event).await?;
    tx.commit().await?;
    Ok(Json(Saved { saved }))
}

/// Streams a stored PDF from the uploads bucket.
pub(crate) async fn pdf(
    state: &AppState,
    object: &str,
    filename: &str,
) -> Result<Response, ApiError> {
    let bytes = state
        .files
        .storage
        .get(&state.files.uploads_bucket, object)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, %object, "reading a document failed");
            ApiError::upstream(
                "storage-unavailable",
                "The document couldn't be fetched. Please try again.",
            )
        })?;
    use axum::{
        http::{HeaderValue, header},
        response::IntoResponse,
    };
    let mut response = bytes.into_response();
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
