//! `oml setup claude-code`'s local half (ADR-0041 d7): the roles it refuses,
//! the Claude Code version it requires, the three `claude` commands it runs,
//! the values it writes to `claude plugin configure --values-stdin`, and the
//! lines it prints in their place with `--print` or with no `claude` on
//! `PATH`. The key travels on stdin alone, never in argv (ADR-0040 d4).

use serde_json::json;

use crate::issue::{configure_line, sh_quote};

/// The plugin the verb installs, as `claude plugin` names it.
pub const PLUGIN: &str = "oml@open-memory-lab";

/// The marketplace's GitHub repository.
pub const MARKETPLACE: &str = "e8kor/open-memory-lab";

/// The first Claude Code with `claude plugin configure --values-stdin`
/// (ADR-0040 d5).
pub const MIN_CLAUDE_CODE: (u32, u32, u32) = (2, 1, 285);

/// Whether a key of `role` is refused as `setup.admin_key`: an admin or
/// operator key never goes into an agent's plugin (ADR-0041 d7 step 2).
#[must_use]
pub fn refuses_role(role: &str) -> bool {
    matches!(role, "admin" | "operator")
}

/// The version `claude --version` printed, as `2.1.312 (Claude Code)`
/// shows it, or `None` when its first word is not three dotted numbers.
#[must_use]
pub fn claude_code_version(output: &str) -> Option<(u32, u32, u32)> {
    let word = output.split_whitespace().next()?;
    let mut parts = word.split('.').map(str::parse::<u32>);
    let version = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    parts.next().is_none().then_some(version)
}

/// `major.minor.patch`.
#[must_use]
pub fn dotted((major, minor, patch): (u32, u32, u32)) -> String {
    format!("{major}.{minor}.{patch}")
}

/// The arguments of the two `claude` commands that install the plugin: the
/// marketplace pinned to the CLI's own tag, so the plugin matches the CLI's
/// release, then the plugin (ADR-0041 d7 step 4).
#[must_use]
pub fn install_commands(version: &str) -> [Vec<String>; 2] {
    [
        vec![
            "plugin".to_owned(),
            "marketplace".to_owned(),
            "add".to_owned(),
            format!("{MARKETPLACE}#v{version}"),
        ],
        vec!["plugin".to_owned(), "install".to_owned(), PLUGIN.to_owned()],
    ]
}

/// The arguments of the command that saves the options from stdin
/// (ADR-0040 d4).
#[must_use]
pub fn configure_command() -> Vec<String> {
    vec![
        "plugin".to_owned(),
        "configure".to_owned(),
        PLUGIN.to_owned(),
        "--values-stdin".to_owned(),
    ]
}

/// The one-line JSON object `claude plugin configure --values-stdin` reads:
/// the plugin's `url` and `api_key` options (ADR-0040 d1).
#[must_use]
pub fn values(url: &str, key: &str) -> String {
    json!({ "url": url, "api_key": key }).to_string()
}

/// The lines printed in place of running the commands: the two installs,
/// then a configure line that reads the key file, or without one reads
/// `OML_API_KEY` from the environment, so the value is printed nowhere
/// (ADR-0041 d7).
#[must_use]
pub fn printed(version: &str, url: &str, key_file: Option<&str>) -> Vec<String> {
    let mut lines: Vec<String> = install_commands(version)
        .iter()
        .map(|args| format!("claude {}", args.join(" ")))
        .collect();
    lines.push(key_file.map_or_else(
        || {
            format!(
                "jq -n --arg u {} '{{url: $u, api_key: env.OML_API_KEY}}' \
                 | claude plugin configure {PLUGIN} --values-stdin",
                sh_quote(url)
            )
        },
        |file| configure_line(url, file),
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_first_word_of_claude_version_as_three_numbers() {
        assert_eq!(
            claude_code_version("2.1.312 (Claude Code)\n"),
            Some((2, 1, 312))
        );
        assert_eq!(claude_code_version("2.1"), None);
        assert_eq!(claude_code_version("2.1.3.4"), None);
        assert_eq!(claude_code_version("Claude Code"), None);
        assert!((2, 1, 284) < MIN_CLAUDE_CODE && (2, 2, 0) > MIN_CLAUDE_CODE);
    }

    #[test]
    fn only_admin_and_operator_keys_are_refused() {
        for role in ["admin", "operator"] {
            assert!(refuses_role(role));
        }
        for role in ["reader", "writer", "dreamer"] {
            assert!(!refuses_role(role));
        }
    }

    #[test]
    fn without_a_key_file_the_printed_configure_line_reads_oml_api_key() {
        let lines = printed("0.4.0", "https://oml.example.com", None);
        assert_eq!(lines.len(), 3);
        assert!(lines[2].contains("env.OML_API_KEY"), "{}", lines[2]);
    }
}
