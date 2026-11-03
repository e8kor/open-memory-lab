//! The redirect allowlist: exactly Claude's hosted callback, and loopback
//! for Claude Code, matched on the strings as written and never after URL
//! parsing (ADR-0031 d6; `docs/api/http-api.md`, "The redirect allowlist").
//! The list is fixed, not a setting: each entry decides where an
//! authorization code may be delivered.

use super::refusal::{RedirectRefused, Refusal};
use super::syntax;

/// The one `https` redirect admitted: where Claude's hosted apps receive
/// the authorization code.
pub const CLAUDE_CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";

const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "[::1]", "localhost"];

/// Which entry of the allowlist a redirect URI is. The page warns that the
/// result goes to a program on this computer for [`RedirectKind::Loopback`]
/// (ADR-0031 d9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectKind {
    ClaudeCallback,
    Loopback,
}

/// Whether `uri` is inside the allowlist, at registration, in a fetched
/// document or in an authorize request: byte-exactly [`CLAUDE_CALLBACK`],
/// or a loopback URI — `http://`, a host byte-exactly `127.0.0.1`, `[::1]`
/// or `localhost`, optionally `:` and a port of one to five digits with no
/// leading zero and a value from 1 to 65535, a path beginning with `/`,
/// optionally `?` and a query — containing no `@`, `\`, `#`, `%2F`, `%2f`,
/// `%5C` or `%5c`, no ASCII control character and no whitespace. Anything
/// else is `oauth.redirect_refused`.
pub fn admit_redirect_uri(uri: &str) -> Result<RedirectKind, Refusal> {
    if uri == CLAUDE_CALLBACK {
        Ok(RedirectKind::ClaudeCallback)
    } else if loopback(uri).is_some() {
        Ok(RedirectKind::Loopback)
    } else {
        Err(Refusal::RedirectRefused(RedirectRefused::OutsideAllowlist))
    }
}

/// Whether a presented redirect URI matches a registered one: two loopback
/// URIs when they are byte-equal once the `:port` part is removed from
/// each (RFC 8252 §7.3), and [`CLAUDE_CALLBACK`] byte for byte, port
/// included. A URI outside the allowlist matches nothing.
#[must_use]
pub fn redirect_matches(registered: &str, presented: &str) -> bool {
    match (loopback(registered), loopback(presented)) {
        (Some(registered), Some(presented)) => registered == presented,
        (None, None) => registered == CLAUDE_CALLBACK && presented == CLAUDE_CALLBACK,
        _ => false,
    }
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
/// match one of them, and is delivered to as presented, its port included;
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

/// A loopback URI with its `:port` part removed: the host and everything
/// from the path's `/` on.
#[derive(PartialEq, Eq)]
struct Loopback<'a> {
    host: &'a str,
    path_and_query: &'a str,
}

fn loopback(uri: &str) -> Option<Loopback<'_>> {
    if uri
        .bytes()
        .any(|b| matches!(b, b'@' | b'\\' | b'#') || b.is_ascii_control())
        || uri.chars().any(char::is_whitespace)
        || ["%2F", "%2f", "%5C", "%5c"].iter().any(|e| uri.contains(e))
    {
        return None;
    }
    let authority = uri.strip_prefix("http://")?;
    let (host, after_host) = LOOPBACK_HOSTS
        .iter()
        .find_map(|host| authority.strip_prefix(host).map(|rest| (*host, rest)))?;
    let path_and_query = match after_host.strip_prefix(':') {
        Some(after_colon) => {
            let digits = after_colon.bytes().take_while(u8::is_ascii_digit).count();
            let (port, rest) = after_colon.split_at(digits);
            syntax::port(port)?;
            rest
        }
        None => after_host,
    };
    path_and_query.starts_with('/').then_some(Loopback {
        host,
        path_and_query,
    })
}
