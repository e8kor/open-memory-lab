//! The rescan (ADR-0065 d13): an admin key runs the server's scanner over
//! the versions a tenant already stores, a window of the journal at a time,
//! and is told which versions it would refuse or hold, by rule and place,
//! never by value. The read changes nothing and journals nothing: a version
//! it would veto stays where it is, for a person to retire (ADR-0001 R4).
//! The recording rescan (`--record-held`) reads the window's held versions
//! alone: each takes the server's rule set as its `scanned_under`, and any
//! finding it made becomes its own, journalled once per window as
//! `scan.findings_recorded` (ADR-0065 d15; ADR-0074 d28). It moves nothing
//! through review.

use std::collections::BTreeMap;

use oml_domain::anchor as anchor_grammar;
use oml_domain::{MemoryStatus, Role, event_types};
use serde_json::json;
use uuid::Uuid;

use super::{App, require_role, scan_targets, version_finding};
use crate::ports::{
    AdmissionEnvelope, Clock, IdGen, JournalStore, MetadataFold, Parsed, RecordedVersion,
    ScanFinding, Scanner, Signer,
};
use crate::types::{AppError, Bearer, FoundBy, MemoryRecord, VersionFinding};

/// The most journal events one rescan call reads.
pub const RESCAN_MAX_EVENTS: usize = 1000;

/// One version a finding fired on: its ids, version and state, and the
/// finding. No text, claim key or anchor value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescanHit {
    pub artifact_id: Uuid,
    pub workarea_id: Uuid,
    pub version: u32,
    pub state: MemoryStatus,
    /// The version's origin kind (`chat`, `sync`, `web`, `agent`), or
    /// `None` for one stored before origins were recorded: the dry run's
    /// rates per origin (ADR-0074 d18).
    pub origin: Option<&'static str>,
    pub finding: ScanFinding,
    /// The field the findings list stores it under (ADR-0074 d23).
    pub field: String,
}

/// What one rescan window found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescanReport {
    /// The rule set the scan ran, or `None` for a scanner that pins none.
    pub scanner_version: Option<&'static str>,
    /// Versions scanned in this window: every stored state but erased.
    pub scanned: u64,
    /// The versions scanned, counted by origin kind, `none` for a version
    /// with none: the denominators of the dry run's rates (ADR-0074 d18).
    pub scanned_by_origin: BTreeMap<&'static str, u64>,
    /// Every finding, in journal order.
    pub hits: Vec<RescanHit>,
    /// Findings counted by class, rule and whether they veto.
    pub by_rule: Vec<(String, String, bool, u64)>,
    /// The `after_seq` of the next window, or `None` at the chain's head.
    pub next_after_seq: Option<u64>,
    /// Held versions whose findings this call recorded under the server's
    /// rule set; zero for the read, which changes nothing (d15).
    pub recorded: u64,
    /// After this call, the tenant's held versions last scanned under
    /// another rule set: doctor's `scan_held_unrescanned` (ADR-0074 d28).
    pub held_unrescanned: u64,
    /// After this call, every stored version so scanned, erased ones aside:
    /// doctor's `scan_rescan_due`.
    pub rescan_due: u64,
    /// After this call, held versions whose `scan_finding` has no row in
    /// the findings list: doctor's `scan_findings_unlisted` (ADR-0074 d23).
    pub findings_unlisted: u64,
    /// After this call, promoted versions with no approval, which only a
    /// version promoted before migration 0044 can be: doctor's
    /// `approval_unmatched` (ADR-0077 d6).
    pub promoted_unapproved: u64,
}

/// A finding as a version's `scan_finding` carries it, as admission's
/// reason names one: `class: rule at place` (d8).
fn finding_reason(found: &[(String, ScanFinding)]) -> Option<String> {
    (!found.is_empty()).then(|| {
        found
            .iter()
            .map(|(_, f)| format!("{}: {} at {}", f.class, f.rule, f.at))
            .collect::<Vec<_>>()
            .join("; ")
    })
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `GET /v2/admin/rescan`, admin by key role: the submissions journalled
    /// after `after_seq`, at most `limit` events of the journal (capped at
    /// [`RESCAN_MAX_EVENTS`]), rescanned body and anchor, as admission
    /// scans them. Read-only: nothing is journalled or changed.
    pub async fn rescan(
        &self,
        bearer: impl Into<Bearer<'_>>,
        after_seq: u64,
        limit: usize,
    ) -> Result<RescanReport, AppError> {
        self.rescan_window(bearer, after_seq, limit, false).await
    }

    /// `POST /v2/admin/rescan`, admin by key role: the read of
    /// [`App::rescan`], and every held version of the window recorded under
    /// the server's rule set: its `scanned_under` set, any finding the rescan
    /// made kept on it, a finding already on the row kept; journalled once
    /// per window as `scan.findings_recorded` when the window holds any
    /// held version (ADR-0065 d15; ADR-0074 d28). It promotes, rejects and
    /// deletes nothing.
    pub async fn rescan_record_held(
        &self,
        bearer: impl Into<Bearer<'_>>,
        after_seq: u64,
        limit: usize,
    ) -> Result<RescanReport, AppError> {
        self.rescan_window(bearer, after_seq, limit, true).await
    }

    /// The findings the server's rule set makes on what a stored version
    /// serves beside its anchor: the body, the title and each decoded view,
    /// the list admission reads ([`super::scan_targets`]). A source its
    /// format no longer accepts is a hold, so the version stays its
    /// author's (ADR-0065 d15).
    fn stored_findings(&self, tenant_id: Uuid, row: &MemoryRecord) -> Vec<(String, ScanFinding)> {
        let parsed = self
            .formats
            .resolve(&row.format.media_type)
            .map_err(|_| "format.unknown".to_owned())
            .and_then(|resolved| {
                resolved
                    .adapter
                    .validate(&row.body)
                    .map_err(|refusal| refusal.reason.to_owned())
            });
        let mut found = Vec::new();
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(reason) => {
                found.push((
                    "/body".to_owned(),
                    ScanFinding {
                        class: "format.invalid".to_owned(),
                        rule: reason,
                        veto: false,
                        at: "/body".to_owned(),
                        place: None,
                    },
                ));
                Parsed::default()
            }
        };
        // The format's own rule, `secret.keyed_value at <key path>`
        // (ADR-0056 d6): a veto, placed at its key path.
        if let Some(veto) = &parsed.veto {
            let at = veto
                .strip_prefix("secret.keyed_value at ")
                .unwrap_or(veto)
                .to_owned();
            found.push((
                format!("/body/{}", at.replace('.', "/")),
                ScanFinding {
                    class: "secret.keyed_value".to_owned(),
                    rule: "keyed-value".to_owned(),
                    veto: true,
                    at,
                    place: None,
                },
            ));
        }
        for target in scan_targets(row.kind, &row.body, &row.format, &parsed) {
            for mut f in self.scanner.findings(&AdmissionEnvelope {
                tenant_id,
                kind: target.kind,
                body: target.text,
            }) {
                if let Some(at) = target.at {
                    f.at = format!("{} at {at}", f.at);
                }
                found.push((target.field(), f));
            }
        }
        found
    }

    async fn rescan_window(
        &self,
        bearer: impl Into<Bearer<'_>>,
        after_seq: u64,
        limit: usize,
        record_held: bool,
    ) -> Result<RescanReport, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "rescan")?;
        let tenant_id = admin.tenant_id;
        let limit = limit.clamp(1, RESCAN_MAX_EVENTS);
        let events = self
            .journal
            .events_range(tenant_id, after_seq.saturating_add(1), limit)
            .await?;
        let ids: Vec<Uuid> = events
            .iter()
            .filter(|e| {
                matches!(
                    e.envelope.event_type.as_str(),
                    event_types::SUBMISSION_REMEMBER | event_types::SUBMISSION_LEARN
                )
            })
            .filter_map(|e| e.envelope.payload["artifact_id"].as_str())
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect();
        let rows = if ids.is_empty() {
            Vec::new()
        } else {
            self.journal.memories(tenant_id, &ids).await?
        };
        let mut scanned = 0;
        let mut scanned_by_origin: BTreeMap<&'static str, u64> = BTreeMap::new();
        let mut hits = Vec::new();
        let current = self.scanner.version();
        let mut recorded: Vec<(Uuid, Vec<(String, ScanFinding)>)> = Vec::new();
        for row in rows.iter().filter(|r| r.status != MemoryStatus::Erased) {
            scanned += 1;
            let origin = row.origin_kind.map(|o| o.as_str());
            *scanned_by_origin
                .entry(origin.unwrap_or("none"))
                .or_default() += 1;
            let mut found = self.stored_findings(tenant_id, row);
            if let Some(anchor) = row.anchor.as_deref() {
                for (field, value) in anchor.scanned() {
                    if field == "/anchor/path" && anchor_grammar::secret_shaped(value) {
                        found.push((
                            field.to_owned(),
                            ScanFinding {
                                class: "anchor.secret_path".to_owned(),
                                rule: "secret-path".to_owned(),
                                veto: true,
                                at: field.to_owned(),
                                place: None,
                            },
                        ));
                    }
                    // Any finding in an anchor refuses (ADR-0057 d2).
                    for mut f in self.scanner.findings(&AdmissionEnvelope {
                        tenant_id,
                        kind: "anchor",
                        body: value,
                    }) {
                        f.veto = true;
                        f.at = f.at.replacen("/anchor", field, 1);
                        found.push((field.to_owned(), f));
                    }
                }
            }
            if record_held && row.status == MemoryStatus::Held {
                recorded.push((row.memory_id, found.clone()));
            }
            hits.extend(found.into_iter().map(|(field, finding)| RescanHit {
                artifact_id: row.memory_id,
                workarea_id: row.workarea_id,
                version: row.version,
                state: row.status,
                origin,
                finding,
                field,
            }));
        }
        let mut by_rule: BTreeMap<(String, String, bool), u64> = BTreeMap::new();
        for h in &hits {
            *by_rule
                .entry((
                    h.finding.class.clone(),
                    h.finding.rule.clone(),
                    h.finding.veto,
                ))
                .or_default() += 1;
        }
        let next_after_seq = (events.len() == limit)
            .then(|| events.last().map(|e| e.envelope.seq))
            .flatten();
        let recorded_n = recorded.len() as u64;
        if !recorded.is_empty() {
            let prev = self.head_event(tenant_id).await?;
            let event = self.seal(
                prev.as_ref(),
                tenant_id,
                event_types::SCAN_FINDINGS_RECORDED,
                &admin.principal,
                Some(admin.key_id),
                json!({
                    "scanned_under": current,
                    "versions": recorded.iter().map(|(id, found)| json!({
                        "artifact_id": id.to_string(),
                        "findings": found.iter().map(|(_, f)| json!({
                            "class": f.class,
                            "rule": f.rule,
                            "action": if f.veto { "veto" } else { "hold" },
                            "at": f.at,
                        })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                }),
            )?;
            self.commit(
                event,
                MetadataFold::FindingsRecorded {
                    scanned_under: current.map(str::to_owned),
                    versions: recorded
                        .iter()
                        .map(|(id, found)| RecordedVersion {
                            memory_id: *id,
                            summary: finding_reason(found),
                            findings: found
                                .iter()
                                .map(|(field, f)| {
                                    version_finding(f, field.clone(), current, FoundBy::Rescan)
                                })
                                .collect::<Vec<VersionFinding>>(),
                        })
                        .collect(),
                },
                None,
            )
            .await?;
        }
        let backlog = self.journal.scan_backlog(tenant_id, current).await?;
        Ok(RescanReport {
            scanner_version: self.scanner.version(),
            scanned,
            scanned_by_origin,
            hits,
            by_rule: by_rule
                .into_iter()
                .map(|((c, r, v), n)| (c, r, v, n))
                .collect(),
            next_after_seq,
            recorded: recorded_n,
            held_unrescanned: backlog.held_unrescanned,
            rescan_due: backlog.rescan_due,
            findings_unlisted: backlog.findings_unlisted,
            promoted_unapproved: backlog.promoted_unapproved,
        })
    }
}
