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
use oml_transport_http::mcp::{ARGUMENTS_OUTSIDE_THE_SCHEMA, MCP_PROTOCOL_VERSION, SERVED_TOOLS};

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
    for (role, principal, grants) in [
        ("writer", "crew/agent-7", writer_grants),
        ("dreamer", "crew/reviewer-3", &["dreamer"][..]),
        ("reader", "crew/agent-recall", &["reader"][..]),
    ] {
        let grants: Vec<Value> = grants
            .iter()
            .map(|grant| json!({"workarea_node_id": crew, "role": grant}))
            .collect();
        let (status, key) = send(
            router,
            "POST",
            "/v2/keys",
            Some(&admin),
            Some(&format!("issue-{role}")),
            Some(&json!({"role": role, "principal": principal, "grants": grants})),
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
        let (status, served) = send(
            router,
            "POST",
            "/v2/review/next",
            Some(dreamer),
            Some(&format!("next-{i}")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{served}");
        let (status, verdict) = send(
            router,
            "POST",
            "/v2/review/verdicts",
            Some(dreamer),
            Some(&format!("verdict-{i}")),
            Some(&json!({"serve_id": served["serve_id"], "verdict": "approve"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{verdict}");
        assert_eq!(verdict["state"], "promoted", "{verdict}");
    }
}

/// The agent remembers each `(claim_key, kind, text)` held, in order.
async fn remember_all(router: &Router, agent: &str, rows: &[(&str, &str, &str)]) {
    for (n, (claim_key, kind, text)) in (1_u64..).zip(rows) {
        let held = call_tool(
            router,
            agent,
            100 + n,
            "oml_remember",
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
async fn oml_recall_takes_include_own_held_and_max_tokens_from_the_catalog_schema() {
    let (router, db) = fresh_router().await;
    let (agent, _, reader, _) = agent_crew(&router).await;

    // tools/list advertises both inputs and the ring's k maximum (ADR-0025
    // d12).
    let listed = rpc(&router, &agent, 1, "tools/list", json!({})).await;
    let recall = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .find(|t| t["name"] == "oml_recall")
        .expect("oml_recall is served");
    let properties = &recall["inputSchema"]["properties"];
    assert_eq!(properties["include_own_held"]["type"], "boolean");
    assert_eq!(properties["include_own_held"]["default"], false);
    assert_eq!(properties["max_tokens"]["type"], "integer");
    assert_eq!(properties["max_tokens"]["minimum"], 1);
    assert_eq!(properties["k"]["maximum"], 100);

    remember_all(
        &router,
        &agent,
        &[("supplier-quote-am-7741", "memory", QUOTE)],
    )
    .await;

    // include_own_held: the author's held row joins the hits, flagged held;
    // without it the recall serves promoted rows only, and no other key sees
    // the row whatever it sends (ADR-0025 d4).
    let own = call_tool(
        &router,
        &agent,
        2,
        "oml_recall",
        json!({"query": "aluminium price", "include_own_held": true}),
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
        "oml_recall",
        json!({"query": "aluminium price", "include_own_held": false}),
    )
    .await;
    assert_eq!(promoted_only["isError"], false, "{promoted_only}");
    assert_eq!(promoted_only["structuredContent"]["hits"], json!([]));
    let theirs = call_tool(
        &router,
        &reader,
        4,
        "oml_recall",
        json!({"query": "aluminium price", "include_own_held": true}),
    )
    .await;
    assert_eq!(theirs["isError"], false, "{theirs}");
    assert_eq!(theirs["structuredContent"]["hits"], json!([]));

    // max_tokens reaches the ring's budget cut: the held row does not fit.
    let budgeted = call_tool(
        &router,
        &agent,
        5,
        "oml_recall",
        json!({"query": "aluminium price", "include_own_held": true, "max_tokens": 1}),
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
    ] {
        let refused = call_tool(&router, &agent, id, "oml_recall", args).await;
        assert_eq!(refused["isError"], true, "{refused}");
        assert_eq!(
            refused["structuredContent"]["error"]["code"], "request.validation",
            "{refused}"
        );
    }

    drop_db(&db).await;
}

#[tokio::test]
async fn oml_recall_steering_items_carry_text_and_tokens_not_body() {
    let (router, db) = fresh_router().await;
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
        "oml_recall",
        json!({"query": "aluminium price"}),
    )
    .await;
    assert_eq!(recall["isError"], false, "{recall}");
    let result = &recall["structuredContent"];
    let steering = result["steering"].as_array().expect("steering");
    let hits = result["hits"].as_array().expect("hits");
    assert_eq!(steering.len(), 1, "{recall}");
    assert_eq!(hits.len(), 1, "{recall}");

    // One claim-item shape: the base fields plus text, and a score on hits
    // only — never the ring's body (mcp-plugin.md, the claim item; ADR-0025
    // d12).
    let base = [
        "artifact_id",
        "claim_key",
        "created_at",
        "kind",
        "promoted_at",
        "replaces",
        "retirement",
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
    with_score.push("score");
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
async fn oml_recall_under_max_tokens_returns_the_budget_object() {
    let (router, db) = fresh_router().await;
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
        "oml_recall",
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
        "oml_recall",
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
    let (router, db) = fresh_router().await;
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
