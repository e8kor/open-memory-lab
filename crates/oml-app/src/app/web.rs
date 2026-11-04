//! The web pages' use cases (ADR-0045): the email allowlist an admin key
//! keeps over `/v2/web/emails` (d2), the admission of a provider's answer
//! against it (d3, d4), and the session rows a signed-in browser acts
//! through (d5). The pages themselves are the transport's.
//!
//! The person's session keys (d6) rest on ADR-0044's user keys and user
//! roots, which another change builds; until it lands, [`App::web_session_keys`]
//! is the one seam that answers [`AppError::WebNotBuilt`], and the user
//! name's workarea check reads every workarea as shared, which is exactly
//! true while no user root exists.

use serde_json::json;
use uuid::Uuid;

use oml_domain::name::{Name, NamedKind};
use oml_domain::web::{
    Email, Provider, ProviderIdentity, SESSION_TTL_DEFAULT_SECS, SignInRefusal, SignInState,
    admit_github, admit_google_id_token, check_user, session_key_names,
};
use oml_domain::{EventTime, Role, Sha256Hash, event_types};

use super::{App, require_role};
use crate::ports::{
    Clock, IdGen, MetadataFold, Scanner, SignInExchange, Signer, StoreError, WebStore,
};
use crate::types::{
    AppError, OpenedSession, PinOutcome, ProviderAnswer, ProviderFailure, WebEmailOutcome,
    WebEmailRequest, WebEntry, WebSessionRecord,
};

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
    /// `POST /v2/web/emails`, admin by key role (ADR-0045 d2): adds the
    /// entry, journalled as `web.email_allowed`, or changes the user or the
    /// admin flag of the tenant's entry for that email, journalled as
    /// `web.email_changed`, which ends its sessions in the same commit. The
    /// journal holds the email's digest, never the address. An email
    /// another tenant's entry holds is [`AppError::WebEmailTaken`]; a user
    /// a workarea's name holds is `name.taken` kind `workarea`; a user
    /// whose composed session key names another key holds, while no entry
    /// names that user, is `name.taken` kind `key`.
    pub async fn allow_web_email(
        &self,
        api_key: &str,
        req: &WebEmailRequest,
    ) -> Result<(WebEntry, WebEmailOutcome), AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "web_emails")?;
        check_user(&req.user)?;
        let tenant_id = admin.tenant_id;
        let email_hash = req.email.digest();
        let existing = self.journal.web_entry_by_email(&email_hash).await?;
        let outcome = match &existing {
            Some(entry) if entry.tenant_id != tenant_id => return Err(AppError::WebEmailTaken),
            Some(entry) if entry.user == req.user && entry.admin == req.admin => {
                return Ok((entry.clone(), WebEmailOutcome::Unchanged));
            }
            Some(_) => WebEmailOutcome::Changed,
            None => WebEmailOutcome::Allowed,
        };
        let user_known = self.web_user_known(tenant_id, &req.user).await?;
        if !user_known {
            self.require_free_user(tenant_id, &req.user).await?;
        }
        let entry = WebEntry {
            tenant_id,
            email: req.email.clone(),
            user: req.user.clone(),
            admin: req.admin,
            google_subject: existing.as_ref().and_then(|e| e.google_subject.clone()),
            github_subject: existing.as_ref().and_then(|e| e.github_subject.clone()),
        };
        let (event_type, fold) = match outcome {
            WebEmailOutcome::Changed => (
                event_types::WEB_EMAIL_CHANGED,
                MetadataFold::WebEmailChanged(entry.clone()),
            ),
            _ => (
                event_types::WEB_EMAIL_ALLOWED,
                MetadataFold::WebEmailAllowed(entry.clone()),
            ),
        };
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_type,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "email_digest": email_hash.to_prefixed(),
                "user": entry.user.as_str(),
                "admin": entry.admin,
                "by_key_id": admin.key_id.to_string(),
            }),
        )?;
        self.commit(event, fold, None).await?;
        Ok((entry, outcome))
    }

    /// `POST /v2/web/emails/remove`, admin by key role (ADR-0045 d2, d13):
    /// the entry and its sessions end in one commit journalled as
    /// `web.email_removed` with the email's digest. An email the tenant's
    /// list does not hold is absent. The user's agent keys are untouched.
    pub async fn remove_web_email(
        &self,
        api_key: &str,
        email: &Email,
    ) -> Result<(WebEntry, u64, Uuid), AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "web_emails")?;
        let tenant_id = admin.tenant_id;
        let email_hash = email.digest();
        let entry = self
            .journal
            .web_entry_by_email(&email_hash)
            .await?
            .filter(|e| e.tenant_id == tenant_id)
            .ok_or(AppError::NotFound)?;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::WEB_EMAIL_REMOVED,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "email_digest": email_hash.to_prefixed(),
                "user": entry.user.as_str(),
                "admin": entry.admin,
                "by_key_id": admin.key_id.to_string(),
            }),
        )?;
        let event_id = event.envelope.event_id;
        let receipt = self
            .commit(event, MetadataFold::WebEmailRemoved { email_hash }, None)
            .await?;
        Ok((entry, receipt.seq, event_id))
    }

    /// `GET /v2/web/emails`, admin by key role: the tenant's entries in the
    /// order of their email's digest after the digest `after`, at most
    /// `limit` (ADR-0045 d2).
    pub async fn web_emails(
        &self,
        api_key: &str,
        after: Option<&Sha256Hash>,
        limit: usize,
    ) -> Result<Vec<WebEntry>, AppError> {
        let admin = self.authenticate(api_key).await?.key;
        require_role(&admin, Role::Admin, "web_emails")?;
        Ok(self
            .journal
            .web_entries(admin.tenant_id, after, limit)
            .await?)
    }

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
        let entry = self
            .journal
            .web_entry_by_email(&email_hash)
            .await
            .map_err(store)?
            .ok_or(refused(SignInRefusal::NotAllowed))?;
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
        self.journal
            .web_session_open(
                record.clone(),
                replaces.map(|old| Sha256Hash::digest(old.as_bytes())),
            )
            .await?;
        Ok(OpenedSession { token, record })
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

    /// The person's session keys (ADR-0045 d6): `<user>-web`, a user key
    /// that creates `universe.<user>`, and `<user>-review`, issued at the
    /// first sign-in. They rest on ADR-0044's user keys and user roots,
    /// which are not built in this build, so this seam answers
    /// [`AppError::WebNotBuilt`] for every act that needs them; the
    /// signed-in page shows who the person is without them.
    pub fn web_session_keys(&self, _session: &WebSessionRecord) -> Result<(), AppError> {
        Err(AppError::WebNotBuilt {
            what: "session keys",
        })
    }

    /// Whether an entry of the tenant already names `user`: its session key
    /// names are then its own, not another key's.
    async fn web_user_known(&self, tenant_id: Uuid, user: &Name) -> Result<bool, AppError> {
        let mut after: Option<Sha256Hash> = None;
        loop {
            let page = self
                .journal
                .web_entries(tenant_id, after.as_ref(), 200)
                .await?;
            if page.iter().any(|e| &e.user == user) {
                return Ok(true);
            }
            match page.last() {
                Some(last) if page.len() == 200 => after = Some(last.email.digest()),
                _ => return Ok(false),
            }
        }
    }

    /// A new user's name must not be a workarea's, nor compose a session
    /// key name another key holds (ADR-0045 d2). Every workarea reads as
    /// shared here, which holds until ADR-0044's user roots exist; that
    /// change exempts the user's own root.
    async fn require_free_user(&self, tenant_id: Uuid, user: &Name) -> Result<(), AppError> {
        if self
            .journal
            .resolve_name(tenant_id, NamedKind::Workarea, user)
            .await?
            .is_some()
        {
            return Err(AppError::NameTaken {
                kind: NamedKind::Workarea,
                name: user.clone(),
            });
        }
        for key_name in session_key_names(user) {
            if self
                .journal
                .resolve_name(tenant_id, NamedKind::Key, &key_name)
                .await?
                .is_some()
            {
                return Err(AppError::NameTaken {
                    kind: NamedKind::Key,
                    name: key_name,
                });
            }
        }
        Ok(())
    }
}

/// A session's shipped lifetime until settings are built: `web.session_ttl`'s
/// default (ADR-0045 d17; configuration.md).
pub const WEB_SESSION_TTL_SECS: i64 = SESSION_TTL_DEFAULT_SECS;
