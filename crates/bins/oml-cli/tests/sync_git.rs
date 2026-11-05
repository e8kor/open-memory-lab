//! The source side of `oml sync` (ADR-0057 d3, d6, d14): a scratch git
//! clone, read at one commit through the object store.

use std::path::{Path, PathBuf};
use std::process::Command;

use oml_cli::sync::run::{Options, load};
use oml_cli::sync::source::{is_secret_shaped, repo_of_remote};

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-sync-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn write(dir: &Path, path: &str, text: &str) {
    let p = dir.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn clone_with(test: &str, origin: &str) -> PathBuf {
    let dir = scratch(test);
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["remote", "add", "origin", origin]);
    dir
}

fn opts(source: &str) -> Options {
    Options {
        source: source.into(),
        workarea: "design".into(),
        rev: "HEAD".into(),
        max_writes: 200,
        base: "main".into(),
        ..Options::default()
    }
}

#[test]
fn sync_reads_the_tree_at_rev_and_never_the_working_tree() {
    let dir = clone_with("tree", "git@github.com:e8kor/Docs.git");
    write(&dir, "docs/guide.md", "# G\n\n## One\n\nCommitted.\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    let first = git(&dir, &["rev-parse", "HEAD"]);
    write(&dir, "docs/guide.md", "# G\n\n## One\n\nSecond commit.\n");
    git(&dir, &["commit", "-q", "-am", "two"]);
    // Uncommitted and untracked work is never read.
    write(&dir, "docs/guide.md", "# G\n\n## One\n\nUncommitted.\n");
    write(&dir, "docs/untracked.md", "# U\n\n## Two\n\nUntracked.\n");
    let head = load(&dir.join("docs"), &opts(".")).unwrap();
    assert_eq!(head.repo, "github.com/e8kor/docs");
    assert_eq!(head.path, "docs");
    assert_eq!(head.items.len(), 1);
    assert!(head.items[0].item.text.ends_with("Second commit."));
    assert_eq!(head.adapter, "headings");
    let old = load(
        &dir,
        &Options {
            rev: first.clone(),
            ..opts("docs")
        },
    )
    .unwrap();
    assert_eq!(old.commit, first);
    assert!(old.items[0].item.text.ends_with("Committed."));
    let err = load(
        &dir,
        &Options {
            rev: "nope".into(),
            ..opts("docs")
        },
    )
    .unwrap_err();
    assert!(err.contains("names no commit"), "{err}");
    let outside = scratch("outside");
    assert!(
        load(&outside, &opts("."))
            .unwrap_err()
            .contains("not inside a git clone")
    );
}

#[test]
fn sync_derives_repo_from_the_remote_without_its_credentials() {
    for (url, repo, creds) in [
        (
            "git@github.com:e8kor/open-memory-lab.git",
            "github.com/e8kor/open-memory-lab",
            false,
        ),
        (
            "https://github.com/e8kor/open-memory-lab.git",
            "github.com/e8kor/open-memory-lab",
            false,
        ),
        (
            "https://user:tok@github.com/e8kor/open-memory-lab",
            "github.com/e8kor/open-memory-lab",
            true,
        ),
        (
            "https://ghp_x@github.com/E8kor/Open-Memory-Lab.git/",
            "github.com/e8kor/open-memory-lab",
            true,
        ),
        (
            "ssh://git@git.example.com:2222/team/docs.git",
            "git.example.com/team/docs",
            false,
        ),
    ] {
        let r = repo_of_remote(url).unwrap();
        assert_eq!((r.repo.as_str(), r.had_credentials), (repo, creds), "{url}");
    }
    let dir = clone_with("creds", "https://user:secret@github.com/e8kor/docs.git");
    write(&dir, "docs/a.md", "# A\n\n## B\n\nC.\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "a"]);
    let loaded = load(&dir, &opts("docs")).unwrap();
    assert_eq!(loaded.repo, "github.com/e8kor/docs");
    assert!(
        loaded.warnings[0].contains("carries credentials"),
        "{:?}",
        loaded.warnings
    );
    assert!(
        !format!("{loaded:?}").contains("secret@"),
        "no credential reaches the run"
    );
    let named = load(
        &dir,
        &Options {
            repo: Some("Bad Name".into()),
            ..opts("docs")
        },
    )
    .unwrap_err();
    assert!(named.contains("is not a repository name"), "{named}");
}

#[test]
fn a_secret_shaped_file_is_never_read() {
    for name in [
        "docs/.env.md",
        "docs/key.pem",
        "docs/id_rsa",
        "docs/aws-credentials.md",
        "docs/my-secret-notes.md",
    ] {
        assert!(is_secret_shaped(name), "{name}");
    }
    assert!(!is_secret_shaped("docs/ADR-0001-the-constitution.md"));
    let dir = clone_with("secret", "git@github.com:e8kor/docs.git");
    write(&dir, "docs/a.md", "# A\n\n## B\n\nC.\n");
    write(&dir, "docs/team-secrets.md", "# S\n\n## T\n\nhunter2\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "a"]);
    let loaded = load(&dir, &opts("docs")).unwrap();
    assert_eq!(loaded.items.len(), 1);
    assert_eq!(loaded.unreadable[0].0, "docs/team-secrets.md");
    assert!(!format!("{loaded:?}").contains("hunter2"));
}

#[test]
fn the_drafts_mode_reads_only_the_records_the_pull_request_adds_or_changes() {
    let dir = clone_with("drafts", "git@github.com:e8kor/docs.git");
    let record = |n: &str, d: &str| {
        format!(
            "# ADR-{n} — R\n\n**Status:** proposed\n\n## Context\n\nC.\n\n## Decision\n\n- **d1 — {d}.** Body.\n"
        )
    };
    write(&dir, "docs/adr/ADR-0001-a.md", &record("0001", "A"));
    write(&dir, "docs/adr/ADR-0002-b.md", &record("0002", "B"));
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    git(&dir, &["checkout", "-q", "-b", "design/x"]);
    write(&dir, "docs/adr/ADR-0002-b.md", &record("0002", "B changed"));
    write(&dir, "docs/adr/ADR-0003-c.md", &record("0003", "C"));
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "draft"]);
    git(&dir, &["checkout", "-q", "main"]);
    let o = Options {
        pr: Some(59),
        rev: "design/x".into(),
        ..opts("docs/adr")
    };
    assert_eq!(o.target_workarea(), "design-pr-59");
    let loaded = load(&dir, &o).unwrap();
    assert_eq!(loaded.adapter, "adr");
    let paths: std::collections::BTreeSet<&str> =
        loaded.items.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(
        paths.into_iter().collect::<Vec<_>>(),
        ["docs/adr/ADR-0002-b.md", "docs/adr/ADR-0003-c.md"]
    );
    assert!(loaded.scope.contains("docs/adr/ADR-0003-c.md"));
    assert!(
        !loaded.scope.contains("docs/adr/ADR-0001-a.md"),
        "a draft never retires what it did not load"
    );
}
