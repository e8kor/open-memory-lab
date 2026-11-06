//! `oml sync`: read a source at one commit, diff it against a workarea and
//! write the plan, or check it, or propose and ratify its edges (ADR-0057
//! d6–d11, d14). Every call goes through [`SyncApi`], so a run is tested
//! against a fake.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::adapter::{EdgeStatement, Kind};
use super::api::{ListQuery, Listed, Me, SyncApi};
use super::hash::{HASHER, HASHER_CRATE, HASHER_RUSTDOC};
use super::plan::{Context, EdgeLine, Entry, Outcome, Plan, Scope, SourceItem, diff, edge_lines};
use super::source::{Git, check_repo, glob_match, is_secret_shaped, repo_of_remote};
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
    /// `--exclude` globs: matching paths are out of the source.
    pub exclude: Vec<String>,
    /// Allow an exclude set other than the one recorded for this source.
    pub change_exclude: bool,
    /// Allow `--adapter` to name another adapter than the one the
    /// workarea's claims of this source were written by.
    pub change_adapter: bool,
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

/// A source read at one commit: its files' bytes, not yet extracted, since
/// the adapter is the workarea's (ADR-0057 d7).
#[derive(Clone, Debug)]
pub struct Loaded {
    pub repo: String,
    pub commit: String,
    pub path: String,
    pub scope: Scope,
    /// Each file under the source an adapter may read, with its bytes.
    pub files: Vec<(String, Vec<u8>)>,
    /// Files never read, secret-shaped names, with why.
    pub skipped: Vec<(String, String)>,
    /// The `--exclude` globs the source was read under.
    pub exclude: Vec<String>,
    /// The adapter `--adapter` named.
    pub requested: Option<Kind>,
    /// Lines for standard error: a stripped credential, for one.
    pub warnings: Vec<String>,
    /// The clone, for the commit graph; `None` for a source built in memory.
    pub root: Option<PathBuf>,
}

impl Loaded {
    /// The paths of the files read.
    #[must_use]
    pub fn paths(&self) -> Vec<String> {
        self.files.iter().map(|(p, _)| p.clone()).collect()
    }
}

/// What one adapter yields over a source.
#[derive(Clone, Debug)]
pub struct Extracted {
    pub adapter: Kind,
    pub items: Vec<SourceItem>,
    pub edges: Vec<EdgeStatement>,
    /// Files skipped, with why: invalid UTF-8, a claim key another file
    /// also yields.
    pub unreadable: Vec<(String, String)>,
    /// Files left out on purpose, with why: undeclared or generated. Their
    /// claims are never retired, and they do not fail the run.
    pub skipped: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

/// The items and edges `kind` extracts from `loaded`.
#[must_use]
pub fn extract(loaded: &Loaded, kind: Kind) -> Extracted {
    let adapter = kind.adapter();
    let mut out = Extracted {
        adapter: kind,
        items: Vec::new(),
        edges: Vec::new(),
        unreadable: Vec::new(),
        skipped: Vec::new(),
        warnings: Vec::new(),
    };
    let mut first: HashMap<String, String> = HashMap::new();
    let excluded = |f: &str| loaded.exclude.iter().any(|g| glob_match(g, f));
    let accepted: Vec<(String, Vec<u8>)> = loaded
        .files
        .iter()
        .filter(|(f, _)| adapter.accepts(f) && !excluded(f))
        .cloned()
        .collect();
    let set = adapter.extract_set(&accepted);
    let per_file = set.files;
    out.unreadable.extend(set.unreadable);
    out.skipped.extend(set.skipped);
    // A claim key two items yield is refused with the file that yields it
    // again, so neither write lands on the other's claim.
    let mut bad: HashMap<String, String> = HashMap::new();
    for (file, x) in &per_file {
        let mut seen_here = HashSet::new();
        for item in &x.items {
            let key = &item.claim_key;
            if !seen_here.insert(key.clone()) {
                bad.entry(file.clone())
                    .or_insert(format!("it yields the claim key {key} twice"));
            } else if let Some(other) = first.get(key) {
                bad.entry(file.clone()).or_insert(format!(
                    "it yields the claim key {key}, which {other} yields too"
                ));
            } else {
                first.insert(key.clone(), file.clone());
            }
        }
    }
    for (file, x) in per_file {
        if let Some(why) = bad.get(&file) {
            out.unreadable.push((file, why.clone()));
            continue;
        }
        out.items.extend(x.items.into_iter().map(|item| SourceItem {
            path: file.clone(),
            item,
        }));
        out.edges.extend(x.edges);
        out.warnings.extend(x.warnings);
    }
    out
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
                    "warn: the origin remote's URL carries credentials (in .git/config, or a \
                     url.insteadOf rewrite in a git config); sync strips them from the anchor, \
                     and they belong in a credential helper (ADR-0057 d17)"
                        .to_owned(),
                );
            }
            remote.repo
        }
    };
    let (names, scope) = match opts.pr {
        Some(_) if !opts.retire_all => {
            let files = git.changed_files(&opts.base, &commit, &path)?;
            (files.clone(), Scope::Files(files))
        }
        _ => (git.files(&commit, &path)?, Scope::Prefix(path.clone())),
    };
    // The exclude set this clone last synced the source under: another is
    // refused unless named (ADR-0057 d7, as the adapter is).
    if let Some(recorded) = git.config(&exclude_config(opts, &path))
        && recorded != exclude_value(&opts.exclude)
        && !opts.change_exclude
    {
        return Err(format!(
            "this clone last synced {path} into {} excluding [{}]; this run excludes [{}]. Pass \
             --change-exclude to change the set",
            opts.target_workarea(),
            recorded.replace(',', ", "),
            opts.exclude.join(", ")
        ));
    }
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let excluded = |f: &str| opts.exclude.iter().any(|g| glob_match(g, f));
    let rust = opts.adapter == Some(Kind::Rust);
    if rust && opts.pr.is_some() {
        return Err(
            "--pr is refused with --adapter rust: a crate's claim names its dependents, which \
             live in manifests a pull request did not change, so the rust adapter reads the \
             whole source (ADR-0061 d4)"
                .into(),
        );
    }
    if !opts.retire_all {
        for file in names
            .iter()
            .filter(|f| if rust { rust_readable(f) } else { readable_kind(f) } && !excluded(f))
        {
            if is_secret_shaped(file) {
                skipped.push((
                    file.clone(),
                    "its path matches a secret pattern, so it is never read (ADR-0057 d6)".into(),
                ));
                continue;
            }
            files.push((file.clone(), git.read(&commit, file)?));
        }
    }
    Ok(Loaded {
        repo,
        commit,
        path,
        scope,
        files,
        skipped,
        exclude: opts.exclude.clone(),
        requested: opts.adapter,
        warnings,
        root: Some(git.root().to_path_buf()),
    })
}

/// The git config entry recording a source's exclude set on this clone.
fn exclude_config(opts: &Options, path: &str) -> String {
    let id = super::hash::sha256(format!("{}\0{path}", opts.target_workarea()).as_bytes());
    format!(
        "oml.sync-{}.exclude",
        &id["sha256:".len().."sha256:".len() + 16]
    )
}

fn exclude_value(globs: &[String]) -> String {
    let mut sorted = globs.to_vec();
    sorted.sort();
    sorted.join(",")
}

/// Records the run's exclude set for its source on this clone, so a later
/// run with another set is refused unless `--change-exclude`.
///
/// # Errors
/// git fails.
pub fn record_exclude(loaded: &Loaded, opts: &Options) -> Result<(), String> {
    let Some(root) = &loaded.root else {
        return Ok(());
    };
    Git::at(root)?.set_config(
        &exclude_config(opts, &loaded.path),
        &exclude_value(&opts.exclude),
    )
}

/// Whether the `rust` adapter reads a file of this name: a manifest, or a
/// `.rs` file under a `src/` directory (ADR-0061 d1).
fn rust_readable(path: &str) -> bool {
    path.rsplit('/').next() == Some("Cargo.toml")
        || (path.ends_with(".rs") && (path.starts_with("src/") || path.contains("/src/")))
}

/// Whether any adapter reads a file of this name: markdown or plain text.
fn readable_kind(path: &str) -> bool {
    path.ends_with(".md") || path.ends_with(".txt")
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
    if opts.retire_all && opts.pr.is_none() {
        return Err(CliError::local(
            "--retire-all retires a draft's claims and requires --pr <n>: it never touches the \
             main workarea (ADR-0057 d14)",
        ));
    }
    let workarea = opts.target_workarea();
    let workarea_id = api.workarea_id(&workarea)?;
    let me = api.me()?;
    let mut ctx = Context {
        workarea: workarea.clone(),
        workarea_id: workarea_id.clone(),
        repo: loaded.repo.clone(),
        rev: loaded.commit.clone(),
        scope: loaded.scope.clone(),
        max_writes: opts.max_writes,
        adopt: opts.adopt,
        allow_rollback: opts.allow_rollback,
        check: opts.check,
        namespaces: Vec::new(),
        skip_paths: HashSet::new(),
        exclude: loaded.exclude.clone(),
    };
    let served = list_served(api, &ctx)?;
    let own = api.list(&ListQuery {
        workarea: workarea.clone(),
        own: true,
        ..ListQuery::default()
    })?;
    let (kind, namespaces) = resolve_adapter(loaded, opts, &ctx, &own, &served)?;
    ctx.namespaces = namespaces;
    guard_rev(loaded, opts, &ctx, &own, &served)?;
    guard_exclude(opts, &ctx, &own, &served)?;
    let x = if opts.retire_all {
        Extracted {
            adapter: kind,
            items: Vec::new(),
            edges: Vec::new(),
            unreadable: Vec::new(),
            skipped: Vec::new(),
            warnings: Vec::new(),
        }
    } else {
        extract(loaded, kind)
    };
    ctx.skip_paths = x
        .unreadable
        .iter()
        .chain(&x.skipped)
        .chain(&loaded.skipped)
        .map(|(p, _)| p.clone())
        .collect();
    let mut p = Printer {
        out,
        quiet: opts.json,
    };
    p.line(&format!(
        "source    {} @ {}  {}  adapter {}  {}",
        loaded.repo,
        &loaded.commit[..12.min(loaded.commit.len())],
        if loaded.path.is_empty() {
            "."
        } else {
            &loaded.path
        },
        kind.name(),
        hashers_line(kind)
    ));
    p.line(&format!("workarea  {workarea}  key {}", me.name));
    for (file, why) in &x.unreadable {
        p.line(&format!("unreadable {file}: {why}"));
    }
    for (file, why) in x.skipped.iter().chain(&loaded.skipped) {
        p.line(&format!("skipped   {file}: {why}"));
    }
    for warning in &x.warnings {
        p.line(&format!("warn      {warning}"));
    }
    if opts.edges {
        return run_edges(api, loaded, &x, opts, &ctx, &me, &served, &mut p);
    }
    let mut plan = if opts.retire_all {
        retire_all(&ctx, &own)
    } else {
        diff(&ctx, &x.items, &own, &served)
    };
    let tips = tips_of(&ctx, &served);
    let known: HashSet<String> = x
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
    let edges = edge_lines(&x.edges, &known, &tips, &existing);
    if opts.check {
        return run_check(api, loaded, &x, opts, &plan, &edges, &mut p);
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
    let failed = !x.unreadable.is_empty() || plan.entries.iter().any(|e| e.outcome.fails());
    Ok(Report {
        exit: u8::from(failed),
        json: plan_json(loaded, &x, &workarea, &plan, &edges, &summary),
    })
}

/// The adapters whose claims this workarea holds under the source, by
/// their keys' namespaces (ADR-0057 d7).
fn recorded_adapters(ctx: &Context, own: &[Listed], served: &[Listed]) -> BTreeSet<Kind> {
    own.iter()
        .chain(served)
        .filter(|v| v.workarea_id.as_deref() == Some(ctx.workarea_id.as_str()))
        .filter(|v| {
            v.anchor.as_ref().is_some_and(|a| {
                a.repo.as_deref() == Some(ctx.repo.as_str())
                    && a.path.as_deref().is_some_and(|p| ctx.scope.contains(p))
            })
        })
        .filter_map(|v| Kind::of_key(&v.claim_key))
        // The rust adapter's namespace is its own: another adapter's run
        // never answers for it, and it never answers for another's
        // (ADR-0061 d10).
        .filter(|k| *k != Kind::Rust)
        .collect()
}

/// The adapter of this run and the namespaces it answers for. The adapter
/// is the one the workarea's claims of this source were written by; a
/// first run takes `--adapter` or infers it from the file names; another
/// adapter is refused unless `--change-adapter`, which then retires the
/// old adapter's claims.
fn resolve_adapter(
    loaded: &Loaded,
    opts: &Options,
    ctx: &Context,
    own: &[Listed],
    served: &[Listed],
) -> Result<(Kind, Vec<Kind>), CliError> {
    let recorded = recorded_adapters(ctx, own, served);
    let names = || {
        recorded
            .iter()
            .map(|k| k.name())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match loaded.requested {
        Some(Kind::Rust) => Ok((Kind::Rust, vec![Kind::Rust])),
        Some(k) if recorded.is_empty() || recorded.contains(&k) => Ok((k, vec![k])),
        Some(k) if opts.change_adapter && !opts.check => {
            let mut all: Vec<Kind> = recorded.iter().copied().collect();
            all.push(k);
            Ok((k, all))
        }
        Some(k) => Err(CliError::local(format!(
            "the workarea's claims of this source were written by the {} adapter; --adapter {} \
             is refused unless --change-adapter, which retires them (ADR-0057 d7)",
            names(),
            k.name()
        ))),
        None => match recorded.len() {
            0 => {
                let k = Kind::default_for(&loaded.paths());
                Ok((k, vec![k]))
            }
            1 => {
                let k = *recorded.iter().next().unwrap_or(&Kind::Headings);
                Ok((k, vec![k]))
            }
            _ => Err(CliError::local(format!(
                "the workarea's claims of this source were written by several adapters ({}); \
                 name one with --adapter",
                names()
            ))),
        },
    }
}

/// Refuses an exclude set that takes in paths this sync's claims live at,
/// unless `--change-exclude`: excluded, they are out of the source, left as
/// they are and never retired.
fn guard_exclude(
    opts: &Options,
    ctx: &Context,
    own: &[Listed],
    served: &[Listed],
) -> Result<(), CliError> {
    if opts.change_exclude || ctx.exclude.is_empty() {
        return Ok(());
    }
    let ours = |k: &str| Kind::of_key(k).is_some_and(|k| ctx.namespaces.contains(&k));
    let hidden: BTreeSet<String> = own
        .iter()
        .chain(served)
        .filter(|v| {
            v.workarea_id.as_deref() == Some(ctx.workarea_id.as_str()) && ours(&v.claim_key)
        })
        .filter_map(|v| v.anchor.as_ref())
        .filter(|a| a.repo.as_deref() == Some(ctx.repo.as_str()))
        .filter_map(|a| a.path.clone())
        .filter(|p| ctx.scope.contains(p) && !ctx.in_source(p))
        .collect();
    if hidden.is_empty() {
        return Ok(());
    }
    Err(CliError::local(format!(
        "--exclude takes in paths this sync's claims live at ({}); pass --change-exclude to leave \
         them out of the source, where they are never retired; nothing was written",
        hidden.into_iter().collect::<Vec<_>>().join(", ")
    )))
}

/// Refuses a run from a checkout older than the one the workarea's claims
/// of this source were read at, or from a clone that does not hold that
/// commit: such a run would retire what was added since and revert what
/// changed (ADR-0057 d20). `--allow-rollback` is the deliberate revert.
fn guard_rev(
    loaded: &Loaded,
    opts: &Options,
    ctx: &Context,
    own: &[Listed],
    served: &[Listed],
) -> Result<(), CliError> {
    let Some(root) = &loaded.root else {
        return Ok(());
    };
    if opts.allow_rollback {
        return Ok(());
    }
    let git = Git::at(root).map_err(CliError::Local)?;
    let ours = |k: &str| Kind::of_key(k).is_some_and(|k| ctx.namespaces.contains(&k));
    let revs: BTreeSet<String> = own
        .iter()
        .chain(served)
        .filter(|v| v.workarea_id.as_deref() == Some(ctx.workarea_id.as_str()))
        .filter(|v| ours(&v.claim_key))
        .filter(|v| {
            v.anchor
                .as_ref()
                .is_some_and(|a| a.repo.as_deref() == Some(ctx.repo.as_str()))
        })
        .flat_map(|v| {
            let anchored = v.anchor.as_ref().and_then(|a| a.rev.clone());
            let checked = v.source_checked.as_ref().map(|c| c.rev.clone());
            anchored.into_iter().chain(checked)
        })
        .filter(|r| !r.is_empty() && *r != loaded.commit)
        .collect();
    let short = |r: &str| r[..r.len().min(12)].to_owned();
    for rev in &revs {
        if !git.knows(rev) {
            return Err(CliError::local(format!(
                "the workarea's claims of this source were read at {}, a commit unknown to this \
                 clone: fetch it first, or pass --allow-rollback for a deliberate revert \
                 (ADR-0057 d20); nothing was written",
                short(rev)
            )));
        }
        if git.is_strict_ancestor(&loaded.commit, rev) {
            return Err(CliError::local(format!(
                "--rev {} is older than {}, which the workarea's claims of this source were read \
                 at: a sync from an older checkout would retire what was added since and revert \
                 what changed. Pull, or pass --allow-rollback for a deliberate revert (ADR-0057 \
                 d20); nothing was written",
                short(&loaded.commit),
                short(rev)
            )));
        }
    }
    Ok(())
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
pub(crate) fn write_plan(
    api: &dyn SyncApi,
    plan: &mut Plan,
    workarea: &str,
) -> Result<(), CliError> {
    // Every source check first, in one batch the API may send concurrently;
    // an entry whose check was refused writes nothing.
    let checked: Vec<usize> = (0..plan.entries.len())
        .filter(|&i| plan.entries[i].check.is_some())
        .collect();
    let results = send_checks(api, &plan.entries, &checked);
    for (i, result) in checked.into_iter().zip(results) {
        match result {
            Ok(_) => {}
            Err(CliError::Refused { code, message, .. }) => {
                let e = &mut plan.entries[i];
                e.note = Some(format!("source check refused: {code} — {message}"));
                e.outcome = Outcome::Refused;
                e.writes_this_run = false;
            }
            Err(other) => return Err(other),
        }
    }
    for e in &mut plan.entries {
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

/// The source checks of `entries[i]` for each `i`, in one batch.
pub(crate) fn send_checks(
    api: &dyn SyncApi,
    entries: &[Entry],
    which: &[usize],
) -> Vec<Result<Value, CliError>> {
    if which.is_empty() {
        return Vec::new();
    }
    let batch: Vec<(String, Value, String)> = which
        .iter()
        .filter_map(|&i| entries[i].check.as_ref())
        .map(|c| {
            (
                c.artifact_id.clone(),
                c.body.clone(),
                c.idempotency_key.clone(),
            )
        })
        .collect();
    api.source_checks(&batch)
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
    x: &Extracted,
    opts: &Options,
    plan: &Plan,
    edges: &[EdgeLine],
    p: &mut Printer<'_>,
) -> Result<Report, CliError> {
    let mut rows = Vec::new();
    let mut failing = !x.unreadable.is_empty();
    let recorded_checks: Vec<usize> = if opts.record {
        (0..plan.entries.len())
            .filter(|&i| plan.entries[i].check.is_some())
            .collect()
    } else {
        Vec::new()
    };
    let mut answers: HashMap<usize, Result<Value, CliError>> = recorded_checks
        .iter()
        .copied()
        .zip(send_checks(api, &plan.entries, &recorded_checks))
        .collect();
    for (i, e) in plan.entries.iter().enumerate() {
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
        let recorded = match answers.remove(&i) {
            None => Value::Null,
            Some(Ok(v)) => v,
            Some(Err(CliError::Refused { code, .. })) => {
                failing = true;
                json!({ "refused": code })
            }
            Some(Err(other)) => return Err(other),
        };
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
            "source": source_json(loaded, x),
            "workarea": opts.target_workarea(),
            "claims": rows,
            "edges": edges,
        }),
    })
}

/// `--edges [--ratify]` (ADR-0057 d10): propose each stated edge between
/// two promoted tips that has none, or ratify each pending one the source
/// states that another key proposed.
#[allow(
    clippy::too_many_arguments,
    reason = "the run's inputs, each read once"
)]
fn run_edges(
    api: &dyn SyncApi,
    loaded: &Loaded,
    x: &Extracted,
    opts: &Options,
    ctx: &Context,
    me: &Me,
    served: &[Listed],
    p: &mut Printer<'_>,
) -> Result<Report, CliError> {
    let tips = tips_of(ctx, served);
    let known: HashSet<String> = x
        .items
        .iter()
        .map(|si| si.item.claim_key.clone())
        .chain(served.iter().map(|v| v.claim_key.clone()))
        .collect();
    let existing = api.claim_edges(&ctx.workarea_id)?;
    let mut lines = edge_lines(&x.edges, &known, &tips, &existing);
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
            "source": source_json(loaded, x),
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

pub(crate) fn where_of(e: &Entry) -> String {
    match &e.anchor {
        Some(a) => {
            let place = a.path.as_deref().or(a.uri.as_deref()).unwrap_or(&e.path);
            match &a.fragment {
                Some(f) => format!("{place}#{f}"),
                None => place.to_owned(),
            }
        }
        None => e.path.clone(),
    }
}

pub(crate) fn entry_line(e: &Entry) -> String {
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

/// The hashers an adapter's anchors name, as the run's first line says them.
fn hashers_line(kind: Kind) -> String {
    match kind {
        Kind::Rust => format!("hashers {HASHER_CRATE}, {HASHER_RUSTDOC}"),
        _ => format!("hasher {HASHER}"),
    }
}

fn source_json(loaded: &Loaded, x: &Extracted) -> Value {
    json!({
        "repo": loaded.repo,
        "rev": loaded.commit,
        "path": loaded.path,
        "adapter": x.adapter.name(),
        "hasher": match x.adapter {
            Kind::Rust => format!("{HASHER_CRATE}, {HASHER_RUSTDOC}"),
            _ => HASHER.to_owned(),
        },
    })
}

fn plan_json(
    loaded: &Loaded,
    x: &Extracted,
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
        "source": source_json(loaded, x),
        "workarea": workarea,
        "items": plan.entries,
        "unreadable": x.unreadable.iter().map(|(f, w)| json!({"path": f, "why": w})).collect::<Vec<_>>(),
        "skipped": x.skipped.iter().chain(&loaded.skipped).map(|(f, w)| json!({"path": f, "why": w})).collect::<Vec<_>>(),
        "warnings": x.warnings,
        "edges": edges,
        "summary": summary,
    })
}
