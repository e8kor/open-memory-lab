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
    ALLOW_PUBLIC_CONFIG, SharedServerConsent, authorize_shared_server, is_secret_shaped, origin_of,
    record_shared_server_consent, repo_of_remote,
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

/// A server's origin is its scheme and host, lowercased, and a port only
/// when it is not the scheme's default; the path and user are not part of
/// it (ADR-0076 d9).
#[test]
fn a_servers_origin_ignores_case_path_user_and_the_default_port() {
    for url in [
        "https://oml.example.com",
        "https://OML.example.com/",
        "https://oml.example.com:443",
        "https://oml.example.com/mcp",
        "https://user@oml.example.com.",
    ] {
        assert_eq!(origin_of(url), "https://oml.example.com", "{url}");
    }
    assert_eq!(origin_of("http://127.0.0.1:8080/"), "http://127.0.0.1:8080");
    assert_ne!(
        origin_of("https://oml.example.com.evil.example"),
        "https://oml.example.com"
    );
}

#[test]
fn shared_server_consent_is_recorded_once_per_origin_and_reused() {
    let dir = clone_with("shared-consent", "git@github.com:e8kor/docs.git");
    let url = "https://oml.example.com";
    assert_eq!(
        authorize_shared_server(url, false, &dir, false).unwrap(),
        SharedServerConsent::NotRequired,
        "a server that is not shared asks nothing"
    );
    let err = authorize_shared_server(url, true, &dir, false).unwrap_err();
    assert!(err.contains("--allow-public-server"), "{err}");
    assert!(
        !err.contains("ADR-"),
        "CLI errors stay self-contained: {err}"
    );
    assert_eq!(
        authorize_shared_server(url, true, &dir, true).unwrap(),
        SharedServerConsent::FirstOptIn
    );
    assert!(
        !git(&dir, &["config", "--local", "--list"]).contains(ALLOW_PUBLIC_CONFIG),
        "authorization alone does not persist consent before the disclosure"
    );
    record_shared_server_consent(&dir, url).unwrap();
    assert_eq!(
        git(
            &dir,
            &["config", "--local", "--get-all", ALLOW_PUBLIC_CONFIG]
        ),
        "https://oml.example.com"
    );
    assert_eq!(
        authorize_shared_server(url, true, &dir, false).unwrap(),
        SharedServerConsent::Recorded,
        "recorded consent needs neither another flag nor another disclosure"
    );

    let outside = scratch("shared-consent-outside-a-clone");
    assert_eq!(
        authorize_shared_server(url, true, &outside, true).unwrap(),
        SharedServerConsent::OneTime,
        "an explicit opt-in outside a clone is honestly one-time"
    );
    assert!(authorize_shared_server(url, true, &outside, false).is_err());
}

#[test]
fn shared_server_consent_follows_the_source_clone_not_the_invocation_clone() {
    let invocation = clone_with("consent-invocation", "git@github.com:e8kor/invocation.git");
    let source = clone_with("consent-source", "git@github.com:e8kor/source.git");
    write(&source, "docs/a.md", "# A\n\n## B\n\nC.\n");
    git(&source, &["add", "."]);
    git(&source, &["commit", "-q", "-m", "a"]);
    let path = source.join("docs");
    let loaded = load(&invocation, &opts(path.to_str().unwrap())).unwrap();
    let root = loaded.root.as_deref().expect("the source clone's root");
    assert_eq!(root, source.canonicalize().unwrap());

    let url = "https://oml.example.com";
    assert_eq!(
        authorize_shared_server(url, true, root, true).unwrap(),
        SharedServerConsent::FirstOptIn
    );
    record_shared_server_consent(root, url).unwrap();
    assert_eq!(
        git(
            &source,
            &["config", "--local", "--get-all", ALLOW_PUBLIC_CONFIG]
        ),
        "https://oml.example.com"
    );
    assert!(
        !git(&invocation, &["config", "--local", "--list"]).contains(ALLOW_PUBLIC_CONFIG),
        "the invocation clone records nothing for another clone's source"
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

// ---------------------------------------------------------------------------
// Consent asked of a server that says it is shared (ADR-0076 d9)
// ---------------------------------------------------------------------------

/// A fixture answering `GET /v2/whoami` with `shared` as given (`None`:
/// the field absent, as a server older than ADR-0076 answers), every other
/// path 404; its URL. It serves until `runtime` shuts down.
fn whoami_fixture(runtime: &tokio::runtime::Runtime, shared: Option<bool>) -> String {
    whoami_fixture_counting(runtime, shared).0
}

/// [`whoami_fixture`], and the number of requests it has had for any path
/// but `/v2/whoami`: what a refusal must leave at zero.
fn whoami_fixture_counting(
    runtime: &tokio::runtime::Runtime,
    shared: Option<bool>,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use axum::routing::get;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let others = std::sync::Arc::new(AtomicUsize::new(0));
    let counted = others.clone();
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .expect("a listener on an ephemeral port");
    let server = format!("http://{}", listener.local_addr().expect("its address"));
    let mut me = serde_json::json!({
        "key_id": "key_w", "name": "writer", "role": "writer", "principal": "p",
        "held_by": "agent", "tenant": {"id": "t1", "name": "oml"},
        "default_workarea": null, "crew": null, "user": null, "grants": [],
    });
    if let Some(shared) = shared {
        me["shared"] = serde_json::Value::Bool(shared);
    }
    let router = axum::Router::new()
        .route(
            "/v2/whoami",
            get(move || {
                let me = me.clone();
                async move { axum::Json(me) }
            }),
        )
        .fallback(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            async { axum::http::StatusCode::NOT_FOUND }
        });
    runtime.spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    (server, others)
}

/// A clone with one committed document under `docs/`.
fn docs_clone(test: &str) -> PathBuf {
    let dir = clone_with(test, "git@github.com:e8kor/docs.git");
    write(&dir, "docs/a.md", "# A\n\n## B\n\nC.\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "a"]);
    dir
}

/// `oml sync docs` against `server`, with `--allow-public-server` when
/// `allow`.
fn sync_to(dir: &Path, server: &str, allow: bool) -> (i32, String, String) {
    let mut args = vec![
        "--url",
        server,
        "--api-key",
        "k",
        "sync",
        "docs",
        "--workarea",
        "w",
    ];
    if allow {
        args.push("--allow-public-server");
    }
    oml(dir, &args)
}

/// The values of the consent the source clone recorded, one origin each.
fn recorded(dir: &Path) -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["config", "--local", "--get-all", "oml.allowPublicServer"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

const DISCLOSURE: &str = "superseding or retiring a memory does not erase";

#[test]
fn sync_to_a_server_that_declares_itself_shared_is_refused_without_consent() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (server, others) = whoami_fixture_counting(&runtime, Some(true));
    let dir = docs_clone("shared-refused");
    let (code, _, err) = sync_to(&dir, &server, false);
    runtime.shutdown_background();
    assert_eq!(code, 2, "{err}");
    assert_eq!(
        others.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a refused sync sends the server nothing but the whoami it asked"
    );
    assert!(
        err.contains("--allow-public-server") && err.contains(DISCLOSURE),
        "{err}"
    );
    assert!(
        recorded(&dir).is_empty(),
        "nothing is recorded on a refusal"
    );
}

/// The operator's choice of 2026-11-11 (ADR-0076 d9): a server whose
/// whoami does not say, one older than ADR-0076, counts as not shared.
#[test]
fn sync_to_a_server_whose_whoami_omits_shared_asks_no_consent() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = whoami_fixture(&runtime, None);
    let dir = docs_clone("shared-omitted");
    let (_, _, err) = sync_to(&dir, &server, false);
    runtime.shutdown_background();
    assert!(!err.contains("--allow-public-server"), "{err}");
    assert!(!err.contains(DISCLOSURE), "{err}");
}

#[test]
fn sync_to_a_server_that_declares_itself_not_shared_asks_nothing() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = whoami_fixture(&runtime, Some(false));
    let dir = docs_clone("not-shared");
    let (_, _, err) = sync_to(&dir, &server, false);
    runtime.shutdown_background();
    assert!(!err.contains("--allow-public-server"), "{err}");
    assert!(!err.contains(DISCLOSURE), "{err}");
    assert!(recorded(&dir).is_empty());
}

#[test]
fn consent_recorded_for_one_origin_does_not_cover_another() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let first = whoami_fixture(&runtime, Some(true));
    let second = whoami_fixture(&runtime, Some(true));
    let dir = docs_clone("per-origin");
    let (_, _, err) = sync_to(&dir, &first, true);
    assert!(
        err.contains(DISCLOSURE),
        "the first opt-in discloses: {err}"
    );
    assert_eq!(
        recorded(&dir),
        std::slice::from_ref(&first),
        "the origin is recorded"
    );
    let (_, _, err) = sync_to(&dir, &first, false);
    assert!(
        !err.contains("--allow-public-server"),
        "recorded consent is reused for its origin: {err}"
    );
    let (code, _, err) = sync_to(&dir, &second, false);
    runtime.shutdown_background();
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--allow-public-server"), "{err}");
    assert_eq!(recorded(&dir), [first], "the other origin is not recorded");
}

#[test]
fn a_recorded_true_is_not_consent_and_doctor_names_it() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = whoami_fixture(&runtime, Some(true));
    let dir = docs_clone("legacy-true");
    git(
        &dir,
        &["config", "--local", "oml.allowPublicServer", "true"],
    );
    let (code, _, err) = sync_to(&dir, &server, false);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--allow-public-server"), "{err}");
    let (_, out, _) = oml(&dir, &["--url", &server, "--api-key", "k", "doctor"]);
    assert!(
        out.contains(
            "warn sync_consent_unbound: oml.allowPublicServer=true names no server and is no \
             longer read; run oml sync --allow-public-server once against the server you mean"
        ),
        "{out}"
    );
    // The remedy the line names clears it: the opt-in records the origin and
    // drops the legacy value, so the line is gone and the origin is consent.
    let _ = sync_to(&dir, &server, true);
    assert_eq!(
        recorded(&dir),
        std::slice::from_ref(&server),
        "the origin replaces the legacy true"
    );
    let (_, out, _) = oml(&dir, &["--url", &server, "--api-key", "k", "doctor"]);
    runtime.shutdown_background();
    assert!(!out.contains("sync_consent_unbound"), "{out}");
    assert!(!out.contains("sync_shared_server"), "{out}");
}

#[test]
fn an_opt_in_beside_a_recorded_origin_and_a_legacy_true_drops_the_true() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = whoami_fixture(&runtime, Some(true));
    let dir = docs_clone("legacy-true-and-origin");
    git(
        &dir,
        &["config", "--local", "oml.allowPublicServer", "true"],
    );
    git(
        &dir,
        &[
            "config",
            "--local",
            "--add",
            "oml.allowPublicServer",
            &server,
        ],
    );
    let _ = sync_to(&dir, &server, true);
    runtime.shutdown_background();
    assert_eq!(recorded(&dir), [server]);
}

/// A `git` ahead of the real one on PATH that refuses what git before 2.30
/// does not know, with git's own words and exit status, and otherwise runs
/// the real git: `config --fixed-value` and `rev-parse --end-of-options`
/// arrived in 2.30. Returns the directory to put first on PATH.
#[cfg(unix)]
fn git_before_2_30(test: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|d| d.join("git"))
        .find(|p| p.is_file())
        .expect("a git on PATH");
    let bin = scratch(&format!("{test}-bin"));
    let script = bin.join("git");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do\n  case \"$a\" in\n    --fixed-value|--end-of-options)\n      echo \"error: unknown option \\`${{a#--}}'\" >&2\n      exit 129;;\n  esac\ndone\nexec {} \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

/// `oml sync docs` with `--allow-public-server` against `server`, with `bin`
/// first on PATH.
#[cfg(unix)]
fn opt_in_with_path(dir: &Path, server: &str, bin: &Path) -> (i32, String, String) {
    let path = std::env::join_paths(
        std::iter::once(bin.to_path_buf())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_oml"))
        .args([
            "--url",
            server,
            "--api-key",
            "k",
            "sync",
            "docs",
            "--workarea",
            "w",
            "--allow-public-server",
        ])
        .current_dir(dir)
        .env("PATH", path)
        .env("OML_CLI_CREDENTIALS", dir.join("no-credentials.toml"))
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

/// The wrapper is the honest stand-in for git 2.25: it must refuse the two
/// options, or the tests below prove nothing.
#[cfg(unix)]
#[test]
fn the_stand_in_for_an_old_git_refuses_what_git_before_2_30_does_not_know() {
    let dir = docs_clone("old-git-stand-in");
    let bin = git_before_2_30("old-git-stand-in");
    for args in [
        [
            "config",
            "--local",
            "--fixed-value",
            "--unset-all",
            "a.b",
            "c",
        ],
        [
            "rev-parse",
            "--verify",
            "--end-of-options",
            "HEAD",
            "x",
            "y",
        ],
    ] {
        let out = Command::new(bin.join("git"))
            .arg("-C")
            .arg(&dir)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(129), "{args:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_first_opt_in_on_a_git_before_2_30_records_the_origin_and_drops_only_the_literal_true() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = whoami_fixture(&runtime, Some(true));
    let dir = docs_clone("old-git-opt-in");
    let bin = git_before_2_30("old-git-opt-in");
    let kept = [
        "https://true",
        "TRUE",
        "true ",
        "https://untrue.example",
        "truer",
    ];
    git(
        &dir,
        &["config", "--local", "oml.allowPublicServer", "true"],
    );
    for value in kept {
        git(
            &dir,
            &["config", "--local", "--add", "oml.allowPublicServer", value],
        );
    }
    let (_, _, err) = opt_in_with_path(&dir, &server, &bin);
    runtime.shutdown_background();
    assert!(!err.contains("could not drop"), "{err}");
    assert!(!err.contains("could not record"), "{err}");
    assert!(!err.contains("unknown option"), "{err}");
    assert!(!err.contains("is not a commit"), "{err}");
    let mut want: Vec<String> = kept.iter().map(|v| (*v).to_owned()).collect();
    want.push(server);
    assert_eq!(recorded(&dir), want);
}

#[cfg(unix)]
#[test]
fn a_first_opt_in_on_a_git_before_2_30_with_no_legacy_value_records_the_origin_without_error() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = whoami_fixture(&runtime, Some(true));
    let dir = docs_clone("old-git-no-legacy");
    let bin = git_before_2_30("old-git-no-legacy");
    let (_, _, err) = opt_in_with_path(&dir, &server, &bin);
    runtime.shutdown_background();
    assert!(!err.contains("could not drop"), "{err}");
    assert!(!err.contains("could not record"), "{err}");
    assert!(!err.contains("is not a commit"), "{err}");
    assert_eq!(recorded(&dir), [server]);
}

#[test]
fn only_the_literal_true_is_removed_and_a_clone_with_none_records_the_origin() {
    let url = "https://oml.example.com";
    let dir = clone_with("unset-literal-true", "git@github.com:e8kor/docs.git");
    record_shared_server_consent(&dir, url).unwrap();
    assert_eq!(recorded(&dir), [url], "no legacy value: no error");

    let kept = [
        "https://true",
        "TRUE",
        "true ",
        "https://untrue.example",
        "truer",
        "xtrue",
    ];
    let add = |value: &str| {
        git(
            &dir,
            &["config", "--local", "--add", "oml.allowPublicServer", value],
        );
    };
    add("true");
    for value in kept {
        add(value);
    }
    add("true");
    record_shared_server_consent(&dir, url).unwrap();
    let mut want = vec![url.to_owned()];
    want.extend(kept.iter().map(|v| (*v).to_owned()));
    assert_eq!(
        recorded(&dir),
        want,
        "every literal true goes, nothing else"
    );
}

#[test]
fn unsetting_a_value_matches_it_literally_and_not_as_a_pattern() {
    let dir = clone_with("unset-literal", "git@github.com:e8kor/docs.git");
    let source = oml_cli::sync::source::Git::at(&dir).unwrap();
    for value in ["a.b", "axb", "a.b.c", "(a|b)", "a"] {
        source.add_config("oml.sample", value).unwrap();
    }
    source.unset_value("oml.sample", "a.b").unwrap();
    source.unset_value("oml.sample", "(a|b)").unwrap();
    source.unset_value("oml.sample", "absent").unwrap();
    assert_eq!(source.config_all("oml.sample"), ["axb", "a.b.c", "a"]);
}

#[test]
fn a_rev_that_looks_like_an_option_is_refused_before_git_sees_it() {
    let dir = docs_clone("rev-option");
    let git = oml_cli::sync::source::Git::at(&dir).unwrap();
    assert!(git.commit("HEAD").is_ok());
    for rev in ["--abbrev-ref", "-n1", "--output=x"] {
        let err = git.commit(rev).unwrap_err();
        assert!(err.contains("is not a commit"), "{rev}: {err}");
    }
    assert!(!dir.join("x").exists());
}

#[test]
fn doctor_names_sync_shared_server_when_consent_for_that_origin_is_missing() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = whoami_fixture(&runtime, Some(true));
    let dir = docs_clone("doctor-shared");
    let (_, out, _) = oml(&dir, &["--url", &server, "--api-key", "k", "doctor"]);
    runtime.shutdown_background();
    assert!(
        out.contains(&format!(
            "warn sync_shared_server: {server} declares itself shared, and consent is not \
             recorded for this origin in this clone; run oml sync with --allow-public-server to \
             opt in"
        )),
        "{out}"
    );
}
