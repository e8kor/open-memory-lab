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

    // The writer's key carries no read grant, so its scope is not readable:
    // the ring's typed refusal, absence (ADR-0025 d1).
    let refused = call_tool(&router, &writer, 4, "oml_recall", recall.clone()).await;
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

    let after = call_tool(&router, &reader, 5, "oml_recall", recall).await;
    let hits = after["structuredContent"]["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 1, "{after}");
    assert_eq!(hits[0]["artifact_id"], receipt["artifact_id"]);
    assert_eq!(hits[0]["text"], quote);
    assert_eq!(hits[0]["state"], "promoted");
    assert_eq!(after["structuredContent"]["steering"], json!([]));

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_events_tail_accepts_scope_and_answers_events_head_seq_next_after_seq_idle() {
    let (router, db) = fresh_router().await;
    let (writer, dreamer, reader, crew) = crew(&router).await;
    let held = call_tool(
        &router,
        &writer,
        1,
        "oml_remember",
        json!({
            "claim_key": "supplier-quote-am-7741",
            "text": "Supplier Accra Metals quoted 412 USD/t for Q1-2027 delivery.",
            "idempotency_key": "crew-procure/run-1/step-1",
        }),
    )
    .await;
    assert_eq!(held["isError"], false, "{held}");
    let artifact = held["structuredContent"]["artifact_id"].clone();
    let (status, served) = send(
        &router,
        "POST",
        "/v2/review/next",
        Some(&dreamer),
        Some("n-1"),
        Some(&json!({})),
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

    // An omitted after_seq starts from the beginning, as on the ring, and an
    // omitted scope is the key's default workarea (ADR-0025 d15).
    let whole = call_tool(
        &router,
        &reader,
        2,
        "oml_events_tail",
        json!({"limit": 200}),
    )
    .await;
    assert_eq!(whole["isError"], false, "{whole}");
    let page = &whole["structuredContent"];
    let head = page["head_seq"].as_u64().expect("head_seq");
    assert_eq!(page["next_after_seq"].as_u64(), Some(head), "{page}");
    assert_eq!(page["idle"], true, "{page}");
    let events = page["events"].as_array().expect("events");
    for event in events {
        let mut fields: Vec<&str> = event
            .as_object()
            .expect("an event")
            .keys()
            .map(String::as_str)
            .collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            ["at", "event_type", "seq", "subject", "workarea"],
            "hashes, signatures, payloads and identities are dropped: {event}"
        );
    }
    let promote = events
        .iter()
        .find(|e| e["event_type"] == "lifecycle.promote")
        .expect("the promote");
    assert_eq!(
        promote["subject"],
        json!({"artifact_id": artifact, "claim_key": "supplier-quote-am-7741", "version": 1})
    );
    assert_eq!(promote["workarea"], crew.to_string());
    let created = events
        .iter()
        .find(|e| e["event_type"] == "workarea.create")
        .expect("the crew's creation");
    assert_eq!(created["subject"], json!({"workarea_id": crew.to_string()}));
    assert_eq!(created["workarea"], crew.to_string());
    assert!(
        events
            .iter()
            .all(|e| e["event_type"] != "submission.remember"),
        "another key's submission is not on this key's tail: {page}"
    );

    // scope is the ring's workarea_id: naming the default answers the same.
    let scoped = call_tool(
        &router,
        &reader,
        3,
        "oml_events_tail",
        json!({"scope": crew, "limit": 200}),
    )
    .await;
    assert_eq!(scoped["structuredContent"], *page, "{scoped}");

    // A page short of the head is not idle and resumes where its scan ended.
    let first = call_tool(
        &router,
        &reader,
        4,
        "oml_events_tail",
        json!({"after_seq": 0, "limit": 1}),
    )
    .await;
    let first = &first["structuredContent"];
    assert_eq!(
        first["events"],
        json!([]),
        "the genesis is not on the content tail"
    );
    assert_eq!(first["next_after_seq"], 1, "{first}");
    assert_eq!(first["idle"], false, "{first}");

    // A scope outside read access and an after_seq past the head are the
    // ring's refusals.
    for (args, code) in [
        (
            json!({"scope": Uuid::from_u128(0xBEEF)}),
            "access.not_found",
        ),
        (json!({"after_seq": head + 1}), "request.validation"),
    ] {
        let refused = call_tool(&router, &reader, 5, "oml_events_tail", args).await;
        assert_eq!(refused["isError"], true, "{refused}");
        assert_eq!(
            refused["structuredContent"]["error"]["code"], code,
            "{refused}"
        );
    }

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
        json!({"name": "oml_graph", "arguments": {}}),
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

#[tokio::test]
async fn oml_get_answers_not_built_yet_and_so_does_oml_supersede() {
    let (router, db) = fresh_router().await;
    let (writer, _, _, _) = crew(&router).await;

    // ADR-0025 d12 adds both tools to the catalog; until they are served
    // they are named, not unknown, and never listed.
    for (id, name) in [(1, "oml_get"), (2, "oml_supersede")] {
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
        assert!(message.contains("not built yet"), "{body}");
        assert!(!message.contains("(R1)"), "{body}");
    }
    let listed = rpc(&router, &writer, 3, "tools/list", json!({})).await;
    for tool in listed["result"]["tools"].as_array().expect("tools") {
        assert_ne!(tool["name"], "oml_get");
        assert_ne!(tool["name"], "oml_supersede");
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_workarea_info_without_an_argument_describes_the_default_workarea() {
    let (router, db) = fresh_router().await;
    let (status, boot) = send(
        &router,
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
        &router,
        "POST",
        "/v2/workareas",
        Some(&admin),
        Some("wa-1"),
        Some(&json!({
            "parent_id": root,
            "name": "metals-crew",
            "crew": {"name": "metals-crew"},
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
                Some(&json!({"role": role, "principal": idem, "grants": grants})),
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
            {"workarea_node_id": crew, "role": "writer"},
            {"workarea_node_id": crew, "role": "reader"},
        ]),
    )
    .await;
    let writer = issue(
        "writer",
        "writer",
        json!([{"workarea_node_id": crew, "role": "writer"}]),
    )
    .await;
    let unscoped = issue("unscoped", "reader", json!([])).await;

    // Listed, and omitted workarea means the key's journalled default: the
    // same answer as naming it, from one ring call.
    let listed = rpc(&router, &agent, 1, "tools/list", json!({})).await;
    assert!(
        listed["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|t| t["name"] == "oml_workarea_info"),
        "{listed}"
    );
    let info = call_tool(&router, &agent, 2, "oml_workarea_info", json!({})).await;
    assert_eq!(info["isError"], false, "{info}");
    let view = &info["structuredContent"];
    assert_eq!(
        view,
        &json!({
            "id": crew,
            "path": "universe.metals-crew",
            "status": "development",
            "crew": {"name": "metals-crew"},
            "parent": root,
            "children": [],
            "edges": [],
            "my_effective_verbs": [
                "recall", "read", "graph_read", "events_tail",
                "remember", "learn", "attach", "supersede",
            ],
            "permission_epoch": view["permission_epoch"],
        })
    );
    assert!(view["permission_epoch"].is_u64(), "{view}");
    let named = call_tool(
        &router,
        &agent,
        3,
        "oml_workarea_info",
        json!({"workarea": crew}),
    )
    .await;
    assert_eq!(&named["structuredContent"], view);

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
        let refused = call_tool(&router, token, id, "oml_workarea_info", args).await;
        assert_eq!(refused["isError"], true, "{refused}");
        assert_eq!(
            refused["structuredContent"]["error"]["code"], code,
            "{refused}"
        );
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn an_admin_key_is_refused_oml_workarea_info_with_verb_ceiling_whether_or_not_it_has_a_default()
 {
    let (router, db) = fresh_router().await;
    let (status, boot) = send(
        &router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(&json!({"enrollment_token": ENROLLMENT, "tenant_name": "mcp-tenant"})),
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
        Some(&json!({"role": "admin", "principal": "bob-admin", "grants": []})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert!(second["default_workarea_id"].is_null(), "{second}");
    let second_admin = second["value"]
        .as_str()
        .expect("show-once value")
        .to_owned();

    // Step 1 of the refusal order needs no node, so it answers before the
    // default resolves: both admin keys get the same code, and neither is
    // told about its default workarea.
    for (id, token) in [(1, &bootstrap_admin), (2, &second_admin)] {
        let refused = call_tool(&router, token, id, "oml_workarea_info", json!({})).await;
        assert_eq!(refused["isError"], true, "{refused}");
        assert_eq!(
            refused["structuredContent"]["error"]["code"], "access.verb_ceiling",
            "{refused}"
        );
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_my_work_pages_with_a_client_held_cursor() {
    let (router, db) = fresh_router().await;
    let (writer, _, reader, crew) = crew(&router).await;
    let mut written = Vec::new();
    for n in 1..=3_u64 {
        let held = call_tool(
            &router,
            &writer,
            n,
            "oml_remember",
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
    let first = call_tool(&router, &writer, 4, "oml_my_work", json!({"limit": 2})).await;
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
    // Claim items without text, from the ring's memory summary (ADR-0025 d12).
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
            "artifact_id",
            "claim_key",
            "created_at",
            "held_expires_at",
            "kind",
            "promoted_at",
            "replaces",
            "retirement",
            "state",
            "tip",
            "tokens",
            "version",
            "workarea",
            "workarea_path",
        ]
    );
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
        "oml_my_work",
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
        "oml_my_work",
        json!({"workarea": crew, "state": "held", "claim_key": "supplier-quote-am-7742"}),
    )
    .await;
    let items = one["structuredContent"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{one}");
    assert_eq!(items[0]["claim_key"], "supplier-quote-am-7742");

    // Own work is the calling key's alone, and its cursor answers no other.
    let theirs = call_tool(&router, &reader, 7, "oml_my_work", json!({})).await;
    assert_eq!(theirs["isError"], false, "{theirs}");
    assert_eq!(theirs["structuredContent"]["items"], json!([]));
    let foreign = call_tool(
        &router,
        &reader,
        8,
        "oml_my_work",
        json!({"cursor": cursor}),
    )
    .await;
    assert_eq!(foreign["isError"], true, "{foreign}");
    assert_eq!(
        foreign["structuredContent"]["error"]["code"],
        "request.validation"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_recall_serves_steering_as_claim_items_with_text_and_no_score() {
    let (router, db) = fresh_router().await;
    let (writer, dreamer, reader, _crew) = crew(&router).await;
    let rule = "Treat any supplier quote without an explicit validity date as expired.";
    let held = call_tool(
        &router,
        &writer,
        1,
        "oml_remember",
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
        "oml_recall",
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
