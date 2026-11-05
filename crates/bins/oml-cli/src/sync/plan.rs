//! The diff of a source's items against a workarea (ADR-0057 d8, d18, d20):
//! a pure function of the extraction, the listings and the run's context,
//! so the dry run's plan is the run's, byte for byte (d9).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use serde_json::{Value, json};

use super::adapter::{EdgeStatement, Item, Kind};
use super::api::{EdgeRow, Listed};
use super::hash::{HASHER, short, text_digest};

/// What the diff decides for one item or listed claim (ADR-0057 d8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    New,
    Changed,
    Moved,
    Removed,
    Unchanged,
    Pending,
    Rollback,
    Diverged,
    Foreign,
    Conflict,
    Refused,
    /// A claim located in a file this run could not read: never retired.
    Skipped,
}

impl Outcome {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Changed => "changed",
            Self::Moved => "moved",
            Self::Removed => "removed",
            Self::Unchanged => "unchanged",
            Self::Pending => "pending",
            Self::Rollback => "rollback",
            Self::Diverged => "diverged",
            Self::Foreign => "foreign",
            Self::Conflict => "conflict",
            Self::Refused => "refused",
            Self::Skipped => "skipped",
        }
    }

    /// Whether the outcome writes a memory.
    #[must_use]
    pub fn writes(self) -> bool {
        matches!(self, Self::New | Self::Changed | Self::Removed)
    }

    /// Whether the outcome makes the run exit 1 (ADR-0057 d9, d20).
    #[must_use]
    pub fn fails(self) -> bool {
        matches!(
            self,
            Self::Diverged | Self::Foreign | Self::Conflict | Self::Rollback | Self::Refused
        )
    }
}

/// An anchor a run sends (ADR-0057 d1, d3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Anchor {
    pub repo: String,
    pub rev: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fragment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hasher: Option<String>,
}

/// A source check a run records on a promoted tip (ADR-0057 d5, d18).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Check {
    pub artifact_id: String,
    pub result: String,
    pub body: Value,
    pub idempotency_key: String,
}

/// One line of the plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub outcome: Outcome,
    pub claim_key: String,
    pub anchor: Option<Anchor>,
    /// The tip a supersession replaces, or the claim's tip otherwise.
    pub replaces: Option<String>,
    pub expected_version: Option<u64>,
    pub text_digest: Option<String>,
    pub idempotency_key: Option<String>,
    pub check: Option<Check>,
    /// Why, for an outcome that writes nothing, or where a span moved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Whether this run sends the write (the --max-writes cap, d9).
    pub writes_this_run: bool,
    #[serde(skip)]
    pub text: Option<String>,
    #[serde(skip)]
    pub path: String,
}

impl Entry {
    /// The body of the write this entry sends.
    #[must_use]
    pub fn write_body(&self, workarea: &str) -> Option<Value> {
        let text = self.text.as_ref()?;
        // Every adapter sync builds reads documents, so every write it sends
        // is of kind doc, derived and never declared by hand (ADR-0063 d2).
        let mut body = json!({ "body": text, "anchor": self.anchor, "origin": { "kind": "doc" } });
        match self.outcome {
            Outcome::New => {
                body["claim_key"] = json!(self.claim_key);
                body["workarea"] = json!(workarea);
            }
            Outcome::Changed => body["expected_version"] = json!(self.expected_version),
            Outcome::Removed => {
                body["expected_version"] = json!(self.expected_version);
                body["retirement"] = json!(true);
            }
            _ => return None,
        }
        Some(body)
    }
}

/// Which listed paths a run answers for: a directory or file prefix, or the
/// named files of a pull request (ADR-0057 d14).
#[derive(Clone, Debug)]
pub enum Scope {
    Prefix(String),
    Files(Vec<String>),
}

impl Scope {
    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        match self {
            Self::Prefix(p) if p.is_empty() => true,
            Self::Prefix(p) => {
                path == p
                    || path
                        .strip_prefix(p.as_str())
                        .is_some_and(|r| r.starts_with('/'))
            }
            Self::Files(files) => files.iter().any(|f| f == path),
        }
    }

    /// The `anchor_path_prefix` that lists this scope.
    #[must_use]
    pub fn prefix(&self) -> String {
        match self {
            Self::Prefix(p) => p.clone(),
            Self::Files(files) => common_dir(files),
        }
    }
}

fn common_dir(files: &[String]) -> String {
    let Some(first) = files.first() else {
        return String::new();
    };
    let mut prefix: Vec<&str> = first.split('/').collect();
    prefix.pop();
    for f in files {
        let parts: Vec<&str> = f.split('/').collect();
        let n = prefix
            .iter()
            .zip(&parts[..parts.len().saturating_sub(1)])
            .take_while(|(a, b)| a == b)
            .count();
        prefix.truncate(n);
    }
    prefix.join("/")
}

/// The run's fixed inputs.
#[derive(Clone, Debug)]
pub struct Context {
    pub workarea: String,
    pub workarea_id: String,
    pub repo: String,
    pub rev: String,
    pub scope: Scope,
    pub max_writes: usize,
    pub adopt: bool,
    pub allow_rollback: bool,
    /// `--check`: every promoted tip of this sync's namespace is compared,
    /// whoever wrote it; any other tip anchored into the source is reported
    /// `foreign` and never marked.
    pub check: bool,
    /// The adapters whose claim keys this run answers for: the workarea's
    /// recorded adapter, and the old one under `--change-adapter`.
    pub namespaces: Vec<Kind>,
    /// Paths read and found unreadable or never read: their claims are
    /// skipped, never retired.
    pub skip_paths: HashSet<String>,
    /// `--exclude` globs: a matching path is out of the source.
    pub exclude: Vec<String>,
}

impl Context {
    /// Whether a path is in this run's source: under its scope and matched
    /// by no `--exclude` glob.
    #[must_use]
    pub fn in_source(&self, path: &str) -> bool {
        self.scope.contains(path)
            && !self
                .exclude
                .iter()
                .any(|g| super::source::glob_match(g, path))
    }
}

/// One extracted item and the path it came from.
#[derive(Clone, Debug)]
pub struct SourceItem {
    pub path: String,
    pub item: Item,
}

/// A claim of the workarea as the listings show it.
#[derive(Clone, Debug)]
struct Claim {
    key: String,
    tip: Option<Listed>,
    held: bool,
    /// Promoted versions older than the tip: (version, anchor hash).
    older: Vec<(u64, String)>,
    /// The greatest version among held and promoted (d19).
    latest_live: u64,
    /// The latest version in any state, for a claim with no tip.
    latest: Option<Listed>,
    /// Whether a version is demoted.
    demoted: bool,
}

impl Claim {
    fn of(key: &str, versions: &[Listed]) -> Self {
        let tip = versions
            .iter()
            .find(|v| v.tip && v.state == "promoted")
            .cloned();
        let held = versions.iter().any(|v| v.state == "held");
        let tip_version = tip.as_ref().map_or(0, |t| t.version);
        let older = versions
            .iter()
            .filter(|v| v.state == "promoted" && v.version < tip_version)
            .filter_map(|v| Some((v.version, v.anchor.as_ref()?.hash.clone()?)))
            .collect();
        let latest_live = versions
            .iter()
            .filter(|v| matches!(v.state.as_str(), "held" | "promoted"))
            .map(|v| v.version)
            .max()
            .unwrap_or(0);
        Self {
            key: key.to_owned(),
            tip,
            held,
            older,
            latest_live,
            latest: versions.iter().max_by_key(|v| v.version).cloned(),
            demoted: versions.iter().any(|v| v.state == "demoted"),
        }
    }

    /// Where the claim's span is: the `moved_to` of its latest check when
    /// that check is `moved`, else its anchor's (ADR-0057 d18).
    fn location(&self) -> Option<(String, Option<String>)> {
        let Some(tip) = self.tip.as_ref() else {
            let a = self.latest.as_ref()?.anchor.as_ref()?;
            return Some((a.path.clone()?, a.fragment.clone()));
        };
        if let Some(c) = &tip.source_checked
            && c.result == "moved"
            && let Some(to) = &c.moved_to
        {
            return Some(to.clone());
        }
        let a = tip.anchor.as_ref()?;
        Some((a.path.clone()?, a.fragment.clone()))
    }

    fn anchored_in(&self, repo: &str) -> bool {
        self.tip
            .as_ref()
            .or(self.latest.as_ref())
            .and_then(|t| t.anchor.as_ref())
            .is_some_and(|a| a.repo.as_deref() == Some(repo))
    }

    /// Located in this repository under the run's scope.
    fn in_scope(&self, ctx: &Context) -> bool {
        self.anchored_in(&ctx.repo) && self.location().is_some_and(|(p, _)| ctx.in_source(&p))
    }

    fn live_tip(&self) -> Option<&Listed> {
        self.tip.as_ref().filter(|t| !t.retirement)
    }
}

/// The plan: every entry in order, the edge statements and the counts.
#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub entries: Vec<Entry>,
}

impl Plan {
    /// The count of each outcome, in the sketch's order, with
    /// `writes_this_run`.
    #[must_use]
    pub fn summary(&self) -> Value {
        let mut counts: BTreeMap<&str, usize> = [
            "new",
            "changed",
            "removed",
            "unchanged",
            "pending",
            "moved",
            "rollback",
            "diverged",
            "foreign",
            "conflict",
            "refused",
            "skipped",
        ]
        .into_iter()
        .map(|k| (k, 0))
        .collect();
        for e in &self.entries {
            *counts.entry(e.outcome.as_str()).or_default() += 1;
        }
        let mut out = serde_json::Map::new();
        for k in [
            "new",
            "changed",
            "removed",
            "unchanged",
            "pending",
            "moved",
            "rollback",
            "diverged",
            "foreign",
            "conflict",
            "refused",
            "skipped",
        ] {
            out.insert(k.to_owned(), json!(counts[k]));
        }
        out.insert(
            "writes_this_run".to_owned(),
            json!(self.entries.iter().filter(|e| e.writes_this_run).count()),
        );
        Value::Object(out)
    }
}

/// The diff (ADR-0057 d8). `own` is the key's own rows in the workarea's
/// subtree in every state; `served` the promoted tips anchored in the
/// repository under the scope.
#[must_use]
pub fn diff(ctx: &Context, items: &[SourceItem], own: &[Listed], served: &[Listed]) -> Plan {
    let in_workarea = |v: &&Listed| v.workarea_id.as_deref() == Some(ctx.workarea_id.as_str());
    let ours = |key: &str| Kind::of_key(key).is_some_and(|k| ctx.namespaces.contains(&k));
    let mut by_key: BTreeMap<String, Vec<Listed>> = BTreeMap::new();
    let own_ids: HashSet<&str> = own.iter().map(|v| v.artifact_id.as_str()).collect();
    for v in own
        .iter()
        .filter(in_workarea)
        .filter(|v| ours(&v.claim_key))
    {
        by_key
            .entry(v.claim_key.clone())
            .or_default()
            .push(v.clone());
    }
    // A claim every version of which was rejected holds no place: its key
    // is proposed again as `new`, under the next version's key (d19).
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
    let mut foreign: HashMap<String, Listed> = HashMap::new();
    let mut cited: Vec<Listed> = Vec::new();
    for v in served.iter().filter(in_workarea) {
        if own_ids.contains(v.artifact_id.as_str()) || by_key.contains_key(&v.claim_key) {
            continue;
        }
        if !ours(&v.claim_key) {
            // Another memory anchored into the source: never this sync's
            // to compare, retire or mark (ADR-0057 d4, UC-SM-01).
            if ctx.check {
                cited.push(v.clone());
            }
            continue;
        }
        if ctx.check {
            by_key.insert(v.claim_key.clone(), vec![v.clone()]);
        } else {
            foreign.insert(v.claim_key.clone(), v.clone());
        }
    }
    let claims: Vec<Claim> = by_key.iter().map(|(k, vs)| Claim::of(k, vs)).collect();
    let claim_index: HashMap<&str, usize> = claims
        .iter()
        .enumerate()
        .map(|(i, c)| (c.key.as_str(), i))
        .collect();

    // Item -> claim, in three passes: by key, by location, by moved span.
    let mut matched: Vec<Option<usize>> = vec![None; items.len()];
    let mut taken = vec![false; claims.len()];
    for (i, si) in items.iter().enumerate() {
        if let Some(&c) = claim_index.get(si.item.claim_key.as_str()) {
            matched[i] = Some(c);
            taken[c] = true;
        }
    }
    let located: HashMap<(String, Option<String>), usize> = claims
        .iter()
        .enumerate()
        .filter(|(c, claim)| !taken[*c] && claim.live_tip().is_some() && claim.in_scope(ctx))
        .filter_map(|(c, claim)| Some((claim.location()?, c)))
        .collect();
    for (i, si) in items.iter().enumerate() {
        if matched[i].is_some() || foreign.contains_key(&si.item.claim_key) {
            continue;
        }
        let place = (si.path.clone(), si.item.fragment.clone());
        if let Some(&c) = located.get(&place)
            && !taken[c]
        {
            matched[i] = Some(c);
            taken[c] = true;
        }
    }
    let digests: Vec<String> = items.iter().map(|si| text_digest(&si.item.text)).collect();
    for (i, si) in items.iter().enumerate() {
        if matched[i].is_some() || foreign.contains_key(&si.item.claim_key) {
            continue;
        }
        let pair = claims.iter().enumerate().find(|(c, claim)| {
            !taken[*c]
                && claim.in_scope(ctx)
                && claim.live_tip().is_some_and(|t| {
                    t.plaintext_digest == digests[i]
                        && t.anchor.as_ref().and_then(|a| a.hash.as_deref())
                            == Some(si.item.hash.as_str())
                })
        });
        if let Some((c, _)) = pair {
            matched[i] = Some(c);
            taken[c] = true;
        }
    }

    let mut sorted: Vec<(String, u8, usize, Entry)> = Vec::new();
    for (i, si) in items.iter().enumerate() {
        let entry = match matched[i] {
            Some(c) => decide(ctx, &claims[c], si, &digests[i]),
            None => match foreign.get(&si.item.claim_key) {
                Some(tip) => Entry {
                    note: Some("the tip was written by another key".to_owned()),
                    replaces: Some(tip.artifact_id.clone()),
                    ..bare(Outcome::Foreign, &si.item.claim_key, &si.path)
                },
                None => new_entry(
                    ctx,
                    si,
                    &digests[i],
                    next_version.get(&si.item.claim_key).copied().unwrap_or(1),
                ),
            },
        };
        sorted.push((si.path.clone(), 0, i, entry));
    }
    for (c, claim) in claims.iter().enumerate() {
        if taken[c] || !claim.in_scope(ctx) {
            continue;
        }
        let Some((path, fragment)) = claim.location() else {
            continue;
        };
        if claim.held {
            sorted.push((path.clone(), 1, c, pending(claim, &path)));
            continue;
        }
        if ctx.skip_paths.contains(&path) {
            sorted.push((
                path.clone(),
                1,
                c,
                Entry {
                    note: Some("its file was not read at this rev, so it is not retired".into()),
                    ..bare(Outcome::Skipped, &claim.key, &path)
                },
            ));
            continue;
        }
        let Some(tip) = claim.live_tip() else {
            continue;
        };
        let anchor = Anchor {
            repo: ctx.repo.clone(),
            rev: ctx.rev.clone(),
            path: path.clone(),
            fragment: fragment.clone(),
            hash: None,
            hasher: None,
        };
        let shown = match &fragment {
            Some(f) => format!("{path}#{f}"),
            None => path.clone(),
        };
        let text = format!(
            "Retired: {} at {} no longer holds {shown}.",
            ctx.repo, ctx.rev
        );
        let entry = Entry {
            anchor: Some(anchor),
            replaces: Some(tip.artifact_id.clone()),
            expected_version: Some(claim.latest_live),
            text_digest: Some(text_digest(&text)),
            idempotency_key: Some(format!(
                "sync:{}:{}:removed:v{}",
                ctx.workarea, claim.key, claim.latest_live
            )),
            check: Some(check(ctx, tip, &claim.key, "removed", None, None)),
            note: Some(format!("absent at {}", &ctx.rev[..ctx.rev.len().min(12)])),
            text: Some(text),
            ..bare(Outcome::Removed, &claim.key, &path)
        };
        sorted.push((path, 1, c, entry));
    }
    for (n, v) in cited.iter().enumerate() {
        let Some(path) = v.anchor.as_ref().and_then(|a| a.path.clone()) else {
            continue;
        };
        if !ctx.in_source(&path) {
            continue;
        }
        sorted.push((
            path.clone(),
            2,
            n,
            Entry {
                replaces: Some(v.artifact_id.clone()),
                note: Some("another memory anchored into the source; not this sync's".into()),
                ..bare(Outcome::Foreign, &v.claim_key, &path)
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

fn bare(outcome: Outcome, key: &str, path: &str) -> Entry {
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
        path: path.to_owned(),
    }
}

fn pending(claim: &Claim, path: &str) -> Entry {
    Entry {
        note: Some(format!("version {} is held", claim.latest_live)),
        ..bare(Outcome::Pending, &claim.key, path)
    }
}

fn anchor_of(ctx: &Context, si: &SourceItem) -> Anchor {
    Anchor {
        repo: ctx.repo.clone(),
        rev: ctx.rev.clone(),
        path: si.path.clone(),
        fragment: si.item.fragment.clone(),
        hash: Some(si.item.hash.clone()),
        hasher: Some(HASHER.to_owned()),
    }
}

fn new_entry(ctx: &Context, si: &SourceItem, digest: &str, version: u64) -> Entry {
    // A first proposal keeps d9's key; a proposal after a rejection names
    // its version, so it never replays the rejected one.
    let at = if version > 1 {
        format!("v{version}:")
    } else {
        String::new()
    };
    Entry {
        anchor: Some(anchor_of(ctx, si)),
        text_digest: Some(digest.to_owned()),
        idempotency_key: Some(format!(
            "sync:{}:{}:new:{at}{}",
            ctx.workarea,
            si.item.claim_key,
            short(&si.item.hash)
        )),
        text: Some(si.item.text.clone()),
        ..bare(Outcome::New, &si.item.claim_key, &si.path)
    }
}

fn check(
    ctx: &Context,
    tip: &Listed,
    key: &str,
    result: &str,
    hash: Option<&str>,
    moved_to: Option<(&str, Option<&str>)>,
) -> Check {
    let mut body = json!({ "rev": ctx.rev, "result": result });
    if let Some(h) = hash {
        body["hash"] = json!(h);
    }
    if let Some((path, fragment)) = moved_to {
        body["path"] = json!(path);
        if let Some(f) = fragment {
            body["fragment"] = json!(f);
        }
    }
    Check {
        artifact_id: tip.artifact_id.clone(),
        result: result.to_owned(),
        idempotency_key: format!(
            "sync:{}:{key}:check:{result}:v{}:{}",
            ctx.workarea,
            tip.version,
            &ctx.rev[..ctx.rev.len().min(16)]
        ),
        body,
    }
}

/// The outcome for an item and the claim it matched.
fn decide(ctx: &Context, claim: &Claim, si: &SourceItem, digest: &str) -> Entry {
    let key = &claim.key;
    if claim.held {
        return pending(claim, &si.path);
    }
    let Some(tip) = &claim.tip else {
        let why = if claim.demoted {
            "demoted"
        } else {
            "not promoted"
        };
        return Entry {
            note: Some(format!("no promoted version: the claim's latest is {why}")),
            ..bare(Outcome::Diverged, key, &si.path)
        };
    };
    let changed = |note: Option<String>| Entry {
        anchor: Some(anchor_of(ctx, si)),
        replaces: Some(tip.artifact_id.clone()),
        expected_version: Some(claim.latest_live),
        text_digest: Some(digest.to_owned()),
        idempotency_key: Some(format!(
            "sync:{}:{key}:changed:v{}:{}",
            ctx.workarea,
            claim.latest_live,
            short(&si.item.hash)
        )),
        // A check names a hash only where the anchor carries one, in this
        // repository (the source-check route refuses it otherwise).
        check: (!tip.retirement
            && tip
                .anchor
                .as_ref()
                .is_some_and(|a| a.repo.as_deref() == Some(ctx.repo.as_str()) && a.hash.is_some()))
        .then(|| check(ctx, tip, key, "changed", Some(&si.item.hash), None)),
        note,
        text: Some(si.item.text.clone()),
        ..bare(Outcome::Changed, key, &si.path)
    };
    if tip.retirement {
        return changed(Some(
            "the claim was retired; the source states it again".into(),
        ));
    }
    let anchored_here = tip
        .anchor
        .as_ref()
        .is_some_and(|a| a.repo.as_deref() == Some(ctx.repo.as_str()));
    if !anchored_here {
        if ctx.adopt && !ctx.check {
            return changed(Some(
                "adopted: the tip had no anchor in this repository".into(),
            ));
        }
        return Entry {
            replaces: Some(tip.artifact_id.clone()),
            note: Some(match &tip.anchor {
                None => "the tip has no anchor; --adopt supersedes it".into(),
                Some(_) => {
                    "the tip is anchored in another repository; --adopt supersedes it".into()
                }
            }),
            ..bare(Outcome::Diverged, key, &si.path)
        };
    }
    let tip_hash = tip.anchor.as_ref().and_then(|a| a.hash.as_deref());
    let place = (si.path.clone(), si.item.fragment.clone());
    let location = claim.location();
    let same_text = tip.plaintext_digest == digest;
    let same_hash = tip_hash == Some(si.item.hash.as_str());
    let moved_check = |c: &Claim| {
        check(
            ctx,
            tip,
            &c.key,
            "moved",
            Some(&si.item.hash),
            Some((&si.path, si.item.fragment.as_deref())),
        )
    };
    if same_text && same_hash {
        let at_anchor = tip.anchor.as_ref().is_some_and(|a| {
            a.path.as_deref() == Some(si.path.as_str()) && a.fragment == si.item.fragment
        });
        if at_anchor {
            return Entry {
                replaces: Some(tip.artifact_id.clone()),
                text_digest: Some(digest.to_owned()),
                check: Some(check(ctx, tip, key, "fresh", Some(&si.item.hash), None)),
                ..bare(Outcome::Unchanged, key, &si.path)
            };
        }
        if location.as_ref() == Some(&place) {
            // Already recorded as moved here: the check states it again, so
            // the claim keeps its location (d18).
            return Entry {
                replaces: Some(tip.artifact_id.clone()),
                text_digest: Some(digest.to_owned()),
                check: Some(moved_check(claim)),
                ..bare(Outcome::Unchanged, key, &si.path)
            };
        }
        let shown = location_text(&place);
        return Entry {
            replaces: Some(tip.artifact_id.clone()),
            text_digest: Some(digest.to_owned()),
            check: Some(moved_check(claim)),
            note: Some(format!("moved to {shown}")),
            ..bare(Outcome::Moved, key, &si.path)
        };
    }
    if let Some((version, _)) = claim.older.iter().rev().find(|(_, h)| *h == si.item.hash) {
        if !ctx.allow_rollback {
            return Entry {
                replaces: Some(tip.artifact_id.clone()),
                text_digest: Some(digest.to_owned()),
                note: Some(format!(
                    "the source at this rev equals version {version}, older than the tip v{}; \
                     --allow-rollback writes it",
                    tip.version
                )),
                ..bare(Outcome::Rollback, key, &si.path)
            };
        }
        return changed(Some(format!("a revert to version {version}'s source")));
    }
    let note = tip_hash.map(|h| format!("{}…→{}…", &short(h)[..4], &short(&si.item.hash)[..4]));
    changed(note)
}

fn location_text(place: &(String, Option<String>)) -> String {
    match &place.1 {
        Some(f) => format!("{}#{f}", place.0),
        None => place.0.clone(),
    }
}

/// An edge statement's state in a run (ADR-0057 d10, d11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EdgeLine {
    pub from: String,
    pub label: String,
    pub to: String,
    /// `unresolved`, `waiting`, `proposed`, `pending`, `ratified` or
    /// `unstated`.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge_id: Option<String>,
    #[serde(skip)]
    pub from_artifact: Option<String>,
    #[serde(skip)]
    pub to_artifact: Option<String>,
    #[serde(skip)]
    pub proposer_key_id: Option<String>,
}

/// Each statement against the promoted tips and the workarea's claim edges:
/// `unresolved` when an end is in neither the source nor the workarea,
/// `waiting` until both ends' tips are promoted, then `proposed` (to be
/// proposed), or the existing edge's state; and each existing edge between
/// two of the source's tips that no statement backs, `unstated`.
#[must_use]
pub fn edge_lines(
    statements: &[EdgeStatement],
    known: &HashSet<String>,
    tips: &HashMap<String, String>,
    edges: &[EdgeRow],
) -> Vec<EdgeLine> {
    let mut out = Vec::new();
    let mut backed = HashSet::new();
    for s in statements {
        let line = |state: &str| EdgeLine {
            from: s.from.clone(),
            label: s.label.clone(),
            to: s.to.clone(),
            state: state.to_owned(),
            edge_id: None,
            from_artifact: tips.get(&s.from).cloned(),
            to_artifact: tips.get(&s.to).cloned(),
            proposer_key_id: None,
        };
        if !known.contains(&s.from) || !known.contains(&s.to) {
            out.push(line("unresolved"));
            continue;
        }
        let (Some(from), Some(to)) = (tips.get(&s.from), tips.get(&s.to)) else {
            out.push(line("waiting"));
            continue;
        };
        match edges
            .iter()
            .find(|e| &e.from_artifact_id == from && &e.to_artifact_id == to && e.label == s.label)
        {
            Some(e) => {
                backed.insert(e.edge_id.clone());
                out.push(EdgeLine {
                    edge_id: Some(e.edge_id.clone()),
                    proposer_key_id: Some(e.proposer_key_id.clone()),
                    ..line(&e.state)
                });
            }
            None => out.push(line("proposed")),
        }
    }
    let by_artifact: HashMap<&str, &str> =
        tips.iter().map(|(k, a)| (a.as_str(), k.as_str())).collect();
    let labels: HashSet<&str> = statements
        .iter()
        .map(|s| s.label.as_str())
        .chain(["amends", "supersedes"])
        .collect();
    for e in edges {
        if backed.contains(&e.edge_id) || !labels.contains(e.label.as_str()) {
            continue;
        }
        if let (Some(from), Some(to)) = (
            by_artifact.get(e.from_artifact_id.as_str()),
            by_artifact.get(e.to_artifact_id.as_str()),
        ) {
            out.push(EdgeLine {
                from: (*from).to_owned(),
                label: e.label.clone(),
                to: (*to).to_owned(),
                state: "unstated".to_owned(),
                edge_id: Some(e.edge_id.clone()),
                from_artifact: Some(e.from_artifact_id.clone()),
                to_artifact: Some(e.to_artifact_id.clone()),
                proposer_key_id: Some(e.proposer_key_id.clone()),
            });
        }
    }
    out
}
