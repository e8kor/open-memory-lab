//! Implicit code capture (ADR-0064): the clone's mode, its workarea, its
//! sources and the state the git hooks and the agent hooks share under
//! `.git/oml/`. Everything here reads the clone's local git config and its
//! files; nothing here opens a network connection.

pub mod agent;
pub mod hooks;
pub mod job;
pub mod review;
pub mod setup;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::sync::hash::sha256;

/// Which layers capture a clone's knowledge (d1). A clone without
/// `oml.mode` is [`Mode::Off`], whatever the person's other clones say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Agent,
    Git,
    Both,
    Off,
}

impl Mode {
    /// The mode `oml setup repo` takes when none is named (question 7, a).
    pub const DEFAULT: Self = Self::Both;

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "agent" => Some(Self::Agent),
            "git" => Some(Self::Git),
            "both" => Some(Self::Both),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Git => "git",
            Self::Both => "both",
            Self::Off => "off",
        }
    }

    #[must_use]
    pub fn includes_git(self) -> bool {
        matches!(self, Self::Git | Self::Both)
    }

    #[must_use]
    pub fn includes_agent(self) -> bool {
        matches!(self, Self::Agent | Self::Both)
    }
}

/// The workarea name of a repository (d4): its last segment and the first
/// six hex digits of SHA-256 of the repository, a pure function of the
/// repository, so two clones on two machines find one workarea and two
/// repositories with one last segment never collide. A name is a label of
/// at most 63 characters that does not read as an id (ADR-0026 d2), and a
/// name is resolved as `<user>.<name>` (ADR-0044 d5).
#[must_use]
pub fn workarea_name(repo: &str) -> String {
    let digest = sha256(repo.as_bytes());
    let hex = &digest["sha256:".len().."sha256:".len() + 6];
    let last = repo.rsplit('/').next().unwrap_or(repo).to_lowercase();
    let mut label: String = last
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if !label.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        label.insert_str(0, "repo-");
    }
    // A name made of hex digits and dashes alone would read as an id.
    if label.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        label.insert_str(0, "repo-");
    }
    label.truncate(63 - 7);
    format!("{}-{hex}", label.trim_end_matches('-'))
}

/// One source of a clone: a path and the adapter that reads it, recorded in
/// `oml.source` as `<path>:<adapter>` (d8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: String,
    pub adapter: String,
}

impl Source {
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}:{}", self.path, self.adapter)
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (path, adapter) = text.rsplit_once(':')?;
        Some(Self {
            path: path.to_owned(),
            adapter: adapter.to_owned(),
        })
    }

    /// Whether a repository-relative path lies under the source.
    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        self.path.is_empty()
            || path == self.path
            || path
                .strip_prefix(self.path.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// The paths no source reads, whatever the adapter (d8).
pub const DEFAULT_EXCLUDES: [&str; 8] = [
    "target/**",
    "vendor/**",
    "third_party/**",
    "node_modules/**",
    "dist/**",
    "build/**",
    "**/*.generated.*",
    "**/*.pb.*",
];

/// The excludes of `source` among `all`: the defaults, and a source nested
/// under another leaves the nested one's paths to its own adapter (d8).
#[must_use]
pub fn excludes_for(source: &Source, all: &[Source]) -> Vec<String> {
    let mut out: Vec<String> = DEFAULT_EXCLUDES.iter().map(|s| (*s).to_owned()).collect();
    for other in all {
        if other != source
            && other.path != source.path
            && other.path.starts_with(&format!("{}/", source.path))
        {
            out.push(format!("{}/**", other.path));
        }
    }
    out
}

/// A git clone as the capture layers read it: the work tree, the git
/// directory and the clone's `oml.*` local configuration, from three git
/// calls and no network.
#[derive(Clone, Debug)]
pub struct Clone_ {
    pub top: PathBuf,
    pub git_dir: PathBuf,
    cfg: BTreeMap<String, Vec<String>>,
}

pub(crate) fn git_in(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

impl Clone_ {
    /// The clone containing `cwd`, or `None` outside one.
    #[must_use]
    pub fn open(cwd: &Path) -> Option<Self> {
        let paths = git_in(cwd, &["rev-parse", "--show-toplevel", "--absolute-git-dir"])?;
        let mut lines = paths.lines();
        let top = PathBuf::from(lines.next()?);
        let git_dir = PathBuf::from(lines.next()?);
        let mut cfg: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if let Some(text) = git_in(
            &top,
            &["config", "--local", "--null", "--get-regexp", r"^oml\."],
        ) {
            for entry in text.split('\0').filter(|e| !e.is_empty()) {
                let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
                cfg.entry(key.to_lowercase())
                    .or_default()
                    .push(value.to_owned());
            }
        }
        Some(Self { top, git_dir, cfg })
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.cfg
            .get(&format!("oml.{}", key.to_lowercase()))
            .and_then(|v| v.last())
            .map(String::as_str)
    }

    #[must_use]
    pub fn mode(&self) -> Mode {
        self.get("mode").and_then(Mode::parse).unwrap_or(Mode::Off)
    }

    #[must_use]
    pub fn sources(&self) -> Vec<Source> {
        self.cfg
            .get("oml.source")
            .into_iter()
            .flatten()
            .filter_map(|s| Source::parse(s))
            .collect()
    }

    /// `.git/oml/`, where the queue, the state, the log and the checks live.
    #[must_use]
    pub fn oml_dir(&self) -> PathBuf {
        self.git_dir.join("oml")
    }

    #[must_use]
    pub fn head(&self) -> Option<String> {
        git_in(&self.top, &["rev-parse", "-q", "--verify", "HEAD"]).map(|s| s.trim().to_owned())
    }

    /// The current branch, `None` when detached.
    #[must_use]
    pub fn branch(&self) -> Option<String> {
        git_in(&self.top, &["symbolic-ref", "-q", "--short", "HEAD"]).map(|s| s.trim().to_owned())
    }

    /// The branch syncing follows (d5): `oml.syncBranch`, else `origin/HEAD`'s
    /// target, else `main` or `master` when the current branch is one.
    #[must_use]
    pub fn sync_branch(&self) -> Option<String> {
        if let Some(b) = self.get("syncbranch") {
            return Some(b.to_owned());
        }
        if let Some(r) = git_in(
            &self.top,
            &["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"],
        ) {
            return r.trim().strip_prefix("origin/").map(ToOwned::to_owned);
        }
        self.branch().filter(|b| b == "main" || b == "master")
    }

    #[must_use]
    pub fn on_sync_branch(&self) -> bool {
        match (self.branch(), self.sync_branch()) {
            (Some(b), Some(s)) => b == s,
            _ => false,
        }
    }

    /// The repository of the `origin` remote with its credentials dropped
    /// (ADR-0057 d3), or the one `oml setup repo --repo` recorded.
    #[must_use]
    pub fn repo(&self) -> Option<String> {
        if let Some(r) = self.get("repo") {
            return Some(r.to_owned());
        }
        let url = git_in(&self.top, &["remote", "get-url", "origin"])?;
        crate::sync::source::repo_of_remote(url.trim())
            .ok()
            .map(|r| r.repo)
    }

    /// Whether git ignores `rel` (a path never opened by a hook, d17).
    #[must_use]
    pub fn ignored(&self, rel: &str) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&self.top)
            .args(["check-ignore", "-q", "--", rel])
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// The paths a hook may open: inside the clone, not a symlink leading
    /// out, not ignored by git, not under a default exclude and not secret-shaped
    /// (d15, d17). `abs` is an absolute path or one relative to the top.
    /// Returns the clone-relative path.
    #[must_use]
    pub fn openable(&self, abs: &Path) -> Option<String> {
        let joined = if abs.is_absolute() {
            abs.to_path_buf()
        } else {
            self.top.join(abs)
        };
        let real_top = self.top.canonicalize().ok()?;
        let real = joined.canonicalize().ok().or_else(|| {
            // A file the session created may not exist yet; its parent does.
            let parent = joined.parent()?.canonicalize().ok()?;
            Some(parent.join(joined.file_name()?))
        })?;
        let rel = real.strip_prefix(&real_top).ok()?.to_str()?.to_owned();
        if rel.is_empty() || rel.starts_with(".git/") || rel == ".git" {
            return None;
        }
        if crate::sync::source::is_secret_shaped(&rel)
            || DEFAULT_EXCLUDES
                .iter()
                .any(|g| crate::sync::source::glob_match(g, &rel))
            || self.ignored(&rel)
        {
            return None;
        }
        Some(rel)
    }
}

/// `.git/oml/state.json`, read leniently: a missing or unreadable file is an
/// empty object.
#[must_use]
pub fn read_json(path: &Path) -> Value {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
}

/// Writes `value` to `path` through a temporary file and a rename, owner
/// only, creating the directory.
///
/// # Errors
///
/// The directory or file cannot be written.
pub fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    crate::owner_only::open_options(&mut options);
    let mut file = options.open(&tmp)?;
    file.write_all(value.to_string().as_bytes())?;
    drop(file);
    std::fs::rename(&tmp, path)
}

/// Seconds since the epoch, for state files.
#[must_use]
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The `oml doctor` lines of a mapped clone (ADR-0064 d23) that the clone's
/// own files answer; empty in a clone with no `oml.mode`. The checks that
/// need the server (a workarea out of progress, held items older than 14
/// days) are not here.
#[must_use]
pub fn doctor_lines(cwd: &Path) -> Vec<String> {
    let Some(clone) = Clone_::open(cwd) else {
        return Vec::new();
    };
    let mode = clone.mode();
    if mode == Mode::Off {
        return Vec::new();
    }
    let mut out = Vec::new();
    let dir = clone.oml_dir();
    let state = read_json(&dir.join("state.json"));
    if clone.get("workarea").is_none() {
        out.push(
            "warn capture_unmapped: oml.mode is set and oml.workarea is not; run oml setup repo"
                .to_owned(),
        );
    }
    if clone.repo().is_none() {
        out.push("warn capture_no_repo: this clone has no origin remote and no oml.repo; run oml setup repo --repo <owner/name>".to_owned());
    }
    if mode.includes_git() {
        let record = read_json(&dir.join("hooks.json"));
        if clone.get("hookspending").is_some() {
            out.push("warn capture_hooks_pending: the hook lines were printed for a committed hook config and not yet added; the first hook that fires clears this".to_owned());
        } else if record["installed_at"].is_u64() && state["hook_at"].is_null() {
            out.push("warn capture_hooks_silent: no git hook has fired since install; a hook manager reset core.hooksPath, or the committed config was never edited (FR-176)".to_owned());
        }
        for (name, entry) in record["files"].as_object().into_iter().flatten() {
            let path = if entry["kind"] == "lefthook" {
                clone.top.join(name)
            } else {
                clone.git_dir.join("hooks").join(name)
            };
            if let (Ok(text), Some(sha)) = (std::fs::read_to_string(&path), entry["sha"].as_str())
                && crate::sync::hash::sha256(text.as_bytes()) != sha
            {
                out.push(format!("warn capture_hook_edited: {name} was edited after oml setup repo marked it; uninstall leaves it in place"));
            }
        }
    }
    match clone.get("synckeyfile") {
        None => out.push("warn capture_sync_key: oml.syncKeyFile is not set".to_owned()),
        Some(file) => match std::fs::metadata(file) {
            Err(_) => out.push(format!("warn capture_sync_key: the sync key file {file} is missing; the background sync cannot run")),
            Ok(meta) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    if meta.permissions().mode() & 0o077 != 0 {
                        out.push(format!("warn capture_sync_key: the sync key file {file} is not owner-only; chmod 600 it"));
                    }
                }
                let _ = meta;
            }
        },
    }
    let queue = job::Queue::load(&clone);
    if !queue.is_empty() {
        let old = std::fs::metadata(dir.join("queue.json"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > std::time::Duration::from_secs(86_400));
        let last = state["job"]["error"].as_str().unwrap_or("none recorded");
        if old {
            out.push(format!("warn capture_queue_old: a queued sync is more than a day old (offline, or the job is failing); the job's last error: {last}"));
        }
    }
    if let Some(error) = state["job"]["error"].as_str() {
        if error.contains("--ff-only") {
            out.push(format!("warn capture_ff_only: {error}"));
        }
        if error.contains("foreign") {
            out.push(format!("warn capture_foreign_source: {error}"));
        }
    }
    if let Ok(log) = std::fs::read_to_string(dir.join("budget.log")) {
        let mut events: Vec<String> = log
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|v| v["event"].as_str().map(ToOwned::to_owned))
            .collect();
        events.sort();
        events.dedup();
        if !events.is_empty() {
            out.push(format!(
                "warn capture_over_budget: a git hook's foreground went over its 100 ms budget: {}",
                events.join(", ")
            ));
        }
    }
    if out.is_empty() {
        out.push(format!(
            "ok capture: mode {}, workarea {}",
            mode.as_str(),
            clone.get("workarea").unwrap_or("?")
        ));
    }
    out
}
