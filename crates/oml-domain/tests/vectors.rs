//! The three chain-format.md test vectors, reproduced byte-for-byte.
//! Every literal below is copied from docs/data/chain-format.md ("Test
//! vectors"); if an assertion here fails, the canonicalization is wrong,
//! not the doc.

use ed25519_dalek::SigningKey;
use oml_domain::{
    ChainHead, EventDraft, KeySet, Sha256Hash, SignedEnvelope, event_types, key_fingerprint,
    next_envelope, sha256_digest_string, sign, verify_chain, verify_event,
};
use serde_json::json;
use uuid::Uuid;

/// The committed TEST KEY's seed: the last 32 bytes of the doc's PKCS#8
/// private key (chain-format.md "TEST KEY — never deployed").
const TEST_SEED_HEX: &str = "09874594afc93c44af9c486d6dd48b68514b5736a7d64cac72d9072f98a6371c";
const TEST_PUBLIC_HEX: &str = "ee8ca0e5bf0b97470c98ce8ee6faf5475f90b5ed69752a13c7dd9c66e4a29383";
const TEST_FINGERPRINT: &str = "d92de0b418b6e0b3b6b10cb2b2392586ce1e02f3c11237dbaf39a283882c1676";

const TENANT: &str = "00000000-0000-4000-8000-0000000000aa";

const V1_SIGNED_BYTES: &str = r#"{"api_key_id":"00000000-0000-4000-8000-0000000000c1","at":"2026-10-29T14:03:17.000000Z","event_id":"00000000-0000-4000-8000-0000000000e1","event_type":"tenant.genesis","payload":{"created_event_id":"00000000-0000-4000-8000-0000000000d2","name":"vector-tenant","provider_seq":2,"tenant_id":"00000000-0000-4000-8000-0000000000aa"},"prev_hash":"sha256:0000000000000000000000000000000000000000000000000000000000000000","principal":"provider/operator-1","seq":1,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}"#;
const V1_HASH: &str = "c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a";
const V1_SIG: &str = "3d22e7cf20b6db79eeb11479c505aeafab69209198fc2d8649c37032297150c56cef410f240fb97f3e603dc445fcd63da1abf36fe29f5633cc6247707b2f0c04";

const V2_SIGNED_BYTES: &str = r#"{"api_key_id":"00000000-0000-4000-8000-0000000000c2","at":"2026-10-29T14:05:00.250000Z","event_id":"00000000-0000-4000-8000-0000000000e2","event_type":"submission.remember","payload":{"artifact_id":"00000000-0000-4000-8000-0000000000a1","attachments":[],"body_bytes":53,"ciphertext_digest":"sha256:505007c2183a32f9a29b6635dc1a56d89711b658d0624eb662dc986141213688","claim_key":"deploy-runbook","derives_from":[],"kind":"memory","plaintext_digest":"sha256:6e04a033aab31ee48e5bf89a4a6ef03e658f25f9b428de3e45c2ac7519dd2532","provenance":"agent","scan":{"result":"pass","scanner_version":"vector-scanner/1"},"simulation_id":null,"version":1,"workarea_id":"00000000-0000-4000-8000-0000000000b1"},"prev_hash":"sha256:c3f4bfb6ef9e00dfd7ae8d8fcc5ddfc2bf69f40ceaa1de4a165f77f8a4a3982a","principal":"crew-redline/agent-3","seq":2,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}"#;
const V2_HASH: &str = "72f61452f67a618a46fd98948708e39d9bc8bf281ad86693ed4e78dd18a368c1";
const V2_SIG: &str = "068b0527c06897539882dfd22dc1970adbef83ca955559f80d529b76f7f26df64b14166b0a934eff9829704b3516f8a7b2c0bfa1a9a0ac529d1ec0e554756908";

const V3_SIGNED_BYTES: &str = r#"{"api_key_id":"00000000-0000-4000-8000-0000000000c3","at":"2026-10-29T16:00:00.000000Z","event_id":"00000000-0000-4000-8000-0000000000e3","event_type":"lifecycle.erase","payload":{"artifact_id":"00000000-0000-4000-8000-0000000000a1","copy_classes":["silver.body","object_store","fts","embeddings"],"dek_destroyed":true,"erased_by_key_id":"00000000-0000-4000-8000-0000000000c3"},"prev_hash":"sha256:72f61452f67a618a46fd98948708e39d9bc8bf281ad86693ed4e78dd18a368c1","principal":"acme/admin-1","seq":3,"tenant_id":"00000000-0000-4000-8000-0000000000aa"}"#;
const V3_HASH: &str = "e7837fc16aa96fbffe4cb6c0b2551522fa44d2c89006e7014bfc67d932512536";
const V3_SIG: &str = "2c38c30d096ee592ad0e26166665382a998775c7efb8c4165935c1c4db50cac03995324750bad240e0c0bb3b4177a132059ad57386d0514fb60f44271fafd804";

fn test_key() -> SigningKey {
    let seed: [u8; 32] = hex::decode(TEST_SEED_HEX).unwrap().try_into().unwrap();
    SigningKey::from_bytes(&seed)
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).unwrap()
}

fn time(s: &str) -> oml_domain::EventTime {
    oml_domain::EventTime::parse(s).unwrap()
}

fn build_chain() -> Vec<SignedEnvelope> {
    let key = test_key();
    let v1 = sign(
        next_envelope(
            None,
            EventDraft {
                tenant_id: uuid(TENANT),
                event_id: uuid("00000000-0000-4000-8000-0000000000e1"),
                event_type: event_types::TENANT_GENESIS.to_owned(),
                principal: "provider/operator-1".to_owned(),
                api_key_id: Some(uuid("00000000-0000-4000-8000-0000000000c1")),
                at: time("2026-10-29T14:03:17.000000Z"),
                payload: json!({
                    "created_event_id": "00000000-0000-4000-8000-0000000000d2",
                    "name": "vector-tenant",
                    "provider_seq": 2,
                    "tenant_id": TENANT,
                }),
            },
        )
        .unwrap(),
        &key,
    )
    .unwrap();

    let v2 = sign(
        next_envelope(
            Some(&v1),
            EventDraft {
                tenant_id: uuid(TENANT),
                event_id: uuid("00000000-0000-4000-8000-0000000000e2"),
                event_type: event_types::SUBMISSION_REMEMBER.to_owned(),
                principal: "crew-redline/agent-3".to_owned(),
                api_key_id: Some(uuid("00000000-0000-4000-8000-0000000000c2")),
                at: time("2026-10-29T14:05:00.250000Z"),
                payload: json!({
                    "artifact_id": "00000000-0000-4000-8000-0000000000a1",
                    "attachments": [],
                    "body_bytes": 53,
                    "ciphertext_digest": "sha256:505007c2183a32f9a29b6635dc1a56d89711b658d0624eb662dc986141213688",
                    "claim_key": "deploy-runbook",
                    "derives_from": [],
                    "kind": "memory",
                    "plaintext_digest": "sha256:6e04a033aab31ee48e5bf89a4a6ef03e658f25f9b428de3e45c2ac7519dd2532",
                    "provenance": "agent",
                    "scan": {"result": "pass", "scanner_version": "vector-scanner/1"},
                    "simulation_id": null,
                    "version": 1,
                    "workarea_id": "00000000-0000-4000-8000-0000000000b1",
                }),
            },
        )
        .unwrap(),
        &key,
    )
    .unwrap();

    let v3 = sign(
        next_envelope(
            Some(&v2),
            EventDraft {
                tenant_id: uuid(TENANT),
                event_id: uuid("00000000-0000-4000-8000-0000000000e3"),
                event_type: event_types::LIFECYCLE_ERASE.to_owned(),
                principal: "acme/admin-1".to_owned(),
                api_key_id: Some(uuid("00000000-0000-4000-8000-0000000000c3")),
                at: time("2026-10-29T16:00:00.000000Z"),
                payload: json!({
                    "artifact_id": "00000000-0000-4000-8000-0000000000a1",
                    "copy_classes": ["silver.body", "object_store", "fts", "embeddings"],
                    "dek_destroyed": true,
                    "erased_by_key_id": "00000000-0000-4000-8000-0000000000c3",
                }),
            },
        )
        .unwrap(),
        &key,
    )
    .unwrap();

    vec![v1, v2, v3]
}

#[test]
fn test_key_matches_the_doc() {
    let key = test_key();
    let public = key.verifying_key();
    assert_eq!(hex::encode(public.as_bytes()), TEST_PUBLIC_HEX);
    assert_eq!(key_fingerprint(&public), TEST_FINGERPRINT);
}

#[test]
fn vectors_reproduce_byte_for_byte() {
    let chain = build_chain();
    let expected = [
        (V1_SIGNED_BYTES, V1_HASH, V1_SIG),
        (V2_SIGNED_BYTES, V2_HASH, V2_SIG),
        (V3_SIGNED_BYTES, V3_HASH, V3_SIG),
    ];
    for (event, (signed_bytes, hash, sig)) in chain.iter().zip(expected) {
        assert_eq!(
            std::str::from_utf8(&event.signed_bytes).unwrap(),
            signed_bytes,
            "signed_bytes of seq {} deviate from the doc",
            event.envelope.seq
        );
        assert_eq!(hex::encode(event.hash.0), hash);
        assert_eq!(hex::encode(event.sig.to_bytes()), sig);
    }
    // The doc states vector 1's signed_bytes are 508 bytes, 2's 870, 3's 550.
    assert_eq!(chain[0].signed_bytes.len(), 508);
    assert_eq!(chain[1].signed_bytes.len(), 870);
    assert_eq!(chain[2].signed_bytes.len(), 550);
}

#[test]
fn vector_2_content_digests_are_reproducible() {
    // chain-format.md vector 2: body = 53 ASCII bytes + \n; the "ciphertext"
    // stand-in is SHA-256("vector-ciphertext:" + body bytes).
    let body = b"The deploy runbook: drain, upgrade, verify, undrain.\n";
    assert_eq!(
        sha256_digest_string(body),
        "sha256:6e04a033aab31ee48e5bf89a4a6ef03e658f25f9b428de3e45c2ac7519dd2532"
    );
    let mut stand_in = b"vector-ciphertext:".to_vec();
    stand_in.extend_from_slice(body);
    let ciphertext = Sha256Hash::digest(&stand_in);
    assert_eq!(
        sha256_digest_string(&ciphertext.0),
        "sha256:505007c2183a32f9a29b6635dc1a56d89711b658d0624eb662dc986141213688"
    );
}

#[test]
fn chain_verifies_against_the_test_key() {
    let chain = build_chain();
    let keys = KeySet::new(vec![test_key().verifying_key()]);
    let stated = ChainHead {
        tenant_id: uuid(TENANT),
        seq: 3,
        hash: Sha256Hash::from_prefixed(&format!("sha256:{V3_HASH}")).unwrap(),
    };
    let head = verify_chain(&chain, &keys, Some(&stated)).unwrap();
    assert_eq!(head, stated);
}

#[test]
fn any_single_changed_byte_fails_verification() {
    let keys = KeySet::new(vec![test_key().verifying_key()]);
    for i in 0..3 {
        let mut chain = build_chain();
        let mid = chain[i].signed_bytes.len() / 2;
        chain[i].signed_bytes[mid] ^= 0x01;
        assert!(
            verify_event(&chain[i], &keys).is_err(),
            "tampered event {i} still verifies"
        );
        assert!(verify_chain(&chain, &keys, None).is_err());
    }
}

#[test]
fn reordering_or_omission_fails() {
    let keys = KeySet::new(vec![test_key().verifying_key()]);
    let chain = build_chain();

    let mut reordered = chain.clone();
    reordered.swap(1, 2);
    assert!(verify_chain(&reordered, &keys, None).is_err());

    let omitted = vec![chain[0].clone(), chain[2].clone()];
    assert!(verify_chain(&omitted, &keys, None).is_err());
}

#[test]
fn a_wrong_key_set_fails_step_3() {
    let chain = build_chain();
    let other = SigningKey::from_bytes(&[7_u8; 32]);
    let keys = KeySet::new(vec![other.verifying_key()]);
    assert!(matches!(
        verify_chain(&chain, &keys, None),
        Err(oml_domain::VerifyError::BadSignature { seq: 1 })
    ));
}
