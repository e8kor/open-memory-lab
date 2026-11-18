//! `oml doctor`'s skill lines (ADR-0080 d15, Consequences): whether the
//! server serves `GET /v1/skills`, whether the key's scope offers more than
//! one page, which offered skill shadows a farther one, whether the `oml`
//! the plugin's hooks run asks before a local skill file is written, and how
//! many skills are kept on disk instead of in OML. Each function is pure
//! over what `main` read, so the lines are proven without a server.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::client::CliError;

/// The server lines from `GET /v1/skills`'s first page under the key, or
/// its refusal: a server without the route predates skills (a warning); a
/// second page and each shadowing skill are informational.
#[must_use]
pub fn server_lines(answer: &Result<Value, CliError>) -> Vec<String> {
    let page = match answer {
        Ok(page) => page,
        Err(CliError::Refused {
            status: 404 | 405, ..
        }) => {
            return vec![
                "warn skills: this server predates skills (it does not serve GET /v1/skills); upgrade the server".to_owned(),
            ];
        }
        Err(CliError::Local(m)) if m.contains("404") => {
            return vec![
                "warn skills: this server predates skills (it does not serve GET /v1/skills); upgrade the server".to_owned(),
            ];
        }
        Err(_) => return Vec::new(),
    };
    let mut lines = Vec::new();
    if page["next_cursor"].is_string() {
        lines.push(format!(
            "info skills: the scope {} offers more than one page of skills (50); the session index prints 20",
            printable(page["scope_path"].as_str().unwrap_or("?"))
        ));
    }
    for s in page["skills"].as_array().map_or(&[][..], Vec::as_slice) {
        let overrides: Vec<&str> = s["overrides"]
            .as_array()
            .map(|o| o.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !overrides.is_empty() {
            let overrides: Vec<String> = overrides.into_iter().map(printable).collect();
            lines.push(format!(
                "info skills: {} from {} shadows the skill of that name at {}",
                printable(s["name"].as_str().unwrap_or("?")),
                printable(s["workspace_path"].as_str().unwrap_or("?")),
                overrides.join(", ")
            ));
        }
    }
    lines
}

/// Whether `c` is printed escaped: a C0 or C1 control, a bidi override or
/// isolate, or a zero-width character, the set a skill's name and
/// description refuse at write (ADR-0080 d1). It restates
/// `oml_domain::claim_kinds::hidden_character`, since the CLI links no
/// domain crate.
fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{200B}'..='\u{200D}'
                | '\u{FEFF}'
                | '\u{2060}'
        )
}

/// `text` with every character `hidden` names written as `\u{XXXX}`, so a
/// field a server sent cannot move the cursor, reorder a line or hide text
/// on the terminal.
#[must_use]
pub fn printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if hidden(c) {
            out.push_str(&format!("\\u{{{:04X}}}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
    out
}

/// One `oml skills list` line for an offered skill, every stored field
/// escaped by [`printable`].
#[must_use]
pub fn list_line(s: &Value) -> String {
    let mark = if s["unreviewed"] == true {
        " [unreviewed]"
    } else {
        ""
    };
    let overrides = s["overrides"]
        .as_array()
        .filter(|o| !o.is_empty())
        .map(|o| {
            let paths: Vec<String> = o.iter().filter_map(Value::as_str).map(printable).collect();
            format!(" (overrides {})", paths.join(", "))
        })
        .unwrap_or_default();
    format!(
        "{} v{} {}{mark}: {}{overrides}",
        printable(s["name"].as_str().unwrap_or("?")),
        s["version"],
        printable(s["workspace_path"].as_str().unwrap_or("?")),
        printable(s["description"].as_str().unwrap_or("")),
    )
}

/// How long the doctor waits for the `oml` on `PATH` to answer its
/// local-skill probe before it stops the process.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// What a probe of a hook program returned.
#[derive(Debug, PartialEq, Eq)]
pub enum Probe {
    /// No such program on `PATH`.
    Missing,
    /// It ran past the timeout and was stopped.
    TimedOut,
    /// It could not be run for another reason.
    Failed,
    /// It exited; its stdout.
    Answered(String),
}

/// Runs `program` with `args`, writes `input` to its stdin, and waits at
/// most `timeout` for it to exit, killing it past that, so a stuck `oml`
/// never holds the doctor (ADR-0080 d15; R13).
#[must_use]
pub fn probe(program: &str, args: &[&str], input: &str, timeout: Duration) -> Probe {
    use std::io::{Read as _, Write as _};
    use std::process::{Command, Stdio};
    let child = Command::new(program)
        .args(args)
        .env_remove("OML_API_KEY")
        .env_remove("OML_URL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Probe::Missing,
        Err(_) => return Probe::Failed,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe::TimedOut;
            }
            Err(_) => return Probe::Failed,
        }
    }
    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    Probe::Answered(stdout)
}

/// The line for a probe of the `oml` on `PATH` with a `Write` under
/// `~/.claude/skills/`: an `oml` that does not answer `ask` predates the
/// rule, one that does not answer in time was stopped, and no `oml` on
/// `PATH` is said plainly; each is informational, since the `PATH` may not
/// be the plugin's (R13). `None` when it asks, or could not be run.
#[must_use]
pub fn hook_probe_line(probe: &Probe) -> Option<String> {
    match probe {
        Probe::Missing => Some(
            "info skills: no oml is on PATH, so whether the plugin's hooks ask before writing local skill files was not checked".to_owned(),
        ),
        Probe::TimedOut => Some(format!(
            "info skills: the oml on PATH did not answer the local-skill probe within {} s and was stopped",
            PROBE_TIMEOUT.as_secs()
        )),
        Probe::Failed => None,
        Probe::Answered(stdout) => {
            let asks = serde_json::from_str::<Value>(stdout.trim())
                .is_ok_and(|doc| doc["hookSpecificOutput"]["permissionDecision"] == "ask");
            (!asks).then(|| {
                "info skills: the oml the plugin's hooks run is older than the local-skill rule, so this oml does not ask before writing local skill files; install the current oml".to_owned()
            })
        }
    }
}

/// The skill directories under `root` (a `.claude/skills` directory) that
/// hold a `SKILL.md`.
#[must_use]
pub fn skills_on_disk(root: &Path) -> usize {
    std::fs::read_dir(root).map_or(0, |entries| {
        entries
            .filter_map(Result::ok)
            .filter(|e| e.path().join("SKILL.md").is_file())
            .count()
    })
}

/// One line per root that keeps skills on disk, with the count
/// (informational: the person may mean it).
#[must_use]
pub fn disk_lines(roots: &[PathBuf]) -> Vec<String> {
    let mut seen: Vec<&PathBuf> = Vec::new();
    let mut lines = Vec::new();
    for root in roots {
        if seen.contains(&root) {
            continue;
        }
        seen.push(root);
        let n = skills_on_disk(root);
        if n > 0 {
            let noun = if n == 1 { "skill is" } else { "skills are" };
            lines.push(format!(
                "info skills: {n} {noun} kept on disk and not in OML ({}); store each with oml_write kind skill",
                root.display()
            ));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn doctor_names_a_server_that_predates_skills_a_second_page_and_a_shadowing_skill() {
        let refused = Err(CliError::Refused {
            status: 404,
            code: "not_found".to_owned(),
            message: "no route".to_owned(),
            rule: String::new(),
            envelope: Value::Null,
        });
        assert_eq!(
            server_lines(&refused),
            [
                "warn skills: this server predates skills (it does not serve GET /v1/skills); upgrade the server"
            ]
        );
        let page = Ok(json!({
            "scope_path": "acme.metals",
            "next_cursor": "c1",
            "skills": [
                {"name": "deploy", "workspace_path": "acme.metals", "overrides": ["acme"]},
                {"name": "lint", "workspace_path": "acme", "overrides": []},
            ],
        }));
        assert_eq!(
            server_lines(&page),
            [
                "info skills: the scope acme.metals offers more than one page of skills (50); the session index prints 20",
                "info skills: deploy from acme.metals shadows the skill of that name at acme",
            ]
        );
    }

    #[test]
    fn doctor_warns_an_oml_that_does_not_ask_before_a_local_skill_write() {
        let asks = r#"{"hookSpecificOutput":{"permissionDecision":"ask"}}"#;
        assert_eq!(hook_probe_line(&Probe::Answered(asks.to_owned())), None);
        assert!(
            hook_probe_line(&Probe::Answered(String::new()))
                .is_some_and(|l| l.contains("does not ask before writing local skill files"))
        );
        assert_eq!(hook_probe_line(&Probe::Failed), None);
    }

    /// The probe stops a program that does not answer within its timeout,
    /// and says so (ADR-0080 d15; R13).
    // `sleep` and `cat` are POSIX programs.
    #[cfg(unix)]
    #[test]
    fn the_doctor_probe_stops_an_oml_that_does_not_answer_in_time() {
        let started = Instant::now();
        let probed = probe("sleep", &["30"], "", Duration::from_millis(300));
        assert_eq!(probed, Probe::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5), "it waited");
        assert_eq!(
            hook_probe_line(&probed).as_deref(),
            Some(
                "info skills: the oml on PATH did not answer the local-skill probe within 5 s and was stopped"
            )
        );
        assert_eq!(PROBE_TIMEOUT, Duration::from_secs(5));
    }

    /// No `oml` on `PATH` prints an info line instead of nothing.
    // `sleep` and `cat` are POSIX programs.
    #[cfg(unix)]
    #[test]
    fn the_doctor_says_so_when_no_oml_is_on_path() {
        let probed = probe("oml-no-such-program-on-path", &[], "", PROBE_TIMEOUT);
        assert_eq!(probed, Probe::Missing);
        assert!(
            hook_probe_line(&probed)
                .is_some_and(|l| l.starts_with("info skills: no oml is on PATH")),
        );
        assert_eq!(
            probe("cat", &[], "{}", PROBE_TIMEOUT),
            Probe::Answered("{}".to_owned())
        );
    }

    /// Every stored field `oml skills list` prints has its control, bidi
    /// and zero-width characters escaped (ADR-0080 d1).
    #[test]
    fn oml_skills_list_escapes_control_characters_in_skill_fields() {
        let line = list_line(&json!({
            "name": "deploy\u{1b}[2J",
            "version": 3,
            "workspace_path": "acme\u{202E}metals",
            "description": "Ship\u{200B} it\u{7}.",
            "unreviewed": true,
            "overrides": ["acme\u{0}"],
        }));
        assert_eq!(
            line,
            "deploy\\u{001B}[2J v3 acme\\u{202E}metals [unreviewed]: Ship\\u{200B} it\\u{0007}. (overrides acme\\u{0000})"
        );
        assert!(!line.chars().any(hidden), "{line:?}");
        assert_eq!(printable("Déployer — vite"), "Déployer — vite");
    }

    #[test]
    fn doctor_counts_the_skills_kept_on_disk_and_not_in_oml() {
        let root = std::env::temp_dir().join(format!("oml-cli-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for name in ["a", "b"] {
            std::fs::create_dir_all(root.join(name)).expect("dir");
            std::fs::write(root.join(name).join("SKILL.md"), "x").expect("file");
        }
        std::fs::create_dir_all(root.join("empty")).expect("dir");
        let lines = disk_lines(&[root.clone(), root.clone()]);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].starts_with("info skills: 2 skills are kept on disk and not in OML"),
            "{lines:?}"
        );
    }
}
