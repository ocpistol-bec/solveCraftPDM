# PDM Integration Points

Where the PDM system plugs into the existing SolveCraft codebase, with the exact seam for each
integration and the extension pattern to follow. This is the *how-to-attach* companion to
`repository-audit.md` (what exists) and `system-design.md` (the PDM design itself).

Principle throughout: **extend existing abstractions, don't duplicate.** PDM treats the
`.solvecraft` file as an opaque, content-addressed, immutable blob and only *inspects* it through
the headless CLI. The CAD kernel never depends on PDM server code.

---

## 1. CAD-side: PDM client crate (main workspace)

**Add** `crates/pdm-client` (L3, between `io` and `engine`-adjacent; depends on `serde`,
`serde_json`, `reqwest`, but **not** on egui). This is the only CAD-workspace addition that ships
in the desktop/web binaries.

Responsibilities:
- Hold vault connection state: server URL, project, current document's PDM identity, checkout
  status, base version, last-sync hash.
- Speak the PDM REST API (HTTP) for open-from-vault / save-to-vault / checkin / checkout /
  get-latest / history / references / bom.
- Own the **local workspace** (offline cache) under the platform user-data dir, mirroring the
  recovery pattern: `<user-data>/pdm/workspace/<org>/<project>/…`.
- Expose a `Vault` trait so the UI can call it without knowing about HTTP; a `NoVault` impl gives
  the "ordinary local files" mode (toDo §8: local-file mode and managed-vault mode).
- Credentials: store **secrets in the OS credential store** (`keyring`), non-secret server
  selection in prefs. Never write credentials into the `.solvecraft` document.

Layering note: `crates/pdm-client` must stay wasm-buildable (web app) — use `reqwest` with the
`json`/`rustls` features and guard any non-wasm-only paths.

## 2. CAD-side: PDM engine commands (`crates/engine/src/cmd/pdm.rs`)

Follow the existing "everything is a command" pattern. Add
`pub static COMMANDS: &[CommandSpec]` and register it in `command_specs()`
(`crates/engine/src/cmd/mod.rs`). Commands use the free `pdm.` prefix (toDo §8):

| Command | Params (JSON) | Behaviour |
|---------|---------------|-----------|
| `pdm.login` | `{server, user?, pass?}` | authenticate against PDM API; cache token (via `pdm-client`) |
| `pdm.open` | `{item, revision?}` | fetch immutable blob from vault, `read_design` into `s.doc`, set `s.path` to a workspace path, record PDM identity in session state |
| `pdm.checkout` | `{}` | exclusive checkout of current doc's item; record base version + lease |
| `pdm.checkin` | `{message?, new_revision?}` | verify integrity (SHA-256), base version, permissions; commit a new immutable `file_version`; release lock |
| `pdm.get_latest` | `{}` | fetch latest, reconcile with local, open |
| `pdm.undo_checkout` | `{}` | discard local edits, re-lock to server version |
| `pdm.history` | `{limit?}` | list file versions / revisions for current doc |
| `pdm.references` | `{recursive?}` | list where-used / uses for current doc (from PDM, not just in-doc occurrences) |
| `pdm.bom` | `{mode: indented\|flattened}` | recursive BOM from the PDM dependency graph |
| `pdm.properties` | `{}` | read/write managed engineering metadata (part number, material, …) |
| `pdm.release` | `{}` | move current item to a released revision (server-controlled) |

These commands are **thin**: they call into `pdm-client` (network/workspace) and then reuse the
existing `file.*`/`doc.open` primitives for the actual bytes. Because `Session::execute`
already snapshots + `catch_unwind`s + rolls back on error (`crates/engine/src/lib.rs:~257`), PDM
commands inherit the "never crash / atomic" guarantees for free.

Session state: add a small `Session.pdm: Option<PdmSession>` field (or an `Arc<Mutex<…>>`) to hold
the current PDM identity (org/project/item/version), base version, and lease id. This is the
volatile link that replaces today's in-memory `Session.path` as the "which file am I editing"
source of truth in managed mode.

## 3. CAD-side: stable document identity (`crates/doc`)

Introduce a **`doc_id: Uuid`** on `Document` (a new `uuid` dependency in `crates/doc` only, or a
thin newtype to keep wasm lean). This is the single most important CAD-side change:

- Add `pub doc_id: Uuid` to `Document` (`crates/doc/src/document.rs:1305`).
- Bump the format version (`FORMAT: 5 → 6`, `crates/doc/src/format.rs:27`) and add a `v5_to_v6`
  migration step that generates a deterministic id for legacy files on first save (e.g. from the
  SHA-256 of the document + original name, or a random one seeded once and persisted).
- `doc_id` is the PDM "part/document identity" anchor and the key of the `pdm://` reference scheme.
- Existing designs keep working (local mode ignores `doc_id`); managed mode requires it.

This preserves backward compatibility: old files load (migration fills `doc_id`), new files carry
it. It is additive, never a rewrite.

## 4. CAD-side: UI (PDM panel + menu items)

**Where a PDM panel goes** — two clean options, both using existing machinery:
- **New workspace tab "PDM":** add to `TABS` (`crates/ui-egui/src/toolbar.rs:10`), add a match arm
  in `panels()` (`toolbar.rs:13`), and register PDM commands with
  `CommandSpec` `tab:"PDM"`. Reuses the entire toolbar/panel system.
- **Left panel section:** add a collapsible section inside `browser.rs` (pattern: `is_open(app,
  key, default)`, `browser.rs:402`) or a second `egui::Panel::left` in `SolveApp::ui()`
  (`lib.rs:699`). Show/hide toggles live in `UiState` (`lib.rs:62`), flippable via `ui.set`.

Panel contents (toDo §8): server selection, auth status, project browser, file browser, checkout
status, version/revision, local-modified flag, checkin/checkout buttons, refresh.

**File menu** (`crates/ui-egui/src/toolbar.rs:133`, `file_menu`): add items that run the new
commands instead of the rfd pickers:
- "Open from Vault…" → `pdm.open` (opens a vault browser, not `pick_open`)
- "Save to Vault / Check-in" → `pdm.checkin`
- "Check-out" / "Get Latest" / "Undo Check-out" → the matching commands
- Keep the existing local Open/Save/Export for unmanaged mode.

**Services host-injection** (`apps/solvecraft/src/main.rs:54–72`): a PDM-aware host can branch
inside `pick_open`/`pick_save` (local picker *or* vault browser). The web host already proves the
override pattern (`apps/solvecraft-web/src/main.rs:362–371`).

**Prefs** (`SolveApp::prefs()`, `lib.rs:270`): add a `pdm` section for **non-secret** server
selection (url, org, default project). Secrets go to the keyring, not prefs (web app keeps prefs in
`localStorage`, so anything there is visible).

## 5. CAD-side: MCP PDM tools (`crates/mcp`)

Registration pattern (from the audit): add `tool(...)` entries in `tool_definitions()`
(`crates/mcp/src/tools.rs:52`) and `match` arms in `dispatch()` (`tools.rs:344`). Two options for
wiring PDM:
- **(a)** Route PDM control methods through the existing `Backend::call` and let the app handle
  `pdm_*` methods — works with both `Remote` (bridged app) and `Headless`.
- **(b)** Add a PDM-aware `Backend` method — but both `Remote` and `Headless` must implement it
  (`crates/mcp/src/backend.rs:15`).

Proposed PDM tools (toDo §18): `pdm_search`, `pdm_get_item`, `pdm_get_file_history`,
`pdm_get_bom`, `pdm_where_used`, `pdm_compare_revisions`, `pdm_create_draft`,
`pdm_create_change_request`, `pdm_request_export`, `pdm_get_job_status`.

The PDM server also exposes its **own** MCP endpoint (`pdm-mcp` crate in the PDM workspace) so an
agent can manage the vault directly; this CAD-side set is for agents driving the *design* through
the PDM-managed document.

## 6. CAD-side: worker hook (headless CLI)

The PDM **background worker** does not link the CAD kernel; it invokes `solvecraft-cli` as a
subprocess (toDo §15 "Use SolveCraft's existing headless CLI"):
- Export: `solvecraft-cli run <workspace>.solvecraft --out artifact.step` (and `.stl`/`.3mf`).
- Measure: `solvecraft-cli eval <file>` → JSON measurements (volume, mass with material density).
- Thumbnail: `solvecraft-cli snapshot <file> --out thumb.png --width 512`.
- BOM geometry check: `solvecraft-cli eval` + the PDM dependency graph.

Keep the CLI on the worker's build image; the worker parses the CLI's JSON output. This isolates
CAD kernel upgrades from the PDM service build.

## 7. PDM server workspace (new, under `apps/pdm/`)

To honor the user's placement and keep the CAD kernel lean, PDM is a **nested Cargo workspace**
rooted at `apps/pdm/` (its own `Cargo.toml` with `[workspace]`), **excluded** from the parent
(`exclude = ["vendor", "apps/pdm"]`). The PDM web portal (React/TS, Vite) lives alongside in
`apps/pdm/`. See `system-design.md` §2 for the crate layout and the exact parent-manifest edits.

## 8. Web portal (separate app, not the WASM CAD app)

The `solvecraft-web` app is the *CAD* browser app (WASM + OPFS). The **PDM web portal** (toDo §4 D)
is a *new* React/TypeScript app: project/file browsing, search, revision history, BOM viz, workflow
approvals, admin, change orders, audit. It talks to the PDM API over HTTPS and embeds a model
viewer for previews (GLTF/3MF + the worker-generated thumbnails). It is **not** the egui viewport
and does not require the CAD app.

## 9. Vault reference scheme (`pdm://`)

Managed cross-document references use a vault URI (toDo §9):
`pdm://<org>/<project>/<doc_id>[@<revision>]`.
- `doc_id` (from §3) is the stable identity; revision is a separate, resolvable component
  (floating `@latest` vs. pinned `@A`).
- PDM owns the parent→child edges and resolves a managed assembly consistently on open.
- In managed mode, an assembly's occurrences that point at other managed documents carry a
  `pdm://` reference rather than an embedded copy; in local/STEP mode they stay embedded
  (backward compatible).

## 10. Integration decision summary

| Seam | Change | File / pattern |
|------|--------|----------------|
| Document identity | add `doc_id: Uuid` + `v5_to_v6` migration | `crates/doc/src/document.rs`, `format.rs` |
| PDM client | new `crates/pdm-client` | L3, wasm-safe |
| PDM commands | `cmd/pdm.rs`, `pdm.*` namespace | `crates/engine/src/cmd/mod.rs` registration |
| Session state | `Session.pdm` | `crates/engine/src/lib.rs` |
| UI panel | "PDM" tab + left section + file menu | `ui-egui` `toolbar.rs`, `browser.rs`, `lib.rs` |
| Prefs | non-secret `pdm` section; secrets → keyring | `ui-egui` `lib.rs:270`, `apps/solvecraft` |
| MCP | PDM tools via `Backend` | `crates/mcp/src/tools.rs`, `backend.rs` |
| Worker | `solvecraft-cli` subprocess | `apps/solvecraft-cli` |
| Web portal | new React/TS app | `apps/pdm/` |
| Server | nested workspace `apps/pdm` | parent `Cargo.toml` exclude |
