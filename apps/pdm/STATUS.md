# SolveCraft PDM — Progress Tracker

Living document. Updated as work lands. See `toDo.txt` (this folder) for the full spec and
`docs/architecture/` for the Phase-0 architecture docs.

## Current state

- **Phase:** 1 — Minimum functional vault ✅ complete & E2E-proven (PDM side **and** CAD-side groundwork)
- **Next:** Phase 2 — Native CAD integration (`pdm.*` engine commands, PDM UI panel, offline workspace)
- **Area:** `apps/pdm` (nested Cargo workspace, excluded from the CAD workspace)

## Phase 0 — Discovery & architecture ✅

- [x] Repository audit of SolveCraft → `docs/architecture/repository-audit.md`
- [x] Integration points → `docs/architecture/pdm-integration-points.md`
- [x] Gap analysis → `docs/architecture/gap-analysis.md`
- [x] System design (schema, API contracts, storage model) → `docs/architecture/system-design.md`
- [x] Dependency-ordered roadmap → `docs/architecture/implementation-roadmap.md`

## Phase 1 — Minimum functional vault

**PDM side ✅ (verified by the E2E acceptance test):**
- [x] `apps/pdm` nested workspace: `pdm-core` (entities, errors, part-number + check-in rules),
      `pdm-db` (Postgres migration 0001 + repository layer), `pdm-storage` (SHA-256
      content-addressed blobs: local filesystem + S3/MinIO via object_store), `pdm-api`
      (axum: bearer-token auth, RBAC roles, org isolation, projects/folders/items CRUD, atomic
      part-number sequences, exclusive checkout with leases, check-in with base-version +
      integrity validation, version history, blob download with Content-Digest, FTS + prefix
      search, immutable audit trail, typed properties, revisions, worker job queue),
      `pdm-worker` (Postgres job queue, `design.analyze` via `solvecraft-cli eval`),
      `pdm-server` + `pdm-cli` binaries; `pdm-mcp` / `pdm-erp` stubs for Phase 5.
- [x] Docker: `apps/pdm/docker/Dockerfile` + `pdm-dev.yml` (postgres, minio + bucket init, server).
- [x] Web file browser: single-page portal served at `/` (connect with token, projects, items,
      checkout/check-in, upload + check-in, versions with SHA-256, downloads, search, audit).
- [x] First E2E acceptance test: `apps/pdm/apps/pdm-server/tests/e2e_vault.rs`, driven by
      `apps/pdm/e2e.sh` — **PASSING** (2 users, 1 project, upload → v1, exclusive checkout 409
      for second user, v2 check-in, stale base-version 409, previous-version download + SHA-256
      integrity, audit history, search).
- [x] Grep-clean: `cargo fmt --check`, `cargo clippy --all-targets -D warnings`,
      `cargo test` all green in the PDM workspace.

**CAD side ✅ (roadmap items 10–11):**
- [x] `Document.doc_id: Uuid` in `crates/doc` — stable identity for PDM references; legacy
      files get one generated on load; local-file mode ignores it. `cargo xtask ci` green
      (fmt, clippy, tests, layers, wasm, assets, licences).
- [x] `crates/pdm-client` (L3): async `Vault` client (reqwest) + `VaultConfig` + local
      offline-workspace path manager + pure helpers, with unit tests. Registered in the
      layering table. Not yet wired into engine/UI (that is Phase 2).
- [x] `THIRD-PARTY-LICENSES.txt` regenerated for the new CAD-side dependencies (uuid,
      reqwest, rustls tree).

## Decision log

- 2026-10-09: PDM lives in a **nested Cargo workspace** at `apps/pdm/` (own `Cargo.lock`,
  `target/`), excluded from the CAD workspace so axum/tokio/sqlx never enter the CAD kernel's
  dependency tree and `cargo xtask wasm` stays clean. (`.gitignore` updated for
  `apps/pdm/target/`.)
- 2026-10-09: PDM treats `.solvecraft` as an opaque, content-addressed (SHA-256), immutable
  blob. The worker shells out to `solvecraft-cli eval` for measurements rather than linking the
  CAD kernel; blob keys are `blobs/{org}/{sha256}` (org-namespaced), staging keys are
  `staging/{org}/{upload_id}`.
- 2026-10-09: Phase-1 auth is Bearer API-token based (tokens stored SHA-256-hashed; Argon2id
  reserved for user passwords, which are optional). RBAC via `users.role` (viewer/editor/admin);
  org isolation is enforced in every query (org_id in the WHERE clause, not by convention).
  OIDC exchange is a Phase-2/4 hardening item.
- 2026-10-09: `doc_id` was introduced as a serde-default field + a `v5→v6` JSON migration that
  assigns a UUID to legacy files on load. (Upstream has since advanced the native format to v6
  independently; the rebase merged cleanly — the PDM side keeps using `Document.doc_id` as the
  stable identity.)
- 2026-10-09: Cross-document assembly references (`pdm://`), where-used, and the EBOM engine are
  Phase 3; the schema tables exist now (`file_references`, `bom_snapshots`) but are unwired.
- 2026-10-09: Web portal is a single self-contained HTML/JS page for Phase 1 (no build step);
  the full React/TS portal is a later upgrade.

## Backlog / notes

- Open question: PDM portal as in-repo Vite app vs. separate repo (assumed in-repo under
  `apps/pdm/pdm-web` later).
- Open question: OIDC provider choice (toDo mandates OIDC + Argon2id fallback).
- CAD repo `plan/` tree is local-only (gitignored); AGENTS.md session-start reads of
  `plan/STATUS.md` / `plan/fusion/*` are unavailable in this checkout.
- Dev ports: PDM server 18080 (8080 is taken by the Odoo dev stack on this machine); PDM
  Postgres exposed on 15432; MinIO on 19000/19001.
- Phase-2 entry points: `crates/engine/src/cmd/pdm.rs` (pdm.* commands) + `crates/ui-egui`
  PDM panel; both build on `solvecraft-pdm-client` + `Document.doc_id`.
