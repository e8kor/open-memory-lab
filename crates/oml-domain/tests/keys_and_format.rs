//! Rotation cross-signature rule, digest/timestamp spellings, and the
//! writer-side chain refusals.

use ed25519_dalek::SigningKey;
use oml_domain::{
    ChainError, EventDraft, EventTime, Sha256Hash, cross_sign_rotation, event_types,
    key_fingerprint, next_envelope, rotation_message, verify_rotation_cross_sig,
};
use serde_json::json;
use uuid::Uuid;

#[test]
fn rotation_message_is_the_ascii_rule() {
    // chain-format.md: cross_sig covers the ASCII bytes
    // "oml-signing-key-rotation:<new_key_id>:<new_public_key_hex>".
    let new_key = SigningKey::from_bytes(&[2_u8; 32]).verifying_key();
    let message = rotation_message(&new_key);
    assert!(message.is_ascii());
    assert_eq!(
        String::from_utf8(message).unwrap(),
        format!(
            "oml-signing-key-rotation:{}:{}",
            key_fingerprint(&new_key),
            hex::encode(new_key.as_bytes())
        )
    );
}

#[test]
fn cross_signature_round_trips_and_rejects_substitution() {
    let old = SigningKey::from_bytes(&[1_u8; 32]);
    let new = SigningKey::from_bytes(&[2_u8; 32]);
    let intruder = SigningKey::from_bytes(&[3_u8; 32]);

    let cross_sig = cross_sign_rotation(&old, &new.verifying_key());
    assert!(
        verify_rotation_cross_sig(&old.verifying_key(), &new.verifying_key(), &cross_sig).is_ok()
    );
    // A cross-sig for one successor does not attest another.
    assert!(
        verify_rotation_cross_sig(&old.verifying_key(), &intruder.verifying_key(), &cross_sig)
            .is_err()
    );
    // And only the old key's signature counts.
    let forged = cross_sign_rotation(&intruder, &new.verifying_key());
    assert!(
        verify_rotation_cross_sig(&old.verifying_key(), &new.verifying_key(), &forged).is_err()
    );
}

#[test]
fn digest_spelling_is_strict() {
    let good = "sha256:c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a";
    assert_eq!(Sha256Hash::from_prefixed(good).unwrap().to_prefixed(), good);
    for bad in [
        "c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a",
        "sha256:C3F4BFB6EF9E00DFD7AE8D8FCC5DDFC2BF69F40CEAA1DE4A165F77F8A4A3982A",
        "sha256:c3f4",
        "sha512:c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a",
    ] {
        assert!(Sha256Hash::from_prefixed(bad).is_err(), "{bad}");
    }
}

#[test]
fn at_is_fixed_width_six_fractional_digits_utc() {
    assert!(EventTime::parse("2026-10-29T14:03:17.000000Z").is_ok());
    for bad in [
        "2026-10-29T14:03:17Z",             // no fraction
        "2026-10-29T14:03:17.000Z",         // three digits
        "2026-10-29T14:03:17.0000000Z",     // seven digits
        "2026-10-29T14:03:17.000000+00:00", // not Z
        "2026-13-29T14:03:17.000000Z",      // month 13
        "2026-10-29t14:03:17.000000Z",      // lowercase t
    ] {
        assert!(EventTime::parse(bad).is_err(), "{bad}");
    }
}

fn draft(event_type: &str) -> EventDraft {
    EventDraft {
        tenant_id: Uuid::from_u128(0xAA),
        event_id: Uuid::from_u128(1),
        event_type: event_type.to_owned(),
        principal: "provider/operator-1".to_owned(),
        api_key_id: None,
        at: EventTime::parse("2026-10-29T14:03:17.000000Z").unwrap(),
        payload: json!({}),
    }
}

#[test]
fn the_first_event_must_be_tenant_genesis() {
    assert_eq!(
        next_envelope(None, draft(event_types::WORKAREA_CREATE)).unwrap_err(),
        ChainError::GenesisTypeRequired
    );
    let genesis = next_envelope(None, draft(event_types::TENANT_GENESIS)).unwrap();
    assert_eq!(genesis.seq, 1);
    assert_eq!(genesis.prev_hash, Sha256Hash::ZERO);
}

#[test]
fn a_payload_must_be_an_object() {
    let mut bad = draft(event_types::TENANT_GENESIS);
    bad.payload = json!(["not", "an", "object"]);
    assert_eq!(
        next_envelope(None, bad).unwrap_err(),
        ChainError::PayloadNotObject
    );
}
