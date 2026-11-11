//! The web pages over the real lead and a migrated Postgres store under its
//! role pair (ADR-0045 d2, d4, d5; ADR-0034 d8): the allowlist's routes and
//! folds, the session rows and the sign-in under the serving role's RLS. The
//! sign-in provider is the in-memory double: no call reaches Google or
//! GitHub. Each test is named after the sentence it proves.

use std::sync::atomic::Ordering;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::web::{GitHubEmail, Provider, capacity_credential};
use oml_app::doubles::MemSignIn;
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{IdGen as _, JournalStore as _};
use oml_app::types::{BootstrapRequest, BootstrapResult, ProviderAnswer};
use oml_fetch::PinnedFetcher;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_app, start_lead_with_models, sweep_connections};
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};
use oml_transport_http::session::WebClient;
use oml_transport_http::traffic::Traffic;
use oml_transport_http::web::{SESSION_COOKIE, SIGN_IN_COOKIE, WebPages};

const ISSUER: &str = "https://oml.example.com";
/// The web client secret the lead is configured with, which `oml-web`
/// presents beside a session's capacity credential (ADR-0070 d2).
const WEB_CLIENT_SECRET: &str = "pg-web-client-secret-0123456789";

fn fresh_name() -> String {
    format!(
        "oml_test_web_{}_{}_{}",
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

fn json_post(path: &str, admin: &str, body: Value) -> Request<Body> {
    Request::post(path)
        .header(header::AUTHORIZATION, format!("Bearer {admin}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("idempotency-key", "k")
        .body(Body::from(body.to_string()))
        .expect("a request")
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

/// The lead over `store` with GitHub sign-in on through `providers`.
async fn lead(store: &PgStore, key: &SigningKey, providers: &MemSignIn) -> Router {
    lead_traced(store, key, providers, Traffic::default()).await
}

/// [`lead`] whose request lines and metrics go to `traffic`, which the
/// caller keeps to read the metrics back.
async fn lead_traced(
    store: &PgStore,
    key: &SigningKey,
    providers: &MemSignIn,
    traffic: Traffic,
) -> Router {
    start_lead_with_models(
        store,
        key.clone(),
        Some(Issuer::parse(ISSUER).expect("a public origin")),
        PinnedFetcher,
        traffic,
        Some(WebPages::new(
            vec![(Provider::GitHub, "github-client".to_owned())],
            providers.clone(),
        )),
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
        None,
        WebClient::new(Some(WEB_CLIENT_SECRET.to_owned()), None),
        false,
    )
    .await
    .expect("the lead starts")
    .router
}

/// A GitHub sign-in of account `id` whose primary verified address is
/// `email`: the callback's status and the session token it set.
async fn sign_in(
    router: &Router,
    providers: &MemSignIn,
    id: u64,
    email: &str,
) -> (StatusCode, Option<String>) {
    let (status, headers, _) = send(
        router,
        Request::post("/web/sign-in")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header("sec-fetch-site", "same-origin")
            .body(Body::from("provider=github"))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let state = state_of(headers[header::LOCATION].to_str().expect("ASCII"));
    let sealed = set_cookie(&headers, SIGN_IN_COOKIE).expect("the sign-in cookie");
    providers.answer(
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
    let (status, headers, _) = send(
        router,
        Request::get(format!("/web/callback/github?code=c&state={state}"))
            .header(header::COOKIE, format!("{SIGN_IN_COOKIE}={sealed}"))
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    (status, set_cookie(&headers, SESSION_COOKIE))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_admin_email_is_added_under_an_admin_key_and_signs_in_through_postgres() {
    let db = fresh_name();
    if !mint_database(&db).await {
        return;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let boot = tenant(&store, &key, "oml").await;
    let providers = MemSignIn::new();
    let router = lead(&store, &key, &providers).await;

    let (status, _, body) = send(
        &router,
        json_post(
            "/v2/web/emails",
            &boot.admin_key.value,
            json!({"email": "Alice@Example.com", "user": "alice", "admin": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The journal holds the digest and never the address.
    let events = store
        .events_range(boot.tenant_id, 1, 100)
        .await
        .expect("the chain");
    let allowed = events
        .iter()
        .find(|e| e.envelope.event_type == "web.email_allowed")
        .expect("web.email_allowed");
    let payload = allowed.envelope.payload.to_string();
    assert!(
        payload.contains("email_digest") && payload.contains("sha256:"),
        "{payload}"
    );
    assert!(
        !payload.to_ascii_lowercase().contains("example.com"),
        "{payload}"
    );

    let (status, token) = sign_in(&router, &providers, 7, "alice@example.com").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    let (status, _, page) = send(
        &router,
        Request::get("/web/admin")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "an admin entry opens the admin page"
    );
    assert!(page.contains("alice · oml"), "{page}");

    // The first sign-in issued alice's session keys under the serving role:
    // credential `session` and `session_user` round-trip through the store.
    let web = store
        .resolve_name(
            boot.tenant_id,
            oml_app::domain::name::NamedKind::Key,
            &Name::parse("alice-web").expect("a name"),
        )
        .await
        .expect("a lookup")
        .expect("alice-web exists");
    let web = store.key(boot.tenant_id, web).await.unwrap().unwrap();
    assert_eq!(web.credential.as_str(), "session");
    assert_eq!(web.session_user.as_ref().map(Name::as_str), Some("alice"));
    assert_eq!(web.key_hash, None);
    // The admin read runs under the serving role and journals itself.
    let (status, _, body) = send(
        &router,
        json_post(
            "/v2/admin/memories/search",
            &boot.admin_key.value,
            json!({"query": "anything", "state": "held"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The queue the admin's Review page reads, which oml-web serves, runs
    // under the serving role as the person's admin capacity (ADR-0070 d7
    // (b)).
    let (status, _, page) = send(
        &router,
        as_web(&cookie, "admin", "GET", "/v2/review/queue", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");

    // A moved account is refused under Postgres's pin as under the double's.
    let (status, token) = sign_in(&router, &providers, 8, "alice@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(token, None);

    // Removing the email ends its session in the same commit.
    let (status, _, body) = send(
        &router,
        json_post(
            "/v2/web/emails/remove",
            &boot.admin_key.value,
            json!({"email": "alice@example.com"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, _, page) = send(
        &router,
        Request::get("/web")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert!(page.contains("Sign in with GitHub"), "signed out: {page}");
    let sweep = sweep_connections(&store, &oml_transport_http::time::system_event_time())
        .await
        .expect("the sweep runs over the web sessions too");
    assert_eq!(sweep.web_sessions_removed, 0);
    drop_db(&db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_email_is_unique_in_the_deployment() {
    let db = fresh_name();
    if !mint_database(&db).await {
        return;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let first = tenant(&store, &key, "oml").await;
    let second = tenant(&store, &key, "acme").await;
    let router = lead(&store, &key, &MemSignIn::new()).await;
    let allow = |admin: &str, user: &str| {
        json_post(
            "/v2/web/emails",
            admin,
            json!({"email": "carol@example.com", "user": user, "admin": false}),
        )
    };
    let (status, _, body) = send(&router, allow(&first.admin_key.value, "carol")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let head = store
        .chain_head(second.tenant_id)
        .await
        .expect("a head")
        .expect("a chain")
        .seq;
    let (status, _, body) = send(&router, allow(&second.admin_key.value, "carol")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let refusal: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(refusal["error"]["code"], "name.taken");
    assert_eq!(refusal["error"]["details"], json!({"kind": "email"}));
    assert_eq!(
        store
            .chain_head(second.tenant_id)
            .await
            .expect("a head")
            .expect("a chain")
            .seq,
        head,
        "nothing journalled"
    );
    // The other tenant's admin cannot list or remove it either.
    let (status, _, _) = send(
        &router,
        json_post(
            "/v2/web/emails/remove",
            &second.admin_key.value,
            json!({"email": "carol@example.com"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    drop_db(&db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_user_named_like_a_shared_workarea_is_name_taken() {
    let db = fresh_name();
    if !mint_database(&db).await {
        return;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let boot = tenant(&store, &key, "oml").await;
    lead_app(&store, key.clone())
        .create_workarea(
            &boot.admin_key.value,
            &Name::parse("research").expect("a name"),
            Some(boot.root_workarea_id.into()),
            None,
        )
        .await
        .expect("a shared workarea");
    let router = lead(&store, &key, &MemSignIn::new()).await;
    let (status, _, body) = send(
        &router,
        json_post(
            "/v2/web/emails",
            &boot.admin_key.value,
            json!({"email": "r@example.com", "user": "research", "admin": false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let refusal: Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(refusal["error"]["details"]["kind"], "workarea");
    let (status, _, body) = send(
        &router,
        json_post(
            "/v2/web/emails",
            &boot.admin_key.value,
            json!({"email": "r@example.com", "user": "a".repeat(57), "admin": false}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    drop_db(&db).await;
}

/// A call as `oml-web` makes it for the session `cookie` names: the
/// capacity's credential, the web client's identity and, on a `POST`, the
/// idempotency key `idem` (ADR-0070 d2, d3 layer 4).
fn as_web(
    cookie: &str,
    capacity: &str,
    method: &str,
    path: &str,
    body: Option<(&str, Value)>,
) -> Request<Body> {
    let token = cookie.split_once('=').expect("a cookie").1;
    let builder = Request::builder()
        .method(method)
        .uri(path)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", capacity_credential(token, capacity)),
        )
        .header("oml-web-client", WEB_CLIENT_SECRET);
    match body {
        Some((idem, body)) => builder
            .header("idempotency-key", idem)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("a request")
}

/// Bulk review under Postgres as the Review page, which `oml-web` serves,
/// makes it (ADR-0055 d10; ADR-0070 d7 (b)): the listing serves each held
/// item's text to the person's reviewer key, the confirmation's read by id
/// records nothing, the bulk verdict records one verdict per item, and a
/// resubmission replays.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bulk_review_lists_confirms_by_id_and_records_each_outcome_once() {
    let db = fresh_name();
    if !mint_database(&db).await {
        return;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let boot = tenant(&store, &key, "oml").await;
    let providers = MemSignIn::new();
    let traffic = Traffic::default();
    let router = lead_traced(&store, &key, &providers, traffic.clone()).await;
    let admin = boot.admin_key.value.clone();
    let (status, _, body) = send(
        &router,
        json_post(
            "/v2/web/emails",
            &admin,
            json!({"email": "alice@example.com", "user": "alice", "admin": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, token) = sign_in(&router, &providers, 7, "alice@example.com").await;
    let cookie = format!("{SESSION_COOKIE}={}", token.expect("a session"));
    // Alice's agent writes three held decisions into her root.
    let (status, _, issued) = send(
        &router,
        json_post(
            "/v2/keys",
            &admin,
            json!({"role": "writer", "name": "alice-loader", "principal": "alice/loader",
                   "user": "alice", "grants": [
                       {"workarea": "alice", "role": "writer"},
                       {"workarea": "alice", "role": "reader"}]}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{issued}");
    let loader: Value = serde_json::from_str(&issued).expect("JSON");
    let loader = loader["value"].as_str().expect("a value").to_owned();
    let mut ids = Vec::new();
    for claim_key in ["adr-0001-d1", "adr-0001-d2", "adr-0001-rejected"] {
        let (status, _, body) = send(
            &router,
            Request::post("/v1/memories")
                .header(header::AUTHORIZATION, format!("Bearer {loader}"))
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", claim_key)
                .body(Body::from(
                    json!({"claim_key": claim_key, "body": format!("Decision {claim_key}.")})
                        .to_string(),
                ))
                .expect("a request"),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        let v: Value = serde_json::from_str(&body).expect("JSON");
        ids.push(v["artifact_id"].as_str().expect("an id").to_owned());
    }
    // The Review page oml-web serves reads and acts through these
    // operations as the person's reviewer key; its page assertions are in
    // oml-web's suite (ADR-0070 d7 (b)).
    let held = |query: &str| {
        as_web(
            &cookie,
            "review",
            "GET",
            &format!("/v2/review/held{query}"),
            None,
        )
    };
    let (status, _, page) = send(&router, held("")).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let listing: Value = serde_json::from_str(&page).expect("JSON");
    let listed: Vec<&str> = listing["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["artifact_id"].as_str().expect("an id"))
        .collect();
    assert_eq!(listed, ids.iter().map(String::as_str).collect::<Vec<_>>());
    assert!(
        page.contains("Decision adr-0001-d2."),
        "the text is served: {page}"
    );
    assert!(
        listing["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["same_user"] == true),
        "same user is named: {page}"
    );
    let (_, _, narrowed) = send(&router, held("?claim_key_prefix=adr-0001-d")).await;
    let narrowed: Value = serde_json::from_str(&narrowed).expect("JSON");
    assert_eq!(narrowed["items"].as_array().unwrap().len(), 2);

    // The confirmation reads the picks by id and records nothing.
    let (status, _, confirm) = send(&router, held(&format!("?ids={},{}", ids[0], ids[1]))).await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    for claim_key in ["adr-0001-d1", "adr-0001-d2"] {
        assert!(
            confirm.contains(&format!("Decision {claim_key}.")),
            "{confirm}"
        );
    }
    assert!(
        !confirm.contains("adr-0001-rejected"),
        "only the picks: {confirm}"
    );
    let (_, _, still) = send(&router, held("")).await;
    assert!(
        ids.iter().all(|id| still.contains(id.as_str())),
        "nothing judged yet: {still}"
    );

    let verdict = || {
        as_web(
            &cookie,
            "review",
            "POST",
            "/v2/review/verdicts/bulk",
            Some((
                "web-act-bulk-1",
                json!({"verdict": "approve", "artifact_ids": [ids[0], ids[1]]}),
            )),
        )
    };
    let (status, _, outcome) = send(&router, verdict()).await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    let outcome: Value = serde_json::from_str(&outcome).expect("JSON");
    assert_eq!(
        outcome["summary"],
        json!({"approved": 2, "rejected": 0, "skipped": 0})
    );
    assert!(
        outcome["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["same_user"] == true)
    );
    let (_, _, again) = send(&router, verdict()).await;
    assert!(
        again.contains(r#""replayed":true"#),
        "a resubmitted form replays: {again}"
    );
    let (_, _, left) = send(&router, held("")).await;
    assert!(left.contains(&ids[2]) && !left.contains(&ids[0]), "{left}");
    // No reason is journalled as none.
    let events = store
        .events_range(boot.tenant_id, 1, 1_000)
        .await
        .expect("journal");
    let approvals: Vec<_> = events
        .iter()
        .filter(|e| e.envelope.event_type == "review.approve")
        .collect();
    assert_eq!(approvals.len(), 2);
    assert!(
        approvals
            .iter()
            .all(|e| e.envelope.payload["reason"].is_null()),
        "{:?}",
        approvals
            .iter()
            .map(|e| &e.envelope.payload)
            .collect::<Vec<_>>()
    );
    // A single approve through the same session reviewer key is marked
    // the same person as the bulk ones are (ADR-0055 d7).
    let (status, _, served) = send(
        &router,
        as_web(
            &cookie,
            "review",
            "POST",
            "/v2/review/next",
            Some(("web-act-next-1", json!({}))),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    let served: Value = serde_json::from_str(&served).expect("JSON");
    let (status, _, body) = send(
        &router,
        as_web(
            &cookie,
            "review",
            "POST",
            "/v2/review/verdicts",
            Some((
                "web-act-verdict-1",
                json!({"serve_id": served["serve_id"], "verdict": "approve"}),
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let events = store
        .events_range(boot.tenant_id, 1, 1_000)
        .await
        .expect("journal");
    let promotes: Vec<_> = events
        .iter()
        .filter(|e| e.envelope.event_type == "lifecycle.promote")
        .map(|e| e.envelope.payload["same_user"].clone())
        .collect();
    assert_eq!(promotes, [true, true, true], "bulk, bulk, single");
    // The page's verdicts are counted, once each: the replayed resubmission
    // adds nothing (ADR-0055 d13).
    let metrics = traffic.metrics().encode();
    assert!(
        metrics.contains(r#"oml_review_bulk_items_total{outcome="approved"} 2"#),
        "{metrics}"
    );
    drop_db(&db).await;
}
