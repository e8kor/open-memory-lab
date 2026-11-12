//! Per-account usage metering over the real lead and a migrated Postgres
//! store under its role pair (ADR-0071): the counter a write moves in its
//! own transaction, so two writers racing at the limit never both commit;
//! a write replayed under its idempotency key counted once; a refused
//! write not counted; the 80% warning and the 100% refusal naming the
//! quota, its limit and what is used; a profile and an override an admin
//! assigns; the daily ledger and its export; isolation of one account's
//! usage from another's; and the drift report doctor reads. The person's
//! acts go over `/v2` as `oml-web` sends them (ADR-0070 d2, d5). Each test
//! is named after the sentence it proves.

use std::sync::atomic::Ordering;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;

use oml_app::domain::EventTime;
use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::signup::{Quota, Quotas, SignupMode};
use oml_app::domain::web::capacity_credential;
use oml_app::domain::web::{GitHubEmail, Provider};
use oml_app::doubles::MemSignIn;
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{IdGen as _, JournalStore as _};
use oml_app::types::{
    BootstrapRequest, BootstrapResult, ProviderAnswer, QuotaProfiles, SignupPolicy,
};
use oml_fetch::PinnedFetcher;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_app, start_lead_with_models};
use oml_test_pg::{DB_N, base_url, drop_db, mint_database, owner_url, run_tag, serving_url};
use oml_transport_http::session::WebClient;
use oml_transport_http::traffic::Traffic;
use oml_transport_http::web::start::SIGN_UP_COOKIE;
use oml_transport_http::web::{SESSION_COOKIE, SIGN_IN_COOKIE, WebPages};

const ISSUER: &str = "https://oml.example.com";
/// `oml-web`'s client identity, which the lead accepts a capacity
/// credential from (ADR-0070 d2).
const WEB_CLIENT_SECRET: &str = "metering-web-client-secret-0123456789";

fn fresh_name() -> String {
    format!(
        "oml_test_meter_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
}

/// A store over `db`'s serving login, migrated through its owner login with
/// `oml.serving_role` set (ADR-0034 d4, d8).
async fn serving_store(db: &str) -> PgStore {
    let store = PgStore::connect(&serving_url(db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(db));
    store
        .migrate(&owner_url(db))
        .await
        .expect("embedded migrations apply");
    store
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, String) {
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
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

fn set_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| {
            v[name.len() + 1..]
                .split(';')
                .next()
                .unwrap_or("")
                .to_owned()
        })
}

fn state_of(location: &str) -> String {
    location
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("state="))
        .expect("a state")
        .to_owned()
}

async fn tenant(store: &PgStore, key: &SigningKey, name: &str) -> BootstrapResult {
    lead_app(store, key.clone())
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse(name).expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: format!("{name}-admin"),
        })
        .await
        .expect("bootstrap")
}

const TERMS: &str = "2026-11-06";

fn policy(tenant_id: uuid::Uuid) -> SignupPolicy {
    SignupPolicy {
        mode: SignupMode::Open,
        tenant_id,
        issuer: Name::parse("root-admin").expect("a name"),
        terms_version: TERMS.to_owned(),
        terms_url: "https://example.com/terms".to_owned(),
        privacy_url: "https://example.com/privacy".to_owned(),
        per_ip_per_hour: 100,
        quotas: Quotas::default(),
    }
}

struct Rig {
    db: String,
    store: PgStore,
    boot: BootstrapResult,
    providers: MemSignIn,
    router: Router,
}

/// The profiles every test runs under: `default` with ADR-0068's limits,
/// `free`, the sign-up profile, with five memories, and `pro` with ten.
fn profiles() -> QuotaProfiles {
    let base = Quotas::default();
    QuotaProfiles {
        profiles: [
            ("default".to_owned(), base),
            ("free".to_owned(), base.with(Quota::Memories, 5)),
            ("pro".to_owned(), base.with(Quota::Memories, 10)),
        ]
        .into(),
        signup_profile: "free".to_owned(),
    }
}

/// A migrated store, the tenant `oml`, and the lead with GitHub sign-in,
/// open sign-up into that tenant, `oml-web`'s client identity and
/// [`profiles`].
async fn rig() -> Option<Rig> {
    let db = fresh_name();
    if !mint_database(&db).await {
        return None;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let boot = tenant(&store, &key, "oml").await;
    let providers = MemSignIn::new();
    let pages = WebPages::new(
        vec![(Provider::GitHub, "github-client".to_owned())],
        providers.clone(),
    )
    .with_signup(policy(boot.tenant_id));
    let router = start_lead_with_models(
        &store,
        key.clone(),
        Some(Issuer::parse(ISSUER).expect("a public origin")),
        PinnedFetcher,
        Traffic::default(),
        Some(pages),
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
        None,
        WebClient::new(Some(WEB_CLIENT_SECRET.to_owned()), None),
        false,
        Some(profiles()),
    )
    .await
    .expect("the lead starts")
    .router;
    Some(Rig {
        db,
        store,
        boot,
        providers,
        router,
    })
}

fn form(path: &str, cookie: Option<&str>, body: &str) -> Request<Body> {
    let mut b = Request::post(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin");
    if let Some(cookie) = cookie {
        b = b.header(header::COOKIE, cookie);
    }
    b.body(Body::from(body.to_owned())).expect("a request")
}

fn page(path: &str, cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::get(path);
    if let Some(cookie) = cookie {
        b = b.header(header::COOKIE, cookie);
    }
    b.body(Body::empty()).expect("a request")
}

fn api(method: &str, path: &str, key: &str, body: Option<Value>) -> Request<Body> {
    keyed(method, path, key, &format!("k-{}", fresh_name()), body)
}

/// [`api`] under the caller's own idempotency key, so the same call can be
/// sent again.
fn keyed(method: &str, path: &str, key: &str, idem: &str, body: Option<Value>) -> Request<Body> {
    let b = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header("idempotency-key", idem);
    match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string())),
        None => b.body(Body::empty()),
    }
    .expect("a request")
}

/// A call as `oml-web` makes it for the session `cookie` names: the
/// capacity's credential and the web client's identity (ADR-0070 d2).
fn as_web(
    cookie: &str,
    capacity: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Request<Body> {
    let credential = capacity_credential(cookie.split_once('=').expect("a cookie").1, capacity);
    let mut request = api(method, path, &credential, body);
    request.headers_mut().insert(
        "oml-web-client",
        WEB_CLIENT_SECRET.parse().expect("a header value"),
    );
    request
}

/// GitHub's callback for account `id` with the primary verified `email`:
/// its status, headers and body.
async fn callback(rig: &Rig, id: u64, email: &str) -> (StatusCode, HeaderMap, String) {
    let (status, headers, _) =
        send(&rig.router, form("/web/sign-in", None, "provider=github")).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let state = state_of(headers[header::LOCATION].to_str().expect("ASCII"));
    let sealed = set_cookie(&headers, SIGN_IN_COOKIE).expect("the sign-in cookie");
    rig.providers.answer(
        Provider::GitHub,
        Ok(ProviderAnswer::GitHub {
            account_id: id,
            emails: vec![GitHubEmail {
                email: email.to_owned(),
                primary: true,
                verified: true,
            }],
        }),
    );
    send(
        &rig.router,
        page(
            &format!("/web/callback/github?code=c&state={state}"),
            Some(&format!("{SIGN_IN_COOKIE}={sealed}")),
        ),
    )
    .await
}

/// A whole open sign-up: the callback's terms page, then its accepted
/// form; the session cookie, as a `Cookie` header value.
async fn sign_up(rig: &Rig, id: u64, email: &str) -> String {
    let (status, headers, body) = callback(rig, id, email).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let pending = set_cookie(&headers, SIGN_UP_COOKIE).expect("the sign-up cookie");
    let (status, headers, body) = send(
        &rig.router,
        form(
            "/web/sign-up",
            Some(&format!("{SIGN_UP_COOKIE}={pending}")),
            &format!("accept_terms=yes&terms_version={TERMS}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
    assert_eq!(headers[header::LOCATION], "/web/start");
    format!(
        "{SESSION_COOKIE}={}",
        set_cookie(&headers, SESSION_COOKIE).expect("a session")
    )
}

async fn json(rig: &Rig, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let (status, headers, body) = send(&rig.router, request).await;
    let value = serde_json::from_str(&body).unwrap_or(Value::String(body));
    (status, headers, value)
}

/// `GET /v2/whoami` as the person's `<user>-web` session key: the answer
/// the onboarding page reads its limits from (ADR-0070 d5).
async fn whoami(rig: &Rig, cookie: &str) -> Value {
    let (status, _, body) = json(rig, as_web(cookie, "web", "GET", "/v2/whoami", None)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// A writer key of the person's own, issued over `POST /v2/keys` as the
/// onboarding page issues it (ADR-0070 d5): its status and answer.
async fn issue_key(rig: &Rig, cookie: &str, suffix: &str) -> (StatusCode, Value) {
    let user = whoami(rig, cookie).await["person"]["user"]
        .as_str()
        .expect("the person's user")
        .to_owned();
    let (status, _, body) = json(
        rig,
        as_web(
            cookie,
            "web",
            "POST",
            "/v2/keys",
            Some(json!({"name": format!("{user}-{suffix}"), "role": "writer"})),
        ),
    )
    .await;
    (status, body)
}

/// The person's agent key: its value.
async fn agent_key(rig: &Rig, cookie: &str) -> String {
    let (status, body) = issue_key(rig, cookie, "claude-code").await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["value"].as_str().expect("the key's value").to_owned()
}

fn remember_body(claim_key: &str, text: &str) -> Value {
    json!({"verb": "remember", "claim_key": claim_key, "kind": "memory", "body": text})
}

async fn remember(rig: &Rig, key: &str, claim_key: &str, text: &str) -> (StatusCode, Value) {
    let (status, _, body) = json(
        rig,
        api(
            "POST",
            "/v1/memories",
            key,
            Some(remember_body(claim_key, text)),
        ),
    )
    .await;
    (status, body)
}

async fn events(rig: &Rig) -> Vec<(String, Value)> {
    rig.store
        .events_range(rig.boot.tenant_id, 1, 10_000)
        .await
        .expect("the chain")
        .into_iter()
        .map(|e| (e.envelope.event_type.clone(), e.envelope.payload.clone()))
        .collect()
}

/// `db` at `DATABASE_URL`'s host as `DATABASE_URL`'s own role, the
/// superuser that minted the pair: the hand edit a drift is made of.
fn superuser_url(db: &str) -> String {
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{prefix}/{db}")
}

/// The meter `name` of a usage answer.
fn meter<'a>(usage: &'a Value, name: &str) -> &'a Value {
    usage["meters"]
        .as_array()
        .expect("meters")
        .iter()
        .find(|m| m["meter"] == name)
        .expect("the meter")
}

async fn own_usage(rig: &Rig, key: &str) -> Value {
    let (status, _, body) = json(rig, api("GET", "/v1/account/usage", key, None)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// What the account's key reads of its six meters, in the order memories,
/// text bytes, writes, recalls, keys, connections.
async fn used(rig: &Rig, key: &str) -> [u64; 6] {
    let usage = own_usage(rig, key).await;
    [
        "memories",
        "text_bytes",
        "writes",
        "recalls",
        "keys",
        "connections",
    ]
    .map(|name| meter(&usage, name)["used"].as_u64().expect("a count"))
}

/// The person reviews the next item their reviewer key is served with
/// `verdict`, as the Review page does through the API (ADR-0070 d7 (b)).
async fn review_next(rig: &Rig, cookie: &str, verdict: &str) {
    let (status, _, served) = json(
        rig,
        as_web(cookie, "review", "POST", "/v2/review/next", Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    let serve_id = served["serve_id"].as_str().expect("an item is served");
    let (status, _, recorded) = json(
        rig,
        as_web(
            cookie,
            "review",
            "POST",
            "/v2/review/verdicts",
            Some(json!({"serve_id": serve_id, "verdict": verdict, "reason": "mine"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recorded}");
}

async fn drift(rig: &Rig) -> Value {
    let (status, _, body) = json(
        rig,
        api(
            "GET",
            "/v2/admin/usage/drift",
            &rig.boot.admin_key.value,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// Sets the account's profile and overrides as the tenant's admin.
async fn set_profile(rig: &Rig, account: &str, body: Value) {
    let (status, _, answer) = json(
        rig,
        Request::put(format!("/v2/admin/accounts/{account}/profile"))
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", rig.boot.admin_key.value),
            )
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
}

/// How many submissions the chain holds.
async fn submissions(rig: &Rig) -> usize {
    events(rig)
        .await
        .into_iter()
        .filter(|(t, _)| t.starts_with("submission."))
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_writers_racing_at_the_memories_limit_never_both_commit() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 1, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    // `free` holds five memories: four written, one left.
    for n in 0..4 {
        let (status, body) = remember(&rig, &key, &format!("k{n}"), &format!("Fact {n}.")).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    // Eight writers race for the last one, each on a task of its own.
    let mut racers = tokio::task::JoinSet::new();
    for n in 0..8 {
        let router = rig.router.clone();
        let request = api(
            "POST",
            "/v1/memories",
            &key,
            Some(
                json!({"verb": "remember", "claim_key": format!("race{n}"), "kind": "memory",
                        "body": format!("Racing fact {n}.")}),
            ),
        );
        racers.spawn(async move {
            let (status, _, body) = send(&router, request).await;
            (
                status,
                serde_json::from_str::<Value>(&body).unwrap_or(Value::String(body)),
            )
        });
    }
    let answers: Vec<(StatusCode, Value)> = racers.join_all().await;
    let won = answers
        .iter()
        .filter(|(s, _)| *s == StatusCode::ACCEPTED)
        .count();
    assert_eq!(won, 1, "{answers:?}");
    for (status, body) in answers.iter().filter(|(s, _)| *s != StatusCode::ACCEPTED) {
        assert_eq!(*status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert_eq!(body["error"]["code"], "limit.quota");
        assert_eq!(
            body["error"]["details"],
            json!({"quota": "memories", "limit": 5, "used": 5})
        );
    }
    let usage = own_usage(&rig, &key).await;
    assert_eq!(meter(&usage, "memories")["used"], 5);
    assert_eq!(meter(&usage, "writes")["used"], 5);
    // Nothing a loser sent was journalled, and the counter agrees with the
    // rows.
    let held = events(&rig)
        .await
        .into_iter()
        .filter(|(t, _)| t.starts_with("submission."))
        .count();
    assert_eq!(held, 5);
    assert_eq!(drift(&rig).await["drift"], json!([]));
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_meter_warns_at_eighty_percent_and_the_write_past_its_limit_names_quota_limit_and_used() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 2, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    let texts = ["One.", "Two two.", "Three three three."];
    for (n, text) in texts.iter().enumerate() {
        let (status, body) = remember(&rig, &key, &format!("k{n}"), text).await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    let usage = own_usage(&rig, &key).await;
    assert_eq!(usage["profile"], "free");
    assert_eq!(usage["profile_declared"], true);
    assert_eq!(
        meter(&usage, "memories"),
        &json!({"meter": "memories", "used": 3, "limit": 5, "percent": 60, "state": "ok"})
    );
    let bytes: usize = texts.iter().map(|t| t.len()).sum();
    assert_eq!(meter(&usage, "text_bytes")["used"], bytes);
    assert_eq!(meter(&usage, "text_bytes")["limit"], Value::Null);
    assert_eq!(meter(&usage, "keys")["used"], 1);

    let (status, body) = remember(&rig, &key, "k3", "Four.").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let usage = own_usage(&rig, &key).await;
    assert_eq!(meter(&usage, "memories")["state"], "warning");
    assert_eq!(meter(&usage, "memories")["percent"], 80);
    // The write that reaches the limit, then its retry under the same
    // idempotency key, which answers the record and is no new write.
    let fifth = || {
        Request::post("/v1/memories")
            .header(header::AUTHORIZATION, format!("Bearer {key}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "fifth")
            .body(Body::from(
                json!({"verb": "remember", "claim_key": "k4", "kind": "memory", "body": "Five."})
                    .to_string(),
            ))
            .expect("a request")
    };
    let (status, _, first) = json(&rig, fifth()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let (status, _, again) = json(&rig, fifth()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again, first, "the replay answers the record");
    let usage = own_usage(&rig, &key).await;
    assert_eq!(meter(&usage, "memories")["state"], "reached");
    assert_eq!(meter(&usage, "writes")["used"], 5);
    let (status, body) = remember(&rig, &key, "k5", "Six.").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(
        body["error"]["details"],
        json!({"quota": "memories", "limit": 5, "used": 5})
    );
    // Reading never blocks: a recall at the limit is answered and counted.
    let (status, _, body) = json(
        &rig,
        api(
            "POST",
            "/v1/memories/recall",
            &key,
            Some(json!({"query": "fact"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(meter(&own_usage(&rig, &key).await, "recalls")["used"], 1);
    // whoami's person, which the onboarding page reads, carries the same
    // counts and limits, an unlimited one null (ADR-0070 d5; ADR-0071 d5).
    let person = whoami(&rig, &ada).await["person"].clone();
    assert_eq!(person["under_quotas"], true, "{person}");
    assert_eq!(
        person["usage"],
        json!({"memories": 5, "text_bytes": bytes + "Four.".len() + "Five.".len(),
               "writes": 5, "keys": 1, "connections": 0})
    );
    assert_eq!(
        person["quotas"],
        json!({"memories": 5, "text_bytes": null, "writes_per_month": null, "keys": 5,
               "connections": 5, "requests_per_minute": 120})
    );
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rejected_chain_leaves_the_memories_meter_and_its_bytes_and_write_stay_counted() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 3, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    let (status, body) = remember(&rig, &key, "k0", "A fact to reject.").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(meter(&own_usage(&rig, &key).await, "memories")["used"], 1);
    review_next(&rig, &ada, "reject").await;
    let usage = own_usage(&rig, &key).await;
    assert_eq!(meter(&usage, "memories")["used"], 0);
    assert_eq!(
        meter(&usage, "text_bytes")["used"],
        "A fact to reject.".len()
    );
    assert_eq!(meter(&usage, "writes")["used"], 1);
    // A promoted chain stays one memory.
    let (status, body) = remember(&rig, &key, "k1", "A fact to keep.").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    review_next(&rig, &ada, "approve").await;
    assert_eq!(meter(&own_usage(&rig, &key).await, "memories")["used"], 1);
    assert_eq!(drift(&rig).await["drift"], json!([]));
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_admin_assigns_a_profile_and_an_override_and_the_new_limit_holds_from_the_next_write() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 4, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    let account = own_usage(&rig, &key).await["account"]
        .as_str()
        .expect("an account")
        .to_owned();
    let admin = rig.boot.admin_key.value.clone();
    let put = |body: Value, key: &str| {
        Request::put(format!("/v2/admin/accounts/{account}/profile"))
            .header(header::AUTHORIZATION, format!("Bearer {key}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request")
    };
    // An undeclared profile and an unknown quota are refused by field.
    let (status, _, body) = json(&rig, put(json!({"profile": "gold"}), &admin)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["details"]["field"], "profile");
    let (status, _, body) = json(
        &rig,
        put(
            json!({"profile": "pro", "overrides": {"bananas": 1}}),
            &admin,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["details"]["field"], "overrides");
    // The person's own key may not set a profile.
    let (status, _, body) = json(&rig, put(json!({"profile": "pro"}), &key)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let (status, _, body) = json(
        &rig,
        put(
            json!({"profile": "pro", "overrides": {"memories": 2}}),
            &admin,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["profile"], "pro");
    assert_eq!(meter(&body, "memories")["limit"], 2);
    let (_, set) = events(&rig)
        .await
        .into_iter()
        .find(|(t, _)| t == "account.profile_set")
        .expect("journalled");
    assert_eq!(set["profile"], "pro");
    assert_eq!(set["overrides"], json!({"memories": 2}));
    for n in 0..2 {
        let (status, body) = remember(&rig, &key, &format!("k{n}"), "Fact.").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    let (status, body) = remember(&rig, &key, "k2", "Fact.").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"]["details"]["limit"], 2);
    // Without the override the profile's ten hold, by user name too.
    let user = own_usage(&rig, &key).await["user"]
        .as_str()
        .expect("a user")
        .to_owned();
    let (status, _, body) = json(
        &rig,
        Request::put(format!("/v2/admin/accounts/{user}/profile"))
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"profile": "pro"}).to_string()))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(meter(&body, "memories")["limit"], 10);
    let (status, body) = remember(&rig, &key, "k2", "Fact.").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let (status, _, body) = json(
        &rig,
        api(
            "GET",
            &format!("/v2/admin/accounts/{account}/usage"),
            &admin,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(meter(&body, "memories")["used"], 3);
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_account_reads_only_its_own_usage_and_a_tenant_key_belongs_to_none() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 5, "ada@example.com").await;
    let ada_key = agent_key(&rig, &ada).await;
    let bob = sign_up(&rig, 6, "bob@example.com").await;
    let bob_key = agent_key(&rig, &bob).await;
    for n in 0..3 {
        let (status, body) = remember(&rig, &ada_key, &format!("k{n}"), "Ada's fact.").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    let ada_usage = own_usage(&rig, &ada_key).await;
    let bob_usage = own_usage(&rig, &bob_key).await;
    assert_ne!(ada_usage["account"], bob_usage["account"]);
    assert_eq!(meter(&ada_usage, "memories")["used"], 3);
    assert_eq!(meter(&bob_usage, "memories")["used"], 0);
    // No parameter names another account on the own route.
    let (status, _, body) = json(
        &rig,
        api(
            "GET",
            &format!(
                "/v1/account/usage?account={}",
                ada_usage["account"].as_str().expect("an id")
            ),
            &bob_key,
            None,
        ),
    )
    .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::BAD_REQUEST,
        "{body}"
    );
    if status == StatusCode::OK {
        assert_eq!(body["account"], bob_usage["account"]);
    }
    // A person's key is refused every admin usage route.
    for path in [
        format!(
            "/v2/admin/accounts/{}/usage",
            ada_usage["account"].as_str().expect("an id")
        ),
        "/v2/admin/usage?period=2026-11".to_owned(),
        "/v2/admin/usage/drift".to_owned(),
    ] {
        let (status, _, body) = json(&rig, api("GET", &path, &bob_key, None)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert_eq!(body["error"]["code"], "access.verb_ceiling", "{path}");
    }
    // The tenant's admin key is no account's.
    let (status, _, body) = json(
        &rig,
        api("GET", "/v1/account/usage", &rig.boot.admin_key.value, None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_rollup_appends_one_ledger_row_per_account_per_closed_day_once_and_exports_it() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 7, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    for n in 0..2 {
        let (status, body) = remember(&rig, &key, &format!("k{n}"), "Twelve bytes").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    let (status, _, _) = json(
        &rig,
        api(
            "POST",
            "/v1/memories/recall",
            &key,
            Some(json!({"query": "bytes"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let account = own_usage(&rig, &key).await["account"]
        .as_str()
        .expect("an account")
        .to_owned();
    // Tomorrow's first pass closes today.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_micros();
    let now = i64::try_from(now).expect("fits");
    let today = EventTime::from_unix_micros(now).expect("a time");
    let tomorrow = EventTime::from_unix_micros(now + 86_400_000_000).expect("a time");
    let appended = rig
        .store
        .roll_up_usage(rig.boot.tenant_id, &tomorrow)
        .await
        .expect("the rollup");
    assert!(appended >= 1, "{appended}");
    let again = rig
        .store
        .roll_up_usage(rig.boot.tenant_id, &tomorrow)
        .await
        .expect("the rollup again");
    assert_eq!(again, 0);
    let period = &today.as_str()[..7];
    let day = &today.as_str()[..10];
    let (status, _, body) = json(
        &rig,
        api(
            "GET",
            &format!("/v2/admin/usage?period={period}"),
            &rig.boot.admin_key.value,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows: Vec<&Value> = body["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter(|r| r["account"] == account.as_str())
        .collect();
    assert_eq!(rows.len(), 1, "{body}");
    let row = rows[0];
    assert_eq!(row["day"], day);
    assert_eq!(row["memories"], 2);
    assert_eq!(row["text_bytes"], 24);
    assert_eq!(row["writes"], 2);
    assert_eq!(row["recalls"], 1);
    assert_eq!(row["keys"], 1);
    // The ledger holds counts: nothing a memory or a query said.
    assert!(!body.to_string().contains("Twelve"), "{body}");
    // The CSV export carries the same row under its header.
    let (status, headers, csv) = send(
        &rig.router,
        api(
            "GET",
            &format!("/v2/admin/usage?period={period}&format=csv"),
            &rig.boot.admin_key.value,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{csv}");
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/csv")
    );
    let mut lines = csv.lines();
    assert_eq!(
        lines.next(),
        Some("day,account,user,profile,memories,text_bytes,writes,recalls,keys,connections")
    );
    assert!(
        lines.any(|l| l.starts_with(&format!("{day},{account},")) && l.ends_with(",2,24,2,1,1,0")),
        "{csv}"
    );
    // A malformed period is refused by name.
    let (status, _, body) = json(
        &rig,
        api(
            "GET",
            "/v2/admin/usage?period=2026-13",
            &rig.boot.admin_key.value,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["details"]["field"], "period");
    // The ledger is never updated by the serving role.
    let mut serving = PgConnection::connect(&serving_url(&rig.db))
        .await
        .expect("serving connect");
    serving
        .execute(format!("SET oml.tenant_id = '{}'", rig.boot.tenant_id).as_str())
        .await
        .expect("tenant");
    let refused = serving
        .execute("UPDATE usage_daily SET memories = 0")
        .await
        .expect_err("no UPDATE grant");
    assert!(
        refused.to_string().contains("permission denied"),
        "{refused}"
    );
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_drift_report_is_empty_after_metered_writes_and_names_a_counter_edited_by_hand() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 8, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    for n in 0..2 {
        let (status, body) = remember(&rig, &key, &format!("k{n}"), "Fact.").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    let report = drift(&rig).await;
    assert_eq!(report, json!({"drift": [], "undeclared_profiles": []}));
    let account = own_usage(&rig, &key).await["account"]
        .as_str()
        .expect("an account")
        .to_owned();
    let mut su = PgConnection::connect(&superuser_url(&rig.db))
        .await
        .expect("superuser connect");
    su.execute(
        format!("UPDATE account_meters SET memories = memories + 5 WHERE user_root = '{account}'")
            .as_str(),
    )
    .await
    .expect("the hand edit");
    su.execute(
        format!(
            "INSERT INTO account_profiles (tenant_id, user_root, profile) VALUES ('{}', '{account}', 'gold')",
            rig.boot.tenant_id
        )
        .as_str(),
    )
    .await
    .expect("an undeclared profile");
    let report = drift(&rig).await;
    assert_eq!(
        report["drift"],
        json!([{"account": account, "user": own_usage(&rig, &key).await["user"], "meter": "memories", "counter": 7, "rows": 2}])
    );
    assert_eq!(report["undeclared_profiles"][0]["profile"], "gold");
    // An undeclared profile holds every limit at zero: the next write is
    // refused, and reading still answers.
    let usage = own_usage(&rig, &key).await;
    assert_eq!(usage["profile_declared"], false);
    let (status, body) = remember(&rig, &key, "k9", "Fact.").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_replayed_under_its_idempotency_key_is_counted_once_and_answers_its_record_at_the_limit()
 {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 9, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    let account = own_usage(&rig, &key).await["account"]
        .as_str()
        .expect("an account")
        .to_owned();
    // One memory, two writes and exactly the bytes of its two versions: the
    // first write reaches the memories limit and its revision the others.
    let v1 = "The canary drains first.";
    let v2 = "The canary drains and settles first.";
    set_profile(
        &rig,
        &account,
        json!({"profile": "free", "overrides": {
            "memories": 1, "text_bytes": v1.len() + v2.len(), "writes_per_month": 2}}),
    )
    .await;
    let first = || {
        keyed(
            "POST",
            "/v1/memories",
            &key,
            "first",
            Some(remember_body("canary", v1)),
        )
    };
    // Two sends at once, as a client that timed out and retried: one write.
    let ((s1, _, a1), (s2, _, a2)) =
        tokio::join!(send(&rig.router, first()), send(&rig.router, first()));
    assert!(
        s1 == StatusCode::ACCEPTED || s2 == StatusCode::ACCEPTED,
        "{a1} {a2}"
    );
    let once = [1, v1.len() as u64, 1, 0, 1, 0];
    assert_eq!(used(&rig, &key).await, once);
    assert_eq!(submissions(&rig).await, 1);
    // Sent again at the memories limit: the record answers, not limit.quota.
    let (status, _, held) = json(&rig, first()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{held}");
    let (status, _, again) = json(&rig, first()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert_eq!(again["artifact_id"], held["artifact_id"]);
    assert_eq!(used(&rig, &key).await, once);
    assert_eq!(submissions(&rig).await, 1);

    // The revision, which reaches the bytes and the monthly writes, and its
    // replays, alone and two at once.
    review_next(&rig, &ada, "approve").await;
    let artifact = held["artifact_id"].as_str().expect("an id").to_owned();
    let revise = || {
        keyed(
            "POST",
            &format!("/v1/memories/{artifact}/supersede"),
            &key,
            "revise",
            Some(json!({"expected_version": 1, "body": v2})),
        )
    };
    let (status, _, successor) = json(&rig, revise()).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{successor}");
    let twice = [1, (v1.len() + v2.len()) as u64, 2, 0, 1, 0];
    assert_eq!(used(&rig, &key).await, twice);
    let ((s1, _, a1), (s2, _, a2)) =
        tokio::join!(send(&rig.router, revise()), send(&rig.router, revise()));
    assert_eq!(
        (s1, s2),
        (StatusCode::ACCEPTED, StatusCode::ACCEPTED),
        "{a1} {a2}"
    );
    for answer in [a1, a2] {
        let answer: Value = serde_json::from_str(&answer).expect("JSON");
        assert_eq!(answer["artifact_id"], successor["artifact_id"]);
    }
    assert_eq!(used(&rig, &key).await, twice);
    assert_eq!(submissions(&rig).await, 2);
    assert_eq!(drift(&rig).await["drift"], json!([]));
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_write_is_not_counted_and_at_the_limit_a_live_claim_key_is_the_conflict_it_was() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 10, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    let (status, _, held) = json(
        &rig,
        keyed(
            "POST",
            "/v1/memories",
            &key,
            "first",
            Some(remember_body("k0", "A fact.")),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{held}");
    let counted = used(&rig, &key).await;
    assert_eq!(counted, [1, "A fact.".len() as u64, 1, 0, 1, 0]);
    let artifact = held["artifact_id"].as_str().expect("an id");
    // Each refusal names its own rule and moves no meter: a live claim_key,
    // the first write's idempotency key under another body, a lineage the
    // key cannot read, a body the scan refuses, and a revision of a version
    // that is not the served tip.
    let refused: [(Request<Body>, StatusCode, &str); 5] = [
        (
            api(
                "POST",
                "/v1/memories",
                &key,
                Some(remember_body("k0", "Another fact under the live claim.")),
            ),
            StatusCode::CONFLICT,
            "submission.claim_key_conflict",
        ),
        (
            keyed(
                "POST",
                "/v1/memories",
                &key,
                "first",
                Some(remember_body("k0", "Another body under the first key.")),
            ),
            StatusCode::CONFLICT,
            "idempotency.digest_mismatch",
        ),
        (
            api(
                "POST",
                "/v1/memories",
                &key,
                Some(
                    json!({"verb": "learn", "claim_key": "k1", "kind": "derived_fact",
                            "body": "Derived from nothing readable.",
                            "derives_from": ["00000000-0000-4000-8000-00000000dead"]}),
                ),
            ),
            StatusCode::NOT_FOUND,
            "access.not_found",
        ),
        (
            api(
                "POST",
                "/v1/memories",
                &key,
                Some(remember_body(
                    "k2",
                    "The deploy key is -----BEGIN OPENSSH PRIVATE KEY----- and more.",
                )),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
            "submission.scan_veto",
        ),
        (
            api(
                "POST",
                &format!("/v1/memories/{artifact}/supersede"),
                &key,
                Some(json!({"expected_version": 1, "body": "A revision of a held version."})),
            ),
            StatusCode::CONFLICT,
            "submission.tip_conflict",
        ),
    ];
    for (request, expected, code) in refused {
        let (status, _, body) = json(&rig, request).await;
        assert_eq!(status, expected, "{code}: {body}");
        assert_eq!(body["error"]["code"], code, "{body}");
        assert_eq!(used(&rig, &key).await, counted, "{code}");
    }
    assert_eq!(submissions(&rig).await, 1);
    // At the memories limit of `free`, five, a write under a live claim_key
    // is still the conflict it was, and a new claim is the quota, which is
    // not counted either.
    for n in 1..5 {
        let (status, body) = remember(&rig, &key, &format!("k{n}"), "A fact.").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    }
    let full = used(&rig, &key).await;
    assert_eq!(full[0], 5);
    let (status, body) = remember(&rig, &key, "k0", "Under the live claim.").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "submission.claim_key_conflict");
    let (status, body) = remember(&rig, &key, "k5", "One too many.").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"]["code"], "limit.quota");
    assert_eq!(used(&rig, &key).await, full);
    assert_eq!(submissions(&rig).await, 5);
    assert_eq!(drift(&rig).await["drift"], json!([]));
    drop_db(&rig.db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_write_over_any_quota_is_refused_as_limit_quota_naming_it_and_journals_and_counts_nothing()
 {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 11, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    let account = own_usage(&rig, &key).await["account"]
        .as_str()
        .expect("an account")
        .to_owned();
    let v1 = "Twenty bytes of text";
    assert_eq!(v1.len(), 20);
    let (status, _, held) = json(
        &rig,
        keyed(
            "POST",
            "/v1/memories",
            &key,
            "first",
            Some(remember_body("k0", v1)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{held}");
    review_next(&rig, &ada, "approve").await;
    let artifact = held["artifact_id"].as_str().expect("an id").to_owned();
    let counted = used(&rig, &key).await;
    assert_eq!(counted, [1, 20, 1, 0, 1, 0]);
    let journalled = events(&rig).await.len();
    // Each quota in turn is the one the next write would pass: a new memory
    // and a revision are refused naming it, its limit and what is used.
    for (overrides, quota, limit, used_now, revision_too) in [
        (json!({"memories": 1}), "memories", 1, 1, false),
        (json!({"text_bytes": 30}), "text_bytes", 30, 20, true),
        (
            json!({"writes_per_month": 1}),
            "writes_per_month",
            1,
            1,
            true,
        ),
    ] {
        set_profile(
            &rig,
            &account,
            json!({"profile": "free", "overrides": overrides}),
        )
        .await;
        let mut writes = vec![api(
            "POST",
            "/v1/memories",
            &key,
            Some(remember_body("k1", "Eleven byte")),
        )];
        if revision_too {
            writes.push(api(
                "POST",
                &format!("/v1/memories/{artifact}/supersede"),
                &key,
                Some(json!({"expected_version": 1, "body": "Eleven byte"})),
            ));
        }
        for request in writes {
            let (status, _, body) = json(&rig, request).await;
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{quota}: {body}");
            assert_eq!(body["error"]["code"], "limit.quota", "{body}");
            assert_eq!(
                body["error"]["details"],
                json!({"quota": quota, "limit": limit, "used": used_now}),
                "{body}"
            );
        }
        assert_eq!(used(&rig, &key).await, counted, "{quota}");
    }
    // Three account.profile_set events and no submission since.
    assert_eq!(events(&rig).await.len(), journalled + 3);
    assert_eq!(submissions(&rig).await, 1);
    // A sixth key is refused the same way, and the keys meter stays.
    for n in 0..4 {
        let (status, body) = issue_key(&rig, &ada, &format!("agent-{n}")).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let (status, body) = issue_key(&rig, &ada, "agent-4").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(
        body["error"]["details"],
        json!({"quota": "keys", "limit": 5, "used": 5})
    );
    assert_eq!(used(&rig, &key).await[4], 5);
    assert_eq!(drift(&rig).await["drift"], json!([]));
    drop_db(&rig.db).await;
}

/// [`issue_key`] under the caller's own idempotency key: the status, whether
/// the answer is a replay, and the body.
async fn issue_key_under(
    rig: &Rig,
    cookie: &str,
    idem: &str,
    suffix: &str,
) -> (StatusCode, bool, Value) {
    let user = whoami(rig, cookie).await["person"]["user"]
        .as_str()
        .expect("the person's user")
        .to_owned();
    let mut request = as_web(
        cookie,
        "web",
        "POST",
        "/v2/keys",
        Some(json!({"name": format!("{user}-{suffix}"), "role": "writer"})),
    );
    request
        .headers_mut()
        .insert("idempotency-key", idem.parse().expect("a header value"));
    let (status, headers, body) = json(rig, request).await;
    let replayed = headers
        .get("idempotency-replay")
        .is_some_and(|v| v == "true");
    (status, replayed, body)
}

/// A key issuance claims its idempotency key before it acts (#259), and the
/// keys meter holds under it: two sends at once of the issuance that
/// reaches the limit are one key, a later send answers its record at the
/// limit and never `limit.quota`, and an issuance past the limit is
/// refused naming the quota, counts nothing and releases its claim, so the
/// same key acts once the limit is raised (ADR-0071 d2, d3; http-api.md
/// idempotency).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_key_issuance_sent_again_under_its_claim_is_one_key_at_the_limit_and_a_refused_one_holds_no_claim()
 {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 12, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    let account = own_usage(&rig, &key).await["account"]
        .as_str()
        .expect("an account")
        .to_owned();
    for n in 0..3 {
        let (status, body) = issue_key(&rig, &ada, &format!("agent-{n}")).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    assert_eq!(used(&rig, &key).await[4], 4);
    // The fifth key reaches the limit: two sends at once issue it once.
    let ((s1, r1, a1), (s2, r2, a2)) = tokio::join!(
        issue_key_under(&rig, &ada, "fifth", "agent-3"),
        issue_key_under(&rig, &ada, "fifth", "agent-3"),
    );
    assert_eq!(
        (s1, s2),
        (StatusCode::CREATED, StatusCode::CREATED),
        "{a1} {a2}"
    );
    assert!(r1 != r2, "one send acts and the other replays it");
    assert_eq!(a1["key_id"], a2["key_id"]);
    assert_eq!(used(&rig, &key).await[4], 5);
    // Sent again at the limit: the record answers, not limit.quota.
    let (status, replayed, again) = issue_key_under(&rig, &ada, "fifth", "agent-3").await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert!(replayed, "{again}");
    assert_eq!(again["key_id"], a1["key_id"]);
    assert_eq!(used(&rig, &key).await[4], 5);

    // A sixth is refused naming the quota, twice under one key, and counts
    // nothing.
    for _ in 0..2 {
        let (status, replayed, body) = issue_key_under(&rig, &ada, "sixth", "agent-4").await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
        assert!(!replayed, "a refusal records nothing: {body}");
        assert_eq!(body["error"]["code"], "limit.quota", "{body}");
        assert_eq!(
            body["error"]["details"],
            json!({"quota": "keys", "limit": 5, "used": 5})
        );
    }
    assert_eq!(used(&rig, &key).await[4], 5);
    // The refusal released its claim: once an admin raises the limit the
    // same key acts, once.
    set_profile(
        &rig,
        &account,
        json!({"profile": "free", "overrides": {"keys": 6}}),
    )
    .await;
    let (status, replayed, sixth) = issue_key_under(&rig, &ada, "sixth", "agent-4").await;
    assert_eq!(status, StatusCode::CREATED, "{sixth}");
    assert!(!replayed, "{sixth}");
    let (status, replayed, again) = issue_key_under(&rig, &ada, "sixth", "agent-4").await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert!(replayed, "{again}");
    assert_eq!(again["key_id"], sixth["key_id"]);
    assert_eq!(used(&rig, &key).await[4], 6);
    assert_eq!(drift(&rig).await["drift"], json!([]));
    drop_db(&rig.db).await;
}

/// A recall never waits on its account's meter: it answers while another
/// session holds the meters' tables, and its count arrives once they are
/// free (ADR-0071 d2, d5, d8).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_recall_answers_while_its_accounts_meter_is_locked_and_its_count_arrives_after() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 13, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    let (status, body) = remember(&rig, &key, "k0", "The canary drains first.").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let recall = || {
        api(
            "POST",
            "/v1/memories/recall",
            &key,
            Some(json!({"query": "canary"})),
        )
    };
    let (status, _, body) = json(&rig, recall()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(used(&rig, &key).await[3], 1);

    let mut locker = PgConnection::connect(&superuser_url(&rig.db))
        .await
        .expect("superuser connect");
    locker.execute("BEGIN").await.expect("begin");
    locker
        .execute("LOCK TABLE account_meters, account_activity IN ACCESS EXCLUSIVE MODE")
        .await
        .expect("lock the meters");
    let answered =
        tokio::time::timeout(std::time::Duration::from_secs(5), json(&rig, recall())).await;
    locker.execute("ROLLBACK").await.expect("unlock");
    let (status, _, body) = answered.expect("the recall answers while its meter is locked");
    assert_eq!(status, StatusCode::OK, "{body}");
    // Reading the usage waits for the counts handed off before it.
    assert_eq!(used(&rig, &key).await[3], 2, "the count arrives");
    drop_db(&rig.db).await;
}

/// The ledger names the profile each account is on when its day closes:
/// the one an admin assigned, and otherwise the sign-up profile, as the
/// usage read names it, so the export keeps it after the configuration
/// moves on (ADR-0071 d4, d5).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ledger_names_the_profile_each_account_is_on_assigned_or_not() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 14, "ada@example.com").await;
    let bob = sign_up(&rig, 15, "bob@example.com").await;
    let (ada_key, bob_key) = (agent_key(&rig, &ada).await, agent_key(&rig, &bob).await);
    let mut accounts = Vec::new();
    for key in [&ada_key, &bob_key] {
        let (status, body) = remember(&rig, key, "k0", "A fact.").await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        accounts.push(
            own_usage(&rig, key).await["account"]
                .as_str()
                .expect("an account")
                .to_owned(),
        );
    }
    set_profile(&rig, &accounts[1], json!({"profile": "pro"})).await;
    assert_eq!(own_usage(&rig, &ada_key).await["profile"], "free");
    assert_eq!(own_usage(&rig, &bob_key).await["profile"], "pro");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_micros();
    let now = i64::try_from(now).expect("fits");
    let today = EventTime::from_unix_micros(now).expect("a time");
    let tomorrow = EventTime::from_unix_micros(now + 86_400_000_000).expect("a time");
    let (tenants, appended) = oml_server_lead::roll_up_usage(&rig.store, &tomorrow)
        .await
        .expect("the rollup");
    assert!(tenants >= 1 && appended >= 2, "{tenants} {appended}");
    let period = &today.as_str()[..7];
    let (status, _, body) = json(
        &rig,
        api(
            "GET",
            &format!("/v2/admin/usage?period={period}"),
            &rig.boot.admin_key.value,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let profile_of = |account: &str| {
        body["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|r| r["account"] == account)
            .unwrap_or_else(|| panic!("a row of {account}: {body}"))["profile"]
            .clone()
    };
    assert_eq!(profile_of(&accounts[0]), "free", "the sign-up profile");
    assert_eq!(profile_of(&accounts[1]), "pro", "the assigned profile");
    drop_db(&rig.db).await;
}

/// An admin takes an account's profile and overrides away with `DELETE`,
/// journalled as `account.profile_set` with no profile, and the account is
/// on the deployment's default again; no other key may (ADR-0071 d5).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_admin_unassigns_an_accounts_profile_and_it_is_on_the_deployments_default_again() {
    let Some(rig) = rig().await else { return };
    let ada = sign_up(&rig, 16, "ada@example.com").await;
    let key = agent_key(&rig, &ada).await;
    let account = own_usage(&rig, &key).await["account"]
        .as_str()
        .expect("an account")
        .to_owned();
    set_profile(
        &rig,
        &account,
        json!({"profile": "pro", "overrides": {"memories": 2}}),
    )
    .await;
    let usage = own_usage(&rig, &key).await;
    assert_eq!(usage["profile"], "pro");
    assert_eq!(meter(&usage, "memories")["limit"], 2);
    let delete = |key: &str| {
        Request::delete(format!("/v2/admin/accounts/{account}/profile"))
            .header(header::AUTHORIZATION, format!("Bearer {key}"))
            .body(Body::empty())
            .expect("a request")
    };
    // The person's own key may not.
    let (status, _, body) = json(&rig, delete(&key)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "access.verb_ceiling", "{body}");
    assert_eq!(own_usage(&rig, &key).await["profile"], "pro");

    let journalled = events(&rig).await.len();
    let (status, _, body) = json(&rig, delete(&rig.boot.admin_key.value)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["profile"], "free", "the sign-up profile again: {body}");
    assert_eq!(meter(&body, "memories")["limit"], 5, "and no override");
    let all = events(&rig).await;
    assert_eq!(all.len(), journalled + 1);
    let (event_type, payload) = all.last().expect("an event");
    assert_eq!(event_type, "account.profile_set");
    assert_eq!(payload["user_root"], account.as_str());
    assert_eq!(payload["profile"], Value::Null);
    assert_eq!(payload["overrides"], json!({}));
    // An account that has none answers the same and journals the act.
    let (status, _, body) = json(&rig, delete(&rig.boot.admin_key.value)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["profile"], "free");
    // An account that is not one of the tenant's is not found.
    let (status, _, body) = json(
        &rig,
        Request::delete("/v2/admin/accounts/nobody/profile")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", rig.boot.admin_key.value),
            )
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    drop_db(&rig.db).await;
}
