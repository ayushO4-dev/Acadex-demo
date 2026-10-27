//! The student profile (architecture.md §9.5): sections, documents, edit
//! requests, and the purpose-bound log of staff reads. Aadhaar and bank
//! account numbers arrive here already sealed by the API; only their last
//! four digits are ever read back in the clear.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Admission {
    pub admission_no: String,
    pub admitted_on: NaiveDate,
    pub quota: String,
}

#[derive(Debug, Default, Serialize, sqlx::FromRow)]
pub struct Personal {
    pub date_of_birth: Option<NaiveDate>,
    pub gender: Option<String>,
    pub blood_group: Option<String>,
    pub category: Option<String>,
    pub nationality: Option<String>,
    pub mother_tongue: Option<String>,
    pub aadhaar_last4: Option<String>,
    pub bank_account_last4: Option<String>,
    pub bank_ifsc: Option<String>,
    pub bank_name: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, sqlx::FromRow)]
pub struct Parent {
    pub father_name: Option<String>,
    pub father_occupation: Option<String>,
    pub father_phone: Option<String>,
    pub mother_name: Option<String>,
    pub mother_occupation: Option<String>,
    pub mother_phone: Option<String>,
    pub guardian_name: Option<String>,
    pub guardian_relation: Option<String>,
    pub guardian_phone: Option<String>,
    pub annual_income: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, sqlx::FromRow)]
pub struct Contact {
    pub phone: Option<String>,
    pub personal_email: Option<String>,
    pub correspondence_address: Option<String>,
    pub permanent_address: Option<String>,
    pub emergency_name: Option<String>,
    pub emergency_relation: Option<String>,
    pub emergency_phone: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Education {
    pub level: String,
    pub board: String,
    pub institution: String,
    pub year: i16,
    pub score: String,
}

pub async fn admission(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Option<Admission>, sqlx::Error> {
    sqlx::query_as(
        "SELECT admission_no, admitted_on, quota FROM profile.admission WHERE student_id = $1",
    )
    .bind(student)
    .fetch_optional(conn)
    .await
}

pub async fn personal(conn: &mut PgConnection, student: Uuid) -> Result<Personal, sqlx::Error> {
    let row = sqlx::query_as(
        "SELECT date_of_birth, gender, blood_group, category, nationality, mother_tongue,
                aadhaar_last4, bank_account_last4, bank_ifsc, bank_name
         FROM profile.personal_info WHERE student_id = $1",
    )
    .bind(student)
    .fetch_optional(conn)
    .await?;
    Ok(row.unwrap_or_default())
}

pub async fn parent(conn: &mut PgConnection, student: Uuid) -> Result<Parent, sqlx::Error> {
    let row = sqlx::query_as(
        "SELECT father_name, father_occupation, father_phone, mother_name, mother_occupation, mother_phone,
                guardian_name, guardian_relation, guardian_phone, annual_income
         FROM profile.parent_info WHERE student_id = $1",
    )
    .bind(student)
    .fetch_optional(conn)
    .await?;
    Ok(row.unwrap_or_default())
}

pub async fn contact(conn: &mut PgConnection, student: Uuid) -> Result<Contact, sqlx::Error> {
    let row = sqlx::query_as(
        "SELECT phone, personal_email, correspondence_address, permanent_address,
                emergency_name, emergency_relation, emergency_phone
         FROM profile.contact_info WHERE student_id = $1",
    )
    .bind(student)
    .fetch_optional(conn)
    .await?;
    Ok(row.unwrap_or_default())
}

pub async fn education(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Vec<Education>, sqlx::Error> {
    sqlx::query_as(
        "SELECT level, board, institution, year, score FROM profile.education WHERE student_id = $1
         ORDER BY array_position(ARRAY['class_10', 'class_12', 'diploma', 'entrance'], level)",
    )
    .bind(student)
    .fetch_all(conn)
    .await
}

/// The student's own direct fields in personal info.
#[derive(Debug, Deserialize)]
pub struct PersonalEdit {
    pub gender: Option<String>,
    pub nationality: Option<String>,
    pub mother_tongue: Option<String>,
}

/// As the acting student (RLS and column grants limit it to these fields).
pub async fn update_personal(
    conn: &mut PgConnection,
    edit: &PersonalEdit,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO profile.personal_info (student_id) VALUES (identity.current_user_id())
         ON CONFLICT (student_id) DO NOTHING",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE profile.personal_info SET gender = $1, nationality = $2, mother_tongue = $3, updated_at = now()
         WHERE student_id = identity.current_user_id()",
    )
    .bind(&edit.gender)
    .bind(&edit.nationality)
    .bind(&edit.mother_tongue)
    .execute(conn)
    .await?;
    Ok(())
}

/// Bank details, the account number sealed by the caller.
pub async fn update_bank(
    conn: &mut PgConnection,
    sealed: Option<(&[u8], &str)>,
    ifsc: Option<&str>,
    bank_name: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO profile.personal_info (student_id) VALUES (identity.current_user_id())
         ON CONFLICT (student_id) DO NOTHING",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE profile.personal_info
         SET bank_account_sealed = $1, bank_account_last4 = $2, bank_ifsc = $3, bank_name = $4, updated_at = now()
         WHERE student_id = identity.current_user_id()",
    )
    .bind(sealed.map(|(s, _)| s))
    .bind(sealed.map(|(_, last4)| last4))
    .bind(ifsc)
    .bind(bank_name)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn update_parent(conn: &mut PgConnection, p: &Parent) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO profile.parent_info (student_id, father_name, father_occupation, father_phone, mother_name,
                                          mother_occupation, mother_phone, guardian_name, guardian_relation,
                                          guardian_phone, annual_income)
         VALUES (identity.current_user_id(), $1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (student_id) DO UPDATE SET
             father_name = EXCLUDED.father_name, father_occupation = EXCLUDED.father_occupation,
             father_phone = EXCLUDED.father_phone, mother_name = EXCLUDED.mother_name,
             mother_occupation = EXCLUDED.mother_occupation, mother_phone = EXCLUDED.mother_phone,
             guardian_name = EXCLUDED.guardian_name, guardian_relation = EXCLUDED.guardian_relation,
             guardian_phone = EXCLUDED.guardian_phone, annual_income = EXCLUDED.annual_income, updated_at = now()",
    )
    .bind(&p.father_name)
    .bind(&p.father_occupation)
    .bind(&p.father_phone)
    .bind(&p.mother_name)
    .bind(&p.mother_occupation)
    .bind(&p.mother_phone)
    .bind(&p.guardian_name)
    .bind(&p.guardian_relation)
    .bind(&p.guardian_phone)
    .bind(&p.annual_income)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn update_contact(conn: &mut PgConnection, c: &Contact) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO profile.contact_info (student_id, phone, personal_email, correspondence_address, permanent_address,
                                           emergency_name, emergency_relation, emergency_phone)
         VALUES (identity.current_user_id(), $1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (student_id) DO UPDATE SET
             phone = EXCLUDED.phone, personal_email = EXCLUDED.personal_email,
             correspondence_address = EXCLUDED.correspondence_address, permanent_address = EXCLUDED.permanent_address,
             emergency_name = EXCLUDED.emergency_name, emergency_relation = EXCLUDED.emergency_relation,
             emergency_phone = EXCLUDED.emergency_phone, updated_at = now()",
    )
    .bind(&c.phone)
    .bind(&c.personal_email)
    .bind(&c.correspondence_address)
    .bind(&c.permanent_address)
    .bind(&c.emergency_name)
    .bind(&c.emergency_relation)
    .bind(&c.emergency_phone)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn replace_education(
    conn: &mut PgConnection,
    rows: &[Education],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM profile.education WHERE student_id = identity.current_user_id()")
        .execute(&mut *conn)
        .await?;
    for row in rows {
        sqlx::query(
            "INSERT INTO profile.education (student_id, level, board, institution, year, score)
             VALUES (identity.current_user_id(), $1, $2, $3, $4, $5)",
        )
        .bind(&row.level)
        .bind(&row.board)
        .bind(&row.institution)
        .bind(row.year)
        .bind(&row.score)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// A sealed PII value, for an authorised reveal: `aadhaar` or `bank_account`.
pub async fn sealed(
    conn: &mut PgConnection,
    student: Uuid,
    field: &str,
) -> Result<Option<Vec<u8>>, sqlx::Error> {
    let column: Option<Option<Vec<u8>>> = match field {
        "aadhaar" => {
            sqlx::query_scalar(
                "SELECT aadhaar_sealed FROM profile.personal_info WHERE student_id = $1",
            )
            .bind(student)
            .fetch_optional(conn)
            .await?
        }
        "bank_account" => {
            sqlx::query_scalar(
                "SELECT bank_account_sealed FROM profile.personal_info WHERE student_id = $1",
            )
            .bind(student)
            .fetch_optional(conn)
            .await?
        }
        _ => None,
    };
    Ok(column.flatten())
}

// ── Documents ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Document {
    pub id: Uuid,
    pub student_id: Uuid,
    pub student_name: String,
    pub roll_no: String,
    pub doc_type: String,
    /// What an "other" document is.
    pub description: Option<String>,
    pub name: String,
    pub content_type: String,
    pub size_bytes: i32,
    pub scan_status: String,
    pub scan_detail: Option<String>,
    pub verification: String,
    pub verified_by: Option<String>,
    pub verified_at: Option<DateTime<Utc>>,
    pub verify_note: Option<String>,
    pub uploaded_at: DateTime<Utc>,
}

macro_rules! document_sql {
    () => {
        "SELECT d.id, d.student_id, u.display_name AS student_name, st.roll_no, d.doc_type, d.description, d.name,
                d.content_type, d.size_bytes, d.scan_status, d.scan_detail, d.verification, v.display_name AS verified_by,
                d.verified_at, d.verify_note, d.uploaded_at
         FROM profile.documents d
         JOIN people.students st ON st.user_id = d.student_id
         JOIN identity.users u ON u.id = d.student_id
         LEFT JOIN identity.users v ON v.id = d.verified_by"
    };
}

pub async fn documents(
    conn: &mut PgConnection,
    student: Uuid,
) -> Result<Vec<Document>, sqlx::Error> {
    sqlx::query_as(concat!(
        document_sql!(),
        " WHERE d.student_id = $1 ORDER BY d.uploaded_at DESC"
    ))
    .bind(student)
    .fetch_all(conn)
    .await
}

/// Clean documents waiting for the registrar, oldest first.
pub async fn documents_to_verify(
    conn: &mut PgConnection,
    limit: i64,
) -> Result<Vec<Document>, sqlx::Error> {
    sqlx::query_as(concat!(
        document_sql!(),
        " WHERE d.verification = 'pending' AND d.scan_status = 'clean' ORDER BY d.uploaded_at LIMIT $1"
    ))
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// A document as uploaded, before it's scanned.
pub struct NewDocument<'a> {
    pub id: Uuid,
    pub doc_type: &'a str,
    /// What an "other" document is.
    pub description: Option<&'a str>,
    pub object: &'a str,
    pub name: &'a str,
    pub content_type: &'a str,
    pub size: i32,
}

pub async fn add_document(
    conn: &mut PgConnection,
    doc: &NewDocument<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO profile.documents (id, student_id, doc_type, description, object, name, content_type, size_bytes)
         VALUES ($1, identity.current_user_id(), $2, $7, $3, $4, $5, $6)",
    )
    .bind(doc.id)
    .bind(doc.doc_type)
    .bind(doc.object)
    .bind(doc.name)
    .bind(doc.content_type)
    .bind(doc.size)
    .bind(doc.description)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow)]
pub struct StoredDocument {
    pub id: Uuid,
    pub student_id: Uuid,
    pub object: String,
    pub name: String,
    pub content_type: String,
    pub scan_status: String,
}

pub async fn stored(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<StoredDocument>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, student_id, object, name, content_type, scan_status FROM profile.documents WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

pub async fn stored_by_object(
    conn: &mut PgConnection,
    object: &str,
) -> Result<Option<StoredDocument>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, student_id, object, name, content_type, scan_status FROM profile.documents WHERE object = $1",
    )
    .bind(object)
    .fetch_optional(conn)
    .await
}

/// As the worker: the scan's outcome. `false` if it was already scanned.
pub async fn record_scan(
    conn: &mut PgConnection,
    id: Uuid,
    status: &str,
    detail: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE profile.documents SET scan_status = $2, scan_detail = $3, scanned_at = now()
         WHERE id = $1 AND scan_status = 'scanning'",
    )
    .bind(id)
    .bind(status)
    .bind(detail)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// As the registrar: verify or reject a clean document.
pub async fn verify(
    conn: &mut PgConnection,
    id: Uuid,
    verified: bool,
    note: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE profile.documents
         SET verification = $2, verified_by = identity.current_user_id(), verified_at = now(), verify_note = $3
         WHERE id = $1 AND scan_status = 'clean' AND verification = 'pending'",
    )
    .bind(id)
    .bind(if verified { "verified" } else { "rejected" })
    .bind(note)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

// ── Edit requests ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct EditRequest {
    pub id: Uuid,
    pub student_id: Uuid,
    pub student_name: String,
    pub roll_no: String,
    pub program_name: String,
    pub field: String,
    pub old_display: Option<String>,
    pub new_display: String,
    pub reason: String,
    pub document_id: Option<Uuid>,
    pub document_name: Option<String>,
    pub status: String,
    pub decided_by: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub created_at: DateTime<Utc>,
}

macro_rules! request_sql {
    () => {
        "SELECT r.id, r.student_id, u.display_name AS student_name, st.roll_no, p.name AS program_name,
                r.field, r.old_display, r.new_display, r.reason, r.document_id, d.name AS document_name,
                r.status, x.display_name AS decided_by, r.decided_at, r.decision_note, r.created_at
         FROM profile.edit_requests r
         JOIN people.students st ON st.user_id = r.student_id
         JOIN identity.users u ON u.id = r.student_id
         JOIN org.programs p ON p.id = st.program_id
         LEFT JOIN profile.documents d ON d.id = r.document_id
         LEFT JOIN identity.users x ON x.id = r.decided_by"
    };
}

/// A student's requests, or (for the registrar) every pending one first.
pub async fn requests(
    conn: &mut PgConnection,
    student: Option<Uuid>,
    limit: i64,
) -> Result<Vec<EditRequest>, sqlx::Error> {
    sqlx::query_as(concat!(
        request_sql!(),
        " WHERE ($1::uuid IS NULL OR r.student_id = $1)
          ORDER BY r.status = 'pending' DESC, CASE WHEN r.status = 'pending' THEN r.created_at END, r.created_at DESC
          LIMIT $2"
    ))
    .bind(student)
    .bind(limit)
    .fetch_all(conn)
    .await
}

pub async fn request(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<EditRequest>, sqlx::Error> {
    sqlx::query_as(concat!(request_sql!(), " WHERE r.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

pub struct NewRequest<'a> {
    pub field: &'a str,
    pub old_display: Option<&'a str>,
    pub new_display: &'a str,
    pub new_value: Option<&'a str>,
    pub new_sealed: Option<&'a [u8]>,
    pub reason: &'a str,
    pub document_id: Option<Uuid>,
}

pub async fn create_request(
    conn: &mut PgConnection,
    r: &NewRequest<'_>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO profile.edit_requests (student_id, field, old_display, new_display, new_value, new_sealed, reason, document_id)
         VALUES (identity.current_user_id(), $1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(r.field)
    .bind(r.old_display)
    .bind(r.new_display)
    .bind(r.new_value)
    .bind(r.new_sealed)
    .bind(r.reason)
    .bind(r.document_id)
    .fetch_one(conn)
    .await
}

pub async fn withdraw_request(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("UPDATE profile.edit_requests SET status = 'withdrawn' WHERE id = $1 AND status = 'pending'")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// As the registrar; an approval applies the change in the same step.
pub async fn decide_request(
    conn: &mut PgConnection,
    id: Uuid,
    approve: bool,
    note: Option<&str>,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT profile.decide_edit_request($1, $2, $3)")
        .bind(id)
        .bind(approve)
        .bind(note)
        .fetch_one(conn)
        .await
}

// ── Staff reads ─────────────────────────────────────────────────────────────

pub async fn log_access(
    conn: &mut PgConnection,
    student: Uuid,
    scope: &str,
    purpose: &str,
    trace_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit.data_access_log (actor_id, actor_role, student_id, scope, purpose, trace_id)
         VALUES (identity.current_user_id(), identity.current_app_role(), $1, $2, $3, $4)",
    )
    .bind(student)
    .bind(scope)
    .bind(purpose)
    .bind(trace_id)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Access {
    pub actor: String,
    pub actor_role: String,
    pub scope: String,
    pub purpose: String,
    pub at: DateTime<Utc>,
}

/// Who read a student's data (RLS: the student themselves, or an administrator).
pub async fn accesses(
    conn: &mut PgConnection,
    student: Uuid,
    limit: i64,
) -> Result<Vec<Access>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.display_name AS actor, l.actor_role, l.scope, l.purpose, l.at
         FROM audit.data_access_log l JOIN identity.users u ON u.id = l.actor_id
         WHERE l.student_id = $1 ORDER BY l.at DESC LIMIT $2",
    )
    .bind(student)
    .bind(limit)
    .fetch_all(conn)
    .await
}

/// Removes the acting student's unverified document; returns its storage object.
pub async fn remove_my_document(conn: &mut PgConnection, id: Uuid) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT profile.remove_my_document($1)")
        .bind(id)
        .fetch_one(conn)
        .await
}
