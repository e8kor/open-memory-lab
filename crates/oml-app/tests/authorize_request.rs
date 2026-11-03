//! `GET /oauth/authorize` as a use case (ADR-0031 d4, d9 step 1;
//! `docs/api/http-api.md`): the bounds first, then the client, the redirect
//! URI, `response_type`, PKCE, `resource` and `scope`. A refusal before the
//! redirect URI is accepted is a page; a later one redirects with the
//! `state` and `iss`; a valid request becomes a sealed handle and writes no
//! row but the document cache.

use oml_app::App;
use oml_app::domain::oauth::{
    ATTEMPTS_PER_PAGE, CLAUDE_CALLBACK, ClientInvalid, Issuer, PkceRequired, RedirectRefused,
    Refusal, RequestClientKind, Validation, open_request,
};
use oml_app::domain::{EventTime, Sha256Hash};
use oml_app::doubles::{
    FixedClock, MarkerScanner, MemDocuments, MemSigner, MemStore, PlainSealer, SeqIdGen,
};
use oml_app::ports::{
    ConnectionStore as _, DocumentFetchFailure, HandleSealer as _, JournalStore as _,
};
use oml_app::types::{AuthorizeOutcome, AuthorizeParams, ClientDescription, PROVIDER_TENANT};

const SEED: [u8; 32] = [7; 32];
const ISSUER: &str = "https://oml.example.com";
const DOC_URL: &str = "https://client.example.com/oauth/metadata.json";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn app(store: MemStore) -> TestApp {
    App::new(
        store,
        MemSigner::new(SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"))
}

fn document() -> Vec<u8> {
    format!(
        r#"{{"client_id":"{DOC_URL}","client_name":"Claude","redirect_uris":["{CLAUDE_CALLBACK}"]}}"#
    )
    .into_bytes()
}

fn documents() -> MemDocuments {
    let docs = MemDocuments::new();
    docs.serve(DOC_URL, Ok((document(), 3_600)));
    docs
}

/// A valid authorize request from the document client.
fn params() -> AuthorizeParams {
    AuthorizeParams {
        client_id: Some(DOC_URL.to_owned()),
        redirect_uri: Some(CLAUDE_CALLBACK.to_owned()),
        response_type: Some("code".to_owned()),
        code_challenge: Some(CHALLENGE.to_owned()),
        code_challenge_method: Some("S256".to_owned()),
        state: Some("opaque state".to_owned()),
        resource: Some(format!("{ISSUER}/mcp")),
        scope: Some("mcp".to_owned()),
        repeated: false,
    }
}

async fn outcome(app: &TestApp, docs: &MemDocuments, params: AuthorizeParams) -> AuthorizeOutcome {
    app.authorize_request(params, docs, &PlainSealer)
        .await
        .expect("an authorize request is answered")
}

fn redirected(error: Refusal, state: Option<&str>) -> AuthorizeOutcome {
    AuthorizeOutcome::RedirectRefusal {
        error,
        redirect_uri: CLAUDE_CALLBACK.to_owned(),
        state: state.map(str::to_owned),
        iss: ISSUER.to_owned(),
    }
}

#[tokio::test]
async fn a_parameter_past_its_bound_is_a_page_refusal_never_a_redirect() {
    let docs = documents();
    let app = app(MemStore::new());
    let page = |fault| AuthorizeOutcome::PageRefusal(Refusal::RequestValidation(fault));

    let long_state = AuthorizeParams {
        state: Some("s".repeat(513)),
        ..params()
    };
    assert_eq!(
        outcome(&app, &docs, long_state).await,
        page(Validation::StateTooLong)
    );
    let long_redirect = AuthorizeParams {
        redirect_uri: Some(format!("{CLAUDE_CALLBACK}?{}", "a".repeat(2048))),
        ..params()
    };
    assert_eq!(
        outcome(&app, &docs, long_redirect).await,
        page(Validation::RedirectUriTooLong)
    );
    let malformed = AuthorizeParams {
        code_challenge: Some("short".to_owned()),
        ..params()
    };
    assert_eq!(
        outcome(&app, &docs, malformed).await,
        page(Validation::CodeChallengeMalformed)
    );
    let repeated = AuthorizeParams {
        repeated: true,
        ..params()
    };
    assert_eq!(
        outcome(&app, &docs, repeated).await,
        page(Validation::ParameterRepeated)
    );
    // Bounded before anything is fetched.
    assert_eq!(docs.fetches(), 0);
}

#[tokio::test]
async fn a_refused_client_or_redirect_uri_is_a_page_and_never_a_redirect() {
    let docs = documents();
    let app = app(MemStore::new());
    let unknown = AuthorizeParams {
        client_id: Some("dcr-never-minted".to_owned()),
        ..params()
    };
    assert_eq!(
        outcome(&app, &docs, unknown).await,
        AuthorizeOutcome::PageRefusal(Refusal::ClientInvalid(ClientInvalid::Unknown))
    );
    let busy = MemDocuments::new();
    busy.serve(DOC_URL, Err(DocumentFetchFailure::Busy));
    assert_eq!(
        outcome(&app, &busy, params()).await,
        AuthorizeOutcome::PageRefusal(Refusal::ClientInvalid(ClientInvalid::FetchBusy))
    );
    let elsewhere = AuthorizeParams {
        redirect_uri: Some("https://evil.example.com/cb".to_owned()),
        ..params()
    };
    assert_eq!(
        outcome(&app, &docs, elsewhere).await,
        AuthorizeOutcome::PageRefusal(Refusal::RedirectRefused(RedirectRefused::OutsideAllowlist))
    );
    // A later refusal does not come first: a bad response_type and no
    // PKCE still answer the redirect URI's refusal on the page.
    let both = AuthorizeParams {
        redirect_uri: Some("https://evil.example.com/cb".to_owned()),
        response_type: Some("token".to_owned()),
        code_challenge: None,
        ..params()
    };
    assert!(matches!(
        outcome(&app, &docs, both).await,
        AuthorizeOutcome::PageRefusal(Refusal::RedirectRefused(_))
    ));
}

#[tokio::test]
async fn an_absent_code_challenge_after_an_accepted_client_redirects_with_pkce_required_state_and_iss()
 {
    let docs = documents();
    let app = app(MemStore::new());
    let absent = AuthorizeParams {
        code_challenge: None,
        ..params()
    };
    let refused = outcome(&app, &docs, absent).await;
    assert_eq!(
        refused,
        redirected(
            Refusal::PkceRequired(PkceRequired::ChallengeAbsent),
            Some("opaque state")
        )
    );
    let AuthorizeOutcome::RedirectRefusal { error, .. } = refused else {
        unreachable!("asserted above")
    };
    assert_eq!(error.code(), "oauth.pkce_required");
    assert_eq!(error.authorize_error(), "invalid_request");

    // A plain method is refused the same way, and a request without state
    // redirects without one.
    let plain = AuthorizeParams {
        code_challenge_method: Some("plain".to_owned()),
        state: None,
        ..params()
    };
    assert_eq!(
        outcome(&app, &docs, plain).await,
        redirected(Refusal::PkceRequired(PkceRequired::MethodNotS256), None)
    );
    // response_type is checked before PKCE.
    let token = AuthorizeParams {
        response_type: Some("token".to_owned()),
        code_challenge: None,
        ..params()
    };
    let unsupported = Refusal::RequestValidation(Validation::ResponseTypeUnsupported);
    assert_eq!(
        outcome(&app, &docs, token).await,
        redirected(unsupported, Some("opaque state"))
    );
    assert_eq!(unsupported.authorize_error(), "unsupported_response_type");
}

#[tokio::test]
async fn a_resource_other_than_issuer_mcp_redirects_with_invalid_target() {
    let docs = documents();
    let app = app(MemStore::new());
    let other = AuthorizeParams {
        resource: Some("https://other.example.com/mcp".to_owned()),
        ..params()
    };
    assert_eq!(
        outcome(&app, &docs, other).await,
        redirected(Refusal::ResourceMismatch, Some("opaque state"))
    );
    assert_eq!(
        Refusal::ResourceMismatch.authorize_error(),
        "invalid_target"
    );

    // An absent resource binds the one resource anyway; a scope naming
    // anything but mcp redirects with invalid_scope.
    let absent = AuthorizeParams {
        resource: None,
        ..params()
    };
    assert!(matches!(
        outcome(&app, &docs, absent).await,
        AuthorizeOutcome::Page(_)
    ));
    let scope = AuthorizeParams {
        scope: Some("mcp admin".to_owned()),
        ..params()
    };
    let invalid_scope = Refusal::RequestValidation(Validation::ScopeUnsupported);
    assert_eq!(
        outcome(&app, &docs, scope).await,
        redirected(invalid_scope, Some("opaque state"))
    );
    assert_eq!(invalid_scope.authorize_error(), "invalid_scope");
}

#[tokio::test]
async fn an_authorize_request_writes_no_authorization_row() {
    let docs = documents();
    let store = MemStore::new();
    let app = app(store.clone());
    let AuthorizeOutcome::Page(page) = outcome(&app, &docs, params()).await else {
        panic!("a valid request renders the page")
    };
    assert_eq!(page.client_name, "Claude");
    assert_eq!(page.verified_by.as_deref(), Some("client.example.com"));
    assert_eq!(page.redirect_host, "claude.ai");
    assert_eq!(page.attempts_left, ATTEMPTS_PER_PAGE);

    // The handle is the request itself, alive ten minutes.
    let opened = PlainSealer.open(&page.handle);
    let now = EventTime::parse("2026-10-29T12:00:00.000000Z").expect("a time");
    let request = open_request(opened.as_deref(), &now).expect("a live request");
    assert_eq!(request.client_id, DOC_URL);
    assert_eq!(request.kind, RequestClientKind::Document);
    assert_eq!(request.document_host.as_deref(), Some("client.example.com"));
    assert_eq!(request.metadata_digest, Sha256Hash::digest(&document()));
    assert_eq!(request.redirect_uri, CLAUDE_CALLBACK);
    assert!(request.redirect_uri_sent);
    assert_eq!(request.code_challenge, CHALLENGE);
    assert_eq!(request.state.as_deref(), Some("opaque state"));
    assert_eq!(request.resource, format!("{ISSUER}/mcp"));
    assert_eq!(request.attempts_left, ATTEMPTS_PER_PAGE);
    assert_eq!(request.expires_at, page.expires_at);

    // No authorization row, and no event: only the document cache wrote.
    let handle_hash = Sha256Hash::digest(page.handle.as_bytes());
    assert_eq!(store.authorization(&handle_hash).await.expect("read"), None);
    assert_eq!(store.authorization_count(), 0);
    let journal = store.events_range(PROVIDER_TENANT, 1, 1000).await;
    assert!(journal.expect("journal").is_empty());
    assert_eq!(store.tenant_count(), 0);
    let cached = store
        .oauth_client(DOC_URL)
        .await
        .expect("read")
        .expect("the document is cached");
    assert_eq!(cached.client_name, "Claude");

    // A registered client opens a page too, its name not verified, and
    // writes nothing at all.
    store
        .insert_dcr(
            ClientDescription {
                client_id: "dcr-1".to_owned(),
                client_name: "Some client".to_owned(),
                redirect_uris: vec![CLAUDE_CALLBACK.to_owned()],
                metadata_digest: Sha256Hash::digest(b"{}"),
            },
            &now,
        )
        .await
        .expect("a registration");
    let registered = AuthorizeParams {
        client_id: Some("dcr-1".to_owned()),
        redirect_uri: None,
        ..params()
    };
    let AuthorizeOutcome::Page(page) = outcome(&app, &docs, registered).await else {
        panic!("a registered client's page")
    };
    assert_eq!(page.verified_by, None);
    let request = open_request(PlainSealer.open(&page.handle).as_deref(), &now).expect("live");
    assert_eq!(request.kind, RequestClientKind::Registered);
    assert!(!request.redirect_uri_sent);
    assert_eq!(store.authorization_count(), 0);
}
