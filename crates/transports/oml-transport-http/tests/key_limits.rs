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
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::BootstrapRequest;

use support::{SEED, state};

/// Ten minutes after the fixed clock's noon, as a client writes it.
const EXPIRY_SENT: &str = "2026-10-29T12:10:00Z";
/// The same instant as the server stores and serves it.
const EXPIRY: &str = "2026-10-29T12:10:00.000000Z";

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
    );
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
