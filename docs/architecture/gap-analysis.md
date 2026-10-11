# PDM Gap Analysis

What is missing between the current SolveCraft codebase and the PDM requirements in
`apps/pdm/toDo.txt`. Each gap is scored: **CAD-side** (change SolveCraft), **PDM-side** (new
server code), or **both**, and tagged with the effort tier it implies. This is the basis for the
dependency-ordered backlog in `implementation-roadmap.md`.

---

## A. Identity & reference model

| Gap | Detail (from audit) | Owner | Tier |
|-----|--------------------|-------|------|
| No document-level stable ID | All ids are per-doc `u64` from `next_id`; identity today = `name` + volatile `Session.path` | CAD | S |
| No cross-document references | Assemblies are single-file; imports are embedded copies | CAD | M |
| No where-used / uses table | Implicit edges only inside one document | PDM | M |
| No `pdm://` vault URI scheme | — | PDM | S |

**Consequence:** PDM must own the document catalog and the reference graph. CAD contributes a
stable `doc_id` (small) and, optionally, managed cross-doc occurrences. BOM/where-used is computed
in PDM from the stored reference graph + the CAD component/occurrence model of each doc.

## B. File versioning & revision

| Gap | Detail | Owner | Tier |
|-----|--------|-------|------|
| No version/revision concept | A save overwrites in place; only crash-recovery `Entry` lists prior states | PDM | M |
| No immutable committed versions | `write_atomic` keeps a `.bak` but that's not versioning | PDM | M |
| No revision scheme (A/B/C vs 1/2/3) | — | PDM | S |
| No released-revision immutability | — | PDM | M |

**Consequence:** PDM introduces `file_versions` (immutable, content-addressed by SHA-256) and
`file_revisions` (engineer-released, pinned to a version). CAD changes are local until check-in.

## C. Checkout / lock / concurrency

| Gap | Detail | Owner | Tier |
|-----|--------|-------|------|
| No checkout/check-in | — | PDM | M |
| No exclusive lock / lease | — | PDM | M |
| No stale-client protection | A check-in must verify base version + hash + permissions | PDM | S |
| No crash/lease recovery | — | PDM | S |

**Consequence:** server-side `file_locks` with atomic acquire, lease expiry, and base-version
validation at check-in. CAD records the base version + hash in `Session.pdm`.

## D. Storage

| Gap | Detail | Owner | Tier |
|-----|--------|-------|------|
| No object-storage abstraction | Files live on the local FS / OPFS | PDM | M |
| No content addressing (SHA-256) | — | PDM | S |
| No multipart upload / dedup / retention / GC | — | PDM | M |

**Consequence:** `pdm-storage` crate with an `ObjectStore` trait (S3/MinIO/localfs). CAD blobs are
immutable once committed; metadata in Postgres.

## E. Engineering data model (PDM-only, greenfield)

| Gap | Detail | Owner | Tier |
|-----|--------|-------|------|
| Part numbers / numbering sequences | toDo §10 | PDM | M |
| Materials / UoM / density for mass calc | CAD has `materials: BTreeMap<String,String>` (names only); no density table | PDM | M |
| Typed custom properties + validation | — | PDM | M |
| EBOM engine (recursive, indented/flattened, phantoms, quantities) | CAD gives per-doc occurrences; PDM stitches the graph | PDM | L |
| Lifecycle states + transitions + approvals | toDo §12 | PDM | M |
| ECR/ECO/ECN change management | toDo §13 | PDM | L |
| Search (full-text) + where-used | toDo §14 | PDM | M |

**Consequence:** most of the PDM domain is net-new Postgres + Rust domain logic. CAD contributes
measurements (volume via `inspect.measure`/`eval`), the component/occurrence structure, and the
`doc_id`.

## F. CAD assembly foundations (toDo §9 — "critical architectural requirement")

| Gap | Detail | Owner | Tier |
|-----|--------|-------|------|
| Managed cross-file assembly open | Resolve all `pdm://` references consistently; fetch missing docs | CAD+PDM | L |
| Circular-reference detection | in-doc `add_occurrence` cycle-checks; cross-doc needs PDM graph check | PDM | S |
| Reference replacement / update | Swap a part for a new revision, propagate | PDM | M |
| Floating-ref resolution + frozen release manifest | Pin `@latest` into a concrete version set at release | PDM | M |

**Consequence:** this is the highest-risk CAD-side work. Phase 3 (assemblies & dependencies)
deliberately lands after Phase 1–2 foundation. The CAD-side needs: a way to mark an occurrence as a
*managed reference* (vs. embedded) and to hold its `pdm://` target + revision policy.

## G. Client / UX (toDo §8, §19)

| Gap | Detail | Owner | Tier |
|-----|--------|-------|------|
| No PDM panel in desktop UI | — | CAD | M |
| No vault browser for open/save | rfd pickers only | CAD | M |
| No credential store | eframe prefs only; web prefs in localStorage | CAD | S |
| No offline workspace manager | recovery folder is the closest analog | CAD | L |
| No web PDM portal | WASM app is CAD-only | PDM | L |

**Consequence:** `pdm-client` crate + UI panel + keyring + local workspace. Local-file mode must
remain first-class (CAD works with no PDM server).

## H. Integration & automation (toDo §15–18)

| Gap | Detail | Owner | Tier |
|-----|--------|-------|------|
| No preview/export pipeline | Building block exists (`solvecraft-cli snapshot`/`run`/`eval`) | PDM | M |
| No drawings / manufacturing doc association | toDo §16 — model association first, drawings later | PDM | M |
| No Odoo ERP connector | toDo §17 | PDM | L |
| No PDM MCP interface | toDo §18; CAD MCP registration pattern exists | PDM | M |
| No durable job queue | toDo §4 C — start Postgres-backed jobs, no broker yet | PDM | M |

## I. Cross-cutting

| Gap | Detail | Owner | Tier |
|-----|--------|-------|------|
| Auth (OIDC) + RBAC + org isolation | toDo §20 | PDM | L |
| Audit trail | toDo §1 | PDM | M |
| Backup/restore + integrity + GC | toDo §21 | PDM | M |
| Performance targets + benchmarks | toDo §22 | PDM | M |
| Deployment (compose, swarm, TLS) | toDo §23 | PDM | M |
| Wasm compatibility of CAD-side additions | `pdm-client` + `doc_id` must build for wasm | CAD | S |

---

## What is *not* a gap (don't rebuild)

- Parametric document model + versioned JSON format → **reuse** for storage.
- Headless re-evaluation + CLI exports/measurements/thumbnails → **reuse** for the worker.
- Single-file assembly model (components/occurrences/joints) → **reuse** per-document; PDM stitches
  the graph.
- Command registry + `Session::execute` atomicity + catch_unwind → **reuse** for `pdm.*` commands.
- Control channel + MCP → **extend**, not replace.
- STEP round-trip preserves assembly → **reuse** for external-STEP ingestion / reference-only parts.

## Effort legend

- **S** — a day or two, well-understood.
- **M** — a focused arc with tests; bounded.
- **L** — multi-arc; needs its own design pass + acceptance tests before build.

## Top risks

1. **Cross-document assembly resolution** (F) — the one CAD-side change that could destabilize the
   kernel if done carelessly. Mitigate: keep embedded mode as the default, add managed refs
   opt-in, exhaustively test.
2. **`doc_id` migration** (A) — must be backward compatible. Mitigate: `v5_to_v6` fills id on load;
   local mode ignores it.
3. **Scope creep across phases** — the toDo explicitly warns against it. Mitigate: the phase
   gates in `implementation-roadmap.md` and the acceptance test per phase.
