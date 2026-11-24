//! `POST /v2/workspaces/{workspace}/exports` over the lead's router and a
//! throwaway PostgreSQL (ADR-0084 d5, d10, d15; #425): a person's key
//! downloads an age file that opens with the recipient's identity and
//! verifies under the server's journal key; the refusals carry their codes
//! and statuses.

use std::io::Read as _;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::IdGen as _;
use oml_bundle::{VerifyOptions, crypt, key_fingerprint};
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_app, lead_state_over, pg_ping};
use oml_test_pg::{drop_db, mint_database, owner_url, run_tag, serving_url};

const ENROLLMENT: &str = "enr_export_fixture_token";
static DB_N: AtomicU64 = AtomicU64::new(0);

fn vectors() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shared/oml-bundle/tests/vectors")
}

/// The TEST-ONLY recipient of the bundle crate's vectors.
fn recipient() -> String {
    std::fs::read_to_string(vectors().join("test-recipient.txt"))
        .expect("the test recipient")
        .trim()
        .to_owned()
}

/// The TEST-ONLY identity that opens what is encrypted to [`recipient`].
fn identity() -> String {
    let hex_bytes = std::fs::read_to_string(vectors().join("TEST-ONLY-age-identity.hex"))
        .expect("the test identity");
    let bytes = hex::decode(hex_bytes.trim()).expect("hex");
    bech32::encode(
        "age-secret-key-",
        bech32::ToBase32::to_base32(&bytes),
        bech32::Variant::Bech32,
    )
    .expect("bech32")
    .to_uppercase()
}

struct Rig {
    router: Router,
    store: PgStore,
    db_name: String,
    fingerprint: String,
}

async fn rig() -> Option<Rig> {
    let db_name = format!(
        "oml_test_exports_{}_{}_{}",
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
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let fingerprint = key_fingerprint(&key.verifying_key());
    let app = lead_app(&store, key);
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
        fingerprint,
    })
}

async fn finish(rig: Rig) {
    drop(rig.router);
    rig.store.pool().close().await;
    drop_db(&rig.db_name).await;
}

async fn call(
    router: &Router,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, String, Vec<u8>) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json");
    if path == "/v2/keys" || path == "/v1/memories" {
        builder = builder.header("idempotency-key", format!("k-{}", OsIds.new_id()));
    }
    let response = router
        .clone()
        .oneshot(
            builder
                .body(Body::from(body.to_string()))
                .expect("a request"),
        )
        .await
        .expect("infallible router");
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let bytes = to_bytes(response.into_body(), 1 << 24)
        .await
        .expect("a body")
        .to_vec();
    (status, content_type, bytes)
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("JSON")
}

async fn bootstrap(router: &Router) -> (String, Uuid) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v2/bootstrap/claim")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "enrollment_token": ENROLLMENT,
                        "name": "root-admin",
                        "tenant_name": "export-tenant",
                        "admin_principal": "platform/admin",
                    })
                    .to_string(),
                ))
                .expect("a request"),
        )
        .await
        .expect("infallible router");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = json_of(&to_bytes(response.into_body(), 1 << 20).await.expect("body"));
    (
        body["value"].as_str().expect("a key").to_owned(),
        Uuid::parse_str(body["tenant_id"].as_str().expect("a tenant")).expect("uuid"),
    )
}

/// A writer key of `ada`, used by `used_by`, writer and reader on her root.
async fn ada_key(router: &Router, admin: &str, name: &str, used_by: &str) -> String {
    let (status, _, body) = call(
        router,
        "/v2/keys",
        admin,
        json!({
            "role": "contributor",
            "name": name,
            "principal": format!("ada/{name}"),
            "used_by": used_by,
            "user": "ada",
            "grants": [
                { "workspace": "ada", "role": "submitter" },
                { "workspace": "ada", "role": "reader" },
            ],
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    json_of(&body)["value"].as_str().expect("a key").to_owned()
}

fn code(body: &[u8]) -> (String, Value) {
    let v = json_of(body);
    (
        v["error"]["code"]
            .as_str()
            .unwrap_or("<no code>")
            .to_owned(),
        v["error"]["details"].clone(),
    )
}

#[tokio::test]
async fn a_person_downloads_an_age_file_that_opens_and_verifies_under_the_servers_journal_key() {
    let Some(rig) = rig().await else { return };
    let r = &rig.router;
    let (admin, tenant) = bootstrap(r).await;
    let person = ada_key(r, &admin, "ada-desk", "person").await;
    let (status, _, body) = call(
        r,
        "/v1/memories",
        &person,
        json!({ "body": "the kiln fires at 1200 C", "claim_key": "kiln" }),
    )
    .await;
    // A write lands held: 202 (http-api.md submit).
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let (status, content_type, file) = call(
        r,
        "/v2/workspaces/ada/exports",
        &person,
        json!({ "mode": "history", "recipients": [recipient()] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&file));
    assert_eq!(content_type, "application/octet-stream");
    let mut tar = Vec::new();
    crypt::decrypting_reader(file.as_slice(), &identity())
        .expect("opens")
        .read_to_end(&mut tar)
        .expect("plaintext");
    let verified = oml_bundle::read_bundle(
        tar.as_slice(),
        &VerifyOptions {
            expect_key: Some(rig.fingerprint.clone()),
            expect_tenant: Some(tenant.to_string()),
            ..VerifyOptions::default()
        },
    )
    .expect("verifies under the journal key");
    assert_eq!(verified.contents.manifest.source.server_id, rig.fingerprint);
    assert_eq!(verified.contents.versions.len(), 1);
    assert_eq!(verified.contents.events.len(), 1);
    finish(rig).await;
}

#[tokio::test]
async fn every_export_refusal_carries_its_code_and_status() {
    let Some(rig) = rig().await else { return };
    let r = &rig.router;
    let (admin, _) = bootstrap(r).await;
    let person = ada_key(r, &admin, "ada-desk", "person").await;
    let agent = ada_key(r, &admin, "ada-agent", "agent").await;
    let ok = json!({ "mode": "history", "recipients": [recipient()] });

    let (status, _, body) = call(r, "/v2/workspaces/ada/exports", &agent, ok.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (c, details) = code(&body);
    assert_eq!(c, "access.verb_ceiling");
    assert_eq!(details["reason"], "export_person_only");

    let (status, _, body) = call(
        r,
        "/v2/workspaces/ada/exports",
        &person,
        json!({ "mode": "history", "recipients": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(code(&body).0, "bundle.unencrypted");

    let (status, _, body) = call(
        r,
        "/v2/workspaces/ada/exports",
        &person,
        json!({ "mode": "release", "recipients": [recipient()] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (c, details) = code(&body);
    assert_eq!(c, "request.validation");
    assert_eq!(details["reason"], "releases_unbuilt");

    // universe lies outside ada's root: the default policy refuses it.
    let (status, _, body) = call(r, "/v2/workspaces/universe/exports", &person, ok).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let (c, details) = code(&body);
    assert_eq!(c, "export.disabled");
    assert_eq!(details["reason"], "own_root");
    finish(rig).await;
}
