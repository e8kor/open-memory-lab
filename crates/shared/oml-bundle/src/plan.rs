//! The import plan (d8, d13): a pure function of a verified bundle and the
//! caller's view of the target workspace. It writes nothing and reads no
//! store; the server builds the view, and `--dry-run` prints the plan.

use std::collections::{BTreeMap, BTreeSet};

use crate::BundleError;
use crate::lines::{State, VersionEntry, VersionLine};
use crate::manifest::{Manifest, Mode};

/// The target's latest version of one claim key, held versions included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetTip {
    pub artifact_id: String,
    pub version: u64,
    pub plaintext_digest: String,
    pub retirement: bool,
    /// The target's ledger says an import from this source wrote this very
    /// version: nobody changed the key on the target since.
    pub written_by_last_import: bool,
    /// An import from this source has ever written this key.
    pub imported_from_this_source: bool,
}

/// The target ledger's newest import from this source (d8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    /// The newest export instant imported, in the chain's `at` form.
    pub exported_at: String,
    /// The digest of the newest release manifest imported, if any.
    pub release_manifest_digest: Option<String>,
}

/// The caller's view of the target workspace, for one source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetView {
    /// The target's tip of each claim key.
    pub tips: BTreeMap<String, TargetTip>,
    pub ledger: Option<Ledger>,
    /// The source (artifact id, version) pairs already imported, for d13's
    /// erasure notices.
    pub imported: BTreeSet<(String, u64)>,
}

/// One claim key's outcome (d8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// Written held.
    New,
    /// A held supersession of the target tip, with `expected_version`.
    Changed,
    /// A held retirement.
    Removed,
    /// Nothing is written.
    Unchanged,
    /// Nothing is written: the caller's format diff finds them equivalent.
    Equivalent,
    /// The target tip changed on the target since the last import, or never
    /// came from it: a held supersession marked `conflict`. The tip is never
    /// written over.
    Conflict,
}

impl Outcome {
    /// The outcome's name as the plan prints it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Changed => "changed",
            Self::Removed => "removed",
            Self::Unchanged => "unchanged",
            Self::Equivalent => "equivalent",
            Self::Conflict => "conflict",
        }
    }
}

/// One planned claim key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedKey {
    pub claim_key: String,
    pub outcome: Outcome,
    /// The bundle's tip of the key, as (artifact id, version); none for a
    /// key the bundle dropped.
    pub source: Option<(String, u64)>,
    /// The target tip's version a write names (d8), if the key exists there.
    pub expected_version: Option<u64>,
}

/// The whole plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// By claim key.
    pub keys: Vec<PlannedKey>,
    /// Markers for versions already imported: each raises
    /// `import.erasure_notice` (d13).
    pub erasure_notices: Vec<(String, u64)>,
    /// The import passed `allow_older` over a newer ledger (d8).
    pub allow_older_used: bool,
}

impl Plan {
    /// How many keys take `outcome`.
    #[must_use]
    pub fn count(&self, outcome: Outcome) -> usize {
        self.keys.iter().filter(|k| k.outcome == outcome).count()
    }
}

/// Plans an import of `manifest` and `versions` into the target `view`
/// (d8). `equivalent` is the caller's format diff (ADR-0056 d11); this
/// crate links no format adapter. Refuses `import.bundle_older` unless
/// `allow_older`, and `import.since_mismatch`, before any key is planned.
pub fn plan_import(
    manifest: &Manifest,
    versions: &[VersionEntry],
    view: &TargetView,
    allow_older: bool,
    equivalent: &dyn Fn(&VersionLine, &TargetTip) -> bool,
) -> Result<Plan, BundleError> {
    let mut allow_older_used = false;
    if let Some(ledger) = &view.ledger
        && ledger.exported_at.as_str() > manifest.exported_at.as_str()
    {
        if !allow_older {
            return Err(BundleError::BundleOlder {
                exported_at: manifest.exported_at.clone(),
                ledger_at: ledger.exported_at.clone(),
            });
        }
        allow_older_used = true;
    }
    if let Some(since) = &manifest.since {
        let base = view
            .ledger
            .as_ref()
            .and_then(|l| l.release_manifest_digest.as_deref());
        if base != Some(since.manifest_digest.as_str()) {
            return Err(BundleError::SinceMismatch {
                detail: format!(
                    "the bundle is based on {}, the target last imported {}",
                    since.label,
                    base.unwrap_or("no release")
                ),
            });
        }
    }

    let tips = bundle_tips(versions);
    let dropped: BTreeSet<&str> = manifest.dropped.iter().map(String::as_str).collect();
    let full_release = manifest.mode == Mode::Release && manifest.since.is_none();
    let mut keys: BTreeSet<&str> = tips.keys().copied().collect();
    keys.extend(dropped.iter().copied());
    if full_release {
        keys.extend(
            view.tips
                .iter()
                .filter(|(_, t)| t.imported_from_this_source)
                .map(|(k, _)| k.as_str()),
        );
    }

    let mut planned = Vec::new();
    for key in keys {
        let source = tips.get(key).copied();
        let target = view.tips.get(key);
        let removed = match source {
            Some(v) => v.retirement,
            None => {
                dropped.contains(key)
                    || (full_release && target.is_some_and(|t| t.imported_from_this_source))
            }
        };
        let outcome = match (target, removed) {
            (None, true) => Outcome::Unchanged,
            (None, false) => Outcome::New,
            (Some(t), true) if t.retirement => Outcome::Unchanged,
            (Some(t), true) if !t.written_by_last_import => Outcome::Conflict,
            (Some(_), true) => Outcome::Removed,
            (Some(t), false) => {
                let v = source.unwrap_or_else(|| unreachable!("a key not removed has a tip"));
                if v.plaintext_digest == t.plaintext_digest && v.retirement == t.retirement {
                    Outcome::Unchanged
                } else if !t.written_by_last_import {
                    Outcome::Conflict
                } else if equivalent(v, t) {
                    Outcome::Equivalent
                } else {
                    Outcome::Changed
                }
            }
        };
        planned.push(PlannedKey {
            claim_key: key.to_owned(),
            outcome,
            source: source.map(|v| (v.artifact_id.clone(), v.version)),
            expected_version: target.map(|t| t.version),
        });
    }

    let erasure_notices = versions
        .iter()
        .filter_map(|e| match e {
            VersionEntry::Erased(m)
                if view.imported.contains(&(m.artifact_id.clone(), m.version)) =>
            {
                Some((m.artifact_id.clone(), m.version))
            }
            _ => None,
        })
        .collect();

    Ok(Plan {
        keys: planned,
        erasure_notices,
        allow_older_used,
    })
}

/// The bundle's tip of each claim key: its newest promoted version, else its
/// newest held one (the exporter's own unreviewed work, d2). Rejected,
/// demoted and withdrawn versions are never a tip; erased versions carry no
/// claim key (d13).
fn bundle_tips(versions: &[VersionEntry]) -> BTreeMap<&str, &VersionLine> {
    let mut tips: BTreeMap<&str, &VersionLine> = BTreeMap::new();
    let rank = |v: &VersionLine| (v.state == State::Promoted, v.version);
    for entry in versions {
        let VersionEntry::Version(v) = entry else {
            continue;
        };
        if !matches!(v.state, State::Promoted | State::Held) {
            continue;
        }
        let slot = tips.entry(v.claim_key.as_str()).or_insert(v);
        if rank(v) > rank(slot) {
            *slot = v;
        }
    }
    tips
}
