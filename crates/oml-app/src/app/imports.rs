//! Workspace imports (ADR-0084 d6-d11, R11, I4; #426): a person's contributor
//! key imports a decrypted bundle it uploaded through one upload session
//! (ADR-0004 d3) into an in-progress workspace. The bundle is verified
//! before anything is written, the plan is made against the target, and
//! every version written lands held by the importer, with the source's
//! origin kind and its provenance. The import binds the upload in the
//! transaction that starts it, and nulls the upload's key when it ends:
//! that is d11's shred (I4).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use uuid::Uuid;

use oml_bundle::plan::{self, Outcome, Plan, PlannedKey, TargetTip, TargetView};
use oml_bundle::{
    BundleError, EdgeKind, Limits, Source, VerifiedBundle, VerifyOptions, VersionEntry,
    VersionLine, import_idempotency_key, manifest_signed_bytes, sha256_digest,
};
use oml_domain::anchor::Anchor;
use oml_domain::claim_kinds::TestShape;
use oml_domain::envelope::event_types;
use oml_domain::import::ImportState;
use oml_domain::name::{NamedKind, Reference};
use oml_domain::origin::OriginKind;
use oml_domain::signup::Quota;
use oml_domain::upload::{self, UploadRefusal, UploadState};
use oml_domain::{EventTime, MemoryKind, Role, UsedBy, WorkspaceStatus};

use super::busy::BusyGuard;
use super::{App, BULK_CHAIN_RETRIES, Caller, in_progress_root_in};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer, StoreError};
use crate::types::{
    AppError, Bearer, ImportBegin, ImportEnded, ImportLedgerEntry, ImportListing, ImportRecord,
    ImportRefusal, ImportTargetView, ImportedVersion, ImportedWrite, KindFields, OnSimilar,
    ProposeClaimEdge, SubmitRequest, SubmitVerb, SupersedeRequest, UploadSession,
};

/// What `POST /v2/imports` asks (ADR-0084 d15, as the coordinator decided
/// 2026-11-17): the completed upload holding the decrypted bundle, the
/// target, and the bundle's checks.
#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub upload_id: Uuid,
    /// An in-progress workspace, or a bare name the import creates in
    /// progress (d6).
    pub target: Reference,
    /// Plan only: nothing is written and the upload is not bound (d8).
    pub dry_run: bool,
    pub allow_older: bool,
    /// The source key's fingerprint, out of band (d5); a first import from
    /// a source requires it (R11).
    pub expect_key: Option<String>,
    /// The source tenant id the importer expects (R13).
    pub expect_tenant: Option<String>,
}

/// The plan as a dry run answers it, and as an import carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    pub plan: Plan,
    pub source: Source,
    pub exported_at: String,
    /// The digest of the source manifest's signed bytes.
    pub manifest_digest: String,
    /// The target, or `None` for a new name the import would create.
    pub target_workspace_id: Option<Uuid>,
    /// Edges of the bundle the import would propose, and those it reports
    /// without a proposal (d6).
    pub edges_proposable: u64,
    pub edges_reported: u64,
}

/// A dry run's plan, or a finished import's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportAnswer {
    Planned(ImportPlan),
    Imported(ImportRecord),
}

/// Where the target stands before anything is written.
enum Target {
    Existing(Uuid),
    New(oml_domain::name::Name),
}

/// What `POST /v2/imports` starts (ADR-0084 I10): a dry run's plan, or an
/// import whose upload is bound and whose row is `running`, to be run by
/// [`App::import_finish`] after the request answers.
#[derive(Debug)]
pub enum ImportStart {
    Planned(Box<ImportPlan>),
    Started(Box<ImportJob>),
}

/// A begun import, owning what its run reads: the verified bundle, the
/// plan, the target's view, the bound upload and its place in the busy
/// gate (I13), which it holds until it is dropped.
pub struct ImportJob {
    api_key: String,
    caller: Caller,
    record: ImportRecord,
    planned: ImportPlan,
    verified: VerifiedBundle,
    view: ImportTargetView,
    session: UploadSession,
    _busy: BusyGuard,
}

impl std::fmt::Debug for ImportJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportJob")
            .field("record", &self.record)
            .finish_non_exhaustive()
    }
}

impl ImportJob {
    /// The import's row as it began, `running`.
    #[must_use]
    pub fn record(&self) -> &ImportRecord {
        &self.record
    }
}

/// How a run ended: the import's row, and the reason its upload's staging
/// key could not be shredded, which the caller logs (I11).
#[derive(Debug)]
pub struct ImportFinished {
    pub import: Result<ImportRecord, AppError>,
    pub shred_failure: Option<String>,
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/imports` run in one call: the plan of a dry run, or the
    /// finished import. The route starts the run and answers before it
    /// ends (I10); this is the same import for an in-process caller. A
    /// staging key that could not be shredded is a store failure here.
    pub async fn import_bundle(
        &self,
        api_key: &str,
        req: &ImportRequest,
        code_of: &(dyn Fn(AppError) -> String + Send + Sync),
    ) -> Result<ImportAnswer, AppError> {
        match self.import_start(api_key, req).await? {
            ImportStart::Planned(plan) => Ok(ImportAnswer::Planned(*plan)),
            ImportStart::Started(job) => {
                let finished = self.import_finish(*job, code_of).await;
                if let Some(reason) = finished.shred_failure {
                    return Err(AppError::Store(StoreError::Backend(reason)));
                }
                finished.import.map(ImportAnswer::Imported)
            }
        }
    }

    /// Verifies, plans and, unless a dry run, binds the upload and begins
    /// the import (ADR-0084 d6-d8, I4, I10, I12, I13).
    pub async fn import_start(
        &self,
        api_key: &str,
        req: &ImportRequest,
    ) -> Result<ImportStart, AppError> {
        let caller = self.authenticate(api_key).await?;
        let key = &caller.key;
        // d11: a person's contributor key, and no other, imports (I1's
        // shape): an import acts on a tree the key must read (ADR-0093 d6).
        if key.role != Role::Contributor || key.used_by != UsedBy::Person {
            return Err(AppError::ImportPersonOnly {
                role: key.role,
                used_by: key.used_by,
            });
        }
        // The upload's state first: a bound or expired upload is refused
        // by its own reason whatever else runs (ADR-0084 I13).
        let session = self.import_upload(&caller, req.upload_id).await?;
        // I13: one import or dry run per tenant, and a bounded number in
        // the process, hold a bundle in memory at a time.
        let mut busy = self
            .imports_busy
            .try_enter(key.tenant_id)
            .ok_or(AppError::ImportBusy)?;
        let target = self.import_target(&caller, &req.target).await?;

        let bytes = self.upload_bytes(&session).await?;
        let verified = oml_bundle::read_bundle(
            bytes.as_slice(),
            &VerifyOptions {
                limits: Limits {
                    max_total_bytes: self.uploads.max_bytes,
                    ..Limits::default()
                },
                expect_key: req.expect_key.clone(),
                expect_tenant: req.expect_tenant.clone(),
            },
        )
        .map_err(AppError::Bundle)?;
        drop(bytes);
        let manifest = &verified.contents.manifest;
        let manifest_digest =
            sha256_digest(&manifest_signed_bytes(manifest).map_err(AppError::Bundle)?);

        let view = match &target {
            Target::Existing(id) => {
                self.journal
                    .import_target_view(
                        key.tenant_id,
                        *id,
                        &manifest.source.server_id,
                        &manifest.source.tenant_id,
                    )
                    .await?
            }
            Target::New(_) => ImportTargetView::default(),
        };
        // R11: the first import from a source pins the key it names, so
        // it names one.
        if view.ledger.is_none() && req.expect_key.is_none() {
            return Err(AppError::ImportExpectKeyRequired);
        }
        // Equivalence is oml-format's diff (ADR-0056 d11), not linked here
        // yet: no two differing digests are taken as equivalent.
        let plan = plan::plan_import(&verified, &target_view(&view), req.allow_older, &|_, _| {
            false
        })
        .map_err(AppError::Bundle)?;
        let (edges_proposable, edges_reported) = edge_counts(&verified);

        // d7's pre-pass: the whole plan fits the target account's meters,
        // or nothing is written.
        let (memories, bytes_planned, writes) =
            planned_charges(&plan, &tips_by_source(&verified), &verified);
        let ceiling = self.ceiling_for(key.tenant_id, key.user).await?;
        self.require_within(
            key.tenant_id,
            ceiling.as_ref(),
            &[
                (Quota::Memories, memories),
                (Quota::TextBytes, bytes_planned),
                (Quota::WritesPerMonth, writes),
            ],
        )
        .await
        .map_err(|e| match e {
            AppError::QuotaReached(reached) => AppError::ImportOverQuota(reached),
            other => other,
        })?;

        let planned = ImportPlan {
            plan,
            source: manifest.source.clone(),
            exported_at: manifest.exported_at.clone(),
            manifest_digest,
            target_workspace_id: match &target {
                Target::Existing(id) => Some(*id),
                Target::New(_) => None,
            },
            edges_proposable,
            edges_reported,
        };
        if req.dry_run {
            return Ok(ImportStart::Planned(Box::new(planned)));
        }

        let target_id = match target {
            Target::Existing(id) => id,
            Target::New(name) => {
                // I16: created and marked in progress in one commit.
                self.create_workspace_as(&caller, &name, None, None, true, None)
                    .await?
                    .workspace
                    .workspace_id
            }
        };
        let now = self.clock.now();
        let import = ImportRecord {
            tenant_id: key.tenant_id,
            import_id: self.ids.new_id(),
            target_workspace_id: target_id,
            importer_key_id: key.key_id,
            upload_id: session.upload_id,
            source_server_id: planned.source.server_id.clone(),
            source_tenant_id: planned.source.tenant_id.clone(),
            manifest_digest: planned.manifest_digest.clone(),
            signer_fingerprint: planned.plan.signer_fingerprint.clone(),
            authenticated: planned.plan.authenticated,
            allow_older: planned.plan.allow_older_used,
            state: ImportState::Running,
            position: 0,
            counts: BTreeMap::new(),
            refusals: Vec::new(),
            created_at: now.clone(),
            ended_at: None,
        };
        // I4, I12: the upload binds and the source's key is pinned in the
        // transaction that starts the import.
        match self.journal.import_begin(&import, &now).await? {
            ImportBegin::Begun => {}
            ImportBegin::KeyPinned { pinned } => {
                return Err(AppError::Bundle(BundleError::SourceKeyChanged {
                    detail: format!(
                        "signed by {}, the ledger pins {pinned}",
                        import.signer_fingerprint
                    ),
                }));
            }
            ImportBegin::NotBindable => {
                let state = self
                    .journal
                    .upload_session(key.tenant_id, session.upload_id)
                    .await?
                    .map_or(UploadState::Abandoned, |s| s.state);
                let expired = state == UploadState::Expired
                    || (state == UploadState::Completed
                        && session
                            .expires_at
                            .as_ref()
                            .is_some_and(|e| e.unix_micros() <= now.unix_micros()));
                return Err(AppError::Upload(if expired {
                    UploadRefusal::Expired
                } else {
                    UploadRefusal::SessionInvalid { state }
                }));
            }
        }
        busy.running_import(import.import_id);
        Ok(ImportStart::Started(Box::new(ImportJob {
            api_key: api_key.to_owned(),
            caller,
            record: import,
            planned,
            verified,
            view,
            session,
            _busy: busy,
        })))
    }

    /// Runs a begun import to its end: writes the plan, journals
    /// `workspace.imported` or ends the row `failed`, and shreds the bound
    /// upload's key, done or failed (d11, I4, I10).
    pub async fn import_finish(
        &self,
        job: ImportJob,
        code_of: &(dyn Fn(AppError) -> String + Send + Sync),
    ) -> ImportFinished {
        let tips = tips_by_source(&job.verified);
        let run = self
            .import_run(
                &job.api_key,
                &job.caller,
                &job.record,
                &job.planned,
                &job.verified,
                &job.view,
                &tips,
                code_of,
            )
            .await;
        let shred_failure = self.shred_bound_upload(&job.session).await.err();
        let import = match run {
            Ok(()) => self
                .journal
                .import_record(job.record.tenant_id, job.record.import_id)
                .await
                .map_err(AppError::from)
                .and_then(|r| r.ok_or(AppError::NotFound)),
            Err(e) => Err(e),
        };
        ImportFinished {
            import,
            shred_failure,
        }
    }

    /// `GET /v2/imports/{id}`: the import as its importing key sees it, and
    /// as a person's `<user>-web` sees an import any key of the person ran,
    /// a key whose user is the session's person (ADR-0084 d15; ADR-0096
    /// d6). Any other caller and any other import is `access.not_found`.
    pub async fn import_status(
        &self,
        bearer: impl Into<Bearer<'_>>,
        import_id: Uuid,
    ) -> Result<ImportRecord, AppError> {
        let key = self.authenticate_bearer(bearer.into()).await?.key;
        let import = self
            .journal
            .import_record(key.tenant_id, import_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if import.importer_key_id == key.key_id {
            return Ok(import);
        }
        if super::auth::is_web_session_key(&key)
            && let Some(root) = key.user
            && self
                .journal
                .key(key.tenant_id, import.importer_key_id)
                .await?
                .is_some_and(|importer| importer.user == Some(root))
        {
            return Ok(import);
        }
        Err(AppError::NotFound)
    }

    /// The tenant's imports by state, for the doctor's metrics.
    pub async fn import_stats(
        &self,
        tenant_id: Uuid,
    ) -> Result<crate::types::ImportStats, AppError> {
        let now = self.clock.now();
        let stopped_before = EventTime::from_unix_micros(
            now.unix_micros()
                .saturating_sub(oml_domain::import::STOPPED_AFTER_MICROS),
        )
        .unwrap_or(now);
        Ok(self
            .journal
            .import_stats(tenant_id, &stopped_before)
            .await?)
    }

    /// The caller's completed import upload; another key's, or none, is
    /// `access.not_found` (ADR-0004 d3).
    async fn import_upload(
        &self,
        caller: &Caller,
        upload_id: Uuid,
    ) -> Result<UploadSession, AppError> {
        let key = &caller.key;
        let session = self
            .journal
            .upload_session(key.tenant_id, upload_id)
            .await?
            .filter(|s| s.key_id == key.key_id)
            .ok_or(AppError::NotFound)?;
        let now = self.clock.now();
        if session.state == UploadState::Expired
            || (session.state.counts_against_limits()
                && session
                    .expires_at
                    .as_ref()
                    .is_some_and(|e| e.unix_micros() <= now.unix_micros()))
        {
            return Err(AppError::Upload(UploadRefusal::Expired));
        }
        if session.state != UploadState::Completed {
            return Err(AppError::Upload(UploadRefusal::SessionInvalid {
                state: session.state,
            }));
        }
        Ok(session)
    }

    /// d6: an existing target is a development workspace in progress the
    /// caller writes; an absent bare name is one the import creates.
    async fn import_target(&self, caller: &Caller, target: &Reference) -> Result<Target, AppError> {
        let key = &caller.key;
        let id = self.resolve(key, NamedKind::Workspace, target).await?;
        let Some(ancestry) = self.journal.workspace_ancestry(key.tenant_id, id).await? else {
            return match target {
                Reference::Name(name) => Ok(Target::New(name.clone())),
                _ => Err(AppError::NotFound),
            };
        };
        self.require_access(caller, id, Role::Submitter, "import")
            .await?;
        let node = ancestry.first().ok_or(AppError::NotFound)?;
        if node.status == WorkspaceStatus::Development && in_progress_root_in(&ancestry).is_some() {
            return Ok(Target::Existing(id));
        }
        // I16: a standard, empty development workspace this key created is
        // an earlier import's target whose mark never landed; the retry
        // sets it.
        if node.status == WorkspaceStatus::Development
            && self
                .journal
                .workspace_creator_if_empty(key.tenant_id, id)
                .await?
                == Some(key.key_id)
        {
            self.mark_import_target(caller, id).await?;
            return Ok(Target::Existing(id));
        }
        Err(AppError::ImportTargetNotInProgress { workspace_id: id })
    }

    /// Marks an import's own empty target `in_progress` under the importing
    /// key (ADR-0030 d1, amended 2026-11-18).
    async fn mark_import_target(
        &self,
        caller: &Caller,
        workspace_id: Uuid,
    ) -> Result<(), AppError> {
        let key = &caller.key;
        let mut attempt = 0;
        loop {
            let prev = self.head_event(key.tenant_id).await?;
            let event = self.seal(
                prev.as_ref(),
                key.tenant_id,
                event_types::WORKSPACE_MODE_SET,
                &key.principal,
                Some(key.key_id),
                json!({
                    "workspace_id": workspace_id.to_string(),
                    "mode": "in_progress",
                    "from": "standard",
                }),
            )?;
            match self
                .commit(
                    event,
                    MetadataFold::WorkspaceModeSet {
                        workspace_id,
                        in_progress: true,
                        held_free: Vec::new(),
                    },
                    None,
                )
                .await
            {
                Err(e) if head_moved(&e) && attempt + 1 < BULK_CHAIN_RETRIES => attempt += 1,
                Err(e) => return Err(e),
                Ok(_) => return Ok(()),
            }
        }
    }

    /// The upload's plaintext, in part order.
    /// The digest is recomputed over what is read: bytes changed at rest
    /// after completion are `upload.digest_mismatch` (ADR-0004 d3, readings
    /// of 2026-11-18).
    async fn upload_bytes(&self, session: &UploadSession) -> Result<Vec<u8>, AppError> {
        use sha2::{Digest, Sha256};
        let mut bytes = Vec::with_capacity(usize::try_from(session.size).unwrap_or(0));
        let mut hash = Sha256::new();
        for n in 1..=upload::part_count(session.size) {
            let part = self.upload_part(session, n).await?;
            hash.update(&part);
            bytes.extend_from_slice(&part);
        }
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if digest != session.sha256 {
            return Err(AppError::Upload(UploadRefusal::DigestMismatch));
        }
        Ok(bytes)
    }

    /// Writes the plan, then journals `workspace.imported`. A version the
    /// target refuses is skipped and named; a store failure ends the
    /// import `failed`, and what it wrote stays held (d11, d13).
    #[allow(
        clippy::too_many_arguments,
        reason = "one import's inputs, read once by the caller"
    )]
    async fn import_run(
        &self,
        api_key: &str,
        caller: &Caller,
        import: &ImportRecord,
        planned: &ImportPlan,
        verified: &VerifiedBundle,
        view: &ImportTargetView,
        tips: &BTreeMap<(String, u64), &VersionLine>,
        code_of: &(dyn Fn(AppError) -> String + Send + Sync),
    ) -> Result<(), AppError> {
        let key = &caller.key;
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        let mut refusals: Vec<ImportRefusal> = Vec::new();
        // I14: every version this import wrote, journalled by
        // `workspace.imported`.
        let mut versions: Vec<Value> = Vec::new();
        // Source artifact id to the target version that carries it, for
        // edges: earlier imports' and this one's.
        let mut carried: BTreeMap<String, Uuid> = view.carried.iter().cloned().collect();
        let target_tips: BTreeMap<&str, (Uuid, u32)> = view
            .tips
            .iter()
            .map(|t| (t.claim_key.as_str(), (t.memory_id, t.version)))
            .collect();

        for planned_key in &planned.plan.keys {
            let outcome = planned_key.outcome;
            if !matches!(
                outcome,
                Outcome::New | Outcome::Changed | Outcome::Removed | Outcome::Conflict
            ) {
                *counts.entry(outcome.as_str().to_owned()).or_default() += 1;
                continue;
            }
            let source = planned_key
                .source
                .as_ref()
                .and_then(|(id, v)| tips.get(&(id.clone(), *v)).copied());
            let (source_id, source_version) = planned_key
                .source
                .clone()
                .unwrap_or_else(|| (String::new(), 0));
            // I14: the write's own commit records it.
            let record = ImportedWrite {
                import_id: import.import_id,
                source_artifact_id: source_id.clone(),
                source_version,
                outcome: outcome.as_str().to_owned(),
                provenance: provenance(
                    &planned.source,
                    import.import_id,
                    source,
                    outcome == Outcome::Conflict,
                ),
            };
            let written = self
                .write_planned(
                    caller,
                    import,
                    verified,
                    planned_key,
                    source,
                    target_tips.get(planned_key.claim_key.as_str()).copied(),
                    &record,
                )
                .await;
            match written {
                Ok((memory_id, replayed)) => {
                    // A replay committed nothing: an earlier import's write,
                    // which this import records as its own too.
                    if replayed
                        && let Err(e) = self
                            .journal
                            .import_version_recorded(
                                key.tenant_id,
                                import.import_id,
                                &ImportedVersion {
                                    memory_id,
                                    source_artifact_id: record.source_artifact_id.clone(),
                                    source_version,
                                    outcome: record.outcome.clone(),
                                    provenance: record.provenance.clone(),
                                },
                            )
                            .await
                    {
                        return self.fail_import(import, &counts, &refusals, e.into()).await;
                    }
                    versions.push(json!({
                        "memory_id": memory_id.to_string(),
                        "source": {"artifact_id": source_id, "version": source_version},
                        "outcome": outcome.as_str(),
                    }));
                    carried.insert(source_id, memory_id);
                    *counts.entry(outcome.as_str().to_owned()).or_default() += 1;
                }
                Err(e) if fatal(&e) => {
                    return self.fail_import(import, &counts, &refusals, e).await;
                }
                Err(e) => {
                    refusals.push(ImportRefusal {
                        claim_key: planned_key.claim_key.clone(),
                        source_artifact_id: source_id,
                        source_version,
                        code: code_of(e),
                    });
                    *counts.entry("refused".to_owned()).or_default() += 1;
                }
            }
        }

        // d6: claim edges whose two ends the target carries become the
        // importer's proposals; every other edge is reported, unproposed.
        let mut proposed = 0_u64;
        let mut reported = 0_u64;
        for edge in &verified.contents.edges {
            let ends = (carried.get(&edge.src), carried.get(&edge.dst));
            match (edge.kind, ends) {
                (EdgeKind::Claim, (Some(from), Some(to))) => {
                    let req = ProposeClaimEdge {
                        from_artifact_id: *from,
                        to_artifact_id: *to,
                        label: edge.label.clone().unwrap_or_default(),
                    };
                    let idem = format!(
                        "import:{}:{}:{}:edge:{}:{}",
                        import.target_workspace_id,
                        planned.source.server_id,
                        planned.source.tenant_id,
                        edge.src,
                        edge.dst
                    );
                    match self.propose_claim_edge(api_key, &req, Some(&idem)).await {
                        Ok(_) => proposed += 1,
                        Err(e) if fatal(&e) => {
                            return self.fail_import(import, &counts, &refusals, e).await;
                        }
                        Err(e) => {
                            // Named like a skipped version, so the
                            // importer reads why (d6, d11).
                            refusals.push(ImportRefusal {
                                claim_key: format!("edge:{}:{}", edge.src, edge.dst),
                                source_artifact_id: edge.src.clone(),
                                source_version: 0,
                                code: code_of(e),
                            });
                            reported += 1;
                        }
                    }
                }
                _ => reported += 1,
            }
        }
        counts.insert("edges_proposed".to_owned(), proposed);
        counts.insert("edges_reported".to_owned(), reported);
        // d13: a marker for a version already imported is counted; the
        // notice itself is not built in this release.
        counts.insert(
            "erasure_markers".to_owned(),
            planned.plan.erasure_notices.len() as u64,
        );

        let manifest = &verified.contents.manifest;
        let ledger = ImportLedgerEntry {
            exported_at: match &view.ledger {
                Some(l) if l.exported_at > manifest.exported_at => l.exported_at.clone(),
                _ => manifest.exported_at.clone(),
            },
            release_label: manifest.release.as_ref().map(|r| r.label.clone()),
            release_manifest_digest: manifest.release.as_ref().map(|r| r.manifest_digest.clone()),
            signer_fingerprint: planned.plan.signer_fingerprint.clone(),
        };
        let payload = json!({
            "import_id": import.import_id.to_string(),
            "workspace_id": import.target_workspace_id.to_string(),
            "manifest_digest": planned.manifest_digest,
            "source": {
                "server_id": planned.source.server_id,
                "tenant_id": planned.source.tenant_id,
            },
            "exported_at": manifest.exported_at,
            "counts": counts,
            "refused": refusals.len(),
            "versions": versions,
            "authenticated": planned.plan.authenticated,
            "allow_older": planned.plan.allow_older_used,
            "signer_fingerprint": planned.plan.signer_fingerprint,
            "upload_digest": self.upload_digest(import).await?,
        });
        let mut attempt = 0;
        loop {
            let prev = self.head_event(key.tenant_id).await?;
            let event = self.seal(
                prev.as_ref(),
                key.tenant_id,
                event_types::WORKSPACE_IMPORTED,
                &key.principal,
                Some(key.key_id),
                payload.clone(),
            )?;
            let fold = MetadataFold::WorkspaceImported(ImportEnded {
                import_id: import.import_id,
                target_workspace_id: import.target_workspace_id,
                source_server_id: planned.source.server_id.clone(),
                source_tenant_id: planned.source.tenant_id.clone(),
                counts: counts.clone(),
                refusals: refusals.clone(),
                ledger: ledger.clone(),
            });
            match self.commit(event, fold, None).await {
                Err(e) if head_moved(&e) && attempt + 1 < BULK_CHAIN_RETRIES => attempt += 1,
                Err(e) => return self.fail_import(import, &counts, &refusals, e).await,
                Ok(_) => return Ok(()),
            }
        }
    }

    /// `sha256:` and the upload's declared digest, which completion
    /// checked (ADR-0004 d3).
    async fn upload_digest(&self, import: &ImportRecord) -> Result<String, AppError> {
        Ok(self
            .journal
            .upload_session(import.tenant_id, import.upload_id)
            .await?
            .map(|s| format!("sha256:{}", s.sha256))
            .unwrap_or_default())
    }

    /// One planned key's write, by the importing key, held (d6, d8). The
    /// version's idempotency key is d8's, so a retry writes nothing twice.
    #[allow(
        clippy::too_many_arguments,
        reason = "one planned key's write and the import it belongs to"
    )]
    async fn write_planned(
        &self,
        caller: &Caller,
        import: &ImportRecord,
        bundle: &VerifiedBundle,
        key: &PlannedKey,
        source: Option<&VersionLine>,
        target_tip: Option<(Uuid, u32)>,
        record: &ImportedWrite,
    ) -> Result<(Uuid, bool), AppError> {
        let (source_id, source_version) = key.source.clone().unwrap_or_default();
        let idempotency_key = import_idempotency_key(
            &import.target_workspace_id.to_string(),
            &bundle.contents.manifest.source,
            &source_id,
            source_version,
        );
        let mut attempt = 0;
        loop {
            let result = match (key.outcome, source, target_tip) {
                (Outcome::New, Some(v), _) => {
                    let fields = VersionFields::of(v, &bundle_body(v, bundle)?)?;
                    let req = SubmitRequest {
                        idempotency_key: idempotency_key.clone(),
                        on_similar: OnSimilar::Hold,
                        claim_key: Some(v.claim_key.clone()),
                        verb: Some(SubmitVerb::Remember),
                        kind: fields.kind,
                        body: fields.body,
                        workspace: Some(Reference::Id(import.target_workspace_id)),
                        derives_from: Vec::new(),
                        severity: fields.severity,
                        rule: fields.rule,
                        kind_fields: fields.kind_fields,
                        valid_from: v.valid_from.clone(),
                        valid_until: v.valid_until.clone(),
                        anchor: fields.anchor,
                        format: v.format.clone(),
                        title: v.title.clone(),
                        origin: fields.origin,
                    };
                    self.submit_memory_as(Bearer::Session(caller), &req, Some(record))
                        .await
                        .map(|o| (o.receipt.memory_id, o.replayed))
                }
                (outcome, source, Some((tip_id, tip_version))) => {
                    let retire = writes_retirement(outcome, source);
                    let (body, fields) = match source {
                        Some(v) => {
                            let fields = VersionFields::of(v, &bundle_body(v, bundle)?)?;
                            (fields.body.clone(), Some(fields))
                        }
                        None => ("Removed from the source workspace.".to_owned(), None),
                    };
                    let req = SupersedeRequest {
                        idempotency_key: idempotency_key.clone(),
                        on_similar: OnSimilar::Hold,
                        target_id: tip_id,
                        body,
                        kind: fields.as_ref().map(|f| f.kind),
                        retirement: retire,
                        derives_from: Vec::new(),
                        reason: None,
                        workspace: None,
                        refuse_vocabulary_target: false,
                        severity: if retire {
                            None
                        } else {
                            fields.as_ref().and_then(|f| f.severity)
                        },
                        rule: if retire {
                            None
                        } else {
                            fields.as_ref().and_then(|f| f.rule.clone())
                        },
                        kind_fields: if retire {
                            None
                        } else {
                            fields.as_ref().and_then(|f| f.kind_fields.clone())
                        },
                        valid_from: source.and_then(|v| v.valid_from.clone()),
                        valid_until: source.and_then(|v| v.valid_until.clone()),
                        anchor: fields.as_ref().and_then(|f| f.anchor.clone()),
                        format: source.and_then(|v| v.format.clone()),
                        title: source.and_then(|v| v.title.clone()),
                        origin: fields.as_ref().and_then(|f| f.origin),
                        expected_version: tip_version,
                    };
                    self.supersede_as(Bearer::Session(caller), &req, Some(record))
                        .await
                        .map(|o| (o.receipt.memory_id, o.replayed))
                }
                // A new key with no source line, or a change with no target
                // tip, cannot be planned (R10).
                _ => {
                    return Err(AppError::Bundle(BundleError::Layout {
                        detail: "a planned key names no version".to_owned(),
                    }));
                }
            };
            match result {
                Err(e) if head_moved(&e) && attempt + 1 < BULK_CHAIN_RETRIES => attempt += 1,
                other => return other,
            }
        }
    }

    /// Ends the import `failed`, keeping what it wrote held, and returns
    /// the failure.
    async fn fail_import(
        &self,
        import: &ImportRecord,
        counts: &BTreeMap<String, u64>,
        refusals: &[ImportRefusal],
        e: AppError,
    ) -> Result<(), AppError> {
        let _ = self
            .journal
            .import_failed(
                import.tenant_id,
                import.import_id,
                &self.clock.now(),
                counts,
                refusals,
            )
            .await;
        Err(e)
    }

    /// Nulls a bound upload's key, then removes its objects (I4). A store
    /// failure is returned for the caller to log; the sweep shreds the key
    /// of a bound upload whose import ended (I11).
    async fn shred_bound_upload(&self, session: &UploadSession) -> Result<(), String> {
        let now: EventTime = self.clock.now();
        match self
            .journal
            .upload_transition(
                session.tenant_id,
                session.upload_id,
                &[UploadState::Bound],
                UploadState::Bound,
                &now,
                None,
                true,
            )
            .await
        {
            Ok(Some(shredded)) => {
                self.remove_objects(&shredded).await;
                Ok(())
            }
            Ok(None) => Err(format!(
                "upload {} is no longer bound, so its key was not shredded here",
                session.upload_id
            )),
            Err(e) => Err(format!(
                "upload {}: the staging key was not shredded: {e}",
                session.upload_id
            )),
        }
    }
}

/// Whether a planned key's write is a retirement (d8, I15): a removal, or
/// a conflict whose source removed the key, never a superseding text.
fn writes_retirement(outcome: Outcome, source: Option<&VersionLine>) -> bool {
    match outcome {
        Outcome::Removed => true,
        Outcome::Conflict => source.is_none_or(|v| v.retirement),
        _ => false,
    }
}

/// The fields of a target write that a version line carries.
struct VersionFields {
    kind: MemoryKind,
    body: String,
    kind_fields: Option<KindFields>,
    anchor: Option<Anchor>,
    origin: Option<OriginKind>,
    /// A guardrail's severity and rule, which the reader checked (I20).
    severity: Option<oml_domain::guardrail::Severity>,
    rule: Option<Value>,
}

impl VersionFields {
    fn of(v: &VersionLine, body: &str) -> Result<Self, AppError> {
        let refused = |field: &'static str, reason: &'static str| {
            AppError::KindField(oml_domain::claim_kinds::FieldRefusal { field, reason })
        };
        let kind = MemoryKind::parse(&v.kind)
            .ok_or_else(|| refused("/kind", "the source's kind is not one this server knows"))?;
        let kind_fields = match (&v.skill, &v.document, &v.test) {
            (Some(skill), None, None) => Some(KindFields::Skill {
                name: str_member(skill, "name")
                    .ok_or_else(|| refused("/skill", "a skill is {name, description}"))?,
                description: str_member(skill, "description")
                    .ok_or_else(|| refused("/skill", "a skill is {name, description}"))?,
            }),
            (None, Some(document), None) => Some(KindFields::Document {
                title: str_member(document, "title"),
            }),
            (None, None, Some(test)) => Some(KindFields::Test {
                shape: str_member(test, "shape")
                    .as_deref()
                    .and_then(TestShape::parse)
                    .ok_or_else(|| refused("/test/shape", "the source's test shape is unknown"))?,
            }),
            (None, None, None) => None,
            _ => return Err(refused("/skill", "a version carries one kind object")),
        };
        let anchor = if v.anchor.is_null() {
            None
        } else {
            Some(
                Anchor::from_json(&v.anchor, "/anchor")
                    .map_err(|_| refused("/anchor", "the source's anchor does not parse here"))?,
            )
        };
        let origin = match v.origin.get("kind").and_then(Value::as_str) {
            None => None,
            Some(kind) => Some(
                OriginKind::parse(kind)
                    .ok_or_else(|| refused("/origin", "the source's origin kind is unknown"))?,
            ),
        };
        let (severity, rule) = match &v.guardrail {
            None => (None, None),
            Some(g) => (
                Some(
                    g.get("severity")
                        .and_then(Value::as_str)
                        .and_then(oml_domain::guardrail::Severity::parse)
                        .ok_or_else(|| refused("/guardrail", "the source's severity is unknown"))?,
                ),
                g.get("rule").filter(|r| !r.is_null()).cloned(),
            ),
        };
        Ok(Self {
            kind,
            body: body.to_owned(),
            kind_fields,
            anchor,
            origin,
            severity,
            rule,
        })
    }
}

/// A version's text: inline, or its body by digest (d1). The reader
/// checked every body's digest; one that is not UTF-8 text is refused.
fn bundle_body(v: &VersionLine, bundle: &VerifiedBundle) -> Result<String, AppError> {
    if let Some(text) = &v.text {
        return Ok(text.clone());
    }
    let refused = AppError::KindField(oml_domain::claim_kinds::FieldRefusal {
        field: "/body",
        reason: "the source's body is not UTF-8 text",
    });
    let digest = v.body.as_deref().unwrap_or_default();
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    let bytes = bundle.contents.bodies.get(hex).ok_or_else(|| {
        AppError::Bundle(BundleError::Layout {
            detail: "a version names a body the bundle does not carry".to_owned(),
        })
    })?;
    String::from_utf8(bytes.clone()).map_err(|_| refused)
}

fn str_member(value: &Value, member: &str) -> Option<String> {
    value.get(member).and_then(Value::as_str).map(str::to_owned)
}

/// A failure that ends the whole import rather than skipping one version:
/// the store's, or the upload's.
fn fatal(e: &AppError) -> bool {
    matches!(e, AppError::Store(_) | AppError::Upload(_))
}

fn head_moved(e: &AppError) -> bool {
    matches!(
        e,
        AppError::Store(StoreError::SeqConflict { .. } | StoreError::PrevHashConflict { .. })
    )
}

/// The crate's view of the target, from the store's (R10).
fn target_view(view: &ImportTargetView) -> TargetView {
    TargetView {
        tips: view
            .tips
            .iter()
            .map(|t| {
                (
                    t.claim_key.clone(),
                    TargetTip {
                        artifact_id: t.memory_id.to_string(),
                        version: u64::from(t.version),
                        plaintext_digest: t.body_digest.clone(),
                        retirement: t.retirement,
                        written_by_last_import: t.written_by_import,
                        imported_from_this_source: t.imported_from_source,
                    },
                )
            })
            .collect(),
        ledger: view.ledger.as_ref().map(|l| plan::Ledger {
            exported_at: l.exported_at.clone(),
            release_manifest_digest: l.release_manifest_digest.clone(),
            signer_fingerprint: l.signer_fingerprint.clone(),
        }),
        imported: view.imported.iter().cloned().collect::<BTreeSet<_>>(),
    }
}

/// Every version line of the bundle by (artifact id, version).
fn tips_by_source(bundle: &VerifiedBundle) -> BTreeMap<(String, u64), &VersionLine> {
    bundle
        .contents
        .versions
        .iter()
        .filter_map(|e| match e {
            VersionEntry::Version(v) => Some(((v.artifact_id.clone(), v.version), v.as_ref())),
            VersionEntry::Erased(_) => None,
        })
        .collect()
}

/// (claim edges with both ends in the bundle, every other edge): what a
/// dry run says the import would propose and would only report (d6).
fn edge_counts(bundle: &VerifiedBundle) -> (u64, u64) {
    let ids: BTreeSet<&str> = bundle
        .contents
        .versions
        .iter()
        .filter_map(|e| match e {
            VersionEntry::Version(v) => Some(v.artifact_id.as_str()),
            VersionEntry::Erased(_) => None,
        })
        .collect();
    let proposable = bundle
        .contents
        .edges
        .iter()
        .filter(|e| {
            e.kind == EdgeKind::Claim
                && ids.contains(e.src.as_str())
                && ids.contains(e.dst.as_str())
        })
        .count() as u64;
    (proposable, bundle.contents.edges.len() as u64 - proposable)
}

/// (memories, text bytes, writes) the plan would charge (d7, ADR-0071 d2).
fn planned_charges(
    plan: &Plan,
    tips: &BTreeMap<(String, u64), &VersionLine>,
    bundle: &VerifiedBundle,
) -> (u64, u64, u64) {
    let mut memories = 0;
    let mut bytes = 0;
    let mut writes = 0;
    for key in &plan.keys {
        if !oml_domain::import::writes(key.outcome.as_str()) {
            continue;
        }
        writes += 1;
        if key.outcome == Outcome::New {
            memories += 1;
        }
        if let Some(v) = key
            .source
            .as_ref()
            .and_then(|(id, n)| tips.get(&(id.clone(), *n)))
        {
            bytes += match (&v.text, &v.body) {
                (Some(t), _) => t.len() as u64,
                (None, Some(d)) => bundle
                    .contents
                    .bodies
                    .get(d.strip_prefix("sha256:").unwrap_or(d))
                    .map_or(0, |b| b.len() as u64),
                (None, None) => 0,
            };
        }
    }
    (memories, bytes, writes)
}

/// `import_provenance` (ADR-0084 d4, amending ADR-0063 d1): the source
/// server, tenant, workspace, artifact id, version, digest and author
/// pseudonym, the source's `origin` verbatim and what the target does not
/// keep of the line (its anchor as sent and its source lineage), and
/// whether the version is d8's `conflict`.
fn provenance(
    source: &Source,
    import_id: Uuid,
    line: Option<&VersionLine>,
    conflict: bool,
) -> Value {
    let mut value = json!({
        "import_id": import_id.to_string(),
        "source_server_id": source.server_id,
        "source_tenant_id": source.tenant_id,
        "conflict": conflict,
    });
    if let (Some(v), Some(obj)) = (line, value.as_object_mut()) {
        obj.insert("workspace".to_owned(), json!(v.workspace));
        obj.insert("artifact_id".to_owned(), json!(v.artifact_id));
        obj.insert("version".to_owned(), json!(v.version));
        obj.insert("plaintext_digest".to_owned(), json!(v.plaintext_digest));
        obj.insert(
            "author".to_owned(),
            json!({
                "used_by": v.author.used_by,
                "same_user": v.author.same_user,
                "pseudonym": v.author.pseudonym,
            }),
        );
        obj.insert("origin".to_owned(), v.origin.clone());
        obj.insert("derives_from".to_owned(), json!(v.derives_from));
        obj.insert("state".to_owned(), json!(state_name(v)));
        obj.insert("promoted_at".to_owned(), json!(v.promoted_at));
    }
    value
}

fn state_name(v: &VersionLine) -> Value {
    serde_json::to_value(v.state).unwrap_or(Value::Null)
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: crate::ports::WebStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `GET /v2/imports` (ADR-0096 d6): as a person's `<user>-web`, the
    /// imports of every key that names the person as its user; as any
    /// other key, that key's own. Newest first, in `state` where one is
    /// named, a page of at most `limit` strictly before `before`, each with
    /// the importing key's id and name. A tenant key linked to the person
    /// is not included (decided by the operator, 2026-11-24).
    pub async fn list_imports(
        &self,
        bearer: impl Into<Bearer<'_>>,
        state: Option<ImportState>,
        before: Option<(EventTime, Uuid)>,
        limit: usize,
    ) -> Result<ImportListing, AppError> {
        let key = self.authenticate_bearer(bearer.into()).await?.key;
        let tenant_id = key.tenant_id;
        let keys = match key.user {
            Some(root) if super::auth::is_web_session_key(&key) => {
                self.journal.web_user_keys(tenant_id, root).await?
            }
            _ => vec![key.clone()],
        };
        let names: BTreeMap<Uuid, Option<oml_domain::name::Name>> =
            keys.iter().map(|k| (k.key_id, k.name.clone())).collect();
        let ids: Vec<Uuid> = names.keys().copied().collect();
        let mut rows = self
            .journal
            .imports_of_keys(
                tenant_id,
                &ids,
                state,
                before.as_ref().map(|(at, id)| (at, *id)),
                limit + 1,
            )
            .await?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let next = more
            .then(|| rows.last().map(|i| (i.created_at.clone(), i.import_id)))
            .flatten();
        Ok(ImportListing {
            items: rows
                .into_iter()
                .map(|i| {
                    let name = names.get(&i.importer_key_id).cloned().flatten();
                    (i, name)
                })
                .collect(),
            next,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conflict_whose_source_removed_the_key_writes_a_retirement() {
        assert!(writes_retirement(Outcome::Conflict, None));
        assert!(writes_retirement(Outcome::Removed, None));
        assert!(!writes_retirement(Outcome::Changed, None));
    }
}
