//! Bearer-token authentication middleware.
//!
//! Every non-public request must carry `Authorization: Bearer <token>`. The token is
//! SHA-256-hashed and looked up in `api_tokens`; the owning user and organization are
//! resolved and injected as an `Authed` extension for the handler.
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use pdm_core::User;

use crate::error::AppError;
use crate::state::AppState;

/// Authenticated caller, injected into request extensions by [`auth_middleware`].
#[derive(Clone)]
pub struct Authed {
    pub org_id: Uuid,
    pub user: Option<User>,
    pub token_id: Uuid,
    pub scopes: String,
}

impl Authed {
    pub fn user_id(&self) -> Option<Uuid> {
        self.user.as_ref().map(|u| u.id)
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

/// Public paths that skip authentication.
fn is_public(path: &str) -> bool {
    path == "/healthz" || path == "/readyz" || path == "/" || path.starts_with("/static/")
}

/// Axum middleware: validate the Bearer token, inject `Authed`.
pub async fn auth_middleware(State(state): State<AppState>, req: Request, next: Next) -> Result<Response, AppError> {
    let path = req.uri().path().to_string();
    if is_public(&path) {
        return Ok(next.run(req).await);
    }

    let header = req.headers().get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    let Some(header) = header else {
        return Err(AppError(pdm_core::PdmError::AuthFailed));
    };
    let Some(token) = header.strip_prefix("Bearer ") else {
        return Err(AppError(pdm_core::PdmError::AuthFailed));
    };

    let hash = sha256_hex(token.as_bytes());
    let Some((tok, user)) = pdm_db::get_token_by_hash(&state.pool, &hash).await? else {
        return Err(AppError(pdm_core::PdmError::AuthFailed));
    };
    if tok.expires_at.is_some_and(|exp| exp < Utc::now()) {
        return Err(AppError(pdm_core::PdmError::AuthFailed));
    }
    let user = match user {
        Some(u) => {
            if !u.active {
                return Err(AppError(pdm_core::PdmError::Forbidden("user is inactive".into())));
            }
            Some(u)
        }
        None => None,
    };

    // Fire-and-forget last-used update; do not fail the request on it.
    {
        let pool = state.pool.clone();
        let id = tok.id;
        tokio::spawn(async move {
            let _ = pdm_db::touch_token(&pool, id).await;
        });
    }

    let authed = Authed { org_id: tok.org_id, user, token_id: tok.id, scopes: tok.scopes };
    let mut req = req;
    req.extensions_mut().insert(authed);
    Ok(next.run(req).await)
}

/// Return the authenticated user or a forbidden error (for endpoints that need a real user).
pub fn require_user(authed: &Authed) -> Result<&User, AppError> {
    authed.user.as_ref().ok_or_else(|| AppError(pdm_core::PdmError::Forbidden("a user identity is required".into())))
}
