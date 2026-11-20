//! The five upload routes over the lead's router and a throwaway
//! PostgreSQL (ADR-0004 d3, as amended 2026-11-17; #451): a bundle of
//! several parts uploads over HTTP, resumes and verifies; the refusals carry
//! their codes and statuses; another key reads absence; nothing is
//! journalled.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::domain::upload::PART_SIZE;
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{IdGen as _, JournalStore as _};
use oml_fetch::PinnedFetcher;
use oml_pg::PgStore;
use oml_server_lead::objects::LocalObjects;
use oml_server_lead::{
    OsIds, UploadSettings, lead_app, lead_state_over, pg_ping, start_lead_with_models,
};
use oml_test_pg::{drop_db, mint_database, owner_url, run_tag, serving_url};
use oml_transport_http::session::WebClient;
use oml_transport_http::traffic::Traffic;

const ENROLLMENT: &str = "enr_upload_fixture_token";
static DB_N: AtomicU64 = AtomicU64::new(0);

struct Rig {
    router: Router,
    store: PgStore,
    db_name: String,
    objects_dir: std::path::PathBuf,
}

async fn rig() -> Option<Rig> {
    let db_name = format!(
        "oml_test_uploads_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db_name).await {
        return None;
    }
    let store = PgStore::connect(&serving_url(&db_name))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db_name));
    store
        .migrate(&owner_url(&db_name))
        .await
        .expect("migrations apply");
    let objects_dir = std::env::temp_dir().join(format!("oml-uploads-http-{db_name}"));
    let objects = LocalObjects::open(&objects_dir).expect("an object directory");
    let app = lead_app(&store, SigningKey::from_bytes(&OsIds.new_secret())).with_uploads(
        Arc::new(objects),
        3 * PART_SIZE,
        4 * PART_SIZE,
    );
    let router = lead_state_over(
        app,
        &store,
        pg_ping(store.pool().clone()),
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    Some(Rig {
        router,
        store,
        db_name,
        objects_dir,
    })
}

async fn finish(rig: Rig) {
    drop(rig.router);
    rig.store.pool().close().await;
    drop_db(&rig.db_name).await;
    let _ = std::fs::remove_dir_all(&rig.objects_dir);
}

async fn call(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    if path == "/v2/keys" {
        builder = builder.header("idempotency-key", format!("issue-{}", OsIds.new_id()));
    }
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(body)).expect("a request"))
        .await
        .expect("infallible router");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("a body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("JSON")
    };
    (status, value)
}

async fn json_call(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    call(
        router,
        method,
        path,
        token,
        Some("application/json"),
        body.to_string().into_bytes(),
    )
    .await
}

async fn bootstrap(router: &Router) -> (String, Uuid) {
    let (status, body) = json_call(
        router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        json!({
            "enrollment_token": ENROLLMENT,
            "name": "root-admin",
            "tenant_name": "upload-tenant",
            "admin_principal": "platform/admin",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    (
        body["value"].as_str().expect("a key").to_owned(),
        Uuid::parse_str(body["tenant_id"].as_str().expect("a tenant")).expect("uuid"),
    )
}

async fn writer(router: &Router, admin: &str, name: &str) -> String {
    let (status, body) = json_call(
        router,
        "POST",
        "/v2/keys",
        Some(admin),
        json!({
            "role": "submitter",
            "name": name,
            "principal": format!("{name}/laptop"),
            "used_by": "person",
            "grants": [{ "workspace": "universe", "role": "submitter" }],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["value"].as_str().expect("a key").to_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("<no code>")
}

#[tokio::test]
async fn a_bundle_of_several_parts_uploads_over_http_resumes_and_verifies_and_another_key_reads_absence()
 {
    let Some(rig) = rig().await else { return };
    let r = &rig.router;
    let (admin, tenant) = bootstrap(r).await;
    let alice = writer(r, &admin, "alice-writer").await;
    let bob = writer(r, &admin, "bob-writer").await;
    let head = rig.store.chain_head(tenant).await.expect("head");
    let bytes: Vec<u8> = (0..(PART_SIZE + 77)).map(|i| (i % 253) as u8).collect();

    let (status, opened) = json_call(
        r,
        "POST",
        "/v2/uploads",
        Some(&alice),
        json!({ "purpose": "import", "size": bytes.len(), "sha256": sha256_hex(&bytes) }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    assert_eq!(opened["state"], "open");
    assert_eq!(opened["part_size"], PART_SIZE);
    assert_eq!(opened["parts"], 2);
    assert_eq!(opened["missing"], json!([1, 2]));
    let id = opened["upload_id"].as_str().expect("an id").to_owned();

    // An 8 MiB part passes the body limit; the JSON media type does not.
    let first = bytes[..PART_SIZE as usize].to_vec();
    let (status, body) = call(
        r,
        "PUT",
        &format!("/v2/uploads/{id}/parts/1"),
        Some(&alice),
        Some("application/json"),
        first.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{body}");
    let (status, body) = call(
        r,
        "PUT",
        &format!("/v2/uploads/{id}/parts/1"),
        Some(&alice),
        Some("application/octet-stream"),
        first,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["missing"], json!([2]));

    // Bob reads absence on every route.
    for (method, path) in [
        ("GET", format!("/v2/uploads/{id}")),
        ("POST", format!("/v2/uploads/{id}/complete")),
        ("DELETE", format!("/v2/uploads/{id}")),
    ] {
        let (status, body) = call(r, method, &path, Some(&bob), None, Vec::new()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}: {body}");
        assert_eq!(code(&body), "access.not_found");
    }

    // Completion with a part missing names it; the retry resumes.
    let (status, body) = call(
        r,
        "POST",
        &format!("/v2/uploads/{id}/complete"),
        Some(&alice),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(code(&body), "upload.incomplete");
    assert_eq!(body["error"]["details"]["missing"], json!([2]));
    let (status, body) = call(
        r,
        "PUT",
        &format!("/v2/uploads/{id}/parts/2"),
        Some(&alice),
        Some("application/octet-stream"),
        bytes[PART_SIZE as usize..].to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = call(
        r,
        "POST",
        &format!("/v2/uploads/{id}/complete"),
        Some(&alice),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "completed");
    let (status, body) = call(
        r,
        "GET",
        &format!("/v2/uploads/{id}"),
        Some(&alice),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "completed");
    assert!(
        body.get("part_key").is_none(),
        "the key is never answered: {body}"
    );

    // Abandoning shreds it; nothing about the upload was journalled.
    let (status, _) = call(
        r,
        "DELETE",
        &format!("/v2/uploads/{id}"),
        Some(&alice),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let row = rig
        .store
        .upload_session(tenant, Uuid::parse_str(&id).expect("uuid"))
        .await
        .expect("read")
        .expect("the row stays");
    assert_eq!(row.part_key, None);
    assert_eq!(rig.store.chain_head(tenant).await.expect("head"), head);
    finish(rig).await;
}

#[tokio::test]
async fn upload_refusals_carry_their_codes_and_statuses_over_http() {
    let Some(rig) = rig().await else { return };
    let r = &rig.router;
    let (admin, _) = bootstrap(r).await;
    let alice = writer(r, &admin, "alice-writer").await;
    let open = |size: u64, sha: String| json!({ "purpose": "import", "size": size, "sha256": sha });
    let refusals = [
        (
            open(3 * PART_SIZE + 1, "a".repeat(64)),
            StatusCode::PAYLOAD_TOO_LARGE,
            "upload.too_large",
        ),
        (
            open(0, "a".repeat(64)),
            StatusCode::BAD_REQUEST,
            "request.validation",
        ),
        (
            open(5, "A".repeat(64)),
            StatusCode::BAD_REQUEST,
            "request.validation",
        ),
        (
            json!({ "purpose": "attachment", "size": 5, "sha256": "a".repeat(64) }),
            StatusCode::BAD_REQUEST,
            "request.validation",
        ),
    ];
    for (body, status, expected) in refusals {
        let (got, answer) = json_call(r, "POST", "/v2/uploads", Some(&alice), body).await;
        assert_eq!((got, code(&answer)), (status, expected), "{answer}");
    }
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/uploads",
        Some(&admin),
        open(5, "a".repeat(64)),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "access.verb_ceiling"),
        "{body}"
    );

    let bytes = b"0123456789".to_vec();
    let (_, opened) = json_call(
        r,
        "POST",
        "/v2/uploads",
        Some(&alice),
        open(10, sha256_hex(&bytes)),
    )
    .await;
    let id = opened["upload_id"].as_str().expect("an id").to_owned();
    let put = |n: &str, body: Vec<u8>| {
        let path = format!("/v2/uploads/{id}/parts/{n}");
        let alice = alice.clone();
        async move {
            call(
                r,
                "PUT",
                &path,
                Some(&alice),
                Some("application/octet-stream"),
                body,
            )
            .await
        }
    };
    let (status, body) = put("1", bytes[..9].to_vec()).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "upload.part_size"),
        "{body}"
    );
    let (status, body) = put("2", bytes.clone()).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "upload.part_number"),
        "{body}"
    );
    let (status, body) = put("x", bytes.clone()).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "upload.part_number"),
        "{body}"
    );
    let mut wrong = bytes.clone();
    wrong[0] = b'9';
    let (status, _) = put("1", wrong).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        r,
        "POST",
        &format!("/v2/uploads/{id}/complete"),
        Some(&alice),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "upload.digest_mismatch"),
        "{body}"
    );
    let (status, body) = put("1", bytes.clone()).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "upload.session_invalid"),
        "{body}"
    );
    assert_eq!(body["error"]["details"]["state"], "failed");

    // The tenant's 4 parts' worth, then the key's fourth session.
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/uploads",
        Some(&alice),
        open(3 * PART_SIZE, "b".repeat(64)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/uploads",
        Some(&alice),
        open(2 * PART_SIZE, "c".repeat(64)),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "upload.tenant_limit"),
        "{body}"
    );
    for _ in 0..3 {
        let (status, body) = json_call(
            r,
            "POST",
            "/v2/uploads",
            Some(&alice),
            open(1, "d".repeat(64)),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/uploads",
        Some(&alice),
        open(1, "d".repeat(64)),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "upload.session_limit"),
        "{body}"
    );
    assert_eq!(body["error"]["details"]["limit"], 4);
    finish(rig).await;
}

/// A lead whose `storage.url` names a directory it cannot create still
/// starts and is ready; an upload then answers `upload.storage_failed`
/// (503, retryable), and the metrics listener names the reason the doctor
/// line reads (ADR-0004 d3, note of 2026-11-18).
#[tokio::test]
async fn a_lead_whose_storage_url_cannot_be_created_boots_serves_readyz_and_answers_uploads_storage_failed()
 {
    let db_name = format!(
        "oml_test_uploads_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db_name).await {
        return;
    }
    let store = PgStore::connect(&serving_url(&db_name))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db_name));
    store
        .migrate(&owner_url(&db_name))
        .await
        .expect("migrations apply");
    // A directory below a regular file cannot be created, as root too.
    let blocker = std::env::temp_dir().join(format!("oml-uploads-blocker-{db_name}"));
    std::fs::write(&blocker, b"not a directory").expect("a file");
    let traffic = Traffic::default();
    let metrics = Arc::clone(traffic.metrics());
    let started = start_lead_with_models(
        &store,
        SigningKey::from_bytes(&OsIds.new_secret()),
        None,
        PinnedFetcher,
        traffic,
        None,
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
        None,
        WebClient::default(),
        false,
        None,
        Some(UploadSettings {
            storage_dir: blocker.join("objects"),
            ..UploadSettings::default()
        }),
    )
    .await
    .expect("the lead starts without a usable object store");
    let r = &started.router;

    let (status, body) = call(r, "GET", "/readyz", None, None, Vec::new()).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let token = started
        .enrollment_token
        .clone()
        .expect("an enrollment token");
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/bootstrap/claim",
        None,
        json!({
            "enrollment_token": token,
            "name": "root-admin",
            "tenant_name": "upload-tenant",
            "admin_principal": "platform/admin",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let admin = body["value"].as_str().expect("a key").to_owned();
    let alice = writer(r, &admin, "alice-writer").await;
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/uploads",
        Some(&alice),
        json!({ "purpose": "import", "size": 10, "sha256": "a".repeat(64) }),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::SERVICE_UNAVAILABLE, "upload.storage_failed"),
        "{body}"
    );

    let scraped = metrics.encode();
    let line = scraped
        .lines()
        .find(|l| l.starts_with("oml_upload_store_unavailable{"))
        .unwrap_or_else(|| panic!("no unavailable line in {scraped}"));
    assert!(line.ends_with("} 1"), "{line}");
    assert!(line.contains("reason=\"not_a_directory\""), "{line}");

    drop(started);
    store.pool().close().await;
    drop_db(&db_name).await;
    let _ = std::fs::remove_file(&blocker);
}
