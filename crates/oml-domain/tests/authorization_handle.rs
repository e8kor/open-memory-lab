//! The sealed authorization request's plaintext: its canonical byte
//! encoding and when an opened handle names a live request (ADR-0031 d4,
//! d9 step 1; `docs/api/http-api.md`, `GET /oauth/authorize`).

use oml_domain::oauth::{
    ATTEMPTS_PER_PAGE, AUTHORIZATION_REQUEST_SECONDS, AuthorizationExpired, AuthorizationRequest,
    CLAUDE_CALLBACK, Refusal, RequestClientKind, open_request,
};
use oml_domain::{EventTime, Sha256Hash};

fn at(s: &str) -> EventTime {
    EventTime::parse(s).expect("the fixed-width form")
}

fn request(expires_at: EventTime) -> AuthorizationRequest {
    AuthorizationRequest {
        client_id: "https://client.example.com/oauth/metadata.json".to_owned(),
        kind: RequestClientKind::Document,
        client_name: "Claude".to_owned(),
        document_host: Some("client.example.com".to_owned()),
        metadata_digest: Sha256Hash::digest(b"{}"),
        redirect_uri: CLAUDE_CALLBACK.to_owned(),
        redirect_uri_sent: true,
        code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_owned(),
        state: Some("xyz \u{e9}".to_owned()),
        resource: "https://oml.example.com/mcp".to_owned(),
        expires_at,
        attempts_left: ATTEMPTS_PER_PAGE,
    }
}

#[test]
fn an_authorization_request_decodes_from_its_own_encoding_and_from_nothing_else() {
    let sealed = request(at("2026-11-03T12:10:00.000000Z"));
    let bytes = sealed.encode();
    assert_eq!(AuthorizationRequest::decode(&bytes), Some(sealed.clone()));

    let registered = AuthorizationRequest {
        kind: RequestClientKind::Registered,
        client_id: "dcr-1".to_owned(),
        document_host: None,
        state: None,
        redirect_uri_sent: false,
        attempts_left: 1,
        ..sealed.clone()
    };
    assert_eq!(
        AuthorizationRequest::decode(&registered.encode()),
        Some(registered)
    );

    // A byte more, a byte fewer, or none at all decode to nothing.
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(AuthorizationRequest::decode(&longer), None);
    assert_eq!(
        AuthorizationRequest::decode(&bytes[..bytes.len() - 1]),
        None
    );
    assert_eq!(AuthorizationRequest::decode(&[]), None);
    // A document client always names its host, and a registered one never.
    let hostless = AuthorizationRequest {
        document_host: None,
        ..sealed.clone()
    };
    assert_eq!(AuthorizationRequest::decode(&hostless.encode()), None);
    // A request carries one to five attempts.
    let spent = AuthorizationRequest {
        attempts_left: 0,
        ..sealed
    };
    assert_eq!(AuthorizationRequest::decode(&spent.encode()), None);
}

#[test]
fn a_handle_past_ten_minutes_names_no_live_request() {
    assert_eq!(AUTHORIZATION_REQUEST_SECONDS, 600);
    let opened_at = at("2026-11-03T12:00:00.000000Z");
    let expires_at = EventTime::from_unix_micros(
        opened_at.unix_micros() + AUTHORIZATION_REQUEST_SECONDS * 1_000_000,
    )
    .expect("ten minutes later");
    let bytes = request(expires_at.clone()).encode();

    let live = open_request(Some(&bytes), &at("2026-11-03T12:09:59.999999Z"));
    assert_eq!(live, Ok(request(expires_at)));
    for past in ["2026-11-03T12:10:00.000000Z", "2026-11-03T13:00:00.000000Z"] {
        assert_eq!(
            open_request(Some(&bytes), &at(past)),
            Err(Refusal::AuthorizationExpired(AuthorizationExpired::Lapsed))
        );
    }
}

#[test]
fn a_handle_that_does_not_open_names_no_live_request() {
    let now = at("2026-11-03T12:00:00.000000Z");
    let unopened = Err(Refusal::AuthorizationExpired(
        AuthorizationExpired::Unopened,
    ));
    assert_eq!(open_request(None, &now), unopened);
    assert_eq!(open_request(Some(b"not a request"), &now), unopened);
    assert_eq!(
        Refusal::AuthorizationExpired(AuthorizationExpired::Unopened).code(),
        "oauth.authorization_expired"
    );
}
