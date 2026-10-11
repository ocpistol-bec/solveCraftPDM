//! End-to-end vault test for Phase 1.
//!
//! Requires:
//!   PDM_E2E=1
//!   PDM_E2E_BASE   (default http://127.0.0.1:8080)
//!   PDM_E2E_ADMIN_TOKEN
//!
//! Verifies the full acceptance flow from toDo §28:
//!   1. Two authenticated users
//!   2. One shared engineering project
//!   3. Upload of a `.solvecraft` design
//!   4. Creation of immutable file versions
//!   5. Exclusive checkout
//!   6. Successful check-in
//!   7. Retrieval of a previous version
//!   8. Audit history
//!   9. Verified file integrity after download

use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;

fn base() -> String {
    std::env::var("PDM_E2E_BASE").unwrap_or_else(|_| "http://127.0.0.1:8080".into())
}

fn admin_token() -> String {
    std::env::var("PDM_E2E_ADMIN_TOKEN").expect("PDM_E2E_ADMIN_TOKEN must be set")
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

async fn create_user_and_token(client: &Client, email: &str, name: &str, role: &str) -> (String, String) {
    let r = client
        .post(format!("{}/users", base()))
        .bearer_auth(admin_token())
        .json(&json!({
            "email": email,
            "display_name": name,
            "role": role,
        }))
        .send()
        .await
        .expect("POST /users");
    let status = r.status();
    assert!(status.is_success(), "create user {email}: {status}");
    let body: serde_json::Value = r.json().await.expect("user JSON");
    let user_id = body["user"]["id"].as_str().unwrap().to_string();

    let r2 = client
        .post(format!("{}/users/{user_id}/tokens", base()))
        .bearer_auth(admin_token())
        .json(&json!({ "name": "e2e" }))
        .send()
        .await
        .expect("POST /users/{id}/tokens");
    let status2 = r2.status();
    assert!(status2.is_success(), "create token: {status2}");
    let tok_body: serde_json::Value = r2.json().await.expect("token JSON");
    let token = tok_body["token"].as_str().unwrap().to_string();
    (user_id, token)
}

#[tokio::test]
#[ignore]
async fn e2e_vault_full_flow() {
    if std::env::var("PDM_E2E").ok().as_deref() != Some("1") {
        eprintln!("PDM_E2E not set; skipping");
        return;
    }

    let client = Client::builder().timeout(Duration::from_secs(30)).build().expect("client");

    // 0. Health check
    let r = client.get(format!("{}/healthz", base())).send().await.unwrap();
    assert_eq!(r.status(), 200, "healthz failed");

    // 1. Two authenticated users
    let (_uid_a, token_a) = create_user_and_token(&client, "alice@test.dev", "Alice", "editor").await;
    let (_uid_b, token_b) = create_user_and_token(&client, "bob@test.dev", "Bob", "editor").await;

    // Verify /me for both
    for (label, tok) in [("A", &token_a), ("B", &token_b)] {
        let r = client.get(format!("{}/me", base())).bearer_auth(tok).send().await.unwrap();
        assert!(r.status().is_success(), "GET /me user {label} failed");
    }

    // 2. One shared project
    let proj_name = format!("e2e-proj-{}", std::process::id());
    let r = client
        .post(format!("{}/projects", base()))
        .bearer_auth(admin_token())
        .json(&json!({ "name": proj_name, "description": "E2E test" }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "POST /projects: {}", r.status());
    let proj: serde_json::Value = r.json().await.unwrap();
    let project_id = proj["project"]["id"].as_str().unwrap().to_string();

    // 3. Upload a .solvecraft design
    let design_v1 = br#"{"format":"solvecraft/5","name":"e2e","units":"mm","params":[],"features":[],"next_id":1,"components":[],"occurrences":[],"assembly":{},"parts":{},"configs":{},"favorites":[],"materials":{},"appearances":{},"body_components":{},"body_offsets":{},"browser_groups":[],"browser_order":{},"named_views":[]}"#;
    let sha_v1 = sha256_hex(design_v1);

    // Create an item
    let r = client
        .post(format!("{}/projects/{project_id}/items", base()))
        .bearer_auth(&token_a)
        .json(&json!({ "description": "E2E test part", "classification": "BE" }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "create item: {}", r.status());
    let item_resp: serde_json::Value = r.json().await.unwrap();
    let item_id = item_resp["item"]["id"].as_str().unwrap().to_string();
    let part_number = item_resp["item"]["part_number"].as_str().unwrap().to_string();
    eprintln!("created item {part_number} ({item_id})");
    assert!(part_number.starts_with("BE-") && part_number != "BE-000000", "auto part number should be BE-000001 or later, got {part_number}");

    // Upload blob
    let r = client
        .put(format!("{}/blobs", base()))
        .bearer_auth(&token_a)
        .header("Content-Type", "application/octet-stream")
        .body(design_v1.to_vec())
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "PUT /blobs: {}", r.status());
    let blob_resp: serde_json::Value = r.json().await.unwrap();
    let uploaded_sha = blob_resp["blob"]["sha256"].as_str().unwrap().to_string();
    assert_eq!(uploaded_sha, sha_v1, "sha mismatch on upload");

    // 5. Exclusive checkout
    let r = client
        .post(format!("{}/items/{item_id}/checkout", base()))
        .bearer_auth(&token_a)
        .json(&json!({ "workstation": "e2e-a" }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "checkout A: {}", r.status());
    let co_a: serde_json::Value = r.json().await.unwrap();
    assert!(co_a["lock"].is_object(), "no lock in checkout response");

    // User B tries to check out → 409
    let r = client
        .post(format!("{}/items/{item_id}/checkout", base()))
        .bearer_auth(&token_b)
        .json(&json!({ "workstation": "e2e-b" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409, "checkout B should conflict, got {}", r.status());

    // 6. Successful check-in (version 1)
    let r = client
        .post(format!("{}/items/{item_id}/checkin", base()))
        .bearer_auth(&token_a)
        .json(&json!({
            "sha256": sha_v1,
            "commit_message": "Initial import",
            "base_version": null,
            "filename": "e2e_part.solvecraft",
        }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "checkin v1: {}", r.status());
    let v1: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v1["version"]["seq"], 1, "first version should be seq 1");
    let v1_id = v1["version"]["id"].as_str().unwrap().to_string();

    // 4. Immutable file versions — upload same blob again, should dedup
    let r = client
        .put(format!("{}/blobs", base()))
        .bearer_auth(&token_a)
        .header("Content-Type", "application/octet-stream")
        .body(design_v1.to_vec())
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "PUT /blobs (dedup): {}", r.status());
    let blob2: serde_json::Value = r.json().await.unwrap();
    assert_eq!(blob2["blob"]["sha256"].as_str().unwrap(), sha_v1, "dedup should return same sha");

    // Create version 2
    let design_v2 = br#"{"format":"solvecraft/5","name":"e2e_v2","units":"mm","params":[],"features":[],"next_id":1,"components":[],"occurrences":[],"assembly":{},"parts":{},"configs":{},"favorites":[],"materials":{},"appearances":{},"body_components":{},"body_offsets":{},"browser_groups":[],"browser_order":{},"named_views":[]}"#;
    let sha_v2 = sha256_hex(design_v2);
    let r = client.put(format!("{}/blobs", base())).bearer_auth(&token_a).body(design_v2.to_vec()).send().await.unwrap();
    assert!(r.status().is_success(), "upload v2 blob: {}", r.status());

    let r = client
        .post(format!("{}/items/{item_id}/checkout", base()))
        .bearer_auth(&token_a)
        .json(&json!({ "workstation": "e2e-a" }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "re-checkout A: {}", r.status());

    let r = client
        .post(format!("{}/items/{item_id}/checkin", base()))
        .bearer_auth(&token_a)
        .json(&json!({
            "sha256": sha_v2,
            "commit_message": "Second revision",
            "base_version": 1,
        }))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "checkin v2: {}", r.status());
    let v2: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v2["version"]["seq"], 2, "second version should be seq 2");

    // Stale check-in: base_version=1 but head is 2
    let _ = client
        .post(format!("{}/items/{item_id}/checkout", base()))
        .bearer_auth(&token_a)
        .json(&json!({ "workstation": "e2e-a" }))
        .send()
        .await
        .unwrap();
    let r = client
        .post(format!("{}/items/{item_id}/checkin", base()))
        .bearer_auth(&token_a)
        .json(&json!({
            "sha256": sha_v1,
            "commit_message": "stale",
            "base_version": 1,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409, "stale checkin should 409, got {}", r.status());
    let stale_body: serde_json::Value = r.json().await.unwrap();
    eprintln!("stale checkin error: {}", stale_body["error"]["message"].as_str().unwrap_or(""));

    // Clean up the extra checkout
    let _ = client.delete(format!("{}/items/{item_id}/checkout", base())).bearer_auth(&token_a).send().await.unwrap();

    // 7. Retrieve previous version
    let r = client.get(format!("{}/versions/{v1_id}/blob", base())).bearer_auth(&token_a).send().await.unwrap();
    assert!(r.status().is_success(), "download v1: {}", r.status());
    let v1_bytes = r.bytes().await.unwrap();
    let v1_downloaded_sha = sha256_hex(&v1_bytes);
    assert_eq!(v1_downloaded_sha, sha_v1, "v1 blob integrity failed");
    eprintln!("v1 blob integrity verified: {v1_downloaded_sha}");

    // 8. Audit history
    let r = client.get(format!("{}/audit?item={item_id}", base())).bearer_auth(&token_a).send().await.unwrap();
    assert!(r.status().is_success(), "audit: {}", r.status());
    let audit: serde_json::Value = r.json().await.unwrap();
    let events = audit["events"].as_array().expect("audit events array");
    let actions: Vec<&str> = events.iter().filter_map(|e| e["action"].as_str()).collect();
    eprintln!("audit actions: {actions:?}");
    assert!(actions.contains(&"item.created"), "no item.created in audit");
    assert!(actions.contains(&"item.checkout"), "no item.checkout in audit");
    assert!(actions.contains(&"item.checkin"), "no item.checkin in audit");

    // 9. Search finds the item
    let r = client.get(format!("{}/search?q={}", base(), part_number.replace('-', "%2D"))).bearer_auth(&token_a).send().await.unwrap();
    assert!(r.status().is_success(), "search: {}", r.status());
    let search: serde_json::Value = r.json().await.unwrap();
    let results = search["results"].as_array().expect("search results array");
    assert!(results.iter().any(|x| x["id"].as_str() == Some(item_id.as_str())), "search did not find item {part_number}");

    eprintln!("\nE2E VAULT PASSED — item {part_number}, 2 versions, audit verified, integrity OK");
}
