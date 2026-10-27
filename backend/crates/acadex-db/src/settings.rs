//! Institution settings (architecture.md A4): configuration an administrator
//! changes, not code.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, sqlx::FromRow)]
pub struct Settings {
    /// Below this, a subject is in attendance shortage.
    pub attendance_threshold_pct: f64,
    /// The month an academic year starts (7 = July).
    pub academic_year_start_month: i16,
    /// Days after publication in which re-evaluation can be requested.
    pub results_appeal_days: i16,
    /// Per subject, in paise.
    pub revaluation_fee_paise: i64,
    pub retotal_fee_paise: i64,
    /// When each of the eight teaching periods starts, "HH:MM".
    pub period_starts: Vec<String>,
    pub period_minutes: i16,
    /// Days a teacher can still change a session's marks; 0 for no limit.
    #[serde(default = "default_lock_days")]
    pub attendance_lock_days: i16,
}

fn default_lock_days() -> i16 {
    7
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            attendance_threshold_pct: 75.0,
            academic_year_start_month: 7,
            results_appeal_days: 15,
            revaluation_fee_paise: 50_000,
            retotal_fee_paise: 20_000,
            period_starts: [
                "09:00", "10:00", "11:00", "12:00", "14:00", "15:00", "16:00", "17:00",
            ]
            .map(str::to_owned)
            .to_vec(),
            period_minutes: 55,
            attendance_lock_days: default_lock_days(),
        }
    }
}

impl Settings {
    /// Academic years are stored by their first year: with a July start,
    /// October 2026 is AY 2026-27, stored as 2026.
    /// A period's start and end, "HH:MM", if it's one of the eight.
    pub fn period_times(&self, slot: i16) -> Option<(String, String)> {
        let start = self
            .period_starts
            .get(usize::try_from(slot).ok()?.checked_sub(1)?)?;
        let time = chrono::NaiveTime::parse_from_str(start, "%H:%M").ok()?;
        let end = time + chrono::Duration::minutes(i64::from(self.period_minutes));
        Some((start.clone(), end.format("%H:%M").to_string()))
    }

    /// Whether a teacher may still change a session held on `date`.
    pub fn attendance_open(&self, date: NaiveDate, today: NaiveDate) -> bool {
        self.attendance_lock_days == 0
            || (today - date).num_days() <= i64::from(self.attendance_lock_days)
    }

    pub fn academic_year_of(&self, date: NaiveDate) -> i16 {
        let year = date.year() as i16;
        if date.month() as i16 >= self.academic_year_start_month {
            year
        } else {
            year - 1
        }
    }
}

/// The institution's settings (one institution per deployment, A2), or the
/// defaults if none have been saved.
pub async fn load(conn: &mut PgConnection) -> Result<Settings, sqlx::Error> {
    let row: Option<Settings> = sqlx::query_as(
        "SELECT attendance_threshold_pct::float8 AS attendance_threshold_pct,
                academic_year_start_month, results_appeal_days, revaluation_fee_paise, retotal_fee_paise,
                period_starts, period_minutes, attendance_lock_days
         FROM org.institution_settings LIMIT 1",
    )
    .fetch_optional(conn)
    .await?;
    Ok(row.unwrap_or_default())
}

/// Saves settings as the acting administrator; RLS refuses anyone else.
/// Returns false if nothing was updated.
pub async fn update(conn: &mut PgConnection, settings: &Settings) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE org.institution_settings
         SET attendance_threshold_pct = $1, academic_year_start_month = $2, results_appeal_days = $3,
             revaluation_fee_paise = $4, retotal_fee_paise = $5,
             period_starts = $6, period_minutes = $7, attendance_lock_days = $8,
             updated_at = now(), updated_by = identity.current_user_id()",
    )
    .bind(settings.attendance_threshold_pct)
    .bind(settings.academic_year_start_month)
    .bind(settings.results_appeal_days)
    .bind(settings.revaluation_fee_paise)
    .bind(settings.retotal_fee_paise)
    .bind(&settings.period_starts)
    .bind(settings.period_minutes)
    .bind(settings.attendance_lock_days)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct GradeBand {
    pub min_pct: f64,
    pub grade: String,
    pub points: i16,
}

/// Replaces the grade scale and the external-exam minimum in one go, so a
/// half-edited scale is never visible.
pub async fn replace_grading(
    conn: &mut PgConnection,
    bands: &[GradeBand],
    external_min_pct: f64,
    internal_min_pct: f64,
) -> Result<(), sqlx::Error> {
    let institution: i16 = sqlx::query_scalar(
        "SELECT institution_id FROM identity.users WHERE id = identity.current_user_id()",
    )
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM exams.grade_scale WHERE institution_id = $1")
        .bind(institution)
        .execute(&mut *conn)
        .await?;
    let mins: Vec<f64> = bands.iter().map(|b| b.min_pct).collect();
    let grades: Vec<&str> = bands.iter().map(|b| b.grade.as_str()).collect();
    let points: Vec<i16> = bands.iter().map(|b| b.points).collect();
    sqlx::query(
        "INSERT INTO exams.grade_scale (institution_id, min_pct, grade, points)
         SELECT $1, m, g, p FROM unnest($2::float8[], $3::text[], $4::int2[]) AS b(m, g, p)",
    )
    .bind(institution)
    .bind(&mins)
    .bind(&grades)
    .bind(&points)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE exams.grading_policy SET external_min_pct = $2, internal_min_pct = $3 WHERE institution_id = $1",
    )
        .bind(institution)
        .bind(external_min_pct)
        .bind(internal_min_pct)
        .execute(conn)
        .await?;
    Ok(())
}

/// The grade scale, external minimum and internal minimum.
pub async fn grading(conn: &mut PgConnection) -> Result<(Vec<GradeBand>, f64, f64), sqlx::Error> {
    let bands = sqlx::query_as(
        "SELECT min_pct::float8 AS min_pct, grade, points FROM exams.grade_scale ORDER BY min_pct DESC",
    )
    .fetch_all(&mut *conn)
    .await?;
    let minimums: Option<(f64, f64)> = sqlx::query_as(
        "SELECT external_min_pct::float8, internal_min_pct::float8 FROM exams.grading_policy LIMIT 1",
    )
    .fetch_optional(conn)
    .await?;
    let (external, internal) = minimums.unwrap_or((40.0, 40.0));
    Ok((bands, external, internal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn academic_year_follows_the_start_month() {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        let july = Settings::default();
        assert_eq!(july.academic_year_of(date(2026, 6, 30)), 2025);
        assert_eq!(july.academic_year_of(date(2026, 7, 1)), 2026);
        assert_eq!(july.academic_year_of(date(2027, 1, 15)), 2026);
        let june = Settings {
            academic_year_start_month: 6,
            ..Settings::default()
        };
        assert_eq!(june.academic_year_of(date(2026, 6, 1)), 2026);
    }

    #[test]
    fn periods_have_start_and_end_times() {
        let s = Settings::default();
        assert_eq!(
            s.period_times(1),
            Some(("09:00".to_owned(), "09:55".to_owned()))
        );
        assert_eq!(
            s.period_times(5),
            Some(("14:00".to_owned(), "14:55".to_owned()))
        );
        assert_eq!(s.period_times(0), None);
        assert_eq!(s.period_times(9), None);
    }
}
