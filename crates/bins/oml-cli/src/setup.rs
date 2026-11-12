//! `oml setup claude-code`'s local half (ADR-0041 d7): the roles it refuses,
//! the Claude Code version it requires, how it reads the marketplace Claude
//! Code already holds under the name `open-memory-lab`, the `claude` commands
//! it runs,
//! the values it writes to `claude plugin configure --values-stdin`, and the
//! lines it prints in their place with `--print` or with no `claude` on
//! `PATH`. The key travels on stdin alone, never in argv (ADR-0040 d4).

use serde_json::{Value, json};

use crate::issue::{configure_line, sh_quote};

/// The plugin the verb installs, as `claude plugin` names it.
pub const PLUGIN: &str = "oml@open-memory-lab";

/// The marketplace's GitHub repository.
pub const MARKETPLACE: &str = "e8kor/open-memory-lab";

/// The prompt `oml setup claude-code` prints to stderr, on a terminal, for
/// a key named without a URL; it shows no suggested value (ADR-0076 d4).
pub const URL_PROMPT: &str = "OML server URL (without /mcp): ";

/// Why `oml setup claude-code` refuses a key named without a URL, off a
/// terminal or after an empty answer, with `OML_URL`'s line when that
/// variable is set beside a key file (ADR-0076 d4, d8).
#[must_use]
pub fn no_server(oml_url_beside_key_file: bool) -> String {
    let mut why = "refused setup.no_server: a key was given without a server URL; pass --url \
                   <server> (without /mcp), or export OML_URL beside OML_API_KEY; nothing was sent"
        .to_owned();
    if oml_url_beside_key_file {
        why.push_str("; ");
        why.push_str(OML_URL_NOT_PAIRED);
    }
    why
}

/// What the prompt and the refusal add when `OML_URL` is set and a key file
/// came without `--url`: the variable is another source, never paired with
/// the file (ADR-0076 d4; ADR-0039 d15).
pub const OML_URL_NOT_PAIRED: &str =
    "OML_URL is set but is never paired with --key-file; pass it as --url";

/// The URL typed at the prompt, or `None` for an empty answer (ADR-0076 d4
/// step 2): one line, its surrounding whitespace cut.
#[must_use]
pub fn answered_url(line: &str) -> Option<String> {
    let url = line.trim();
    (!url.is_empty()).then(|| url.to_owned())
}

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

/// The name the marketplace declares for itself in `marketplace.json`.
pub const MARKETPLACE_NAME: &str = "open-memory-lab";

/// The `claude` arguments that list the declared marketplaces as JSON.
#[must_use]
pub fn marketplace_list_command() -> Vec<String> {
    ["plugin", "marketplace", "list", "--json"]
        .map(ToOwned::to_owned)
        .to_vec()
}

/// The `claude` arguments that list the installed plugins as JSON.
#[must_use]
pub fn plugin_list_command() -> Vec<String> {
    ["plugin", "list", "--json"].map(ToOwned::to_owned).to_vec()
}

/// The `claude` arguments that remove the marketplace, which also
/// uninstalls each plugin installed from it.
#[must_use]
pub fn remove_command() -> Vec<String> {
    ["plugin", "marketplace", "remove", MARKETPLACE_NAME]
        .map(ToOwned::to_owned)
        .to_vec()
}

/// The `claude` arguments that add the marketplace pinned to the CLI's own
/// tag, so the plugin matches the CLI's release (ADR-0041 d7 step 4).
#[must_use]
pub fn add_command(version: &str) -> Vec<String> {
    vec![
        "plugin".to_owned(),
        "marketplace".to_owned(),
        "add".to_owned(),
        format!("{MARKETPLACE}#v{version}"),
    ]
}

/// The `claude` arguments that install `plugin`.
#[must_use]
pub fn install_command(plugin: &str) -> Vec<String> {
    vec!["plugin".to_owned(), "install".to_owned(), plugin.to_owned()]
}

/// The arguments of the two `claude` commands that install the plugin into
/// a Claude Code without the marketplace: the add at the CLI's own tag,
/// then the plugin.
#[must_use]
pub fn install_commands(version: &str) -> [Vec<String>; 2] {
    [add_command(version), install_command(PLUGIN)]
}

/// What Claude Code holds under [`MARKETPLACE_NAME`], read from `claude
/// plugin marketplace list --json`.
#[derive(Debug, PartialEq, Eq)]
pub enum Marketplace {
    /// None: the verb adds it.
    Absent,
    /// [`MARKETPLACE`] at the CLI's own tag: the verb adds nothing.
    Pinned,
    /// [`MARKETPLACE`] at another tag, or at none, named: Claude Code 2.1.312
    /// refuses an add whose source differs from the declared one, so the
    /// verb removes it and adds it again.
    Repin(String),
    /// Another source under the same name, described: the verb refuses it
    /// as `setup.marketplace` rather than remove what someone else set up.
    Foreign(String),
}

/// Reads `list`, the output of `claude plugin marketplace list --json`, for
/// the marketplace the verb installs from at `version`; `None` when it is
/// not a JSON array of objects.
#[must_use]
pub fn marketplace(list: &str, version: &str) -> Option<Marketplace> {
    let entries: Vec<Value> = serde_json::from_str(list.trim()).ok()?;
    let Some(entry) = entries
        .iter()
        .find(|e| e["name"].as_str() == Some(MARKETPLACE_NAME))
    else {
        return Some(Marketplace::Absent);
    };
    if entry["source"].as_str() != Some("github") || entry["repo"].as_str() != Some(MARKETPLACE) {
        let mut shown = entry.as_object()?.clone();
        shown.remove("installLocation");
        return Some(Marketplace::Foreign(Value::Object(shown).to_string()));
    }
    Some(match entry["ref"].as_str() {
        Some(tag) if tag == format!("v{version}") => Marketplace::Pinned,
        Some(tag) => Marketplace::Repin(format!("{MARKETPLACE}#{tag}")),
        None => Marketplace::Repin(format!("{MARKETPLACE} at its default branch")),
    })
}

/// The plugins other than [`PLUGIN`] installed from [`MARKETPLACE_NAME`],
/// read from `claude plugin list --json`, in its order: removing the
/// marketplace uninstalls them, so the verb installs them again; `None`
/// when it is not a JSON array.
#[must_use]
pub fn other_plugins(list: &str) -> Option<Vec<String>> {
    let entries: Vec<Value> = serde_json::from_str(list.trim()).ok()?;
    let suffix = format!("@{MARKETPLACE_NAME}");
    Some(
        entries
            .iter()
            .filter_map(|e| e["id"].as_str())
            .filter(|id| id.ends_with(&suffix) && *id != PLUGIN)
            .map(ToOwned::to_owned)
            .collect(),
    )
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
    fn the_marketplace_is_pinned_only_at_the_clis_own_tag_of_its_own_repository() {
        let list = |entry: Value| json!([{"name": "omc", "source": "git"}, entry]).to_string();
        let ours = |tag: Option<&str>| {
            let mut e = json!({"name": "open-memory-lab", "source": "github",
                               "repo": "e8kor/open-memory-lab", "installLocation": "/x"});
            if let Some(tag) = tag {
                e["ref"] = json!(tag);
            }
            list(e)
        };
        assert_eq!(
            marketplace(&ours(Some("v0.4.0")), "0.4.0"),
            Some(Marketplace::Pinned)
        );
        assert_eq!(
            marketplace(&ours(Some("v0.3.1")), "0.4.0"),
            Some(Marketplace::Repin(
                "e8kor/open-memory-lab#v0.3.1".to_owned()
            ))
        );
        assert!(matches!(
            marketplace(&ours(None), "0.4.0"),
            Some(Marketplace::Repin(_))
        ));
        let foreign = list(json!({"name": "open-memory-lab", "source": "directory",
                                  "path": "/src/oml", "installLocation": "/x"}));
        match marketplace(&foreign, "0.4.0") {
            Some(Marketplace::Foreign(shown)) => {
                assert!(shown.contains("/src/oml") && !shown.contains("installLocation"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            marketplace(&json!([]).to_string(), "0.4.0"),
            Some(Marketplace::Absent)
        );
        assert_eq!(marketplace("not json", "0.4.0"), None);
    }

    #[test]
    fn the_plugins_reinstalled_are_the_marketplaces_own_other_than_oml() {
        let list = json!([{"id": "oml@open-memory-lab"}, {"id": "omc@omc"},
                          {"id": "oml-admin@open-memory-lab"}])
        .to_string();
        assert_eq!(
            other_plugins(&list),
            Some(vec!["oml-admin@open-memory-lab".to_owned()])
        );
        assert_eq!(other_plugins("{}"), None);
    }

    /// The refusal names `setup.no_server`, both ways to name a server and
    /// that nothing was sent; with `OML_URL` beside a key file it also says
    /// why the variable was not used (ADR-0076 d4, d8).
    #[test]
    fn the_no_server_refusal_names_the_reason_and_both_ways_to_name_a_server() {
        let plain = no_server(false);
        assert!(plain.starts_with("refused setup.no_server: "), "{plain}");
        assert!(
            plain.contains("--url <server>") && plain.contains("OML_URL beside OML_API_KEY"),
            "{plain}"
        );
        assert!(plain.ends_with("nothing was sent"), "{plain}");
        assert!(no_server(true).ends_with(OML_URL_NOT_PAIRED));
    }

    /// An empty or blank answer is no URL; any other is used as typed,
    /// trimmed (ADR-0076 d4).
    #[test]
    fn an_empty_answer_at_the_prompt_is_no_url() {
        assert_eq!(answered_url("\n"), None);
        assert_eq!(answered_url("   "), None);
        assert_eq!(
            answered_url(" https://oml.example.com\n").as_deref(),
            Some("https://oml.example.com")
        );
    }

    /// The printed configure line saves the URL the key was checked against
    /// (ADR-0076 d4).
    #[test]
    fn the_printed_configure_line_saves_the_url_the_key_was_checked_against() {
        let lines = printed(
            "0.4.1",
            "https://oml.example.com",
            Some("/home/you/agent.key"),
        );
        assert!(lines[2].contains("https://oml.example.com"), "{}", lines[2]);
    }

    #[test]
    fn without_a_key_file_the_printed_configure_line_reads_oml_api_key() {
        let lines = printed("0.4.0", "https://oml.example.com", None);
        assert_eq!(lines.len(), 3);
        assert!(lines[2].contains("env.OML_API_KEY"), "{}", lines[2]);
    }
}
