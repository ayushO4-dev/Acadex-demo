//! Activities (architecture.md §9.2): general, seminar, workshop and
//! industrial visit, one base table plus a table per type, through
//! draft → submitted → hod_review → approved | rejected.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

/// Each type's own fields; the `kind` tag picks the type.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Details {
    General {
        category: String,
        organiser: String,
        role: String,
        level: String,
        award: Option<String>,
    },
    Seminar {
        speaker: String,
        organisation: String,
        mode: String,
        duration_hours: f64,
    },
    Workshop {
        conducting_body: String,
        to_on: NaiveDate,
        hours: f64,
        #[serde(default)]
        skills: Vec<String>,
        #[serde(default)]
        certificate: bool,
    },
    IndustrialVisit {
        location: String,
        sector: String,
        accompanying_faculty: String,
        #[serde(default)]
        report: String,
    },
}

impl Details {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::General { .. } => "general",
            Self::Seminar { .. } => "seminar",
            Self::Workshop { .. } => "workshop",
            Self::IndustrialVisit { .. } => "industrial_visit",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct NewActivity {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub occurred_on: NaiveDate,
    #[serde(flatten)]
    pub details: Details,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Activity {
    pub id: Uuid,
    pub student_id: Uuid,
    pub student_name: String,
    pub roll_no: String,
    pub program_name: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub occurred_on: NaiveDate,
    pub status: String,
    pub points: Option<i16>,
    pub suggested_points: i16,
    pub review_note: Option<String>,
    pub attachments: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub decided_at: Option<DateTime<Utc>>,
    /// The type's own fields, as entered.
    pub details: sqlx::types::Json<Value>,
}

macro_rules! activity_sql {
    () => {
        "SELECT a.id, a.student_id, u.display_name AS student_name, st.roll_no, p.name AS program_name,
                a.kind, a.title, a.description, a.occurred_on, a.status, a.points,
                coalesce(activities.suggested_points(a.id), 0::smallint) AS suggested_points, a.review_note,
                (SELECT count(*) FROM activities.attachments x WHERE x.activity_id = a.id) AS attachments,
                a.created_at, a.updated_at, a.submitted_at, a.decided_at,
                coalesce(to_jsonb(g), to_jsonb(s), to_jsonb(w), to_jsonb(v), '{}'::jsonb) - 'activity_id' AS details
         FROM activities.activities a
         JOIN people.students st ON st.user_id = a.student_id
         JOIN identity.users u ON u.id = a.student_id
         JOIN org.programs p ON p.id = st.program_id
         LEFT JOIN activities.general g ON g.activity_id = a.id
         LEFT JOIN activities.seminars s ON s.activity_id = a.id
         LEFT JOIN activities.workshops w ON w.activity_id = a.id
         LEFT JOIN activities.industrial_visits v ON v.activity_id = a.id"
    };
}

#[derive(Debug, Default, Deserialize)]
pub struct Filter {
    pub kind: Option<String>,
    pub status: Option<String>,
    /// Only the caller's own (a student's list).
    #[serde(default)]
    pub mine: bool,
}

/// Activities the caller may see (RLS: a student's own; their program's for
/// faculty; their department's for an HOD), newest first.
pub async fn list(
    conn: &mut PgConnection,
    filter: &Filter,
    limit: i64,
) -> Result<Vec<Activity>, sqlx::Error> {
    sqlx::query_as(concat!(
        activity_sql!(),
        " WHERE ($1::text IS NULL OR a.kind = $1) AND ($2::text IS NULL OR a.status = $2)
            AND (NOT $3 OR a.student_id = identity.current_user_id())
          ORDER BY a.status IN ('submitted', 'hod_review') DESC, coalesce(a.submitted_at, a.updated_at) DESC
          LIMIT $4"
    ))
    .bind(filter.kind.as_deref())
    .bind(filter.status.as_deref())
    .bind(filter.mine)
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn get(conn: &mut PgConnection, id: Uuid) -> Result<Option<Activity>, sqlx::Error> {
    sqlx::query_as(concat!(activity_sql!(), " WHERE a.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

async fn insert_details(
    conn: &mut PgConnection,
    id: Uuid,
    details: &Details,
) -> Result<(), sqlx::Error> {
    match details {
        Details::General {
            category,
            organiser,
            role,
            level,
            award,
        } => {
            sqlx::query("INSERT INTO activities.general VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(id)
                .bind(category)
                .bind(organiser)
                .bind(role)
                .bind(level)
                .bind(award)
                .execute(conn)
                .await?;
        }
        Details::Seminar {
            speaker,
            organisation,
            mode,
            duration_hours,
        } => {
            sqlx::query("INSERT INTO activities.seminars VALUES ($1, $2, $3, $4, $5::numeric)")
                .bind(id)
                .bind(speaker)
                .bind(organisation)
                .bind(mode)
                .bind(duration_hours)
                .execute(conn)
                .await?;
        }
        Details::Workshop {
            conducting_body,
            to_on,
            hours,
            skills,
            certificate,
        } => {
            sqlx::query(
                "INSERT INTO activities.workshops VALUES ($1, $2, $3, $4::numeric, $5, $6)",
            )
            .bind(id)
            .bind(conducting_body)
            .bind(to_on)
            .bind(hours)
            .bind(skills)
            .bind(certificate)
            .execute(conn)
            .await?;
        }
        Details::IndustrialVisit {
            location,
            sector,
            accompanying_faculty,
            report,
        } => {
            sqlx::query("INSERT INTO activities.industrial_visits VALUES ($1, $2, $3, $4, $5)")
                .bind(id)
                .bind(location)
                .bind(sector)
                .bind(accompanying_faculty)
                .bind(report)
                .execute(conn)
                .await?;
        }
    }
    Ok(())
}

/// A new draft, as the acting student.
pub async fn create(conn: &mut PgConnection, new: &NewActivity) -> Result<Uuid, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO activities.activities (student_id, kind, title, description, occurred_on)
         VALUES (identity.current_user_id(), $1, $2, $3, $4) RETURNING id",
    )
    .bind(new.details.kind())
    .bind(&new.title)
    .bind(&new.description)
    .bind(new.occurred_on)
    .fetch_one(&mut *conn)
    .await?;
    insert_details(conn, id, &new.details).await?;
    Ok(id)
}

/// Rewrites a draft (RLS: the student's own, while a draft). `false` if
/// there's no such draft; the kind can't change.
pub async fn update(
    conn: &mut PgConnection,
    id: Uuid,
    new: &NewActivity,
) -> Result<bool, sqlx::Error> {
    let updated = sqlx::query(
        "UPDATE activities.activities SET title = $2, description = $3, occurred_on = $4
         WHERE id = $1 AND status = 'draft' AND kind = $5",
    )
    .bind(id)
    .bind(&new.title)
    .bind(&new.description)
    .bind(new.occurred_on)
    .bind(new.details.kind())
    .execute(&mut *conn)
    .await?;
    if updated.rows_affected() == 0 {
        return Ok(false);
    }
    let table = match new.details {
        Details::General { .. } => "DELETE FROM activities.general WHERE activity_id = $1",
        Details::Seminar { .. } => "DELETE FROM activities.seminars WHERE activity_id = $1",
        Details::Workshop { .. } => "DELETE FROM activities.workshops WHERE activity_id = $1",
        Details::IndustrialVisit { .. } => {
            "DELETE FROM activities.industrial_visits WHERE activity_id = $1"
        }
    };
    sqlx::query(table).bind(id).execute(&mut *conn).await?;
    insert_details(conn, id, &new.details).await?;
    Ok(true)
}

pub async fn delete_draft(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let deleted =
        sqlx::query("DELETE FROM activities.activities WHERE id = $1 AND status = 'draft'")
            .bind(id)
            .execute(conn)
            .await?;
    Ok(deleted.rows_affected() == 1)
}

/// Moves an activity, recording the move first (so it's recorded even when
/// the activity leaves the caller's sight, e.g. returned to a draft). The
/// trigger and RLS refuse moves the caller's role can't make.
pub async fn transition(
    conn: &mut PgConnection,
    id: Uuid,
    from: &str,
    to: &str,
    points: Option<i16>,
    note: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let recorded = sqlx::query(
        "INSERT INTO activities.reviews (activity_id, actor_id, from_status, to_status, points, note)
         SELECT id, identity.current_user_id(), $2, $3, $4, $5
         FROM activities.activities WHERE id = $1 AND status = $2",
    )
    .bind(id)
    .bind(from)
    .bind(to)
    .bind(points)
    .bind(note)
    .execute(&mut *conn)
    .await?;
    if recorded.rows_affected() == 0 {
        return Ok(false);
    }
    // Students keep their note field as the reviewer left it.
    let moved = sqlx::query(
        "UPDATE activities.activities
         SET status = $3, points = $4,
             review_note = CASE WHEN identity.current_app_role() = 'student' THEN review_note ELSE $5 END
         WHERE id = $1 AND status = $2",
    )
    .bind(id)
    .bind(from)
    .bind(to)
    .bind(points)
    .bind(note)
    .execute(conn)
    .await?;
    Ok(moved.rows_affected() == 1)
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Attachment {
    pub id: Uuid,
    pub name: String,
    pub content_type: String,
    pub size_bytes: i32,
    pub uploaded_at: DateTime<Utc>,
}

pub async fn attachments(
    conn: &mut PgConnection,
    activity: Uuid,
) -> Result<Vec<Attachment>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, content_type, size_bytes, uploaded_at FROM activities.attachments
         WHERE activity_id = $1 ORDER BY uploaded_at",
    )
    .bind(activity)
    .fetch_all(conn)
    .await
}

/// Records evidence stored at `object` (RLS: on the student's own draft).
pub async fn add_attachment(
    conn: &mut PgConnection,
    id: Uuid,
    activity: Uuid,
    object: &str,
    name: &str,
    content_type: &str,
    size: i32,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO activities.attachments (id, activity_id, object, name, content_type, size_bytes)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(id)
    .bind(activity)
    .bind(object)
    .bind(name)
    .bind(content_type)
    .bind(size)
    .fetch_one(conn)
    .await
}

/// Where an attachment is stored, if the caller may see it.
pub async fn attachment_object(
    conn: &mut PgConnection,
    activity: Uuid,
    attachment: Uuid,
) -> Result<Option<(String, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT object, name, content_type FROM activities.attachments WHERE id = $2 AND activity_id = $1",
    )
    .bind(activity)
    .bind(attachment)
    .fetch_optional(conn)
    .await
}

/// Removes an attachment from a draft; returns its object to delete.
pub async fn remove_attachment(
    conn: &mut PgConnection,
    activity: Uuid,
    attachment: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "DELETE FROM activities.attachments WHERE id = $2 AND activity_id = $1 RETURNING object",
    )
    .bind(activity)
    .bind(attachment)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Review {
    pub actor: String,
    pub actor_role: String,
    pub from_status: String,
    pub to_status: String,
    pub points: Option<i16>,
    pub note: Option<String>,
    pub at: DateTime<Utc>,
}

pub async fn reviews(conn: &mut PgConnection, activity: Uuid) -> Result<Vec<Review>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.display_name AS actor, u.role::text AS actor_role, r.from_status, r.to_status, r.points, r.note, r.at
         FROM activities.reviews r JOIN identity.users u ON u.id = r.actor_id
         WHERE r.activity_id = $1 ORDER BY r.at, r.id",
    )
    .bind(activity)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct KindTotal {
    pub kind: String,
    pub approved: i64,
    pub pending: i64,
    pub points: i64,
}

/// The acting student's approved points and open submissions, per type.
pub async fn my_totals(conn: &mut PgConnection) -> Result<Vec<KindTotal>, sqlx::Error> {
    sqlx::query_as(
        "SELECT kind, count(*) FILTER (WHERE status = 'approved') AS approved,
                count(*) FILTER (WHERE status IN ('submitted', 'hod_review')) AS pending,
                coalesce(sum(points) FILTER (WHERE status = 'approved'), 0)::bigint AS points
         FROM activities.activities WHERE student_id = identity.current_user_id()
         GROUP BY kind",
    )
    .fetch_all(conn)
    .await
}
