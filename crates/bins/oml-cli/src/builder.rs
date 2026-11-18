//! `oml doctor`'s builder line (ADR-0082 Consequences): whether the server
//! serves `POST /v1/memories/preview`, which the console's builder draws
//! every form's preview from. Pure over what `main` read, so the line is
//! proven without a server.

use serde_json::Value;

use crate::client::CliError;

/// The line from a preview of an empty body, which writes nothing on any
/// server: a server that routes no preview predates the builder (a
/// warning); any other answer, a refusal of the key included, says nothing.
#[must_use]
pub fn server_lines(answer: &Result<Value, CliError>) -> Vec<String> {
    let predates = match answer {
        Err(CliError::Refused {
            status: 404 | 405, ..
        }) => true,
        Err(CliError::Local(m)) => m.contains("404") || m.contains("405"),
        _ => false,
    };
    if predates {
        vec![
            "warn builder: this server predates the builder (it does not serve POST /v1/memories/preview), so the console draws no preview; upgrade the server".to_owned(),
        ]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn doctor_names_a_server_that_predates_the_builder_and_nothing_for_one_that_serves_preview() {
        let refused = |status| {
            Err(CliError::Refused {
                status,
                code: String::new(),
                message: String::new(),
                rule: String::new(),
                envelope: Value::Null,
            })
        };
        for status in [404, 405] {
            assert_eq!(
                server_lines(&refused(status)),
                vec![
                    "warn builder: this server predates the builder (it does not serve POST /v1/memories/preview), so the console draws no preview; upgrade the server"
                ]
            );
        }
        assert!(server_lines(&Ok(json!({ "would_capture": false }))).is_empty());
        assert!(server_lines(&refused(403)).is_empty());
    }
}
