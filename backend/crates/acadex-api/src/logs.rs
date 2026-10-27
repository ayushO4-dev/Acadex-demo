//! The Logs page (administration only): the audit trail, newest first,
//! filtered, and as CSV.

use std::fmt::Write as _;

use acadex_auth::Permission;
use acadex_db::{
    audit::{self, Filter, LogEntry},
    rls,
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use chrono::{NaiveDate, Utc};
use chrono_tz::Asia::Kolkata;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    AppState,
    auth::{AuthUser, ClientInfo},
    error::ApiError,
};

const PAGE_SIZE: i64 = 50;
/// An export is for an auditor, not a backup: past this, narrow the dates.
const EXPORT_LIMIT: i64 = 10_000;

#[derive(Deserialize)]
pub struct LogQuery {
    before: Option<i64>,
    /// `YYYY-MM-DD`, inclusive, Indian time.
    from: Option<String>,
    to: Option<String>,
    category: Option<String>,
    outcome: Option<String>,
    role: Option<String>,
    who: Option<String>,
}

impl LogQuery {
    fn filter(&self) -> Result<Filter, ApiError> {
        let date = |value: &Option<String>, name: &str| {
            value
                .as_deref()
                .filter(|v| !v.is_empty())
                .map(|v| {
                    NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| {
                        ApiError::invalid(
                            "invalid-date",
                            "Invalid date",
                            format!("{name}: YYYY-MM-DD"),
                        )
                    })
                })
                .transpose()
        };
        let from = date(&self.from, "from")?;
        let to = date(&self.to, "to")?;
        if let (Some(from), Some(to)) = (from, to)
            && from > to
        {
            return Err(ApiError::invalid(
                "invalid-range",
                "The start date is after the end date",
                "from must not be after to",
            ));
        }
        let prefixes = match self
            .category
            .as_deref()
            .filter(|c| !c.is_empty() && *c != "all")
        {
            None => None,
            Some(category) => Some(audit::category_prefixes(category).ok_or_else(|| {
                ApiError::invalid("invalid-category", "Unknown category", category.to_owned())
            })?),
        };
        let outcome = match self.outcome.as_deref() {
            None | Some("" | "all") => None,
            Some("success") => Some("success"),
            Some("denied") => Some("denied"),
            Some("error") => Some("error"),
            Some(other) => {
                return Err(ApiError::invalid(
                    "invalid-outcome",
                    "Unknown outcome",
                    other.to_owned(),
                ));
            }
        };
        let text = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty() && *v != "all")
                .map(|v| v.chars().take(100).collect::<String>())
        };
        Ok(Filter {
            from,
            to,
            prefixes,
            outcome,
            role: text(&self.role),
            who: text(&self.who),
        })
    }
}

#[derive(Serialize)]
pub struct LogPage {
    entries: Vec<LogEntry>,
    /// Pass as `before` for the next (older) page; `None` on the last page.
    next_before: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<LogQuery>,
) -> Result<Json<LogPage>, ApiError> {
    // The log is administration's alone: everyone's actions, institution-wide.
    user.require(Permission::AuditReadAll)?;
    let filter = query.filter()?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let entries = audit::page(&mut tx, &filter, query.before, PAGE_SIZE).await?;
    let next_before = (entries.len() as i64 == PAGE_SIZE)
        .then(|| entries.last().map(|entry| entry.id))
        .flatten();
    Ok(Json(LogPage {
        entries,
        next_before,
    }))
}

/// `GET /audit/logs/export`: the same filters, as CSV, newest first. The
/// export is itself logged, with the filters used and how many rows left.
pub async fn export(
    State(state): State<AppState>,
    user: AuthUser,
    client: ClientInfo,
    Query(query): Query<LogQuery>,
) -> Result<Response, ApiError> {
    // The log is administration's alone: everyone's actions, institution-wide.
    user.require(Permission::AuditReadAll)?;
    let filter = query.filter()?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let entries = audit::page(&mut tx, &filter, None, EXPORT_LIMIT).await?;
    let mut event = client.event("audit.exported", "action_log", None);
    event.metadata = json!({
        "rows": entries.len(),
        "from": filter.from,
        "to": filter.to,
        "category": query.category,
        "outcome": filter.outcome,
        "role": filter.role,
        "who": filter.who,
    });
    audit::record(&mut tx, &event).await?;
    tx.commit().await?;

    let mut response = to_csv(&entries).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    let name = format!(
        "attachment; filename=\"acadex-logs-{}.csv\"",
        Utc::now().with_timezone(&Kolkata).format("%Y%m%d-%H%M")
    );
    if let Ok(value) = HeaderValue::from_str(&name) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

fn to_csv(entries: &[LogEntry]) -> String {
    let mut out = String::from(
        "time_ist,actor,actor_role,action,resource_type,resource_id,outcome,ip,user_agent,trace_id,metadata\r\n",
    );
    for e in entries {
        let time = e
            .occurred_at
            .with_timezone(&Kolkata)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let fields = [
            time.as_str(),
            e.actor_name.as_deref().unwrap_or(""),
            e.actor_role.as_deref().unwrap_or(""),
            &e.action,
            &e.resource_type,
            e.resource_id.as_deref().unwrap_or(""),
            &e.outcome,
            e.ip.as_deref().unwrap_or(""),
            e.user_agent.as_deref().unwrap_or(""),
            e.trace_id.as_deref().unwrap_or(""),
            &e.metadata.to_string(),
        ];
        for (i, field) in fields.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let _ = write!(out, "{}", csv_field(field));
        }
        out.push_str("\r\n");
    }
    out
}

/// Quotes a field when it needs it, and defuses spreadsheet formulas: a cell
/// starting with `=`, `+`, `-`, `@` (or a tab or CR) is prefixed with `'`, so
/// a user agent like `=HYPERLINK(...)` opens as text, not as a formula.
fn csv_field(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if value.contains([',', '"', '\n', '\r']) || value.starts_with('\'') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_are_quoted_and_formulas_defused() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("-1"), "\"'-1\"");
    }
}
