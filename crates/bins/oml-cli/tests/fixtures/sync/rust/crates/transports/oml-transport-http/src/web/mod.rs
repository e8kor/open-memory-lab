//! The web pages' edge (ADR-0045): the six routes of [`WEB_ROUTES`], the
//! sealed sign-in cookie, the session cookie, the CSRF token and the
//! same-origin check, and the `web_sign_in` log line. The routes exist
//! only where the deployment sets `oauth.issuer` and at least one provider;
//! unset, each is an unknown path. A follower routes none of them.
//!
//! Nothing an agent holds reaches a page (ADR-0045 d7): a request carrying
//! an `Authorization` header is refused before anything else is read, and
//! the API and `/mcp` never read the `Cookie` header, so a session cookie
//! there is `auth.missing`. No log line holds a code, a `state`, a cookie
//! or an email (d14): the request line names the route template, and the
//! sign-in's own line names its provider, outcome, tenant and user.
//!
//! The person's session keys rest on ADR-0044's user roots, not built
//! here: the user page shows who the person is and every action but
//! `sign_out` answers the app's `not_built` refusal.

pub mod admin_read;
pub mod emails;
pub mod page;
pub mod views;
