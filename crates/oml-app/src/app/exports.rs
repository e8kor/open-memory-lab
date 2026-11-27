//! Workspace export in `history` mode (ADR-0084 d1-d5, d10, d13, d15;
//! #425): a person's key writes a signed `oml-bundle/1` of one workspace's
//! subtree, encrypted to the age recipients the request names, and the
//! export is journalled as `workspace.exported` with its bytes counted on
//! the `exports` meter. `release` mode and `since` wait for releases (#414)
//! and are refused (reading I3).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use serde_json::{Value, json};
use uuid::Uuid;

use oml_bundle::{
    Author, BundleContents, EdgeKind, EdgeLine, ErasureMarker, EventLine, Manifest, Mode, Source,
    State, Verdict, VerdictLine, VersionEntry, VersionLine, WorkspaceLine, crypt, pseudonym,
    sha256_digest,
};
use oml_domain::export::{ExportPolicy, may_export};
use oml_domain::name::{NamedKind, Reference};
use oml_domain::vocab_edges::EdgeState;
use oml_domain::{MemoryStatus, Role, SignedEnvelope, event_types};

use super::{App, Caller};
use crate::ports::{
    Clock, ConnectionStore, IdGen, MetadataFold, Scanner, Signer, StoreError, WebStore,
};
use crate::types::{
    AppError, Bearer, ClaimEdgeSelection, KeyRecord, MemoryRecord, WorkspaceRecord,
};

/// How many journal events one read of the export's journal walk takes.
const EVENT_PAGE: usize = 500;

/// How many claim edges one read of the export's edge walk takes.
const EDGE_PAGE: usize = 500;

/// `POST /v2/workspaces/{workspace}/exports`, as the transport parsed it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportRequest {
    /// `history` or `release`; absent takes the workspace's default (d2).
    pub mode: Option<String>,
    /// age X25519 public keys, `age1…`; at least one (d5).
    pub recipients: Vec<String>,
    /// A release label: refused until releases are built (reading I3).
    pub release: Option<String>,
    /// A base release label: refused until releases are built (I3).
    pub since: Option<String>,
}

/// What a bundle carries, by file (`workspace.exported`'s `counts`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportCounts {
    pub workspaces: u64,
    pub versions: u64,
    pub erased: u64,
    pub verdicts: u64,
    pub events: u64,
    pub edges: u64,
}

/// A finished export: the age file and what was journalled of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// The encrypted bundle, binary age.
    pub file: Vec<u8>,
    /// `sha256:` of `manifest.json` as the tar carries it.
    pub manifest_digest: String,
    pub counts: ExportCounts,
    /// The `sha256:` fingerprint of each recipient, in the request's order.
    pub recipients: Vec<String>,
    /// The journal key's fingerprint the manifest is signed under, and the
    /// source server id (reading I5).
    pub signer_fingerprint: String,
}

/// The mode an export takes, refusing what is not built (d2, reading I3).
fn export_mode(req: &ExportRequest, in_user_root: bool) -> Result<(), AppError> {
    let unbuilt = |field| AppError::ExportModeRefused {
        field,
        reason: "releases_unbuilt",
    };
    if req.release.is_some() {
        return Err(unbuilt("release"));
    }
    if req.since.is_some() {
        return Err(unbuilt("since"));
    }
    match req.mode.as_deref() {
        Some("history") => Ok(()),
        Some("release") => Err(unbuilt("mode")),
        // A user root takes `history`; elsewhere the default is `release`.
        None if in_user_root => Ok(()),
        None => Err(unbuilt("mode")),
        Some(_) => Err(AppError::ExportModeRefused {
            field: "mode",
            reason: "mode is history or release",
        }),
    }
}

/// Every recipient parses as an age X25519 public key, and there is at
/// least one (d5), checked before anything is read.
fn recipient_fingerprints(recipients: &[String]) -> Result<Vec<String>, AppError> {
    if recipients.is_empty() {
        return Err(AppError::BundleUnencrypted {
            detail: "an export names at least one recipient".to_owned(),
        });
    }
    recipients
        .iter()
        .map(|r| {
            crypt::recipient_fingerprint(r).map_err(|_| AppError::BundleUnencrypted {
                detail: "a recipient is not an age X25519 public key".to_owned(),
            })
        })
        .collect()
}

fn bundle_failure(err: &oml_bundle::BundleError) -> AppError {
    // The export assembles only what the store holds; a record the writer
    // refuses is a defect, answered as the store failing, never as content.
    AppError::Store(StoreError::Backend(format!(
        "the export's bundle could not be written: {}",
        err.code()
    )))
}

fn line_state(status: MemoryStatus) -> Option<State> {
    match status {
        MemoryStatus::Held => Some(State::Held),
        MemoryStatus::Promoted => Some(State::Promoted),
        MemoryStatus::Demoted => Some(State::Demoted),
        MemoryStatus::Rejected => Some(State::Rejected),
        MemoryStatus::Withdrawn => Some(State::Withdrawn),
        MemoryStatus::Erased => None,
    }
}

fn text_member(payload: &Value, member: &str) -> Option<String> {
    payload
        .get(member)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn object_member(payload: &Value, member: &str) -> Option<Value> {
    payload.get(member).filter(|v| !v.is_null()).cloned()
}

/// The signed event as `events.jsonl` carries it (chain-format.md).
fn event_line(event: &SignedEnvelope) -> EventLine {
    let e = &event.envelope;
    EventLine {
        tenant_id: e.tenant_id.to_string(),
        seq: e.seq,
        event_id: e.event_id.to_string(),
        event_type: e.event_type.clone(),
        prev_hash: e.prev_hash.to_prefixed(),
        principal: e.principal.clone(),
        api_key_id: e.api_key_id.map(|k| k.to_string()),
        at: e.at.as_str().to_owned(),
        payload: e.payload.clone(),
        hash: event.hash.to_prefixed(),
        sig: format!("ed25519:{}", hex_lower(&event.sig.to_bytes())),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// What the journal walk found for the exported versions: each one's
/// submission event, and the verdicts on them, in chain order.
#[derive(Default)]
struct JournalFacts {
    submissions: BTreeMap<Uuid, SignedEnvelope>,
    verdicts: Vec<SignedEnvelope>,
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: WebStore + ConnectionStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/workspaces/{workspace}/exports`: a person's key exports the
    /// workspace's subtree in `history` mode as a signed bundle encrypted to
    /// `recipients` (ADR-0084 d1-d5, d10, d13). In order: a key not used by
    /// a person, or an admin key, is `access.verb_ceiling`; no recipient is
    /// `bundle.unencrypted`; `release`, `since` and a `release` mode are
    /// `request.validation` (`releases_unbuilt`); a workspace the key cannot
    /// read is `access.not_found`; the tenant's policy is `export.disabled`;
    /// a node of the subtree the key cannot read is `export.not_readable`.
    /// Journalled as `workspace.exported`, its bytes on the `exports` meter.
    pub async fn export_workspace(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
        req: &ExportRequest,
    ) -> Result<Exported, AppError> {
        const VERB: &str = "export";
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = caller.key.clone();
        if !may_export(
            key.role,
            key.used_by,
            key.profile.is_some(),
            key.crew_id.is_some(),
        ) {
            return Err(AppError::ExportPersonOnly {
                role: key.role,
                used_by: key.used_by,
            });
        }
        let recipients = recipient_fingerprints(&req.recipients)?;
        // What the request alone decides is refused before any read; the
        // default mode needs the workspace's place.
        if req.mode.is_some() || req.release.is_some() || req.since.is_some() {
            export_mode(req, true)?;
        }
        let tenant_id = key.tenant_id;
        // I13: one export per tenant builds its bundle at a time.
        let _busy = self
            .exports_busy
            .try_enter(tenant_id)
            .ok_or(AppError::ExportBusy)?;
        let workspace_id = self.resolve(&key, NamedKind::Workspace, &workspace).await?;
        self.observer.workspace(workspace_id);
        self.require_access(&caller, workspace_id, Role::Reader, VERB)
            .await?;
        let ancestry = self
            .journal
            .workspace_ancestry(tenant_id, workspace_id)
            .await?
            .unwrap_or_default();
        let in_user_root = ancestry.iter().any(|w| w.user_root);
        export_mode(req, in_user_root)?;
        match self.journal.export_policy(tenant_id).await? {
            ExportPolicy::Off => return Err(AppError::ExportDisabled { reason: None }),
            ExportPolicy::OwnRoot => {
                if !self.in_own_root(&key, workspace_id).await?
                    && !self
                        .journal
                        .export_allowed(tenant_id, workspace_id, &self.clock.now())
                        .await?
                {
                    return Err(AppError::ExportDisabled {
                        reason: Some("own_root"),
                    });
                }
            }
            ExportPolicy::Allowed => {}
        }
        let subtree = self
            .journal
            .workspace_subtree(tenant_id, workspace_id)
            .await?;
        for node in &subtree {
            let node_ancestry = self.ancestry(tenant_id, node.workspace_id).await?;
            if !caller.effective(&node_ancestry).contains(&Role::Reader) {
                return Err(AppError::ExportNotReadable);
            }
        }
        let paths = workspace_paths(&ancestry, &subtree, workspace_id);
        let nodes: Vec<Uuid> = subtree.iter().map(|w| w.workspace_id).collect();
        let user_root = self.person_root_of(&key).await?;
        // I6: a linked key owns itself alone; only a user or session key's
        // person widens the set.
        let own_root = self.own_set_root_of(&key).await?;
        let own_keys = self.export_own_keys(&caller, own_root).await?;
        // I13: what the bundle would carry, by its stored bytes, is weighed
        // before a body is read.
        if self
            .journal
            .export_body_bytes(tenant_id, &nodes, &own_keys)
            .await?
            > self.uploads.max_bytes
        {
            return Err(AppError::ExportTooLarge {
                max_bytes: self.uploads.max_bytes,
            });
        }
        let rows = self
            .journal
            .export_versions(tenant_id, &nodes, &own_keys)
            .await?;
        // The store reads by the rule; the use case holds it again, so a
        // store that regressed leaks nothing (d2, R3).
        let rows: Vec<MemoryRecord> = rows
            .into_iter()
            .filter(|m| {
                nodes.contains(&m.workspace_id)
                    && (m.status == MemoryStatus::Promoted
                        || (m.status == MemoryStatus::Erased && m.promoted_at.is_some())
                        || own_keys.contains(&m.author_key_id))
            })
            .collect();
        // I17: what the bundle carries whole; a version's lineage and the
        // verdicts name nothing else.
        let whole: BTreeSet<Uuid> = rows
            .iter()
            .filter(|m| m.status != MemoryStatus::Erased)
            .map(|m| m.memory_id)
            .collect();
        let facts = self.export_journal_facts(tenant_id, &rows).await?;
        let edges = self.export_edges(&caller, &nodes, &rows, &paths).await?;

        let salt = self.ids.new_secret();
        let mut keys: BTreeMap<Uuid, Option<KeyRecord>> = BTreeMap::new();
        let exporter_pseudonym = pseudonym(&salt, &key.key_id.to_string());
        let mut versions = Vec::with_capacity(rows.len());
        let mut events = Vec::new();
        let mut bodies = BTreeMap::new();
        let mut counts = ExportCounts::default();
        for m in &rows {
            let Some(state) = line_state(m.status) else {
                // d13: the artifact id, the version and the fact of erasure.
                versions.push(VersionEntry::Erased(ErasureMarker {
                    artifact_id: m.memory_id.to_string(),
                    version: u64::from(m.version),
                    erased: true,
                }));
                counts.erased += 1;
                continue;
            };
            let submission = facts.submissions.get(&m.memory_id).ok_or_else(|| {
                AppError::Store(StoreError::Backend(format!(
                    "the submission event of version {} is missing",
                    m.memory_id
                )))
            })?;
            let payload = &submission.envelope.payload;
            let author = self
                .author_of(&mut keys, tenant_id, m.author_key_id)
                .await?;
            let simulation_id = text_member(payload, "simulation_id");
            // R1, R12: the exporting key's own writes carry their signed
            // event; a simulation row never does.
            let source_event = (m.author_key_id == key.key_id
                && simulation_id.is_none()
                && submission.envelope.api_key_id == Some(key.key_id)
                && matches!(
                    submission.envelope.event_type.as_str(),
                    event_types::SUBMISSION_REMEMBER | event_types::SUBMISSION_LEARN
                ))
            .then(|| {
                events.push(event_line(submission));
                submission.envelope.event_id.to_string()
            });
            let (text, body) = if m.body.len() > oml_bundle::INLINE_TEXT_MAX {
                let digest = sha256_digest(m.body.as_bytes());
                let hex = digest.trim_start_matches("sha256:").to_owned();
                bodies.insert(hex, m.body.as_bytes().to_vec());
                (None, Some(digest))
            } else {
                (Some(m.body.clone()), None)
            };
            versions.push(VersionEntry::Version(Box::new(VersionLine {
                artifact_id: m.memory_id.to_string(),
                version: u64::from(m.version),
                claim_key: m.claim_key.clone(),
                workspace: paths.get(&m.workspace_id).cloned().unwrap_or_default(),
                kind: m.kind.as_str().to_owned(),
                format: Some(m.format.media_type.as_str().to_owned()),
                title: m.format.title.clone(),
                state,
                retirement: m.retirement,
                replaces: payload
                    .get("replaces")
                    .and_then(|r| r.get("version"))
                    .and_then(Value::as_u64),
                simulation_id,
                text,
                body,
                plaintext_digest: m.body_digest.clone(),
                anchor: payload.get("anchor").cloned().unwrap_or(Value::Null),
                valid_from: text_member(payload, "valid_from"),
                valid_until: text_member(payload, "valid_until"),
                skill: object_member(payload, "skill"),
                document: object_member(payload, "document"),
                test: object_member(payload, "test"),
                // I20: a guardrail's severity and rule travel with it.
                guardrail: m
                    .guardrail
                    .as_ref()
                    .map(|g| serde_json::json!({"severity": g.severity.as_str(), "rule": g.rule})),
                derives_from: m
                    .derives_from
                    .iter()
                    .filter(|id| whole.contains(id))
                    .map(ToString::to_string)
                    .collect(),
                origin: payload.get("origin").cloned().unwrap_or(Value::Null),
                attachments: Vec::new(),
                author: Author {
                    used_by: author,
                    // I7: the promotion's mark as journalled, never
                    // recomputed.
                    same_user: m
                        .approval
                        .as_ref()
                        .and_then(|a| a.same_user)
                        .unwrap_or(false),
                    pseudonym: pseudonym(&salt, &m.author_key_id.to_string()),
                },
                created_at: m.created_at.as_str().to_owned(),
                promoted_at: m.promoted_at.as_ref().map(|t| t.as_str().to_owned()),
                source_event,
            })));
            counts.versions += 1;
        }
        // I17: a verdict travels only for a version exported whole, never
        // for an erasure marker.
        let by_id: BTreeMap<Uuid, &MemoryRecord> = rows
            .iter()
            .filter(|m| whole.contains(&m.memory_id))
            .map(|m| (m.memory_id, m))
            .collect();
        let mut verdicts = Vec::new();
        for event in &facts.verdicts {
            let e = &event.envelope;
            let Some(m) = e.payload["artifact_id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .and_then(|id| by_id.get(&id))
            else {
                continue;
            };
            let verdict = if e.event_type == event_types::REVIEW_APPROVE {
                Verdict::Approve
            } else {
                Verdict::Reject
            };
            let reviewer_id = e.payload["reviewer_key_id"]
                .as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .or(e.api_key_id);
            let Some(reviewer_id) = reviewer_id else {
                continue;
            };
            let used_by = self.author_of(&mut keys, tenant_id, reviewer_id).await?;
            // I7: the approving verdict that promoted the version carries the
            // promotion's mark and instant; every other carries none.
            let promoting =
                verdict == Verdict::Approve && m.approval.as_ref().is_some_and(|a| a.at == e.at);
            verdicts.push(VerdictLine {
                artifact_id: m.memory_id.to_string(),
                version: u64::from(m.version),
                verdict,
                reviewer: Author {
                    used_by,
                    same_user: promoting
                        && m.approval
                            .as_ref()
                            .and_then(|a| a.same_user)
                            .unwrap_or(false),
                    pseudonym: pseudonym(&salt, &reviewer_id.to_string()),
                },
                approved_at: promoting.then(|| e.at.as_str().to_owned()),
                at: e.at.as_str().to_owned(),
            });
        }
        counts.verdicts = verdicts.len() as u64;
        counts.events = events.len() as u64;
        counts.edges = edges.len() as u64;
        let workspaces: Vec<WorkspaceLine> = subtree
            .iter()
            .map(|w| WorkspaceLine {
                name: w.name.as_str().to_owned(),
                path: paths.get(&w.workspace_id).cloned().unwrap_or_default(),
                status: w.status.as_str().to_owned(),
                parent: (w.workspace_id != workspace_id)
                    .then(|| w.parent_id.and_then(|p| paths.get(&p).cloned()))
                    .flatten(),
            })
            .collect();
        counts.workspaces = workspaces.len() as u64;

        let server_id = self.signer.fingerprint();
        let tenant_name = self
            .journal
            .tenant_name(tenant_id)
            .await?
            .map(|n| n.as_str().to_owned())
            .unwrap_or_default();
        let public = self.signer.verifying_key();
        let contents = BundleContents {
            manifest: Manifest {
                format: oml_bundle::FORMAT.to_owned(),
                source: Source {
                    server_id: server_id.clone(),
                    tenant_id: tenant_id.to_string(),
                    tenant_name,
                },
                workspaces: vec![paths.get(&workspace_id).cloned().unwrap_or_default()],
                mode: Mode::History,
                release: None,
                since: None,
                dropped: Vec::new(),
                exported_at: self.clock.now().as_str().to_owned(),
                exporter_pseudonym,
                event_keys: vec![hex_lower(public.as_bytes())],
                files: Vec::new(),
                signature: None,
            },
            workspaces,
            versions,
            verdicts,
            events,
            edges,
            bodies,
        };
        let mut tar = Vec::new();
        let manifest =
            oml_bundle::write_bundle_with(&contents, &public, &|b| self.signer.sign(b), &mut tar)
                .map_err(|e| bundle_failure(&e))?;
        // I13: the bundle an import could take, at most one upload, before
        // anything is encrypted, journalled or metered.
        if tar.len() as u64 > self.uploads.max_bytes {
            return Err(AppError::ExportTooLarge {
                max_bytes: self.uploads.max_bytes,
            });
        }
        let manifest_digest = sha256_digest(
            &serde_json::to_vec(&manifest)
                .map_err(|e| AppError::Store(StoreError::Backend(format!("manifest: {e}"))))?,
        );
        let refs: Vec<&str> = req.recipients.iter().map(String::as_str).collect();
        let mut file = Vec::new();
        let mut writer = crypt::encrypting_writer(&refs, &mut file).map_err(|_| {
            AppError::BundleUnencrypted {
                detail: "the recipients could not be encrypted to".to_owned(),
            }
        })?;
        writer
            .write_all(&tar)
            .and_then(|()| writer.finish().map(drop))
            .map_err(|e| AppError::Store(StoreError::Backend(format!("encrypting: {e}"))))?;
        drop(tar);

        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::WORKSPACE_EXPORTED,
            &key.principal,
            Some(key.key_id),
            json!({
                "workspace_id": workspace_id.to_string(),
                "manifest_digest": manifest_digest,
                "mode": "history",
                "counts": {
                    "workspaces": counts.workspaces,
                    "versions": counts.versions,
                    "erased": counts.erased,
                    "verdicts": counts.verdicts,
                    "events": counts.events,
                    "edges": counts.edges,
                },
                "recipients": recipients,
                "bytes": file.len(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::WorkspaceExported {
                user_root,
                bytes: file.len() as u64,
            },
            None,
        )
        .await?;
        Ok(Exported {
            file,
            manifest_digest,
            counts,
            recipients,
            signer_fingerprint: server_id,
        })
    }

    /// The exporter's own key set (ADR-0084 I6 as written): for a key acting
    /// for a person, that person's keys, web and agent keys alike, and the
    /// key itself; for a session key, also the set its session read
    /// (ADR-0045 d16). A key acting for no person owns only itself.
    async fn export_own_keys(
        &self,
        caller: &Caller,
        user_root: Option<Uuid>,
    ) -> Result<Vec<Uuid>, AppError> {
        let mut own = caller.own_key_ids();
        if let Some(root) = user_root {
            for k in self
                .journal
                .web_user_keys(caller.key.tenant_id, root)
                .await?
            {
                if !own.contains(&k.key_id) {
                    own.push(k.key_id);
                }
            }
        }
        Ok(own)
    }

    /// A key's `used_by`, read once per export; a key the tenant no longer
    /// holds reads as `agent`, the narrowest claim.
    async fn author_of(
        &self,
        keys: &mut BTreeMap<Uuid, Option<KeyRecord>>,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<String, AppError> {
        if let std::collections::btree_map::Entry::Vacant(slot) = keys.entry(key_id) {
            slot.insert(self.journal.key(tenant_id, key_id).await?);
        }
        Ok(keys
            .get(&key_id)
            .and_then(Option::as_ref)
            .map_or(oml_domain::UsedBy::Agent, |k| k.used_by)
            .as_str()
            .to_owned())
    }

    /// One walk of the tenant's journal for the exported versions'
    /// submission events and the verdicts on them. Unindexed: the walk is
    /// linear in the chain, a bottleneck row (ADR-0084 d15).
    async fn export_journal_facts(
        &self,
        tenant_id: Uuid,
        rows: &[MemoryRecord],
    ) -> Result<JournalFacts, AppError> {
        let wanted: BTreeSet<String> = rows.iter().map(|m| m.memory_id.to_string()).collect();
        let mut facts = JournalFacts::default();
        let mut from = 1;
        loop {
            let page = self
                .journal
                .events_range(tenant_id, from, EVENT_PAGE)
                .await?;
            let Some(last) = page.last() else { break };
            from = last.envelope.seq + 1;
            let full = page.len() == EVENT_PAGE;
            for event in page {
                let e = &event.envelope;
                let Some(id) = e.payload["artifact_id"].as_str() else {
                    continue;
                };
                if !wanted.contains(id) {
                    continue;
                }
                match e.event_type.as_str() {
                    event_types::SUBMISSION_REMEMBER | event_types::SUBMISSION_LEARN => {
                        if let Ok(id) = Uuid::parse_str(id) {
                            facts.submissions.insert(id, event);
                        }
                    }
                    event_types::REVIEW_APPROVE | event_types::REVIEW_REJECT => {
                        facts.verdicts.push(event);
                    }
                    _ => {}
                }
            }
            if !full {
                break;
            }
        }
        Ok(facts)
    }

    /// The ratified claim and brief edges whose two ends are exported
    /// (d1, reading I8).
    async fn export_edges(
        &self,
        caller: &Caller,
        nodes: &[Uuid],
        rows: &[MemoryRecord],
        paths: &BTreeMap<Uuid, String>,
    ) -> Result<Vec<EdgeLine>, AppError> {
        let tenant_id = caller.key.tenant_id;
        let exported: BTreeSet<Uuid> = rows
            .iter()
            .filter(|m| m.status != MemoryStatus::Erased)
            .map(|m| m.memory_id)
            .collect();
        let mut out = Vec::new();
        let selection = ClaimEdgeSelection {
            artifact_id: None,
            within: Some(nodes.to_vec()),
            readable_nodes: nodes.to_vec(),
            reader_key_id: caller.key.key_id,
            held_nodes: Vec::new(),
            current_scanner: None,
        };
        let mut after = None;
        loop {
            let page = self
                .journal
                .claim_edges(tenant_id, &selection, after.as_ref(), EDGE_PAGE)
                .await?;
            for edge in &page {
                if edge.state == EdgeState::Ratified
                    && exported.contains(&edge.src_artifact_id)
                    && exported.contains(&edge.dst_artifact_id)
                {
                    out.push(EdgeLine {
                        kind: EdgeKind::Claim,
                        src: edge.src_artifact_id.to_string(),
                        dst: edge.dst_artifact_id.to_string(),
                        label: Some(edge.label.clone()),
                    });
                }
            }
            match page.last() {
                Some(last) if page.len() == EDGE_PAGE => {
                    after = Some(crate::types::ClaimEdgePosition {
                        created_at: last.created_at.clone(),
                        edge_id: last.edge_id,
                    });
                }
                _ => break,
            }
        }
        for node in nodes {
            for edge in self.journal.brief_edges(tenant_id, *node).await? {
                if edge.state == EdgeState::Ratified && exported.contains(&edge.artifact_id) {
                    out.push(EdgeLine {
                        kind: EdgeKind::Brief,
                        src: edge.artifact_id.to_string(),
                        dst: paths.get(node).cloned().unwrap_or_default(),
                        label: None,
                    });
                }
            }
        }
        Ok(out)
    }
}

/// Each subtree node's dotted path: the exported root's path from its
/// ancestry (node to root), then each descendant's parent path and name.
fn workspace_paths(
    ancestry: &[WorkspaceRecord],
    subtree: &[WorkspaceRecord],
    root: Uuid,
) -> BTreeMap<Uuid, String> {
    let root_path = ancestry
        .iter()
        .rev()
        .map(|w| w.name.as_str())
        .collect::<Vec<_>>()
        .join(".");
    let by_id: BTreeMap<Uuid, &WorkspaceRecord> =
        subtree.iter().map(|w| (w.workspace_id, w)).collect();
    let mut paths = BTreeMap::new();
    for w in subtree {
        let mut parts = Vec::new();
        let mut at = Some(w);
        while let Some(node) = at {
            if node.workspace_id == root {
                break;
            }
            parts.push(node.name.as_str());
            at = node.parent_id.and_then(|p| by_id.get(&p).copied());
        }
        parts.push(&root_path);
        parts.reverse();
        paths.insert(w.workspace_id, parts.join("."));
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
    use crate::types::{BootstrapRequest, GrantRequest, IssueKeyRequest};
    use oml_domain::UsedBy;
    use oml_domain::access::Covers;
    use oml_domain::name::Name;

    fn name(text: &str) -> Name {
        Name::parse(text).expect("a fixture name")
    }

    #[tokio::test]
    async fn a_second_export_of_one_tenant_while_one_runs_is_export_busy() {
        let app = App::new(
            MemStore::new(),
            MemSigner::new([7; 32]),
            FixedClock::new(),
            SeqIdGen::new(),
            MarkerScanner::default(),
        );
        let boot = app
            .bootstrap_tenant(&BootstrapRequest {
                tenant_name: name("acme"),
                admin_key_name: name("root-admin"),
                admin_principal: "acme-admin".to_owned(),
            })
            .await
            .expect("bootstrap");
        let person = app
            .issue_key(
                &boot.admin_key.value,
                &IssueKeyRequest {
                    profile: None,
                    role: Role::Reader,
                    name: name("alice-desk"),
                    principal: "alice/desk".to_owned(),
                    used_by: Some(UsedBy::Person),
                    default_workspace: None,
                    crew: None,
                    grants: vec![GrantRequest {
                        node: Reference::parse("alice").expect("a reference"),
                        role: Role::Reader,
                        deny: Vec::new(),
                        covers: Covers::Subtree,
                    }],
                    user: Some(name("alice")),
                    connect_code: false,
                    guardrail_scope: None,
                    expiry: None,
                    requests_per_minute: None,
                    person: None,
                },
            )
            .await
            .expect("a person's key");
        let req = ExportRequest {
            mode: Some("history".to_owned()),
            recipients: vec![
                "age1c7uc8xfckg2ntq76m2tsazw6xqsm9awvg8v4f4g57xqgcvs3e5pq9963un".to_owned(),
            ],
            release: None,
            since: None,
        };
        // I13: the running export holds the tenant's place.
        let running = app.exports_busy.try_enter(boot.tenant_id).expect("enters");
        let err = app
            .export_workspace(
                person.value.as_str(),
                Reference::parse("alice").expect("a reference"),
                &req,
            )
            .await
            .expect_err("one export per tenant");
        assert!(matches!(err, AppError::ExportBusy), "{err:?}");
        drop(running);
        app.export_workspace(
            person.value.as_str(),
            Reference::parse("alice").expect("a reference"),
            &req,
        )
        .await
        .expect("the freed place admits the next export");
    }
}
