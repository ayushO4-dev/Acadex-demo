//! Departments: they own programs, and their HOD's reach is those programs.

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, types::Json};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProgramRef {
    pub id: i16,
    pub name: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Department {
    pub id: i16,
    pub code: String,
    pub name: String,
    pub hod_id: Option<Uuid>,
    pub hod_name: Option<String>,
    pub programs: Json<Vec<ProgramRef>>,
    pub students: i64,
    pub faculty: i64,
}

macro_rules! department_sql {
    () => {
        "SELECT d.id, d.code, d.name, d.hod_id, h.display_name AS hod_name,
                coalesce((SELECT jsonb_agg(jsonb_build_object('id', p.id, 'name', p.name) ORDER BY p.name)
                          FROM org.programs p WHERE p.department_id = d.id), '[]'::jsonb) AS programs,
                (SELECT count(*) FROM people.students s
                 JOIN org.programs p ON p.id = s.program_id WHERE p.department_id = d.id) AS students,
                (SELECT count(*) FROM people.faculty f
                 JOIN org.programs p ON p.id = f.program_id WHERE p.department_id = d.id) AS faculty
         FROM org.departments d
         LEFT JOIN identity.users h ON h.id = d.hod_id"
    };
}

/// All departments. Student counts are what the caller's RLS lets them see.
pub async fn departments(conn: &mut PgConnection) -> Result<Vec<Department>, sqlx::Error> {
    sqlx::query_as(concat!(department_sql!(), " ORDER BY d.name"))
        .fetch_all(conn)
        .await
}

/// The department the acting user heads, if any.
pub async fn headed_by_me(conn: &mut PgConnection) -> Result<Option<Department>, sqlx::Error> {
    sqlx::query_as(concat!(
        department_sql!(),
        " WHERE d.hod_id = identity.current_user_id()"
    ))
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Deserialize)]
pub struct NewDepartment {
    pub code: String,
    pub name: String,
}

pub async fn create(
    conn: &mut PgConnection,
    department: &NewDepartment,
) -> Result<i16, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO org.departments (id, institution_id, code, name)
         SELECT coalesce(max(d.id), 0) + 1, u.institution_id, $1, $2
         FROM identity.users u LEFT JOIN org.departments d ON true
         WHERE u.id = identity.current_user_id()
         GROUP BY u.institution_id
         RETURNING id",
    )
    .bind(&department.code)
    .bind(&department.name)
    .fetch_one(conn)
    .await
}

/// Renames a department and sets its HOD (who must have the `hod` role) and
/// programs. `false` if there's no such department, or the HOD isn't one.
pub async fn update(
    conn: &mut PgConnection,
    id: i16,
    name: &str,
    hod_id: Option<Uuid>,
    program_ids: &[i16],
) -> Result<bool, sqlx::Error> {
    if let Some(hod) = hod_id {
        let is_hod: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM identity.users WHERE id = $1 AND role = 'hod')",
        )
        .bind(hod)
        .fetch_one(&mut *conn)
        .await?;
        if !is_hod {
            return Ok(false);
        }
    }
    let updated = sqlx::query("UPDATE org.departments SET name = $2, hod_id = $3 WHERE id = $1")
        .bind(id)
        .bind(name)
        .bind(hod_id)
        .execute(&mut *conn)
        .await?;
    if updated.rows_affected() == 0 {
        return Ok(false);
    }
    sqlx::query("UPDATE org.programs SET department_id = $1 WHERE id = ANY($2)")
        .bind(id)
        .bind(program_ids)
        .execute(conn)
        .await?;
    Ok(true)
}

/// Adds a program (and this year's and next year's attendance partitions).
pub async fn create_program(
    conn: &mut PgConnection,
    code: &str,
    name: &str,
    semesters: i16,
    department_id: Option<i16>,
) -> Result<i16, sqlx::Error> {
    sqlx::query_scalar("SELECT org.create_program($1, $2, $3, $4)")
        .bind(code)
        .bind(name)
        .bind(semesters)
        .bind(department_id)
        .fetch_one(conn)
        .await
}

/// Renames or resizes a program; its code never changes.
pub async fn update_program(
    conn: &mut PgConnection,
    id: i16,
    name: &str,
    semesters: i16,
    department_id: Option<i16>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT org.update_program($1, $2, $3, $4)")
        .bind(id)
        .bind(name)
        .bind(semesters)
        .bind(department_id)
        .execute(conn)
        .await?;
    Ok(())
}
