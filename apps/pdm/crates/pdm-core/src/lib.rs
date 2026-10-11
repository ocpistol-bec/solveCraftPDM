//! PDM core: domain entities, errors, and pure business logic.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use thiserror::Error;
use uuid::Uuid;

// ── Error ────────────────────────────────────────────────────────────────

#[derive(Error, Debug)]
pub enum PdmError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("authentication failed")]
    AuthFailed,
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("integrity mismatch: expected {expected}, got {actual}")]
    IntegrityMismatch { expected: String, actual: String },
    #[error("validation error: {0}")]
    Validation(String),
    #[error("database error: {0}")]
    Db(#[source] sqlx::Error),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl PdmError {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::Conflict(_) => 409,
            Self::AuthFailed => 401,
            Self::Forbidden(_) => 403,
            Self::IntegrityMismatch { .. } => 422,
            Self::Validation(_) => 422,
            Self::Db(_) => 500,
            Self::Storage(_) => 502,
            Self::Internal(_) => 500,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::AuthFailed => "auth_failed",
            Self::Forbidden(_) => "forbidden",
            Self::IntegrityMismatch { .. } => "integrity_mismatch",
            Self::Validation(_) => "validation",
            Self::Db(_) => "db_error",
            Self::Storage(_) => "storage_error",
            Self::Internal(_) => "internal",
        }
    }
}

// ── Entities ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub org_id: Uuid,
    pub email: String,
    pub display_name: String,
    /// 'viewer' | 'editor' | 'admin' (Phase-1 simplification of the roles model).
    pub role: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiToken {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Option<Uuid>,
    pub name: String,
    pub scopes: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Folder {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Item {
    pub id: Uuid,
    pub org_id: Uuid,
    pub project_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub part_number: String,
    pub description: String,
    pub classification: String,
    pub doc_id: Option<Uuid>,
    pub lifecycle_state: String,
    pub owner_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileRecord {
    pub id: Uuid,
    pub item_id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Blob {
    pub id: Uuid,
    pub org_id: Uuid,
    pub sha256: String,
    pub size_bytes: i64,
    pub storage_key: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileVersion {
    pub id: Uuid,
    pub file_id: Uuid,
    pub seq: i32,
    pub blob_id: Uuid,
    pub author_id: Uuid,
    pub committed_at: DateTime<Utc>,
    pub commit_message: String,
    pub base_version_id: Option<Uuid>,
    pub native_format: String,
    pub measurements: Option<serde_json::Value>,
}

/// A version with its blob's SHA-256 and byte size joined in, for listing and the portal.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct VersionDetail {
    pub id: Uuid,
    pub file_id: Uuid,
    pub seq: i32,
    pub blob_id: Uuid,
    pub author_id: Uuid,
    pub committed_at: DateTime<Utc>,
    pub commit_message: String,
    pub base_version_id: Option<Uuid>,
    pub native_format: String,
    pub sha256: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileRevision {
    pub id: Uuid,
    pub item_id: Uuid,
    pub label: String,
    pub version_id: Uuid,
    pub released_by: Option<Uuid>,
    pub released_at: DateTime<Utc>,
    pub release_note: String,
    pub scheme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FileLock {
    pub id: Uuid,
    pub item_id: Uuid,
    pub user_id: Uuid,
    pub workstation: String,
    pub base_version_id: Option<Uuid>,
    pub acquired_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AuditEvent {
    pub id: i64,
    pub org_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub agent_id: String,
    pub action: String,
    pub object_type: String,
    pub object_id: Option<Uuid>,
    pub detail: Option<serde_json::Value>,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Job {
    pub id: Uuid,
    pub org_id: Uuid,
    pub kind: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub last_error: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Property {
    pub id: Uuid,
    pub item_id: Uuid,
    pub prop_key: String,
    pub value_type: String,
    pub value: serde_json::Value,
    pub computed: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Material {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub density: Option<f64>,
}

// ── Part numbering ───────────────────────────────────────────────────────

/// Generate a part number like "BE-000001" from a prefix and sequence.
pub fn generate_part_number(prefix: &str, seq: i64) -> String {
    format!("{prefix}-{seq:06}")
}

// ── Versioning rules ─────────────────────────────────────────────────────

/// Validate that a check-in's base version is consistent with the current head.
///
/// - `None, None` → first version ever, OK.
/// - `Some(b), Some(h)` → must be equal, otherwise stale.
/// - `None, Some(_)`  → client claims no base but head exists → stale.
/// - `Some(_), None`  → client has a base but no head yet (shouldn't happen) → OK.
pub fn validate_checkin(base_seq: Option<i32>, head_seq: Option<i32>) -> Result<(), PdmError> {
    match (base_seq, head_seq) {
        (None, None) => Ok(()),
        (Some(b), Some(h)) => {
            if b == h {
                Ok(())
            } else {
                Err(PdmError::Conflict(format!("stale base version {b}, current head is {h}")))
            }
        }
        (None, Some(h)) => Err(PdmError::Conflict(format!("no base version supplied but head is {h}"))),
        (Some(_), None) => Ok(()),
    }
}

/// Default lock lease duration.
pub fn default_lease() -> chrono::Duration {
    chrono::Duration::hours(24)
}

/// Check whether a lock has expired.
pub fn lock_expired(lock: &FileLock, now: &DateTime<Utc>) -> bool {
    &lock.lease_expires_at < now
}

// ── Lifecycle states ─────────────────────────────────────────────────────

pub const LIFECYCLE_STATES: &[&str] = &["wip", "in_review", "approved", "released", "obsolete"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_number_format() {
        assert_eq!(generate_part_number("BE", 1), "BE-000001");
        assert_eq!(generate_part_number("ASM", 42), "ASM-000042");
        assert_eq!(generate_part_number("MFG", 1000000), "MFG-1000000");
    }

    #[test]
    fn checkin_first_version() {
        assert!(validate_checkin(None, None).is_ok());
    }

    #[test]
    fn checkin_matching_base() {
        assert!(validate_checkin(Some(1), Some(1)).is_ok());
        assert!(validate_checkin(Some(5), Some(5)).is_ok());
    }

    #[test]
    fn checkin_stale_base() {
        let err = validate_checkin(Some(1), Some(2)).unwrap_err();
        assert!(matches!(err, PdmError::Conflict(_)));
        assert!(err.to_string().contains("stale"));
    }

    #[test]
    fn checkin_no_base_with_head() {
        let err = validate_checkin(None, Some(3)).unwrap_err();
        assert!(matches!(err, PdmError::Conflict(_)));
    }

    #[test]
    fn lock_expiry() {
        let now = Utc::now();
        let active = FileLock {
            id: Uuid::new_v4(),
            item_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            workstation: String::new(),
            base_version_id: None,
            acquired_at: now,
            lease_expires_at: now + chrono::Duration::hours(1),
        };
        assert!(!lock_expired(&active, &now));

        let expired = FileLock { lease_expires_at: now - chrono::Duration::seconds(1), ..active.clone() };
        assert!(lock_expired(&expired, &now));
    }

    #[test]
    fn error_status_codes() {
        assert_eq!(PdmError::NotFound("x".into()).status_code(), 404);
        assert_eq!(PdmError::Conflict("x".into()).status_code(), 409);
        assert_eq!(PdmError::AuthFailed.status_code(), 401);
        assert_eq!(PdmError::Forbidden("x".into()).status_code(), 403);
        assert_eq!(PdmError::IntegrityMismatch { expected: "a".into(), actual: "b".into() }.status_code(), 422);
        assert_eq!(PdmError::Validation("x".into()).status_code(), 422);
        assert_eq!(PdmError::Db(sqlx::Error::RowNotFound).status_code(), 500);
        assert_eq!(PdmError::Storage("x".into()).status_code(), 502);
        assert_eq!(PdmError::Internal("x".into()).status_code(), 500);
    }
}
