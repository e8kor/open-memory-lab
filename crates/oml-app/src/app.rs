//! The use-cases: every verb the transports will front, enforcing the
//! constitution here — verb ceilings (R1), reviewer-never-author (R2),
//! promoted-only reads (R3) — never in an adapter (ADR-0019 d1). Every
//! workarea-scoped verb resolves effective access from the key's grants and
//! denials on each request, in the one refusal order of roles-and-keys.md
//! (ADR-0025 d1).

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::access::{
    AccessRefusal, Grant, IssuanceRefusal, KeyDenial, TailCaller, TailSubject,
    admissible_grant_roles, check_issuance, effective_roles, effective_verbs, resolve,
    tail_visible,
};
use oml_domain::budget::{budget_tokens, cut, hit_order};
use oml_domain::name::{Name, UNIVERSE};
use oml_domain::{
    ChainHead, Envelope, EventDraft, HeldBy, KeySet, MemoryKind, MemoryStatus, Role, Sha256Hash,
    SignedEnvelope, canonical_bytes, check_reviewer, event_types, next_envelope, nfc,
    sha256_digest_string, transition,
};

use crate::cursor::ListingCursor;
use crate::ports::{
    AdmissionEnvelope, Clock, CommitReceipt, CommitRequest, IdGen, IdempotencyPut,
    IdempotencyRecord, IdempotencyScope, JournalStore, MetadataFold, OutboxRow, ScanOutcome,
    Scanner, Signer, StoreError, SubjectRow, TailEvent,
};
use crate::types::{
    AppError, BootstrapRequest, BootstrapResult, CreatedWorkarea, Crew, DenialRecord, GrantRecord,
    HIT_KINDS, IssueKeyRequest, IssuedKey, KeyAccess, KeyRecord, ListRequest, ListingFilter,
    ListingPosition, ListingScope, MemoryPage, MemoryRecord, PROVIDER_TENANT, RecallHit,
    RecallRequest, RecallResult, ServedMemory, SubmitOutcome, SubmitReceipt, SubmitRequest,
    SupersedeOutcome, SupersedeReceipt, SupersedeRequest, TailPage, TailedEvent, Verdict,
    VerdictOutcome, VersionRef, WorkareaInfo, WorkareaRecord,
};

const EVENTS_PAGE: usize = 512;

/// The fixed reason a failed issuance's `key.revoked` digests
/// (roles-and-keys.md issuance order; ADR-0025 d2).
const ISSUANCE_INCOMPLETE: &str = "issuance_incomplete";

/// The application, generic over its driven ports.
pub struct App<J, S, C, I, Sc> {
    journal: J,
    signer: S,
    clock: C,
    ids: I,
    scanner: Sc,
}

/// The authenticated key and what its effective access is computed from,
/// read once per request and never cached (workareas.md effective access;
/// ADR-0025 d1, d14).
struct Caller {
    key: KeyRecord,
    grants: Vec<Grant>,
    denials: Vec<KeyDenial>,
    /// The epoch of the access read above (workareas.md permission epoch).
    permission_epoch: u64,
}

impl Caller {
    /// `effective_roles` at the node whose ancestry, node to root, this is.
    fn effective(&self, ancestry: &[Uuid]) -> BTreeSet<Role> {
        effective_roles(&self.grants, &self.denials, ancestry)
    }

    /// Whether a global per-key denial covers the node: deny-wins, own rows
    /// included (roles-and-keys.md#own).
    fn denied(&self, ancestry: &[Uuid]) -> bool {
        self.denials.iter().any(|d| ancestry.contains(&d.node))
    }
}

/// An admin verb: checked on the key's own role, never on grants
/// (roles-and-keys.md permission matrix; http-api.md authorization).
fn require_role(key: &KeyRecord, role: Role, verb: &'static str) -> Result<(), AppError> {
    if key.role == role {
        Ok(())
    } else {
        Err(AppError::VerbCeiling {
            role: key.role,
            verb,
        })
    }
}

/// Step 1 of the refusal order: the key's role must admit the grant role the
/// verb needs. It reads no node, so it runs before the workarea resolves
/// (roles-and-keys.md grant admissibility and refusal order; ADR-0025 d1).
fn require_admissible(key: &KeyRecord, needed: Role, verb: &'static str) -> Result<(), AppError> {
    if admissible_grant_roles(key.role).contains(&needed) {
        Ok(())
    } else {
        Err(AppError::VerbCeiling {
            role: key.role,
            verb,
        })
    }
}

/// A `held_by: service` key is refused every review-class verb before any
/// lookup: its dreamer grant confers them like any other, and this rule, not
/// the grant, keeps it out of review (roles-and-keys.md; ADR-0025 d1).
fn refuse_service(key: &KeyRecord, verb: &'static str) -> Result<(), AppError> {
    if key.held_by == HeldBy::Service {
        Err(AppError::VerbCeiling {
            role: key.role,
            verb,
        })
    } else {
        Ok(())
    }
}

/// A refusal-order answer as the typed refusal; `node` is the workarea
/// `access.grant_refused` names.
fn access_refusal(refusal: AccessRefusal, node: Uuid, verb: &'static str) -> AppError {
    match refusal {
        AccessRefusal::VerbCeiling { role, .. } => AppError::VerbCeiling { role, verb },
        AccessRefusal::GrantRefused { needed_role } => AppError::GrantRefused {
            workarea_id: node,
            needed_role,
        },
        AccessRefusal::NotFound => AppError::NotFound,
    }
}

/// Issuance steps 1–3 as the typed refusal.
fn issuance_refusal(refusal: IssuanceRefusal) -> AppError {
    match refusal {
        IssuanceRefusal::RoleExclusive => AppError::RoleExclusive,
        IssuanceRefusal::IssuerCeiling {
            role,
            admissible,
            grant_index,
        } => AppError::IssuerCeiling {
            role,
            admissible,
            grant_index,
        },
    }
}

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// 256 bits as exactly 43 base62 characters, zero-padded (roles-and-keys.md
/// key format: `oml_` + 43 × `[A-Za-z0-9]`).
fn base62_43(bytes: &[u8; 32]) -> String {
    let mut num = bytes.to_vec();
    let mut digits: Vec<u8> = Vec::with_capacity(43);
    while num.iter().any(|&b| b != 0) {
        let mut rem: u32 = 0;
        for b in &mut num {
            let acc = (rem << 8) | u32::from(*b);
            *b = u8::try_from(acc / 62).unwrap_or(u8::MAX);
            rem = acc % 62;
        }
        digits.push(BASE62[rem as usize]);
    }
    while digits.len() < 43 {
        digits.push(b'0');
    }
    digits.reverse();
    String::from_utf8(digits).expect("base62 alphabet is ASCII")
}

/// A node's derived path from its ancestry, listed node to root: the names
/// from `universe` down to the node, joined with `.` (ADR-0025 d14). Every
/// path the app serves is this derivation.
fn derived_path(ancestry: &[WorkareaRecord]) -> String {
    ancestry
        .iter()
        .rev()
        .map(|w| w.name.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn submit_request_digest(req: &SubmitRequest, workarea_id: Uuid) -> String {
    let doc = json!({
        "body": req.body,
        "claim_key": req.claim_key,
        "kind": req.kind.as_str(),
        "derives_from": req.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "workarea_id": workarea_id.to_string(),
    });
    sha256_digest_string(doc.to_string().as_bytes())
}

/// The request digest of a supersession: SHA-256 over the RFC 8785 JSON of
/// the method, the route template, the path parameters and the parsed body,
/// so one Idempotency-Key reused for two targets never replays the first
/// (http-api.md idempotency; ADR-0025 d10).
fn supersede_request_digest(req: &SupersedeRequest) -> Result<String, AppError> {
    let doc = json!({
        "method": "POST",
        "route": "/v1/memories/{artifact_id}/supersede",
        "path_params": { "artifact_id": req.target_id.to_string() },
        "body": {
            "body": req.body,
            "kind": req.kind.map(MemoryKind::as_str),
            "retirement": req.retirement,
            "derives_from": req.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "reason": req.reason,
        },
    });
    Ok(sha256_digest_string(&canonical_bytes(&doc)?))
}

/// A recall's `query_digest`: SHA-256 over the RFC 8785 JSON of an object
/// whose members are always present, defaults applied — `kinds` sorted by
/// code point with duplicates removed, `max_tokens` null when absent, and
/// `scope` the resolved node. `terms` and `expand_terms` digest as absent:
/// the vocabulary filter is unbuilt, and a request that sends either is
/// refused before it reaches a recall (retrieval.md, the read log; ADR-0025
/// d9, d16).
fn recall_query_digest(query: &str, req: &RecallRequest, scope: Uuid) -> Result<String, AppError> {
    let mut kinds: Vec<&str> = req.kinds.iter().map(|kind| kind.as_str()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    let doc = json!({
        "query": query,
        "k": req.k,
        "kinds": kinds,
        "include_bodies": req.include_bodies,
        "include_own_held": req.include_own_held,
        "max_tokens": req.max_tokens,
        "terms": [],
        "expand_terms": "none",
        "scope": scope.to_string(),
    });
    Ok(sha256_digest_string(&canonical_bytes(&doc)?))
}

/// The successor's kind: an omitted one is the target's; a `memory`,
/// `derived_fact` or `steering` target admits any of those three, a
/// vocabulary-kind target keeps its kind; a retirement keeps the target's
/// kind and never retires a term, which deprecates instead
/// (audit-and-lineage.md supersession chains; ADR-0025 d11).
fn successor_kind(
    target: MemoryKind,
    sent: Option<MemoryKind>,
    retirement: bool,
) -> Result<MemoryKind, AppError> {
    const CLASS: [MemoryKind; 3] = [
        MemoryKind::Memory,
        MemoryKind::DerivedFact,
        MemoryKind::Steering,
    ];
    if retirement && target == MemoryKind::Term {
        return Err(AppError::TermRetirement);
    }
    let Some(sent) = sent else {
        return Ok(target);
    };
    let admitted = if retirement || !CLASS.contains(&target) {
        sent == target
    } else {
        CLASS.contains(&sent)
    };
    if admitted {
        Ok(sent)
    } else {
        Err(AppError::KindChangeRefused { target, sent })
    }
}

/// One event's commit request, with its outbox row.
fn commit_request(
    event: SignedEnvelope,
    fold: MetadataFold,
    idempotency: Option<IdempotencyPut>,
) -> CommitRequest {
    let outbox = OutboxRow {
        event_id: event.envelope.event_id,
        topic: event.envelope.event_type.clone(),
        // Digest-only, like the journal: no body plaintext rides the
        // outbox either (storage.md bronze mirror).
        payload: json!({
            "tenant_id": event.envelope.tenant_id.to_string(),
            "seq": event.envelope.seq,
            "event_type": event.envelope.event_type,
        }),
    };
    CommitRequest {
        event,
        fold,
        outbox,
        idempotency,
    }
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    pub fn new(journal: J, signer: S, clock: C, ids: I, scanner: Sc) -> Self {
        Self {
            journal,
            signer,
            clock,
            ids,
            scanner,
        }
    }

    // -- internals ----------------------------------------------------------

    /// The key the value names and the grants and denials of its effective
    /// access: one hash lookup and one access read per request (workareas.md
    /// effective access; ADR-0025 d1).
    async fn authenticate(&self, api_key: &str) -> Result<Caller, AppError> {
        let hash = sha256_digest_string(api_key.as_bytes());
        let key = self
            .journal
            .key_by_hash(&hash)
            .await?
            .ok_or(AppError::Unauthorized)?;
        // A provider-level key carries no grant and no denial, since no
        // tenant's row can name it (postgres-schema.md): it holds no access,
        // so every verb refuses it by its role (ADR-0025 d13).
        if key.tenant_id == PROVIDER_TENANT {
            return Ok(Caller {
                key,
                grants: Vec::new(),
                denials: Vec::new(),
                permission_epoch: 0,
            });
        }
        // The access read answers nothing only for a key its own tenant
        // does not hold: fail closed.
        let access = self
            .journal
            .key_access(key.tenant_id, key.key_id)
            .await?
            .ok_or(AppError::Unauthorized)?;
        Ok(Caller {
            key,
            permission_epoch: access.permission_epoch(),
            grants: access.grants.into_iter().map(|g| g.grant).collect(),
            denials: access.denials.into_iter().map(|d| d.denial).collect(),
        })
    }

    /// The node's ancestry as ids, node to root; empty for a node the tenant
    /// does not hold, which confers nothing.
    async fn ancestry(&self, tenant_id: Uuid, node: Uuid) -> Result<Vec<Uuid>, AppError> {
        Ok(self
            .journal
            .workarea_ancestry(tenant_id, node)
            .await?
            .map(|ancestry| ancestry.into_iter().map(|w| w.workarea_id).collect())
            .unwrap_or_default())
    }

    /// The refusal order for a verb needing grant role `needed` on `node`
    /// (roles-and-keys.md; ADR-0025 d1): verb ceiling, allowed, grant
    /// refused, not found.
    async fn require_access(
        &self,
        caller: &Caller,
        node: Uuid,
        needed: Role,
        verb: &'static str,
    ) -> Result<(), AppError> {
        let ancestry = self.ancestry(caller.key.tenant_id, node).await?;
        resolve(caller.key.role, &caller.effective(&ancestry), needed)
            .map_err(|refusal| access_refusal(refusal, node, verb))
    }

    /// Whether the caller can read `memory`: its own row in any state,
    /// unless a global denial masks the row's workarea, or a promoted version
    /// within its reader effective access (roles-and-keys.md#own; ADR-0025
    /// d3). Own is the authoring key, never the principal.
    async fn readable(&self, caller: &Caller, memory: &MemoryRecord) -> Result<bool, AppError> {
        let ancestry = self
            .ancestry(caller.key.tenant_id, memory.workarea_id)
            .await?;
        if memory.author_key_id == caller.key.key_id {
            return Ok(!caller.denied(&ancestry));
        }
        Ok(memory.status == MemoryStatus::Promoted
            && caller.effective(&ancestry).contains(&Role::Reader))
    }

    /// The served tip of `claim_key` in the workarea as a refusal may name it:
    /// only when the caller can read it (errors.md `submission.tip_conflict`
    /// details; ADR-0025 d10, d16).
    async fn readable_tip(
        &self,
        caller: &Caller,
        workarea_id: Uuid,
        claim_key: &str,
    ) -> Result<Option<VersionRef>, AppError> {
        match self
            .journal
            .claim_tip(caller.key.tenant_id, workarea_id, claim_key)
            .await?
        {
            Some(tip) if self.readable(caller, &tip).await? => Ok(Some(VersionRef {
                artifact_id: tip.memory_id,
                version: tip.version,
            })),
            _ => Ok(None),
        }
    }

    /// `memory` as served to the caller: `derives_from` keeps only the
    /// ancestors `readable` accepts, so a promoted row derived from its
    /// author's held row names that row to no other key, and `replaces`
    /// obeys the same test, so one item never names in `replaces` what its
    /// `derives_from` hides (http-api.md get; ADR-0025 d3). The path is the
    /// ancestry's names from the root, joined with `.` (ADR-0025 d14).
    ///
    /// Not set-wise yet: each ancestor, the replaced version and the path
    /// are read for this one row, so recall, get and every listing issue
    /// those reads once per served row, against ADR-0024 d4 ("never N+1").
    /// Only the rows themselves hydrate set-wise (`JournalStore::memories`).
    /// The cost is unmeasured (FR-7, FR-25).
    async fn served(
        &self,
        caller: &Caller,
        mut memory: MemoryRecord,
    ) -> Result<ServedMemory, AppError> {
        let tenant_id = caller.key.tenant_id;
        let mut derives_from = Vec::with_capacity(memory.derives_from.len());
        for ancestor in memory.derives_from {
            if let Some(m) = self.journal.memory(tenant_id, ancestor).await?
                && self.readable(caller, &m).await?
            {
                derives_from.push(ancestor);
            }
        }
        memory.derives_from = derives_from;
        let mut replaces = None;
        if let Some(id) = memory.replaces
            && let Some(m) = self.journal.memory(tenant_id, id).await?
            && self.readable(caller, &m).await?
        {
            replaces = Some(VersionRef {
                artifact_id: id,
                version: m.version,
            });
        }
        let workarea_path = self
            .journal
            .workarea_ancestry(tenant_id, memory.workarea_id)
            .await?
            .map(|ancestry| derived_path(&ancestry));
        Ok(ServedMemory {
            memory,
            replaces,
            workarea_path,
        })
    }

    /// The nodes of the subtree at `root` where `role` is effective for the
    /// caller, parent before child: subtree(root) ∩ effective access, each
    /// node judged by the whole formula (ADR-0025 d1).
    async fn nodes_with(
        &self,
        caller: &Caller,
        root: Uuid,
        role: Role,
    ) -> Result<Vec<Uuid>, AppError> {
        let tenant_id = caller.key.tenant_id;
        let above = self.ancestry(tenant_id, root).await?;
        if above.is_empty() {
            return Ok(Vec::new());
        }
        let mut ancestries: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::from([(root, above)]);
        let mut nodes = Vec::new();
        // The subtree lists a parent before its children, so each node's
        // ancestry is its parent's with the node in front.
        for node in self.journal.workarea_subtree(tenant_id, root).await? {
            let id = node.workarea_id;
            if id != root {
                let parent = node
                    .parent_id
                    .and_then(|p| ancestries.get(&p))
                    .ok_or_else(|| {
                        StoreError::Backend(format!(
                            "the subtree of {root} lists {id} before its parent"
                        ))
                    })?;
                let ancestry = std::iter::once(id).chain(parent.iter().copied()).collect();
                ancestries.insert(id, ancestry);
            }
            if caller.effective(&ancestries[&id]).contains(&role) {
                nodes.push(id);
            }
        }
        Ok(nodes)
    }

    /// Every node where dreamer is effective for the caller: the union over
    /// its dreamer grants' subtrees, a row outside which is never served
    /// (ADR-0025 d1).
    async fn dreamer_nodes(&self, caller: &Caller) -> Result<Vec<Uuid>, AppError> {
        let roots: BTreeSet<Uuid> = caller
            .grants
            .iter()
            .filter(|g| g.role == Role::Dreamer)
            .map(|g| g.node)
            .collect();
        let mut nodes = BTreeSet::new();
        for root in roots {
            nodes.extend(self.nodes_with(caller, root, Role::Dreamer).await?);
        }
        Ok(nodes.into_iter().collect())
    }

    async fn head_event(&self, tenant_id: Uuid) -> Result<Option<SignedEnvelope>, AppError> {
        match self.journal.chain_head(tenant_id).await? {
            None => Ok(None),
            Some(head) => Ok(self
                .journal
                .events_range(tenant_id, head.seq, 1)
                .await?
                .into_iter()
                .next()),
        }
    }

    /// `next_envelope` + signing through the signer port. The envelope is
    /// sealed over the full canonical bytes (chain-format.md).
    fn seal(
        &self,
        prev: Option<&SignedEnvelope>,
        tenant_id: Uuid,
        event_type: &str,
        principal: &str,
        api_key_id: Option<Uuid>,
        payload: Value,
    ) -> Result<SignedEnvelope, AppError> {
        let draft = EventDraft {
            tenant_id,
            event_id: self.ids.new_id(),
            event_type: event_type.to_owned(),
            principal: principal.to_owned(),
            api_key_id,
            at: self.clock.now(),
            payload,
        };
        let envelope = next_envelope(prev, draft)?;
        let signed_bytes = envelope.signed_bytes()?;
        let hash = Sha256Hash::digest(&signed_bytes);
        let sig = self.signer.sign(&signed_bytes);
        Ok(SignedEnvelope {
            envelope,
            signed_bytes,
            hash,
            sig,
        })
    }

    /// The response recorded under `scope` for this request, if any; a
    /// record of another request is `idempotency.digest_mismatch`
    /// (http-api.md idempotency).
    async fn recorded<T: DeserializeOwned>(
        &self,
        scope: &IdempotencyScope,
        request_digest: &str,
    ) -> Result<Option<T>, AppError> {
        let Some(record) = self.journal.idempotency_get(scope).await? else {
            return Ok(None);
        };
        if record.request_digest != request_digest {
            return Err(AppError::IdempotencyDigestMismatch);
        }
        serde_json::from_value(record.response)
            .map(Some)
            .map_err(|e| AppError::StoredResponse(e.to_string()))
    }

    async fn commit(
        &self,
        event: SignedEnvelope,
        fold: MetadataFold,
        idempotency: Option<IdempotencyPut>,
    ) -> Result<CommitReceipt, AppError> {
        Ok(self
            .journal
            .commit(commit_request(event, fold, idempotency))
            .await?)
    }

    /// Commits chain-consecutive events as one transaction: a refusal of
    /// any of them writes none (ADR-0025 d10).
    async fn commit_chain(
        &self,
        links: Vec<(SignedEnvelope, MetadataFold)>,
    ) -> Result<CommitReceipt, AppError> {
        Ok(self
            .journal
            .commit_chain(
                links
                    .into_iter()
                    .map(|(event, fold)| commit_request(event, fold, None))
                    .collect(),
            )
            .await?)
    }

    fn mint_key(
        &self,
        tenant_id: Uuid,
        req: &IssueKeyRequest,
        default_workarea: Option<Uuid>,
        issued_by: Option<Uuid>,
    ) -> (KeyRecord, String) {
        let secret = self.ids.new_secret();
        let value = format!("oml_{}", base62_43(&secret));
        let record = KeyRecord {
            key_id: self.ids.new_id(),
            tenant_id,
            role: req.role,
            principal: req.principal.clone(),
            // The schema default when no attestation was sent
            // (postgres-schema.md api_keys.held_by); the journal says null.
            held_by: req.held_by.unwrap_or(HeldBy::Agent),
            default_workarea,
            issued_by,
            key_hash: sha256_digest_string(value.as_bytes()),
            // Issuance takes neither yet (ADR-0026 d8, d9).
            name: None,
            crew_id: None,
        };
        (record, value)
    }

    /// The catalog payload of `key.issued` (event-catalog.md): never the
    /// value (roles-and-keys.md show-once rule), and `held_by` as sent.
    fn key_issued_payload(record: &KeyRecord, held_by: Option<HeldBy>) -> Value {
        json!({
            "key_id": record.key_id.to_string(),
            "role": record.role.as_str(),
            "principal": record.principal,
            "issued_by_key_id": record.issued_by.map(|k| k.to_string()),
            "expiry": Value::Null,
            "value_disclosed": false,
            "held_by": held_by.map(HeldBy::as_str),
            "default_workarea_id": record.default_workarea.map(|w| w.to_string()),
        })
    }

    /// One `grant.issued` on `key_id`, sealed on `prev`: its payload's epoch
    /// is its own seq (event-catalog.md `grant.issued`; ADR-0025 d2).
    async fn commit_grant(
        &self,
        prev: &SignedEnvelope,
        issuer: &KeyRecord,
        key_id: Uuid,
        grant: Grant,
    ) -> Result<(SignedEnvelope, GrantRecord), AppError> {
        let epoch = prev.envelope.seq + 1;
        let event = self.seal(
            Some(prev),
            issuer.tenant_id,
            event_types::GRANT_ISSUED,
            &issuer.principal,
            Some(issuer.key_id),
            json!({
                "grant_id": grant.grant_id.to_string(),
                "key_id": key_id.to_string(),
                "workarea_node_id": grant.node.to_string(),
                "role": grant.role.as_str(),
                "deny": grant
                    .deny
                    .iter()
                    .map(|d| json!({ "subtree_node_id": d.to_string() }))
                    .collect::<Vec<_>>(),
                "epoch": epoch,
            }),
        )?;
        let receipt = self
            .commit(
                event.clone(),
                MetadataFold::GrantIssued {
                    key_id,
                    grant: grant.clone(),
                },
                None,
            )
            .await?;
        Ok((
            event,
            GrantRecord {
                grant,
                epoch: receipt.seq,
            },
        ))
    }

    /// The `key.revoked` that follows an issuance which failed after its
    /// `key.issued`, in a commit of its own, over the fixed reason
    /// (roles-and-keys.md issuance order; ADR-0025 d2).
    async fn revoke_incomplete(
        &self,
        issuer: &KeyRecord,
        key_id: Uuid,
    ) -> Result<CommitReceipt, AppError> {
        let prev = self.head_event(issuer.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            issuer.tenant_id,
            event_types::KEY_REVOKED,
            &issuer.principal,
            Some(issuer.key_id),
            json!({
                "key_id": key_id.to_string(),
                "revoked_by_key_id": issuer.key_id.to_string(),
                "reason_digest": sha256_digest_string(ISSUANCE_INCOMPLETE.as_bytes()),
            }),
        )?;
        self.commit(event, MetadataFold::KeyRevoked { key_id }, None)
            .await
    }

    // -- use-cases ----------------------------------------------------------

    /// The operator ceremony: genesis event, the `universe` root workarea,
    /// first admin key (roles-and-keys.md bootstrap; workareas.md, one
    /// universe per tenant). The root's name is the first segment of every
    /// served `workarea_path` (ADR-0025 d14). Unauthenticated by design — the
    /// composition decides who may run it.
    pub async fn bootstrap_tenant(
        &self,
        req: &BootstrapRequest,
    ) -> Result<BootstrapResult, AppError> {
        let tenant_id = self.ids.new_id();
        let genesis = self.seal(
            None,
            tenant_id,
            event_types::TENANT_GENESIS,
            "operator",
            None,
            json!({ "tenant_name": req.tenant_name.as_str() }),
        )?;
        let mut prev = genesis.clone();
        let fold = MetadataFold::TenantGenesis {
            name: req.tenant_name.clone(),
        };
        self.commit(genesis, fold, None).await?;

        let root_workarea_id = self.ids.new_id();
        let workarea = WorkareaRecord {
            workarea_id: root_workarea_id,
            tenant_id,
            parent_id: None,
            name: Name::parse(UNIVERSE).expect("universe is a name (ADR-0026 d6)"),
            crew: None,
            crew_id: None,
        };
        let event = self.seal(
            Some(&prev),
            tenant_id,
            event_types::WORKAREA_CREATE,
            "operator",
            None,
            json!({
                "workarea_id": root_workarea_id.to_string(),
                "parent_id": Value::Null,
                "name": workarea.name.as_str(),
                "status": workarea.status(),
                "crew": Value::Null,
                "prefix": Value::Null,
            }),
        )?;
        prev = event.clone();
        self.commit(event, MetadataFold::WorkareaCreated(workarea), None)
            .await?;

        let issue = IssueKeyRequest {
            role: Role::Admin,
            principal: req.admin_principal.clone(),
            held_by: Some(HeldBy::Human),
            default_workarea: Some(root_workarea_id),
            grants: Vec::new(),
        };
        let (record, value) = self.mint_key(tenant_id, &issue, Some(root_workarea_id), None);
        let key_id = record.key_id;
        let event = self.seal(
            Some(&prev),
            tenant_id,
            event_types::KEY_ISSUED,
            "operator",
            None,
            Self::key_issued_payload(&record, issue.held_by),
        )?;
        let receipt = self
            .commit(event, MetadataFold::KeyIssued(record), None)
            .await?;

        Ok(BootstrapResult {
            tenant_id,
            root_workarea_id,
            admin_key: IssuedKey {
                key_id,
                value,
                role: issue.role,
                principal: issue.principal,
                held_by: issue.held_by,
                default_workarea: issue.default_workarea,
                access: KeyAccess {
                    issued_seq: receipt.seq,
                    grants: Vec::new(),
                    denials: Vec::new(),
                },
            },
        })
    }

    /// Issue a key with its grants, validated in the issuance order of
    /// roles-and-keys.md (ADR-0025 d2): the two-key rule, the issuer ceiling,
    /// grant admissibility, node existence, deny containment, the default's
    /// coverage. `key.issued` commits first and one `grant.issued` per grant
    /// follows; the value is returned only after the last of them commits.
    /// An issuance that fails after `key.issued` journals `key.revoked` over
    /// the fixed reason and answers the failure: the value was never
    /// disclosed, so the key authenticates nothing either way.
    pub async fn issue_key(
        &self,
        api_key: &str,
        req: &IssueKeyRequest,
    ) -> Result<IssuedKey, AppError> {
        let issuer = self.authenticate(api_key).await?.key;
        if issuer.role != Role::Admin && issuer.role != Role::Operator {
            return Err(AppError::VerbCeiling {
                role: issuer.role,
                verb: "issue_key",
            });
        }
        let tenant_id = issuer.tenant_id;
        let grants: Vec<Grant> = req
            .grants
            .iter()
            .map(|g| Grant {
                grant_id: self.ids.new_id(),
                node: g.node,
                role: g.role,
                deny: g.deny.clone(),
            })
            .collect();
        // Steps 1–3: the two-key rule, the issuer ceiling, admissibility.
        check_issuance(issuer.role, req.role, &grants).map_err(issuance_refusal)?;
        // Step 4: every grant node and deny node is the tenant's.
        let mut ancestries: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
        for node in grants
            .iter()
            .flat_map(|g| std::iter::once(g.node).chain(g.deny.iter().copied()))
        {
            if let Entry::Vacant(slot) = ancestries.entry(node) {
                let ancestry = self.ancestry(tenant_id, node).await?;
                if ancestry.is_empty() {
                    return Err(AppError::NotFound);
                }
                slot.insert(ancestry);
            }
        }
        // Step 5: each deny node lies inside its own grant's subtree.
        for (grant_index, grant) in grants.iter().enumerate() {
            if let Some(&outside) = grant
                .deny
                .iter()
                .find(|d| !ancestries[*d].contains(&grant.node))
            {
                return Err(AppError::DenyOutsideGrant {
                    grant_index,
                    subtree_node_id: outside,
                });
            }
        }
        // Step 6: a sent default lies in some grant's subtree and outside
        // that grant's denies; absent, it is the first grant's node, and
        // never the issuer's (workareas.md per-grant attenuation).
        let default_workarea = match req.default_workarea {
            None => grants.first().map(|g| g.node),
            Some(workarea_id) => {
                let ancestry = self.ancestry(tenant_id, workarea_id).await?;
                let covered = grants.iter().any(|g| {
                    ancestry.contains(&g.node) && !g.deny.iter().any(|d| ancestry.contains(d))
                });
                if !covered {
                    return Err(AppError::DefaultUncovered { workarea_id });
                }
                Some(workarea_id)
            }
        };

        let (record, value) = self.mint_key(tenant_id, req, default_workarea, Some(issuer.key_id));
        let key_id = record.key_id;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_ISSUED,
            &issuer.principal,
            Some(issuer.key_id),
            Self::key_issued_payload(&record, req.held_by),
        )?;
        let mut last = event.clone();
        let receipt = self
            .commit(event, MetadataFold::KeyIssued(record), None)
            .await?;
        let mut stored = Vec::with_capacity(grants.len());
        for grant in grants {
            match self.commit_grant(&last, &issuer, key_id, grant).await {
                Ok((event, record)) => {
                    last = event;
                    stored.push(record);
                }
                Err(failure) => {
                    // The failure answers whether or not the revocation
                    // commits: one that cannot leaves a key whose value
                    // nobody holds, which authenticates nothing.
                    let _revocation = self.revoke_incomplete(&issuer, key_id).await;
                    return Err(failure);
                }
            }
        }
        Ok(IssuedKey {
            key_id,
            value,
            role: req.role,
            principal: req.principal.clone(),
            held_by: req.held_by,
            default_workarea,
            access: KeyAccess {
                issued_seq: receipt.seq,
                grants: stored,
                denials: Vec::new(),
            },
        })
    }

    /// Journal a global per-key denial on `key_id` over the subtree at
    /// `node`: deny-wins across all of the key's grants, its epoch its own
    /// `key.denied` seq. Admin key role only; a key or node the tenant does
    /// not hold is absent (http-api.md keys, grants, denials; ADR-0025 d2).
    pub async fn deny_key(
        &self,
        api_key: &str,
        key_id: Uuid,
        node: Uuid,
    ) -> Result<DenialRecord, AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "deny")?;
        let tenant_id = admin.tenant_id;
        if self.journal.key_access(tenant_id, key_id).await?.is_none()
            || self.journal.workarea(tenant_id, node).await?.is_none()
        {
            return Err(AppError::NotFound);
        }
        let denial = KeyDenial {
            denial_id: self.ids.new_id(),
            node,
        };
        let prev = self.head_event(tenant_id).await?;
        let epoch = prev.as_ref().map_or(1, |p| p.envelope.seq + 1);
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_DENIED,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "denial_id": denial.denial_id.to_string(),
                "key_id": key_id.to_string(),
                "workarea_node_id": node.to_string(),
                "epoch": epoch,
            }),
        )?;
        let receipt = self
            .commit(
                event,
                MetadataFold::KeyDenied {
                    key_id,
                    denial: denial.clone(),
                    created_by: admin.key_id,
                },
                None,
            )
            .await?;
        Ok(DenialRecord {
            denial,
            created_by: admin.key_id,
            epoch: receipt.seq,
        })
    }

    /// Create a node under `parent_id`, admin by key role. The crew is
    /// recorded, not operated: `workarea.create` journals it and the fold
    /// stores it, so the workarea read serves it from the store (event-
    /// catalog.md `workarea.create`; ADR-0025 d14). The answer carries the
    /// node's path derived from the parent's stored ancestry, as the read
    /// derives it. A name the tenant already holds, `universe` included, is
    /// the store's `NameTaken`, and nothing is journalled (ADR-0026 d5, d6).
    pub async fn create_workarea(
        &self,
        api_key: &str,
        name: &Name,
        parent_id: Option<Uuid>,
        crew: Option<Crew>,
    ) -> Result<CreatedWorkarea, AppError> {
        let key = self.authenticate(api_key).await?.key;
        require_role(&key, Role::Admin, "create_workarea")?;
        let ancestors = match parent_id {
            Some(parent) => self
                .journal
                .workarea_ancestry(key.tenant_id, parent)
                .await?
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let workarea = WorkareaRecord {
            workarea_id: self.ids.new_id(),
            tenant_id: key.tenant_id,
            parent_id,
            name: name.clone(),
            crew,
            crew_id: None,
        };
        let prev = self.head_event(key.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::WORKAREA_CREATE,
            &key.principal,
            Some(key.key_id),
            json!({
                "workarea_id": workarea.workarea_id.to_string(),
                "parent_id": parent_id.map(|p| p.to_string()),
                "name": workarea.name.as_str(),
                "status": workarea.status(),
                "crew": workarea.crew,
                "prefix": Value::Null,
            }),
        )?;
        self.commit(event, MetadataFold::WorkareaCreated(workarea.clone()), None)
            .await?;
        let ancestry: Vec<WorkareaRecord> =
            std::iter::once(workarea.clone()).chain(ancestors).collect();
        Ok(CreatedWorkarea {
            path: derived_path(&ancestry),
            workarea,
        })
    }

    /// One workarea as the calling key may see it, at reader effective
    /// access by the refusal order and otherwise absent (http-api.md
    /// workareas; ADR-0025 d14): the node and its derived path, the
    /// immediate children the key can read, the verbs its effective roles
    /// on the node confer, and the permission epoch of the access this
    /// request read. An omitted node is the key's journalled default,
    /// resolved only after step 1 of the refusal order, as recall resolves
    /// it (roles-and-keys.md refusal order; mcp-plugin.md
    /// `oml_workarea_info`).
    pub async fn workarea_info(
        &self,
        api_key: &str,
        node: Option<Uuid>,
    ) -> Result<WorkareaInfo, AppError> {
        let caller = self.authenticate(api_key).await?;
        require_admissible(&caller.key, Role::Reader, "workarea_info")?;
        let node = node
            .or(caller.key.default_workarea)
            .ok_or(AppError::WorkareaUnresolved)?;
        let tenant_id = caller.key.tenant_id;
        let ancestry = self
            .journal
            .workarea_ancestry(tenant_id, node)
            .await?
            .unwrap_or_default();
        let ids: Vec<Uuid> = ancestry.iter().map(|w| w.workarea_id).collect();
        let effective = caller.effective(&ids);
        resolve(caller.key.role, &effective, Role::Reader)
            .map_err(|refusal| access_refusal(refusal, node, "workarea_info"))?;
        let workarea = ancestry.first().cloned().ok_or(AppError::NotFound)?;
        let mut children = Vec::new();
        // A child's ancestry is the node's with the child in front.
        for child in self.journal.workarea_children(tenant_id, node).await? {
            let child_ancestry: Vec<Uuid> = std::iter::once(child.workarea_id)
                .chain(ids.iter().copied())
                .collect();
            if caller.effective(&child_ancestry).contains(&Role::Reader) {
                children.push(child);
            }
        }
        Ok(WorkareaInfo {
            path: derived_path(&ancestry),
            workarea,
            children,
            my_effective_verbs: effective_verbs(&effective, caller.key.held_by),
            permission_epoch: caller.permission_epoch,
        })
    }

    /// `GET /v1/memories` (http-api.md search; ADR-0025 d3, d13). With
    /// `own`, the own-work listing: every row the calling key authored, in
    /// every state, across every node minus the subtrees its global
    /// denials cover; any tenant role but operator, no grant needed, and a
    /// `workarea_id` filter naming a node where the key holds some role.
    /// Without `own`, the scoped listing: reader on the scope node by the
    /// refusal order, and the promoted served tips of subtree(scope) ∩ the
    /// nodes where reader is effective, narrowed to a `workarea_id` subtree
    /// where reader is effective. Newest first; the cursor is bound to the
    /// key and to `api.cursor_ttl`, and every page re-applies the filter.
    pub async fn list_memories(
        &self,
        api_key: &str,
        req: &ListRequest,
    ) -> Result<MemoryPage, AppError> {
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        let tenant_id = key.tenant_id;
        let subtree_ids = |node: Uuid| async move {
            Ok::<BTreeSet<Uuid>, AppError>(
                self.journal
                    .workarea_subtree(tenant_id, node)
                    .await?
                    .into_iter()
                    .map(|w| w.workarea_id)
                    .collect(),
            )
        };
        let (scope, subtree) = if req.own {
            // The operator never reads content (roles-and-keys.md#own).
            if key.role == Role::Operator {
                return Err(AppError::VerbCeiling {
                    role: key.role,
                    verb: "list",
                });
            }
            // A filter names only a node the naming rule names to the key.
            let subtree = match req.workarea_id {
                None => None,
                Some(node) => {
                    let ancestry = self.ancestry(tenant_id, node).await?;
                    if caller.effective(&ancestry).is_empty() {
                        return Err(AppError::NotFound);
                    }
                    Some(subtree_ids(node).await?.into_iter().collect())
                }
            };
            let mut masked = BTreeSet::new();
            for denial in &caller.denials {
                masked.extend(subtree_ids(denial.node).await?);
            }
            let scope = ListingScope::Own {
                key_ids: vec![key.key_id],
                masked: masked.into_iter().collect(),
            };
            (scope, subtree)
        } else {
            require_admissible(key, Role::Reader, "list")?;
            let scope = req
                .scope
                .or(key.default_workarea)
                .ok_or(AppError::WorkareaUnresolved)?;
            self.require_access(&caller, scope, Role::Reader, "list")
                .await?;
            let mut nodes = self.nodes_with(&caller, scope, Role::Reader).await?;
            if let Some(node) = req.workarea_id {
                let ancestry = self.ancestry(tenant_id, node).await?;
                if !caller.effective(&ancestry).contains(&Role::Reader) {
                    return Err(AppError::NotFound);
                }
                let within = subtree_ids(node).await?;
                nodes.retain(|n| within.contains(n));
            }
            (ListingScope::Tips { nodes }, None)
        };
        let now = self.clock.now();
        let after = req
            .cursor
            .as_deref()
            .map(|cursor| ListingCursor::resume(cursor, key.key_id, &now))
            .transpose()?;
        let filter = ListingFilter {
            scope,
            subtree,
            state: req.state,
            kind: req.kind,
            claim_key: req.claim_key.clone(),
            since: req.since.clone(),
            until: req.until.clone(),
        };
        // One row past the page says whether another page exists.
        let mut rows = self
            .journal
            .list_memories(
                tenant_id,
                &filter,
                after.as_ref(),
                req.limit.saturating_add(1),
            )
            .await?;
        let more = rows.len() > req.limit;
        rows.truncate(req.limit);
        let next_cursor = rows.last().filter(|_| more).map(|last| {
            ListingCursor {
                key_id: key.key_id,
                position: ListingPosition {
                    created_at: last.created_at.clone(),
                    artifact_id: last.memory_id,
                },
                issued_at: now.clone(),
            }
            .encode()
        });
        let mut items = Vec::with_capacity(rows.len());
        for m in rows {
            // R3 defense in depth: nothing outside the scope's predicate
            // leaves the application layer, whatever the store returned.
            let admitted = match &filter.scope {
                ListingScope::Own { key_ids, masked } => {
                    key_ids.contains(&m.author_key_id) && !masked.contains(&m.workarea_id)
                }
                ListingScope::Tips { nodes } => {
                    m.status == MemoryStatus::Promoted && m.is_tip && nodes.contains(&m.workarea_id)
                }
            };
            if admitted {
                items.push(self.served(&caller, m).await?);
            }
        }
        Ok(MemoryPage { items, next_cursor })
    }

    /// Submit a memory: writer on the resolved node, which must exist, by
    /// the refusal order (ADR-0025 d1); every `derives_from` target readable
    /// (ADR-0025 d3); the claim text normalized to NFC before the digest,
    /// the scan and the byte count (admission.md capture; ADR-0025 d6);
    /// same-call deterministic scan, whose veto is a refusal that journals
    /// nothing; a new claim's `claim_key` free in the workarea (ADR-0025
    /// d18); held on arrival as the claim's version 1, idempotency record
    /// honored (admission.md, http-api.md).
    pub async fn submit_memory(
        &self,
        api_key: &str,
        req: &SubmitRequest,
    ) -> Result<SubmitOutcome, AppError> {
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Writer, "submit")?;
        let workarea_id = req
            .workarea_id
            .or(key.default_workarea)
            .ok_or(AppError::WorkareaUnresolved)?;
        self.require_access(&caller, workarea_id, Role::Writer, "submit")
            .await?;

        let scope = IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workarea_id,
            idem_key: req.idempotency_key.clone(),
        };
        let request_digest = submit_request_digest(req, workarea_id);
        if let Some(record) = self.journal.idempotency_get(&scope).await? {
            if record.request_digest != request_digest {
                return Err(AppError::IdempotencyDigestMismatch);
            }
            let receipt: SubmitReceipt = serde_json::from_value(record.response)
                .map_err(|e| AppError::StoredResponse(e.to_string()))?;
            return Ok(SubmitOutcome {
                receipt,
                replayed: true,
            });
        }

        // Capture normalizes first: the digest, the scan and the byte count
        // all see the NFC text (admission.md capture; ADR-0025 d6).
        let body = nfc(&req.body);

        // Deterministic scan: same call, before the commit transaction. A
        // veto is a refusal like any other: it journals nothing and stores
        // no row (admission.md; errors.md submission.scan_veto).
        let scan_finding = match self.scanner.scan(&AdmissionEnvelope {
            tenant_id: key.tenant_id,
            kind: req.kind.as_str(),
            body: &body,
        }) {
            ScanOutcome::Pass => None,
            ScanOutcome::Hold(reason) => Some(reason),
            ScanOutcome::Veto(reason) => return Err(AppError::ScanVeto(reason)),
        };

        // The lineage closure: the author plus every declared ancestor's
        // closure — R2 runs over the whole of it (roles-and-keys.md). An
        // ancestor the key cannot read is indistinguishable from none.
        let mut principals = BTreeSet::from([key.principal.clone()]);
        let mut key_ids = BTreeSet::from([key.key_id]);
        for ancestor in &req.derives_from {
            let m = self
                .journal
                .memory(key.tenant_id, *ancestor)
                .await?
                .ok_or(AppError::LineageUnknown(*ancestor))?;
            if !self.readable(&caller, &m).await? {
                return Err(AppError::LineageUnknown(*ancestor));
            }
            principals.extend(m.lineage_principals);
            key_ids.extend(m.lineage_key_ids);
        }

        // The head first, then the claim_key read: a capture that commits
        // in between moves the head, so this commit conflicts and its retry
        // reads the key again (ADR-0025 d10, ordinals; d18).
        let prev = self.head_event(key.tenant_id).await?;
        if self
            .journal
            .claim_key_state(key.tenant_id, workarea_id, &req.claim_key)
            .await?
            .live
        {
            return Err(AppError::ClaimKeyConflict);
        }

        let memory_id = self.ids.new_id();
        let body_digest = sha256_digest_string(body.as_bytes());
        let body_bytes = body.len() as u64;
        let memory = MemoryRecord {
            memory_id,
            tenant_id: key.tenant_id,
            workarea_id,
            claim_key: req.claim_key.clone(),
            version: 1,
            kind: req.kind,
            body,
            body_digest: body_digest.clone(),
            body_bytes,
            status: MemoryStatus::Held,
            is_tip: false,
            promoted_at: None,
            retirement: false,
            replaces: None,
            author_principal: key.principal.clone(),
            author_key_id: key.key_id,
            derives_from: req.derives_from.clone(),
            lineage_principals: principals.into_iter().collect(),
            lineage_key_ids: key_ids.into_iter().collect(),
            scan_finding: scan_finding.clone(),
            created_at: self.clock.now(),
        };

        // The catalog payload of submission.remember: digest-only, body
        // plaintext never rides the journal (ADR-0002 d1; event-catalog.md).
        // The PoC stores the body inline and unencrypted, so there is no
        // ciphertext digest, and it pins no scan rule set, so there is no
        // scanner version (admission.md rule-set registry).
        let payload = json!({
            "artifact_id": memory_id.to_string(),
            "workarea_id": workarea_id.to_string(),
            "claim_key": req.claim_key,
            "version": memory.version,
            "kind": req.kind.as_str(),
            "plaintext_digest": body_digest,
            "body_bytes": body_bytes,
            "ciphertext_digest": Value::Null,
            "attachments": [],
            "derives_from": req.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "provenance": "agent",
            "simulation_id": Value::Null,
            "replaces": Value::Null,
            "retirement": false,
            "scan": {
                "result": if scan_finding.is_some() { "hold" } else { "pass" },
                "scanner_version": Value::Null,
                "finding": scan_finding,
            },
        });
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::SUBMISSION_REMEMBER,
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let seq = event.envelope.seq;

        let receipt = SubmitReceipt {
            memory_id,
            claim_key: req.claim_key.clone(),
            version: memory.version,
            workarea_id,
            state: MemoryStatus::Held.as_str().to_owned(),
            journal_seq: seq,
            body_digest,
        };
        let idempotency = Some(IdempotencyPut {
            scope,
            record: IdempotencyRecord {
                request_digest,
                response: serde_json::to_value(&receipt)
                    .map_err(|e| AppError::StoredResponse(e.to_string()))?,
            },
        });
        self.commit(
            event,
            MetadataFold::MemoryHeld {
                memory,
                enqueue: true,
            },
            idempotency,
        )
        .await?;
        Ok(SubmitOutcome {
            receipt,
            replayed: false,
        })
    }

    /// Serve the next reviewable held item to this dreamer, claiming it,
    /// from the rows within its dreamer effective access only; a
    /// `held_by: service` key is refused before any lookup (ADR-0025 d1).
    pub async fn review_next(&self, api_key: &str) -> Result<Option<ServedMemory>, AppError> {
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Dreamer, "review")?;
        refuse_service(key, "review")?;
        let workareas = self.dreamer_nodes(&caller).await?;
        match self
            .journal
            .review_claim_next(key.tenant_id, &key.principal, key.key_id, &workareas)
            .await?
        {
            None => Ok(None),
            Some(memory) => Ok(Some(self.served(&caller, memory).await?)),
        }
    }

    /// Record a verdict. Dreamer on the row's workarea by the refusal order,
    /// with no exception for review, and never from a `held_by: service` key
    /// (ADR-0025 d1); R2 runs here against the row's reviewer-exclusion set
    /// — its lineage closure plus, for a held successor, the authors of the
    /// held rivals of its target (`check_reviewer`; ADR-0025 d10). With the
    /// PoC policy k = 1 an approval ratifies immediately, as a second
    /// journalled event carrying the policy snapshot, committed in the
    /// verdict's own transaction (admission.md review and
    /// ratification): `lifecycle.promote` for a first version, and for a
    /// successor `lifecycle.supersede`, which compare-and-sets that its
    /// target is still the served tip of the successor's workarea and
    /// `claim_key`. An approval whose target lost the tip is
    /// `submission.tip_conflict`, journals nothing and leaves the row held
    /// and queued (ADR-0025 d10).
    pub async fn post_verdict(
        &self,
        api_key: &str,
        memory_id: Uuid,
        verdict: Verdict,
        reason: Option<&str>,
    ) -> Result<VerdictOutcome, AppError> {
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Dreamer, "verdict")?;
        refuse_service(key, "verdict")?;
        let memory = self
            .journal
            .memory(key.tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        self.require_access(&caller, memory.workarea_id, Role::Dreamer, "verdict")
            .await?;
        let mut exclusion = memory.lineage_closure();
        if let Some(target) = memory.replaces
            && memory.status == MemoryStatus::Held
        {
            for rival in self.journal.held_successors(key.tenant_id, target).await? {
                if rival.memory_id != memory.memory_id
                    && rival.workarea_id == memory.workarea_id
                    && rival.claim_key == memory.claim_key
                {
                    exclusion.add_principal(rival.author_principal);
                    exclusion.add_key_id(rival.author_key_id);
                }
            }
        }
        check_reviewer(key.role, &key.principal, key.key_id, &exclusion)?;

        let (verdict_type, target_status) = match verdict {
            Verdict::Approve => (event_types::REVIEW_APPROVE, MemoryStatus::Promoted),
            Verdict::Reject => (event_types::REVIEW_REJECT, MemoryStatus::Rejected),
        };
        // The pure transition function is the arbiter: anything but a held
        // row refuses here (admission.md lifecycle).
        let new_status = transition(memory.status, target_status)?;

        // The head first, then the compare-and-set read: a commit landing
        // in between moves the head, so the verdict conflicts and its retry
        // reads the target again.
        let prev = self.head_event(key.tenant_id).await?;
        let replaced = match (verdict, memory.replaces) {
            (Verdict::Approve, Some(target_id)) => {
                let target = self.journal.memory(key.tenant_id, target_id).await?;
                match target {
                    Some(t)
                        if t.status == MemoryStatus::Promoted
                            && t.is_tip
                            && t.workarea_id == memory.workarea_id
                            && t.claim_key == memory.claim_key =>
                    {
                        Some(t)
                    }
                    _ => {
                        return Err(AppError::TipConflict {
                            current_tip: self
                                .readable_tip(&caller, memory.workarea_id, &memory.claim_key)
                                .await?,
                        });
                    }
                }
            }
            _ => None,
        };

        let verdict_event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            verdict_type,
            &key.principal,
            Some(key.key_id),
            json!({
                "artifact_id": memory_id.to_string(),
                "verdict": match verdict { Verdict::Approve => "approve", Verdict::Reject => "reject" },
                "reason": reason,
                "served_body_digest": memory.body_digest,
            }),
        )?;
        let verdict_event_id = verdict_event.envelope.event_id;

        match verdict {
            Verdict::Reject => {
                let receipt = self
                    .commit(
                        verdict_event,
                        MetadataFold::MemoryRejected { memory_id },
                        None,
                    )
                    .await?;
                Ok(VerdictOutcome {
                    memory_id,
                    version: memory.version,
                    state: new_status,
                    journal_seq: receipt.seq,
                })
            }
            Verdict::Approve => {
                // Ratification snapshots the policy it was decided under
                // (admission.md). The PoC journals no review.ratify: at k = 1
                // the approving verdict is the ratifying act, so
                // ratification_event_id names it. Nothing is self-signed in
                // the PoC (ADR-0025 d17).
                let policy = json!({ "k": 1, "required_roles": ["dreamer"] });
                let (event_type, payload, fold) = match replaced {
                    None => (
                        event_types::LIFECYCLE_PROMOTE,
                        json!({
                            "artifact_id": memory_id.to_string(),
                            "version": memory.version,
                            "workarea_id": memory.workarea_id.to_string(),
                            "claim_key": memory.claim_key,
                            "ratification_event_id": verdict_event_id.to_string(),
                            "previous_tip_version": Value::Null,
                            "self_signed": false,
                            "policy": policy,
                        }),
                        MetadataFold::MemoryPromoted { memory_id },
                    ),
                    Some(target) => (
                        event_types::LIFECYCLE_SUPERSEDE,
                        json!({
                            "claim_key": memory.claim_key,
                            "workarea_id": memory.workarea_id.to_string(),
                            "new_artifact_id": memory_id.to_string(),
                            "new_version": memory.version,
                            "replaces_artifact_id": target.memory_id.to_string(),
                            "replaces_version": target.version,
                            "retirement": memory.retirement,
                            "ratification_event_id": verdict_event_id.to_string(),
                            "self_signed": false,
                            "policy": policy,
                        }),
                        MetadataFold::TipSuperseded {
                            successor: memory_id,
                            replaced: target.memory_id,
                        },
                    ),
                };
                let ratification = self.seal(
                    Some(&verdict_event),
                    key.tenant_id,
                    event_type,
                    "oml-server",
                    None,
                    payload,
                )?;
                // The verdict and its ratification commit as one transaction,
                // so the compare-and-set conditions the verdict too: a lost
                // one journals neither, and a chain-head conflict leaves no
                // verdict for the retry to journal twice (ADR-0025 d10).
                let links = vec![
                    (
                        verdict_event,
                        MetadataFold::VerdictRecorded {
                            memory_id,
                            reviewer_key_id: key.key_id,
                        },
                    ),
                    (ratification, fold),
                ];
                let receipt = match self.commit_chain(links).await {
                    Err(AppError::Store(StoreError::TipMoved { .. })) => {
                        return Err(AppError::TipConflict {
                            current_tip: self
                                .readable_tip(&caller, memory.workarea_id, &memory.claim_key)
                                .await?,
                        });
                    }
                    other => other?,
                };
                Ok(VerdictOutcome {
                    memory_id,
                    version: memory.version,
                    state: new_status,
                    journal_seq: receipt.seq,
                })
            }
        }
    }

    /// Recall: reader on the scope node by the refusal order, and no
    /// tenant-wide fallback (ADR-0025 d1). Segment 1 is the applicable
    /// steering — the steering in force on the scope and each ancestor the
    /// key reads, ordered structurally and never by the query; the hits
    /// follow, over subtree(scope) ∩ the nodes where reader is effective, of
    /// the kinds the request admits, score descending then `artifact_id`
    /// ascending (retrieval.md, the recall-ordering contract; ADR-0025 d8).
    /// With `include_own_held` the calling key's own held rows join the hit
    /// candidates under that one order, flagged by state (d4). The merged
    /// candidates are cut to `k`, then by `max_tokens` — steering never cut
    /// and spent first, the hits the longest rank prefix that fits (d7) —
    /// and only the rows served are hydrated, in one set-wise read. The
    /// result names the served ids and the digest of the whole request, the
    /// read log's two inputs (d9).
    pub async fn recall(
        &self,
        api_key: &str,
        req: &RecallRequest,
    ) -> Result<RecallResult, AppError> {
        // A budget counts bodies, so it is refused with bodies off before
        // anything is read, as the request's other shape refusals are
        // (retrieval.md token budgets).
        if req.max_tokens.is_some() && !req.include_bodies {
            return Err(AppError::BudgetWithoutBodies);
        }
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reader, "recall")?;
        let scope = req
            .workarea_id
            .or(key.default_workarea)
            .ok_or(AppError::WorkareaUnresolved)?;
        self.require_access(&caller, scope, Role::Reader, "recall")
            .await?;
        // The query is its NFC text, searched and digested alike (ADR-0025
        // d9).
        let query = nfc(&req.query);

        // The scope's ancestry runs node to root, so each ancestor's own
        // ancestry is its suffix; the store is handed the readable ones root
        // first, the order of their depth.
        let ancestry = self.ancestry(key.tenant_id, scope).await?;
        let applicable: Vec<Uuid> = (0..ancestry.len())
            .rev()
            .filter(|&i| caller.effective(&ancestry[i..]).contains(&Role::Reader))
            .map(|i| ancestry[i])
            .collect();
        // R3 defense in depth, as for the hits below: only a readable served
        // tip in force, never a retirement version (ADR-0025 d8, d11).
        let steering: Vec<MemoryRecord> = self
            .journal
            .steering_in_force(key.tenant_id, &applicable)
            .await?
            .into_iter()
            .filter(|m| {
                m.kind == MemoryKind::Steering
                    && m.status == MemoryStatus::Promoted
                    && m.is_tip
                    && !m.retirement
                    && applicable.contains(&m.workarea_id)
            })
            .collect();

        let readable = self.nodes_with(&caller, scope, Role::Reader).await?;
        let kinds: Vec<MemoryKind> = HIT_KINDS
            .into_iter()
            .filter(|kind| req.kinds.is_empty() || req.kinds.contains(kind))
            .collect();
        let mut candidates = self
            .journal
            .recall_fts(key.tenant_id, &readable, &query, &kinds, req.k)
            .await?;
        if req.include_own_held {
            // The own key set is the calling key alone until rotation is
            // built (roles-and-keys.md#own); the readable set already
            // excludes the key's global denials.
            candidates.extend(
                self.journal
                    .recall_own_held(
                        key.tenant_id,
                        &[key.key_id],
                        &readable,
                        &query,
                        &kinds,
                        req.k,
                    )
                    .await?,
            );
        }
        // One total order over both lists, then the k cut, then the budget
        // cut over the counts `body_bytes` gives without any body read.
        hit_order(&mut candidates, |c| (c.score, c.memory_id));
        candidates.truncate(req.k);
        let steering_counts: Vec<u64> = steering
            .iter()
            .map(|m| budget_tokens(m.body_bytes))
            .collect();
        let hit_counts: Vec<u64> = candidates
            .iter()
            .map(|c| budget_tokens(c.body_bytes))
            .collect();
        let budget = cut(&steering_counts, &hit_counts, req.max_tokens);
        if let Some(budget) = &budget {
            candidates.truncate(budget.hits_served);
        }

        let ids: Vec<Uuid> = candidates.iter().map(|c| c.memory_id).collect();
        let mut rows: BTreeMap<Uuid, MemoryRecord> = self
            .journal
            .memories(key.tenant_id, &ids)
            .await?
            .into_iter()
            .map(|m| (m.memory_id, m))
            .collect();
        let mut served_ids: Vec<Uuid> = steering.iter().map(|m| m.memory_id).collect();
        let mut served_steering = Vec::with_capacity(steering.len());
        for m in steering {
            served_steering.push(self.served(&caller, m).await?);
        }
        let mut hits = Vec::with_capacity(candidates.len());
        for candidate in &candidates {
            let Some(m) = rows.remove(&candidate.memory_id) else {
                continue;
            };
            // R3 defense in depth: the gate changed visibility. Nothing
            // unpromoted or unreadable leaves the application layer except
            // the caller's own held rows when it asked for them; a replaced
            // version or any version of a retired claim is promoted history,
            // never a hit; a retirement version is never in force (ADR-0025
            // d4, d8). Each hit names only the ancestors the caller can read
            // (d3).
            let admitted = match m.status {
                MemoryStatus::Promoted => m.is_tip,
                MemoryStatus::Held => req.include_own_held && m.author_key_id == key.key_id,
                _ => false,
            };
            if admitted
                && !m.retirement
                && kinds.contains(&m.kind)
                && readable.contains(&m.workarea_id)
            {
                served_ids.push(m.memory_id);
                hits.push(RecallHit {
                    served: self.served(&caller, m).await?,
                    score: candidate.score,
                });
            }
        }
        Ok(RecallResult {
            steering: served_steering,
            hits,
            budget,
            served_ids,
            query_digest: recall_query_digest(&query, req, scope)?,
        })
    }

    /// Get one memory: the authoring key sees its own rows in every state,
    /// with no grant needed and global denials still masking them; everyone
    /// else sees promoted rows, replaced versions included, with reader
    /// effective access on the row's workarea; anything else is absence,
    /// indistinguishable from no row. `derives_from` names only the
    /// ancestors the caller can read (http-api.md; ADR-0025 d3). An
    /// operator key never reads content and is refused by its role before
    /// any lookup (step 1 of the refusal order; ADR-0025 d1).
    pub async fn get_memory(
        &self,
        api_key: &str,
        memory_id: Uuid,
    ) -> Result<ServedMemory, AppError> {
        let caller = self.authenticate(api_key).await?;
        if caller.key.role == Role::Operator {
            return Err(AppError::VerbCeiling {
                role: caller.key.role,
                verb: "get",
            });
        }
        let memory = self
            .journal
            .memory(caller.key.tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if self.readable(&caller, &memory).await? {
            self.served(&caller, memory).await
        } else {
            Err(AppError::NotFound)
        }
    }

    /// Propose a successor of the target's claim, held like any write: the
    /// target stays the served tip, and stays in everyone's recall, until a
    /// non-author ratifies the successor (ADR-0025 d10). With `retirement`
    /// the successor proposes retiring the claim (d11). The supersede rule
    /// runs in its order: a key role admitting writer or dreamer (step 1);
    /// the idempotency record, scoped to the key with a nil workarea, looked
    /// up next and replayed whatever the target's state now; the target
    /// readable (step 2); a writer's own target and writer on its workarea
    /// (step 3), or a dreamer's dreamer and reader there, whoever authored
    /// it (step 4, edit-as-proposal); then the target-dependent refusals —
    /// `X-OML-Workarea`, the served tip, the kind and retirement checks,
    /// and from `oml_supersede` the vocabulary-kind check (step 5); then the
    /// checks every submit runs. The successor takes the next ordinal of the
    /// claim_key and journals `submission.remember` (`submission.learn` for
    /// a `derived_fact`) naming what it replaces, with the idempotency record
    /// in the same transaction; a commit that finds the record already
    /// written by a concurrent attempt under the same key writes nothing and
    /// answers that attempt's receipt as a replay ([`IdempotencyPut`] says
    /// what the slice does not claim).
    pub async fn supersede(
        &self,
        api_key: &str,
        req: &SupersedeRequest,
    ) -> Result<SupersedeOutcome, AppError> {
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        let admissible = admissible_grant_roles(key.role);
        let dreamer = admissible.contains(&Role::Dreamer);
        if !dreamer && !admissible.contains(&Role::Writer) {
            return Err(AppError::VerbCeiling {
                role: key.role,
                verb: "supersede",
            });
        }

        // The record is looked up between steps 1 and 2 and replays its
        // stored response whatever the target or the key's access now: the
        // response names only the caller's own successor (http-api.md
        // idempotency). The scope's workarea is nil, so no target's
        // workarea ever enters a record.
        let scope = IdempotencyScope {
            tenant_id: key.tenant_id,
            key_id: key.key_id,
            workarea_id: Uuid::nil(),
            idem_key: req.idempotency_key.clone(),
        };
        let request_digest = supersede_request_digest(req)?;
        if let Some(receipt) = self.recorded(&scope, &request_digest).await? {
            return Ok(SupersedeOutcome {
                receipt,
                replayed: true,
            });
        }

        // Step 2: an unreadable target is indistinguishable from none.
        let target = self
            .journal
            .memory(key.tenant_id, req.target_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if !self.readable(&caller, &target).await? {
            return Err(AppError::NotFound);
        }
        if dreamer {
            // Step 4: edit-as-proposal needs dreamer and reader on the
            // target's workarea, whoever authored it.
            self.require_access(&caller, target.workarea_id, Role::Dreamer, "supersede")
                .await?;
            self.require_access(&caller, target.workarea_id, Role::Reader, "supersede")
                .await?;
        } else {
            // Step 3: own is the target version's authoring key, never the
            // claim's first version (roles-and-keys.md#own).
            if target.author_key_id != key.key_id {
                return Err(AppError::AuthorOnly);
            }
            self.require_access(&caller, target.workarea_id, Role::Writer, "supersede")
                .await?;
        }

        // Step 5, in its order: the scope is the target's workarea, the
        // target is the served tip, the kind stays in its class and a
        // retirement keeps the target's kind and never retires a term, and
        // on oml_supersede the target is of no vocabulary kind (ADR-0025 d12).
        if let Some(workarea_id) = req.workarea_id
            && workarea_id != target.workarea_id
        {
            return Err(AppError::WorkareaNotTarget { workarea_id });
        }
        if target.status != MemoryStatus::Promoted || !target.is_tip {
            return Err(AppError::TipConflict {
                current_tip: self
                    .readable_tip(&caller, target.workarea_id, &target.claim_key)
                    .await?,
            });
        }
        let kind = successor_kind(target.kind, req.kind, req.retirement)?;
        if req.refuse_vocabulary_target
            && matches!(
                target.kind,
                MemoryKind::Term | MemoryKind::TermMapping | MemoryKind::VocabRelease
            )
        {
            return Err(AppError::VocabularyTarget(target.kind));
        }

        // The checks every submit runs: NFC first, then the scan, whose veto
        // journals nothing, then every declared ancestor readable
        // (admission.md capture; ADR-0025 d3, d6).
        let body = nfc(&req.body);
        let scan_finding = match self.scanner.scan(&AdmissionEnvelope {
            tenant_id: key.tenant_id,
            kind: kind.as_str(),
            body: &body,
        }) {
            ScanOutcome::Pass => None,
            ScanOutcome::Hold(reason) => Some(reason),
            ScanOutcome::Veto(reason) => return Err(AppError::ScanVeto(reason)),
        };
        // derives_from ⊇ {target}; the lineage closure is the proposer plus
        // every ancestor's closure, the target's included (roles-and-keys.md).
        let mut derives_from = vec![target.memory_id];
        for ancestor in &req.derives_from {
            if !derives_from.contains(ancestor) {
                derives_from.push(*ancestor);
            }
        }
        let mut principals = BTreeSet::from([key.principal.clone()]);
        let mut key_ids = BTreeSet::from([key.key_id]);
        principals.extend(target.lineage_principals.iter().cloned());
        key_ids.extend(target.lineage_key_ids.iter().copied());
        for ancestor in &derives_from[1..] {
            let m = self
                .journal
                .memory(key.tenant_id, *ancestor)
                .await?
                .ok_or(AppError::LineageUnknown(*ancestor))?;
            if !self.readable(&caller, &m).await? {
                return Err(AppError::LineageUnknown(*ancestor));
            }
            principals.extend(m.lineage_principals);
            key_ids.extend(m.lineage_key_ids);
        }

        // The head first, then the ordinal: 1 + the greatest version of the
        // claim_key in the workarea, whatever its state; a capture that
        // commits in between moves the head, and the retry reads both again
        // (ADR-0025 d10, ordinals).
        let prev = self.head_event(key.tenant_id).await?;
        let version = self
            .journal
            .claim_key_state(key.tenant_id, target.workarea_id, &target.claim_key)
            .await?
            .greatest_version
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                StoreError::Backend(format!(
                    "claim_key {:?} has no next ordinal",
                    target.claim_key
                ))
            })?;

        let successor_id = self.ids.new_id();
        let body_digest = sha256_digest_string(body.as_bytes());
        let body_bytes = body.len() as u64;
        let replaces = VersionRef {
            artifact_id: target.memory_id,
            version: target.version,
        };
        let successor = MemoryRecord {
            memory_id: successor_id,
            tenant_id: key.tenant_id,
            workarea_id: target.workarea_id,
            claim_key: target.claim_key.clone(),
            version,
            kind,
            body,
            body_digest: body_digest.clone(),
            body_bytes,
            status: MemoryStatus::Held,
            is_tip: false,
            promoted_at: None,
            retirement: req.retirement,
            replaces: Some(target.memory_id),
            author_principal: key.principal.clone(),
            author_key_id: key.key_id,
            derives_from: derives_from.clone(),
            lineage_principals: principals.into_iter().collect(),
            lineage_key_ids: key_ids.into_iter().collect(),
            scan_finding: scan_finding.clone(),
            created_at: self.clock.now(),
        };

        // The catalog payload of the submission, digest-only, plus what it
        // replaces and whether it retires (event-catalog.md
        // submission.remember; ADR-0025 d10, d11).
        let event_type = if kind == MemoryKind::DerivedFact {
            event_types::SUBMISSION_LEARN
        } else {
            event_types::SUBMISSION_REMEMBER
        };
        let payload = json!({
            "artifact_id": successor_id.to_string(),
            "workarea_id": target.workarea_id.to_string(),
            "claim_key": target.claim_key,
            "version": version,
            "kind": kind.as_str(),
            "plaintext_digest": body_digest,
            "body_bytes": body_bytes,
            "ciphertext_digest": Value::Null,
            "attachments": [],
            "derives_from": derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "provenance": "agent",
            "simulation_id": Value::Null,
            "replaces": {
                "artifact_id": target.memory_id.to_string(),
                "version": target.version,
            },
            "retirement": req.retirement,
            "scan": {
                "result": if scan_finding.is_some() { "hold" } else { "pass" },
                "scanner_version": Value::Null,
                "finding": scan_finding,
            },
        });
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_type,
            &key.principal,
            Some(key.key_id),
            payload,
        )?;
        let receipt = SupersedeReceipt {
            memory_id: successor_id,
            claim_key: target.claim_key.clone(),
            version,
            workarea_id: target.workarea_id,
            state: MemoryStatus::Held.as_str().to_owned(),
            retirement: req.retirement,
            replaces,
            journal_seq: event.envelope.seq,
            body_digest,
        };
        let idempotency = Some(IdempotencyPut {
            scope: scope.clone(),
            record: IdempotencyRecord {
                request_digest: request_digest.clone(),
                response: serde_json::to_value(&receipt)
                    .map_err(|e| AppError::StoredResponse(e.to_string()))?,
            },
        });
        let committed = self
            .commit(
                event,
                MetadataFold::MemoryHeld {
                    memory: successor,
                    enqueue: true,
                },
                idempotency,
            )
            .await;
        if let Err(AppError::Store(StoreError::IdempotencyRecorded)) = committed {
            // An attempt under the same key committed between this one's
            // lookup and its commit, which wrote nothing: the record that
            // attempt committed answers this one, as it answers any replay
            // (http-api.md idempotency).
            let receipt = self
                .recorded(&scope, &request_digest)
                .await?
                .ok_or(AppError::Store(StoreError::IdempotencyRecorded))?;
            return Ok(SupersedeOutcome {
                receipt,
                replayed: true,
            });
        }
        committed?;
        Ok(SupersedeOutcome {
            receipt,
            replayed: false,
        })
    }

    /// Verify the tenant chain end to end with the domain procedure
    /// (chain-format.md steps 1–7 and 9). Admin ceiling (roles-and-keys.md:
    /// chain verification is an admin verb).
    pub async fn verify_chain(&self, api_key: &str) -> Result<ChainHead, AppError> {
        let key = self.authenticate(api_key).await?.key;
        require_role(&key, Role::Admin, "verify_chain")?;
        let mut events = Vec::new();
        let mut from = 1_u64;
        loop {
            let page = self
                .journal
                .events_range(key.tenant_id, from, EVENTS_PAGE)
                .await?;
            let n = page.len();
            events.extend(page);
            if n < EVENTS_PAGE {
                break;
            }
            from += n as u64;
        }
        let stated_head = self.journal.chain_head(key.tenant_id).await?;
        let keys = KeySet::new(vec![self.signer.verifying_key()]);
        Ok(oml_domain::verify_chain(
            &events,
            &keys,
            stated_head.as_ref(),
        )?)
    }

    /// The events tail (http-api.md#events-tail, the single declaration
    /// site; ADR-0025 d15): reader on the scope by the refusal order — the
    /// named `workarea_id`, else the key's default, else
    /// `request.workarea_unresolved` — then one bounded scan of at most
    /// `limit` raw events from `after_seq + 1`. An event stays only where
    /// the per-family rule ([`tail_visible`]) shows it to this key within
    /// subtree(scope), judged on the event's own payload and its subject's
    /// authoring key and never on later row state, and it is served with
    /// only the names the key may learn. `next_after_seq` is the last raw
    /// seq scanned, or `after_seq` when no raw event follows it, and
    /// `head_seq` is read in the scan's snapshot; an `after_seq` beyond it is
    /// refused.
    pub async fn tail_events(
        &self,
        api_key: &str,
        workarea_id: Option<Uuid>,
        after_seq: u64,
        limit: usize,
    ) -> Result<TailPage, AppError> {
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reader, "events_tail")?;
        let scope = workarea_id
            .or(key.default_workarea)
            .ok_or(AppError::WorkareaUnresolved)?;
        self.require_access(&caller, scope, Role::Reader, "events_tail")
            .await?;
        let scan = self
            .journal
            .events_with_subjects(key.tenant_id, after_seq.saturating_add(1), limit)
            .await?;
        if after_seq > scan.head_seq {
            return Err(AppError::AfterSeqBeyondHead {
                after_seq,
                head_seq: scan.head_seq,
            });
        }
        let next_after_seq = scan
            .events
            .last()
            .map_or(after_seq, |e| e.event.envelope.seq);

        // The own key set is the calling key alone until rotation is built
        // (roles-and-keys.md#own).
        let own_keys = [key.key_id];
        let tail_caller = TailCaller {
            own_keys: &own_keys,
            grants: &caller.grants,
            denials: &caller.denials,
            scope,
        };
        let mut ancestries: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
        let mut events = Vec::new();
        for TailEvent { event, subject } in scan.events {
            let envelope = &event.envelope;
            let Some(judged) = judged_subject(envelope, subject.as_ref()) else {
                continue;
            };
            let ancestry: &Vec<Uuid> = match ancestries.entry(judged.node) {
                Entry::Occupied(known) => known.into_mut(),
                Entry::Vacant(new) => new.insert(self.ancestry(key.tenant_id, judged.node).await?),
            };
            let tail_subject = TailSubject {
                workareas: std::slice::from_ref(ancestry),
                author_key: judged.author_key,
                promoted: judged.promoted,
                self_signed: judged.self_signed,
                outside_audience: judged.outside_audience,
            };
            if !tail_visible(&envelope.event_type, &tail_subject, &tail_caller) {
                continue;
            }
            let names_actor = envelope.api_key_id.is_some_and(|k| own_keys.contains(&k));
            let names_node = !caller.effective(ancestry).is_empty();
            let payload = served_payload(envelope, judged.claim, names_actor, names_node);
            events.push(TailedEvent {
                event,
                names_actor,
                payload,
            });
        }
        Ok(TailPage {
            events,
            head_seq: scan.head_seq,
            next_after_seq,
        })
    }
}

/// What the content tail judges one event by, assembled from the event's
/// own payload and its subject's authoring row (http-api.md#events-tail,
/// the subject table; ADR-0025 d15).
struct JudgedSubject {
    /// Whether the subject is a claim version, not a workarea node.
    claim: bool,
    /// The subject's workarea: the payload's `workarea_id`, else the
    /// subject row's.
    node: Uuid,
    author_key: Option<Uuid>,
    promoted: bool,
    self_signed: bool,
    outside_audience: bool,
}

/// The subject of `envelope` as the tail judges it, or `None` for an event
/// it cannot place, which the tail never shows. The author is the envelope
/// key of a `submission.*` or `simulation.write` event and the subject
/// row's authoring key for a `lifecycle.*` one. *Promoted* holds for a
/// `lifecycle.promote` or `lifecycle.supersede`, the promotion of the
/// version it names; every other `lifecycle.*` event fails it, so only its
/// author sees it — the journal read that judges the latest transition
/// lands with the write paths of those types, none of which this build
/// journals. A subject is `self_signed` unless the promoting payload says
/// `false`, and outside every audience when its submission names a
/// simulation, whose audience is unbuilt. `lifecycle.reassign` and
/// `workarea.relate_*` name two workareas, are never journalled by this
/// build, and are not placed: they stay hidden until they are.
fn judged_subject(envelope: &Envelope, subject: Option<&SubjectRow>) -> Option<JudgedSubject> {
    let event_type = envelope.event_type.as_str();
    let payload = &envelope.payload;
    if event_type == "lifecycle.reassign" || event_type.starts_with("workarea.relate_") {
        return None;
    }
    let named = payload
        .get("workarea_id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok());
    if event_type.starts_with("workarea.") {
        return Some(JudgedSubject {
            claim: false,
            node: named?,
            author_key: None,
            promoted: false,
            self_signed: false,
            outside_audience: false,
        });
    }
    let submission = event_type.starts_with("submission.") || event_type == "simulation.write";
    if !submission && !event_type.starts_with("lifecycle.") {
        return None;
    }
    let promoted = matches!(
        event_type,
        event_types::LIFECYCLE_PROMOTE | event_types::LIFECYCLE_SUPERSEDE
    );
    Some(JudgedSubject {
        claim: true,
        node: named.or(subject.map(|s| s.workarea_id))?,
        author_key: if submission {
            envelope.api_key_id
        } else {
            subject.map(|s| s.author_key_id)
        },
        promoted,
        self_signed: payload.get("self_signed") != Some(&Value::Bool(false)),
        outside_audience: payload.get("simulation_id").is_some_and(|s| !s.is_null())
            || payload.get("provenance").and_then(Value::as_str) == Some("simulation"),
    })
}

/// The payload as the tail serves it (http-api.md#events-tail, what a
/// served event names): to a key outside the actor's own key set every
/// field naming a key — `proposer_key_id`, `ratifier_key_id` and each
/// `*_by_key_id` — is null, and a claim subject's `workarea_id` on a node
/// where the key holds no role is null (roles-and-keys.md#own, naming a
/// node).
fn served_payload(envelope: &Envelope, claim: bool, names_actor: bool, names_node: bool) -> Value {
    let mut payload = envelope.payload.clone();
    if let Some(fields) = payload.as_object_mut() {
        for (field, value) in fields.iter_mut() {
            let names_key = field == "proposer_key_id"
                || field == "ratifier_key_id"
                || field.ends_with("_by_key_id");
            let withheld_node = claim && field == "workarea_id" && !names_node;
            if (names_key && !names_actor) || withheld_node {
                *value = Value::Null;
            }
        }
    }
    payload
}
