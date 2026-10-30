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
use oml_transport_http::mcp::{MCP_PROTOCOL_VERSION, SERVED_TOOLS};

const ENROLLMENT: &str = "enr_test_fixture_token";

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev".to_owned())
}

fn db_url(name: &str) -> String {
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{prefix}/{name}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

async fn fresh_router() -> (Router, String) {
    let db_name = format!(
        "oml_test_mcp_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst)
    );
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin
        .execute(format!("CREATE DATABASE {db_name}").as_str())
        .await
        .expect("create throwaway database");
    let store = PgStore::connect(&db_url(&db_name)).await.expect("connect");
    store.migrate().await.expect("embedded migrations apply");
    let signing = SigningKey::from_bytes(&OsIds.new_secret());
    let router = lead_state(
        &store,
        signing,
        pg_ping(store.pool().clone()),
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    (router, db_name)
}

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
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
    let (status, boot) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(&json!({"enrollment_token": ENROLLMENT, "tenant_name": "mcp-tenant"})),
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
        Some(&json!({"parent_id": root, "name": "metals-crew"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{wa}");
    let crew = Uuid::parse_str(wa["id"].as_str().expect("workarea id")).expect("uuid");
    let mut keys = Vec::new();
    for (role, principal) in [
        ("writer", "crew/agent-7"),
        ("dreamer", "crew/reviewer-3"),
        ("reader", "crew/agent-recall"),
    ] {
        let (status, key) = send(
            router,
            "POST",
            "/v2/keys",
            Some(&admin),
            Some(&format!("issue-{role}")),
            Some(&json!({
                "role": role,
                "principal": principal,
                "grants": [{"workarea_node_id": crew, "role": role}],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{key}");
        keys.push(key["value"].as_str().expect("show-once value").to_owned());
    }
    let reader = keys.pop().expect("reader");
    let dreamer = keys.pop().expect("dreamer");
    let writer = keys.pop().expect("writer");
    (writer, dreamer, reader, crew)
}

#[tokio::test]
async fn the_mcp_endpoint_refuses_a_call_without_a_valid_key_before_any_dispatch() {
    let (router, db) = fresh_router().await;
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
    let (router, db) = fresh_router().await;
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

#[tokio::test]
async fn no_promote_sign_demote_delete_or_admin_verb_is_a_tool() {
    let (router, db) = fresh_router().await;
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
async fn an_agent_remembers_through_mcp_and_recalls_only_after_another_identity_ratifies() {
    let (router, db) = fresh_router().await;
    let (writer, dreamer, reader, crew) = crew(&router).await;
    let quote = "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery.";
    let remember = json!({
        "claim_key": "supplier-quote-am-7741",
        "text": quote,
        "idempotency_key": "crew-procure/run-1/step-1",
    });

    // Held on arrival.
    let held = call_tool(&router, &writer, 1, "oml_remember", remember.clone()).await;
    assert_eq!(held["isError"], false, "{held}");
    let receipt = &held["structuredContent"];
    assert_eq!(receipt["state"], "held");
    assert_eq!(receipt["workarea"], crew.to_string());
    assert_eq!(receipt["idempotent_replay"], false);
    let text: Value =
        serde_json::from_str(held["content"][0]["text"].as_str().expect("text")).expect("json");
    assert_eq!(&text, receipt, "the text content is the structured content");

    // A retry with the same idempotency key is the stored response.
    let replay = call_tool(&router, &writer, 2, "oml_remember", remember).await;
    assert_eq!(replay["structuredContent"]["idempotent_replay"], true);
    assert_eq!(
        replay["structuredContent"]["artifact_id"],
        receipt["artifact_id"]
    );

    // Not visible to a reader while held.
    let recall = json!({"query": "Accra Metals"});
    let before = call_tool(&router, &reader, 3, "oml_recall", recall.clone()).await;
    assert_eq!(before["isError"], false, "{before}");
    assert_eq!(before["structuredContent"]["hits"], json!([]));

    // The writer's key carries no read grant: the ring's typed refusal.
    let refused = call_tool(&router, &writer, 4, "oml_recall", recall.clone()).await;
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(refused["structuredContent"]["error"]["code"].is_string());
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

    let after = call_tool(&router, &reader, 5, "oml_recall", recall).await;
    let hits = after["structuredContent"]["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1, "{after}");
    assert_eq!(hits[0]["artifact_id"], receipt["artifact_id"]);
    assert_eq!(hits[0]["text"], quote);
    assert_eq!(hits[0]["state"], "promoted");
    assert_eq!(after["structuredContent"]["steering"], json!([]));

    // The events tail shows the journal the loop wrote.
    let tail = call_tool(
        &router,
        &reader,
        6,
        "oml_events_tail",
        json!({"limit": 200}),
    )
    .await;
    assert_eq!(tail["isError"], false, "{tail}");
    let items = tail["structuredContent"]["items"]
        .as_array()
        .expect("items");
    assert!(!items.is_empty());
    assert_eq!(
        tail["structuredContent"]["head_seq"],
        items.last().expect("last")["seq"]
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn a_tool_refusal_is_the_ring_refusal_as_an_error_result() {
    let (router, db) = fresh_router().await;
    let (writer, _, reader, _) = crew(&router).await;

    // learn without lineage: the ring's typed refusal, not a transport error.
    let learned = call_tool(
        &router,
        &writer,
        1,
        "oml_learn",
        json!({"claim_key": "k", "text": "t", "derives_from": [], "idempotency_key": "l-1"}),
    )
    .await;
    assert_eq!(learned["isError"], true);
    assert_eq!(
        learned["structuredContent"]["error"]["code"],
        "submission.lineage_required"
    );

    // A reader cannot remember (R1 on the ring).
    let denied = call_tool(
        &router,
        &reader,
        2,
        "oml_remember",
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
        "oml_remember",
        json!({"claim_key": "k", "text": "t", "idempotency_key": "m-1", "tenant": "other"}),
    )
    .await;
    assert_eq!(malformed["isError"], true);
    assert_eq!(
        malformed["structuredContent"]["error"]["code"],
        "request.validation"
    );

    // A catalog tool that is not built yet is named, not unknown.
    let unbuilt = rpc(
        &router,
        &writer,
        4,
        "tools/call",
        json!({"name": "oml_my_work", "arguments": {}}),
    )
    .await;
    assert_eq!(unbuilt["error"]["code"], -32602);
    assert!(
        unbuilt["error"]["message"]
            .as_str()
            .expect("message")
            .contains("not built yet")
    );

    drop_db(&db).await;
}
