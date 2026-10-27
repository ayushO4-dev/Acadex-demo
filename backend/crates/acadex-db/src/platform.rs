//! The platform console: the vendor's operator keeps the university's
//! profile and its administrators, through the SECURITY DEFINER functions in
//! 0941_platform.sql (which check the caller is a platform operator).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Institution {
    pub name: String,
    pub short_name: Option<String>,
    pub city: Option<String>,
    pub website: Option<String>,
    pub contact_email: Option<String>,
    /// When the logo last changed, if there is one (a cache-busting version).
    #[serde(default)]
    pub logo_updated_at: Option<DateTime<Utc>>,
    /// Every university account's email must be on one of these.
    #[serde(default)]
    pub email_domains: Vec<String>,
}

/// The deployment's university (one per deployment).
pub async fn institution(conn: &mut PgConnection) -> Result<Institution, sqlx::Error> {
    sqlx::query_as(
        "SELECT name, short_name, city, website, contact_email, logo_updated_at, email_domains FROM org.institutions ORDER BY id LIMIT 1",
    )
    .fetch_one(conn)
    .await
}

pub async fn update_institution(
    conn: &mut PgConnection,
    i: &Institution,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT identity.platform_update_institution($1, $2, $3, $4, $5)")
        .bind(&i.name)
        .bind(i.short_name.as_deref())
        .bind(i.city.as_deref())
        .bind(i.website.as_deref())
        .bind(i.contact_email.as_deref())
        .execute(&mut *conn)
        .await?;
    sqlx::query("SELECT identity.platform_set_email_domains($1)")
        .bind(&i.email_domains)
        .execute(conn)
        .await?;
    Ok(())
}

/// University accounts whose email isn't on the domains (made before they were set).
pub async fn off_domain_accounts(conn: &mut PgConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT identity.platform_off_domain_accounts()")
        .fetch_one(conn)
        .await
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Administrator {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub active: bool,
    pub signed_in: bool,
    pub created_at: DateTime<Utc>,
}

pub async fn administrators(conn: &mut PgConnection) -> Result<Vec<Administrator>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM identity.platform_list_admins()")
        .fetch_all(conn)
        .await
}

pub async fn create_admin(
    conn: &mut PgConnection,
    email: &str,
    name: &str,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar("SELECT identity.platform_create_admin($1, $2)")
        .bind(email)
        .bind(name)
        .fetch_one(conn)
        .await
}

pub async fn set_admin_active(
    conn: &mut PgConnection,
    user: Uuid,
    active: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT identity.platform_set_admin_active($1, $2)")
        .bind(user)
        .bind(active)
        .execute(conn)
        .await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
pub struct AdminLogin {
    pub subject: Option<String>,
    pub email: String,
    pub display_name: String,
    pub active: bool,
}

pub async fn admin_login(
    conn: &mut PgConnection,
    user: Uuid,
) -> Result<Option<AdminLogin>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM identity.platform_admin_login($1)")
        .bind(user)
        .fetch_optional(conn)
        .await
}

pub async fn link_login(
    conn: &mut PgConnection,
    user: Uuid,
    subject: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT identity.platform_link_login($1, $2)")
        .bind(user)
        .bind(subject)
        .execute(conn)
        .await?;
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Usage {
    pub role: String,
    pub users: i64,
    pub active: i64,
}

pub async fn usage(conn: &mut PgConnection) -> Result<Vec<Usage>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM identity.platform_usage()")
        .fetch_all(conn)
        .await
}

/// The university's logo and its type, for the sign-in page.
pub async fn logo(
    conn: &mut PgConnection,
) -> Result<Option<(Vec<u8>, String, DateTime<Utc>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT logo, logo_type, logo_updated_at FROM org.institutions
         WHERE logo IS NOT NULL ORDER BY id LIMIT 1",
    )
    .fetch_optional(conn)
    .await
}

/// Sets (or with `None`, removes) the university's logo.
pub async fn set_logo(
    conn: &mut PgConnection,
    logo: Option<(&[u8], &str)>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT identity.platform_set_logo($1, $2)")
        .bind(logo.map(|(bytes, _)| bytes))
        .bind(logo.map(|(_, kind)| kind))
        .execute(conn)
        .await?;
    Ok(())
}
