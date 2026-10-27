//! The institution's settings and grading scale, which pages read (the
//! attendance threshold, the grade bands). The full product's administration
//! (users, departments, programs, editing the settings) isn't in the demo.

use acadex_db::{
    rls,
    settings::{self, GradeBand, Settings},
};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

use crate::{AppState, auth::AuthUser, error::ApiError};

#[derive(Serialize, Deserialize)]
pub struct Configuration {
    #[serde(flatten)]
    settings: Settings,
    grade_scale: Vec<GradeBand>,
    external_min_pct: f64,
    internal_min_pct: f64,
}

/// Readable by everyone signed in: pages show the threshold and the scale.
pub async fn configuration(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Configuration>, ApiError> {
    let settings = state.cache.settings(&state.pool).await?;
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let (grade_scale, external_min_pct, internal_min_pct) = settings::grading(&mut tx).await?;
    Ok(Json(Configuration {
        settings,
        grade_scale,
        external_min_pct,
        internal_min_pct,
    }))
}
