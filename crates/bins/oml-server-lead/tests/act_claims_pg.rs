//! The web console's acts claim their `Idempotency-Key` before they act, on
//! the real lead and a migrated Postgres store under its role pair
//! (ADR-0070 d3 layer 4, d5; ADR-0034 d8; #233): two sends of one form that
//! reach the API at once, as they do from two `oml-web` replicas, act once
//! and answer alike, a key's value is shown in the first answer only, and a
//! claim a stopped attempt left holds the key, so a retry acts nothing. The
//! sign-in provider is the in-memory double. Each test is named after the
//! sentence it proves.

use std::sync::atomic::Ordering;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::domain::Sha256Hash;
use oml_app::domain::name::{Name, NamedKind};
use oml_app::domain::oauth::Issuer;
use oml_app::domain::web::{GitHubEmail, Provider, capacity_credential};
use oml_app::doubles::MemSignIn;
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{IdGen as _, IdempotencyClaim, IdempotencyScope, JournalStore as _};
use oml_app::types::{BootstrapRequest, BootstrapResult, ProviderAnswer};
use oml_fetch::PinnedFetcher;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_app, start_lead_with_models};
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};
use oml_transport_http::session::WebClient;
use oml_transport_http::traffic::Traffic;
use oml_transport_http::web::{SESSION_COOKIE, SIGN_IN_COOKIE, WebPages};

const ISSUER: &str = "https://oml.example.com";
const WEB_CLIENT_SECRET: &str = "pg-web-client-secret-0123456789";

fn fresh_name() -> String {
    format!(
        "oml_test_claims_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    )
}

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

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a readable body");
    (
        status,
        headers,
        serde_json::from_slice(&body).unwrap_or(Value::Null),
    )
}

fn replayed(headers: &HeaderMap) -> bool {
    headers
        .get("idempotency-replay")
        .is_some_and(|v| v == "true")
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

async fn tenant(store: &PgStore, key: &SigningKey) -> BootstrapResult {
    lead_app(store, key.clone())
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("oml").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "oml-admin".to_owned(),
        })
        .await
        .expect("bootstrap")
}

async fn lead(store: &PgStore, key: &SigningKey, providers: &MemSignIn) -> Router {
    start_lead_with_models(
        store,
        key.clone(),
        Some(Issuer::parse(ISSUER).expect("a public origin")),
        PinnedFetcher,
        Traffic::default(),
        Some(WebPages::new(
            vec![(Provider::GitHub, "github-client".to_owned())],
            providers.clone(),
        )),
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
        None,
        WebClient::new(Some(WEB_CLIENT_SECRET.to_owned()), None),
    )
    .await
    .expect("the lead starts")
    .router
}

/// Alice on the allowlist, signed in by GitHub: her session cookie's token.
async fn alice(router: &Router, providers: &MemSignIn, admin: &str) -> String {
    let (status, _, body) = send(
        router,
        Request::post("/v2/web/emails")
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "alice")
            .body(Body::from(
                json!({"email": "alice@example.com", "user": "alice", "admin": false}).to_string(),
            ))
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
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
    let location = headers[header::LOCATION].to_str().expect("ASCII");
    let state = location
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("state="))
        .expect("a state")
        .to_owned();
    let sealed = set_cookie(&headers, SIGN_IN_COOKIE).expect("the sign-in cookie");
    providers.answer(
        Provider::GitHub,
        Ok(ProviderAnswer::GitHub {
            account_id: 7,
            emails: vec![GitHubEmail {
                email: "alice@example.com".to_owned(),
                primary: true,
                verified: true,
            }],
        }),
    );
    let (_, headers, _) = send(
        router,
        Request::get(format!("/web/callback/github?code=c&state={state}"))
            .header(header::COOKIE, format!("{SIGN_IN_COOKIE}={sealed}"))
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    set_cookie(&headers, SESSION_COOKIE).expect("a session")
}

/// A `POST` as `oml-web` makes it for the session `token` as `<user>-web`.
fn as_web(token: &str, path: &str, idem: &str, body: &Value) -> Request<Body> {
    Request::post(path)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", capacity_credential(token, "web")),
        )
        .header("oml-web-client", WEB_CLIENT_SECRET)
        .header("idempotency-key", idem)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request")
}

fn create(admin: &str, idem: &str, body: &Value) -> Request<Body> {
    Request::post("/v2/workareas")
        .header(header::AUTHORIZATION, format!("Bearer {admin}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("idempotency-key", idem)
        .body(Body::from(body.to_string()))
        .expect("a request")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_concurrent_key_issues_under_one_key_issue_once_and_show_the_secret_in_one_answer_only()
{
    let db = fresh_name();
    if !mint_database(&db).await {
        return;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let boot = tenant(&store, &key).await;
    let providers = MemSignIn::new();
    let router = lead(&store, &key, &providers).await;
    let token = alice(&router, &providers, &boot.admin_key.value).await;
    let owner = sqlx::PgPool::connect(&owner_url(&db))
        .await
        .expect("owner login");
    // A bearer key shows its value once, a connector key its code (ADR-0031
    // d7): each pair of sends shows that secret in one answer alone.
    for (idem, name, connect_code) in [
        ("issue-1", "alice-laptop", false),
        ("issue-2", "alice-phone", true),
    ] {
        let body = json!({"name": name, "role": "writer", "connect_code": connect_code});
        // Two sends of one Keys form, as two oml-web replicas forward a
        // double click: before #233 both acted, and the second answered
        // name.taken.
        let ((s1, h1, a1), (s2, h2, a2)) = tokio::join!(
            send(&router, as_web(&token, "/v2/keys", idem, &body)),
            send(&router, as_web(&token, "/v2/keys", idem, &body)),
        );
        assert_eq!(
            (s1, s2),
            (StatusCode::CREATED, StatusCode::CREATED),
            "{a1} {a2}"
        );
        let (fresh, replay) = match (replayed(&h1), replayed(&h2)) {
            (false, true) => (a1, a2),
            (true, false) => (a2, a1),
            other => panic!("one send acts and the other replays it: {other:?} {a1} {a2}"),
        };
        let secret = if connect_code {
            fresh["connect_code"]["code"].as_str()
        } else {
            fresh["value"].as_str()
        }
        .unwrap_or_else(|| panic!("the first answer shows the secret: {fresh}"))
        .to_owned();
        assert!(
            !replay.to_string().contains(&secret),
            "a replay holds no secret: {replay}"
        );
        let mut kept = fresh.clone();
        kept.as_object_mut().unwrap().remove("value");
        if let Some(code) = kept.get_mut("connect_code").and_then(Value::as_object_mut) {
            code.remove("code");
        }
        assert_eq!(replay, kept, "both answers agree but for the secret");
        let mut tx = owner.begin().await.expect("begin");
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(boot.tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .expect("tenant");
        let raw: Vec<String> = sqlx::query_scalar(
            "SELECT response::text FROM idempotency WHERE key LIKE '%:keys.issue:' || $1",
        )
        .bind(idem)
        .fetch_all(&mut *tx)
        .await
        .expect("the record");
        tx.rollback().await.expect("rollback");
        assert_eq!(raw.len(), 1, "one record: {raw:?}");
        assert!(
            !raw[0].contains(&secret),
            "the stored record holds no secret"
        );
        let (status, _, again) = send(&router, as_web(&token, "/v2/keys", idem, &body)).await;
        assert_eq!(status, StatusCode::CREATED, "{again}");
        assert_eq!(
            again, replay,
            "a later resubmission replays the same answer"
        );
    }
    owner.close().await;
    drop(router);
    drop(store);
    drop_db(&db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_concurrent_creates_under_one_key_create_once_and_answer_alike_on_postgres() {
    let db = fresh_name();
    if !mint_database(&db).await {
        return;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let boot = tenant(&store, &key).await;
    let router = lead(&store, &key, &MemSignIn::new()).await;
    let admin = boot.admin_key.value.clone();
    let body = json!({"name": "twice", "parent": "universe"});
    let ((s1, h1, b1), (s2, h2, b2)) = tokio::join!(
        send(&router, create(&admin, "c-1", &body)),
        send(&router, create(&admin, "c-1", &body)),
    );
    assert_eq!(
        (s1, s2),
        (StatusCode::CREATED, StatusCode::CREATED),
        "{b1} {b2}"
    );
    assert_eq!(
        [replayed(&h1), replayed(&h2)]
            .iter()
            .filter(|r| **r)
            .count(),
        1,
        "one send acts and the other replays it"
    );
    assert_eq!(b1, b2, "both answers agree");
    drop(router);
    drop(store);
    drop_db(&db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_send_whose_key_a_stopped_attempt_holds_answers_in_progress_and_acts_nothing_on_postgres()
{
    let db = fresh_name();
    if !mint_database(&db).await {
        return;
    }
    let store = serving_store(&db).await;
    let key = SigningKey::from_bytes(&OsIds.new_secret());
    let boot = tenant(&store, &key).await;
    let router = lead(&store, &key, &MemSignIn::new()).await;
    let admin = boot.admin_key.value.clone();
    let body = json!({"name": "stuck", "parent": "universe"});
    let key_id = store
        .resolve_name(
            boot.tenant_id,
            NamedKind::Key,
            &Name::parse("root-admin").expect("a name"),
        )
        .await
        .expect("a lookup")
        .expect("the admin key");
    let mut bytes = b"\n".to_vec();
    bytes.extend_from_slice(body.to_string().as_bytes());
    // The claim of an attempt whose process stopped after its act and
    // before its record.
    let claimed = store
        .idempotency_claim(
            &IdempotencyScope {
                tenant_id: boot.tenant_id,
                key_id,
                workarea_id: Uuid::nil(),
                idem_key: "workarea.create:c-2".to_owned(),
            },
            &Sha256Hash::digest(&bytes).to_prefixed(),
        )
        .await
        .expect("claim");
    assert_eq!(claimed, IdempotencyClaim::Claimed);
    let (status, headers, refused) = send(&router, create(&admin, "c-2", &body)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.in_progress");
    assert!(headers.contains_key(header::RETRY_AFTER));
    let root = store
        .resolve_name(
            boot.tenant_id,
            NamedKind::Workarea,
            &Name::parse("universe").expect("a name"),
        )
        .await
        .expect("a lookup")
        .expect("the root");
    let children = store
        .workarea_children(boot.tenant_id, root)
        .await
        .expect("children");
    assert!(
        children.iter().all(|n| n.name.as_str() != "stuck"),
        "nothing acted: {children:?}"
    );
    drop(router);
    drop(store);
    drop_db(&db).await;
}
