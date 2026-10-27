//! The university's public face: its name and logo, for the sign-in page.
//! (The full product's platform console, where the vendor manages the
//! university, isn't in the demo.)

use acadex_db::platform;
use axum::{Json, extract::State};
use serde::Serialize;

use crate::{AppState, error::ApiError};

/// The university's name, for the sign-in page and the public notice board.
#[derive(Serialize)]
pub struct PublicInstitution {
    name: String,
    short_name: Option<String>,
    /// Set when there's a logo: also its version, for the URL.
    logo_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Where its accounts' addresses must be (forms hint with it).
    email_domains: Vec<String>,
}

pub async fn public_institution(
    State(state): State<AppState>,
) -> Result<Json<PublicInstitution>, ApiError> {
    let mut conn = state.pool.acquire().await?;
    let i = platform::institution(&mut conn).await?;
    Ok(Json(PublicInstitution {
        name: i.name,
        short_name: i.short_name,
        logo_updated_at: i.logo_updated_at,
        email_domains: i.email_domains,
    }))
}

pub async fn public_logo(
    State(state): State<AppState>,
) -> Result<axum::response::Response, ApiError> {
    use axum::{http::header, response::IntoResponse};
    let mut conn = state.pool.acquire().await?;
    let (bytes, kind, updated) = platform::logo(&mut conn)
        .await?
        .ok_or_else(|| ApiError::not_found("logo"))?;
    Ok((
        [
            (header::CONTENT_TYPE, kind),
            (header::CACHE_CONTROL, "public, max-age=300".to_owned()),
            (header::ETAG, format!("\"{}\"", updated.timestamp_millis())),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
        ],
        bytes,
    )
        .into_response())
}
