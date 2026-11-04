//! Application-layer records and use-case request/response shapes.

use oml_domain::access::{Grant, KeyDenial};
use oml_domain::budget::Budget;
use oml_domain::claim_edges::{EdgeField, LabelRefusal};
use oml_domain::name::{Name, NamedKind, Naming, Reference};
use oml_domain::oauth::{
    ConnectionState, KeyState, RefreshDecision, RefreshRow, Refusal, Token, TokenKind,
};
use oml_domain::vocab_edges::{CycleRefused, EdgeState};
use oml_domain::{
    ChainError, EventTime, FrozenOp, HeldBy, JcsError, LadderRefusal, LineageClosure, MemoryKind,
    MemoryStatus, Role, Sha256Hash, SignedEnvelope, TransitionError, VerdictError, VerifyError,
    WorkareaStatus,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cursor::CursorError;
use crate::ports::StoreError;

/// The tenant a provider-level key resolves under. Its row names no tenant
/// (postgres-schema.md `api_keys.tenant_id` NULL), and no tenant holds the
/// nil id, so every tenant-scoped read under it answers nothing and no
/// grant or denial can name the key (ADR-0025 d13).
pub const PROVIDER_TENANT: Uuid = Uuid::nil();

/// How a key authenticates, fixed for its life (postgres-schema.md
/// `api_keys.credential`; roles-and-keys.md connections; ADR-0031 d7): a
/// bearer key has a value, and a connector key has none and acts only
/// through a connection. A key is one or the other, never both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    Bearer,
    Connection,
}

impl Credential {
    /// The spelling of `key.issued.credential` and of the column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Credential::Bearer => "bearer",
            Credential::Connection => "connection",
        }
    }

    /// The credential a spelling names; `None` for any other string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bearer" => Some(Credential::Bearer),
            "connection" => Some(Credential::Connection),
            _ => None,
        }
    }
}

/// A stored API key row: everything about a bearer key is looked up by the
/// value's hash; the value itself is never stored (roles-and-keys.md key
/// format).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRecord {
    pub key_id: Uuid,
    /// The owning tenant; [`PROVIDER_TENANT`] for a provider-level key.
    pub tenant_id: Uuid,
    pub role: Role,
    pub principal: String,
    pub held_by: HeldBy,
    pub default_workarea: Option<Uuid>,
    /// The key that issued this one; `None` for the bootstrap ceremony's
    /// first admin key, which no key issues (postgres-schema.md
    /// `api_keys.issued_by`).
    pub issued_by: Option<Uuid>,
    /// `bearer` or `connection` (ADR-0031 d7).
    pub credential: Credential,
    /// `sha256:<hex>` of the full key string, prefix included. A connector
    /// key has none, and the store refuses one that carries a hash, so no
    /// value ever resolves it through `key_by_hash`; a bearer key has none
    /// only when it predates its hash, as a PoC key does (postgres-schema.md
    /// `api_keys_connector_has_no_hash`; ADR-0026 d11).
    pub key_hash: Option<String>,
    /// The key's name: unique in its tenant, operator keys sharing the
    /// provider's one namespace, and never released, a revoked key's
    /// included (roles-and-keys.md key names; ADR-0026 d1, d9, d10). Every
    /// issuance gives one ([`IssueKeyRequest::name`]), so `None` is only a
    /// key issued before names, until the lead names it (ADR-0026 d11).
    pub name: Option<Name>,
    /// The crew record named at issuance, fixed for the key's life; `None`
    /// for a key with no crew (ADR-0026 d8).
    pub crew_id: Option<Uuid>,
    /// The `at` of the key's `key.revoked`; the row stays as history, so a
    /// revoked key reads back with it. `None` for a live key, as every
    /// issuance folds one (postgres-schema.md `api_keys.revoked_at`;
    /// ADR-0026 d10).
    pub revoked_at: Option<EventTime>,
}

/// A connect code as `connection.code_issued` mints it, for the commit's
/// tenant: the code is shown once and never stored, so the row holds only
/// the SHA-256 of its twelve decoded symbols, which no journal payload
/// carries (roles-and-keys.md connect codes; ADR-0031 d8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectCode {
    pub code_id: Uuid,
    /// The connector key the code opens a connection for.
    pub key_id: Uuid,
    pub code_hash: Sha256Hash,
    pub expires_at: EventTime,
}

/// A stored connect code (postgres-schema.md `connect_codes`; ADR-0031 d8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectCodeRecord {
    pub code_id: Uuid,
    pub key_id: Uuid,
    pub code_hash: Sha256Hash,
    pub expires_at: EventTime,
    /// The seq of its `connection.code_issued`.
    pub issued_seq: u64,
    /// Its first correct entry on the authorization page.
    pub redeemed_at: Option<EventTime>,
    /// The seq of the `connection.code_issued`, `connection.revoked` or
    /// `key.revoked` that voided it.
    pub voided_seq: Option<u64>,
}

/// How a connection's client was identified (postgres-schema.md
/// `oauth_connections.client_kind`; ADR-0031 d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    /// A client ID metadata document, fetched and verified.
    Cimd,
    /// A dynamic client registration.
    Dcr,
}

impl ClientKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ClientKind::Cimd => "cimd",
            ClientKind::Dcr => "dcr",
        }
    }
}

/// A stored connection: one OAuth client's standing authority to act as one
/// connector key at `/mcp`. Connect creates it `authorized`, the token
/// exchange's `connection.opened` opens it, and it ends `revoked`, once
/// (postgres-schema.md `oauth_connections`; roles-and-keys.md connections;
/// ADR-0031 d7, d10, d12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRecord {
    pub connection_id: Uuid,
    pub key_id: Uuid,
    /// The connect code it came from; one code yields at most one.
    pub code_id: Uuid,
    pub client_id: String,
    pub client_kind: ClientKind,
    /// SHA-256 of the client's document or registration.
    pub client_metadata_digest: Sha256Hash,
    pub redirect_uri: String,
    /// Whether the authorize request named `redirect_uri`.
    pub redirect_uri_sent: bool,
    /// `<issuer>/mcp` at Connect, which its tokens are bound to.
    pub resource: String,
    /// The S256 PKCE challenge.
    pub code_challenge: String,
    /// The authorization code's SHA-256, kept after redemption so a second
    /// redemption is recognized.
    pub auth_code_hash: Option<Sha256Hash>,
    pub auth_code_expires_at: Option<EventTime>,
    pub auth_code_redeemed_at: Option<EventTime>,
    pub state: ConnectionState,
    /// The seq of its `connection.opened`.
    pub opened_seq: Option<u64>,
    /// The seq of its `connection.revoked`, or of the `connection.opened`
    /// that replaced it.
    pub revoked_seq: Option<u64>,
    pub created_at: EventTime,
}

/// An OAuth client the authorization server knows, a cached client ID
/// metadata document or a dynamic registration: a provider-tenant row that
/// holds no tenant content (postgres-schema.md `oauth_clients`; ADR-0031
/// d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthClientRecord {
    /// A metadata document's https URL, or a server-minted id.
    pub client_id: String,
    pub kind: ClientKind,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    /// SHA-256 of the document or registration as received.
    pub metadata_digest: Sha256Hash,
    /// A cached document's fetch; it is kept at most 24 hours from it.
    pub fetched_at: Option<EventTime>,
    /// A cached document's `Cache-Control` lifetime from `fetched_at`, in
    /// seconds, at most 24 hours; past it the copy is served only when a
    /// fresh fetch fails or a fetch cap is full (ADR-0031 d5).
    pub max_age: Option<u32>,
    /// A cached document's last use, the cache's eviction order.
    pub last_used_at: Option<EventTime>,
    /// Stamped by the first Connect that names the client; eviction and the
    /// removal of an unused registration read it, and a refetch keeps it
    /// (ADR-0031 d5, d9).
    pub connected_at: Option<EventTime>,
    pub created_at: EventTime,
}

/// A client's self-description as admitted: a fetched client ID metadata
/// document or a dynamic registration, before the store keeps it
/// (postgres-schema.md `oauth_clients`; ADR-0031 d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientDescription {
    /// The document's https URL, or the server-minted id of a registration.
    pub client_id: String,
    pub client_name: String,
    /// Each inside the redirect allowlist (ADR-0031 d6).
    pub redirect_uris: Vec<String>,
    /// SHA-256 of the document or registration as received.
    pub metadata_digest: Sha256Hash,
}

/// What caching a fetched document did (ADR-0031 d5): a full cache never
/// refuses a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentCaching {
    /// The document is cached, refreshed in place or as a new row; a new
    /// row in a full cache evicted the named least recently used client
    /// that had not connected.
    Cached { evicted: Option<String> },
    /// The cache is full and every cached client has connected: nothing was
    /// written, and the fetched document serves its request uncached.
    NotCached,
}

/// An authorization request whose connect code verified: a provider-tenant
/// row found by the SHA-256 of its confirmation handle, never the handle,
/// and naming the code's tenant, code and key as plain ids
/// (postgres-schema.md `oauth_authorizations`; ADR-0031 d9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRecord {
    pub authorization_id: Uuid,
    /// SHA-256 of the confirmation's 256-bit handle.
    pub handle_hash: Sha256Hash,
    pub client_id: String,
    pub client_kind: ClientKind,
    pub metadata_digest: Sha256Hash,
    pub redirect_uri: String,
    pub redirect_uri_sent: bool,
    /// The S256 PKCE challenge.
    pub code_challenge: String,
    /// Echoed verbatim.
    pub state: Option<String>,
    /// The sealed handle's expiry.
    pub expires_at: EventTime,
    pub code_id: Uuid,
    pub code_tenant_id: Uuid,
    pub key_id: Uuid,
}

/// The query of `GET /oauth/authorize`, each parameter as received after its
/// one form decoding, `None` where absent (http-api.md; ADR-0031 d4).
/// `repeated` is whether any parameter was sent more than once (RFC 6749
/// §3.1), which the edge sees and the bounds refuse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorizeParams {
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub response_type: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub state: Option<String>,
    pub resource: Option<String>,
    pub scope: Option<String>,
    pub repeated: bool,
}

/// The connect-code page of a valid authorize request (ADR-0031 d9 step 1):
/// the sealed handle its form carries, and what the page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizePage {
    /// The authorization request sealed under the lead's in-memory key.
    pub handle: String,
    pub client_name: String,
    /// A document client's whole host, shown as "verified by"; `None` for a
    /// registered client, whose name is marked "name not verified".
    pub verified_by: Option<String>,
    /// The redirect URI's host.
    pub redirect_host: String,
    pub attempts_left: u8,
    pub expires_at: EventTime,
}

/// What `GET /oauth/authorize` answers (ADR-0031 d4, d9 step 1;
/// errors.md "`oauth` — connections").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizeOutcome {
    /// The page for a live authorization request.
    Page(AuthorizePage),
    /// A refusal of a bound, the client or the redirect URI: a `400` page,
    /// never a redirect, since neither the redirect URI nor the `state` was
    /// accepted.
    PageRefusal(Refusal),
    /// A refusal after the client and the redirect URI were accepted: a
    /// redirect to `redirect_uri` with the refusal's RFC `error`
    /// ([`Refusal::authorize_error`]), its description, the `state` echoed
    /// and `iss` (RFC 9207 §2).
    RedirectRefusal {
        error: Refusal,
        redirect_uri: String,
        state: Option<String>,
        iss: String,
    },
}

/// What Connect adds to an authorization request's row to make its
/// `authorized` connection: the rest of the connection is copied from the
/// row the confirmation deletes (postgres-schema.md "connections
/// (ADR-0031)"; ADR-0031 d9, d10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectGrant {
    pub connection_id: Uuid,
    /// `<issuer>/mcp` at Connect.
    pub resource: String,
    /// SHA-256 of the authorization code Connect mints.
    pub auth_code_hash: Sha256Hash,
    pub auth_code_expires_at: EventTime,
    /// Connect's time: the connection's `created_at` and the client's
    /// `connected_at` stamp.
    pub at: EventTime,
}

/// What Connect's one transaction did (postgres-schema.md "connections
/// (ADR-0031)"; roles-and-keys.md connections; ADR-0031 d8, d9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    /// The request's row is deleted, its client stamped `connected_at`
    /// where it was null, and the connection created `authorized` under
    /// `tenant_id`, the code's tenant.
    Authorized {
        tenant_id: Uuid,
        connection: Box<ConnectionRecord>,
    },
    /// No row holds the handle's digest: Connect or Cancel used it, or it
    /// lapsed and was removed. Answered `oauth.authorization_expired`.
    NoRequest,
    /// The request's connect code was voided since its entry, by a newer
    /// code or a connection revocation of its key. Nothing changed;
    /// answered `oauth.authorization_expired` (`code_voided`).
    CodeVoided,
}

/// A granted workarea as the confirmation names it: by its name, with the
/// grant's role (ADR-0031 d9 step 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationGrant {
    pub workarea_id: Uuid,
    pub workarea: Name,
    pub role: Role,
}

/// What the confirmation shows a correct code's holder before the
/// connection opens (http-api.md "The authorization page"; ADR-0031 d9
/// step 2): the tenant by the name the operator gave it, the key's name,
/// principal and role, every granted workarea with its grant's role, and
/// the workareas each verb-kind sentence names, from the key's effective
/// access at each granted workarea. The page builds its sentences from
/// these; "Everything it writes is held…" follows from a non-empty
/// `writes_in` or `proposes_in`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationView {
    /// The confirmation form's 256-bit handle, in base62; the store holds
    /// only its SHA-256.
    pub handle: String,
    pub tenant_name: Name,
    /// `None` only for a key issued before names, which the lead names
    /// before it serves (ADR-0026 d11).
    pub key_name: Option<Name>,
    pub principal: String,
    pub role: Role,
    /// In grant order.
    pub grants: Vec<ConfirmationGrant>,
    /// Granted workareas where writer is effective: "Claude can remember
    /// and learn as this agent in …".
    pub writes_in: Vec<Name>,
    /// Granted workareas where reader is effective: "Claude can recall and
    /// read as this agent in …".
    pub reads_in: Vec<Name>,
    /// Granted workareas where dreamer is effective, its propose class:
    /// "Claude can propose edits and edges as this agent in …".
    pub proposes_in: Vec<Name>,
    /// Granted workareas whose tenant policy permits `self_signed` rows.
    /// No journalled tenant policy permits them in this build, so it is
    /// always empty here.
    pub self_signed_in: Vec<Name>,
}

/// What a code entry on the authorization page answers (http-api.md
/// `POST /oauth/authorize` step 2; ADR-0031 d8, d9 step 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodeEntryOutcome {
    /// A correct code of a live key: spent, its request's row written.
    Confirmation(Box<ConfirmationView>),
    /// Any other entry — unknown, malformed, expired, voided, spent, or a
    /// revoked or expired key's code alike: the form again,
    /// `400 oauth.connect_code_invalid`, under a new sealed handle carrying
    /// one attempt fewer.
    WrongCode { handle: String, attempts_left: u8 },
    /// The fifth wrong entry, or an entry on a handle that opens to a
    /// request past its ten minutes: the request ends, redirected to
    /// `redirect_uri` with `error` ([`Refusal::authorize_error`],
    /// `access_denied`), the `state` echoed and `iss`. The code is not
    /// read, so a lapsed handle spends nothing and counts no failure.
    Denied {
        error: Refusal,
        redirect_uri: String,
        state: Option<String>,
        iss: String,
    },
}

/// The confirmation's two buttons (`action=connect|cancel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    Connect,
    Cancel,
}

/// What a confirmation answers: a `303` to the request's redirect URI
/// (http-api.md `POST /oauth/authorize` step 3; ADR-0031 d9 step 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmOutcome {
    /// Connect: the connection is `authorized`; the redirect carries the
    /// single-use authorization code, alive 60 seconds, the `state`
    /// verbatim and `iss`. The code appears here and nowhere else.
    Connected {
        redirect_uri: String,
        code: String,
        state: Option<String>,
        iss: String,
    },
    /// Cancel: the redirect carries `error=access_denied`, the `state` and
    /// `iss`.
    Cancelled {
        redirect_uri: String,
        state: Option<String>,
        iss: String,
    },
    /// A handle that names a row whose request outlived its ten minutes or
    /// whose code is voided: `oauth.authorization_expired`, redirected to
    /// the row's `redirect_uri` with `error` ([`Refusal::authorize_error`],
    /// `access_denied`), the `state` echoed and `iss` (errors.md: a page
    /// only when the handle is unknown). Nothing is connected.
    Denied {
        error: Refusal,
        redirect_uri: String,
        state: Option<String>,
        iss: String,
    },
}

/// A row a pre-tenant lookup found by a digest, with the tenant whose
/// `oml.tenant_id` everything after it runs under (postgres-schema.md
/// "connections (ADR-0031)"; ADR-0031 d8, d10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantRow<T> {
    pub tenant_id: Uuid,
    pub row: T,
}

/// A token as its mint hands it to the store: the SHA-256 of the token
/// string and its expiry, never the token (postgres-schema.md
/// `oauth_tokens`; ADR-0031 d10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedToken {
    pub token_hash: Sha256Hash,
    /// Set at minting; rotation never changes it.
    pub expires_at: EventTime,
}

/// The access and refresh tokens one exchange or one refresh mints
/// (ADR-0031 d10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenPair {
    pub access: MintedToken,
    pub refresh: MintedToken,
}

/// A stored token row (postgres-schema.md `oauth_tokens`; ADR-0031 d10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRecord {
    pub token_hash: Sha256Hash,
    pub connection_id: Uuid,
    pub kind: TokenKind,
    pub expires_at: EventTime,
    /// A refresh token's rotation: presenting it after this is reuse.
    pub rotated_at: Option<EventTime>,
    pub created_at: EventTime,
}

/// What a refresh reads under its connection's row lock, before its
/// conditional update: the presented refresh token's rotation and expiry
/// at the refresh's time, its connection's state, client and resource, and
/// its key's state (postgres-schema.md, redemption and rotation are
/// conditional updates; ADR-0031 d10, d11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshFacts {
    pub connection_id: Uuid,
    pub connection: ConnectionState,
    pub client_id: String,
    pub resource: String,
    pub key: KeyState,
    pub rotated: bool,
    pub expired: bool,
}

impl RefreshFacts {
    /// The facts as the domain's refresh rule reads them.
    #[must_use]
    pub fn row(&self) -> RefreshRow<'_> {
        RefreshRow {
            rotated: self.rotated,
            expired: self.expired,
            connection: self.connection,
            client_id: &self.client_id,
            resource: &self.resource,
            key: self.key,
        }
    }
}

/// What a refresh's one transaction did: the decision its caller's rule
/// took on the facts read under the lock, and those facts, `None` for a
/// digest no refresh row of the tenant holds. Only
/// [`RefreshDecision::Rotate`] wrote anything: the presented token's
/// `rotated_at` and the new pair (ADR-0031 d10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshOutcome {
    pub decision: RefreshDecision,
    pub facts: Option<RefreshFacts>,
}

/// How many rows one sweep removed or cleared (postgres-schema.md, the
/// removal paragraph; ADR-0031 d9, d10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// `authorized` connections whose authorization code lapsed unexchanged.
    pub connections_removed: u64,
    /// Token rows past their own expiry, or of a revoked connection.
    pub tokens_removed: u64,
    /// Revoked connections whose `auth_code_hash` was cleared.
    pub codes_cleared: u64,
    /// Authorization requests 24 hours past their expiry.
    pub authorizations_removed: u64,
}

/// Client authentication a token request sent, which no client here may
/// send, since every client is public (http-api.md `POST /oauth/token`;
/// ADR-0031 d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientAuthentication {
    /// An `Authorization` header, by the scheme it used, answered `401`
    /// with `WWW-Authenticate` naming that scheme.
    Header { scheme: String },
    /// A `client_secret` in the body, answered `400`.
    BodySecret,
}

/// A `grant_type=authorization_code` form, each parameter after its one
/// form decoding; an absent `code` or a repeated parameter is the edge's
/// `request.validation` (http-api.md `POST /oauth/token`; ADR-0031 d10).
/// Its `Debug` names no secret.
#[derive(Clone, PartialEq, Eq)]
pub struct ExchangeForm {
    pub code: String,
    pub client_id: String,
    pub redirect_uri: Option<String>,
    pub code_verifier: String,
    pub resource: Option<String>,
    pub client_authentication: Option<ClientAuthentication>,
}

impl std::fmt::Debug for ExchangeForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExchangeForm")
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .field("resource", &self.resource)
            .field("client_authentication", &self.client_authentication)
            .finish_non_exhaustive()
    }
}

/// A `grant_type=refresh_token` form (http-api.md `POST /oauth/token`;
/// ADR-0031 d10, d11). Its `Debug` names no secret.
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshForm {
    pub refresh_token: String,
    pub client_id: String,
    pub resource: Option<String>,
    pub client_authentication: Option<ClientAuthentication>,
}

impl std::fmt::Debug for RefreshForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshForm")
            .field("client_id", &self.client_id)
            .field("resource", &self.resource)
            .field("client_authentication", &self.client_authentication)
            .finish_non_exhaustive()
    }
}

/// The pair one exchange or one refresh answers: `token_type` is `Bearer`
/// and `scope` `mcp`, each fixed, and `expires_in` the access token's
/// lifetime in seconds. A [`Token`]'s `Debug` names only its kind
/// (http-api.md `POST /oauth/token`; ADR-0031 d10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedTokens {
    pub access_token: Token,
    pub refresh_token: Token,
    pub expires_in: u64,
}

/// A dynamic registration as `201` answers it: a server-minted
/// `client_id`, no secret and no registration access token, its
/// `token_endpoint_auth_method` always `none` (http-api.md
/// `POST /oauth/register`; ADR-0031 d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredClient {
    pub client_id: String,
    /// When the registration was stored: its `client_id_issued_at`.
    pub issued_at: EventTime,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    pub token_endpoint_auth_method: &'static str,
}

/// A stored grant row: the domain grant and its epoch, the journal seq of
/// the `grant.issued` that wrote it (postgres-schema.md `grants`;
/// workareas.md permission epoch; ADR-0025 d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRecord {
    pub grant: Grant,
    pub epoch: u64,
}

/// A stored global per-key denial: the domain denial, the key that
/// journalled it, and its epoch, the journal seq of its `key.denied`
/// (postgres-schema.md `key_denials`; ADR-0025 d2, d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenialRecord {
    pub denial: KeyDenial,
    pub created_by: Uuid,
    pub epoch: u64,
}

/// What effective access is computed from for one key, read in one
/// snapshot: the floor of its permission epoch, its grants, its global
/// denials in force and the epochs of the lifts that ended the others, each
/// list in epoch order (workareas.md effective access and permission epoch;
/// ADR-0025 d1, d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAccess {
    /// The journal seq of the key's `key.issued`.
    pub issued_seq: u64,
    pub grants: Vec<GrantRecord>,
    /// The denials no `key.denial_lifted` has lifted: only these mask.
    pub denials: Vec<DenialRecord>,
    /// The journal seq of each `key.denial_lifted` on the key. A lifted
    /// denial masks nothing and its row stays as history; its lift still
    /// moves the permission epoch (event-catalog.md `key.denial_lifted`).
    pub lift_epochs: Vec<u64>,
}

impl KeyAccess {
    /// The key's permission epoch over what is built: the latest of its
    /// `key.issued`, `grant.issued`, `key.denied` and `key.denial_lifted`
    /// seqs (workareas.md permission epoch; ADR-0025 d14).
    #[must_use]
    pub fn permission_epoch(&self) -> u64 {
        self.grants
            .iter()
            .map(|g| g.epoch)
            .chain(self.denials.iter().map(|d| d.epoch))
            .chain(self.lift_epochs.iter().copied())
            .fold(self.issued_seq, u64::max)
    }
}

/// A workarea's crew: recorded, not operated — OML never contacts the
/// external manager (openapi.yaml `CrewRecord`; event-catalog.md
/// `workarea.create`). An absent member is stored and served absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crew {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charter_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_manager: Option<String>,
}

/// A workarea node: id, parent, name, its rung on the status ladder and
/// its crew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkareaRecord {
    pub workarea_id: Uuid,
    pub tenant_id: Uuid,
    pub parent_id: Option<Uuid>,
    /// Unique in the tenant across the whole tree and never changed; the
    /// root's is `universe` (ADR-0026 d1, d6).
    pub name: Name,
    /// The node's rung on the status ladder, as its row stores it: one
    /// column, written at create and by a ratified transition alone
    /// (ADR-0009 d1; workareas.md the status ladder). `workarea.create`
    /// stores `development`, and a tenant's bootstrap stores `committed` on
    /// the universe root, which never moves (http-api.md workareas).
    pub status: WorkareaStatus,
    /// The crew object a PoC `workarea.create` copied in, kept as the
    /// history the naming of rows that predate names reads it from; `None`
    /// on every workarea created since crews are records (ADR-0026 d8, d11).
    pub crew: Option<Crew>,
    /// The crew record the workarea names; `None` when it names none
    /// (ADR-0026 d8).
    pub crew_id: Option<Uuid>,
}

/// Where a status-ladder proposal stands: `pending` until a second identity
/// ratifies it, then `ratified` for good (ADR-0009 d3; postgres-schema.md
/// `workarea_transitions.state`). A proposal made stale by another's
/// ratification stays `pending` as history; the status read, not the row,
/// tells it apart (workareas.md, the status read lists only live proposals).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionState {
    Pending,
    Ratified,
}

impl TransitionState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TransitionState::Pending => "pending",
            TransitionState::Ratified => "ratified",
        }
    }

    /// The state an exact word spells; `None` for any other string.
    #[must_use]
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "pending" => Some(TransitionState::Pending),
            "ratified" => Some(TransitionState::Ratified),
            _ => None,
        }
    }
}

/// One status-ladder proposal as its `workarea_transitions` row holds it:
/// the node, the step from one rung to the next, the proposer's key and
/// principal, and the ratifier's key once a second identity ratified it
/// (ADR-0009 d3; workareas.md, a pending proposal is a row). The journal
/// seqs of the proposal and the ratification are the store's, written from
/// the events the folds commit with, never carried here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionRecord {
    /// The events' `transition_id`.
    pub transition_id: Uuid,
    pub tenant_id: Uuid,
    pub workarea_id: Uuid,
    pub from: WorkareaStatus,
    pub to: WorkareaStatus,
    pub state: TransitionState,
    pub proposer_key_id: Uuid,
    /// The principal the proposing key's row holds, which the proposal's
    /// fold checks; the ratifying transaction refuses this principal under
    /// any of its keys, not only the proposing key (ADR-0010 R2).
    pub proposer_principal: String,
    /// `None` while pending; the ratifying key once ratified.
    pub ratifier_key_id: Option<Uuid>,
}

/// A workarea's status read (http-api.md workareas, `GET
/// /v2/workareas/{id}/status`): the rung its row stores, the review policy
/// object in force, and its live proposals in proposal order, the pending
/// rows whose `from` is that rung; a proposal another's ratification made
/// stale is not listed (workareas.md, the status read lists only live
/// proposals; ADR-0009 d3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkareaStatusView {
    pub workarea_id: Uuid,
    pub status: WorkareaStatus,
    /// The policy a verdict's ratification and a transition's
    /// `policy_snapshot` read, so the three never disagree.
    pub policy: serde_json::Value,
    pub pending: Vec<TransitionView>,
}

/// One live proposal as the status read serves it, the openapi
/// `Transition` shape: never the proposer's principal nor the tenant, and
/// the proposing key only to a key in the proposer's own key set, as the
/// tail serves it (workareas.md, the status read names the proposing key
/// only to its own key set; http-api.md#events-tail; ADR-0025 d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionView {
    pub transition_id: Uuid,
    pub workarea_id: Uuid,
    pub from: WorkareaStatus,
    pub to: WorkareaStatus,
    pub state: TransitionState,
    /// The proposing key when it is in the reader's own key set; `None`
    /// to every other reader, an admin key included.
    pub proposed_by_key_id: Option<Uuid>,
}

/// A crew record: named, recorded and not operated — OML never contacts the
/// external manager (workareas.md crews; postgres-schema.md `crews`;
/// ADR-0026 d8). The name never changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrewRecord {
    pub crew_id: Uuid,
    pub tenant_id: Uuid,
    /// Unique in the tenant (ADR-0026 d1).
    pub name: Name,
    pub charter_uri: Option<String>,
    pub external_manager: Option<String>,
}

/// A node as `workarea.create` committed it, with the path derived from the
/// stored ancestry: the same derivation the workarea read serves, so the
/// create and the read never name one node two ways (ADR-0025 d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedWorkarea {
    pub workarea: WorkareaRecord,
    pub path: String,
    /// The crew record the create named, as the workarea read serves it.
    pub crew: Option<CrewRecord>,
}

/// One workarea as the calling key may see it (http-api.md workareas;
/// ADR-0025 d14): the node, its path derived from the `parent_id` names
/// from `universe`, the crew record it names, the immediate children the
/// key can read, the verbs its effective access confers there, and its
/// permission epoch. No workarea edge can exist in this build, so the read
/// serves no edges. An admin key reads it by its key role: every child, no
/// effective verb (ADR-0026 d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkareaInfo {
    pub workarea: WorkareaRecord,
    pub path: String,
    /// The crew record the workarea names, read from the store; `None` when
    /// it names none (ADR-0026 d8).
    pub crew: Option<CrewRecord>,
    /// Ordered by name in byte order, then id; a child the key cannot read
    /// is absent, not marked.
    pub children: Vec<WorkareaRecord>,
    /// The effective-verbs table of roles-and-keys.md over the roles
    /// effective on the node, in table order; empty for an admin key, which
    /// holds no grant role.
    pub my_effective_verbs: Vec<&'static str>,
    /// The key's permission epoch as its access was read for this request
    /// (workareas.md permission epoch).
    pub permission_epoch: u64,
}

/// One node of a workarea tree read, with its derived path and the crew
/// record it names, if any (http-api.md workareas, `GET
/// /v2/workareas/{id}/tree`; ADR-0026 d8, d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub workarea: WorkareaRecord,
    pub path: String,
    pub crew: Option<CrewRecord>,
}

/// One page of a workarea tree read, in path order, and the cursor of the
/// next page, `None` once the tree is exhausted (http-api.md workareas and
/// pagination; ADR-0026 d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreePage {
    pub items: Vec<TreeNode>,
    pub next_cursor: Option<String>,
}

/// The rows of one tenant's chain that have no name yet: the tenant's own
/// row, workareas and keys. Every row a creation writes is named, so on the
/// names schema these are the rows that predate names (ADR-0026 d9, d11;
/// postgres-schema.md names and tenants).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnnamedRows {
    pub tenant: bool,
    /// In id order.
    pub workareas: Vec<Uuid>,
    /// In id order.
    pub keys: Vec<Uuid>,
}

impl UnnamedRows {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.tenant && self.workareas.is_empty() && self.keys.is_empty()
    }
}

/// One chain the lead named at its start: the seq of its
/// `system.names_assigned`, what that event assigns, and the denials on
/// admin keys rule 5 lifted in the same commit, in journal order (ADR-0026
/// d11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignedNames {
    pub tenant_id: Uuid,
    pub journal_seq: u64,
    pub naming: Naming,
    pub lifted_denials: Vec<Uuid>,
}

/// Why the lead did not name a chain's rows that predate names; the chain
/// journals nothing (http-api.md rows created before names; ADR-0026 d11).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NamingRefusal {
    /// The chain already journals `system.names_assigned` at `seq`, yet a
    /// row has no name: the names of a chain are assigned once.
    #[error(
        "the chain journals system.names_assigned at seq {seq}, yet a row has no name (postgres-schema.md names and tenants)"
    )]
    AlreadyAssigned { seq: u64 },
    /// A row with no name has no creation event in the chain to name it by.
    #[error("the {} row {id} has no name and no creation event in the chain", .kind.as_str())]
    NoCreationEvent { kind: NamedKind, id: Uuid },
}

/// The metadata row of one memory version. The body lives here, never in the
/// journal — journal payloads are digest-only (ADR-0002 d1, admission.md).
/// The claim fields follow postgres-schema.md `memories` (ADR-0025 d6, d10,
/// d11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRecord {
    pub memory_id: Uuid,
    pub tenant_id: Uuid,
    pub workarea_id: Uuid,
    /// Names one live claim per workarea; its versions share it
    /// (audit-and-lineage.md supersession chains).
    pub claim_key: String,
    /// The ordinal within the `claim_key`: 1 for a new claim, 1 + the
    /// greatest version of the key in any state for a successor (ADR-0025
    /// d10).
    pub version: u32,
    pub kind: MemoryKind,
    /// The NFC-normalized claim text.
    pub body: String,
    pub body_digest: String,
    /// UTF-8 octets of the NFC body, the count budget tokens derive from
    /// (retrieval.md token budgets; ADR-0025 d6).
    pub body_bytes: u64,
    pub status: MemoryStatus,
    /// Whether this version is its claim's served tip: set when a version
    /// is promoted, cleared when a ratified successor replaces it. A held,
    /// rejected or replaced version is never the tip (ADR-0025 d10).
    pub is_tip: bool,
    /// The `at` of the event that promoted this version; `None` until then.
    pub promoted_at: Option<EventTime>,
    /// A retirement version: ratified as tip, it retires the claim (ADR-0025
    /// d11).
    pub retirement: bool,
    /// The version a supersession replaces; `None` for a version 1.
    pub replaces: Option<Uuid>,
    pub author_principal: String,
    pub author_key_id: Uuid,
    pub derives_from: Vec<Uuid>,
    /// The row's lineage closure, flattened: the author plus every declared
    /// ancestor's closure (roles-and-keys.md lineage-closure exclusion).
    pub lineage_principals: Vec<String>,
    pub lineage_key_ids: Vec<Uuid>,
    pub scan_finding: Option<String>,
    pub created_at: EventTime,
}

/// What a capture reads of one `claim_key` in one workarea before it commits
/// (audit-and-lineage.md supersession chains; ADR-0025 d10, d18).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClaimKeyState {
    /// The greatest `version` of any row of the key, whatever its state;
    /// `None` when no row carries the key.
    pub greatest_version: Option<u32>,
    /// Whether a held, promoted or demoted version holds the key: a new
    /// claim (version 1) collides then. A key whose every version is
    /// rejected or erased is released.
    pub live: bool,
}

/// A version a supersession replaces, as served: its id and its ordinal
/// (openapi.yaml `MemorySummary.replaces`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRef {
    pub artifact_id: Uuid,
    pub version: u32,
}

/// One memory version as the calling key is served it (http-api.md get and
/// recall; ADR-0025 d3, d12): the row with `derives_from` cut to the
/// ancestors the key can read, the version it replaces only where the key
/// can read that version too, and the workarea's derived path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedMemory {
    pub memory: MemoryRecord,
    pub replaces: Option<VersionRef>,
    /// The `parent_id` names from the root to the row's workarea, joined
    /// with `.` (ADR-0025 d14); `None` for a node the tenant does not hold.
    pub workarea_path: Option<String>,
}

impl MemoryRecord {
    /// The domain closure used by `check_reviewer` (ADR-0001 R2).
    #[must_use]
    pub fn lineage_closure(&self) -> LineageClosure {
        let mut closure = LineageClosure::new();
        for principal in &self.lineage_principals {
            closure.add_principal(principal.clone());
        }
        for key_id in &self.lineage_key_ids {
            closure.add_key_id(*key_id);
        }
        closure
    }
}

// ---------------------------------------------------------------------------
// Use-case requests and responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BootstrapRequest {
    /// The tenant's name, unique in the deployment (ADR-0026 d1).
    pub tenant_name: Name,
    /// The name of the ceremony's admin key, the tenant's first key, which
    /// the claim names (http-api.md bootstrap; ADR-0026 d9).
    pub admin_key_name: Name,
    pub admin_principal: String,
}

/// Show-once: the value is returned exactly here and never stored
/// (roles-and-keys.md key lifecycle), and only after the key's last
/// `grant.issued` committed. The rest is the key as `key.issued` journalled
/// it and its grants as stored, ids and epochs included (ADR-0025 d2).
#[derive(Debug, Clone)]
pub struct IssuedKey {
    pub key_id: Uuid,
    /// The bearer key's value; empty for a connector key, which has none
    /// and whose [`IssuedKey::connect_code`] stands in its place (ADR-0031
    /// d7).
    pub value: String,
    pub role: Role,
    pub principal: String,
    /// The issuer's attestation as journalled; `None` when none was sent.
    pub held_by: Option<HeldBy>,
    /// The sent default, else the first grant's node; `None` for a
    /// zero-grant key.
    pub default_workarea: Option<Uuid>,
    /// The key's name as journalled ([`IssueKeyRequest::name`]).
    pub name: Name,
    /// The crew record named at issuance, fixed for the key's life
    /// (ADR-0026 d8).
    pub crew_id: Option<Uuid>,
    pub access: KeyAccess,
    /// A connector key's connect code, returned only after the key's last
    /// `grant.issued` and its `connection.code_issued` committed; `None` for
    /// a bearer key (roles-and-keys.md issuance order, step 7; ADR-0031 d7).
    pub connect_code: Option<ConnectCodeIssued>,
}

/// A connect code as its minting answers it: `{code_id, code, expires_at,
/// connector_url}` (http-api.md `POST /v2/keys/{key_id}/connect-codes`;
/// openapi.yaml `ConnectCode`; ADR-0031 d7, d8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectCodeIssued {
    pub code_id: Uuid,
    /// The twelve symbols in three groups of four, shown exactly once:
    /// `Some` in a fresh answer and `None` in the response an idempotency
    /// record stores, so no record holds a code (ADR-0031 d7, d8).
    pub code: Option<String>,
    pub expires_at: EventTime,
    /// `<issuer>/mcp`, from `oauth.issuer`, the URL to enter as the custom
    /// connector.
    pub connector_url: String,
}

#[derive(Debug, Clone)]
pub struct BootstrapResult {
    pub tenant_id: Uuid,
    pub root_workarea_id: Uuid,
    pub admin_key: IssuedKey,
}

/// `POST /v2/keys`, validated in the issuance order (roles-and-keys.md
/// grant admissibility and refusal order; ADR-0025 d2).
///
/// Every workarea and crew reference takes an id or a name, resolved in the
/// issuer's tenant after the issuer's role is checked and before step 1
/// (roles-and-keys.md issuance order; ADR-0026 d4).
#[derive(Debug, Clone)]
pub struct IssueKeyRequest {
    pub role: Role,
    /// The key's name, required at issuance and distinct from its principal
    /// and its label; whether it is taken is checked after every other check
    /// (step 8, `name.taken`; ADR-0026 d5, d9).
    pub name: Name,
    pub principal: String,
    /// The issuing admin's attestation; `None` journals null and stores the
    /// schema default, `agent`.
    pub held_by: Option<HeldBy>,
    /// `None` takes the first grant's node; a key never inherits its
    /// issuer's default.
    pub default_workarea: Option<Reference>,
    /// The key's crew record, fixed for its life; one the tenant does not
    /// hold is absent at step 4 (ADR-0026 d8).
    pub crew: Option<Reference>,
    pub grants: Vec<GrantRequest>,
    /// Issue a connector key: no value, a connect code in its place, after
    /// step 7 admits a key that can hold a connection (roles-and-keys.md
    /// issuance order, step 7; ADR-0031 d7).
    pub connect_code: bool,
}

/// One requested grant: a role over the subtree of `node`, less the
/// subtrees of `deny`, each of which must lie inside it (workareas.md
/// per-grant attenuation), every node by id or name.
#[derive(Debug, Clone)]
pub struct GrantRequest {
    pub node: Reference,
    pub role: Role,
    pub deny: Vec<Reference>,
}

/// `POST /v2/crews` (http-api.md crews; ADR-0026 d8): every field but the
/// name optional.
#[derive(Debug, Clone)]
pub struct CrewCreateRequest {
    pub name: Name,
    pub charter_uri: Option<String>,
    pub external_manager: Option<String>,
}

/// `POST /v2/crews/{crew_id}/update`: the fields to change, at least one;
/// a field not sent keeps its value, and the name never changes
/// (http-api.md crews; ADR-0026 d8).
#[derive(Debug, Clone, Default)]
pub struct CrewUpdate {
    pub charter_uri: Option<String>,
    pub external_manager: Option<String>,
}

/// The two submission verbs, each its own journal event (http-api.md
/// submit; event-catalog.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitVerb {
    Remember,
    Learn,
}

impl SubmitVerb {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Remember => "remember",
            Self::Learn => "learn",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubmitRequest {
    pub idempotency_key: String,
    /// `None` derives the key from the author and the text; a sent key
    /// starting `auto-` is refused (ADR-0039 d6).
    pub claim_key: Option<String>,
    /// `None` is `learn` when `derives_from` is non-empty and `remember`
    /// otherwise; a sent verb is honoured (ADR-0039 d3).
    pub verb: Option<SubmitVerb>,
    pub kind: MemoryKind,
    /// The claim text as sent; capture normalizes it to NFC (admission.md).
    pub body: String,
    /// The body's `workarea`, by id or name; `None` resolves to the key's
    /// default workarea (http-api.md; ADR-0026 d4, ADR-0039 d7).
    pub workarea: Option<Reference>,
    pub derives_from: Vec<Uuid>,
}

/// The stored idempotent response: a replayed submit returns this byte for
/// byte (http-api.md idempotency record).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitReceipt {
    pub memory_id: Uuid,
    pub claim_key: String,
    pub version: u32,
    pub workarea_id: Uuid,
    pub state: String,
    pub journal_seq: u64,
    pub body_digest: String,
}

#[derive(Debug, Clone)]
pub struct SubmitOutcome {
    pub receipt: SubmitReceipt,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Reject,
}

#[derive(Debug, Clone)]
pub struct VerdictOutcome {
    pub memory_id: Uuid,
    pub version: u32,
    pub state: MemoryStatus,
    pub journal_seq: u64,
    /// Whether this answers a committed verdict's idempotency record
    /// rather than a verdict this call journalled (http-api.md idempotency).
    pub replayed: bool,
}

/// A serve as a verdict sent it: `serve_id` as written, digested as sent,
/// and the row it names (http-api.md idempotency).
#[derive(Debug, Clone)]
pub struct NamedServe {
    pub serve_id: String,
    pub memory_id: Uuid,
}

/// A review verdict as `POST /v2/review/verdicts` received it. Its
/// idempotency record is scoped to the key, the nil workarea and the
/// `Idempotency-Key`, and looked up before the default serve resolves, so a
/// retry replays what it committed and never resolves a new target
/// (http-api.md idempotency; ADR-0039 d10).
#[derive(Debug, Clone)]
pub struct VerdictRequest {
    /// `None` when omitted: the reviewer key's one claimed item.
    pub serve: Option<NamedServe>,
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub idempotency_key: String,
}

/// The kinds recall serves as hits. Vocabulary kinds join them only by a
/// workarea's journalled opt-in, which is unbuilt (retrieval.md, the
/// recall-ordering contract; ADR-0021 d9, ADR-0025 d8).
pub const HIT_KINDS: [MemoryKind; 2] = [MemoryKind::Memory, MemoryKind::DerivedFact];

#[derive(Debug, Clone)]
pub struct RecallRequest {
    pub query: String,
    pub k: usize,
    /// The body's `workarea`, the scope node, by id or name; `None`
    /// resolves to the key's default workarea, and a key with neither is
    /// refused — there is no tenant-wide fallback (ADR-0025 d1; ADR-0026
    /// d4, ADR-0039 d4).
    pub workarea: Option<Reference>,
    /// The hit kinds the request admits; empty when it sent no filter. It
    /// narrows the hits only, never the steering (retrieval.md, the
    /// recall-ordering contract; ADR-0025 d8).
    pub kinds: Vec<MemoryKind>,
    /// `include_text`: whether the served rows carry their text; a budget
    /// counts text, so `max_tokens` without it is refused (retrieval.md
    /// token budgets; ADR-0025 d7, ADR-0039 d7).
    pub include_text: bool,
    /// `include_unreviewed`: the calling key's own held rows join the
    /// ranked hits, flagged by state (retrieval.md, own held rows in recall;
    /// ADR-0025 d4, ADR-0039 d4).
    pub include_unreviewed: bool,
    /// The budget in budget tokens under counter `oml-tc/1`; `None` is no
    /// budget (retrieval.md token budgets; ADR-0025 d7).
    pub max_tokens: Option<u64>,
}

/// One recall candidate as the store ranks it, before the `k` and budget
/// cuts: its id, the clear `body_bytes` its budget-token count derives
/// from, and the store's score. It carries no body, so a row the cuts drop
/// is never hydrated; the served rows hydrate in one set-wise read
/// (retrieval.md token budgets; ADR-0024 d4, ADR-0025 d7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecallCandidate {
    pub memory_id: Uuid,
    pub body_bytes: u64,
    /// The store's rank: `ts_rank` in Postgres, the count of distinct query
    /// terms matched in the in-memory store (retrieval.md, the
    /// recall-ordering contract).
    pub score: f32,
}

/// One recall hit: the row as the calling key is served it, and the store's
/// score for it (openapi.yaml `RecallResponse.hits`; ADR-0025 d8). A hit
/// reads as the served row it carries.
#[derive(Debug, Clone)]
pub struct RecallHit {
    pub served: ServedMemory,
    /// The store's rank: `ts_rank` in Postgres, the count of distinct query
    /// terms matched in the in-memory store (retrieval.md, the
    /// recall-ordering contract).
    pub score: f32,
}

impl core::ops::Deref for RecallHit {
    type Target = ServedMemory;

    fn deref(&self) -> &ServedMemory {
        &self.served
    }
}

/// Applicable steering, ordered structurally and never ranked, precedes the
/// hits, ordered by score descending then `artifact_id` ascending
/// (retrieval.md, the recall-ordering contract; ADR-0025 d8), cut to `k`
/// and then to the budget (ADR-0025 d7).
#[derive(Debug, Clone)]
pub struct RecallResult {
    pub steering: Vec<ServedMemory>,
    pub hits: Vec<RecallHit>,
    /// How the `max_tokens` cut was applied; `None` when no budget was sent,
    /// and the response then carries no budget object (retrieval.md token
    /// budgets).
    pub budget: Option<Budget>,
    /// The artifact ids served after the `k` and budget cuts, steering then
    /// hits, own held rows included: what the read log records, never the
    /// candidates before the cuts (retrieval.md, the read log; ADR-0025 d9).
    pub served_ids: Vec<Uuid>,
    /// SHA-256 over the RFC 8785 JSON of the whole request, every member
    /// present with its default applied (retrieval.md, the read log;
    /// ADR-0025 d9).
    pub query_digest: String,
}

/// Whose rows a listing reads: the store's own predicate, so no page
/// reaches a row outside it (http-api.md search; ADR-0025 d13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingScope {
    /// The own-work listing: every row the key set authored, in every
    /// state, except the rows on a `masked` node — the subtrees the key's
    /// global per-key denials cover (roles-and-keys.md#own). The key set is
    /// the calling key alone until rotation is built.
    Own {
        key_ids: Vec<Uuid>,
        masked: Vec<Uuid>,
    },
    /// The scoped listing: the promoted served tips on exactly these nodes,
    /// subtree(scope) ∩ reader effective access as the use-case computes
    /// it; an empty set lists nothing (ADR-0025 d1, d13).
    Tips { nodes: Vec<Uuid> },
}

/// A listing's predicates, re-applied on every page: a cursor carries a
/// position only (http-api.md pagination; ADR-0025 d13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingFilter {
    pub scope: ListingScope,
    /// Only rows on these nodes — the own-work listing's `workarea_id`
    /// subtree; `None` narrows nothing.
    pub subtree: Option<Vec<Uuid>>,
    pub state: Option<MemoryStatus>,
    pub kind: Option<MemoryKind>,
    pub claim_key: Option<String>,
    /// `created_at` at or after this instant.
    pub since: Option<EventTime>,
    /// `created_at` before this instant.
    pub until: Option<EventTime>,
}

/// Where a listing page resumes: the `created_at` and id of the last row
/// the previous page served, in the order `created_at` descending, then
/// `artifact_id` descending (http-api.md search).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingPosition {
    pub created_at: EventTime,
    pub artifact_id: Uuid,
}

/// One stored claim edge: a labelled connection from one of a tenant's
/// versions to another, pending until a key that is not its proposer
/// ratifies it (postgres-schema.md `claim_edges`; ADR-0008 d5; ADR-0021;
/// ADR-0012). Nothing deletes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimEdgeRecord {
    pub edge_id: Uuid,
    pub tenant_id: Uuid,
    /// The source version (`from_artifact_id` on the proposal).
    pub src_artifact_id: Uuid,
    /// The target version (`to_artifact_id` on the proposal).
    pub dst_artifact_id: Uuid,
    /// The label as proposed, admitted by the domain's label check.
    pub label: String,
    /// `pending` or `ratified`; this build stores no rejected edge, as
    /// `oml:about` is refused at proposal (ADR-0027 is not accepted).
    pub state: EdgeState,
    pub proposer_key_id: Uuid,
    /// The proposer key's principal, which R2 at ratification compares the
    /// ratifier's against (errors.md `review.ratifier_is_proposer`).
    pub proposer_principal: String,
    /// The ratifying key; `None` while the edge is pending.
    pub ratifier_key_id: Option<Uuid>,
    /// The `at` of its `claim_edge.proposed` event, which the store refuses
    /// any other value for: the listing's order key (http-api.md
    /// pagination).
    pub created_at: EventTime,
}

/// `GET /v2/claim-edges`'s predicates, re-applied on every page; `None`
/// narrows nothing, and two present predicates both hold (http-api.md,
/// claim edges).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClaimEdgeFilter {
    /// Only edges with this version at either endpoint.
    pub artifact_id: Option<Uuid>,
    /// Only edges with an endpoint version in this workarea.
    // reading: http-api.md names `workarea_id` on the graph read but not
    // which endpoint it constrains; an edge is a connection either way, so
    // it matches as `artifact_id` does, at either endpoint.
    pub workarea_id: Option<Uuid>,
}

/// Where a claim-edge page resumes: the `created_at` and id of the last
/// edge the previous page served, in the order `created_at` descending,
/// then `edge_id` descending, as [`ListingPosition`] is (http-api.md
/// pagination: newest first on every list endpoint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimEdgePosition {
    pub created_at: EventTime,
    pub edge_id: Uuid,
}

/// `GET /v1/memories` as the use-case takes it (http-api.md search;
/// ADR-0025 d13).
#[derive(Debug, Clone)]
pub struct ListRequest {
    /// `own=true`: the own-work listing, every row the key wrote.
    pub own: bool,
    /// The query's `workarea`, by id or name, the subtree the listing reads
    /// (ADR-0039 d4, d7). On the scoped listing `None` resolves to the
    /// key's default workarea, and a key with neither is refused; on the
    /// own-work listing it narrows the key's rows to that subtree, and
    /// `None` lists every workarea the key wrote in.
    pub workarea: Option<Reference>,
    pub state: Option<MemoryStatus>,
    pub kind: Option<MemoryKind>,
    pub claim_key: Option<String>,
    pub since: Option<EventTime>,
    pub until: Option<EventTime>,
    /// The page size, which the transport bounds by `api.page_limit_max`.
    pub limit: usize,
    /// The opaque cursor of the previous page.
    pub cursor: Option<String>,
}

/// One listing page: summaries as the calling key is served them, and the
/// cursor of the next page, `None` once the listing is exhausted.
#[derive(Debug, Clone)]
pub struct MemoryPage {
    pub items: Vec<ServedMemory>,
    pub next_cursor: Option<String>,
}

/// One page of the tenant's crew records in name order, and the cursor of
/// the next page, `None` once the listing is exhausted (http-api.md crews;
/// ADR-0026 d8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrewPage {
    pub items: Vec<CrewRecord>,
    pub next_cursor: Option<String>,
}

/// One key as the key read serves it (http-api.md keys, grants, denials;
/// ADR-0026 d9, d10): its row, a revoked key's included, the access its
/// grants and its denials in force give it, and its attestation as
/// `key.issued` journalled it, `None` when none was sent. Never its value,
/// which no store holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDetail {
    pub key: KeyRecord,
    pub held_by: Option<HeldBy>,
    pub access: KeyAccess,
}

/// A row a key's record references, as `whoami` serves it: its id and its
/// name (ADR-0039 d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedRef {
    pub id: Uuid,
    pub name: Name,
}

/// A key's default workarea as `whoami` serves it: its id, its name and the
/// path derived from its ancestry, root first (ADR-0039 d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkareaRef {
    pub id: Uuid,
    pub name: Name,
    pub path: String,
}

/// The rows a key's own record references, read in one store query: its
/// tenant, its default workarea and its crew record, `None` where the key
/// names none (ADR-0039 d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyReferences {
    pub tenant: NamedRef,
    pub default_workarea: Option<WorkareaRef>,
    pub crew: Option<NamedRef>,
}

/// `GET /v2/whoami`: the calling key's own record, the references it names
/// and its stored grants, never its denials, which name subtrees it may not
/// read (ADR-0039 d15). A provider-level key names no tenant, workarea or
/// crew and holds no grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Whoami {
    pub key: KeyRecord,
    pub tenant: Option<NamedRef>,
    pub default_workarea: Option<WorkareaRef>,
    pub crew: Option<NamedRef>,
    pub grants: Vec<GrantRecord>,
}

/// `POST /v1/memories/{artifact_id}/supersede` (http-api.md supersede;
/// ADR-0025 d10, d11). The successor's `claim_key` and workarea are always
/// the target's.
#[derive(Debug, Clone)]
pub struct SupersedeRequest {
    pub idempotency_key: String,
    pub target_id: Uuid,
    /// The successor's text as sent; capture normalizes it to NFC. With
    /// `retirement`, why the claim is retired.
    pub body: String,
    /// `None` takes the target's kind, the one field a successor inherits.
    pub kind: Option<MemoryKind>,
    /// A retire-proposal: ratified, the successor retires the claim.
    pub retirement: bool,
    /// Ancestors beside the target, which is always one.
    pub derives_from: Vec<Uuid>,
    /// Digested with the request, never journalled.
    pub reason: Option<String>,
    /// The body's `workarea`, by id or name: when sent, it must be the
    /// target's workarea (ADR-0039 d7).
    pub workarea: Option<Reference>,
    /// Set by `oml_supersede`, which carries no vocabulary fields: a target
    /// of a vocabulary kind is refused, and superseded on the HTTP API
    /// instead (mcp-plugin.md `oml_supersede`; ADR-0025 d12).
    pub refuse_vocabulary_target: bool,
}

/// The stored idempotent response of a supersession: a replay returns this
/// byte for byte, naming only the caller's own successor (openapi.yaml
/// `SubmitAccepted`; ADR-0025 d10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersedeReceipt {
    pub memory_id: Uuid,
    pub claim_key: String,
    pub version: u32,
    pub workarea_id: Uuid,
    pub state: String,
    pub retirement: bool,
    pub replaces: VersionRef,
    pub journal_seq: u64,
    pub body_digest: String,
}

/// `POST /v2/claim-edges` as the use-case takes it: a labelled edge from
/// one of the tenant's versions to another (http-api.md, claim edges;
/// ADR-0008 d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposeClaimEdge {
    pub from_artifact_id: Uuid,
    pub to_artifact_id: Uuid,
    pub label: String,
}

/// The stored idempotent response of a claim-edge proposal, the committed
/// edge as openapi.yaml's `ClaimEdge` serves it: a replay returns this byte
/// for byte, naming only the caller's own edge (http-api.md idempotency,
/// claim edges).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimEdgeProposal {
    pub edge_id: Uuid,
    /// The edge's source version, `src_artifact_id` as stored.
    pub from_artifact_id: Uuid,
    /// The edge's target version, `dst_artifact_id` as stored.
    pub to_artifact_id: Uuid,
    pub label: String,
    /// `pending` on a proposal, which confers nothing until a key that is
    /// not its proposer ratifies it, and `ratified` on that ratification
    /// (ADR-0008 d5; ADR-0012 R2).
    pub state: String,
    pub proposer_key_id: Uuid,
    /// `None` on a proposal; the ratifying key on a ratification.
    pub ratifier_key_id: Option<Uuid>,
}

impl From<&ClaimEdgeRecord> for ClaimEdgeProposal {
    fn from(edge: &ClaimEdgeRecord) -> Self {
        Self {
            edge_id: edge.edge_id,
            from_artifact_id: edge.src_artifact_id,
            to_artifact_id: edge.dst_artifact_id,
            label: edge.label.clone(),
            state: edge.state.as_str().to_owned(),
            proposer_key_id: edge.proposer_key_id,
            ratifier_key_id: edge.ratifier_key_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClaimEdgeProposalOutcome {
    pub proposal: ClaimEdgeProposal,
    pub replayed: bool,
}

/// `POST /v2/claim-edges/{edge_id}/ratify` as the use-case answers it: the
/// ratified edge as openapi.yaml's `ClaimEdge` serves it, which is also its
/// stored idempotent response (http-api.md, claim edges and idempotency).
#[derive(Debug, Clone)]
pub struct ClaimEdgeRatification {
    pub edge: ClaimEdgeProposal,
    /// The seq of the `review.ratify` this call committed; `None` on a
    /// replay, whose record holds the `ClaimEdge` alone.
    pub journal_seq: Option<u64>,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct SupersedeOutcome {
    pub receipt: SupersedeReceipt,
    pub replayed: bool,
}

/// One event as the content tail serves it to the calling key
/// (http-api.md#events-tail; ADR-0025 d15).
#[derive(Debug, Clone)]
pub struct TailedEvent {
    /// The journalled event, its hash and signature as stored.
    pub event: SignedEnvelope,
    /// Whether the event's actor, its envelope `api_key_id`, is in the
    /// calling key's own key set; outside it the tail serves `principal` and
    /// `api_key_id` as null.
    pub names_actor: bool,
    /// The payload as served: outside the actor's own key set every field
    /// naming a key is null, and a claim-subject workarea id the naming rule
    /// withholds is null (roles-and-keys.md#own).
    pub payload: serde_json::Value,
}

/// One page of the events tail: what one bounded scan let the key see, the
/// head read with it, and where the next page resumes
/// (http-api.md#events-tail; ADR-0025 d15).
#[derive(Debug, Clone)]
pub struct TailPage {
    pub events: Vec<TailedEvent>,
    pub head_seq: u64,
    pub next_after_seq: u64,
}

/// Who a use case serves: a key value, as every route presents it, or the
/// caller a connection's access token resolved to at `/mcp`, which only
/// [`App::resolve_access_token`](crate::App::resolve_access_token) builds,
/// so a token reaches the agent use cases the facade dispatches and nothing
/// else, and acts as exactly its key (roles-and-keys.md connections;
/// ADR-0031 d10, d11). It has no `Debug`, so no log line prints a key value.
#[derive(Clone, Copy)]
pub enum Bearer<'a> {
    /// A key value.
    Key(&'a str),
    /// The caller of a live connection's access token, resolved on this
    /// request.
    Connection(&'a crate::app::Caller),
}

impl<'a> From<&'a str> for Bearer<'a> {
    fn from(value: &'a str) -> Self {
        Bearer::Key(value)
    }
}

impl<'a> From<&'a String> for Bearer<'a> {
    fn from(value: &'a String) -> Self {
        Bearer::Key(value)
    }
}

/// A typed application refusal; every variant names the rule it enforces.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unknown api key (reads fail closed — http-api.md authentication)")]
    Unauthorized,
    #[error(
        "{verb} is outside the {role:?} verb ceiling (roles-and-keys.md permission matrix, R1)"
    )]
    VerbCeiling { role: Role, verb: &'static str },
    /// Issuance steps 2 and 3: with no `grant_index`, `role` is the requested
    /// key role and `admissible` the key roles the issuer may issue; with
    /// one, `role` is the first refused grant's role and `admissible` the key
    /// role's admissible grant roles (roles-and-keys.md issuance order).
    #[error(
        "{role:?} is outside the admissible set {admissible:?} (roles-and-keys.md issuer ceiling, access.issuer_ceiling)"
    )]
    IssuerCeiling {
        role: Role,
        admissible: &'static [Role],
        grant_index: Option<usize>,
    },
    #[error(
        "an admin key never carries a dreamer grant (roles-and-keys.md two-key rule, access.role_exclusive)"
    )]
    RoleExclusive,
    #[error(
        "the node is readable but effective access does not confer {needed_role:?} (roles-and-keys.md refusal order, access.grant_refused)"
    )]
    GrantRefused {
        workarea_id: Uuid,
        needed_role: Role,
    },
    /// The deny node as the request spelled it (http-api.md names and ids).
    #[error(
        "deny node {subtree_node} lies outside grant {grant_index}'s subtree (roles-and-keys.md issuance order, request.validation)"
    )]
    DenyOutsideGrant {
        grant_index: usize,
        subtree_node: Reference,
    },
    /// The default as the request spelled it.
    #[error(
        "no grant of the key covers default workarea {workarea} (roles-and-keys.md issuance order, request.validation)"
    )]
    DefaultUncovered { workarea: Reference },
    #[error(
        "no workarea: the key has no default and the request names none (http-api.md request.workarea_unresolved)"
    )]
    WorkareaUnresolved,
    #[error(
        "max_tokens counts text that include_text false would not send (retrieval.md token budgets, request.validation)"
    )]
    BudgetWithoutBodies,
    #[error(
        "after_seq {after_seq} is beyond head_seq {head_seq} (http-api.md events tail, request.validation)"
    )]
    AfterSeqBeyondHead { after_seq: u64, head_seq: u64 },
    #[error(
        "not found — absence and no-access are indistinguishable (http-api.md access.not_found)"
    )]
    NotFound,
    #[error(
        "a writer may supersede only the claims its own key authored (roles-and-keys.md#own, access.author_only)"
    )]
    AuthorOnly,
    /// The target is not the current served tip, at supersede or at the
    /// ratifying verdict; `current_tip` only when the caller can read it
    /// (errors.md `submission.tip_conflict`; ADR-0025 d10, d16).
    #[error(
        "the supersede target is not the current promoted tip (http-api.md submission.tip_conflict)"
    )]
    TipConflict { current_tip: Option<VersionRef> },
    /// The `workarea` as the request spelled it.
    #[error(
        "workarea names {workarea}, not the target's workarea: a supersession is captured in its target's (http-api.md supersede, request.validation)"
    )]
    WorkareaNotTarget { workarea: Reference },
    #[error(
        "a {target:?} target admits no {sent:?} successor: a kind changes only within its class, and a retirement keeps the target's (audit-and-lineage.md, request.validation)"
    )]
    KindChangeRefused {
        target: MemoryKind,
        sent: MemoryKind,
    },
    #[error(
        "a term is never retired: terms deprecate (ADR-0021 d3; ADR-0025 d11, request.validation)"
    )]
    TermRetirement,
    #[error(
        "oml_supersede carries no vocabulary fields, so a {0:?} target is superseded on the HTTP API (mcp-plugin.md oml_supersede; ADR-0025 d12, request.validation)"
    )]
    VocabularyTarget(MemoryKind),
    /// A claim-edge label the domain's label check refused; `field` is
    /// `details.field` (errors.md `request.validation`; ADR-0021 d5).
    #[error("{reason}")]
    ClaimEdgeLabel {
        field: EdgeField,
        reason: LabelRefusal,
    },
    /// The ratifier's key or principal is in either endpoint's lineage
    /// closure (errors.md `review.author_in_lineage`; ADR-0012 R2).
    #[error("the ratifier is in an endpoint's lineage closure (review.author_in_lineage)")]
    AuthorInLineage,
    /// The edge is decided: ratified before this call, or by a commit that
    /// won the store's compare-and-set. Nothing was journalled.
    // reading: errors.md declares no edge-specific code; a verdict on a
    // decided row answers `review.serve_invalid` with `details.reason`
    // `consumed`, and a decided edge answers alike.
    #[error("claim edge {edge_id} is already decided (review.serve_invalid, consumed)")]
    EdgeConsumed { edge_id: Uuid },
    /// Ratifying the edge would close a cycle over its label's ratified
    /// edges (errors.md `vocab.cycle_refused`; ADR-0021 d5).
    #[error(transparent)]
    CycleRefused(#[from] CycleRefused),
    /// A stored endpoint of a ratified or ratifying `oml:is_a` edge whose
    /// `claim_key` is not a CURIE, or that is not a term: the label check
    /// refuses such an edge at proposal and at ratification, so only a row
    /// that defies it reaches here; the cycle check cannot run, and the
    /// ratification fails closed and journals nothing (ADR-0021 d2, d5).
    #[error("claim_key {claim_key:?} of a term edge's endpoint is not a term's CURIE")]
    TermEndpoint { claim_key: String },
    #[error(
        "a sent claim_key starts auto-, the prefix only the server derives (ADR-0039 d6, request.validation)"
    )]
    DerivedClaimKeySent,
    #[error(
        "a held, promoted or demoted version holds this claim_key in the workarea (audit-and-lineage.md, submission.claim_key_conflict)"
    )]
    ClaimKeyConflict,
    #[error("deterministic scan veto: {0} (admission.md — a refusal that journals nothing)")]
    ScanVeto(String),
    #[error(
        "derives_from names {0}, which the key cannot read or no row is (fail closed, access.not_found)"
    )]
    LineageUnknown(Uuid),
    #[error(
        "idempotency key reused with a different request (http-api.md idempotency.digest_mismatch)"
    )]
    IdempotencyDigestMismatch,
    #[error("stored idempotent response is unreadable: {0}")]
    StoredResponse(String),
    /// A verdict sent without `serve_id` while the reviewer key holds no
    /// claimed review item, or `several`; nothing was journalled
    /// (http-api.md review; ADR-0039 d10).
    #[error(
        "a verdict without serve_id applies to the reviewer key's one claimed item, and it holds {}: send serve_id (ADR-0039 d10, request.validation, field serve_id)",
        if *.several { "several" } else { "none" }
    )]
    ServeIdUnresolved { several: bool },
    /// A creation's name its kind already holds in the namespace, a revoked
    /// key's included; nothing was journalled. Only admin and operator keys
    /// hold a creating verb, and the bootstrap claim checks its token first,
    /// so no agent key ever receives it (errors.md `name.taken`; ADR-0026
    /// d5, d10).
    #[error("the {} name {} is already taken in its namespace (errors.md name.taken)", .kind.as_str(), .name.as_str())]
    NameTaken { kind: NamedKind, name: Name },
    /// Denials attenuate grants and an admin key holds none, so no denial on
    /// one is journalled (http-api.md keys, grants, denials; ADR-0026 d15).
    #[error(
        "a global denial on an admin key is refused: an admin key carries no grant for it to attenuate (roles-and-keys.md permission matrix footnote 6, request.validation, field key)"
    )]
    DenialOnAdminKey,
    #[error(
        "a crew update sends charter_uri or external_manager, at least one (http-api.md crews, request.validation)"
    )]
    EmptyCrewUpdate,
    /// The lead's naming of a chain's rows that predate names refused; the
    /// chain journals nothing (ADR-0026 d11).
    #[error("the rows of tenant {tenant_id} that predate names were not named: {refusal}")]
    NamingRefused {
        tenant_id: Uuid,
        refusal: NamingRefusal,
    },
    /// The status ladder refused a proposal of `from` → `to` on the node,
    /// before anything was sealed: nothing is journalled (errors.md
    /// `workarea.transition_refused`, details `{workarea_id, from, to,
    /// reason}`; ADR-0009 d1, d3, d4).
    #[error(
        "workarea {workarea_id}: {} to {} is refused: {reason} (errors.md workarea.transition_refused)",
        .from.as_str(),
        .to.as_str()
    )]
    TransitionRefused {
        workarea_id: Uuid,
        from: WorkareaStatus,
        to: WorkareaStatus,
        reason: LadderRefusal,
    },
    /// A `validated` node was asked for an operation freeze forbids
    /// (errors.md `workarea.frozen`; ADR-0009 d5).
    #[error("workarea {workarea_id} is validated and refuses {op:?} (errors.md workarea.frozen)")]
    WorkareaFrozen { workarea_id: Uuid, op: FrozenOp },
    /// The ratifying key is the proposing key or holds the proposer's
    /// principal, of a status transition or a claim edge (errors.md
    /// `review.ratifier_is_proposer`; ADR-0009 d3; ADR-0010 R2; ADR-0012).
    #[error(
        "the ratifier is the proposer: proposal and ratification are two identities (errors.md review.ratifier_is_proposer)"
    )]
    RatifierIsProposer,
    /// A listing cursor this server will not resume from (http-api.md
    /// pagination, request.validation).
    #[error(transparent)]
    Cursor(#[from] CursorError),
    #[error(transparent)]
    Verdict(#[from] VerdictError),
    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error(transparent)]
    Chain(#[from] ChainError),
    #[error(transparent)]
    ChainVerify(#[from] VerifyError),
    #[error(transparent)]
    Jcs(#[from] JcsError),
    /// A refusal of the connection flow, one `oauth` code or
    /// `request.validation`; its status and shape are the edge's
    /// (errors.md "`oauth` — connections"; ADR-0031 d13).
    #[error(transparent)]
    OAuth(#[from] Refusal),
    /// A token request that sent client authentication, by its scheme
    /// (`None` for a `client_secret` in the body): `oauth.client_invalid`
    /// as `invalid_client`, which a header answers `401` naming the scheme
    /// it used (http-api.md `POST /oauth/token`; ADR-0031 d5).
    #[error(
        "oauth.client_invalid: clients are public and send no client authentication (ADR-0031 d5)"
    )]
    ClientAuthentication { scheme: Option<String> },
    #[error(transparent)]
    Store(StoreError),
}

/// A store refusal as the application answers it: the unique index's
/// refusal of a creation's name is [`AppError::NameTaken`], whichever
/// commit carried it; every other refusal passes through (ADR-0026 d5).
impl From<StoreError> for AppError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NameTaken { kind, name } => AppError::NameTaken { kind, name },
            other => AppError::Store(other),
        }
    }
}
