//! PDM server binary: boots the API and (optionally) the worker in one process.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::Arc;

use clap::Parser;
use sqlx::postgres::PgPoolOptions;
use tracing::{info, warn};

/// SolveCraft PDM server
#[derive(Parser)]
struct Args {
    /// Listen address (default 0.0.0.0:8080)
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: String,

    /// PostgreSQL connection string (default from PDM_DATABASE_URL env)
    #[arg(long)]
    database_url: Option<String>,
}

type MainError = Box<dyn std::error::Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), MainError> {
    let args = Args::parse();

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    tracing_subscriber::fmt().with_env_filter(filter).with_target(false).init();

    let database_url = args.database_url.clone().or_else(|| std::env::var("PDM_DATABASE_URL").ok()).unwrap_or_else(|| {
        warn!("PDM_DATABASE_URL not set; using localhost default");
        "postgresql://pdm:pdm@localhost:5432/pdm".to_string()
    });

    let pool = PgPoolOptions::new().max_connections(20).connect(&database_url).await.map_err(|e| format!("failed to connect to PostgreSQL: {e}"))?;
    info!(url = %database_url.replace("pdm@", "****@"), "connected to database");

    pdm_db::migrate(&pool).await?;
    info!("migrations applied");

    let backend = pdm_storage::Backend::from_env().map_err(|e| format!("storage backend config failed: {e}"))?;
    let store = Arc::new(pdm_storage::BlobStore::create(&backend).await.map_err(|e| format!("storage init failed: {e}"))?);
    info!(backend = %backend.bucket_name(), "object storage ready");

    let max_upload: u64 = std::env::var("PDM_MAX_UPLOAD_BYTES").ok().and_then(|v| v.parse().ok()).unwrap_or(256 * 1024 * 1024);
    let lease_hours: i64 = std::env::var("PDM_LOCK_LEASE_HOURS").ok().and_then(|v| v.parse().ok()).unwrap_or(24);

    let state = pdm_api::AppState { pool: pool.clone(), store: store.clone(), max_upload, lease_hours };
    let app = pdm_api::build_router(state.clone());

    // Optional in-process worker (disable with PDM_WORKER=0 to run a dedicated
    // worker container instead).
    let worker_enabled: bool = std::env::var("PDM_WORKER").map(|v| v != "0").unwrap_or(true);
    if worker_enabled {
        let worker = pdm_worker::Worker::new(pool.clone(), store.clone(), pdm_worker::WorkerConfig::from_env());
        tokio::spawn(async move {
            worker.run_forever().await;
        });
        info!("worker started in-process");
    }

    let listener = tokio::net::TcpListener::bind(&args.listen).await.map_err(|e| format!("bind {0} failed: {e}", args.listen))?;
    info!(listen = %args.listen, "PDM server listening");
    axum::serve(listener, app).await.map_err(|e| format!("server error: {e}"))?;
    Ok(())
}
