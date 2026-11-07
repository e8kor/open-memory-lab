//! `oml review commit` (ADR-0064 d18): a person's bulk act at a merge or a
//! release. The held set is written to a file the person edits; the file's
//! verdicts are submitted as bulk verdicts; the workarea is committed only
//! when nothing is held, and marked in progress again.

use serde_json::Value;

/// One line of the decision file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Reject(String),
    Leave,
}

/// The file the person edits: one line per held item, `leave` unless edited
/// to `approve` or `reject <reason>`.
#[must_use]
pub fn render(items: &[Value], workarea: &str) -> String {
    let mut out = format!(
        "# {} held item(s) in {workarea}. Edit each line's first word to `approve`, `reject <reason>` or leave it\n# as `leave`, then run the command again with --file. A line that says `leave` stays held and stops the commit.\n",
        items.len()
    );
    for item in items {
        let text = item["text"].as_str().unwrap_or("").replace('\n', " ");
        let shown: String = text.chars().take(80).collect();
        let anchor = match (
            item["anchor"]["path"].as_str(),
            item["anchor"]["fragment"].as_str(),
        ) {
            (Some(p), Some(f)) => format!("{p}#{f}"),
            (Some(p), None) => p.to_owned(),
            _ => String::new(),
        };
        out.push_str(&format!(
            "leave {}  # {} {anchor} | {shown}\n",
            item["artifact_id"].as_str().unwrap_or(""),
            item["claim_key"].as_str().unwrap_or("")
        ));
    }
    out
}

/// The decisions a file states, each with its artifact id.
///
/// # Errors
///
/// A line that is not `approve|reject <reason>|leave` followed by an id.
pub fn parse(text: &str) -> Result<Vec<(String, Decision)>, String> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let bad = || {
            format!(
                "line {}: expected `approve <id>`, `reject <reason> <id>` or `leave <id>`",
                n + 1
            )
        };
        let (verb, rest) = line.split_once(char::is_whitespace).ok_or_else(bad)?;
        let rest = rest.trim();
        match verb {
            "approve" => out.push((rest.to_owned(), Decision::Approve)),
            "leave" => out.push((rest.to_owned(), Decision::Leave)),
            "reject" => {
                let (reason, id) = rest.rsplit_once(char::is_whitespace).ok_or_else(bad)?;
                if reason.trim().is_empty() {
                    return Err(format!("line {}: a reject needs its reason", n + 1));
                }
                out.push((
                    id.trim().to_owned(),
                    Decision::Reject(reason.trim().to_owned()),
                ));
            }
            _ => return Err(bad()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_rendered_file_parses_back_as_all_leave() {
        let items = vec![
            json!({"artifact_id": "a1", "claim_key": "k", "text": "one\ntwo", "anchor": {"path": "src/x.rs", "fragment": "L1-L2"}}),
        ];
        let parsed = parse(&render(&items, "pat.w")).unwrap();
        assert_eq!(parsed, vec![("a1".to_owned(), Decision::Leave)]);
    }

    #[test]
    fn approve_reject_and_leave_are_read_and_a_reject_needs_its_reason() {
        let parsed =
            parse("approve a1\nreject duplicate of the first a2 # why\nleave a3\n").unwrap();
        assert_eq!(parsed[0], ("a1".into(), Decision::Approve));
        assert_eq!(
            parsed[1],
            (
                "a2".into(),
                Decision::Reject("duplicate of the first".into())
            )
        );
        assert_eq!(parsed[2], ("a3".into(), Decision::Leave));
        assert!(parse("reject a2").is_err());
        assert!(parse("maybe a2").is_err());
    }
}
