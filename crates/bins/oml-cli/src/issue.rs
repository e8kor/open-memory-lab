//! The key tool's local half (ADR-0028; cli/reference.md `oml keys`): the
//! reader grant the issuing verbs send beside a writer grant, the shell
//! quoting of the two export lines `oml keys issue-agent` prints, the
//! `claude plugin configure` and `oml setup claude-code` lines it adds with
//! `--out`, the owner-only file
//! `--out` writes the key value to, and what `--connect-code` prints for its
//! answer (ADR-0031 d7). Everything here runs on
//! the operator's machine; the server takes exactly the grants it is sent
//! (ADR-0025 d2).

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The grants an issuing verb sends: each requested grant in order, and
/// beside every writer grant a reader grant on the same workarea, unless
/// `write_only` or the request already names that reader grant (ADR-0028
/// d2). A default of the CLI's alone; nothing here widens what the server
/// would accept. No issuing verb takes a per-grant deny mask yet, so neither
/// grant carries one; a verb that takes one copies the writer grant's masks
/// onto its reader grant (ADR-0028 d2; cli/reference.md `oml keys`).
///
/// Each workarea is the id or the name the operator typed, and the added
/// reader grant spells it the same way. The CLI resolves no name (ADR-0026
/// d4), so "already names that reader grant" compares spellings: a reader
/// grant given by a workarea's id does not count for a writer grant given
/// by its name.
#[must_use]
pub fn with_reader_beside_writers(
    grants: &[(String, String)],
    write_only: bool,
) -> Vec<(String, String)> {
    let mut sent: Vec<(String, String)> = Vec::with_capacity(grants.len() * 2);
    for (node, role) in grants {
        sent.push((node.clone(), role.clone()));
        // A writer grant on the node alone brings a reader grant on the node
        // alone (ADR-0044 d4).
        let (base, reader) = match role.split_once(':') {
            Some((base, covers)) => (base, format!("reader:{covers}")),
            None => (role.as_str(), "reader".to_owned()),
        };
        let reads = |(n, r): &(String, String)| n == node && *r == reader;
        if base == "writer" && !write_only && !grants.iter().chain(&sent).any(reads) {
            sent.push((node.clone(), reader.clone()));
        }
    }
    sent
}

/// POSIX single quotes: the text verbatim, each `'` written as `'\''`, so a
/// printed `export` line means exactly the text whatever it holds.
#[must_use]
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The line `oml keys issue-agent --out` prints beside its two export lines:
/// `jq` reads the key file and pipes `url` and `api_key` into `claude plugin
/// configure --values-stdin`, so the value reaches Claude Code's keychain
/// without entering argv or a transcript (ADR-0040 d4, amending ADR-0028
/// d1). `server` is the plugin's `url` option, without `/mcp`.
#[must_use]
pub fn configure_line(server: &str, key_file: &str) -> String {
    format!(
        "jq -n --arg u {} --rawfile k {} '{{url: $u, api_key: ($k | rtrimstr(\"\\n\"))}}' \
         | claude plugin configure oml@open-memory-lab --values-stdin",
        sh_quote(server),
        sh_quote(key_file),
    )
}

/// The line `oml keys issue-agent --out` prints after the configure line:
/// `oml setup claude-code` with the server as `--url` and the key file as
/// `--key-file`, which installs the plugin at the CLI's own release and
/// configures it from the same file, so the value never enters argv
/// (ADR-0041 d7). `server` is the plugin's `url` option, without `/mcp`.
#[must_use]
pub fn setup_line(server: &str, key_file: &str) -> String {
    format!(
        "oml setup claude-code --url {} --key-file {}",
        sh_quote(server),
        sh_quote(key_file),
    )
}

/// The file `oml keys issue-agent --out` writes the key value to: created
/// exclusively and owner-only before anything is sent, so a path that cannot
/// hold the value fails while no key exists yet, and no file is ever
/// overwritten (ADR-0028 d1; roles-and-keys.md "Key format").
#[derive(Debug)]
pub struct KeyFile {
    path: PathBuf,
    shown: String,
    file: File,
}

impl KeyFile {
    /// Creates `path` owner-only ([`crate::owner_only`]), refusing a path that exists, is not
    /// UTF-8, or holds a quote or a newline (the printed line reads the file
    /// back inside single quotes, so either would make it unsafe to run).
    ///
    /// # Errors
    ///
    /// The refusal, naming its reason, when the file is not created.
    pub fn create(path: &Path) -> Result<Self, String> {
        let path = std::path::absolute(path)
            .map_err(|e| format!("cannot resolve the --out path {}: {e}", path.display()))?;
        let shown = path
            .to_str()
            .ok_or_else(|| {
                format!(
                    "refused: the --out path {} is not UTF-8, so the line that reads it back \
                     cannot be printed; nothing was sent (ADR-0028 d1)",
                    path.display()
                )
            })?
            .to_owned();
        if shown.contains(['\'', '"', '\n', '\r']) {
            return Err(format!(
                "refused: the --out path {shown:?} holds a quote or a newline, so the line that \
                 reads it back would not be safe to run; nothing was sent (ADR-0028 d1)"
            ));
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        crate::owner_only::open_options(&mut options);
        let file = options.open(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                format!(
                    "refused: {shown} already exists, and --out never overwrites a file; \
                     nothing was sent (ADR-0028 d1)"
                )
            } else {
                format!("cannot create {shown}: {e}; nothing was sent")
            }
        })?;
        let key_file = Self { path, shown, file };
        // The umask can only narrow the creation mode, and on Windows the
        // file was created with the DACL it inherited: set it exactly.
        if let Err(e) = crate::owner_only::restrict(&key_file.file) {
            let shown = key_file.shown.clone();
            // The refusal is the restriction's failure; a leftover empty
            // file would only be reported, never retried.
            let _ = key_file.discard();
            return Err(format!(
                "cannot make {shown} owner-only: {e}; nothing was sent"
            ));
        }
        Ok(key_file)
    }

    /// The absolute path, as the printed line names it.
    #[must_use]
    pub fn shown(&self) -> &str {
        &self.shown
    }

    /// Writes the value and one newline and syncs it to disk. On failure the
    /// partial file is removed: the value is then lost, and the caller names
    /// the issued key so it can be revoked.
    ///
    /// # Errors
    ///
    /// The write or sync failure.
    pub fn write_value(mut self, value: &str) -> Result<(), String> {
        let written = self
            .file
            .write_all(format!("{value}\n").as_bytes())
            .and_then(|()| self.file.sync_all());
        if let Err(e) = written {
            drop(self.file);
            let _ = std::fs::remove_file(&self.path);
            return Err(e.to_string());
        }
        Ok(())
    }

    /// Removes the empty file after a refused issuance.
    ///
    /// # Errors
    ///
    /// The removal failure, for the caller to report beside the refusal.
    pub fn discard(self) -> Result<(), String> {
        drop(self.file);
        std::fs::remove_file(&self.path)
            .map_err(|e| format!("the empty key file {} was not removed: {e}", self.shown))
    }
}

/// What a connect-code verb prints and exits with: each stdout line, each
/// stderr line, and the exit code.
#[derive(Debug, PartialEq, Eq)]
pub struct Printed {
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub exit: u8,
}

/// What `oml keys issue-agent --connect-code` prints for its `201`
/// (cli/reference.md `--connect-code`; ADR-0031 d7). A fresh answer puts
/// exactly two lines on stdout, the connector URL the server names (never
/// the address the CLI used) and the code, and the key, the code's expiry and
/// the next step on stderr. An answer without a code, the stored body an
/// idempotent replay (`Idempotency-Replay: true`) returns among them, puts
/// nothing on stdout, names the key and how to mint a code, and exits 2.
/// With `json` the body goes to stdout as it came, and only the exit code and
/// the missing code's stderr line still differ.
#[must_use]
pub fn connect_code_printed(answer: &Value, replayed: bool, json: bool) -> Printed {
    let text = |v: &Value| v.as_str().unwrap_or("").to_owned();
    let key_name = text(&answer["name"]);
    let key_id = text(&answer["key_id"]);
    let connect_code = &answer["connect_code"];
    let code = connect_code["code"].as_str().filter(|_| !replayed);
    let mut stdout = Vec::new();
    if json {
        stdout.push(serde_json::to_string_pretty(answer).unwrap_or_default());
    }
    let Some(code) = code else {
        let why = if replayed {
            "this answer is an idempotent replay, and the connect code went only to the first \
             answer"
        } else {
            "the 201 carried no connect code"
        };
        return Printed {
            stdout,
            stderr: vec![format!(
                "connector key {key_name} ({key_id}) was issued, but {why}; \
                 `oml keys connect-code {key_name}` mints a new one (ADR-0031 d7)"
            )],
            exit: 2,
        };
    };
    if json {
        return Printed {
            stdout,
            stderr: Vec::new(),
            exit: 0,
        };
    }
    Printed {
        stdout: vec![text(&connect_code["connector_url"]), code.to_owned()],
        stderr: vec![
            format!(
                "connector key {key_name} ({key_id}) issued to {}: no value, and the connect \
                 code above in its place, single use, shown this once and expiring at {}",
                text(&answer["principal"]),
                text(&connect_code["expires_at"]),
            ),
            NEXT_STEP.to_owned(),
        ],
        exit: 0,
    }
}

/// The step after a connect code is printed (cli/reference.md
/// `--connect-code`).
const NEXT_STEP: &str = "next: add a custom connector with exactly the URL above at Customize > \
     Connectors on claude.ai, and enter the code on the page that opens before it expires; give \
     the code only to the person who will connect, for a page they reached by adding the \
     connector themselves";

/// What `oml keys connect-code <key>` prints for the `201` of
/// `POST /v2/keys/{key}/connect-codes` (cli/reference.md; ADR-0031 d7, d8):
/// as [`connect_code_printed`], the connector URL and the code on stdout and
/// the expiry and the next step on stderr, and for an answer without a code,
/// a replay's among them, nothing on stdout, the key named, and exit 2.
#[must_use]
pub fn minted_code_printed(key: &str, answer: &Value, replayed: bool, json: bool) -> Printed {
    let text = |v: &Value| v.as_str().unwrap_or("").to_owned();
    let code = answer["code"].as_str().filter(|_| !replayed);
    let mut stdout = Vec::new();
    if json {
        stdout.push(serde_json::to_string_pretty(answer).unwrap_or_default());
    }
    let Some(code) = code else {
        let why = if replayed {
            "this answer is an idempotent replay, and the connect code went only to the first \
             answer"
        } else {
            "the 201 carried no connect code"
        };
        return Printed {
            stdout,
            stderr: vec![format!(
                "a connect code for connector key {key} was minted, but {why}; \
                 `oml keys connect-code {key}` mints a new one (ADR-0031 d7)"
            )],
            exit: 2,
        };
    };
    if json {
        return Printed {
            stdout,
            stderr: Vec::new(),
            exit: 0,
        };
    }
    Printed {
        stdout: vec![text(&answer["connector_url"]), code.to_owned()],
        stderr: vec![
            format!(
                "connect code for connector key {key}: single use, shown this once, expiring at \
                 {}; the key's earlier codes no longer connect",
                text(&answer["expires_at"]),
            ),
            NEXT_STEP.to_owned(),
        ],
        exit: 0,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{Value, json};

    use super::{
        KeyFile, configure_line, connect_code_printed, minted_code_printed, setup_line,
        with_reader_beside_writers,
    };

    /// ADR-0044 d4 with ADR-0028 d2: a writer grant on a node alone brings a
    /// reader grant on that node alone, and a subtree writer grant a subtree
    /// reader grant.
    #[test]
    fn a_writer_node_grant_brings_a_reader_node_grant() {
        let sent = with_reader_beside_writers(
            &[
                ("universe".to_owned(), "writer:node".to_owned()),
                ("alice".to_owned(), "writer".to_owned()),
            ],
            false,
        );
        let pairs: Vec<(&str, &str)> = sent.iter().map(|(n, r)| (n.as_str(), r.as_str())).collect();
        assert_eq!(
            pairs,
            vec![
                ("universe", "writer:node"),
                ("universe", "reader:node"),
                ("alice", "writer"),
                ("alice", "reader"),
            ]
        );
    }

    /// A `201` of `POST /v2/keys` with `connect_code: true`, as openapi.yaml
    /// `IssuedKey` and `ConnectCode` shape it; a replay omits the code.
    fn issued(code: Option<&str>) -> Value {
        let mut connect_code = json!({
            "code_id": "cc_01J9ZK",
            "expires_at": "2026-11-04T10:10:00Z",
            "connector_url": "https://oml.example.com/mcp",
        });
        if let Some(code) = code {
            connect_code["code"] = json!(code);
        }
        json!({
            "key_id": "key_01J9ZK",
            "name": "claude-mobile",
            "role": "writer",
            "principal": "crew/claude-mobile",
            "default_workarea_id": "metals",
            "grants": [],
            "connect_code": connect_code,
        })
    }

    #[test]
    fn issue_agent_connect_code_prints_exactly_the_connector_url_and_the_code_on_stdout() {
        let printed = connect_code_printed(&issued(Some("7KQ2-M9XD-4TPA")), false, false);
        assert_eq!(
            printed.stdout,
            ["https://oml.example.com/mcp", "7KQ2-M9XD-4TPA"],
            "{printed:?}"
        );
        assert_eq!(printed.exit, 0, "{printed:?}");
        let stderr = printed.stderr.join("\n");
        for part in [
            "claude-mobile",
            "key_01J9ZK",
            "2026-11-04T10:10:00Z",
            "Customize > Connectors",
        ] {
            assert!(stderr.contains(part), "stderr names {part}: {stderr}");
        }
        assert!(
            !stderr.contains("7KQ2-M9XD-4TPA"),
            "the code is on stdout alone: {stderr}"
        );
    }

    #[test]
    fn a_replayed_issuance_prints_no_code_names_the_key_and_exits_2() {
        let printed = connect_code_printed(&issued(None), true, false);
        assert!(printed.stdout.is_empty(), "{printed:?}");
        assert_eq!(printed.exit, 2, "{printed:?}");
        let stderr = printed.stderr.join("\n");
        for part in [
            "claude-mobile",
            "key_01J9ZK",
            "first answer",
            "oml keys connect-code claude-mobile",
        ] {
            assert!(stderr.contains(part), "stderr names {part}: {stderr}");
        }
    }

    #[test]
    fn a_replayed_issuance_with_json_prints_the_body_as_it_came_and_exits_2() {
        let body = issued(None);
        let printed = connect_code_printed(&body, true, true);
        let shown: Value =
            serde_json::from_str(&printed.stdout.join("\n")).expect("one JSON document");
        assert_eq!(shown, body, "{printed:?}");
        assert_eq!(printed.exit, 2, "{printed:?}");
        assert!(
            printed.stderr.join("\n").contains("oml keys connect-code"),
            "{printed:?}"
        );
    }

    #[test]
    fn a_fresh_issuance_with_json_prints_the_201_alone_and_exits_0() {
        let body = issued(Some("7KQ2-M9XD-4TPA"));
        let printed = connect_code_printed(&body, false, true);
        let shown: Value =
            serde_json::from_str(&printed.stdout.join("\n")).expect("one JSON document");
        assert_eq!(shown, body, "{printed:?}");
        assert!(printed.stderr.is_empty(), "{printed:?}");
        assert_eq!(printed.exit, 0, "{printed:?}");
    }

    #[test]
    fn a_201_without_a_code_that_is_no_replay_prints_no_code_and_exits_2() {
        let printed = connect_code_printed(&issued(None), false, false);
        assert!(printed.stdout.is_empty(), "{printed:?}");
        assert_eq!(printed.exit, 2, "{printed:?}");
        assert!(
            printed
                .stderr
                .join("\n")
                .contains("oml keys connect-code claude-mobile"),
            "{printed:?}"
        );
    }

    /// An empty directory of this test's own, under the OS temp directory.
    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oml-cli-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_key_file_is_owner_only_before_its_value_is_written() {
        let dir = scratch("key-file-owner-only");
        let path = dir.join("crew-agent-7");
        let key_file = KeyFile::create(&path).expect("created");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path)
                .expect("exists")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "owner-only while still empty");
        }
        #[cfg(windows)]
        assert_eq!(
            crate::owner_only::dacl_of(&path).expect("its DACL reads back"),
            crate::owner_only::Dacl::owner_only(),
            "one protected entry, the current user's, while still empty"
        );
        key_file.write_value("oml_value").expect("written");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(text, "oml_value\n");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }

    #[test]
    fn a_key_file_never_overwrites_a_file_that_exists() {
        let dir = scratch("key-file-exists");
        let path = dir.join("taken");
        std::fs::write(&path, "kept\n").expect("an existing file");
        let refusal = KeyFile::create(&path).expect_err("refused");
        assert!(refusal.contains("already exists"), "{refusal}");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(text, "kept\n", "the existing file is untouched");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }

    #[cfg(windows)]
    #[test]
    fn no_other_open_of_a_key_file_succeeds_while_the_cli_holds_it() {
        let dir = scratch("key-file-unshared");
        let path = dir.join("crew-agent-7");
        let key_file = KeyFile::create(&path).expect("created");
        let refused = std::fs::File::open(&path).expect_err("a second open is refused");
        assert_eq!(
            refused.raw_os_error(),
            Some(
                i32::try_from(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION)
                    .expect("fits")
            ),
            "{refused}"
        );
        key_file.discard().expect("discarded");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }

    /// The configure line pipes the key file through `jq` into `claude
    /// plugin configure --values-stdin`, naming the server and the file,
    /// each single-quoted, so the value never enters argv (ADR-0040 d4).
    #[test]
    fn the_configure_line_reads_the_key_file_into_claude_plugin_configure() {
        assert_eq!(
            configure_line(
                "https://oml.example.com",
                "/home/op/.config/oml/keys/crew-agent-7"
            ),
            "jq -n --arg u 'https://oml.example.com' --rawfile k \
             '/home/op/.config/oml/keys/crew-agent-7' \
             '{url: $u, api_key: ($k | rtrimstr(\"\\n\"))}' \
             | claude plugin configure oml@open-memory-lab --values-stdin"
        );
    }

    /// The setup line runs `oml setup claude-code` with the server as `--url`
    /// and the key file as `--key-file`, each single-quoted, so the value
    /// never enters argv (ADR-0041 d7).
    #[test]
    fn the_setup_line_runs_setup_claude_code_with_the_url_and_the_key_file() {
        assert_eq!(
            setup_line(
                "https://oml.example.com",
                "/home/op/.config/oml/keys/crew-agent-7"
            ),
            "oml setup claude-code --url 'https://oml.example.com' --key-file \
             '/home/op/.config/oml/keys/crew-agent-7'"
        );
    }

    #[test]
    fn keys_connect_code_prints_exactly_the_connector_url_and_the_code_on_stdout() {
        let answer = json!({
            "code_id": "cc_01J9ZK",
            "code": "7KQ2-M9XD-4TPA",
            "expires_at": "2026-11-04T10:10:00Z",
            "connector_url": "https://oml.example.com/mcp",
        });
        let printed = minted_code_printed("claude-mobile", &answer, false, false);
        assert_eq!(
            printed.stdout,
            ["https://oml.example.com/mcp", "7KQ2-M9XD-4TPA"]
        );
        assert_eq!(printed.exit, 0);
        assert!(
            printed.stderr[0].contains("claude-mobile"),
            "{:?}",
            printed.stderr
        );
    }

    #[test]
    fn keys_connect_code_prints_no_code_and_exits_2_on_a_replayed_answer() {
        let answer = json!({
            "code_id": "cc_01J9ZK",
            "expires_at": "2026-11-04T10:10:00Z",
            "connector_url": "https://oml.example.com/mcp",
        });
        let printed = minted_code_printed("claude-mobile", &answer, true, false);
        assert!(printed.stdout.is_empty(), "{:?}", printed.stdout);
        assert_eq!(printed.exit, 2);
        assert!(
            printed.stderr[0].contains("oml keys connect-code claude-mobile"),
            "{:?}",
            printed.stderr
        );
    }
}
