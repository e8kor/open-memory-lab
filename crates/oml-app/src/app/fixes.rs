//! The two lifecycle primitives an admin fix reaches (ADR-0089 d16): a
//! demotion, which stops serving a promoted version and returns the tip to
//! the latest earlier promoted, non-demoted version of its claim, keeping
//! every byte (ADR-0009 d5; R4); and an erasure, which takes every version
//! of a claim, empties its body and what derives from it, and destroys its
//! data keys (ADR-0005 d2, amended 2026-11-17).
//!
//! No transport routes either: the one surface is the fix act,
//! `POST /v2/admin/fixes` (ADR-0089 d15, d16), which journals `admin.fix`
//! and calls these in the same transaction. Until it lands they are reached
//! in process alone, by an admin key.

use serde_json::{Value, json};
use uuid::Uuid;

use std::collections::BTreeSet;

use oml_domain::name::{Name, NamedKind};
use oml_domain::origin::OriginKind;
use oml_domain::valid_time::{ValidEnd, ValidTime, ratified_end};
use oml_domain::{
    MemoryKind, MemoryStatus, Role, SignedEnvelope, UsedBy, VerdictError, check_approval_class,
    check_reviewer, event_types, is_person_reviewer, sha256_digest_string, transition,
};

use super::{
    App, claim_review_policy, commit_request, guardrail_fields, guardrail_payload, kind_fields,
    nfc, require_role, valid_time, with_format_fields, with_kind_fields, with_valid_time,
};
use crate::ports::{
    AdmissionEnvelope, Clock, CommitRequest, IdGen, JournalStore, MetadataFold, ScanOutcome,
    Scanner, Signer, StoreError, WebStore,
};
use crate::types::{
    AppError, Approval, Bearer, Credential, FixAct, FixRecord, FixState, FormatFields,
    GuardrailFields, KeyRecord, KindFields, MemoryRecord, VersionRef,
};
use oml_domain::access::effective_roles;
use oml_domain::guardrail::GuardrailScope;
use oml_domain::ladder::{FrozenOp, WorkspaceStatus};

/// What the copies an erasure reaches are called in its event, as ADR-0005
/// d4's inventory names them.
const ERASE_COPY_CLASSES: [&str; 5] = [
    "postgres.body",
    "fts",
    "document_passages",
    "telemetry",
    "silver.body",
];

/// A demotion as committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Demotion {
    pub demoted: VersionRef,
    /// The version served as the tip again, when the demoted one was the
    /// tip and a promoted, non-demoted version lies below it.
    pub restored: Option<VersionRef>,
    pub journal_seq: u64,
}

/// A demotion read and sealed, not yet committed.
struct PlannedDemotion {
    event: SignedEnvelope,
    fold: MetadataFold,
    demotion: Demotion,
    memory: MemoryRecord,
    restored: Option<MemoryRecord>,
}

/// An erasure as committed: every version of the claim, by version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Erasure {
    pub erased: Vec<VersionRef>,
    pub journal_seq: u64,
}

fn version_ref(m: &MemoryRecord) -> VersionRef {
    VersionRef {
        artifact_id: m.memory_id,
        version: m.version,
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
    /// Demotes `memory_id`, any promoted version (ADR-0089 d16): it is
    /// served by no read at any instant, and when it was the tip the latest
    /// earlier promoted, non-demoted version of its claim is the tip again,
    /// or none is. Journals `lifecycle.demote` under the admin's key. A
    /// version that is not promoted is [`AppError::Transition`]; a
    /// retirement or a `term`, `term_mapping` or `vocab_release` version is
    /// [`AppError::DemoteKindUnsupported`]; one the
    /// tenant does not hold is [`AppError::NotFound`]; a version or claim
    /// that changed since it was read is [`crate::ports::StoreError::TipMoved`].
    ///
    /// # Errors
    /// The refusals above, a key that is not an admin's, and the store's.
    pub async fn demote_version(
        &self,
        bearer: impl Into<Bearer<'_>>,
        memory_id: Uuid,
    ) -> Result<Demotion, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "demote")?;
        let planned = self.plan_demotion(&admin, memory_id, None).await?;
        self.commit_requests(vec![commit_request(planned.event, planned.fold, None)])
            .await?;
        Ok(planned.demotion)
    }

    /// A demotion of `memory_id` read and sealed, not committed:
    /// [`Self::demote_version`] commits it alone, sealed on the head read
    /// after its reads (`prev` `None`), and the fix act commits it with its
    /// `admin.fix`, sealed on the head it read first (ADR-0089 d16).
    async fn plan_demotion(
        &self,
        admin: &KeyRecord,
        memory_id: Uuid,
        prev: Option<Option<SignedEnvelope>>,
    ) -> Result<PlannedDemotion, AppError> {
        let tenant_id = admin.tenant_id;
        let memory = self
            .journal
            .memory(tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        transition(memory.status, MemoryStatus::Demoted)?;
        // A retirement and a vocabulary version are not demoted: each needs
        // a reading of its own (ADR-0089 d16, note of 2026-11-18).
        if memory.retirement
            || matches!(
                memory.kind,
                MemoryKind::Term | MemoryKind::TermMapping | MemoryKind::VocabRelease
            )
        {
            return Err(AppError::DemoteKindUnsupported {
                kind: memory.kind,
                retirement: memory.retirement,
            });
        }
        let versions = self
            .journal
            .claim_versions(tenant_id, memory.workspace_id, &memory.claim_key)
            .await?;
        let restored = if memory.is_tip {
            versions
                .iter()
                .filter(|v| v.version < memory.version && v.status == MemoryStatus::Promoted)
                .max_by_key(|v| v.version)
                .cloned()
        } else {
            None
        };
        // The version it replaced gets back the end it had before this
        // version's ratification; the event names both, so a rebuild reads
        // them rather than infers them (ADR-0089 d16, note of 2026-11-18).
        let reopened = match memory.replaces {
            Some(r)
                if self
                    .journal
                    .memory(tenant_id, r)
                    .await?
                    .is_some_and(|m| m.status == MemoryStatus::Promoted) =>
            {
                self.journal
                    .valid_until_before(tenant_id, r)
                    .await?
                    .map(|before| (r, before))
            }
            _ => None,
        };
        let prev = match prev {
            Some(prev) => prev,
            None => self.head_event(tenant_id).await?,
        };
        let event = self.demote_event(
            prev.as_ref(),
            admin,
            &memory,
            restored.as_ref(),
            reopened.as_ref(),
        )?;
        let demotion = Demotion {
            demoted: version_ref(&memory),
            restored: restored.as_ref().map(version_ref),
            journal_seq: event.envelope.seq,
        };
        let fold = MetadataFold::MemoryDemoted {
            memory_id,
            restored: restored.as_ref().map(|r| r.memory_id),
            reopened,
        };
        Ok(PlannedDemotion {
            event,
            fold,
            demotion,
            memory,
            restored,
        })
    }

    /// Erases the claim `memory_id` is a version of: every version of its
    /// `claim_key` in its workspace, whatever its state (ADR-0089 d16;
    /// ADR-0005 d2, amended 2026-11-17). One `lifecycle.erase` names them
    /// all, and in its transaction each row's body and what derives from it
    /// are emptied, its passages and telemetry erased and its data key
    /// destroyed; the digest, title, attribution and lineage stay. It
    /// commits through `commit_requests`, so the tenant's widen
    /// signals drop with it (ADR-0005 d4). A claim already erased is
    /// [`AppError::Transition`]; a version the tenant does not hold is
    /// [`AppError::NotFound`]; a claim that gained a version since it was
    /// read is [`crate::ports::StoreError::TipMoved`].
    ///
    /// # Errors
    /// The refusals above, a key that is not an admin's, and the store's.
    pub async fn erase_claim(
        &self,
        bearer: impl Into<Bearer<'_>>,
        memory_id: Uuid,
    ) -> Result<Erasure, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "erase")?;
        let (event, fold, erasure, _) = self.plan_erasure(&admin, memory_id, None).await?;
        self.commit_requests(vec![commit_request(event, fold, None)])
            .await?;
        Ok(erasure)
    }

    /// An erasure of the claim `memory_id` is a version of, read and sealed,
    /// not committed; `prev` as [`Self::plan_demotion`]'s (ADR-0089 d16).
    async fn plan_erasure(
        &self,
        admin: &KeyRecord,
        memory_id: Uuid,
        prev: Option<Option<SignedEnvelope>>,
    ) -> Result<(SignedEnvelope, MetadataFold, Erasure, MemoryRecord), AppError> {
        let tenant_id = admin.tenant_id;
        let named = self
            .journal
            .memory(tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let versions = self
            .journal
            .claim_versions(tenant_id, named.workspace_id, &named.claim_key)
            .await?;
        // An erase takes every version, so a claim is erased once.
        transition(named.status, MemoryStatus::Erased)?;
        let prev = match prev {
            Some(prev) => prev,
            None => self.head_event(tenant_id).await?,
        };
        let event = self.erase_event(prev.as_ref(), admin, &named, &versions)?;
        let erasure = Erasure {
            erased: versions.iter().map(version_ref).collect(),
            journal_seq: event.envelope.seq,
        };
        let fold = MetadataFold::ClaimErased {
            memory_ids: versions.iter().map(|v| v.memory_id).collect(),
        };
        Ok((event, fold, erasure, named))
    }

    /// `lifecycle.demote` of `memory`, returning `restored` (ADR-0089 d16;
    /// event-catalog.md), sealed on `prev` under the admin's key, so the
    /// fix act can chain it after its `admin.fix`.
    fn demote_event(
        &self,
        prev: Option<&SignedEnvelope>,
        admin: &KeyRecord,
        memory: &MemoryRecord,
        restored: Option<&MemoryRecord>,
        reopened: Option<&(Uuid, Option<ValidEnd>)>,
    ) -> Result<SignedEnvelope, AppError> {
        self.seal(
            prev,
            admin.tenant_id,
            event_types::LIFECYCLE_DEMOTE,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "artifact_id": memory.memory_id.to_string(),
                "version": memory.version,
                "workspace_id": memory.workspace_id.to_string(),
                "claim_key": memory.claim_key,
                "restored_artifact_id": restored.map(|r| r.memory_id.to_string()),
                "restored_version": restored.map(|r| r.version),
                "reopened_artifact_id": reopened.map(|(id, _)| id.to_string()),
                "restored_valid_until": reopened.and_then(|(_, end)| end.as_ref().map(ValidEnd::as_str)),
                "demoted_by_key_id": admin.key_id.to_string(),
            }),
        )
    }

    /// `lifecycle.erase` naming `named` and every version of its claim
    /// (ADR-0089 d16; event-catalog.md), sealed on `prev` under the admin's
    /// key.
    fn erase_event(
        &self,
        prev: Option<&SignedEnvelope>,
        admin: &KeyRecord,
        named: &MemoryRecord,
        versions: &[MemoryRecord],
    ) -> Result<SignedEnvelope, AppError> {
        self.seal(
            prev,
            admin.tenant_id,
            event_types::LIFECYCLE_ERASE,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "artifact_id": named.memory_id.to_string(),
                "version": named.version,
                "workspace_id": named.workspace_id.to_string(),
                "claim_key": named.claim_key,
                "artifact_ids": versions
                    .iter()
                    .map(|v| Value::from(v.memory_id.to_string()))
                    .collect::<Vec<_>>(),
                "dek_destroyed": true,
                "copy_classes": ERASE_COPY_CLASSES,
                "erased_by_key_id": admin.key_id.to_string(),
            }),
        )
    }
}

/// The longest a fix's reason is, in bytes, as an investigation's is.
pub const FIX_REASON_MAX_BYTES: usize = 1024;

/// The principal of the reserved per-tenant server key a revert's restored
/// version is authored by (ADR-0089 d8, d17 c5).
pub const SERVER_KEY_PRINCIPAL: &str = "oml:server";

/// An admin's fix request (ADR-0089 d2, d15): the act, the version it
/// names, the reason, and for an edit the new body and title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixRequest {
    pub act: FixAct,
    pub target: Uuid,
    pub reason: String,
    /// An edit's body; refused on every other act.
    pub body: Option<String>,
    /// An edit's title, where its format takes one; refused on every
    /// other act.
    pub title: Option<String>,
}

/// A fix as a person's reviewer key reads it (ADR-0089 d15): never the
/// admin's key, principal or user, and whether this key may confirm or
/// revert it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewedFix {
    pub fix: FixRecord,
    /// Whether this key may confirm it: false when it acts for the
    /// admin's own person and the strict rule holds (ADR-0089, amendment
    /// of 2026-11-18; #490).
    pub settles: bool,
}

/// Whether an instruction-class version leaves service or changes: what
/// sets `loosens_guardrail` (ADR-0089 d6, d17 c7).
fn instruction(m: &MemoryRecord) -> bool {
    m.kind.class().needs_person()
}

/// The reserved server key's id in `tenant_id`: one per tenant, derived
/// from the tenant's id so every replica finds the same one.
fn server_key_id(tenant_id: Uuid) -> Uuid {
    use sha2::{Digest, Sha256};
    let digest = Sha256::new()
        .chain_update(b"oml:server-key\0")
        .chain_update(tenant_id.as_bytes())
        .finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // RFC 9562 version 8 (custom) and the RFC variant.
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/admin/fixes {act, target, reason}`: an admin key used by a
    /// person takes a marked break-glass fix that serves at once (ADR-0089
    /// d1-d4, d16). One commit journals the lifecycle event the act causes
    /// and then `admin.fix` naming it. Refusals, before anything is
    /// journalled: an admin key not used by a person,
    /// [`AppError::FixRequiresPerson`]; a reason empty or over
    /// [`FIX_REASON_MAX_BYTES`], or a body or title on an act that takes
    /// none, [`AppError::AdminField`]; a reason the scan vetoes; a demotion
    /// of a version that is not approved, [`AppError::FixTargetNotPromoted`];
    /// an edit or retirement of a
    /// version that is not the served tip, [`AppError::TipConflict`], or in
    /// a frozen workspace, [`AppError::WorkspaceFrozen`]; an edit whose body
    /// the scan vetoes or would hold, [`AppError::ScanVeto`] or
    /// [`AppError::FixScanHold`].
    ///
    /// # Errors
    /// The refusals above, a key that is not an admin's, and the store's.
    pub async fn admin_fix(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &FixRequest,
    ) -> Result<FixRecord, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "fix")?;
        if admin.used_by != UsedBy::Person {
            return Err(AppError::FixRequiresPerson {
                used_by: admin.used_by,
            });
        }
        let reason = req.reason.trim();
        if reason.is_empty() || reason.len() > FIX_REASON_MAX_BYTES {
            return Err(AppError::AdminField {
                field: "/reason",
                message: format!("a fix's reason is 1 to {FIX_REASON_MAX_BYTES} bytes"),
            });
        }
        if req.act != FixAct::Edit {
            if req.body.is_some() {
                return Err(AppError::AdminField {
                    field: "/body",
                    message: "only an edit carries a body".to_owned(),
                });
            }
            if req.title.is_some() {
                return Err(AppError::AdminField {
                    field: "/title",
                    message: "only an edit carries a title".to_owned(),
                });
            }
        }
        self.scan_veto_only(admin.tenant_id, "reason", reason)?;
        let fix_id = self.ids.new_id();
        let prev = self.head_event(admin.tenant_id).await?;
        let (links, fix) = match req.act {
            FixAct::Demote => {
                self.demote_fix(&admin, req.target, prev.as_ref(), fix_id, reason)
                    .await?
            }
            FixAct::Erase => {
                let (event, fold, _, named) = self
                    .plan_erasure(&admin, req.target, Some(prev.clone()))
                    .await?;
                let fix = FixRecord {
                    fix_id,
                    act: FixAct::Erase,
                    target_id: named.memory_id,
                    workspace_id: named.workspace_id,
                    claim_key: named.claim_key.clone(),
                    replaced_id: Some(named.memory_id),
                    served_id: None,
                    reason: reason.to_owned(),
                    loosens_guardrail: instruction(&named),
                    admin_key_id: admin.key_id,
                    lifecycle_event_id: event.envelope.event_id,
                    state: FixState::Unconfirmed,
                    fixed_at: event.envelope.at.clone(),
                    settled_by_key_id: None,
                    settled_same_user: None,
                    settled_at: None,
                    restored_id: None,
                };
                (vec![commit_request(event, fold, None)], fix)
            }
            FixAct::Edit | FixAct::Retire => {
                self.rewrite_fix(&admin, req, prev.as_ref(), fix_id, reason)
                    .await?
            }
        };
        let last = &links.last().expect("a fix's lifecycle event").event;
        let fix_event = self.seal(
            Some(last),
            admin.tenant_id,
            event_types::ADMIN_FIX,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "fix_id": fix.fix_id.to_string(),
                "act": fix.act.as_str(),
                "targets": [fix.target_id.to_string()],
                "workspace_id": fix.workspace_id.to_string(),
                "claim_key": fix.claim_key,
                "replaced_artifact_id": fix.replaced_id.map(|id| id.to_string()),
                "served_artifact_id": fix.served_id.map(|id| id.to_string()),
                "reason": fix.reason,
                "loosens_guardrail": fix.loosens_guardrail,
                "lifecycle_event_id": fix.lifecycle_event_id.to_string(),
            }),
        )?;
        let fix = FixRecord {
            fixed_at: fix_event.envelope.at.clone(),
            ..fix
        };
        let mut links = links;
        links.push(commit_request(
            fix_event,
            MetadataFold::AdminFixRecorded(Box::new(fix.clone())),
            None,
        ));
        match self.commit_requests(links).await {
            Err(StoreError::TipMoved { .. }) => {
                return Err(AppError::TipConflict {
                    current_tip: self
                        .journal
                        .claim_tip(admin.tenant_id, fix.workspace_id, &fix.claim_key)
                        .await?
                        .as_ref()
                        .map(version_ref),
                });
            }
            other => other?,
        };
        self.observer
            .admin_fix(fix.act.as_str(), fix.loosens_guardrail);
        Ok(fix)
    }

    /// A demotion as a fix: only an approved version (ADR-0089 d16). The
    /// claim's last one is demoted too: its key is then released and a new
    /// claim continues its numbers (O8 of 2026-11-19).
    async fn demote_fix(
        &self,
        admin: &KeyRecord,
        target: Uuid,
        prev: Option<&SignedEnvelope>,
        fix_id: Uuid,
        reason: &str,
    ) -> Result<(Vec<CommitRequest>, FixRecord), AppError> {
        let memory = self
            .journal
            .memory(admin.tenant_id, target)
            .await?
            .ok_or(AppError::NotFound)?;
        if memory.status != MemoryStatus::Promoted {
            return Err(AppError::FixTargetNotPromoted {
                status: memory.status,
            });
        }
        let planned = self
            .plan_demotion(admin, target, Some(prev.cloned()))
            .await?;
        let m = &planned.memory;
        let fix = FixRecord {
            fix_id,
            act: FixAct::Demote,
            target_id: m.memory_id,
            workspace_id: m.workspace_id,
            claim_key: m.claim_key.clone(),
            replaced_id: Some(m.memory_id),
            served_id: planned.restored.as_ref().map(|r| r.memory_id),
            reason: reason.to_owned(),
            // A served instruction-class tip taken out of service
            // (ADR-0089 d17 c7).
            loosens_guardrail: m.is_tip && instruction(m),
            admin_key_id: admin.key_id,
            lifecycle_event_id: planned.event.envelope.event_id,
            state: FixState::Unconfirmed,
            fixed_at: planned.event.envelope.at.clone(),
            settled_by_key_id: None,
            settled_same_user: None,
            settled_at: None,
            restored_id: None,
        };
        Ok((vec![commit_request(planned.event, planned.fold, None)], fix))
    }

    /// An edit or a retirement as a fix: a successor of the served tip
    /// written by the admin key, scanned as its kind requires, and promoted
    /// in the same commit (ADR-0089 d2, d4, d12).
    async fn rewrite_fix(
        &self,
        admin: &KeyRecord,
        req: &FixRequest,
        prev: Option<&SignedEnvelope>,
        fix_id: Uuid,
        reason: &str,
    ) -> Result<(Vec<CommitRequest>, FixRecord), AppError> {
        let tenant_id = admin.tenant_id;
        let target = self
            .journal
            .memory(tenant_id, req.target)
            .await?
            .ok_or(AppError::NotFound)?;
        if target.status != MemoryStatus::Promoted || !target.is_tip {
            return Err(AppError::TipConflict {
                current_tip: self
                    .journal
                    .claim_tip(tenant_id, target.workspace_id, &target.claim_key)
                    .await?
                    .as_ref()
                    .map(version_ref),
            });
        }
        let retirement = req.act == FixAct::Retire;
        if retirement && target.kind == MemoryKind::Term {
            return Err(AppError::TermRetirement);
        }
        // In a frozen workspace an admin demotes and erases, and never edits
        // or retires (ADR-0089 d12).
        self.refuse_frozen(tenant_id, target.workspace_id, FrozenOp::Write)
            .await?;
        let body = if retirement {
            nfc(reason)
        } else {
            nfc(req.body.as_deref().ok_or(AppError::AdminField {
                field: "/body",
                message: "an edit carries the new body".to_owned(),
            })?)
        };
        let kind = target.kind;
        let media_type = if retirement {
            "text/plain".to_owned()
        } else {
            target.format.media_type.clone()
        };
        let title = if retirement {
            None
        } else {
            req.title.clone().or_else(|| target.format.title.clone())
        };
        let (format, parsed) =
            self.capture_format(kind, Some(&media_type), title.as_deref(), &body, retirement)?;
        let guardrail = if retirement {
            None
        } else {
            guardrail_fields(
                kind,
                false,
                target.guardrail.as_ref().map(|g| g.severity),
                target.guardrail.as_ref().and_then(|g| g.rule.as_ref()),
                &body,
            )?
        };
        let (admitted_fields, _) = kind_fields(
            kind,
            retirement,
            true,
            if retirement {
                None
            } else {
                target.kind_fields.as_deref()
            },
            Some(&target.claim_key),
            &body,
            &media_type,
        )?;
        self.scan_kind_fields(tenant_id, admitted_fields.as_ref())?;
        // The scan its kind requires: a veto refuses the edit, and a finding
        // that would hold it refuses it as a fix, since a fix serves at
        // once (ADR-0089 d4, d17 c6).
        let (outcome, found) = self.scanner.assess(&AdmissionEnvelope {
            tenant_id,
            kind: kind.as_str(),
            body: &body,
        });
        let mut scan_finding = match outcome {
            ScanOutcome::Pass => None,
            ScanOutcome::Hold(reason) => Some(reason),
            ScanOutcome::Veto(reason) => return Err(AppError::ScanVeto(reason)),
        };
        let mut findings = self.admitted(&found, "/body".to_owned());
        self.scan_derived(
            tenant_id,
            kind,
            &format,
            &parsed,
            &mut scan_finding,
            &mut findings,
        )?;
        if let Some(finding) = scan_finding {
            return Err(AppError::FixScanHold { finding });
        }
        let valid = valid_time(None, None, kind, retirement)?;
        let successor = self
            .fix_version(
                admin,
                &target,
                FixVersion {
                    kind,
                    body,
                    format,
                    guardrail,
                    kind_fields: admitted_fields.map(Box::new),
                    valid,
                    retirement,
                    derives_from: vec![target.memory_id],
                    lineage: (vec![], vec![]),
                },
            )
            .await?;
        let submitted = self.submission_event(prev, admin, &successor, &target, fix_id)?;
        let promoted = self.fix_promotion(
            &submitted,
            &successor,
            &target,
            Approval {
                used_by: UsedBy::Person,
                same_user: Some(false),
                at: submitted.envelope.at.clone(),
                fix: None,
            },
            None,
        )?;
        let loosens = instruction(&target);
        let fix = FixRecord {
            fix_id,
            act: req.act,
            target_id: target.memory_id,
            workspace_id: target.workspace_id,
            claim_key: target.claim_key.clone(),
            replaced_id: Some(target.memory_id),
            served_id: Some(successor.memory_id),
            reason: reason.to_owned(),
            loosens_guardrail: loosens,
            admin_key_id: admin.key_id,
            lifecycle_event_id: promoted.0.envelope.event_id,
            state: FixState::Unconfirmed,
            fixed_at: promoted.0.envelope.at.clone(),
            settled_by_key_id: None,
            settled_same_user: None,
            settled_at: None,
            restored_id: None,
        };
        let mut held = commit_request(
            submitted,
            MetadataFold::MemoryHeld {
                memory: successor,
                enqueue: true,
                findings: findings.into(),
                imported: None,
            },
            None,
        );
        held.written = Some(crate::ports::Written {
            author_account: admin.user,
            starts_chain: false,
        });
        Ok((
            vec![held, commit_request(promoted.0, promoted.1, None)],
            fix,
        ))
    }

    /// A new version of `replaced`'s claim, the next ordinal, held until the
    /// promotion that commits with it.
    async fn fix_version(
        &self,
        author: &KeyRecord,
        replaced: &MemoryRecord,
        v: FixVersion,
    ) -> Result<MemoryRecord, AppError> {
        let state = self
            .journal
            .claim_key_state(author.tenant_id, replaced.workspace_id, &replaced.claim_key)
            .await?;
        let version = state
            .greatest_version
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                StoreError::Backend(format!(
                    "claim_key {:?} has no next ordinal",
                    replaced.claim_key
                ))
            })?;
        let mut principals = BTreeSet::from([author.principal.clone()]);
        let mut key_ids = BTreeSet::from([author.key_id]);
        principals.extend(replaced.lineage_principals.iter().cloned());
        key_ids.extend(replaced.lineage_key_ids.iter().copied());
        principals.extend(v.lineage.0);
        key_ids.extend(v.lineage.1);
        let body_digest = sha256_digest_string(v.body.as_bytes());
        Ok(MemoryRecord {
            memory_id: self.ids.new_id(),
            tenant_id: author.tenant_id,
            workspace_id: replaced.workspace_id,
            claim_key: replaced.claim_key.clone(),
            version,
            kind: v.kind,
            body_bytes: v.body.len() as u64,
            body: v.body,
            body_digest,
            status: MemoryStatus::Held,
            is_tip: false,
            promoted_at: None,
            retirement: v.retirement,
            replaces: Some(replaced.memory_id),
            author_principal: author.principal.clone(),
            author_key_id: author.key_id,
            author_person: self.person_of(author).await?,
            derives_from: v.derives_from,
            lineage_principals: principals.into_iter().collect(),
            lineage_key_ids: key_ids.into_iter().collect(),
            scan_finding: None,
            scanned_under: self.scanner.version().map(str::to_owned),
            created_at: self.clock.now(),
            guardrail: v.guardrail,
            kind_fields: v.kind_fields,
            valid: Box::new(v.valid),
            anchor: None,
            origin_kind: Some(OriginKind::Manual),
            source_check: None,
            approval: None,
            format: Box::new(v.format),
        })
    }

    /// `submission.remember` of a fix's version, by `author`, naming the fix
    /// (event-catalog.md; `provenance: admin_fix`).
    fn submission_event(
        &self,
        prev: Option<&SignedEnvelope>,
        author: &KeyRecord,
        m: &MemoryRecord,
        replaced: &MemoryRecord,
        fix_id: Uuid,
    ) -> Result<SignedEnvelope, AppError> {
        let mut payload = json!({
            "artifact_id": m.memory_id.to_string(),
            "workspace_id": m.workspace_id.to_string(),
            "claim_key": m.claim_key,
            "version": m.version,
            "kind": m.kind.as_str(),
            "plaintext_digest": m.body_digest,
            "body_bytes": m.body_bytes,
            "ciphertext_digest": Value::Null,
            "attachments": [],
            "derives_from": m.derives_from.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "anchor": Value::Null,
            "origin": m.origin_kind.unwrap_or(OriginKind::Manual).to_json(),
            "provenance": "admin_fix",
            "fix_id": fix_id.to_string(),
            "simulation_id": Value::Null,
            "replaces": {
                "artifact_id": replaced.memory_id.to_string(),
                "version": replaced.version,
            },
            "retirement": m.retirement,
            "scan": {
                "result": "pass",
                "scanner_version": self.scanner.version(),
                "finding": Value::Null,
            },
        });
        guardrail_payload(&mut payload, m.guardrail.as_deref())?;
        let mut payload = with_kind_fields(payload, m.kind_fields.as_deref());
        with_valid_time(&mut payload, &m.valid);
        with_format_fields(&mut payload, &m.format);
        self.seal(
            prev,
            author.tenant_id,
            event_types::SUBMISSION_REMEMBER,
            &author.principal,
            Some(author.key_id),
            payload,
        )
    }

    /// The promotion of a fix's version over `replaced`, sealed by the
    /// server on `prev`: `lifecycle.supersede` when `replaced` is the served
    /// tip, `lifecycle.promote` when the claim serves no version
    /// (ADR-0025 d10). `ratification` names the verdict that promotes it, a
    /// revert's; an admin's edit has none and names its `admin.fix`.
    fn fix_promotion(
        &self,
        prev: &SignedEnvelope,
        m: &MemoryRecord,
        replaced: &MemoryRecord,
        approval: Approval,
        ratification: Option<Uuid>,
    ) -> Result<(SignedEnvelope, MetadataFold), AppError> {
        let same_user = approval.same_user.unwrap_or(false);
        let policy = claim_review_policy(m.kind, Some(replaced.kind));
        let approval = Some(Box::new(approval));
        let (event_type, mut payload, fold) =
            if replaced.is_tip && replaced.status == MemoryStatus::Promoted {
                let fold = MetadataFold::TipSuperseded {
                    successor: m.memory_id,
                    replaced: replaced.memory_id,
                    replaced_valid_until: ratified_end(&replaced.valid, &m.valid).map(Box::new),
                    approval,
                };
                (
                    event_types::LIFECYCLE_SUPERSEDE,
                    json!({
                        "claim_key": m.claim_key,
                        "workspace_id": m.workspace_id.to_string(),
                        "new_artifact_id": m.memory_id.to_string(),
                        "new_version": m.version,
                        "replaces_artifact_id": replaced.memory_id.to_string(),
                        "replaces_version": replaced.version,
                        "retirement": m.retirement,
                        "ratification_event_id": ratification.map(|id| id.to_string()),
                        "self_signed": false,
                        "same_user": same_user,
                        "policy": policy,
                    }),
                    fold,
                )
            } else {
                (
                    event_types::LIFECYCLE_PROMOTE,
                    json!({
                        "artifact_id": m.memory_id.to_string(),
                        "version": m.version,
                        "workspace_id": m.workspace_id.to_string(),
                        "claim_key": m.claim_key,
                        "ratification_event_id": ratification.map(|id| id.to_string()),
                        "previous_tip_version": Value::Null,
                        "self_signed": false,
                        "same_user": same_user,
                        "policy": policy,
                    }),
                    MetadataFold::MemoryPromoted {
                        memory_id: m.memory_id,
                        approval,
                    },
                )
            };
        if let MetadataFold::TipSuperseded {
            replaced_valid_until: Some(end),
            ..
        } = &fold
            && let Some(members) = payload.as_object_mut()
        {
            members.insert("replaces_valid_until".to_owned(), Value::from(end.as_str()));
        }
        let event = self.seal(
            Some(prev),
            m.tenant_id,
            event_type,
            "oml-server",
            None,
            payload,
        )?;
        Ok((event, fold))
    }

    /// `GET /v2/admin/fixes?state=unconfirmed`: the tenant's fixes, oldest
    /// first, the unconfirmed ones alone by default (ADR-0089 d15).
    ///
    /// # Errors
    /// A key that is not an admin's, and the store's.
    pub async fn admin_fixes(
        &self,
        bearer: impl Into<Bearer<'_>>,
        unconfirmed_only: bool,
    ) -> Result<Vec<FixRecord>, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "fix")?;
        Ok(self
            .journal
            .fixes(admin.tenant_id, unconfirmed_only)
            .await?)
    }

    /// `GET /v2/review/fixes`: the unconfirmed fixes on workspaces where a
    /// person's reviewer key holds reviewer, each marked whether this key
    /// may confirm or revert it (ADR-0089 d15; amendment of 2026-11-18).
    ///
    /// # Errors
    /// A key that is not a person's reviewer key, and the store's.
    pub async fn review_fixes(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<Vec<ReviewedFix>, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        self.require_fix_reviewer(&caller.key)?;
        let mut listed = Vec::new();
        for fix in self.journal.fixes(caller.key.tenant_id, true).await? {
            if self
                .require_access(&caller, fix.workspace_id, Role::Reviewer, "fix")
                .await
                .is_err()
            {
                continue;
            }
            let settles = self.settle_rule(&caller.key, &fix).await?.is_some();
            listed.push(ReviewedFix { fix, settles });
        }
        Ok(listed)
    }

    /// `POST /v2/review/fixes/{id}/confirm`: a person confirms a fix,
    /// journalled as `admin.fix_confirmed`; the version it serves reads
    /// `confirmed: true` from then on (ADR-0089 d7, d8).
    ///
    /// # Errors
    /// See `settle_checks`; and the store's.
    pub async fn confirm_fix(
        &self,
        bearer: impl Into<Bearer<'_>>,
        fix_id: Uuid,
    ) -> Result<FixRecord, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        let prev = self.head_event(key.tenant_id).await?;
        let (fix, same_user) = self.settle_checks(&caller, fix_id).await?;
        let event = self.seal(
            prev.as_ref(),
            key.tenant_id,
            event_types::ADMIN_FIX_CONFIRMED,
            &key.principal,
            Some(key.key_id),
            json!({
                "fix_id": fix_id.to_string(),
                "same_user": same_user,
            }),
        )?;
        let at = event.envelope.at.clone();
        self.commit(
            event,
            MetadataFold::AdminFixSettled {
                fix_id,
                state: FixState::Confirmed,
                settled_by_key_id: key.key_id,
                same_user,
                restored_id: None,
            },
            None,
        )
        .await?;
        Ok(FixRecord {
            state: FixState::Confirmed,
            settled_by_key_id: Some(key.key_id),
            settled_same_user: Some(same_user),
            settled_at: Some(at),
            ..fix
        })
    }

    /// `POST /v2/review/fixes/{id}/revert`: a person reverts a fix. A new
    /// version carrying the text the fix took out of service is authored by
    /// the tenant's reserved server key, derives from that version, and is
    /// promoted by this person's verdict, all with `admin.fix_reverted` in
    /// one commit (ADR-0089 d8, d17 c5). An erasure is
    /// [`AppError::FixNotRevertible`]; a fix whose claim moved on since, or
    /// whose version took no tip out of service, is
    /// [`AppError::FixSuperseded`]; R2 refuses a reverter in the restored
    /// version's lineage, the admin's principal included.
    ///
    /// # Errors
    /// The refusals above, `settle_checks`'s, and the store's.
    pub async fn revert_fix(
        &self,
        bearer: impl Into<Bearer<'_>>,
        fix_id: Uuid,
    ) -> Result<FixRecord, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        let tenant_id = key.tenant_id;
        let prev = self.head_event(tenant_id).await?;
        let (fix, same_user) = self.settle_checks(&caller, fix_id).await?;
        if fix.act == FixAct::Erase {
            return Err(AppError::FixNotRevertible);
        }
        let original = match fix.replaced_id {
            Some(id) => self.journal.memory(tenant_id, id).await?,
            None => None,
        }
        .filter(|m| m.status != MemoryStatus::Erased)
        .ok_or(AppError::FixSuperseded)?;
        // The claim must still be served by the version the fix made its
        // tip, or by none when the fix made none (ADR-0089 d8).
        let tip = self
            .journal
            .claim_tip(tenant_id, fix.workspace_id, &fix.claim_key)
            .await?;
        if tip.as_ref().map(|t| t.memory_id) != fix.served_id {
            return Err(AppError::FixSuperseded);
        }
        // A demotion of a version below the tip took nothing out of service:
        // the claim had moved on before the fix (ADR-0089 d8, d17).
        if fix.act == FixAct::Demote && fix.served_id.is_none() && tip.is_some() {
            return Err(AppError::FixSuperseded);
        }
        self.refuse_frozen(tenant_id, fix.workspace_id, FrozenOp::Promote)
            .await?;
        let replaced = match tip {
            Some(t) => t,
            // No version is served: the restored version is the claim's
            // tip again, replacing the version it carries the text of.
            None => original.clone(),
        };
        let (server, mut links) = self.server_key(tenant_id, prev.as_ref()).await?;
        let admin_principal = self
            .journal
            .key(tenant_id, fix.admin_key_id)
            .await?
            .map(|k| k.principal);
        let restored = self
            .fix_version(
                &server,
                &replaced,
                FixVersion {
                    kind: original.kind,
                    body: original.body.clone(),
                    format: (*original.format).clone(),
                    guardrail: original.guardrail.clone(),
                    kind_fields: original.kind_fields.clone(),
                    valid: (*original.valid).clone(),
                    retirement: original.retirement,
                    derives_from: vec![original.memory_id],
                    lineage: (
                        original
                            .lineage_principals
                            .iter()
                            .cloned()
                            .chain(admin_principal)
                            .collect(),
                        original
                            .lineage_key_ids
                            .iter()
                            .copied()
                            .chain([fix.admin_key_id])
                            .collect(),
                    ),
                },
            )
            .await?;
        // R2 over the restored version's lineage: the original's and the
        // admin's (ADR-0089 d8).
        check_reviewer(
            key.role,
            &key.principal,
            key.key_id,
            &restored.lineage_closure(),
        )?;
        check_approval_class(
            restored.kind,
            key.role,
            key.used_by,
            key.crew_id.is_some(),
            key.profile.is_some(),
        )?;
        let last = links.last().map(|l: &CommitRequest| l.event.clone());
        let submitted = self.submission_event(
            last.as_ref().or(prev.as_ref()),
            &server,
            &restored,
            &replaced,
            fix_id,
        )?;
        let verdict = self.seal(
            Some(&submitted),
            tenant_id,
            event_types::REVIEW_APPROVE,
            &key.principal,
            Some(key.key_id),
            json!({
                "artifact_id": restored.memory_id.to_string(),
                "verdict": "approve",
                "reason": format!("revert of admin fix {fix_id}"),
                "served_body_digest": restored.body_digest,
                "format": restored.format.media_type,
            }),
        )?;
        let (promotion, fold) = self.fix_promotion(
            &verdict,
            &restored,
            &replaced,
            Approval {
                used_by: key.used_by,
                same_user: Some(same_user),
                at: verdict.envelope.at.clone(),
                fix: None,
            },
            Some(verdict.envelope.event_id),
        )?;
        let reverted = self.seal(
            Some(&promotion),
            tenant_id,
            event_types::ADMIN_FIX_REVERTED,
            &key.principal,
            Some(key.key_id),
            json!({
                "fix_id": fix_id.to_string(),
                "same_user": same_user,
                "restored_artifact_id": restored.memory_id.to_string(),
                "restored_version": restored.version,
                "derives_from": original.memory_id.to_string(),
            }),
        )?;
        let at = reverted.envelope.at.clone();
        let restored_id = restored.memory_id;
        let mut held = commit_request(
            submitted,
            MetadataFold::MemoryHeld {
                memory: restored,
                enqueue: true,
                findings: Vec::new().into(),
                imported: None,
            },
            None,
        );
        held.written = Some(crate::ports::Written {
            author_account: None,
            starts_chain: false,
        });
        links.push(held);
        links.push(commit_request(
            verdict,
            MetadataFold::VerdictRecorded {
                memory_id: restored_id,
                reviewer_key_id: key.key_id,
            },
            None,
        ));
        links.push(commit_request(promotion, fold, None));
        links.push(commit_request(
            reverted,
            MetadataFold::AdminFixSettled {
                fix_id,
                state: FixState::Reverted,
                settled_by_key_id: key.key_id,
                same_user,
                restored_id: Some(restored_id),
            },
            None,
        ));
        match self.commit_requests(links).await {
            Err(StoreError::TipMoved { .. }) => return Err(AppError::FixSuperseded),
            other => other?,
        };
        Ok(FixRecord {
            state: FixState::Reverted,
            settled_by_key_id: Some(key.key_id),
            settled_same_user: Some(same_user),
            settled_at: Some(at),
            restored_id: Some(restored_id),
            ..fix
        })
    }

    /// The tenant's reserved server key, and when the tenant has none yet,
    /// the links that issue it and revoke it at once, to lead the revert's
    /// commit (ADR-0089 d17 c5): role reader with no grant, `used_by:
    /// service`, principal [`SERVER_KEY_PRINCIPAL`], a derived name, and a
    /// value nobody is shown. It authenticates nothing.
    fn server_key_record(&self, tenant_id: Uuid) -> KeyRecord {
        let key_id = server_key_id(tenant_id);
        let secret = self.ids.new_secret();
        KeyRecord {
            key_id,
            tenant_id,
            role: Role::Reader,
            principal: SERVER_KEY_PRINCIPAL.to_owned(),
            used_by: UsedBy::Service,
            default_workspace: None,
            issued_by: None,
            credential: Credential::Bearer,
            key_hash: Some(sha256_digest_string(&secret)),
            name: Some(Name::derived(NamedKind::Key, key_id)),
            crew_id: None,
            user: None,
            revoked_at: None,
            session_user: None,
            guardrail_scope: GuardrailScope::ISSUANCE_DEFAULT,
            profile: None,
            expiry: None,
            requests_per_minute: None,
            linked_user: None,
        }
    }

    /// See [`Self::server_key_record`]: the key as stored, or as issued and
    /// revoked in links sealed by the server on `prev`.
    async fn server_key(
        &self,
        tenant_id: Uuid,
        prev: Option<&SignedEnvelope>,
    ) -> Result<(KeyRecord, Vec<CommitRequest>), AppError> {
        let key_id = server_key_id(tenant_id);
        if let Some(stored) = self.journal.key(tenant_id, key_id).await? {
            return Ok((stored, Vec::new()));
        }
        let record = self.server_key_record(tenant_id);
        let issued = self.seal(
            prev,
            tenant_id,
            event_types::KEY_ISSUED,
            "oml-server",
            None,
            Self::key_issued_payload(&record, Some(UsedBy::Service)),
        )?;
        let revoked = self.seal(
            Some(&issued),
            tenant_id,
            event_types::KEY_REVOKED,
            "oml-server",
            None,
            json!({
                "key_id": key_id.to_string(),
                "revoked_by_key_id": Value::Null,
                "reason_digest": sha256_digest_string(b"reserved server key, revoked at issuance"),
            }),
        )?;
        let links = vec![
            commit_request(issued, MetadataFold::KeyIssued(record.clone()), None),
            commit_request(revoked, MetadataFold::KeyRevoked { key_id }, None),
        ];
        Ok((record, links))
    }

    /// The checks a confirm and a revert share, in order: a person's
    /// reviewer key, the fix the tenant holds, reviewer on its workspace,
    /// the fix unconfirmed, and who may settle it (ADR-0089 d8; amendment of
    /// 2026-11-18). Returns the fix and whether the settlement is
    /// `same_user`.
    async fn settle_checks(
        &self,
        caller: &super::Caller,
        fix_id: Uuid,
    ) -> Result<(FixRecord, bool), AppError> {
        let key = &caller.key;
        self.require_fix_reviewer(key)?;
        let fix = self
            .journal
            .fix(key.tenant_id, fix_id)
            .await?
            .ok_or(AppError::NotFound)?;
        self.require_access(caller, fix.workspace_id, Role::Reviewer, "fix")
            .await
            .map_err(|_| AppError::NotFound)?;
        if fix.state != FixState::Unconfirmed {
            return Err(AppError::FixSettled {
                state: fix.state.as_str(),
            });
        }
        let same_user = self
            .settle_rule(key, &fix)
            .await?
            .ok_or(AppError::SamePerson)?;
        Ok((fix, same_user))
    }

    /// Who settles a fix (ADR-0089 d8, as amended 2026-11-18; #490): a key
    /// acting for another person than the admin's settles it, `Some(false)`;
    /// a key acting for the admin's own person settles it, marked
    /// `same_user`, `Some(true)`, only when the target lies inside that
    /// person's own user root, in a workspace that is neither protected nor
    /// frozen, and is a member of no release; otherwise `None`.
    async fn settle_rule(
        &self,
        key: &KeyRecord,
        fix: &FixRecord,
    ) -> Result<Option<bool>, AppError> {
        if !self
            .same_person(key.tenant_id, fix.admin_key_id, key)
            .await?
        {
            return Ok(Some(false));
        }
        let development = self
            .journal
            .workspace(key.tenant_id, fix.workspace_id)
            .await?
            .is_some_and(|w| w.status == WorkspaceStatus::Development);
        let loose = development
            && self.in_own_root(key, fix.workspace_id).await?
            && self.member_of_no_release(fix);
        Ok(loose.then_some(true))
    }

    /// Whether the fix's target is a member of no release, published or
    /// withdrawn (ADR-0089, amendment of 2026-11-18). Releases are not
    /// built (ADR-0085, R3), so every target is a member of none; R3 fills
    /// this check.
    #[allow(clippy::unused_self, reason = "R3 reads releases here")]
    fn member_of_no_release(&self, _fix: &FixRecord) -> bool {
        true
    }

    /// A person's reviewer key: reviewer role, used by a person, no crew
    /// and no profile (ADR-0089 d8, d15).
    #[allow(clippy::unused_self, reason = "an App check beside its peers")]
    fn require_fix_reviewer(&self, key: &KeyRecord) -> Result<(), AppError> {
        if key.role != Role::Reviewer {
            return Err(AppError::VerbCeiling {
                role: key.role,
                verb: "fix",
            });
        }
        if !is_person_reviewer(
            key.role,
            key.used_by,
            key.crew_id.is_some(),
            key.profile.is_some(),
        ) {
            return Err(AppError::Verdict(VerdictError::PersonRequired(
                MemoryKind::Memory,
            )));
        }
        Ok(())
    }
}

/// The tenant's admin fixes as `oml doctor` and the fix gauges read them
/// (ADR-0089 d15). Every count but `reverted` is of unconfirmed fixes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FixStats {
    pub unconfirmed: u64,
    /// The oldest unconfirmed fix's age in seconds, 0 with none.
    pub oldest_unconfirmed_seconds: u64,
    pub loosening_guardrail: u64,
    /// On a workspace where no live person's reviewer key holds reviewer.
    pub without_person_reviewer: u64,
    /// On a workspace where only keys of the admin's own person hold
    /// reviewer, so no one may confirm it outside that person's own root.
    pub admin_person_only: u64,
    pub reverted: u64,
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: WebStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The tenant's fix counts for `oml doctor` and the fix gauges
    /// (ADR-0089 d15). Reads no claim text and journals nothing.
    ///
    /// # Errors
    /// The store's.
    pub async fn fix_stats(&self, tenant_id: Uuid) -> Result<FixStats, AppError> {
        let all = self.journal.fixes(tenant_id, false).await?;
        let mut stats = FixStats::default();
        if all.is_empty() {
            return Ok(stats);
        }
        let now = self.clock.now();
        let reviewers: Vec<_> = self
            .journal
            .web_person_reviewers(tenant_id)
            .await?
            .into_iter()
            .filter(|r| r.key.revoked_at.is_none() && !r.key.expired_at(&now))
            .collect();
        for fix in &all {
            match fix.state {
                FixState::Reverted => stats.reverted += 1,
                FixState::Confirmed => {}
                FixState::Unconfirmed => {
                    stats.unconfirmed += 1;
                    let age = (now.unix_micros() - fix.fixed_at.unix_micros()).max(0) / 1_000_000;
                    stats.oldest_unconfirmed_seconds = stats
                        .oldest_unconfirmed_seconds
                        .max(u64::try_from(age).unwrap_or(0));
                    if fix.loosens_guardrail {
                        stats.loosening_guardrail += 1;
                    }
                    let ids: Vec<Uuid> = self
                        .journal
                        .workspace_ancestry(tenant_id, fix.workspace_id)
                        .await?
                        .unwrap_or_default()
                        .iter()
                        .map(|w| w.workspace_id)
                        .collect();
                    let mut any = false;
                    let mut other = false;
                    for r in &reviewers {
                        if effective_roles(&r.grants, &r.denials, &ids).contains(&Role::Reviewer) {
                            any = true;
                            if !self
                                .same_person(tenant_id, fix.admin_key_id, &r.key)
                                .await?
                            {
                                other = true;
                            }
                        }
                    }
                    if !any {
                        stats.without_person_reviewer += 1;
                    } else if !other {
                        stats.admin_person_only += 1;
                    }
                }
            }
        }
        Ok(stats)
    }

    /// [`Self::fix_stats`] of the admin key's tenant, for the admin rescan
    /// report's doctor lines (ADR-0089 d15).
    ///
    /// # Errors
    /// A key that is not an admin's, and the store's.
    pub async fn admin_fix_stats(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<FixStats, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "rescan")?;
        self.fix_stats(admin.tenant_id).await
    }
}

/// What a fix's new version carries.
struct FixVersion {
    kind: MemoryKind,
    body: String,
    format: FormatFields,
    guardrail: Option<Box<GuardrailFields>>,
    kind_fields: Option<Box<KindFields>>,
    valid: ValidTime,
    retirement: bool,
    derives_from: Vec<Uuid>,
    /// Principals and key ids its lineage gains beyond its author's and
    /// the replaced version's.
    lineage: (Vec<String>, Vec<Uuid>),
}
