//! The two import routes over the lead's router and a throwaway
//! PostgreSQL (ADR-0084 d6-d11, d15, R11, I4; #426): a person's writer key
//! uploads a bundle and plans, then runs, an import into a new name created
//! in progress; the refusals reach the route with their codes and statuses;
//! another key reads absence.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::IdGen as _;
use oml_pg::PgStore;
use oml_server_lead::objects::LocalObjects;
use oml_server_lead::{OsIds, lead_app, lead_state_over, pg_ping};
use oml_test_pg::{drop_db, mint_database, owner_url, run_tag, serving_url};

const ENROLLMENT: &str = "enr_import_fixture_token";
static DB_N: AtomicU64 = AtomicU64::new(0);

struct Rig {
    router: Router,
    store: PgStore,
    db_name: String,
    objects_dir: std::path::PathBuf,
}

async fn rig() -> Option<Rig> {
    let db_name = format!(
        "oml_test_imports_{}_{}_{}",
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
    let objects_dir = std::env::temp_dir().join(format!("oml-imports-http-{db_name}"));
    let objects = LocalObjects::open(&objects_dir).expect("an object directory");
    let app = lead_app(&store, SigningKey::from_bytes(&OsIds.new_secret())).with_uploads(
        Arc::new(objects),
        oml_app::domain::upload::DEFAULT_MAX_BYTES,
        oml_app::domain::upload::DEFAULT_TENANT_MAX_BYTES,
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

/// Polls `GET /v2/imports/{id}` until the import is no longer running
/// (ADR-0084 I10), and answers its row.
async fn ended(r: &Router, key: &str, id: &str) -> Value {
    for _ in 0..500 {
        let (status, read) = call(
            r,
            "GET",
            &format!("/v2/imports/{id}"),
            Some(key),
            None,
            Vec::new(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{read}");
        if read["import"]["state"] != "running" {
            return read["import"].clone();
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("import {id} still running after 10 s");
}

/// The scan runs on imported text: a version carrying an age identity is
/// refused and named, and the others land held (ADR-0084 d6, d11).
#[tokio::test]
async fn an_imported_version_carrying_an_age_secret_key_is_refused_by_the_scan_and_named() {
    let Some(rig) = rig().await else { return };
    let r = &rig.router;
    let (admin, _) = bootstrap(r).await;
    let ada = ada_key(r, &admin, "ada-laptop", "person").await;
    let fingerprint = oml_bundle::key_fingerprint(&source_key().verifying_key());
    // Built at run time with a checksum that holds, so no identity sits in
    // the source and the scan's bech32 check reads it as one.
    let identity = bech32::encode(
        "age-secret-key-",
        bech32::ToBase32::to_base32(&[7_u8; 32]),
        bech32::Variant::Bech32,
    )
    .expect("bech32")
    .to_uppercase();
    let bytes = bundle_with(vec![
        line("art-1", "deploy-day", "Deploys go out on Tuesdays."),
        line("art-2", "the-key", &format!("The identity is {identity}.")),
    ]);
    let up = upload(r, &ada, &bytes).await;
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/imports",
        Some(&ada),
        json!({ "upload_id": up, "target": "notes", "expect_key": fingerprint }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let id = body["import"]["import_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let import = ended(r, &ada, &id).await;
    assert_eq!(import["state"], "done", "{import}");
    assert_eq!(import["counts"]["new"], 1, "{import}");
    assert_eq!(import["counts"]["refused"], 1, "{import}");
    let refused = import["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .find(|r| r["claim_key"] == "the-key")
        .expect("the key is named")
        .clone();
    assert_eq!(refused["code"], "submission.scan_veto", "{import}");
    finish(rig).await;
}

/// A document travels as its body and its kind object, never its
/// passages, which are derived: the import writes it through the one write
/// path, which cuts its passages again by the one rule (ADR-0081 d3;
/// ADR-0084 d6).
#[tokio::test]
async fn an_imported_document_is_cut_into_its_passages_again_on_the_target() {
    let Some(rig) = rig().await else { return };
    let r = &rig.router;
    let (admin, tenant) = bootstrap(r).await;
    let ada = ada_key(r, &admin, "ada-laptop", "person").await;
    let fingerprint = oml_bundle::key_fingerprint(&source_key().verifying_key());
    let text = "# Runbook\n\nWhat it covers.\n\n## Restart\n\nRestart the lead.\n";
    let mut doc = line("art-d", "doc/runbook", text);
    doc.kind = "document".to_owned();
    doc.format = Some("text/markdown".to_owned());
    doc.document = Some(json!({"title": "Runbook"}));
    let up = upload(r, &ada, &bundle_with(vec![doc])).await;
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/imports",
        Some(&ada),
        json!({ "upload_id": up, "target": "notes", "expect_key": fingerprint }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let id = body["import"]["import_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let import = ended(r, &ada, &id).await;
    assert_eq!(import["state"], "done", "{import}");
    assert_eq!(import["counts"]["new"], 1, "{import}");
    let mut tx = rig.store.pool().begin().await.expect("begin");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("tenant");
    let fragments: Vec<String> = sqlx::query_scalar(
        "SELECT d.fragment FROM document_passages d JOIN memories m \
           ON m.tenant_id = d.tenant_id AND m.id = d.memory_id \
          WHERE d.tenant_id = $1 AND m.claim_key = 'doc/runbook' ORDER BY d.ordinal",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await
    .expect("passages");
    tx.rollback().await.expect("rollback");
    assert_eq!(fragments, vec!["runbook", "restart"], "cut again on import");
    finish(rig).await;
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
    if path == "/v2/keys" || path == "/v2/workspaces" {
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
            "tenant_name": "import-tenant",
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

async fn ada_key(router: &Router, admin: &str, name: &str, used_by: &str) -> String {
    let (status, body) = json_call(
        router,
        "POST",
        "/v2/keys",
        Some(admin),
        json!({
            "role": "writer",
            "name": name,
            "principal": format!("ada/{name}"),
            "user": "ada",
            "used_by": used_by,
            "grants": [
                { "workspace": "ada", "role": "writer" },
                { "workspace": "ada", "role": "reader" },
            ],
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

fn source_key() -> SigningKey {
    SigningKey::from_bytes(&[42_u8; 32])
}

fn line(artifact_id: &str, claim_key: &str, text: &str) -> oml_bundle::VersionLine {
    oml_bundle::VersionLine {
        artifact_id: artifact_id.to_owned(),
        version: 1,
        claim_key: claim_key.to_owned(),
        workspace: "universe.ada.notes".to_owned(),
        kind: "memory".to_owned(),
        format: None,
        title: None,
        state: oml_bundle::State::Promoted,
        retirement: false,
        replaces: None,
        simulation_id: None,
        text: Some(text.to_owned()),
        body: None,
        plaintext_digest: oml_bundle::sha256_digest(text.as_bytes()),
        anchor: Value::Null,
        valid_from: None,
        valid_until: None,
        skill: None,
        document: None,
        test: None,
        guardrail: None,
        derives_from: Vec::new(),
        origin: json!({"kind": "manual"}),
        attachments: Vec::new(),
        author: oml_bundle::Author {
            used_by: "person".to_owned(),
            same_user: false,
            pseudonym: "b".repeat(64),
        },
        created_at: "2026-11-01T10:00:00.000000Z".to_owned(),
        promoted_at: Some("2026-11-01T11:00:00.000000Z".to_owned()),
        source_event: None,
    }
}

fn bundle() -> Vec<u8> {
    bundle_with(vec![
        line("art-1", "deploy-day", "Deploys go out on Tuesdays."),
        line("art-2", "rollback", "A rollback is one command."),
    ])
}

fn bundle_with(lines: Vec<oml_bundle::VersionLine>) -> Vec<u8> {
    let contents = oml_bundle::BundleContents {
        manifest: oml_bundle::Manifest {
            format: oml_bundle::FORMAT.to_owned(),
            source: oml_bundle::Source {
                server_id: "00000000-0000-4000-8000-0000000000f1".to_owned(),
                tenant_id: "00000000-0000-4000-8000-0000000000aa".to_owned(),
                tenant_name: "source".to_owned(),
            },
            workspaces: vec!["universe.ada.notes".to_owned()],
            mode: oml_bundle::Mode::History,
            release: None,
            since: None,
            dropped: Vec::new(),
            exported_at: "2026-11-17T09:00:00.000000Z".to_owned(),
            exporter_pseudonym: "b".repeat(64),
            event_keys: Vec::new(),
            files: Vec::new(),
            signature: None,
        },
        workspaces: Vec::new(),
        versions: lines
            .into_iter()
            .map(|v| oml_bundle::VersionEntry::Version(Box::new(v)))
            .collect(),
        verdicts: Vec::new(),
        events: Vec::new(),
        edges: Vec::new(),
        bodies: std::collections::BTreeMap::new(),
    };
    let mut out = Vec::new();
    oml_bundle::write_bundle(&contents, &source_key(), &mut out).expect("the fixture writes");
    out
}

/// Opens, fills and completes one upload of `bytes` (a single part).
async fn upload(r: &Router, key: &str, bytes: &[u8]) -> String {
    let (status, opened) = json_call(
        r,
        "POST",
        "/v2/uploads",
        Some(key),
        json!({ "purpose": "import", "size": bytes.len(), "sha256": sha256_hex(bytes) }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    let id = opened["upload_id"].as_str().expect("an id").to_owned();
    let (status, body) = call(
        r,
        "PUT",
        &format!("/v2/uploads/{id}/parts/1"),
        Some(key),
        Some("application/octet-stream"),
        bytes.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = json_call(
        r,
        "POST",
        &format!("/v2/uploads/{id}/complete"),
        Some(key),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

#[tokio::test]
async fn a_person_plans_then_imports_a_bundle_over_http_and_each_refusal_carries_its_code() {
    let Some(rig) = rig().await else { return };
    let r = &rig.router;
    let (admin, _) = bootstrap(r).await;
    let ada = ada_key(r, &admin, "ada-laptop", "person").await;
    let agent = ada_key(r, &admin, "ada-agent", "agent").await;
    let fingerprint = oml_bundle::key_fingerprint(&source_key().verifying_key());
    let bytes = bundle();

    // An agent's key never imports (d11; I1's shape).
    let up = upload(r, &agent, &bytes).await;
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/imports",
        Some(&agent),
        json!({ "upload_id": up, "target": "notes", "expect_key": fingerprint }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(code(&body), "access.verb_ceiling");
    assert_eq!(body["error"]["details"]["reason"], "import_person_only");

    // A first import names the key it pins (R11).
    let up = upload(r, &ada, &bytes).await;
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/imports",
        Some(&ada),
        json!({ "upload_id": up, "target": "notes" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(code(&body), "request.validation");
    assert_eq!(body["error"]["details"]["field"], "/expect_key");

    // A dry run answers the plan and binds nothing.
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/imports",
        Some(&ada),
        json!({ "upload_id": up, "target": "notes", "dry_run": true, "expect_key": fingerprint }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["plan"]["counts"]["new"], 2, "{body}");
    assert_eq!(body["plan"]["authenticated"], true);
    assert_eq!(body["plan"]["target_workspace_id"], Value::Null);

    // I10: the import answers 202 with its row running, and runs on.
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/imports",
        Some(&ada),
        json!({ "upload_id": up, "target": "notes", "expect_key": fingerprint }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["import"]["state"], "running", "{body}");
    let id = body["import"]["import_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let import = ended(r, &ada, &id).await;
    assert_eq!(import["state"], "done", "{import}");
    assert_eq!(import["counts"]["new"], 2, "{import}");
    assert_eq!(import["position"], 2);

    let (status, read) = call(
        r,
        "GET",
        &format!("/v2/imports/{id}"),
        Some(&ada),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["import"]["state"], "done");
    let (status, body) = call(
        r,
        "GET",
        &format!("/v2/imports/{id}"),
        Some(&agent),
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // The upload bound once: a second import of it is the session's state.
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/imports",
        Some(&ada),
        json!({ "upload_id": up, "target": "notes", "expect_key": fingerprint }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(code(&body), "upload.session_invalid");
    assert_eq!(body["error"]["details"]["state"], "bound");

    // A tampered bundle is refused under its d7 code, 422.
    let mut tampered = bytes.clone();
    let at = tampered
        .windows(8)
        .position(|w| w == b"Tuesdays")
        .expect("the line");
    tampered[at] = b'W';
    let up = upload(r, &ada, &tampered).await;
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/imports",
        Some(&ada),
        json!({ "upload_id": up, "target": "notes", "expect_key": fingerprint }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(code(&body), "bundle.digest_mismatch");

    // A standard workspace another key created is not an import target
    // (d6; ADR-0030 d1 as amended 2026-11-18).
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/workspaces",
        Some(&admin),
        json!({ "name": "plain", "parent": "ada" }),
    )
    .await;
    assert!(status.is_success(), "{body}");
    let up = upload(r, &ada, &bytes).await;
    let (status, body) = json_call(
        r,
        "POST",
        "/v2/imports",
        Some(&ada),
        json!({ "upload_id": up, "target": "plain", "expect_key": fingerprint }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(code(&body), "import.target_not_in_progress");
    finish(rig).await;
}
