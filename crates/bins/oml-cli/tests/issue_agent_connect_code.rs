//! `oml keys issue-agent --connect-code`'s local refusal, through the real
//! binary and with no server route behind it (cli/reference.md
//! `--connect-code`; ADR-0031 d7). The verb's answer to a `201` is proven
//! over canned responses in `oml_cli::issue`; its `/v2` wiring and its
//! end-to-end tests follow ADR-0039.

use std::io::ErrorKind;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;

/// An empty directory of this test's own, under the OS temp directory.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

#[test]
fn out_with_connect_code_is_refused_with_exit_2_before_anything_is_sent() {
    let dir = scratch("connect-code-out");
    let listener = TcpListener::bind("127.0.0.1:0").expect("a listener on an ephemeral port");
    listener.set_nonblocking(true).expect("non-blocking");
    let server = format!("http://{}", listener.local_addr().expect("its address"));
    let path = dir.join("crew-claude-mobile");

    let out = Command::new(env!("CARGO_BIN_EXE_oml"))
        .env("OML_CLI_CREDENTIALS", dir.join("credentials.toml"))
        .env_remove("OML_CLI_KEY")
        .env_remove("OML_CLI_SERVER")
        .args(["--server", &server, "--key", "oml_test_admin"])
        .args([
            "keys",
            "issue-agent",
            "claude-mobile",
            "--principal",
            "crew/claude-mobile",
            "--workarea",
            "metals",
            "--connect-code",
            "--out",
        ])
        .arg(&path)
        .output()
        .expect("the oml binary runs");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(2), "a local refusal: {stderr}");
    assert!(out.stdout.is_empty(), "nothing on stdout: {stderr}");
    assert!(
        stderr.contains("--out") && stderr.contains("--connect-code"),
        "the refusal names both flags: {stderr}"
    );
    assert!(!path.exists(), "no key file is created: {stderr}");
    match listener.accept() {
        Err(e) if e.kind() == ErrorKind::WouldBlock => {}
        Ok(_) => panic!("the CLI connected to the server: {stderr}"),
        Err(e) => panic!("the listener failed: {e}"),
    }
    std::fs::remove_dir_all(&dir).expect("cleaned up");
}
