//! The redirect allowlist: exactly Claude's hosted callback, matched on the
//! string as written and never after URL parsing (ADR-0031 d6;
//! `docs/api/http-api.md`, "The redirect allowlist"). The list is fixed,
//! not a setting: each entry decides where an authorization code may be
//! delivered. Loopback redirects are deferred, and admitting one is a later
//! recorded decision (ADR-0031, "Loopback redirects for Claude Code").

use super::refusal::{RedirectRefused, Refusal};

/// The one redirect admitted: where Claude's hosted apps receive the
/// authorization code.
pub const CLAUDE_CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";

/// Whether `uri` is inside the allowlist, at registration, in a fetched
/// document or in an authorize request: byte-exactly [`CLAUDE_CALLBACK`].
/// Anything else is `oauth.redirect_refused`.
pub fn admit_redirect_uri(uri: &str) -> Result<(), Refusal> {
    if uri == CLAUDE_CALLBACK {
        Ok(())
    } else {
        Err(Refusal::RedirectRefused(RedirectRefused::OutsideAllowlist))
    }
}

/// Whether a presented redirect URI matches a registered one: both
/// [`CLAUDE_CALLBACK`], byte for byte. A URI outside the allowlist matches
/// nothing.
#[must_use]
pub fn redirect_matches(registered: &str, presented: &str) -> bool {
    registered == CLAUDE_CALLBACK && presented == CLAUDE_CALLBACK
}

/// The redirect URI an authorize request delivers to, and whether the
/// request sent it — the token endpoint's `redirect_uri` rule turns on that
/// (`oauth_authorizations.redirect_uri_sent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedRedirect<'a> {
    pub uri: &'a str,
    pub sent: bool,
}

/// An authorize request's `redirect_uri` against the client's registered
/// URIs (ADR-0031 d6): a presented URI must be inside the allowlist and
/// match one of them, and is delivered to as presented;
/// a client with exactly one registered URI may omit it. Every refusal is
/// `oauth.redirect_refused`, shown on the page and never redirected.
pub fn select_redirect<'a, S: AsRef<str>>(
    registered: &'a [S],
    presented: Option<&'a str>,
) -> Result<SelectedRedirect<'a>, Refusal> {
    let refused = |reason| Err(Refusal::RedirectRefused(reason));
    match (presented, registered) {
        (Some(uri), _) => {
            admit_redirect_uri(uri)?;
            if registered.iter().any(|r| redirect_matches(r.as_ref(), uri)) {
                Ok(SelectedRedirect { uri, sent: true })
            } else {
                refused(RedirectRefused::NotRegistered)
            }
        }
        (None, [only]) => {
            let uri = only.as_ref();
            admit_redirect_uri(uri)?;
            Ok(SelectedRedirect { uri, sent: false })
        }
        (None, _) => refused(RedirectRefused::Omitted),
    }
}
