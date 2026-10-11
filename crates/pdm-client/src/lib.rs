//! PDM vault client: a thin, typed HTTP client for the SolveCraft PDM server plus a
//! local offline-workspace path manager.
//!
//! This is the CAD-side foundation for PDM integration (roadmap Phase 1 item 11). It is
//! not yet wired into the engine or UI; Phase 2 adds the `pdm.*` engine commands and the
//! UI panel on top of this client. All methods are async; callers drive them with any
//! Tokio/async runtime. Network-free helpers (URL building, SHA-256, workspace paths,
//! part-number formatting) are pure and unit-tested.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub type Result<T> = std::result::Result<T, PdmClientError>;

// ── Errors ───────────────────────────────────────────────────────────────

/// Errors from talking to the PDM vault.
#[derive(Debug, thiserror::Error)]
pub enum PdmClientError {
    #[error("vault http error: {0}")]
    Http(#[source] reqwest::Error),
    /// The server returned a non-2xx status with a PDM error body.
    #[error("vault error {code}: {message}")]
    Api { code: u16, message: String },
    #[error("invalid vault response: {0}")]
    Decode(String),
    #[error("invalid vault config: {0}")]
    Config(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

// ── DTOs ─────────────────────────────────────────────────────────────────
//
// These mirror a subset of the server's JSON (field names match the server's serde
// output). Optional server fields are `Option` with `#[serde(default)]` so older/newer
// servers stay compatible. `id` fields are UUIDs serialised as strings.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub part_number: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub classification: String,
    #[serde(default)]
    pub lifecycle_state: String,
    #[serde(default)]
    pub doc_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub file_id: String,
    pub seq: i32,
    #[serde(default)]
    pub commit_message: String,
    #[serde(default)]
    pub author_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lock {
    pub id: String,
    pub item_id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub workstation: String,
    #[serde(default)]
    pub lease_expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckinReq {
    pub sha256: String,
    #[serde(default)]
    pub commit_message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_version: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub object_type: String,
    #[serde(default)]
    pub at: Option<String>,
}

// ── Config ───────────────────────────────────────────────────────────────

/// Connection + workspace configuration for a vault client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    /// Base URL of the PDM server, e.g. `http://pdm.example:8080` (no trailing slash).
    pub base_url: String,
    /// Bearer API token.
    pub token: String,
    /// Root directory for the local offline workspace (where managed files are cached).
    pub workspace_dir: PathBuf,
}

impl VaultConfig {
    /// Validate the config for use.
    pub fn validate(&self) -> Result<()> {
        if self.base_url.trim().is_empty() {
            return Err(PdmClientError::Config("base_url is empty".into()));
        }
        if self.token.trim().is_empty() {
            return Err(PdmClientError::Config("token is empty".into()));
        }
        Ok(())
    }
}

// ── Client ───────────────────────────────────────────────────────────────

pub struct Vault {
    http: reqwest::Client,
    cfg: VaultConfig,
}

impl Vault {
    pub fn new(cfg: VaultConfig) -> Result<Self> {
        cfg.validate()?;
        let http = reqwest::Client::builder().build().map_err(PdmClientError::Http)?;
        Ok(Self { http, cfg })
    }

    pub fn config(&self) -> &VaultConfig {
        &self.cfg
    }

    // ── pure helpers (network-free, unit-tested) ─────────────────────────

    /// Absolute URL for a vault path, without trailing-slash surprises.
    pub fn url(&self, path: &str) -> String {
        let base = self.cfg.base_url.trim_end_matches('/');
        let p = if path.starts_with('/') { path } else { &format!("/{path}") };
        format!("{base}{p}")
    }

    /// Where a managed item's version N lives in the local workspace.
    pub fn item_version_path(&self, item_id: &str, seq: i32) -> PathBuf {
        self.cfg.workspace_dir.join("items").join(item_id).join(format!("v{seq}.solvecraft"))
    }

    /// Path of the most recently cached version for an item.
    pub fn item_latest_path(&self, item_id: &str) -> PathBuf {
        self.cfg.workspace_dir.join("items").join(item_id).join("latest.solvecraft")
    }

    /// SHA-256 of a byte slice as lowercase hex.
    pub fn sha256_hex(data: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(data);
        hex::encode(h.finalize())
    }

    /// Format a part number from a classification prefix and 1-based sequence.
    pub fn part_number(classification: &str, seq: u64) -> String {
        format!("{classification}-{seq:06}")
    }

    // ── request plumbing ─────────────────────────────────────────────────

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let b = match method {
            reqwest::Method::GET => self.http.get(self.url(path)),
            reqwest::Method::POST => self.http.post(self.url(path)),
            reqwest::Method::PUT => self.http.put(self.url(path)),
            reqwest::Method::DELETE => self.http.delete(self.url(path)),
            _ => self.http.request(method, self.url(path)),
        };
        b.header("Authorization", format!("Bearer {}", self.cfg.token))
    }

    /// Run a request and, on success, decode the JSON body into `T`.
    async fn get_json<T: for<'de> Deserialize<'de>>(&self, method: reqwest::Method, path: &str) -> Result<T> {
        let resp = self.request(method, path).send().await.map_err(PdmClientError::Http)?;
        decode_json(resp).await
    }

    // ── API surface ──────────────────────────────────────────────────────

    /// `GET /me` — the authenticated identity.
    pub async fn me(&self) -> Result<serde_json::Value> {
        self.get_json(reqwest::Method::GET, "/me").await
    }

    /// `GET /projects`
    pub async fn projects(&self) -> Result<Vec<Project>> {
        let v: serde_json::Value = self.get_json(reqwest::Method::GET, "/projects").await?;
        v.get("projects")
            .and_then(|p| p.as_array())
            .cloned()
            .ok_or_else(|| PdmClientError::Decode("missing 'projects' array".into()))
            .and_then(|a| serde_json::from_value(serde_json::json!(a)).map_err(|e| PdmClientError::Decode(e.to_string())))
    }

    /// `GET /projects/{id}/items`
    pub async fn items(&self, project_id: &str) -> Result<Vec<Item>> {
        let v: serde_json::Value = self.get_json(reqwest::Method::GET, &format!("/projects/{project_id}/items")).await?;
        let arr = v.get("items").and_then(|p| p.as_array()).cloned().ok_or_else(|| PdmClientError::Decode("missing 'items' array".into()))?;
        serde_json::from_value(serde_json::json!(arr)).map_err(|e| PdmClientError::Decode(e.to_string()))
    }

    /// `POST /projects/{id}/items`
    pub async fn create_item(&self, project_id: &str, description: &str, classification: &str) -> Result<Item> {
        let body = serde_json::json!({ "description": description, "classification": classification });
        let resp =
            self.request(reqwest::Method::POST, &format!("/projects/{project_id}/items")).json(&body).send().await.map_err(PdmClientError::Http)?;
        let v: serde_json::Value = decode_json(resp).await?;
        let item = v.get("item").cloned().ok_or_else(|| PdmClientError::Decode("missing 'item'".into()))?;
        serde_json::from_value(item).map_err(|e| PdmClientError::Decode(e.to_string()))
    }

    /// `POST /items/{id}/checkout`
    pub async fn checkout(&self, item_id: &str, workstation: &str) -> Result<serde_json::Value> {
        let body = serde_json::json!({ "workstation": workstation });
        let resp = self.request(reqwest::Method::POST, &format!("/items/{item_id}/checkout")).json(&body).send().await;
        let resp = resp.map_err(PdmClientError::Http)?;
        decode_json(resp).await
    }

    /// `POST /items/{id}/checkin`
    pub async fn checkin(&self, item_id: &str, req: &CheckinReq) -> Result<Version> {
        let resp = self.request(reqwest::Method::POST, &format!("/items/{item_id}/checkin")).json(req).send().await;
        let resp = resp.map_err(PdmClientError::Http)?;
        let v: serde_json::Value = decode_json(resp).await?;
        let version = v.get("version").cloned().ok_or_else(|| PdmClientError::Decode("missing 'version'".into()))?;
        serde_json::from_value(version).map_err(|e| PdmClientError::Decode(e.to_string()))
    }

    /// `DELETE /items/{id}/checkout`
    pub async fn undo_checkout(&self, item_id: &str) -> Result<()> {
        let resp = self.request(reqwest::Method::DELETE, &format!("/items/{item_id}/checkout")).send().await;
        let resp = resp.map_err(PdmClientError::Http)?;
        if !resp.status().is_success() {
            return Err(map_status(resp).await);
        }
        Ok(())
    }

    /// `GET /items/{id}/versions`
    pub async fn versions(&self, item_id: &str) -> Result<Vec<Version>> {
        let v: serde_json::Value = self.get_json(reqwest::Method::GET, &format!("/items/{item_id}/versions")).await?;
        let arr = v.get("versions").and_then(|p| p.as_array()).cloned().ok_or_else(|| PdmClientError::Decode("missing 'versions' array".into()))?;
        serde_json::from_value(serde_json::json!(arr)).map_err(|e| PdmClientError::Decode(e.to_string()))
    }

    /// `GET /search`
    pub async fn search(&self, q: &str, project: Option<&str>) -> Result<Vec<Item>> {
        let mut path = format!("/search?q={}", urlenc(q));
        if let Some(p) = project {
            path.push_str(&format!("&project={p}"));
        }
        let v: serde_json::Value = self.get_json(reqwest::Method::GET, &path).await?;
        let arr = v.get("results").and_then(|p| p.as_array()).cloned().ok_or_else(|| PdmClientError::Decode("missing 'results' array".into()))?;
        serde_json::from_value(serde_json::json!(arr)).map_err(|e| PdmClientError::Decode(e.to_string()))
    }

    /// `GET /audit`
    pub async fn audit(&self, item_id: Option<&str>) -> Result<Vec<AuditEvent>> {
        let path = match item_id {
            Some(i) => format!("/audit?item={i}"),
            None => "/audit".to_string(),
        };
        let v: serde_json::Value = self.get_json(reqwest::Method::GET, &path).await?;
        let arr = v.get("events").and_then(|p| p.as_array()).cloned().ok_or_else(|| PdmClientError::Decode("missing 'events' array".into()))?;
        serde_json::from_value(serde_json::json!(arr)).map_err(|e| PdmClientError::Decode(e.to_string()))
    }

    /// `GET /versions/{id}/blob` — download a version to `dest`, returning its SHA-256.
    pub async fn download_version(&self, version_id: &str, dest: &Path) -> Result<String> {
        let resp = self.request(reqwest::Method::GET, &format!("/versions/{version_id}/blob")).send().await.map_err(PdmClientError::Http)?;
        if !resp.status().is_success() {
            return Err(map_status(resp).await);
        }
        let bytes = resp.bytes().await.map_err(PdmClientError::Http)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(PdmClientError::Io)?;
        }
        std::fs::write(dest, &bytes).map_err(PdmClientError::Io)?;
        Ok(Self::sha256_hex(&bytes))
    }

    /// `PUT /blobs` — upload raw bytes; returns the server-computed SHA-256.
    pub async fn upload_blob(&self, data: &[u8]) -> Result<String> {
        let resp = self.request(reqwest::Method::PUT, "/blobs").body(data.to_vec()).send().await;
        let resp = resp.map_err(PdmClientError::Http)?;
        if !resp.status().is_success() {
            return Err(map_status(resp).await);
        }
        let v: serde_json::Value = decode_json(resp).await?;
        v.get("blob")
            .and_then(|b| b.get("sha256"))
            .and_then(|s| s.as_str())
            .map(str::to_string)
            .ok_or_else(|| PdmClientError::Decode("missing blob.sha256".into()))
    }
}

// ── helpers ──────────────────────────────────────────────────────────────

async fn decode_json<T: for<'de> Deserialize<'de>>(resp: reqwest::Response) -> Result<T> {
    if !resp.status().is_success() {
        return Err(map_status(resp).await);
    }
    let t: T = resp.json().await.map_err(|e| PdmClientError::Decode(e.to_string()))?;
    Ok(t)
}

/// Map a non-2xx response to a `PdmClientError::Api`, parsing the PDM error body when present.
async fn map_status(resp: reqwest::Response) -> PdmClientError {
    let code = resp.status().as_u16();
    let message = resp.text().await.unwrap_or_default();
    let parsed = serde_json::from_str::<serde_json::Value>(&message);
    if let Ok(parsed) = parsed
        && let Some(msg) = parsed.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str())
    {
        return PdmClientError::Api { code, message: msg.to_string() };
    }
    PdmClientError::Api { code, message }
}

/// Minimal URL-encode for query strings (reserves only what is necessary).
fn urlenc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> VaultConfig {
        VaultConfig { base_url: "http://vault.example:8080".into(), token: "tok".into(), workspace_dir: PathBuf::from("/tmp/pdm-ws") }
    }

    #[test]
    fn config_validation() {
        assert!(cfg().validate().is_ok());
        let mut bad = cfg();
        bad.base_url = "  ".into();
        assert!(matches!(bad.validate(), Err(PdmClientError::Config(_))));
        let mut bad2 = cfg();
        bad2.token = "".into();
        assert!(matches!(bad2.validate(), Err(PdmClientError::Config(_))));
    }

    #[test]
    fn url_building_strips_trailing_slash() {
        let mut c = cfg();
        c.base_url = "http://x:8080/".into();
        let v = Vault::new(c).unwrap();
        assert_eq!(v.url("/items/1"), "http://x:8080/items/1");
        assert_eq!(v.url("items/1"), "http://x:8080/items/1");
    }

    #[test]
    fn sha256_is_stable() {
        assert_eq!(Vault::sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        let a = Vault::sha256_hex(b"hello");
        let b = Vault::sha256_hex(b"hello");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn part_number_formatting() {
        assert_eq!(Vault::part_number("BE", 1), "BE-000001");
        assert_eq!(Vault::part_number("MFG", 1000000), "MFG-1000000");
    }

    #[test]
    fn workspace_paths_are_nested() {
        let v = Vault::new(cfg()).unwrap();
        assert_eq!(v.item_version_path("item-1", 3), Path::new("/tmp/pdm-ws/items/item-1/v3.solvecraft"));
        assert!(v.item_latest_path("item-1").ends_with("latest.solvecraft"));
    }

    #[test]
    fn urlenc_encodes_spaces_and_specials() {
        assert_eq!(urlenc("BE-000001"), "BE-000001");
        assert_eq!(urlenc("a b/c"), "a%20b%2Fc");
    }

    #[test]
    fn checkin_req_serialises_optionals() {
        let r = CheckinReq { sha256: "abc".into(), commit_message: "msg".into(), base_version: None, filename: None, mime_type: None };
        let j = serde_json::to_value(&r).unwrap();
        assert!(j.get("filename").is_none(), "None filename must be omitted");
        assert_eq!(j["commit_message"], "msg");
    }
}
