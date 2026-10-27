//! The demo data, compiled into the binary (`acadex-api seed`, which Docker
//! Compose runs after the migrations). Each file runs once, recorded in
//! ops.seed_log.

pub const ALL: &[(&str, &str)] = &[
    ("00_base.sql", include_str!("../seeds/00_base.sql")),
    ("10_roles.sql", include_str!("../seeds/10_roles.sql")),
    ("20_academic.sql", include_str!("../seeds/20_academic.sql")),
    ("30_exams.sql", include_str!("../seeds/30_exams.sql")),
    ("40_results.sql", include_str!("../seeds/40_results.sql")),
    (
        "50_revaluation.sql",
        include_str!("../seeds/50_revaluation.sql"),
    ),
    (
        "60_activities.sql",
        include_str!("../seeds/60_activities.sql"),
    ),
    ("70_profile.sql", include_str!("../seeds/70_profile.sql")),
    ("80_support.sql", include_str!("../seeds/80_support.sql")),
    (
        "90_practicals.sql",
        include_str!("../seeds/90_practicals.sql"),
    ),
    (
        "92_timetable.sql",
        include_str!("../seeds/92_timetable.sql"),
    ),
    (
        "93_electives.sql",
        include_str!("../seeds/93_electives.sql"),
    ),
    (
        "95_fees_accounts.sql",
        include_str!("../seeds/95_fees_accounts.sql"),
    ),
    (
        "96_platform_admin.sql",
        include_str!("../seeds/96_platform_admin.sql"),
    ),
    (
        "97_university.sql",
        include_str!("../seeds/97_university.sql"),
    ),
    (
        "98_names_check.sql",
        include_str!("../seeds/98_names_check.sql"),
    ),
    (
        "99_demo_identities.sql",
        include_str!("../seeds/99_demo_identities.sql"),
    ),
    (
        "99_institution_logo.sql",
        include_str!("../seeds/99_institution_logo.sql"),
    ),
];

/// Applies every seed file not yet recorded, in order; returns the ones applied.
pub async fn apply(pool: &sqlx::PgPool) -> Result<Vec<&'static str>, sqlx::Error> {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS ops.seed_log (name text PRIMARY KEY, applied_at timestamptz NOT NULL DEFAULT now())",
    )
    .execute(pool)
    .await?;
    let mut applied = Vec::new();
    for (name, sql) in ALL {
        let done: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM ops.seed_log WHERE name = $1)")
                .bind(name)
                .fetch_one(pool)
                .await?;
        if done {
            continue;
        }
        tracing::info!(name, "seeding");
        sqlx::raw_sql(*sql).execute(pool).await?;
        sqlx::query("INSERT INTO ops.seed_log (name) VALUES ($1)")
            .bind(name)
            .execute(pool)
            .await?;
        applied.push(*name);
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_seed_file_is_compiled_in() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/seeds");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".sql"))
            .collect();
        on_disk.sort();
        let listed: Vec<&str> = super::ALL.iter().map(|(n, _)| *n).collect();
        assert_eq!(on_disk, listed, "add new seed files to seeds::ALL");
    }
}
