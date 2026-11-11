//! The source a run reads: committed files of one git clone at one commit,
//! through git's object store and never the working tree (ADR-0057 d6), and
//! the repository's name derived from its `origin` remote without
//! credentials (d3).

use std::path::{Path, PathBuf};
use std::process::Command;

/// A git clone, addressed by a directory inside it.
pub struct Git {
    dir: PathBuf,
}

impl Git {
    /// The clone containing `dir`.
    ///
    /// # Errors
    /// `dir` is not inside a git clone, or git is not installed.
    pub fn at(dir: &Path) -> Result<Self, String> {
        let git = Self {
            dir: dir.to_path_buf(),
        };
        let top = git
            .run(&["rev-parse", "--show-toplevel"])
            .map_err(|e| format!("{} is not inside a git clone ({e}); run oml sync from a clone because only committed files are read", dir.display()))?;
        Ok(Self {
            dir: PathBuf::from(top.trim()),
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.dir
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&self.dir).args(args);
        // The run reads the object store alone; no pager, no prompt.
        cmd.env("GIT_TERMINAL_PROMPT", "0").env("GIT_PAGER", "cat");
        cmd
    }

    fn run(&self, args: &[&str]) -> Result<String, String> {
        let bytes = self.run_bytes(args)?;
        String::from_utf8(bytes).map_err(|e| format!("git {}: not UTF-8 ({e})", args.join(" ")))
    }

    fn run_bytes(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let out = self
            .command(args)
            .output()
            .map_err(|e| format!("cannot run git: {e}"))?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    /// The full object id of the commit `rev` names.
    ///
    /// # Errors
    /// `rev` names no commit.
    pub fn commit(&self, rev: &str) -> Result<String, String> {
        self.run(&[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{rev}^{{commit}}"),
        ])
        .map(|s| s.trim().to_owned())
        .map_err(|_| {
            format!(
                "--rev {rev:?} is not a commit in {}; fetch it or choose another revision",
                self.dir.display()
            )
        })
    }

    /// Whether this clone holds the commit `rev`.
    #[must_use]
    pub fn knows(&self, rev: &str) -> bool {
        self.command(&["cat-file", "-e", &format!("{rev}^{{commit}}")])
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// Whether `ancestor` is a strict ancestor of `descendant`; false when
    /// either is unknown to this clone.
    #[must_use]
    pub fn is_strict_ancestor(&self, ancestor: &str, descendant: &str) -> bool {
        ancestor != descendant
            && self
                .command(&["merge-base", "--is-ancestor", ancestor, descendant])
                .output()
                .is_ok_and(|o| o.status.success())
    }

    /// The files under `path` (a file or a directory, repository-relative)
    /// in the tree of `commit`, sorted by path.
    ///
    /// # Errors
    /// git fails.
    pub fn files(&self, commit: &str, path: &str) -> Result<Vec<String>, String> {
        let spec = if path.is_empty() { "." } else { path };
        let out = self.run_bytes(&["ls-tree", "-r", "-z", "--name-only", commit, "--", spec])?;
        let mut files: Vec<String> = out
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        files.sort();
        Ok(files)
    }

    /// The files changed between the merge base of `base` and `head`, and
    /// `head`, under `path`, present at `head`: what a pull request adds or
    /// changes (ADR-0057 d14).
    ///
    /// # Errors
    /// git fails.
    pub fn changed_files(&self, base: &str, head: &str, path: &str) -> Result<Vec<String>, String> {
        let base = self.run(&["merge-base", base, head])?;
        let spec = if path.is_empty() { "." } else { path };
        let out = self.run_bytes(&[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--diff-filter=AM",
            base.trim(),
            head,
            "--",
            spec,
        ])?;
        let mut files: Vec<String> = out
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        files.sort();
        Ok(files)
    }

    /// The bytes of `path` at `commit`.
    ///
    /// # Errors
    /// git fails.
    pub fn read(&self, commit: &str, path: &str) -> Result<Vec<u8>, String> {
        self.run_bytes(&["cat-file", "blob", &format!("{commit}:{path}")])
    }

    /// The `origin` remote's URL, if one is set.
    #[must_use]
    pub fn origin_url(&self) -> Option<String> {
        self.run(&["remote", "get-url", "origin"])
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    }

    /// A local git config value of this clone.
    #[must_use]
    pub fn config(&self, name: &str) -> Option<String> {
        self.run(&["config", "--local", "--get", name])
            .ok()
            .map(|s| s.trim().to_owned())
    }

    /// Every value of a multi-valued local git config entry of this clone;
    /// empty when it is unset.
    #[must_use]
    pub fn config_all(&self, name: &str) -> Vec<String> {
        self.run(&["config", "--local", "--get-all", name])
            .map(|s| s.lines().map(|l| l.trim().to_owned()).collect())
            .unwrap_or_default()
    }

    /// Adds one value to a multi-valued local git config entry of this
    /// clone, keeping the values it holds.
    ///
    /// # Errors
    /// git fails.
    pub fn add_config(&self, name: &str, value: &str) -> Result<(), String> {
        self.run(&["config", "--local", "--add", name, value])
            .map(|_| ())
    }

    /// Sets a local git config value of this clone.
    ///
    /// # Errors
    /// git fails.
    pub fn set_config(&self, name: &str, value: &str) -> Result<(), String> {
        self.run(&["config", "--local", name, value]).map(|_| ())
    }

    /// `arg` (relative to `cwd`, or absolute) as a path relative to the
    /// clone's root, normalised; refused when it leaves the clone.
    ///
    /// # Errors
    /// The path is outside the clone.
    pub fn relative(&self, cwd: &Path, arg: &str) -> Result<String, String> {
        let joined = if Path::new(arg).is_absolute() {
            PathBuf::from(arg)
        } else {
            cwd.join(arg)
        };
        let root = self.dir.canonicalize().unwrap_or_else(|_| self.dir.clone());
        let cwd_abs = |p: &Path| -> PathBuf {
            // The source may be absent from the working tree yet present
            // at --rev: canonicalize the deepest existing ancestor.
            let mut existing = p.to_path_buf();
            let mut rest = Vec::new();
            while !existing.exists() {
                match (existing.file_name(), existing.parent()) {
                    (Some(name), Some(parent)) => {
                        rest.push(name.to_owned());
                        existing = parent.to_path_buf();
                    }
                    _ => break,
                }
            }
            let mut out = existing.canonicalize().unwrap_or(existing);
            for name in rest.iter().rev() {
                out.push(name);
            }
            out
        };
        let full = normalise(&cwd_abs(&joined));
        let rel = full
            .strip_prefix(&root)
            .map_err(|_| format!("{arg} is outside the clone at {}", root.display()))?;
        Ok(rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"))
    }
}

fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// What a remote URL says about the repository: its name under ADR-0057
/// d3's grammar, and whether the URL carries credentials.
#[derive(Debug, PartialEq, Eq)]
pub struct Remote {
    pub repo: String,
    pub had_credentials: bool,
}

/// The repository name of a remote URL: the scheme, any user and password,
/// a port and a trailing `.git` dropped, in lower case
/// (`git@github.com:e8kor/open-memory-lab.git` → `github.com/e8kor/open-memory-lab`).
///
/// # Errors
/// The URL yields no name under the grammar.
pub fn repo_of_remote(url: &str) -> Result<Remote, String> {
    let mut had_credentials = false;
    let rest = if let Some((scheme, after)) = url.split_once("://") {
        let (authority, path) = after.split_once('/').unwrap_or((after, ""));
        let host = match authority.rsplit_once('@') {
            Some((userinfo, host)) => {
                // A password, or any user on http(s), is a credential; ssh's
                // `git@` user is not.
                had_credentials =
                    userinfo.contains(':') || (scheme.starts_with("http") && !userinfo.is_empty());
                host
            }
            None => authority,
        };
        let host = host.split(':').next().unwrap_or(host);
        format!("{host}/{path}")
    } else if let Some((userhost, path)) = url.split_once(':') {
        // scp-like: [user@]host:path
        let host = userhost.rsplit('@').next().unwrap_or(userhost);
        format!("{host}/{}", path.trim_start_matches('/'))
    } else {
        return Err("the origin remote names no host; pass --repo".to_owned());
    };
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest).to_lowercase();
    // The remainder may carry a token (`?access_token=`), so it is never
    // echoed.
    check_repo(&rest).map_err(|_| {
        "cannot derive a repository name from origin; pass --repo <repository>".to_owned()
    })?;
    Ok(Remote {
        repo: rest,
        had_credentials,
    })
}

/// ADR-0057 d3's `repo` grammar.
///
/// # Errors
/// The name breaks it, named.
pub fn check_repo(repo: &str) -> Result<(), String> {
    let segments: Vec<&str> = repo.split('/').collect();
    let ok = !repo.is_empty()
        && repo.len() <= 200
        && segments.len() <= 8
        && segments.iter().all(|s| {
            !s.is_empty()
                && *s != "."
                && *s != ".."
                && s.chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                && s.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
                })
        });
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{repo:?} is invalid; pass --repo as 1-8 lower-case segments matching [a-z0-9][a-z0-9._-]*, separated by /"
        ))
    }
}

/// Whether any component of a path matches the secret patterns, which a
/// run refuses to read (ADR-0057 d6, ADR-0047 d3): the one list `oml-scan`
/// keeps and the server's anchor scan applies (ADR-0065 d4). A directory so
/// named hides every file under it.
#[must_use]
pub fn is_secret_shaped(path: &str) -> bool {
    oml_scan::secret_path(path)
}

/// Whether a repository-relative path matches an `--exclude` glob: `**`
/// matches any number of segments, `*` any run within one, `?` one
/// character.
#[must_use]
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn segments(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => segments(&p[1..], s) || (!s.is_empty() && segments(p, &s[1..])),
            (Some(pat), Some(seg)) => {
                one(pat.as_bytes(), seg.as_bytes()) && segments(&p[1..], &s[1..])
            }
            _ => false,
        }
    }
    fn one(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => one(&p[1..], s) || (!s.is_empty() && one(p, &s[1..])),
            (Some(b'?'), Some(_)) => one(&p[1..], &s[1..]),
            (Some(a), Some(b)) => a == b && one(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let p: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    segments(&p, &s)
}

/// A server's origin as consent records it: the scheme and host
/// lowercased, the host's trailing dot cut, and the port when it is not the
/// scheme's default; no user, path, query or fragment (ADR-0076 d9).
#[must_use]
pub fn origin_of(url: &str) -> String {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://").unwrap_or(("https", url));
    let scheme = scheme.to_lowercase();
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (authority, None),
    };
    let host = host.trim_end_matches('.').to_lowercase();
    let default = match scheme.as_str() {
        "https" => "443",
        "http" => "80",
        _ => "",
    };
    match port {
        Some(port) if port != default => format!("{scheme}://{host}:{port}"),
        _ => format!("{scheme}://{host}"),
    }
}

/// The git config entry holding, one value per server, the origins this
/// clone's operator consented to sync to (ADR-0076 d9, amending ADR-0057
/// d6). A value of `true`, which ADR-0057 d6 wrote and which names no
/// server, is not consent.
pub const ALLOW_PUBLIC_CONFIG: &str = "oml.allowPublicServer";

/// How a sync is allowed to use a server that says it is shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedServerConsent {
    /// The server does not say it is shared: its whoami answers `shared:
    /// false`, or, by the operator's choice of 2026-11-11, omits `shared`
    /// as a server older than ADR-0076 does.
    NotRequired,
    /// The source's clone recorded an earlier opt-in for this origin.
    Recorded,
    /// This invocation supplied the first explicit opt-in for this origin in
    /// the source's clone.
    FirstOptIn,
    /// The source is outside a clone, so this explicit opt-in cannot be reused.
    OneTime,
}

/// Authorize a sync to `url`, whose whoami said `shared`, from the source
/// clone's recorded consent for that origin or this invocation's explicit
/// flag. The caller must print the disclosure before passing a `FirstOptIn`
/// result to [`record_shared_server_consent`] (ADR-0076 d9).
///
/// # Errors
/// A shared server is not allowed without recorded or explicit consent.
pub fn authorize_shared_server(
    url: &str,
    shared: bool,
    source: &Path,
    explicit: bool,
) -> Result<SharedServerConsent, String> {
    if !shared {
        return Ok(SharedServerConsent::NotRequired);
    }
    let git = Git::at(source).ok();
    let origin = origin_of(url);
    if git
        .as_ref()
        .is_some_and(|git| git.config_all(ALLOW_PUBLIC_CONFIG).contains(&origin))
    {
        return Ok(SharedServerConsent::Recorded);
    }
    if !explicit {
        return Err(format!(
            "{url} declares itself shared: its admins can read submitted memories, and \
             superseding or retiring a memory does not erase submitted text or anchors; pass \
             --allow-public-server to opt in; consent is reused for this server when the source \
             is inside a git clone"
        ));
    }
    Ok(if git.is_some() {
        SharedServerConsent::FirstOptIn
    } else {
        SharedServerConsent::OneTime
    })
}

/// Record the disclosed first opt-in for `url`'s origin in the clone
/// containing the sync source, beside any other origin it holds.
///
/// # Errors
/// The source is outside a clone or its local git config cannot be written.
pub fn record_shared_server_consent(source: &Path, url: &str) -> Result<(), String> {
    let git = Git::at(source)
        .map_err(|e| format!("cannot record shared-server consent for this clone: {e}"))?;
    git.add_config(ALLOW_PUBLIC_CONFIG, &origin_of(url))
        .map_err(|e| format!("could not record {ALLOW_PUBLIC_CONFIG} for this clone: {e}"))
}
