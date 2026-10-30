//! Chain construction and the nine-step verification procedure of
//! `docs/data/chain-format.md`.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::Value;
use uuid::Uuid;

use crate::envelope::{Envelope, EventTime, Sha256Hash, event_types};
use crate::jcs::{self, JcsError};
use crate::keys::key_fingerprint;

/// A journalled event: the envelope plus its stored `signed_bytes` and the
/// two derived fields. The stored bytes are authoritative — verification
/// runs over them, never over a re-serialization (chain-format.md).
#[derive(Debug, Clone)]
pub struct SignedEnvelope {
    pub envelope: Envelope,
    pub signed_bytes: Vec<u8>,
    pub hash: Sha256Hash,
    pub sig: Signature,
}

/// What a writer supplies for the next event; `seq` and `prev_hash` are the
/// chain's to assign ([`next_envelope`]).
#[derive(Debug, Clone)]
pub struct EventDraft {
    pub tenant_id: Uuid,
    pub event_id: Uuid,
    pub event_type: String,
    pub principal: String,
    pub api_key_id: Option<Uuid>,
    pub at: EventTime,
    pub payload: Value,
}

/// A refusal to construct or sign an envelope.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ChainError {
    #[error("payload must be a JSON object (chain-format.md signed fields)")]
    PayloadNotObject,
    #[error("draft tenant {draft} does not match the chain tenant {chain}")]
    TenantMismatch { draft: Uuid, chain: Uuid },
    #[error(
        "the first event of a chain must be tenant.genesis at seq 1 (chain-format.md genesis rule)"
    )]
    GenesisTypeRequired,
    #[error("tenant.genesis is only lawful at seq 1 (chain-format.md genesis rule)")]
    GenesisNotFirst,
    #[error("per-tenant seq exhausted")]
    SeqOverflow,
    #[error(transparent)]
    Jcs(#[from] JcsError),
}

/// Assigns `seq` and `prev_hash` for the event after `prev` (or the genesis
/// event when `prev` is `None`), enforcing the genesis rule.
pub fn next_envelope(
    prev: Option<&SignedEnvelope>,
    draft: EventDraft,
) -> Result<Envelope, ChainError> {
    if !draft.payload.is_object() {
        return Err(ChainError::PayloadNotObject);
    }
    let (seq, prev_hash) = match prev {
        None => {
            if draft.event_type != event_types::TENANT_GENESIS {
                return Err(ChainError::GenesisTypeRequired);
            }
            (1, Sha256Hash::ZERO)
        }
        Some(p) => {
            if p.envelope.tenant_id != draft.tenant_id {
                return Err(ChainError::TenantMismatch {
                    draft: draft.tenant_id,
                    chain: p.envelope.tenant_id,
                });
            }
            if draft.event_type == event_types::TENANT_GENESIS {
                return Err(ChainError::GenesisNotFirst);
            }
            let seq = p
                .envelope
                .seq
                .checked_add(1)
                .ok_or(ChainError::SeqOverflow)?;
            (seq, p.hash)
        }
    };
    Ok(Envelope {
        tenant_id: draft.tenant_id,
        seq,
        event_id: draft.event_id,
        event_type: draft.event_type,
        prev_hash,
        principal: draft.principal,
        api_key_id: draft.api_key_id,
        at: draft.at,
        payload: draft.payload,
    })
}

/// Computes `signed_bytes`, `hash` = SHA-256(signed_bytes), and
/// `sig` = pure Ed25519 (RFC 8032) over `signed_bytes` — over the full
/// canonical bytes, not the hash (chain-format.md, "hash and sig").
pub fn sign(envelope: Envelope, key: &SigningKey) -> Result<SignedEnvelope, ChainError> {
    let signed_bytes = envelope.signed_bytes()?;
    let hash = Sha256Hash::digest(&signed_bytes);
    let sig = key.sign(&signed_bytes);
    Ok(SignedEnvelope {
        envelope,
        signed_bytes,
        hash,
        sig,
    })
}

/// The verifier's authenticated key set. Per-event key attribution is by
/// trial verification: an event verifies iff some key in the set verifies it
/// (chain-format.md, "The verifier holds a key set, not a key").
#[derive(Debug, Clone)]
pub struct KeySet {
    keys: Vec<VerifyingKey>,
}

impl KeySet {
    #[must_use]
    pub fn new(keys: Vec<VerifyingKey>) -> Self {
        Self { keys }
    }

    /// Trial verification: the first key that verifies `sig` over `msg`.
    #[must_use]
    pub fn verify(&self, msg: &[u8], sig: &Signature) -> Option<&VerifyingKey> {
        self.keys.iter().find(|k| k.verify(msg, sig).is_ok())
    }

    /// Lowercase hex SHA-256 fingerprints of the member keys.
    #[must_use]
    pub fn fingerprints(&self) -> Vec<String> {
        self.keys.iter().map(key_fingerprint).collect()
    }
}

/// A chain head: (tenant, seq, hash) — what verification names on success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainHead {
    pub tenant_id: Uuid,
    pub seq: u64,
    pub hash: Sha256Hash,
}

/// A verification failure, named by the chain-format.md step that caught it.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    #[error("empty chain: nothing to verify")]
    Empty,
    #[error("events of more than one tenant in one chain (seq {seq})")]
    TenantMixed { seq: u64 },
    #[error("step 1: seq broken at position {index}: expected {expected}, found {found}")]
    SeqBroken {
        index: usize,
        expected: u64,
        found: u64,
    },
    #[error("step 2: event 1 is not tenant.genesis with an all-zero prev_hash")]
    BadGenesis,
    #[error("step 3: signature of seq {seq} verifies under no key in the set")]
    BadSignature { seq: u64 },
    #[error("step 4: SHA-256(signed_bytes) of seq {seq} does not equal the stored hash")]
    HashMismatch { seq: u64 },
    #[error("step 5: prev_hash of seq {seq} does not equal the previous event's recomputed hash")]
    PrevHashMismatch { seq: u64 },
    #[error("step 6: stored signed_bytes of seq {seq} are not RFC 8785 canonical")]
    NotCanonical { seq: u64 },
    #[error("step 7: parsed signed fields of seq {seq} do not match the served envelope")]
    FieldMismatch { seq: u64 },
    #[error("step 8: genesis reference check failed: {0}")]
    GenesisReference(String),
    #[error("step 9: the stated chain head does not match the verified chain")]
    HeadMismatch,
}

/// Per-event checks: steps 3, 4, 6 and 7 of the chain-format.md procedure.
pub fn verify_event(event: &SignedEnvelope, keys: &KeySet) -> Result<(), VerifyError> {
    let seq = event.envelope.seq;
    // Step 3: pure Ed25519 over the STORED signed_bytes, trial-verified.
    if keys.verify(&event.signed_bytes, &event.sig).is_none() {
        return Err(VerifyError::BadSignature { seq });
    }
    // Step 4: recompute the hash.
    if Sha256Hash::digest(&event.signed_bytes) != event.hash {
        return Err(VerifyError::HashMismatch { seq });
    }
    // Step 6: the stored bytes re-canonicalize to themselves.
    let parsed: Value = serde_json::from_slice(&event.signed_bytes)
        .map_err(|_| VerifyError::NotCanonical { seq })?;
    let recanonical =
        jcs::canonical_bytes(&parsed).map_err(|_| VerifyError::NotCanonical { seq })?;
    if recanonical != event.signed_bytes {
        return Err(VerifyError::NotCanonical { seq });
    }
    // Step 7: the nine parsed fields match the served envelope.
    if parsed != event.envelope.signed_value() {
        return Err(VerifyError::FieldMismatch { seq });
    }
    Ok(())
}

/// The chain-format.md verification procedure, steps 1–7 and 9, over one
/// tenant's events ordered by `seq` ascending. Step 8 (the genesis
/// provider-chain reference) needs the provider chain and is
/// [`verify_genesis_reference`], called by holders of that chain.
pub fn verify_chain(
    events: &[SignedEnvelope],
    keys: &KeySet,
    stated_head: Option<&ChainHead>,
) -> Result<ChainHead, VerifyError> {
    let first = events.first().ok_or(VerifyError::Empty)?;
    let tenant_id = first.envelope.tenant_id;

    // Step 1: seq starts at 1 and increments by exactly 1.
    for (index, event) in events.iter().enumerate() {
        let expected = index as u64 + 1;
        if event.envelope.seq != expected {
            return Err(VerifyError::SeqBroken {
                index,
                expected,
                found: event.envelope.seq,
            });
        }
        if event.envelope.tenant_id != tenant_id {
            return Err(VerifyError::TenantMixed {
                seq: event.envelope.seq,
            });
        }
    }

    // Step 2: genesis shape.
    if first.envelope.event_type != event_types::TENANT_GENESIS
        || first.envelope.prev_hash != Sha256Hash::ZERO
    {
        return Err(VerifyError::BadGenesis);
    }

    // Steps 3, 4, 6, 7 per event; step 5 against the recomputed hashes.
    let mut prev_computed: Option<Sha256Hash> = None;
    let mut last_computed = Sha256Hash::ZERO;
    for event in events {
        verify_event(event, keys)?;
        let computed = Sha256Hash::digest(&event.signed_bytes);
        if let Some(prev) = prev_computed
            && event.envelope.prev_hash != prev
        {
            return Err(VerifyError::PrevHashMismatch {
                seq: event.envelope.seq,
            });
        }
        prev_computed = Some(computed);
        last_computed = computed;
    }

    let head = ChainHead {
        tenant_id,
        seq: events.len() as u64,
        hash: last_computed,
    };

    // Step 9: the stated head equals the last event's (seq, hash); its
    // signature was verified in step 3.
    if let Some(stated) = stated_head
        && *stated != head
    {
        return Err(VerifyError::HeadMismatch);
    }
    Ok(head)
}

/// Step 8: a non-provider tenant's genesis payload names, by
/// `created_event_id`, a `tenant.created` event in the provider chain whose
/// payload `tenant_id` matches. The caller supplies that provider event with
/// its own verified linkage.
pub fn verify_genesis_reference(
    genesis: &Envelope,
    provider_created: &Envelope,
) -> Result<(), VerifyError> {
    let fail = |msg: &str| VerifyError::GenesisReference(msg.to_owned());
    if genesis.event_type != event_types::TENANT_GENESIS {
        return Err(fail("the referencing event is not tenant.genesis"));
    }
    if provider_created.event_type != event_types::TENANT_CREATED {
        return Err(fail("the referenced event is not tenant.created"));
    }
    let created_event_id = genesis
        .payload
        .get("created_event_id")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("genesis payload has no created_event_id string"))?;
    if created_event_id != provider_created.event_id.to_string() {
        return Err(fail(
            "created_event_id does not name the provider tenant.created event",
        ));
    }
    let created_tenant = provider_created
        .payload
        .get("tenant_id")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("tenant.created payload has no tenant_id string"))?;
    if created_tenant != genesis.tenant_id.to_string() {
        return Err(fail(
            "the provider tenant.created payload names a different tenant",
        ));
    }
    Ok(())
}
