//! The diff of a lake's claims against a workarea (ADR-0062 d8, d10, d11): a
//! pure function of the extraction, the key's own listing and the tables the
//! run read, so the dry run's plan is the run's, byte for byte.
//!
//! Phase 1 reads the key's own rows only: another key's claim under a lake key
//! is the server's `claim_key_conflict`, reported by the write. A table
//! recreated under the same path (`recreated`, d8) and the once-a-day cadence
//! of unchanged checks (d11) come with object storage.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::json;

use super::extract::{LakeItem, stem};
use crate::sync::api::Listed;
use crate::sync::hash::{short, text_digest};
use crate::sync::plan::{Anchor, Check, Claim, Entry, Outcome, Plan};

/// A run's fixed inputs.
#[derive(Clone, Debug)]
pub struct Context {
    pub workarea: String,
    pub workarea_id: String,
    pub max_writes: usize,
    /// `--check`: nothing is written.
    pub check: bool,
}

/// What a run found of one table: its key stem and the version read, or why
/// it was not read.
#[derive(Clone, Debug)]
pub enum TableState {
    Read { version: u64 },
    NotRead,
}

fn anchor_of(i: &LakeItem) -> Anchor {
    Anchor {
        repo: None,
        uri: Some(i.uri.clone()),
        rev: i.rev.clone(),
        path: None,
        fragment: i.fragment.clone(),
        hash: Some(i.hash.clone()),
        hasher: Some(i.hasher.to_owned()),
    }
}

fn bare(outcome: Outcome, key: &str, rel: &str) -> Entry {
    Entry {
        outcome,
        claim_key: key.to_owned(),
        anchor: None,
        replaces: None,
        expected_version: None,
        text_digest: None,
        idempotency_key: None,
        check: None,
        note: None,
        writes_this_run: false,
        text: None,
        path: rel.to_owned(),
    }
}

fn check(
    ctx: &Context,
    tip: &Listed,
    key: &str,
    result: &str,
    rev: &str,
    hash: Option<&str>,
) -> Check {
    let mut body = json!({ "rev": rev, "result": result });
    if let Some(h) = hash {
        body["hash"] = json!(h);
    }
    Check {
        artifact_id: tip.artifact_id.clone(),
        result: result.to_owned(),
        idempotency_key: format!(
            "lake:{}:{key}:check:{result}:v{}:{rev}",
            ctx.workarea, tip.version
        ),
        body,
    }
}

/// Which table's claim a key is, by the stems of `tables`: `<stem>/table`,
/// `<stem>/schema`, `<stem>/schema/<n>` or `<stem>/col/<physical name>`.
fn table_of<'a>(key: &str, stems: &'a BTreeMap<String, String>) -> Option<&'a str> {
    stems.iter().find_map(|(s, rel)| {
        let rest = key.strip_prefix(s.as_str())?.strip_prefix('/')?;
        (rest == "table"
            || rest == "schema"
            || rest.starts_with("schema/")
            || rest.starts_with("col/"))
        .then_some(rel.as_str())
    })
}

/// The diff. `tables` maps each discovered table's path to what the run found.
#[must_use]
pub fn diff(
    ctx: &Context,
    items: &[LakeItem],
    own: &[Listed],
    tables: &BTreeMap<String, TableState>,
) -> Plan {
    let stems: BTreeMap<String, String> =
        tables.keys().map(|rel| (stem(rel), rel.clone())).collect();
    let mut by_key: BTreeMap<String, Vec<Listed>> = BTreeMap::new();
    for v in own
        .iter()
        .filter(|v| v.workarea_id.as_deref() == Some(ctx.workarea_id.as_str()))
        .filter(|v| v.claim_key.starts_with("delta/"))
    {
        by_key
            .entry(v.claim_key.clone())
            .or_default()
            .push(v.clone());
    }
    // A claim every version of which was rejected holds no place: its key is
    // proposed again as `new`, under the next version's key.
    let mut next_version: HashMap<String, u64> = HashMap::new();
    by_key.retain(|k, vs| {
        let live = vs
            .iter()
            .any(|v| matches!(v.state.as_str(), "held" | "promoted" | "demoted"));
        if !live {
            next_version.insert(
                k.clone(),
                vs.iter().map(|v| v.version).max().unwrap_or(0) + 1,
            );
        }
        live
    });
    let claims: BTreeMap<&str, Claim> = by_key
        .iter()
        .map(|(k, vs)| (k.as_str(), Claim::of(k, vs)))
        .collect();

    let mut sorted: Vec<(String, u8, usize, Entry)> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (n, i) in items.iter().enumerate() {
        seen.insert(i.claim_key.as_str());
        let entry = match claims.get(i.claim_key.as_str()) {
            None => new_entry(ctx, i, next_version.get(&i.claim_key).copied().unwrap_or(1)),
            Some(claim) => decide(ctx, claim, i),
        };
        sorted.push((i.rel.clone(), 0, n, entry));
    }
    for (n, (key, claim)) in claims.iter().enumerate() {
        if seen.contains(key) {
            continue;
        }
        let Some(rel) = table_of(key, &stems) else {
            // Another table's claim, or one whose table is gone: not this
            // run's to retire (d10; a removed table is the next phase's).
            continue;
        };
        let Some(state) = tables.get(rel) else {
            continue;
        };
        let TableState::Read { version } = state else {
            sorted.push((
                rel.to_owned(),
                1,
                n,
                Entry {
                    note: Some("its table was not read, so the claim is not retired".into()),
                    ..bare(Outcome::Skipped, key, rel)
                },
            ));
            continue;
        };
        if claim.held {
            sorted.push((
                rel.to_owned(),
                1,
                n,
                Entry {
                    note: Some(format!("version {} is held", claim.latest_live)),
                    ..bare(Outcome::Pending, key, rel)
                },
            ));
            continue;
        }
        let Some(tip) = claim.live_tip() else {
            continue;
        };
        let (uri, fragment) = tip
            .anchor
            .as_ref()
            .map(|a| (a.uri.clone().unwrap_or_default(), a.fragment.clone()))
            .unwrap_or_default();
        let rev = version.to_string();
        let shown = fragment.as_deref().unwrap_or("the table");
        let text = format!("Retired: {uri} at version {rev} no longer states {shown}.");
        sorted.push((
            rel.to_owned(),
            1,
            n,
            Entry {
                anchor: Some(Anchor {
                    repo: None,
                    uri: Some(uri),
                    rev: rev.clone(),
                    path: None,
                    fragment,
                    hash: None,
                    hasher: None,
                }),
                replaces: Some(tip.artifact_id.clone()),
                expected_version: Some(claim.latest_live),
                text_digest: Some(text_digest(&text)),
                idempotency_key: Some(format!(
                    "lake:{}:{key}:removed:v{}",
                    ctx.workarea, claim.latest_live
                )),
                check: Some(check(ctx, tip, key, "removed", &rev, None)),
                note: Some(format!("absent at version {rev}")),
                text: Some(text),
                ..bare(Outcome::Removed, key, rel)
            },
        ));
    }
    sorted.sort_by(|a, b| (a.0.as_str(), a.1, a.2).cmp(&(b.0.as_str(), b.1, b.2)));
    let mut entries: Vec<Entry> = sorted.into_iter().map(|(_, _, _, e)| e).collect();
    let mut budget = ctx.max_writes;
    for e in &mut entries {
        if e.outcome.writes() && budget > 0 && !ctx.check {
            e.writes_this_run = true;
            budget -= 1;
        }
    }
    Plan { entries }
}

fn new_entry(ctx: &Context, i: &LakeItem, version: u64) -> Entry {
    // A first proposal keeps one key; a proposal after a rejection names its
    // version, so it never replays the rejected one.
    let at = if version > 1 {
        format!("v{version}:")
    } else {
        String::new()
    };
    Entry {
        anchor: Some(anchor_of(i)),
        text_digest: Some(text_digest(&i.text)),
        idempotency_key: Some(format!(
            "lake:{}:{}:new:{at}{}",
            ctx.workarea,
            i.claim_key,
            short(&i.hash)
        )),
        text: Some(i.text.clone()),
        ..bare(Outcome::New, &i.claim_key, &i.rel)
    }
}

/// The version a tip was last known at: its latest source check, else its
/// anchor.
fn last_rev(tip: &Listed) -> Option<u64> {
    tip.source_checked
        .as_ref()
        .map(|c| c.rev.as_str())
        .or_else(|| tip.anchor.as_ref().and_then(|a| a.rev.as_deref()))
        .and_then(|r| r.parse().ok())
}

fn decide(ctx: &Context, claim: &Claim, i: &LakeItem) -> Entry {
    let key = &claim.key;
    if claim.held {
        return Entry {
            note: Some(format!("version {} is held", claim.latest_live)),
            ..bare(Outcome::Pending, key, &i.rel)
        };
    }
    let Some(tip) = &claim.tip else {
        let why = if claim.demoted {
            "demoted"
        } else {
            "not promoted"
        };
        return Entry {
            note: Some(format!("no promoted version: the claim's latest is {why}")),
            ..bare(Outcome::Diverged, key, &i.rel)
        };
    };
    let digest = text_digest(&i.text);
    let digests_equal = tip.plaintext_digest == digest;
    let tip_hash = tip.anchor.as_ref().and_then(|a| a.hash.as_deref());
    let anchored_here = tip
        .anchor
        .as_ref()
        .is_some_and(|a| a.uri.as_deref() == Some(i.uri.as_str()));
    let version: u64 = i.rev.parse().unwrap_or(0);
    // A version below the one the claim was last known at: the table was
    // rolled back or recreated, which a later phase tells apart (d8).
    if let Some(known) = last_rev(tip)
        && version < known
        && anchored_here
    {
        return Entry {
            replaces: Some(tip.artifact_id.clone()),
            note: Some(format!(
                "the table is at version {version}, below version {known} this claim was last known at"
            )),
            ..bare(Outcome::Rollback, key, &i.rel)
        };
    }
    let changed = |note: Option<String>| Entry {
        anchor: Some(anchor_of(i)),
        replaces: Some(tip.artifact_id.clone()),
        expected_version: Some(claim.latest_live),
        text_digest: Some(digest.clone()),
        idempotency_key: Some(format!(
            "lake:{}:{key}:changed:v{}:{}",
            ctx.workarea,
            claim.latest_live,
            short(&i.hash)
        )),
        check: (!tip.retirement && anchored_here && tip_hash.is_some())
            .then(|| check(ctx, tip, key, "changed", &i.rev, Some(&i.hash))),
        note,
        text: Some(i.text.clone()),
        ..bare(Outcome::Changed, key, &i.rel)
    };
    if tip.retirement {
        return changed(Some(
            "the claim was retired; the table states it again".into(),
        ));
    }
    if !anchored_here {
        return Entry {
            replaces: Some(tip.artifact_id.clone()),
            note: Some("the tip is anchored at another location".into()),
            ..bare(Outcome::Diverged, key, &i.rel)
        };
    }
    if digests_equal && tip_hash == Some(i.hash.as_str()) {
        // Nothing the claim states moved. The version did, perhaps: a check
        // at the new version says so, and a table whose version did not move
        // records nothing.
        let moved = last_rev(tip) != Some(version);
        return Entry {
            replaces: Some(tip.artifact_id.clone()),
            text_digest: Some(digest),
            check: moved.then(|| check(ctx, tip, key, "fresh", &i.rev, Some(&i.hash))),
            ..bare(Outcome::Unchanged, key, &i.rel)
        };
    }
    // The tip's hash is the server's text: its first four characters, or
    // as many as it has.
    let head = |digest: &str| short(digest).chars().take(4).collect::<String>();
    let note = tip_hash.map(|h| format!("{}…→{}…", head(h), head(&i.hash)));
    changed(note)
}
