//! Support tickets (architecture.md §9.7): raised by anyone, worked by
//! support agents, with per-priority SLA targets the `support.sla` job
//! enforces by escalating.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Ticket {
    pub id: Uuid,
    pub number: i64,
    pub subject: String,
    pub category: String,
    pub priority: String,
    pub status: String,
    pub requester_id: Uuid,
    pub requester_name: String,
    pub requester_role: String,
    pub requester_roll_no: Option<String>,
    pub assignee_id: Option<Uuid>,
    pub assignee_name: Option<String>,
    pub linked_resource: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub first_response_due: DateTime<Utc>,
    pub resolve_due: DateTime<Utc>,
    pub first_responded_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub escalation_level: i16,
    pub last_message_at: Option<DateTime<Utc>>,
    pub messages: i64,
    /// A message the caller hasn't seen since they last opened it.
    pub unread: bool,
}

macro_rules! ticket_sql {
    () => {
        "SELECT t.id, t.number, t.subject, t.category, t.priority, t.status,
                t.requester_id, r.display_name AS requester_name, r.role::text AS requester_role,
                st.roll_no AS requester_roll_no,
                t.assignee_id, a.display_name AS assignee_name, t.linked_resource,
                t.created_at, t.updated_at, t.first_response_due, t.resolve_due,
                t.first_responded_at, t.resolved_at, t.escalation_level,
                last.at AS last_message_at, coalesce(last.n, 0) AS messages,
                coalesce(last.at > coalesce(rd.last_read_at, '-infinity'), false) AS unread
         FROM support.tickets t
         JOIN identity.users r ON r.id = t.requester_id
         LEFT JOIN people.students st ON st.user_id = t.requester_id
         LEFT JOIN identity.users a ON a.id = t.assignee_id
         LEFT JOIN support.reads rd ON rd.ticket_id = t.id AND rd.user_id = identity.current_user_id()
         LEFT JOIN LATERAL (
             SELECT max(m.created_at) FILTER (WHERE m.author_id IS DISTINCT FROM identity.current_user_id()) AS at,
                    count(*) AS n
             FROM support.messages m WHERE m.ticket_id = t.id
         ) last ON true"
    };
}

#[derive(Debug, Default, Deserialize)]
pub struct Filter {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub category: Option<String>,
    /// "me", "unassigned", or none for any.
    pub assignee: Option<String>,
    /// Words in the subject, or a ticket number.
    pub q: Option<String>,
}

/// Tickets the caller may see (RLS: their own, or every one for agents);
/// open ones first, most urgent deadline first, then the latest closed.
pub async fn list(
    conn: &mut PgConnection,
    filter: &Filter,
    limit: i64,
) -> Result<Vec<Ticket>, sqlx::Error> {
    let q = filter.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let number: Option<i64> = q.and_then(|q| q.trim_start_matches('#').parse().ok());
    sqlx::query_as(concat!(
        ticket_sql!(),
        " WHERE ($1::text IS NULL OR t.status = $1 OR ($1 = 'active' AND t.status NOT IN ('resolved', 'closed')))
            AND ($2::text IS NULL OR t.priority = $2)
            AND ($3::text IS NULL OR t.category = $3)
            AND ($4::text IS NULL OR ($4 = 'me' AND t.assignee_id = identity.current_user_id())
                                  OR ($4 = 'unassigned' AND t.assignee_id IS NULL))
            AND ($5::text IS NULL OR t.subject ILIKE '%' || $5 || '%' OR t.number = $6)
          ORDER BY t.status IN ('resolved', 'closed'), t.resolve_due, t.updated_at DESC
          LIMIT $7"
    ))
    .bind(filter.status.as_deref())
    .bind(filter.priority.as_deref())
    .bind(filter.category.as_deref())
    .bind(filter.assignee.as_deref())
    .bind(q)
    .bind(number)
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn get(conn: &mut PgConnection, id: Uuid) -> Result<Option<Ticket>, sqlx::Error> {
    sqlx::query_as(concat!(ticket_sql!(), " WHERE t.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Message {
    pub id: i64,
    pub author_id: Option<Uuid>,
    /// None for the system (escalations).
    pub author_name: Option<String>,
    pub author_role: Option<String>,
    pub body: String,
    pub internal: bool,
    pub created_at: DateTime<Utc>,
}

/// The conversation (RLS hides internal notes from requesters).
pub async fn messages(conn: &mut PgConnection, ticket: Uuid) -> Result<Vec<Message>, sqlx::Error> {
    sqlx::query_as(
        "SELECT m.id, m.author_id, u.display_name AS author_name, u.role::text AS author_role,
                m.body, m.internal, m.created_at
         FROM support.messages m LEFT JOIN identity.users u ON u.id = m.author_id
         WHERE m.ticket_id = $1 ORDER BY m.id",
    )
    .bind(ticket)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Attachment {
    pub id: Uuid,
    pub message_id: Option<i64>,
    pub name: String,
    pub content_type: String,
    pub size_bytes: i32,
    pub uploaded_at: DateTime<Utc>,
}

pub async fn attachments(
    conn: &mut PgConnection,
    ticket: Uuid,
) -> Result<Vec<Attachment>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, message_id, name, content_type, size_bytes, uploaded_at FROM support.attachments
         WHERE ticket_id = $1 ORDER BY uploaded_at",
    )
    .bind(ticket)
    .fetch_all(conn)
    .await
}

pub struct NewTicket<'a> {
    pub subject: &'a str,
    pub category: &'a str,
    pub priority: &'a str,
    pub linked_resource: Option<&'a str>,
    pub body: &'a str,
}

/// Raises a ticket as the acting user, with its first message.
pub async fn create(
    conn: &mut PgConnection,
    t: &NewTicket<'_>,
) -> Result<(Uuid, i64), sqlx::Error> {
    let (id, number): (Uuid, i64) = sqlx::query_as(
        "INSERT INTO support.tickets (requester_id, subject, category, priority, linked_resource,
                                      first_response_due, resolve_due)
         VALUES (identity.current_user_id(), $1, $2, $3, $4, now(), now())
         RETURNING id, number",
    )
    .bind(t.subject)
    .bind(t.category)
    .bind(t.priority)
    .bind(t.linked_resource)
    .fetch_one(&mut *conn)
    .await?;
    add_message(conn, id, t.body, false).await?;
    mark_read(conn, id).await?;
    Ok((id, number))
}

/// A message as the acting user; returns its id.
pub async fn add_message(
    conn: &mut PgConnection,
    ticket: Uuid,
    body: &str,
    internal: bool,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO support.messages (ticket_id, author_id, body, internal)
         VALUES ($1, identity.current_user_id(), $2, $3) RETURNING id",
    )
    .bind(ticket)
    .bind(body)
    .bind(internal)
    .fetch_one(conn)
    .await
}

/// The first public agent reply stops the first-response clock.
pub async fn record_first_response(
    conn: &mut PgConnection,
    ticket: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE support.tickets SET first_responded_at = now() WHERE id = $1 AND first_responded_at IS NULL")
        .bind(ticket)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn set_status(
    conn: &mut PgConnection,
    ticket: Uuid,
    status: &str,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("UPDATE support.tickets SET status = $2 WHERE id = $1 AND status <> $2")
        .bind(ticket)
        .bind(status)
        .execute(conn)
        .await?;
    Ok(done.rows_affected() == 1)
}

#[derive(Debug, Deserialize)]
pub struct Change {
    pub status: Option<String>,
    pub priority: Option<String>,
    /// `Some(None)` unassigns.
    #[serde(default, with = "serde_with_double_option")]
    pub assignee_id: Option<Option<Uuid>>,
}

/// Distinguishes a missing `assignee_id` from an explicit null.
mod serde_with_double_option {
    use serde::{Deserialize, Deserializer};
    use uuid::Uuid;

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<Uuid>>, D::Error> {
        Option::<Uuid>::deserialize(d).map(Some)
    }
}

/// An agent's change to status, priority or assignee.
pub async fn update(
    conn: &mut PgConnection,
    ticket: Uuid,
    c: &Change,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE support.tickets SET
             status = coalesce($2, status),
             priority = coalesce($3, priority),
             assignee_id = CASE WHEN $4 THEN $5 ELSE assignee_id END
         WHERE id = $1",
    )
    .bind(ticket)
    .bind(c.status.as_deref())
    .bind(c.priority.as_deref())
    .bind(c.assignee_id.is_some())
    .bind(c.assignee_id.flatten())
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

pub async fn mark_read(conn: &mut PgConnection, ticket: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO support.reads (ticket_id, user_id) VALUES ($1, identity.current_user_id())
         ON CONFLICT (ticket_id, user_id) DO UPDATE SET last_read_at = now()",
    )
    .bind(ticket)
    .execute(conn)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)] // one column each, as the INSERT has them
pub async fn add_attachment(
    conn: &mut PgConnection,
    id: Uuid,
    ticket: Uuid,
    message: Option<i64>,
    object: &str,
    name: &str,
    content_type: &str,
    size: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO support.attachments (id, ticket_id, message_id, uploader_id, object, name, content_type, size_bytes)
         VALUES ($1, $2, $3, identity.current_user_id(), $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(ticket)
    .bind(message)
    .bind(object)
    .bind(name)
    .bind(content_type)
    .bind(size)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn attachment_object(
    conn: &mut PgConnection,
    ticket: Uuid,
    attachment: Uuid,
) -> Result<Option<(String, String, String)>, sqlx::Error> {
    sqlx::query_as("SELECT object, name, content_type FROM support.attachments WHERE ticket_id = $1 AND id = $2")
        .bind(ticket)
        .bind(attachment)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Agent {
    pub id: Uuid,
    pub display_name: String,
    /// Tickets they hold that aren't resolved or closed.
    pub open_tickets: i64,
}

/// Who tickets can be assigned to: active support agents, least loaded first.
pub async fn agents(conn: &mut PgConnection) -> Result<Vec<Agent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.display_name,
                (SELECT count(*) FROM support.tickets t WHERE t.assignee_id = u.id
                   AND t.status NOT IN ('resolved', 'closed')) AS open_tickets
         FROM identity.users u WHERE u.role = 'support_agent' AND u.active
         ORDER BY open_tickets, u.display_name",
    )
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Escalation {
    pub id: Uuid,
    pub number: i64,
    pub level: i16,
    pub priority: String,
    pub assignee_id: Option<Uuid>,
}

/// The `support.sla` job, as the worker: escalates tickets that missed their
/// first response (level 1) or resolution (level 2) target. An escalation
/// raises the priority a step (to urgent at level 2), assigns the least-loaded
/// agent if nobody has it, and notes why in the conversation. Each level
/// happens once per ticket, so running it again changes nothing.
pub async fn escalate_overdue(conn: &mut PgConnection) -> Result<Vec<Escalation>, sqlx::Error> {
    let escalated: Vec<Escalation> = sqlx::query_as(
        "WITH overdue AS (
             SELECT t.id,
                    CASE WHEN now() > t.resolve_due THEN 2 ELSE 1 END AS level
             FROM support.tickets t
             WHERE t.status NOT IN ('resolved', 'closed')
               AND ((t.escalation_level < 2 AND now() > t.resolve_due)
                    OR (t.escalation_level < 1 AND t.first_responded_at IS NULL AND now() > t.first_response_due))
             FOR UPDATE OF t SKIP LOCKED
         ),
         least_loaded AS (
             SELECT u.id FROM identity.users u
             WHERE u.role = 'support_agent' AND u.active
             ORDER BY (SELECT count(*) FROM support.tickets x WHERE x.assignee_id = u.id
                         AND x.status NOT IN ('resolved', 'closed')), u.display_name
             LIMIT 1
         )
         UPDATE support.tickets t SET
             escalation_level = o.level,
             escalated_at = now(),
             priority = CASE WHEN o.level = 2 THEN 'urgent'
                             ELSE (ARRAY['normal', 'high', 'urgent', 'urgent'])[array_position(ARRAY['low', 'normal', 'high', 'urgent'], t.priority)] END,
             assignee_id = coalesce(t.assignee_id, (SELECT id FROM least_loaded))
         FROM overdue o
         WHERE t.id = o.id
         RETURNING t.id, t.number, t.escalation_level AS level, t.priority, t.assignee_id",
    )
    .fetch_all(&mut *conn)
    .await?;
    for e in &escalated {
        let why = if e.level == 2 {
            "Escalated: not resolved within the target time. Priority raised to urgent."
        } else {
            "Escalated: no first response within the target time. Priority raised."
        };
        sqlx::query("INSERT INTO support.messages (ticket_id, author_id, body, internal) VALUES ($1, NULL, $2, true)")
            .bind(e.id)
            .bind(why)
            .execute(&mut *conn)
            .await?;
    }
    Ok(escalated)
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct SlaTarget {
    pub priority: String,
    pub first_response_minutes: i32,
    pub resolve_minutes: i32,
}

pub async fn sla_targets(conn: &mut PgConnection) -> Result<Vec<SlaTarget>, sqlx::Error> {
    sqlx::query_as(
        "SELECT priority, first_response_minutes, resolve_minutes FROM support.sla_policy
         ORDER BY array_position(ARRAY['urgent', 'high', 'normal', 'low'], priority)",
    )
    .fetch_all(conn)
    .await
}

/// Sets one priority's targets; false if row-level security refused (not an administrator).
pub async fn set_sla_target(
    conn: &mut PgConnection,
    target: &SlaTarget,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE support.sla_policy SET first_response_minutes = $2, resolve_minutes = $3 WHERE priority = $1",
    )
    .bind(&target.priority)
    .bind(target.first_response_minutes)
    .bind(target.resolve_minutes)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() == 1)
}
