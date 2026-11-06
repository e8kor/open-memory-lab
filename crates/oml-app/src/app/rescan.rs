//! The rescan (ADR-0065 d13): an admin key runs the server's scanner over
//! the versions a tenant already stores, a window of the journal at a time,
//! and is told which versions it would refuse or hold, by rule and place,
//! never by value. It changes nothing and journals nothing: a version it
//! would veto stays where it is, for a person to retire (ADR-0001 R4).

use std::collections::BTreeMap;

use oml_domain::anchor as anchor_grammar;
use oml_domain::{MemoryStatus, Role, event_types};
use uuid::Uuid;

use super::{App, require_role};
use crate::ports::{AdmissionEnvelope, Clock, IdGen, JournalStore, ScanFinding, Scanner, Signer};
use crate::types::{AppError, Bearer};

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
    pub finding: ScanFinding,
}

/// What one rescan window found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescanReport {
    /// The rule set the scan ran, or `None` for a scanner that pins none.
    pub scanner_version: Option<&'static str>,
    /// Versions scanned in this window: every stored state but erased.
    pub scanned: u64,
    /// Every finding, in journal order.
    pub hits: Vec<RescanHit>,
    /// Findings counted by class, rule and whether they veto.
    pub by_rule: Vec<(String, String, bool, u64)>,
    /// The `after_seq` of the next window, or `None` at the chain's head.
    pub next_after_seq: Option<u64>,
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
        let mut hits = Vec::new();
        for row in rows.iter().filter(|r| r.status != MemoryStatus::Erased) {
            scanned += 1;
            let mut found = self.scanner.findings(&AdmissionEnvelope {
                tenant_id,
                kind: row.kind.as_str(),
                body: &row.body,
            });
            if let Some(anchor) = row.anchor.as_deref() {
                for (field, value) in anchor.scanned() {
                    if field == "/anchor/path" && anchor_grammar::secret_shaped(value) {
                        found.push(ScanFinding {
                            class: "anchor.secret_path".to_owned(),
                            rule: "secret-path".to_owned(),
                            veto: true,
                            at: field.to_owned(),
                        });
                    }
                    // Any finding in an anchor refuses (ADR-0057 d2).
                    for mut f in self.scanner.findings(&AdmissionEnvelope {
                        tenant_id,
                        kind: "anchor",
                        body: value,
                    }) {
                        f.veto = true;
                        f.at = f.at.replacen("/anchor", field, 1);
                        found.push(f);
                    }
                }
            }
            hits.extend(found.into_iter().map(|finding| RescanHit {
                artifact_id: row.memory_id,
                workarea_id: row.workarea_id,
                version: row.version,
                state: row.status,
                finding,
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
        Ok(RescanReport {
            scanner_version: self.scanner.version(),
            scanned,
            hits,
            by_rule: by_rule
                .into_iter()
                .map(|((c, r, v), n)| (c, r, v, n))
                .collect(),
            next_after_seq,
        })
    }
}
