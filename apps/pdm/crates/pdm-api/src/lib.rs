//! PDM HTTP API (axum): auth, catalog, files/versions, locks, search, audit, portal.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::Path;

use axum::Router;
use axum::middleware;
use axum::routing::{get, get_service};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

mod auth;
mod error;
mod portal;
mod routes;
mod state;

pub use auth::{Authed, auth_middleware};
pub use error::AppError;
pub use state::AppState;

/// Resolve the portal static directory. Uses PDM_PORTAL_DIR if set, otherwise the
/// `static/` directory next to this crate's source. `None` if it does not exist.
fn portal_dir() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("PDM_PORTAL_DIR") {
        let p = Path::new(&dir).to_path_buf();
        return if p.is_dir() { Some(p) } else { None };
    }
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
    if p.is_dir() { Some(p) } else { None }
}

/// Build the full API router. The embedded single-file portal is always served at
/// `/`; if a `static/` directory is present it is mounted at `/static/*` and its
/// `index.html` (if any) takes precedence over the embedded one.
pub fn build_router(state: AppState) -> Router {
    let api = routes::api_router();

    let mut router: Router<AppState> = Router::new();
    router = router.merge(api);

    if let Some(dir) = portal_dir() {
        let index = dir.join("index.html");
        if index.is_file() {
            router = router.route("/", get_service(ServeFile::new(index)));
        } else {
            router = router.route("/", get(portal::default_index));
        }
        router = router.nest_service("/static", ServeDir::new(dir));
    } else {
        router = router.route("/", get(portal::default_index));
    }

    router = router.layer(middleware::from_fn_with_state(state.clone(), auth_middleware)).layer(TraceLayer::new_for_http());

    router.with_state(state)
}
