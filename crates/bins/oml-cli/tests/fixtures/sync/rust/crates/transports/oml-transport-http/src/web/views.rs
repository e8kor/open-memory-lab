//! The user page's and the admin page's views and acts (ADR-0045 d9–d13,
//! d21): each view a `GET` selected by `view`, each act one `action` of the
//! page's closed set run as the session key its capacity needs. An act
//! either redirects back to its view (`303`) or answers a page that shows
//! what it produced once — a key value, a connect code, a served review
//! item — under `no-store`.

