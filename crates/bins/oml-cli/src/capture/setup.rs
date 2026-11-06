//! `oml setup repo` (ADR-0064 d1, d2, d4, d17): the one command a person
//! runs per clone. The first run in a person's account also does the
//! per-person part: `<user>.code`, its code-parent mark under the person's
//! reviewer key, and the person's sync key.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::hooks;
use super::{Clone_, Mode, Source, git_in, workarea_name};
use crate::client::{CallOpts, CliError, Client};
use crate::issue::KeyFile;
use crate::sync::adapter::Kind;
use crate::sync::source::{is_public_server, repo_of_remote};

/// Everything `oml setup repo` was given.
#[derive(Debug, Default)]
pub struct Args {
    pub mode: Option<Mode>,
    pub user: Option<String>,
    pub repo: Option<String>,
    /// Write the clone's configuration and hooks only; the workarea, the
    /// mark and the sync key already exist (set up from another clone).
    pub local: bool,
    pub sync_key_file: Option<PathBuf>,
    pub reviewer_key_file: Option<PathBuf>,
    pub allow_public_server: bool,
    pub url: Option<String>,
}

const KEYS: [&str; 10] = [
    "mode",
    "workarea",
    "syncKeyFile",
    "source",
    "server",
    "repo",
    "user",
    "syncBranch",
    "allowPublicServer",
    "hooksPending",
];

fn write_opts() -> CallOpts {
    CallOpts {
        auth: true,
        idempotency: true,
    }
}

fn set(top: &Path, key: &str, value: &str) -> Result<(), String> {
    git_in(top, &["config", "--local", &format!("oml.{key}"), value])
        .map(|_| ())
        .ok_or_else(|| format!("git config oml.{key} failed"))
}

fn unset(top: &Path, key: &str) {
    let _ = git_in(
        top,
        &["config", "--local", "--unset-all", &format!("oml.{key}")],
    );
}

/// The sources a clone gets by default (d8): decision records, documents,
/// and the Rust adapter when this build has one.
#[must_use]
pub fn default_sources(top: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    let has_adr = std::fs::read_dir(top.join("docs/adr")).is_ok_and(|d| {
        d.flatten().any(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("ADR-") && n.ends_with(".md")
        })
    });
    if has_adr {
        out.push(Source {
            path: "docs/adr".into(),
            adapter: "adr".into(),
        });
    }
    if top.join("docs").is_dir() {
        out.push(Source {
            path: "docs".into(),
            adapter: "headings".into(),
        });
    }
    if top.join("README.md").is_file() {
        out.push(Source {
            path: "README.md".into(),
            adapter: "headings".into(),
        });
    }
    // Behind a check: the adapter is a sync adapter only once it merges.
    if Kind::parse("rust").is_ok() && top.join("Cargo.toml").is_file() {
        out.push(Source {
            path: String::new(),
            adapter: "rust".into(),
        });
    }
    out
}

fn is_not_found(e: &CliError) -> bool {
    matches!(e, CliError::Refused { status: 404, .. })
}

fn refused(e: CliError) -> String {
    match e {
        CliError::Refused { code, message, .. } => format!("{code}: {message}"),
        CliError::Local(m) | CliError::Unanswered(m) => m,
    }
}

fn key_from(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read the key file {}: {e}", path.display()))?;
    Ok(text.trim().to_owned())
}

/// The per-person and per-repository server steps (d2, d3, d4); each is
/// skipped when it is already done.
fn server_steps(
    admin: &Client,
    reviewer: Option<&Client>,
    user: &str,
    name: &str,
    key_file: &Path,
    out: &mut Vec<String>,
) -> Result<(), String> {
    let code = format!("{user}.code");
    match admin.get(&format!("/v2/workareas/{code}")) {
        Ok(_) => {}
        Err(e) if is_not_found(&e) => {
            admin
                .post(
                    "/v2/workareas",
                    &json!({"name": "code", "parent": user}),
                    &write_opts(),
                )
                .map_err(refused)?;
            out.push(format!("created {code}"));
        }
        Err(e) => return Err(refused(e)),
    }
    let marker = reviewer.unwrap_or(admin);
    match marker.post(&format!("/v2/workareas/{code}/mode"), &json!({"children": "agent"}), &write_opts()) {
        Ok(_) => out.push(format!("marked {code} a code parent: agent keys create in-progress repository workareas under it and nowhere else")),
        Err(CliError::Refused { envelope, .. }) if envelope.to_string().contains("mode_is_human_only") => {
            return Err(format!(
                "marking {code} needs your reviewer key, a dreamer key held by you, and the key this CLI holds is not one; pass --reviewer-key-file <file> (R1: no agent and no administrator marks a code parent)"
            ));
        }
        Err(e) => return Err(refused(e)),
    }
    if !key_file.exists() {
        if let Some(dir) = key_file.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let file = KeyFile::create(key_file)?;
        let body = json!({
            "role": "writer",
            "name": format!("{user}-code-sync"),
            "held_by": "agent",
            "principal": format!("{user}/code-sync"),
            "user": user,
            "default_workarea": code,
            "grants": [{"workarea": code, "role": "writer"}, {"workarea": code, "role": "reader"}],
        });
        let response = match admin.post("/v2/keys", &body, &write_opts()) {
            Ok((_, response)) => response,
            Err(e) => {
                let _ = file.discard();
                return Err(refused(e));
            }
        };
        let value = response["value"].as_str().ok_or(
            "the sync key was issued but the answer carried no value; revoke it and run again",
        )?;
        file.write_value(value)?;
        out.push(format!(
            "issued the sync key {user}-code-sync to {}",
            key_file.display()
        ));
    }
    let sync = Client::new(admin.server().to_owned(), Some(key_from(key_file)?));
    match sync.get(&format!("/v2/workareas/{user}.{name}")) {
        Ok(_) => {}
        Err(e) if is_not_found(&e) => {
            sync.post(
                "/v2/workareas",
                &json!({"name": name, "parent": code, "mode": "in_progress"}),
                &write_opts(),
            )
            .map_err(refused)?;
            out.push(format!(
                "created {user}.{name}, in progress from its first instant"
            ));
        }
        Err(e) => return Err(refused(e)),
    }
    Ok(())
}

/// Runs `oml setup repo`; the lines to print, or the refusal.
///
/// # Errors
///
/// A refusal that names its reason and the flag that resolves it.
pub fn run(args: &Args, cwd: &Path, admin: Option<&Client>) -> Result<Vec<String>, String> {
    let clone = Clone_::open(cwd).ok_or("oml setup repo runs inside a git clone")?;
    let top = clone.top.clone();
    let mut out = Vec::new();
    let defaulted = args.mode.is_none();
    let mode = args.mode.unwrap_or(Mode::DEFAULT);

    if mode == Mode::Off {
        let report = hooks::uninstall(&clone)?;
        out.extend(report.lines);
        for key in KEYS {
            unset(&top, key);
        }
        let _ = std::fs::remove_dir_all(clone.oml_dir());
        out.push("mode off: the git hooks and the oml.* keys are removed; every memory and the workarea stay where they are".to_owned());
        return Ok(out);
    }

    let repo = clone
        .repo()
        .or_else(|| args.repo.clone())
        .ok_or("this clone has no origin remote; pass --repo <owner/name> (ADR-0064 d4)")?;
    let repo = args.repo.clone().unwrap_or(repo);
    if let Some(url) = git_in(&top, &["remote", "get-url", "origin"])
        && repo_of_remote(url.trim()).is_ok_and(|r| r.had_credentials)
    {
        out.push("warn: the origin remote carries credentials; the anchors strip them, and they belong in a credential helper (ADR-0057 d3)".to_owned());
    }
    if let Some(url) = &args.url
        && is_public_server(url)
    {
        if !args.allow_public_server {
            return Err(format!(
                "{url} is the maintainer's public server: its admins read every memory and nothing written can be erased, so client code is not captured there unless you pass --allow-public-server (ADR-0064 d17, ADR-0047 question 11)"
            ));
        }
        set(&top, "allowPublicServer", "true")?;
        out.push("warn: capturing to the public server: its admins read every memory, and nothing written can be erased".to_owned());
    }
    let user = args
        .user
        .clone()
        .or_else(|| clone.get("user").map(ToOwned::to_owned))
        .ok_or("--user <your user root> is required the first time in a clone (ADR-0044)")?;
    let name = workarea_name(&repo);
    let key_file = match (&args.sync_key_file, clone.get("synckeyfile")) {
        (Some(p), _) => p.clone(),
        (None, Some(p)) => PathBuf::from(p),
        (None, None) => {
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
                .ok_or("neither XDG_CONFIG_HOME nor HOME names a directory for the sync key")?;
            base.join("oml/keys").join(format!("{user}-code-sync.key"))
        }
    };
    let key_file = std::path::absolute(&key_file).unwrap_or(key_file);

    if !args.local {
        let admin = admin.ok_or("no server: pass --url and --api-key, or set OML_URL and OML_API_KEY (or --local to write this clone's settings only)")?;
        let reviewer = match &args.reviewer_key_file {
            Some(p) => Some(Client::new(admin.server().to_owned(), Some(key_from(p)?))),
            None => None,
        };
        server_steps(admin, reviewer.as_ref(), &user, &name, &key_file, &mut out)?;
    }

    set(&top, "mode", mode.as_str())?;
    set(&top, "user", &user)?;
    set(&top, "workarea", &format!("{user}.{name}"))?;
    set(&top, "syncKeyFile", &key_file.to_string_lossy())?;
    if let Some(r) = &args.repo {
        set(&top, "repo", r)?;
    }
    if let Some(url) = &args.url {
        set(&top, "server", url.trim_end_matches('/'))?;
    }
    unset(&top, "source");
    for s in default_sources(&top) {
        git_in(
            &top,
            &["config", "--local", "--add", "oml.source", &s.key()],
        )
        .ok_or("git config oml.source failed")?;
    }

    // Hooks are what the mode says: installed for git and both, removed
    // for agent.
    let clone = Clone_::open(cwd).ok_or("the clone vanished")?;
    let report = if mode.includes_git() {
        hooks::install(&clone)?
    } else {
        hooks::uninstall(&clone)?
    };
    if report.pending {
        set(&top, "hooksPending", "true")?;
    }
    out.extend(report.lines);
    if defaulted {
        out.push("mode both (the default): the git hooks see every commit, including a person's commits made outside any agent, and run with no harness and no network; the agent hooks are the only place an anchor can be filled for what an agent learned; the overlap costs nothing, since one sync key owns the sync. Pass --mode agent when a shared hook manager owns this clone's hooks, or --mode git for a clone no agent works in.".to_owned());
    }
    out.push(format!("mode {}: {user}.{name} for {repo}", mode.as_str()));
    Ok(out)
}

#[must_use]
pub fn json_of(lines: &[String]) -> Value {
    json!({ "lines": lines })
}
