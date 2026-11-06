//! The source side of `oml sync` (ADR-0057 d3, d6, d14): a scratch git
//! clone, read at one commit through the object store.

use std::path::{Path, PathBuf};
use std::process::Command;

use oml_cli::sync::adapter::Kind;
use oml_cli::sync::run::{Extracted, Loaded, Options, extract, load};

fn extracted(loaded: &Loaded) -> Extracted {
    extract(loaded, Kind::default_for(&loaded.paths()))
}
use oml_cli::sync::source::{
    ALLOW_PUBLIC_CONFIG, PublicServerConsent, authorize_public_server, is_secret_shaped,
    repo_of_remote,
};

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
    let x = extracted(&head);
    assert_eq!(x.items.len(), 1);
    assert!(x.items[0].item.text.ends_with("Second commit."));
    assert_eq!(x.adapter, Kind::Headings);
    let old = load(
        &dir,
        &Options {
            rev: first.clone(),
            ..opts("docs")
        },
    )
    .unwrap();
    assert_eq!(old.commit, first);
    assert!(extracted(&old).items[0].item.text.ends_with("Committed."));
    let err = load(
        &dir,
        &Options {
            rev: "nope".into(),
            ..opts("docs")
        },
    )
    .unwrap_err();
    assert!(err.contains("is not a commit"), "{err}");
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
        loaded.warnings[0].contains("contains credentials"),
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
    assert!(named.contains("is invalid"), "{named}");
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
    assert_eq!(extracted(&loaded).items.len(), 1);
    assert_eq!(loaded.skipped[0].0, "docs/team-secrets.md");
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
    let x = extracted(&loaded);
    assert_eq!(x.adapter, Kind::Adr);
    let paths: std::collections::BTreeSet<&str> = x.items.iter().map(|i| i.path.as_str()).collect();
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

#[test]
fn a_secret_shaped_directory_or_name_is_never_read() {
    for name in [
        "docs/secrets/aws.md",
        "docs/.env.d/x.md",
        "docs/certs/site.p12",
        "docs/.npmrc",
        "docs/.netrc",
        "docs/.pgpass",
        "docs/keys/id_ed25519",
        "docs/store.jks",
    ] {
        assert!(is_secret_shaped(name), "{name}");
    }
    assert!(!is_secret_shaped("docs/adr/ADR-0001-the-constitution.md"));
    assert!(!is_secret_shaped("docs/guide/keyboard.md"));
}

#[test]
fn a_bad_remote_is_refused_without_echoing_it() {
    let err = repo_of_remote("https://github.com/x/y.git?access_token=abc").unwrap_err();
    assert!(!err.contains("abc"), "{err}");
    assert!(err.contains("--repo"), "{err}");
}

fn oml(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_oml"))
        .args(args)
        .current_dir(cwd)
        .env("OML_CLI_CREDENTIALS", cwd.join("no-credentials.toml"))
        .env_remove("OML_URL")
        .env_remove("OML_API_KEY")
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn sync_refuses_the_public_server_without_allow_public_server() {
    use oml_cli::sync::source::is_public_server;
    for url in [
        "https://memory.doublegate.io",
        "https://MEMORY.doublegate.io/",
        "https://memory.doublegate.io:443",
        "http://memory.doublegate.io",
        "https://memory.doublegate.io/mcp",
        "memory.doublegate.io",
    ] {
        assert!(is_public_server(url), "{url}");
    }
    assert!(!is_public_server(
        "https://memory.doublegate.io.example.com"
    ));
    assert!(!is_public_server("https://oml.example.com"));
    let dir = clone_with("public", "git@github.com:e8kor/docs.git");
    write(&dir, "docs/a.md", "# A\n\n## B\n\nC.\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "a"]);
    let (code, _, err) = oml(
        &dir,
        &[
            "--url",
            "https://Memory.Doublegate.io:443/",
            "--api-key",
            "k",
            "sync",
            "docs",
            "--workarea",
            "w",
        ],
    );
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("--allow-public-server")
            && err.contains("superseding or retiring a memory does not erase"),
        "{err}"
    );
    assert!(
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["config", "--local", "--get", "oml.allowPublicServer"])
            .output()
            .unwrap()
            .stdout
            .is_empty(),
        "nothing is recorded on a refusal"
    );
}

#[test]
fn public_server_consent_is_recorded_once_and_reused() {
    let dir = clone_with("public-consent", "git@github.com:e8kor/docs.git");
    let url = "https://memory.doublegate.io";
    let err = authorize_public_server(url, &dir, false).unwrap_err();
    assert!(err.contains("--allow-public-server"), "{err}");
    assert!(
        !err.contains("ADR-"),
        "CLI errors stay self-contained: {err}"
    );
    assert_eq!(
        authorize_public_server(url, &dir, true).unwrap(),
        PublicServerConsent::FirstOptIn
    );
    assert_eq!(
        git(&dir, &["config", "--local", "--get", ALLOW_PUBLIC_CONFIG]),
        "true"
    );
    assert_eq!(
        authorize_public_server(url, &dir, false).unwrap(),
        PublicServerConsent::Recorded,
        "recorded consent needs neither another flag nor another disclosure"
    );
    assert_eq!(
        authorize_public_server("https://oml.example.com", &dir, false).unwrap(),
        PublicServerConsent::NotRequired
    );
}

#[test]
fn retire_all_requires_pr() {
    let dir = scratch("retire-all");
    let (code, _, err) = oml(
        &dir,
        &[
            "--url",
            "http://127.0.0.1:9",
            "--api-key",
            "k",
            "sync",
            "docs",
            "--workarea",
            "w",
            "--retire-all",
        ],
    );
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--pr"), "{err}");
}

#[test]
fn oml_anchor_prints_the_anchor_of_a_committed_file_or_fragment() {
    let dir = clone_with("anchor", "https://user:tok@github.com/e8kor/Docs.git");
    let record = "# ADR-0001 — One\n\n**Status:** accepted\n\n## Context\n\nC.\n\n## Decision\n\n- **d1 — First.** Body.\n";
    write(&dir, "docs/adr/ADR-0001-one.md", record);
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "a"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    // The working tree differs; the anchor is of HEAD.
    write(&dir, "docs/adr/ADR-0001-one.md", "changed");
    let (code, out, err) = oml(&dir.join("docs"), &["anchor", "adr/ADR-0001-one.md#d1"]);
    assert_eq!(code, 0, "{err}");
    let a: serde_json::Value = serde_json::from_str(&out).unwrap();
    let x = oml_cli::sync::adr::AdrAdapter;
    let items =
        oml_cli::sync::adapter::Adapter::extract(&x, "docs/adr/ADR-0001-one.md", record.as_bytes())
            .unwrap()
            .items;
    assert_eq!(
        a,
        serde_json::json!({"repo": "github.com/e8kor/docs", "rev": head, "path": "docs/adr/ADR-0001-one.md",
            "fragment": "d1", "hash": items[1].hash, "hasher": "oml-md/1"})
    );
    let (code, out, _) = oml(&dir, &["anchor", "docs/adr/ADR-0001-one.md"]);
    assert_eq!(code, 0);
    let whole: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(whole.get("fragment").is_none());
    assert_eq!(whole["hash"], oml_cli::sync::hash::md_hash(record));
    let (code, out, _) = oml(&dir, &["anchor", "docs/adr/ADR-0001-one.md#L1-L3"]);
    assert_eq!(code, 0);
    let lines: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        lines["hash"],
        oml_cli::sync::hash::md_hash("# ADR-0001 — One\n\n**Status:** accepted")
    );
    let (code, _, err) = oml(&dir, &["anchor", "docs/adr/ADR-0001-one.md#d9"]);
    assert_eq!(code, 2);
    assert!(
        err.contains("d9") && err.contains("d1"),
        "names the fragments there are: {err}"
    );
}

#[test]
fn exclude_globs_match_paths_by_segment() {
    use oml_cli::sync::source::glob_match;
    assert!(glob_match("docs/adr/**", "docs/adr/ADR-0001-a.md"));
    assert!(glob_match("docs/adr/**", "docs/adr/x/y.md"));
    assert!(!glob_match("docs/adr/**", "docs/adrs/a.md"));
    assert!(glob_match("**/draft-*.md", "docs/a/draft-1.md"));
    assert!(glob_match("**/draft-*.md", "draft-1.md"));
    assert!(!glob_match("docs/*.md", "docs/a/b.md"));
    assert!(glob_match("docs/?.md", "docs/a.md"));
}

#[test]
fn a_changed_exclude_set_on_this_clone_is_refused_unless_explicit() {
    let dir = clone_with("exclude", "git@github.com:e8kor/docs.git");
    write(&dir, "docs/a.md", "# A\n\n## B\n\nC.\n");
    write(
        &dir,
        "docs/adr/ADR-0001-x.md",
        "# ADR-0001 — X\n\n## Decision\n\n- **d1 — X.** Y.\n",
    );
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "a"]);
    let o = Options {
        exclude: vec!["docs/adr/**".into()],
        ..opts("docs")
    };
    let loaded = load(&dir, &o).unwrap();
    assert_eq!(
        loaded.paths(),
        ["docs/a.md"],
        "an excluded file is never read"
    );
    oml_cli::sync::run::record_exclude(&loaded, &o).unwrap();
    // The same set again is fine; another set is refused on this clone.
    assert!(load(&dir, &o).is_ok());
    let err = load(&dir, &opts("docs")).unwrap_err();
    assert!(
        err.contains("--change-exclude") && err.contains("docs/adr/**"),
        "{err}"
    );
    assert!(
        load(
            &dir,
            &Options {
                change_exclude: true,
                ..opts("docs")
            }
        )
        .is_ok()
    );
}

#[test]
fn a_rust_source_loads_rs_files_and_manifests_only_when_the_rust_adapter_is_named() {
    let dir = clone_with("rust-load", "git@github.com:e8kor/Code.git");
    write(&dir, "crates/a/Cargo.toml", "[package]\nname = \"a\"\n");
    write(&dir, "crates/a/src/lib.rs", "//! A.\n");
    write(&dir, "crates/a/README.md", "# A\n\n## One\n\nText.\n");
    git(&dir, &["add", "-f", "."]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    // The working tree is not read: an uncommitted edit is not in the load.
    write(&dir, "crates/a/src/lib.rs", "//! Uncommitted.\n");
    let named = load(
        &dir,
        &Options {
            adapter: Some(Kind::Rust),
            ..opts("crates")
        },
    )
    .unwrap();
    assert_eq!(
        named.paths(),
        ["crates/a/Cargo.toml", "crates/a/src/lib.rs"]
    );
    assert_eq!(named.files[1].1, b"//! A.\n");
    let unnamed = load(&dir, &opts("crates")).unwrap();
    assert_eq!(unnamed.paths(), ["crates/a/README.md"]);
}
