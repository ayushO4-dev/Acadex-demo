//! Notices. Which ones a caller sees is RLS's job (`comms.is_for`): current
//! institution-wide notices for everyone; targeted ones for their program,
//! batch, section or named recipients; all of them for publishers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    #[default]
    General,
    Important,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Important => "important",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PublicNotice {
    pub id: Uuid,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub published_at: DateTime<Utc>,
}

/// The public notice board: current institution-wide notices. Needs no acting
/// user, so it runs outside an RLS user transaction.
#[tracing::instrument(name = "db.public_notices", skip_all, fields(otel.kind = "client"))]
pub async fn public(conn: &mut PgConnection, limit: i64) -> Result<Vec<PublicNotice>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, severity::text AS severity, title, body, published_at
         FROM comms.notices
         WHERE program_id IS NULL AND withdrawn_at IS NULL AND published_at <= now()
           AND (expires_at IS NULL OR expires_at > now())
         ORDER BY published_at DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct NoticeAttachment {
    pub id: Uuid,
    pub name: String,
    pub content_type: String,
    pub size_bytes: i32,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Notice {
    pub id: Uuid,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub published_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    /// `None` for institution-wide notices.
    pub program_name: Option<String>,
    pub batch_year: Option<i16>,
    pub section: Option<String>,
    /// Addressed to named people.
    pub individual: bool,
    pub read: bool,
    /// Critical notices are acknowledged, not just read.
    pub acknowledged: bool,
    pub attachments: sqlx::types::Json<Vec<NoticeAttachment>>,
    pub author: Option<String>,
    /// Corrected after publishing.
    pub edited_at: Option<DateTime<Utc>>,
    /// Withdrawn: only publishers still see it.
    pub withdrawn_at: Option<DateTime<Utc>>,
    pub withdraw_reason: Option<String>,
}

macro_rules! notice_sql {
    () => {
        "SELECT n.id, n.severity::text AS severity, n.title, n.body, n.published_at, n.expires_at,
                p.name AS program_name, n.batch_year, n.section, n.individual,
                r.notice_id IS NOT NULL AS read, r.acknowledged_at IS NOT NULL AS acknowledged,
                coalesce((SELECT jsonb_agg(jsonb_build_object('id', a.id, 'name', a.name, 'content_type', a.content_type,
                                                              'size_bytes', a.size_bytes) ORDER BY a.uploaded_at)
                          FROM comms.notice_attachments a WHERE a.notice_id = n.id), '[]'::jsonb) AS attachments,
                au.display_name AS author, n.edited_at, n.withdrawn_at, n.withdraw_reason
         FROM comms.notices n
         LEFT JOIN org.programs p ON p.id = n.program_id
         LEFT JOIN identity.users au ON au.id = n.created_by
         LEFT JOIN comms.notice_reads r
                ON r.notice_id = n.id AND r.user_id = identity.current_user_id()"
    };
}

/// What the acting user may see, newest first, with their read state;
/// `before` continues from the last notice of the previous page (keyset).
#[tracing::instrument(name = "db.notices", skip_all, fields(otel.kind = "client"))]
pub async fn visible(
    conn: &mut PgConnection,
    limit: i64,
    before: Option<(DateTime<Utc>, Uuid)>,
) -> Result<Vec<Notice>, sqlx::Error> {
    sqlx::query_as(concat!(
        notice_sql!(),
        " WHERE ($2::timestamptz IS NULL OR (n.published_at, n.id) < ($2, $3))
          ORDER BY n.published_at DESC, n.id DESC LIMIT $1"
    ))
    .bind(limit)
    .bind(before.map(|(at, _)| at))
    .bind(before.map(|(_, id)| id))
    .fetch_all(conn)
    .await
}

pub async fn get(conn: &mut PgConnection, id: Uuid) -> Result<Option<Notice>, sqlx::Error> {
    sqlx::query_as(concat!(notice_sql!(), " WHERE n.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Current critical notices addressed to the acting user that they haven't
/// acknowledged: shown before anything else until they do. Only the
/// audience's, not every notice a publisher can see.
pub async fn awaiting_acknowledgement(conn: &mut PgConnection) -> Result<Vec<Notice>, sqlx::Error> {
    sqlx::query_as(concat!(
        notice_sql!(),
        " WHERE n.severity = 'critical' AND n.withdrawn_at IS NULL AND n.published_at <= now()
            AND (n.expires_at IS NULL OR n.expires_at > now())
            AND comms.is_for(identity.current_user_id(), n)
            AND r.acknowledged_at IS NULL
          ORDER BY n.published_at"
    ))
    .fetch_all(conn)
    .await
}

/// "I have read this": a read with an acknowledgement. `false` if the notice
/// isn't visible to the caller.
pub async fn acknowledge(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "INSERT INTO comms.notice_reads (user_id, notice_id, acknowledged_at)
         SELECT identity.current_user_id(), id, now() FROM comms.notices WHERE id = $1
         ON CONFLICT (user_id, notice_id) DO UPDATE SET acknowledged_at = coalesce(comms.notice_reads.acknowledged_at, now())",
    )
    .bind(id)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Current notices addressed to the acting user that they haven't read: the
/// bell's badge (publishers' view of every notice doesn't count).
pub async fn unread_count(conn: &mut PgConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM comms.notices n
         WHERE n.published_at <= now() AND (n.expires_at IS NULL OR n.expires_at > now())
           AND comms.is_for(identity.current_user_id(), n)
           AND NOT EXISTS (SELECT 1 FROM comms.notice_reads r
                           WHERE r.notice_id = n.id AND r.user_id = identity.current_user_id())",
    )
    .fetch_one(conn)
    .await
}

/// Marks notices read. Ids the user can't see are silently skipped: the
/// SELECT is filtered by RLS before anything is inserted.
pub async fn mark_read(conn: &mut PgConnection, ids: &[Uuid]) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO comms.notice_reads (user_id, notice_id)
         SELECT identity.current_user_id(), id FROM comms.notices WHERE id = ANY($1)
         ON CONFLICT DO NOTHING",
    )
    .bind(ids)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}

#[derive(Debug, Default, Deserialize)]
pub struct NewNotice {
    pub title: String,
    pub body: String,
    pub severity: Severity,
    /// `None` for the whole institution.
    pub program_id: Option<i16>,
    /// Students of one batch (admission year), and of one section.
    #[serde(default)]
    pub batch_year: Option<i16>,
    #[serde(default)]
    pub section: Option<String>,
    /// Named people instead (any role); overrides the rest.
    #[serde(default)]
    pub recipient_ids: Vec<Uuid>,
    /// Later than now: scheduled.
    #[serde(default)]
    pub publish_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// Publishes (or schedules) a notice as the acting user. Returns its id.
pub async fn publish(conn: &mut PgConnection, notice: &NewNotice) -> Result<Uuid, sqlx::Error> {
    let individual = !notice.recipient_ids.is_empty();
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO comms.notices (institution_id, program_id, batch_year, section, individual, severity,
                                    title, body, published_at, expires_at, created_by)
         SELECT u.institution_id, $1, $2, $3, $4, $5::comms.notice_severity, $6, $7,
                coalesce($8, now()), $9, u.id
         FROM identity.users u WHERE u.id = identity.current_user_id()
         RETURNING id",
    )
    .bind(if individual { None } else { notice.program_id })
    .bind(if individual { None } else { notice.batch_year })
    .bind(if individual { None } else { notice.section.as_deref() })
    .bind(individual)
    .bind(notice.severity.as_str())
    .bind(&notice.title)
    .bind(&notice.body)
    .bind(notice.publish_at)
    .bind(notice.expires_at)
    .fetch_one(&mut *conn)
    .await?;
    if individual {
        sqlx::query("INSERT INTO comms.notice_recipients (notice_id, user_id) SELECT $1, unnest($2::uuid[]) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(&notice.recipient_ids)
            .execute(conn)
            .await?;
    }
    Ok(id)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Receipts {
    /// Everyone the notice reaches.
    pub audience: i64,
    pub read: i64,
    pub acknowledged: i64,
    pub emailed: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Reader {
    pub display_name: String,
    pub role: String,
    pub roll_no: Option<String>,
    pub read_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

/// For publishers: how many it reaches, read and acknowledged.
pub async fn receipts(conn: &mut PgConnection, id: Uuid) -> Result<Receipts, sqlx::Error> {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM comms.audience($1)) AS audience,
                (SELECT count(*) FROM comms.notice_reads WHERE notice_id = $1) AS read,
                (SELECT count(*) FROM comms.notice_reads WHERE notice_id = $1 AND acknowledged_at IS NOT NULL) AS acknowledged,
                (SELECT count(*) FROM comms.notice_deliveries WHERE notice_id = $1) AS emailed",
    )
    .bind(id)
    .fetch_one(conn)
    .await
}

/// Everyone in the audience with their read and acknowledgement times, unread first.
pub async fn readers(
    conn: &mut PgConnection,
    id: Uuid,
    limit: i64,
) -> Result<Vec<Reader>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.display_name, u.role::text AS role, st.roll_no, r.read_at, r.acknowledged_at
         FROM comms.audience($1) a
         JOIN identity.users u ON u.id = a.user_id
         LEFT JOIN people.students st ON st.user_id = a.user_id
         LEFT JOIN comms.notice_reads r ON r.notice_id = $1 AND r.user_id = a.user_id
         ORDER BY r.read_at IS NOT NULL, r.read_at DESC, a.display_name
         LIMIT $2",
    )
    .bind(id)
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn add_attachment(
    conn: &mut PgConnection,
    id: Uuid,
    notice: Uuid,
    object: &str,
    name: &str,
    content_type: &str,
    size: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO comms.notice_attachments (id, notice_id, object, name, content_type, size_bytes)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(notice)
    .bind(object)
    .bind(name)
    .bind(content_type)
    .bind(size)
    .execute(conn)
    .await?;
    Ok(())
}

/// Where an attachment is stored, if its notice is visible to the caller.
pub async fn attachment_object(
    conn: &mut PgConnection,
    notice: Uuid,
    attachment: Uuid,
) -> Result<Option<(String, String, String)>, sqlx::Error> {
    sqlx::query_as("SELECT object, name, content_type FROM comms.notice_attachments WHERE notice_id = $1 AND id = $2")
        .bind(notice)
        .bind(attachment)
        .fetch_optional(conn)
        .await
}

/// all | important | critical: which notices the acting user gets by email.
pub async fn email_level(conn: &mut PgConnection) -> Result<String, sqlx::Error> {
    let level: Option<String> =
        sqlx::query_scalar("SELECT email_level FROM identity.notification_prefs WHERE user_id = identity.current_user_id()")
            .fetch_optional(conn)
            .await?;
    Ok(level.unwrap_or_else(|| "important".to_owned()))
}

pub async fn set_email_level(conn: &mut PgConnection, level: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO identity.notification_prefs (user_id, email_level) VALUES (identity.current_user_id(), $1)
         ON CONFLICT (user_id) DO UPDATE SET email_level = EXCLUDED.email_level, updated_at = now()",
    )
    .bind(level)
    .execute(conn)
    .await?;
    Ok(())
}

// ── Fan-out (the worker) ────────────────────────────────────────────────────

#[derive(Debug, sqlx::FromRow)]
pub struct Outgoing {
    pub id: Uuid,
    pub severity: String,
    pub title: String,
    pub body: String,
    pub author: Option<String>,
}

/// Live notices not yet fanned out: just published, or scheduled ones whose
/// time has come. Older than a day are skipped (no emails about stale news).
pub async fn due_for_fanout(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<Outgoing>, sqlx::Error> {
    sqlx::query_as(
        "SELECT n.id, n.severity::text AS severity, n.title, n.body, u.display_name AS author
         FROM comms.notices n LEFT JOIN identity.users u ON u.id = n.created_by
         WHERE n.fanned_out_at IS NULL AND n.published_at <= now() AND n.published_at > now() - interval '1 day'
           AND (n.expires_at IS NULL OR n.expires_at > now())
         ORDER BY n.published_at LIMIT $1",
    )
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn outgoing(conn: &mut PgConnection, id: Uuid) -> Result<Option<Outgoing>, sqlx::Error> {
    sqlx::query_as(
        "SELECT n.id, n.severity::text AS severity, n.title, n.body, u.display_name AS author
         FROM comms.notices n LEFT JOIN identity.users u ON u.id = n.created_by
         WHERE n.id = $1 AND n.published_at <= now() AND n.fanned_out_at IS NULL",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, sqlx::FromRow)]
pub struct Recipient {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: String,
    pub email_level: String,
}

/// Who should get an email: the audience, by their preference, not yet sent.
pub async fn email_recipients(
    conn: &mut PgConnection,
    id: Uuid,
    severity: &str,
) -> Result<Vec<Recipient>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.user_id, a.email, a.display_name, a.email_level FROM comms.audience($1) a
         WHERE ($2 = 'critical' OR a.email_level = 'all' OR ($2 = 'important' AND a.email_level = 'important'))
           AND NOT EXISTS (SELECT 1 FROM comms.notice_deliveries d
                           WHERE d.notice_id = $1 AND d.user_id = a.user_id AND d.channel = 'email')",
    )
    .bind(id)
    .bind(severity)
    .fetch_all(conn)
    .await
}

pub async fn record_delivery(
    conn: &mut PgConnection,
    id: Uuid,
    user: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO comms.notice_deliveries (notice_id, user_id, channel) VALUES ($1, $2, 'email') ON CONFLICT DO NOTHING")
        .bind(id)
        .bind(user)
        .execute(conn)
        .await?;
    Ok(())
}

/// Done: and tells listening browsers (scheduled notices weren't live when
/// they were inserted, so the insert trigger didn't).
pub async fn mark_fanned_out(conn: &mut PgConnection, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE comms.notices SET fanned_out_at = now() WHERE id = $1 AND fanned_out_at IS NULL",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("SELECT comms.announce($1)")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct NoticeEdit {
    pub title: String,
    pub body: String,
    pub severity: Severity,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// Corrects a notice; returns how it read before, or `None` if there's no
/// such (unwithdrawn) notice.
pub async fn edit(
    conn: &mut PgConnection,
    id: Uuid,
    edit: &NoticeEdit,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT comms.edit_notice($1, $2, $3, $4, $5)")
        .bind(id)
        .bind(&edit.title)
        .bind(&edit.body)
        .bind(edit.severity.as_str())
        .bind(edit.expires_at)
        .fetch_one(conn)
        .await
}

pub async fn withdraw(
    conn: &mut PgConnection,
    id: Uuid,
    reason: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT comms.withdraw_notice($1, $2)")
        .bind(id)
        .bind(reason)
        .fetch_one(conn)
        .await
}

pub async fn delete_scheduled(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT comms.delete_scheduled_notice($1)")
        .bind(id)
        .fetch_one(conn)
        .await
}
