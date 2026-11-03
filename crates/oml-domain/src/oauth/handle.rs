//! The authorization request a page's form carries (ADR-0031 d9 step 1;
//! `docs/api/http-api.md`, `GET /oauth/authorize`). A valid authorize
//! request writes no row: its handle is the request itself, sealed by an
//! AEAD under a key the lead holds only in memory. This module is the
//! plaintext — its fields, its one canonical byte encoding and when an
//! opened handle names a live request. Sealing and opening are the edge's.

use crate::envelope::{EventTime, Sha256Hash};

use super::lifecycle::ATTEMPTS_PER_PAGE;
use super::refusal::{AuthorizationExpired, Refusal};

/// An authorization request is alive 10 minutes from its authorize request
/// (ADR-0031 d9, a design constant by d14).
pub const AUTHORIZATION_REQUEST_SECONDS: i64 = 600;

/// The encoding's version byte; a handle of any other version decodes to
/// nothing.
const VERSION: u8 = 1;

/// How the request's client was identified (ADR-0031 d5): the page names a
/// document client as verified by its host and a registered one as "name
/// not verified" (d9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestClientKind {
    Document,
    Registered,
}

/// What the sealed handle carries (ADR-0031 d9 step 1), each as accepted by
/// the authorize request's checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub client_id: String,
    pub kind: RequestClientKind,
    pub client_name: String,
    /// The document URL's whole host for a document client; `None` for a
    /// registered one.
    pub document_host: Option<String>,
    /// SHA-256 of the document or registration as received.
    pub metadata_digest: Sha256Hash,
    /// Where the authorization code is delivered, as presented or as the
    /// client's one registered URI.
    pub redirect_uri: String,
    /// Whether the authorize request sent `redirect_uri`; the token
    /// endpoint's `redirect_uri` rule turns on it.
    pub redirect_uri_sent: bool,
    /// The S256 PKCE challenge.
    pub code_challenge: String,
    /// Echoed verbatim on the redirect; `None` when the request sent none.
    pub state: Option<String>,
    /// `<issuer>/mcp`.
    pub resource: String,
    pub expires_at: EventTime,
    /// Wrong code entries left, one to [`ATTEMPTS_PER_PAGE`].
    pub attempts_left: u8,
}

impl AuthorizationRequest {
    /// The one byte encoding the handle seals: a version byte, then each
    /// field in declaration order — a string as its byte length (`u32`,
    /// big-endian) and its UTF-8 bytes, an optional string as a `0` or `1`
    /// byte before it, the kind and `redirect_uri_sent` as one byte each,
    /// the digest as its 32 bytes, the expiry as its 27 bytes, and the
    /// attempts as one byte.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![VERSION];
        put_str(&mut out, &self.client_id);
        out.push(match self.kind {
            RequestClientKind::Document => 0,
            RequestClientKind::Registered => 1,
        });
        put_str(&mut out, &self.client_name);
        put_opt(&mut out, self.document_host.as_deref());
        out.extend_from_slice(&self.metadata_digest.0);
        put_str(&mut out, &self.redirect_uri);
        out.push(u8::from(self.redirect_uri_sent));
        put_str(&mut out, &self.code_challenge);
        put_opt(&mut out, self.state.as_deref());
        put_str(&mut out, &self.resource);
        out.extend_from_slice(self.expires_at.as_str().as_bytes());
        out.push(self.attempts_left);
        out
    }

    /// The request [`AuthorizationRequest::encode`] wrote, and nothing
    /// else: every byte is read exactly once, a document client names its
    /// host and a registered one does not, and the attempts are one to
    /// [`ATTEMPTS_PER_PAGE`]; anything else is `None`.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        if r.byte()? != VERSION {
            return None;
        }
        let client_id = r.string()?;
        let kind = match r.byte()? {
            0 => RequestClientKind::Document,
            1 => RequestClientKind::Registered,
            _ => return None,
        };
        let client_name = r.string()?;
        let document_host = r.optional()?;
        let metadata_digest = Sha256Hash(r.take(32)?.try_into().ok()?);
        let redirect_uri = r.string()?;
        let redirect_uri_sent = r.flag()?;
        let code_challenge = r.string()?;
        let state = r.optional()?;
        let resource = r.string()?;
        let expires_at = EventTime::parse(std::str::from_utf8(r.take(27)?).ok()?).ok()?;
        let attempts_left = r.byte()?;
        let consistent = r.0.is_empty()
            && (kind == RequestClientKind::Document) == document_host.is_some()
            && (1..=ATTEMPTS_PER_PAGE).contains(&attempts_left);
        consistent.then_some(Self {
            client_id,
            kind,
            client_name,
            document_host,
            metadata_digest,
            redirect_uri,
            redirect_uri_sent,
            code_challenge,
            state,
            resource,
            expires_at,
            attempts_left,
        })
    }
}

/// The live request an opened handle names (ADR-0031 d9 step 1): `opened`
/// is the handle's plaintext, `None` when it did not open under the lead's
/// key. One that did not open or does not decode is
/// [`AuthorizationExpired::Unopened`]; one whose expiry is at or before
/// `now` is [`AuthorizationExpired::Lapsed`]. Both answer
/// `oauth.authorization_expired`.
pub fn open_request(
    opened: Option<&[u8]>,
    now: &EventTime,
) -> Result<AuthorizationRequest, Refusal> {
    let request =
        opened
            .and_then(AuthorizationRequest::decode)
            .ok_or(Refusal::AuthorizationExpired(
                AuthorizationExpired::Unopened,
            ))?;
    if now.unix_micros() >= request.expires_at.unix_micros() {
        return Err(Refusal::AuthorizationExpired(AuthorizationExpired::Lapsed));
    }
    Ok(request)
}

fn put_str(out: &mut Vec<u8>, value: &str) {
    let len = u32::try_from(value.len()).expect("a request field is bounded far below 4 GiB");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn put_opt(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            put_str(out, value);
        }
        None => out.push(0),
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }

    fn byte(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn flag(&mut self) -> Option<bool> {
        match self.byte()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    fn string(&mut self) -> Option<String> {
        let len = u32::from_be_bytes(self.take(4)?.try_into().ok()?);
        let bytes = self.take(usize::try_from(len).ok()?)?;
        String::from_utf8(bytes.to_vec()).ok()
    }

    fn optional(&mut self) -> Option<Option<String>> {
        match self.byte()? {
            0 => Some(None),
            1 => self.string().map(Some),
            _ => None,
        }
    }
}
