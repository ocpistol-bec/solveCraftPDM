//! PDM MCP endpoint (toDo §18). Phase 1 stub.
//!
//! The full tool surface (pdm_search, pdm_get_item, pdm_get_bom,
//! pdm_where_used, ...) lands in Phase 5. The server will mount these under
//! the MCP stdio/JSON-RPC protocol, executing every tool with an
//! authenticated identity and recording AI-initiated changes in the audit log.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// Names of the PDM MCP tools that will be exposed (Phase 5).
pub const PLANNED_TOOLS: &[&str] = &[
    "pdm_search",
    "pdm_get_item",
    "pdm_get_file_history",
    "pdm_get_bom",
    "pdm_where_used",
    "pdm_compare_revisions",
    "pdm_create_draft",
    "pdm_create_change_request",
    "pdm_request_export",
    "pdm_get_job_status",
];
