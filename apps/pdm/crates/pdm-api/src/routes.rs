use axum::extract::{Extension, Path, Query, State};
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use bytes::Bytes;
use serde::Deserialize;
use uuid::Uuid;

use pdm_core::PdmError;

use crate::auth::{Authed, require_user};
use crate::error::AppError;
use crate::state::AppState;

// ── Request body types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateProjectReq {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateItemReq {
    pub part_number: Option<String>,
    pub description: Option<String>,
    pub classification: Option<String>,
}

#[derive(Deserialize)]
pub struct CheckoutReq {
    pub workstation: Option<String>,
}

#[derive(Deserialize)]
pub struct CheckinReq {
    pub sha256: String,
    pub commit_message: Option<String>,
    pub base_version: Option<i32>,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub project: Option<Uuid>,
}

#[derive(Deserialize)]
pub struct SetPropertyReq {
    pub value: serde_json::Value,
    pub value_type: Option<String>,
}

#[derive(Deserialize)]
pub struct ReleaseReq {
    pub label: Option<String>,
    pub note: Option<String>,
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn require_org(authed: &Authed) -> Uuid {
    authed.org_id
}

// ── Handlers ────────────────────────────────────────────────────────────

/// GET /healthz
pub async fn healthz() -> &'static str {
    "ok"
}

/// GET /readyz — checks DB connectivity.
pub async fn readyz(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let db_ok = sqlx::query("SELECT 1").execute(&state.pool).await.is_ok();
    Ok(serde_json::json!({ "ready": db_ok, "db": db_ok }).into())
}

/// GET /me
pub async fn me(Extension(authed): Extension<Authed>) -> Result<Json<serde_json::Value>, AppError> {
    let user = authed.user.clone();
    Ok(serde_json::json!({
        "org_id": authed.org_id,
        "user": user,
        "scopes": authed.scopes
    })
    .into())
}

// ── Projects ────────────────────────────────────────────────────────────

/// GET /projects
pub async fn list_projects(State(state): State<AppState>, Extension(authed): Extension<Authed>) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let projects = pdm_db::list_projects(&state.pool, org).await?;
    Ok(serde_json::json!({ "projects": projects }).into())
}

/// POST /projects
pub async fn create_project(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Json(body): Json<CreateProjectReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let desc = body.description.unwrap_or_default();
    let project = pdm_db::create_project(&state.pool, org, &body.name, &desc).await?;
    Ok(serde_json::json!({ "project": project }).into())
}

// ── Items ───────────────────────────────────────────────────────────────

/// POST /projects/{project_id}/items
pub async fn create_item(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateItemReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let user = require_user(&authed)?.clone();
    let classification = body.classification.unwrap_or_else(|| "BE".to_string());

    let part_number = if let Some(pn) = &body.part_number {
        pn.clone()
    } else {
        let seq = pdm_db::next_part_number(&state.pool, org, &classification).await?;
        pdm_core::generate_part_number(&classification, seq)
    };

    let desc = body.description.unwrap_or_default();
    let item = pdm_db::create_item(&state.pool, org, project_id, None, &part_number, &desc, &classification, Some(user.id)).await?;

    pdm_db::record_audit(
        &state.pool,
        org,
        Some(user.id),
        "",
        "item.created",
        "item",
        Some(item.id),
        Some(serde_json::json!({ "part_number": item.part_number })),
    )
    .await?;

    Ok(serde_json::json!({ "item": item }).into())
}

/// GET /items/{item_id}
pub async fn get_item(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(item_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let item = pdm_db::get_item(&state.pool, item_id, org).await?.ok_or_else(|| AppError(PdmError::NotFound(format!("item {item_id}"))))?;
    let file = pdm_db::get_file(&state.pool, item_id).await?;
    let lock = pdm_db::get_lock(&state.pool, item_id).await?;
    let versions = match &file {
        Some(f) => pdm_db::list_version_details(&state.pool, f.id).await?,
        None => vec![],
    };
    let head_version = versions.first().cloned();

    Ok(serde_json::json!({
        "item": item,
        "file": file,
        "lock": lock,
        "versions": versions,
        "head_version": head_version,
    })
    .into())
}

/// GET /projects/{project_id}/items
pub async fn list_items(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let items = pdm_db::list_items(&state.pool, org, project_id).await?;
    Ok(serde_json::json!({ "items": items }).into())
}

// ── Checkout / Checkin ──────────────────────────────────────────────────

/// POST /items/{item_id}/checkout
pub async fn checkout(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(item_id): Path<Uuid>,
    Json(body): Json<CheckoutReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let user = require_user(&authed)?.clone();
    let workstation = body.workstation.unwrap_or_else(|| "unknown".to_string());

    // The file record may not exist before the first check-in.
    let file = pdm_db::get_file(&state.pool, item_id).await?;
    let head = match &file {
        Some(f) => pdm_db::get_head_version(&state.pool, f.id).await?,
        None => None,
    };
    let base_version_id = head.as_ref().map(|v| v.id);

    let lock = pdm_db::acquire_lock(&state.pool, item_id, user.id, &workstation, base_version_id, state.lease_hours).await?;

    pdm_db::record_audit(
        &state.pool,
        org,
        Some(user.id),
        "",
        "item.checkout",
        "item",
        Some(item_id),
        Some(serde_json::json!({ "workstation": workstation, "base_version_id": base_version_id })),
    )
    .await?;

    Ok(serde_json::json!({
        "lock": lock,
        "base_version": head,
    })
    .into())
}

/// DELETE /items/{item_id}/checkout
pub async fn undo_checkout(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(item_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let user = require_user(&authed)?.clone();
    pdm_db::release_lock(&state.pool, item_id, user.id).await?;

    pdm_db::record_audit(&state.pool, org, Some(user.id), "", "item.undo_checkout", "item", Some(item_id), None).await?;

    Ok(serde_json::json!({ "ok": true }).into())
}

/// POST /items/{item_id}/checkin
pub async fn checkin(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(item_id): Path<Uuid>,
    Json(body): Json<CheckinReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let user = require_user(&authed)?.clone();

    // 1. Verify lock is held by this user
    let lock = pdm_db::get_lock(&state.pool, item_id).await?.ok_or_else(|| AppError(PdmError::Conflict("no active checkout".into())))?;
    if lock.user_id != user.id {
        return Err(AppError(PdmError::Conflict("lock held by another user".into())));
    }

    // 2. Verify base version. Ensure the file record exists (created on first check-in).
    let filename = body.filename.clone().unwrap_or_default();
    let mime = body.mime_type.clone().unwrap_or_else(|| "application/octet-stream".to_string());
    let file = pdm_db::ensure_file(&state.pool, item_id, &filename, &mime).await?;
    let head = pdm_db::get_head_version(&state.pool, file.id).await?;
    pdm_core::validate_checkin(body.base_version, head.as_ref().map(|v| v.seq)).map_err(AppError)?;

    // 3. Verify blob integrity
    let blob = pdm_db::get_blob_by_sha(&state.pool, org, &body.sha256)
        .await?
        .ok_or_else(|| AppError(PdmError::IntegrityMismatch { expected: body.sha256.clone(), actual: "not_found".into() }))?;
    state
        .store
        .verify_by_sha(&body.sha256)
        .await
        .map_err(|e| AppError(PdmError::IntegrityMismatch { expected: body.sha256.clone(), actual: e.to_string() }))?;

    // 4. Create version
    let commit_msg = body.commit_message.unwrap_or_default();
    let version = pdm_db::create_version(&state.pool, file.id, blob.id, user.id, &commit_msg, head.as_ref().map(|v| v.id), "").await?;

    // 4b. Enqueue background analysis (measurements via `solvecraft-cli eval`); the
    //     worker stores the result on the version when it succeeds.
    let _ = pdm_db::enqueue_job(&state.pool, org, "design.analyze", &serde_json::json!({ "version_id": version.id, "item_id": item_id })).await;

    // 5. Release lock
    pdm_db::release_lock(&state.pool, item_id, user.id).await?;

    // 6. Audit
    pdm_db::record_audit(
        &state.pool,
        org,
        Some(user.id),
        "",
        "item.checkin",
        "item",
        Some(item_id),
        Some(serde_json::json!({
            "version_seq": version.seq,
            "commit_message": commit_msg,
            "sha256": body.sha256
        })),
    )
    .await?;

    Ok(serde_json::json!({ "version": version }).into())
}

// ── Blob upload / download ──────────────────────────────────────────────

/// PUT /blobs — upload raw bytes, content-address by SHA-256.
pub async fn upload_blob(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    if body.len() as u64 > state.max_upload {
        return Err(AppError(PdmError::Validation(format!("upload size {} exceeds limit {}", body.len(), state.max_upload))));
    }
    if body.is_empty() {
        return Err(AppError(PdmError::Validation("empty upload".into())));
    }

    let (sha, size) = state.store.put_content_addressed(&body).await.map_err(|e| AppError(PdmError::Storage(e.to_string())))?;
    let storage_key = sha.clone();

    let blob =
        pdm_db::upload_blob(&state.pool, org, &sha, i64::try_from(size).map_err(|_| PdmError::Validation("file too large".into()))?, &storage_key)
            .await?;

    Ok(serde_json::json!({ "blob": blob }).into())
}

/// GET /versions/{version_id}/blob
pub async fn download_version_blob(State(state): State<AppState>, Path(version_id): Path<Uuid>) -> Result<Response, AppError> {
    let version = pdm_db::get_version(&state.pool, version_id).await?.ok_or_else(|| AppError(PdmError::NotFound(format!("version {version_id}"))))?;

    let blob = pdm_db::get_blob_by_id(&state.pool, version.blob_id).await?.ok_or_else(|| AppError(PdmError::NotFound("blob for version".into())))?;

    let data = state.store.get_by_sha(&blob.sha256).await.map_err(|e| AppError(PdmError::Storage(e.to_string())))?;

    let digest = format!("sha-256={}", blob.sha256);
    let response = axum::http::Response::builder()
        .status(200)
        .header("content-type", "application/octet-stream")
        .header("content-length", data.len().to_string())
        .header("content-digest", digest)
        .body(axum::body::Body::from(data))
        .map_err(|e| AppError(PdmError::Internal(format!("response build: {e}"))))?;

    Ok(response)
}

// ── Versions ────────────────────────────────────────────────────────────

/// GET /items/{item_id}/versions
pub async fn list_item_versions(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(item_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _org = require_org(&authed);
    let file = pdm_db::get_file(&state.pool, item_id).await?;
    let versions = match file {
        Some(f) => pdm_db::list_version_details(&state.pool, f.id).await?,
        None => vec![],
    };
    Ok(serde_json::json!({ "versions": versions }).into())
}

// ── Search ──────────────────────────────────────────────────────────────

/// GET /search?q=...&project=...
pub async fn search(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let results = pdm_db::search_items(&state.pool, org, q.project, &q.q).await?;
    Ok(serde_json::json!({ "results": results }).into())
}

// ── Audit ───────────────────────────────────────────────────────────────

/// GET /audit?item=...&limit=...
#[derive(Deserialize)]
pub struct AuditQuery {
    pub item: Option<Uuid>,
    pub limit: Option<u32>,
}

pub async fn audit(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let limit = q.limit.unwrap_or(50).min(200);
    let events = pdm_db::list_audit(&state.pool, org, q.item, limit).await?;
    Ok(serde_json::json!({ "events": events }).into())
}

// ── Properties ──────────────────────────────────────────────────────────

/// GET /items/{item_id}/properties
pub async fn get_properties(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(item_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _org = require_org(&authed);
    let props = pdm_db::list_properties(&state.pool, item_id).await?;
    Ok(serde_json::json!({ "properties": props }).into())
}

/// PUT /items/{item_id}/properties/{key}
pub async fn set_property(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path((item_id, key)): Path<(Uuid, String)>,
    Json(body): Json<SetPropertyReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _org = require_org(&authed);
    let _user = require_user(&authed)?;
    let value_type = body.value_type.unwrap_or_else(|| "text".to_string());
    let prop = pdm_db::upsert_property(&state.pool, item_id, &key, &value_type, &body.value).await?;
    Ok(serde_json::json!({ "property": prop }).into())
}

// ── Release ─────────────────────────────────────────────────────────────

/// POST /items/{item_id}/release
pub async fn release(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(item_id): Path<Uuid>,
    Json(body): Json<ReleaseReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let org = require_org(&authed);
    let user = require_user(&authed)?.clone();

    let file = pdm_db::get_file(&state.pool, item_id).await?.ok_or_else(|| AppError(PdmError::NotFound("file not found".into())))?;
    let head = pdm_db::get_head_version(&state.pool, file.id).await?.ok_or_else(|| AppError(PdmError::NotFound("no versions".into())))?;

    let label = if let Some(l) = body.label {
        l
    } else {
        // Auto-generate: A, B, C...
        let existing = pdm_db::list_revisions(&state.pool, item_id).await.map(|r| r.len()).unwrap_or(0);
        (b'A' + existing as u8).to_string()
    };

    let note = body.note.unwrap_or_default();
    let revision = pdm_db::create_revision(&state.pool, item_id, &label, head.id, Some(user.id), &note, "alpha").await?;

    pdm_db::record_audit(
        &state.pool,
        org,
        Some(user.id),
        "",
        "item.release",
        "item",
        Some(item_id),
        Some(serde_json::json!({ "revision": revision.label, "version_seq": head.seq })),
    )
    .await?;

    Ok(serde_json::json!({ "revision": revision }).into())
}

// ── Users / Admin ───────────────────────────────────────────────────────

const ROLE_LEVEL: &[(&str, u8)] = &[("viewer", 0), ("editor", 1), ("admin", 2)];

fn role_level(role: &str) -> u8 {
    ROLE_LEVEL.iter().find(|(r, _)| *r == role).map(|(_, l)| *l).unwrap_or(0)
}

fn require_role<'a>(authed: &'a Authed, min_role: &str) -> Result<&'a pdm_core::User, AppError> {
    let user = require_user(authed)?;
    let min = role_level(min_role);
    if role_level(&user.role) < min {
        return Err(AppError(PdmError::Forbidden(format!("requires role '{}' but user has '{}'", min_role, user.role))));
    }
    Ok(user)
}

#[derive(Deserialize)]
pub struct CreateUserReq {
    pub email: String,
    pub display_name: Option<String>,
    pub role: Option<String>,
}

/// POST /users — admin creates a user in the current org.
pub async fn create_user_handler(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Json(body): Json<CreateUserReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let admin = require_role(&authed, "admin")?;
    let org = require_org(&authed);
    let role = body.role.as_deref().unwrap_or("editor");
    let display = body.display_name.as_deref().unwrap_or("");
    let user = pdm_db::create_user(&state.pool, org, &body.email, display, role).await?;
    pdm_db::record_audit(
        &state.pool,
        org,
        Some(admin.id),
        "",
        "user.created",
        "user",
        Some(user.id),
        Some(serde_json::json!({ "email": user.email, "role": role })),
    )
    .await?;
    Ok(serde_json::json!({ "user": user }).into())
}

#[derive(Deserialize)]
pub struct CreateTokenReq {
    pub name: Option<String>,
}

/// POST /users/{user_id}/tokens — admin creates an API token for a user.
pub async fn create_token_handler(
    State(state): State<AppState>,
    Extension(authed): Extension<Authed>,
    Path(user_id): Path<Uuid>,
    Json(body): Json<CreateTokenReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let admin = require_role(&authed, "admin")?;
    let org = require_org(&authed);
    let target = pdm_db::get_user_by_id(&state.pool, user_id).await?.ok_or_else(|| AppError(PdmError::NotFound(format!("user {user_id}"))))?;
    if target.org_id != org {
        return Err(AppError(PdmError::Forbidden("user not in this org".into())));
    }
    let token_value = format!("pdm_{}", Uuid::new_v4());
    let hash = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(token_value.as_bytes());
        hex::encode(h.finalize())
    };
    let name = body.name.as_deref().unwrap_or("api-token");
    let _tok = pdm_db::create_token(&state.pool, org, Some(user_id), name, &hash, "full", None).await?;
    pdm_db::record_audit(
        &state.pool,
        org,
        Some(admin.id),
        "",
        "user.token_created",
        "user",
        Some(user_id),
        Some(serde_json::json!({ "token_name": name })),
    )
    .await?;
    Ok(serde_json::json!({ "token": token_value, "user_id": user_id }).into())
}

// ── Router ──────────────────────────────────────────────────────────────

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/me", get(me))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{project_id}/items", get(list_items).post(create_item))
        .route("/items/{item_id}", get(get_item))
        .route("/items/{item_id}/checkout", post(checkout).delete(undo_checkout))
        .route("/items/{item_id}/checkin", post(checkin))
        .route("/items/{item_id}/versions", get(list_item_versions))
        .route("/items/{item_id}/properties", get(get_properties))
        .route("/items/{item_id}/properties/{key}", put(set_property))
        .route("/items/{item_id}/release", post(release))
        .route("/blobs", put(upload_blob))
        .route("/versions/{version_id}/blob", get(download_version_blob))
        .route("/search", get(search))
        .route("/audit", get(audit))
        .route("/users", post(create_user_handler))
        .route("/users/{user_id}/tokens", post(create_token_handler))
}
