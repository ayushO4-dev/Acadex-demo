//! User administration, through the SECURITY DEFINER functions in
//! 0320_admin.sql: they check the acting user is an administrator, and keep
//! emails out of every other query.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub active: bool,
    /// Has signed in at least once (their login is linked).
    pub signed_in: bool,
    pub created_at: DateTime<Utc>,
    /// Roll number, program and section for students; designation and program for staff.
    pub detail: Option<String>,
}

pub struct UserQuery<'a> {
    pub search: Option<&'a str>,
    pub role: Option<&'a str>,
    /// Keyset: the last row of the previous page.
    pub after: Option<(&'a str, Uuid)>,
    pub limit: i32,
}

pub async fn list_users(
    conn: &mut PgConnection,
    query: &UserQuery<'_>,
) -> Result<Vec<UserRow>, sqlx::Error> {
    // LIKE wildcards in the search are matched literally.
    let search = query.search.map(|s| {
        s.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    });
    sqlx::query_as("SELECT * FROM identity.admin_list_users($1, $2, $3, $4, $5)")
        .bind(search)
        .bind(query.role)
        .bind(query.after.map(|(name, _)| name))
        .bind(query.after.map(|(_, id)| id))
        .bind(query.limit)
        .fetch_all(conn)
        .await
}

#[derive(Debug, Deserialize)]
pub struct NewUser {
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub program_id: Option<i16>,
    pub roll_no: Option<String>,
    pub batch_year: Option<i16>,
    pub semester: Option<i16>,
    pub section: Option<String>,
    pub designation: Option<String>,
    pub department_id: Option<i16>,
}

pub async fn create_user(conn: &mut PgConnection, user: &NewUser) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar("SELECT identity.admin_create_user($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)")
        .bind(&user.email)
        .bind(&user.display_name)
        .bind(&user.role)
        .bind(user.program_id)
        .bind(user.roll_no.as_deref())
        .bind(user.batch_year)
        .bind(user.semester)
        .bind(user.section.as_deref())
        .bind(user.designation.as_deref())
        .bind(user.department_id)
        .fetch_one(conn)
        .await
}

pub async fn set_role(conn: &mut PgConnection, user: Uuid, role: &str) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT identity.admin_set_role($1, $2)")
        .bind(user)
        .bind(role)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn set_active(
    conn: &mut PgConnection,
    user: Uuid,
    active: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT identity.admin_set_active($1, $2)")
        .bind(user)
        .bind(active)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn link_login(
    conn: &mut PgConnection,
    user: Uuid,
    subject: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT identity.admin_link_login($1, $2)")
        .bind(user)
        .bind(subject)
        .execute(conn)
        .await?;
    Ok(())
}

/// The user's linked Auth0 subject (if they've signed in) and email.
pub async fn login_of(
    conn: &mut PgConnection,
    user: Uuid,
) -> Result<Option<(Option<String>, String)>, sqlx::Error> {
    sqlx::query_as("SELECT subject, email FROM identity.admin_login_of($1)")
        .bind(user)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RoleCount {
    pub role: String,
    pub users: i64,
    pub active: i64,
}

/// Users per role, for the admin dashboard.
pub async fn role_counts(conn: &mut PgConnection) -> Result<Vec<RoleCount>, sqlx::Error> {
    sqlx::query_as(
        "SELECT role::text AS role, count(*) AS users, count(*) FILTER (WHERE active) AS active
         FROM identity.users WHERE role <> 'platform_admin' GROUP BY role ORDER BY role",
    )
    .fetch_all(conn)
    .await
}

/// Everything an administrator can correct about a user.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
pub struct UserDetail {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub active: bool,
    pub signed_in: bool,
    pub program_id: Option<i16>,
    pub roll_no: Option<String>,
    pub batch_year: Option<i16>,
    pub semester: Option<i16>,
    pub section: Option<String>,
    pub graduated_on: Option<chrono::NaiveDate>,
    pub designation: Option<String>,
}

pub async fn user_detail(
    conn: &mut PgConnection,
    user: Uuid,
) -> Result<Option<UserDetail>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM identity.admin_user_detail($1)")
        .bind(user)
        .fetch_optional(conn)
        .await
}

#[derive(Debug, Deserialize)]
pub struct UserUpdate {
    pub display_name: String,
    pub email: String,
    pub program_id: Option<i16>,
    pub roll_no: Option<String>,
    pub batch_year: Option<i16>,
    pub semester: Option<i16>,
    pub section: Option<String>,
    pub designation: Option<String>,
}

pub async fn update_user(
    conn: &mut PgConnection,
    user: Uuid,
    update: &UserUpdate,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT identity.admin_update_user($1, $2, $3, $4, $5, $6, $7, $8, $9)")
        .bind(user)
        .bind(&update.display_name)
        .bind(&update.email)
        .bind(update.program_id)
        .bind(update.roll_no.as_deref())
        .bind(update.batch_year)
        .bind(update.semester)
        .bind(update.section.as_deref())
        .bind(update.designation.as_deref())
        .execute(conn)
        .await?;
    Ok(())
}
