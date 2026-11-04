//! The connect flow end to end over HTTP against the real lead (ADR-0031 d1,
//! d2, d7–d11, d14; FR-34's continuous-integration criterion). The lead is
//! `start_lead` over a migrated Postgres store, served on an ephemeral TCP
//! port; every step is an HTTP request to it: the bootstrap claim, the
//! `oml` binary's `keys issue-agent --connect-code`, `keys show`,
//! `keys disconnect` and `keys connect-code`, a dynamic client
//! registration, the authorization page, the token exchange, and `/mcp`
//! under the access token, under the refresh token and under no credential.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::response::IntoResponse as _;
use ed25519_dalek::SigningKey;
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};

use oml_app::domain::oauth::Issuer;
use oml_pg::PgStore;
use oml_server_lead::start_lead;

const ISSUER: &str = "https://oml.example.com";
const CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";
/// RFC 7636 appendix B: the verifier and its S256 challenge.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const FORM: &str = "application/x-www-form-urlencoded";

// ---------------------------------------------------------------------------
// Throwaway databases and role pairs (the cli_e2e.rs pattern)
// ---------------------------------------------------------------------------

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev".to_owned())
}

/// The password of every minted test role; the roles live only as long as
/// their throwaway database.
const PAIR_PASSWORD: &str = "oml_test_pair";

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

fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

/// Mints `db`'s owner/serving role pair and creates `db` owned by the owner
/// (ADR-0034 d2, d8); `false`, with `did not run: <role> cannot create
/// roles` printed, when `DATABASE_URL`'s role is not a superuser. Under
/// `OML_TEST_REQUIRE_SPLIT=1`, which CI sets, the same condition fails.
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
// The lead over TCP, and the oml binary against it
// ---------------------------------------------------------------------------

struct Lead {
    db: String,
    url: String,
    admin_key: String,
    creds: PathBuf,
    server: tokio::task::JoinHandle<()>,
}

impl Lead {
    /// The real lead over a fresh database, `oauth.issuer` set to `issuer`
    /// or unset, served on an ephemeral port, and its first admin key
    /// claimed over HTTP with the enrollment token the start minted.
    async fn start(issuer: Option<&str>) -> Option<Self> {
        Self::start_behind(issuer, |router| router).await
    }

    /// [`Lead::start`], its router served through `front`: what the
    /// public origin passes to the lead, standing in for a reverse proxy.
    async fn start_behind(
        issuer: Option<&str>,
        front: impl FnOnce(axum::Router) -> axum::Router,
    ) -> Option<Self> {
        let db = format!(
            "oml_test_connect_{}_{}_{}",
            std::process::id(),
            DB_N.fetch_add(1, Ordering::SeqCst),
            run_tag()
        );
        if !mint_database(&db).await {
            return None;
        }
        let owner = role_url(&db, &format!("{db}_owner"));
        let store = PgStore::connect(&role_url(&db, &format!("{db}_serving")))
            .await
            .expect("connect")
            .with_owner_url(&owner);
        store.migrate(&owner).await.expect("migrations apply");
        let mut seed = [0_u8; 32];
        getrandom::getrandom(&mut seed).expect("OS randomness");
        let issuer = issuer.map(|i| Issuer::parse(i).expect("a public origin"));
        let started = start_lead(&store, SigningKey::from_bytes(&seed), issuer)
            .await
            .expect("the start reads the store");
        let enrollment = started
            .enrollment_token
            .expect("an empty database mints one");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("ephemeral bind");
        let url = format!("http://{}", listener.local_addr().expect("local addr"));
        let router = front(started.router);
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve");
        });
        let claim_url = format!("{url}/v2/bootstrap/claim");
        let (status, _, claimed) = blocking(move || {
            http(
                agent()
                    .post(&claim_url)
                    .set("Content-Type", "application/json"),
                Some(
                    json!({
                        "enrollment_token": enrollment,
                        "name": "root-admin",
                        "tenant_name": "acme",
                    })
                    .to_string(),
                ),
            )
        })
        .await;
        assert_eq!(status, 201, "{claimed}");
        let creds = std::env::temp_dir().join(format!("{db}.toml"));
        Some(Self {
            admin_key: claimed["value"].as_str().expect("the value").to_owned(),
            db,
            url,
            creds,
            server,
        })
    }

    /// The `oml` binary under the admin key; returns (exit code, stdout,
    /// stderr).
    async fn oml(&self, args: &[&str]) -> (i32, String, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_oml"));
        cmd.arg("--url")
            .arg(&self.url)
            .arg("--api-key")
            .arg(&self.admin_key)
            .args(args)
            .env("OML_CLI_CREDENTIALS", &self.creds);
        for name in ["OML_URL", "OML_API_KEY", "OML_CLI_KEY", "OML_CLI_SERVER"] {
            cmd.env_remove(name);
        }
        let out = blocking(move || cmd.output().expect("the oml binary runs")).await;
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// [`Lead::oml`] with `--json`, its stdout parsed.
    async fn oml_json(&self, args: &[&str]) -> (i32, Value, String) {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let (code, stdout, stderr) = self.oml(&all).await;
        let parsed = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
        (code, parsed, format!("stdout: {stdout}\nstderr: {stderr}"))
    }

    async fn stop(self) {
        self.server.abort();
        let _ = self.server.await;
        let _ = std::fs::remove_file(&self.creds);
        drop_db(&self.db).await;
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f)
        .await
        .expect("the blocking call ends")
}

/// An agent that follows no redirect, so the authorize page's `303` is the
/// answer read.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(30))
        .build()
}

/// Sends `request` with `body`; returns (status, headers, body as JSON, or
/// as a string when it is not JSON).
fn http(request: ureq::Request, body: Option<String>) -> (u16, Vec<(String, String)>, Value) {
    let result = match body {
        Some(body) => request.send_string(&body),
        None => request.call(),
    };
    let response = match result {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(e) => panic!("the lead answers: {e}"),
    };
    let status = response.status();
    let headers = response
        .headers_names()
        .into_iter()
        .map(|name| {
            let value = response.header(&name).unwrap_or_default().to_owned();
            (name, value)
        })
        .collect();
    let text = response.into_string().expect("a readable body");
    let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (status, headers, body)
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
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

/// The value of the page form's hidden `handle`.
fn handle_of(page: &Value) -> String {
    let page = page.as_str().expect("an HTML page");
    let marker = r#"name="handle" value=""#;
    let start = page.find(marker).expect("a handle field") + marker.len();
    let end = start + page[start..].find('"').expect("a closing quote");
    page[start..end].to_owned()
}

/// One `POST /mcp` JSON-RPC request under `bearer`, or under no
/// `Authorization` header.
async fn mcp(
    url: &str,
    bearer: Option<&str>,
    method: &str,
    params: Value,
) -> (u16, Vec<(String, String)>, Value) {
    let url = format!("{url}/mcp");
    let bearer = bearer.map(|b| format!("Bearer {b}"));
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string();
    blocking(move || {
        let mut request = agent()
            .post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json, text/event-stream");
        if let Some(bearer) = &bearer {
            request = request.set("Authorization", bearer);
        }
        http(request, Some(body))
    })
    .await
}

async fn tool(url: &str, bearer: &str, name: &str, arguments: Value) -> Value {
    let (status, _, body) = mcp(
        url,
        Some(bearer),
        "tools/call",
        json!({ "name": name, "arguments": arguments }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    body["result"].clone()
}

/// What a connection's flow leaves the client holding.
struct Connected {
    access_token: String,
    refresh_token: String,
}

/// A dynamic registration, the authorize page, `code` entered and Connect,
/// then the token exchange: the flow Claude's hosted apps run, over HTTP.
async fn connect(url: &str, code: &str) -> Connected {
    let base = url.to_owned();
    let code = code.to_owned();
    blocking(move || {
        let (status, _, registered) = http(
            agent()
                .post(&format!("{base}/oauth/register"))
                .set("Content-Type", "application/json"),
            Some(
                json!({
                    "client_name": "Claude",
                    "redirect_uris": [CALLBACK],
                    "token_endpoint_auth_method": "none",
                    "grant_types": ["authorization_code", "refresh_token"],
                    "response_types": ["code"],
                })
                .to_string(),
            ),
        );
        assert_eq!(status, 201, "{registered}");
        let client_id = registered["client_id"]
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
        let (status, _, page) = http(
            agent().get(&format!("{base}/oauth/authorize?{query}")),
            None,
        );
        assert_eq!(status, 200, "{page}");
        let post = || {
            agent()
                .post(&format!("{base}/oauth/authorize"))
                .set("Content-Type", FORM)
                .set("Sec-Fetch-Site", "same-origin")
        };
        let (status, _, page) = http(
            post(),
            Some(form(&[
                ("handle", &handle_of(&page)),
                ("connect_code", &code),
            ])),
        );
        assert_eq!(status, 200, "{page}");
        assert!(
            page.as_str().expect("a page").contains("claude-mobile"),
            "{page}"
        );
        let (status, headers, _) = http(
            post(),
            Some(form(&[
                ("handle", &handle_of(&page)),
                ("action", "connect"),
            ])),
        );
        assert_eq!(status, 303);
        let location = header(&headers, "location").expect("a redirect");
        let (target, query) = location.split_once('?').expect("a query");
        assert_eq!(target, CALLBACK);
        let authorization_code = query
            .split('&')
            .find_map(|pair| pair.strip_prefix("code="))
            .map(pct_decode)
            .expect("an authorization code");
        let (status, _, pair) = http(
            agent()
                .post(&format!("{base}/oauth/token"))
                .set("Content-Type", FORM),
            Some(form(&[
                ("grant_type", "authorization_code"),
                ("code", &authorization_code),
                ("client_id", &client_id),
                ("redirect_uri", CALLBACK),
                ("code_verifier", VERIFIER),
            ])),
        );
        assert_eq!(status, 200, "{pair}");
        Connected {
            access_token: pair["access_token"]
                .as_str()
                .expect("an access token")
                .to_owned(),
            refresh_token: pair["refresh_token"]
                .as_str()
                .expect("a refresh token")
                .to_owned(),
        }
    })
    .await
}

/// The two lines `--connect-code` prints on stdout: the connector URL and
/// the code (cli/reference.md `--connect-code`).
fn connector_url_and_code(stdout: &str) -> (String, String) {
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "exactly two lines: {stdout:?}");
    (lines[0].to_owned(), lines[1].to_owned())
}

const CHALLENGE_MISSING: &str = r#"Bearer resource_metadata="https://oml.example.com/.well-known/oauth-protected-resource/mcp", scope="mcp""#;
const CHALLENGE_FAILED: &str = r#"Bearer resource_metadata="https://oml.example.com/.well-known/oauth-protected-resource/mcp", scope="mcp", error="invalid_token""#;

// ---------------------------------------------------------------------------
// The flow
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connector_key_issued_by_the_cli_connects_by_code_and_acts_at_mcp_as_its_key_until_disconnected()
 {
    let Some(lead) = Lead::start(Some(ISSUER)).await else {
        return;
    };

    // The CLI issues the connector key: the URL and the code, no value.
    let (code, stdout, stderr) = lead
        .oml(&[
            "keys",
            "issue-agent",
            "claude-mobile",
            "--principal",
            "crew/claude-mobile",
            "--workarea",
            "universe",
            "--connect-code",
        ])
        .await;
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (connector_url, first_code) = connector_url_and_code(&stdout);
    assert_eq!(connector_url, format!("{ISSUER}/mcp"));
    assert!(
        !stdout.contains("oml_"),
        "no key value is printed: {stdout}"
    );

    // An unauthenticated /mcp call points at the protected-resource document.
    let (status, headers, body) = mcp(&lead.url, None, "tools/list", json!({})).await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["error"]["code"], "auth.missing", "{body}");
    assert_eq!(
        header(&headers, "www-authenticate"),
        Some(CHALLENGE_MISSING)
    );

    let connected = connect(&lead.url, &first_code).await;

    // The access token acts at /mcp exactly as the key it is bound to.
    let (status, _, listed) = mcp(
        &lead.url,
        Some(&connected.access_token),
        "tools/list",
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{listed}");
    let written = tool(
        &lead.url,
        &connected.access_token,
        "oml_write",
        json!({
            "text": "The metals desk closes its book at 17:00 CET (source: desk runbook, 2026-11-04).",
            "idempotency_key": "connect-flow-1",
        }),
    )
    .await;
    assert_eq!(written["isError"], false, "{written}");
    let receipt = &written["structuredContent"];
    assert_eq!(receipt["state"], "held", "{written}");
    let artifact_id = receipt["artifact_id"]
        .as_str()
        .expect("an artifact_id")
        .to_owned();
    let own = tool(&lead.url, &connected.access_token, "oml_read", json!({})).await;
    assert_eq!(own["isError"], false, "{own}");
    assert!(
        own.to_string().contains(&artifact_id),
        "the author reads its held write: {own}"
    );

    // The token is never a credential on the rings, and carries no challenge there.
    let ring_url = format!("{}/v1/memories", lead.url);
    let bearer = format!("Bearer {}", connected.access_token);
    let (status, headers, body) =
        blocking(move || http(agent().get(&ring_url).set("Authorization", &bearer), None)).await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["error"]["code"], "auth.invalid", "{body}");
    assert_eq!(body["error"]["details"]["reason"], "unknown", "{body}");
    assert_eq!(header(&headers, "www-authenticate"), None);

    // A refresh token never serves as a bearer credential at /mcp.
    let (status, headers, body) = mcp(
        &lead.url,
        Some(&connected.refresh_token),
        "tools/list",
        json!({}),
    )
    .await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["error"]["code"], "auth.invalid", "{body}");
    assert_eq!(header(&headers, "www-authenticate"), Some(CHALLENGE_FAILED));

    // The admin lists the key's one open connection.
    let (code, shown, out) = lead.oml_json(&["keys", "show", "claude-mobile"]).await;
    assert_eq!(code, 0, "{out}");
    assert_eq!(shown["credential"], "connection", "{shown}");
    let connections = shown["connections"].as_array().expect("connections");
    assert_eq!(connections.len(), 1, "{shown}");
    assert_eq!(connections[0]["state"], "open", "{shown}");
    assert_eq!(connections[0]["redirect_uri"], CALLBACK, "{shown}");
    assert_eq!(
        connections[0]["resource"],
        format!("{ISSUER}/mcp"),
        "{shown}"
    );
    let connection_id = connections[0]["connection_id"]
        .as_str()
        .expect("a connection_id")
        .to_owned();

    // Disconnecting ends the connection at its next request.
    let (code, revoked, out) = lead
        .oml_json(&[
            "keys",
            "disconnect",
            "claude-mobile",
            &connection_id,
            "--reason",
            "phone lost",
        ])
        .await;
    assert_eq!(code, 0, "{out}");
    assert_eq!(revoked["state"], "revoked", "{revoked}");
    assert_eq!(revoked["revoked_cause"], "admin", "{revoked}");
    let (status, headers, body) = mcp(
        &lead.url,
        Some(&connected.access_token),
        "tools/list",
        json!({}),
    )
    .await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["error"]["code"], "auth.invalid", "{body}");
    assert_eq!(header(&headers, "www-authenticate"), Some(CHALLENGE_FAILED));
    let (code, shown, out) = lead.oml_json(&["keys", "show", "claude-mobile"]).await;
    assert_eq!(code, 0, "{out}");
    assert_eq!(shown["connections"][0]["state"], "revoked", "{shown}");

    // A fresh code reconnects the same key.
    let (code, stdout, stderr) = lead.oml(&["keys", "connect-code", "claude-mobile"]).await;
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (connector_url, second_code) = connector_url_and_code(&stdout);
    assert_eq!(connector_url, format!("{ISSUER}/mcp"));
    assert_ne!(second_code, first_code);
    let reconnected = connect(&lead.url, &second_code).await;
    let (status, _, body) = mcp(
        &lead.url,
        Some(&reconnected.access_token),
        "tools/list",
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    lead.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_oauth_issuer_mcp_answers_realm_oml_and_the_cli_cannot_mint_a_connect_code() {
    let Some(lead) = Lead::start(None).await else {
        return;
    };
    let (status, headers, body) = mcp(&lead.url, None, "tools/list", json!({})).await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(
        header(&headers, "www-authenticate"),
        Some(r#"Bearer realm="oml""#)
    );
    let (status, headers, body) = mcp(
        &lead.url,
        Some("oml_at_0000000000000000000000000000000000000000000"),
        "tools/list",
        json!({}),
    )
    .await;
    assert_eq!(status, 401, "{body}");
    assert_eq!(body["error"]["code"], "auth.invalid", "{body}");
    assert_eq!(
        header(&headers, "www-authenticate"),
        Some(r#"Bearer realm="oml""#)
    );

    // A bearer key still works at /mcp.
    let (status, _, body) = mcp(&lead.url, Some(&lead.admin_key), "tools/list", json!({})).await;
    assert_eq!(status, 200, "{body}");

    let (code, stdout, stderr) = lead
        .oml(&[
            "keys",
            "issue-agent",
            "claude-mobile",
            "--principal",
            "crew/claude-mobile",
            "--workarea",
            "universe",
            "--connect-code",
        ])
        .await;
    assert_ne!(code, 0, "{stdout}{stderr}");
    assert!(stdout.is_empty(), "no code is printed: {stdout}");
    assert!(stderr.contains("oauth.connect_refused"), "{stderr}");

    let (code, doctor, out) = lead.oml_json(&["doctor"]).await;
    assert_eq!(code, 0, "{out}");
    assert_eq!(doctor["oauth_discovery"]["state"], "off", "{doctor}");

    lead.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doctor_reports_oauth_discovery_failing_naming_the_url_when_the_issuer_does_not_serve_the_documents()
 {
    let Some(lead) = Lead::start(Some("https://oml.example.invalid")).await else {
        return;
    };
    let (code, doctor, out) = lead.oml_json(&["doctor"]).await;
    assert_eq!(code, 1, "{out}");
    let discovery = &doctor["oauth_discovery"];
    assert_eq!(discovery["state"], "failing", "{doctor}");
    assert_eq!(
        discovery["issuer"], "https://oml.example.invalid",
        "{doctor}"
    );
    assert!(
        discovery["url"]
            .as_str()
            .is_some_and(|u| u.starts_with("https://oml.example.invalid/.well-known/")),
        "{doctor}"
    );
    assert_eq!(doctor["verdict"], "fail", "{doctor}");

    let (code, stdout, stderr) = lead.oml(&["doctor"]).await;
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stdout.lines().any(|l| l.starts_with("oauth")
            && l.contains("failing")
            && l.contains("https://oml.example.invalid/.well-known/")),
        "{stdout}"
    );

    lead.stop().await;
}

/// A proxy that forwards only `/mcp`, `/v1`, `/v2` and the probes answers
/// `404` for `/.well-known/*`; with the lead naming an issuer that is still
/// `failing` with the issuer's URL, never `off`, because whether
/// connections are on is the lead's answer and not the route's (ADR-0031
/// d14).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn doctor_reports_oauth_failing_not_off_when_the_public_origin_answers_404_for_discovery_while_the_lead_has_an_issuer()
 {
    let Some(lead) = Lead::start_behind(Some("https://oml.example.invalid"), |router| {
        router.layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                if request.uri().path().starts_with("/.well-known/") {
                    return axum::http::StatusCode::NOT_FOUND.into_response();
                }
                next.run(request).await
            },
        ))
    })
    .await
    else {
        return;
    };
    let (code, doctor, out) = lead.oml_json(&["doctor"]).await;
    assert_eq!(code, 1, "{out}");
    let discovery = &doctor["oauth_discovery"];
    assert_eq!(discovery["state"], "failing", "{doctor}");
    assert_eq!(
        discovery["issuer"], "https://oml.example.invalid",
        "{doctor}"
    );
    assert!(
        discovery["url"]
            .as_str()
            .is_some_and(|u| u.starts_with("https://oml.example.invalid/.well-known/")),
        "{doctor}"
    );
    assert_eq!(doctor["verdict"], "fail", "{doctor}");

    lead.stop().await;
}
