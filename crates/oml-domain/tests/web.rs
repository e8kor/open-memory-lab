//! The web pages' sign-in core (ADR-0045 d2, d3, d4, d8), each test named
//! after the sentence it proves.

use oml_domain::name::Name;
use oml_domain::web::{
    Email, GitHubEmail, Provider, SIGN_IN_TTL_SECS, SignInRefusal, SignInState, admit_github,
    admit_google_id_token, base62, base64url_decode, capacity_credential, check_user, csrf_matches,
    csrf_token, hmac_sha256, session_key_names,
};
use serde_json::{Value, json};

const CLIENT: &str = "client-123.apps.googleusercontent.com";
const NONCE: &str = "n0nce";
const NOW: i64 = 1_800_000_000;

fn b64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

fn id_token(claims: &Value) -> String {
    format!(
        "{}.{}.{}",
        b64url(br#"{"alg":"RS256"}"#),
        b64url(claims.to_string().as_bytes()),
        b64url(b"signature-not-checked")
    )
}

fn claims() -> Value {
    json!({
        "iss": "https://accounts.google.com",
        "aud": CLIENT,
        "exp": NOW + 600,
        "nonce": NONCE,
        "sub": "1234567890",
        "email": "Alice@Example.com",
        "email_verified": true,
    })
}

#[test]
fn base64url_round_trips_every_length() {
    for len in 0..8 {
        let bytes: Vec<u8> = (0..len).map(|i| i * 37).collect();
        assert_eq!(base64url_decode(&b64url(&bytes)), Some(bytes));
    }
    assert_eq!(base64url_decode("a"), None);
    assert_eq!(base64url_decode("ab+c"), None);
}

#[test]
fn a_google_token_from_the_token_endpoint_signs_in_its_verified_email() {
    let identity = admit_google_id_token(&id_token(&claims()), CLIENT, NONCE, NOW)
        .expect("a token Google issued for this client");
    assert_eq!(identity.provider, Provider::Google);
    assert_eq!(identity.subject, "1234567890");
    assert_eq!(identity.email.as_str(), "alice@example.com");
}

#[test]
fn a_google_token_with_email_verified_false_is_refused() {
    let mut c = claims();
    c["email_verified"] = json!(false);
    assert_eq!(
        admit_google_id_token(&id_token(&c), CLIENT, NONCE, NOW),
        Err(SignInRefusal::EmailUnverified)
    );
    c["email_verified"] = Value::Null;
    assert_eq!(
        admit_google_id_token(&id_token(&c), CLIENT, NONCE, NOW),
        Err(SignInRefusal::EmailUnverified)
    );
}

#[test]
fn a_google_token_for_another_client_another_issuer_or_past_its_expiry_is_refused() {
    for (field, value) in [
        ("aud", json!("someone-else")),
        ("iss", json!("https://evil.example")),
        ("exp", json!(NOW)),
    ] {
        let mut c = claims();
        c[field] = value;
        assert_eq!(
            admit_google_id_token(&id_token(&c), CLIENT, NONCE, NOW),
            Err(SignInRefusal::ProviderRefused),
            "{field}"
        );
    }
    assert_eq!(
        admit_google_id_token("not.a.jwt.at-all", CLIENT, NONCE, NOW),
        Err(SignInRefusal::ProviderRefused)
    );
}

#[test]
fn a_google_token_whose_aud_array_names_any_audience_besides_the_client_is_refused() {
    for aud in [
        json!([CLIENT, "another-client.apps.googleusercontent.com"]),
        json!(["another-client.apps.googleusercontent.com", CLIENT]),
        json!([CLIENT, CLIENT, "x"]),
        json!(["someone-else"]),
        json!([CLIENT, 7]),
        json!([]),
    ] {
        let mut c = claims();
        c["aud"] = aud.clone();
        assert_eq!(
            admit_google_id_token(&id_token(&c), CLIENT, NONCE, NOW),
            Err(SignInRefusal::ProviderRefused),
            "{aud}"
        );
    }
}

#[test]
fn a_google_token_whose_aud_names_the_client_id_alone_as_a_string_or_an_array_signs_in() {
    for aud in [json!(CLIENT), json!([CLIENT]), json!([CLIENT, CLIENT])] {
        let mut c = claims();
        c["aud"] = aud.clone();
        let identity = admit_google_id_token(&id_token(&c), CLIENT, NONCE, NOW)
            .unwrap_or_else(|refusal| panic!("{aud}: {refusal:?}"));
        assert_eq!(identity.subject, "1234567890", "{aud}");
    }
}

#[test]
fn a_google_token_with_another_nonce_is_a_state_mismatch() {
    assert_eq!(
        admit_google_id_token(&id_token(&claims()), CLIENT, "other", NOW),
        Err(SignInRefusal::StateMismatch)
    );
}

#[test]
fn github_signs_in_only_the_primary_verified_email() {
    let emails = vec![
        GitHubEmail {
            email: "secondary@example.com".to_owned(),
            primary: false,
            verified: true,
        },
        GitHubEmail {
            email: "Primary@Example.com".to_owned(),
            primary: true,
            verified: true,
        },
    ];
    let identity = admit_github(42, &emails).expect("one primary verified address");
    assert_eq!(identity.email.as_str(), "primary@example.com");
    assert_eq!(identity.subject, "42");

    let unverified = vec![
        GitHubEmail {
            email: "primary@example.com".to_owned(),
            primary: true,
            verified: false,
        },
        GitHubEmail {
            email: "secondary@example.com".to_owned(),
            primary: false,
            verified: true,
        },
    ];
    assert_eq!(
        admit_github(42, &unverified),
        Err(SignInRefusal::EmailUnverified)
    );
}

#[test]
fn an_email_is_compared_whole_and_ascii_case_insensitively_with_no_folding() {
    let a = Email::parse("Alice.Smith+tag@Example.COM").expect("an address");
    assert_eq!(a.as_str(), "alice.smith+tag@example.com");
    assert_eq!(
        a.digest(),
        Email::parse("alice.smith+tag@example.com")
            .unwrap()
            .digest()
    );
    assert_ne!(
        a.digest(),
        Email::parse("alicesmith@example.com").unwrap().digest()
    );
    for bad in ["no-at", "@example.com", "a@", "a@b@c", "a b@c", ""] {
        assert!(Email::parse(bad).is_err(), "{bad}");
    }
    assert!(Email::parse(&format!("{}@x", "a".repeat(253))).is_err());
}

#[test]
fn a_user_is_at_most_56_characters_so_every_session_key_name_fits() {
    let longest = Name::parse(&"a".repeat(56)).expect("a name");
    assert!(check_user(&longest).is_ok());
    assert_eq!(session_key_names(&longest).len(), 4);
    let too_long = Name::parse(&"a".repeat(57)).expect("a name");
    assert!(check_user(&too_long).is_err());
    let names: Vec<String> = session_key_names(&Name::parse("alice").unwrap())
        .iter()
        .map(|n| n.as_str().to_owned())
        .collect();
    assert_eq!(
        names,
        ["alice-web", "alice-review", "alice-curate", "alice-admin"]
    );
}

#[test]
fn a_sign_in_admits_only_its_provider_its_state_and_its_ten_minutes() {
    let started = SignInState::start(Provider::Google, [[1; 32], [2; 32], [3; 32]], NOW);
    let opened = SignInState::decode(&started.encode()).expect("its own encoding");
    assert_eq!(opened, started);
    assert_eq!(started.verifier.len(), 64);
    assert!(started.nonce.is_some());
    assert!(opened.admits(Provider::Google, &started.state, NOW + 1));
    assert!(!opened.admits(Provider::GitHub, &started.state, NOW + 1));
    assert!(!opened.admits(Provider::Google, "another-state", NOW + 1));
    assert!(!opened.admits(Provider::Google, &started.state, NOW + SIGN_IN_TTL_SECS));
    assert!(!opened.admits(Provider::Google, &started.state, NOW - 1));
    let github = SignInState::start(Provider::GitHub, [[1; 32], [2; 32], [3; 32]], NOW);
    assert_eq!(github.nonce, None, "GitHub issues no ID token, so no nonce");
    assert_eq!(SignInState::decode(b"{}"), None);
}

#[test]
fn the_csrf_token_is_the_sessions_and_worthless_without_it() {
    let token = csrf_token("session-a");
    assert!(csrf_matches("session-a", &token));
    assert!(!csrf_matches("session-b", &token));
    assert!(!csrf_matches("session-a", ""));
    assert_ne!(token, csrf_token("session-b"));
}

/// HMAC-SHA256 is RFC 4231's: its test case 2, and a key longer than the
/// block, hashed first.
#[test]
fn hmac_sha256_answers_the_rfc_4231_vectors() {
    assert_eq!(
        hex::encode(hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(
        hex::encode(hmac_sha256(&[b'k'; 100], b"m")),
        "0482265a92ce176b0374f2192380b21e2d40126432c52f865cc0957053ae7a14"
    );
}

/// A capacity credential is `omls_` and the base62 of HMAC-SHA256 keyed by
/// the session token over the capacity's purpose, one per capacity, the
/// value computed here independently by Python's hmac (ADR-0070 d2).
#[test]
fn a_capacity_credential_is_the_base62_hmac_of_the_session_token_over_its_capacity() {
    assert_eq!(
        capacity_credential("tok", "web"),
        "omls_NWs52SyPhq5hZwG2dqOo1Q4llEtq86F47PMEKifoKpM"
    );
    assert_ne!(
        capacity_credential("tok", "web"),
        capacity_credential("tok", "admin")
    );
    assert_ne!(
        capacity_credential("tok", "web"),
        capacity_credential("tok2", "web")
    );
    assert_eq!(base62(&[0, 0]), "0");
    assert_eq!(base62(&[0, 61]), "z");
    assert_eq!(base62(&[0, 62]), "10");
}
