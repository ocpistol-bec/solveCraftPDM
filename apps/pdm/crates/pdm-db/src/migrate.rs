use futures::StreamExt;
use pdm_core::PdmError;
use sqlx::Executor;
use sqlx::PgPool;

const MIGRATION_0001: &str = include_str!("../migrations/0001_init.sql");

pub async fn run_migrations(pool: &PgPool) -> Result<(), PdmError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
             version INTEGER PRIMARY KEY, \
             applied_at TIMESTAMPTZ NOT NULL DEFAULT now())",
    )
    .execute(pool)
    .await
    .map_err(PdmError::Db)?;

    let applied: Vec<i32> = sqlx::query_scalar("SELECT version FROM schema_migrations").fetch_all(pool).await.map_err(PdmError::Db)?;

    let mut tx = pool.begin().await.map_err(PdmError::Db)?;

    if !applied.contains(&1) {
        {
            let mut stream = tx.execute_many(MIGRATION_0001).map(|res| res.map_err(PdmError::Db));
            while let Some(res) = stream.next().await {
                res?;
            }
        }
        sqlx::query("INSERT INTO schema_migrations (version) VALUES (1)").execute(&mut *tx).await.map_err(PdmError::Db)?;
        tracing::info!("applied migration 0001");
    }

    tx.commit().await.map_err(PdmError::Db)?;
    Ok(())
}
