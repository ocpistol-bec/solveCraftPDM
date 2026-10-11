# SolveCraft PDM — System Design

Phase-0 design for the PDM platform described in `apps/pdm/toDo.txt`. Grounded in
`repository-audit.md` (what exists) and `gap-analysis.md` (what's missing). This document covers
the component architecture, workspace layout, database schema, API contracts, the
object-storage transaction model, and the versioning/revision model.

---

## 1. Component architecture

Per toDo §4, a **modular monolith** — one deployable API service plus a worker, not
microservices. Internal module boundaries are clean enough to split later.

```
                        ┌─────────────────────────────────────────────┐
                        │                pdm-server                   │
                        │  ┌─────────────┐      ┌──────────────────┐  │
  SolveCraft desktop ──▶│  │  pdm-api    │◀────▶│   pdm-worker     │  │
  (pdm-client crate)    │  │  (axum)     │      │ (jobs: exports,  │  │
                        │  │  auth/RBAC, │      │  thumbnails, BOM, │  │
  Web portal (React) ──▶│  │  REST      │      │  analysis)       │  │
  (apps/pdm/pdm-web)    │  └──────┬──────┘      └────────┬─────────┘  │
                        └─────────┼───────────────────────┼───────────┘
                                  │                       │
                        ┌─────────▼─────────┐   ┌─────────▼──────────┐
                        │   PostgreSQL      │   │   Object storage   │
                        │   (metadata, jobs,│   │   (S3/MinIO/local  │
                        │   outbox, audit)  │   │    blobs, SHA-256) │
                        └───────────────────┘   └────────────────────┘
                                  │
                        ┌─────────▼─────────┐        ┌───────────────────┐
                        │  Odoo (ERP)       │◀──────▶│  pdm-erp          │
                        └───────────────────┘  sync  │  (outbox adapter) │
                                                     └───────────────────┘
```

- **pdm-api** (axum + tokio + sqlx): auth, RBAC, REST for projects/folders/items/versions/
  revisions/locks/BOM/lifecycle/change orders/search/audit. Stateless — horizontally scalable.
- **pdm-worker**: polls the Postgres-backed job table (`pdm-core::jobs`), runs idempotent jobs with
  bounded concurrency, retries with backoff, dead-letter handling. Jobs: CAD analysis (via
  `solvecraft-cli` subprocess), STEP/STL/3MF export, PNG thumbnails, geometry measurements, BOM
  extraction, ERP sync (via the outbox table).
- **pdm-web**: React/TypeScript portal (Vite). Talks only to the REST API. Model viewer is
  GLTF/3MF + worker-generated thumbnails; it is *not* the egui CAD app.
- **pdm-mcp**: MCP endpoint exposing `pdm_search`, `pdm_get_item`, `pdm_get_bom`, … (toDo §18).
  Every PDM MCP action runs with an authenticated identity (API token), never bypassing lifecycle
  rules.
- **pdm-erp**: Odoo 18 connector. Reads the `integration_outbox` table; PDM stays authoritative for
  engineering identity; Odoo authoritative for inventory/purchasing/manufacturing.

## 2. Workspace layout (new nested Cargo workspace under `apps/pdm/`)

`apps/pdm/` becomes its own Cargo workspace so axum/tokio/sqlx don't enter the CAD kernel's
dependency tree, and so the CAD workspace stays wasm-buildable. The PDM *server* never links the
CAD kernel; it inspects designs by shelling out to `solvecraft-cli`.

```
apps/pdm/
├── Cargo.toml            # [workspace] root, excluded from the parent manifest
├── crates/
│   ├── pdm-core/         # domain: entities, versioning, revision, BOM, lifecycle, refs
│   │                     #   pure, unit-tested, no I/O; the "shared contracts" (toDo §26 Agent 1)
│   ├── pdm-db/           # SQLx types + migrations + queries (Postgres)
│   ├── pdm-storage/      # ObjectStore trait: S3 / MinIO / localfs; SHA-256, multipart
│   ├── pdm-api/          # axum service (auth, RBAC, REST, OpenAPI 3.1, audit)
│   ├── pdm-worker/       # job runner (durable jobs, retries, dead-letter)
│   ├── pdm-mcp/          # PDM MCP tools (toDo §18)
│   └── pdm-erp/          # Odoo connector (outbox-driven)
├── apps/
│   ├── pdm-server/       # binary: runs API + (optionally) worker in-process for dev
│   ├── pdm-cli/          # ops CLI: migrate, import, gc, integrity, backup
│   └── pdm-web/          # React/TypeScript portal (Vite; not a Rust crate)
├── migrations/           # SQL, applied by pdm-cli migrate / pdm-api at boot
├── docker/               # Dockerfiles, compose (dev + prod), swarm examples
└── toDo.txt              # the spec (moved or kept here)
```

Parent-manifest edit: add `"apps/pdm"` to `exclude` in the root `Cargo.toml` (nested workspace
roots are skipped by the `apps/*` glob otherwise). Nothing below L5 in the CAD tree is affected.

CAD-side additions (main workspace, see `pdm-integration-points.md`):
- `crates/pdm-client` — vault client used by the desktop/web CAD apps (HTTP + local workspace).
- `Document.doc_id` + `v5_to_v6` migration in `crates/doc`.
- `crates/engine/src/cmd/pdm.rs` — the `pdm.*` commands.
- `ui-egui` PDM panel + file-menu items; `crates/mcp` PDM tools.

## 3. Database schema (PostgreSQL, migrations from day one)

Only the load-bearing tables; full DDL lands in `migrations/0001_init.sql`. Conventions: UUID
primary keys, `timestamptz`, every table carries `org_id` (organization isolation, enforced in
every query — toDo §5), optimistic concurrency via `version` counters on mutable rows.

**Identity & tenancy**
- `organizations (id, name, created_at)`
- `users (id, org_id, oidc_sub, email, display_name, active)`
- `groups`, `roles`, `permissions`, `memberships (user_id, role, scope, scope_id)`
- `service_accounts`, `api_tokens (id, subject, secret_hash, scopes, expires_at, last_used_at)`

**Projects & files**
- `projects (id, org_id, name, description)`
- `folders (id, project_id, parent_id, name)`
- `items (id, org_id, project_id, folder_id, part_number, description, classification,
  doc_id uuid /* SolveCraft Document.doc_id */, lifecycle_state, owner_id,
  created_at, updated_at)` — a *managed document*; `doc_id` links it to the CAD identity.
  `UNIQUE (org_id, part_number)` within an active (non-obsolete) scope.
- `files (id, item_id, filename, mime, size_bytes)` — the managed file record (name is mutable,
  never a key).
- `file_blobs (id, org_id, sha256 bytea, size, storage_key, created_at)` — content-addressed,
  deduplicated; immutable. `UNIQUE (org_id, sha256)`.
- `file_versions (id, file_id, seq int, blob_id, author_id, committed_at, commit_message,
  native_format /* solvecraft/N */, base_version_id /* lineage */, checksum_verified bool)` —
  immutable once committed; `UNIQUE (file_id, seq)`.
- `file_revisions (id, item_id, label /* A, B, … or 1, 2, … */, version_id, released_by,
  released_at, release_note, scheme)` — pinned to one immutable version; released ⇒ immutable.
- `file_locks (id, item_id, user_id, workstation, base_version_id, acquired_at, lease_expires_at)`
  — at most one live row per item (`WHERE item_id=? AND lease_expires_at > now()`); atomic via
  `INSERT … ON CONFLICT DO NOTHING`.
- `workspaces (id, user_id, project_id, machine, last_sync_at)` — offline cache registration.

**References & BOM**
- `file_references (id, from_item_id, from_feature_id /* optional u64 within doc */,
  to_item_id, revision_policy /* pinned|latest */, resolved_revision_id,
  occurrence_count, suppressed, status /* resolved|missing|circular */)` — the PDM-owned
  parent→child graph. Built by the worker when a document's occurrences are analysed
  (managed occurrences carry `pdm://` URIs; see §4).
- `bom_snapshots (id, revision_id, generated_at, jsonb)` — the frozen EBOM for a released
  assembly revision (toDo §11: released BOMs never change when children move on).

**Engineering metadata**
- `materials (id, org_id, name, density /* g/mm³ */)`, `units_of_measure`
- `properties (id, item_id, key, value_type /* text|number|bool|date|enum|unit|ref */,
  value jsonb, computed bool /* CAD-computed vs entered */)`

**Workflow**
- `lifecycle_definitions / lifecycle_states / lifecycle_transitions` (configurable per org;
  default `WIP → In Review → Approved → Released → Obsolete`, toDo §12)
- `approval_requests / approval_decisions`
- `change_requests (ECR)`, `change_orders (ECO)`, `change_affected_items`,
  `change_notices (ECN)` (toDo §13); multi-item releases commit atomically.

**Operations**
- `jobs (id, kind, payload jsonb, status /* queued|running|done|failed|dead */,
  attempts, max_attempts, last_error, run_after, locked_by, created_at)` +
  `job_attempts (id, job_id, started_at, ended_at, ok, error)`
- `audit_events (id, org_id, actor_id, agent_id /* nullable: AI agent */, action,
  object_type, object_id, before jsonb, after jsonb, at)` — append-only.
- `integration_outbox (id, org_id, kind, payload jsonb, status, sent_at, attempts)` —
  durable ERP events (toDo §17); `integration_mappings`, `sync_status`.

## 4. Versioning, revision & reference model

- **File versions**: every successful check-in creates a new immutable `file_versions` row pointing
  at a `file_blobs` content hash. `base_version_id` gives the lineage. Stale check-ins (client
  `base_version` ≠ server latest, or hash mismatch) are rejected, never silently merged (toDo §7).
- **Revisions**: a release operation pins a `file_revisions` label (configurable scheme) to a
  version. Released revisions are immutable; changing released content starts a new working
  lineage and a new revision (toDo §6, §12). "Restore" = create a new working version from an old
  blob.
- **References**: PDM owns `file_references` keyed by `doc_id` (stable SolveCraft document
  identity), never by filename or path. Managed assemblies carry `pdm://org/project/doc_id@rev`
  URIs on their managed occurrences; the worker resolves them into a dependency set on open.
  **Release manifest**: releasing an assembly snapshot resolves every floating (`@latest`)
  reference into its concrete revision and stores the frozen `bom_snapshot` (toDo §9).
- **Check-in validation order** (toDo §7): permissions → lock ownership → base version →
  integrity (SHA-256 of payload vs. `file_blobs`) → dependency validity (referenced items exist and
  are resolvable) → required metadata → workflow restrictions (released state ⇒ no check-in).

## 5. Object-storage transaction model

- `pdm-storage` exposes an `ObjectStore` trait: `put`, `get`, `delete_if_unreferenced`,
  `multipart_*`, `verify(sha256)`. Backends: S3/MinIO (default) and localfs (dev/test).
- **Commit protocol** (atomic from the metadata DB's point of view):
  1. Client uploads to a *staging* key (`staging/<upload_id>`, multipart-capable);
  2. Worker/API computes SHA-256; on mismatch the upload is rejected and staging garbage;
  3. `PUT` to the canonical content-addressed key (`blobs/<org>/<sha256[:2]>/<sha256>`);
     objects with an existing hash are deduplicated (skip, verify size);
  4. In one Postgres transaction: insert `file_blobs` (if new) + `file_versions` row + release the
     `file_locks` row + append `audit_events`;
  5. Staging key deleted.
- **Immutability**: canonical blob keys are never overwritten; `file_versions` rows never mutated
  after commit. Garbage collection (toDo §4 B) deletes a blob only when no `file_versions`
  references it *and* it isn't pinned by a released revision's retention policy. Orphan/`staging/`
  objects older than a TTL are swept by the `pdm-cli integrity` job.
- **Downloads** are streamed (never loaded fully in memory); the client verifies SHA-256 on
  arrival (toDo §22, and the first acceptance test's "verified integrity after download").

## 6. API contracts (REST, OpenAPI 3.1)

Auth: OIDC bearer tokens (or `api_tokens` for service accounts); every request is scoped to one
organization; all ids are opaque UUIDs — clients never trust path ids without an authorization
check (toDo §20). Rate limiting + upload size caps at the API layer.

Key endpoints (v1, minimal set; full listing in the OpenAPI doc):

```
POST   /auth/token                      # OIDC exchange / api-token issue
GET    /org/{org}/projects              # list
POST   /org/{org}/projects              # create
GET    /org/{org}/projects/{p}/items?folder=&q=    # browse + search
POST   /org/{org}/projects/{p}/items    # create item (assigns part number)
GET    /org/{org}/items/{i}             # item + current revision + lock
POST   /org/{org}/items/{i}/lock        # checkout (atomic; returns lease + base_version)
DELETE /org/{org}/items/{i}/lock        # undo checkout
POST   /org/{org}/items/{i}/uploads     # → upload_id (multipart or single PUT)
PUT    /org/{org}/items/{i}/uploads/{u}/part/{n}   # multipart parts
POST   /org/{org}/items/{i}/checkin     # {upload_id, base_version, message} → new version
GET    /org/{org}/items/{i}/versions    # history
GET    /org/{org}/versions/{v}/blob     # streamed, Content-Digest: SHA-256
POST   /org/{org}/items/{i}/release     # {scheme?, note} → revision; freezes refs (assembly)
GET    /org/{org}/items/{i}/where-used[?recursive=true]
GET    /org/{org}/items/{i}/uses[?recursive=true]
GET    /org/{org}/revisions/{r}/bom?mode=indented|flattened
GET    /org/{org}/search?q=&filters=…   # part number, description, material, state…
GET    /org/{org}/audit?object=&since=…
POST   /org/{org}/changes               # ECR → ECO → release (toDo §13)
GET    /org/{org}/jobs/{j}              # worker job status (exports, previews)
POST   /org/{org}/items/{i}/exports     # request STEP/STL/PDF (enqueues job)
```

Error shape: `{ "error": { "code": "base_version_mismatch", "message": … } }` with stable machine
codes (`unauthorized`, `forbidden`, `lock_held`, `base_version_mismatch`, `integrity_mismatch`,
`workflow_blocked`, `not_found`).

## 7. Security model (toDo §20)

- TLS at the reverse proxy (compose examples include Caddy/Traefik); HTTP-only behind that.
- OIDC for people; `api_tokens` (Argon2id-hashed secrets, short-lived, scoped) for machines and
  the CAD `pdm-client`.
- RBAC: role × scope (org / project / item). Default roles: `viewer`, `editor`, `reviewer`,
  `release-manager`, `admin`. Lifecycle transitions require the role named in
  `lifecycle_transitions`.
- Organization isolation: `org_id` in every table, in every query (enforced by the query layer in
  `pdm-db`, not by convention alone); object-storage keys namespaced by org.
- Audit: every state change appends an immutable `audit_events` row in the same transaction.
- AI agents act through API tokens (`agent_id` recorded in audit; toDo §18).

## 8. Reliability & operations (toDo §21–23)

- `pdm-cli migrate` applies/rolls back SQL migrations; `pdm-cli integrity` verifies blob hashes vs
  `file_blobs`, finds orphans, and GCs unreferenced blobs (retention-aware).
- Backups: `pg_dump` + blob-store replication + config; restore validation test in CI.
- Deployment: `docker/pdm-dev` compose (pdm-api, pdm-worker, pdm-web, postgres, minio) for
  development; `docker/pdm-prod` compose + swarm overlay; external Postgres/S3 supported by
  env. Health: `GET /healthz` (alive) and `GET /readyz` (DB + storage reachable).
- Jobs are durable (Postgres table), idempotent, bounded concurrency, exponential retry, then
  dead-letter; no message broker until Proven needs (toDo §23).

## 9. First end-to-end acceptance test (toDo §28)

Implemented as a Playwright/Rust test against the dev compose stack, plus a CAD-side test through
the real `pdm-client`:

1. Two users authenticate (OIDC or api tokens) into one shared project.
2. User A creates an item and uploads a `.solvecraft` design (multipart); worker verifies SHA-256;
   version 1 is immutable.
3. User A checks out (lock acquired, lease returned, `base_version=1`).
4. User B's concurrent checkout attempt fails atomically (`lock_held`).
5. User A edits locally (SolveCraft), checks in with `base_version=1`; a second check-in with a
   stale `base_version` is rejected.
6. Version 2 is committed; a previous version (1) is retrieved and its download hash verifies.
7. The audit history shows both check-ins with actor, time, base version.
8. File integrity after download: SHA-256 of the retrieved bytes equals `file_blobs.sha256`.

Passing this is the Phase 1 gate (see `implementation-roadmap.md`).

## 10. Explicit non-goals for now

No microservices, no Kafka/Redis (Postgres-backed jobs first), no drawing generation (toDo §16:
association first), no MBOM (EBOM first, conversion later), no model-diff visualization beyond
metadata/references/BOM/measurements (toDo §6), no web-based CAD editor.
