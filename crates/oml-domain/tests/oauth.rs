//! ADR-0031's pure modules: the connect code (d8), PKCE and the authorize
//! bounds (d4), the redirect allowlist (d6), client identity (d5), the
//! fetch-address allow-list (d5), the issuer, discovery and the challenge
//! (d1, d2, d3), token formats (d10), refresh rotation and code redemption
//! with reuse detection (d10, d11), the connection lifecycle (d7, d8, d9,
//! d12), and the refusals' codes (d13).

use std::net::IpAddr;

use oml_domain::oauth::{
    ATTEMPTS_PER_PAGE, AuthorizationCodeInvalid, AuthorizationExpired, CLAUDE_CALLBACK,
    CROCKFORD_ALPHABET, ChallengeCause, ClientId, ClientInvalid, ConnectCode, ConnectRefused,
    ConnectionAct, ConnectionState, ExchangeDecision, ExchangeRequest, ExchangeRow, Issuer,
    IssuerError, KeyState, OAUTH_CODES, PkceRequired, RedirectKind, RedirectRefused,
    RefreshDecision, RefreshInvalid, RefreshRow, Refusal, RevokedCause, SelectedRedirect, Token,
    TokenKind, Validation, admit_fetch_answers, admit_redirect_uri, admitted_fetch_address,
    after_wrong_entry, authorization_server_document, base62_43, check_authorize_bounds,
    check_code_entry, check_confirmation, check_pkce_method, check_resource, classify_client_id,
    connection_transition, decide_exchange, decide_refresh, is_key_value, mcp_challenge,
    protected_resource_document, redirect_matches, refresh_token_digest, s256_challenge,
    select_redirect, token_kind, validate_document, validate_registration, verify_s256,
};
use oml_domain::oauth::{AuthorizeBounds, CodeRow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// RFC 7636 Appendix B.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

const DOC_URL: &str = "https://client.example.com/oauth/metadata.json";
const CLIENT: &str = DOC_URL;

fn issuer() -> Issuer {
    Issuer::parse("https://oml.example.com").expect("a public origin")
}

fn ip(s: &str) -> IpAddr {
    s.parse().expect("an address literal")
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("serializable")
}

fn without(mut value: Value, member: &str) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove(member);
    }
    value
}

// ---------------------------------------------------------------- d8

#[test]
fn a_connect_code_is_twelve_crockford_symbols_in_three_groups_of_four() {
    assert_eq!(CROCKFORD_ALPHABET.len(), 32);
    for absent in ['I', 'L', 'O', 'U'] {
        assert!(!CROCKFORD_ALPHABET.contains(absent), "{absent}");
    }

    assert_eq!(ConnectCode::from_random([0; 8]).grouped(), "0000-0000-0000");
    assert_eq!(
        ConnectCode::from_random([0xff; 8]).grouped(),
        "ZZZZ-ZZZZ-ZZZZ"
    );
    assert_eq!(
        ConnectCode::from_random([0, 0, 0, 0, 0, 0, 0, 1]).grouped(),
        "0000-0000-0001"
    );
    assert_eq!(
        ConnectCode::from_random([0, 0, 0, 0, 0, 0, 0, 32]).grouped(),
        "0000-0000-0010"
    );
    // The top four of the 64 random bits are not part of the code: 60 bits.
    assert_eq!(
        ConnectCode::from_random([0xf0, 0, 0, 0, 0, 0, 0, 0]).grouped(),
        "0000-0000-0000"
    );

    // Expected value computed independently (Python) for 0x0123456789abcdef.
    let code = ConnectCode::from_random([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
    let shown = code.grouped();
    assert_eq!(shown, "28T5-CY4T-QKFF");
    assert_eq!(shown.len(), 14);
    for (i, c) in shown.char_indices() {
        if i == 4 || i == 9 {
            assert_eq!(c, '-');
        } else {
            assert!(CROCKFORD_ALPHABET.contains(c), "{c}");
        }
    }
    assert_eq!(code.symbols(), "28T5CY4TQKFF");
}

#[test]
fn entry_ignores_case_hyphens_and_spaces_and_reads_o_as_zero_and_i_l_as_one() {
    let canonical = ConnectCode::parse("01KQ-M9XD-4T1A").expect("a code");
    assert_eq!(canonical.grouped(), "01KQ-M9XD-4T1A");
    for entry in [
        "01kq-m9xd-4t1a",
        "01KQM9XD4T1A",
        " 01KQ M9XD 4T1A ",
        "0-1-K-Q-M-9-X-D-4-T-1-A",
        "OIKQ-M9XD-4TLA",
        "oikq m9xd-4tla",
        "0lKQ-M9XD-4TiA",
    ] {
        assert_eq!(ConnectCode::parse(entry), Ok(canonical.clone()), "{entry}");
    }
}

#[test]
fn a_code_with_u_or_a_wrong_length_never_decodes() {
    for entry in [
        "01KQ-M9XD-4T1U",
        "u1KQ-M9XD-4T1A",
        "01KQ-M9XD-4T1",
        "01KQ-M9XD-4T1AA",
        "",
        "----",
        "01KQ_M9XD_4T1A",
        "01KQ-M9XD-4T1A\t",
        "01KQ-M9XD-4T1\u{c4}",
    ] {
        assert_eq!(
            ConnectCode::parse(entry),
            Err(Refusal::ConnectCodeInvalid),
            "{entry:?}"
        );
    }
}

#[test]
fn a_code_is_stored_as_the_sha256_of_its_twelve_decoded_symbols() {
    let code = ConnectCode::parse("oikq m9xd-4tla").expect("a code");
    let expected: [u8; 32] = Sha256::digest(b"01KQM9XD4T1A").into();
    assert_eq!(code.digest(), expected);
    // Computed independently (Python hashlib).
    assert_eq!(
        hex::encode(code.digest()),
        "e79cb0fc65639304c652e5227401866186deaf2bcdc6a2a8693dff7d3bc9fccf"
    );
}

#[test]
fn a_connect_code_or_a_token_never_prints_through_debug() {
    let code = ConnectCode::parse("01KQ-M9XD-4T1A").expect("a code");
    let shown = format!("{code:?}");
    assert!(!shown.contains("01KQ") && !shown.contains("M9XD") && !shown.contains("4T1A"));

    let token = Token::mint(TokenKind::Refresh, [9; 32]);
    let shown = format!("{token:?}");
    assert!(!shown.contains(&token.as_str()["oml_rt_".len()..]));
}

#[test]
fn an_unknown_expired_spent_voided_or_dead_keys_code_answers_one_refusal() {
    let live = CodeRow {
        redeemed: false,
        voided: false,
        expired: false,
        key: KeyState::Active,
    };
    assert_eq!(check_code_entry(Some(&live)), Ok(()));
    let one_answer = Err(Refusal::ConnectCodeInvalid);
    assert_eq!(check_code_entry(None), one_answer);
    for row in [
        CodeRow {
            redeemed: true,
            ..live
        },
        CodeRow {
            voided: true,
            ..live
        },
        CodeRow {
            expired: true,
            ..live
        },
        CodeRow {
            key: KeyState::Revoked,
            ..live
        },
        CodeRow {
            key: KeyState::Expired,
            ..live
        },
    ] {
        assert_eq!(check_code_entry(Some(&row)), one_answer, "{row:?}");
    }
}

#[test]
fn the_fifth_wrong_entry_ends_the_authorization_request() {
    assert_eq!(ATTEMPTS_PER_PAGE, 5);
    let mut left = ATTEMPTS_PER_PAGE;
    for expected in [4, 3, 2, 1] {
        left = after_wrong_entry(left).expect("the form again");
        assert_eq!(left, expected);
    }
    assert_eq!(
        after_wrong_entry(left),
        Err(Refusal::AuthorizationExpired(
            AuthorizationExpired::AttemptsSpent
        ))
    );
}

#[test]
fn a_confirmation_whose_code_was_voided_answers_authorization_expired() {
    assert_eq!(check_confirmation(false), Ok(()));
    assert_eq!(
        check_confirmation(true),
        Err(Refusal::AuthorizationExpired(
            AuthorizationExpired::CodeVoided
        ))
    );
}

// ---------------------------------------------------------------- d4

#[test]
fn an_s256_verifier_matches_its_challenge_and_plain_is_refused() {
    assert_eq!(s256_challenge(VERIFIER), CHALLENGE);
    assert!(verify_s256(VERIFIER, CHALLENGE));
    assert!(!verify_s256(
        "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXl",
        CHALLENGE
    ));
    // plain would compare the verifier with itself; under S256 it never matches.
    assert!(!verify_s256(VERIFIER, VERIFIER));

    assert_eq!(check_pkce_method(Some(CHALLENGE), Some("S256")), Ok(()));
    assert_eq!(
        check_pkce_method(Some(CHALLENGE), Some("plain")),
        Err(Refusal::PkceRequired(PkceRequired::MethodNotS256))
    );
    assert_eq!(
        check_pkce_method(Some(CHALLENGE), Some("s256")),
        Err(Refusal::PkceRequired(PkceRequired::MethodNotS256))
    );
    // An absent method is plain (RFC 7636 §4.3).
    assert_eq!(
        check_pkce_method(Some(CHALLENGE), None),
        Err(Refusal::PkceRequired(PkceRequired::MethodAbsent))
    );
    assert_eq!(
        check_pkce_method(None, Some("S256")),
        Err(Refusal::PkceRequired(PkceRequired::ChallengeAbsent))
    );
}

#[test]
fn a_verifier_outside_rfc_7636_syntax_never_verifies() {
    for verifier in [
        "",
        &"a".repeat(42),
        &"a".repeat(129),
        &format!("{}+", "a".repeat(42)),
        &format!("{} ", "a".repeat(42)),
    ] {
        let challenge = s256_challenge(verifier);
        assert!(!verify_s256(verifier, &challenge), "{verifier:?}");
    }
    let shortest = "a".repeat(43);
    assert!(verify_s256(&shortest, &s256_challenge(&shortest)));
    let longest = format!("{}-._~", "Z9".repeat(62));
    assert_eq!(longest.len(), 128);
    assert!(verify_s256(&longest, &s256_challenge(&longest)));
}

#[test]
fn authorize_parameters_past_their_bounds_are_request_validation() {
    let (state, client_id, redirect_uri) = ("s".repeat(512), "c".repeat(2048), "r".repeat(2048));
    let at_bounds = AuthorizeBounds {
        state: Some(&state),
        client_id: Some(&client_id),
        redirect_uri: Some(&redirect_uri),
        code_challenge: Some(CHALLENGE),
    };
    assert_eq!(check_authorize_bounds(&at_bounds), Ok(()));
    let none = AuthorizeBounds {
        state: None,
        client_id: None,
        redirect_uri: None,
        code_challenge: None,
    };
    assert_eq!(check_authorize_bounds(&none), Ok(()));

    let long_state = "s".repeat(513);
    let long_client = "c".repeat(2049);
    let long_redirect = "r".repeat(2049);
    for (bounds, fault) in [
        (
            AuthorizeBounds {
                state: Some(&long_state),
                ..at_bounds
            },
            Validation::StateTooLong,
        ),
        (
            AuthorizeBounds {
                client_id: Some(&long_client),
                ..at_bounds
            },
            Validation::ClientIdTooLong,
        ),
        (
            AuthorizeBounds {
                redirect_uri: Some(&long_redirect),
                ..at_bounds
            },
            Validation::RedirectUriTooLong,
        ),
    ] {
        assert_eq!(
            check_authorize_bounds(&bounds),
            Err(Refusal::RequestValidation(fault))
        );
    }
    let short = &CHALLENGE[..42];
    let long = format!("{CHALLENGE}A");
    let padded = format!("{}=", &CHALLENGE[..42]);
    let plus = format!("{}+", &CHALLENGE[..42]);
    for challenge in [short, &long, &padded, &plus] {
        assert_eq!(
            check_authorize_bounds(&AuthorizeBounds {
                code_challenge: Some(challenge),
                ..at_bounds
            }),
            Err(Refusal::RequestValidation(
                Validation::CodeChallengeMalformed
            )),
            "{challenge}"
        );
    }
}

// ---------------------------------------------------------------- d6

#[test]
fn only_the_claude_callback_and_loopback_redirects_are_admitted() {
    assert_eq!(
        admit_redirect_uri("https://claude.ai/api/mcp/auth_callback"),
        Ok(RedirectKind::ClaudeCallback)
    );
    for loopback in [
        "http://127.0.0.1/callback",
        "http://127.0.0.1:53682/callback",
        "http://[::1]:8080/cb",
        "http://localhost:1/cb",
        "http://localhost:65535/cb?x=1",
        "http://localhost/",
    ] {
        assert_eq!(
            admit_redirect_uri(loopback),
            Ok(RedirectKind::Loopback),
            "{loopback}"
        );
    }
    for refused in [
        "https://claude.ai/api/mcp/auth_callback/",
        "https://claude.ai/api/mcp/auth_callback?x=1",
        "https://claude.ai/api/mcp/auth_callback#f",
        "HTTPS://claude.ai/api/mcp/auth_callback",
        "https://claude.ai:443/api/mcp/auth_callback",
        "https://claude.com/api/mcp/auth_callback",
        "https://evil.example/callback",
        "com.example.app:/callback",
        "claude://callback",
        "http://example.com/cb",
        "https://127.0.0.1/cb",
        "https://localhost/cb",
        // the ADR's refusals
        "http://localhost:1@evil.example/cb",
        "http://localhost:/cb",
        "http://localhost:08080/cb",
        "http://localhost:65536/cb",
        "http://127.1/cb",
        "http://0x7f000001/cb",
        "http://localhost./cb",
        "http://LOCALHOST/cb",
        // and the rest of the grammar
        "http://localhost:0/cb",
        "http://localhost:123456/cb",
        "http://localhost",
        "http://localhost?x=1",
        "http://localhost/cb#f",
        "http://localhost/a\\b",
        "http://localhost/a%2Fb",
        "http://localhost/a%2fb",
        "http://localhost/a%5Cb",
        "http://localhost/a%5cb",
        "http://localhost/a b",
        "http://localhost/a\nb",
        "http://localhost/a\u{7f}",
        "http://localhost/cb\u{a0}",
        "http://localhost/cb@x",
        "http://[0:0:0:0:0:0:0:1]/cb",
        "http://127.0.0.2/cb",
        "*",
        "",
    ] {
        assert_eq!(
            admit_redirect_uri(refused),
            Err(Refusal::RedirectRefused(RedirectRefused::OutsideAllowlist)),
            "{refused:?}"
        );
    }
}

#[test]
fn a_loopback_redirect_matches_on_any_port_with_its_exact_path() {
    assert!(redirect_matches(
        "http://127.0.0.1/callback",
        "http://127.0.0.1:53682/callback"
    ));
    assert!(redirect_matches(
        "http://127.0.0.1:8080/callback",
        "http://127.0.0.1:9090/callback"
    ));
    assert!(redirect_matches(
        "http://localhost/cb?x=1",
        "http://localhost:1234/cb?x=1"
    ));
    assert!(redirect_matches("http://[::1]:5/cb", "http://[::1]/cb"));
    assert!(redirect_matches(CLAUDE_CALLBACK, CLAUDE_CALLBACK));

    assert!(!redirect_matches(
        "http://127.0.0.1/callback",
        "http://localhost:53682/callback"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1/callback",
        "http://127.0.0.1:53682/callbac"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1/callback",
        "http://127.0.0.1:53682/Callback"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1/callback",
        "http://127.0.0.1:08080/callback"
    ));

    let registered = ["http://127.0.0.1/callback".to_owned()];
    assert_eq!(
        select_redirect(&registered, Some("http://127.0.0.1:53682/callback")),
        Ok(SelectedRedirect {
            uri: "http://127.0.0.1:53682/callback",
            sent: true,
        })
    );
    assert_eq!(
        select_redirect(&registered, None),
        Ok(SelectedRedirect {
            uri: "http://127.0.0.1/callback",
            sent: false,
        })
    );
}

#[test]
fn a_redirect_differing_by_a_trailing_slash_or_query_is_refused() {
    assert!(!redirect_matches(
        CLAUDE_CALLBACK,
        "https://claude.ai/api/mcp/auth_callback/"
    ));
    assert!(!redirect_matches(
        CLAUDE_CALLBACK,
        "https://claude.ai/api/mcp/auth_callback?x=1"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1/cb",
        "http://127.0.0.1:5000/cb/"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1/cb",
        "http://127.0.0.1:5000/cb?x=1"
    ));
    assert!(!redirect_matches(
        "http://127.0.0.1/cb?x=1",
        "http://127.0.0.1:5000/cb?x=2"
    ));

    let claude = [CLAUDE_CALLBACK.to_owned()];
    assert_eq!(
        select_redirect(&claude, Some("https://claude.ai/api/mcp/auth_callback/")),
        Err(Refusal::RedirectRefused(RedirectRefused::OutsideAllowlist))
    );
    let loopback = ["http://127.0.0.1/cb".to_owned()];
    for presented in ["http://127.0.0.1:5000/cb/", "http://127.0.0.1:5000/cb?x=1"] {
        assert_eq!(
            select_redirect(&loopback, Some(presented)),
            Err(Refusal::RedirectRefused(RedirectRefused::NotRegistered)),
            "{presented}"
        );
    }
}

#[test]
fn a_client_with_several_redirects_must_name_one() {
    let registered = [CLAUDE_CALLBACK.to_owned(), "http://127.0.0.1/cb".to_owned()];
    assert_eq!(
        select_redirect(&registered, None),
        Err(Refusal::RedirectRefused(RedirectRefused::Omitted))
    );
    assert_eq!(
        select_redirect(&registered, Some(CLAUDE_CALLBACK)),
        Ok(SelectedRedirect {
            uri: CLAUDE_CALLBACK,
            sent: true,
        })
    );
}

// ---------------------------------------------------------------- d5

fn document(client_id: &str) -> Value {
    json!({
        "client_id": client_id,
        "client_name": "Example client",
        "redirect_uris": [CLAUDE_CALLBACK],
    })
}

fn document_url() -> oml_domain::oauth::DocumentUrl<'static> {
    match classify_client_id(DOC_URL) {
        Ok(ClientId::Document(url)) => url,
        other => panic!("not a document URL: {other:?}"),
    }
}

#[test]
fn a_document_whose_client_id_differs_from_its_url_is_refused() {
    let url = document_url();
    assert_eq!(url.as_str(), DOC_URL);
    assert_eq!(url.host(), "client.example.com");
    let metadata = validate_document(&url, &bytes(&document(DOC_URL))).expect("admitted");
    assert_eq!(metadata.client_name, "Example client");
    assert_eq!(metadata.redirect_uris, vec![CLAUDE_CALLBACK.to_owned()]);

    let mismatch = Err(Refusal::ClientInvalid(ClientInvalid::ClientIdMismatch));
    for other in [
        "https://client.example.com/oauth/metadata.json/",
        "https://client.example.com/oauth/other.json",
        "https://CLIENT.example.com/oauth/metadata.json",
        "https://client.example.com/oauth/metadata.json?x=1",
    ] {
        assert_eq!(
            validate_document(&url, &bytes(&document(other))),
            mismatch,
            "{other}"
        );
    }
    let absent = without(document(DOC_URL), "client_id");
    assert_eq!(validate_document(&url, &bytes(&absent)), mismatch);
    let mut not_a_string = document(DOC_URL);
    not_a_string["client_id"] = json!([DOC_URL]);
    assert_eq!(validate_document(&url, &bytes(&not_a_string)), mismatch);
}

#[test]
fn a_document_that_is_not_json_lacks_a_member_or_asks_for_client_authentication_is_refused() {
    let url = document_url();
    let refused = |reason| Err(Refusal::ClientInvalid(reason));

    assert_eq!(
        validate_document(&url, b"<html>"),
        refused(ClientInvalid::NotJson)
    );
    assert_eq!(
        validate_document(&url, b"[1]"),
        refused(ClientInvalid::NotJson)
    );
    for member in ["client_name", "redirect_uris"] {
        let doc = without(document(DOC_URL), member);
        assert_eq!(
            validate_document(&url, &bytes(&doc)),
            refused(ClientInvalid::MissingMember),
            "{member}"
        );
    }
    let mut empty = document(DOC_URL);
    empty["redirect_uris"] = json!([]);
    assert_eq!(
        validate_document(&url, &bytes(&empty)),
        refused(ClientInvalid::MissingMember)
    );

    let mut none = document(DOC_URL);
    none["token_endpoint_auth_method"] = json!("none");
    assert!(validate_document(&url, &bytes(&none)).is_ok());
    for method in ["client_secret_basic", "private_key_jwt"] {
        let mut doc = document(DOC_URL);
        doc["token_endpoint_auth_method"] = json!(method);
        assert_eq!(
            validate_document(&url, &bytes(&doc)),
            refused(ClientInvalid::ClientAuthentication),
            "{method}"
        );
    }

    let mut outside = document(DOC_URL);
    outside["redirect_uris"] = json!([CLAUDE_CALLBACK, "https://evil.example/cb"]);
    assert_eq!(
        validate_document(&url, &bytes(&outside)),
        Err(Refusal::RedirectRefused(RedirectRefused::OutsideAllowlist))
    );

    // Past the 16 KiB fetch bound the document was never fully received.
    let mut large = document(DOC_URL);
    large["padding"] = json!("x".repeat(16 * 1024));
    assert_eq!(
        validate_document(&url, &bytes(&large)),
        refused(ClientInvalid::Fetch)
    );
}

#[test]
fn a_document_url_outside_the_admitted_form_is_refused() {
    for admitted in [
        DOC_URL,
        "https://xn--bcher-kva.example/m.json",
        "https://a.b.example/oauth/client/metadata",
        "https://client.example.com/a%41b",
        "https://client.example.com/a//b",
        "https://client-1.example.com/a@b",
    ] {
        assert!(
            matches!(classify_client_id(admitted), Ok(ClientId::Document(_))),
            "{admitted}"
        );
    }
    for refused in [
        "https://client.example.com",
        "https://client.example.com/",
        "https://user@client.example.com/m.json",
        "https://user:pw@client.example.com/m.json",
        "https://client.example.com/m.json#f",
        "https://client.example.com/m.json?x=1",
        "https://client.example.com/./m.json",
        "https://client.example.com/a/../m.json",
        "https://client.example.com/a/..",
        "https://client.example.com/a/%2e%2e/m.json",
        "https://client.example.com/a%2Fb.json",
        "https://client.example.com/a%2fb.json",
        "https://client.example.com/a%2Eb.json",
        "https://client.example.com:443/m.json",
        "https://client.example.com:8443/m.json",
        "https://Client.example.com/m.json",
        "HTTPS://client.example.com/m.json",
        "https://client.example.com/a b",
        "https://client.example.com/a\\b",
        "https://client.example.com/a\"b",
        "https://client.example.com/a%zz",
        "https://client.example.com/a%4",
        "https://b\u{fc}cher.example/m.json",
        "https://client.example.com/\u{e9}",
        "https://127.0.0.1/m.json",
        "https://[::1]/m.json",
        "https://client.example.com./m.json",
        "https://client..example.com/m.json",
        "https://-client.example.com/m.json",
        "https://client-.example.com/m.json",
        "https://client.example.123/m.json",
        "https:///m.json",
    ] {
        assert_eq!(
            classify_client_id(refused).map(|_| ()),
            Err(Refusal::ClientInvalid(ClientInvalid::ClientIdForm)),
            "{refused:?}"
        );
    }
    // Anything that is not an https URL names a registered client.
    for registered in ["c_0123456789abcdef", "http://client.example.com/m.json"] {
        assert_eq!(
            classify_client_id(registered),
            Ok(ClientId::Registered(registered))
        );
    }
}

fn registration() -> Value {
    json!({
        "client_name": "Example client",
        "redirect_uris": ["http://127.0.0.1/callback"],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    })
}

#[test]
fn a_registration_asking_for_client_authentication_is_refused() {
    let admitted = validate_registration(&bytes(&registration())).expect("admitted");
    assert_eq!(admitted.client_name, "Example client");
    assert_eq!(admitted.redirect_uris, vec!["http://127.0.0.1/callback"]);

    // Absent reads as client_secret_basic (RFC 7591 §2) and is registered as none.
    let absent = without(registration(), "token_endpoint_auth_method");
    assert!(validate_registration(&bytes(&absent)).is_ok());

    for method in [
        json!("client_secret_basic"),
        json!("client_secret_post"),
        json!("private_key_jwt"),
        json!(""),
        json!(1),
        json!(null),
    ] {
        let mut body = registration();
        body["token_endpoint_auth_method"] = method.clone();
        assert_eq!(
            validate_registration(&bytes(&body)).map(|_| ()),
            Err(Refusal::RequestValidation(
                Validation::TokenEndpointAuthMethod
            )),
            "{method}"
        );
    }
}

#[test]
fn a_registration_without_a_name_or_outside_its_grants_or_redirects_is_refused() {
    let refused = |fault| Err(Refusal::RequestValidation(fault));

    let body = without(registration(), "client_name");
    assert_eq!(
        validate_registration(&bytes(&body)).map(|_| ()),
        refused(Validation::ClientNameMissing)
    );
    let body = without(registration(), "redirect_uris");
    assert_eq!(
        validate_registration(&bytes(&body)).map(|_| ()),
        refused(Validation::RedirectUrisMissing)
    );
    let mut body = registration();
    body["grant_types"] = json!(["authorization_code", "client_credentials"]);
    assert_eq!(
        validate_registration(&bytes(&body)).map(|_| ()),
        refused(Validation::GrantTypes)
    );
    let mut body = registration();
    body["response_types"] = json!(["token"]);
    assert_eq!(
        validate_registration(&bytes(&body)).map(|_| ()),
        refused(Validation::ResponseTypes)
    );
    assert_eq!(
        validate_registration(b"not json").map(|_| ()),
        refused(Validation::NotJsonObject)
    );

    let mut body = registration();
    body["redirect_uris"] = json!(["http://127.0.0.1/callback", "myapp://callback"]);
    assert_eq!(
        validate_registration(&bytes(&body)).map(|_| ()),
        Err(Refusal::RedirectRefused(RedirectRefused::OutsideAllowlist))
    );

    // grant_types and response_types default when absent (RFC 7591 §2).
    let body = without(without(registration(), "grant_types"), "response_types");
    assert!(validate_registration(&bytes(&body)).is_ok());
}

#[test]
fn loopback_private_shared_link_local_unique_local_and_multicast_addresses_are_refused() {
    for refused in [
        // IPv4: loopback, private, shared, link-local, multicast, and the
        // rest of the registry's not-globally-reachable blocks.
        "127.0.0.1",
        "127.255.255.254",
        "10.0.0.1",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "100.64.0.1",
        "100.127.255.255",
        "169.254.169.254",
        "224.0.0.1",
        "239.255.255.250",
        "0.0.0.0",
        "0.1.2.3",
        "192.0.0.1",
        "192.0.0.9",
        "192.0.2.1",
        "198.18.0.1",
        "198.19.255.255",
        "198.51.100.1",
        "203.0.113.1",
        "240.0.0.1",
        "255.255.255.255",
        "192.88.99.1",
        // IPv6: loopback, unspecified, link-local, unique-local, site-local,
        // multicast, discard, and every form that embeds an IPv4 address.
        "::1",
        "::",
        "fe80::1",
        "fc00::1",
        "fd12:3456::1",
        "fec0::1",
        "ff02::1",
        "ff0e::1",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "::ffff:8.8.8.8",
        "::ffff:0:8.8.8.8",
        "::8.8.8.8",
        "64:ff9b::808:808",
        "64:ff9b:1::1",
        "2002:808:808::1",
        "2001::1",
        "2001:2::1",
        "2001:1ff:ffff::1",
        "2001:db8::1",
        "3fff::1",
        "3fff:fff:ffff::1",
        "100::1",
        "5f00::1",
    ] {
        assert!(!admitted_fetch_address(ip(refused)), "{refused}");
    }
    for admitted in [
        "8.8.8.8",
        "1.1.1.1",
        "160.79.104.1",
        "172.15.255.255",
        "172.32.0.1",
        "100.63.255.255",
        "100.128.0.1",
        "192.88.98.255",
        "192.0.1.1",
        "223.255.255.255",
        "2606:4700:4700::1111",
        "2a00:1450:4001::1",
        "2001:200::1",
        "3fff:1000::1",
    ] {
        assert!(admitted_fetch_address(ip(admitted)), "{admitted}");
    }

    // A fetch is refused when any answer is refused, or when there is none.
    let fetch = Err(Refusal::ClientInvalid(ClientInvalid::Fetch));
    assert_eq!(admit_fetch_answers(&[ip("8.8.8.8"), ip("10.0.0.1")]), fetch);
    assert_eq!(admit_fetch_answers(&[]), fetch);
    assert_eq!(
        admit_fetch_answers(&[ip("8.8.8.8"), ip("2606:4700:4700::1111")]),
        Ok(())
    );
}

// ---------------------------------------------------------------- d1, d2, d3

#[test]
fn the_protected_resource_document_names_exactly_issuer_slash_mcp() {
    let issuer = issuer();
    assert_eq!(issuer.as_str(), "https://oml.example.com");
    assert_eq!(issuer.resource(), "https://oml.example.com/mcp");
    assert_eq!(
        issuer.protected_resource_metadata_url(),
        "https://oml.example.com/.well-known/oauth-protected-resource/mcp"
    );
    assert_eq!(
        protected_resource_document(&issuer),
        json!({
            "resource": "https://oml.example.com/mcp",
            "authorization_servers": ["https://oml.example.com"],
            "scopes_supported": ["mcp"],
            "bearer_methods_supported": ["header"],
            "resource_name": "Open Memory Lab",
        })
    );

    let ported = Issuer::parse("https://oml.example.com:8443").expect("a public origin");
    assert_eq!(
        protected_resource_document(&ported)["resource"],
        "https://oml.example.com:8443/mcp"
    );
    assert_eq!(
        protected_resource_document(&ported)["authorization_servers"],
        json!(["https://oml.example.com:8443"])
    );
}

#[test]
fn the_authorization_server_document_advertises_s256_public_clients_and_iss() {
    assert_eq!(
        authorization_server_document(&issuer()),
        json!({
            "issuer": "https://oml.example.com",
            "authorization_endpoint": "https://oml.example.com/oauth/authorize",
            "token_endpoint": "https://oml.example.com/oauth/token",
            "registration_endpoint": "https://oml.example.com/oauth/register",
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
            "scopes_supported": ["mcp"],
            "client_id_metadata_document_supported": true,
            "authorization_response_iss_parameter_supported": true,
        })
    );
}

#[test]
fn the_challenge_adds_invalid_token_only_when_a_credential_failed() {
    let issuer = issuer();
    assert_eq!(
        mcp_challenge(Some(&issuer), ChallengeCause::Missing),
        r#"Bearer resource_metadata="https://oml.example.com/.well-known/oauth-protected-resource/mcp", scope="mcp""#
    );
    assert_eq!(
        mcp_challenge(Some(&issuer), ChallengeCause::CredentialFailed),
        r#"Bearer resource_metadata="https://oml.example.com/.well-known/oauth-protected-resource/mcp", scope="mcp", error="invalid_token""#
    );
    for cause in [ChallengeCause::Missing, ChallengeCause::CredentialFailed] {
        assert_eq!(mcp_challenge(None, cause), r#"Bearer realm="oml""#);
    }
}

#[test]
fn an_issuer_with_a_path_or_trailing_slash_is_refused_at_start() {
    assert_eq!(
        Issuer::parse("https://oml.example.com/"),
        Err(IssuerError::TrailingSlash)
    );
    assert_eq!(
        Issuer::parse("https://oml.example.com:8443/"),
        Err(IssuerError::TrailingSlash)
    );
    for path in [
        "https://oml.example.com/oml",
        "https://oml.example.com/mcp",
        "https://oml.example.com/a/",
        "https://oml.example.com?x=1",
        "https://oml.example.com#f",
    ] {
        assert_eq!(Issuer::parse(path), Err(IssuerError::Path), "{path}");
    }
    assert_eq!(
        Issuer::parse("https://oml.example.com:443"),
        Err(IssuerError::DefaultPort)
    );
    for port in [
        "https://oml.example.com:",
        "https://oml.example.com:0",
        "https://oml.example.com:08443",
        "https://oml.example.com:65536",
        "https://oml.example.com:8x",
    ] {
        assert_eq!(Issuer::parse(port), Err(IssuerError::Port), "{port}");
    }
    assert_eq!(
        Issuer::parse("http://oml.example.com"),
        Err(IssuerError::NotHttps)
    );
    for host in [
        "https://",
        "https://OML.example.com",
        "https://user@oml.example.com",
        "https://127.0.0.1",
        "https://oml.example.com.",
    ] {
        assert_eq!(Issuer::parse(host), Err(IssuerError::Host), "{host}");
    }
    assert!(Issuer::parse("https://oml.example.com:8443").is_ok());
}

#[test]
fn a_resource_parameter_other_than_issuer_slash_mcp_is_refused() {
    let issuer = issuer();
    assert_eq!(check_resource(None, &issuer), Ok(()));
    assert_eq!(
        check_resource(Some("https://oml.example.com/mcp"), &issuer),
        Ok(())
    );
    for other in [
        "https://oml.example.com/mcp/",
        "https://oml.example.com",
        "https://OML.example.com/mcp",
        "https://other.example.com/mcp",
    ] {
        assert_eq!(
            check_resource(Some(other), &issuer),
            Err(Refusal::ResourceMismatch),
            "{other}"
        );
    }
}

// ---------------------------------------------------------------- d10

#[test]
fn an_access_token_never_parses_as_a_key_and_a_key_never_as_a_token() {
    let random = [7_u8; 32];
    let access = Token::mint(TokenKind::Access, random);
    let refresh = Token::mint(TokenKind::Refresh, random);
    assert!(access.as_str().starts_with("oml_at_"));
    assert!(refresh.as_str().starts_with("oml_rt_"));
    assert_eq!(access.as_str().len(), 50);
    assert_eq!(access.kind(), TokenKind::Access);
    assert_eq!(token_kind(access.as_str()), Some(TokenKind::Access));
    assert_eq!(token_kind(refresh.as_str()), Some(TokenKind::Refresh));
    assert!(!is_key_value(access.as_str()));
    assert!(!is_key_value(refresh.as_str()));

    let key = format!("oml_{}", base62_43(&random));
    assert_eq!(key.len(), 47);
    assert!(is_key_value(&key));
    assert_eq!(token_kind(&key), None);
    // A key whose base62 part begins "at" or "rt" is still a key, never a token.
    for start in ["at", "rt"] {
        let lookalike = format!("oml_{start}{}", &base62_43(&random)[2..]);
        assert!(is_key_value(&lookalike));
        assert_eq!(token_kind(&lookalike), None);
    }

    // Expected values computed independently (Python).
    assert_eq!(base62_43(&[0; 32]), "0".repeat(43));
    assert_eq!(
        base62_43(&[0xff; 32]),
        "yhjskwdA6OZ1AL1YmHWZWm8LLG7HjnuCA2j5rOw8Xp1"
    );
    let ascending: [u8; 32] = std::array::from_fn(|i| u8::try_from(i).expect("small"));
    assert_eq!(
        base62_43(&ascending),
        "003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf"
    );

    // An access token, a key or any other string sent as a refresh token is
    // refused before any lookup.
    let not_refresh = Err(Refusal::RefreshInvalid(RefreshInvalid::NotARefreshToken));
    assert_eq!(refresh_token_digest(access.as_str()), not_refresh);
    assert_eq!(refresh_token_digest(&key), not_refresh);
    assert_eq!(refresh_token_digest("oml_rt_short"), not_refresh);
    assert_eq!(refresh_token_digest(refresh.as_str()), Ok(refresh.digest()));
    let expected: [u8; 32] = Sha256::digest(refresh.as_str().as_bytes()).into();
    assert_eq!(refresh.digest(), expected);
}

// ------------------------------------------------- d10, d11: refresh rotation

fn open_refresh(rotated: bool) -> RefreshRow<'static> {
    RefreshRow {
        rotated,
        expired: false,
        connection: ConnectionState::Open,
        client_id: CLIENT,
        resource: "https://oml.example.com/mcp",
        key: KeyState::Active,
    }
}

#[test]
fn presenting_a_rotated_refresh_token_revokes_the_connection() {
    let issuer = issuer();

    let first = open_refresh(false);
    let decision = decide_refresh(Some(&first), CLIENT, &issuer);
    assert_eq!(decision, RefreshDecision::Rotate);
    assert_eq!(decision.refusal(), None);
    assert_eq!(decision.act(), None);

    // The same token again, now rotated, is reuse.
    let reused = open_refresh(true);
    let decision = decide_refresh(Some(&reused), CLIENT, &issuer);
    assert_eq!(decision, RefreshDecision::RevokeForReuse);
    assert_eq!(
        decision.refusal(),
        Some(Refusal::RefreshInvalid(RefreshInvalid::Reused))
    );
    let act = decision.act().expect("a revocation");
    assert_eq!(act, ConnectionAct::RefreshReuse);
    let revoked = connection_transition(ConnectionState::Open, act).expect("open");
    assert_eq!(
        revoked,
        ConnectionState::Revoked(RevokedCause::RefreshReuse)
    );

    // The successor the first rotation minted is refused now, and nothing rotates.
    let successor = RefreshRow {
        connection: revoked,
        ..open_refresh(false)
    };
    assert_eq!(
        decide_refresh(Some(&successor), CLIENT, &issuer),
        RefreshDecision::Refuse(RefreshInvalid::ConnectionNotOpen)
    );
    // The rotated one once more revokes nothing a second time.
    let again = RefreshRow {
        connection: revoked,
        ..open_refresh(true)
    };
    assert_eq!(
        decide_refresh(Some(&again), CLIENT, &issuer),
        RefreshDecision::Refuse(RefreshInvalid::ConnectionNotOpen)
    );

    // Rotated and past its own expiry, while its row stands, is still reuse.
    let rotated_expired = RefreshRow {
        expired: true,
        ..open_refresh(true)
    };
    assert_eq!(
        decide_refresh(Some(&rotated_expired), CLIENT, &issuer),
        RefreshDecision::RevokeForReuse
    );
    // Expired and never rotated is refused, and revokes nothing.
    let lapsed = RefreshRow {
        expired: true,
        ..open_refresh(false)
    };
    assert_eq!(
        decide_refresh(Some(&lapsed), CLIENT, &issuer),
        RefreshDecision::Refuse(RefreshInvalid::Expired)
    );
}

#[test]
fn a_refresh_failing_a_check_before_rotation_rotates_and_revokes_nothing() {
    let issuer = issuer();
    assert_eq!(
        decide_refresh(None, CLIENT, &issuer),
        RefreshDecision::Refuse(RefreshInvalid::Unknown)
    );
    // A rotated token from another client is not reuse: the client check comes first.
    assert_eq!(
        decide_refresh(Some(&open_refresh(true)), "c_other", &issuer),
        RefreshDecision::Refuse(RefreshInvalid::ClientMismatch)
    );
    for key in [KeyState::Revoked, KeyState::Expired] {
        let row = RefreshRow {
            key,
            ..open_refresh(true)
        };
        assert_eq!(
            decide_refresh(Some(&row), CLIENT, &issuer),
            RefreshDecision::Refuse(RefreshInvalid::KeyEnded)
        );
    }
    let elsewhere = RefreshRow {
        resource: "https://old.example.com/mcp",
        ..open_refresh(false)
    };
    assert_eq!(
        decide_refresh(Some(&elsewhere), CLIENT, &issuer),
        RefreshDecision::Refuse(RefreshInvalid::ResourceNotCurrent)
    );
    for refused in [
        RefreshDecision::Refuse(RefreshInvalid::Unknown),
        RefreshDecision::Refuse(RefreshInvalid::Expired),
    ] {
        assert_eq!(refused.act(), None);
        assert!(matches!(
            refused.refusal(),
            Some(Refusal::RefreshInvalid(_))
        ));
    }
}

// --------------------------------------------- d10: authorization code redemption

fn authorized_row() -> ExchangeRow<'static> {
    ExchangeRow {
        state: ConnectionState::Authorized,
        client_id: CLIENT,
        code_challenge: CHALLENGE,
        redirect_uri: CLAUDE_CALLBACK,
        redirect_uri_sent: true,
        resource: "https://oml.example.com/mcp",
        key: KeyState::Active,
        code_redeemed: false,
        code_expired: false,
        code_voided: false,
    }
}

fn exchange_request() -> ExchangeRequest<'static> {
    ExchangeRequest {
        client_id: CLIENT,
        code_verifier: VERIFIER,
        redirect_uri: Some(CLAUDE_CALLBACK),
    }
}

#[test]
fn a_reused_authorization_code_revokes_only_for_the_holder_of_its_verifier() {
    let issuer = issuer();
    let row = authorized_row();
    let decision = decide_exchange(Some(&row), &exchange_request(), &issuer);
    assert_eq!(decision, ExchangeDecision::Redeem);
    assert_eq!(decision.refusal(), None);
    assert_eq!(decision.act(), Some(ConnectionAct::Exchange));
    let open = connection_transition(row.state, ConnectionAct::Exchange).expect("authorized");
    assert_eq!(open, ConnectionState::Open);

    let redeemed = ExchangeRow {
        state: open,
        code_redeemed: true,
        ..row
    };
    let decision = decide_exchange(Some(&redeemed), &exchange_request(), &issuer);
    assert_eq!(decision, ExchangeDecision::RevokeForReuse);
    assert_eq!(
        decision.refusal(),
        Some(Refusal::AuthorizationCodeInvalid(
            AuthorizationCodeInvalid::Reused
        ))
    );
    let act = decision.act().expect("a revocation");
    assert_eq!(act, ConnectionAct::CodeReuse);
    assert_eq!(
        connection_transition(open, act),
        Ok(ConnectionState::Revoked(RevokedCause::CodeReuse))
    );
    // Whatever the code's expiry.
    let late = ExchangeRow {
        code_expired: true,
        ..redeemed
    };
    assert_eq!(
        decide_exchange(Some(&late), &exchange_request(), &issuer),
        ExchangeDecision::RevokeForReuse
    );

    // A redeemed code read from a log, without its verifier or from another
    // client, revokes nothing.
    let without_verifier = ExchangeRequest {
        code_verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXl",
        ..exchange_request()
    };
    let other_client = ExchangeRequest {
        client_id: "c_other",
        ..exchange_request()
    };
    for request in [without_verifier, other_client] {
        assert_eq!(
            decide_exchange(Some(&redeemed), &request, &issuer),
            ExchangeDecision::Refuse(AuthorizationCodeInvalid::Redeemed)
        );
    }
    // A connection revoked already is revoked once.
    let revoked = ExchangeRow {
        state: ConnectionState::Revoked(RevokedCause::Admin),
        ..redeemed
    };
    assert_eq!(
        decide_exchange(Some(&revoked), &exchange_request(), &issuer),
        ExchangeDecision::Refuse(AuthorizationCodeInvalid::Redeemed)
    );
}

#[test]
fn a_failed_exchange_consumes_nothing_and_names_its_check() {
    let issuer = issuer();
    let row = authorized_row();
    let refused = |reason| ExchangeDecision::Refuse(reason);

    assert_eq!(
        decide_exchange(None, &exchange_request(), &issuer),
        refused(AuthorizationCodeInvalid::Unknown)
    );
    let cases = [
        (
            row,
            ExchangeRequest {
                client_id: "c_other",
                ..exchange_request()
            },
            AuthorizationCodeInvalid::ClientMismatch,
        ),
        (
            row,
            ExchangeRequest {
                code_verifier: CHALLENGE,
                ..exchange_request()
            },
            AuthorizationCodeInvalid::VerifierMismatch,
        ),
        (
            row,
            ExchangeRequest {
                redirect_uri: None,
                ..exchange_request()
            },
            AuthorizationCodeInvalid::RedirectMismatch,
        ),
        (
            row,
            ExchangeRequest {
                redirect_uri: Some("https://claude.ai/api/mcp/auth_callback/"),
                ..exchange_request()
            },
            AuthorizationCodeInvalid::RedirectMismatch,
        ),
        (
            ExchangeRow {
                code_voided: true,
                ..row
            },
            exchange_request(),
            AuthorizationCodeInvalid::CodeVoided,
        ),
        (
            ExchangeRow {
                key: KeyState::Revoked,
                ..row
            },
            exchange_request(),
            AuthorizationCodeInvalid::KeyEnded,
        ),
        (
            ExchangeRow {
                resource: "https://old.example.com/mcp",
                ..row
            },
            exchange_request(),
            AuthorizationCodeInvalid::ResourceNotCurrent,
        ),
        (
            ExchangeRow {
                code_expired: true,
                ..row
            },
            exchange_request(),
            AuthorizationCodeInvalid::Expired,
        ),
    ];
    for (row, request, reason) in cases {
        let decision = decide_exchange(Some(&row), &request, &issuer);
        assert_eq!(decision, refused(reason), "{reason:?}");
        assert_eq!(decision.act(), None);
    }

    // Omitted at authorize: the token request may omit it or must name the
    // client's one registered URI.
    let omitted = ExchangeRow {
        redirect_uri_sent: false,
        ..row
    };
    for presented in [None, Some(CLAUDE_CALLBACK)] {
        let request = ExchangeRequest {
            redirect_uri: presented,
            ..exchange_request()
        };
        assert_eq!(
            decide_exchange(Some(&omitted), &request, &issuer),
            ExchangeDecision::Redeem
        );
    }
    let request = ExchangeRequest {
        redirect_uri: Some("http://127.0.0.1/cb"),
        ..exchange_request()
    };
    assert_eq!(
        decide_exchange(Some(&omitted), &request, &issuer),
        refused(AuthorizationCodeInvalid::RedirectMismatch)
    );
}

// ---------------------------------------------------------------- lifecycle

#[test]
fn a_connection_opens_once_from_authorized_and_is_revoked_once() {
    use ConnectionAct::{AdminRevoke, CodeReuse, Exchange, RefreshReuse, Replace};
    use ConnectionState::{Authorized, Open, Revoked};

    assert_eq!(connection_transition(Authorized, Exchange), Ok(Open));
    for (act, cause) in [
        (AdminRevoke, RevokedCause::Admin),
        (RefreshReuse, RevokedCause::RefreshReuse),
        (CodeReuse, RevokedCause::CodeReuse),
        (Replace, RevokedCause::Replaced),
    ] {
        assert_eq!(connection_transition(Open, act), Ok(Revoked(cause)));
        // An authorized connection is not journalled and cannot be named.
        assert!(connection_transition(Authorized, act).is_err());
        // A revoked connection never reopens and is revoked once.
        for again in [AdminRevoke, RefreshReuse, CodeReuse, Replace, Exchange] {
            assert!(connection_transition(Revoked(cause), again).is_err());
        }
    }
    assert!(connection_transition(Open, Exchange).is_err());

    assert_eq!(
        [
            RevokedCause::Admin,
            RevokedCause::RefreshReuse,
            RevokedCause::CodeReuse,
            RevokedCause::Replaced,
        ]
        .map(RevokedCause::as_str),
        ["admin", "refresh_reuse", "code_reuse", "replaced"]
    );
    assert_eq!(
        [Authorized, Open, Revoked(RevokedCause::Admin)].map(ConnectionState::as_str),
        ["authorized", "open", "revoked"]
    );
}

// ---------------------------------------------------------------- d13

#[test]
fn every_refusal_names_one_code_of_the_closed_taxonomy() {
    assert_eq!(
        OAUTH_CODES,
        [
            "oauth.client_invalid",
            "oauth.redirect_refused",
            "oauth.pkce_required",
            "oauth.resource_mismatch",
            "oauth.authorization_expired",
            "oauth.connect_code_invalid",
            "oauth.authorization_code_invalid",
            "oauth.refresh_invalid",
            "oauth.origin_refused",
            "oauth.attach_refused",
            "oauth.connect_refused",
        ]
    );
    let one_of_each = [
        Refusal::ClientInvalid(ClientInvalid::Unknown),
        Refusal::RedirectRefused(RedirectRefused::OutsideAllowlist),
        Refusal::PkceRequired(PkceRequired::ChallengeAbsent),
        Refusal::ResourceMismatch,
        Refusal::AuthorizationExpired(AuthorizationExpired::AttemptsSpent),
        Refusal::ConnectCodeInvalid,
        Refusal::AuthorizationCodeInvalid(AuthorizationCodeInvalid::Unknown),
        Refusal::RefreshInvalid(RefreshInvalid::Unknown),
        Refusal::OriginRefused,
        Refusal::AttachRefused,
        Refusal::ConnectRefused(ConnectRefused::Role),
        Refusal::RequestValidation(Validation::StateTooLong),
    ];
    let codes = one_of_each.map(|r| r.code());
    assert_eq!(codes[..11], OAUTH_CODES);
    assert_eq!(codes[11], "request.validation");
    for refusal in one_of_each {
        assert!(
            refusal.to_string().starts_with(refusal.code()),
            "{refusal:?}"
        );
    }

    assert_eq!(
        ClientInvalid::ClientIdForm.details_reason(),
        Some("client_id_form")
    );
    assert_eq!(
        ClientInvalid::FetchBusy.details_reason(),
        Some("fetch_busy")
    );
    assert_eq!(ClientInvalid::NotJson.details_reason(), None);
    assert_eq!(
        [
            ConnectRefused::Role,
            ConnectRefused::HeldByService,
            ConnectRefused::BearerKey,
            ConnectRefused::Revoked,
            ConnectRefused::Expired,
            ConnectRefused::NotConfigured,
            ConnectRefused::ConnectorKey,
        ]
        .map(ConnectRefused::as_str),
        [
            "role",
            "held_by_service",
            "bearer_key",
            "revoked",
            "expired",
            "not_configured",
            "connector_key",
        ]
    );
}
