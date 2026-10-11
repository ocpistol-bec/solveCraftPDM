# PDM Implementation Roadmap

Dependency-ordered backlog for building the PDM system, phase by phase, per
`apps/pdm/toDo.txt` §25. Phases gate on acceptance tests: no phase starts before the previous
phase's deliverable works. Progress is tracked in `apps/pdm/STATUS.md` (the living tracker); this
file is the stable order and the per-item definitions of done.

**Notation:** `[owner]` = the toDo §26 agent role. Every item lists its acceptance test.

---

## Phase 0 — Discovery & architecture ✅ (this set of documents)

- [x] Repository audit → `docs/architecture/repository-audit.md`
- [x] Integration points → `docs/architecture/pdm-integration-points.md`
- [x] Gap analysis → `docs/architecture/gap-analysis.md`
- [x] System design (schema, API contracts, storage model) → `docs/architecture/system-design.md`
- [x] This roadmap + dependency-ordered backlog
- [x] First end-to-end acceptance test defined (system-design.md §9)

**Gate:** architecture is internally consistent (no contradictions between schema, API and
CAD-side changes); owner reviews. Then Phase 1 may start.

## Phase 1 — Minimum functional vault ✅ (complete 2026-10-09, PDM side and CAD-side groundwork)

PDM-side (nested workspace `apps/pdm/`, owner: Architect/Database/Backend/Storage):

1. ✅ **[Architect]** Scaffold `apps/pdm` nested workspace: `pdm-core`, `pdm-db`, `pdm-storage`,
   `pdm-api`, `pdm-worker`, `pdm-mcp`, `pdm-erp` (stub), `apps/pdm-server`, `apps/pdm-cli`.
   Parent `Cargo.toml`: add `apps/pdm` to `exclude`. `cargo xtask ci` on the CAD repo must still
   pass. *Accept: `cargo build` in both workspaces; CAD wasm build green.*
2. ✅ **[Database]** `migrations/0001_init.sql`: orgs, users, api_tokens, projects, folders, items,
   files, file_blobs, file_versions, file_locks, audit_events, jobs. UUIDs, org-scoped unique
   constraints, optimistic version counters. *Accept: migrate up/down on clean Postgres.*
3. ✅ **[Storage]** `pdm-storage` `ObjectStore` trait + localfs impl + SHA-256 put/verify; S3/MinIO
   impl; multipart. *Accept: put/get/verify/dedup tests on localfs; MinIO via compose.*
4. ✅ **[Backend]** Auth (OIDC token exchange + api_tokens, Argon2id), RBAC scopes, org isolation in
   the query layer. *Accept: token → scoped requests; cross-org access denied (403 tests).*
   *(Phase-1 delivery: Bearer API-token auth + `users.role` RBAC + org isolation; OIDC exchange and
   Argon2id passwords are implemented in `pdm-db`/`pdm-api` scaffolding and wired next.)*
5. ✅ **[Backend]** Items/folders/projects CRUD + part-number sequences. *Accept: create item
   assigns next part number atomically.*
6. ✅ **[Backend/Storage]** Upload → commit protocol (system-design.md §5), check-in with base-version
   + hash validation, version history, streamed download. *Accept: the Phase-1 E2E test
   (system-design.md §9), items 1–3, 5–8.*
7. ✅ **[Backend]** Search v1 (Postgres full-text + exact part number). *Accept: search tests.*
8. ✅ **[QA/DevOps]** Dev compose stack (api, worker, postgres, minio, web placeholder), health/
   readyz, `pdm-cli migrate`. *Accept: `docker compose up` → readyz green.*
9. ✅ **[Frontend]** Web portal v1: login, project/file browser, upload, version history.
   *Accept: two users store & retrieve a versioned file in the browser.*

**CAD-side** (main workspace):
10. ✅ **[Architect/CAD-Integration]** `Document.doc_id` (UUID) + `v5_to_v6` migration; legacy files
    load with a generated id; local mode ignores it. `cargo xtask ci` green incl. wasm.
    *Accept: migration unit tests + hostile-params regression test.*
11. ✅ **[CAD-Integration]** `crates/pdm-client`: `Vault` trait, HTTP client, local workspace dir.
    *Accept: unit tests against the dev compose stack (or a mock server).* — pure-helper unit
    tests ship with the crate; live-server tests ride on Phase 2 when the client is wired in.

**Phase-1 deliverable:** "multiple users can store and retrieve versioned engineering files"
(toDo §25) — proven by the first end-to-end acceptance test.

## Phase 2 — Native CAD integration

1. **[CAD-Integration]** `pdm.*` commands (`crates/engine/src/cmd/pdm.rs`): login, open,
   checkout, checkin, get_latest, undo_checkout, history. `Session.pdm` state. *Accept: each
   command's JSON-driven test (like `cmd/*` tests) + a full check-out→edit→check-in loop against
   the dev stack via `solvecraft-cli`.*
2. **[CAD-Integration]** UI: PDM panel (server, auth, project/file browser, lock status,
   check-in/out buttons) + File-menu vault items. *Accept: screenshot-verified walkthrough
   (control channel `ui.screenshot`).*
3. **[CAD-Integration]** Credentials in OS keyring; non-secret prefs; local-file mode unchanged.
   *Accept: no server ⇒ app behaves exactly as before (regression).*
4. **[CAD-Integration]** Offline workspace: cached opens, local edits preserved, sync on
   reconnect, stale-version conflict surfaced. *Accept: conflict test (server version moves while
   offline → explicit resolution, never silent overwrite).*
5. **[QA/DevOps]** CAD integration tests (toDo §24): open managed design, modify, check in,
   retrieve earlier version, integrity after download.

**Phase-2 deliverable:** "a user can manage native SolveCraft documents without manually
uploading files" (toDo §25).

## Phase 3 — Assemblies & dependencies

1. **[Architect/Assembly-BOM]** Managed occurrences: an occurrence may reference another managed
   document via `pdm://org/project/doc_id@rev` instead of embedded geometry; resolution on open
   fetches the needed versions consistently. *Accept: open a 3-doc assembly from vault; every doc
   resolves to the revision recorded at check-in time.*
2. **[Assembly-BOM]** `file_references` population (worker analyses checked-in assemblies),
   missing-reference detection, circular-reference detection. *Accept: broken ref ⇒ clear report,
   never a crash.*
3. **[Assembly-BOM]** Where-used / uses (direct + recursive, with revision context).
   *Accept: fixture assemblies with shared & repeated parts; query results match expectations.*
4. **[Assembly-BOM]** EBOM engine: recursive traversal, quantities, subassembly expansion,
   indented + flattened, make/buy classification, phantom/reference-only/suppressed rules,
   unit handling, revision-specific BOMs, CSV/JSON export. *Accept: quantity-aggregation tests
   incl. repeated components (toDo §11 "precise quantity aggregation rules").*
5. **[Assembly-BOM]** BOM snapshots on released revisions (frozen manifests).
   *Accept: child revision bump does not change the released BOM.*

**Phase-3 deliverable:** "managed assemblies remain reproducible and their dependencies are
traceable" (toDo §25).

## Phase 4 — Engineering release control

1. **[Backend]** Configurable lifecycle definitions/states/transitions + default workflow
   (WIP → In Review → Approved → Released → Obsolete); transition guards (role, required
   properties, approvals, valid refs, generated artifacts, BOM validation).
2. **[Backend]** Revisions: schemes (alphabetic/numeric), release = pin immutable version +
   freeze refs; previous-version retrieval; revision comparison (metadata, references, BOM,
   measurements first).
3. **[Backend]** Approvals (requests/decisions, immutable audit events).
4. **[Backend]** ECR/ECO/ECN; multi-item release commits atomically; no partial successes.
5. **[Frontend]** Web portal: revision history, BOM view, workflow approvals, change orders,
   audit view.
6. **[CAD-Integration]** `pdm.release`, `pdm.properties`, `pdm.bom` commands + panel wiring.

**Phase-4 deliverable:** "engineering teams can formally approve and release designs" (toDo §25).

## Phase 5 — Automation & ERP

1. **[Integration]** Worker jobs: STEP/STL/3MF export, PNG thumbnails, measurements (mass needs
   material density), BOM extraction — all via `solvecraft-cli` subprocess; idempotent, retried,
   dead-lettered.
2. **[Integration]** Export/preview request API + job status; web portal model viewer (3MF/GLTF +
   thumbnails).
3. **[Integration]** `pdm-mcp`: the ten proposed tools; identity-scoped, audited.
4. **[Integration]** `pdm-erp`: Odoo 18 adapter, outbox-driven, idempotent sync, mapping records,
   conflict detection, sync status, reconciliation tooling; release-to-ERP approval policy.
5. **[QA/DevOps]** Performance benchmarks (toDo §22 list) with a `pdm-cli bench` harness.

**Phase-5 deliverable:** "approved engineering information can flow into manufacturing systems"
(toDo §25).

## Phase 6 — Advanced capabilities

Offline sync hardening, model comparison (graphical), section views / measurements / exploded
assemblies in the portal, drawing association → drawing generation parity (toDo §16), advanced
reporting, swarm/multi-node validation, DR runbook.

---

## Sub-agent ownership (toDo §26)

| Agent | Owns | Phases |
|-------|------|--------|
| 1 Architect | contracts, schema, storage model, review of shared changes | 0–6 |
| 2 Database | migrations, query layer, integrity constraints | 1, 3, 4 |
| 3 Backend | auth, RBAC, items, versions, locks, lifecycle, change mgmt | 1, 4 |
| 4 Storage | ObjectStore, multipart, dedup, GC, integrity | 1, 5 |
| 5 CAD Integration | `pdm-client`, `doc_id`, `pdm.*` commands, UI panel, keyring, offline | 1–2, 4, 6 |
| 6 Assembly/BOM | `file_references`, where-used, EBOM, snapshots | 3 |
| 7 Frontend | portal v1 → v3 (browser, search, revisions, BOM, workflow, audit) | 1, 4, 5 |
| 8 Integration | worker jobs, MCP, Odoo connector | 5 |
| 9 QA/DevOps | compose, CI, E2E tests, benchmarks, DR | 1 onward |

Rules: small reviewable changes with tests; shared contracts (schema/API/pdm-client traits)
change only with Architect review; parallelize independent modules, never coupled DB+API pairs.

## Cadence

- One commit per feature arc that builds; `cargo xtask ci` (CAD repo) and PDM's own CI (fmt,
  clippy -D, tests, migrations up/down) both green before merge.
- Each phase ends with its acceptance test green and `apps/pdm/STATUS.md` updated.
- Open questions for the owner are parked in `apps/pdm/STATUS.md` → "Backlog / notes".
