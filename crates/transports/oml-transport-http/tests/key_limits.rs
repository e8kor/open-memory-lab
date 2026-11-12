//! A key's expiry and its own request rate over HTTP (ADR-0079): set at
//! `POST /v2/keys`, served by the key reads and `whoami`, changed by `POST
//! /v2/keys/{key}/limits` for an admin key that is not the key itself, and
//! enforced on `/v2` and `/mcp` with the refusals errors.md names. Over the
//! in-memory doubles, with a clock the tests move. Each test is named after
//! the sentence it proves.

mod support;

use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::App;
use oml_app::domain::event_types;
use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::BootstrapRequest;

use support::{SEED, state};

/// Ten minutes after the fixed clock's noon, as a client writes it.
const EXPIRY_SENT: &str = "2026-10-29T12:10:00Z";
/// The same instant as the server stores and serves it.
const EXPIRY: &str = "2026-10-29T12:10:00.000000Z";

/// The deployment's `oauth.issuer`, and the fixture client's callback and
/// PKCE pair (RFC 7636 appendix B).
const ISSUER: &str = "https://oml.example.com";
const CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const FORM: &str = "application/x-www-form-urlencoded";

/// An `Idempotency-Key` no other request of the suite sends.
fn fresh_idempotency_key() -> String {
    static SENT: AtomicU64 = AtomicU64::new(0);
    format!("key-limits-{}", SENT.fetch_add(1, Ordering::Relaxed))
}

struct World {
    store: MemStore,
    router: Router,
    clock: FixedClock,
    tenant: uuid::Uuid,
    admin: String,
    admin_name: &'static str,
}

async fn world() -> World {
    let store = MemStore::new();
    let clock = FixedClock::new();
    let app = App::new(
        store.clone(),
        MemSigner::new(SEED),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"));
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("acme-admin").expect("a name"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    World {
        router: state(&store, app).into_router(),
        clock,
        tenant: boot.tenant_id,
        admin: boot.admin_key.value,
        admin_name: "acme-admin",
        store,
    }
}

impl World {
    async fn call(
        &self,
        method: Method,
        path: &str,
        bearer: &str,
        body: Option<&Value>,
    ) -> (StatusCode, HeaderMap, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {bearer}"));
        if body.is_some() {
            request = request
                .header("idempotency-key", fresh_idempotency_key())
                .header(header::CONTENT_TYPE, "application/json");
        }
        let request = request
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
            .expect("a request");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("an answer");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (
            status,
            headers,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get(&self, path: &str, bearer: &str) -> (StatusCode, HeaderMap, Value) {
        self.call(Method::GET, path, bearer, None).await
    }

    async fn post(&self, path: &str, bearer: &str, body: &Value) -> (StatusCode, HeaderMap, Value) {
        self.call(Method::POST, path, bearer, Some(body)).await
    }

    /// A key of `role` named `name` on `universe`, with `limits` merged into
    /// its issuance: the `201`'s body.
    async fn issue(&self, name: &str, role: &str, limits: Value) -> Value {
        let mut body = json!({
            "role": role,
            "name": name,
            "grants": if role == "admin" {
                json!([])
            } else {
                json!([{ "workarea": "universe", "role": role }])
            },
        });
        for (member, value) in limits.as_object().expect("an object") {
            body[member] = value.clone();
        }
        let (status, _, issued) = self.post("/v2/keys", &self.admin, &body).await;
        assert_eq!(status, StatusCode::CREATED, "{issued}");
        issued
    }

    async fn head_seq(&self) -> u64 {
        self.store
            .chain_head(self.tenant)
            .await
            .expect("head")
            .expect("a chain")
            .seq
    }

    /// One request through the router: its status, headers and raw body.
    async fn send(&self, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("an answer");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, headers, bytes.to_vec())
    }

    /// A writer connector key named `name` on `universe`, with `limits`
    /// merged into its issuance: its `201`'s body and its connect code.
    async fn connector(&self, name: &str, limits: Value) -> (Value, String) {
        let mut body = json!({
            "role": "writer",
            "name": name,
            "connect_code": true,
            "grants": [
                { "workarea": "universe", "role": "writer" },
                { "workarea": "universe", "role": "reader" },
            ],
        });
        for (member, value) in limits.as_object().expect("an object") {
            body[member] = value.clone();
        }
        let (status, _, issued) = self.post("/v2/keys", &self.admin, &body).await;
        assert_eq!(status, StatusCode::CREATED, "{issued}");
        let code = issued["connect_code"]["code"]
            .as_str()
            .expect("the code, shown once")
            .to_owned();
        (issued, code)
    }

    /// A client registered, the connect code entered on the authorization
    /// page and Connect confirmed: the client's id and the authorization
    /// code (ADR-0031 d9).
    async fn connected(&self, connect_code: &str) -> (String, String) {
        let (status, _, body) = self
            .send(
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
        let (status, _, page) = self
            .send(
                Request::get(format!("/oauth/authorize?{query}"))
                    .body(Body::empty())
                    .expect("a request"),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, page) = self
            .send(page_post(&[
                ("handle", &handle_of(&page)),
                ("connect_code", connect_code),
            ]))
            .await;
        assert_eq!(status, StatusCode::OK);
        let (status, headers, _) = self
            .send(page_post(&[
                ("handle", &handle_of(&page)),
                ("action", "connect"),
            ]))
            .await;
        assert_eq!(status, StatusCode::SEE_OTHER);
        let location = headers[header::LOCATION].to_str().expect("ASCII");
        let code = location
            .split_once('?')
            .expect("a query")
            .1
            .split('&')
            .find_map(|pair| pair.strip_prefix("code="))
            .expect("a code")
            .to_owned();
        (client_id, code)
    }

    /// `POST /oauth/token` redeeming `code` for `client_id`.
    async fn exchange(&self, client_id: &str, code: &str) -> (StatusCode, Value) {
        let (status, _, body) = self
            .send(
                Request::post("/oauth/token")
                    .header(header::CONTENT_TYPE, FORM)
                    .body(Body::from(form(&[
                        ("grant_type", "authorization_code"),
                        ("code", code),
                        ("client_id", client_id),
                        ("redirect_uri", CALLBACK),
                        ("code_verifier", VERIFIER),
                    ])))
                    .expect("a request"),
            )
            .await;
        (status, json_of(&body))
    }

    /// The access token of a connection opened with `connect_code`.
    async fn access_token(&self, connect_code: &str) -> String {
        let (client_id, code) = self.connected(connect_code).await;
        let (status, tokens) = self.exchange(&client_id, &code).await;
        assert_eq!(status, StatusCode::OK, "{tokens}");
        tokens["access_token"]
            .as_str()
            .expect("an access token")
            .to_owned()
    }

    /// One JSON-RPC frame at `/mcp` under `bearer`.
    async fn mcp(&self, bearer: &str, id: u64, method: &str, params: Value) -> (StatusCode, Value) {
        let (status, _, body) = self
            .send(
                Request::post("/mcp")
                    .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
                            .to_string(),
                    ))
                    .expect("a request"),
            )
            .await;
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }
}

/// Every byte outside RFC 3986's unreserved set as `%XX`.
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

/// The value of the authorization page form's hidden `handle`.
fn handle_of(body: &[u8]) -> String {
    let page = String::from_utf8(body.to_vec()).expect("a UTF-8 page");
    let marker = r#"name="handle" value=""#;
    let start = page.find(marker).expect("a handle field") + marker.len();
    let end = start + page[start..].find('"').expect("a closing quote");
    page[start..end].to_owned()
}

/// A post of the authorization page's own form.
fn page_post(fields: &[(&str, &str)]) -> Request<Body> {
    Request::post("/oauth/authorize")
        .header(header::CONTENT_TYPE, FORM)
        .header("sec-fetch-site", "same-origin")
        .body(Body::from(form(fields)))
        .expect("a request")
}

/// A `tools/call` of `oml_read` with a query, the smallest call that
/// reaches a use case.
fn recall() -> Value {
    json!({ "name": "oml_read", "arguments": { "query": "anything" } })
}

fn value_of(issued: &Value) -> &str {
    issued["value"].as_str().expect("a bearer key's value")
}

fn refusal(body: &Value) -> (&str, &Value) {
    (
        body["error"]["code"].as_str().expect("a typed refusal"),
        &body["error"]["details"],
    )
}

/// The issuance answer, the admin's key read and the key's own `whoami`
/// serve the expiry as the instant it names and the rate as sent
/// (ADR-0079 d1, d3, d4).
#[tokio::test]
async fn a_key_issued_with_an_expiry_and_a_rate_reads_them_back_on_its_answer_the_key_read_and_whoami()
 {
    let w = world().await;
    let issued = w
        .issue(
            "reviewer",
            "reader",
            json!({ "expiry": EXPIRY_SENT, "requests_per_minute": 30 }),
        )
        .await;
    assert_eq!(issued["expiry"], EXPIRY);
    assert_eq!(issued["requests_per_minute"], 30);

    let (status, _, read) = w.get("/v2/keys/reviewer", &w.admin).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["expiry"], EXPIRY);
    assert_eq!(read["expired"], false);
    assert_eq!(read["requests_per_minute"], 30);

    let (status, _, me) = w.get("/v2/whoami", value_of(&issued)).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["expiry"], EXPIRY);
    assert_eq!(me["requests_per_minute"], 30);

    let plain = w.issue("plain", "reader", json!({})).await;
    assert_eq!(plain["expiry"], Value::Null);
    assert_eq!(plain["requests_per_minute"], Value::Null);
}

/// From its expiry on a key's request is `401 auth.invalid` with `reason:
/// expired` and the date, on `/v2` and at `/mcp`, where a value no key has
/// is `reason: unknown`; the admin's read then shows the key expired and
/// not revoked (ADR-0079 d2, d3).
#[tokio::test]
async fn a_request_under_an_expired_key_is_auth_invalid_expired_on_v2_and_mcp_and_the_key_reads_as_expired()
 {
    let w = world().await;
    let issued = w
        .issue("reviewer", "reader", json!({ "expiry": EXPIRY_SENT }))
        .await;
    let key = value_of(&issued);
    let (status, _, _) = w.get("/v2/whoami", key).await;
    assert_eq!(status, StatusCode::OK);
    w.clock.advance(600);

    let (status, _, body) = w.get("/v2/whoami", key).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(
        refusal(&body),
        (
            "auth.invalid",
            &json!({ "reason": "expired", "expiry": EXPIRY })
        )
    );
    let (status, headers, body) = w
        .post(
            "/mcp",
            key,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(refusal(&body).1["reason"], "expired");
    assert!(
        headers.contains_key(header::WWW_AUTHENTICATE),
        "{headers:?}"
    );
    let (status, _, body) = w.get("/v2/whoami", "oml_no-such-key").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        refusal(&body),
        ("auth.invalid", &json!({ "reason": "unknown" }))
    );

    let (status, _, read) = w.get("/v2/keys/reviewer", &w.admin).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["expired"], true);
    assert_eq!(read["revoked_at"], Value::Null);
    assert_eq!(read["grants"].as_array().map(Vec::len), Some(1));
}

/// An `expiry` that names no instant, or one not later than now, and a rate
/// outside its bound are `request.validation` naming the field, and nothing
/// is journalled (ADR-0079 d1, d4).
#[tokio::test]
async fn an_expiry_that_is_a_bare_date_or_past_and_a_rate_out_of_bound_are_refused_naming_the_field()
 {
    let w = world().await;
    let head = w.head_seq().await;
    for (n, (limits, field)) in [
        (json!({ "expiry": "2026-12-01" }), "/expiry"),
        (json!({ "expiry": "2026-10-29T11:00:00Z" }), "/expiry"),
        (json!({ "requests_per_minute": 0 }), "/requests_per_minute"),
        (
            json!({ "requests_per_minute": 1_000_001 }),
            "/requests_per_minute",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let mut body = json!({
            "role": "reader",
            "name": format!("refused-{n}"),
            "grants": [{ "workarea": "universe", "role": "reader" }],
        });
        for (member, value) in limits.as_object().expect("an object") {
            body[member] = value.clone();
        }
        let (status, _, refused) = w.post("/v2/keys", &w.admin, &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_eq!(
            refusal(&refused),
            ("request.validation", &json!({ "field": field })),
            "{limits}"
        );
    }
    assert_eq!(w.head_seq().await, head, "nothing journalled");
}

/// The request past a key's own rate is `429 limit.rate` naming the quota
/// and the limit, with `Retry-After` the seconds left in the minute
/// (ADR-0079 d4).
#[tokio::test]
async fn the_request_past_a_keys_own_rate_is_429_limit_rate_with_retry_after() {
    let w = world().await;
    let issued = w
        .issue("reviewer", "reader", json!({ "requests_per_minute": 2 }))
        .await;
    let key = value_of(&issued);
    w.clock.advance(15);
    for _ in 0..2 {
        let (status, _, body) = w.get("/v2/whoami", key).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    let (status, headers, body) = w.get("/v2/whoami", key).await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(
        refusal(&body),
        (
            "limit.rate",
            &json!({ "quota": "requests_per_minute", "limit": 2 })
        )
    );
    assert_eq!(headers[header::RETRY_AFTER], "45", "{headers:?}");
    w.clock.advance(45);
    let (status, _, body) = w.get("/v2/whoami", key).await;
    assert_eq!(status, StatusCode::OK, "the next minute admits it: {body}");
}

/// `POST /v2/keys/{key}/limits` replaces a member sent, clears one sent as
/// null and keeps one not sent, answers the limits as they stand, and
/// journals one `key.limits_set` each time; an expired key whose date is
/// moved authenticates again (ADR-0079 d5).
#[tokio::test]
async fn an_admin_key_sets_and_clears_another_keys_limits_and_each_change_is_one_key_limits_set() {
    let w = world().await;
    let issued = w
        .issue("reviewer", "reader", json!({ "expiry": EXPIRY_SENT }))
        .await;
    let key = value_of(&issued);
    w.clock.advance(600);
    let (status, _, _) = w.get("/v2/whoami", key).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let head = w.head_seq().await;

    let (status, _, set) = w
        .post(
            "/v2/keys/reviewer/limits",
            &w.admin,
            &json!({ "expiry": "2026-10-29T12:40:00+00:00", "requests_per_minute": 60 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    assert_eq!(
        set,
        json!({
            "key_id": issued["key_id"],
            "name": "reviewer",
            "expiry": "2026-10-29T12:40:00.000000Z",
            "expired": false,
            "requests_per_minute": 60,
        })
    );
    let (status, _, me) = w.get("/v2/whoami", key).await;
    assert_eq!(status, StatusCode::OK, "{me}");

    let (status, _, cleared) = w
        .post(
            "/v2/keys/reviewer/limits",
            &w.admin,
            &json!({ "requests_per_minute": null }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert_eq!(cleared["expiry"], "2026-10-29T12:40:00.000000Z", "kept");
    assert_eq!(cleared["requests_per_minute"], Value::Null, "cleared");

    assert_eq!(w.head_seq().await, head + 2);
    let events = w
        .store
        .events_range(w.tenant, head + 1, 2)
        .await
        .expect("events");
    for event in &events {
        assert_eq!(event.envelope.event_type, event_types::KEY_LIMITS_SET);
        assert_eq!(event.envelope.payload["key_id"], issued["key_id"]);
    }
    assert_eq!(
        events[1].envelope.payload["requests_per_minute"],
        Value::Null
    );
}

/// The limits route is the admin key role's and never names the calling
/// key: an agent's key is `access.verb_ceiling` whichever key it names, an
/// admin key naming itself is `request.validation` with `reason: own_key`,
/// a body naming no limit or an unknown member is `request.validation`, and
/// each refusal journals nothing (ADR-0079 d5; R1).
#[tokio::test]
async fn an_agent_key_changes_no_limits_and_an_admin_key_never_its_own() {
    let w = world().await;
    let agent = w
        .issue(
            "agent",
            "writer",
            json!({ "expiry": EXPIRY_SENT, "requests_per_minute": 30 }),
        )
        .await;
    let wider = json!({ "expiry": "2026-10-29T12:40:00Z", "requests_per_minute": 1000 });
    let head = w.head_seq().await;

    for target in ["agent", w.admin_name] {
        let (status, _, body) = w
            .post(
                &format!("/v2/keys/{target}/limits"),
                value_of(&agent),
                &wider,
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{target}: {body}");
        let (code, details) = refusal(&body);
        assert_eq!(code, "access.verb_ceiling");
        assert_eq!(details["verb"], "set_key_limits");
    }
    let (status, _, body) = w
        .post(
            &format!("/v2/keys/{}/limits", w.admin_name),
            &w.admin,
            &wider,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        refusal(&body),
        (
            "request.validation",
            &json!({ "field": "key", "reason": "own_key" })
        )
    );
    for body in [json!({}), json!({ "expires": EXPIRY_SENT })] {
        let (status, _, refused) = w.post("/v2/keys/agent/limits", &w.admin, &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_eq!(refusal(&refused).0, "request.validation", "{body}");
    }
    let (status, _, body) = w
        .post("/v2/keys/no-such-key/limits", &w.admin, &wider)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    assert_eq!(w.head_seq().await, head, "nothing journalled");
    let (_, _, read) = w.get("/v2/keys/agent", &w.admin).await;
    assert_eq!(read["expiry"], EXPIRY);
    assert_eq!(read["requests_per_minute"], 30);
}

/// The body of an admin key's issuance, held by an agent, with `limits`
/// merged in.
fn admin_key_body(name: &str, limits: &Value) -> Value {
    let mut body = json!({ "role": "admin", "name": name, "held_by": "agent", "grants": [] });
    for (member, value) in limits.as_object().expect("an object") {
        body[member] = value.clone();
    }
    body
}

/// An admin key, an agent's included, carries no limit and is given none:
/// `POST /v2/keys` for role `admin` with `expiry` or `requests_per_minute`
/// is `request.validation` naming the member with `reason: admin_key`, and
/// `POST /v2/keys/{key}/limits` naming an admin key is `request.validation`
/// with `field: key, reason: admin_key`, whether it sets or clears. So a
/// limited admin key cannot shed its limits through a second admin key it
/// issues, since none is ever limited, and no admin key ends another by a
/// near expiry: the first still authenticates past the date that was asked.
/// Nothing is journalled by a refusal (ADR-0079 d1, d5; R1).
#[tokio::test]
async fn an_admin_key_is_issued_with_no_limit_and_given_none_by_another_admin_key() {
    let w = world().await;
    let head = w.head_seq().await;
    for (limits, field) in [
        (
            json!({ "expiry": EXPIRY_SENT, "requests_per_minute": 30 }),
            "/expiry",
        ),
        (json!({ "expiry": EXPIRY_SENT }), "/expiry"),
        (json!({ "requests_per_minute": 30 }), "/requests_per_minute"),
    ] {
        let (status, _, refused) = w
            .post(
                "/v2/keys",
                &w.admin,
                &admin_key_body("limited-admin", &limits),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{limits}: {refused}");
        assert_eq!(
            refusal(&refused),
            (
                "request.validation",
                &json!({ "field": field, "reason": "admin_key" })
            ),
            "{limits}"
        );
    }
    assert_eq!(w.head_seq().await, head, "nothing journalled");

    let (status, _, second) = w
        .post(
            "/v2/keys",
            &w.admin,
            &admin_key_body("second-admin", &json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{second}");
    assert_eq!(second["expiry"], Value::Null);
    assert_eq!(second["requests_per_minute"], Value::Null);
    let head = w.head_seq().await;
    for (caller, target, change) in [
        // The second key clears the first's limits.
        (
            value_of(&second),
            w.admin_name,
            json!({ "expiry": null, "requests_per_minute": null }),
        ),
        // The second key ends the first by a near expiry.
        (
            value_of(&second),
            w.admin_name,
            json!({ "expiry": EXPIRY_SENT }),
        ),
        // The first limits the second, an admin key an agent holds.
        (
            w.admin.as_str(),
            "second-admin",
            json!({ "expiry": EXPIRY_SENT, "requests_per_minute": 1 }),
        ),
    ] {
        let (status, _, body) = w
            .post(&format!("/v2/keys/{target}/limits"), caller, &change)
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{target}: {body}");
        assert_eq!(
            refusal(&body),
            (
                "request.validation",
                &json!({ "field": "key", "reason": "admin_key" })
            ),
            "{target}: {change}"
        );
    }
    assert_eq!(w.head_seq().await, head, "nothing journalled");

    w.clock.advance(660);
    for (bearer, name) in [
        (w.admin.as_str(), w.admin_name),
        (value_of(&second), "second-admin"),
    ] {
        for _ in 0..2 {
            let (status, _, me) = w.get("/v2/whoami", bearer).await;
            assert_eq!(status, StatusCode::OK, "{name}: {me}");
            assert_eq!(me["expiry"], Value::Null, "{name}");
            assert_eq!(me["requests_per_minute"], Value::Null, "{name}");
        }
    }
}

/// A connection is exactly its key over HTTP: from the connector key's
/// expiry on, its access token at `/mcp` is `401 auth.invalid` with
/// `reason: expired`, the date and the challenge, on `tools/list` and on
/// `tools/call` alike, where before it both answered (ADR-0079 d2;
/// ADR-0031 d2, d11).
#[tokio::test]
async fn an_expired_connector_keys_access_token_is_auth_invalid_expired_at_mcp_on_tools_list_and_tools_call()
 {
    let w = world().await;
    let (_, code) = w
        .connector("claude-mobile", json!({ "expiry": EXPIRY_SENT }))
        .await;
    let token = w.access_token(&code).await;
    let (status, listed) = w.mcp(&token, 1, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let (status, called) = w.mcp(&token, 2, "tools/call", recall()).await;
    assert_eq!(status, StatusCode::OK, "{called}");
    assert_eq!(called["result"]["isError"], false, "{called}");
    w.clock.advance(600);

    for (id, method, params) in [(3, "tools/list", json!({})), (4, "tools/call", recall())] {
        let (status, headers, body) = w
            .send(
                Request::post("/mcp")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
                            .to_string(),
                    ))
                    .expect("a request"),
            )
            .await;
        let body = json_of(&body);
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method}: {body}");
        assert_eq!(
            refusal(&body),
            (
                "auth.invalid",
                &json!({ "reason": "expired", "expiry": EXPIRY })
            ),
            "{method}"
        );
        assert!(
            headers.contains_key(header::WWW_AUTHENTICATE),
            "{method}: {headers:?}"
        );
    }
}

/// A key that expired between Connect and the exchange opens no
/// connection: the token endpoint answers `invalid_grant` and spends
/// nothing, so once an admin moves the date the same authorization code
/// redeems and its access token works (ADR-0079 d2, d5; ADR-0031 d11).
#[tokio::test]
async fn the_token_exchange_for_a_key_that_expired_after_connect_is_invalid_grant_and_redeems_once_the_date_is_moved()
 {
    let w = world().await;
    // Thirty seconds after noon: inside the authorization code's minute.
    let (_, code) = w
        .connector("claude-mobile", json!({ "expiry": "2026-10-29T12:00:30Z" }))
        .await;
    let (client_id, authorization_code) = w.connected(&code).await;
    w.clock.advance(31);

    let (status, refused) = w.exchange(&client_id, &authorization_code).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["error"], "invalid_grant", "{refused}");
    assert_eq!(refused["access_token"], Value::Null, "{refused}");

    let (status, _, set) = w
        .post(
            "/v2/keys/claude-mobile/limits",
            &w.admin,
            &json!({ "expiry": EXPIRY_SENT }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    let (status, tokens) = w.exchange(&client_id, &authorization_code).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the refusal spent nothing: {tokens}"
    );
    let token = tokens["access_token"].as_str().expect("an access token");
    let (status, listed) = w.mcp(token, 1, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
}

/// Each item of `GET /v2/keys` serves the key's `expiry`, whether it has
/// come, and its `requests_per_minute`: the values for a key that carries
/// them and null, false, null for one that does not; once the date has
/// come the item reads `expired: true` (ADR-0079 d3).
#[tokio::test]
async fn each_item_of_the_keys_listing_serves_the_keys_expiry_whether_it_has_come_and_its_rate() {
    let w = world().await;
    for (name, limits) in [
        (
            "ada-limited",
            json!({ "expiry": EXPIRY_SENT, "requests_per_minute": 30 }),
        ),
        ("ada-plain", json!({})),
    ] {
        let mut body = json!({
            "role": "writer",
            "name": name,
            "user": "ada",
            "grants": [{ "workarea": "ada", "role": "writer" }],
        });
        for (member, value) in limits.as_object().expect("an object") {
            body[member] = value.clone();
        }
        let (status, _, issued) = w.post("/v2/keys", &w.admin, &body).await;
        assert_eq!(status, StatusCode::CREATED, "{issued}");
    }
    let limits_of = |listing: &Value| -> Vec<(String, Value, Value, Value)> {
        listing["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|item| {
                (
                    item["name"].as_str().expect("a name").to_owned(),
                    item["expiry"].clone(),
                    item["expired"].clone(),
                    item["requests_per_minute"].clone(),
                )
            })
            .collect()
    };

    let (status, _, listing) = w.get("/v2/keys?user=ada", &w.admin).await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    assert_eq!(
        limits_of(&listing),
        [
            (
                "ada-limited".to_owned(),
                json!(EXPIRY),
                json!(false),
                json!(30)
            ),
            (
                "ada-plain".to_owned(),
                Value::Null,
                json!(false),
                Value::Null
            ),
        ],
        "{listing}"
    );
    w.clock.advance(600);
    let (status, _, listing) = w.get("/v2/keys?user=ada", &w.admin).await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    assert_eq!(
        limits_of(&listing),
        [
            (
                "ada-limited".to_owned(),
                json!(EXPIRY),
                json!(true),
                json!(30)
            ),
            (
                "ada-plain".to_owned(),
                Value::Null,
                json!(false),
                Value::Null
            ),
        ],
        "{listing}"
    );
}

/// A connector key's own rate is reached through its access token at
/// `/mcp`: each `tools/call` counts, the one past the rate is a tool
/// result carrying `limit.rate` with the quota and the limit, and the
/// frames that call no tool (`initialize`, `tools/list`, `ping`) are not
/// counted, before the rate is reached or after (ADR-0079 d4).
#[tokio::test]
async fn a_connector_keys_rate_is_reached_by_its_access_token_at_mcp_and_frames_that_call_no_tool_are_not_counted()
 {
    let w = world().await;
    let (_, code) = w
        .connector("claude-mobile", json!({ "requests_per_minute": 2 }))
        .await;
    let token = w.access_token(&code).await;
    let no_tool = |id: u64| {
        let (w, token) = (&w, token.as_str());
        async move {
            for (n, method) in ["initialize", "tools/list", "ping"].into_iter().enumerate() {
                let (status, frame) = w.mcp(token, id + n as u64, method, json!({})).await;
                assert_eq!(status, StatusCode::OK, "{method}: {frame}");
                assert_eq!(frame["error"], Value::Null, "{method}: {frame}");
            }
        }
    };
    // More frames than the rate admits requests, and none is counted.
    no_tool(10).await;
    no_tool(20).await;

    for id in [1, 2] {
        let (status, called) = w.mcp(&token, id, "tools/call", recall()).await;
        assert_eq!(status, StatusCode::OK, "{called}");
        assert_eq!(called["result"]["isError"], false, "call {id}: {called}");
    }
    let (status, called) = w.mcp(&token, 3, "tools/call", recall()).await;
    assert_eq!(status, StatusCode::OK, "{called}");
    assert_eq!(called["result"]["isError"], true, "{called}");
    assert_eq!(
        refusal(&called["result"]["structuredContent"]),
        (
            "limit.rate",
            &json!({ "quota": "requests_per_minute", "limit": 2 })
        ),
        "{called}"
    );
    no_tool(30).await;

    w.clock.advance(60);
    let (status, called) = w.mcp(&token, 4, "tools/call", recall()).await;
    assert_eq!(status, StatusCode::OK, "{called}");
    assert_eq!(
        called["result"]["isError"], false,
        "the next minute admits it: {called}"
    );
}
