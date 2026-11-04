//! One `http_request` line per request, written by the traffic layer, with
//! the request's origin and outcome and nothing a caller sends as content
//! (ADR-0042 d1–d3, d7, d9, d14).

mod support;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::{HeldBy, Role};
use oml_app::doubles::MemStore;
use oml_app::types::{BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, IssuedKey};
use oml_transport_http::traffic::{Resolved, ScopeObserver, Traffic, TrustedProxies};
use oml_transport_http::{OAUTH_ROUTES, ROUTES};

use support::{Captured, TestApp, app, state};

const ISSUER: &str = "https://oml.example.com";
const CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const FORM: &str = "application/x-www-form-urlencoded";

fn observed(store: &MemStore) -> TestApp {
    app(store)
        .with_observer(Arc::new(ScopeObserver))
        .with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"))
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a readable body")
        .to_vec();
    (status, headers, body)
}

fn pct(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn form(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(name, value)| format!("{}={}", pct(name), pct(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn json_of(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("a JSON body")
}

fn handle_of(body: &[u8]) -> String {
    let page = String::from_utf8(body.to_vec()).expect("a UTF-8 page");
    let marker = r#"name="handle" value=""#;
    let start = page.find(marker).expect("a handle field") + marker.len();
    let end = start + page[start..].find('"').expect("a closing quote");
    page[start..end].to_owned()
}

fn page_post(fields: &[(&str, &str)]) -> Request<Body> {
    Request::post("/oauth/authorize")
        .header(header::CONTENT_TYPE, FORM)
        .header("sec-fetch-site", "same-origin")
        .body(Body::from(form(fields)))
        .expect("a request")
}

async fn tenant(app: &TestApp) -> BootstrapResult {
    app.bootstrap_tenant(&BootstrapRequest {
        tenant_name: Name::parse("acme").expect("a name"),
        admin_key_name: Name::parse("root-admin").expect("a name"),
        admin_principal: "alice-admin".to_owned(),
    })
    .await
    .expect("bootstrap")
}

async fn key(app: &TestApp, boot: &BootstrapResult, name: &str, connector: bool) -> IssuedKey {
    app.issue_key(
        &boot.admin_key.value,
        &IssueKeyRequest {
            role: Role::Writer,
            name: Name::parse(name).expect("a name"),
            principal: format!("crew/{name}"),
            held_by: Some(HeldBy::Agent),
            default_workarea: None,
            crew: None,
            grants: vec![
                GrantRequest {
                    node: boot.root_workarea_id.into(),
                    role: Role::Writer,
                    deny: Vec::new(),
                    covers: Covers::Subtree,
                },
                GrantRequest {
                    node: boot.root_workarea_id.into(),
                    role: Role::Reader,
                    deny: Vec::new(),
                    covers: Covers::Subtree,
                },
            ],
            connect_code: connector,
            user: None,
            guardrail_scope: None,
        },
    )
    .await
    .expect("a key")
}

/// The connect flow through the router, to the authorize redirect and the
/// token pair: its `Location` and its access token.
async fn connect(router: &Router, connect_code: &str) -> (String, String) {
    let (status, _, body) = send(
        router,
        Request::post("/oauth/register")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "client_name": "Claude",
                    "redirect_uris": [CALLBACK],
                    "token_endpoint_auth_method": "none",
                    "grant_types": ["authorization_code", "refresh_token"],
                    "response_types": ["code"],
                })
                .to_string(),
            ))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let client_id = json_of(&body)["client_id"]
        .as_str()
        .expect("a client_id")
        .to_owned();
    let query = form(&[
        ("client_id", &client_id),
        ("redirect_uri", CALLBACK),
        ("response_type", "code"),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
        ("state", "st"),
        ("resource", &format!("{ISSUER}/mcp")),
        ("scope", "mcp"),
    ]);
    let (status, _, body) = send(
        router,
        Request::get(format!("/oauth/authorize?{query}"))
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, body) = send(
        router,
        page_post(&[
            ("handle", &handle_of(&body)),
            ("connect_code", connect_code),
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, headers, _) = send(
        router,
        page_post(&[("handle", &handle_of(&body)), ("action", "connect")]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION]
        .to_str()
        .expect("ASCII")
        .to_owned();
    let code = location
        .split_once('?')
        .expect("a query")
        .1
        .split('&')
        .find_map(|pair| pair.strip_prefix("code="))
        .expect("a code")
        .to_owned();
    let (status, _, body) = send(
        router,
        Request::post("/oauth/token")
            .header(header::CONTENT_TYPE, FORM)
            .body(Body::from(form(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("client_id", &client_id),
                ("redirect_uri", CALLBACK),
                ("code_verifier", VERIFIER),
            ])))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let access = json_of(&body)["access_token"]
        .as_str()
        .expect("an access token")
        .to_owned();
    (location, access)
}

fn submit(value: &str, body: &str) -> Request<Body> {
    Request::post("/v1/memories")
        .header(header::AUTHORIZATION, format!("Bearer {value}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("idempotency-key", "idem-line-1")
        .header(header::USER_AGENT, "oml-cli/0.4.1 hook/session-start")
        .body(Body::from(json!({ "body": body }).to_string()))
        .expect("a request")
}

#[tokio::test]
async fn no_log_line_contains_a_request_body_the_bearer_value_a_token_a_query_or_the_authorize_location()
 {
    let store = MemStore::new();
    let app = observed(&store);
    let boot = tenant(&app).await;
    let writer = key(&app, &boot, "agent-7", false).await;
    let connector = key(&app, &boot, "claude-mobile", true).await;
    let connect_code = connector
        .connect_code
        .and_then(|c| c.code)
        .expect("the code, shown once");
    let router = state(&store, app).into_router();

    let (guard, captured) = Captured::start();
    let (status, _, _) = send(&router, submit(&writer.value, "the zanzibarword fact")).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, _, _) = send(
        &router,
        Request::get("/v1/memories?claim_key=quixoticquery")
            .header(header::AUTHORIZATION, format!("Bearer {}", writer.value))
            .header(header::COOKIE, "session=cookiecrumb")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (location, access) = connect(&router, &connect_code).await;
    let (status, _, _) = send(
        &router,
        Request::post("/mcp")
            .header(header::AUTHORIZATION, format!("Bearer {access}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": {"name": "oml_read", "arguments": {"query": "zanzibarword"}}})
                .to_string(),
            ))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    drop(guard);

    let text = captured.text();
    let code = location.split("code=").nth(1).expect("a code");
    for (what, secret) in [
        ("the bearer value", writer.value.as_str()),
        ("the access token", access.as_str()),
        ("the connect code", connect_code.as_str()),
        ("the authorize Location", location.as_str()),
        ("the authorization code", code),
        ("a body word", "zanzibarword"),
        ("a query value", "quixoticquery"),
        ("a cookie", "cookiecrumb"),
    ] {
        assert!(!text.contains(secret), "{what} is in the log:\n{text}");
    }
    let lines = captured.events("http_request");
    let routes: Vec<&str> = lines.iter().filter_map(|l| l["route"].as_str()).collect();
    assert!(routes.contains(&"/v1/memories"), "{routes:?}");
    assert!(routes.contains(&"/oauth/authorize"), "{routes:?}");
    assert!(routes.contains(&"/mcp"), "{routes:?}");
}

#[tokio::test]
async fn the_request_line_carries_the_resolved_key_the_target_workarea_and_the_route_template() {
    let store = MemStore::new();
    let app = observed(&store);
    let boot = tenant(&app).await;
    let writer = key(&app, &boot, "agent-7", false).await;
    let router = state(&store, app).into_router();

    let (guard, captured) = Captured::start();
    let (status, _, _) = send(&router, submit(&writer.value, "a fact")).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, headers, _) = send(
        &router,
        Request::get("/v2/keys/agent-7")
            .header(header::AUTHORIZATION, format!("Bearer {}", writer.value))
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    drop(guard);

    let lines = captured.events("http_request");
    assert_eq!(lines.len(), 2, "{lines:?}");
    let line = &lines[0];
    assert_eq!(line["level"], "INFO");
    assert!(line["timestamp"].is_string() && line["target"].is_string());
    assert_eq!(line["method"], "POST");
    assert_eq!(line["route"], "/v1/memories");
    assert_eq!(line["status"], 202);
    assert_eq!(line["request_id_source"], "minted");
    assert_eq!(line["correlation_id"], line["request_id"]);
    assert_eq!(line["correlation_id_source"], "request_id");
    assert_eq!(line["tenant_id"], boot.tenant_id.to_string());
    assert_eq!(line["key_id"], writer.key_id.to_string());
    assert_eq!(line["key_name"], "agent-7");
    assert_eq!(line["principal"], "crew/agent-7");
    assert_eq!(line["role"], "writer");
    assert_eq!(line["credential"], "bearer");
    assert_eq!(line["workarea_id"], boot.root_workarea_id.to_string());
    assert_eq!(line["user_agent"], "oml-cli/0.4.1 hook/session-start");
    assert!(line["bytes_in"].as_u64().is_some_and(|n| n > 0));
    assert!(line["bytes_out"].as_u64().is_some_and(|n| n > 0));
    assert!(line["duration_ms"].is_number() && line["auth_ms"].is_number());
    assert!(line["store_ms"].is_number() && line["store_calls"].is_number());
    assert!(line.get("error_code").is_none() && line.get("idempotency_replay").is_none());

    let refused = &lines[1];
    assert_eq!(
        refused["route"], "/v2/keys/{key}",
        "the template, never the path"
    );
    assert_eq!(refused["error_code"], "access.verb_ceiling");
    assert_eq!(
        refused["request_id"],
        headers["x-request-id"].to_str().expect("ASCII")
    );
}

#[tokio::test]
async fn a_probe_answered_200_writes_no_line_and_an_unmatched_path_logs_unmatched_without_a_replaced_id()
 {
    let store = MemStore::new();
    let router = state(&store, observed(&store)).into_router();
    let (guard, captured) = Captured::start();
    for probe in ["/healthz", "/readyz"] {
        let (status, _, _) = send(
            &router,
            Request::get(probe).body(Body::empty()).expect("a request"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, _, _) = send(
        &router,
        Request::get("/wp-admin/setup.php?x=1")
            .header("x-request-id", "evil value")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    drop(guard);
    let lines = captured.events("http_request");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["route"], "unmatched");
    assert_eq!(lines[0]["request_id_source"], "replaced");
    let text = captured.text();
    assert!(
        !text.contains("evil value") && !text.contains("wp-admin"),
        "{text}"
    );
}

#[tokio::test]
async fn a_probe_answered_503_writes_its_line_at_error() {
    let store = MemStore::new();
    let st = oml_transport_http::ApiState::new(
        Arc::new(observed(&store)),
        store.clone(),
        Arc::new(|| Box::pin(async { false })),
        true,
        None,
    );
    let router = st.into_router();
    let (guard, captured) = Captured::start();
    let (status, _, _) = send(
        &router,
        Request::get("/readyz")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    drop(guard);
    let lines = captured.events("http_request");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["level"], "ERROR");
    assert_eq!(lines[0]["route"], "/readyz");
}

fn from_peer(peer: &str, forwarded: &str) -> Request<Body> {
    let mut req = Request::get("/v2/whoami")
        .header("x-forwarded-for", forwarded)
        .body(Body::empty())
        .expect("a request");
    req.extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().expect("an address")));
    req
}

#[tokio::test]
async fn a_forwarded_header_from_an_untrusted_peer_leaves_client_ip_the_peer() {
    let store = MemStore::new();
    let trusted = TrustedProxies::parse(&["127.0.0.1/32"]).expect("parses");
    let router = state(&store, observed(&store))
        .with_traffic(Traffic::new(trusted))
        .into_router();
    let (guard, captured) = Captured::start();
    send(&router, from_peer("203.0.113.9:5000", "198.51.100.1")).await;
    send(&router, from_peer("127.0.0.1:5001", "198.51.100.1")).await;
    drop(guard);
    let lines = captured.events("http_request");
    assert_eq!(lines[0]["client_ip"], "203.0.113.9");
    assert_eq!(lines[0]["client_ip_source"], "peer");
    assert_eq!(lines[1]["client_ip"], "198.51.100.1");
    assert_eq!(lines[1]["client_ip_source"], "forwarded");
}

#[tokio::test]
async fn every_route_attaches_what_it_resolved_to_its_response() {
    let store = MemStore::new();
    let router = state(&store, observed(&store)).into_router();
    let routes = ROUTES.iter().chain(OAUTH_ROUTES).chain(&[("POST", "/mcp")]);
    for &(method, path) in routes {
        let path = path
            .replace("{artifact_id}", "0b5f6c3e-5d0b-4d8f-9a5e-2f1d7c9e8a10")
            .replace("{key}", "agent-7")
            .replace("{connection_id}", "0b5f6c3e-5d0b-4d8f-9a5e-2f1d7c9e8a11")
            .replace("{crew}", "crew-a")
            .replace("{workarea}", "area-a");
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(&path)
                    .body(Body::empty())
                    .expect("a request"),
            )
            .await
            .expect("the router answers");
        assert!(
            response.extensions().get::<Resolved>().is_some(),
            "{method} {path} attaches nothing"
        );
    }
}
