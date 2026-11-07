//! The git hooks (ADR-0064 d6, d7): `oml hook git <event>` and the
//! installer. Every hook is one line that returns 0 to git whatever `oml`
//! does, and its foreground reads the clone's local state, appends to the
//! queue and starts a detached job. It opens no network connection.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use super::job::{Queue, QueueEntry};
use super::{Clone_, Source, excludes_for, git_in, now_secs, read_json, write_json};
use crate::sync::hash::sha256;
use crate::sync::source::{glob_match, is_secret_shaped};

/// The marker every installed file or line carries (d7).
pub const MARKER: &str = "# oml-managed";
/// The five hooks the git layer installs.
pub const GIT_HOOKS: [&str; 5] = [
    "post-commit",
    "post-checkout",
    "post-merge",
    "post-rewrite",
    "pre-push",
];
/// The foreground budget of d22, logged when a hook goes over it.
pub const BUDGET_MS: u128 = 100;

/// The line a hook file holds for `event`.
#[must_use]
pub fn hook_line(event: &str) -> String {
    // pre-push prints to the person and reads the refs git sends; every
    // other hook is silent.
    if event == "pre-push" {
        format!("oml hook git {event} \"$@\" || true {MARKER}")
    } else {
        format!("oml hook git {event} \"$@\" </dev/null >/dev/null 2>&1 || true {MARKER}")
    }
}

/// The same line in a file that already holds other hooks, whose stdin it
/// must leave for them.
fn chained_line(event: &str) -> String {
    format!("oml hook git {event} \"$@\" </dev/null || true {MARKER}")
}

// ---------------------------------------------------------------------------
// The foreground
// ---------------------------------------------------------------------------

/// `oml hook git <event> [args]`: never fails, never waits for the network.
pub fn run(event: &str, args: &[String], stdin: &mut dyn Read, cwd: &Path) {
    let started = std::time::Instant::now();
    let Some(clone) = Clone_::open(cwd) else {
        return;
    };
    if !clone.mode().includes_git() {
        return;
    }
    let _ = std::fs::create_dir_all(clone.oml_dir());
    if clone.get("hookspending").is_some() {
        // The first hook that fires clears the mark (d7).
        let _ = git_in(
            &clone.top,
            &["config", "--local", "--unset", "oml.hooksPending"],
        );
    }
    let head = clone.head();
    let state_path = clone.oml_dir().join("state.json");
    let mut state = read_json(&state_path);
    state["hook_at"] = json!(now_secs());
    state["branch"] = json!(clone.branch());
    state["sync_branch"] = json!(clone.on_sync_branch());
    if let Some(head) = &head {
        state["hook_head"] = json!(head);
    }
    let sources = clone.sources();
    match event {
        "post-checkout" => {
            // A branch switch only records the branch (d5).
            if args.get(2).map(String::as_str) != Some("1") {
                return;
            }
        }
        "post-commit" => {
            if let Some(head) = &head {
                let changed = changed_in_commit(&clone);
                if clone.on_sync_branch() {
                    queue_sources(&clone, &sources, head, &changed, "post-commit", false);
                } else {
                    queue_checks(&clone, &sources, head, &changed);
                }
            }
        }
        "post-merge" => {
            if let (Some(head), true) = (&head, clone.on_sync_branch()) {
                let changed = git_lines(
                    &clone.top,
                    &["diff", "--name-only", "-z", "ORIG_HEAD", "HEAD"],
                );
                queue_sources(&clone, &sources, head, &changed, "post-merge", false);
            }
        }
        "post-rewrite" => {
            if let (Some(head), true) = (&head, clone.on_sync_branch()) {
                let amend = args.first().map(String::as_str) == Some("amend");
                let changed = if amend {
                    changed_in_commit(&clone)
                } else {
                    Vec::new()
                };
                queue_sources(&clone, &sources, head, &changed, "post-rewrite", true);
            }
        }
        "pre-push" => {
            let mut text = String::new();
            let _ = stdin.read_to_string(&mut text);
            pre_push(&clone, &sources, &text);
        }
        _ => return,
    }
    let _ = write_json(&state_path, &state);
    if !Queue::load(&clone).is_empty() {
        spawn_job(&clone);
    }
    let elapsed = started.elapsed().as_millis();
    if elapsed > BUDGET_MS {
        log_overrun(&clone, event, elapsed);
    }
}

fn git_lines(top: &Path, args: &[&str]) -> Vec<String> {
    git_in(top, args)
        .map(|s| {
            s.split(['\0', '\n'])
                .filter(|l| !l.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn changed_in_commit(clone: &Clone_) -> Vec<String> {
    git_lines(
        &clone.top,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-z",
            "--root",
            "HEAD",
        ],
    )
}

/// The changed paths a source reads: under it, not excluded, not secret-shaped.
fn relevant(source: &Source, all: &[Source], changed: &[String]) -> Vec<String> {
    let excludes = excludes_for(source, all);
    changed
        .iter()
        .filter(|p| {
            source.contains(p) && !is_secret_shaped(p) && !excludes.iter().any(|g| glob_match(g, p))
        })
        .cloned()
        .collect()
}

fn entry_of(
    source: &Source,
    all: &[Source],
    commit: &str,
    paths: Vec<String>,
    reason: &str,
    rewritten: bool,
) -> QueueEntry {
    QueueEntry {
        source: source.path.clone(),
        adapter: source.adapter.clone(),
        commit: commit.to_owned(),
        paths,
        reason: reason.to_owned(),
        rewritten,
        exclude: excludes_for(source, all),
    }
}

/// Queues `commit` for each source the change reaches; an empty `changed`
/// queues every source whole (a rebase, or a merge with no ORIG_HEAD).
fn queue_sources(
    clone: &Clone_,
    sources: &[Source],
    commit: &str,
    changed: &[String],
    reason: &str,
    rewritten: bool,
) {
    let _ = Queue::update(clone, |q| {
        for s in sources {
            let paths = relevant(s, sources, changed);
            if !changed.is_empty() && paths.is_empty() {
                continue;
            }
            q.enqueue(entry_of(s, sources, commit, paths, reason, rewritten));
        }
    });
}

/// A commit off the sync branch is checked, never synced (d5, d9).
fn queue_checks(clone: &Clone_, sources: &[Source], commit: &str, changed: &[String]) {
    let _ = Queue::update(clone, |q| {
        for s in sources {
            let paths = relevant(s, sources, changed);
            if !paths.is_empty() {
                q.enqueue_check(entry_of(s, sources, commit, paths, "check", false));
            }
        }
    });
}

/// `pre-push` (d9): the cached check of each pushed commit, warn-only.
fn pre_push(clone: &Clone_, sources: &[Source], refs: &str) {
    let zero = "0".repeat(40);
    let mut commits: Vec<String> = Vec::new();
    for line in refs.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let (Some(local), Some(remote)) = (parts.get(1), parts.get(3)) else {
            continue;
        };
        if *local == zero {
            continue;
        }
        let range = if *remote == zero {
            (*local).to_owned()
        } else {
            format!("{remote}..{local}")
        };
        let mut found = git_lines(&clone.top, &["rev-list", "-n", "20", &range]);
        if found.is_empty() {
            found.push((*local).to_owned());
        }
        commits.extend(found);
    }
    if commits.is_empty() {
        commits.extend(clone.head());
    }
    let (mut stale, mut removed, mut untracked) = (0, 0, 0);
    let mut first: Vec<String> = Vec::new();
    let mut unchecked: Vec<String> = Vec::new();
    for commit in &commits {
        let path = clone.oml_dir().join("check").join(format!("{commit}.json"));
        let Ok(bytes) = std::fs::read(&path) else {
            unchecked.push(commit.chars().take(12).collect());
            continue;
        };
        let doc: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        stale += doc["stale"].as_u64().unwrap_or(0);
        removed += doc["removed"].as_u64().unwrap_or(0);
        untracked += doc["untracked"].as_u64().unwrap_or(0);
        for claim in doc["claims"].as_array().into_iter().flatten() {
            if first.len() < 5 {
                let fragment = claim["fragment"]
                    .as_str()
                    .map(|f| format!("#{f}"))
                    .unwrap_or_default();
                first.push(format!(
                    "{} {}{fragment}",
                    claim["state"].as_str().unwrap_or(""),
                    claim["path"].as_str().unwrap_or("")
                ));
            }
        }
    }
    let mut lines = Vec::new();
    if stale + removed + untracked > 0 {
        lines.push(format!("oml: {stale} stale, {removed} removed, {untracked} untracked anchored claim(s) under the pushed paths (not blocking)"));
        lines.extend(first.iter().map(|l| format!("oml:   {l}")));
    }
    if !unchecked.is_empty() {
        lines.push(format!("oml: not checked yet: {}", unchecked.join(", ")));
    }
    for line in lines {
        eprintln!("{line}");
    }
    // A fresh check of the range for the next push (d9).
    let _ = Queue::update(clone, |q| {
        for commit in &commits {
            if clone
                .oml_dir()
                .join("check")
                .join(format!("{commit}.json"))
                .exists()
            {
                continue;
            }
            let changed = git_lines(
                &clone.top,
                &[
                    "diff-tree",
                    "--no-commit-id",
                    "--name-only",
                    "-r",
                    "-z",
                    "--root",
                    commit,
                ],
            );
            for s in sources {
                let paths = relevant(s, sources, &changed);
                if !paths.is_empty() {
                    q.enqueue_check(entry_of(s, sources, commit, paths, "check", false));
                }
            }
        }
    });
}

/// Starts `oml sync --flush --background` detached: its own process group,
/// its output in `.git/oml/log`, rotated at 1 MiB. Not waited for.
pub fn spawn_job(clone: &Clone_) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let log = clone.oml_dir().join("log");
    if std::fs::metadata(&log).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = std::fs::rename(&log, clone.oml_dir().join("log.1"));
    }
    let Ok(out) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    else {
        return;
    };
    let Ok(err) = out.try_clone() else { return };
    let mut cmd = Command::new(exe);
    cmd.args(["sync", "--flush", "--background"])
        .current_dir(&clone.top)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }
    let _ = cmd.spawn();
}

/// The overrun goes into a local log for `oml doctor`; it never fails the
/// operation (d22).
fn log_overrun(clone: &Clone_, event: &str, ms: u128) {
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(clone.oml_dir().join("budget.log"))
    {
        let _ = writeln!(f, "{}", json!({"event": event, "ms": ms, "at": now_secs()}));
    }
}

// ---------------------------------------------------------------------------
// The installer
// ---------------------------------------------------------------------------

/// What an install or uninstall did, line by line, for the person.
#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    /// The hooks were left for the person to add to a committed config.
    pub pending: bool,
}

fn hooks_json(clone: &Clone_) -> PathBuf {
    clone.oml_dir().join("hooks.json")
}

fn shell_shebang(first: &str) -> bool {
    let Some(rest) = first.strip_prefix("#!") else {
        return false;
    };
    let last = rest.split_whitespace().last().unwrap_or("");
    matches!(
        last.rsplit('/').next(),
        Some("sh" | "bash" | "dash" | "zsh" | "ksh")
    )
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}
#[cfg(not(unix))]
fn make_executable(_: &Path) {}

fn digest(text: &str) -> String {
    sha256(text.as_bytes())
}

fn lefthook_config(top: &Path) -> bool {
    [
        "lefthook.yml",
        "lefthook.yaml",
        ".lefthook.yml",
        ".lefthook.yaml",
    ]
    .iter()
    .any(|f| top.join(f).exists())
}

fn lefthook_local(events: &[&str]) -> String {
    let mut out = format!("{MARKER}\n");
    for e in events {
        let run = if *e == "pre-push" {
            format!("oml hook git {e} || true")
        } else {
            format!("oml hook git {e} </dev/null >/dev/null 2>&1 || true")
        };
        out.push_str(&format!("{e}:\n  commands:\n    oml:\n      run: {run}\n"));
    }
    out
}

fn manual_lines(report: &mut Report, why: &str) {
    report.pending = true;
    report.lines.push(format!("git hooks: nothing was written, because {why}; add these lines to the hooks you run, then the first hook that fires clears the pending mark:"));
    for e in GIT_HOOKS {
        report
            .lines
            .push(format!("  {e}: oml hook git {e} \"$@\" || true"));
    }
}

/// Installs the five hooks without overwriting any (d7).
///
/// # Errors
///
/// A hooks directory that cannot be written.
pub fn install(clone: &Clone_) -> Result<Report, String> {
    let mut report = Report::default();
    let hooks_path = git_in(
        &clone.top,
        &["config", "--local", "--get", "core.hooksPath"],
    )
    .map(|s| s.trim().to_owned());
    let dir = clone.git_dir.join("hooks");
    if let Some(configured) = hooks_path.filter(|p| !p.is_empty()) {
        let abs = if Path::new(&configured).is_absolute() {
            PathBuf::from(&configured)
        } else {
            clone.top.join(&configured)
        };
        if abs.starts_with(&clone.top) {
            manual_lines(
                &mut report,
                "core.hooksPath points into the work tree (husky, the pre-commit framework), and a committed hook enrols every clone, which the per-clone setting refuses",
            );
        } else {
            manual_lines(
                &mut report,
                &format!("core.hooksPath {configured} is shared by other clones"),
            );
        }
        return Ok(report);
    }
    if lefthook_config(&clone.top) {
        let local = clone.top.join("lefthook-local.yml");
        if local.exists() {
            manual_lines(&mut report, "lefthook-local.yml already exists");
            return Ok(report);
        }
        let text = lefthook_local(&GIT_HOOKS);
        std::fs::write(&local, &text)
            .map_err(|e| format!("cannot write lefthook-local.yml: {e}"))?;
        exclude(clone, "/lefthook-local.yml", true);
        record(
            clone,
            "lefthook-local.yml",
            json!({"kind": "lefthook", "sha": digest(&text)}),
        )?;
        report.lines.push("git hooks: lefthook found; the commands are in lefthook-local.yml, which lefthook reads for this clone only".to_owned());
        return Ok(report);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    for event in GIT_HOOKS {
        let path = dir.join(event);
        match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let text = format!("#!/bin/sh\n{}\n", hook_line(event));
                std::fs::write(&path, &text)
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                make_executable(&path);
                record(
                    clone,
                    event,
                    json!({"kind": "created", "sha": digest(&text)}),
                )?;
                report.lines.push(format!("git hooks: wrote {event}"));
            }
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
            Ok(existing) if existing.contains(MARKER) => {
                report
                    .lines
                    .push(format!("git hooks: {event} is already installed"));
            }
            Ok(existing) => {
                let first = existing.lines().next().unwrap_or("");
                if !shell_shebang(first) {
                    report.pending = true;
                    report.lines.push(format!("git hooks: {event} is not a shell script, so nothing was written; add `{}` to it", chained_line(event)));
                    continue;
                }
                let backup = clone.oml_dir().join("hooks-backup");
                std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
                std::fs::write(backup.join(event), &existing).map_err(|e| e.to_string())?;
                let (head, rest) = existing.split_once('\n').unwrap_or((&existing, ""));
                let text = format!("{head}\n{}\n{rest}", chained_line(event));
                std::fs::write(&path, &text)
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                record(
                    clone,
                    event,
                    json!({"kind": "chained", "sha": digest(&text), "original": digest(&existing)}),
                )?;
                report.lines.push(format!("git hooks: added one line to the existing {event} (a copy is in .git/oml/hooks-backup/)"));
            }
        }
    }
    Ok(report)
}

fn record(clone: &Clone_, name: &str, entry: Value) -> Result<(), String> {
    let path = hooks_json(clone);
    let mut doc = read_json(&path);
    doc["files"][name] = entry;
    doc["installed_at"] = json!(now_secs());
    write_json(&path, &doc).map_err(|e| e.to_string())
}

fn exclude(clone: &Clone_, line: &str, add: bool) {
    let path = clone.git_dir.join("info/exclude");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let has = text.lines().any(|l| l == line);
    if add && !has {
        let _ = std::fs::create_dir_all(clone.git_dir.join("info"));
        let _ = std::fs::write(
            &path,
            format!(
                "{text}{}{line}\n",
                if text.is_empty() || text.ends_with('\n') {
                    ""
                } else {
                    "\n"
                }
            ),
        );
    } else if !add && has {
        let kept: Vec<&str> = text.lines().filter(|l| *l != line).collect();
        let _ = std::fs::write(&path, format!("{}\n", kept.join("\n")));
    }
}

/// Removes what the record names: an oml-only file whose content is
/// unchanged, a marked line, the lefthook file. A file edited after install
/// stays, with a note (d7).
///
/// # Errors
///
/// A file that cannot be written back.
pub fn uninstall(clone: &Clone_) -> Result<Report, String> {
    let mut report = Report::default();
    let doc = read_json(&hooks_json(clone));
    let dir = clone.git_dir.join("hooks");
    for (name, entry) in doc["files"].as_object().into_iter().flatten() {
        let kind = entry["kind"].as_str().unwrap_or("");
        let recorded = entry["sha"].as_str().unwrap_or("");
        if kind == "lefthook" {
            let path = clone.top.join(name);
            match std::fs::read_to_string(&path) {
                Ok(text) if digest(&text) == recorded => {
                    let _ = std::fs::remove_file(&path);
                    exclude(clone, "/lefthook-local.yml", false);
                    report
                        .lines
                        .push("git hooks: removed lefthook-local.yml".to_owned());
                }
                Ok(_) => report.lines.push(
                    "git hooks: lefthook-local.yml was edited after install, so it stays"
                        .to_owned(),
                ),
                Err(_) => {}
            }
            continue;
        }
        let path = dir.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if digest(&text) != recorded {
            report.lines.push(format!("git hooks: {name} was edited after install, so it stays; remove its `{MARKER}` line by hand"));
            continue;
        }
        if kind == "created" {
            let _ = std::fs::remove_file(&path);
            report.lines.push(format!("git hooks: removed {name}"));
        } else {
            // The copy taken at install is the file as it was.
            let restored = std::fs::read_to_string(clone.oml_dir().join("hooks-backup").join(name))
                .unwrap_or_else(|_| {
                    let kept: Vec<&str> = text.lines().filter(|l| !l.contains(MARKER)).collect();
                    format!("{}\n", kept.join("\n"))
                });
            std::fs::write(&path, restored)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            report
                .lines
                .push(format!("git hooks: removed the oml line from {name}"));
        }
    }
    let _ = std::fs::remove_file(hooks_json(clone));
    Ok(report)
}
