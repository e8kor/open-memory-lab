//! The answers a caller may need to build itself, for a fake transport or a
//! test of its own, are built outside the crate (#281): a struct with a
//! private field takes no struct literal there, so each has a constructor.

use oml_sdk::{ConnectCode, KeyIssued};

/// A key issue and its connect code are built outside the crate as first
/// answers, never as replays, and read back what they were given.
#[test]
fn a_key_issue_and_a_connect_code_are_built_outside_the_crate_as_first_answers() {
    let code = ConnectCode::new(
        "c1".to_owned(),
        Some("omlc_x".to_owned()),
        "2026-11-12T10:00:00.000000Z".to_owned(),
        "https://oml.example.com/connect".to_owned(),
    );
    assert!(!code.replayed());
    assert_eq!(code.code.as_deref(), Some("omlc_x"));
    let issued = KeyIssued::new(
        "k1".to_owned(),
        "alice-laptop".to_owned(),
        None,
        Some(code.clone()),
    );
    assert!(!issued.replayed());
    assert_eq!(issued.key_id, "k1");
    assert_eq!(issued.name, "alice-laptop");
    assert_eq!(issued.connect_code, Some(code));
    assert_eq!(issued.value, None);
}
