use std::sync::Arc;

use pdm_storage::BlobStore;
use sqlx::PgPool;

/// Shared application state, cloned into each handler.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub store: Arc<BlobStore>,
    /// Maximum request body size in bytes (uploads).
    pub max_upload: u64,
    /// Lock lease length in hours.
    pub lease_hours: i64,
}
