//! Client identity: the form of a client ID metadata document's URL, the
//! fetched document's validation, and a dynamic registration's validation
//! (ADR-0031 d5; `docs/api/http-api.md`, "Client identity" and
//! `POST /oauth/register`). Every client is public; none holds a secret.

use serde_json::{Map, Value};

use super::redirect::admit_redirect_uri;
use super::refusal::{ClientInvalid, Refusal, Validation};
use super::syntax;

/// A fetch is cut at 16 KiB of bytes received (ADR-0031 d5).
pub const DOCUMENT_MAX_BYTES: usize = 16 * 1024;

const GRANT_TYPES: [&str; 2] = ["authorization_code", "refresh_token"];

/// What a `client_id` names: a client ID metadata document by its URL, or
/// a client a registration minted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientId<'a> {
    Document(DocumentUrl<'a>),
    Registered(&'a str),
}

/// A document URL in the admitted form, whose host the page names as the
/// one that verified the client (ADR-0031 d9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentUrl<'a> {
    url: &'a str,
    host_end: usize,
}

impl<'a> DocumentUrl<'a> {
    /// The URL, which is also the `client_id`.
    #[must_use]
    pub fn as_str(&self) -> &'a str {
        self.url
    }

    /// The URL's whole host, lower-case ASCII, never shortened.
    #[must_use]
    pub fn host(&self) -> &'a str {
        &self.url["https://".len()..self.host_end]
    }
}

/// A client's admitted metadata: the name the page shows and the redirect
/// URIs, each inside the allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientMetadata {
    pub client_name: String,
    pub redirect_uris: Vec<String>,
}

/// Which kind of client a `client_id` names (ADR-0031 d5). One that begins
/// `https:`, in any case, is a document URL and must be in the admitted
/// form, else `oauth.client_invalid` (`reason: client_id_form`):
///
/// - `https://`, then a host that is a lower-case ASCII DNS name (an
///   internationalized one as its A-label), with no userinfo and no port —
///   the default is the only one admitted, and a WHATWG serialization
///   omits it;
/// - a path beyond the root `/`, of RFC 3986 path characters and `%`
///   followed by two hex digits, never `%2F`, `%2f`, `%2E` or `%2e`, and
///   with no `.` or `..` segment;
/// - no query and no fragment.
///
/// A string this admits is its own WHATWG URL serialization, so one client
/// has one spelling; an `xn--` label is admitted without being decoded.
/// Any other `client_id` names a registered client, which the store looks
/// up.
pub fn classify_client_id(client_id: &str) -> Result<ClientId<'_>, Refusal> {
    let https = client_id
        .get(.."https:".len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https:"));
    if !https {
        return Ok(ClientId::Registered(client_id));
    }
    document_url(client_id)
        .map(ClientId::Document)
        .ok_or(Refusal::ClientInvalid(ClientInvalid::ClientIdForm))
}

fn document_url(url: &str) -> Option<DocumentUrl<'_>> {
    let after_scheme = url.strip_prefix("https://")?;
    let host_len = after_scheme.find('/')?;
    let (host, path) = after_scheme.split_at(host_len);
    (syntax::dns_host(host) && path != "/" && path_admitted(path)).then_some(DocumentUrl {
        url,
        host_end: "https://".len() + host_len,
    })
}

fn path_admitted(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let encoded = match bytes.get(i + 1..i + 3) {
                Some(&[hi, lo]) if hi.is_ascii_hexdigit() && lo.is_ascii_hexdigit() => [hi, lo],
                _ => return false,
            };
            if matches!(&encoded, b"2F" | b"2f" | b"2E" | b"2e") {
                return false;
            }
            i += 3;
        } else if b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&b) {
            i += 1;
        } else {
            return false;
        }
    }
    !path
        .split('/')
        .any(|segment| segment == "." || segment == "..")
}

/// A fetched client ID metadata document (ADR-0031 d5): JSON within the
/// 16 KiB bound, whose `client_id` is exactly its URL, carrying
/// `client_name` and a non-empty `redirect_uris`, with
/// `token_endpoint_auth_method` absent or `none` — each failure
/// `oauth.client_invalid` — and every redirect URI inside the allowlist,
/// else `oauth.redirect_refused`. Both are answered on the page.
pub fn validate_document(url: &DocumentUrl<'_>, body: &[u8]) -> Result<ClientMetadata, Refusal> {
    let invalid = |reason| Refusal::ClientInvalid(reason);
    if body.len() > DOCUMENT_MAX_BYTES {
        return Err(invalid(ClientInvalid::Fetch));
    }
    let document = json_object(body).ok_or(invalid(ClientInvalid::NotJson))?;
    if document.get("client_id").and_then(Value::as_str) != Some(url.as_str()) {
        return Err(invalid(ClientInvalid::ClientIdMismatch));
    }
    let client_name = non_empty_string(document.get("client_name"))
        .ok_or(invalid(ClientInvalid::MissingMember))?;
    let redirect_uris = non_empty_strings(document.get("redirect_uris"))
        .ok_or(invalid(ClientInvalid::MissingMember))?;
    if document
        .get("token_endpoint_auth_method")
        .is_some_and(|method| method != "none")
    {
        return Err(invalid(ClientInvalid::ClientAuthentication));
    }
    for uri in &redirect_uris {
        admit_redirect_uri(uri)?;
    }
    Ok(ClientMetadata {
        client_name,
        redirect_uris,
    })
}

/// A `POST /oauth/register` body (ADR-0031 d5; RFC 7591): a JSON object
/// with a non-empty `redirect_uris`, each inside the allowlist (else
/// `oauth.redirect_refused`, `invalid_redirect_uri`); a `client_name`, which
/// the page shows; `token_endpoint_auth_method` absent — read by RFC 7591
/// §2 as `client_secret_basic` and registered as `none` — or `none`;
/// `grant_types`, when sent, within `authorization_code` and
/// `refresh_token`; and `response_types`, when sent, `["code"]`. Every
/// other failure is `request.validation`, carried as
/// `invalid_client_metadata`. Members this does not name are ignored.
pub fn validate_registration(body: &[u8]) -> Result<ClientMetadata, Refusal> {
    let fault = |fault| Refusal::RequestValidation(fault);
    let registration = json_object(body).ok_or(fault(Validation::NotJsonObject))?;
    let redirect_uris = non_empty_strings(registration.get("redirect_uris"))
        .ok_or(fault(Validation::RedirectUrisMissing))?;
    for uri in &redirect_uris {
        admit_redirect_uri(uri)?;
    }
    let client_name = non_empty_string(registration.get("client_name"))
        .ok_or(fault(Validation::ClientNameMissing))?;
    if registration
        .get("token_endpoint_auth_method")
        .is_some_and(|method| method != "none")
    {
        return Err(fault(Validation::TokenEndpointAuthMethod));
    }
    let within = |member: &str, admitted: &[&str]| {
        registration.get(member).is_none_or(|value| {
            non_empty_strings(Some(value))
                .is_some_and(|values| values.iter().all(|v| admitted.contains(&v.as_str())))
        })
    };
    if !within("grant_types", &GRANT_TYPES) {
        return Err(fault(Validation::GrantTypes));
    }
    if !within("response_types", &["code"]) {
        return Err(fault(Validation::ResponseTypes));
    }
    Ok(ClientMetadata {
        client_name,
        redirect_uris,
    })
}

fn json_object(body: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice(body) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn non_empty_strings(value: Option<&Value>) -> Option<Vec<String>> {
    let items = value?.as_array().filter(|items| !items.is_empty())?;
    items
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}
