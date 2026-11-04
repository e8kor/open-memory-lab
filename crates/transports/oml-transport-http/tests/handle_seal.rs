//! The authorization request's handle, sealed by ChaCha20-Poly1305 under a
//! key the process generates at its start and holds only in memory
//! (ADR-0031 d9 step 1): a restart, a takeover or a tampered handle names no
//! live request.

use oml_app::domain::oauth::{
    ATTEMPTS_PER_PAGE, AuthorizationExpired, AuthorizationRequest, CLAUDE_CALLBACK, Refusal,
    RequestClientKind, open_request,
};
use oml_app::domain::{EventTime, Sha256Hash};
use oml_app::ports::HandleSealer as _;
use oml_transport_http::oauth::seal::AeadSealer;

fn at(s: &str) -> EventTime {
    EventTime::parse(s).expect("the fixed-width form")
}

fn request() -> AuthorizationRequest {
    AuthorizationRequest {
        client_id: "dcr-1".to_owned(),
        kind: RequestClientKind::Registered,
        client_name: "Some client".to_owned(),
        document_host: None,
        metadata_digest: Sha256Hash::digest(b"{}"),
        redirect_uri: CLAUDE_CALLBACK.to_owned(),
        redirect_uri_sent: false,
        code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_owned(),
        state: Some("state".to_owned()),
        resource: "https://oml.example.com/mcp".to_owned(),
        expires_at: at("2026-11-03T12:10:00.000000Z"),
        attempts_left: ATTEMPTS_PER_PAGE,
    }
}

const NOW: &str = "2026-11-03T12:00:00.000000Z";

fn unopened() -> Result<AuthorizationRequest, Refusal> {
    Err(Refusal::AuthorizationExpired(
        AuthorizationExpired::Unopened,
    ))
}

#[test]
fn a_handle_sealed_by_one_process_does_not_open_under_another_process_s_key() {
    let lead = AeadSealer::generate();
    let restarted = AeadSealer::generate();
    let handle = lead.seal(&request().encode());

    let opened = lead.open(&handle);
    assert_eq!(open_request(opened.as_deref(), &at(NOW)), Ok(request()));
    assert_eq!(restarted.open(&handle), None);
    assert_eq!(
        open_request(restarted.open(&handle).as_deref(), &at(NOW)),
        unopened()
    );
}

#[test]
fn a_tampered_handle_names_no_live_request() {
    let lead = AeadSealer::generate();
    let handle = lead.seal(&request().encode());
    // Two seals of one request differ, so no handle repeats a nonce.
    assert_ne!(lead.seal(&request().encode()), handle);
    // The handle hides what it carries.
    assert!(!handle.contains(&hex_of("dcr-1")));

    for at_char in [0, handle.len() / 2, handle.len() - 1] {
        let mut tampered = handle.clone().into_bytes();
        tampered[at_char] = if tampered[at_char] == b'0' {
            b'1'
        } else {
            b'0'
        };
        let tampered = String::from_utf8(tampered).expect("hex");
        assert_eq!(lead.open(&tampered), None, "a flipped symbol at {at_char}");
        assert_eq!(
            open_request(lead.open(&tampered).as_deref(), &at(NOW)),
            unopened()
        );
    }
    for cut in [
        &handle[..handle.len() - 2],
        &handle[2..],
        "",
        "not hex at all",
        &handle[..10],
    ] {
        assert_eq!(lead.open(cut), None);
    }
    let mut longer = handle.clone();
    longer.push_str("00");
    assert_eq!(lead.open(&longer), None);
    assert_eq!(lead.open(&handle.to_uppercase()), None);
}

fn hex_of(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}
