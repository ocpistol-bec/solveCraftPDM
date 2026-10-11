//! PDM database layer: migration runner and repository queries.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use chrono::{DateTime, Duration, Utc};
use pdm_core::*;
use sqlx::PgPool;
use uuid::Uuid;

pub mod migrate;

// ── Migrations ───────────────────────────────────────────────────────────

pub async fn migrate(pool: &PgPool) -> Result<(), PdmError> {
    migrate::run_migrations(pool).await
}

// ── Organizations ────────────────────────────────────────────────────────

pub async fn create_org(pool: &PgPool, name: &str) -> Result<Organization, PdmError> {
    sqlx::query_as::<_, Organization>("INSERT INTO organizations (name) VALUES ($1) RETURNING id, name, created_at")
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(PdmError::Db)
}

pub async fn get_org_by_name(pool: &PgPool, name: &str) -> Result<Option<Organization>, PdmError> {
    sqlx::query_as::<_, Organization>("SELECT id, name, created_at FROM organizations WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(PdmError::Db)
}

// ── Users ────────────────────────────────────────────────────────────────

pub async fn create_user(pool: &PgPool, org_id: Uuid, email: &str, display_name: &str, role: &str) -> Result<User, PdmError> {
    sqlx::query_as::<_, User>(
        "INSERT INTO users (org_id, email, display_name, role)
         VALUES ($1, $2, $3, $4)
         RETURNING id, org_id, email, display_name, role, active, created_at",
    )
    .bind(org_id)
    .bind(email)
    .bind(display_name)
    .bind(role)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_user_by_email(pool: &PgPool, org_id: Uuid, email: &str) -> Result<Option<User>, PdmError> {
    sqlx::query_as::<_, User>(
        "SELECT id, org_id, email, display_name, role, active, created_at
             FROM users WHERE org_id = $1 AND email = $2",
    )
    .bind(org_id)
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_user_by_id(pool: &PgPool, id: Uuid) -> Result<Option<User>, PdmError> {
    sqlx::query_as::<_, User>(
        "SELECT id, org_id, email, display_name, role, active, created_at
             FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

// ── API Tokens ───────────────────────────────────────────────────────────

pub async fn create_token(
    pool: &PgPool,
    org_id: Uuid,
    user_id: Option<Uuid>,
    name: &str,
    token_hash: &str,
    scopes: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<ApiToken, PdmError> {
    sqlx::query_as::<_, ApiToken>(
        "INSERT INTO api_tokens (org_id, user_id, name, token_hash, scopes, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, org_id, user_id, name, scopes, expires_at, created_at",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(name)
    .bind(token_hash)
    .bind(scopes)
    .bind(expires_at)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_token_by_hash(pool: &PgPool, hash: &str) -> Result<Option<(ApiToken, Option<User>)>, PdmError> {
    let token: Option<ApiToken> = sqlx::query_as::<_, ApiToken>(
        "SELECT id, org_id, user_id, name, scopes, expires_at, created_at
         FROM api_tokens WHERE token_hash = $1",
    )
    .bind(hash)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)?;

    let Some(t) = token else { return Ok(None) };
    let user = if let Some(uid) = t.user_id { get_user_by_id(pool, uid).await? } else { None };
    Ok(Some((t, user)))
}

pub async fn touch_token(pool: &PgPool, id: Uuid) -> Result<(), PdmError> {
    sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE id = $1").bind(id).execute(pool).await.map_err(PdmError::Db)?;
    Ok(())
}

// ── Projects ─────────────────────────────────────────────────────────────

pub async fn create_project(pool: &PgPool, org_id: Uuid, name: &str, description: &str) -> Result<Project, PdmError> {
    sqlx::query_as::<_, Project>(
        "INSERT INTO projects (org_id, name, description)
         VALUES ($1, $2, $3)
         RETURNING id, org_id, name, description, created_at",
    )
    .bind(org_id)
    .bind(name)
    .bind(description)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn list_projects(pool: &PgPool, org_id: Uuid) -> Result<Vec<Project>, PdmError> {
    sqlx::query_as::<_, Project>(
        "SELECT id, org_id, name, description, created_at
             FROM projects WHERE org_id = $1 ORDER BY created_at",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_project(pool: &PgPool, id: Uuid, org_id: Uuid) -> Result<Option<Project>, PdmError> {
    sqlx::query_as::<_, Project>(
        "SELECT id, org_id, name, description, created_at
             FROM projects WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

// ── Folders ──────────────────────────────────────────────────────────────

pub async fn create_folder(pool: &PgPool, org_id: Uuid, project_id: Uuid, parent_id: Option<Uuid>, name: &str) -> Result<Folder, PdmError> {
    sqlx::query_as::<_, Folder>(
        "INSERT INTO folders (org_id, project_id, parent_id, name)
         VALUES ($1, $2, $3, $4)
         RETURNING id, org_id, project_id, parent_id, name",
    )
    .bind(org_id)
    .bind(project_id)
    .bind(parent_id)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn list_folders(pool: &PgPool, project_id: Uuid) -> Result<Vec<Folder>, PdmError> {
    sqlx::query_as::<_, Folder>(
        "SELECT id, org_id, project_id, parent_id, name
             FROM folders WHERE project_id = $1 ORDER BY name",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(PdmError::Db)
}

// ── Items ────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn create_item(
    pool: &PgPool,
    org_id: Uuid,
    project_id: Uuid,
    folder_id: Option<Uuid>,
    part_number: &str,
    description: &str,
    classification: &str,
    owner_id: Option<Uuid>,
) -> Result<Item, PdmError> {
    sqlx::query_as::<_, Item>(
        "INSERT INTO items (org_id, project_id, folder_id, part_number, description, classification, owner_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, org_id, project_id, folder_id, part_number, description, classification, doc_id,
                   lifecycle_state, owner_id, created_at, updated_at",
    )
    .bind(org_id)
    .bind(project_id)
    .bind(folder_id)
    .bind(part_number)
    .bind(description)
    .bind(classification)
    .bind(owner_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate key") {
            PdmError::Conflict(format!("part number {part_number} already exists"))
        } else {
            PdmError::Db(e)
        }
    })
}

pub async fn get_item(pool: &PgPool, id: Uuid, org_id: Uuid) -> Result<Option<Item>, PdmError> {
    sqlx::query_as::<_, Item>(
        "SELECT id, org_id, project_id, folder_id, part_number, description, classification,
                    doc_id, lifecycle_state, owner_id, created_at, updated_at
             FROM items WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn list_items(pool: &PgPool, org_id: Uuid, project_id: Uuid) -> Result<Vec<Item>, PdmError> {
    sqlx::query_as::<_, Item>(
        "SELECT id, org_id, project_id, folder_id, part_number, description, classification,
                    doc_id, lifecycle_state, owner_id, created_at, updated_at
             FROM items WHERE org_id = $1 AND project_id = $2
             ORDER BY part_number",
    )
    .bind(org_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn search_items(pool: &PgPool, org_id: Uuid, project_id: Option<Uuid>, query: &str) -> Result<Vec<Item>, PdmError> {
    let sql = format!(
        "SELECT id, org_id, project_id, folder_id, part_number, description, classification,
                doc_id, lifecycle_state, owner_id, created_at, updated_at
         FROM items
         WHERE org_id = $1 AND (
             search_tsv @@ plainto_tsquery('english', $2)
             OR part_number ILIKE '%' || $2 || '%'
             OR description ILIKE '%' || $2 || '%'
         )
         {}
         ORDER BY part_number
         LIMIT 100",
        if project_id.is_some() { "AND project_id = $3" } else { "" }
    );

    let mut q = sqlx::query_as::<_, Item>(&sql).bind(org_id).bind(query);
    if let Some(pid) = project_id {
        q = q.bind(pid);
    }
    q.fetch_all(pool).await.map_err(PdmError::Db)
}

/// Atomically get and increment the next part-number sequence for a prefix.
pub async fn next_part_number(pool: &PgPool, org_id: Uuid, prefix: &str) -> Result<i64, PdmError> {
    let seq = sqlx::query_scalar::<_, i64>(
        "INSERT INTO number_sequences (org_id, prefix, next_value)
         VALUES ($1, $2, 1)
         ON CONFLICT (org_id, prefix)
         DO UPDATE SET next_value = number_sequences.next_value + 1
         RETURNING next_value",
    )
    .bind(org_id)
    .bind(prefix)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)?;
    Ok(seq)
}

// ── Files & Blobs ────────────────────────────────────────────────────────

pub async fn ensure_file(pool: &PgPool, item_id: Uuid, filename: &str, mime: &str) -> Result<FileRecord, PdmError> {
    // Try to find existing
    let existing: Option<FileRecord> =
        sqlx::query_as::<_, FileRecord>("SELECT id, item_id, filename, mime_type, created_at FROM files WHERE item_id = $1")
            .bind(item_id)
            .fetch_optional(pool)
            .await
            .map_err(PdmError::Db)?;

    if let Some(f) = existing {
        return Ok(f);
    }

    sqlx::query_as::<_, FileRecord>(
        "INSERT INTO files (item_id, filename, mime_type)
         VALUES ($1, $2, $3)
         RETURNING id, item_id, filename, mime_type, created_at",
    )
    .bind(item_id)
    .bind(filename)
    .bind(mime)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_file(pool: &PgPool, item_id: Uuid) -> Result<Option<FileRecord>, PdmError> {
    sqlx::query_as::<_, FileRecord>("SELECT id, item_id, filename, mime_type, created_at FROM files WHERE item_id = $1")
        .bind(item_id)
        .fetch_optional(pool)
        .await
        .map_err(PdmError::Db)
}

pub async fn upload_blob(pool: &PgPool, org_id: Uuid, sha256: &str, size_bytes: i64, storage_key: &str) -> Result<Blob, PdmError> {
    // Dedup: check if blob already exists
    let existing: Option<Blob> = sqlx::query_as::<_, Blob>(
        "SELECT id, org_id, sha256, size_bytes, storage_key, created_at
         FROM file_blobs WHERE org_id = $1 AND sha256 = $2",
    )
    .bind(org_id)
    .bind(sha256)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)?;

    if let Some(b) = existing {
        return Ok(b);
    }

    sqlx::query_as::<_, Blob>(
        "INSERT INTO file_blobs (org_id, sha256, size_bytes, storage_key)
         VALUES ($1, $2, $3, $4)
         RETURNING id, org_id, sha256, size_bytes, storage_key, created_at",
    )
    .bind(org_id)
    .bind(sha256)
    .bind(size_bytes)
    .bind(storage_key)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_blob_by_sha(pool: &PgPool, org_id: Uuid, sha256: &str) -> Result<Option<Blob>, PdmError> {
    sqlx::query_as::<_, Blob>(
        "SELECT id, org_id, sha256, size_bytes, storage_key, created_at
             FROM file_blobs WHERE org_id = $1 AND sha256 = $2",
    )
    .bind(org_id)
    .bind(sha256)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_blob_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Blob>, PdmError> {
    sqlx::query_as::<_, Blob>(
        "SELECT id, org_id, sha256, size_bytes, storage_key, created_at
             FROM file_blobs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

// ── File Versions ────────────────────────────────────────────────────────

pub async fn create_version(
    pool: &PgPool,
    file_id: Uuid,
    blob_id: Uuid,
    author_id: Uuid,
    commit_message: &str,
    base_version_id: Option<Uuid>,
    native_format: &str,
) -> Result<FileVersion, PdmError> {
    // Get current head seq
    let head_seq: Option<i32> =
        sqlx::query_scalar("SELECT MAX(seq) FROM file_versions WHERE file_id = $1").bind(file_id).fetch_one(pool).await.map_err(PdmError::Db)?;

    let next_seq = head_seq.map(|h| h + 1).unwrap_or(1);

    sqlx::query_as::<_, FileVersion>(
        "INSERT INTO file_versions (file_id, seq, blob_id, author_id, commit_message, base_version_id, native_format)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, file_id, seq, blob_id, author_id, committed_at, commit_message,
                   base_version_id, native_format, measurements",
    )
    .bind(file_id)
    .bind(next_seq)
    .bind(blob_id)
    .bind(author_id)
    .bind(commit_message)
    .bind(base_version_id)
    .bind(native_format)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_head_version(pool: &PgPool, file_id: Uuid) -> Result<Option<FileVersion>, PdmError> {
    sqlx::query_as::<_, FileVersion>(
        "SELECT id, file_id, seq, blob_id, author_id, committed_at, commit_message,
                    base_version_id, native_format, measurements
             FROM file_versions WHERE file_id = $1 ORDER BY seq DESC LIMIT 1",
    )
    .bind(file_id)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn list_versions(pool: &PgPool, file_id: Uuid) -> Result<Vec<FileVersion>, PdmError> {
    sqlx::query_as::<_, FileVersion>(
        "SELECT id, file_id, seq, blob_id, author_id, committed_at, commit_message,
                    base_version_id, native_format, measurements
             FROM file_versions WHERE file_id = $1 ORDER BY seq DESC",
    )
    .bind(file_id)
    .fetch_all(pool)
    .await
    .map_err(PdmError::Db)
}

/// Version history joined with the blob's SHA-256 and size (for listing + the portal).
pub async fn list_version_details(pool: &PgPool, file_id: Uuid) -> Result<Vec<pdm_core::VersionDetail>, PdmError> {
    sqlx::query_as::<_, pdm_core::VersionDetail>(
        "SELECT v.id, v.file_id, v.seq, v.blob_id, v.author_id, v.committed_at,
                v.commit_message, v.base_version_id, v.native_format,
                b.sha256, b.size_bytes
         FROM file_versions v
         JOIN file_blobs b ON b.id = v.blob_id
         WHERE v.file_id = $1
         ORDER BY v.seq DESC",
    )
    .bind(file_id)
    .fetch_all(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_version(pool: &PgPool, id: Uuid) -> Result<Option<FileVersion>, PdmError> {
    sqlx::query_as::<_, FileVersion>(
        "SELECT id, file_id, seq, blob_id, author_id, committed_at, commit_message,
                    base_version_id, native_format, measurements
             FROM file_versions WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn get_version_by_seq(pool: &PgPool, file_id: Uuid, seq: i32) -> Result<Option<FileVersion>, PdmError> {
    sqlx::query_as::<_, FileVersion>(
        "SELECT id, file_id, seq, blob_id, author_id, committed_at, commit_message,
                    base_version_id, native_format, measurements
             FROM file_versions WHERE file_id = $1 AND seq = $2",
    )
    .bind(file_id)
    .bind(seq)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

// ── Locks ────────────────────────────────────────────────────────────────

pub async fn acquire_lock(
    pool: &PgPool,
    item_id: Uuid,
    user_id: Uuid,
    workstation: &str,
    base_version_id: Option<Uuid>,
    lease_hours: i64,
) -> Result<FileLock, PdmError> {
    let lease = Duration::hours(lease_hours);
    let row = sqlx::query_as::<_, FileLock>(
        "INSERT INTO file_locks (item_id, user_id, workstation, base_version_id, lease_expires_at)
         VALUES ($1, $2, $3, $4, now() + $5)
         ON CONFLICT (item_id) DO UPDATE
           SET user_id = EXCLUDED.user_id,
               workstation = EXCLUDED.workstation,
               base_version_id = EXCLUDED.base_version_id,
               acquired_at = now(),
               lease_expires_at = EXCLUDED.lease_expires_at
           WHERE file_locks.lease_expires_at < now() OR file_locks.user_id = EXCLUDED.user_id
         RETURNING id, item_id, user_id, workstation, base_version_id, acquired_at, lease_expires_at",
    )
    .bind(item_id)
    .bind(user_id)
    .bind(workstation)
    .bind(base_version_id)
    .bind(lease)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)?;

    match row {
        Some(lock) => Ok(lock),
        None => Err(PdmError::Conflict("item is locked by another user".into())),
    }
}

pub async fn release_lock(pool: &PgPool, item_id: Uuid, user_id: Uuid) -> Result<(), PdmError> {
    let result = sqlx::query("DELETE FROM file_locks WHERE item_id = $1 AND user_id = $2")
        .bind(item_id)
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(PdmError::Db)?;
    if result.rows_affected() == 0 {
        return Err(PdmError::Conflict("lock not found or owned by another user".into()));
    }
    Ok(())
}

pub async fn get_lock(pool: &PgPool, item_id: Uuid) -> Result<Option<FileLock>, PdmError> {
    sqlx::query_as::<_, FileLock>(
        "SELECT id, item_id, user_id, workstation, base_version_id, acquired_at, lease_expires_at
             FROM file_locks WHERE item_id = $1 AND lease_expires_at > now()",
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

// ── Audit ────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn record_audit(
    pool: &PgPool,
    org_id: Uuid,
    actor_id: Option<Uuid>,
    agent_id: &str,
    action: &str,
    object_type: &str,
    object_id: Option<Uuid>,
    detail: Option<serde_json::Value>,
) -> Result<AuditEvent, PdmError> {
    sqlx::query_as::<_, AuditEvent>(
        "INSERT INTO audit_events (org_id, actor_id, agent_id, action, object_type, object_id, detail)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, org_id, actor_id, agent_id, action, object_type, object_id, detail, at",
    )
    .bind(org_id)
    .bind(actor_id)
    .bind(agent_id)
    .bind(action)
    .bind(object_type)
    .bind(object_id)
    .bind(detail)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn list_audit(pool: &PgPool, org_id: Uuid, object_id: Option<Uuid>, limit: u32) -> Result<Vec<AuditEvent>, PdmError> {
    let sql = format!(
        "SELECT id, org_id, actor_id, agent_id, action, object_type, object_id, detail, at
         FROM audit_events
         WHERE org_id = $1 {}
         ORDER BY at DESC
         LIMIT {}",
        if object_id.is_some() { "AND object_id = $2" } else { "" },
        limit
    );

    let mut q = sqlx::query_as::<_, AuditEvent>(&sql).bind(org_id);
    if let Some(oid) = object_id {
        q = q.bind(oid);
    }
    q.fetch_all(pool).await.map_err(PdmError::Db)
}

// ── Jobs ─────────────────────────────────────────────────────────────────

pub async fn enqueue_job(pool: &PgPool, org_id: Uuid, kind: &str, payload: &serde_json::Value) -> Result<Job, PdmError> {
    sqlx::query_as::<_, Job>(
        "INSERT INTO jobs (org_id, kind, payload) VALUES ($1, $2, $3)
         RETURNING id, org_id, kind, payload, status, attempts, max_attempts, last_error, created_at",
    )
    .bind(org_id)
    .bind(kind)
    .bind(payload)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn claim_jobs(pool: &PgPool, worker_id: &str, limit: i64) -> Result<Vec<Job>, PdmError> {
    sqlx::query_as::<_, Job>(
        "UPDATE jobs SET status = 'running', locked_by = $1, updated_at = now()
             WHERE id IN (
                 SELECT id FROM jobs
                 WHERE status = 'queued' AND run_after <= now()
                 ORDER BY run_after
                 LIMIT $2
                 FOR UPDATE SKIP LOCKED
             )
             RETURNING id, org_id, kind, payload, status, attempts, max_attempts, last_error, created_at",
    )
    .bind(worker_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn complete_job(pool: &PgPool, job_id: Uuid, ok: bool, error: &str) -> Result<(), PdmError> {
    let new_status = if ok { "done" } else { "failed" };
    sqlx::query(
        "UPDATE jobs SET status = $2, last_error = $3, updated_at = now()
         WHERE id = $1",
    )
    .bind(job_id)
    .bind(new_status)
    .bind(error)
    .execute(pool)
    .await
    .map_err(PdmError::Db)?;
    Ok(())
}

pub async fn get_job(pool: &PgPool, id: Uuid, org_id: Uuid) -> Result<Option<Job>, PdmError> {
    sqlx::query_as::<_, Job>(
        "SELECT id, org_id, kind, payload, status, attempts, max_attempts, last_error, created_at
             FROM jobs WHERE id = $1 AND org_id = $2",
    )
    .bind(id)
    .bind(org_id)
    .fetch_optional(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn record_job_attempt(pool: &PgPool, job_id: Uuid, ok: bool, error: &str) -> Result<(), PdmError> {
    sqlx::query("INSERT INTO job_attempts (job_id, ended_at, ok, error) VALUES ($1, now(), $2, $3)")
        .bind(job_id)
        .bind(ok)
        .bind(error)
        .execute(pool)
        .await
        .map_err(PdmError::Db)?;
    // Also increment attempts on the job
    sqlx::query("UPDATE jobs SET attempts = attempts + 1 WHERE id = $1").bind(job_id).execute(pool).await.map_err(PdmError::Db)?;
    Ok(())
}

// ── Properties ───────────────────────────────────────────────────────────

pub async fn upsert_property(pool: &PgPool, item_id: Uuid, key: &str, value_type: &str, value: &serde_json::Value) -> Result<Property, PdmError> {
    sqlx::query_as::<_, Property>(
        "INSERT INTO properties (item_id, prop_key, value_type, value)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (item_id, prop_key)
         DO UPDATE SET value_type = EXCLUDED.value_type, value = EXCLUDED.value, updated_at = now()
         RETURNING id, item_id, prop_key, value_type, value, computed, updated_at",
    )
    .bind(item_id)
    .bind(key)
    .bind(value_type)
    .bind(value)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}

pub async fn list_properties(pool: &PgPool, item_id: Uuid) -> Result<Vec<Property>, PdmError> {
    sqlx::query_as::<_, Property>(
        "SELECT id, item_id, prop_key, value_type, value, computed, updated_at
             FROM properties WHERE item_id = $1 ORDER BY prop_key",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(PdmError::Db)
}

// ── Revisions ────────────────────────────────────────────────────────────

pub async fn create_revision(
    pool: &PgPool,
    item_id: Uuid,
    label: &str,
    version_id: Uuid,
    released_by: Option<Uuid>,
    release_note: &str,
    scheme: &str,
) -> Result<FileRevision, PdmError> {
    sqlx::query_as::<_, FileRevision>(
        "INSERT INTO file_revisions (item_id, label, version_id, released_by, release_note, scheme)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, item_id, label, version_id, released_by, released_at, release_note, scheme",
    )
    .bind(item_id)
    .bind(label)
    .bind(version_id)
    .bind(released_by)
    .bind(release_note)
    .bind(scheme)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate key") { PdmError::Conflict(format!("revision label '{label}' already exists")) } else { PdmError::Db(e) }
    })
}

pub async fn list_revisions(pool: &PgPool, item_id: Uuid) -> Result<Vec<FileRevision>, PdmError> {
    sqlx::query_as::<_, FileRevision>(
        "SELECT id, item_id, label, version_id, released_by, released_at, release_note, scheme
             FROM file_revisions WHERE item_id = $1 ORDER BY released_at",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(PdmError::Db)
}

// ── Materials ────────────────────────────────────────────────────────────

pub async fn get_material(pool: &PgPool, org_id: Uuid, name: &str) -> Result<Option<Material>, PdmError> {
    sqlx::query_as::<_, Material>("SELECT id, org_id, name, density FROM materials WHERE org_id = $1 AND name = $2")
        .bind(org_id)
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(PdmError::Db)
}

pub async fn upsert_material(pool: &PgPool, org_id: Uuid, name: &str, density: Option<f64>) -> Result<Material, PdmError> {
    sqlx::query_as::<_, Material>(
        "INSERT INTO materials (org_id, name, density)
         VALUES ($1, $2, $3)
         ON CONFLICT (org_id, name)
         DO UPDATE SET density = EXCLUDED.density
         RETURNING id, org_id, name, density",
    )
    .bind(org_id)
    .bind(name)
    .bind(density)
    .fetch_one(pool)
    .await
    .map_err(PdmError::Db)
}
