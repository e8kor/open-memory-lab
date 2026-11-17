//! `oml doctor`'s skill lines (ADR-0080 d15, Consequences): whether the
//! server serves `GET /v1/skills`, whether the key's scope offers more than
//! one page, which offered skill shadows a farther one, whether the `oml`
//! the plugin's hooks run asks before a local skill file is written, and how
//! many skills are kept on disk instead of in OML. Each function is pure
//! over what `main` read, so the lines are proven without a server.

use std::path::{Path, PathBuf};

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
            page["scope_path"].as_str().unwrap_or("?")
        ));
    }
    for s in page["skills"].as_array().map_or(&[][..], Vec::as_slice) {
        let overrides: Vec<&str> = s["overrides"]
            .as_array()
            .map(|o| o.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if !overrides.is_empty() {
            lines.push(format!(
                "info skills: {} from {} shadows the skill of that name at {}",
                s["name"].as_str().unwrap_or("?"),
                s["workspace_path"].as_str().unwrap_or("?"),
                overrides.join(", ")
            ));
        }
    }
    lines
}

/// The `PreToolUse` document a probe of the `oml` on `PATH` printed for a
/// `Write` under `~/.claude/skills/`, or `None` when it could not be run: an
/// `oml` that does not answer `ask` predates the rule (informational: the PATH may not be the plugin's).
#[must_use]
pub fn hook_probe_line(stdout: Option<&str>) -> Option<String> {
    let stdout = stdout?;
    let asks = serde_json::from_str::<Value>(stdout.trim())
        .is_ok_and(|doc| doc["hookSpecificOutput"]["permissionDecision"] == "ask");
    (!asks).then(|| {
        "info skills: the oml the plugin's hooks run is older than the local-skill rule, so this oml does not ask before writing local skill files; install the current oml".to_owned()
    })
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
        assert_eq!(hook_probe_line(Some(asks)), None);
        assert!(
            hook_probe_line(Some(""))
                .is_some_and(|l| l.contains("does not ask before writing local skill files"))
        );
        assert_eq!(hook_probe_line(None), None);
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
