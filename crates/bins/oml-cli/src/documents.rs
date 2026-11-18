//! `oml doctor`'s document lines (ADR-0081 Consequences): a server that
//! predates documents, a path of this clone loaded both as a document and
//! by another adapter, which `oml sync` refuses to extend (d7, the build's
//! reading R5), and a document of this clone whose file now cuts past the
//! 4,096-passage cap, whose next version the server refuses (d3). Each
//! function is pure over what `main` read, so the lines are proven without
//! a server.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::client::CliError;

/// The server line from MCP's `resources/templates/list`: a server whose
/// templates name no `document`, or that answers no such method, predates
/// documents (a warning); an unreachable or refused call says nothing.
#[must_use]
pub fn server_lines(answer: &Result<Value, CliError>) -> Vec<String> {
    let predates = vec![
        "warn documents: this server predates documents (its MCP resources serve no document template); upgrade the server"
            .to_owned(),
    ];
    match answer {
        Ok(rpc) if rpc["error"]["code"] == -32601 => predates,
        Ok(rpc) => match rpc["result"]["resourceTemplates"].as_array() {
            Some(templates) if templates.iter().any(|t| t["name"] == "document") => Vec::new(),
            Some(_) => predates,
            None => Vec::new(),
        },
        Err(CliError::Refused {
            status: 404 | 405, ..
        }) => predates,
        Err(_) => Vec::new(),
    }
}

/// The paths of this clone the workspace holds both ways (R5): a
/// `document` claim and claims of another kind whose anchor names the same
/// file, from the listing items `main` read for this clone's repository.
/// Each line names the path and both kinds of key, so the person knows
/// which `oml sync --change-adapter` run settles it.
#[must_use]
pub fn mixed_lines(items: &[Value]) -> Vec<String> {
    let mut by_path: BTreeMap<&str, (Vec<&str>, Vec<&str>)> = BTreeMap::new();
    for item in items {
        let (Some(path), Some(key)) = (
            item["anchor"]["path"].as_str(),
            item["claim_key"].as_str(),
        ) else {
            continue;
        };
        let entry = by_path.entry(path).or_default();
        if item["kind"] == "document" {
            entry.0.push(key);
        } else {
            entry.1.push(key);
        }
    }
    by_path
        .into_iter()
        .filter(|(_, (docs, others))| !docs.is_empty() && !others.is_empty())
        .map(|(path, (docs, others))| {
            format!(
                "warn documents: {path} is loaded both as a document ({}) and by another adapter ({}); oml sync refuses the path until one is retired with --change-adapter",
                docs.join(", "),
                others.join(", ")
            )
        })
        .collect()
}

/// The documents of this clone whose file cuts past the passage cap: each
/// `(path, text)` read from the clone for a `document` claim anchored in
/// it, cut by the server's own rule. The server refuses that file's next
/// version with `too_many_passages`.
#[must_use]
pub fn cap_lines(files: &[(String, String)]) -> Vec<String> {
    files
        .iter()
        .filter_map(|(path, text)| {
            let format = if path.ends_with(".md") {
                oml_passage::Format::Markdown
            } else {
                oml_passage::Format::Plain
            };
            oml_passage::passages(text, format).err().map(|e| {
                format!(
                    "warn documents: {path} cuts into {} passages, past the {} a document holds; the server refuses its next version (request.validation, too_many_passages), so split the file",
                    e.count,
                    oml_passage::MAX_PASSAGES
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn doctor_names_a_server_whose_resources_serve_no_document_template() {
        let old = Ok(json!({"result": {"resourceTemplates": [{"name": "skill"}]}}));
        assert_eq!(
            server_lines(&old),
            vec![
                "warn documents: this server predates documents (its MCP resources serve no document template); upgrade the server"
            ]
        );
        let none = Ok(json!({"error": {"code": -32601, "message": "method not found"}}));
        assert_eq!(server_lines(&none).len(), 1);
        let new = Ok(json!({"result": {"resourceTemplates": [{"name": "document"}, {"name": "skill"}]}}));
        assert!(server_lines(&new).is_empty());
    }

    #[test]
    fn doctor_names_a_path_loaded_as_a_document_and_by_another_adapter() {
        let items = vec![
            json!({"kind": "document", "claim_key": "doc/docs/guide", "anchor": {"path": "docs/guide.md"}}),
            json!({"kind": "memory", "claim_key": "doc/docs/guide/install", "anchor": {"path": "docs/guide.md"}}),
            json!({"kind": "memory", "claim_key": "doc/docs/guide/intro/a", "anchor": {"path": "docs/guide/intro.md"}}),
        ];
        assert_eq!(
            mixed_lines(&items),
            vec![
                "warn documents: docs/guide.md is loaded both as a document (doc/docs/guide) and by another adapter (doc/docs/guide/install); oml sync refuses the path until one is retired with --change-adapter"
            ]
        );
    }

    #[test]
    fn doctor_names_a_document_file_past_the_passage_cap() {
        let long = "a\n\n".repeat(oml_passage::MAX_PASSAGES * oml_passage::BLOCK_LINES);
        let lines = cap_lines(&[
            ("docs/long.txt".to_owned(), long),
            ("docs/short.md".to_owned(), "# T\n\nbody\n".to_owned()),
        ]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("warn documents: docs/long.txt cuts into "));
    }
}
