use std::net::IpAddr;

use serde_json::Value;
use sqlx::PgConnection;

/// One audited action. The actor isn't a field: `audit.record` takes it from
/// the transaction's RLS settings, so callers can't attribute actions to anyone else.
#[derive(Debug)]
pub struct Event<'a> {
    pub action: &'a str,
    pub resource_type: &'a str,
    pub resource_id: Option<String>,
    pub outcome: Outcome,
    pub ip: Option<IpAddr>,
    pub user_agent: Option<&'a str>,
    pub trace_id: Option<String>,
    pub metadata: Value,
}

/// Where an action came from, for code that records events but isn't an
/// HTTP handler (the webhook's settlement, reused by reconciliation).
#[derive(Clone, Debug, Default)]
pub struct Origin {
    pub ip: Option<IpAddr>,
    pub user_agent: Option<String>,
    pub trace_id: Option<String>,
}

impl Origin {
    /// A successful event from here.
    pub fn event<'a>(
        &'a self,
        action: &'a str,
        resource_type: &'a str,
        resource_id: Option<String>,
    ) -> Event<'a> {
        Event {
            action,
            resource_type,
            resource_id,
            outcome: Outcome::Success,
            ip: self.ip,
            user_agent: self.user_agent.as_deref(),
            trace_id: self.trace_id.clone(),
            metadata: Value::Object(Default::default()),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Outcome {
    Success,
    Denied,
    Error,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Denied => "denied",
            Self::Error => "error",
        }
    }
}

/// Appends to the hash-chained log inside the caller's transaction, so the
/// entry commits or rolls back together with the action it describes.
#[tracing::instrument(name = "db.audit_record", skip_all, fields(otel.kind = "client", audit.action = event.action))]
pub async fn record(conn: &mut PgConnection, event: &Event<'_>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT audit.record($1, $2, $3, $4, $5::inet, $6, $7, $8::jsonb)")
        .bind(event.action)
        .bind(event.resource_type)
        .bind(event.resource_id.as_deref())
        .bind(event.outcome.as_str())
        .bind(event.ip.map(|ip| ip.to_string()))
        .bind(event.user_agent)
        .bind(event.trace_id.as_deref())
        .bind(event.metadata.to_string())
        .fetch_one(conn)
        .await
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct LogEntry {
    pub id: i64,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub actor_name: Option<String>,
    pub actor_role: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub outcome: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub trace_id: Option<String>,
    pub metadata: Value,
}

/// What the Logs page is narrowed to. Every field is optional.
#[derive(Debug, Default)]
pub struct Filter {
    /// First and last day, inclusive, in Indian time.
    pub from: Option<chrono::NaiveDate>,
    pub to: Option<chrono::NaiveDate>,
    /// Action prefixes (the part before the dot), from [`category_prefixes`].
    pub prefixes: Option<&'static [&'static str]>,
    pub outcome: Option<&'static str>,
    /// Actor's role (administration, looking across everyone).
    pub role: Option<String>,
    /// Part of the actor's name (administration).
    pub who: Option<String>,
}

/// The Logs page's categories, as the action prefixes each covers.
pub fn category_prefixes(category: &str) -> Option<&'static [&'static str]> {
    Some(match category {
        "auth" => &["auth", "identity"],
        "academic" => &["attendance", "calendar", "mooc"],
        "exams" => &["exams", "examform", "marks", "results", "revaluation"],
        "fees" => &["payment", "fees", "refund", "concession", "reconcile"],
        "activities" => &["activity"],
        "profile" => &["profile", "privacy"],
        "support" => &["support"],
        "notices" => &["notice"],
        "admin" => &[
            "user",
            "department",
            "settings",
            "superadmin",
            "audit",
            "ops",
        ],
        _ => return None,
    })
}

macro_rules! filtered_log {
    () => {
        "SELECT l.id, l.occurred_at, u.display_name AS actor_name, l.actor_role, l.action,
                l.resource_type, l.resource_id, l.outcome, host(l.ip) AS ip, l.user_agent,
                l.trace_id, l.metadata
         FROM audit.action_log l
         LEFT JOIN identity.users u ON u.id = l.actor_id
         WHERE ($1::bigint IS NULL OR l.id < $1)
           AND ($2::date IS NULL OR l.occurred_at >= $2::timestamp AT TIME ZONE 'Asia/Kolkata')
           AND ($3::date IS NULL OR l.occurred_at < ($3::date + 1)::timestamp AT TIME ZONE 'Asia/Kolkata')
           AND ($4::text[] IS NULL OR split_part(l.action, '.', 1) = ANY($4))
           AND ($5::text IS NULL OR l.outcome = $5)
           AND ($6::text IS NULL OR l.actor_role = $6)
           AND ($7::text IS NULL OR u.display_name ILIKE '%' || $7 || '%')
         ORDER BY l.id DESC
         LIMIT $8"
    };
}

/// Newest first, `limit` rows older than `before` (keyset pagination — no
/// OFFSET, rule R2). RLS decides whose rows: your own, or everyone's for
/// administration.
pub async fn page(
    conn: &mut PgConnection,
    filter: &Filter,
    before: Option<i64>,
    limit: i64,
) -> Result<Vec<LogEntry>, sqlx::Error> {
    let prefixes: Option<Vec<&str>> = filter.prefixes.map(<[&str]>::to_vec);
    sqlx::query_as(filtered_log!())
        .bind(before)
        .bind(filter.from)
        .bind(filter.to)
        .bind(prefixes)
        .bind(filter.outcome)
        .bind(filter.role.as_deref())
        .bind(filter.who.as_deref())
        .bind(limit)
        .fetch_all(conn)
        .await
}
