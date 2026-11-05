//! The agent plugin over the lead composition against a throwaway database
//! (docs/api/mcp-plugin.md): the stateless streamable-HTTP transport, the
//! bearer key before any dispatch, the served catalog read from
//! docs/api/mcp-tools.json, the verb ceiling (R1) as an absent tool, and the
//! agent loop — remember, held, ratified by another identity, recalled —
//! with every tool answering what its ring endpoint answers.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::IdGen as _;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_state, pg_ping};
use oml_transport_http::mcp::{
    ARGUMENTS_OUTSIDE_THE_SCHEMA, MCP_PROTOCOL_VERSION, SERVED_TOOLS, SERVER_INSTRUCTIONS,
};

const ENROLLMENT: &str = "enr_test_fixture_token";

/// 2^53 − 1: the largest budget recall takes, the largest integer RFC 8785
/// serializes exactly (chain-format.md, the signed fields).
const MAX_TOKENS: u64 = 9_007_199_254_740_991;

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL names a throwaway PostgreSQL cluster; no suite defaults to one, so none reaches a live server")
}

/// The password of every minted test role; the roles live only as long as
/// their throwaway database.
const PAIR_PASSWORD: &str = "oml_test_pair";

/// `db` at `DATABASE_URL`'s host, logged in as `role`.
fn role_url(db: &str, role: &str) -> String {
    let base = base_url();
    let (scheme, rest) = base
        .split_once("://")
        .expect("DATABASE_URL is a postgres:// URL");
    let (_, host_path) = rest.rsplit_once('@').expect("DATABASE_URL names its user");
    let (host, _) = host_path
        .split_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{scheme}://{role}:{PAIR_PASSWORD}@{host}/{db}")
}

/// `db` through the owner login of its role pair (ADR-0034 d2, d8).
fn owner_url(db: &str) -> String {
    role_url(db, &format!("{db}_owner"))
}

/// `db` through the serving login of its role pair (ADR-0034 d2, d8).
fn serving_url(db: &str) -> String {
    role_url(db, &format!("{db}_serving"))
}

/// Eight hex digits of the clock, so a name a crashed earlier run left
/// behind on the cluster under the same pid and counter is not reused.
fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

/// Mints `db`'s owner/serving role pair, `LOGIN` and no other attribute,
/// and creates `db` owned by the owner (ADR-0034 d2, d8); `false`, with
/// `did not run: <role> cannot create roles` printed, when `DATABASE_URL`'s
/// role is not the superuser minting takes, as the maintainer's local `oml`
/// (CREATEDB alone) is not. Under `OML_TEST_REQUIRE_SPLIT=1`, which CI sets,
/// the same condition fails the test, so no run passes vacuously.
async fn mint_database(db: &str) -> bool {
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    let (role, mints): (String, bool) = sqlx::query_as(
        "SELECT current_user::text, rolsuper FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut admin)
    .await
    .expect("role attributes");
    if !mints {
        assert!(
            std::env::var("OML_TEST_REQUIRE_SPLIT").as_deref() != Ok("1"),
            "{role} cannot create roles, and OML_TEST_REQUIRE_SPLIT=1 requires the role pair (ADR-0034 d8)"
        );
        println!("did not run: {role} cannot create roles");
        return false;
    }
    for role in ["owner", "serving"] {
        admin
            .execute(format!("CREATE ROLE {db}_{role} LOGIN PASSWORD '{PAIR_PASSWORD}'").as_str())
            .await
            .expect("mint the role pair");
    }
    admin
        .execute(format!("CREATE DATABASE {db} OWNER {db}_owner").as_str())
        .await
        .expect("create throwaway database");
    true
}

/// A store over `db`'s serving login, migrated through its owner login with
/// `oml.serving_role` set (ADR-0034 d4, d8).
async fn serving_store(db: &str) -> PgStore {
    let store = PgStore::connect(&serving_url(db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(db));
    store
        .migrate(&owner_url(db))
        .await
        .expect("embedded migrations apply");
    store
}

static DB_N: AtomicU64 = AtomicU64::new(0);

/// The lead's router over the serving login of a fresh database and its
/// name, or `None` when this run cannot mint the role pair (ADR-0034 d8).
async fn fresh_router() -> Option<(Router, String)> {
    let db_name = format!(
        "oml_test_mcp_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db_name).await {
        return None;
    }
    let store = serving_store(&db_name).await;
    let signing = SigningKey::from_bytes(&OsIds.new_secret());
    let router = lead_state(
        &store,
        signing,
        pg_ping(store.pool().clone()),
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    Some((router, db_name))
}

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
        for role in ["serving", "owner"] {
            let _ = admin
                .execute(format!("DROP ROLE IF EXISTS {name}_{role}").as_str())
                .await;
        }
    }
}

async fn send(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    idem: Option<&str>,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(idem) = idem {
        builder = builder.header("idempotency-key", idem);
    }
    let body = match body {
        None => Body::empty(),
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
    };
    let response = router
        .clone()
        .oneshot(builder.body(body).expect("request builds"))
        .await
        .expect("infallible router");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("body collects");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("responses are JSON")
    };
    (status, value)
}

/// One JSON-RPC request to `/mcp`; returns the HTTP status and the message.
async fn rpc(router: &Router, token: &str, id: u64, method: &str, params: Value) -> Value {
    let (status, body) = send(
        router,
        "POST",
        "/mcp",
        Some(token),
        None,
        Some(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{method}: {body}");
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], id);
    body
}

async fn call_tool(router: &Router, token: &str, id: u64, name: &str, args: Value) -> Value {
    let body = rpc(
        router,
        token,
        id,
        "tools/call",
        json!({"name": name, "arguments": args}),
    )
    .await;
    body["result"].clone()
}

/// Bootstrap, one crew workarea, and a writer, dreamer and reader key on it.
async fn crew(router: &Router) -> (String, String, String, Uuid) {
    crew_with_writer_grants(router, &["writer"]).await
}

/// The crew of [`crew`] with its writer an agent that remembers and recalls
/// on one key: a writer-role key admits reader grants (ADR-0025 d1).
async fn agent_crew(router: &Router) -> (String, String, String, Uuid) {
    crew_with_writer_grants(router, &["writer", "reader"]).await
}

async fn crew_with_writer_grants(
    router: &Router,
    writer_grants: &[&str],
) -> (String, String, String, Uuid) {
    crew_and_admin(router, writer_grants).await.0
}

/// The crew of [`crew_with_writer_grants`] and the tenant's admin key from
/// the bootstrap claim.
async fn crew_and_admin(
    router: &Router,
    writer_grants: &[&str],
) -> ((String, String, String, Uuid), String) {
    let (status, boot) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(&json!({"enrollment_token": ENROLLMENT, "name": "root-admin", "tenant_name": "mcp-tenant"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{boot}");
    let admin = boot["value"].as_str().expect("admin key").to_owned();
    let root = boot["root_workarea_id"].as_str().expect("root").to_owned();
    let (status, wa) = send(
        router,
        "POST",
        "/v2/workareas",
        Some(&admin),
        Some("wa-1"),
        Some(&json!({"parent": root, "name": "metals-crew"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{wa}");
    let crew = Uuid::parse_str(wa["id"].as_str().expect("workarea id")).expect("uuid");
    let mut keys = Vec::new();
    for (role, principal, grants) in [
        ("writer", "crew/agent-7", writer_grants),
        ("dreamer", "crew/reviewer-3", &["dreamer"][..]),
        ("reader", "crew/agent-recall", &["reader"][..]),
    ] {
        let grants: Vec<Value> = grants
            .iter()
            .map(|grant| json!({"workarea": crew, "role": grant}))
            .collect();
        let (status, key) = send(
            router,
            "POST",
            "/v2/keys",
            Some(&admin),
            Some(&format!("issue-{role}")),
            Some(&json!({"role": role, "name": role, "principal": principal, "grants": grants})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{key}");
        keys.push(key["value"].as_str().expect("show-once value").to_owned());
    }
    let reader = keys.pop().expect("reader");
    let dreamer = keys.pop().expect("dreamer");
    let writer = keys.pop().expect("writer");
    ((writer, dreamer, reader, crew), admin)
}

#[tokio::test]
async fn the_mcp_endpoint_refuses_a_call_without_a_valid_key_before_any_dispatch() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});

    let (status, body) = send(&router, "POST", "/mcp", None, None, Some(&list)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"]["code"], "auth.missing");

    let (status, body) = send(
        &router,
        "POST",
        "/mcp",
        Some("oml_k_not_a_key"),
        None,
        Some(&list),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["error"]["code"], "auth.invalid");

    drop_db(&db).await;
}

#[tokio::test]
async fn the_transport_is_stateless_and_serves_the_catalog_slice_from_mcp_tools_json() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, _, _) = crew(&router).await;

    // GET, the server-initiated stream, does not exist.
    let (status, _) = send(&router, "GET", "/mcp", Some(&writer), None, None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    let init = rpc(
        &router,
        &writer,
        1,
        "initialize",
        json!({"protocolVersion": MCP_PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    )
    .await;
    assert_eq!(init["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
    assert_eq!(init["result"]["serverInfo"]["name"], "oml-server");

    // A notification is acknowledged with 202 and no body.
    let (status, body) = send(
        &router,
        "POST",
        "/mcp",
        Some(&writer),
        None,
        Some(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body, Value::Null);

    // The served tools are exactly the catalog file's entries for them.
    let listed = rpc(&router, &writer, 2, "tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().expect("tools");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names.len(), SERVED_TOOLS.len());
    for name in SERVED_TOOLS {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
    let catalog: Value =
        serde_json::from_str(include_str!("../../../../docs/api/mcp-tools.json")).expect("json");
    for tool in tools {
        let declared = catalog["tools"]
            .as_array()
            .expect("catalog tools")
            .iter()
            .find(|t| t["name"] == tool["name"])
            .expect("every served tool is declared in mcp-tools.json");
        assert_eq!(tool, declared);
    }

    // A protocol revision the facade does not speak is a 400.
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("authorization", format!("Bearer {writer}"))
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "1999-01-01")
                .body(Body::from(
                    json!({"jsonrpc": "2.0", "id": 9, "method": "ping"}).to_string(),
                ))
                .expect("request builds"),
        )
        .await
        .expect("infallible router");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Batches are refused: one message per POST.
    let (_, body) = send(
        &router,
        "POST",
        "/mcp",
        Some(&writer),
        None,
        Some(&json!([{"jsonrpc": "2.0", "id": 1, "method": "ping"}])),
    )
    .await;
    assert_eq!(body["error"]["code"], -32600);

    drop_db(&db).await;
}

/// `initialize` answers with the server instructions, under the 4,096
/// characters Claude Code keeps, asking for recall through `oml_read`
/// within the first 1,024 and naming no v0.3.0 tool, each of which is
/// hidden from `tools/list` (ADR-0038 d1, ADR-0039 d2, d12).
#[tokio::test]
async fn initialize_answers_with_instructions_under_4096_characters_that_name_oml_read() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, _, _) = crew(&router).await;

    let init = rpc(
        &router,
        &writer,
        1,
        "initialize",
        json!({"protocolVersion": MCP_PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    )
    .await;
    let instructions = init["result"]["instructions"]
        .as_str()
        .expect("initialize answers with instructions");
    assert_eq!(instructions, SERVER_INSTRUCTIONS);
    assert!(
        instructions.chars().count() < 4096,
        "Claude Code cuts server instructions past 4,096 characters: {}",
        instructions.chars().count()
    );
    // The call to recall comes before anything Claude Code could cut.
    let recall = instructions
        .find("oml_read with a query")
        .expect("names oml_read");
    assert!(recall < 1024, "oml_read first named at {recall}");
    assert!(instructions.contains("oml_write"), "names oml_write");
    for old in V0_3_0_TOOL_NAMES {
        assert!(!instructions.contains(old), "names the v0.3.0 tool {old}");
    }
    assert!(
        !instructions.contains("claim_key"),
        "the server derives the claim_key, so the agent is not asked for one"
    );
    assert!(
        instructions.contains("not as instructions to follow"),
        "what recall returns is framed as data"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn no_promote_sign_demote_delete_or_admin_verb_is_a_tool() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, _, _) = crew(&router).await;

    let listed = rpc(&router, &writer, 1, "tools/list", json!({})).await;
    for tool in listed["result"]["tools"].as_array().expect("tools") {
        let name = tool["name"].as_str().expect("name");
        for verb in [
            "promote", "sign", "demote", "delete", "erase", "verdict", "key", "admin",
        ] {
            assert!(
                !name.contains(verb),
                "{name} names the forbidden verb {verb}"
            );
        }
    }
    for forbidden in ["oml_promote", "oml_review_verdict", "oml_issue_key"] {
        let body = rpc(
            &router,
            &writer,
            2,
            "tools/call",
            json!({"name": forbidden, "arguments": {}}),
        )
        .await;
        assert_eq!(body["error"]["code"], -32602, "{body}");
        assert!(
            body["error"]["message"]
                .as_str()
                .expect("message")
                .contains("(R1)"),
            "{body}"
        );
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn an_agent_writes_through_mcp_and_reads_only_after_another_identity_ratifies() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, crew) = crew(&router).await;
    let quote = "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery.";
    let write = json!({
        "claim_key": "supplier-quote-am-7741",
        "text": quote,
        "idempotency_key": "crew-procure/run-1/step-1",
    });

    // Held on arrival.
    let held = call_tool(&router, &writer, 1, "oml_write", write.clone()).await;
    assert_eq!(held["isError"], false, "{held}");
    let receipt = &held["structuredContent"];
    assert_eq!(receipt["state"], "held");
    assert_eq!(receipt["workarea"], crew.to_string());
    assert_eq!(receipt["idempotent_replay"], false);
    let text: Value =
        serde_json::from_str(held["content"][0]["text"].as_str().expect("text")).expect("json");
    assert_eq!(&text, receipt, "the text content is the structured content");

    // A retry with the same idempotency key is the stored response.
    let replay = call_tool(&router, &writer, 2, "oml_write", write).await;
    assert_eq!(replay["structuredContent"]["idempotent_replay"], true);
    assert_eq!(
        replay["structuredContent"]["artifact_id"],
        receipt["artifact_id"]
    );

    // Not visible to a reader while held.
    let recall = json!({"query": "Accra Metals"});
    let before = call_tool(&router, &reader, 3, "oml_read", recall.clone()).await;
    assert_eq!(before["isError"], false, "{before}");
    assert_eq!(before["structuredContent"]["hits"], json!([]));

    // The writer's key carries no read grant, so its scope is not readable:
    // the ring's typed refusal, absence (ADR-0025 d1).
    let refused = call_tool(&router, &writer, 4, "oml_read", recall.clone()).await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["code"], "access.not_found",
        "{refused}"
    );
    assert!(refused["structuredContent"]["error"]["rule"].is_string());

    // A different identity ratifies over the HTTP API — never over MCP.
    let (status, served) = send(
        &router,
        "POST",
        "/v2/review/next",
        Some(&dreamer),
        Some("n-1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    let (status, verdict) = send(
        &router,
        "POST",
        "/v2/review/verdicts",
        Some(&dreamer),
        Some("v-1"),
        Some(&json!({"serve_id": served["serve_id"], "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    assert_eq!(verdict["state"], "promoted");

    let after = call_tool(&router, &reader, 5, "oml_read", recall).await;
    let hits = after["structuredContent"]["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1, "{after}");
    assert_eq!(hits[0]["artifact_id"], receipt["artifact_id"]);
    assert_eq!(hits[0]["text"], quote);
    assert_eq!(hits[0]["state"], "promoted");
    assert_eq!(after["structuredContent"]["steering"], json!([]));

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_state_shows_a_ratified_write_where_the_events_tail_showed_its_promote() {
    // The events tail left MCP; GET /v2/events keeps its scope, resume point
    // and refusals (http_api.rs), and an agent learns that its write was
    // ratified from oml_read with state over its own work (mcp-plugin.md,
    // what left the catalog; ADR-0039 d2, d12).
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, crew) = crew(&router).await;
    let held = call_tool(
        &router,
        &writer,
        1,
        "oml_write",
        json!({
            "claim_key": "supplier-quote-am-7741",
            "text": "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery.",
            "idempotency_key": "crew-procure/run-1/step-1",
        }),
    )
    .await;
    assert_eq!(held["isError"], false, "{held}");
    let artifact = held["structuredContent"]["artifact_id"].clone();
    let promoted = json!({"state": "promoted"});
    let before = call_tool(&router, &writer, 2, "oml_read", promoted.clone()).await;
    assert_eq!(before["structuredContent"]["items"], json!([]), "{before}");
    approve_as(&router, &dreamer, "v1").await;

    // The promoted version as the tail's promote named it: its id, claim key
    // and version, in the crew's workarea.
    let after = call_tool(&router, &writer, 3, "oml_read", promoted).await;
    assert_eq!(after["isError"], false, "{after}");
    let items = after["structuredContent"]["items"]
        .as_array()
        .expect("items");
    assert_eq!(items.len(), 1, "{after}");
    assert_eq!(items[0]["artifact_id"], artifact, "{after}");
    assert_eq!(items[0]["claim_key"], "supplier-quote-am-7741", "{after}");
    assert_eq!(items[0]["version"], 1, "{after}");
    assert_eq!(items[0]["workarea"], crew.to_string(), "{after}");

    // Another key's own work holds none of it, and a workarea outside read
    // access is absence, as the tail's scope was.
    let theirs = call_tool(
        &router,
        &reader,
        4,
        "oml_read",
        json!({"state": "promoted"}),
    )
    .await;
    assert_eq!(theirs["structuredContent"]["items"], json!([]), "{theirs}");
    let refused = call_tool(
        &router,
        &reader,
        5,
        "oml_read",
        json!({"own": false, "workarea": Uuid::from_u128(0xBEEF)}),
    )
    .await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["code"], "access.not_found",
        "{refused}"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn a_tool_refusal_is_the_ring_refusal_as_an_error_result() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, reader, _) = crew(&router).await;

    // A learn naming an ancestor the key cannot read: the ring's typed
    // refusal, absence, not a transport error.
    let learned = call_tool(
        &router,
        &writer,
        1,
        "oml_write",
        json!({"claim_key": "k", "text": "t", "derives_from": [Uuid::from_u128(0xBEEF)],
               "idempotency_key": "l-1"}),
    )
    .await;
    assert_eq!(learned["isError"], true);
    assert_eq!(
        learned["structuredContent"]["error"]["code"],
        "access.not_found"
    );

    // A reader cannot remember (R1 on the ring).
    let denied = call_tool(
        &router,
        &reader,
        2,
        "oml_write",
        json!({"claim_key": "k", "text": "t", "idempotency_key": "r-1"}),
    )
    .await;
    assert_eq!(denied["isError"], true);
    assert_eq!(
        denied["structuredContent"]["error"]["code"],
        "access.verb_ceiling"
    );

    // Arguments outside the input schema are a validation refusal.
    let malformed = call_tool(
        &router,
        &writer,
        3,
        "oml_write",
        json!({"claim_key": "k", "text": "t", "idempotency_key": "m-1", "tenant": "other"}),
    )
    .await;
    assert_eq!(malformed["isError"], true);
    assert_eq!(
        malformed["structuredContent"]["error"]["code"],
        "request.validation"
    );

    // A designed tool is named as designed, not unknown.
    let designed = rpc(
        &router,
        &writer,
        4,
        "tools/call",
        json!({"name": "oml_vocab", "arguments": {}}),
    )
    .await;
    assert_eq!(designed["error"]["code"], -32602);
    assert!(
        designed["error"]["message"]
            .as_str()
            .expect("message")
            .contains("is designed")
    );

    drop_db(&db).await;
}

/// One `tools/call` to `/mcp`; the JSON-RPC message and the response's
/// `X-Request-Id`.
async fn call_tool_traced(
    router: &Router,
    token: &str,
    id: u64,
    name: &str,
    args: Value,
) -> (Value, String) {
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                   "params": {"name": name, "arguments": args}})
            .to_string(),
        ))
        .expect("request builds");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("infallible router");
    assert_eq!(response.status(), StatusCode::OK);
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .expect("every response carries X-Request-Id")
        .to_owned();
    let bytes = to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("body collects");
    let body: Value = serde_json::from_slice(&bytes).expect("responses are JSON");
    (body, request_id)
}

#[tokio::test]
async fn a_failing_tool_answers_is_error_with_the_envelope_in_structured_content_and_text() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, _, _) = crew(&router).await;

    // A refusal the ring answers (a learn naming an unknown ancestor) and one the facade
    // answers before any ring call (an argument outside the input schema)
    // are the one errors.md envelope, request_id included (ADR-0039 d11).
    for (id, args, code) in [
        (
            1,
            json!({"claim_key": "k", "text": "t", "derives_from": [Uuid::from_u128(0xBEEF)],
                   "idempotency_key": "e-1"}),
            "access.not_found",
        ),
        (
            2,
            json!({"claim_key": "k", "text": "t", "idempotency_key": "e-2", "tenant": "other"}),
            "request.validation",
        ),
    ] {
        let (body, request_id) = call_tool_traced(&router, &writer, id, "oml_write", args).await;
        // A tool's refusal is a result, never a JSON-RPC error.
        assert!(body.get("error").is_none(), "{body}");
        let result = &body["result"];
        assert_eq!(result["isError"], true, "{body}");
        let envelope = &result["structuredContent"];
        let keys: Vec<&str> = envelope
            .as_object()
            .expect("the envelope is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["error", "request_id"], "{body}");
        assert_eq!(envelope["request_id"], request_id.as_str(), "{body}");
        assert_eq!(envelope["error"]["code"], code, "{body}");
        for field in ["message", "rule"] {
            assert!(envelope["error"][field].is_string(), "{body}");
        }
        assert!(envelope["error"]["retryable"].is_boolean(), "{body}");
        let content = result["content"].as_array().expect("content");
        assert_eq!(content.len(), 1, "{body}");
        assert_eq!(content[0]["type"], "text", "{body}");
        let text: Value = serde_json::from_str(content[0]["text"].as_str().expect("text"))
            .expect("the text content is the envelope as JSON");
        assert_eq!(&text, envelope, "{body}");
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn a_tool_that_is_not_served_answers_json_rpc_32602() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, _, _) = crew(&router).await;

    // An unknown name and a catalog tool not built yet are protocol faults:
    // a JSON-RPC error, no tool result (ADR-0039 d11).
    for (id, name) in (1..).zip(["oml_promote", "oml_graph", "not_a_tool"]) {
        let (body, _) = call_tool_traced(&router, &writer, id, name, json!({})).await;
        assert!(body.get("result").is_none(), "{body}");
        assert_eq!(body["error"]["code"], -32602, "{body}");
        assert!(body["error"]["message"].is_string(), "{body}");
    }

    drop_db(&db).await;
}

/// The eight tool names v0.3.0 served, which v0.5.1 removed (ADR-0039 d12).
const V0_3_0_TOOL_NAMES: [&str; 8] = [
    "oml_remember",
    "oml_learn",
    "oml_recall",
    "oml_get",
    "oml_supersede",
    "oml_my_work",
    "oml_workarea_info",
    "oml_events_tail",
];

#[tokio::test]
async fn the_v0_3_0_tool_names_answer_unknown_tool() {
    // Removed in v0.5.1, each old name is a name the server does not serve:
    // a JSON-RPC -32602 with the unknown-tool message and no tool result,
    // whatever arguments it carries (ADR-0039 d11, d12).
    let router = doubles_router();
    let (agent, _, _, _) = agent_crew(&router).await;
    for (id, name) in (1..).zip(V0_3_0_TOOL_NAMES) {
        let body = rpc(
            &router,
            &agent,
            id,
            "tools/call",
            json!({"name": name, "arguments": {}}),
        )
        .await;
        assert!(body.get("result").is_none(), "{name}: {body}");
        assert_eq!(body["error"]["code"], -32602, "{name}: {body}");
        let message = body["error"]["message"].as_str().expect("message");
        assert!(
            message.starts_with(&format!("Unknown tool: {name}.")),
            "{name}: {body}"
        );
    }
}

#[tokio::test]
async fn tools_list_names_three_tools_each_with_its_annotations() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, _, _) = crew(&router).await;

    // Exactly the three task-shaped tools, in catalog order, each with
    // ADR-0039 d5's hints: the reads read-only, the write neither read-only
    // nor destructive, all three idempotent and closed-world (d2, d5).
    let listed = rpc(&router, &writer, 1, "tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().expect("tools");
    let named: Vec<(&str, &Value)> = tools
        .iter()
        .map(|t| (t["name"].as_str().expect("name"), &t["annotations"]))
        .collect();
    let read_only = json!({"readOnlyHint": true, "idempotentHint": true, "openWorldHint": false});
    assert_eq!(
        named,
        [
            (
                "oml_write",
                &json!({
                    "readOnlyHint": false,
                    "destructiveHint": false,
                    "idempotentHint": true,
                    "openWorldHint": false,
                })
            ),
            ("oml_read", &read_only),
            ("oml_workarea", &read_only),
        ],
        "{listed}"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn tools_list_names_no_designed_tool() {
    let designed: Value =
        serde_json::from_str(include_str!("../../../../docs/api/mcp-tools.designed.json"))
            .expect("json");
    let designed_tools: Vec<&str> = designed["tools"]
        .as_array()
        .expect("designed tools")
        .iter()
        .map(|t| t["name"].as_str().expect("name"))
        .collect();
    assert_eq!(designed_tools, ["oml_relate_propose", "oml_vocab"]);
    let designed_arguments = designed["designed_arguments"]
        .as_object()
        .expect("designed arguments");

    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, _, _) = crew(&router).await;

    // No designed tool, no designed argument, and none of the v0.3.0 names
    // or the two tools with no successor tool is listed (ADR-0039 d2, d10).
    let listed = rpc(&router, &writer, 1, "tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().expect("tools");
    for tool in tools {
        let name = tool["name"].as_str().expect("name");
        assert!(!designed_tools.contains(&name), "{name} is designed");
        let properties = tool["inputSchema"]["properties"]
            .as_object()
            .expect("properties");
        for argument in designed_arguments
            .get(name)
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(serde_json::Map::keys)
        {
            assert!(
                !properties.contains_key(argument),
                "{name} lists the designed argument {argument}"
            );
        }
    }
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for gone in [
        "oml_remember",
        "oml_learn",
        "oml_recall",
        "oml_get",
        "oml_supersede",
        "oml_my_work",
        "oml_workarea_info",
        "oml_events_tail",
        "oml_attach",
        "oml_graph",
    ] {
        assert!(!names.contains(&gone), "{gone} is listed: {names:?}");
    }

    // A designed tool is a protocol fault that names where it is designed,
    // never an unknown tool under the verb ceiling (ADR-0039 d11).
    for (id, name) in (2..).zip(designed_tools) {
        let body = rpc(
            &router,
            &writer,
            id,
            "tools/call",
            json!({"name": name, "arguments": {}}),
        )
        .await;
        assert_eq!(body["error"]["code"], -32602, "{body}");
        let message = body["error"]["message"].as_str().expect("message");
        assert!(message.contains("mcp-tools.designed.json"), "{body}");
        assert!(!message.contains("(R1)"), "{body}");
    }

    drop_db(&db).await;
}

/// The claim the supersession tests propose versions of.
const CLAIM: &str = "supplier-quote-am-7741";
const REVISED: &str =
    "Accra Metals revised its aluminium price to 408 USD/t for Q1-2027 delivery, quote AM-7741-r2.";

/// The writer remembers [`QUOTE`] as version 1 of [`CLAIM`] and the dreamer
/// ratifies it; its `artifact_id`.
async fn ratified(router: &Router, writer: &str, dreamer: &str) -> String {
    remember_all(router, writer, &[(CLAIM, "memory", QUOTE)]).await;
    approve_as(router, dreamer, "v1").await;
    let work = call_tool(router, writer, 90, "oml_read", json!({"claim_key": CLAIM})).await;
    let items = work["structuredContent"]["items"]
        .as_array()
        .expect("items");
    assert_eq!(items.len(), 1, "{work}");
    items[0]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned()
}

/// The artifact ids of an `oml_read` recall result's hits, in rank order.
fn hit_ids(result: &Value) -> Vec<&str> {
    result["structuredContent"]["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .filter_map(|hit| hit["artifact_id"].as_str())
        .collect()
}

#[tokio::test]
async fn oml_read_get_returns_the_authors_held_row_to_its_author_and_not_found_to_everyone_else() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    // The writer holds no reader grant: its own rows need none
    // (roles-and-keys.md#own; ADR-0025 d3).
    let (writer, dreamer, reader, crew) = crew(&router).await;
    remember_all(&router, &writer, &[(CLAIM, "memory", QUOTE)]).await;
    let work = call_tool(&router, &writer, 1, "oml_read", json!({})).await;
    let id = work["structuredContent"]["items"][0]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned();

    // The author reads its held row back: the claim item with text,
    // derives_from and attachments, get's one item (mcp-plugin.md
    // `oml_read`; ADR-0025 d12, ADR-0039 d4).
    let got = call_tool(&router, &writer, 2, "oml_read", json!({"artifact_id": id})).await;
    assert_eq!(got["isError"], false, "{got}");
    assert_eq!(got["structuredContent"]["mode"], "get", "{got}");
    let items = got["structuredContent"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{got}");
    let item = &items[0];
    assert_eq!(
        fields(item),
        vec![
            "anchor",
            "artifact_id",
            "attachments",
            "claim_key",
            "created_at",
            "derives_from",
            "kind",
            "promoted_at",
            "replaces",
            "retirement",
            "source_checked",
            "stale",
            "state",
            "text",
            "tip",
            "tokens",
            "version",
            "workarea",
            "workarea_path",
        ]
    );
    assert_eq!(item["state"], "held");
    assert_eq!(item["tip"], false);
    assert_eq!(item["text"], QUOTE);
    assert_eq!(item["tokens"], tokens(QUOTE));
    assert_eq!(item["workarea"], crew.to_string());
    assert_eq!(item["derives_from"], json!([]));
    assert_eq!(item["attachments"], json!([]));
    // The item is what the ring's get answers, `workarea_id` named
    // `workarea`.
    let (status, ring) = send(
        &router,
        "GET",
        &format!("/v1/memories/{id}"),
        Some(&writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ring}");
    for name in fields(item) {
        let from = match name {
            "workarea" => "workarea_id",
            other => other,
        };
        assert_eq!(item[name], ring[from], "{name}");
    }

    // Everyone else gets absence: a held row of another key, an id naming
    // no row and an id that can name none are indistinguishable.
    let unknown = OsIds.new_id().to_string();
    for (n, token, artifact_id) in [
        (3, &reader, id.as_str()),
        (4, &dreamer, id.as_str()),
        (5, &writer, unknown.as_str()),
        (6, &writer, "art_01JB2YN4P6R8TA1CW3EY5G7J9L"),
    ] {
        let refused = call_tool(
            &router,
            token,
            n,
            "oml_read",
            json!({"artifact_id": artifact_id}),
        )
        .await;
        assert_eq!(refused["isError"], true, "{refused}");
        assert_eq!(
            refused["structuredContent"]["error"]["code"], "access.not_found",
            "{refused}"
        );
    }

    // Ratified, the row is any reader's to get, promoted and the tip.
    approve_as(&router, &dreamer, "v1").await;
    let got = call_tool(&router, &reader, 7, "oml_read", json!({"artifact_id": id})).await;
    assert_eq!(got["isError"], false, "{got}");
    let item = &got["structuredContent"]["items"][0];
    assert_eq!(item["state"], "promoted");
    assert_eq!(item["tip"], true);
    assert_eq!(item["text"], QUOTE);

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_write_with_replaces_enters_admission_held_and_leaves_the_tip_until_ratified() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, crew) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;

    let proposed = call_tool(
        &router,
        &writer,
        1,
        "oml_write",
        json!({
            "expected_version": 1,
            "replaces": v1,
            "text": REVISED,
            "reason": "supplier issued a revised quote",
            "idempotency_key": "crew-procure/run-2/step-1",
        }),
    )
    .await;
    assert_eq!(proposed["isError"], false, "{proposed}");
    let receipt = &proposed["structuredContent"];
    let v2 = receipt["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned();
    assert_eq!(
        receipt,
        &json!({
            "mode": "supersede",
            "artifact_id": v2,
            "claim_key": CLAIM,
            "version": 2,
            "state": "held",
            "retirement": false,
            "replaces": {"artifact_id": v1, "version": 1},
            "workarea": crew.to_string(),
            "similar": [],
            "idempotent_replay": false,
        })
    );

    // Held: every reader's recall still serves the target as the tip, and
    // the successor is its author's alone to read back.
    let recall = json!({"query": "aluminium price"});
    let before = call_tool(&router, &reader, 2, "oml_read", recall.clone()).await;
    assert_eq!(hit_ids(&before), [v1.as_str()], "{before}");
    let target = call_tool(&router, &reader, 3, "oml_read", json!({"artifact_id": v1})).await;
    assert_eq!(
        target["structuredContent"]["items"][0]["tip"], true,
        "{target}"
    );
    let own = call_tool(&router, &writer, 4, "oml_read", json!({"artifact_id": v2})).await;
    let item = &own["structuredContent"]["items"][0];
    assert_eq!(item["state"], "held", "{own}");
    assert_eq!(item["tip"], false, "{own}");
    assert_eq!(item["text"], REVISED, "{own}");
    assert_eq!(item["replaces"], json!({"artifact_id": v1, "version": 1}));
    assert_eq!(item["derives_from"], json!([v1]));
    let hidden = call_tool(&router, &reader, 5, "oml_read", json!({"artifact_id": v2})).await;
    assert_eq!(
        hidden["structuredContent"]["error"]["code"], "access.not_found",
        "{hidden}"
    );

    // A non-author ratifies: the tip moves, and the target stays readable
    // as history.
    approve_as(&router, &dreamer, "v2").await;
    let after = call_tool(&router, &reader, 6, "oml_read", recall).await;
    assert_eq!(hit_ids(&after), [v2.as_str()], "{after}");
    let old = call_tool(&router, &reader, 7, "oml_read", json!({"artifact_id": v1})).await;
    assert_eq!(
        old["structuredContent"]["items"][0]["state"], "promoted",
        "{old}"
    );
    assert_eq!(old["structuredContent"]["items"][0]["tip"], false, "{old}");
    let new = call_tool(&router, &reader, 8, "oml_read", json!({"artifact_id": v2})).await;
    assert_eq!(
        new["structuredContent"]["items"][0]["state"], "promoted",
        "{new}"
    );
    assert_eq!(new["structuredContent"]["items"][0]["tip"], true, "{new}");

    // The target is no longer the tip: the ring's refusal names the one to
    // propose against.
    let stale = call_tool(
        &router,
        &writer,
        9,
        "oml_write",
        json!({ "expected_version": 2,"replaces": v1, "text": REVISED, "idempotency_key": "crew-procure/run-3/step-1"}),
    )
    .await;
    assert_eq!(stale["isError"], true, "{stale}");
    let error = &stale["structuredContent"]["error"];
    assert_eq!(error["code"], "submission.tip_conflict", "{stale}");
    assert_eq!(
        error["details"]["current_tip"],
        json!({"artifact_id": v2, "version": 2})
    );

    // A reader key proposes nothing (R1 on the ring).
    let denied = call_tool(
        &router,
        &reader,
        10,
        "oml_write",
        json!({ "expected_version": 1,"replaces": v2, "text": REVISED, "idempotency_key": "reader-1"}),
    )
    .await;
    assert_eq!(
        denied["structuredContent"]["error"]["code"], "access.verb_ceiling",
        "{denied}"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_write_with_retirement_retires_the_claim_once_ratified_and_get_answers_its_history() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, _) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;
    let why = "Retired: quote AM-7741 lapsed on 2026-12-01 and was not renewed.";

    let proposed = call_tool(
        &router,
        &writer,
        1,
        "oml_write",
        json!({
            "expected_version": 1,
            "replaces": v1,
            "text": why,
            "retirement": true,
            "idempotency_key": "crew-procure/run-2/step-1",
        }),
    )
    .await;
    assert_eq!(proposed["isError"], false, "{proposed}");
    let receipt = &proposed["structuredContent"];
    assert_eq!(receipt["retirement"], true, "{receipt}");
    assert_eq!(receipt["state"], "held", "{receipt}");
    assert_eq!(receipt["version"], 2, "{receipt}");
    assert_eq!(
        receipt["replaces"],
        json!({"artifact_id": v1, "version": 1})
    );
    let v2 = receipt["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned();

    // Proposed, not in force: the claim is served until a non-author
    // ratifies the retirement.
    let recall = json!({"query": "aluminium price"});
    let before = call_tool(&router, &reader, 2, "oml_read", recall.clone()).await;
    assert_eq!(hit_ids(&before), [v1.as_str()], "{before}");

    // Ratified, the claim is retired: recall serves nothing of it, and get
    // answers every version as history (audit-and-lineage.md; ADR-0025 d11).
    approve_as(&router, &dreamer, "v2").await;
    let after = call_tool(&router, &reader, 3, "oml_read", recall).await;
    assert_eq!(after["structuredContent"]["hits"], json!([]), "{after}");
    assert_eq!(after["structuredContent"]["steering"], json!([]), "{after}");
    let old = call_tool(&router, &reader, 4, "oml_read", json!({"artifact_id": v1})).await;
    assert_eq!(
        old["structuredContent"]["items"][0]["state"], "promoted",
        "{old}"
    );
    assert_eq!(old["structuredContent"]["items"][0]["tip"], false, "{old}");
    let retirement = call_tool(&router, &reader, 5, "oml_read", json!({"artifact_id": v2})).await;
    let item = &retirement["structuredContent"]["items"][0];
    assert_eq!(item["state"], "promoted", "{retirement}");
    assert_eq!(item["tip"], true, "{retirement}");
    assert_eq!(item["retirement"], true, "{retirement}");
    assert_eq!(item["text"], why, "{retirement}");

    drop_db(&db).await;
}

#[tokio::test]
async fn a_replayed_oml_write_supersession_is_an_idempotent_replay() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, _, _) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;
    let args = json!({
        "expected_version": 1,
        "replaces": v1,
        "text": REVISED,
        "idempotency_key": "crew-procure/run-2/step-1",
    });

    let first = call_tool(&router, &writer, 1, "oml_write", args.clone()).await;
    assert_eq!(first["isError"], false, "{first}");
    assert_eq!(first["structuredContent"]["idempotent_replay"], false);

    // The same key and request: the stored response, flagged, and no
    // second write — even once the target is no longer the tip (D13).
    let mut expected = first["structuredContent"].clone();
    expected["idempotent_replay"] = json!(true);
    let replay = call_tool(&router, &writer, 2, "oml_write", args.clone()).await;
    assert_eq!(replay["isError"], false, "{replay}");
    assert_eq!(replay["structuredContent"], expected);
    approve_as(&router, &dreamer, "v2").await;
    let late = call_tool(&router, &writer, 3, "oml_write", args.clone()).await;
    assert_eq!(late["isError"], false, "{late}");
    assert_eq!(late["structuredContent"], expected);
    let work = call_tool(&router, &writer, 4, "oml_read", json!({"claim_key": CLAIM})).await;
    assert_eq!(
        work["structuredContent"]["items"]
            .as_array()
            .expect("items")
            .len(),
        2,
        "{work}"
    );

    // The same key with a different request is a typed refusal.
    let mut changed = args;
    changed["text"] = json!("Accra Metals revised its aluminium price to 405 USD/t.");
    let mismatch = call_tool(&router, &writer, 5, "oml_write", changed).await;
    assert_eq!(mismatch["isError"], true, "{mismatch}");
    assert_eq!(
        mismatch["structuredContent"]["error"]["code"], "idempotency.digest_mismatch",
        "{mismatch}"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_write_refuses_a_vocabulary_kind_target_that_the_http_api_supersedes() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, _, _) = crew(&router).await;
    remember_all(
        &router,
        &writer,
        &[(
            CLAIM,
            "term",
            "aluminium: a light silvery-white metal, symbol Al.",
        )],
    )
    .await;
    let work = call_tool(&router, &writer, 1, "oml_read", json!({})).await;
    let term = work["structuredContent"]["items"][0]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned();
    let propose = |id: u64, idem: &str| {
        let (router, writer, term) = (&router, &writer, &term);
        let args = json!({ "expected_version": 1,"replaces": term, "text": "aluminium: element 13.", "idempotency_key": idem});
        async move { call_tool(router, writer, id, "oml_write", args).await }
    };

    // Held, the term is no tip: the tip check answers before the
    // vocabulary-kind check (ADR-0025 d10 step 5).
    let held = propose(2, "term-1").await;
    assert_eq!(
        held["structuredContent"]["error"]["code"], "submission.tip_conflict",
        "{held}"
    );

    // Ratified, it is refused on this tool, which carries no vocabulary
    // fields (ADR-0025 d12) ...
    approve_as(&router, &dreamer, "term").await;
    let refused = propose(3, "term-2").await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["code"], "request.validation",
        "{refused}"
    );
    assert!(
        refused["structuredContent"]["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("HTTP API")),
        "{refused}"
    );

    // ... and superseded on the HTTP API, held like any successor.
    let (status, accepted) = send(
        &router,
        "POST",
        &format!("/v1/memories/{term}/supersede"),
        Some(&writer),
        Some("term-3"),
        Some(&json!({ "expected_version": 1,"body": "aluminium: element 13."})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    assert_eq!(accepted["state"], "held");
    assert_eq!(accepted["replaces"]["artifact_id"], term.as_str());

    drop_db(&db).await;
}

/// The `oml_write` refusal `result` is: `request.validation` naming
/// `field` in `details.field` (ADR-0039 d3, ADR-0025 d16).
fn refused_naming(result: &Value, field: &str) {
    assert_eq!(result["isError"], true, "{result}");
    let error = &result["structuredContent"]["error"];
    assert_eq!(error["code"], "request.validation", "{result}");
    assert_eq!(error["details"]["field"], field, "{result}");
}

/// The capture event types the key's events tail journals for
/// `artifact_id`.
async fn capture_events(router: &Router, token: &str, artifact_id: &str) -> Vec<String> {
    let (status, events) = send(
        router,
        "GET",
        "/v2/events?after_seq=0&limit=200",
        Some(token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{events}");
    events["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|e| e["payload"]["artifact_id"] == artifact_id)
        .filter_map(|e| e["event_type"].as_str())
        .filter(|t| t.starts_with("submission."))
        .map(str::to_owned)
        .collect()
}

/// The number of claim versions the key's own-work listing holds.
async fn own_versions(router: &Router, token: &str) -> usize {
    let work = call_tool(router, token, 99, "oml_read", json!({})).await;
    work["structuredContent"]["items"]
        .as_array()
        .expect("items")
        .len()
}

#[tokio::test]
async fn oml_write_without_replaces_submits_and_journals_learn_when_derives_from_is_set() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, _, _, crew) = agent_crew(&router).await;

    // No claim_key and no kind: the server derives the key and the kind is
    // memory (ADR-0039 d3, d6).
    let first = call_tool(
        &router,
        &agent,
        1,
        "oml_write",
        json!({"text": QUOTE, "idempotency_key": "crew-procure/run-1/step-1"}),
    )
    .await;
    assert_eq!(first["isError"], false, "{first}");
    let receipt = &first["structuredContent"];
    let ancestor = receipt["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned();
    let claim_key = receipt["claim_key"].as_str().expect("claim key").to_owned();
    assert!(claim_key.starts_with("auto-"), "{receipt}");
    assert_eq!(
        receipt,
        &json!({
            "mode": "submit",
            "artifact_id": ancestor,
            "claim_key": claim_key,
            "version": 1,
            "state": "held",
            "workarea": crew.to_string(),
            "retirement": false,
            "replaces": null,
            "similar": [],
            "idempotent_replay": false,
        })
    );
    assert_eq!(
        capture_events(&router, &agent, &ancestor).await,
        ["submission.remember"]
    );
    let got = call_tool(
        &router,
        &agent,
        2,
        "oml_read",
        json!({"artifact_id": ancestor}),
    )
    .await;
    assert_eq!(
        got["structuredContent"]["items"][0]["kind"], "memory",
        "{got}"
    );

    // derives_from non-empty: the same endpoint journals learn.
    let derived = call_tool(
        &router,
        &agent,
        3,
        "oml_write",
        json!({
            "text": ALLOY,
            "claim_key": "supplier-quote-ta-2207",
            "derives_from": [ancestor],
            "workarea": "metals-crew",
            "idempotency_key": "crew-procure/run-1/step-2",
        }),
    )
    .await;
    assert_eq!(derived["isError"], false, "{derived}");
    let receipt = &derived["structuredContent"];
    assert_eq!(receipt["mode"], "submit", "{receipt}");
    assert_eq!(receipt["claim_key"], "supplier-quote-ta-2207", "{receipt}");
    assert_eq!(receipt["workarea"], crew.to_string(), "{receipt}");
    let artifact = receipt["artifact_id"].as_str().expect("artifact id");
    assert_eq!(
        capture_events(&router, &agent, artifact).await,
        ["submission.learn"]
    );
    let got = call_tool(
        &router,
        &agent,
        4,
        "oml_read",
        json!({"artifact_id": artifact}),
    )
    .await;
    assert_eq!(
        got["structuredContent"]["items"][0]["derives_from"],
        json!([ancestor])
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_write_with_replaces_supersedes_that_version() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, crew) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;

    // The target's own workarea, by name, is accepted.
    let proposed = call_tool(
        &router,
        &writer,
        1,
        "oml_write",
        json!({
            "expected_version": 1,
            "replaces": v1,
            "text": REVISED,
            "reason": "supplier issued a revised quote",
            "workarea": "metals-crew",
            "idempotency_key": "crew-procure/run-2/step-1",
        }),
    )
    .await;
    assert_eq!(proposed["isError"], false, "{proposed}");
    let receipt = &proposed["structuredContent"];
    let v2 = receipt["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned();
    assert_eq!(
        receipt,
        &json!({
            "mode": "supersede",
            "artifact_id": v2,
            "claim_key": CLAIM,
            "version": 2,
            "state": "held",
            "workarea": crew.to_string(),
            "retirement": false,
            "replaces": {"artifact_id": v1, "version": 1},
            "similar": [],
            "idempotent_replay": false,
        })
    );
    let own = call_tool(&router, &writer, 2, "oml_read", json!({"artifact_id": v2})).await;
    let item = &own["structuredContent"]["items"][0];
    assert_eq!(item["text"], REVISED, "{own}");
    assert_eq!(item["derives_from"], json!([v1]));

    // Held, the target stays the tip until a non-author ratifies.
    let recall = json!({"query": "aluminium price"});
    let before = call_tool(&router, &reader, 3, "oml_read", recall.clone()).await;
    assert_eq!(hit_ids(&before), [v1.as_str()], "{before}");
    approve_as(&router, &dreamer, "v2").await;
    let after = call_tool(&router, &reader, 4, "oml_read", recall).await;
    assert_eq!(hit_ids(&after), [v2.as_str()], "{after}");

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_write_with_retirement_true_proposes_retiring_the_target() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, _) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;
    let why = "Retired: quote AM-7741 lapsed on 2026-12-01 and was not renewed.";

    let proposed = call_tool(
        &router,
        &writer,
        1,
        "oml_write",
        json!({
            "expected_version": 1,
            "replaces": v1,
            "text": why,
            "retirement": true,
            "idempotency_key": "crew-procure/run-2/step-1",
        }),
    )
    .await;
    assert_eq!(proposed["isError"], false, "{proposed}");
    let receipt = &proposed["structuredContent"];
    assert_eq!(receipt["mode"], "supersede", "{receipt}");
    assert_eq!(receipt["retirement"], true, "{receipt}");
    assert_eq!(receipt["state"], "held", "{receipt}");
    assert_eq!(
        receipt["replaces"],
        json!({"artifact_id": v1, "version": 1})
    );

    // A proposal until a non-author ratifies; then recall serves none of
    // the claim (ADR-0025 d11).
    let recall = json!({"query": "aluminium price"});
    let before = call_tool(&router, &reader, 2, "oml_read", recall.clone()).await;
    assert_eq!(hit_ids(&before), [v1.as_str()], "{before}");
    approve_as(&router, &dreamer, "v2").await;
    let after = call_tool(&router, &reader, 3, "oml_read", recall).await;
    assert_eq!(after["structuredContent"]["hits"], json!([]), "{after}");

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_write_refuses_retirement_without_replaces_naming_retirement() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, _, _, _) = agent_crew(&router).await;

    // retirement and reason belong to a supersession; on a submission each
    // is refused by name, false included, never dropped.
    for (id, (field, value)) in (1..).zip([
        ("retirement", json!(true)),
        ("retirement", json!(false)),
        ("reason", json!("supplier issued a revised quote")),
    ]) {
        let mut args = json!({"text": QUOTE, "idempotency_key": format!("refused-{id}")});
        args[field] = value;
        let refused = call_tool(&router, &agent, id, "oml_write", args).await;
        refused_naming(&refused, &format!("/{field}"));
    }
    assert_eq!(own_versions(&router, &agent).await, 0);

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_write_refuses_claim_key_with_replaces_naming_claim_key() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, _, _) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;

    // The successor takes its target's claim_key, so a sent one is refused,
    // even the target's own.
    for (id, claim_key) in (1..).zip([CLAIM, "supplier-quote-am-7742"]) {
        let refused = call_tool(
            &router,
            &writer,
            id,
            "oml_write",
            json!({
                "expected_version": 1,
                "replaces": v1,
                "claim_key": claim_key,
                "text": REVISED,
                "idempotency_key": format!("refused-{id}"),
            }),
        )
        .await;
        refused_naming(&refused, "/claim_key");
    }
    assert_eq!(own_versions(&router, &writer).await, 1);

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_write_refuses_a_workarea_other_than_the_targets_naming_workarea() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, _, _) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;

    let refused = call_tool(
        &router,
        &writer,
        1,
        "oml_write",
        json!({
            "expected_version": 1,
            "replaces": v1,
            "workarea": Uuid::from_u128(0xBEEF),
            "text": REVISED,
            "idempotency_key": "refused-1",
        }),
    )
    .await;
    refused_naming(&refused, "/workarea");
    assert_eq!(own_versions(&router, &writer).await, 1);

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_write_refuses_a_vocabulary_kind_target() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, _, _) = crew(&router).await;
    remember_all(
        &router,
        &writer,
        &[(
            CLAIM,
            "term",
            "aluminium: a light silvery-white metal, symbol Al.",
        )],
    )
    .await;
    approve_as(&router, &dreamer, "term").await;
    let work = call_tool(&router, &writer, 1, "oml_read", json!({})).await;
    let term = work["structuredContent"]["items"][0]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned();

    // A ratified term is superseded on the HTTP API, never on this tool,
    // which carries no vocabulary fields (ADR-0021; ADR-0039 d3).
    let refused = call_tool(
        &router,
        &writer,
        2,
        "oml_write",
        json!({ "expected_version": 1,"replaces": term, "text": "aluminium: element 13.", "idempotency_key": "term-1"}),
    )
    .await;
    refused_naming(&refused, "/replaces");
    assert!(
        refused["structuredContent"]["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("HTTP API")),
        "{refused}"
    );

    // Nor does a supersession on this tool take a vocabulary kind: the
    // kind is refused before the target is read.
    let refused = call_tool(
        &router,
        &writer,
        3,
        "oml_write",
        json!({
            "expected_version": 1,
            "replaces": term,
            "kind": "term",
            "text": "aluminium: element 13.",
            "idempotency_key": "term-2",
        }),
    )
    .await;
    refused_naming(&refused, "/kind");
    assert_eq!(own_versions(&router, &writer).await, 1);

    drop_db(&db).await;
}

/// The error code of a tool result that must be a refusal.
fn refusal_code(result: &Value) -> &str {
    assert_eq!(result["isError"], true, "{result}");
    result["structuredContent"]["error"]["code"]
        .as_str()
        .expect("error code")
}

#[tokio::test]
async fn oml_write_by_a_reader_key_is_refused_at_the_verb_ceiling_in_every_mode() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, _) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;

    // A reader key's role admits no writer grant, so each of oml_write's
    // modes stops at step 1 of the refusal order, before the target or the
    // workarea is read (R1; roles-and-keys.md, refusal order; ADR-0039 d2).
    for (id, (mode, args)) in (1..).zip([
        (
            "submit",
            json!({"text": QUOTE, "idempotency_key": "reader/submit"}),
        ),
        (
            "learn",
            json!({"text": ALLOY, "derives_from": [v1], "idempotency_key": "reader/learn"}),
        ),
        (
            "supersede",
            json!({ "expected_version": 1,"replaces": v1, "text": REVISED, "reason": "revised", "idempotency_key": "reader/supersede"}),
        ),
        (
            "retire",
            json!({ "expected_version": 1,"replaces": v1, "text": "Retired: quote AM-7741 lapsed.", "retirement": true, "idempotency_key": "reader/retire"}),
        ),
    ]) {
        let refused = call_tool(&router, &reader, id, "oml_write", args).await;
        assert_eq!(refusal_code(&refused), "access.verb_ceiling", "{mode}");
    }
    assert_eq!(own_versions(&router, &reader).await, 0);

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_recall_and_scoped_listing_are_refused_without_a_reader_grant_and_to_an_admin_key()
{
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    // The writer here holds a writer grant alone.
    let ((writer, dreamer, _, _), admin) = crew_and_admin(&router, &["writer"]).await;
    ratified(&router, &writer, &dreamer).await;

    let recall = json!({"query": "aluminium price"});
    let scoped = json!({"own": false});
    for (id, (mode, args)) in (1..).zip([("recall", &recall), ("listing", &scoped)]) {
        // Reader is not effective on the writer's default workarea, so the
        // read answers step 4 of the refusal order: the node is not found,
        // never named as refused (roles-and-keys.md, refusal order).
        let refused = call_tool(&router, &writer, id, "oml_read", args.clone()).await;
        assert_eq!(refusal_code(&refused), "access.not_found", "{mode}");
        // An admin key reads no claim content, whatever its grants: step 1
        // (errors.md, access.verb_ceiling; ADR-0026 d15).
        let refused = call_tool(&router, &admin, 10 + id, "oml_read", args.clone()).await;
        assert_eq!(refusal_code(&refused), "access.verb_ceiling", "{mode}");
    }
    // The writer's own listing is not a scoped read and still answers.
    let own = call_tool(&router, &writer, 20, "oml_read", json!({})).await;
    assert_eq!(own["isError"], false, "{own}");

    drop_db(&db).await;
}

/// The v0.3.0 writes' simulation fields stay designed arguments of their
/// successor on MCP, as `x-oml-designed-fields` keeps them on HTTP
/// (ADR-0039 d9, d10).
#[test]
fn the_v0_3_0_simulation_fields_are_designed_arguments_of_oml_write() {
    let designed: Value =
        serde_json::from_str(include_str!("../../../../docs/api/mcp-tools.designed.json"))
            .expect("json");
    let served: Value =
        serde_json::from_str(include_str!("../../../../docs/api/mcp-tools.json")).expect("json");
    let write = served["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|t| t["name"] == "oml_write")
        .expect("oml_write is served");
    let arguments = &designed["designed_arguments"]["oml_write"];
    for (field, says) in [
        ("simulation_id", "submission.simulation_id_required"),
        ("simulated_time", "never backdating"),
    ] {
        let description = arguments[field]["description"]
            .as_str()
            .unwrap_or_else(|| panic!("{field} is a designed argument of oml_write"));
        assert!(description.contains(says), "{field}: {description}");
        assert_eq!(arguments[field]["type"], "string", "{field}");
        assert!(
            write["inputSchema"]["properties"].get(field).is_none(),
            "{field} is served"
        );
    }
}

#[tokio::test]
async fn a_designed_argument_sent_to_oml_write_is_refused_as_outside_its_input_schema() {
    let designed: Value =
        serde_json::from_str(include_str!("../../../../docs/api/mcp-tools.designed.json"))
            .expect("json");
    let arguments = designed["designed_arguments"]["oml_write"]
        .as_object()
        .expect("oml_write's designed arguments")
        .clone();
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, _, _, _) = agent_crew(&router).await;

    // Each is refused before the ring is reached, and nothing is stored
    // (ADR-0039 d2, d9; mcp-tools.designed.json, _meta).
    for (id, (field, schema)) in (1..).zip(&arguments) {
        let mut args = json!({"text": QUOTE, "idempotency_key": format!("designed/{field}")});
        args[field.as_str()] = type_valid(schema, field);
        let refused = call_tool(&router, &agent, id, "oml_write", args).await;
        assert!(outside_the_input_schema(&refused), "{field}: {refused}");
    }
    assert_eq!(own_versions(&router, &agent).await, 0);

    drop_db(&db).await;
}

#[tokio::test]
async fn a_repeated_idempotency_key_replays_with_idempotent_replay_true() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, _, _) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;

    // A submission and a supersession each replay their stored receipt,
    // flagged, and write nothing a second time (D13).
    for (id, args) in [
        (
            1,
            json!({"text": ALLOY, "idempotency_key": "crew-procure/run-3/step-1"}),
        ),
        (
            3,
            json!({ "expected_version": 1,"replaces": v1, "text": REVISED, "idempotency_key": "crew-procure/run-3/step-2"}),
        ),
    ] {
        let first = call_tool(&router, &writer, id, "oml_write", args.clone()).await;
        assert_eq!(first["isError"], false, "{first}");
        assert_eq!(first["structuredContent"]["idempotent_replay"], false);
        let mut expected = first["structuredContent"].clone();
        expected["idempotent_replay"] = json!(true);
        let replay = call_tool(&router, &writer, id + 1, "oml_write", args).await;
        assert_eq!(replay["isError"], false, "{replay}");
        assert_eq!(replay["structuredContent"], expected);
    }
    assert_eq!(own_versions(&router, &writer).await, 3);

    drop_db(&db).await;
}

/// `oml_write`'s idempotency key has the scope of the endpoint its mode
/// maps to: a submission's record is scoped to (key, resolved workarea), a
/// supersession's to the calling key alone, so one key reused across the
/// two modes is two writes, neither a replay nor
/// `idempotency.digest_mismatch`, and the catalog says both scopes
/// (ADR-0039 d3, ADR-0025 d10, D13).
#[tokio::test]
async fn a_reused_idempotency_key_across_modes_is_a_separate_write() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../../../docs/api/mcp-tools.json")).expect("json");
    let write = catalog["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|t| t["name"] == "oml_write")
        .expect("oml_write is declared");
    let said = write["inputSchema"]["properties"]["idempotency_key"]["description"]
        .as_str()
        .expect("description");
    assert!(
        said.contains("Without replaces it is scoped to (key, resolved workarea)")
            && said.contains("with replaces to the calling key alone")
            && said.contains("is a separate write"),
        "{said}"
    );

    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, _, _) = crew(&router).await;
    let v1 = ratified(&router, &writer, &dreamer).await;

    let key = "crew-procure/run-5/step-1";
    let submitted = call_tool(
        &router,
        &writer,
        1,
        "oml_write",
        json!({"text": ALLOY, "idempotency_key": key}),
    )
    .await;
    assert_eq!(submitted["isError"], false, "{submitted}");
    assert_eq!(submitted["structuredContent"]["idempotent_replay"], false);

    let superseded = call_tool(
        &router,
        &writer,
        2,
        "oml_write",
        json!({ "expected_version": 1,"replaces": v1, "text": REVISED, "idempotency_key": key}),
    )
    .await;
    assert_eq!(superseded["isError"], false, "{superseded}");
    assert_eq!(superseded["structuredContent"]["idempotent_replay"], false);
    assert_ne!(
        superseded["structuredContent"]["artifact_id"],
        submitted["structuredContent"]["artifact_id"]
    );
    assert_eq!(own_versions(&router, &writer).await, 3);

    drop_db(&db).await;
}

/// `oml_write`'s `workarea` takes the node's name as well as its id: the
/// facade passes it through as the body's `workarea` and the server
/// resolves it,
/// so a write naming `metals-crew` is captured there and the own-work
/// listing under that name lists it, while a derived path is refused as
/// `request.validation` naming the HTTP field the argument maps to; the
/// served catalog says so (mcp-plugin.md, a workarea by id or name;
/// errors.md `details.field`; ADR-0026 d4, d7).
#[tokio::test]
async fn oml_write_with_a_workarea_name_submits_into_it() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, _, _, crew) = agent_crew(&router).await;

    let remembered = call_tool(
        &router,
        &agent,
        1,
        "oml_write",
        json!({
            "workarea": "metals-crew",
            "claim_key": "supplier-quote-am-7741",
            "text": "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery.",
            "idempotency_key": "crew-procure/run-1/step-1",
        }),
    )
    .await;
    assert_eq!(remembered["isError"], false, "{remembered}");
    let receipt = &remembered["structuredContent"];
    assert_eq!(receipt["workarea"], crew.to_string(), "{remembered}");
    assert_eq!(receipt["state"], "held");
    let mine = call_tool(
        &router,
        &agent,
        2,
        "oml_read",
        json!({"workarea": "metals-crew"}),
    )
    .await;
    assert_eq!(mine["isError"], false, "{mine}");
    let items = mine["structuredContent"]["items"]
        .as_array()
        .expect("items");
    assert_eq!(items.len(), 1, "{mine}");
    assert_eq!(items[0]["artifact_id"], receipt["artifact_id"]);

    let refused = call_tool(
        &router,
        &agent,
        3,
        "oml_write",
        json!({
            "workarea": "universe.metals-crew",
            "claim_key": "supplier-quote-am-7742",
            "text": "Supplier Accra Metals quoted 415 USD/t.",
            "idempotency_key": "crew-procure/run-1/step-2",
        }),
    )
    .await;
    assert_eq!(refused["isError"], true, "{refused}");
    let error = &refused["structuredContent"]["error"];
    assert_eq!(error["code"], "request.validation", "{refused}");
    assert_eq!(error["details"]["field"], "/workarea", "{refused}");

    // The served entry of oml_write describes what the build serves: the id or the name, and no path (ADR-0025 d12,
    // ADR-0039 d10).
    let listed = rpc(&router, &agent, 4, "tools/list", json!({})).await;
    let tool = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|t| t["name"] == "oml_write")
        .expect("oml_write is served");
    let described = tool["inputSchema"]["properties"]["workarea"]["description"]
        .as_str()
        .expect("a description");
    assert!(described.contains("id or name"), "{described}");
    assert!(!described.contains("path given here"), "{described}");

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_workarea_without_an_argument_describes_the_default_workarea() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (status, boot) = send(
        &router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(&json!({"enrollment_token": ENROLLMENT, "name": "root-admin", "tenant_name": "mcp-tenant"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{boot}");
    let admin = boot["value"].as_str().expect("admin key").to_owned();
    let root = boot["root_workarea_id"].as_str().expect("root").to_owned();
    let (status, wa) = send(
        &router,
        "POST",
        "/v2/workareas",
        Some(&admin),
        Some("wa-1"),
        // A workarea names at most one crew record (ADR-0026 d8); this one
        // names none, so `crew` is null.
        Some(&json!({
            "parent": root,
            "name": "metals-crew",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{wa}");
    let crew = wa["id"].as_str().expect("workarea id").to_owned();
    let issue = |idem: &'static str, role: &'static str, grants: Value| {
        let (router, admin) = (&router, &admin);
        async move {
            let (status, key) = send(
                router,
                "POST",
                "/v2/keys",
                Some(admin),
                Some(idem),
                Some(&json!({"role": role, "name": idem, "principal": idem, "grants": grants})),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED, "{key}");
            key["value"].as_str().expect("show-once value").to_owned()
        }
    };
    let agent = issue(
        "agent",
        "writer",
        json!([
            {"workarea": crew, "role": "writer"},
            {"workarea": crew, "role": "reader"},
        ]),
    )
    .await;
    let writer = issue(
        "writer",
        "writer",
        json!([{"workarea": crew, "role": "writer"}]),
    )
    .await;
    let unscoped = issue("unscoped", "reader", json!([])).await;

    // Listed, and with workarea omitted it reads the key's journalled
    // default: the same answer as naming it, from one ring call (ADR-0039
    // d10).
    let listed = rpc(&router, &agent, 1, "tools/list", json!({})).await;
    assert!(
        listed["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|t| t["name"] == "oml_workarea"),
        "{listed}"
    );
    let info = call_tool(&router, &agent, 2, "oml_workarea", json!({})).await;
    assert_eq!(info["isError"], false, "{info}");
    let view = &info["structuredContent"];
    assert_eq!(
        view,
        &json!({
            "id": crew,
            "name": "metals-crew",
            "path": "universe.metals-crew",
            "status": "development",
            "crew": null,
            "parent": root,
            "children": [],
            "edges": [],
            "my_effective_verbs": [
                "recall", "read", "graph_read", "events_tail",
                "remember", "learn", "attach", "supersede",
            ],
            "permission_epoch": view["permission_epoch"],
            "policy": {"k": 1, "required_roles": ["dreamer"]},
            "pending_transitions": [],
            "brief": view["brief"],
        })
    );
    assert_eq!(
        view["brief"]["items"],
        json!([]),
        "no brief edge, no member"
    );
    assert!(view["permission_epoch"].is_u64(), "{view}");
    // By id and by name, the same node (ADR-0026 d4).
    for (id, workarea) in [(3, crew.as_str()), (8, "metals-crew")] {
        let named = call_tool(
            &router,
            &agent,
            id,
            "oml_workarea",
            json!({"workarea": workarea}),
        )
        .await;
        assert_eq!(&named["structuredContent"], view);
    }

    // A key with no default is refused by name; a node the key cannot read
    // is absence, as on the ring; an unknown argument is validation.
    for (id, token, args, code) in [
        (4, &unscoped, json!({}), "request.workarea_unresolved"),
        (5, &writer, json!({}), "access.not_found"),
        (6, &agent, json!({"workarea": root}), "access.not_found"),
        (
            7,
            &agent,
            json!({"workarea_id": crew}),
            "request.validation",
        ),
    ] {
        let refused = call_tool(&router, token, id, "oml_workarea", args).await;
        assert_eq!(refused["isError"], true, "{refused}");
        assert_eq!(
            refused["structuredContent"]["error"]["code"], code,
            "{refused}"
        );
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn an_admin_keys_oml_workarea_is_the_rings_structure_read_of_its_default() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (status, boot) = send(
        &router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(&json!({"enrollment_token": ENROLLMENT, "name": "root-admin", "tenant_name": "mcp-tenant"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{boot}");
    let bootstrap_admin = boot["value"].as_str().expect("admin key").to_owned();
    // A second admin key: admin admits no grant role, so it has no grants
    // and no default (roles-and-keys.md issuance order step 6).
    let (status, second) = send(
        &router,
        "POST",
        "/v2/keys",
        Some(&bootstrap_admin),
        Some("second-admin"),
        Some(
            &json!({"role": "admin", "name": "bob-admin", "principal": "bob-admin", "grants": []}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert!(second["default_workarea_id"].is_null(), "{second}");
    let second_admin = second["value"]
        .as_str()
        .expect("show-once value")
        .to_owned();

    // The facade rides the ring, whose workarea read answers an admin key by
    // its key role, structure only (ADR-0026 d15): the bootstrap admin reads
    // its default, the root, with no effective verb, and the admin key with
    // no default is refused by name.
    let read = call_tool(&router, &bootstrap_admin, 1, "oml_workarea", json!({})).await;
    assert_eq!(read["isError"], false, "{read}");
    assert_eq!(read["structuredContent"]["path"], "universe", "{read}");
    assert_eq!(
        read["structuredContent"]["my_effective_verbs"],
        json!([]),
        "{read}"
    );
    let refused = call_tool(&router, &second_admin, 2, "oml_workarea", json!({})).await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["code"], "request.workarea_unresolved",
        "{refused}"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_listing_pages_with_a_client_held_cursor() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, reader, crew) = crew(&router).await;
    let mut written = Vec::new();
    for n in 1..=3_u64 {
        let held = call_tool(
            &router,
            &writer,
            n,
            "oml_write",
            json!({
                "claim_key": format!("supplier-quote-am-774{n}"),
                "text": format!("Supplier Accra Metals quoted 41{n} USD/t."),
                "idempotency_key": format!("crew-procure/run-1/step-{n}"),
            }),
        )
        .await;
        assert_eq!(held["isError"], false, "{held}");
        written.push(held["structuredContent"]["artifact_id"].clone());
    }
    written.reverse();

    // The first page, newest first, and a cursor the client holds.
    let first = call_tool(&router, &writer, 4, "oml_read", json!({"limit": 2})).await;
    assert_eq!(first["isError"], false, "{first}");
    let page = &first["structuredContent"];
    let items = page["items"].as_array().expect("items");
    assert_eq!(
        items
            .iter()
            .map(|i| i["artifact_id"].clone())
            .collect::<Vec<_>>(),
        written[..2]
    );
    // Claim items with text, from the ring's memory summary (ADR-0025 d12,
    // ADR-0039 d4).
    let mut fields: Vec<&str> = items[0]
        .as_object()
        .expect("item")
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        vec![
            "anchor",
            "artifact_id",
            "claim_key",
            "created_at",
            "held_expires_at",
            "kind",
            "promoted_at",
            "replaces",
            "retirement",
            "source_checked",
            "stale",
            "state",
            "text",
            "tip",
            "tokens",
            "version",
            "workarea",
            "workarea_path",
        ]
    );
    assert_eq!(page["mode"], "listing");
    assert_eq!(items[0]["state"], "held");
    assert_eq!(items[0]["tip"], false);
    assert_eq!(items[0]["workarea"], crew.to_string());
    assert_eq!(items[0]["workarea_path"], "universe.metals-crew");
    assert_eq!(items[0]["tokens"], 10);
    let cursor = page["next_cursor"]
        .as_str()
        .expect("a next page")
        .to_owned();

    // The cursor resumes where the page ended; the last page holds none.
    let second = call_tool(
        &router,
        &writer,
        5,
        "oml_read",
        json!({"limit": 2, "cursor": cursor}),
    )
    .await;
    assert_eq!(second["isError"], false, "{second}");
    let items = second["structuredContent"]["items"]
        .as_array()
        .expect("items");
    assert_eq!(
        items
            .iter()
            .map(|i| i["artifact_id"].clone())
            .collect::<Vec<_>>(),
        written[2..]
    );
    assert_eq!(second["structuredContent"]["next_cursor"], Value::Null);

    // The filters pass through to the ring.
    let one = call_tool(
        &router,
        &writer,
        6,
        "oml_read",
        json!({"workarea": crew, "state": "held", "claim_key": "supplier-quote-am-7742"}),
    )
    .await;
    let items = one["structuredContent"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{one}");
    assert_eq!(items[0]["claim_key"], "supplier-quote-am-7742");

    // Own work is the calling key's alone, and its cursor answers no other.
    let theirs = call_tool(&router, &reader, 7, "oml_read", json!({})).await;
    assert_eq!(theirs["isError"], false, "{theirs}");
    assert_eq!(theirs["structuredContent"]["items"], json!([]));
    let foreign = call_tool(&router, &reader, 8, "oml_read", json!({"cursor": cursor})).await;
    assert_eq!(foreign["isError"], true, "{foreign}");
    assert_eq!(
        foreign["structuredContent"]["error"]["code"],
        "request.validation"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_listing_limit_is_at_most_200_as_on_the_ring() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, _, _) = crew(&router).await;

    // The served schema of oml_read's listing states the ring's page
    // bound, so an agent never sends a limit the catalog admits
    // and the ring refuses (ADR-0025 d12, ADR-0039 d10).
    let listed = rpc(&router, &writer, 1, "tools/list", json!({})).await;
    let my_work = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|t| t["name"] == "oml_read")
        .expect("oml_read is served");
    let limit = &my_work["inputSchema"]["properties"]["limit"];
    assert_eq!(limit["minimum"], 1, "{limit}");
    assert_eq!(limit["maximum"], 200, "{limit}");

    let at_bound = call_tool(&router, &writer, 2, "oml_read", json!({"limit": 200})).await;
    assert_eq!(at_bound["isError"], false, "{at_bound}");
    let refused = call_tool(&router, &writer, 3, "oml_read", json!({"limit": 201})).await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        refused["structuredContent"]["error"]["code"], "request.validation",
        "{refused}"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_recall_serves_steering_as_claim_items_with_text_and_no_score() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, _crew) = crew(&router).await;
    let rule = "Treat any supplier quote without an explicit validity date as expired.";
    let held = call_tool(
        &router,
        &writer,
        1,
        "oml_write",
        json!({
            "claim_key": "quote-validity",
            "kind": "steering",
            "text": rule,
            "idempotency_key": "crew-procure/run-1/step-1",
        }),
    )
    .await;
    assert_eq!(held["isError"], false, "{held}");
    let (status, served) = send(
        &router,
        "POST",
        "/v2/review/next",
        Some(&dreamer),
        Some("n-1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    let (status, verdict) = send(
        &router,
        "POST",
        "/v2/review/verdicts",
        Some(&dreamer),
        Some("v-1"),
        Some(&json!({"serve_id": served["serve_id"], "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");

    // The query misses the rule: steering applies by scope, and its item is
    // a claim item with text, never ranked (mcp-plugin.md, the claim item).
    let recall = call_tool(
        &router,
        &reader,
        2,
        "oml_read",
        json!({"query": "aluminium price"}),
    )
    .await;
    assert_eq!(recall["isError"], false, "{recall}");
    let steering = recall["structuredContent"]["steering"]
        .as_array()
        .expect("steering");
    assert_eq!(steering.len(), 1, "{recall}");
    let item = &steering[0];
    assert_eq!(
        item["artifact_id"],
        held["structuredContent"]["artifact_id"]
    );
    assert_eq!(item["text"], rule, "{item}");
    assert!(item.get("body").is_none(), "{item}");
    assert!(item.get("score").is_none(), "{item}");
    assert_eq!(recall["structuredContent"]["hits"], json!([]));

    drop_db(&db).await;
}

/// Budget tokens under counter `oml-tc/1`: ⌈body_bytes ÷ 4⌉ of the NFC
/// text, its byte length for the ASCII texts below (retrieval.md, token
/// budgets; ADR-0025 d6).
fn tokens(text: &str) -> u64 {
    u64::try_from(text.len().div_ceil(4)).expect("a short text")
}

/// The dreamer approves the next `n` rows review serves it, whatever their
/// order, over the review ring — never over MCP.
async fn approve(router: &Router, dreamer: &str, n: usize) {
    for i in 0..n {
        approve_as(router, dreamer, &i.to_string()).await;
    }
}

/// The dreamer approves the one row review serves it next, under
/// idempotency keys tagged `tag`, over the review ring.
async fn approve_as(router: &Router, dreamer: &str, tag: &str) {
    let (status, served) = send(
        router,
        "POST",
        "/v2/review/next",
        Some(dreamer),
        Some(&format!("next-{tag}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    let (status, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Some(dreamer),
        Some(&format!("verdict-{tag}")),
        Some(&json!({"serve_id": served["serve_id"], "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    assert_eq!(verdict["state"], "promoted", "{verdict}");
}

/// The agent writes each `(claim_key, kind, text)` through `oml_write`,
/// held, in order.
async fn remember_all(router: &Router, agent: &str, rows: &[(&str, &str, &str)]) {
    for (n, (claim_key, kind, text)) in (1_u64..).zip(rows) {
        let held = call_tool(
            router,
            agent,
            100 + n,
            "oml_write",
            json!({
                "claim_key": claim_key,
                "kind": kind,
                "text": text,
                "idempotency_key": format!("crew-procure/run-1/step-{n}"),
            }),
        )
        .await;
        assert_eq!(held["isError"], false, "{held}");
        assert_eq!(held["structuredContent"]["state"], "held", "{held}");
    }
}

/// The sorted member names of a JSON object.
fn fields(item: &Value) -> Vec<&str> {
    let mut names: Vec<&str> = item
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    names
}

const STEERING: &str = "Treat any supplier quote without an explicit validity date as expired.";
const QUOTE: &str =
    "Accra Metals aluminium price is 412 USD/t for Q1-2027 delivery, quote AM-7741.";
const ALLOY: &str =
    "Tema Alloys aluminium price is 409 USD/t for Q1-2027 delivery, valid until 2026-12-15.";

#[tokio::test]
async fn oml_read_recall_takes_include_unreviewed_and_max_tokens_as_it_declares_them() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, _, reader, _) = agent_crew(&router).await;

    // tools/list advertises, on oml_read, the one flag for
    // unreviewed rows and the ring's k and max_tokens maxima, the latter the
    // largest integer RFC 8785 digests exactly (retrieval.md token budgets;
    // ADR-0025 d7, d12; ADR-0039 d4, d10).
    let listed = rpc(&router, &agent, 1, "tools/list", json!({})).await;
    let recall = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|t| t["name"] == "oml_read")
        .expect("oml_read is served");
    let properties = &recall["inputSchema"]["properties"];
    assert_eq!(properties["include_unreviewed"]["type"], "boolean");
    assert_eq!(properties["include_unreviewed"]["default"], false);
    assert_eq!(properties["max_tokens"]["type"], "integer");
    assert_eq!(properties["max_tokens"]["minimum"], 1);
    assert_eq!(properties["max_tokens"]["maximum"], MAX_TOKENS);
    assert_eq!(properties["k"]["maximum"], 100);

    remember_all(
        &router,
        &agent,
        &[("supplier-quote-am-7741", "memory", QUOTE)],
    )
    .await;

    // include_unreviewed: the author's held row joins the hits, flagged held;
    // without it the recall serves promoted rows only, and no other key sees
    // the row whatever it sends (ADR-0025 d4).
    let own = call_tool(
        &router,
        &agent,
        2,
        "oml_read",
        json!({"query": "aluminium price", "include_unreviewed": true}),
    )
    .await;
    assert_eq!(own["isError"], false, "{own}");
    let hits = own["structuredContent"]["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1, "{own}");
    assert_eq!(hits[0]["state"], "held", "{own}");
    assert_eq!(hits[0]["tip"], false, "{own}");
    assert_eq!(hits[0]["text"], QUOTE, "{own}");
    let promoted_only = call_tool(
        &router,
        &agent,
        3,
        "oml_read",
        json!({"query": "aluminium price", "include_unreviewed": false}),
    )
    .await;
    assert_eq!(promoted_only["isError"], false, "{promoted_only}");
    assert_eq!(promoted_only["structuredContent"]["hits"], json!([]));
    let theirs = call_tool(
        &router,
        &reader,
        4,
        "oml_read",
        json!({"query": "aluminium price", "include_unreviewed": true}),
    )
    .await;
    assert_eq!(theirs["isError"], false, "{theirs}");
    assert_eq!(theirs["structuredContent"]["hits"], json!([]));

    // max_tokens reaches the ring's budget cut: the held row does not fit.
    let budgeted = call_tool(
        &router,
        &agent,
        5,
        "oml_read",
        json!({"query": "aluminium price", "include_unreviewed": true, "max_tokens": 1}),
    )
    .await;
    assert_eq!(budgeted["isError"], false, "{budgeted}");
    assert_eq!(budgeted["structuredContent"]["hits"], json!([]));
    assert_eq!(budgeted["structuredContent"]["budget"]["hits_omitted"], 1);
    assert_eq!(
        budgeted["structuredContent"]["budget"]["next_hit_tokens"],
        tokens(QUOTE)
    );

    // Out of range is the ring's typed refusal, as on the HTTP API.
    for (id, args) in [
        (6, json!({"query": "aluminium price", "max_tokens": 0})),
        (7, json!({"query": "aluminium price", "k": 101})),
        (
            8,
            json!({"query": "aluminium price", "max_tokens": MAX_TOKENS + 1}),
        ),
    ] {
        let refused = call_tool(&router, &agent, id, "oml_read", args).await;
        assert_eq!(refused["isError"], true, "{refused}");
        assert_eq!(
            refused["structuredContent"]["error"]["code"], "request.validation",
            "{refused}"
        );
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_recall_steering_items_carry_text_and_tokens_not_body() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, dreamer, _, crew) = agent_crew(&router).await;
    remember_all(
        &router,
        &agent,
        &[
            ("quote-validity", "steering", STEERING),
            ("supplier-quote-am-7741", "memory", QUOTE),
        ],
    )
    .await;
    approve(&router, &dreamer, 2).await;

    let recall = call_tool(
        &router,
        &agent,
        1,
        "oml_read",
        json!({"query": "aluminium price"}),
    )
    .await;
    assert_eq!(recall["isError"], false, "{recall}");
    let result = &recall["structuredContent"];
    let steering = result["steering"].as_array().expect("steering");
    let hits = result["hits"].as_array().expect("hits");
    assert_eq!(steering.len(), 1, "{recall}");
    assert_eq!(hits.len(), 1, "{recall}");

    // One claim-item shape: the base fields plus text, and a score and the
    // matched words on hits only — never the ring's body (mcp-plugin.md, the
    // claim item; ADR-0025 d12; ADR-0049 d6).
    let base = [
        "anchor",
        "artifact_id",
        "claim_key",
        "created_at",
        "kind",
        "promoted_at",
        "replaces",
        "retirement",
        "source_checked",
        "stale",
        "state",
        "text",
        "tip",
        "tokens",
        "version",
        "workarea",
        "workarea_path",
    ];
    assert_eq!(fields(&steering[0]), base, "{recall}");
    let mut with_score = base.to_vec();
    with_score.extend(["matched", "score"]);
    with_score.sort_unstable();
    assert_eq!(fields(&hits[0]), with_score, "{recall}");
    assert!(hits[0]["score"].is_number(), "{recall}");

    for (item, kind, claim_key, text) in [
        (&steering[0], "steering", "quote-validity", STEERING),
        (&hits[0], "memory", "supplier-quote-am-7741", QUOTE),
    ] {
        assert_eq!(item["kind"], kind, "{item}");
        assert_eq!(item["claim_key"], claim_key, "{item}");
        assert_eq!(item["text"], text, "{item}");
        assert_eq!(item["tokens"], tokens(text), "{item}");
        assert_eq!(item["version"], 1, "{item}");
        assert_eq!(item["state"], "promoted", "{item}");
        assert_eq!(item["tip"], true, "{item}");
        assert_eq!(item["retirement"], false, "{item}");
        assert_eq!(item["replaces"], Value::Null, "{item}");
        assert_eq!(item["workarea"], crew.to_string(), "{item}");
        assert_eq!(item["workarea_path"], "universe.metals-crew", "{item}");
        assert!(item["created_at"].is_string(), "{item}");
        assert!(item["promoted_at"].is_string(), "{item}");
    }
    assert!(result.get("budget").is_none(), "no max_tokens, no budget");

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_recall_under_max_tokens_returns_the_budget_object() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, dreamer, _, _) = agent_crew(&router).await;
    remember_all(
        &router,
        &agent,
        &[
            ("quote-validity", "steering", STEERING),
            ("supplier-quote-am-7741", "memory", QUOTE),
            ("supplier-quote-ta-0912", "memory", ALLOY),
        ],
    )
    .await;
    approve(&router, &dreamer, 3).await;
    let text_of = |item: &Value| {
        [QUOTE, ALLOY]
            .into_iter()
            .find(|text| item["text"] == *text)
            .expect("one of the two memories")
    };

    let open = call_tool(
        &router,
        &agent,
        1,
        "oml_read",
        json!({"query": "aluminium price"}),
    )
    .await;
    assert_eq!(open["isError"], false, "{open}");
    let ranked = open["structuredContent"]["hits"]
        .as_array()
        .expect("hits")
        .clone();
    assert_eq!(ranked.len(), 2, "{open}");
    assert!(
        open["structuredContent"].get("budget").is_none(),
        "no max_tokens, no budget: {open}"
    );

    // A budget that fits the steering and the first hit: steering whole and
    // first, the longest rank prefix of hits, and the cut reported in the
    // ring's budget object (retrieval.md, token budgets; ADR-0025 d7).
    let first = tokens(text_of(&ranked[0]));
    let max_tokens = tokens(STEERING) + first;
    let budgeted = call_tool(
        &router,
        &agent,
        2,
        "oml_read",
        json!({"query": "aluminium price", "max_tokens": max_tokens}),
    )
    .await;
    assert_eq!(budgeted["isError"], false, "{budgeted}");
    let result = &budgeted["structuredContent"];
    assert_eq!(result["steering"][0]["text"], STEERING, "{budgeted}");
    let hits = result["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1, "{budgeted}");
    assert_eq!(hits[0]["artifact_id"], ranked[0]["artifact_id"]);
    assert_eq!(
        result["budget"],
        json!({
            "max_tokens": max_tokens,
            "counter": "oml-tc/1",
            "steering_tokens": tokens(STEERING),
            "used_tokens": max_tokens,
            "truncated": true,
            "hits_omitted": 1,
            "next_hit_tokens": tokens(text_of(&ranked[1])),
        })
    );

    drop_db(&db).await;
}

/// The refusal a tool answers when its arguments are not its input schema,
/// as distinct from any refusal the ring answers to well-typed arguments.
fn outside_the_input_schema(result: &Value) -> bool {
    result["isError"] == true
        && result["structuredContent"]["error"]["message"]
            .as_str()
            .is_some_and(|message| message.starts_with(ARGUMENTS_OUTSIDE_THE_SCHEMA))
}

/// A value of the type `schema` declares: the first enum member, an
/// integer's minimum, one item of an array and every property of an object,
/// so a closed nested argument is held to the catalog too.
fn type_valid(schema: &Value, tag: &str) -> Value {
    if let Some(first) = schema["enum"]
        .as_array()
        .and_then(|members| members.first())
    {
        return first.clone();
    }
    match schema["type"].as_str() {
        Some("string") => json!(tag),
        Some("integer") => json!(schema["minimum"].as_u64().unwrap_or(1)),
        Some("boolean") => json!(true),
        Some("array") => json!([type_valid(&schema["items"], tag)]),
        Some("object") => Value::Object(
            schema["properties"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, property)| (name.clone(), type_valid(property, tag)))
                .collect(),
        ),
        other => panic!("no type-valid value for a schema of type {other:?}: {schema}"),
    }
}

#[tokio::test]
async fn every_advertised_input_property_is_accepted() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, _, _, _) = agent_crew(&router).await;

    // tools/list is the served subset of mcp-tools.json, so the server must
    // never advertise a field its argument struct refuses (ADR-0025 d12).
    let listed = rpc(&router, &agent, 1, "tools/list", json!({})).await;
    let mut id = 1;
    for tool in listed["result"]["tools"].as_array().expect("tools") {
        let name = tool["name"].as_str().expect("name");
        let schema = &tool["inputSchema"];
        let properties = schema["properties"].as_object().expect("properties");
        let required = |id: u64| -> serde_json::Map<String, Value> {
            schema["required"]
                .as_array()
                .expect("required")
                .iter()
                .filter_map(Value::as_str)
                .map(|p| {
                    (
                        p.to_owned(),
                        type_valid(&properties[p], &format!("{p}-{id}")),
                    )
                })
                .collect()
        };
        for (property, declared) in properties {
            id += 1;
            let mut args = required(id);
            args.insert(
                property.clone(),
                type_valid(declared, &format!("{property}-{id}")),
            );
            let result = call_tool(&router, &agent, id, name, Value::Object(args)).await;
            assert!(
                !outside_the_input_schema(&result),
                "{name} advertises {property} and refuses it: {result}"
            );
        }
        // The control: a property the schema does not declare is refused as
        // outside it, so the assertion above can fail.
        id += 1;
        let mut args = required(id);
        args.insert("undeclared".to_owned(), json!(true));
        let result = call_tool(&router, &agent, id, name, Value::Object(args)).await;
        assert!(outside_the_input_schema(&result), "{name}: {result}");
    }

    drop_db(&db).await;
}

/// An `oml_read` call's structured result, asserted not to be a refusal.
async fn read(router: &Router, token: &str, id: u64, args: Value) -> Value {
    let result = call_tool(router, token, id, "oml_read", args).await;
    assert_eq!(result["isError"], false, "{result}");
    result["structuredContent"].clone()
}

/// The `artifact_id`s of a result's `items`, in order.
fn item_ids(result: &Value) -> Vec<String> {
    result["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["artifact_id"].as_str())
        .map(str::to_owned)
        .collect()
}

/// The writer stores `text` through `oml_write` and gets its `artifact_id`.
async fn write(router: &Router, writer: &str, n: u64, text: &str) -> String {
    let written = call_tool(
        router,
        writer,
        200 + n,
        "oml_write",
        json!({"text": text, "idempotency_key": format!("crew-procure/run-9/step-{n}")}),
    )
    .await;
    assert_eq!(written["isError"], false, "{written}");
    written["structuredContent"]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned()
}

/// The dreamer gives the one row review serves it next `verdict`, over
/// the review ring; the served row's `artifact_id`.
async fn judge(router: &Router, dreamer: &str, tag: &str, verdict: &str) -> String {
    let (status, served) = send(
        router,
        "POST",
        "/v2/review/next",
        Some(dreamer),
        Some(&format!("next-{tag}")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    let (status, judged) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Some(dreamer),
        Some(&format!("verdict-{tag}")),
        Some(&json!({"serve_id": served["serve_id"], "verdict": verdict, "reason": "checked against the quote"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{judged}");
    served["item"]["artifact_id"]
        .as_str()
        .expect("served artifact id")
        .to_owned()
}

#[tokio::test]
async fn a_read_argument_outside_its_mode_is_refused_naming_it() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, _, _, _) = agent_crew(&router).await;
    let stored = write(&router, &agent, 1, QUOTE).await;

    // artifact_id fetches, query recalls, neither lists, and an argument of
    // another mode is refused by name rather than dropped (ADR-0039 d4).
    for (id, args, field) in [
        (1, json!({"artifact_id": stored, "k": 3}), "/k"),
        (
            2,
            json!({"artifact_id": stored, "workarea": "metals-crew"}),
            "/workarea",
        ),
        (3, json!({"artifact_id": stored, "state": "held"}), "/state"),
        (4, json!({"query": "aluminium", "cursor": "c-1"}), "/cursor"),
        (5, json!({"query": "aluminium", "own": false}), "/own"),
        (6, json!({"query": "aluminium", "kind": "memory"}), "/kind"),
        (7, json!({"own": false, "max_tokens": 50}), "/max_tokens"),
        (
            8,
            json!({"include_unreviewed": true}),
            "/include_unreviewed",
        ),
        (9, json!({"kinds": ["memory"]}), "/kinds"),
        // The lineage read is designed, not routed: its fields are refused
        // by name in every mode (ADR-0039 d2, d4, d9).
        (
            10,
            json!({"artifact_id": stored, "lineage": "ancestors"}),
            "/lineage",
        ),
        (11, json!({"artifact_id": stored, "depth": 2}), "/depth"),
        (12, json!({"depth": 2}), "/depth"),
    ] {
        let refused = call_tool(&router, &agent, id, "oml_read", args).await;
        refused_naming(&refused, field);
        assert!(!outside_the_input_schema(&refused), "{refused}");
    }

    // A field no mode declares is outside the schema, as on every tool.
    let undeclared = call_tool(&router, &agent, 13, "oml_read", json!({"scope": "x"})).await;
    assert!(outside_the_input_schema(&undeclared), "{undeclared}");

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_refuses_artifact_id_and_query_together() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, _, _, _) = agent_crew(&router).await;
    let stored = write(&router, &agent, 1, QUOTE).await;

    let refused = call_tool(
        &router,
        &agent,
        1,
        "oml_read",
        json!({"artifact_id": stored, "query": "aluminium"}),
    )
    .await;
    refused_naming(&refused, "/query");
    // Each alone is served.
    read(&router, &agent, 2, json!({"artifact_id": stored})).await;
    read(&router, &agent, 3, json!({"query": "aluminium"})).await;

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_with_no_mode_argument_lists_the_keys_own_work() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, reader, crew) = crew(&router).await;
    let first = write(&router, &writer, 1, QUOTE).await;
    let second = write(&router, &writer, 2, ALLOY).await;

    // own defaults to true: everything the key wrote, newest first, held
    // rows included, and nothing another key wrote (ADR-0039 d4).
    let own = read(&router, &writer, 1, json!({})).await;
    assert_eq!(own["mode"], "listing", "{own}");
    assert_eq!(item_ids(&own), [second.clone(), first.clone()], "{own}");
    let items = own["items"].as_array().expect("items");
    assert!(items.iter().all(|i| i["state"] == "held"), "{own}");
    assert_eq!(items[0]["workarea"], crew.to_string(), "{own}");
    assert_eq!(own["next_cursor"], Value::Null, "{own}");
    assert_eq!(
        read(&router, &writer, 2, json!({"own": true})).await,
        own,
        "own: true is the default"
    );
    let theirs = read(&router, &reader, 3, json!({})).await;
    assert_eq!(theirs["items"], json!([]), "{theirs}");

    // The listing filters and the client-held cursor pass through.
    let page = read(&router, &writer, 4, json!({"limit": 1})).await;
    assert_eq!(item_ids(&page), std::slice::from_ref(&second), "{page}");
    let cursor = page["next_cursor"].as_str().expect("a next page");
    let next = read(&router, &writer, 5, json!({"limit": 1, "cursor": cursor})).await;
    assert_eq!(item_ids(&next), std::slice::from_ref(&first), "{next}");
    for (id, args) in [
        (6, json!({"kind": "steering"})),
        (7, json!({"since": "2999-01-01T00:00:00Z"})),
        (8, json!({"until": "2000-01-01T00:00:00Z"})),
        (9, json!({"state": "promoted"})),
        (10, json!({"claim_key": "no-such-claim"})),
    ] {
        let none = read(&router, &writer, id, args).await;
        assert_eq!(none["items"], json!([]), "{none}");
    }
    let mine = read(
        &router,
        &writer,
        11,
        json!({"kind": "memory", "workarea": "metals-crew", "since": "2000-01-01T00:00:00Z", "until": "2999-01-01T00:00:00Z"}),
    )
    .await;
    assert_eq!(item_ids(&mine), [second, first], "{mine}");

    drop_db(&db).await;
}

/// The catalog states the bounds the listing applies: `since` takes a
/// version written at its instant and `until` leaves it out, as the store
/// compares (oml-port-contracts, "since is inclusive and until exclusive";
/// ADR-0039 d4).
#[test]
fn the_catalog_says_since_is_inclusive_and_until_exclusive() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../../../docs/api/mcp-tools.json")).expect("json");
    let read = catalog["tools"]
        .as_array()
        .expect("catalog tools")
        .iter()
        .find(|t| t["name"] == "oml_read")
        .expect("oml_read is declared");
    let described = |field: &str| {
        read["inputSchema"]["properties"][field]["description"]
            .as_str()
            .expect("described")
            .to_owned()
    };
    assert_eq!(
        described("since"),
        "Listing: versions written at or after this RFC 3339 date-time (inclusive)."
    );
    assert_eq!(
        described("until"),
        "Listing: versions written before this RFC 3339 date-time (exclusive)."
    );
}

#[tokio::test]
async fn oml_read_until_leaves_out_a_version_written_at_its_instant_and_since_takes_it() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, _, _, _) = crew(&router).await;
    let stored = write(&router, &writer, 1, QUOTE).await;
    let own = read(&router, &writer, 1, json!({})).await;
    assert_eq!(item_ids(&own), std::slice::from_ref(&stored), "{own}");
    let at = own["items"][0]["created_at"]
        .as_str()
        .expect("created_at")
        .to_owned();

    let until = read(&router, &writer, 2, json!({"until": at})).await;
    assert_eq!(
        until["items"],
        json!([]),
        "until excludes its instant: {until}"
    );
    let since = read(&router, &writer, 3, json!({"since": at})).await;
    assert_eq!(
        item_ids(&since),
        std::slice::from_ref(&stored),
        "since includes its instant: {since}"
    );
    let both = read(&router, &writer, 4, json!({"since": at, "until": at})).await;
    assert_eq!(both["items"], json!([]), "{both}");

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_with_own_false_lists_the_default_workarea() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, crew) = crew(&router).await;
    let promoted = write(&router, &writer, 1, QUOTE).await;
    assert_eq!(judge(&router, &dreamer, "v1", "approve").await, promoted);
    let held = write(&router, &writer, 2, ALLOY).await;

    // own: false is the scoped listing, new on MCP: the promoted tips of
    // the key's default workarea, never a held row, whoever wrote them
    // (ADR-0039 d4, d10).
    let scoped = read(&router, &reader, 1, json!({"own": false})).await;
    assert_eq!(scoped["mode"], "listing", "{scoped}");
    assert_eq!(
        item_ids(&scoped),
        std::slice::from_ref(&promoted),
        "{scoped}"
    );
    assert_eq!(scoped["items"][0]["state"], "promoted", "{scoped}");
    assert_eq!(scoped["items"][0]["workarea"], crew.to_string(), "{scoped}");
    assert!(!item_ids(&scoped).contains(&held), "{scoped}");
    // The same key's own listing is another read: it wrote nothing.
    let own = read(&router, &reader, 2, json!({})).await;
    assert_eq!(own["items"], json!([]), "{own}");
    // Naming the default workarea is the same listing.
    let named = read(
        &router,
        &reader,
        3,
        json!({"own": false, "workarea": "metals-crew"}),
    )
    .await;
    assert_eq!(named, scoped);

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_read_state_tells_an_agent_whether_its_write_was_approved() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, _, _) = crew(&router).await;
    let first = write(&router, &writer, 1, QUOTE).await;
    let second = write(&router, &writer, 2, ALLOY).await;

    // Before any verdict both writes are held, and none is approved.
    let held = read(&router, &writer, 1, json!({"state": "held"})).await;
    assert_eq!(item_ids(&held), [second.clone(), first.clone()], "{held}");
    let promoted = read(&router, &writer, 2, json!({"state": "promoted"})).await;
    assert_eq!(promoted["items"], json!([]), "{promoted}");

    // One approved and one rejected: the state filter over the key's own
    // work answers which, with no events cursor (ADR-0039 d2).
    let approved = judge(&router, &dreamer, "a", "approve").await;
    let rejected = judge(&router, &dreamer, "b", "reject").await;
    let mut both = vec![approved.clone(), rejected.clone()];
    both.sort_unstable();
    let mut written = vec![first, second];
    written.sort_unstable();
    assert_eq!(both, written);
    for (id, state, expected) in [
        (3, "promoted", vec![approved]),
        (4, "rejected", vec![rejected]),
        (5, "held", Vec::new()),
    ] {
        let page = read(&router, &writer, id, json!({"state": state})).await;
        assert_eq!(item_ids(&page), expected, "{state}: {page}");
        assert!(
            page["items"]
                .as_array()
                .expect("items")
                .iter()
                .all(|i| i["state"] == state),
            "{page}"
        );
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn every_oml_read_mode_returns_claim_items_with_text_under_mode() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (writer, dreamer, reader, _) = crew(&router).await;
    let stored = write(&router, &writer, 1, QUOTE).await;
    judge(&router, &dreamer, "v1", "approve").await;

    // ADR-0025 d12's claim item and its text, in each mode's container.
    let base = [
        "anchor",
        "artifact_id",
        "claim_key",
        "created_at",
        "kind",
        "promoted_at",
        "replaces",
        "retirement",
        "source_checked",
        "stale",
        "state",
        "text",
        "tip",
        "tokens",
        "version",
        "workarea",
        "workarea_path",
    ];
    let with = |extra: &[&'static str]| {
        let mut names: Vec<&str> = base.iter().copied().chain(extra.iter().copied()).collect();
        names.sort_unstable();
        names
    };

    let got = read(&router, &reader, 1, json!({"artifact_id": stored})).await;
    assert_eq!(fields(&got), ["items", "mode"], "{got}");
    assert_eq!(got["mode"], "get");
    assert_eq!(item_ids(&got), std::slice::from_ref(&stored), "{got}");
    assert_eq!(
        fields(&got["items"][0]),
        with(&["attachments", "derives_from"])
    );
    assert_eq!(got["items"][0]["text"], QUOTE);

    let recalled = read(
        &router,
        &reader,
        2,
        json!({"query": "aluminium price", "workarea": "metals-crew", "k": 5, "kinds": ["memory"], "include_unreviewed": false, "max_tokens": MAX_TOKENS}),
    )
    .await;
    assert_eq!(recalled["mode"], "recall", "{recalled}");
    assert_eq!(
        fields(&recalled),
        ["budget", "hits", "mode", "recall_id", "search", "steering"]
    );
    let hit = &recalled["hits"][0];
    assert_eq!(hit["artifact_id"], stored.as_str(), "{recalled}");
    assert_eq!(fields(hit), with(&["matched", "score"]));
    assert_eq!(hit["text"], QUOTE);

    for (id, token, args) in [(3, &writer, json!({})), (4, &reader, json!({"own": false}))] {
        let listed = read(&router, token, id, args).await;
        assert_eq!(listed["mode"], "listing", "{listed}");
        assert_eq!(fields(&listed), ["items", "mode", "next_cursor"]);
        assert_eq!(item_ids(&listed), std::slice::from_ref(&stored), "{listed}");
        assert_eq!(fields(&listed["items"][0]), with(&["held_expires_at"]));
        assert_eq!(listed["items"][0]["text"], QUOTE);
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_workarea_returns_the_node_with_the_callers_verbs() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let (agent, _, reader, crew) = agent_crew(&router).await;

    // Listed with ADR-0039 d5's read-only hints, beside oml_read.
    let listed = rpc(&router, &agent, 1, "tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().expect("tools");
    for name in ["oml_read", "oml_workarea"] {
        let tool = tools
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is served: {listed}"));
        assert_eq!(
            tool["annotations"],
            json!({"readOnlyHint": true, "idempotentHint": true, "openWorldHint": false}),
            "{name}"
        );
    }

    // One GET /v2/workareas/{workarea}: the node and the verbs this key's
    // effective access carries there, the default when none is named.
    let node = call_tool(&router, &agent, 2, "oml_workarea", json!({})).await;
    assert_eq!(node["isError"], false, "{node}");
    let view = &node["structuredContent"];
    assert_eq!(view["id"], crew.to_string(), "{view}");
    assert_eq!(view["name"], "metals-crew", "{view}");
    assert_eq!(view["path"], "universe.metals-crew", "{view}");
    assert_eq!(
        view["my_effective_verbs"],
        json!([
            "recall",
            "read",
            "graph_read",
            "events_tail",
            "remember",
            "learn",
            "attach",
            "supersede",
        ]),
        "{view}"
    );
    assert!(view["permission_epoch"].is_u64(), "{view}");
    for (id, workarea) in [(3, crew.to_string()), (4, "metals-crew".to_owned())] {
        let named = call_tool(
            &router,
            &agent,
            id,
            "oml_workarea",
            json!({"workarea": workarea}),
        )
        .await;
        assert_eq!(&named["structuredContent"], view);
    }
    // Another key's verbs are its own: the reader reads, never writes.
    let theirs = call_tool(&router, &reader, 5, "oml_workarea", json!({})).await;
    assert_eq!(
        theirs["structuredContent"]["my_effective_verbs"],
        json!(["recall", "read", "graph_read", "events_tail"]),
        "{theirs}"
    );
    // An argument outside the schema is refused, as on every tool.
    let refused = call_tool(&router, &agent, 6, "oml_workarea", json!({"scope": "x"})).await;
    assert!(outside_the_input_schema(&refused), "{refused}");

    drop_db(&db).await;
}

/// The lead's router over in-memory doubles, its bootstrap open under
/// [`ENROLLMENT`]: it needs no role pair, for what a test over it proves is
/// the facade's, not the store's.
fn doubles_router() -> Router {
    use oml_app::App;
    use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new([7; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let ping: oml_transport_http::DbPing = std::sync::Arc::new(|| Box::pin(async { true }));
    oml_transport_http::ApiState::new(
        std::sync::Arc::new(app),
        store,
        ping,
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router()
}

/// `oml_workarea` serves the workarea read's policy, live proposals and
/// ratified brief, each brief member as the claim item every tool returns,
/// with its text, and no fourth tool is added (ADR-0046 d3, d9; ADR-0039
/// d8).
#[tokio::test]
async fn oml_workarea_serves_the_policy_the_live_proposals_and_the_brief_as_claim_items() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let ((writer, dreamer, _reader, crew), admin) =
        crew_and_admin(&router, &["writer", "reader"]).await;
    let artifact = ratified(&router, &writer, &dreamer).await;
    let mut curators = Vec::new();
    for name in ["dora", "dan"] {
        let (status, key) = send(
            &router,
            "POST",
            "/v2/keys",
            Some(&admin),
            Some(&format!("issue-{name}")),
            Some(&json!({
                "role": "dreamer", "name": name, "principal": name,
                "grants": [{"workarea": crew, "role": "dreamer"}, {"workarea": crew, "role": "reader"}],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{key}");
        curators.push(key["value"].as_str().expect("value").to_owned());
    }
    let (status, edge) = send(
        &router,
        "POST",
        "/v2/edges",
        Some(&curators[0]),
        Some("brief-1"),
        Some(&json!({"kind": "brief", "from_artifact_id": artifact, "to_workarea_id": crew})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{edge}");
    let id = edge["edge_id"].as_str().expect("edge id");
    let (status, ratified_edge) = send(
        &router,
        "POST",
        &format!("/v2/edges/{id}/ratify"),
        Some(&curators[1]),
        Some("brief-2"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ratified_edge}");
    let (status, proposal) = send(
        &router,
        "POST",
        &format!("/v2/workareas/{crew}/transitions"),
        Some(&curators[0]),
        Some("t-1"),
        Some(&json!({"to": "committed"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{proposal}");

    let result = call_tool(&router, &writer, 1, "oml_workarea", json!({})).await;
    let node = &result["structuredContent"];
    assert_eq!(
        node["policy"],
        json!({"k": 1, "required_roles": ["dreamer"]}),
        "{result}"
    );
    assert_eq!(
        node["pending_transitions"][0]["transition_id"],
        proposal["transition_id"]
    );
    let item = &node["brief"]["items"][0];
    assert_eq!(item["artifact_id"], artifact.as_str(), "{result}");
    assert_eq!(item["text"], QUOTE, "{result}");
    assert_eq!(node["brief"]["budget"]["max_tokens"], 400);
    drop_db(&db).await;
}
