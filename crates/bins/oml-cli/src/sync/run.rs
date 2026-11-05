//! `oml sync`: read a source at one commit, diff it against a workarea and
//! write the plan, or check it, or propose and ratify its edges (ADR-0057
//! d6–d11, d14). Every call goes through [`SyncApi`], so a run is tested
//! against a fake.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;

use serde_json::{Value, json};

use super::adapter::{EdgeStatement, Kind};
use super::api::{ListQuery, Listed, Me, SyncApi};
use super::hash::HASHER;
use super::plan::{Context, EdgeLine, Entry, Outcome, Plan, Scope, SourceItem, diff, edge_lines};
use super::source::{Git, check_repo, is_secret_shaped, repo_of_remote};
use crate::client::CliError;

/// The request cap of a bulk verdict, and so a run's default write cap
/// (ADR-0055 d3, ADR-0057 d9).
pub const MAX_WRITES: usize = 200;

/// What a run was asked to do.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub source: String,
    pub workarea: String,
    pub adapter: Option<Kind>,
    pub rev: String,
    pub repo: Option<String>,
    pub dry_run: bool,
    pub check: bool,
    pub record: bool,
    pub json: bool,
    pub max_writes: usize,
    pub adopt: bool,
    pub retire_all: bool,
    pub edges: bool,
    pub ratify: bool,
    pub allow_rollback: bool,
    /// Drafts mode: the pull request whose records load into
    /// `<workarea>-pr-<n>` (ADR-0057 d14).
    pub pr: Option<u64>,
    pub base: String,
}

impl Options {
    /// The workarea the run diffs: `<workarea>-pr-<n>` in drafts mode.
    #[must_use]
    pub fn target_workarea(&self) -> String {
        match self.pr {
            Some(n) => format!("{}-pr-{n}", self.workarea),
            None => self.workarea.clone(),
        }
    }
}

/// A source read at one commit.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub repo: String,
    pub commit: String,
    pub path: String,
    pub scope: Scope,
    pub adapter: &'static str,
    pub items: Vec<SourceItem>,
    pub edges: Vec<EdgeStatement>,
    /// Files refused or unreadable, with why.
    pub unreadable: Vec<(String, String)>,
    /// Lines for standard error: a stripped credential, for one.
    pub warnings: Vec<String>,
}

/// Reads the source of `opts` from the clone around `cwd` (ADR-0057 d6,
/// d7): the tree at `--rev`, never the working tree.
///
/// # Errors
/// A source outside a clone, a rev that is no commit, a repository name
/// that cannot be derived.
pub fn load(cwd: &Path, opts: &Options) -> Result<Loaded, String> {
    let start = {
        let p = cwd.join(&opts.source);
        let mut dir = p.clone();
        while !dir.is_dir() {
            if !dir.pop() {
                dir = cwd.to_path_buf();
                break;
            }
        }
        dir
    };
    let git = Git::at(&start)?;
    let path = git.relative(cwd, &opts.source)?;
    let commit = git.commit(&opts.rev)?;
    let mut warnings = Vec::new();
    let repo = match &opts.repo {
        Some(r) => {
            check_repo(r)?;
            r.clone()
        }
        None => {
            let url = git
                .origin_url()
                .ok_or("the clone has no origin remote; pass --repo (ADR-0057 d3)")?;
            let remote = repo_of_remote(&url)?;
            if remote.had_credentials {
                warnings.push(
                    "warn: the origin remote's URL carries credentials; sync strips them from the \
                     anchor, and they should be removed from .git/config (ADR-0057 d17)"
                        .to_owned(),
                );
            }
            remote.repo
        }
    };
    let (files, scope) = match opts.pr {
        Some(_) if !opts.retire_all => {
            let files = git.changed_files(&opts.base, &commit, &path)?;
            (files.clone(), Scope::Files(files))
        }
        _ => (git.files(&commit, &path)?, Scope::Prefix(path.clone())),
    };
    let kind = opts.adapter.unwrap_or_else(|| Kind::default_for(&files));
    let adapter = kind.adapter();
    let mut items = Vec::new();
    let mut edges = Vec::new();
    let mut unreadable = Vec::new();
    if !opts.retire_all {
        for file in files.iter().filter(|f| adapter.accepts(f)) {
            if is_secret_shaped(file) {
                unreadable.push((
                    file.clone(),
                    "its name matches a secret pattern, so it is never read (ADR-0057 d6)".into(),
                ));
                continue;
            }
            let bytes = git.read(&commit, file)?;
            match adapter.extract(file, &bytes) {
                Ok(x) => {
                    items.extend(x.items.into_iter().map(|item| SourceItem {
                        path: file.clone(),
                        item,
                    }));
                    edges.extend(x.edges);
                }
                Err(why) => unreadable.push((file.clone(), why)),
            }
        }
    }
    Ok(Loaded {
        repo,
        commit,
        path,
        scope,
        adapter: adapter.name(),
        items,
        edges,
        unreadable,
        warnings,
    })
}

/// A run's result: its exit code and its JSON document.
#[derive(Debug)]
pub struct Report {
    pub exit: u8,
    pub json: Value,
}

/// Runs `opts` over `loaded` through `api`, printing to `out` unless
/// `--json`.
///
/// # Errors
/// A call fails without a typed refusal, or the server does not accept
/// anchors.
pub fn execute(
    api: &dyn SyncApi,
    loaded: &Loaded,
    opts: &Options,
    out: &mut dyn Write,
) -> Result<Report, CliError> {
    let workarea = opts.target_workarea();
    let workarea_id = api.workarea_id(&workarea)?;
    let me = api.me()?;
    let ctx = Context {
        workarea: workarea.clone(),
        workarea_id: workarea_id.clone(),
        repo: loaded.repo.clone(),
        rev: loaded.commit.clone(),
        scope: loaded.scope.clone(),
        max_writes: opts.max_writes,
        adopt: opts.adopt,
        allow_rollback: opts.allow_rollback,
        check: opts.check,
    };
    let served = list_served(api, &ctx)?;
    let mut p = Printer {
        out,
        quiet: opts.json,
    };
    p.line(&format!(
        "source    {} @ {}  {}  adapter {}  hasher {HASHER}",
        loaded.repo,
        &loaded.commit[..12.min(loaded.commit.len())],
        if loaded.path.is_empty() {
            "."
        } else {
            &loaded.path
        },
        loaded.adapter
    ));
    p.line(&format!("workarea  {workarea}  key {}", me.name));
    for (file, why) in &loaded.unreadable {
        p.line(&format!("unreadable {file}: {why}"));
    }
    if opts.edges {
        return run_edges(api, loaded, opts, &ctx, &me, &served, &mut p);
    }
    let own = api.list(&ListQuery {
        workarea: workarea.clone(),
        own: true,
        ..ListQuery::default()
    })?;
    let mut plan = if opts.retire_all {
        retire_all(&ctx, &own)
    } else {
        diff(&ctx, &loaded.items, &own, &served)
    };
    let tips = tips_of(&ctx, &served);
    let known: HashSet<String> = loaded
        .items
        .iter()
        .map(|si| si.item.claim_key.clone())
        .chain(served.iter().map(|v| v.claim_key.clone()))
        .chain(own.iter().map(|v| v.claim_key.clone()))
        .collect();
    let existing = if opts.check {
        api.claim_edges(&workarea_id)?
    } else {
        Vec::new()
    };
    let edges = edge_lines(&loaded.edges, &known, &tips, &existing);
    if opts.check {
        return run_check(api, loaded, opts, &plan, &edges, &mut p);
    }
    if !opts.dry_run {
        write_plan(api, &mut plan, &workarea)?;
    }
    for e in &plan.entries {
        p.line(&entry_line(e));
    }
    let summary = plan.summary();
    p.line(&summary_line(&summary, opts));
    p.line(&edge_summary(&edges));
    let wrote = plan.entries.iter().any(|e| e.writes_this_run);
    if !opts.dry_run && wrote {
        p.line(&format!(
            "review    oml review list --workarea {workarea} --author-key {}",
            me.name
        ));
        p.line(&format!(
            "          oml review list --workarea {workarea} --author-key {} --ids-only | oml review approve --ids -",
            me.name
        ));
    }
    let failed = !loaded.unreadable.is_empty() || plan.entries.iter().any(|e| e.outcome.fails());
    Ok(Report {
        exit: u8::from(failed),
        json: plan_json(loaded, &workarea, &plan, &edges, &summary),
    })
}

/// The promoted tips anchored in the repository under the source's path,
/// listed with the anchor filters, which a server before v0.5.8 refuses
/// (ADR-0057 d4, d17).
fn list_served(api: &dyn SyncApi, ctx: &Context) -> Result<Vec<Listed>, CliError> {
    api.list(&ListQuery {
        workarea: ctx.workarea.clone(),
        own: false,
        anchor_repo: Some(ctx.repo.clone()),
        anchor_path_prefix: Some(ctx.scope.prefix()).filter(|p| !p.is_empty()),
    })
    .map_err(|e| match e {
        CliError::Refused { code, .. } if code == "request.validation" => CliError::local(
            "the server does not accept anchors: its listing refused anchor_repo, so it is older \
             than v0.5.8; nothing was read or written (ADR-0057 d17)",
        ),
        other => other,
    })
}

/// Claim key → the artifact id of its live promoted tip in the workarea.
fn tips_of(ctx: &Context, served: &[Listed]) -> HashMap<String, String> {
    served
        .iter()
        .filter(|v| {
            v.workarea_id.as_deref() == Some(ctx.workarea_id.as_str())
                && v.tip
                && v.state == "promoted"
                && !v.retirement
        })
        .map(|v| (v.claim_key.clone(), v.artifact_id.clone()))
        .collect()
}

/// Sends each source check and each write this run carries, in order,
/// recording refusals on the entry (ADR-0057 d8, d9).
fn write_plan(api: &dyn SyncApi, plan: &mut Plan, workarea: &str) -> Result<(), CliError> {
    for e in &mut plan.entries {
        if let Some(c) = &e.check {
            match api.source_check(&c.artifact_id, &c.body, &c.idempotency_key) {
                Ok(_) => {}
                Err(CliError::Refused { code, message, .. }) => {
                    e.note = Some(format!("source check refused: {code} — {message}"));
                    e.outcome = Outcome::Refused;
                    e.writes_this_run = false;
                    continue;
                }
                Err(other) => return Err(other),
            }
        }
        if !e.writes_this_run {
            continue;
        }
        let Some(body) = e.write_body(workarea) else {
            continue;
        };
        let key = e.idempotency_key.clone().unwrap_or_default();
        let result = match (&e.outcome, &e.replaces) {
            (Outcome::New, _) => api.submit(&body, &key),
            (_, Some(target)) => api.supersede(target, &body, &key),
            (_, None) => continue,
        };
        match result {
            Ok(_) => {}
            Err(CliError::Refused { code, message, .. }) => {
                e.writes_this_run = false;
                if code == "submission.claim_key_conflict" {
                    e.outcome = Outcome::Conflict;
                    e.note = Some("another key holds the claim key".into());
                } else {
                    e.outcome = Outcome::Refused;
                    e.note = Some(format!("{code} — {message}"));
                }
            }
            Err(other) => return Err(other),
        }
    }
    Ok(())
}

/// `--retire-all`: a retirement for every live claim the key wrote in the
/// workarea under the source, and `pending` for one with a held version
/// (ADR-0057 d14).
fn retire_all(ctx: &Context, own: &[Listed]) -> Plan {
    // The diff with no items retires exactly that set: every claim under the
    // scope is `removed`, every held one `pending`.
    let mut plan = diff(ctx, &[], own, &[]);
    for e in &mut plan.entries {
        if e.outcome == Outcome::Removed {
            e.check = None;
            e.note = Some("the draft's pull request closed".into());
            if let Some(k) = &mut e.idempotency_key {
                *k = k.replacen(":removed:", ":retire-all:", 1);
            }
        }
    }
    plan
}

/// `--check` (ADR-0057 d11): each claim's state and each edge's, exit 1 on
/// `stale`, `removed` or `untracked`; `--record` records the source checks.
fn run_check(
    api: &dyn SyncApi,
    loaded: &Loaded,
    opts: &Options,
    plan: &Plan,
    edges: &[EdgeLine],
    p: &mut Printer<'_>,
) -> Result<Report, CliError> {
    let mut rows = Vec::new();
    let mut failing = !loaded.unreadable.is_empty();
    for e in &plan.entries {
        let state = match e.outcome {
            Outcome::Unchanged => "fresh",
            Outcome::Moved => "moved",
            Outcome::Changed | Outcome::Rollback => "stale",
            Outcome::Removed => "removed",
            Outcome::Pending => "pending",
            Outcome::New => "untracked",
            Outcome::Diverged if e.note.as_deref().is_some_and(|n| n.contains("no anchor")) => {
                "unhashed"
            }
            other => other.as_str(),
        };
        failing |= matches!(state, "stale" | "removed" | "untracked");
        let mut recorded = Value::Null;
        if opts.record
            && let Some(c) = &e.check
        {
            recorded = match api.source_check(&c.artifact_id, &c.body, &c.idempotency_key) {
                Ok(v) => v,
                Err(CliError::Refused { code, .. }) => {
                    failing = true;
                    json!({ "refused": code })
                }
                Err(other) => return Err(other),
            };
        }
        p.line(&format!("{state:<9} {:<20} {}", e.claim_key, where_of(e)));
        rows.push(json!({ "state": state, "claim_key": e.claim_key, "anchor": e.anchor, "recorded": recorded }));
    }
    for e in edges {
        let state = match e.state.as_str() {
            "pending" | "ratified" => "stated",
            "proposed" => "waiting",
            other => other,
        };
        p.line(&format!("{state:<9} {} -[{}]-> {}", e.from, e.label, e.to));
    }
    Ok(Report {
        exit: u8::from(failing),
        json: json!({
            "source": source_json(loaded),
            "workarea": opts.target_workarea(),
            "claims": rows,
            "edges": edges,
        }),
    })
}

/// `--edges [--ratify]` (ADR-0057 d10): propose each stated edge between
/// two promoted tips that has none, or ratify each pending one the source
/// states that another key proposed.
fn run_edges(
    api: &dyn SyncApi,
    loaded: &Loaded,
    opts: &Options,
    ctx: &Context,
    me: &Me,
    served: &[Listed],
    p: &mut Printer<'_>,
) -> Result<Report, CliError> {
    let tips = tips_of(ctx, served);
    let known: HashSet<String> = loaded
        .items
        .iter()
        .map(|si| si.item.claim_key.clone())
        .chain(served.iter().map(|v| v.claim_key.clone()))
        .collect();
    let existing = api.claim_edges(&ctx.workarea_id)?;
    let mut lines = edge_lines(&loaded.edges, &known, &tips, &existing);
    let mut refused = false;
    for e in &mut lines {
        if opts.dry_run {
            continue;
        }
        let (Some(from), Some(to)) = (e.from_artifact.clone(), e.to_artifact.clone()) else {
            continue;
        };
        if !opts.ratify && e.state == "proposed" {
            let key = format!("sync:{}:edge:{from}:{}:{to}", ctx.workarea, e.label);
            match api.propose_edge(&from, &to, &e.label, &key) {
                Ok(v) => {
                    e.state = v["state"].as_str().unwrap_or("pending").to_owned();
                    e.edge_id = v["edge_id"].as_str().map(ToOwned::to_owned);
                }
                Err(CliError::Refused { code, .. }) => {
                    refused = true;
                    e.state = format!("refused {code}");
                }
                Err(other) => return Err(other),
            }
        } else if opts.ratify
            && e.state == "pending"
            && e.proposer_key_id.as_deref() != Some(me.key_id.as_str())
            && let Some(id) = e.edge_id.clone()
        {
            match api.ratify_edge(&id, &format!("sync:{}:ratify:{id}", ctx.workarea)) {
                Ok(_) => e.state = "ratified".to_owned(),
                Err(CliError::Refused { code, .. }) => {
                    refused = true;
                    e.state = format!("refused {code}");
                }
                Err(other) => return Err(other),
            }
        }
    }
    for e in &lines {
        p.line(&format!(
            "{:<10} {} -[{}]-> {}",
            e.state, e.from, e.label, e.to
        ));
    }
    p.line(&edge_summary(&lines));
    Ok(Report {
        exit: u8::from(refused),
        json: json!({
            "source": source_json(loaded),
            "workarea": opts.target_workarea(),
            "edges": lines,
        }),
    })
}

struct Printer<'a> {
    out: &'a mut dyn Write,
    quiet: bool,
}

impl Printer<'_> {
    fn line(&mut self, text: &str) {
        if !self.quiet {
            let _ = writeln!(self.out, "{text}");
        }
    }
}

fn where_of(e: &Entry) -> String {
    match &e.anchor {
        Some(a) => match &a.fragment {
            Some(f) => format!("{}#{f}", a.path),
            None => a.path.clone(),
        },
        None => e.path.clone(),
    }
}

fn entry_line(e: &Entry) -> String {
    let mut line = format!(
        "{:<9} {:<20} {}",
        e.outcome.as_str(),
        e.claim_key,
        where_of(e)
    );
    if let Some(note) = &e.note {
        line.push_str("  ");
        line.push_str(note);
    }
    if e.outcome.writes() && !e.writes_this_run {
        line.push_str("  (next run: --max-writes reached)");
    }
    line
}

fn summary_line(s: &Value, opts: &Options) -> String {
    format!(
        "plan      {} new, {} changed, {} removed, {} unchanged, {} pending, {} moved; \
         {} rollback, {} diverged, {} foreign, {} conflict, {} refused; {} {} \
         (--max-writes {})",
        s["new"],
        s["changed"],
        s["removed"],
        s["unchanged"],
        s["pending"],
        s["moved"],
        s["rollback"],
        s["diverged"],
        s["foreign"],
        s["conflict"],
        s["refused"],
        if opts.dry_run {
            "a run would write"
        } else {
            "this run wrote"
        },
        s["writes_this_run"],
        opts.max_writes
    )
}

fn edge_summary(edges: &[EdgeLine]) -> String {
    let count = |label: &str| {
        edges
            .iter()
            .filter(|e| e.label == label && e.state != "unstated")
            .count()
    };
    let unresolved = edges.iter().filter(|e| e.state == "unresolved").count();
    format!(
        "edges     {} amends, {} supersedes stated, {unresolved} unresolved; proposed by --edges once both ends are promoted",
        count("amends"),
        count("supersedes")
    )
}

fn source_json(loaded: &Loaded) -> Value {
    json!({
        "repo": loaded.repo,
        "rev": loaded.commit,
        "path": loaded.path,
        "adapter": loaded.adapter,
        "hasher": HASHER,
    })
}

fn plan_json(
    loaded: &Loaded,
    workarea: &str,
    plan: &Plan,
    edges: &[EdgeLine],
    summary: &Value,
) -> Value {
    let edges: Vec<Value> = edges
        .iter()
        .map(|e| {
            let state = if e.state == "proposed" {
                "waiting"
            } else {
                e.state.as_str()
            };
            json!({ "from": e.from, "label": e.label, "to": e.to, "state": state })
        })
        .collect();
    json!({
        "source": source_json(loaded),
        "workarea": workarea,
        "items": plan.entries,
        "unreadable": loaded.unreadable.iter().map(|(f, w)| json!({"path": f, "why": w})).collect::<Vec<_>>(),
        "edges": edges,
        "summary": summary,
    })
}
