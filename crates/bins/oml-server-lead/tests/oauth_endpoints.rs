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
    ConfirmAction, ConfirmOutcome, GrantRequest, IssueKeyRequest,
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
    let router = ApiState::new(Arc::clone(&app), store.clone(), ping, true, None).into_router();
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
                }],
                connect_code: true,
            },
        )
        .await
        .expect("a connector key");
    let connect_code = issued
        .connect_code
        .and_then(|c| c.code)
        .expect("the code, shown once");
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
