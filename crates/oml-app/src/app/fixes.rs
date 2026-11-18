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

use oml_domain::{MemoryKind, MemoryStatus, Role, SignedEnvelope, event_types, transition};

use super::{App, commit_request, require_role};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::types::{AppError, Bearer, KeyRecord, MemoryRecord, VersionRef};

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
        let restored = if memory.is_tip {
            self.journal
                .claim_versions(tenant_id, memory.workspace_id, &memory.claim_key)
                .await?
                .into_iter()
                .filter(|v| v.version < memory.version && v.status == MemoryStatus::Promoted)
                .max_by_key(|v| v.version)
        } else {
            None
        };
        let prev = self.head_event(tenant_id).await?;
        let event = self.demote_event(prev.as_ref(), &admin, &memory, restored.as_ref())?;
        let journal_seq = event.envelope.seq;
        let fold = MetadataFold::MemoryDemoted {
            memory_id,
            restored: restored.as_ref().map(|r| r.memory_id),
        };
        self.commit_requests(vec![commit_request(event, fold, None)])
            .await?;
        Ok(Demotion {
            demoted: version_ref(&memory),
            restored: restored.as_ref().map(version_ref),
            journal_seq,
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
        let prev = self.head_event(tenant_id).await?;
        let event = self.erase_event(prev.as_ref(), &admin, &named, &versions)?;
        let journal_seq = event.envelope.seq;
        let fold = MetadataFold::ClaimErased {
            memory_ids: versions.iter().map(|v| v.memory_id).collect(),
        };
        self.commit_requests(vec![commit_request(event, fold, None)])
            .await?;
        Ok(Erasure {
            erased: versions.iter().map(version_ref).collect(),
            journal_seq,
        })
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
