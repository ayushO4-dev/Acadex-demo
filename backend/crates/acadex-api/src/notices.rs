//! Notices: the public board, the signed-in list with read state, publishing
//! (exam cell and administration) with targeting, scheduling and attachments,
//! read receipts, acknowledgement of critical notices, email preferences,
//! and the live stream behind the bell.

use acadex_auth::Permission;
use acadex_db::{
    audit,
    notices::{self, NewNotice, Notice, PublicNotice, Reader, Receipts},
    outbox, rls,
};
use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use chrono::{Duration, Utc};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{AuthUser, ClientInfo},
    error::ApiError,
};

pub const MAX_UPLOAD_BYTES: usize = 5 * 1024 * 1024;

const PUBLIC_LIMIT: i64 = 20;
const LIST_LIMIT: i64 = 50;
/// The public board may be a minute stale, so the CDN can absorb its traffic
/// (architecture.md §2.6, tier 0).
const PUBLIC_CACHE: &str = "public, max-age=60, stale-while-revalidate=300";

/// No sign-in, and the same answer for everyone, so it is cacheable.
pub async fn public(State(state): State<AppState>) -> Result<Response, ApiError> {
    // Deliberately not `begin_as`: with no acting user, RLS shows only what
    // the public policy allows.
    let mut tx = state.pool.begin().await?;
    let list: Vec<PublicNotice> = notices::public(&mut tx, PUBLIC_LIMIT).await?;
    let mut response = Json(list).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(PUBLIC_CACHE),
    );
    Ok(response)
}

#[derive(Serialize)]
pub struct NoticeList {
    notices: Vec<Notice>,
    unread: i64,
    /// Pass back as before/before_id for older notices.
    next: Option<(chrono::DateTime<Utc>, Uuid)>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    before: Option<chrono::DateTime<Utc>>,
    before_id: Option<Uuid>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ListQuery>,
) -> Result<Json<NoticeList>, ApiError> {
    user.require(Permission::NoticesRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let notices = notices::visible(&mut tx, LIST_LIMIT, query.before.zip(query.before_id)).await?;
    let unread = notices::unread_count(&mut tx).await?;
    let next = (notices.len() == LIST_LIMIT as usize)
        .then(|| notices.last().map(|n| (n.published_at, n.id)))
        .flatten();
    Ok(Json(NoticeList {
        notices,
        unread,
        next,
    }))
}

#[derive(Deserialize)]
pub struct ReadRequest {
    ids: Vec<Uuid>,
}

pub async fn mark_read(
    State(state): State<AppState>,
    user: AuthUser,
    Json(request): Json<ReadRequest>,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::NoticesRead)?;
    if request.ids.len() > LIST_LIMIT as usize {
        return Err(ApiError::invalid(
            "too-many",
            "Too many notices at once",
            format!("at most {LIST_LIMIT} per request"),
        ));
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    notices::mark_read(&mut tx, &request.ids).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn publish(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Json(mut notice): Json<NewNotice>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    user.require(Permission::NoticesPublish)?;
    notice.title = notice.title.trim().to_owned();
    notice.body = notice.body.trim().to_owned();
    if notice.title.is_empty() || notice.title.chars().count() > 200 {
        return Err(ApiError::invalid(
            "invalid-title",
            "Title must be 1–200 characters",
            "title",
        ));
    }
    if notice.body.is_empty() || notice.body.chars().count() > 10_000 {
        return Err(ApiError::invalid(
            "invalid-body",
            "Message must be 1–10,000 characters",
            "body",
        ));
    }

    notice.section = notice
        .section
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_uppercase);
    if notice
        .section
        .as_deref()
        .is_some_and(|s| !(s.len() == 1 && s.chars().all(|c| c.is_ascii_uppercase())))
    {
        return Err(ApiError::invalid(
            "invalid-section",
            "A section is one letter",
            "section",
        ));
    }
    if notice
        .batch_year
        .is_some_and(|y| !(2000..=2100).contains(&y))
    {
        return Err(ApiError::invalid(
            "invalid-batch",
            "That batch isn't valid",
            "batch_year",
        ));
    }
    notice.recipient_ids.sort();
    notice.recipient_ids.dedup();
    if notice.recipient_ids.len() > 500 {
        return Err(ApiError::invalid(
            "too-many-recipients",
            "Up to 500 named recipients",
            "recipient_ids",
        ));
    }
    let now = Utc::now();
    if let Some(at) = notice.publish_at
        && (at < now - Duration::minutes(5) || at > now + Duration::days(365))
    {
        return Err(ApiError::invalid(
            "invalid-schedule",
            "Schedule it within the next year",
            "publish_at",
        ));
    }
    let starts = notice.publish_at.unwrap_or(now).max(now);
    if notice.expires_at.is_some_and(|e| e <= starts) {
        return Err(ApiError::invalid(
            "invalid-expiry",
            "It must expire after it's published",
            "expires_at",
        ));
    }

    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    if let Some(program_id) = notice.program_id {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM org.programs WHERE id = $1)")
                .bind(program_id)
                .fetch_one(&mut *tx)
                .await?;
        if !exists {
            return Err(ApiError::invalid(
                "unknown-program",
                "No such program",
                program_id.to_string(),
            ));
        }
    }
    if !notice.recipient_ids.is_empty() {
        let known: i64 =
            sqlx::query_scalar("SELECT count(*) FROM identity.users WHERE id = ANY($1) AND active")
                .bind(&notice.recipient_ids)
                .fetch_one(&mut *tx)
                .await?;
        if known != notice.recipient_ids.len() as i64 {
            return Err(ApiError::invalid(
                "unknown-recipient",
                "Some recipients aren't active users",
                "recipient_ids",
            ));
        }
    }
    let id = notices::publish(&mut tx, &notice).await?;
    // Live now: fan out at once. Scheduled: the release job does it when its time comes.
    let scheduled = notice.publish_at.is_some_and(|at| at > now);
    if !scheduled {
        outbox::enqueue(
            &mut tx,
            "notice.published",
            Some(&id.to_string()),
            &json!({ "notice_id": id }),
        )
        .await?;
    }
    let mut event = client.event("notice.published", "notice", Some(id.to_string()));
    event.metadata = json!({
        "title": notice.title,
        "severity": notice.severity.as_str(),
        "program_id": notice.program_id,
        "batch_year": notice.batch_year,
        "section": notice.section,
        "recipients": notice.recipient_ids.len(),
        "publish_at": notice.publish_at,
    });
    audit::record(&mut tx, &event).await?;
    tx.commit().await?;
    if !scheduled {
        state.outbox_wakeup.notify_one();
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": id, "scheduled": scheduled })),
    ))
}

/// Corrects a notice's title, message, severity or expiry. Its audience
/// can't change: that's a different notice. The audit entry keeps the text
/// it replaced; a critical notice's readers acknowledge the new text.
pub async fn edit(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
    Json(mut edit): Json<notices::NoticeEdit>,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::NoticesPublish)?;
    edit.title = edit.title.trim().to_owned();
    edit.body = edit.body.trim().to_owned();
    if edit.title.is_empty() || edit.title.chars().count() > 200 {
        return Err(ApiError::invalid(
            "invalid-title",
            "Title must be 1–200 characters",
            "title",
        ));
    }
    if edit.body.is_empty() || edit.body.chars().count() > 10_000 {
        return Err(ApiError::invalid(
            "invalid-body",
            "Message must be 1–10,000 characters",
            "body",
        ));
    }
    if edit.expires_at.is_some_and(|e| e <= Utc::now()) {
        return Err(ApiError::invalid(
            "invalid-expiry",
            "The expiry must be in the future",
            "expires_at",
        ));
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let before = notices::edit(&mut tx, id, &edit)
        .await
        .map_err(|err| match &err {
            sqlx::Error::Database(db) if db.is_check_violation() => ApiError::invalid(
                "invalid-expiry",
                "It must expire after it's published",
                "expires_at",
            ),
            _ => err.into(),
        })?
        .ok_or_else(|| ApiError::not_found("notice"))?;
    let mut event = client.event("notice.edited", "notice", Some(id.to_string()));
    event.metadata =
        json!({ "before": before, "title": edit.title, "severity": edit.severity.as_str() });
    audit::record(&mut tx, &event).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Withdrawal {
    reason: String,
}

/// Withdraws a published notice: it ends now, for its audience and the
/// public board. Publishers still see it, with the reason.
pub async fn withdraw(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
    Json(body): Json<Withdrawal>,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::NoticesPublish)?;
    let reason = body.reason.trim();
    if !(3..=500).contains(&reason.chars().count()) {
        return Err(ApiError::invalid(
            "reason-required",
            "Say why it's being withdrawn",
            "3 to 500 characters",
        ));
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    if !notices::withdraw(&mut tx, id, reason).await? {
        return Err(ApiError::conflict(
            "not-withdrawable",
            "Only a published notice that isn't already withdrawn can be withdrawn",
        ));
    }
    let mut event = client.event("notice.withdrawn", "notice", Some(id.to_string()));
    event.metadata = json!({ "reason": reason });
    audit::record(&mut tx, &event).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Deletes a notice that's still scheduled: nobody has seen it.
pub async fn delete_scheduled(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::NoticesPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    if !notices::delete_scheduled(&mut tx, id).await? {
        return Err(ApiError::conflict(
            "already-published",
            "Only a scheduled notice can be deleted; withdraw a published one",
        ));
    }
    audit::record(
        &mut tx,
        &client.event("notice.deleted", "notice", Some(id.to_string())),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Critical notices waiting for the caller's acknowledgement.
pub async fn awaiting_acknowledgement(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<Notice>>, ApiError> {
    user.require(Permission::NoticesRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    Ok(Json(notices::awaiting_acknowledgement(&mut tx).await?))
}

pub async fn acknowledge(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::NoticesRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    if !notices::acknowledge(&mut tx, id).await? {
        return Err(ApiError::not_found("notice"));
    }
    audit::record(
        &mut tx,
        &client.event("notice.acknowledged", "notice", Some(id.to_string())),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct ReceiptView {
    #[serde(flatten)]
    totals: Receipts,
    readers: Vec<Reader>,
}

/// For publishers: reach, reads, acknowledgements, emails.
pub async fn receipts(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ReceiptView>, ApiError> {
    user.require(Permission::NoticesPublish)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    notices::get(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::not_found("notice"))?;
    Ok(Json(ReceiptView {
        totals: notices::receipts(&mut tx, id).await?,
        readers: notices::readers(&mut tx, id, 300).await?,
    }))
}

/// Multipart `file` on a notice: PDF, PNG or JPEG, up to 5 MB.
pub async fn attach(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Path(id): Path<Uuid>,
    mut form: Multipart,
) -> Result<StatusCode, ApiError> {
    user.require(Permission::NoticesPublish)?;
    let bad = |detail: &str| {
        ApiError::invalid(
            "invalid-upload",
            "That upload isn't valid",
            detail.to_owned(),
        )
    };
    let mut file: Option<(Vec<u8>, String)> = None;
    while let Some(field) = form.next_field().await.map_err(|_| bad("malformed form"))? {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or("attachment").to_owned();
            let bytes = field
                .bytes()
                .await
                .map_err(|_| bad("the file is larger than 5 MB"))?;
            file = Some((bytes.to_vec(), name));
        }
    }
    let (bytes, name) = file.ok_or_else(|| bad("attach a file"))?;
    if bytes.is_empty() || bytes.len() > MAX_UPLOAD_BYTES {
        return Err(bad("the file must be under 5 MB"));
    }
    let (content_type, _) = sniff(&bytes).ok_or_else(|| bad("upload a PDF, PNG or JPEG"))?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    notices::get(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::not_found("notice"))?;
    tx.commit().await?;
    let attachment = Uuid::new_v4();
    let object = format!("notices/{id}/{attachment}");
    let size = bytes.len() as i32;
    state
        .files
        .storage
        .put(&state.files.uploads_bucket, &object, content_type, bytes)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "storing a notice attachment failed");
            ApiError::upstream(
                "storage-unavailable",
                "The file couldn't be stored. Please try again.",
            )
        })?;
    let display: String = name.chars().filter(|c| !c.is_control()).take(120).collect();
    let display = if display.is_empty() {
        "attachment".to_owned()
    } else {
        display
    };
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    notices::add_attachment(
        &mut tx,
        attachment,
        id,
        &object,
        &display,
        content_type,
        size,
    )
    .await?;
    audit::record(
        &mut tx,
        &client.event("notice.attached", "notice", Some(id.to_string())),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::CREATED)
}

pub async fn attachment(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, attachment)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    user.require(Permission::NoticesRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let (object, name, content_type) = notices::attachment_object(&mut tx, id, attachment)
        .await?
        .ok_or_else(|| ApiError::not_found("attachment"))?;
    tx.commit().await?;
    let bytes = state
        .files
        .storage
        .get(&state.files.uploads_bucket, &object)
        .await
        .map_err(|err| {
            tracing::error!(error = %err, "reading a notice attachment failed");
            ApiError::upstream(
                "storage-unavailable",
                "The file couldn't be fetched. Please try again.",
            )
        })?;
    let content_type = match content_type.as_str() {
        "application/pdf" => "application/pdf",
        "image/png" => "image/png",
        _ => "image/jpeg",
    };
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || ".-_ ".contains(*c))
        .collect();
    if let Ok(value) = HeaderValue::from_str(&format!("inline; filename=\"{safe}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

#[derive(Serialize, Deserialize)]
pub struct Preferences {
    /// all | important | critical
    email_level: String,
}

pub async fn preferences(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Preferences>, ApiError> {
    user.require(Permission::NoticesRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    Ok(Json(Preferences {
        email_level: notices::email_level(&mut tx).await?,
    }))
}

pub async fn set_preferences(
    State(state): State<AppState>,
    user: AuthUser,
    Json(prefs): Json<Preferences>,
) -> Result<Json<Preferences>, ApiError> {
    user.require(Permission::NoticesRead)?;
    if !matches!(prefs.email_level.as_str(), "all" | "important" | "critical") {
        return Err(ApiError::invalid(
            "invalid-preference",
            "Choose all, important or critical",
            "email_level",
        ));
    }
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    notices::set_email_level(&mut tx, &prefs.email_level).await?;
    tx.commit().await?;
    Ok(Json(prefs))
}

/// Server-sent events for the bell: on connect the unread count, then each
/// notice that goes live for this user (from Postgres NOTIFY, via every API
/// instance's listener), with the new count.
pub async fn stream(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError> {
    user.require(Permission::NoticesRead)?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let unread = notices::unread_count(&mut tx).await?;
    tx.commit().await?;
    let hello = futures_util::stream::once(async move {
        Ok(Event::default()
            .event("unread")
            .data(json!({ "unread": unread }).to_string()))
    });
    let live = tokio_stream::wrappers::BroadcastStream::new(state.notices_live.subscribe())
        .filter_map(move |id| {
            let state = state.clone();
            let user = user;
            async move {
                let id = id.ok()?;
                let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await.ok()?;
                // Only notices addressed to this user (publishers see all, but aren't notified of all).
                let notice = notices::get(&mut tx, id).await.ok()??;
                let addressed: bool = sqlx::query_scalar("SELECT comms.is_for(identity.current_user_id(), n) FROM comms.notices n WHERE n.id = $1")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await
                    .ok()?;
                if !addressed {
                    return None;
                }
                let unread = notices::unread_count(&mut tx).await.ok()?;
                let data = json!({ "id": notice.id, "title": notice.title, "severity": notice.severity, "unread": unread });
                Some(Ok(Event::default().event("notice").data(data.to_string())))
            }
        });
    Ok(Sse::new(hello.chain(live)).keep_alive(KeepAlive::default()))
}

/// What a file really is, from its first bytes, whatever its name says.
pub fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"%PDF-") {
        Some(("application/pdf", "pdf"))
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("image/jpeg", "jpg"))
    } else {
        None
    }
}
