//! Guardrails on the wire (ADR-0037 d4, d5, d10, d14; build steps
//! S1 and S2): kind `guardrail` with its `severity` and `rule` on
//! `POST /v1/memories` and its supersede, refused on MCP's `oml_write`; the
//! key's `guardrail_scope` on `POST /v2/keys`, `GET /v2/keys/{key}` and
//! `GET /v2/whoami`; and the set on `GET /v1/guardrails`. The lead's router
//! over in-memory doubles, as `mcp_alias_log.rs` builds it.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt as _;

const ENROLLMENT: &str = "enr_test_fixture_token";

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

/// One request; the status and the JSON body.
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

/// One `tools/call` to `/mcp`; the tool result.
async fn call_tool(router: &Router, token: &str, name: &str, arguments: Value) -> Value {
    let (status, body) = send(
        router,
        "POST",
        "/mcp",
        Some(token),
        None,
        Some(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                     "params": {"name": name, "arguments": arguments}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{name}: {body}");
    body["result"].clone()
}

/// The `details.field` of a refused tool call's error envelope.
fn tool_refusal_field(result: &Value) -> (String, String) {
    assert_eq!(result["isError"], true, "{result}");
    let envelope = &result["structuredContent"]["error"];
    (
        envelope["code"].as_str().unwrap_or_default().to_owned(),
        envelope["details"]["field"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    )
}

struct Tenant {
    admin: String,
    crew: String,
    writer: String,
    reviewer: String,
}

/// Bootstrap, one workspace `metals-crew`, a writer and a reviewer on it.
async fn tenant(router: &Router) -> Tenant {
    let (status, boot) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(&json!({"enrollment_token": ENROLLMENT, "name": "root-admin", "tenant_name": "gr-tenant"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{boot}");
    let admin = boot["value"].as_str().expect("admin key").to_owned();
    let root = boot["root_workspace_id"].as_str().expect("root").to_owned();
    let (status, wa) = send(
        router,
        "POST",
        "/v2/workspaces",
        Some(&admin),
        Some("wa-1"),
        Some(&json!({"parent": root, "name": "metals-crew"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{wa}");
    let crew = wa["id"].as_str().expect("workspace id").to_owned();
    let writer = issue(
        router,
        &admin,
        json!({"role": "writer", "name": "writer", "principal": "crew/agent-7",
               "grants": [{"workspace": crew, "role": "writer"}, {"workspace": crew, "role": "reader"}]}),
    )
    .await["value"]
        .as_str()
        .expect("writer value")
        .to_owned();
    let reviewer = issue(
        router,
        &admin,
        // A person's: only a person's verdict approves a guardrail
        // (ADR-0080 d16).
        json!({"role": "reviewer", "name": "reviewer", "principal": "crew/reviewer", "used_by": "person",
               "grants": [{"workspace": crew, "role": "reviewer"}, {"workspace": crew, "role": "reader"}]}),
    )
    .await["value"]
        .as_str()
        .expect("reviewer value")
        .to_owned();
    Tenant {
        admin,
        crew,
        writer,
        reviewer,
    }
}

async fn issue(router: &Router, admin: &str, body: Value) -> Value {
    let idem = format!("issue-{}", body["name"].as_str().unwrap_or("key"));
    let (status, key) = send(
        router,
        "POST",
        "/v2/keys",
        Some(admin),
        Some(&idem),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");
    key
}

fn deny_push() -> Value {
    json!({"rule": "oml-gr/1", "tool": "shell", "command": ["git", "push", "*"], "effect": "deny"})
}

/// Submit a guardrail on the crew workspace; its `artifact_id`.
async fn submit_guardrail(router: &Router, t: &Tenant, idem: &str, body: Value) -> String {
    let (status, receipt) = send(
        router,
        "POST",
        "/v1/memories",
        Some(&t.writer),
        Some(idem),
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    receipt["artifact_id"]
        .as_str()
        .expect("artifact_id")
        .to_owned()
}

#[tokio::test]
async fn an_agent_reviewers_approval_of_a_guardrail_is_refused_person_required_and_it_stays_held() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let agent = issue(
        &router,
        &t.admin,
        json!({"role": "reviewer", "name": "crew-reviewer", "principal": "crew/agent-reviewer", "used_by": "agent",
               "grants": [{"workspace": t.crew, "role": "reviewer"}, {"workspace": t.crew, "role": "reader"}]}),
    )
    .await["value"]
        .as_str()
        .expect("agent reviewer value")
        .to_owned();
    let rule = submit_guardrail(
        &router,
        &t,
        "g-agent",
        json!({"kind": "guardrail", "severity": "never", "body": "Never push to main.",
               "workspace": t.crew}),
    )
    .await;
    // Review next never serves it this key: it is in the person lane
    // (ADR-0074 d19; ADR-0080 d16).
    let (status, served) = send(
        &router,
        "POST",
        "/v2/review/next",
        Some(&agent),
        Some("n-agent"),
        Some(&json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{served}");
    // A verdict that names it anyway is refused at the verdict.
    let serve_id = format!(
        "srv_{}",
        uuid::Uuid::parse_str(&rule).expect("an id").simple()
    );
    let (status, refused) = send(
        &router,
        "POST",
        "/v2/review/verdicts",
        Some(&agent),
        Some("v-agent"),
        Some(&json!({"serve_id": serve_id, "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert_eq!(
        refused["error"]["code"], "review.person_required",
        "{refused}"
    );
    assert_eq!(
        refused["error"]["details"],
        json!({"reason": "instruction_needs_person", "kind": "guardrail"}),
        "{refused}"
    );
    // Held still, so no rule is in force yet.
    let (status, set) = send(
        &router,
        "GET",
        "/v1/guardrails",
        Some(&t.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["guardrails"], json!([]), "{set}");
}

#[tokio::test]
async fn review_next_with_needs_person_serves_the_lane_and_the_listing_counts_it_per_author() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let rule = submit_guardrail(
        &router,
        &t,
        "g-lane",
        json!({"kind": "guardrail", "severity": "never", "body": "Never push to main.",
               "workspace": t.crew}),
    )
    .await;
    submit_guardrail(
        &router,
        &t,
        "m-lane",
        json!({"kind": "memory", "body": "Quotes drift.", "workspace": t.crew}),
    )
    .await;
    let (status, page) = send(
        &router,
        "GET",
        "/v2/review/held?needs=person&limit=50",
        Some(&t.reviewer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["items"].as_array().map(Vec::len), Some(1), "{page}");
    assert_eq!(page["items"][0]["artifact_id"], rule.as_str());
    assert_eq!(page["lane_authors"][0]["held"], 1, "{page}");
    let (status, served) = send(
        &router,
        "POST",
        "/v2/review/next",
        Some(&t.reviewer),
        Some("n-lane"),
        Some(&json!({"needs": "person"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    assert_eq!(served["item"]["artifact_id"], rule.as_str());
    let (status, refused) = send(
        &router,
        "POST",
        "/v2/review/next",
        Some(&t.reviewer),
        Some("n-bad"),
        Some(&json!({"needs": "admin"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["error"]["details"]["field"], "/needs", "{refused}");
}

/// The reviewer takes the next item and approves it; the served item.
async fn approve_next(router: &Router, t: &Tenant, n: u32) -> Value {
    let (status, served) = send(
        router,
        "POST",
        "/v2/review/next",
        Some(&t.reviewer),
        Some(&format!("n-{n}")),
        Some(&json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    let (status, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Some(&t.reviewer),
        Some(&format!("v-{n}")),
        Some(&json!({"serve_id": served["serve_id"], "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    assert_eq!(verdict["state"], "promoted", "{verdict}");
    served
}

#[tokio::test]
async fn post_v1_memories_takes_kind_guardrail_with_its_severity_and_rule() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let id = submit_guardrail(
        &router,
        &t,
        "g-1",
        json!({"kind": "guardrail", "severity": "never", "rule": deny_push(),
               "body": "Never push to main.", "workspace": t.crew}),
    )
    .await;
    let (status, memory) = send(
        &router,
        "GET",
        &format!("/v1/memories/{id}"),
        Some(&t.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{memory}");
    assert_eq!(memory["kind"], "guardrail");
    assert_eq!(memory["state"], "held");
    assert_eq!(memory["severity"], "never");
    assert_eq!(memory["rule"], deny_push());
}

#[tokio::test]
async fn a_guardrail_without_a_severity_is_refused_naming_severity() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let (status, body) = send(
        &router,
        "POST",
        "/v1/memories",
        Some(&t.writer),
        Some("g-1"),
        Some(&json!({"kind": "guardrail", "body": "Never push to main.", "workspace": t.crew})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "request.validation", "{body}");
    assert_eq!(body["error"]["details"]["field"], "/severity", "{body}");
}

#[tokio::test]
async fn severity_and_rule_are_refused_on_any_other_kind_naming_the_field() {
    let router = doubles_router();
    let t = tenant(&router).await;
    for (n, (field, value)) in [("severity", json!("must")), ("rule", deny_push())]
        .into_iter()
        .enumerate()
    {
        let mut body = json!({"kind": "memory", "body": "a fact", "workspace": t.crew});
        body[field] = value;
        let (status, refusal) = send(
            &router,
            "POST",
            "/v1/memories",
            Some(&t.writer),
            Some(&format!("m-{n}")),
            Some(&body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refusal}");
        assert_eq!(
            refusal["error"]["details"]["field"],
            format!("/{field}"),
            "{refusal}"
        );
    }
}

#[tokio::test]
async fn an_unknown_severity_is_refused_naming_severity() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let (status, body) = send(
        &router,
        "POST",
        "/v1/memories",
        Some(&t.writer),
        Some("g-1"),
        Some(&json!({"kind": "guardrail", "severity": "may", "body": "Push freely.", "workspace": t.crew})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["details"]["field"], "/severity", "{body}");
}

#[tokio::test]
async fn oml_write_refuses_kind_guardrail_and_a_guardrail_target() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let result = call_tool(
        &router,
        &t.writer,
        "oml_write",
        json!({"text": "Never push to main.", "kind": "guardrail", "idempotency_key": "w-1"}),
    )
    .await;
    assert_eq!(
        tool_refusal_field(&result),
        ("request.validation".to_owned(), "/kind".to_owned())
    );

    let target = submit_guardrail(
        &router,
        &t,
        "g-1",
        json!({"kind": "guardrail", "severity": "must", "body": "Run the tests.", "workspace": t.crew}),
    )
    .await;
    approve_next(&router, &t, 1).await;
    let result = call_tool(
        &router,
        &t.writer,
        "oml_write",
        json!({ "expected_version": 1,"text": "Run the tests twice.", "replaces": target, "idempotency_key": "w-2"}),
    )
    .await;
    assert_eq!(
        tool_refusal_field(&result),
        ("request.validation".to_owned(), "/replaces".to_owned())
    );
}

#[tokio::test]
async fn oml_write_refuses_the_skill_document_and_test_kinds_naming_kind() {
    let router = doubles_router();
    let t = tenant(&router).await;
    for kind in ["skill", "document", "test"] {
        let result = call_tool(
            &router,
            &t.writer,
            "oml_write",
            json!({"text": "Run it.", "kind": kind, "idempotency_key": format!("w-{kind}")}),
        )
        .await;
        assert_eq!(
            tool_refusal_field(&result),
            ("request.validation".to_owned(), "/kind".to_owned()),
            "{kind}"
        );
    }
}

#[tokio::test]
async fn oml_write_with_replaces_naming_a_document_is_refused_naming_kind() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let document = submit_guardrail(
        &router,
        &t,
        "d-1",
        json!({"kind": "document", "body": "# Runbook\nStep one.", "workspace": t.crew}),
    )
    .await;
    approve_next(&router, &t, 1).await;
    let result = call_tool(
        &router,
        &t.writer,
        "oml_write",
        json!({"expected_version": 1, "text": "# Runbook\nStep two.", "replaces": document,
               "idempotency_key": "w-doc"}),
    )
    .await;
    assert_eq!(
        tool_refusal_field(&result),
        ("request.validation".to_owned(), "/kind".to_owned())
    );
}

#[tokio::test]
async fn oml_write_retiring_a_skill_is_refused_naming_kind() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let skill = submit_guardrail(
        &router,
        &t,
        "k-1",
        json!({"kind": "skill", "skill": {"name": "deploy", "description": "Deploy it."},
               "body": "Run the deploy script.", "workspace": t.crew}),
    )
    .await;
    approve_next(&router, &t, 1).await;
    let result = call_tool(
        &router,
        &t.writer,
        "oml_write",
        json!({"expected_version": 1, "text": "No longer used.", "replaces": skill,
               "retirement": true, "idempotency_key": "w-retire"}),
    )
    .await;
    assert_eq!(
        tool_refusal_field(&result),
        ("request.validation".to_owned(), "/kind".to_owned())
    );
}

#[tokio::test]
async fn post_v1_memories_takes_kind_skill_with_its_object_and_serves_it_back() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let id = submit_guardrail(
        &router,
        &t,
        "s-1",
        json!({"kind": "skill", "skill": {"name": "deploy", "description": "Deploy the service."},
               "body": "Run the deploy script.", "workspace": t.crew}),
    )
    .await;
    let (status, memory) = send(
        &router,
        "GET",
        &format!("/v1/memories/{id}"),
        Some(&t.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{memory}");
    assert_eq!(memory["kind"], "skill");
    assert_eq!(memory["claim_key"], "skill/deploy");
    assert_eq!(memory["format"], "text/markdown");
    assert_eq!(
        memory["skill"],
        json!({"name": "deploy", "description": "Deploy the service."})
    );
    // Two objects at once, an unknown member and a skill object on another
    // kind are each refused naming the field.
    for (idem, body, field) in [
        (
            "s-2",
            json!({"kind": "skill", "skill": {"name": "a", "description": "b"},
                   "document": {"title": "c"}, "body": "x", "workspace": t.crew}),
            "/document",
        ),
        (
            "s-3",
            json!({"kind": "skill", "skill": {"name": "a", "description": "b", "extra": 1},
                   "body": "x", "workspace": t.crew}),
            "/skill",
        ),
        (
            "s-4",
            json!({"kind": "memory", "skill": {"name": "a", "description": "b"},
                   "body": "x", "workspace": t.crew}),
            "/skill",
        ),
    ] {
        let (status, refused) = send(
            &router,
            "POST",
            "/v1/memories",
            Some(&t.writer),
            Some(idem),
            Some(&body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_eq!(refused["error"]["details"]["field"], field, "{refused}");
    }
}

#[tokio::test]
async fn the_serve_of_a_held_guardrail_carries_the_load_warning() {
    let router = doubles_router();
    let t = tenant(&router).await;
    submit_guardrail(
        &router,
        &t,
        "g-1",
        json!({"kind": "guardrail", "severity": "should", "body": "Prefer small commits.", "workspace": t.crew}),
    )
    .await;
    let served = approve_next(&router, &t, 1).await;
    let warning = served["item"]["load_warning"]
        .as_str()
        .unwrap_or_else(|| panic!("no load_warning in {served}"));
    assert!(warning.contains("will load this rule"), "{warning}");
}

#[tokio::test]
async fn get_v1_guardrails_serves_the_promoted_set_under_the_keys_scope() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let id = submit_guardrail(
        &router,
        &t,
        "g-1",
        json!({"kind": "guardrail", "severity": "never", "rule": deny_push(),
               "body": "Never push to main.", "workspace": t.crew}),
    )
    .await;
    approve_next(&router, &t, 1).await;
    let (status, set) = send(
        &router,
        "GET",
        &format!("/v1/guardrails?workspace={}", t.crew),
        Some(&t.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(set["scope_node"], t.crew, "{set}");
    // The header of the hook names the scope node by its path (d6).
    assert_eq!(set["scope_path"], "universe.metals-crew", "{set}");
    assert_eq!(set["guardrail_scope"], "ancestors", "{set}");
    assert_eq!(set["over_cap"], false, "{set}");
    assert!(
        set["set_digest"]
            .as_str()
            .is_some_and(|d| d.starts_with("sha256:")),
        "{set}"
    );
    let served = set["guardrails"].as_array().expect("guardrails");
    assert_eq!(served.len(), 1, "{set}");
    let g = &served[0];
    assert_eq!(g["artifact_id"], id.as_str());
    assert_eq!(g["severity"], "never");
    assert_eq!(g["text"], "Never push to main.");
    assert_eq!(g["rule"], deny_push());
    assert_eq!(g["workspace_path"], "universe.metals-crew");
    assert_eq!(g["conflict"], json!([]));
    let verdicts = g["verdicts"].as_array().expect("verdicts");
    assert_eq!(verdicts.len(), 1, "{g}");
    // A verdict names its reviewer's attestation and time, never its key
    // (ADR-0037 d5, ADR-0025 d15).
    assert_eq!(verdicts[0]["used_by"], "person", "{g}");
    assert!(verdicts[0].get("key_id").is_none(), "{g}");
    assert_eq!(set["own_held"], json!([]));
}

#[tokio::test]
async fn no_parameter_or_field_changes_the_guardrail_scope() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let (status, body) = send(
        &router,
        "GET",
        &format!(
            "/v1/guardrails?workspace={}&guardrail_scope=workspace",
            t.crew
        ),
        Some(&t.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "request.validation", "{body}");
    assert_eq!(
        body["error"]["details"]["field"], "guardrail_scope",
        "{body}"
    );
}

#[tokio::test]
async fn post_v2_keys_takes_guardrail_scope_and_the_key_reads_serve_it() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let issued = issue(
        &router,
        &t.admin,
        json!({"role": "reader", "name": "narrow", "principal": "crew/narrow",
               "guardrail_scope": "workspace",
               "grants": [{"workspace": t.crew, "role": "reader"}]}),
    )
    .await;
    assert_eq!(issued["guardrail_scope"], "workspace", "{issued}");
    let value = issued["value"].as_str().expect("value");
    let (status, me) = send(&router, "GET", "/v2/whoami", Some(value), None, None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["guardrail_scope"], "workspace", "{me}");
    let (status, key) = send(
        &router,
        "GET",
        "/v2/keys/narrow",
        Some(&t.admin),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{key}");
    assert_eq!(key["guardrail_scope"], "workspace", "{key}");

    // Sent none, the issuance default (d4; d18 Q1).
    let (status, me) = send(&router, "GET", "/v2/whoami", Some(&t.writer), None, None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["guardrail_scope"], "ancestors", "{me}");
}

#[tokio::test]
async fn a_guardrail_scope_other_than_workspace_or_ancestors_is_refused() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let (status, body) = send(
        &router,
        "POST",
        "/v2/keys",
        Some(&t.admin),
        Some("issue-off"),
        Some(
            &json!({"role": "reader", "name": "off", "principal": "crew/off",
                     "guardrail_scope": "off",
                     "grants": [{"workspace": t.crew, "role": "reader"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["error"]["details"]["field"], "/guardrail_scope",
        "{body}"
    );
}

#[tokio::test]
async fn recall_refuses_kinds_guardrail_naming_kinds_and_the_listing_serves_the_kind() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let id = submit_guardrail(
        &router,
        &t,
        "g-1",
        json!({"kind": "guardrail", "severity": "must", "body": "Run the tests.", "workspace": t.crew}),
    )
    .await;
    let (status, body) = send(
        &router,
        "POST",
        "/v1/memories/recall",
        Some(&t.writer),
        None,
        Some(&json!({"query": "tests", "kinds": ["guardrail"], "workspace": t.crew})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["details"]["field"], "/kinds", "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("GET /v1/guardrails")),
        "{body}"
    );

    let (status, page) = send(
        &router,
        "GET",
        "/v1/memories?own=true&kind=guardrail",
        Some(&t.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let items = page["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{page}");
    assert_eq!(items[0]["artifact_id"], id.as_str());
    assert_eq!(items[0]["severity"], "must");
}

/// One JSON-RPC request to `/mcp`; its `result`.
async fn rpc(router: &Router, token: &str, method: &str, params: Value) -> Value {
    let (status, body) = send(
        router,
        "POST",
        "/mcp",
        Some(token),
        None,
        Some(&json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{method}: {body}");
    body["result"].clone()
}

#[tokio::test]
async fn the_prompt_renders_the_hook_text() {
    let router = doubles_router();
    let t = tenant(&router).await;
    submit_guardrail(
        &router,
        &t,
        "g-1",
        json!({"kind": "guardrail", "severity": "never", "rule": deny_push(),
               "body": "Never push to main.", "workspace": t.crew}),
    )
    .await;
    approve_next(&router, &t, 1).await;

    let init = rpc(&router, &t.writer, "initialize", json!({})).await;
    assert_eq!(
        init["capabilities"]["prompts"],
        json!({"listChanged": false}),
        "{init}"
    );
    let listed = rpc(&router, &t.writer, "prompts/list", json!({})).await;
    let names: Vec<&str> = listed["prompts"]
        .as_array()
        .expect("prompts")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert_eq!(names, ["guardrails"], "{listed}");

    let got = rpc(
        &router,
        &t.writer,
        "prompts/get",
        json!({"name": "guardrails"}),
    )
    .await;
    let text = got["messages"][0]["content"]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("a text message: {got}"));
    let (_, set) = send(
        &router,
        "GET",
        "/v1/guardrails",
        Some(&t.writer),
        None,
        None,
    )
    .await;
    let frame = oml_guardrail::render_frame(
        &set,
        &oml_guardrail::FrameOptions {
            origin: "this server",
            checked: false,
            whole_over_cap: false,
        },
    )
    .expect("the set renders");
    assert_eq!(text, frame.text);
    assert_eq!(got["messages"][0]["role"], "user");
}

/// The author key reads its own held guardrail with `include_own_held`:
/// served as a rule, `unreviewed` with `effect_inside: ask` and a null
/// `promoted_at`, beside its signed `deny`; without it, listed by key and
/// version alone; another key never gets it outside an in-progress
/// workspace (ADR-0080 d10, d14).
#[tokio::test]
async fn the_author_key_reads_its_own_held_guardrail_with_include_own_held() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let id = submit_guardrail(
        &router,
        &t,
        "g-own",
        json!({"kind": "guardrail", "severity": "never", "rule": deny_push(),
               "body": "Never push to main.", "workspace": t.crew}),
    )
    .await;
    let read = |key: String, own: &'static str| {
        let router = router.clone();
        let crew = t.crew.clone();
        async move {
            let (status, set) = send(
                &router,
                "GET",
                &format!("/v1/guardrails?workspace={crew}&include_own_held={own}"),
                Some(&key),
                None,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{set}");
            set
        }
    };
    let without = read(t.writer.clone(), "false").await;
    assert_eq!(without["guardrails"], json!([]), "{without}");
    assert_eq!(without["own_held"][0]["version"], 1, "{without}");
    let with = read(t.writer.clone(), "true").await;
    let served = with["guardrails"].as_array().expect("guardrails");
    assert_eq!(served.len(), 1, "{with}");
    let g = &served[0];
    assert_eq!(g["artifact_id"], id.as_str());
    assert_eq!(g["unreviewed"], true, "{g}");
    assert_eq!(g["effect_inside"], "ask", "{g}");
    assert_eq!(
        g["rule"]["effect"], "deny",
        "the stored rule is unchanged: {g}"
    );
    assert_eq!(g["promoted_at"], Value::Null, "{g}");
    let other = read(t.reviewer.clone(), "true").await;
    assert_eq!(other["guardrails"], json!([]), "{other}");
    let (status, body) = send(
        &router,
        "GET",
        &format!("/v1/guardrails?workspace={}&include_own_held=yes", t.crew),
        Some(&t.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["error"]["details"]["field"], "include_own_held",
        "{body}"
    );
}
