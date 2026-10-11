#!/usr/bin/env bash
#
# PDM Phase-1 end-to-end acceptance test.
#
# Boots a clean Postgres (via docker compose), migrates, bootstraps an org +
# admin + token, starts the PDM server with local-fs storage, runs the Rust
# e2e test that exercises the full vault flow (2 users, project, upload,
# versioning, checkout/lock, checkin, stale-checkin rejection, download +
# SHA-256 integrity, audit trail, search), then tears the server down.
#
# Usage:
#   apps/pdm/e2e.sh
#
# Requires: docker (for Postgres), rust/cargo, the PDM binaries built
# (or it builds them), and the solvecraft-cli for the worker (optional; if
# absent, design.analyze jobs are marked skipped).
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"   # repo root
PDM="$ROOT/apps/pdm"
cd "$PDM"

# ── Configurable knobs ───────────────────────────────────────────────────
DBHOST="${PDM_E2E_DBHOST:-localhost}"
DBPORT="${PDM_E2E_DBPORT:-15432}"
DB_URL="postgresql://pdm:pdm@${DBHOST}:${DBPORT}/pdm"
SERVER_PORT="${PDM_E2E_PORT:-18080}"
STORE_DIR="/tmp/pdm-e2e-store"
COMPOSE="docker compose -f docker/pdm-dev.yml"

log() { echo "[e2e] $*"; }

# ── 1. Postgres ──────────────────────────────────────────────────────────
log "ensuring Postgres is up (compose service: postgres)…"
$COMPOSE up -d postgres >/dev/null
for _ in $(seq 1 60); do
  docker exec docker-postgres-1 pg_isready -U pdm -d pdm >/dev/null 2>&1 && break
  sleep 1
done
docker exec docker-postgres-1 pg_isready -U pdm -d pdm >/dev/null

# ── 2. Build binaries if missing ─────────────────────────────────────────
if [[ ! -x target/debug/pdm-cli || ! -x target/debug/pdm-server ]]; then
  log "building pdm-cli + pdm-server…"
  cargo build -p pdm-cli -p pdm-server
fi

# ── 3. Clean schema + bootstrap ─────────────────────────────────────────
log "resetting schema and bootstrapping org/admin/token…"
# psql runs *inside* the container, so use its local socket (default port 5432).
docker exec -e PGPASSWORD=pdm docker-postgres-1 psql -U pdm -d pdm \
  -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;" >/dev/null
PDM_DATABASE_URL="$DB_URL" ./target/debug/pdm-cli migrate
PDM_DATABASE_URL="$DB_URL" ./target/debug/pdm-cli create-org --name Acme
PDM_DATABASE_URL="$DB_URL" ./target/debug/pdm-cli create-user \
  --org Acme --email admin@acme.com --display-name Admin --role admin
TOKEN="$(PDM_DATABASE_URL="$DB_URL" ./target/debug/pdm-cli create-token \
  --org Acme --user admin@acme.com --name e2e-admin 2>/dev/null \
  | sed -n 's/^token: //p')"
[[ -n "$TOKEN" ]] || { log "ERROR: failed to create admin token"; exit 1; }
log "admin token: $TOKEN"

# ── 4. Start the server ─────────────────────────────────────────────────
# Free the port first so a stale server can't shadow the one we start here.
fuser -k "${SERVER_PORT}/tcp" 2>/dev/null || true
sleep 1
rm -rf "$STORE_DIR"
PDM_DATABASE_URL="$DB_URL" PDM_STORAGE_LOCAL="$STORE_DIR" \
PDM_WORKER=1 RUST_LOG=warn \
  ./target/debug/pdm-server --listen 0.0.0.0:"$SERVER_PORT" &
SRV_PID=$!
log "started server (pid $SRV_PID); waiting for /healthz…"
for _ in $(seq 1 40); do
  curl -s "http://127.0.0.1:${SERVER_PORT}/healthz" >/dev/null 2>&1 && break
  sleep 0.5
done
curl -s "http://127.0.0.1:${SERVER_PORT}/healthz" >/dev/null
trap 'log "stopping server (pid $SRV_PID)"; kill $SRV_PID 2>/dev/null || true; wait $SRV_PID 2>/dev/null || true' EXIT

# ── 5. Run the e2e test ─────────────────────────────────────────────────
log "running e2e_vault test…"
PDM_E2E=1 PDM_E2E_BASE="http://127.0.0.1:${SERVER_PORT}" PDM_E2E_ADMIN_TOKEN="$TOKEN" \
  cargo test -p pdm-server --test e2e_vault -- --nocapture --ignored

log "PASS: PDM Phase-1 end-to-end acceptance test succeeded."
