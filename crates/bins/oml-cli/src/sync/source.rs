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
            .map_err(|e| format!("{} is not inside a git clone ({e}); oml sync reads committed files only (ADR-0057 d6)", dir.display()))?;
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
                "--rev {rev:?} names no commit in {} (ADR-0057 d6)",
                self.dir.display()
            )
        })
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
        return Err(format!("the remote {url:?} names no host; pass --repo"));
    };
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest).to_lowercase();
    check_repo(&rest)?;
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
            "{repo:?} is not a repository name: lower case, 1 to 8 segments of [a-z0-9][a-z0-9._-]* (ADR-0057 d3); pass --repo"
        ))
    }
}

/// Whether a path's file name matches the client's secret patterns, which
/// a run refuses to read (ADR-0057 d6, ADR-0047 d3): `.env*`, `*.pem`,
/// `*.key`, `id_*`, `*credentials*`, `*secret*`.
#[must_use]
pub fn is_secret_shaped(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    name.starts_with(".env")
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_")
        || name.contains("credentials")
        || name.contains("secret")
}

/// The maintainer's public server, which a run refuses unless
/// `--allow-public-server` (ADR-0057 d6, ADR-0043).
pub const PUBLIC_SERVER: &str = "https://memory.doublegate.io";

/// Whether `url` is the public server.
#[must_use]
pub fn is_public_server(url: &str) -> bool {
    url.trim_end_matches('/')
        .eq_ignore_ascii_case(PUBLIC_SERVER)
}

/// The git config entry recording that this clone's operator allowed the
/// public server (ADR-0057 d17).
pub const ALLOW_PUBLIC_CONFIG: &str = "oml.allowPublicServer";
