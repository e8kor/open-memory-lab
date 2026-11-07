//! Implicit capture's git layer through the real binary (ADR-0064 d1, d6 to
//! d10): `oml setup repo` installs hooks that never fail a git operation,
//! queue what a commit changed within a latency budget, and uninstall to
//! the bytes they found.
#![cfg(unix)]

use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use oml_cli::capture::job::{QueueEntry, sync_args};
use oml_cli::capture::{Mode, workarea_name};
use serde_json::Value;

const OML: &str = env!("CARGO_BIN_EXE_oml");

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cap-git-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_out(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn git_out(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
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
        .unwrap()
}

fn write(dir: &Path, path: &str, text: &str) {
    let p = dir.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A clone of `acme/widgets` with one commit holding a decision record and
/// a document.
fn clone(test: &str) -> PathBuf {
    let root = scratch(test);
    let dir = root.join("clone");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(
        &dir,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
    );
    write(
        &dir,
        "docs/adr/ADR-0001-first.md",
        "# ADR-0001 First\n\nbody\n",
    );
    write(&dir, "docs/a.md", "# A\n\ntext\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    dir
}

fn root_of(dir: &Path) -> PathBuf {
    dir.parent().unwrap().to_path_buf()
}

/// `oml` with a scratch home and no inherited OML variables.
fn oml(dir: &Path, args: &[&str]) -> Output {
    oml_with(dir, args, &[])
}

fn oml_with(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let home = root_of(dir).join("home");
    let mut cmd = Command::new(OML);
    cmd.current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"));
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn setup(dir: &Path, mode: Option<&str>, server: &str) -> Output {
    let key = root_of(dir).join("sync.key");
    std::fs::write(&key, "oml_sync_fixture_key\n").unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut args = vec![
        "setup",
        "repo",
        "--local",
        "--user",
        "pat",
        "--sync-key-file",
        key.to_str().unwrap(),
        "--url",
        server,
    ];
    if let Some(mode) = mode {
        args.extend(["--mode", mode]);
    }
    oml(dir, &args)
}

fn commit_change(dir: &Path, path: &str, text: &str) {
    write(dir, path, text);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "change"]);
}

/// A directory first on `PATH` holding `oml` as a symlink to the binary.
fn path_with_oml(dir: &Path) -> String {
    let bin = root_of(dir).join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let link = bin.join("oml");
    if !link.exists() {
        std::os::unix::fs::symlink(OML, &link).unwrap();
    }
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn git_env_commit(dir: &Path, path_env: &str, text: &str) -> Output {
    write(dir, "docs/a.md", text);
    git(dir, &["add", "."]);
    let home = root_of(dir).join("home");
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "edit",
        ])
        .env("PATH", path_env)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_STATE_HOME", home.join("state"))
        .output()
        .unwrap()
}

fn queue(dir: &Path) -> Value {
    let text =
        std::fs::read_to_string(dir.join(".git/oml/queue.json")).unwrap_or_else(|_| "{}".into());
    serde_json::from_str(&text).unwrap()
}

const ALL_HOOKS: [&str; 5] = [
    "post-commit",
    "post-checkout",
    "post-merge",
    "post-rewrite",
    "pre-push",
];

#[test]
fn setup_with_no_mode_takes_both_and_prints_why() {
    let dir = clone("default-both");
    let out = setup(&dir, None, "http://127.0.0.1:1");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(git(&dir, &["config", "--local", "oml.mode"]), "both");
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("both"), "{said}");
    assert!(
        said.contains("every commit"),
        "the reason is printed: {said}"
    );
    assert_eq!(
        git(&dir, &["config", "--local", "oml.workarea"]),
        format!("pat.{}", workarea_name("github.com/acme/widgets"))
    );
    for hook in ALL_HOOKS {
        let text = std::fs::read_to_string(dir.join(".git/hooks").join(hook)).unwrap();
        assert!(text.contains("# oml-managed"), "{hook}: {text}");
        assert!(
            text.contains(&format!("oml hook git {hook}")),
            "{hook}: {text}"
        );
    }
}

#[test]
fn the_workarea_name_is_a_function_of_the_repository() {
    let a = workarea_name("github.com/acme/widgets");
    assert_eq!(a, workarea_name("github.com/acme/widgets"));
    assert!(a.starts_with("widgets-"), "{a}");
    assert_eq!(a.len(), "widgets-".len() + 6, "{a}");
    assert_ne!(
        a,
        workarea_name("github.com/other/widgets"),
        "same last segment, another repository"
    );
    assert!(oml_cli_name_is_label(&a), "{a}");
}

fn oml_cli_name_is_label(name: &str) -> bool {
    name.len() <= 63
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && name.as_bytes()[0].is_ascii_lowercase()
}

#[test]
fn a_clone_without_oml_mode_maps_nothing() {
    let dir = clone("unmapped");
    commit_change(&dir, "docs/a.md", "# A\n\nnew\n");
    let out = oml(&dir, &["hook", "git", "post-commit"]);
    assert!(out.status.success());
    assert!(
        !dir.join(".git/oml").exists(),
        "nothing was written for an unmapped clone"
    );
    assert!(out.stdout.is_empty());
}

#[test]
fn install_never_overwrites_an_existing_hook() {
    let dir = clone("no-overwrite");
    let hook = dir.join(".git/hooks/post-commit");
    let original = "#!/bin/sh\necho mine\nexit 3\n";
    std::fs::write(&hook, original).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    let after = std::fs::read_to_string(&hook).unwrap();
    assert!(
        after.contains("echo mine\nexit 3"),
        "the original stays: {after}"
    );
    assert_eq!(after.matches("# oml-managed").count(), 1, "{after}");
    assert!(after.starts_with("#!/bin/sh\n"), "shebang first: {after}");
    let backups: Vec<_> = std::fs::read_dir(dir.join(".git/oml/hooks-backup"))
        .unwrap()
        .collect();
    assert_eq!(backups.len(), 1, "a copy of the hook it changed");
}

#[test]
fn uninstall_restores_prior_hooks_and_off_removes_the_keys() {
    let dir = clone("uninstall");
    let hook = dir.join(".git/hooks/post-commit");
    let original = "#!/bin/sh\necho mine\n";
    std::fs::write(&hook, original).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        setup(&dir, Some("both"), "http://127.0.0.1:1")
            .status
            .success()
    );
    assert!(dir.join(".git/hooks/pre-push").exists());
    let off = oml(&dir, &["setup", "repo", "--local", "--mode", "off"]);
    assert!(
        off.status.success(),
        "{}",
        String::from_utf8_lossy(&off.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&hook).unwrap(),
        original,
        "byte for byte"
    );
    for hook in ["pre-push", "post-checkout", "post-merge", "post-rewrite"] {
        assert!(
            !dir.join(".git/hooks").join(hook).exists(),
            "{hook} was ours alone"
        );
    }
    assert!(
        !git_out(&dir, &["config", "--local", "--get", "oml.mode"])
            .status
            .success()
    );
    assert!(
        !git_out(&dir, &["config", "--local", "--get", "oml.workarea"])
            .status
            .success()
    );
    assert!(
        !dir.join(".git/oml/queue.json").exists(),
        "off removes the queue"
    );
}

#[test]
fn uninstall_leaves_a_file_edited_after_install_and_says_so() {
    let dir = clone("edited");
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    let hook = dir.join(".git/hooks/pre-push");
    let mut text = std::fs::read_to_string(&hook).unwrap();
    text.push_str("echo added later\n");
    std::fs::write(&hook, &text).unwrap();
    let off = oml(&dir, &["setup", "repo", "--local", "--mode", "agent"]);
    assert!(off.status.success());
    assert_eq!(
        std::fs::read_to_string(&hook).unwrap(),
        text,
        "an edited file stays"
    );
    assert!(
        String::from_utf8_lossy(&off.stdout).contains("pre-push"),
        "{}",
        String::from_utf8_lossy(&off.stdout)
    );
    assert!(
        !dir.join(".git/hooks/post-commit").exists(),
        "an unchanged one goes"
    );
    assert_eq!(git(&dir, &["config", "--local", "oml.mode"]), "agent");
}

#[test]
fn a_committed_hook_config_is_never_written() {
    let dir = clone("committed");
    write(&dir, ".husky/pre-commit", "npm test\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "husky"]);
    git(&dir, &["config", "core.hooksPath", ".husky"]);
    let before = std::fs::read_dir(dir.join(".husky")).unwrap().count();
    let out = setup(&dir, Some("git"), "http://127.0.0.1:1");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_dir(dir.join(".husky")).unwrap().count(),
        before
    );
    assert!(!dir.join(".git/hooks/post-commit").exists());
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        said.contains("oml hook git post-commit"),
        "the line is printed: {said}"
    );
    assert!(
        said.contains("enrols"),
        "and why nothing was written: {said}"
    );
    assert_eq!(
        git(&dir, &["config", "--local", "oml.hooksPending"]),
        "true"
    );
}

#[test]
fn lefthook_commands_go_into_the_local_file_beside_the_committed_one() {
    let dir = clone("lefthook");
    write(&dir, "lefthook.yml", "pre-commit:\n  commands: {}\n");
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "lefthook"]);
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    let local = std::fs::read_to_string(dir.join("lefthook-local.yml")).unwrap();
    assert!(local.contains("oml hook git post-commit"), "{local}");
    assert_eq!(
        std::fs::read_to_string(dir.join("lefthook.yml")).unwrap(),
        "pre-commit:\n  commands: {}\n"
    );
    assert!(
        git(&dir, &["status", "--porcelain"]).is_empty(),
        "the local file is not a change to commit"
    );
    assert!(
        oml(&dir, &["setup", "repo", "--local", "--mode", "off"])
            .status
            .success()
    );
    assert!(!dir.join("lefthook-local.yml").exists());
}

#[test]
fn every_git_hook_exits_zero_when_oml_is_missing_or_crashes() {
    let dir = clone("never-fails");
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    // oml is not on PATH at all.
    let bare = "/usr/bin:/bin".to_owned();
    let out = git_env_commit(&dir, &bare, "# A\n\none\n");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // oml is on PATH and dies of a signal.
    let fake = root_of(&dir).join("crashbin");
    std::fs::create_dir_all(&fake).unwrap();
    std::fs::write(fake.join("oml"), "#!/bin/sh\nkill -SEGV $$\n").unwrap();
    std::fs::set_permissions(fake.join("oml"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let crashing = format!("{}:/usr/bin:/bin", fake.display());
    let out = git_env_commit(&dir, &crashing, "# A\n\ntwo\n");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // And one that exits non-zero on its own.
    std::fs::write(fake.join("oml"), "#!/bin/sh\nexit 77\n").unwrap();
    let out = git_env_commit(&dir, &crashing, "# A\n\nthree\n");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_commit_finishes_and_queues_when_the_server_is_down_or_never_answers() {
    // Down: nothing listens on port 1.
    let dir = clone("server-down");
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    let path = path_with_oml(&dir);
    let started = Instant::now();
    let out = git_env_commit(&dir, &path, "# A\n\ndown\n");
    assert!(out.status.success());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    let head = git(&dir, &["rev-parse", "HEAD"]);
    assert_eq!(
        queue(&dir)["entries"]["docs:headings"]["commit"],
        head.as_str(),
        "{}",
        queue(&dir)
    );
    // Offline, a flush leaves the entry and exits 0.
    let flush = oml(&dir, &["sync", "--flush"]);
    assert!(
        flush.status.success(),
        "{}",
        String::from_utf8_lossy(&flush.stderr)
    );
    assert_eq!(
        queue(&dir)["entries"]["docs:headings"]["commit"],
        head.as_str()
    );

    // Never answers: a listener that accepts and holds the connection.
    let dir = clone("server-hangs");
    let hang = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", hang.local_addr().unwrap());
    let held = std::thread::spawn(move || {
        let mut conns = Vec::new();
        while let Ok((c, _)) = hang.accept() {
            conns.push(c);
        }
    });
    assert!(setup(&dir, Some("git"), &url).status.success());
    let path = path_with_oml(&dir);
    let started = Instant::now();
    let out = git_env_commit(&dir, &path, "# A\n\nhangs\n");
    assert!(out.status.success());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the foreground waits for no network: {:?}",
        started.elapsed()
    );
    assert!(queue(&dir)["entries"]["docs:headings"]["commit"].is_string());
    drop(held);
}

#[test]
fn the_foreground_of_every_git_hook_is_under_100_ms() {
    let dir = clone("latency");
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    commit_change(&dir, "docs/a.md", "# A\n\nfast\n");
    let head = git(&dir, &["rev-parse", "HEAD"]);
    let invocations: [(&str, Vec<&str>, Option<String>); 5] = [
        ("post-commit", vec![], None),
        ("post-checkout", vec![&head, &head, "1"], None),
        ("post-merge", vec!["0"], None),
        (
            "post-rewrite",
            vec!["amend"],
            Some(format!("{head} {head}\n")),
        ),
        (
            "pre-push",
            vec!["origin", "https://github.com/acme/widgets.git"],
            Some(format!(
                "refs/heads/main {head} refs/heads/main {}\n",
                "0".repeat(40)
            )),
        ),
    ];
    for (hook, extra, stdin) in invocations {
        let mut samples = Vec::new();
        for _ in 0..7 {
            let started = Instant::now();
            let mut cmd = Command::new(OML);
            cmd.current_dir(&dir)
                .args(["hook", "git", hook])
                .args(&extra)
                .env_clear()
                .env("PATH", std::env::var("PATH").unwrap_or_default())
                .env("HOME", root_of(&dir).join("home"))
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = cmd.spawn().unwrap();
            if let Some(text) = &stdin {
                use std::io::Write as _;
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(text.as_bytes())
                    .unwrap();
            }
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "{hook}");
            samples.push(started.elapsed());
        }
        samples.sort();
        let median = samples[samples.len() / 2];
        assert!(
            median < Duration::from_millis(100),
            "{hook}: median {median:?} of {samples:?}"
        );
    }
}

#[test]
fn post_commit_queues_the_changed_paths_of_the_sources_they_belong_to() {
    let dir = clone("queues");
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    commit_change(&dir, "docs/a.md", "# A\n\nnew\n");
    assert!(oml(&dir, &["hook", "git", "post-commit"]).status.success());
    let q = queue(&dir);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    assert_eq!(
        q["entries"]["docs:headings"]["commit"],
        head.as_str(),
        "{q}"
    );
    assert_eq!(q["entries"]["docs:headings"]["paths"][0], "docs/a.md");
    assert!(
        q["entries"].get("docs/adr:adr").is_none(),
        "the decision records did not change: {q}"
    );
}

#[test]
fn a_queue_entry_is_replaced_by_a_newer_commit_and_keeps_the_older_paths() {
    let dir = clone("replaced");
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    commit_change(&dir, "docs/a.md", "# A\n\none\n");
    assert!(oml(&dir, &["hook", "git", "post-commit"]).status.success());
    commit_change(&dir, "docs/b.md", "# B\n\ntwo\n");
    assert!(oml(&dir, &["hook", "git", "post-commit"]).status.success());
    let q = queue(&dir);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    let e = &q["entries"]["docs:headings"];
    assert_eq!(
        e["commit"],
        head.as_str(),
        "the newest commit subsumes the older"
    );
    let paths: Vec<&str> = e["paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        paths.contains(&"docs/a.md") && paths.contains(&"docs/b.md"),
        "{paths:?}"
    );
}

#[test]
fn post_checkout_records_the_branch_and_queues_nothing() {
    let dir = clone("checkout");
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    git(&dir, &["checkout", "-q", "-b", "topic"]);
    let head = git(&dir, &["rev-parse", "HEAD"]);
    assert!(
        oml(&dir, &["hook", "git", "post-checkout", &head, &head, "1"])
            .status
            .success()
    );
    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(".git/oml/state.json")).unwrap())
            .unwrap();
    assert_eq!(state["branch"], "topic");
    assert_eq!(state["sync_branch"], false);
    assert!(
        queue(&dir)["entries"]
            .as_object()
            .is_none_or(|e| e.is_empty())
    );
    // A file checkout (third argument 0) records nothing.
    assert!(
        oml(&dir, &["hook", "git", "post-checkout", &head, &head, "0"])
            .status
            .success()
    );
}

#[test]
fn post_commit_off_the_sync_branch_queues_nothing() {
    let dir = clone("off-branch");
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    git(&dir, &["config", "--local", "oml.syncBranch", "main"]);
    git(&dir, &["checkout", "-q", "-b", "topic"]);
    commit_change(&dir, "docs/a.md", "# A\n\ntopic\n");
    assert!(oml(&dir, &["hook", "git", "post-commit"]).status.success());
    assert!(
        queue(&dir)["entries"]
            .as_object()
            .is_none_or(|e| e.is_empty()),
        "{}",
        queue(&dir)
    );
}

#[test]
fn a_hook_never_asks_for_a_rollback_and_a_rewrite_is_fast_forward_only() {
    let entry = QueueEntry {
        source: "docs".into(),
        adapter: "headings".into(),
        commit: "a".repeat(40),
        paths: vec!["docs/a.md".into()],
        reason: "post-rewrite".into(),
        rewritten: true,
        exclude: vec!["docs/adr/**".into()],
    };
    let args = sync_args(&entry, "pat.widgets-3fa2c1");
    assert!(args.contains(&"--ff-only".to_owned()), "{args:?}");
    assert!(!args.iter().any(|a| a == "--allow-rollback"), "{args:?}");
    assert!(
        args.windows(2).any(|w| w == ["--paths", "docs/a.md"]),
        "{args:?}"
    );
    assert!(
        args.windows(2).any(|w| w == ["--rev", &"a".repeat(40)]),
        "{args:?}"
    );
    assert!(
        args.windows(2).any(|w| w == ["--exclude", "docs/adr/**"]),
        "{args:?}"
    );
    assert!(
        args.windows(2)
            .any(|w| w == ["--workarea", "pat.widgets-3fa2c1"]),
        "{args:?}"
    );
}

#[test]
fn pre_push_reads_the_cached_check_and_never_blocks() {
    let dir = clone("pre-push");
    assert!(
        setup(&dir, Some("git"), "http://127.0.0.1:1")
            .status
            .success()
    );
    commit_change(&dir, "docs/a.md", "# A\n\nstale?\n");
    let checked = git(&dir, &["rev-parse", "HEAD"]);
    std::fs::create_dir_all(dir.join(".git/oml/check")).unwrap();
    std::fs::write(
        dir.join(format!(".git/oml/check/{checked}.json")),
        r#"{"stale":2,"removed":1,"untracked":0,"claims":[{"state":"stale","path":"docs/a.md","fragment":"a"}]}"#,
    )
    .unwrap();
    commit_change(&dir, "docs/b.md", "# B\n\nunchecked\n");
    let unchecked = git(&dir, &["rev-parse", "HEAD"]);
    let zero = "0".repeat(40);
    let run = |stdin: String| {
        let mut child = Command::new(OML)
            .current_dir(&dir)
            .args([
                "hook",
                "git",
                "pre-push",
                "origin",
                "https://github.com/acme/widgets.git",
            ])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", root_of(&dir).join("home"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write as _;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let out = run(format!(
        "refs/heads/main {unchecked} refs/heads/main {zero}\n"
    ));
    assert!(out.status.success());
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(said.contains("not checked yet"), "{said}");
    let out = run(format!(
        "refs/heads/main {checked} refs/heads/main {zero}\n"
    ));
    assert!(out.status.success());
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        said.contains("2 stale") && said.contains("docs/a.md"),
        "{said}"
    );
}

#[test]
fn the_mode_names_what_runs() {
    assert!(Mode::Both.includes_git() && Mode::Both.includes_agent());
    assert!(Mode::Git.includes_git() && !Mode::Git.includes_agent());
    assert!(!Mode::Agent.includes_git() && Mode::Agent.includes_agent());
    assert!(!Mode::Off.includes_git() && !Mode::Off.includes_agent());
    assert_eq!(Mode::parse("both"), Some(Mode::Both));
    assert_eq!(Mode::parse("nope"), None);
}
