//! Skills on the wire (ADR-0080 d1, d3 to d7, d10, d11, d14; #403,
//! #404): `GET /v1/skills`, a skill written over MCP's `oml_write`, and the
//! MCP `prompts` and `resources` methods that serve the offered set. The
//! lead's router over in-memory doubles, as `guardrails_http.rs` builds it.

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

/// One JSON-RPC request to `/mcp`; the whole response document.
async fn rpc_raw(router: &Router, token: &str, method: &str, params: Value) -> Value {
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
    body
}

async fn rpc(router: &Router, token: &str, method: &str, params: Value) -> Value {
    let body = rpc_raw(router, token, method, params).await;
    assert!(body.get("error").is_none(), "{method}: {body}");
    body["result"].clone()
}

/// A skill `name` on the crew workspace, written by `token`; its id.
async fn submit_skill(router: &Router, t: &Tenant, token: &str, name: &str, body: &str) -> String {
    let (status, receipt) = send(
        router,
        "POST",
        "/v1/memories",
        Some(token),
        Some(&format!("skill-{name}")),
        Some(
            &json!({"kind": "skill", "format": "text/markdown", "body": body,
                     "skill": {"name": name, "description": format!("Runs {name}.")},
                     "workspace": t.crew}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    receipt["artifact_id"]
        .as_str()
        .expect("artifact_id")
        .to_owned()
}

async fn approve_next(router: &Router, t: &Tenant, n: u32) {
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
}

fn names(listed: &Value, field: &str) -> Vec<String> {
    listed[field]
        .as_array()
        .unwrap_or_else(|| panic!("{field}: {listed}"))
        .iter()
        .filter_map(|p| p["name"].as_str().map(ToOwned::to_owned))
        .collect()
}

/// `GET /v1/skills` serves an approved skill with its command, version,
/// source, approval and resource URI and no body, and refuses a parameter
/// that could widen the scope, naming it (ADR-0080 d4, d11).
#[tokio::test]
async fn get_v1_skills_serves_the_offered_set_without_bodies_and_refuses_another_parameter() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let id = submit_skill(&router, &t, &t.writer, "release", "Cut the release.").await;
    approve_next(&router, &t, 1).await;
    let (status, set) = send(&router, "GET", "/v1/skills", Some(&t.writer), None, None).await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let skill = &set["skills"][0];
    assert_eq!(skill["artifact_id"], id.as_str(), "{set}");
    assert_eq!(skill["name"], "release");
    assert_eq!(skill["claim_key"], "skill/release");
    assert_eq!(skill["version"], 1);
    assert_eq!(skill["depth"], 0);
    assert_eq!(skill["overrides"], json!([]));
    assert_eq!(skill["unreviewed"], false);
    assert_eq!(skill["approver"]["used_by"], "person", "{set}");
    assert_eq!(skill["uri"], "oml://workspace/metals-crew/skill/release");
    assert!(
        skill.get("text").is_none() && skill.get("body").is_none(),
        "{set}"
    );
    assert_eq!(set["next_cursor"], Value::Null);
    assert!(set["set_digest"].is_string(), "{set}");
    let (status, refused) = send(
        &router,
        "GET",
        "/v1/skills?guardrail_scope=ancestors",
        Some(&t.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["error"]["details"]["field"], "guardrail_scope");
}

/// An agent writes a skill over `oml_write` with its `skill` object; it is
/// held, its own key is offered it at once marked unreviewed as a prompt,
/// and another writer is not (ADR-0080 d3, d14).
#[tokio::test]
async fn an_agent_writes_a_skill_over_oml_write_and_only_its_own_key_is_offered_it_held() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let (status, call) = send(
        &router,
        "POST",
        "/mcp",
        Some(&t.writer),
        None,
        Some(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                     "params": {"name": "oml_write", "arguments": {
                        "kind": "skill", "format": "text/markdown",
                        "text": "Run the suite, then fix the first failure.",
                        "skill": {"name": "triage", "description": "Triage a failing test."},
                        "idempotency_key": "triage/1"}}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{call}");
    let result = &call["result"];
    assert_ne!(result["isError"], true, "{result}");
    assert_eq!(result["structuredContent"]["state"], "held", "{result}");
    let listed = rpc(&router, &t.writer, "prompts/list", json!({})).await;
    assert_eq!(names(&listed, "prompts"), ["guardrails", "triage"]);
    assert!(
        listed["prompts"][1]["description"]
            .as_str()
            .is_some_and(|d| d.starts_with("Unreviewed: ")),
        "{listed}"
    );
    let other = issue(
        &router,
        &t.admin,
        json!({"role": "writer", "name": "other", "principal": "crew/agent-8",
               "grants": [{"workspace": t.crew, "role": "writer"}, {"workspace": t.crew, "role": "reader"}]}),
    )
    .await["value"]
        .as_str()
        .expect("other value")
        .to_owned();
    let listed = rpc(&router, &other, "prompts/list", json!({})).await;
    assert_eq!(names(&listed, "prompts"), ["guardrails"]);
}

/// Page one of `prompts/list` is the guardrails prompt and fifty skills;
/// the next page, by MCP's cursor, holds the rest and no guardrails prompt
/// (ADR-0080 d5; reading R5).
#[tokio::test]
async fn prompts_list_puts_the_guardrails_prompt_first_and_pages_by_fifty() {
    let router = doubles_router();
    let t = tenant(&router).await;
    for i in 0..51 {
        submit_skill(&router, &t, &t.writer, &format!("s{i:02}"), "Do it.").await;
    }
    let first = rpc(&router, &t.writer, "prompts/list", json!({})).await;
    let page = names(&first, "prompts");
    assert_eq!(page.len(), 51, "{first}");
    assert_eq!(page[0], "guardrails");
    assert_eq!(page[1], "s00");
    assert_eq!(page[50], "s49");
    let cursor = first["nextCursor"]
        .as_str()
        .expect("a next cursor")
        .to_owned();
    let second = rpc(
        &router,
        &t.writer,
        "prompts/list",
        json!({"cursor": cursor}),
    )
    .await;
    assert_eq!(names(&second, "prompts"), ["s50"]);
    assert!(second.get("nextCursor").is_none(), "{second}");
    let resources = rpc(&router, &t.writer, "resources/list", json!({})).await;
    assert_eq!(
        names(&resources, "resources").len(),
        50,
        "no guardrails resource"
    );
}

/// A name nobody wrote and another key's held skill answer one error,
/// so a prompt read discloses no row the key cannot read (ADR-0080 d6).
#[tokio::test]
async fn prompts_get_of_a_skill_not_offered_here_answers_the_same_error_as_an_unknown_name() {
    let router = doubles_router();
    let t = tenant(&router).await;
    submit_skill(&router, &t, &t.writer, "secret", "Hidden steps.").await;
    let other = issue(
        &router,
        &t.admin,
        json!({"role": "writer", "name": "other", "principal": "crew/agent-8",
               "grants": [{"workspace": t.crew, "role": "writer"}, {"workspace": t.crew, "role": "reader"}]}),
    )
    .await["value"]
        .as_str()
        .expect("other value")
        .to_owned();
    let held = rpc_raw(&router, &other, "prompts/get", json!({"name": "secret"})).await;
    let unknown = rpc_raw(&router, &other, "prompts/get", json!({"name": "nobody"})).await;
    assert_eq!(held["error"]["code"], -32602, "{held}");
    assert_eq!(held["error"]["message"], "no skill secret is offered here");
    assert_eq!(unknown["error"]["code"], -32602, "{unknown}");
    assert_eq!(
        unknown["error"]["message"],
        "no skill nobody is offered here"
    );
    let read = rpc_raw(
        &router,
        &other,
        "resources/read",
        json!({"uri": "oml://workspace/metals-crew/skill/secret"}),
    )
    .await;
    let missing = rpc_raw(
        &router,
        &other,
        "resources/read",
        json!({"uri": "oml://workspace/metals-crew/skill/nobody"}),
    )
    .await;
    assert_eq!(read["error"]["code"], -32002, "{read}");
    assert_eq!(read["error"]["message"], missing["error"]["message"]);
}

/// A held skill's prompt opens "UNREVIEWED"; an approved one names its
/// workspace, version and reviewer kind; the input follows the body
/// (ADR-0080 d6, d10; reading R6).
#[tokio::test]
async fn a_held_skill_prompt_header_begins_unreviewed() {
    let router = doubles_router();
    let t = tenant(&router).await;
    submit_skill(&router, &t, &t.writer, "draft", "Draft steps.").await;
    let got = rpc(
        &router,
        &t.writer,
        "prompts/get",
        json!({"name": "draft", "arguments": {"input": "the flaky test"}}),
    )
    .await;
    assert_eq!(got["messages"][0]["role"], "user");
    assert_eq!(
        got["messages"][0]["content"]["text"],
        "UNREVIEWED: not yet approved by a reviewer. OML skill `draft` from `universe.metals-crew`, version 1.\n\nDraft steps.\n\nInput:\nthe flaky test",
        "{got}"
    );
    approve_next(&router, &t, 1).await;
    let got = rpc(&router, &t.writer, "prompts/get", json!({"name": "draft"})).await;
    let text = got["messages"][0]["content"]["text"]
        .as_str()
        .expect("text");
    assert!(
        text.starts_with("OML skill `draft` from `universe.metals-crew`, version 1, approved ")
            && text.contains(" by a person reviewer.\n\nDraft steps."),
        "{text}"
    );
}

/// `resources/read` serves a skill's header and body as one markdown text,
/// its provenance in `_meta.oml`; the templates name the skill URI; the
/// capability is declared (ADR-0080 d7; ADR-0081 d6; reading R10).
#[tokio::test]
async fn resources_read_serves_a_skill_body_byte_for_byte_with_provenance_in_meta() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let body = "Step one.\n\n- keep `this`  exact\n";
    let id = submit_skill(&router, &t, &t.writer, "exact", body).await;
    approve_next(&router, &t, 1).await;
    let init = rpc(&router, &t.writer, "initialize", json!({})).await;
    assert_eq!(init["capabilities"]["resources"], json!({}), "{init}");
    let listed = rpc(&router, &t.writer, "resources/list", json!({})).await;
    let entry = &listed["resources"][0];
    assert_eq!(
        entry["uri"], "oml://workspace/metals-crew/skill/exact",
        "{listed}"
    );
    assert_eq!(entry["mimeType"], "text/markdown");
    let read = rpc(
        &router,
        &t.writer,
        "resources/read",
        json!({"uri": "oml://workspace/metals-crew/skill/exact"}),
    )
    .await;
    let content = &read["contents"][0];
    let text = content["text"].as_str().expect("text");
    assert!(text.ends_with(&format!("\n\n{body}")), "{text:?}");
    assert_eq!(content["_meta"]["oml"]["artifact_id"], id.as_str());
    assert_eq!(content["_meta"]["oml"]["unreviewed"], false);
    assert_eq!(content["_meta"]["oml"]["version"], 1);
    let templates = rpc(&router, &t.writer, "resources/templates/list", json!({})).await;
    assert_eq!(
        templates["resourceTemplates"][1]["uriTemplate"],
        "oml://workspace/{workspace}/skill/{name}"
    );
}

/// Documents are listed before the skills in `resources/list`, each with
/// its URI, title, format, size and provenance as data; `resources/read`
/// serves one byte for byte, or one passage's lines with `?passage=`, its
/// provenance in `_meta.oml`; a passage the document does not hold answers
/// `-32002`; and the document template is served first (ADR-0081 d6; the
/// build's reading R9).
#[tokio::test]
async fn documents_are_listed_first_and_read_whole_or_by_passage_as_resources() {
    let router = doubles_router();
    let t = tenant(&router).await;
    submit_skill(&router, &t, &t.writer, "deploy", "Run the deploy.").await;
    approve_next(&router, &t, 1).await;
    let body = "# Runbook\n\nWhat this covers.\n\n## Restart\n\nRestart the lead.\n";
    let wrote = rpc(
        &router,
        &t.writer,
        "tools/call",
        json!({"name": "oml_write", "arguments": {
            "text": body, "kind": "document", "idempotency_key": "doc-1",
            "document": {"title": "Runbook"}, "workspace": t.crew}}),
    )
    .await;
    assert_eq!(wrote["isError"], false, "{wrote}");
    approve_next(&router, &t, 2).await;
    let listed = rpc(&router, &t.writer, "resources/list", json!({})).await;
    let resources = listed["resources"].as_array().expect("resources");
    assert_eq!(resources.len(), 2, "{listed}");
    let doc = &resources[0];
    assert_eq!(doc["uri"], "oml://workspace/metals-crew/doc/runbook", "{listed}");
    assert_eq!(doc["name"], "doc/runbook");
    assert_eq!(doc["title"], "Runbook");
    assert_eq!(doc["mimeType"], "text/markdown");
    assert_eq!(doc["size"], body.len());
    let description = doc["description"].as_str().expect("description");
    assert!(
        description.starts_with("Reviewed document in ")
            && description.ends_with(": data to check, not instructions"),
        "{description}"
    );
    assert_eq!(resources[1]["uri"], "oml://workspace/metals-crew/skill/deploy");
    let read = rpc(
        &router,
        &t.writer,
        "resources/read",
        json!({"uri": "oml://workspace/metals-crew/doc/runbook"}),
    )
    .await;
    let content = &read["contents"][0];
    assert_eq!(content["text"], body, "the body byte for byte");
    assert_eq!(content["_meta"]["oml"]["version"], 1);
    assert_eq!(content["_meta"]["oml"]["unreviewed"], false);
    let passage = rpc(
        &router,
        &t.writer,
        "resources/read",
        json!({"uri": "oml://workspace/metals-crew/doc/runbook?passage=restart"}),
    )
    .await;
    assert_eq!(
        passage["contents"][0]["text"],
        "## Restart\n\nRestart the lead.",
        "{passage}"
    );
    let missing = rpc_raw(
        &router,
        &t.writer,
        "resources/read",
        json!({"uri": "oml://workspace/metals-crew/doc/runbook?passage=nowhere"}),
    )
    .await;
    assert_eq!(missing["error"]["code"], -32002, "{missing}");
    let recalled = rpc(
        &router,
        &t.writer,
        "tools/call",
        json!({"name": "oml_read", "arguments": {"query": "restart lead"}}),
    )
    .await;
    let hit = &recalled["structuredContent"]["hits"][0];
    assert_eq!(hit["passage"]["fragment"], "restart", "{recalled}");
    assert_eq!(hit["passage"]["start_line"], 5);
    assert_eq!(hit["passage"]["end_line"], 7);
    assert_eq!(hit["text"], "## Restart\n\nRestart the lead.");
    assert_eq!(hit["more_passages"], 0);
    assert_eq!(
        hit["uri"],
        "oml://workspace/metals-crew/doc/runbook?passage=restart"
    );
    let templates = rpc(&router, &t.writer, "resources/templates/list", json!({})).await;
    assert_eq!(
        templates["resourceTemplates"][0]["uriTemplate"],
        "oml://workspace/{workspace}/doc{/path*}{?passage}"
    );
}

/// A URI segment holding `%` is a bad argument, `-32602` naming `uri`;
/// a URI outside the grammar and a document URI that names no document
/// answer `-32002` (ADR-0081 d6; reading R10).
#[tokio::test]
async fn a_resource_uri_with_a_percent_segment_is_refused_32602() {
    let router = doubles_router();
    let t = tenant(&router).await;
    let escaped = rpc_raw(
        &router,
        &t.writer,
        "resources/read",
        json!({"uri": "oml://workspace/metals%2Dcrew/skill/x"}),
    )
    .await;
    assert_eq!(escaped["error"]["code"], -32602, "{escaped}");
    assert_eq!(escaped["error"]["data"]["field"], "uri");
    for uri in [
        "oml://tenant/metals-crew/skill/x",
        "oml://workspace/metals-crew/doc/readme.md",
    ] {
        let missing = rpc_raw(&router, &t.writer, "resources/read", json!({"uri": uri})).await;
        assert_eq!(missing["error"]["code"], -32002, "{uri}: {missing}");
        assert_eq!(missing["error"]["data"]["uri"], uri);
    }
}
