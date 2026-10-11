//! PDM Odoo ERP connector (toDo §17). Phase 1 stub.
//!
//! The outbox-driven connector that maps released engineering items/EBOMs to
//! Odoo 18 `product.template` / `product.product` / manufacturing BOM records
//! lands in Phase 5. The `integration_outbox` table already accumulates events
//! so nothing is lost while the real mapping is built.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use pdm_core::PdmError;
use serde_json::Value;
use sqlx::PgPool;

/// Process pending outbox entries destined for Odoo.
///
/// Phase 1 reads the outbox and marks entries as `sent` without calling Odoo.
/// Phase 5 implements the version-specific mapping and idempotent sync.
pub async fn process_outbox(pool: &PgPool) -> Result<usize, PdmError> {
    let pending: Vec<(uuid::Uuid, String, Value)> = sqlx::query_as(
        "SELECT id, kind, payload FROM integration_outbox
         WHERE status = 'pending' ORDER BY created_at LIMIT 50",
    )
    .fetch_all(pool)
    .await
    .map_err(PdmError::Db)?;

    for (id, _kind, _payload) in &pending {
        let _ = sqlx::query("UPDATE integration_outbox SET status = 'sent', sent_at = now() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await
            .map_err(PdmError::Db)?;
    }
    Ok(pending.len())
}
