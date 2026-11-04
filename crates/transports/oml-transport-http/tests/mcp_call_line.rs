//! Each `/mcp` POST writes one `mcp_call` line under the HTTP request's
//! ids, before its `http_request` line, with closed values only: never the
//! JSON-RPC id, an argument, or a tool name the facade does not recognize
//! (ADR-0042 d10).

mod support;

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::{HeldBy, Role};
use oml_app::doubles::MemStore;
use oml_app::types::{BootstrapRequest, GrantRequest, IssueKeyRequest};
use oml_transport_http::traffic::ScopeObserver;

use support::{Captured, app, state};

async fn agent() -> (Router, String) {
    let store = MemStore::new();
    let app = app(&store).with_observer(Arc::new(ScopeObserver));
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let grants = [Role::Writer, Role::Reader].map(|role| GrantRequest {
        node: boot.root_workarea_id.into(),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    });
    let key = app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                role: Role::Writer,
                name: Name::parse("agent-7").expect("a name"),
                principal: "crew/agent-7".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: Some(boot.root_workarea_id.into()),
                crew: None,
                grants: grants.to_vec(),
                connect_code: false,
                user: None,
            },
        )
        .await
        .expect("a key");
    (state(&store, app).into_router(), key.value)
}

/// One POST of `message`; the lines it wrote.
async fn call(router: &Router, key: &str, message: &Value) -> Vec<Value> {
    let (guard, captured) = Captured::start();
    let response = router
        .clone()
        .oneshot(
            Request::post("/mcp")
                .header(header::AUTHORIZATION, format!("Bearer {key}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-request-id", "mcp-req-1")
                .body(Body::from(message.to_string()))
                .expect("a request"),
        )
        .await
        .expect("the router answers");
    to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    drop(guard);
    let lines: Vec<Value> = captured
        .lines()
        .into_iter()
        .filter(|l| l["event"] == "mcp_call" || l["event"] == "http_request")
        .collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0]["event"], "mcp_call", "the call's line comes first");
    assert_eq!(lines[1]["event"], "http_request");
    assert_eq!(lines[0]["request_id"], "mcp-req-1");
    assert_eq!(lines[0]["correlation_id"], lines[1]["correlation_id"]);
    assert!(lines[0]["duration_ms"].is_number());
    lines
}

fn text_of(lines: &[Value]) -> String {
    lines.iter().map(Value::to_string).collect()
}

#[tokio::test]
async fn initialize_logs_the_client_info_cut_and_replaced_and_its_rpc_method() {
    let (router, key) = agent().await;
    let lines = call(
        &router,
        &key,
        &json!({"jsonrpc": "2.0", "id": "rpc-id-zz", "method": "initialize",
                "params": {"clientInfo": {"name": "claude-code\u{7}", "version": "2.1.0"}}}),
    )
    .await;
    let line = &lines[0];
    assert_eq!(line["rpc_method"], "initialize");
    assert_eq!(line["outcome"], "result");
    assert_eq!(line["client_name"], "claude-code?");
    assert_eq!(line["client_version"], "2.1.0");
    assert!(line["tenant_id"].is_string() && line["key_id"].is_string());
    assert!(
        !text_of(&lines).contains("rpc-id-zz"),
        "the JSON-RPC id is never logged"
    );
}

#[tokio::test]
async fn a_tool_call_logs_its_tool_and_mode_and_never_its_arguments() {
    let (router, key) = agent().await;
    let lines = call(
        &router,
        &key,
        &json!({"jsonrpc": "2.0", "id": 424242, "method": "tools/call",
                "params": {"name": "oml_read", "arguments": {"query": "zanzibarword"}}}),
    )
    .await;
    let line = &lines[0];
    assert_eq!(line["rpc_method"], "tools/call");
    assert_eq!(line["tool"], "oml_read");
    assert_eq!(line["mode"], "recall");
    assert_eq!(line["outcome"], "result");
    assert!(line.get("alias_of").is_none() && line.get("client_name").is_none());
    let text = text_of(&lines);
    assert!(
        !text.contains("zanzibarword") && !text.contains("424242"),
        "{text}"
    );
}

#[tokio::test]
async fn a_refused_tool_call_logs_tool_error_and_the_envelope_code() {
    let (router, key) = agent().await;
    let lines = call(
        &router,
        &key,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "oml_write", "arguments": {"text": "x"}}}),
    )
    .await;
    let line = &lines[0];
    assert_eq!(line["tool"], "oml_write");
    assert_eq!(line["mode"], "submit");
    assert_eq!(line["outcome"], "tool_error");
    assert_eq!(line["error_code"], "request.validation");
}

#[tokio::test]
async fn an_unrecognized_tool_name_logs_unknown_and_an_alias_logs_its_successor() {
    let (router, key) = agent().await;
    let lines = call(
        &router,
        &key,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "drop_tables_now", "arguments": {}}}),
    )
    .await;
    assert_eq!(lines[0]["tool"], "unknown");
    assert_eq!(lines[0]["outcome"], "rpc_error");
    assert_eq!(lines[0]["error_code"], "-32602");
    assert!(!text_of(&lines).contains("drop_tables_now"));

    let lines = call(
        &router,
        &key,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "oml_recall", "arguments": {"query": "x"}}}),
    )
    .await;
    assert_eq!(lines[0]["tool"], "oml_recall");
    assert_eq!(lines[0]["alias_of"], "oml_read");
    assert!(lines[0].get("mode").is_none());

    let lines = call(
        &router,
        &key,
        &json!({"jsonrpc": "2.0", "id": 4, "method": "resources/evil"}),
    )
    .await;
    assert_eq!(lines[0]["rpc_method"], "unknown");
    assert_eq!(lines[0]["error_code"], "-32601");
    assert!(!text_of(&lines).contains("resources/evil"));
}
