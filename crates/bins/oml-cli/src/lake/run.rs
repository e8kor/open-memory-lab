//! The run of `oml sync --adapter delta` (ADR-0062 d2, d8, d11): it reads each
//! table's log, maps it to claims, diffs them against the key's own claims in
//! the workarea, and writes, checks or prints the plan through the same calls
//! `oml sync` makes for documents.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use oml_delta_log::LogError;
use serde_json::{Value, json};

use super::extract::{LakeItem, items};
use super::plan::{Context, TableState, diff};
use super::read::{TableRead, read_lake};
use super::uri::{canonical, join};
use crate::client::CliError;
use crate::sync::api::{ListQuery, SyncApi};
use crate::sync::plan::Outcome;
use crate::sync::run::{Report, entry_line, send_checks, write_plan};

/// The discovery depth when none is given: prefixes below the lake root
/// (ADR-0062 d10).
pub const DEPTH: usize = 3;

/// What a lake run was asked to do.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The lake root on a disk.
    pub root: PathBuf,
    /// The canonical location the root stands for, when it is a mirror or a
    /// copy; omitted, the root's own `file://` location.
    pub uri_base: Option<String>,
    pub workarea: String,
    pub dry_run: bool,
    /// Report each claim's state against the log; write no memory.
    pub check: bool,
    /// With `check`, record the source checks.
    pub record: bool,
    pub json: bool,
    /// The most memories one run writes.
    pub max_writes: usize,
    pub depth: usize,
    /// Read every table at this version (a test's lake at an earlier moment).
    pub at_version: Option<u64>,
}

fn base_uri(opts: &Options) -> Result<String, CliError> {
    match &opts.uri_base {
        Some(b) => canonical(b).map_err(|e| CliError::local(format!("--uri-base: {e}"))),
        None => {
            let root = opts
                .root
                .canonicalize()
                .map_err(|e| CliError::local(format!("{}: {e}", opts.root.display())))?;
            let path = root.to_str().ok_or_else(|| {
                CliError::local("the lake root's path is not UTF-8: pass --uri-base")
            })?;
            canonical(&format!("file://{path}")).map_err(|e| {
                CliError::local(format!(
                    "the lake root {path} has no canonical file:// location ({e}); \
                     pass --uri-base <its location>"
                ))
            })
        }
    }
}

fn table_json(t: &TableRead) -> Value {
    match &t.outcome {
        Ok(s) => json!({ "table": t.rel, "state": "read", "version": s.version }),
        Err(LogError::Unsupported(why)) => {
            json!({ "table": t.rel, "state": "unsupported", "reason": why })
        }
        Err(LogError::Unreadable(why)) => {
            json!({ "table": t.rel, "state": "unreadable", "reason": why })
        }
    }
}

/// # Errors
/// A call fails without a typed refusal, or the location is not canonical.
pub fn execute(api: &dyn SyncApi, opts: &Options, out: &mut dyn Write) -> Result<Report, CliError> {
    let base = base_uri(opts)?;
    let depth = if opts.depth == 0 { DEPTH } else { opts.depth };
    let workarea_id = api.workarea_id(&opts.workarea)?;
    let me = api.me()?;
    let tables = read_lake(&opts.root, depth, opts.at_version);
    let mut say = |text: &str| {
        if !opts.json {
            let _ = writeln!(out, "{text}");
        }
    };
    say(&format!(
        "source    {base}  adapter delta  hashers oml-delta-table/1, oml-delta-schema/1, oml-delta-col/1"
    ));
    say(&format!("workarea  {}  key {}", opts.workarea, me.name));
    let mut states: BTreeMap<String, TableState> = BTreeMap::new();
    let mut all: Vec<LakeItem> = Vec::new();
    let mut unread = false;
    for t in &tables {
        match &t.outcome {
            Ok(snapshot) => {
                let uri = join(&base, &t.rel)
                    .map_err(|e| CliError::local(format!("the location of {}: {e}", t.rel)))?;
                states.insert(
                    t.rel.clone(),
                    TableState::Read {
                        version: snapshot.version,
                    },
                );
                all.extend(items(&t.rel, &uri, snapshot));
            }
            Err(e) => {
                unread = true;
                states.insert(t.rel.clone(), TableState::NotRead);
                say(&format!("{:<11} {}: {e}", state_word(e), t.rel));
            }
        }
    }
    if tables.is_empty() {
        say(&format!(
            "warn      no Delta table (a directory with a _delta_log) under {}",
            opts.root.display()
        ));
    }
    let own = api.list(&ListQuery {
        workarea: opts.workarea.clone(),
        own: true,
        ..ListQuery::default()
    })?;
    let ctx = Context {
        workarea: opts.workarea.clone(),
        workarea_id,
        max_writes: opts.max_writes,
        check: opts.check,
    };
    let mut plan = diff(&ctx, &all, &own, &states);
    if opts.check {
        return run_check(api, opts, &plan, &tables, &base, &mut say);
    }
    if !opts.dry_run {
        write_plan(api, &mut plan, &opts.workarea)?;
    }
    for e in &plan.entries {
        say(&entry_line(e));
    }
    let summary = plan.summary();
    say(&format!(
        "plan      {} new, {} changed, {} removed, {} unchanged, {} pending, {} rollback, \
         {} diverged, {} conflict, {} refused, {} skipped; {} {} (--max-writes {})",
        summary["new"],
        summary["changed"],
        summary["removed"],
        summary["unchanged"],
        summary["pending"],
        summary["rollback"],
        summary["diverged"],
        summary["conflict"],
        summary["refused"],
        summary["skipped"],
        if opts.dry_run {
            "a run would write"
        } else {
            "this run wrote"
        },
        summary["writes_this_run"],
        opts.max_writes,
    ));
    if !opts.dry_run && plan.entries.iter().any(|e| e.writes_this_run) {
        say(&format!(
            "review    oml review list --workarea {} --author-key {}",
            opts.workarea, me.name
        ));
    }
    let failed = unread || plan.entries.iter().any(|e| e.outcome.fails());
    Ok(Report {
        exit: u8::from(failed),
        json: json!({
            "source": { "uri": base, "adapter": "delta" },
            "workarea": opts.workarea,
            "tables": tables.iter().map(table_json).collect::<Vec<_>>(),
            "items": plan.entries,
            "summary": summary,
        }),
    })
}

fn state_word(e: &LogError) -> &'static str {
    match e {
        LogError::Unsupported(_) => "unsupported",
        LogError::Unreadable(_) => "unreadable",
    }
}

/// `--check`: each claim's state against the log, exit 1 on `stale`, `removed`
/// or `untracked`; `--record` records the source checks.
fn run_check(
    api: &dyn SyncApi,
    opts: &Options,
    plan: &crate::sync::plan::Plan,
    tables: &[TableRead],
    base: &str,
    say: &mut dyn FnMut(&str),
) -> Result<Report, CliError> {
    let recorded: Vec<usize> = if opts.record {
        (0..plan.entries.len())
            .filter(|&i| plan.entries[i].check.is_some())
            .collect()
    } else {
        Vec::new()
    };
    let mut answers: BTreeMap<usize, Result<Value, CliError>> = recorded
        .iter()
        .copied()
        .zip(send_checks(api, &plan.entries, &recorded))
        .collect();
    let mut failing = tables.iter().any(|t| t.outcome.is_err());
    let mut rows = Vec::new();
    for (i, e) in plan.entries.iter().enumerate() {
        let state = match e.outcome {
            Outcome::Unchanged => "fresh",
            Outcome::Changed | Outcome::Rollback => "stale",
            Outcome::Removed => "removed",
            Outcome::Pending => "pending",
            Outcome::New => "untracked",
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
        say(&format!(
            "{state:<9} {:<20} {}",
            e.claim_key,
            crate::sync::run::where_of(e)
        ));
        rows.push(json!({
            "state": state, "claim_key": e.claim_key, "anchor": e.anchor, "recorded": recorded
        }));
    }
    Ok(Report {
        exit: u8::from(failing),
        json: json!({
            "source": { "uri": base, "adapter": "delta" },
            "workarea": opts.workarea,
            "tables": tables.iter().map(table_json).collect::<Vec<_>>(),
            "claims": rows,
        }),
    })
}
