//! `oml_write`'s structure modes over MCP (ADR-0095 d1, d3, d5, d6, d8, d9):
//! a contributor key creates a child inside its own person's root, and no
//! other key creates; a reviewer proposes a status move and a contributor a
//! brief member, each ratified by a different identity off MCP; one mode a
//! call; the catalog keeps three tools with `oml_workspace` read-only; and
//! the server instructions stay under Claude Code's cut. The lead's router
//! over in-memory doubles, as `guardrails_http.rs` builds it. Each test is
//! named after the sentence it proves.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::doubles::MemStore;
use oml_app::ports::JournalStore as _;
use oml_transport_http::mcp::{MAX_INSTRUCTIONS_CHARS, SERVER_INSTRUCTIONS};

const ENROLLMENT: &str = "enr_test_fixture_token";

fn doubles_router() -> (Router, MemStore) {
    use oml_app::App;
    use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, SeqIdGen};
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new([7; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let ping: oml_transport_http::DbPing = std::sync::Arc::new(|| Box::pin(async { true }));
    let router = oml_transport_http::ApiState::new(
        std::sync::Arc::new(app),
        store.clone(),
        ping,
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    (router, store)
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

/// One JSON-RPC request to `/mcp`; the message.
async fn rpc(router: &Router, token: &str, method: &str, params: Value) -> Value {
    let (status, body) = send(
        router,
        "POST",
        "/mcp",
        Some(token),
        None,
        Some(&json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{method}: {body}");
    body
}

/// One `oml_write` call; the tool result.
async fn write(router: &Router, token: &str, arguments: Value) -> Value {
    rpc(
        router,
        token,
        "tools/call",
        json!({"name": "oml_write", "arguments": arguments}),
    )
    .await["result"]
        .clone()
}

/// A successful tool result's output.
fn output(result: &Value) -> &Value {
    assert_eq!(result["isError"], false, "{result}");
    &result["structuredContent"]
}

/// A refused tool result's code and `details`.
fn refused(result: &Value) -> (String, Value) {
    assert_eq!(result["isError"], true, "{result}");
    let envelope = &result["structuredContent"]["error"];
    (
        envelope["code"].as_str().unwrap_or_default().to_owned(),
        envelope["details"].clone(),
    )
}

struct Tenant {
    tenant_id: uuid::Uuid,
    admin: String,
    crew: String,
    /// A tenant contributor key on the crew, naming no user.
    writer: String,
    /// An agent's reviewer key on the crew.
    curator: String,
    /// A person's reviewer key on the crew.
    person: String,
    /// `ada`'s agent: a contributor key naming her, submitter and reader on
    /// her root.
    ada_agent: String,
    ada_root: String,
    /// `ada`'s own reviewer key, used by her, on her root.
    ada_review: String,
    /// `ada`'s submitter key: it writes without recall.
    ada_submitter: String,
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

fn value(key: &Value) -> String {
    key["value"].as_str().expect("a key value").to_owned()
}

/// Bootstrap, the shared workspace `crew` with its keys, and the person
/// `ada` with her keys.
async fn tenant(router: &Router) -> Tenant {
    let (status, boot) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(&json!({"enrollment_token": ENROLLMENT, "person": "founder", "email": "founder@example.com", "name": "root-admin", "tenant_name": "ws-tenant"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{boot}");
    let admin = value(&boot);
    let tenant_id =
        uuid::Uuid::parse_str(boot["tenant_id"].as_str().expect("a tenant id")).expect("a uuid");
    let root = boot["root_workspace_id"].as_str().expect("root").to_owned();
    let (status, crew) = send(
        router,
        "POST",
        "/v2/workspaces",
        Some(&admin),
        Some("wa-crew"),
        Some(&json!({"parent": root, "name": "crew"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{crew}");
    let crew = crew["id"].as_str().expect("an id").to_owned();
    let both = |role: &str| json!([{"workspace": crew, "role": role}, {"workspace": crew, "role": "reader"}]);
    let writer = value(
        &issue(
            router,
            &admin,
            json!({"role": "contributor", "name": "crew-writer", "principal": "crew/writer",
                   "grants": both("submitter")}),
        )
        .await,
    );
    let curator = value(
        &issue(
            router,
            &admin,
            json!({"role": "reviewer", "name": "crew-curator", "principal": "crew/curator",
                   "used_by": "agent", "grants": both("reviewer")}),
        )
        .await,
    );
    let person = value(
        &issue(
            router,
            &admin,
            json!({"role": "reviewer", "name": "crew-person", "principal": "crew/person",
                   "used_by": "person", "grants": both("reviewer")}),
        )
        .await,
    );
    let ada_agent_key = issue(
        router,
        &admin,
        json!({"role": "contributor", "name": "ada-agent", "principal": "ada/agent",
               "used_by": "agent", "user": "ada",
               "grants": [{"workspace": "ada", "role": "submitter"}, {"workspace": "ada", "role": "reader"}]}),
    )
    .await;
    let ada_agent = value(&ada_agent_key);
    let ada_root = ada_agent_key["default_workspace_id"]
        .as_str()
        .expect("the default is ada's root")
        .to_owned();
    let ada_review = value(
        &issue(
            router,
            &admin,
            json!({"role": "reviewer", "name": "ada-review", "principal": "ada/review",
                   "used_by": "person", "user": "ada",
                   "grants": [{"workspace": "ada", "role": "reviewer"}, {"workspace": "ada", "role": "reader"}]}),
        )
        .await,
    );
    let ada_submitter = value(
        &issue(
            router,
            &admin,
            json!({"role": "submitter", "name": "ada-hook", "principal": "ada/hook",
                   "used_by": "agent", "user": "ada",
                   "grants": [{"workspace": "ada", "role": "submitter"}]}),
        )
        .await,
    );
    Tenant {
        tenant_id,
        admin,
        crew,
        writer,
        curator,
        person,
        ada_agent,
        ada_root,
        ada_review,
        ada_submitter,
    }
}

/// Each event's type and payload, in chain order.
async fn events(store: &MemStore, tenant_id: uuid::Uuid) -> Vec<(String, Value)> {
    store
        .events_range(tenant_id, 1, 10_000)
        .await
        .expect("the chain")
        .into_iter()
        .map(|e| (e.envelope.event_type.clone(), e.envelope.payload.clone()))
        .collect()
}

fn count(events: &[(String, Value)], event_type: &str) -> usize {
    events.iter().filter(|(t, _)| t == event_type).count()
}

/// The workspace read as `key` makes it: status and JSON body.
async fn workspace(router: &Router, key: &str, node: &str) -> (StatusCode, Value) {
    send(
        router,
        "GET",
        &format!("/v2/workspaces/{node}"),
        Some(key),
        None,
        None,
    )
    .await
}

#[tokio::test]
async fn a_contributor_key_creates_a_child_inside_its_own_root_over_mcp() {
    let (router, store) = doubles_router();
    let t = tenant(&router).await;
    let result = write(
        &router,
        &t.ada_agent,
        json!({"create_workspace": "notes", "reason": "a separate task", "idempotency_key": "c-1"}),
    )
    .await;
    let out = output(&result);
    assert_eq!(out["mode"], "create_workspace", "{out}");
    assert_eq!(out["name"], "notes");
    assert_eq!(
        out["parent"],
        json!(t.ada_root),
        "the key's default, her root"
    );
    assert_eq!(out["path"], "universe.ada.notes");
    assert_eq!(out["status"], "development");
    assert_eq!(out["idempotent_replay"], false);
    let chain = events(&store, t.tenant_id).await;
    let (event_type, payload) = chain.last().expect("an event");
    assert_eq!(event_type, "workspace.create");
    assert_eq!(payload["name"], "notes");
    assert_eq!(payload["reason"], "a separate task");
    // And a grandchild under it, by name.
    let result = write(
        &router,
        &t.ada_agent,
        json!({"create_workspace": "drafts", "workspace": "notes", "idempotency_key": "c-2"}),
    )
    .await;
    assert_eq!(output(&result)["path"], "universe.ada.notes.drafts");
}

#[tokio::test]
async fn create_workspace_over_mcp_refuses_an_admin_key_a_tenant_key_and_a_submitter_key_naming_create_is_own_root_only()
 {
    let (router, store) = doubles_router();
    let t = tenant(&router).await;
    let before = count(&events(&store, t.tenant_id).await, "workspace.create");
    for (who, key) in [
        ("admin", &t.admin),
        ("tenant contributor", &t.writer),
        ("submitter", &t.ada_submitter),
        ("person's reviewer", &t.ada_review),
        ("agent reviewer", &t.curator),
    ] {
        let result = write(
            &router,
            key,
            json!({"create_workspace": "side", "workspace": t.crew, "idempotency_key": format!("c-{who}")}),
        )
        .await;
        let (code, details) = refused(&result);
        assert_eq!(code, "access.verb_ceiling", "{who}: {details}");
        assert_eq!(details["reason"], "create_is_own_root_only", "{who}");
    }
    assert_eq!(
        count(&events(&store, t.tenant_id).await, "workspace.create"),
        before,
        "nothing is created"
    );
    // The admin key keeps its create on HTTP.
    let (status, created) = send(
        &router,
        "POST",
        "/v2/workspaces",
        Some(&t.admin),
        Some("wa-side"),
        Some(&json!({"parent": t.crew, "name": "side"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
}

#[tokio::test]
async fn a_child_created_over_mcp_is_development_standard_and_carries_no_grant() {
    let (router, store) = doubles_router();
    let t = tenant(&router).await;
    let grants_before = count(&events(&store, t.tenant_id).await, "grant.issued");
    let created = write(
        &router,
        &t.ada_agent,
        json!({"create_workspace": "notes", "idempotency_key": "c-1"}),
    )
    .await;
    let child = output(&created)["id"]
        .as_str()
        .expect("the child's id")
        .to_owned();
    assert_eq!(
        count(&events(&store, t.tenant_id).await, "grant.issued"),
        grants_before,
        "a create writes no grant"
    );
    let (status, view) = workspace(&router, &t.ada_agent, &child).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        (&view["status"], &view["mode"]),
        (&json!("development"), &json!("standard"))
    );
    // The child admits exactly what its parent admits.
    let (_, parent) = workspace(&router, &t.ada_agent, &t.ada_root).await;
    assert_eq!(view["my_effective_verbs"], parent["my_effective_verbs"]);
    let (_, reviewer_view) = workspace(&router, &t.ada_review, &child).await;
    let (_, reviewer_parent) = workspace(&router, &t.ada_review, &t.ada_root).await;
    assert_eq!(
        reviewer_view["my_effective_verbs"],
        reviewer_parent["my_effective_verbs"]
    );
    // A key that reads nothing of the root reads nothing of the child.
    let (status, _) = workspace(&router, &t.writer, &child).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_child_created_inside_an_in_progress_subtree_widens_no_audience() {
    let (router, _store) = doubles_router();
    let t = tenant(&router).await;
    let proj = write(
        &router,
        &t.ada_agent,
        json!({"create_workspace": "proj", "idempotency_key": "c-proj"}),
    )
    .await;
    let proj = output(&proj)["id"].as_str().expect("an id").to_owned();
    let (status, moded) = send(
        &router,
        "POST",
        &format!("/v2/workspaces/{proj}/mode"),
        Some(&t.ada_review),
        Some("m-1"),
        Some(&json!({"mode": "in_progress"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moded}");
    // A second agent of ada's, which reads a sibling and not proj.
    let (_, side) = send(
        &router,
        "POST",
        "/v2/workspaces",
        Some(&t.admin),
        Some("wa-side"),
        Some(&json!({"parent": t.ada_root, "name": "side"})),
    )
    .await;
    let side = side["id"].as_str().expect("an id").to_owned();
    let helper = value(
        &issue(
            &router,
            &t.admin,
            json!({"role": "contributor", "name": "ada-helper", "principal": "ada/helper",
                   "used_by": "agent", "user": "ada",
                   "grants": [{"workspace": side, "role": "submitter"}, {"workspace": side, "role": "reader"}]}),
        )
        .await,
    );
    let child = write(
        &router,
        &t.ada_agent,
        json!({"create_workspace": "scratch", "workspace": proj, "idempotency_key": "c-scratch"}),
    )
    .await;
    let child = output(&child)["id"].as_str().expect("an id").to_owned();
    let (status, view) = workspace(&router, &t.ada_agent, &child).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["in_progress_root"]["id"], json!(proj), "{view}");
    assert_eq!(
        view["mode"], "standard",
        "the child is not a root of its own"
    );
    for node in [&proj, &child] {
        let (status, _) = workspace(&router, &helper, node).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{node}");
    }
    // A held row written in the child reaches no key that proj's held rows
    // do not reach.
    let held = write(
        &router,
        &t.ada_agent,
        json!({"text": "The scratch notes say the build is slow.", "workspace": child,
               "idempotency_key": "w-held"}),
    )
    .await;
    let held = output(&held)["artifact_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let (status, _) = send(
        &router,
        "GET",
        &format!("/v1/memories/{held}"),
        Some(&helper),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn create_workspace_replays_its_recorded_answer_under_the_same_idempotency_key() {
    let (router, store) = doubles_router();
    let t = tenant(&router).await;
    let args = json!({"create_workspace": "notes", "idempotency_key": "c-same"});
    let first = write(&router, &t.ada_agent, args.clone()).await;
    let again = write(&router, &t.ada_agent, args).await;
    let (first, again) = (output(&first), output(&again));
    assert_eq!(first["id"], again["id"]);
    assert_eq!(
        (&first["idempotent_replay"], &again["idempotent_replay"]),
        (&json!(false), &json!(true))
    );
    let creates = events(&store, t.tenant_id)
        .await
        .into_iter()
        .filter(|(event_type, payload)| {
            event_type == "workspace.create" && payload["name"] == "notes"
        })
        .count();
    assert_eq!(creates, 1);
}

#[tokio::test]
async fn two_mode_fields_in_one_call_are_refused_naming_the_second() {
    let (router, _store) = doubles_router();
    let t = tenant(&router).await;
    for (n, (args, field)) in [
        (
            json!({"create_workspace": "x", "propose_status": "protected"}),
            "/propose_status",
        ),
        (
            json!({"propose_status": "protected", "propose_brief": "00000000-0000-0000-0000-000000000001"}),
            "/propose_brief",
        ),
        (
            json!({"withdraw": "00000000-0000-0000-0000-000000000001", "create_workspace": "x"}),
            "/create_workspace",
        ),
        (
            json!({"create_workspace": "x", "text": "a fact"}),
            "/text",
        ),
        (
            json!({"propose_status": "protected", "kind": "memory"}),
            "/kind",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut args = args;
        args["idempotency_key"] = json!(format!("two-{n}"));
        let (code, details) = refused(&write(&router, &t.ada_agent, args).await);
        assert_eq!(
            (code.as_str(), &details["field"]),
            ("request.validation", &json!(field)),
            "case {n}"
        );
    }
}

#[tokio::test]
async fn propose_status_over_mcp_journals_workspace_status_proposed_and_ratifies_nothing() {
    let (router, store) = doubles_router();
    let t = tenant(&router).await;
    let result = write(
        &router,
        &t.curator,
        json!({"propose_status": "protected", "workspace": t.crew, "reason": "release candidate",
               "idempotency_key": "s-1"}),
    )
    .await;
    let out = output(&result);
    assert_eq!(
        (&out["mode"], &out["to"], &out["state"]),
        (
            &json!("propose_status"),
            &json!("protected"),
            &json!("pending")
        ),
        "{out}"
    );
    let chain = events(&store, t.tenant_id).await;
    let (event_type, payload) = chain.last().expect("an event");
    assert_eq!(event_type, "workspace.status_proposed");
    assert_eq!(payload["reason"], "release candidate");
    assert_eq!(count(&chain, "workspace.status_transition"), 0);
    let (_, view) = workspace(&router, &t.person, &t.crew).await;
    assert_eq!(
        view["status"], "development",
        "nothing moves until ratified"
    );
    let pending = &view["pending_transitions"][0];
    assert_eq!(pending["transition_id"], out["transition_id"]);
    assert!(pending["proposed_at"].is_string(), "{pending}");
    assert_eq!(pending["proposed_by"], Value::Null, "another person's key");
    // A second proposal of the same move is the conflict that names it.
    let (code, details) = refused(
        &write(
            &router,
            &t.person,
            json!({"propose_status": "protected", "workspace": t.crew, "idempotency_key": "s-2"}),
        )
        .await,
    );
    assert_eq!(code, "proposal.pending");
    assert_eq!(details["transition_id"], out["transition_id"]);
}

#[tokio::test]
async fn propose_status_refuses_a_contributor_key_with_access_verb_ceiling() {
    let (router, store) = doubles_router();
    let t = tenant(&router).await;
    let (code, _) = refused(
        &write(
            &router,
            &t.writer,
            json!({"propose_status": "protected", "workspace": t.crew, "idempotency_key": "s-1"}),
        )
        .await,
    );
    assert_eq!(code, "access.verb_ceiling");
    assert_eq!(
        count(
            &events(&store, t.tenant_id).await,
            "workspace.status_proposed"
        ),
        0
    );
}

#[tokio::test]
async fn propose_brief_over_mcp_by_a_contributor_journals_brief_edge_proposed() {
    let (router, store) = doubles_router();
    let t = tenant(&router).await;
    let written = write(
        &router,
        &t.writer,
        json!({"text": "Deploys go out on Tuesdays.", "workspace": t.crew, "idempotency_key": "w-1"}),
    )
    .await;
    let artifact = output(&written)["artifact_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let (status, served) = send(
        &router,
        "POST",
        "/v2/review/next",
        Some(&t.person),
        Some("n-1"),
        Some(&json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    let (status, verdict) = send(
        &router,
        "POST",
        "/v2/review/verdicts",
        Some(&t.person),
        Some("v-1"),
        Some(&json!({"serve_id": served["serve_id"], "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    let result = write(
        &router,
        &t.writer,
        json!({"propose_brief": artifact, "workspace": t.crew, "reason": "every deploy needs it",
               "idempotency_key": "b-1"}),
    )
    .await;
    let out = output(&result);
    assert_eq!(
        (&out["mode"], &out["artifact_id"], &out["state"]),
        (&json!("propose_brief"), &json!(artifact), &json!("pending")),
        "{out}"
    );
    let chain = events(&store, t.tenant_id).await;
    let (event_type, payload) = chain.last().expect("an event");
    assert_eq!(event_type, "brief_edge.proposed");
    assert_eq!(payload["reason"], "every deploy needs it");
    assert_eq!(count(&chain, "review.ratify"), 0, "ratified by nobody yet");
    // The same claim again is the conflict that names the edge.
    let (code, details) = refused(
        &write(
            &router,
            &t.curator,
            json!({"propose_brief": artifact, "workspace": t.crew, "idempotency_key": "b-2"}),
        )
        .await,
    );
    assert_eq!(code, "proposal.pending");
    assert_eq!(details["edge_id"], out["edge_id"]);
    // The listing serves when it was proposed, and names the key to no
    // other person's key.
    let (status, listed) = send(
        &router,
        "GET",
        &format!("/v2/edges?kind=brief&workspace_id={}", t.crew),
        Some(&t.person),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let item = &listed["items"][0];
    assert!(item["proposed_at"].is_string(), "{item}");
    assert_eq!(item["proposed_by"], Value::Null);
}

#[tokio::test]
async fn tools_list_serves_three_tools_and_oml_workspace_stays_read_only() {
    let (router, _store) = doubles_router();
    let t = tenant(&router).await;
    let listed = rpc(&router, &t.ada_agent, "tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().expect("tools");
    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, ["oml_write", "oml_read", "oml_workspace"]);
    let workspace_tool = &tools[2];
    assert_eq!(
        workspace_tool["annotations"]["readOnlyHint"], true,
        "{workspace_tool}"
    );
    assert_eq!(
        workspace_tool["inputSchema"]["properties"]
            .as_object()
            .expect("properties")
            .keys()
            .collect::<Vec<_>>(),
        ["workspace"]
    );
    let write_tool = &tools[0];
    assert_eq!(
        write_tool["annotations"],
        json!({"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false})
    );
    let properties = &write_tool["inputSchema"]["properties"];
    for mode in [
        "create_workspace",
        "propose_status",
        "propose_brief",
        "guardrail",
    ] {
        assert!(properties.get(mode).is_some(), "oml_write lacks {mode}");
    }
    assert!(
        properties["kind"]["enum"]
            .as_array()
            .expect("kinds")
            .contains(&json!("guardrail"))
    );
    // A call to a fourth name stays an unknown tool.
    let (status, body) = send(
        &router,
        "POST",
        "/mcp",
        Some(&t.ada_agent),
        None,
        Some(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                     "params": {"name": "oml_structure", "arguments": {}}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"]["code"], -32602, "{body}");
}

#[test]
fn the_server_instructions_stay_under_4096_characters() {
    assert!(
        SERVER_INSTRUCTIONS.chars().count() < MAX_INSTRUCTIONS_CHARS,
        "{} characters",
        SERVER_INSTRUCTIONS.chars().count()
    );
    assert_eq!(MAX_INSTRUCTIONS_CHARS, 4096);
    assert!(
        SERVER_INSTRUCTIONS.contains("creates a child workspace inside the person's own root"),
        "the instructions describe the create"
    );
    assert!(
        SERVER_INSTRUCTIONS
            .contains("A proposal changes nothing until a different identity ratifies it"),
        "the instructions say a proposal changes nothing alone"
    );
}
