//! The weekly class timetable. Students see their class's week, teachers
//! what they teach, and anyone else a class they pick. (In the full product
//! heads of department and administration also edit it; the demo shows it
//! read-only, to start marking attendance from.)

use acadex_auth::{Permission, Role};
use acadex_db::{
    curriculum, rls,
    timetable::{self, ClassOffering, Entry},
};
use axum::{
    Json,
    extract::{Query, State},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{AppState, auth::AuthUser, error::ApiError};

#[derive(Serialize)]
struct Period {
    slot: i16,
    starts: String,
    ends: String,
}

#[derive(Serialize)]
struct Program {
    id: i16,
    code: String,
    name: String,
}

#[derive(Serialize)]
pub struct Week {
    academic_year: i16,
    /// "class" (one program, semester and section) or "teacher" (the caller's own).
    view: &'static str,
    program_id: Option<i16>,
    semester: Option<i16>,
    section: Option<String>,
    periods: Vec<Period>,
    entries: Vec<Entry>,
    /// The caller may add and remove periods here.
    editable: bool,
    /// Programs to choose from (staff), and the class's subjects to place (editors).
    programs: Vec<Program>,
    offerings: Vec<ClassOffering>,
}

#[derive(Deserialize)]
pub struct WeekQuery {
    program_id: Option<i16>,
    semester: Option<i16>,
    section: Option<String>,
}

pub async fn week(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<WeekQuery>,
) -> Result<Json<Week>, ApiError> {
    let settings = state.cache.settings(&state.pool).await?;
    let academic_year = settings.academic_year_of(Utc::now().date_naive());
    let periods = (1..=8)
        .filter_map(|slot| {
            settings
                .period_times(slot)
                .map(|(starts, ends)| Period { slot, starts, ends })
        })
        .collect();
    let mut tx = rls::begin_as(&state.pool, user.id, user.role.as_str()).await?;
    let section = query
        .section
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_uppercase);

    // A student's own class, whatever they ask for.
    if user.role == Role::Student {
        let Some((program, semester, own_section)) = timetable::my_class(&mut tx).await? else {
            return Err(ApiError::not_found("class"));
        };
        let entries = timetable::for_class(
            &mut tx,
            program,
            semester,
            Some(&own_section),
            academic_year,
            Some(user.id),
        )
        .await?;
        return Ok(Json(Week {
            academic_year,
            view: "class",
            program_id: Some(program),
            semester: Some(semester),
            section: Some(own_section),
            periods,
            entries,
            editable: false,
            programs: Vec::new(),
            offerings: Vec::new(),
        }));
    }

    // A teacher with no class asked for: their own week.
    if query.program_id.is_none() && matches!(user.role, Role::Faculty | Role::Hod) {
        let entries = timetable::for_teacher(&mut tx, academic_year).await?;
        if !entries.is_empty() || user.role == Role::Faculty {
            return Ok(Json(Week {
                academic_year,
                view: "teacher",
                program_id: None,
                semester: None,
                section: None,
                periods,
                entries,
                editable: false,
                programs: all_programs(&mut tx).await?,
                offerings: Vec::new(),
            }));
        }
    }

    // Any class: managers default to one of theirs.
    let programs = all_programs(&mut tx).await?;
    let manageable: Vec<i16> = if user.has(Permission::CurriculumManage) {
        curriculum::manageable_programs(&mut tx)
            .await?
            .into_iter()
            .map(|(id, _, _)| id)
            .collect()
    } else {
        Vec::new()
    };
    let program = query
        .program_id
        .filter(|p| programs.iter().any(|x| x.id == *p))
        .or_else(|| manageable.first().copied())
        .or_else(|| programs.first().map(|p| p.id));
    let Some(program) = program else {
        return Err(ApiError::not_found("program"));
    };
    let semester = query.semester.filter(|s| (1..=8).contains(s)).unwrap_or(1);
    let editable = manageable.contains(&program);
    let entries = timetable::for_class(
        &mut tx,
        program,
        semester,
        section.as_deref(),
        academic_year,
        None,
    )
    .await?;
    let offerings = if editable {
        timetable::class_offerings(&mut tx, program, semester, academic_year).await?
    } else {
        Vec::new()
    };
    Ok(Json(Week {
        academic_year,
        view: "class",
        program_id: Some(program),
        semester: Some(semester),
        section,
        periods,
        entries,
        editable,
        programs,
        offerings,
    }))
}

async fn all_programs(tx: &mut sqlx::PgConnection) -> Result<Vec<Program>, ApiError> {
    let rows: Vec<(i16, String, String)> =
        sqlx::query_as("SELECT id, code, name FROM org.programs ORDER BY name")
            .fetch_all(tx)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, code, name)| Program { id, code, name })
        .collect())
}
