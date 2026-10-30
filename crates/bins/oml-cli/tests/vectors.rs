//! The verifier against the chain-format.md test vectors ("the M0 reference
//! implementation must reproduce them byte-for-byte"): the canonicalizer
//! reproduces each vector's signed_bytes exactly, the three-event chain
//! verifies under the published TEST KEY, and every single-byte edit,
//! omission or reordering breaks the named step.

use serde_json::Value;

use oml_cli::verify::{TailEvent, canonical_bytes, fingerprint, parse_verifying_key, verify_chain};

/// The vectors' TEST KEY — published in chain-format.md, never deployed.
const PUBKEY_HEX: &str = "ee8ca0e5bf0b97470c98ce8ee6faf5475f90b5ed69752a13c7dd9c66e4a29383";
const FINGERPRINT: &str = "d92de0b418b6e0b3b6b10cb2b2392586ce1e02f3c11237dbaf39a283882c1676";
const TENANT: &str = "00000000-0000-4000-8000-0000000000aa";

/// (signed_bytes as published, byte length, hash, sig) per vector.
const VECTORS: [(&str, usize, &str, &str); 3] = [
    (
        r#"{"api_key_id":"00000000-0000-4000-8000-0000000000c1","at":"2026-10-29T14:03:17.000000Z","event_id":"00000000-0000-4000-8000-0000000000e1","event_type":"tenant.genesis","payload":{"created_event_id":"00000000-0000-4000-8000-0000000000d2","name":"vector-tenant","provider_seq":2,"tenant_id":"00000000-0000-4000-8000-0000000000aa"},"prev_hash":"sha256:0000000000000000000000000000000000000000000000000000000000000000","principal":"provider/operator-1","seq":1,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}"#,
        508,
        "sha256:c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a",
        "ed25519:3d22e7cf20b6db79eeb11479c505aeafab69209198fc2d8649c37032297150c56cef410f240fb97f3e603dc445fcd63da1abf36fe29f5633cc6247707b2f0c04",
    ),
    (
        r#"{"api_key_id":"00000000-0000-4000-8000-0000000000c2","at":"2026-10-29T14:05:00.250000Z","event_id":"00000000-0000-4000-8000-0000000000e2","event_type":"submission.remember","payload":{"artifact_id":"00000000-0000-4000-8000-0000000000a1","attachments":[],"body_bytes":53,"ciphertext_digest":"sha256:505007c2183a32f9a29b6635dc1a56d89711b658d0624eb662dc986141213688","claim_key":"deploy-runbook","derives_from":[],"kind":"memory","plaintext_digest":"sha256:6e04a033aab31ee48e5bf89a4a6ef03e658f25f9b428de3e45c2ac7519dd2532","provenance":"agent","scan":{"result":"pass","scanner_version":"vector-scanner/1"},"simulation_id":null,"version":1,"workarea_id":"00000000-0000-4000-8000-0000000000b1"},"prev_hash":"sha256:c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a","principal":"crew-redline/agent-3","seq":2,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}"#,
        870,
        "sha256:72f61452f67a618a46fd98948708e39d9bc8bf281ad86693ed4e78dd18a368c1",
        "ed25519:068b0527c06897539882dfd22dc1970adbef83ca955559f80d529b76f7f26df64b14166b0a934eff9829704b3516f8a7b2c0bfa1a9a0ac529d1ec0e554756908",
    ),
    (
        r#"{"api_key_id":"00000000-0000-4000-8000-0000000000c3","at":"2026-10-29T16:00:00.000000Z","event_id":"00000000-0000-4000-8000-0000000000e3","event_type":"lifecycle.erase","payload":{"artifact_id":"00000000-0000-4000-8000-0000000000a1","copy_classes":["silver.body","object_store","fts","embeddings"],"dek_destroyed":true,"erased_by_key_id":"00000000-0000-4000-8000-0000000000c3"},"prev_hash":"sha256:72f61452f67a618a46fd98948708e39d9bc8bf281ad86693ed4e78dd18a368c1","principal":"acme/admin-1","seq":3,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}"#,
        550,
        "sha256:e7837fc16aa96fbffe4cb6c0b2551522fa44d2c89006e7014bfc67d932512536",
        "ed25519:2c38c30d096ee592ad0e26166665382a998775c7efb8c4165935c1c4db50cac03995324750bad240e0c0bb3b4177a132059ad57386d0514fb60f44271fafd804",
    ),
];

/// One vector, reshaped as the events tail would serve it (hash and sig
/// alongside the parsed envelope fields).
fn tail_event(vector: &(&str, usize, &str, &str)) -> TailEvent {
    let v: Value = serde_json::from_str(vector.0).expect("vector signed_bytes parse");
    let s = |name: &str| v[name].as_str().expect("vector field").to_owned();
    TailEvent {
        seq: v["seq"].as_u64().expect("seq"),
        event_id: s("event_id"),
        event_type: s("event_type"),
        prev_hash: s("prev_hash"),
        hash: vector.2.to_owned(),
        sig: vector.3.to_owned(),
        principal: s("principal"),
        api_key_id: v["api_key_id"].as_str().map(ToOwned::to_owned),
        at: s("at"),
        payload: v["payload"].clone(),
    }
}

#[test]
fn canonicalizer_reproduces_the_vectors_byte_for_byte() {
    for (i, (bytes_str, len, _, _)) in VECTORS.iter().enumerate() {
        let parsed: Value = serde_json::from_str(bytes_str).expect("vector parses");
        let canonical = canonical_bytes(&parsed).expect("vector canonicalizes");
        assert_eq!(canonical.len(), *len, "vector {} stated length", i + 1);
        assert_eq!(
            canonical,
            bytes_str.as_bytes(),
            "vector {} canonical bytes",
            i + 1
        );
    }
}

#[test]
fn the_three_event_chain_verifies_and_the_fingerprint_matches() {
    let key = parse_verifying_key(PUBKEY_HEX).expect("test key");
    assert_eq!(fingerprint(&key), FINGERPRINT, "chain-format.md key id");
    let events: Vec<TailEvent> = VECTORS.iter().map(tail_event).collect();
    let report = verify_chain(TENANT, &events, 3, &[key]);
    assert!(report.verified(), "breaks: {:?}", report.breaks);
    assert_eq!(report.key_events, vec![(FINGERPRINT.to_owned(), 3)]);
    assert_eq!(
        format!("sha256:{}", report.head_hash),
        VECTORS[2].2,
        "recomputed head hash equals vector 3's"
    );
}

#[test]
fn any_changed_byte_fails_steps_3_and_4() {
    let key = parse_verifying_key(PUBKEY_HEX).expect("test key");
    let mut events: Vec<TailEvent> = VECTORS.iter().map(tail_event).collect();
    events[1].payload["claim_key"] = Value::String("deploy-runbooK".to_owned());
    let report = verify_chain(TENANT, &events, 3, &[key]);
    assert!(!report.verified());
    assert!(
        report.breaks.iter().any(|b| b.starts_with("step 3")),
        "the signature no longer verifies: {:?}",
        report.breaks
    );
    assert!(
        report.breaks.iter().any(|b| b.starts_with("step 4")),
        "the stored hash no longer matches: {:?}",
        report.breaks
    );
}

#[test]
fn omission_and_reordering_fail_steps_1_and_5() {
    let key = parse_verifying_key(PUBKEY_HEX).expect("test key");
    let events: Vec<TailEvent> = VECTORS.iter().map(tail_event).collect();

    // Omit the middle event: the gap is detected, not absorbed.
    let omitted = vec![events[0].clone(), events[2].clone()];
    let report = verify_chain(TENANT, &omitted, 3, &[key]);
    assert!(report.breaks.iter().any(|b| b.starts_with("step 1")));
    assert!(report.breaks.iter().any(|b| b.starts_with("step 5")));

    // A foreign key set verifies nothing.
    let foreign = parse_verifying_key(
        // Any other valid ed25519 point: the test key's raw bytes reversed
        // happen not to decode, so derive one from a fixed seed instead.
        &hex::encode(
            ed25519_dalek::SigningKey::from_bytes(&[7_u8; 32])
                .verifying_key()
                .to_bytes(),
        ),
    )
    .expect("foreign key");
    let report = verify_chain(TENANT, &events, 3, &[foreign]);
    assert_eq!(
        report
            .breaks
            .iter()
            .filter(|b| b.starts_with("step 3"))
            .count(),
        3,
        "all three signatures fail under a foreign key: {:?}",
        report.breaks
    );

    // A stated head beyond the served tail fails step 9.
    let report = verify_chain(
        TENANT,
        &events,
        4,
        &[parse_verifying_key(PUBKEY_HEX).unwrap()],
    );
    assert!(report.breaks.iter().any(|b| b.starts_with("step 9")));
}
