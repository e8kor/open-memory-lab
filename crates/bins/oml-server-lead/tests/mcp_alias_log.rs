//! The `mcp_alias_called` log line of the hidden aliases (ADR-0039 d12;
//! docs/api/mcp-plugin.md, "The alias window"), in a test binary of its own:
//! the capture installs the process's global subscriber, which no other test
//! in this binary shares, so no parallel test can race the callsite's
//! interest and drop the line.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_transport_http::mcp::SERVED_TOOLS;

const ENROLLMENT: &str = "enr_test_fixture_token";

/// The eight v0.3.0 tool names and the successor each call's log names.
const V0_3_0_TOOLS: [(&str, &str); 8] = [
    ("oml_remember", "oml_write"),
    ("oml_learn", "oml_write"),
    ("oml_recall", "oml_read"),
    ("oml_get", "oml_read"),
    ("oml_supersede", "oml_write"),
    ("oml_my_work", "oml_read"),
    ("oml_workarea_info", "oml_workarea"),
    ("oml_events_tail", "GET /v2/events"),
];

/// The lead's router over in-memory doubles, its bootstrap open under
/// [`ENROLLMENT`].
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

/// One request; the status, the `X-Request-Id` and the JSON body.
async fn send(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    idem: Option<&str>,
    body: Option<&Value>,
) -> (StatusCode, String, Value) {
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
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .expect("every response carries X-Request-Id")
        .to_owned();
    let bytes = to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("body collects");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("responses are JSON")
    };
    (status, request_id, value)
}

/// One `tools/call` to `/mcp` with empty arguments; its `X-Request-Id`.
async fn call_tool(router: &Router, token: &str, id: u64, name: &str) -> String {
    let (status, request_id, body) = send(
        router,
        "POST",
        "/mcp",
        Some(token),
        None,
        Some(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                     "params": {"name": name, "arguments": {}}})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{name}: {body}");
    assert_eq!(body["id"], id, "{body}");
    request_id
}

/// Bootstrap, one crew workarea and an agent key on it; the key's value,
/// its `key_id` and its tenant's id.
async fn agent(router: &Router) -> (String, String, String) {
    let (status, _, boot) = send(
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
    let (status, _, wa) = send(
        router,
        "POST",
        "/v2/workareas",
        Some(&admin),
        Some("wa-1"),
        Some(&json!({"parent": root, "name": "metals-crew"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{wa}");
    let crew = wa["id"].as_str().expect("workarea id").to_owned();
    let grants =
        json!([{"workarea": crew, "role": "writer"}, {"workarea": crew, "role": "reader"}]);
    let (status, _, key) = send(
        router,
        "POST",
        "/v2/keys",
        Some(&admin),
        Some("issue-writer"),
        Some(&json!({"role": "writer", "name": "writer", "principal": "crew/agent-7", "grants": grants})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");
    let value = key["value"].as_str().expect("show-once value").to_owned();
    let key_id = key["key_id"].as_str().expect("key_id").to_owned();
    let (status, _, me) = send(router, "GET", "/v2/whoami", Some(&value), None, None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    let tenant_id = me["tenant"]["id"].as_str().expect("tenant id").to_owned();
    (value, key_id, tenant_id)
}

/// The log lines a [`tracing`] subscriber writes, captured in memory.
#[derive(Clone, Default)]
struct CapturedLog(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl CapturedLog {
    fn take(&self) -> String {
        let mut bytes = self.0.lock().expect("log lock");
        String::from_utf8(std::mem::take(&mut *bytes)).expect("log lines are UTF-8")
    }
}

impl std::io::Write for CapturedLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn an_alias_call_logs_mcp_alias_called_naming_its_successor_key_tenant_and_request() {
    let log = CapturedLog::default();
    let writer = log.clone();
    // The lead's own subscriber: JSON lines, the event name a field like
    // any other, so a grep for the name still finds them (ADR-0042 d1).
    let subscriber = oml_server_lead::log_subscriber(move || writer.clone());
    tracing::subscriber::set_global_default(subscriber).expect("the only subscriber");

    let router = doubles_router();
    let (token, key_id, tenant_id) = agent(&router).await;
    log.take();

    // A refused call is logged too: the log names who still calls the old
    // name, whatever the call's outcome. The line carries the stable event
    // name and ids only, never the key's value (observability.md,
    // structured logging and the plaintext rule).
    for (id, (alias, successor)) in (1..).zip(V0_3_0_TOOLS) {
        let request_id = call_tool(&router, &token, id, alias).await;
        let lines = log.take();
        let logged: Vec<&str> = lines
            .lines()
            .filter(|line| line.contains("mcp_alias_called"))
            .collect();
        assert_eq!(logged.len(), 1, "{alias}: {lines}");
        let line: Value = serde_json::from_str(logged[0]).expect("a JSON line");
        for (field, value) in [
            ("event", "mcp_alias_called"),
            ("tool", alias),
            ("successor", successor),
            ("key_id", key_id.as_str()),
            ("tenant_id", tenant_id.as_str()),
            ("request_id", request_id.as_str()),
        ] {
            assert_eq!(line[field], value, "{alias}: {field} in {line}");
        }
        assert!(
            !logged[0].contains(&token),
            "{alias}: the key's value is logged"
        );
    }

    // A served tool is not an alias, and logs none; the capture is live,
    // so the absence is not a dropped line.
    for (id, name) in (20..).zip(SERVED_TOOLS) {
        call_tool(&router, &token, id, name).await;
        let lines = log.take();
        assert!(!lines.contains("mcp_alias_called"), "{name}: {lines}");
    }
    call_tool(&router, &token, 30, "oml_get").await;
    assert!(log.take().contains("mcp_alias_called"));
}
