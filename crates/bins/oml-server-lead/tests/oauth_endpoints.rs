//! The unversioned OAuth surface over the in-memory doubles: the two
//! discovery documents, `POST /oauth/register` and `POST /oauth/token`,
//! present only where the deployment sets `oauth.issuer`, outside both rings
//! (docs/api/http-api.md, "Connections: OAuth for hosted MCP clients";
//! docs/api/errors.md, "`oauth` — connections"; ADR-0031 d1, d3, d4, d5,
//! d13). The documents are built from the issuer alone, and every refusal
//! is the RFC's `{error, error_description}` whose description begins with
//! the OML code. Requests go through `tower::ServiceExt::oneshot` against
//! the real router.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::oauth::{
    Issuer, UNUSED_REGISTRATIONS_MAX, authorization_server_document, protected_resource_document,
};
use oml_app::domain::{HeldBy, Role, Sha256Hash};
use oml_app::doubles::{
    FixedClock, MarkerScanner, MemDocuments, MemSigner, MemStore, PlainSealer, SeqIdGen,
};
use oml_app::ports::{Clock as _, ConnectionStore as _};
use oml_app::types::{
    AuthorizeOutcome, AuthorizeParams, BootstrapRequest, ClientDescription, CodeEntryOutcome,
    ConfirmAction, ConfirmOutcome, ConfirmationGrant, ConfirmationView, GrantRequest,
    IssueKeyRequest,
};
use oml_transport_http::{ApiState, DbPing};

/// The fixture deployment's `oauth.issuer`.
const ISSUER: &str = "https://oml.example.com";
const CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";
/// RFC 7636 appendix B: the verifier and its S256 challenge.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
const FORM: &str = "application/x-www-form-urlencoded";

type MemApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

struct Rig {
    app: Arc<MemApp>,
    store: MemStore,
    clock: FixedClock,
    router: Router,
}

/// The transport over the doubles, its app given `issuer` as `oauth.issuer`
/// when one is named.
fn rig(issuer: Option<&str>) -> Rig {
    rig_with(issuer, None)
}

/// [`rig`] whose authorize page fetches client documents from `documents`.
fn rig_with(issuer: Option<&str>, documents: Option<MemDocuments>) -> Rig {
    let store = MemStore::new();
    let clock = FixedClock::new();
    let app = App::new(
        store.clone(),
        MemSigner::new([7; 32]),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let app = Arc::new(match issuer {
        Some(issuer) => app.with_oauth_issuer(Issuer::parse(issuer).expect("a public origin")),
        None => app,
    });
    let ping: DbPing = Arc::new(|| Box::pin(async { true }));
    let state = ApiState::new(Arc::clone(&app), store.clone(), ping, true, None);
    let router = match documents {
        Some(documents) => state.with_document_fetcher(documents),
        None => state,
    }
    .into_router();
    Rig {
        app,
        store,
        clock,
        router,
    }
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

fn get(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).expect("a request")
}

fn token_request(body: &str) -> Request<Body> {
    Request::post("/oauth/token")
        .header(header::CONTENT_TYPE, FORM)
        .body(Body::from(body.to_owned()))
        .expect("a request")
}

fn register_request(body: &Value) -> Request<Body> {
    Request::post("/oauth/register")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("a request")
}

fn json_of(body: &[u8]) -> Value {
    serde_json::from_slice(body).expect("a JSON body")
}

/// The RFC error body's `error`, after checking its shape: exactly
/// `error` and `error_description`, the description beginning with
/// `code` and a colon (errors.md, "`oauth` — connections").
fn rfc_error(body: &[u8], code: &str) -> String {
    let body = json_of(body);
    let object = body.as_object().expect("an RFC error object");
    assert_eq!(object.len(), 2, "{body}");
    let description = body["error_description"]
        .as_str()
        .expect("an error_description");
    assert!(
        description.starts_with(&format!("{code}: ")),
        "{description} begins with {code}"
    );
    body["error"].as_str().expect("an error").to_owned()
}

fn registration() -> Value {
    json!({
        "client_name": "Example MCP client",
        "redirect_uris": [CALLBACK],
        "token_endpoint_auth_method": "none",
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
    })
}

/// Without `oauth.issuer` the documents and the endpoints do not exist:
/// each answers `404`, as an unknown path does (http-api.md connections).
#[tokio::test]
async fn with_oauth_issuer_unset_the_discovery_documents_and_oauth_endpoints_answer_404() {
    let rig = rig(None);
    for request in [
        get("/.well-known/oauth-protected-resource/mcp"),
        get("/.well-known/oauth-authorization-server"),
        get("/.well-known/oauth-protected-resource"),
        register_request(&registration()),
        token_request("grant_type=refresh_token&refresh_token=oml_rt_x&client_id=c"),
    ] {
        let path = request.uri().to_string();
        let (status, _, _) = send(&rig.router, request).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

/// `/readyz` names the lead's `oauth.issuer`, or null where it is unset,
/// on a route that does not depend on the discovery documents, so doctor
/// learns whether connections are on from the lead and not from the path
/// it is checking (ADR-0031 d14).
#[tokio::test]
async fn readyz_names_the_leads_oauth_issuer_or_null_where_it_is_unset() {
    for (issuer, expected) in [(Some(ISSUER), json!(ISSUER)), (None, Value::Null)] {
        let rig = rig(issuer);
        let (_, _, body) = send(&rig.router, get("/readyz")).await;
        let body = json_of(&body);
        assert_eq!(body["oauth_issuer"], expected, "{body}");
    }
}

/// The documents are the issuer's, whatever `Host` the request names
/// (RFC 9728 §3.3; ADR-0031 d3).
#[tokio::test]
async fn the_protected_resource_document_names_exactly_issuer_slash_mcp_whatever_the_host_header() {
    let rig = rig(Some(ISSUER));
    let issuer = Issuer::parse(ISSUER).expect("a public origin");
    for host in ["oml.example.com", "attacker.example", "127.0.0.1:8080"] {
        let request = Request::get("/.well-known/oauth-protected-resource/mcp")
            .header(header::HOST, host)
            .body(Body::empty())
            .expect("a request");
        let (status, headers, body) = send(&rig.router, request).await;
        assert_eq!(status, StatusCode::OK, "{host}");
        assert!(
            headers[header::CONTENT_TYPE]
                .to_str()
                .expect("ASCII")
                .starts_with("application/json"),
            "{headers:?}"
        );
        let document = json_of(&body);
        assert_eq!(
            document["resource"], "https://oml.example.com/mcp",
            "{host}"
        );
        assert_eq!(document, protected_resource_document(&issuer), "{host}");

        let request = Request::get("/.well-known/oauth-authorization-server")
            .header(header::HOST, host)
            .body(Body::empty())
            .expect("a request");
        let (status, _, body) = send(&rig.router, request).await;
        assert_eq!(status, StatusCode::OK, "{host}");
        assert_eq!(
            json_of(&body),
            authorization_server_document(&issuer),
            "{host}"
        );
    }
}

/// Only the path-inserted form is served; the root form and an OpenID
/// Connect discovery document are not (http-api.md connections).
#[tokio::test]
async fn the_root_protected_resource_path_is_not_served() {
    let rig = rig(Some(ISSUER));
    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/openid-configuration",
    ] {
        let (status, _, _) = send(&rig.router, get(path)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

/// A parameter sent more than once is `request.validation` as
/// `invalid_request` (RFC 6749 §3.2), even when the copies agree.
#[tokio::test]
async fn a_token_request_with_a_repeated_parameter_is_invalid_request() {
    let rig = rig(Some(ISSUER));
    for body in [
        "grant_type=refresh_token&refresh_token=oml_rt_a&refresh_token=oml_rt_b&client_id=c",
        "grant_type=refresh_token&grant_type=refresh_token&refresh_token=oml_rt_a&client_id=c",
        "grant_type=authorization_code&code=x&client_id=c&client_id=c&code_verifier=v",
        "grant_type=authorization_code&code=x&client_id=c&code_verifier=v&resource=a&resource=a",
    ] {
        let (status, _, response) = send(&rig.router, token_request(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(
            rfc_error(&response, "request.validation"),
            "invalid_request",
            "{body}"
        );
    }
}

/// Client authentication is refused, since every client is public: an
/// `Authorization` header answers `401` naming the scheme it used, and a
/// `client_secret` in the body `400`, both `invalid_client` (RFC 6749 §5.2;
/// ADR-0031 d5).
#[tokio::test]
async fn a_token_request_with_basic_authorization_answers_401_naming_basic() {
    let rig = rig(Some(ISSUER));
    let body = "grant_type=refresh_token&refresh_token=oml_rt_a&client_id=c";
    let request = Request::post("/oauth/token")
        .header(header::CONTENT_TYPE, FORM)
        .header(header::AUTHORIZATION, "Basic Yzpz")
        .body(Body::from(body))
        .expect("a request");
    let (status, headers, response) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(headers[header::WWW_AUTHENTICATE], r#"Basic realm="oml""#);
    assert_eq!(
        rfc_error(&response, "oauth.client_invalid"),
        "invalid_client"
    );

    let (status, headers, response) = send(
        &rig.router,
        token_request(&format!("{body}&client_secret=s")),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        headers.get(header::WWW_AUTHENTICATE).is_none(),
        "{headers:?}"
    );
    assert_eq!(
        rfc_error(&response, "oauth.client_invalid"),
        "invalid_client"
    );
}

/// A registered client's connection, from the authorize request through
/// Connect: the client's id and the authorization code (ADR-0031 d9).
async fn authorization_code(rig: &Rig) -> (String, String) {
    let (status, _, body) = send(&rig.router, register_request(&registration())).await;
    assert_eq!(status, StatusCode::CREATED);
    let client_id = json_of(&body)["client_id"]
        .as_str()
        .expect("a client_id")
        .to_owned();
    let connect_code = issue_connector(rig).await;
    let params = AuthorizeParams {
        client_id: Some(client_id.clone()),
        redirect_uri: Some(CALLBACK.to_owned()),
        response_type: Some("code".to_owned()),
        code_challenge: Some(CHALLENGE.to_owned()),
        code_challenge_method: Some("S256".to_owned()),
        state: Some("st".to_owned()),
        resource: Some(format!("{ISSUER}/mcp")),
        scope: Some("mcp".to_owned()),
        repeated: false,
    };
    let AuthorizeOutcome::Page(page) = rig
        .app
        .authorize_request(params, &MemDocuments::new(), &PlainSealer)
        .await
        .expect("an authorize request is answered")
    else {
        panic!("a registered client's valid request is a page");
    };
    let CodeEntryOutcome::Confirmation(view) = rig
        .app
        .enter_code(&page.handle, &connect_code, &PlainSealer)
        .await
        .expect("code entry is answered")
    else {
        panic!("a correct code shows the confirmation");
    };
    let ConfirmOutcome::Connected { code, .. } = rig
        .app
        .confirm(&view.handle, ConfirmAction::Connect)
        .await
        .expect("Connect")
    else {
        panic!("Connect answers an authorization code");
    };
    (client_id, code)
}

/// The pair and every refusal at the token endpoint carry
/// `Cache-Control: no-store` (RFC 6749 §5.1; http-api.md `POST /oauth/token`).
#[tokio::test]
async fn token_responses_carry_cache_control_no_store() {
    let rig = rig(Some(ISSUER));
    let (client_id, code) = authorization_code(&rig).await;
    let body = format!(
        "grant_type=authorization_code&code={code}&client_id={client_id}\
         &redirect_uri=https%3A%2F%2Fclaude.ai%2Fapi%2Fmcp%2Fauth_callback\
         &code_verifier={VERIFIER}&resource=https%3A%2F%2Foml.example.com%2Fmcp"
    );
    let (status, headers, response) = send(&rig.router, token_request(&body)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&response)
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    let pair = json_of(&response);
    assert_eq!(pair["token_type"], "Bearer");
    assert_eq!(pair["scope"], "mcp");
    assert_eq!(pair["expires_in"], 3_600);
    assert!(
        pair["access_token"]
            .as_str()
            .is_some_and(|t| t.starts_with("oml_at_")),
        "{pair}"
    );
    let refresh = pair["refresh_token"].as_str().expect("a refresh token");
    assert!(refresh.starts_with("oml_rt_"), "{pair}");

    // A refresh rotates; the refusal of a replayed code is no-store too.
    let (status, headers, _) = send(
        &rig.router,
        token_request(&format!(
            "grant_type=refresh_token&refresh_token={refresh}&client_id={client_id}"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    for body in [
        "grant_type=refresh_token&refresh_token=oml_rt_unknown&client_id=c".to_owned(),
        "grant_type=password&username=u&password=p".to_owned(),
        "grant_type=refresh_token&refresh_token=a&refresh_token=b".to_owned(),
    ] {
        let (status, headers, _) = send(&rig.router, token_request(&body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store", "{body}");
    }
}

/// Each refusal names its OML code first in `error_description`, beside
/// the RFC `error` a client branches on (errors.md, "`oauth` —
/// connections"; ADR-0031 d13).
#[tokio::test]
async fn an_oauth_refusals_error_description_begins_with_its_oml_code() {
    let rig = rig(Some(ISSUER));
    for (body, code, error) in [
        (
            "grant_type=refresh_token&refresh_token=not-a-token&client_id=c",
            "oauth.refresh_invalid",
            "invalid_grant",
        ),
        (
            "grant_type=authorization_code&code=unknown&client_id=c&code_verifier=v",
            "oauth.authorization_code_invalid",
            "invalid_grant",
        ),
        (
            "grant_type=refresh_token&refresh_token=oml_rt_a&client_id=c\
             &resource=https%3A%2F%2Fother.example%2Fmcp",
            "oauth.resource_mismatch",
            "invalid_target",
        ),
        (
            "grant_type=password&username=u&password=p",
            "request.validation",
            "unsupported_grant_type",
        ),
        (
            "grant_type=refresh_token&client_id=c",
            "request.validation",
            "invalid_request",
        ),
        (
            "refresh_token=oml_rt_a",
            "request.validation",
            "invalid_request",
        ),
        (
            "grant_type=refresh_token&refresh_token=%ZZ",
            "request.validation",
            "invalid_request",
        ),
    ] {
        let (status, _, response) = send(&rig.router, token_request(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(rfc_error(&response, code), error, "{body}");
    }

    // A body in another media type is refused before it is read.
    let request = Request::post("/oauth/token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("a request");
    let (status, _, response) = send(&rig.router, request).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(
        rfc_error(&response, "request.unsupported_media"),
        "invalid_request"
    );

    // A registration's refusals take RFC 7591's names.
    let mut outside = registration();
    outside["redirect_uris"] = json!(["https://attacker.example/cb"]);
    let mut unnamed = registration();
    unnamed
        .as_object_mut()
        .expect("an object")
        .remove("client_name");
    for (body, code, error) in [
        (outside, "oauth.redirect_refused", "invalid_redirect_uri"),
        (unnamed, "request.validation", "invalid_client_metadata"),
    ] {
        let (status, _, response) = send(&rig.router, register_request(&body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(rfc_error(&response, code), error, "{body}");
    }
}

/// The redirect allowlist is Claude's hosted callback alone: a registration
/// naming a loopback redirect, on any host form or port, writes nothing and
/// answers `400 invalid_redirect_uri` whose `error_description` is
/// `oauth.redirect_refused` and its errors.md rule (ADR-0031 d6, d13).
#[tokio::test]
async fn a_registration_with_a_loopback_redirect_is_refused_as_invalid_redirect_uri() {
    let rig = rig(Some(ISSUER));
    for loopback in [
        "http://127.0.0.1:53682/callback",
        "http://127.0.0.1/callback",
        "http://[::1]:8080/cb",
        "http://localhost:1/cb",
        "http://localhost/",
    ] {
        let mut body = registration();
        body["redirect_uris"] = json!([loopback]);
        let (status, _, response) = send(&rig.router, register_request(&body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{loopback}");
        assert_eq!(
            rfc_error(&response, "oauth.redirect_refused"),
            "invalid_redirect_uri",
            "{loopback}"
        );
        assert_eq!(
            json_of(&response)["error_description"],
            "oauth.redirect_refused: An authorization code goes only to \
             https://claude.ai/api/mcp/auth_callback, by exact match; a refused redirect is \
             never followed (ADR-0031 d6; RFC 6749 §4.1.2.1).",
            "{loopback}"
        );
    }
}

/// A registration answers `201` with a minted `client_id`, no secret and
/// `token_endpoint_auth_method` `none`; past the cap of unused
/// registrations it writes nothing and answers `429 limit.rate` as
/// `invalid_request` with `Retry-After` (errors.md; ADR-0031 d5, d14).
#[tokio::test]
async fn registration_past_the_cap_answers_429_limit_rate() {
    let rig = rig(Some(ISSUER));
    let mut unauthenticated = registration();
    unauthenticated
        .as_object_mut()
        .expect("an object")
        .remove("token_endpoint_auth_method");
    let (status, _, body) = send(&rig.router, register_request(&unauthenticated)).await;
    assert_eq!(status, StatusCode::CREATED);
    let registered = json_of(&body);
    assert!(registered["client_id"].is_string(), "{registered}");
    assert!(registered["client_id_issued_at"].is_u64(), "{registered}");
    assert_eq!(registered["client_name"], "Example MCP client");
    assert_eq!(registered["redirect_uris"], json!([CALLBACK]));
    assert_eq!(registered["token_endpoint_auth_method"], "none");
    assert!(registered.get("client_secret").is_none(), "{registered}");
    assert!(
        registered.get("registration_access_token").is_none(),
        "{registered}"
    );

    let now = rig.clock.now();
    for filler in 1..UNUSED_REGISTRATIONS_MAX {
        rig.store
            .insert_dcr(
                ClientDescription {
                    client_id: format!("filler-{filler}"),
                    client_name: "filler".to_owned(),
                    redirect_uris: vec![CALLBACK.to_owned()],
                    metadata_digest: Sha256Hash::digest(b"filler"),
                },
                &now,
            )
            .await
            .expect("a registration under the cap");
    }
    let (status, headers, body) = send(&rig.router, register_request(&registration())).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.contains_key(header::RETRY_AFTER), "{headers:?}");
    assert_eq!(rfc_error(&body, "limit.rate"), "invalid_request");
}

// ---------------------------------------------------------------------------
// The authorization page (http-api.md "The authorization page"; ADR-0031 d9)
// ---------------------------------------------------------------------------

/// A tenant, its admin and one connector key, writer on the root workarea:
/// the key's connect code, shown once (ADR-0031 d7, d8).
async fn issue_connector(rig: &Rig) -> String {
    let boot = rig
        .app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let issued = rig
        .app
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
    issued
        .connect_code
        .and_then(|c| c.code)
        .expect("the code, shown once")
}

/// A registered client named `client_name`: its minted `client_id`.
async fn register_client(rig: &Rig, client_name: &str) -> String {
    let mut body = registration();
    body["client_name"] = json!(client_name);
    let (status, _, response) = send(&rig.router, register_request(&body)).await;
    assert_eq!(status, StatusCode::CREATED);
    json_of(&response)["client_id"]
        .as_str()
        .expect("a client_id")
        .to_owned()
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

/// `GET /oauth/authorize` for `client_id` with `state`, S256 PKCE and the
/// resource and scope a valid request carries.
fn authorize_get(client_id: &str, state: &str) -> Request<Body> {
    let query = form(&[
        ("client_id", client_id),
        ("redirect_uri", CALLBACK),
        ("response_type", "code"),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("resource", &format!("{ISSUER}/mcp")),
        ("scope", "mcp"),
    ]);
    get(&format!("/oauth/authorize?{query}"))
}

/// The page's own `POST`, with the fetch metadata a browser sends on it.
fn page_post(fields: &[(&str, &str)]) -> Request<Body> {
    page_post_with(fields, &[("sec-fetch-site", "same-origin")])
}

fn page_post_with(fields: &[(&str, &str)], headers: &[(&str, &str)]) -> Request<Body> {
    let mut request = Request::post("/oauth/authorize").header(header::CONTENT_TYPE, FORM);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(Body::from(form(fields))).expect("a request")
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

/// The redirect's query parameters, each decoded once.
fn redirect_query(headers: &HeaderMap) -> (String, Vec<(String, String)>) {
    let location = headers[header::LOCATION]
        .to_str()
        .expect("ASCII")
        .to_owned();
    let (target, query) = location.split_once('?').expect("a query");
    let params = query
        .split('&')
        .map(|pair| {
            let (name, value) = pair.split_once('=').expect("name=value");
            (name.to_owned(), pct_decode(value))
        })
        .collect();
    (target.to_owned(), params)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn base64url(bytes: &[u8]) -> String {
    base64(bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

/// The headers every authorization page carries, its stylesheet's digest
/// in `style-src`, and no cookie and no script (http-api.md "The
/// authorization page"; ADR-0031 d9).
fn assert_page(status: StatusCode, headers: &HeaderMap, body: &[u8]) {
    let page = html(body);
    let what = format!("{status} {page}");
    assert!(
        headers[header::CONTENT_TYPE]
            .to_str()
            .expect("ASCII")
            .starts_with("text/html"),
        "{what}"
    );
    let style_start = page.find("<style>").expect("one inline stylesheet") + "<style>".len();
    let style_end = page.find("</style>").expect("a closed stylesheet");
    assert_eq!(page.matches("<style").count(), 1, "{what}");
    let digest = base64(&Sha256Hash::digest(&page.as_bytes()[style_start..style_end]).0);
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        format!(
            "default-src 'none'; style-src 'sha256-{digest}'; form-action 'self' \
             https://claude.ai; frame-ancestors 'none'; base-uri 'none'"
        )
        .as_str(),
        "{what}"
    );
    assert_eq!(headers[header::CACHE_CONTROL], "no-store", "{what}");
    assert_eq!(headers[header::REFERRER_POLICY], "same-origin", "{what}");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff", "{what}");
    assert!(headers.get(header::SET_COOKIE).is_none(), "{what}");
    assert!(!page.to_ascii_lowercase().contains("<script"), "{what}");
    let handler = page.split('<').skip(1).any(|tag| {
        let tag = tag.split('>').next().unwrap_or("");
        tag.split_whitespace()
            .skip(1)
            .any(|attribute| attribute.to_ascii_lowercase().starts_with("on"))
    });
    assert!(!handler, "no event handler attribute: {what}");
}

#[tokio::test]
async fn every_authorization_page_carries_the_declared_csp_no_store_same_origin_referrer_policy_and_nosniff_and_sets_no_cookie()
 {
    let rig = rig(Some(ISSUER));
    let client_id = register_client(&rig, "Example MCP client").await;
    let connect_code = issue_connector(&rig).await;
    let mut pages = Vec::new();

    // The code form, and a refusal before the client is accepted.
    let (status, headers, body) = send(&rig.router, authorize_get(&client_id, "st")).await;
    assert_eq!(status, StatusCode::OK);
    let first = handle_of(&html(&body));
    pages.push((status, headers, body));
    let (status, headers, body) = send(&rig.router, authorize_get("unregistered", "st")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    pages.push((status, headers, body));

    // A wrong code, a cross-site submission and a malformed combination.
    let (status, headers, body) = send(
        &rig.router,
        page_post(&[("handle", &first), ("connect_code", "0000-0000-0000")]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    pages.push((status, headers, body));
    let (status, headers, body) = send(
        &rig.router,
        page_post_with(
            &[("handle", &first), ("connect_code", &connect_code)],
            &[("sec-fetch-site", "cross-site")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    pages.push((status, headers, body));
    let (status, headers, body) = send(&rig.router, page_post(&[("handle", &first)])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    pages.push((status, headers, body));

    // The confirmation.
    let (status, headers, body) = send(
        &rig.router,
        page_post(&[("handle", &first), ("connect_code", &connect_code)]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", html(&body));
    pages.push((status, headers, body));

    for (status, headers, body) in &pages {
        assert_page(*status, headers, body);
    }
}

#[tokio::test]
async fn a_post_without_same_origin_fetch_metadata_or_a_matching_origin_is_refused_403_and_consumes_no_attempt()
 {
    let rig = rig(Some(ISSUER));
    let client_id = register_client(&rig, "Example MCP client").await;
    let connect_code = issue_connector(&rig).await;
    let (_, _, body) = send(&rig.router, authorize_get(&client_id, "st")).await;
    let handle = handle_of(&html(&body));
    let failures = rig.app.connect_code_failures();

    for refused in [
        vec![("sec-fetch-site", "cross-site")],
        vec![("sec-fetch-site", "same-site")],
        vec![("sec-fetch-site", "none")],
        vec![("sec-fetch-site", "cross-site"), ("origin", ISSUER)],
        vec![],
        vec![("origin", "https://attacker.example")],
        vec![("origin", "http://oml.example.com")],
        vec![("origin", "https://oml.example.com:443")],
        vec![("origin", "https://oml.example.com/")],
    ] {
        for code in [connect_code.as_str(), "0000-0000-0000"] {
            let (status, headers, body) = send(
                &rig.router,
                page_post_with(&[("handle", &handle), ("connect_code", code)], &refused),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{refused:?}");
            assert!(headers.get(header::LOCATION).is_none(), "{refused:?}");
            assert!(
                html(&body).contains("oauth.origin_refused"),
                "{refused:?}: {}",
                html(&body)
            );
        }
    }
    assert_eq!(rig.app.connect_code_failures(), failures);

    // The same handle still carries all five attempts, and the code is
    // unspent: the first wrong entry leaves four, then the code confirms.
    let (status, _, body) = send(
        &rig.router,
        page_post(&[("handle", &handle), ("connect_code", "0000-0000-0000")]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let wrong = html(&body);
    assert!(wrong.contains("oauth.connect_code_invalid"), "{wrong}");
    assert!(wrong.contains("Attempts left: 4."), "{wrong}");
    let (status, _, body) = send(
        &rig.router,
        page_post_with(
            &[
                ("handle", &handle_of(&wrong)),
                ("connect_code", &connect_code),
            ],
            &[("origin", ISSUER)],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", html(&body));
    assert!(html(&body).contains("claude-mobile"), "{}", html(&body));
}

#[tokio::test]
async fn origin_null_is_refused() {
    let rig = rig(Some(ISSUER));
    let client_id = register_client(&rig, "Example MCP client").await;
    let connect_code = issue_connector(&rig).await;
    let (_, _, body) = send(&rig.router, authorize_get(&client_id, "st")).await;
    let handle = handle_of(&html(&body));
    let fields = [("handle", handle.as_str()), ("connect_code", &connect_code)];

    let (status, _, body) = send(&rig.router, page_post_with(&fields, &[("origin", "null")])).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(html(&body).contains("oauth.origin_refused"));

    // The code was not read: the issuer's own Origin then confirms.
    let (status, _, body) = send(&rig.router, page_post_with(&fields, &[("origin", ISSUER)])).await;
    assert_eq!(status, StatusCode::OK, "{}", html(&body));
}

#[tokio::test]
async fn a_code_and_an_action_in_one_post_is_request_validation_on_the_page() {
    let rig = rig(Some(ISSUER));
    let client_id = register_client(&rig, "Example MCP client").await;
    let connect_code = issue_connector(&rig).await;
    let (_, _, body) = send(&rig.router, authorize_get(&client_id, "st")).await;
    let first = handle_of(&html(&body));
    let failures = rig.app.connect_code_failures();

    let both = [
        ("handle", first.as_str()),
        ("connect_code", connect_code.as_str()),
        ("action", "connect"),
    ];
    for fields in [
        &both[..],
        // An action before a verified code.
        &[("handle", first.as_str()), ("action", "connect")][..],
        &[("handle", first.as_str())][..],
        &[("connect_code", connect_code.as_str())][..],
        &[("handle", first.as_str()), ("action", "approve")][..],
        &[
            ("handle", first.as_str()),
            ("connect_code", connect_code.as_str()),
            ("extra", "x"),
        ][..],
        &[
            ("handle", first.as_str()),
            ("handle", first.as_str()),
            ("connect_code", connect_code.as_str()),
        ][..],
    ] {
        let (status, headers, body) = send(&rig.router, page_post(fields)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{fields:?}");
        assert!(headers.get(header::LOCATION).is_none(), "{fields:?}");
        assert_page(status, &headers, &body);
        assert!(
            html(&body).contains("request.validation"),
            "{fields:?}: {}",
            html(&body)
        );
    }
    assert_eq!(rig.app.connect_code_failures(), failures);

    // The code was never spent; once verified, a second code on the
    // confirmation's handle is the same refusal, and Connect still works.
    let (status, _, body) = send(
        &rig.router,
        page_post(&[("handle", &first), ("connect_code", &connect_code)]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", html(&body));
    let confirmation = handle_of(&html(&body));
    let (status, _, body) = send(
        &rig.router,
        page_post(&[("handle", &confirmation), ("connect_code", &connect_code)]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        html(&body).contains("request.validation"),
        "{}",
        html(&body)
    );
    let (status, _, _) = send(
        &rig.router,
        page_post(&[("handle", &confirmation), ("action", "connect")]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn a_crafted_client_name_is_escaped_and_cannot_render_a_host() {
    let crafted = "Claude</p><p class=\"verified\">verified by <b>claude.ai</b><script>x()</script>\
                   \u{202E}moc.elgoog";
    let document_url = "https://client.example/oauth/client.json";
    let documents = MemDocuments::new();
    documents.serve(
        document_url,
        Ok((
            json!({
                "client_id": document_url,
                "client_name": crafted,
                "redirect_uris": [CALLBACK],
            })
            .to_string()
            .into_bytes(),
            300,
        )),
    );
    let rig = rig_with(Some(ISSUER), Some(documents));
    let registered = register_client(&rig, crafted).await;

    for (client_id, verified_by) in [
        (registered.as_str(), None),
        (document_url, Some("client.example")),
    ] {
        let (status, headers, body) = send(&rig.router, authorize_get(client_id, "st")).await;
        assert_eq!(status, StatusCode::OK, "{client_id}: {}", html(&body));
        assert_page(status, &headers, &body);
        let page = html(&body);
        assert!(!page.contains("<script"), "{page}");
        assert!(!page.contains("verified by <b>claude.ai</b>"), "{page}");
        assert!(page.contains("&lt;b&gt;claude.ai&lt;/b&gt;"), "{page}");
        assert!(!page.contains('\u{202E}'), "{page}");
        // The one host line is the server's, apart from the name.
        match verified_by {
            None => {
                assert_eq!(page.matches(r#"class="verified""#).count(), 0, "{page}");
                assert!(page.contains("name not verified"), "{page}");
            }
            Some(host) => {
                assert_eq!(page.matches(r#"class="verified""#).count(), 1, "{page}");
                assert!(
                    page.contains(&format!(
                        r#"<p class="verified">verified by <b>{host}</b></p>"#
                    )),
                    "{page}"
                );
                assert!(!page.contains("name not verified"), "{page}");
            }
        }
        assert!(
            page.contains("<p>Returns to <b>claude.ai</b></p>"),
            "{page}"
        );
    }

    // A long name is shortened.
    let long = "n".repeat(500);
    let client_id = register_client(&rig, &long).await;
    let (_, _, body) = send(&rig.router, authorize_get(&client_id, "st")).await;
    let page = html(&body);
    assert!(!page.contains(&"n".repeat(65)), "{page}");
    assert!(page.contains(&format!("{}…", "n".repeat(63))), "{page}");
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[tokio::test]
async fn the_pages_sentences_match_http_api_md_word_for_word() {
    let doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../docs/api/http-api.md"
    ))
    .expect("docs/api/http-api.md");
    let start = doc
        .find("**The authorization page.**")
        .expect("the page's paragraph");
    let end = start + doc[start..].find("**Behind a proxy.**").expect("its end");
    let section = collapse(&doc[start..end]);
    let names = &section[section.find("It names the client").expect("the names")..];
    let sentences: Vec<String> = names
        .split('"')
        .skip(1)
        .step_by(2)
        .map(|s| s.replace('`', ""))
        .collect();
    assert_eq!(sentences.len(), 10, "{sentences:?}");

    // The code form and the wrong-code form, over the real routes.
    let rig = rig(Some(ISSUER));
    let client_id = register_client(&rig, "Example MCP client").await;
    issue_connector(&rig).await;
    let (_, _, body) = send(&rig.router, authorize_get(&client_id, "st")).await;
    let code_form = collapse(&html(&body));
    let (_, _, body) = send(
        &rig.router,
        page_post(&[
            ("handle", &handle_of(&code_form)),
            ("connect_code", "0000-0000-0000"),
        ]),
    )
    .await;
    let wrong = collapse(&html(&body));

    // The confirmation, with every sentence kind present.
    let name = |n: &str| Name::parse(n).expect("a name");
    let grant = |n: &str, role| ConfirmationGrant {
        workarea_id: uuid::Uuid::nil(),
        workarea: name(n),
        role,
    };
    let view = ConfirmationView {
        handle: "h".to_owned(),
        tenant_name: name("acme"),
        key_name: Some(name("claude-mobile")),
        principal: "crew/claude-mobile".to_owned(),
        role: Role::Writer,
        grants: vec![grant("alpha", Role::Writer), grant("beta", Role::Reader)],
        writes_in: vec![name("alpha"), name("gamma")],
        reads_in: vec![name("beta")],
        proposes_in: vec![name("delta")],
        self_signed_in: vec![name("alpha"), name("gamma")],
    };
    let confirmation = collapse(&oml_transport_http::oauth::page::confirmation(&view));

    let expected = [
        (&code_form, "verified by".to_owned()),
        (&code_form, "name not verified".to_owned()),
        (&code_form, sentences[2].clone()),
        (&wrong, sentences[2].clone()),
        (&wrong, sentences[3].clone()),
        (
            &confirmation,
            sentences[4].replace("<workareas>", "alpha, gamma"),
        ),
        (&confirmation, sentences[5].replace("<workareas>", "beta")),
        (&confirmation, sentences[6].replace("<workareas>", "delta")),
        (&confirmation, sentences[7].clone()),
        (&confirmation, sentences[8].replace("<workarea>", "alpha")),
        (&confirmation, sentences[8].replace("<workarea>", "gamma")),
        (&confirmation, sentences[9].clone()),
    ];
    // "verified by" is a document client's; the form names a registered one.
    for (page, sentence) in &expected[1..] {
        assert!(page.contains(sentence.as_str()), "{sentence:?} in {page}");
    }
    assert_eq!(expected[0].1, sentences[0]);
    assert_eq!(expected[1].1, sentences[1]);
    assert!(!code_form.contains(&sentences[3]), "{code_form}");
    // The last sentence closes the list.
    let last = confirmation.find(&sentences[9]).expect("the last sentence");
    for (page, sentence) in &expected[5..11] {
        let at = page.find(sentence.as_str()).expect("a sentence");
        assert!(at < last, "{sentence:?} before the last sentence");
    }
}

#[tokio::test]
async fn connect_redirects_303_with_code_the_verbatim_state_and_iss() {
    let state = "a b&c=d/é?%+#";
    for action in ["cancel", "connect"] {
        let rig = rig(Some(ISSUER));
        let client_id = register_client(&rig, "Example MCP client").await;
        let connect_code = issue_connector(&rig).await;
        let (_, _, body) = send(&rig.router, authorize_get(&client_id, state)).await;
        let (status, _, body) = send(
            &rig.router,
            page_post(&[
                ("handle", &handle_of(&html(&body))),
                ("connect_code", &connect_code),
            ]),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", html(&body));
        let confirmation = handle_of(&html(&body));
        let (status, headers, _) = send(
            &rig.router,
            page_post(&[("handle", &confirmation), ("action", action)]),
        )
        .await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{action}");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store", "{action}");
        assert_eq!(headers[header::REFERRER_POLICY], "same-origin", "{action}");
        let location = headers[header::LOCATION].to_str().expect("ASCII");
        assert!(
            location.contains(&format!("&state={}&", pct(state))),
            "{location}"
        );
        assert!(
            location.ends_with("&iss=https%3A%2F%2Foml.example.com"),
            "{location}"
        );
        let (target, params) = redirect_query(&headers);
        assert_eq!(target, CALLBACK);
        let names: Vec<&str> = params.iter().map(|(n, _)| n.as_str()).collect();
        let value = |name: &str| {
            params
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(value("state").as_deref(), Some(state), "{action}");
        assert_eq!(value("iss").as_deref(), Some(ISSUER), "{action}");
        if action == "cancel" {
            assert_eq!(names, ["error", "state", "iss"], "{location}");
            assert_eq!(value("error").as_deref(), Some("access_denied"));
        } else {
            assert_eq!(names, ["code", "state", "iss"], "{location}");
            let code = value("code").expect("a code");
            let body = format!(
                "grant_type=authorization_code&code={code}&client_id={client_id}\
                 &redirect_uri={}&code_verifier={VERIFIER}",
                pct(CALLBACK)
            );
            let (status, _, response) = send(&rig.router, token_request(&body)).await;
            assert_eq!(status, StatusCode::OK, "{}", html(&response));
        }
        // The row is gone: the same confirmation again is the page's
        // oauth.authorization_expired, never a second redirect.
        let (status, headers, body) = send(
            &rig.router,
            page_post(&[("handle", &confirmation), ("action", action)]),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{action}");
        assert!(headers.get(header::LOCATION).is_none());
        assert!(html(&body).contains("oauth.authorization_expired"));
    }
}

/// A log sink the test reads back.
#[derive(Clone, Default)]
struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("the sink").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The process's one subscriber, at every level, into a sink the test reads.
/// It is process-wide, not this thread's: a callsite another test's thread
/// reaches first caches its interest for every thread, and under a
/// thread-local subscriber alone that interest is the other thread's none.
fn captured_log() -> Captured {
    static SINK: std::sync::OnceLock<Captured> = std::sync::OnceLock::new();
    SINK.get_or_init(|| {
        let sink = Captured::default();
        let writer = sink.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("no other subscriber in this test binary");
        sink
    })
    .clone()
}

#[tokio::test]
async fn no_log_line_of_a_full_flow_carries_a_code_token_verifier_handle_or_their_digests() {
    let sink = captured_log();
    let rig = rig(Some(ISSUER));
    let client_id = register_client(&rig, "Example MCP client").await;
    let connect_code = issue_connector(&rig).await;
    let mut secrets = vec![
        connect_code.clone(),
        VERIFIER.to_owned(),
        CHALLENGE.to_owned(),
    ];
    let (_, _, body) = send(&rig.router, authorize_get(&client_id, "st")).await;
    let first = handle_of(&html(&body));
    let (_, _, body) = send(
        &rig.router,
        page_post(&[("handle", &first), ("connect_code", "0000-0000-0000")]),
    )
    .await;
    let second = handle_of(&html(&body));
    let (_, _, body) = send(
        &rig.router,
        page_post(&[("handle", &second), ("connect_code", &connect_code)]),
    )
    .await;
    let confirmation = handle_of(&html(&body));
    let (status, headers, _) = send(
        &rig.router,
        page_post(&[("handle", &confirmation), ("action", "connect")]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, params) = redirect_query(&headers);
    let code = params
        .iter()
        .find(|(n, _)| n == "code")
        .map(|(_, v)| v.clone())
        .expect("a code");
    let (status, _, response) = send(
        &rig.router,
        token_request(&format!(
            "grant_type=authorization_code&code={code}&client_id={client_id}\
             &redirect_uri={}&code_verifier={VERIFIER}",
            pct(CALLBACK)
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pair = json_of(&response);
    let refresh = pair["refresh_token"].as_str().expect("a refresh token");
    let (status, _, response) = send(
        &rig.router,
        token_request(&format!(
            "grant_type=refresh_token&refresh_token={refresh}&client_id={client_id}"
        )),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let rotated = json_of(&response);
    secrets.extend([
        first,
        second,
        confirmation,
        code,
        pair["access_token"].as_str().expect("a token").to_owned(),
        refresh.to_owned(),
        rotated["access_token"]
            .as_str()
            .expect("a token")
            .to_owned(),
        rotated["refresh_token"]
            .as_str()
            .expect("a token")
            .to_owned(),
    ]);

    let log = String::from_utf8(sink.0.lock().expect("the sink").clone()).expect("UTF-8");
    assert!(
        log.contains("oauth_authorize"),
        "the page's request lines are logged: {log}"
    );
    assert!(
        log.contains(CALLBACK) && !log.contains(&format!("{CALLBACK}?")),
        "the redirect is logged without its query: {log}"
    );
    for secret in &secrets {
        let digest = Sha256Hash::digest(secret.as_bytes()).0;
        for form in [
            secret.clone(),
            hex::encode(digest),
            base64(&digest),
            base64url(&digest),
        ] {
            assert!(!log.contains(&form), "{form} in {log}");
        }
    }
}

// ---------------------------------------------------------------------------
// A reviewer's connect code (ADR-0067 d3)
// ---------------------------------------------------------------------------

/// `POST /v2/keys/{key}/connect-codes` takes `ttl_seconds`: a lifetime past
/// the default is minted for a key confined to one workarea and refused
/// `not_confined` for one that reaches the universe, a lifetime outside 1 to
/// 2,592,000 seconds is `request.validation` naming `/ttl_seconds`, and
/// `GET /v2/keys/{key}` names the live code as extended without showing it.
#[tokio::test]
async fn a_reviewer_code_is_minted_for_a_confined_key_and_named_on_the_key() {
    let rig = rig(Some(ISSUER));
    let boot = rig
        .app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    let demo = rig
        .app
        .create_workarea(
            &admin,
            &Name::parse("demo").expect("a name"),
            Some(boot.root_workarea_id.into()),
            None,
        )
        .await
        .expect("a workarea")
        .workarea
        .workarea_id;
    let connector = |name: &str, node| IssueKeyRequest {
        profile: None,
        role: Role::Writer,
        name: Name::parse(name).expect("a name"),
        principal: format!("crew/{name}"),
        held_by: Some(HeldBy::Agent),
        default_workarea: None,
        crew: None,
        grants: vec![GrantRequest {
            node,
            role: Role::Writer,
            deny: Vec::new(),
            covers: Covers::Subtree,
        }],
        connect_code: true,
        user: None,
        guardrail_scope: None,
    };
    rig.app
        .issue_key(&admin, &connector("reviewer", demo.into()))
        .await
        .expect("a confined connector key");
    rig.app
        .issue_key(&admin, &connector("wide", boot.root_workarea_id.into()))
        .await
        .expect("a connector key on the universe");

    let post = |key: &str, body: &str| {
        Request::builder()
            .method("POST")
            .uri(format!("/v2/keys/{key}/connect-codes"))
            .header(header::AUTHORIZATION, format!("Bearer {admin}"))
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "reviewer-code")
            .body(Body::from(body.to_owned()))
            .expect("request builds")
    };

    let (status, _, body) = send(&rig.router, post("reviewer", r#"{"ttl_seconds": 604800}"#)).await;
    assert_eq!(status, StatusCode::CREATED, "{}", json_of(&body));
    let minted = json_of(&body);
    assert!(minted["code"].is_string(), "{minted}");

    let (status, _, body) = send(&rig.router, post("wide", r#"{"ttl_seconds": 604800}"#)).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        json_of(&body)
    );
    let refused = json_of(&body);
    assert_eq!(
        refused["error"]["code"], "oauth.connect_refused",
        "{refused}"
    );
    assert_eq!(
        refused["error"]["details"]["reason"], "not_confined",
        "{refused}"
    );

    for body in [r#"{"ttl_seconds": 0}"#, r#"{"ttl_seconds": 2592001}"#] {
        let (status, _, answer) = send(&rig.router, post("reviewer", body)).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{body}: {}",
            json_of(&answer)
        );
        let refused = json_of(&answer);
        assert_eq!(refused["error"]["code"], "request.validation", "{refused}");
        assert_eq!(
            refused["error"]["details"]["field"], "/ttl_seconds",
            "{refused}"
        );
    }

    let shown = Request::builder()
        .method("GET")
        .uri("/v2/keys/reviewer")
        .header(header::AUTHORIZATION, format!("Bearer {admin}"))
        .body(Body::empty())
        .expect("request builds");
    let (status, _, body) = send(&rig.router, shown).await;
    assert_eq!(status, StatusCode::OK, "{}", json_of(&body));
    let key = json_of(&body);
    assert_eq!(
        key["live_connect_code"]["code_id"], minted["code_id"],
        "{key}"
    );
    assert_eq!(key["live_connect_code"]["extended"], true, "{key}");
    assert!(
        !key.to_string()
            .contains(minted["code"].as_str().expect("code")),
        "the key's view never carries the code"
    );
}
