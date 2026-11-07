//! The background sync (ADR-0064 d8, d9): the queue a hook appends to, the
//! lock one job holds, and `oml sync --flush`, which runs each entry as an
//! ordinary `oml sync` under the person's sync key and records the check
//! that `pre-push` prints. Nothing here runs in a git hook's foreground.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Clone_, now_secs, read_json, write_json};

/// A job stops between requests after this long and leaves the queue (d8).
pub const WALL_CLOCK: Duration = Duration::from_secs(120);
/// The most memories one run writes (ADR-0057 d9).
pub const MAX_WRITES: &str = "200";

/// One source's newest commit to sync, with the paths changed since the
/// last sync: a sync is a diff of state at one commit, so the newest
/// subsumes the older and the paths are united (d8).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub source: String,
    pub adapter: String,
    pub commit: String,
    /// Empty: the whole source.
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub reason: String,
    /// A rebase or amend produced the commit (d6); the sync is still
    /// fast-forward only and never passes `--allow-rollback`.
    #[serde(default)]
    pub rewritten: bool,
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl QueueEntry {
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}:{}", self.source, self.adapter)
    }
}

/// `.git/oml/queue.json`: the `entries` to sync and the `checks` to run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queue {
    #[serde(default)]
    pub entries: BTreeMap<String, QueueEntry>,
    #[serde(default)]
    pub checks: BTreeMap<String, QueueEntry>,
}

fn united(old: &[String], new: &[String]) -> Vec<String> {
    if old.is_empty() || new.is_empty() {
        return Vec::new();
    }
    let mut all: Vec<String> = old.iter().chain(new).cloned().collect();
    all.sort();
    all.dedup();
    all
}

impl Queue {
    /// The queue of `clone`; empty when absent or unreadable.
    #[must_use]
    pub fn load(clone: &Clone_) -> Self {
        serde_json::from_value(read_json(&clone.oml_dir().join("queue.json"))).unwrap_or_default()
    }

    /// Adds `entry`: a newer commit replaces the older one of its source.
    pub fn enqueue(&mut self, entry: QueueEntry) {
        let key = entry.key();
        let merged = match self.entries.remove(&key) {
            Some(old) => QueueEntry {
                paths: united(&old.paths, &entry.paths),
                rewritten: old.rewritten || entry.rewritten,
                ..entry
            },
            None => entry,
        };
        self.entries.insert(key, merged);
    }

    pub fn enqueue_check(&mut self, entry: QueueEntry) {
        self.checks
            .insert(format!("{}:{}", entry.commit, entry.key()), entry);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.checks.is_empty()
    }

    /// Writes the queue, under the queue lock.
    ///
    /// # Errors
    ///
    /// The file cannot be written.
    pub fn save(&self, clone: &Clone_) -> std::io::Result<()> {
        write_json(
            &clone.oml_dir().join("queue.json"),
            &serde_json::to_value(self).unwrap_or(Value::Null),
        )
    }

    /// Loads the queue, applies `change` and saves it, holding the queue
    /// lock so two hooks do not lose each other's entries.
    ///
    /// # Errors
    ///
    /// The lock or the file cannot be used.
    pub fn update(clone: &Clone_, change: impl FnOnce(&mut Self)) -> std::io::Result<Self> {
        std::fs::create_dir_all(clone.oml_dir())?;
        let lock = File::create(clone.oml_dir().join("queue.lock"))?;
        lock.lock()?;
        let mut queue = Self::load(clone);
        change(&mut queue);
        queue.save(clone)?;
        Ok(queue)
    }
}

/// The arguments of the `oml sync` that runs `entry` (d8): the commit named
/// by `--rev`, only the changed paths, fast-forward only, bounded, and never
/// `--allow-rollback`, whatever produced the commit.
#[must_use]
pub fn sync_args(entry: &QueueEntry, workarea: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "sync",
        &entry.source,
        "--workarea",
        workarea,
        "--adapter",
        &entry.adapter,
        "--rev",
        &entry.commit,
        "--ff-only",
        "--max-writes",
        MAX_WRITES,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    for path in &entry.paths {
        args.extend(["--paths".to_owned(), path.clone()]);
    }
    for glob in &entry.exclude {
        args.extend(["--exclude".to_owned(), glob.clone()]);
    }
    args
}

/// [`sync_args`] as the check of d9: the same listing and paths, no write.
#[must_use]
pub fn check_args(entry: &QueueEntry, workarea: &str) -> Vec<String> {
    let mut args = vec!["--json".to_owned()];
    args.extend(
        sync_args(entry, workarea)
            .into_iter()
            .filter(|a| a != "--ff-only" && a != "--max-writes" && a != MAX_WRITES),
    );
    args.push("--check".to_owned());
    args
}

fn record_state(clone: &Clone_, change: impl FnOnce(&mut serde_json::Map<String, Value>)) {
    let path = clone.oml_dir().join("state.json");
    let mut state = read_json(&path);
    if let Some(map) = state.as_object_mut() {
        change(map);
    }
    let _ = write_json(&path, &state);
}

/// Runs `args` as `oml`, with the sync key and server in its environment
/// alone, and stops it when `deadline` passes.
fn run_oml(
    clone: &Clone_,
    args: &[String],
    server: &str,
    key: &str,
    deadline: Instant,
) -> Result<(i32, String, String), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find oml itself: {e}"))?;
    let mut child = Command::new(exe)
        .args(args)
        .current_dir(&clone.top)
        .env("OML_URL", server)
        .env("OML_API_KEY", key)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start oml sync: {e}"))?;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let (mut o, mut e) = (String::new(), String::new());
        if let Some(s) = out.as_mut() {
            let _ = s.read_to_string(&mut o);
        }
        if let Some(s) = err.as_mut() {
            let _ = s.read_to_string(&mut e);
        }
        (o, e)
    });
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let (o, e) = reader.join().unwrap_or_default();
                return Ok((status.code().unwrap_or(-1), o, e));
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the job reached its wall-clock limit of 120 s".to_owned());
            }
            Err(e) => return Err(format!("waiting for oml sync: {e}")),
        }
    }
}

/// The sync key, read from `oml.syncKeyFile`, which must be owner-only.
fn sync_key(clone: &Clone_) -> Result<String, String> {
    let file = clone
        .get("synckeyfile")
        .ok_or("oml.syncKeyFile is not set")?;
    let path = Path::new(file);
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("the sync key file {file} is unreadable: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(format!("the sync key file {file} is not owner-only"));
        }
    }
    let _ = meta;
    let key = std::fs::read_to_string(path)
        .map_err(|e| format!("the sync key file {file} is unreadable: {e}"))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(format!("the sync key file {file} is empty"));
    }
    Ok(key.to_owned())
}

fn server_of(clone: &Clone_) -> Option<String> {
    clone
        .get("server")
        .map(ToOwned::to_owned)
        .or_else(|| std::env::var("OML_URL").ok())
        .or_else(|| crate::creds::load().url)
}

/// What a check run found, folded into `.git/oml/check/<commit>.json`
/// beside the other sources' (d9).
fn store_check(clone: &Clone_, entry: &QueueEntry, document: &Value) {
    let (mut stale, mut removed, mut untracked) = (0_u64, 0_u64, 0_u64);
    let mut claims = Vec::new();
    for claim in document["claims"].as_array().into_iter().flatten() {
        let state = claim["state"].as_str().unwrap_or("");
        match state {
            "stale" => stale += 1,
            "removed" => removed += 1,
            "untracked" => untracked += 1,
            _ => continue,
        }
        claims.push(json!({
            "state": state,
            "path": claim["anchor"]["path"],
            "fragment": claim["anchor"]["fragment"],
        }));
    }
    let path = clone
        .oml_dir()
        .join("check")
        .join(format!("{}.json", entry.commit));
    let mut file = read_json(&path);
    file["by_source"][entry.key()] =
        json!({"stale": stale, "removed": removed, "untracked": untracked, "claims": claims});
    let (mut s, mut r, mut u) = (0, 0, 0);
    let mut all = Vec::new();
    for part in file["by_source"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
    {
        s += part["stale"].as_u64().unwrap_or(0);
        r += part["removed"].as_u64().unwrap_or(0);
        u += part["untracked"].as_u64().unwrap_or(0);
        all.extend(part["claims"].as_array().cloned().unwrap_or_default());
    }
    all.truncate(20);
    file["stale"] = json!(s);
    file["removed"] = json!(r);
    file["untracked"] = json!(u);
    file["claims"] = json!(all);
    let _ = write_json(&path, &file);
}

/// `oml sync --flush` (d8): runs every queued entry, newest first of each
/// source, under the lock. Offline, a refused guard or a missing key leaves
/// the entry and ends the job with a line in the log and `state.json`; the
/// job exits 0, since a hook started it and nothing waits for it.
///
/// # Errors
///
/// `cwd` is not inside a clone mapped by `oml setup repo`.
pub fn flush(cwd: &Path) -> Result<u8, String> {
    let clone = Clone_::open(cwd).ok_or("oml sync --flush runs inside a git clone")?;
    if clone.mode() == super::Mode::Off {
        return Err("this clone is not mapped: run oml setup repo".to_owned());
    }
    std::fs::create_dir_all(clone.oml_dir()).map_err(|e| e.to_string())?;
    let lock = File::create(clone.oml_dir().join("sync.lock")).map_err(|e| e.to_string())?;
    match lock.try_lock() {
        Ok(()) => {}
        // A job holds it and re-reads the queue before it releases (d8).
        Err(std::fs::TryLockError::WouldBlock) => return Ok(0),
        Err(std::fs::TryLockError::Error(e)) => return Err(e.to_string()),
    }
    let deadline = Instant::now() + WALL_CLOCK;
    let fail = |why: String| {
        record_state(&clone, |s| {
            s.insert(
                "job".into(),
                json!({"ok": false, "error": why, "at": now_secs()}),
            );
        });
        eprintln!("oml sync --flush: {why}");
    };
    let Some(workarea) = clone.get("workarea").map(ToOwned::to_owned) else {
        fail("oml.workarea is not set; run oml setup repo".to_owned());
        return Ok(0);
    };
    let key = match sync_key(&clone) {
        Ok(key) => key,
        Err(why) => {
            fail(why);
            return Ok(0);
        }
    };
    let Some(server) = server_of(&clone) else {
        fail("no server is recorded for this clone; run oml setup repo --url".to_owned());
        return Ok(0);
    };
    if crate::sync::source::is_public_server(&server)
        && clone.get("allowpublicserver") != Some("true")
    {
        fail(format!(
            "{server} is the public server and --allow-public-server is not recorded for this clone (ADR-0064 d17)"
        ));
        return Ok(0);
    }
    let public = crate::sync::source::is_public_server(&server);
    let with_public = |mut args: Vec<String>| {
        if public {
            args.push("--allow-public-server".to_owned());
        }
        args
    };
    loop {
        let queue = Queue::load(&clone);
        let (entry, is_check) = match (queue.entries.values().next(), queue.checks.values().next())
        {
            (Some(e), _) => (e.clone(), false),
            (None, Some(c)) => (c.clone(), true),
            (None, None) => break,
        };
        let args = if is_check {
            check_args(&entry, &workarea)
        } else {
            sync_args(&entry, &workarea)
        };
        match run_oml(&clone, &with_public(args), &server, &key, deadline) {
            Err(why) => {
                fail(why);
                return Ok(0);
            }
            Ok((code, stdout, stderr)) => {
                let ran_check =
                    is_check && code <= 1 && serde_json::from_str::<Value>(&stdout).is_ok();
                if ran_check {
                    if let Ok(doc) = serde_json::from_str::<Value>(&stdout) {
                        store_check(&clone, &entry, &doc);
                    }
                } else if code != 0 {
                    // Offline, refused by the fast-forward guard, or
                    // failed: the entry stays for the next flush.
                    fail(
                        stderr
                            .lines()
                            .last()
                            .unwrap_or("oml sync failed")
                            .to_owned(),
                    );
                    return Ok(0);
                } else if let Ok((_, doc, _)) = run_oml(
                    &clone,
                    &with_public(check_args(&entry, &workarea)),
                    &server,
                    &key,
                    deadline,
                ) && let Ok(doc) = serde_json::from_str::<Value>(&doc)
                {
                    store_check(&clone, &entry, &doc);
                }
                let done = entry.commit.clone();
                let key_of = entry.key();
                let _ = Queue::update(&clone, |q| {
                    // A newer commit queued meanwhile stays (d8).
                    let bucket = if is_check {
                        &mut q.checks
                    } else {
                        &mut q.entries
                    };
                    let k = if is_check {
                        format!("{done}:{key_of}")
                    } else {
                        key_of.clone()
                    };
                    if bucket.get(&k).is_some_and(|e| e.commit == done) {
                        bucket.remove(&k);
                    }
                });
                record_state(&clone, |s| {
                    s.insert(
                        "job".into(),
                        json!({"ok": true, "commit": done, "at": now_secs()}),
                    );
                });
            }
        }
    }
    Ok(0)
}
