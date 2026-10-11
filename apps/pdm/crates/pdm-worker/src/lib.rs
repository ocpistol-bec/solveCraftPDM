//! PDM background worker: polls the Postgres-backed job queue and executes jobs.
//!
//! Phase 1 supports a single job kind, `design.analyze`, which downloads the
//! committed `.solvecraft` blob and runs `solvecraft-cli eval` to extract
//! geometry measurements, storing them on the file version. Unknown job kinds
//! are marked done (stub) so enqueued follow-up work cannot jam the queue.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::sync::Arc;
use std::time::Duration;

use pdm_core::{Job, PdmError};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};
use uuid::Uuid;

use pdm_storage::BlobStore;

/// Configuration for the worker loop.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Poll interval between queue checks.
    pub poll_interval: Duration,
    /// Maximum number of jobs claimed per poll.
    pub batch_size: i64,
    /// Maximum number of concurrently running jobs.
    pub concurrency: usize,
    /// Path to the `solvecraft-cli` binary; if `None`, design.analyze jobs are
    /// marked done with a `skipped` note instead of failing.
    pub solve_cli: Option<String>,
}

impl WorkerConfig {
    pub fn from_env() -> Self {
        let parse = |name: &str, def: u64| -> u64 { std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(def) };
        Self {
            poll_interval: Duration::from_millis(parse("PDM_WORKER_POLL_MS", 1000)),
            batch_size: parse("PDM_WORKER_BATCH", 5) as i64,
            concurrency: parse("PDM_WORKER_CONCURRENCY", 2) as usize,
            solve_cli: std::env::var("PDM_SOLVECRAFT_CLI").ok().filter(|s| !s.is_empty()),
        }
    }
}

pub struct Worker {
    pool: PgPool,
    store: Arc<BlobStore>,
    cfg: WorkerConfig,
}

impl Worker {
    pub fn new(pool: PgPool, store: Arc<BlobStore>, cfg: WorkerConfig) -> Self {
        Self { pool, store, cfg }
    }

    /// Run the worker loop until the task is cancelled (process shutdown).
    pub async fn run_forever(&self) {
        let sem = Arc::new(Semaphore::new(self.cfg.concurrency.max(1)));
        info!(
            poll_ms = self.cfg.poll_interval.as_millis() as u64,
            batch = self.cfg.batch_size,
            concurrency = self.cfg.concurrency,
            cli = ?self.cfg.solve_cli,
            "worker started"
        );
        loop {
            let pool = self.pool.clone();
            let store = self.store.clone();
            let cfg = self.cfg.clone();
            let sem = sem.clone();
            let claimed = match pdm_db::claim_jobs(&pool, "inline", cfg.batch_size).await {
                Ok(j) => j,
                Err(e) => {
                    warn!(error = %e, "claim_jobs failed; retrying next tick");
                    vec![]
                }
            };
            for job in claimed {
                let pool = pool.clone();
                let store = store.clone();
                let cfg = cfg.clone();
                let sem = sem.clone();
                tokio::spawn(async move {
                    let _permit = match sem.acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => return, // semaphore closed
                    };
                    run_job(&pool, &store, &cfg, &job).await;
                    drop(_permit);
                });
            }
            tokio::time::sleep(cfg.poll_interval).await;
        }
    }
}

async fn run_job(pool: &PgPool, store: &Arc<BlobStore>, cfg: &WorkerConfig, job: &Job) {
    let (ok, err, measurements) = match job.kind.as_str() {
        "design.analyze" => match analyze_design(pool, store, cfg, job).await {
            Ok(m) => (true, String::new(), Some(m)),
            Err(e) => (false, e.to_string(), None),
        },
        other => {
            debug!(kind = other, "unknown job kind; marking done");
            (true, String::new(), None)
        }
    };

    let _ = pdm_db::record_job_attempt(pool, job.id, ok, &err).await;
    if ok {
        let _ = pdm_db::complete_job(pool, job.id, true, "").await;
        if let Some(m) = measurements {
            let _ = store_measurements(pool, job, m).await;
        }
    } else if job.attempts + 1 < job.max_attempts {
        let _ = sqlx::query(
            "UPDATE jobs SET status = 'queued', last_error = $2, run_after = now() + interval '30 seconds', updated_at = now() WHERE id = $1",
        )
        .bind(job.id)
        .bind(&err)
        .execute(pool)
        .await;
        warn!(job = %job.id, error = %err, "job will be retried");
    } else {
        let _ = pdm_db::complete_job(pool, job.id, false, &err).await;
        let _ = sqlx::query("UPDATE jobs SET status = 'dead' WHERE id = $1").bind(job.id).execute(pool).await;
        warn!(job = %job.id, error = %err, "job dead-lettered");
    }
}

async fn store_measurements(pool: &PgPool, job: &Job, measurements: serde_json::Value) {
    let version_id = job.payload.get("version_id").and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok());
    let Some(version_id) = version_id else {
        return;
    };
    let _ = sqlx::query("UPDATE file_versions SET measurements = $2 WHERE id = $1").bind(version_id).bind(&measurements).execute(pool).await;
}

/// Run `solvecraft-cli eval` on the committed blob; return measurements JSON.
async fn analyze_design(pool: &PgPool, store: &Arc<BlobStore>, cfg: &WorkerConfig, job: &Job) -> Result<serde_json::Value, PdmError> {
    let version_id = job
        .payload
        .get("version_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| PdmError::Validation("missing version_id in job payload".into()))?;

    let version = pdm_db::get_version(pool, version_id).await?.ok_or_else(|| PdmError::NotFound(format!("version {version_id}")))?;
    let blob = pdm_db::get_blob_by_id(pool, version.blob_id).await?.ok_or_else(|| PdmError::NotFound("blob for version".into()))?;

    let Some(cli) = cfg.solve_cli.as_deref() else {
        return Ok(serde_json::json!({ "skipped": true, "reason": "solvecraft-cli not configured" }));
    };

    let data = store.get_by_sha(&blob.sha256).await.map_err(|e| PdmError::Storage(e.to_string()))?;

    let tmp = std::env::temp_dir().join(format!("pdm-job-{}.solvecraft", job.id));
    std::fs::write(&tmp, &data).map_err(|e| PdmError::Storage(e.to_string()))?;

    let out = tokio::process::Command::new(cli).arg("eval").arg(&tmp).output().await.map_err(|e| PdmError::Internal(e.to_string()))?;
    let _ = std::fs::remove_file(&tmp);

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(PdmError::Internal(format!("solvecraft-cli eval failed: {stderr}")));
    }
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    serde_json::from_str(&stdout).map_err(|e| PdmError::Internal(format!("bad eval output: {e}")))
}
