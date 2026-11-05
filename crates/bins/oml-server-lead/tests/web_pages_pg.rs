//! The web pages over the real lead and a migrated Postgres store under its
//! role pair (ADR-0045 d2, d4, d5; ADR-0034 d8): the allowlist's routes and
//! folds, the session rows and the sign-in under the serving role's RLS. The
//! sign-in provider is the in-memory double: no call reaches Google or
//! GitHub. Each test is named after the sentence it proves.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;

use oml_app::domain::name::Name;
use oml_app::domain::oauth::Issuer;
use oml_app::domain::web::{GitHubEmail, Provider};
use oml_app::doubles::MemSignIn;
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{IdGen as _, JournalStore as _};
use oml_app::types::{BootstrapRequest, BootstrapResult, ProviderAnswer};
use oml_fetch::PinnedFetcher;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_app, start_lead_with, sweep_connections};
use oml_transport_http::traffic::Traffic;
use oml_transport_http::web::{SESSION_COOKIE, SIGN_IN_COOKIE, WebPages};

const ISSUER: &str = "https://oml.example.com";

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev".to_owned())
}

/// The password of every minted test role; the roles live only as long as
/// their throwaway database.
const PAIR_PASSWORD: &str = "oml_test_pair";

/// `db` at `DATABASE_URL`'s host, logged in as `role`.
fn role_url(db: &str, role: &str) -> String {
    let base = base_url();
    let (scheme, rest) = base
        .split_once("://")
        .expect("DATABASE_URL is a postgres:// URL");
    let (_, host_path) = rest.rsplit_once('@').expect("DATABASE_URL names its user");
    let (host, _) = host_path
        .split_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{scheme}://{role}:{PAIR_PASSWORD}@{host}/{db}")
}

/// `db` through the owner login of its role pair (ADR-0034 d2, d8).
fn owner_url(db: &str) -> String {
    role_url(db, &format!("{db}_owner"))
}

/// `db` through the serving login of its role pair (ADR-0034 d2, d8).
fn serving_url(db: &str) -> String {
    role_url(db, &format!("{db}_serving"))
}

/// Eight hex digits of the clock, so a name a crashed earlier run left
/// behind on the cluster under the same pid and counter is not reused.
fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

fn fresh_name() -> String {
    format!(
        "oml_test_web_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
}

/// Mints `db`'s owner/serving role pair, `LOGIN` and no other attribute,
/// and creates `db` owned by the owner (ADR-0034 d2, d8); `false`, with
/// `did not run: <role> cannot create roles` printed, when `DATABASE_URL`'s
/// role is not the superuser minting takes, as the maintainer's local `oml`
/// (CREATEDB alone) is not. Under `OML_TEST_REQUIRE_SPLIT=1`, which CI sets,
/// the same condition fails the test, so no run passes vacuously.
async fn mint_database(db: &str) -> bool {
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    let (role, mints): (String, bool) = sqlx::query_as(
        "SELECT current_user::text, rolsuper FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut admin)
    .await
    .expect("role attributes");
    if !mints {
        assert!(
            std::env::var("OML_TEST_REQUIRE_SPLIT").as_deref() != Ok("1"),
            "{role} cannot create roles, and OML_TEST_REQUIRE_SPLIT=1 requires the role pair (ADR-0034 d8)"
        );
        println!("did not run: {role} cannot create roles");
        return false;
    }
    for role in ["owner", "serving"] {
        admin
            .execute(format!("CREATE ROLE {db}_{role} LOGIN PASSWORD '{PAIR_PASSWORD}'").as_str())
            .await
            .expect("mint the role pair");
    }
    admin
        .execute(format!("CREATE DATABASE {db} OWNER {db}_owner").as_str())
        .await
        .expect("create throwaway database");
    true
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

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
        for role in ["serving", "owner"] {
            let _ = admin
                .execute(format!("DROP ROLE IF EXISTS {name}_{role}").as_str())
                .await;
        }
    }
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
    start_lead_with(
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
    let (status, _, page) = send(
        &router,
        Request::get("/web/admin?view=queue")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .expect("a request"),
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

/// The value of the first `<input … name="{name}" value="…">` on `page`.
fn input_value(page: &str, name: &str) -> String {
    let at = page
        .find(&format!(r#"name="{name}" value=""#))
        .unwrap_or_else(|| panic!("no input {name}: {page}"));
    let rest = &page[at + name.len() + 15..];
    rest[..rest.find('"').expect("a closing quote")].to_owned()
}

fn form_post(path: &str, cookie: &str, fields: &[(&str, &str)]) -> Request<Body> {
    let body: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{k}={}", v.replace(' ', "+")))
        .collect();
    Request::post(path)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("sec-fetch-site", "same-origin")
        .body(Body::from(body.join("&")))
        .expect("a request")
}

/// The admin page's bulk review (ADR-0055 d10): the listing shows each
/// held item's text under the person's reviewer key, "select all" checks
/// every box, `bulk_confirm` shows what was picked and records nothing,
/// `bulk_verdict` records one verdict per item and shows each outcome, a
/// resubmitted confirmation replays, and a form without the CSRF token is
/// refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_admin_page_lists_confirms_and_shows_each_outcome() {
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
    let get = |path: &str| {
        Request::get(path)
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .expect("a request")
    };
    let (status, _, page) = send(&router, get("/web/admin?view=held")).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    for (i, id) in ids.iter().enumerate() {
        assert!(
            page.contains(&format!(r#"name="pick_{i:03}" value="{id}">"#)),
            "{page}"
        );
    }
    assert!(
        page.contains("Decision adr-0001-d2."),
        "the text is shown: {page}"
    );
    assert!(page.contains("(you)"), "same user is shown: {page}");
    let (_, _, all) = send(
        &router,
        get("/web/admin?view=held&claim_key_prefix=adr-0001-d&select=all"),
    )
    .await;
    assert_eq!(all.matches(" checked>").count(), 2, "{all}");
    let csrf = input_value(&page, "csrf_token");

    // Without the token nothing happens.
    let (status, _, _) = send(
        &router,
        form_post(
            "/web/admin",
            &cookie,
            &[
                ("action", "bulk_confirm"),
                ("verdict", "approve"),
                ("pick_000", &ids[0]),
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The confirmation records nothing.
    let (status, _, confirm) = send(
        &router,
        form_post(
            "/web/admin",
            &cookie,
            &[
                ("action", "bulk_confirm"),
                ("csrf_token", &csrf),
                ("verdict", "approve"),
                ("reason", ""),
                ("pick_000", &ids[0]),
                ("pick_001", &ids[1]),
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirm}");
    assert!(confirm.contains("Approve 2 memories?"), "{confirm}");
    // Each picked item is shown by its claim key and first line, so the
    // person sees what they confirm, not bare ids (ADR-0055 d10).
    for claim_key in ["adr-0001-d1", "adr-0001-d2"] {
        assert!(confirm.contains(&format!(">{claim_key}<")), "{confirm}");
        assert!(
            confirm.contains(&format!("Decision {claim_key}.")),
            "{confirm}"
        );
    }
    assert!(
        !confirm.contains("adr-0001-rejected"),
        "only the picks: {confirm}"
    );
    let (_, _, still) = send(&router, get("/web/admin?view=held")).await;
    assert!(
        ids.iter().all(|id| still.contains(id.as_str())),
        "nothing judged yet: {still}"
    );

    let joined = input_value(&confirm, "artifact_ids");
    assert_eq!(joined, format!("{} {}", ids[0], ids[1]));
    let idem = input_value(&confirm, "idempotency_key");
    let verdict_form = [
        ("action", "bulk_verdict"),
        ("csrf_token", csrf.as_str()),
        ("verdict", "approve"),
        ("reason", ""),
        ("artifact_ids", joined.as_str()),
        ("idempotency_key", idem.as_str()),
    ];
    let (status, _, outcome) = send(&router, form_post("/web/admin", &cookie, &verdict_form)).await;
    assert_eq!(status, StatusCode::OK, "{outcome}");
    assert!(
        outcome.contains("2 approved, 0 rejected, 0 skipped."),
        "{outcome}"
    );
    assert!(outcome.contains("same user"), "{outcome}");
    let (_, _, again) = send(&router, form_post("/web/admin", &cookie, &verdict_form)).await;
    assert!(
        again.contains("replayed"),
        "a resubmitted form replays: {again}"
    );
    let (_, _, left) = send(&router, get("/web/admin?view=held")).await;
    assert!(left.contains(&ids[2]) && !left.contains(&ids[0]), "{left}");
    // An empty reason is journalled as none, as on the API.
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
    // The page's verdicts are counted, once each: the replayed resubmission
    // adds nothing (ADR-0055 d13).
    let metrics = traffic.metrics().encode();
    assert!(
        metrics.contains(r#"oml_review_bulk_items_total{outcome="approved"} 2"#),
        "{metrics}"
    );
    drop_db(&db).await;
}
