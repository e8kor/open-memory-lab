//! The connect flow end to end through the real lead (ADR-0031 d7–d12;
//! docs/api/http-api.md, "Connections: OAuth for hosted MCP clients";
//! docs/architecture/roles-and-keys.md, "Connections"). The lead is started
//! in-process over a migrated Postgres store with `oauth.issuer` set; the
//! connector key and its connect code are minted through the App use case
//! over the same store, since the `/v2` issuance route waits for ADR-0039.
//! Every other step goes through the lead's router: a dynamic client
//! registration, `GET /oauth/authorize`, the code and Connect posted with
//! `Sec-Fetch-Site: same-origin`, the exchange and a refresh at
//! `/oauth/token`, and the replay of the rotated refresh token, which ends
//! the connection.

use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::oauth::{ConnectionState, Issuer, RevokedCause};
use oml_app::domain::{HeldBy, Role};
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{ConnectionStore as _, IdGen as _, JournalStore as _};
use oml_app::types::{BootstrapRequest, GrantRequest, IssueKeyRequest};
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_app, start_lead, sweep_connections};
use oml_transport_http::time::{system_event_time, system_event_time_plus};

const ISSUER: &str = "https://oml.example.com";
const CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";
/// RFC 7636 appendix B: the verifier and its S256 challenge.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const FORM: &str = "application/x-www-form-urlencoded";

// ---------------------------------------------------------------------------
// Throwaway databases and role pairs (the document_fetcher.rs pattern)
// ---------------------------------------------------------------------------

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .expect("DATABASE_URL names a throwaway PostgreSQL cluster; no suite defaults to one, so none reaches a live server")
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
        "oml_test_flow_{}_{}_{}",
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

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

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

fn pct_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).expect("ASCII");
            out.push(u8::from_str_radix(hex, 16).expect("a hex escape"));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("UTF-8")
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

fn html(body: &[u8]) -> String {
    String::from_utf8(body.to_vec()).expect("a UTF-8 page")
}

/// The value of the page form's hidden `handle`.
fn handle_of(page: &str) -> String {
    let marker = r#"name="handle" value=""#;
    let start = page.find(marker).expect("a handle field") + marker.len();
    let end = start + page[start..].find('"').expect("a closing quote");
    page[start..end].to_owned()
}

/// The page's own `POST`, with the fetch metadata a browser sends on it.
fn page_post(fields: &[(&str, &str)]) -> Request<Body> {
    Request::post("/oauth/authorize")
        .header(header::CONTENT_TYPE, FORM)
        .header("sec-fetch-site", "same-origin")
        .body(Body::from(form(fields)))
        .expect("a request")
}

fn token_request(fields: &[(&str, &str)]) -> Request<Body> {
    Request::post("/oauth/token")
        .header(header::CONTENT_TYPE, FORM)
        .body(Body::from(form(fields)))
        .expect("a request")
}

/// The RFC 6749 §5.2 body's `error`, after checking that its description
/// begins with `code` (errors.md, "`oauth` — connections").
fn rfc_error(body: &[u8], code: &str) -> String {
    let body = json_of(body);
    let description = body["error_description"]
        .as_str()
        .expect("an error_description");
    assert!(
        description.starts_with(&format!("{code}: ")),
        "{description} begins with {code}"
    );
    body["error"].as_str().expect("an error").to_owned()
}

// ---------------------------------------------------------------------------
// The flow
// ---------------------------------------------------------------------------

/// What one run of the flow left behind for the assertions.
struct Flow {
    store: PgStore,
    tenant_id: Uuid,
    admin_key: String,
    key_id: Uuid,
    /// The tenant's head after the key's issuance, its
    /// `connection.code_issued` the last event.
    issued_head: u64,
    /// The replayed refresh token's refusal: status and RFC body.
    replay: (StatusCode, Vec<u8>),
    /// The rotated refresh token presented after the replay.
    after_replay: (StatusCode, Vec<u8>),
}

/// Runs the flow over `db`, or `None` where the role pair cannot be minted.
async fn run_flow(db: &str) -> Option<Flow> {
    if !mint_database(db).await {
        return None;
    }
    let store = serving_store(db).await;
    let issuer = Issuer::parse(ISSUER).expect("a public origin");
    let signing_key = SigningKey::from_bytes(&OsIds.new_secret());

    // The connector key and its code through the App use case: the /v2
    // issuance route waits for ADR-0039.
    let app = lead_app(&store, signing_key.clone()).with_oauth_issuer(issuer.clone());
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let issued = app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Writer,
                name: Name::parse("claude-mobile").expect("a name"),
                principal: "crew/claude-mobile".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: None,
                crew: None,
                grants: vec![GrantRequest {
                    node: boot.root_workarea_id.into(),
                    role: Role::Writer,
                    deny: Vec::new(),
                    covers: Covers::Subtree,
                }],
                connect_code: true,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("a connector key");
    assert!(issued.value.is_empty(), "a connector key has no value");
    let connect_code = issued
        .connect_code
        .and_then(|c| c.code)
        .expect("the code, shown once");
    let issued_head = store
        .chain_head(boot.tenant_id)
        .await
        .expect("the head")
        .expect("a chain")
        .seq;

    // The real lead, started over the store the key was minted in.
    let started = start_lead(&store, signing_key, Some(issuer))
        .await
        .expect("the start reads the store");
    assert!(started.fork_check.is_ok(), "{:?}", started.fork_check);
    let router = started.router;

    // A dynamic client registration.
    let (status, _, body) = send(
        &router,
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
    assert_eq!(status, StatusCode::CREATED, "{}", html(&body));
    let client_id = json_of(&body)["client_id"]
        .as_str()
        .expect("a client_id")
        .to_owned();

    // GET /oauth/authorize renders the connect-code page.
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
        &router,
        Request::get(format!("/oauth/authorize?{query}"))
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", html(&body));

    // The code, then Connect.
    let (status, _, body) = send(
        &router,
        page_post(&[
            ("handle", &handle_of(&html(&body))),
            ("connect_code", &connect_code),
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", html(&body));
    let page = html(&body);
    assert!(page.contains("claude-mobile"), "{page}");
    let (status, headers, _) = send(
        &router,
        page_post(&[("handle", &handle_of(&page)), ("action", "connect")]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION]
        .to_str()
        .expect("ASCII")
        .to_owned();
    let (target, query) = location.split_once('?').expect("a query");
    assert_eq!(target, CALLBACK);
    let code = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("code="))
        .map(pct_decode)
        .expect("an authorization code");

    // The exchange opens the connection.
    let (status, _, body) = send(
        &router,
        token_request(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("client_id", &client_id),
            ("redirect_uri", CALLBACK),
            ("code_verifier", VERIFIER),
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", html(&body));
    let pair = json_of(&body);
    assert_eq!(pair["token_type"], "Bearer");
    assert_eq!(pair["scope"], "mcp");
    let first_refresh = pair["refresh_token"]
        .as_str()
        .expect("a refresh token")
        .to_owned();
    let open = app
        .list_connections(&boot.admin_key.value, issued.key_id.into())
        .await
        .expect("the admin lists the key's connections");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].state, ConnectionState::Open);

    // A refresh rotates the refresh token.
    let (status, _, body) = send(
        &router,
        token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &first_refresh),
            ("client_id", &client_id),
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", html(&body));
    let rotated = json_of(&body)["refresh_token"]
        .as_str()
        .expect("a refresh token")
        .to_owned();
    assert_ne!(rotated, first_refresh);

    // The rotated-out token presented again is reuse; then the live one.
    let (status, _, body) = send(
        &router,
        token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &first_refresh),
            ("client_id", &client_id),
        ]),
    )
    .await;
    let replay = (status, body);
    let (status, _, body) = send(
        &router,
        token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &rotated),
            ("client_id", &client_id),
        ]),
    )
    .await;
    let after_replay = (status, body);

    Some(Flow {
        store,
        tenant_id: boot.tenant_id,
        admin_key: boot.admin_key.value,
        key_id: issued.key_id,
        issued_head,
        replay,
        after_replay,
    })
}

#[tokio::test]
async fn a_connector_key_connects_by_code_exchanges_refreshes_and_a_replayed_refresh_token_ends_the_connection()
 {
    let db = fresh_name();
    let Some(flow) = run_flow(&db).await else {
        return;
    };
    let (status, body) = &flow.replay;
    assert_eq!(*status, StatusCode::BAD_REQUEST, "{}", html(body));
    assert_eq!(rfc_error(body, "oauth.refresh_invalid"), "invalid_grant");
    // The connection is over, so the token the refresh handed out is too.
    let (status, body) = &flow.after_replay;
    assert_eq!(*status, StatusCode::BAD_REQUEST, "{}", html(body));
    assert_eq!(rfc_error(body, "oauth.refresh_invalid"), "invalid_grant");

    let app = lead_app(&flow.store, SigningKey::from_bytes(&OsIds.new_secret()));
    let connections = app
        .list_connections(&flow.admin_key, flow.key_id.into())
        .await
        .expect("the admin lists the key's connections");
    assert_eq!(connections.len(), 1, "{connections:?}");
    assert_eq!(
        connections[0].state,
        ConnectionState::Revoked(RevokedCause::RefreshReuse)
    );
    assert!(connections[0].revoked_seq.is_some());

    drop(app);
    drop(flow);
    drop_db(&db).await;
}

#[tokio::test]
async fn the_whole_flow_writes_exactly_connection_code_issued_connection_opened_and_connection_revoked_to_the_journal()
 {
    let db = fresh_name();
    let Some(flow) = run_flow(&db).await else {
        return;
    };
    let events = flow
        .store
        .events_range(flow.tenant_id, 1, 10_000)
        .await
        .expect("the journal reads");
    let connection_events: Vec<&str> = events
        .iter()
        .map(|e| e.envelope.event_type.as_str())
        .filter(|t| t.starts_with("connection."))
        .collect();
    assert_eq!(
        connection_events,
        [
            "connection.code_issued",
            "connection.opened",
            "connection.revoked"
        ]
    );
    // Issuance ended at its code; the page, the exchange, the refresh and
    // the replay wrote the opening and the revocation and nothing else.
    let code_issued = events
        .iter()
        .find(|e| e.envelope.event_type == "connection.code_issued")
        .expect("the code's event");
    assert_eq!(code_issued.envelope.seq, flow.issued_head);
    let after: Vec<&str> = events
        .iter()
        .filter(|e| e.envelope.seq > flow.issued_head)
        .map(|e| e.envelope.event_type.as_str())
        .collect();
    assert_eq!(after, ["connection.opened", "connection.revoked"]);

    let opened = &events[events.len() - 2].envelope;
    let revoked = &events[events.len() - 1].envelope;
    let key_id = flow.key_id.to_string();
    assert_eq!(
        opened.payload["key_id"],
        key_id.as_str(),
        "{}",
        opened.payload
    );
    assert_eq!(
        revoked.payload["key_id"],
        key_id.as_str(),
        "{}",
        revoked.payload
    );
    assert_eq!(
        revoked.payload["connection_id"], opened.payload["connection_id"],
        "{} {}",
        opened.payload, revoked.payload
    );
    assert_eq!(
        revoked.payload["cause"], "refresh_reuse",
        "{}",
        revoked.payload
    );

    drop(flow);
    drop_db(&db).await;
}

#[tokio::test]
async fn the_lead_sweep_removes_a_registration_that_has_not_connected_24_hours_after_it_registered_and_keeps_a_younger_one()
 {
    let db = fresh_name();
    if !mint_database(&db).await {
        return;
    }
    let store = serving_store(&db).await;
    let issuer = Issuer::parse(ISSUER).expect("a public origin");
    let started = start_lead(
        &store,
        SigningKey::from_bytes(&OsIds.new_secret()),
        Some(issuer),
    )
    .await
    .expect("the start reads the store");
    let (status, _, body) = send(
        &started.router,
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
    assert_eq!(status, StatusCode::CREATED, "{}", html(&body));
    let client_id = json_of(&body)["client_id"]
        .as_str()
        .expect("a client_id")
        .to_owned();

    // An hour on, the registration is younger than 24 hours and stays.
    let sweep = sweep_connections(&store, &system_event_time_plus(60 * 60))
        .await
        .expect("the sweep runs");
    assert_eq!(sweep.registrations_removed, 0, "{sweep:?}");
    assert!(
        store
            .oauth_client(&client_id)
            .await
            .expect("a read")
            .is_some()
    );

    // Past 24 hours, unconnected, it is gone.
    let sweep = sweep_connections(&store, &system_event_time_plus(24 * 60 * 60 + 60))
        .await
        .expect("the sweep runs");
    assert_eq!(sweep.registrations_removed, 1, "{sweep:?}");
    assert!(
        store
            .oauth_client(&client_id)
            .await
            .expect("a read")
            .is_none()
    );

    drop(started);
    drop(store);
    drop_db(&db).await;
}

#[tokio::test]
async fn the_lead_sweep_removes_a_revoked_connections_tokens_and_clears_its_code_once() {
    let db = fresh_name();
    let Some(flow) = run_flow(&db).await else {
        return;
    };
    let sweep = sweep_connections(&flow.store, &system_event_time())
        .await
        .expect("the sweep runs");
    assert_eq!(sweep.tenants, 1, "{sweep:?}");
    assert!(sweep.tokens_removed >= 2, "{sweep:?}");
    assert_eq!(sweep.codes_cleared, 1, "{sweep:?}");

    let again = sweep_connections(&flow.store, &system_event_time())
        .await
        .expect("the sweep runs");
    assert_eq!(
        (again.tokens_removed, again.codes_cleared),
        (0, 0),
        "{again:?}"
    );

    drop(flow);
    drop_db(&db).await;
}
