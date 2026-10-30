//! Property tests: any event sequence built through `next_envelope` verifies
//! as a chain, and any single-byte tamper fails verification.

use ed25519_dalek::SigningKey;
use oml_domain::{
    EventDraft, EventTime, KeySet, SignedEnvelope, event_types, next_envelope, sign, verify_chain,
};
use proptest::prelude::*;
use serde_json::{Map, Value, json};
use uuid::Uuid;

fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn arb_string(min: usize, max: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(any::<char>(), min..max).prop_map(String::from_iter)
}

/// Payload values stay inside the chain's shapes: strings, sub-2^53
/// integers, booleans, null, and one nested array (chain-format.md).
fn arb_payload() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        Just(Value::Null),
        arb_string(0, 12).prop_map(Value::String),
        (-(9_007_199_254_740_991_i64)..=9_007_199_254_740_991_i64).prop_map(Value::from),
    ];
    proptest::collection::btree_map(arb_string(1, 8), leaf, 0..4).prop_map(|m| {
        let mut obj = Map::new();
        for (k, v) in m {
            obj.insert(k, v);
        }
        obj.insert("extra".to_owned(), json!(["a", 1, null]));
        Value::Object(obj)
    })
}

#[derive(Debug, Clone)]
struct DraftSpec {
    event_type: &'static str,
    principal: String,
    with_key: bool,
    payload: Value,
}

fn arb_draft() -> impl Strategy<Value = DraftSpec> {
    (
        proptest::sample::select(vec![
            event_types::WORKAREA_CREATE,
            event_types::KEY_ISSUED,
            event_types::SUBMISSION_REMEMBER,
            event_types::REVIEW_APPROVE,
            event_types::REVIEW_REJECT,
            event_types::REVIEW_RATIFY,
            event_types::LIFECYCLE_PROMOTE,
            event_types::LIFECYCLE_SUPERSEDE,
        ]),
        "[a-z]{1,8}/[a-z0-9-]{1,8}",
        any::<bool>(),
        arb_payload(),
    )
        .prop_map(|(event_type, principal, with_key, payload)| DraftSpec {
            event_type,
            principal,
            with_key,
            payload,
        })
}

fn build_chain(tenant: Uuid, specs: &[DraftSpec], key: &SigningKey) -> Vec<SignedEnvelope> {
    let at = EventTime::parse("2026-10-29T14:03:17.000000Z").unwrap();
    let mut chain: Vec<SignedEnvelope> = Vec::with_capacity(specs.len() + 1);
    let genesis = next_envelope(
        None,
        EventDraft {
            tenant_id: tenant,
            event_id: Uuid::from_u128(1),
            event_type: event_types::TENANT_GENESIS.to_owned(),
            principal: "provider/operator-1".to_owned(),
            api_key_id: None,
            at: at.clone(),
            payload: json!({"created_event_id": null, "name": "prop", "provider_seq": null}),
        },
    )
    .unwrap();
    chain.push(sign(genesis, key).unwrap());
    for (i, spec) in specs.iter().enumerate() {
        let envelope = next_envelope(
            Some(chain.last().unwrap()),
            EventDraft {
                tenant_id: tenant,
                event_id: Uuid::from_u128(i as u128 + 2),
                event_type: spec.event_type.to_owned(),
                principal: spec.principal.clone(),
                api_key_id: spec.with_key.then(|| Uuid::from_u128(0xC0 + i as u128)),
                at: at.clone(),
                payload: spec.payload.clone(),
            },
        )
        .unwrap();
        chain.push(sign(envelope, key).unwrap());
    }
    chain
}

proptest! {
    #[test]
    fn any_sequence_through_next_envelope_verifies(
        specs in proptest::collection::vec(arb_draft(), 0..6),
        seed in 1_u8..=255,
    ) {
        let key = signing_key(seed);
        let chain = build_chain(Uuid::from_u128(0xAA), &specs, &key);
        let keys = KeySet::new(vec![key.verifying_key()]);
        let head = verify_chain(&chain, &keys, None).unwrap();
        prop_assert_eq!(head.seq, chain.len() as u64);
        prop_assert_eq!(head.hash, chain.last().unwrap().hash);
    }

    #[test]
    fn any_single_byte_tamper_fails(
        specs in proptest::collection::vec(arb_draft(), 0..5),
        seed in 1_u8..=255,
        pick in any::<prop::sample::Index>(),
        byte_pick in any::<prop::sample::Index>(),
        bit in 0_u8..8,
    ) {
        let key = signing_key(seed);
        let mut chain = build_chain(Uuid::from_u128(0xAA), &specs, &key);
        let event_index = pick.index(chain.len());
        let byte_index = byte_pick.index(chain[event_index].signed_bytes.len());
        chain[event_index].signed_bytes[byte_index] ^= 1 << bit;
        let keys = KeySet::new(vec![key.verifying_key()]);
        prop_assert!(verify_chain(&chain, &keys, None).is_err());
    }
}
