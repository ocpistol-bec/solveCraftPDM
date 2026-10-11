# SolveCraft Repository Audit

Phase 0 — factual inventory of what exists in the SolveCraft codebase, to ground the PDM
integration. Every claim cites `file:line` and the exact Rust identifier. Verified against the
tree as it stands (v0.1.1). This is a *what-is* document; interpretation lives in
`gap-analysis.md` and `system-design.md`.

---

## 1. Workspace & layering

Cargo workspace (`Cargo.toml`): `members = ["crates/*", "apps/*", "xtask"]`, `exclude = ["vendor"]`.
Edition 2024, `unsafe_code = "forbid"` workspace-wide.

| Layer | Crate | Notes |
|-------|-------|-------|
| L0 | `crates/geom` | pure geometry, `Vec2/Vec3/Quat` |
| L1 | `crates/sketch`, `crates/kernel`, `crates/render` | constraint solver; B-rep (uses `truck*`); rasterizer |
| L2 | `crates/doc` | the `Document` model + incremental evaluator |
| L3 | `crates/io` | file read/write, import/export |
| L4 | `crates/engine` | `Session`, command registry, everything-is-a-command |
| L5 | `crates/ui-egui` | desktop UI (egui/eframe), control channel dispatch |
| — | `crates/mcp` | MCP server library (stdio JSON-RPC) |
| apps | `solvecraft`, `solvecraft-cli`, `solvecraft-web`, `pdm` | host binaries |

`vendor/` holds `truck-*` patches (Apache-2.0, see `ATTRIBUTION.md`); only `crates/kernel` may use
`truck`. Layering is enforced by `cargo xtask layers`. Nothing below L5 depends on
egui/eframe/winit/wgpu/rfd. **PDM must respect this**: the CAD kernel stays lean and wasm-buildable.

## 2. Native document format (`.solvecraft`)

- **Format:** pretty-printed JSON of `Document` (`serde_json::to_string_pretty`), not binary/zip.
- Root struct: `pub struct Document` — `crates/doc/src/document.rs:1305`. Notable fields:
  `format: String`, `name: String`, `units: String`, `params: Vec<Param>`,
  `features: Vec<Feature>` (the parametric timeline), `marker: Option<usize>` (rollback),
  `next_id: u64` (id counter), `components: Vec<Component>`, `occurrences: Vec<Occurrence>`,
  `assembly: joints::Assembly`, `sheet`, `plastic`, `parts: BTreeMap<u64, StandardPart>`,
  `configs`, `materials: BTreeMap<String,String>`, `appearances`, `body_components`, `browser_*`,
  `named_views`.
- **Version marker:** `Document.format = "solvecraft/N"`. `pub const FORMAT: u32 = 5;` at
  `crates/doc/src/format.rs:27`; `format_string(n)`, `version_of(s)` in the same file.
- **Migrations:** `crates/doc/src/format.rs` defines `JSON_STEPS: [JsonStep; 4]` (each upgrades
  `i+1 → i+2`): `v2_to_v3` (body colours → named appearances), `v3_to_v4` (joint `flip` polarity),
  `v4_to_v5` (adds `participants:["*"]` to features in multi-component designs). Files newer than
  `FORMAT` are refused. `normalise()` backfills feature input names + missing occurrences on load.
- **Read:** `Document::from_json` (`document.rs:1517`) → `crate::format::read` (parses, migrates,
  validates `check_numbers`, occurrence rigidity, caps at `MAX_FEATURES`/`MAX_PARAMS` = 10 000,
  `document.rs:14`).
- **Write:** `Document::to_json` (`document.rs:1524`).
- **Disk I/O** (`crates/io/src/lib.rs`): `DESIGN_EXT = "solvecraft"` (line 27),
  `MAX_DESIGN_BYTES = 256 MiB` (line 29), `read_design(bytes) -> Result<Document>` (line 346),
  `write_design(doc) -> Vec<u8>` (line 355). `write_atomic` + `.bak` in `crates/io/src/safe_write.rs`
  (temp + fsync + rename). The B-rep model is **not** stored — only the parametric `Document`;
  `ModelState` is recomputed on load.

**PDM implication:** the native file is a versioned, self-describing JSON blob — ideal for
content-addressed immutable storage. The PDM worker can re-open and re-evaluate it headlessly.

## 3. Identifiers

- **No UUIDs anywhere** in `crates/` (zero `uuid` matches). All ids are plain `u64` from
  `Document.next_id`, per-document only:
  - `Feature.id: u64` — `document.rs:1246`
  - `Component.id: u64` — `document.rs:813` (root component = id 0, not stored)
  - `Occurrence.id: u64` — `assembly.rs:109`
  - `Joint.id: u64` — `joints.rs:113`
- **Stable within a file:** yes — ids + `next_id` are serialized, so they survive save/load.
- **Stable across files:** *not found*. A document's identity today is `Document.name` (mutable
  string) + the live file path (`Session.path`, in-memory only). There is **no `doc_id` field**.
- Persistent *geometry* identity is name-based: body names, `face_names`/`edge_names` on `Feature`
  (`crates/doc/src/naming.rs`).

**PDM implication:** PDM must introduce a stable document identity (a `doc_id` UUID) and own all
cross-document linkage. Internal `u64` ids are meaningless across files.

## 4. Assemblies & references

- **Single-file assemblies: fully supported.** `Component` + `Occurrence` form a tree by
  `parent` (0 = root). Each occurrence has `transform: Mat` (4×4) and `grounded`. References are by
  in-document `u64`, never by file path.
- Helpers: `crates/doc/src/assembly.rs:307` — `occurrence_of`, `component_transform`, `placements`,
  `body_frames`, `component_within`, `add_occurrence` (cycle-checked, capped 100 000).
- `Assembly` (`joints.rs:154`): `joints: Vec<Joint>`, `origins`, `links`, `contact_sets`,
  `studies`, `exploded`. `Joint` (`joints.rs:113`) connects two in-document occurrences via
  `JointOrigin { occurrence: u64, snap: Snap }` (`joints.rs:87`).
- **Cross-document references: not found.** Imported STEP/mesh geometry is *embedded* in the
  document (`FeatureKind::Import { file, step: String, components: Vec<StepNode> }` and
  `MeshImport { file, MeshData }`), so a design reopens without its source file.
  `FeatureKind::Import.file` / `MeshImport.file` are display-only filenames.
- **Where-used / uses: not found** (zero `where_used` matches). Implicit "uses" edges exist only
  inside one document: `Extrude.sketch`, `Sweep.sketch`, `Loft.sections[].sketch`,
  `Pattern.features`, `Mirror.features`, `Combine.tools`, `Occurrence.component/parent`,
  `Joint.a/b`.

**PDM implication:** PDM owns parent/child document linkage, the dependency graph, where-used, and
BOM. The CAD-side foundation (components/occurrences) exists but is single-file.

## 5. Open / save / export

All in `crates/engine/src/cmd/file.rs` (registry is "everything is a command"):

| Command id | Function | Behaviour |
|-----------|----------|-----------|
| `file.new` | `new_doc` | empty design |
| `doc.open` | `open` | reads `.solvecraft` via `read_design`, or STEP/mesh → `add_step`/`add_mesh`; sets `s.path` |
| `file.save` | `save`/`write` | `write_design` + `write_atomic` (`.bak`); sets `s.path`, `mark_saved` |
| `file.save_as` | `save_as` | same, new path |
| `file.export` | `export`/`export_to` | STL/OBJ/STEP/3MF/IGES to a path; assembly STEP only when occurrences exist |
| `file.save_mesh` | `save_stl` | STL of mesh bodies |
| `file.insert_step` / `file.insert_mesh` | `insert_step`/`add_step`, `insert_mesh`/`add_mesh` | embed geometry |
| `doc.recovery_list` / `doc.recover` / `doc.recovery_discard` | — | crash recovery (see below) |

- The live session carries the path: `Session.path: Option<String>` — `crates/engine/src/lib.rs:142`.
  The path is **not** persisted in the document.
- **Recovery** (`crates/engine/src/recovery.rs`): `Entry { id, path: Option<String>, name,
  saved_at, features }` (line 42), `Autosaver` (line 117) with `save`/`tick`/`close`,
  `default_dir()` (line 21, platform user-data dir + `/recovery`). This is the only on-disk
  structure that lists multiple related documents — crash recovery, not a PDM registry.

## 6. STEP / STL / 3MF / OBJ / IGES

- **Import** (`crates/io/src/lib.rs` + `crates/kernel`):
  `is_step_path` (line 52: `.step/.stp/.igs/.iges`), `is_mesh_path` (line 108:
  `.3mf/.stl/.obj`), `step_import_feature(bytes, file) -> StepFeature` (line 76) → kernel
  `step_import_shared`/`step_import`/`step_validate`/`step_orientation_errors`
  (`kernel/src/step_in/mod.rs`), `iges_to_step` (`kernel/src/iges_in.rs`). Mesh readers:
  `read_3mf` (`io/src/threemf.rs`), `read_obj` (`io/src/obj.rs`), `read_stl` (line 360,
  binary+ASCII, `MAX_STL_TRIANGLES = 20_000_000`). Caps: `MAX_STEP_BYTES = 512 MiB`.
- **Export** (`crates/io/src/lib.rs` + `kernel/src/step_out.rs`):
  `pub enum Format { StlBinary, StlAscii, Obj, Step, Iges, ThreeMf, Design }` + `from_name`.
  `pub fn export(state, bodies, format, name) -> Result<Vec<u8>>` (line 265). Assembly-preserving
  export: `step_assembly(doc, local, name)` (line 321) → `step_export_products`
  (`step_out.rs:1235`), building an `ExportProduct` tree; flat parts via `step_export_bodies`
  (`step_out.rs:1460`). `Format::Design` is deliberately rejected ("save the document instead").

**PDM implication:** assembly structure *does* survive STEP round-trip — relevant for the
"reference-only" and external-STEP ingestion paths.

## 7. Command architecture

- **`CommandSpec`** — `crates/engine/src/cmd/mod.rs:34`:
  `id: &'static str`, `label`, `tab` (toolbar tab), `panel`, `icon`, `shortcut`,
  `params: &'static str` (one-line human doc, **not** a JSON schema), `enabled: Enabled`
  (`fn(&Session) -> Result<(), String>`), `run: Run` (`fn(&mut Session, &Value) -> Result<Value>`),
  `undoable: bool`.
- **Registry:** each command module exposes `pub static COMMANDS: &[CommandSpec]`;
  `command_specs()` (line ~134) aggregates them into a flat `Vec<&'static CommandSpec>`;
  `find_command(id)` (line ~168) does a linear scan with legacy-id remap
  (`crates/engine/src/legacy_ids.rs`). **No `HashMap`.**
- **Param validation:** imperative, inside each `run`, via `crates/engine/src/params.rs`
  helpers: `bad`, `num`, `str_`, `bool_`, `req_vec2/vec3`, `vec2_list`, `expr/req_expr`,
  `string_list`, `finite` (abs < 1e9).
- **Command id namespaces (~300 total):** `sketch.*` (95), `solid.*` (34), `construct.*` (18),
  `timeline.*` (12), `parameters.*` (12), `inspect.*` (12), `joint.*` (11), `component.*` (11),
  `sheet.*` (10), `plastic.*` (7), `file.*` (7), `contact.*` (7), `motion.*` (5), `config.*` (5),
  `canvas.*` (5), `browser.*` (5), `view.*` (4), `parts.*` (4), `explode.*` (4), `doc.*` (4),
  `surface.*` (3), `occurrence.*` (3), `model.*` (3), `appearance.*` (3), `select.*`/`selection.*`
  (3), `material.*` (2), `edit.*` (2), `document.*` (2), `engine.*` (1), `expr.*` (1), `face.*`
  (1), `body.*` (1). **Convention: `<area>.<thing>[.<variant>]`, lowercase, dot-separated. The
  `pdm.` prefix is free.**

## 8. Execution model

- **`Session`** — `crates/engine/src/lib.rs:~210`: `doc: Arc<Document>`, `model: Model`,
  `undo: Vec<Snapshot>` (cap `MAX_UNDO = 200`), `redo`, `active_sketch`, `selection`,
  `path: Option<String>`, `saved: Arc<Document>` (dirty tracking), `revision: u64`,
  `active_component: u64`, etc. `Snapshot { label, doc, active_sketch }` (~line 185).
- **`Session::execute(id, params)`** (~line 257): `find_command` → `EngineError::UnknownCommand`;
  `spec.enabled(self)` → `EngineError::Disabled`; take `before` `Snapshot`; run
  `(spec.run)(self, params)` under `catch_unwind`; panic → `EngineError::Internal`. If doc changed
  and `undoable`: push `before` to undo, clear redo, `refresh()` (re-evaluate). On `Err` with a
  changed doc: restore `before`, `refresh()`.
- **`EngineError`** (`lib.rs:30–42`): `UnknownCommand`, `Disabled`, `BadParams{cmd,msg}`, `Other`,
  `Internal`. `From` impls: `DocError`, `SketchError`, `KernelError`, `IoError` → `Other`.
- **Incremental evaluator:** `Model` — `crates/doc/src/eval.rs:368`; `evaluate(doc)` (line 489)
  walks `doc.features` in order, fingerprints each (feature def + referenced params + prior-state
  digest), reuses cached `FeatureResult { id, name, error, warning, ms, skipped, state }` up to the
  first change. Headless re-evaluation works: `Session::new(doc)` calls `model.evaluate(&doc)`;
  `doc.open`/`file.export`/`inspect.measure` are ordinary commands.

## 9. Control protocol (JSON lines over loopback TCP)

- **Server:** `apps/solvecraft/src/control_server.rs` — `start(port, ctx)` binds `127.0.0.1:PORT`,
  one thread/connection; line JSON `{"id", "method", "params"}` → reply
  `{"id", "ok", "result"}` or `{"ok": false, "error"}`; 60 s reply timeout; requests drained on the
  UI thread between frames.
- **Dispatch:** `crates/ui-egui/src/control.rs` `handle(app, ctx, req)` (line 88) — a large
  `match` on `method`:
  - `engine.execute` / `command` → `app.run(id, params)`
  - `engine.script` → `Session::run_script` (stops at first error)
  - `engine.commands` → registry listing
  - `document.inspect` → params/timeline/bodies/selection (+ `measure`)
  - `ui.start`, `ui.click`, `ui.move`, `ui.drag`, `ui.key`, `ui.text`, `ui.scroll`, `ui.menu`,
    `ui.menuPick`, `ui.selection`, `ui.screenshot`, `ui.render`, `ui.view`, `ui.set`,
    `ui.documents`, `ui.home`, `ui.prefs`, `ui.help`, `ui.confirm`, `ui.window`, `ui.resize`,
    `ui.worldToScreen`, `ui.sketchToScreen`, `ui.triad`, `ui.handles`, `ui.at`, `ui.editFeature`,
    `ui.rename`, `ui.dialogInput`, `ui.capture`, `ui.shortcut` (~30 methods)
  - `app.quit`
- **Doc:** `docs/control-protocol.md`.

## 10. Headless CLI

`apps/solvecraft-cli/src/main.rs`:

| Subcommand | Purpose |
|-----------|---------|
| `run <script\|design\|step\|mesh> [--in] [--out …] [--save] [--quiet] [--results]` | build/open then export to one or more paths |
| `eval <script\|design\|step>` (alias `inspect`) | load + print measurements/doc info as JSON |
| `snapshot <…> --out shot.png [--width --height --view]` | CPU-render model to PNG |
| `exec <command> [json]` | run one command on a fresh design |
| `commands` | dump the full registry as JSON |
| `recipe <recipe.json>` | Fusion-oracle recipe → script JSON |
| `oracle <case-dir>…` | replay oracle recipes vs `measure.json` |
| `step-corpus <case-dir>…` | import + compare |
| `mcp [--in …] [--connect HOST:PORT]` | MCP server on stdio; headless or bridged |
| `save-stress`, `bench-edit` | hidden perf/recovery tools |

- `run` loads a design (`load`), applies a script (`{"command","params"}` array or `{"commands":[…]}`),
  then `file.export` per `--out` (format inferred from extension) and/or `file.save_as` for `--save`.
  **This is the ready-made hook for PDM background workers** (exports, thumbnails, measurements).
- Script step shape: `{"command":"<id>","params":{…}}` (`id` also accepted as `command` key).

## 11. Desktop UI (egui)

- **Main struct** `SolveApp` — `crates/ui-egui/src/lib.rs:145`. Fields: `session: Session`,
  `ui: UiState`, `cam: Camera`, `services: Services`, plus per-feature state.
- **Layout** `SolveApp::ui()` (line 699): app bar (`toolbar.rs:40`, doc tabs, version), save-change
  prompts, start page, toolbar (workspace tabs + panels, `toolbar.rs:236`), timeline, palette,
  left browser panel (`browser.rs:424`, `exact_size(250.0)`), central 3D viewport, floating
  dialogs.
- **Workspace tabs** `crates/ui-egui/src/toolbar.rs:10`:
  `["SOLID","SURFACE","MESH","SHEET METAL","PLASTIC","UTILITIES"]` (+`SKETCH` dynamic).
- **Driving commands:** `SolveApp::run(id, params)` (line 345) → `session.execute`
  (post-run camera fit on `file.new`/`doc.open`, recents on save); `SolveApp::start(id)` (line 382)
  is the toolbar-click path (tools/dialogs/sketch/capture, else `run`); `open_path` (line 547),
  `insert_path` (line 557).
- **File open/save (rfd, host-injected via `Services`)** — `apps/solvecraft/src/main.rs:54–72`:
  `pick_open` (filters solvecraft/step/stp/igs/iges/3mf/stl), `pick_save`. Consumed by
  `toolbar::file_menu` (`toolbar.rs:133`): Open…, Save/Save As…, Export STL/STEP/3MF/OBJ,
  Insert STEP/Mesh…. Also `home.rs:256`, `context_menu.rs`, `params_dialog.rs`,
  `documents.rs:617`.
- **Prefs** — eframe `Storage`, key `solvecraft.prefs` (`apps/solvecraft/src/main.rs:50`);
  `SolveApp::prefs()` (`lib.rs:270`) serializes dark/grid/perspective/recent/shortcuts/autosave +
  the `prefs::Prefs` struct (`crates/ui-egui/src/prefs.rs`); `load_prefs` (`lib.rs:290`).
  **No credential/keyring store** exists.

## 12. MCP

- **Library crate** `crates/mcp`, spoken over **stdio JSON-RPC 2.0 (newline-delimited)**, launched
  by `solvecraft-cli mcp` (`apps/solvecraft-cli/src/main.rs:213`).
- **`Server`** — `crates/mcp/src/server.rs:68`; handles `initialize`, `ping`, `tools/list`,
  `tools/call`, `resources/list/read`, `prompts/list`. `PROTOCOL_VERSION = "2025-06-18"`.
- **`Backend` trait** — `crates/mcp/src/backend.rs:15` (`call`, `has_ui`, `describe`); two impls:
  `Remote` (line 32, TCP to the running app's control port — speaks the control protocol),
  `Headless` (line 107, in-process `Session`; `ui.render` via CPU rasterizer, `ui.screenshot`
  errors).
- **Tool descriptors** — `crates/mcp/src/tools.rs`: `tool_definitions()` (line 52) built with
  `tool(name,title,desc,schema,read_only,destructive)` (line 33, emits
  `annotations.{readOnlyHint,destructiveHint,idempotentHint,openWorldHint}`); `dispatch()`
  (line 344). 15 tools: `list_commands`, `execute`, `batch`, `inspect_design`, `measure`,
  `body_topology`, `set_parameter`, `screenshot`, `export`, `undo`, `redo`, `new_design`, `open`,
  `save`.
- **Resources:** `solvecraft://design`, `solvecraft://commands`.
- **Doc:** `docs/mcp.md` (current).

**PDM implication:** the registration pattern for new PDM MCP tools is
"add a `tool(...)` entry in `tool_definitions()` + a `match` arm in `dispatch()`", or route PDM
control methods through `Backend::call`.

## 13. Web app (browser)

- **Pure WASM**, `apps/solvecraft-web`; compiles the same `solvecraft_ui_egui::SolveApp` via
  eframe `WebRunner` onto `<canvas id="solvecraft_canvas">` (WebGPU, WebGL2 fallback via `?webgl`).
  Build: `trunk build --release` → `dist/web` (dev server `127.0.0.1:8781`). **No WebSocket /
  control channel** (confirmed: no `web-sys` websocket feature).
- **Storage:** OPFS (Origin Private File System, `localStorage` fallback for insecure pages),
  `apps/solvecraft-web/src/files.rs`. Path conventions `OPFS="/opfs/"`, `DOWNLOADS="/downloads/"`,
  `UPLOAD="/upload/"`. The engine reads/writes an in-memory VFS
  (`solvecraft_engine::io::vfs`); `files::flush` (line 356) drains `vfs::take_writes()`.
  **Autosave** every 30 s → `recovery.solvecraft` + `recovery.json` (line 460).
- **UI:** `Shell` (`main.rs:~80`) wraps `SolveApp` + `Files`; a "Files in this browser" egui window
  with Open/Insert/Rename/Download/Delete; drag-drop; File System Access API.
- **No separate model viewer** — it reuses the egui viewport. The PDM web portal is planned as a
  *separate* React/TS app (toDo §4 D).

## 14. What's real vs. aspirational

| Capability | Status |
|-----------|--------|
| Parametric `.solvecraft` JSON, versioned + migratable | **Real** |
| Single-file assemblies (components/occurrences/joints) | **Real** |
| STEP/STL/3MF/OBJ/IGES import+export (assembly-preserving STEP) | **Real** |
| Headless re-evaluation + CLI (`run`/`eval`/`snapshot`/`mcp`) | **Real** |
| Everything-is-a-command registry (~300 cmds) | **Real** |
| Control channel + MCP (15 tools) | **Real** |
| Crash recovery (autosave folder) | **Real** |
| Web app (WASM + OPFS) | **Real** |
| Document-level stable UUID | **Absent** |
| Cross-document / assembly references | **Absent** |
| Where-used / uses tables | **Absent** |
| File versioning / revision model | **Absent** |
| Checkout / lock / auth / credential store | **Absent** |
| Object-storage abstraction (content-addressed) | **Absent** |
| BOM engine, lifecycle, change mgmt, search, ERP | **Absent** (buildable on top of existing foundation) |
