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
///
/// Only live claims count, held or promoted and no retirement, and of the
/// other kinds only those under the document's own key, `doc/<stem>/`,
/// which the `headings` and `lines` adapters write for that file (the
/// review's finding S9 of 2026-11-18).
#[must_use]
pub fn mixed_lines(items: &[Value]) -> Vec<String> {
    let live = |item: &&Value| {
        matches!(item["state"].as_str(), Some("held" | "promoted"))
            && item["retirement"] != Value::Bool(true)
    };
    let mut by_path: BTreeMap<&str, (Vec<&str>, Vec<&str>)> = BTreeMap::new();
    for item in items.iter().filter(live) {
        let (Some(path), Some(key)) = (item["anchor"]["path"].as_str(), item["claim_key"].as_str())
        else {
            continue;
        };
        let entry = by_path.entry(path).or_default();
        if item["kind"] == "document" {
            entry.0.push(key);
        } else {
            entry.1.push(key);
        }
    }
    for (docs, others) in by_path.values_mut() {
        others.retain(|key| docs.iter().any(|d| key.starts_with(&format!("{d}/"))));
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
/// `(path, text, format)` read from the clone for a `document` claim
/// anchored in it, cut by the server's own rule under the claim's format,
/// never the file's extension (S9). The server refuses that file's next
/// version with `too_many_passages`.
#[must_use]
pub fn cap_lines(files: &[(String, String, String)]) -> Vec<String> {
    files
        .iter()
        .filter_map(|(path, text, media_type)| {
            let format = if media_type == "text/markdown" {
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

/// The most bytes of a document file doctor reads: a document's body cap
/// (ADR-0081 d1).
pub const DOCUMENT_FILE_MAX: u64 = 1024 * 1024;

/// A document file of this clone as doctor reads it, `path` under `root`:
/// `None` unless it is a regular file, never a symlink, of at most
/// [`DOCUMENT_FILE_MAX`] bytes (S9), so doctor never follows a link out of
/// the clone or reads a file the server would refuse whole.
#[must_use]
pub fn read_document_file(root: &std::path::Path, path: &str) -> Option<String> {
    let full = root.join(path);
    let meta = std::fs::symlink_metadata(&full).ok()?;
    if !meta.file_type().is_file() || meta.len() > DOCUMENT_FILE_MAX {
        return None;
    }
    std::fs::read_to_string(full).ok()
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
        let new =
            Ok(json!({"result": {"resourceTemplates": [{"name": "document"}, {"name": "skill"}]}}));
        assert!(server_lines(&new).is_empty());
    }

    #[test]
    fn doctor_names_a_path_loaded_as_a_document_and_by_another_adapter() {
        let items = vec![
            json!({"kind": "document", "claim_key": "doc/docs/guide", "state": "promoted",
                   "anchor": {"path": "docs/guide.md"}}),
            json!({"kind": "memory", "claim_key": "doc/docs/guide/install", "state": "promoted",
                   "anchor": {"path": "docs/guide.md"}}),
            json!({"kind": "memory", "claim_key": "doc/docs/guide/intro/a", "state": "promoted",
                   "anchor": {"path": "docs/guide/intro.md"}}),
        ];
        assert_eq!(
            mixed_lines(&items),
            vec![
                "warn documents: docs/guide.md is loaded both as a document (doc/docs/guide) and by another adapter (doc/docs/guide/install); oml sync refuses the path until one is retired with --change-adapter"
            ]
        );
    }

    #[test]
    fn doctor_counts_only_live_claims_under_the_documents_own_key() {
        let items = vec![
            json!({"kind": "document", "claim_key": "doc/docs/guide", "state": "promoted",
                   "anchor": {"path": "docs/guide.md"}}),
            // Retired, rejected, or outside doc/docs/guide/: none mixes.
            json!({"kind": "memory", "claim_key": "doc/docs/guide/old", "state": "promoted",
                   "retirement": true, "anchor": {"path": "docs/guide.md"}}),
            json!({"kind": "memory", "claim_key": "doc/docs/guide/no", "state": "rejected",
                   "anchor": {"path": "docs/guide.md"}}),
            json!({"kind": "memory", "claim_key": "notes/guide-review", "state": "promoted",
                   "anchor": {"path": "docs/guide.md"}}),
        ];
        assert!(mixed_lines(&items).is_empty(), "{:?}", mixed_lines(&items));
        let retired_doc = vec![
            json!({"kind": "document", "claim_key": "doc/docs/guide", "state": "promoted",
                   "retirement": true, "anchor": {"path": "docs/guide.md"}}),
            json!({"kind": "memory", "claim_key": "doc/docs/guide/install", "state": "held",
                   "anchor": {"path": "docs/guide.md"}}),
        ];
        assert!(
            mixed_lines(&retired_doc).is_empty(),
            "a retired document mixes nothing"
        );
    }

    #[test]
    fn doctor_cuts_a_document_file_by_its_claims_format_not_its_extension() {
        let headings = "## h\nx\n".repeat(oml_passage::MAX_PASSAGES + 1);
        // Stored as plain text: one passage-run of lines, not 4,097 sections.
        assert!(
            cap_lines(&[(
                "docs/notes.md".to_owned(),
                headings.clone(),
                "text/plain".to_owned()
            )])
            .is_empty()
        );
        // Stored as markdown under a .txt name: cut at its headings.
        assert_eq!(
            cap_lines(&[(
                "docs/notes.txt".to_owned(),
                headings,
                "text/markdown".to_owned()
            )])
            .len(),
            1
        );
    }

    #[test]
    fn doctor_reads_a_document_file_only_when_it_is_a_regular_file_of_at_most_1_mib() {
        let dir = std::env::temp_dir().join(format!("oml-doc-doctor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("small.md"), "# T\n\nbody\n").expect("write");
        std::fs::write(dir.join("big.md"), "a".repeat(1024 * 1024 + 1)).expect("write");
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("small.md"), dir.join("link.md")).expect("link");
        assert_eq!(
            read_document_file(&dir, "small.md").as_deref(),
            Some("# T\n\nbody\n")
        );
        assert_eq!(read_document_file(&dir, "big.md"), None, "past 1 MiB");
        #[cfg(unix)]
        assert_eq!(read_document_file(&dir, "link.md"), None, "a symlink");
        assert_eq!(read_document_file(&dir, "missing.md"), None);
        std::fs::remove_dir_all(&dir).expect("clean");
    }

    #[test]
    fn doctor_names_a_document_file_past_the_passage_cap() {
        let long = "a\n\n".repeat(oml_passage::MAX_PASSAGES * oml_passage::BLOCK_LINES);
        let lines = cap_lines(&[
            ("docs/long.txt".to_owned(), long, "text/plain".to_owned()),
            (
                "docs/short.md".to_owned(),
                "# T\n\nbody\n".to_owned(),
                "text/markdown".to_owned(),
            ),
        ]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("warn documents: docs/long.txt cuts into "));
    }
}
