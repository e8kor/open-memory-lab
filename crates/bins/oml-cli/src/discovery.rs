//! Doctor's `oauth_discovery` check (observability.md, the doctor contract;
//! ADR-0031 d14): the issuer the lead serves at the configured URL, then
//! both discovery documents fetched again from that issuer's own origin,
//! wherever the CLI runs, so a reverse proxy that does not pass
//! `/.well-known/oauth-*` through, or a node serving another issuer, shows
//! as `failing` with the URL that failed.

use std::time::Duration;

use serde_json::{Value, json};

use crate::client::{CliError, Client};

/// The RFC 8414 document's path.
pub const AUTHORIZATION_SERVER: &str = "/.well-known/oauth-authorization-server";
/// The RFC 9728 document's path-inserted URL for `/mcp`.
pub const PROTECTED_RESOURCE: &str = "/.well-known/oauth-protected-resource/mcp";

/// How long each discovery fetch may take; Claude waits 10 seconds for
/// discovery (ADR-0031, what Claude expects).
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Why the two documents fetched from `issuer` do not describe it, or
/// `None` when they do: the authorization server's `issuer` is the lead's,
/// and the protected resource is `<issuer>/mcp` with that one
/// authorization server (ADR-0031 d3).
#[must_use]
pub fn mismatch(issuer: &str, authorization_server: &Value, protected: &Value) -> Option<String> {
    let resource = format!("{issuer}/mcp");
    if authorization_server["issuer"].as_str() != Some(issuer) {
        return Some(format!(
            "issuer is {}, not the lead's {issuer}",
            authorization_server["issuer"]
        ));
    }
    if protected["resource"].as_str() != Some(resource.as_str()) {
        return Some(format!(
            "resource is {}, not {resource}",
            protected["resource"]
        ));
    }
    if protected["authorization_servers"] != json!([issuer]) {
        return Some(format!(
            "authorization_servers is {}, not [{issuer:?}]",
            protected["authorization_servers"]
        ));
    }
    None
}

/// The check against the lead at `url`: `off` where it serves no
/// authorization server document (`oauth.issuer` unset), `ok` with the
/// issuer where both documents fetched from the issuer describe it, and
/// `failing` with the URL and the reason otherwise.
#[must_use]
pub fn check(url: &str) -> Value {
    let lead_url = format!("{}{AUTHORIZATION_SERVER}", url.trim_end_matches('/'));
    let lead = Client::with_timeout(url.to_owned(), None, FETCH_TIMEOUT);
    let issuer = match lead.probe(AUTHORIZATION_SERVER) {
        Ok((404, _)) => return json!({ "state": "off" }),
        Ok((200, doc)) => match doc["issuer"].as_str() {
            Some(issuer) => issuer.to_owned(),
            None => return failing(None, &lead_url, "the document names no issuer"),
        },
        Ok((status, _)) => return failing(None, &lead_url, &format!("status {status}")),
        Err(e) => return failing(None, &lead_url, &why(e)),
    };
    let public = Client::with_timeout(issuer.clone(), None, FETCH_TIMEOUT);
    let mut documents = Vec::new();
    for path in [AUTHORIZATION_SERVER, PROTECTED_RESOURCE] {
        let fetched = format!("{issuer}{path}");
        match public.probe(path) {
            Ok((200, doc)) => documents.push(doc),
            Ok((status, _)) => {
                return failing(Some(&issuer), &fetched, &format!("status {status}"));
            }
            Err(e) => return failing(Some(&issuer), &fetched, &why(e)),
        }
    }
    if let Some(why) = mismatch(&issuer, &documents[0], &documents[1]) {
        return failing(
            Some(&issuer),
            &format!("{issuer}{AUTHORIZATION_SERVER}"),
            &why,
        );
    }
    json!({ "state": "ok", "issuer": issuer })
}

/// A failed fetch's reason; the probe sends no key, so none carries one.
fn why(e: CliError) -> String {
    match e {
        CliError::Refused { status, code, .. } => format!("{status} {code}"),
        CliError::Local(why) | CliError::Unanswered(why) => why,
    }
}

fn failing(issuer: Option<&str>, url: &str, reason: &str) -> Value {
    json!({ "state": "failing", "issuer": issuer, "url": url, "reason": reason })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::mismatch;

    const ISSUER: &str = "https://oml.example.com";

    #[test]
    fn documents_that_name_the_issuer_and_its_mcp_resource_match() {
        let server = json!({ "issuer": ISSUER });
        let resource = json!({
            "resource": "https://oml.example.com/mcp",
            "authorization_servers": [ISSUER],
        });
        assert_eq!(mismatch(ISSUER, &server, &resource), None);
    }

    #[test]
    fn a_node_serving_another_issuer_or_a_rewritten_resource_is_named() {
        let resource = json!({
            "resource": "https://oml.example.com/mcp",
            "authorization_servers": [ISSUER],
        });
        let other = json!({ "issuer": "https://other.example.com" });
        assert!(mismatch(ISSUER, &other, &resource).is_some_and(|w| w.contains("issuer")));
        let server = json!({ "issuer": ISSUER });
        let rewritten = json!({
            "resource": "https://oml.example.com/api/mcp",
            "authorization_servers": [ISSUER],
        });
        assert!(mismatch(ISSUER, &server, &rewritten).is_some_and(|w| w.contains("resource")));
    }
}
