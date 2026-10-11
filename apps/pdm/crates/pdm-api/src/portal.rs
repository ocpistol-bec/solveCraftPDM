//! Minimal Phase-1 web file browser served at `/`.
//!
//! A single self-contained page: connect with an API token, browse projects and
//! items, upload/check-in versions, check out/in, and download. This is the
//! lightweight stand-in for the full React portal (Phase 1 scope: "web file
//! browser"). No external assets, no build step.

use axum::response::{Html, IntoResponse};

pub async fn default_index() -> impl IntoResponse {
    Html(PAGE)
}

const PAGE: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>SolveCraft PDM</title>
<style>
  :root { --bg:#10151c; --panel:#1a2230; --line:#2a3648; --fg:#d7e0ec; --dim:#8494a8;
          --acc:#5aa9ff; --ok:#4caf7d; --err:#e05d5d; --mono:ui-monospace,Menlo,Consolas,monospace; }
  * { box-sizing:border-box; }
  body { margin:0; background:var(--bg); color:var(--fg); font:14px/1.5 system-ui,sans-serif; }
  header { background:var(--panel); border-bottom:1px solid var(--line); padding:10px 16px;
           display:flex; align-items:center; gap:12px; position:sticky; top:0; z-index:5; }
  header h1 { font-size:15px; margin:0; font-weight:600; }
  header .dot { width:9px; height:9px; border-radius:50%; background:var(--err); }
  header .dot.on { background:var(--ok); }
  main { padding:16px; max-width:1100px; margin:0 auto; }
  .bar { display:flex; flex-wrap:wrap; gap:8px; margin-bottom:14px; }
  input, select, button { background:var(--panel); color:var(--fg); border:1px solid var(--line);
           border-radius:6px; padding:7px 10px; font:inherit; }
  button { cursor:pointer; background:#223049; }
  button:hover { background:#2a3b5c; }
  button:disabled { opacity:.45; cursor:default; }
  label.dim { color:var(--dim); font-size:12px; }
  table { width:100%; border-collapse:collapse; background:var(--panel); border:1px solid var(--line);
          border-radius:8px; overflow:hidden; }
  th, td { text-align:left; padding:8px 10px; border-bottom:1px solid var(--line); font-size:13px; }
  th { background:#151d29; color:var(--dim); font-weight:600; text-transform:uppercase; font-size:11px; }
  tr:last-child td { border-bottom:none; }
  .muted { color:var(--dim); } .mono { font-family:var(--mono); font-size:12px; }
  .lock { color:#e0a53f; font-size:12px; }
  .err { color:var(--err); white-space:pre-wrap; background:#2a1d22; border:1px solid var(--err);
         padding:8px 10px; border-radius:6px; margin:10px 0; font-family:var(--mono); font-size:12px; }
  .ok { color:var(--ok); }
  .row { display:flex; gap:10px; align-items:center; flex-wrap:wrap; margin:6px 0; }
  h2 { font-size:13px; color:var(--dim); text-transform:uppercase; letter-spacing:.05em;
       margin:20px 0 8px; font-weight:600; }
  .ver td { vertical-align:top; }
  .hidden { display:none; }
</style>
</head>
<body>
<header>
  <span class="dot" id="dot"></span>
  <h1>SolveCraft PDM</h1>
  <span class="muted" id="who" style="font-size:12px;"></span>
  <div style="flex:1"></div>
  <button id="relog" class="hidden">Disconnect</button>
</header>
<main>
  <div id="conn">
    <h2>Connect</h2>
    <div class="row">
      <input id="token" placeholder="API token (pdm_...)" style="min-width:340px;" autocomplete="off">
      <button id="connect">Connect</button>
    </div>
    <p class="muted" style="font-size:12px">Create a token with <span class="mono">pdm-cli token create</span>,
      then paste it here.</p>
  </div>

  <div id="app" class="hidden">
    <div class="bar">
      <select id="project"></select>
      <button id="newproj">+ Project</button>
      <button id="newitem">+ Item</button>
      <input id="search" placeholder="search part no / description">
      <button id="searchgo">Search</button>
    </div>

    <h2>Items</h2>
    <table id="items"><thead><tr>
      <th>Part no</th><th>Description</th><th>State</th><th>Head ver</th><th>Lock</th><th></th>
    </tr></thead><tbody></tbody></table>

    <div id="detail" class="hidden">
      <h2 id="detailtitle">Item</h2>
      <div class="row">
        <label class="dim">file</label><input id="dfile" style="width:220px">
        <label class="dim">message</label><input id="dmsg" placeholder="commit message" style="width:260px">
        <button id="co">Check out</button>
        <button id="uc">Undo checkout</button>
        <button id="ci">Check in (upload)</button>
        <button id="rel">Release revision</button>
      </div>
      <h2>Versions</h2>
      <table id="vers" class="ver"><thead><tr>
        <th>Seq</th><th>SHA-256</th><th>Author</th><th>When</th><th>Message</th><th></th>
      </tr></thead><tbody></tbody></table>
    </div>

    <h2>Audit</h2>
    <table id="audit"><thead><tr>
      <th>When</th><th>Actor</th><th>Action</th><th>Object</th><th>Detail</th>
    </tr></thead><tbody></tbody></table>
  </div>

  <div id="errbox" class="err hidden"></div>
</main>

<script>
"use strict";
let TOKEN = localStorage.getItem("pdm_token") || "";
let ME = null, CUR_ITEM = null, LOCK = null;

const $ = id => document.getElementById(id);
function esc(s){ return String(s ?? "").replace(/[&<>"]/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;","\"":"&quot;"}[c])); }
function err(m){ const b=$("errbox"); b.textContent = m || "error"; b.classList.remove("hidden");
  setTimeout(()=>b.classList.add("hidden"), 6000); }
function ok(m){ const b=$("errbox"); b.textContent = m; b.classList.remove("hidden");
  b.style.borderColor = "var(--ok)"; b.style.color = "var(--ok)";
  setTimeout(()=>{ b.classList.add("hidden"); b.style.borderColor=""; b.style.color=""; }, 4000); }

async function api(method, path, body, rawBody){
  const headers = {};
  if (TOKEN) headers["Authorization"] = "Bearer " + TOKEN;
  let payload;
  if (rawBody !== undefined){ payload = rawBody; }
  else if (body !== undefined){ headers["Content-Type"] = "application/json"; payload = JSON.stringify(body); }
  const r = await fetch(path, { method, headers, body: payload });
  const ctype = r.headers.get("content-type") || "";
  let data;
  if (ctype.includes("application/json")) data = await r.json();
  else if (ctype === "text/plain") data = (await r.text());
  else data = await r.arrayBuffer();
  if (!r.ok){
    let msg;
    try { msg = (typeof data === "object" && data !== null && data.error) ?
      (data.error.message || data.error) : (typeof data === "string" ? data : "HTTP " + r.status); }
    catch(e){ msg = "HTTP " + r.status; }
    throw new Error(msg);
  }
  return data;
}

async function connect(){
  TOKEN = $("token").value.trim();
  if (!TOKEN){ err("Enter an API token"); return; }
  try {
    ME = await api("GET", "/me");
    localStorage.setItem("pdm_token", TOKEN);
    showApp();
  } catch(e){ err("Connect failed: " + e.message); }
}

function showApp(){
  $("dot").classList.add("on");
  $("who").textContent = ME.user ? (ME.user.display_name || ME.user.email) : "service account";
  $("conn").classList.add("hidden");
  $("app").classList.remove("hidden");
  $("relog").classList.remove("hidden");
  loadProjects(); loadAudit();
}

async function loadProjects(){
  try {
    const d = await api("GET", "/projects");
    const sel = $("project"); sel.innerHTML = "";
    for (const p of d.projects){
      const o = document.createElement("option"); o.value = p.id;
      o.textContent = p.name + (p.description ? " — " + p.description : "");
      sel.appendChild(o);
    }
    if (d.projects.length) loadItems(d.projects[0].id);
  } catch(e){ err("projects: " + e.message); }
}

async function loadItems(projectId){
  try {
    const d = await api("GET", "/projects/" + projectId + "/items");
    const tb = $("items").querySelector("tbody"); tb.innerHTML = "";
    for (const it of d.items){
      const tr = document.createElement("tr");
      tr.innerHTML =
        "<td class='mono'>" + esc(it.part_number) + "</td>" +
        "<td>" + esc(it.description || "") + "</td>" +
        "<td class='muted'>" + esc(it.lifecycle_state) + "</td>" +
        "<td>" + (it.head_version ? it.head_version.seq : "—") + "</td>" +
        "<td>" + (it.lock ? "<span class='lock'>🔒 " + esc(it.lock_owner || "") + "</span>" : "") + "</td>" +
        "<td><button data-id='" + it.id + "'>Open</button></td>";
      tr.querySelector("button").onclick = () => openItem(it.id, projectId);
      tb.appendChild(tr);
    }
  } catch(e){ err("items: " + e.message); }
}

async function openItem(id, projectId){
  CUR_ITEM = id;
  try {
    const d = await api("GET", "/items/" + id);
    $("detail").classList.remove("hidden");
    $("detailtitle").textContent = d.item.part_number + "  ·  " + (d.item.description || "");
    $("dfile").value = d.item.filename || "";
    LOCK = d.lock;
    loadVersions(d);
  } catch(e){ err("open item: " + e.message); }
}

async function loadVersions(d){
  const tb = $("vers").querySelector("tbody"); tb.innerHTML = "";
  for (const v of (d.versions || [])){
    const tr = document.createElement("tr");
    tr.innerHTML =
      "<td>" + v.seq + "</td>" +
      "<td class='mono muted'>" + esc(v.sha256 || "").slice(0,16) + "…</td>" +
      "<td class='muted'>" + esc(v.author_name || "—") + "</td>" +
      "<td class='muted'>" + esc(v.committed_at ? v.committed_at.slice(0,19).replace("T"," ") : "") + "</td>" +
      "<td>" + esc(v.commit_message || "") + "</td>" +
      "<td><button data-v='" + v.id + "'>Download</button></td>";
    tr.querySelector("button").onclick = () => downloadVersion(v.id);
    tb.appendChild(tr);
  }
}

async function downloadVersion(vid){
  try {
    const r = await fetch("/versions/" + vid + "/blob", { headers: { "Authorization": "Bearer " + TOKEN } });
    if (!r.ok) throw new Error("HTTP " + r.status);
    const buf = await r.arrayBuffer();
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([buf]));
    a.download = (CUR_ITEM ? $("dfile").value || "download" : "download");
    a.click(); URL.revokeObjectURL(a.href);
  } catch(e){ err("download: " + e.message); }
}

async function loadAudit(){
  try {
    const d = await api("GET", "/audit?limit=25");
    const tb = $("audit").querySelector("tbody"); tb.innerHTML = "";
    for (const ev of (d.events || [])){
      const tr = document.createElement("tr");
      tr.innerHTML =
        "<td class='muted'>" + esc(ev.at ? ev.at.slice(0,19).replace("T"," ") : "") + "</td>" +
        "<td class='muted'>" + esc(ev.actor_name || "system") + "</td>" +
        "<td class='mono'>" + esc(ev.action) + "</td>" +
        "<td class='muted'>" + esc(ev.object_type || "") + "</td>" +
        "<td class='muted'>" + esc(ev.detail ? JSON.stringify(ev.detail) : "") + "</td>";
      tb.appendChild(tr);
    }
  } catch(e){ /* audit is best-effort */ }
}

async function checkout(){
  if (!CUR_ITEM) return;
  try {
    const d = await api("POST", "/items/" + CUR_ITEM + "/checkout", { workstation: location.hostname });
    LOCK = d.lock; ok("Checked out (base v" + (d.base_version ? d.base_version.seq : "—") + ")");
    loadAudit();
    const fresh = await api("GET", "/items/" + CUR_ITEM); openItem(CUR_ITEM, null);
  } catch(e){ err("checkout: " + e.message); }
}

async function undoCheckout(){
  if (!CUR_ITEM) return;
  try { await api("DELETE", "/items/" + CUR_ITEM + "/checkout");
    LOCK = null; ok("Checkout undone"); loadAudit();
    const fresh = await api("GET", "/items/" + CUR_ITEM); openItem(CUR_ITEM, null);
  } catch(e){ err("undo: " + e.message); }
}

async function checkin(){
  if (!CUR_ITEM) return;
  const f = $("dfile").files && $("dfile").files[0];
  if (!f){ err("Pick a file to check in"); return; }
  try {
    const buf = await f.arrayBuffer();
    const digest = await crypto.subtle.digest("SHA-256", buf);
    const sha = Array.from(new Uint8Array(digest)).map(b => b.toString(16).padStart(2,"0")).join("");
    const up = await api("PUT", "/blobs", null, new Uint8Array(buf));
    const base = (await api("GET", "/items/" + CUR_ITEM)).head_version;
    const d = await api("POST", "/items/" + CUR_ITEM + "/checkin", {
      sha256: up.blob.sha256, size: up.blob.size_bytes,
      base_version: base ? base.seq : null,
      commit_message: $("dmsg").value || "", filename: f.name,
    });
    ok("Checked in v" + d.version.seq + "  (" + sha.slice(0,12) + "…)");
    LOCK = null; $("dmsg").value = "";
    loadAudit();
    const fresh = await api("GET", "/items/" + CUR_ITEM); openItem(CUR_ITEM, null);
  } catch(e){ err("checkin: " + e.message); }
}

async function release(){
  if (!CUR_ITEM) return;
  try {
    const d = await api("POST", "/items/" + CUR_ITEM + "/release", {});
    ok("Released as revision " + d.revision.label); loadAudit();
  } catch(e){ err("release: " + e.message); }
}

$("connect").onclick = connect;
$("relog").onclick = () => { TOKEN = ""; localStorage.removeItem("pdm_token");
  location.reload(); };
$("newproj").onclick = async () => {
  const name = prompt("Project name"); if (!name) return;
  const desc = prompt("Description (optional)") || "";
  try { await api("POST", "/projects", { name, description: desc }); ok("Project created");
    loadProjects(); } catch(e){ err("new project: " + e.message); }
};
$("newitem").onclick = async () => {
  const pid = $("project").value; if (!pid){ err("Pick a project first"); return; }
  const desc = prompt("Item description"); if (desc === null) return;
  const pn = prompt("Part number (leave blank to auto-assign)", "");
  const cls = prompt("Classification (BE / ASM / MFG)", "BE") || "BE";
  try {
    const d = await api("POST", "/projects/" + pid + "/items",
      { part_number: pn || null, description: desc || null, classification: cls });
    ok("Item created: " + d.item.part_number);
    loadItems(pid); openItem(d.item.id, pid);
  } catch(e){ err("new item: " + e.message); }
};
$("searchgo").onclick = async () => {
  const q = $("search").value.trim(); if (!q) return;
  try {
    const d = await api("GET", "/search?q=" + encodeURIComponent(q));
    const tb = $("items").querySelector("tbody"); tb.innerHTML = "";
    for (const it of d.results){
      const tr = document.createElement("tr");
      tr.innerHTML =
        "<td class='mono'>" + esc(it.part_number) + "</td>" +
        "<td>" + esc(it.description || "") + "</td>" +
        "<td class='muted'>" + esc(it.lifecycle_state) + "</td>" +
        "<td class='muted'>" + esc(it.classification) + "</td>" +
        "<td><button data-id='" + it.id + "'>Open</button></td>";
      tr.querySelector("button").onclick = () => openItem(it.id, null);
      tb.appendChild(tr);
    }
  } catch(e){ err("search: " + e.message); }
};
// The "file" input doubles as the check-in file picker.
$("dfile").type = "file";
$("co").onclick = checkout;
$("uc").onclick = undoCheckout;
$("ci").onclick = checkin;
$("rel").onclick = release;

if (TOKEN) connect();
</script>
</body>
</html>
"##;
