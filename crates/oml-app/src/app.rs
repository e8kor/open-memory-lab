//! The use-cases: every verb the transports will front, enforcing the
//! constitution here — verb ceilings (R1), reviewer-never-author (R2),
//! promoted-only reads (R3) — never in an adapter (ADR-0019 d1).

use std::collections::BTreeSet;

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::{
    ChainHead, EventDraft, KeySet, MemoryStatus, Role, Sha256Hash, SignedEnvelope, check_reviewer,
    event_types, next_envelope, sha256_digest_string, transition,
};

use crate::ports::{
    AdmissionEnvelope, Clock, CommitReceipt, CommitRequest, IdGen, IdempotencyPut,
    IdempotencyRecord, IdempotencyScope, JournalStore, MetadataFold, OutboxRow, ScanOutcome,
    Scanner, Signer,
};
use crate::types::{
    AppError, BootstrapRequest, BootstrapResult, IssueKeyRequest, IssuedKey, KeyRecord,
    MemoryRecord, RecallRequest, RecallResult, SubmitOutcome, SubmitReceipt, SubmitRequest,
    SupersedeOutcome, SupersedeRequest, Verdict, VerdictOutcome, WorkareaRecord,
};

const EVENTS_PAGE: usize = 512;

/// The application, generic over its driven ports.
pub struct App<J, S, C, I, Sc> {
    journal: J,
    signer: S,
    clock: C,
    ids: I,
    scanner: Sc,
}

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

fn submit_request_digest(req: &SubmitRequest, workarea_id: Uuid) -> String {
    let doc = json!({
        "body": req.body,
        "claim_key": req.claim_key,
        "derives_from": req.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "workarea_id": workarea_id.to_string(),
    });
    sha256_digest_string(doc.to_string().as_bytes())
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

    async fn authenticate(&self, api_key: &str) -> Result<KeyRecord, AppError> {
        let hash = sha256_digest_string(api_key.as_bytes());
        self.journal
            .key_by_hash(&hash)
            .await?
            .ok_or(AppError::Unauthorized)
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

    async fn commit(
        &self,
        event: SignedEnvelope,
        fold: MetadataFold,
        idempotency: Option<IdempotencyPut>,
    ) -> Result<CommitReceipt, AppError> {
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
        Ok(self
            .journal
            .commit(CommitRequest {
                event,
                fold,
                outbox,
                idempotency,
            })
            .await?)
    }

    fn mint_key(
        &self,
        tenant_id: Uuid,
        req: &IssueKeyRequest,
        default_workarea: Option<Uuid>,
    ) -> (KeyRecord, String) {
        let secret = self.ids.new_secret();
        let value = format!("oml_{}", base62_43(&secret));
        let record = KeyRecord {
            key_id: self.ids.new_id(),
            tenant_id,
            role: req.role,
            principal: req.principal.clone(),
            held_by: req.held_by,
            default_workarea,
            key_hash: sha256_digest_string(value.as_bytes()),
        };
        (record, value)
    }

    fn key_issued_payload(record: &KeyRecord) -> Value {
        // Issued-without-value: key id, role, principal — never the value
        // (roles-and-keys.md show-once rule).
        json!({
            "key_id": record.key_id.to_string(),
            "role": record.role.as_str(),
            "principal": record.principal,
            "held_by": record.held_by.as_str(),
            "value_disclosed": false,
        })
    }

    // -- use-cases ----------------------------------------------------------

    /// The operator ceremony: genesis event, root workarea, first admin key
    /// (roles-and-keys.md bootstrap). Unauthenticated by design — the
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
            json!({ "tenant_name": req.tenant_name }),
        )?;
        let mut prev = genesis.clone();
        self.commit(genesis, MetadataFold::TenantGenesis, None)
            .await?;

        let root_workarea_id = self.ids.new_id();
        let workarea = WorkareaRecord {
            workarea_id: root_workarea_id,
            tenant_id,
            parent_id: None,
            name: "root".to_owned(),
        };
        let event = self.seal(
            Some(&prev),
            tenant_id,
            event_types::WORKAREA_CREATE,
            "operator",
            None,
            json!({
                "workarea_id": root_workarea_id.to_string(),
                "name": workarea.name,
                "parent_id": Value::Null,
            }),
        )?;
        prev = event.clone();
        self.commit(event, MetadataFold::WorkareaCreated(workarea), None)
            .await?;

        let issue = IssueKeyRequest {
            role: Role::Admin,
            principal: req.admin_principal.clone(),
            held_by: oml_domain::HeldBy::Human,
            default_workarea: Some(root_workarea_id),
        };
        let (record, value) = self.mint_key(tenant_id, &issue, Some(root_workarea_id));
        let key_id = record.key_id;
        let event = self.seal(
            Some(&prev),
            tenant_id,
            event_types::KEY_ISSUED,
            "operator",
            None,
            Self::key_issued_payload(&record),
        )?;
        self.commit(event, MetadataFold::KeyIssued(record), None)
            .await?;

        Ok(BootstrapResult {
            tenant_id,
            root_workarea_id,
            admin_key: IssuedKey { key_id, value },
        })
    }

    /// Issue a key under the issuer ceiling: only admin (and operator) keys
    /// issue, and never a role above their own (roles-and-keys.md).
    pub async fn issue_key(
        &self,
        api_key: &str,
        req: &IssueKeyRequest,
    ) -> Result<IssuedKey, AppError> {
        let issuer = self.authenticate(api_key).await?;
        if issuer.role != Role::Admin && issuer.role != Role::Operator {
            return Err(AppError::VerbCeiling {
                role: issuer.role,
                verb: "issue_key",
            });
        }
        if issuer.role == Role::Admin && req.role == Role::Operator {
            return Err(AppError::IssuerCeiling {
                issuer: issuer.role,
                requested: req.role,
            });
        }
        let default_workarea = req.default_workarea.or(issuer.default_workarea);
        let (record, value) = self.mint_key(issuer.tenant_id, req, default_workarea);
        let key_id = record.key_id;
        let prev = self.head_event(issuer.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            issuer.tenant_id,
            event_types::KEY_ISSUED,
            &issuer.principal,
            Some(issuer.key_id),
            Self::key_issued_payload(&record),
        )?;
        self.commit(event, MetadataFold::KeyIssued(record), None)
            .await?;
        Ok(IssuedKey { key_id, value })
    }

    pub async fn create_workarea(
        &self,
        api_key: &str,
        name: &str,
        parent_id: Option<Uuid>,
    ) -> Result<WorkareaRecord, AppError> {
        let key = self.authenticate(api_key).await?;
        require_role(&key, Role::Admin, "create_workarea")?;
        let workarea = WorkareaRecord {
            workarea_id: self.ids.new_id(),
            tenant_id: key.tenant_id,
            parent_id,
            name: name.to_owned(),
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
                "name": workarea.name,
                "parent_id": parent_id.map(|p| p.to_string()),
            }),
        )?;
        self.commit(event, MetadataFold::WorkareaCreated(workarea.clone()), None)
            .await?;
        Ok(workarea)
    }

    /// Submit a memory: writer ceiling, same-call deterministic scan, held
    /// on arrival, idempotency record honored (admission.md, http-api.md).
    pub async fn submit_memory(
        &self,
        api_key: &str,
        req: &SubmitRequest,
    ) -> Result<SubmitOutcome, AppError> {
        let key = self.authenticate(api_key).await?;
        require_role(&key, Role::Writer, "submit")?;
        let workarea_id = req
            .workarea_id
            .or(key.default_workarea)
            .ok_or(AppError::WorkareaUnresolved)?;

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

        // Deterministic scan: same call, before the commit transaction
        // (admission.md). A veto is a journalled rejection, not a drop.
        let scan = self.scanner.scan(&AdmissionEnvelope {
            tenant_id: key.tenant_id,
            kind: "memory",
            body: &req.body,
        });
        let (status, scan_finding) = match &scan {
            ScanOutcome::Pass => (MemoryStatus::Held, None),
            ScanOutcome::Hold(reason) => (MemoryStatus::Held, Some(reason.clone())),
            ScanOutcome::Veto(reason) => (MemoryStatus::Rejected, Some(reason.clone())),
        };
        let vetoed = status == MemoryStatus::Rejected;

        // The lineage closure: the author plus every declared ancestor's
        // closure — R2 runs over the whole of it (roles-and-keys.md).
        let mut principals = BTreeSet::from([key.principal.clone()]);
        let mut key_ids = BTreeSet::from([key.key_id]);
        for ancestor in &req.derives_from {
            let m = self
                .journal
                .memory(key.tenant_id, *ancestor)
                .await?
                .ok_or(AppError::LineageUnknown(*ancestor))?;
            principals.extend(m.lineage_principals);
            key_ids.extend(m.lineage_key_ids);
        }

        let memory_id = self.ids.new_id();
        let body_digest = sha256_digest_string(req.body.as_bytes());
        let memory = MemoryRecord {
            memory_id,
            tenant_id: key.tenant_id,
            workarea_id,
            claim_key: req.claim_key.clone(),
            body: req.body.clone(),
            body_digest: body_digest.clone(),
            status,
            author_principal: key.principal.clone(),
            author_key_id: key.key_id,
            derives_from: req.derives_from.clone(),
            lineage_principals: principals.into_iter().collect(),
            lineage_key_ids: key_ids.into_iter().collect(),
            superseded_by: None,
            scan_finding: scan_finding.clone(),
            created_at: self.clock.now(),
        };

        let prev = self.head_event(key.tenant_id).await?;
        let payload = json!({
            "artifact_id": memory_id.to_string(),
            "workarea_id": workarea_id.to_string(),
            "claim_key": req.claim_key,
            // Digest-only payload: body plaintext never rides the journal
            // (ADR-0002 d1, admission.md capture).
            "body_digest": body_digest,
            "derives_from": req.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "scan": {
                "result": if vetoed { "veto" } else if scan_finding.is_some() { "hold" } else { "pass" },
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
            workarea_id,
            state: status.as_str().to_owned(),
            journal_seq: seq,
            body_digest,
        };
        // A vetoed submission is journalled as rejected but stores no
        // idempotency record: the caller gets the typed error (PoC choice).
        let idempotency = if vetoed {
            None
        } else {
            Some(IdempotencyPut {
                scope,
                record: IdempotencyRecord {
                    request_digest,
                    response: serde_json::to_value(&receipt)
                        .map_err(|e| AppError::StoredResponse(e.to_string()))?,
                },
            })
        };
        self.commit(
            event,
            MetadataFold::MemoryHeld {
                memory,
                enqueue: !vetoed,
            },
            idempotency,
        )
        .await?;

        if let ScanOutcome::Veto(reason) = scan {
            return Err(AppError::ScanVeto(reason));
        }
        Ok(SubmitOutcome {
            receipt,
            replayed: false,
        })
    }

    /// Serve the next reviewable held item to this dreamer, claiming it.
    pub async fn review_next(&self, api_key: &str) -> Result<Option<MemoryRecord>, AppError> {
        let key = self.authenticate(api_key).await?;
        require_role(&key, Role::Dreamer, "review")?;
        Ok(self
            .journal
            .review_claim_next(key.tenant_id, &key.principal, key.key_id)
            .await?)
    }

    /// Record a verdict. R2 runs here against the row's lineage closure
    /// (`check_reviewer`); with the PoC policy k = 1, an approval ratifies
    /// immediately — a separate journalled `lifecycle.promote` carrying the
    /// policy snapshot (admission.md review and ratification).
    pub async fn post_verdict(
        &self,
        api_key: &str,
        memory_id: Uuid,
        verdict: Verdict,
        reason: Option<&str>,
    ) -> Result<VerdictOutcome, AppError> {
        let key = self.authenticate(api_key).await?;
        require_role(&key, Role::Dreamer, "verdict")?;
        let memory = self
            .journal
            .memory(key.tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        check_reviewer(key.role, &key.principal, key.key_id, &memory.lineage_closure())?;

        let (verdict_type, target_status) = match verdict {
            Verdict::Approve => (event_types::REVIEW_APPROVE, MemoryStatus::Promoted),
            Verdict::Reject => (event_types::REVIEW_REJECT, MemoryStatus::Rejected),
        };
        // The pure transition function is the arbiter: anything but a held
        // row refuses here (admission.md lifecycle).
        let new_status = transition(memory.status, target_status)?;

        let prev = self.head_event(key.tenant_id).await?;
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
                    state: new_status,
                    journal_seq: receipt.seq,
                })
            }
            Verdict::Approve => {
                let verdict_signed = verdict_event.clone();
                self.commit(
                    verdict_event,
                    MetadataFold::VerdictRecorded {
                        memory_id,
                        reviewer_key_id: key.key_id,
                    },
                    None,
                )
                .await?;
                // Ratification: the promotion event snapshots the policy it
                // was decided under (admission.md).
                let promote = self.seal(
                    Some(&verdict_signed),
                    key.tenant_id,
                    event_types::LIFECYCLE_PROMOTE,
                    "oml-server",
                    None,
                    json!({
                        "artifact_id": memory_id.to_string(),
                        "policy": { "k": 1, "required_roles": ["dreamer"] },
                        "verdict_event_id": verdict_event_id.to_string(),
                    }),
                )?;
                let receipt = self
                    .commit(promote, MetadataFold::MemoryPromoted { memory_id }, None)
                    .await?;
                Ok(VerdictOutcome {
                    memory_id,
                    state: new_status,
                    journal_seq: receipt.seq,
                })
            }
        }
    }

    /// Recall: reader ceiling, promoted rows only, steering segment ahead of
    /// hits (retrieval.md; the PoC serves FTS hits with an empty steering
    /// segment).
    pub async fn recall(
        &self,
        api_key: &str,
        req: &RecallRequest,
    ) -> Result<RecallResult, AppError> {
        let key = self.authenticate(api_key).await?;
        require_role(&key, Role::Reader, "recall")?;
        let workarea = req.workarea_id.or(key.default_workarea);
        let hits = self
            .journal
            .recall_fts(key.tenant_id, workarea, &req.query, req.k)
            .await?
            .into_iter()
            // R3 defense in depth: the gate changed visibility; nothing
            // unpromoted leaves the application layer.
            .filter(|m| m.status == MemoryStatus::Promoted)
            .collect();
        Ok(RecallResult {
            steering: Vec::new(),
            hits,
        })
    }

    /// Get one memory: the author sees its own rows in every state; everyone
    /// else sees promoted (and superseded) rows under the reader ceiling,
    /// and absence is indistinguishable from no-access (http-api.md).
    pub async fn get_memory(
        &self,
        api_key: &str,
        memory_id: Uuid,
    ) -> Result<MemoryRecord, AppError> {
        let key = self.authenticate(api_key).await?;
        let memory = self
            .journal
            .memory(key.tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if memory.author_principal == key.principal {
            return Ok(memory);
        }
        match memory.status {
            MemoryStatus::Promoted | MemoryStatus::Superseded => {
                require_role(&key, Role::Reader, "get")
                    .map_err(|_| AppError::NotFound)
                    .map(|()| memory)
            }
            MemoryStatus::Held | MemoryStatus::Rejected => Err(AppError::NotFound),
        }
    }

    /// Supersede the caller's own promoted claim: one journalled
    /// `lifecycle.supersede` transitions the target and admits the successor
    /// as a fresh held row queued for review.
    pub async fn supersede(
        &self,
        api_key: &str,
        req: &SupersedeRequest,
    ) -> Result<SupersedeOutcome, AppError> {
        let key = self.authenticate(api_key).await?;
        require_role(&key, Role::Writer, "supersede")?;
        let target = self
            .journal
            .memory(key.tenant_id, req.target_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if target.author_principal != key.principal {
            return Err(AppError::AuthorOnly);
        }
        if target.superseded_by.is_some() {
            return Err(AppError::TipConflict);
        }
        // Promoted -> Superseded is the only lawful source state.
        transition(target.status, MemoryStatus::Superseded)?;

        // The successor is a new submission: scanned like any write.
        if let ScanOutcome::Veto(reason) = self.scanner.scan(&AdmissionEnvelope {
            tenant_id: key.tenant_id,
            kind: "memory",
            body: &req.body,
        }) {
            return Err(AppError::ScanVeto(reason));
        }

        let successor_id = self.ids.new_id();
        let body_digest = sha256_digest_string(req.body.as_bytes());
        let mut lineage_principals: BTreeSet<String> =
            target.lineage_principals.iter().cloned().collect();
        lineage_principals.insert(key.principal.clone());
        let mut lineage_key_ids: BTreeSet<Uuid> = target.lineage_key_ids.iter().copied().collect();
        lineage_key_ids.insert(key.key_id);
        let successor = MemoryRecord {
            memory_id: successor_id,
            tenant_id: key.tenant_id,
            workarea_id: target.workarea_id,
            claim_key: req.claim_key.clone().or_else(|| target.claim_key.clone()),
            body: req.body.clone(),
            body_digest: body_digest.clone(),
            status: MemoryStatus::Held,
            author_principal: key.principal.clone(),
            author_key_id: key.key_id,
            derives_from: vec![req.target_id],
            lineage_principals: lineage_principals.into_iter().collect(),
            lineage_key_ids: lineage_key_ids.into_iter().collect(),
            superseded_by: None,
            scan_finding: None,
            created_at: self.clock.now(),
        };

        let prev = self.head_event(key.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::LIFECYCLE_SUPERSEDE,
            &key.principal,
            Some(key.key_id),
            json!({
                "artifact_id": req.target_id.to_string(),
                "successor_id": successor_id.to_string(),
                "successor_body_digest": body_digest,
            }),
        )?;
        let receipt = self
            .commit(
                event,
                MetadataFold::MemorySuperseded {
                    superseded: req.target_id,
                    successor,
                },
                None,
            )
            .await?;
        Ok(SupersedeOutcome {
            superseded_id: req.target_id,
            successor_id,
            journal_seq: receipt.seq,
        })
    }

    /// Verify the tenant chain end to end with the domain procedure
    /// (chain-format.md steps 1–7 and 9). Admin ceiling (roles-and-keys.md:
    /// chain verification is an admin verb).
    pub async fn verify_chain(&self, api_key: &str) -> Result<ChainHead, AppError> {
        let key = self.authenticate(api_key).await?;
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

    /// The events tail: reader ceiling, `seq` ascending from `after_seq`
    /// (http-api.md events tail; payloads are digest-only by construction).
    pub async fn tail_events(
        &self,
        api_key: &str,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<SignedEnvelope>, AppError> {
        let key = self.authenticate(api_key).await?;
        require_role(&key, Role::Reader, "events_tail")?;
        Ok(self
            .journal
            .events_range(key.tenant_id, after_seq + 1, limit)
            .await?)
    }
}
