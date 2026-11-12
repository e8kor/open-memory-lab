//! The auth context's use cases (ADR-0070 d6): a sign-in's start and the
//! admission of a provider's answer against the allowlist (ADR-0045 d3,
//! d4), open sign-up after the terms page (ADR-0068 d2), the session keys
//! a first sign-in issues (ADR-0045 d6), the session rows a signed-in
//! browser holds (d5), and the caller a capacity credential acts as
//! (ADR-0070 d2). They serve the lead's sign-in routes and the API's
//! resolution of an `omls_` credential; no page calls them, the pages
//! being `oml-web`'s (ADR-0070 d7 (c)).

use serde_json::json;
use uuid::Uuid;

use oml_domain::access::Covers;
use oml_domain::name::{Name, NamedKind, Reference};
use oml_domain::signup::{SignupMode, drawn_user};
use oml_domain::web::{
    Provider, ProviderIdentity, SESSION_TTL_DEFAULT_SECS, SignInRefusal, SignInState, admit_github,
    admit_google_id_token, capacity_credential, session_key_names,
};
use oml_domain::{EventTime, HeldBy, Role, Sha256Hash, event_types};

use super::pages::key_name;
use super::signup::DRAWS_MAX;
use super::{App, Caller, Capacity, Issuance};
use crate::ports::{
    Clock, ConnectionStore, IdGen, MetadataFold, Scanner, SignInExchange, Signer, StoreError,
    WebStore,
};
use crate::types::{
    AppError, Credential, GrantRequest, IssueKeyRequest, KeyAccess, KeyRecord, OpenedSession,
    PROVIDER_TENANT, PinOutcome, ProviderAnswer, ProviderFailure, SignupPolicy, WebCredentialKey,
    WebCredentialRows, WebEntry, WebSessionRecord,
};

/// The name of the session key `capacity` names for `user`, for the
/// store's read of the session gate's rows: `None` for a word that is no
/// capacity or a name that is not a name, which then reads no key.
fn session_key_name(user: &Name, capacity: &str) -> Option<Name> {
    Capacity::parse(capacity).and_then(|c| key_name(user, c.suffix()).ok())
}

/// Whether `key` is the session key `name` of `session`'s user: the row
/// holding the name, a session key serving that user, not revoked
/// (ADR-0045 d6).
fn serves_session(key: &KeyRecord, session: &WebSessionRecord, name: &Name) -> bool {
    key.name.as_ref() == Some(name)
        && key.credential == Credential::Session
        && key.session_user.as_ref() == Some(&session.user)
        && key.revoked_at.is_none()
}

/// What a provider callback carries for the application to judge
/// (ADR-0045 d3): the provider in the path, the query's `code` and
/// `state`, the sealed cookie's state once opened (`None` when it was
/// absent or did not open), the redirect URI the flow named, and the
/// provider's client id, which a Google ID token's `aud` must name.
#[derive(Debug, Clone, Copy)]
pub struct Callback<'a> {
    pub provider: Provider,
    pub code: Option<&'a str>,
    pub state: Option<&'a str>,
    pub opened: Option<&'a SignInState>,
    pub redirect_uri: &'a str,
    pub client_id: &'a str,
}

/// Why a callback admitted no one: a refusal the page names, or the
/// provider failure behind `provider_refused` and `limit.rate`, which the
/// doctor counter reads (ADR-0045 d15, d18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackRefusal {
    Refused(SignInRefusal),
    /// In open mode, a verified identity no entry holds: the terms page
    /// comes next, and nothing is written yet (ADR-0068 d2).
    Unlisted(ProviderIdentity),
    Provider(ProviderFailure),
    Store(String),
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: WebStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// A sign-in's start (ADR-0045 d3): fresh `state`, PKCE verifier and
    /// nonce, each 256 bits, and the time it started. It writes nothing;
    /// the transport seals it into the browser's cookie.
    pub fn web_sign_in_start(&self, provider: Provider) -> SignInState {
        SignInState::start(
            provider,
            [
                self.ids.new_secret(),
                self.ids.new_secret(),
                self.ids.new_secret(),
            ],
            self.clock.now().unix_micros().div_euclid(1_000_000),
        )
    }

    /// The callback's judgement (ADR-0045 d3, d4): no outbound call unless
    /// the sealed state opened, names this provider, is under ten minutes
    /// old and carries the query's `state`; then one exchange, the
    /// provider's answer admitted only for a verified email, the entry
    /// that email names found before any tenant is known, and the provider
    /// subject pinned at the entry's first sign-in or compared after it.
    pub async fn web_admit<X: SignInExchange>(
        &self,
        exchange: &X,
        callback: Callback<'_>,
    ) -> Result<WebEntry, CallbackRefusal> {
        let refused = |r| CallbackRefusal::Refused(r);
        let now_secs = self.clock.now().unix_micros().div_euclid(1_000_000);
        let (Some(opened), Some(state), Some(code)) =
            (callback.opened, callback.state, callback.code)
        else {
            return Err(refused(SignInRefusal::StateMismatch));
        };
        if !opened.admits(callback.provider, state, now_secs) || code.is_empty() {
            return Err(refused(SignInRefusal::StateMismatch));
        }
        let answer = exchange
            .redeem(
                callback.provider,
                code,
                &opened.verifier,
                callback.redirect_uri,
            )
            .await
            .map_err(CallbackRefusal::Provider)?;
        let identity: ProviderIdentity = match (callback.provider, answer) {
            (Provider::Google, ProviderAnswer::GoogleIdToken(token)) => admit_google_id_token(
                &token,
                callback.client_id,
                opened.nonce.as_deref().unwrap_or(""),
                now_secs,
            )
            .map_err(refused)?,
            (Provider::GitHub, ProviderAnswer::GitHub { account_id, emails }) => {
                admit_github(account_id, &emails).map_err(refused)?
            }
            _ => return Err(refused(SignInRefusal::ProviderRefused)),
        };
        let email_hash = identity.email.digest();
        let store = |e: StoreError| CallbackRefusal::Store(e.to_string());
        let Some(entry) = self
            .journal
            .web_entry_by_email(&email_hash)
            .await
            .map_err(store)?
        else {
            // Open mode takes an unlisted, verified identity to the terms
            // page (ADR-0068 d2); allowlist mode refuses it as before.
            return Err(
                if self
                    .signup_policy()
                    .is_some_and(|p| p.mode == oml_domain::signup::SignupMode::Open)
                {
                    CallbackRefusal::Unlisted(identity)
                } else {
                    refused(SignInRefusal::NotAllowed)
                },
            );
        };
        // A disabled person signs in to nothing (ADR-0068 d8).
        if entry.disabled_at.is_some() {
            return Err(refused(SignInRefusal::Disabled));
        }
        match self
            .journal
            .web_pin(
                entry.tenant_id,
                &email_hash,
                identity.provider,
                &identity.subject,
            )
            .await
            .map_err(store)?
        {
            PinOutcome::Pinned | PinOutcome::Matched => Ok(entry),
            PinOutcome::Differs => Err(refused(SignInRefusal::IdentityChanged)),
        }
    }

    /// Opens a session for an admitted entry under a fresh 256-bit token,
    /// living `ttl_secs` (ADR-0045 d5), and ends the session the browser
    /// arrived with, `replaces`, in the same transaction. Only the token's
    /// digest is stored.
    pub async fn web_session_open(
        &self,
        entry: &WebEntry,
        ttl_secs: i64,
        replaces: Option<&str>,
    ) -> Result<OpenedSession, AppError> {
        let token: String = self
            .ids
            .new_secret()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let created_at = self.clock.now();
        let expires_at = EventTime::from_unix_micros(
            created_at.unix_micros() + ttl_secs.saturating_mul(1_000_000),
        )
        .ok_or_else(|| AppError::Store(StoreError::Backend("session expiry overflows".into())))?;
        let record = WebSessionRecord {
            token_hash: Sha256Hash::digest(token.as_bytes()),
            tenant_id: entry.tenant_id,
            email_hash: entry.email.digest(),
            user: entry.user.clone(),
            admin: entry.admin,
            created_at,
            expires_at,
        };
        let credentials = self
            .web_session_capacities(entry)
            .await?
            .into_iter()
            .map(|capacity| {
                let credential = capacity_credential(&token, capacity);
                (capacity, Sha256Hash::digest(credential.as_bytes()))
            })
            .collect();
        self.journal
            .web_session_open(
                record.clone(),
                replaces.map(|old| Sha256Hash::digest(old.as_bytes())),
                credentials,
            )
            .await?;
        Ok(OpenedSession { token, record })
    }

    /// The capacities a session's entry reaches when it opens (ADR-0070
    /// d2): `web` and `review`, `curate` where `<user>-curate` exists, and
    /// `admin` for an admin's entry. Each gets a capacity credential whose
    /// digest is stored beside the session row.
    async fn web_session_capacities(
        &self,
        entry: &WebEntry,
    ) -> Result<Vec<&'static str>, AppError> {
        let curate = format!("{}-curate", entry.user.as_str());
        let has_curate = self
            .journal
            .web_session_keys(entry.tenant_id, &entry.user)
            .await?
            .iter()
            .any(|k| k.revoked_at.is_none() && k.name.as_ref().map(Name::as_str) == Some(&curate));
        Ok(["web", "review", "curate", "admin"]
            .into_iter()
            .filter(|c| match *c {
                "curate" => has_curate,
                "admin" => entry.admin,
                _ => true,
            })
            .collect())
    }

    /// The live session a cookie's token names: `None` for no row, and for
    /// a row past its expiry, which is ended here (ADR-0045 d5). A row ends
    /// with its entry, so a live row's entry stands as the row says.
    pub async fn web_session(&self, token: &str) -> Result<Option<WebSessionRecord>, AppError> {
        let token_hash = Sha256Hash::digest(token.as_bytes());
        let Some(session) = self.journal.web_session(&token_hash).await? else {
            return Ok(None);
        };
        if session.expires_at.unix_micros() <= self.clock.now().unix_micros() {
            self.journal
                .web_session_end(session.tenant_id, &token_hash)
                .await?;
            return Ok(None);
        }
        Ok(Some(session))
    }

    /// `sign_out`: the session's row ends (ADR-0045 d5).
    pub async fn web_sign_out(&self, session: &WebSessionRecord) -> Result<(), AppError> {
        Ok(self
            .journal
            .web_session_end(session.tenant_id, &session.token_hash)
            .await?)
    }
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: WebStore + ConnectionStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The first sign-in's keys (ADR-0045 d6, d20 e): `<user>-web`, a user
    /// key whose issuance creates `universe.<user>`, writer and reader on
    /// it; `<user>-review`, a tenant dreamer key with dreamer and reader on
    /// that root; and, for an admin entry, `<user>-admin`. Each is issued
    /// by the admin key that added the entry, only when missing, so a sign-in
    /// whose issuance failed part way completes at the next one. Every
    /// session key's principal is its own name, so R2 lets `<user>-review`
    /// approve the person's agents' writes.
    pub async fn web_ensure_keys(&self, entry: &WebEntry) -> Result<(), AppError> {
        let tenant_id = entry.tenant_id;
        let held = self
            .journal
            .web_session_keys(tenant_id, &entry.user)
            .await?;
        let holds = |capacity: Capacity| {
            held.iter().any(|k| {
                k.revoked_at.is_none()
                    && k.name.as_ref().is_some_and(|n| {
                        n.as_str() == format!("{}-{}", entry.user.as_str(), capacity.suffix())
                    })
            })
        };
        let needed: Vec<Capacity> = [Capacity::Web, Capacity::Review, Capacity::Admin]
            .into_iter()
            .filter(|c| *c != Capacity::Admin || entry.admin)
            .filter(|c| !holds(*c))
            .collect();
        if needed.is_empty() {
            return Ok(());
        }
        let issuer = self
            .journal
            .key(tenant_id, entry.added_by)
            .await?
            .filter(|k| k.revoked_at.is_none() && k.role == Role::Admin)
            .ok_or(AppError::WebKeyMissing { capacity: "web" })?;
        let root = Reference::Name(entry.user.clone());
        let grant = |role: Role| GrantRequest {
            node: root.clone(),
            role,
            deny: Vec::new(),
            covers: Covers::Subtree,
        };
        for capacity in needed {
            let name = key_name(&entry.user, capacity.suffix())?;
            let (role, grants, user) = match capacity {
                Capacity::Web => (
                    Role::Writer,
                    vec![grant(Role::Writer), grant(Role::Reader)],
                    Some(entry.user.clone()),
                ),
                Capacity::Review => (
                    Role::Dreamer,
                    vec![grant(Role::Dreamer), grant(Role::Reader)],
                    None,
                ),
                Capacity::Admin | Capacity::Curate => (Role::Admin, Vec::new(), None),
            };
            let req = IssueKeyRequest {
                profile: None,
                // The issuance default; a page offers no scope (ADR-0037
                // d17, d18 Q5).
                guardrail_scope: None,
                role,
                principal: name.as_str().to_owned(),
                name,
                held_by: Some(HeldBy::Human),
                default_workarea: None,
                crew: None,
                grants,
                connect_code: false,
                user,
            };
            self.issue_key_by(&issuer, &req, Issuance::Session(&entry.user))
                .await?;
        }
        Ok(())
    }

    /// The caller of one of the person's session keys, from a live session
    /// (ADR-0045 d6): the key serving the session's user, live, and for
    /// the admin capacity only while the entry is an admin's. The web key's
    /// own set is every key naming its user (d16). The request line names
    /// the key. The rows are read one after another, and checked as the
    /// session gate checks them.
    pub async fn web_caller(
        &self,
        session: &WebSessionRecord,
        capacity: Capacity,
    ) -> Result<Caller, AppError> {
        if capacity == Capacity::Admin && !session.admin {
            return Err(AppError::NotFound);
        }
        let tenant_id = session.tenant_id;
        let name = key_name(&session.user, capacity.suffix())?;
        let key = match self
            .journal
            .resolve_name(tenant_id, NamedKind::Key, &name)
            .await?
        {
            Some(id) => self.journal.key(tenant_id, id).await?,
            None => None,
        };
        let rows = match key.filter(|k| serves_session(k, session, &name)) {
            Some(key) => {
                let access = self.journal.key_access(tenant_id, key.key_id).await?;
                let (user_root, own_keys) = match key.user {
                    Some(root) => (
                        self.journal.workarea(tenant_id, root).await?,
                        self.journal.web_user_keys(tenant_id, root).await?,
                    ),
                    None => (None, Vec::new()),
                };
                Some(WebCredentialKey {
                    key,
                    access,
                    user_root,
                    own_keys,
                })
            }
            None => None,
        };
        self.session_key_caller(session, capacity, rows)
            .map(|(caller, _)| caller)
    }

    /// The caller `rows` make for `capacity` of `session`, with the access
    /// it was built from: every check a session key's caller passes
    /// (ADR-0045 d6; ADR-0070 d2), made on rows already read. The admin
    /// capacity only for an admin's entry; the key the one holding the
    /// capacity's name, a session key serving the session's user, not
    /// revoked; its access held by its tenant; a user key's root a user
    /// root (ADR-0044 d14); and the web key's own set every key naming its
    /// user (ADR-0045 d16). The request line names the key once it is
    /// found.
    fn session_key_caller(
        &self,
        session: &WebSessionRecord,
        capacity: Capacity,
        rows: Option<WebCredentialKey>,
    ) -> Result<(Caller, Option<KeyAccess>), AppError> {
        let missing = AppError::WebKeyMissing {
            capacity: capacity.suffix(),
        };
        if capacity == Capacity::Admin && !session.admin {
            return Err(AppError::NotFound);
        }
        let name = key_name(&session.user, capacity.suffix())?;
        let rows = rows
            .filter(|r| serves_session(&r.key, session, &name))
            .ok_or(missing)?;
        self.observer.credential(Some(&rows.key), None);
        // As `access_of` reads it: nothing for a provider-level key, and a
        // key its own tenant does not hold fails closed.
        let access = if rows.key.tenant_id == PROVIDER_TENANT {
            None
        } else {
            let access = rows.access.ok_or(AppError::Unauthorized)?;
            if let Some(root) = rows.key.user {
                let is_root = rows
                    .user_root
                    .as_ref()
                    .is_some_and(|w| w.workarea_id == root && w.user_root);
                if !is_root {
                    return Err(AppError::UserRootInvalid);
                }
            }
            Some(access)
        };
        let mut caller = Caller::of(rows.key, access.clone());
        if capacity == Capacity::Web && caller.key.user.is_some() {
            caller.own_keys = rows.own_keys.iter().map(|k| k.key_id).collect();
            if !caller.own_keys.contains(&caller.key.key_id) {
                caller.own_keys.push(caller.key.key_id);
            }
        }
        Ok((caller, access))
    }

    /// The caller a capacity credential acts as (ADR-0070 d2): the live
    /// session it derives from, re-checked as [`Self::web_caller`] checks a
    /// session on every request, under the capacity it names. A credential
    /// no live session holds, or whose capacity the session no longer
    /// reaches, is [`AppError::Unauthorized`]; the API answers it
    /// `auth.invalid`, as it answers an unknown key.
    ///
    /// The session, the key and its access are read in one store call
    /// (#297), and every check is made on those rows as it was made on
    /// each read. With `first_resolution`, which the gate passes for a
    /// request that carries no body, the request's first resolution of
    /// the caller decides on the access read here in place of reading the
    /// same rows again; every later one, and every resolution of a request
    /// with a body, reads them when it runs, as before.
    pub async fn web_credential_caller(
        &self,
        credential: &str,
        first_resolution: bool,
    ) -> Result<(Caller, WebSessionRecord), AppError> {
        let digest = Sha256Hash::digest(credential.as_bytes());
        let Some(rows) = self
            .journal
            .web_credential_rows(&digest, session_key_name)
            .await?
        else {
            self.observer.credential(None, None);
            return Err(AppError::Unauthorized);
        };
        let WebCredentialRows {
            session,
            capacity,
            key,
        } = rows;
        // A row past its expiry is ended here and names no session
        // (ADR-0045 d5), whatever else was read beside it.
        if session.expires_at.unix_micros() <= self.clock.now().unix_micros() {
            self.journal
                .web_session_end(session.tenant_id, &session.token_hash)
                .await?;
            self.observer.credential(None, None);
            return Err(AppError::Unauthorized);
        }
        let capacity = Capacity::parse(&capacity).ok_or(AppError::Unauthorized)?;
        match self.session_key_caller(&session, capacity, key) {
            // A person of the sign-up tenant's request rate binds their
            // session keys at the API as it binds their agent keys
            // (ADR-0068 d5).
            Ok((caller, access)) => {
                self.admit_request(&caller.key).await?;
                if first_resolution {
                    *caller.gate_read.lock().expect("unpoisoned") = access;
                }
                Ok((caller, session))
            }
            Err(AppError::Store(e)) => Err(AppError::Store(e)),
            Err(_) => Err(AppError::Unauthorized),
        }
    }

    /// A person's own sign-up (ADR-0068 d2), after the terms page: in open
    /// mode, for a provider-verified identity no entry holds, with the
    /// current terms version accepted; an address an entry holds is refused
    /// whatever its tenant or flag. A drawn user, the entry under
    /// `signup.issuer` journalled as `web.signed_up` with the email's
    /// digest, the provider subject pinned. The caller then issues the
    /// session keys and opens a session.
    pub async fn web_sign_up(
        &self,
        identity: &ProviderIdentity,
        accepted_terms: &str,
    ) -> Result<WebEntry, AppError> {
        let policy = self
            .signup_policy()
            .filter(|p| p.mode == SignupMode::Open)
            .ok_or(AppError::NotFound)?;
        if accepted_terms != policy.terms_version {
            return Err(AppError::WebField {
                field: "terms_version",
                message: "the terms changed since this page was shown; read them again",
            });
        }
        let tenant_id = policy.tenant_id;
        let email_hash = identity.email.digest();
        if let Some(entry) = self.journal.web_entry_by_email(&email_hash).await? {
            // Someone holds the address, so this is not a sign-up: a
            // disabled entry stays refused, and a live one signs in through
            // the callback, which pins the provider subject (ADR-0045 d4).
            // Opening a session here would skip that pin for the holder of
            // a cookie issued before the entry existed (ADR-0068 d2).
            return Err(AppError::WebSignIn(if entry.disabled_at.is_some() {
                oml_domain::web::SignInRefusal::Disabled
            } else {
                oml_domain::web::SignInRefusal::StateMismatch
            }));
        }
        let issuer = self.signup_issuer(policy).await?;
        let mut user = None;
        for _ in 0..DRAWS_MAX {
            let drawn = drawn_user(&self.ids.new_secret());
            if self.user_name_free(tenant_id, &drawn).await? {
                user = Some(drawn);
                break;
            }
        }
        let user = user.ok_or_else(|| AppError::NameTaken {
            kind: NamedKind::Workarea,
            name: drawn_user(&[0; 32]),
        })?;
        let entry = WebEntry {
            tenant_id,
            email: identity.email.clone(),
            user,
            admin: false,
            google_subject: None,
            github_subject: None,
            added_by: issuer.key_id,
            terms_version: Some(policy.terms_version.clone()),
            disabled_at: None,
        };
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::WEB_SIGNED_UP,
            &issuer.principal,
            Some(issuer.key_id),
            json!({
                "email_digest": email_hash.to_prefixed(),
                "user": entry.user.as_str(),
                "admin": false,
                "provider": identity.provider.as_str(),
                "terms_version": policy.terms_version,
                "self_signup": true,
                "by_key_id": issuer.key_id.to_string(),
            }),
        )?;
        self.commit(event, MetadataFold::WebEmailAllowed(entry.clone()), None)
            .await?;
        self.journal
            .web_pin(tenant_id, &email_hash, identity.provider, &identity.subject)
            .await?;
        Ok(entry)
    }

    /// The live admin key `signup.issuer` names in the sign-up tenant;
    /// otherwise the start's check failed or the key was revoked since,
    /// and the sign-up fails closed (ADR-0068 d1, d11).
    async fn signup_issuer(&self, policy: &SignupPolicy) -> Result<KeyRecord, AppError> {
        let id = self
            .journal
            .resolve_name(policy.tenant_id, NamedKind::Key, &policy.issuer)
            .await?
            .ok_or(AppError::WebKeyMissing {
                capacity: "signup issuer",
            })?;
        self.journal
            .key(policy.tenant_id, id)
            .await?
            .filter(|k| k.role == Role::Admin && k.revoked_at.is_none())
            .ok_or(AppError::WebKeyMissing {
                capacity: "signup issuer",
            })
    }

    /// Whether the start's settings name a live admin key in an existing
    /// tenant (ADR-0068 d1): the composition stops when not.
    pub async fn check_signup_policy(&self) -> Result<(), String> {
        let Some(policy) = self.signup_policy() else {
            return Ok(());
        };
        self.signup_issuer(policy).await.map(|_| ()).map_err(|_| {
            format!(
                "signup.issuer {} is not a live admin key of the tenant signup.tenant names (ADR-0068 d1)",
                policy.issuer.as_str()
            )
        })
    }

    /// A drawn name no workarea, key or entry of the tenant holds.
    async fn user_name_free(&self, tenant_id: Uuid, user: &Name) -> Result<bool, AppError> {
        if self
            .journal
            .resolve_name(tenant_id, NamedKind::Workarea, user)
            .await?
            .is_some()
        {
            return Ok(false);
        }
        for key in session_key_names(user) {
            if self
                .journal
                .resolve_name(tenant_id, NamedKind::Key, &key)
                .await?
                .is_some()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Whether the person holds any bearer or connector key: a sign-in
    /// whose person holds none lands on the onboarding page (ADR-0068 d10).
    pub async fn web_has_agent_key(&self, entry: &WebEntry) -> Result<bool, AppError> {
        let Some(root) = self.signup_root(entry.tenant_id, &entry.user).await? else {
            return Ok(false);
        };
        Ok(self
            .journal
            .web_user_keys(entry.tenant_id, root)
            .await?
            .iter()
            .any(|k| k.credential != Credential::Session && k.revoked_at.is_none()))
    }
}

/// A session's shipped lifetime until settings are built: `web.session_ttl`'s
/// default (ADR-0045 d17; configuration.md).
pub const WEB_SESSION_TTL_SECS: i64 = SESSION_TTL_DEFAULT_SECS;
