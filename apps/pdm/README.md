# SolveCraft PDM

Self-hosted Product Data Management for SolveCraft CAD: versioned vault for
`.solvecraft` designs, checkout/checkin, part numbers, BOM, lifecycle and
change management, ERP sync, and AI/MCP access.

**Spec:** `toDo.txt` (this directory). **Architecture:** `docs/architecture/`
at the repo root. **Progress:** `STATUS.md`.

## Layout

`apps/pdm/` is its **own Cargo workspace** (excluded from the CAD workspace in
the root `Cargo.toml`). The CAD kernel never links the PDM service; the PDM
worker inspects designs by shelling out to `solvecraft-cli`.

| Path | What it is |
|------|------------|
| `crates/pdm-core` | Domain entities, errors, pure versioning/part-number/lifecycle rules |
| `crates/pdm-db` | Postgres migrations + repository queries (SQLx, runtime) |
| `crates/pdm-storage` | Content-addressed blob store (SHA-256): local FS and S3/MinIO |
| `crates/pdm-api` | Axum HTTP API: auth, catalog, blobs, versions, locks, search, audit, portal |
| `crates/pdm-worker` | Durable job queue (Postgres) runner; `design.analyze` via `solvecraft-cli eval` |
| `crates/pdm-mcp` | PDM MCP endpoint (Phase 5; stub) |
| `crates/pdm-erp` | Odoo outbox connector (Phase 5; stub) |
| `apps/pdm-server` | Binary: API + in-process worker; also hosts the Phase-1 E2E test |
| `apps/pdm-cli` | Ops CLI: `migrate`, `create-org`, `create-user`, `create-token` |
| `docker/` | `Dockerfile` (multi-stage: builds `solvecraft-cli` + PDM binaries) and `pdm-dev.yml` |
| `e2e.sh` | Phase-1 acceptance test: boots Postgres, bootstraps, serves, runs `e2e_vault` |

CAD-side integration (Phase 2+): `crates/pdm-client` (vault HTTP client +
workspace paths), `Document.doc_id` (stable identity, format `solvecraft/6`),
and the future `pdm.*` engine commands + UI panel.

## Requirements

- Rust (stable)
- Docker (Postgres 16; MinIO for S3-backed storage)
- Optional: `solvecraft-cli` on `PATH` or `PDM_SOLVECRAFT_CLI` for worker analysis

## Quick start (development)

```sh
# 1. Boot Postgres + MinIO + PDM server (server runs migrations on boot)
docker compose -f docker/pdm-dev.yml up -d

# 2. Bootstrap an org, an admin, and a token (adjust ports if 15432 is taken)
pdm-cli --database-url postgresql://pdm:pdm@localhost:15432/pdm create-org --name Acme
pdm-cli --database-url ... create-user --org acme --email admin@acme.com --display-name Admin --role admin
pdm-cli --database-url ... create-token --org acme --user admin@acme.com
# → prints a `pdm_...` bearer token

# 3. Open the web portal
#    http://localhost:8080  (or 18080 on the host if 8080 is taken)
#    paste the token on the "Connect" screen.
```

## Configuration (server)

| Variable | Default | Purpose |
|----------|---------|---------|
| `PDM_DATABASE_URL` | `postgresql://pdm:pdm@localhost:15432/pdm` | Postgres connection |
| `PDM_STORAGE_LOCAL` | – | Local blob dir (wins over S3 when set) |
| `PDM_S3_ENDPOINT` / `PDM_S3_BUCKET` / `PDM_S3_ACCESS_KEY` / `PDM_S3_SECRET_KEY` | – | S3/MinIO blob backend |
| `PDM_WORKER` | `1` | `0` to run a dedicated worker instead of in-process |
| `PDM_WORKER_POLL_MS` | `500` | Job queue poll interval |
| `PDM_SOLVECRAFT_CLI` | `solvecraft-cli` | Binary for `design.analyze` jobs |
| `RUST_LOG` | `info` | Log filter |

Server flags: `pdm-server --listen 0.0.0.0:8080` (port overridable).

## Tests

```sh
# Unit tests (core rules, storage round-trips, client helpers)
cargo test --workspace

# Phase-1 end-to-end acceptance test (boots Postgres via compose; :18080 on host)
./e2e.sh
```

## API (Phase 1)

All routes are Bearer-token authenticated except `/healthz`, `/readyz`, and
the portal at `/`.

- `GET /me` — identity
- `GET|POST /projects` — list / create projects
- `GET|POST /projects/{id}/items` — list / create items (auto part number per classification)
- `GET /items/{id}` — item + file + lock + versions + head version
- `POST /items/{id}/checkout` / `DELETE /items/{id}/checkout` — acquire / release the exclusive lock
- `PUT /blobs` — upload bytes (content-addressed, SHA-256, dedup)
- `POST /items/{id}/checkin` — commit a version `{sha256, commit_message, base_version, filename?, mime_type?}`
- `GET /items/{id}/versions`, `GET /versions/{id}/blob` — history / download (with `Content-Digest`)
- `POST /items/{id}/release` — cut a revision from the head version
- `GET /search?q=`, `GET /audit?item=`
- `GET|PUT /items/{id}/properties[/{key}]` — typed metadata

Errors: `{"error": {"code": "...", "message": "..."}}` with stable codes
(`auth_failed`, `forbidden`, `not_found`, `conflict`, `integrity_mismatch`,
`validation`, `db_error`, `storage_error`, `internal`).
