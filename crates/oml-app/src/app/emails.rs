//! The email allowlist an admin key keeps over `/v2/web/emails`
//! (ADR-0045 d2, d10): adding, changing and removing an entry, and the
//! listing. Signing in against it is the auth context's (`auth.rs`).

use serde_json::json;
use uuid::Uuid;

use oml_domain::name::{Name, NamedKind};
use oml_domain::web::{Email, check_user, session_key_names};
use oml_domain::{Role, Sha256Hash, event_types};

use super::{App, require_role};
use crate::ports::{Clock, IdGen, MetadataFold, Scanner, Signer, WebStore};
use crate::types::{AppError, WebEmailOutcome, WebEmailRequest, WebEntry};
use crate::types::{Bearer, Credential, KeyRecord};

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
    /// journal holds the email's digest, never the address. An entry added
    /// for a person who signs in already, or changed to name another such
    /// person, is pending until that person accepts it
    /// ([`Self::accept_web_email`]); it signs in to nothing meanwhile (the
    /// operator's decision of 2026-11-19, ADR-0045 d2's note). An email
    /// another tenant's entry holds is [`AppError::WebEmailTaken`]; a user
    /// a workspace's name holds is `name.taken` kind `workspace`; a user
    /// whose composed session key names another key holds, while no entry
    /// names that user, is `name.taken` kind `key`.
    pub async fn allow_web_email(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &WebEmailRequest,
    ) -> Result<(WebEntry, WebEmailOutcome), AppError> {
        self.allow_web_email_keeping(bearer, req, false).await
    }

    /// [`Self::allow_web_email`], refusing with `keep_an_admin` to un-flag
    /// the tenant's last admin entry, checked in the commit's own
    /// transaction (ADR-0045 d10; ADR-0070 d5).
    pub async fn allow_web_email_keeping(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &WebEmailRequest,
        keep_an_admin: bool,
    ) -> Result<(WebEntry, WebEmailOutcome), AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
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
        // An entry that keeps its person keeps its state; one that comes to
        // name a person who signs in already waits on them (the operator's
        // decision of 2026-11-19, ADR-0045 d2's note).
        let pending = match &existing {
            Some(entry) if entry.user == req.user => entry.pending,
            _ => self.person_exists(tenant_id, &req.user).await?,
        };
        let entry = WebEntry {
            tenant_id,
            email: req.email.clone(),
            user: req.user.clone(),
            admin: req.admin,
            google_subject: existing.as_ref().and_then(|e| e.google_subject.clone()),
            github_subject: existing.as_ref().and_then(|e| e.github_subject.clone()),
            added_by: existing.as_ref().map_or(admin.key_id, |e| e.added_by),
            terms_version: existing.as_ref().and_then(|e| e.terms_version.clone()),
            disabled_at: existing.as_ref().and_then(|e| e.disabled_at.clone()),
            pending,
        };
        let (event_type, fold) = match outcome {
            WebEmailOutcome::Changed => (
                event_types::WEB_EMAIL_CHANGED,
                MetadataFold::WebEmailChanged {
                    entry: entry.clone(),
                    keep_an_admin,
                },
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
                "pending": entry.pending,
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
        bearer: impl Into<Bearer<'_>>,
        email: &Email,
    ) -> Result<(WebEntry, u64, Uuid), AppError> {
        self.remove_web_email_keeping(bearer, email, false).await
    }

    /// [`Self::remove_web_email`], refusing with `keep_an_admin` to remove
    /// the tenant's last admin entry, checked in the commit's own
    /// transaction (ADR-0045 d10; ADR-0070 d5).
    pub async fn remove_web_email_keeping(
        &self,
        bearer: impl Into<Bearer<'_>>,
        email: &Email,
        keep_an_admin: bool,
    ) -> Result<(WebEntry, u64, Uuid), AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
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
                "pending": entry.pending,
            }),
        )?;
        let event_id = event.envelope.event_id;
        let receipt = self
            .commit(
                event,
                MetadataFold::WebEmailRemoved {
                    email_hash,
                    keep_an_admin,
                },
                None,
            )
            .await?;
        Ok((entry, receipt.seq, event_id))
    }

    /// `POST /v2/web/emails/accept` (the operator's decision of
    /// 2026-11-19, ADR-0045 d2's note): the person a pending entry names
    /// accepts it from a console session they already have, journalled as
    /// `web.email_accepted` with the email's digest; it signs in as them
    /// from this commit on. Refused, journalling nothing, with
    /// [`AppError::WebEmailRefused`]: a caller that is not a console
    /// session, `session_only`; an entry that waits on no one,
    /// `not_pending`; a session of any other person, the adding admin's
    /// included, `not_named_person`. An email no entry of the caller's
    /// tenant holds is [`AppError::NotFound`].
    pub async fn accept_web_email(
        &self,
        bearer: impl Into<Bearer<'_>>,
        email: &Email,
    ) -> Result<WebEntry, AppError> {
        let (caller, mut entry) = self.pending_entry_of_caller(bearer.into(), email).await?;
        let email_hash = email.digest();
        let prev = self.head_event(caller.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            caller.tenant_id,
            event_types::WEB_EMAIL_ACCEPTED,
            &caller.principal,
            Some(caller.key_id),
            json!({
                "email_digest": email_hash.to_prefixed(),
                "user": entry.user.as_str(),
                "accepted_by_key_id": caller.key_id.to_string(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::WebEmailAccepted {
                email_hash,
                user: entry.user.clone(),
            },
            None,
        )
        .await?;
        entry.pending = false;
        Ok(entry)
    }

    /// `POST /v2/web/emails/decline` (the operator's decision of
    /// 2026-11-19, ADR-0045 d2's note): the person a pending entry names
    /// removes it from a console session they already have, journalled as
    /// `web.email_removed` with `pending: true` and their session key as
    /// `by_key_id`. The refusals are [`Self::accept_web_email`]'s. An admin
    /// removes a pending entry as any other ([`Self::remove_web_email`]).
    pub async fn decline_web_email(
        &self,
        bearer: impl Into<Bearer<'_>>,
        email: &Email,
    ) -> Result<WebEntry, AppError> {
        let (caller, entry) = self.pending_entry_of_caller(bearer.into(), email).await?;
        let email_hash = email.digest();
        let prev = self.head_event(caller.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            caller.tenant_id,
            event_types::WEB_EMAIL_REMOVED,
            &caller.principal,
            Some(caller.key_id),
            json!({
                "email_digest": email_hash.to_prefixed(),
                "user": entry.user.as_str(),
                "admin": entry.admin,
                "by_key_id": caller.key_id.to_string(),
                "pending": true,
            }),
        )?;
        self.commit(
            event,
            MetadataFold::WebEmailRemoved {
                email_hash,
                keep_an_admin: false,
            },
            None,
        )
        .await?;
        Ok(entry)
    }

    /// The caller, a console session, and the tenant's pending entry for
    /// `email` that names the session's own person; otherwise the refusal
    /// [`Self::accept_web_email`] names.
    async fn pending_entry_of_caller(
        &self,
        bearer: Bearer<'_>,
        email: &Email,
    ) -> Result<(KeyRecord, WebEntry), AppError> {
        let caller = self.authenticate_bearer(bearer).await?.key;
        let Some(session_user) = caller
            .session_user
            .clone()
            .filter(|_| caller.credential == Credential::Session)
        else {
            return Err(AppError::WebEmailRefused {
                reason: "session_only",
            });
        };
        let entry = self
            .journal
            .web_entry_by_email(&email.digest())
            .await?
            .filter(|e| e.tenant_id == caller.tenant_id)
            .ok_or(AppError::NotFound)?;
        if !entry.pending {
            return Err(AppError::WebEmailRefused {
                reason: "not_pending",
            });
        }
        if entry.user != session_user {
            return Err(AppError::WebEmailRefused {
                reason: "not_named_person",
            });
        }
        Ok((caller, entry))
    }

    /// Whether `user` is a person who signs in already: an entry of the
    /// tenant names them, or their session keys exist, which their first
    /// sign-in made and nothing deletes (ADR-0045 d6). A user root no one
    /// signed in to takes its first email at once (the build's reading of
    /// the operator's decision of 2026-11-19, ADR-0045 d2's note).
    async fn person_exists(&self, tenant_id: Uuid, user: &Name) -> Result<bool, AppError> {
        if self.web_user_known(tenant_id, user).await? {
            return Ok(true);
        }
        Ok(!self
            .journal
            .web_session_keys(tenant_id, user)
            .await?
            .is_empty())
    }

    /// `GET /v2/web/emails`, admin by key role: the tenant's entries in the
    /// order of their email's digest after the digest `after`, at most
    /// `limit` (ADR-0045 d2).
    pub async fn list_web_emails(
        &self,
        bearer: impl Into<Bearer<'_>>,
        after: Option<&Sha256Hash>,
        limit: usize,
    ) -> Result<Vec<WebEntry>, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "web_emails")?;
        Ok(self
            .journal
            .web_entries(admin.tenant_id, after, limit)
            .await?)
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

    /// A new user's name must not be a shared workspace's, nor compose a
    /// session key name another key holds (ADR-0045 d2). The user's own
    /// root, which a user key naming them created, is theirs, and so are
    /// session keys serving them (ADR-0044 d1, d2).
    async fn require_free_user(&self, tenant_id: Uuid, user: &Name) -> Result<(), AppError> {
        if let Some(id) = self
            .journal
            .resolve_name(tenant_id, NamedKind::Workspace, user)
            .await?
            && !self
                .journal
                .workspace(tenant_id, id)
                .await?
                .is_some_and(|w| w.user_root)
        {
            return Err(AppError::NameTaken {
                kind: NamedKind::Workspace,
                name: user.clone(),
            });
        }
        for key_name in session_key_names(user) {
            if let Some(id) = self
                .journal
                .resolve_name(tenant_id, NamedKind::Key, &key_name)
                .await?
                && !self
                    .journal
                    .key(tenant_id, id)
                    .await?
                    .is_some_and(|k| k.session_user.as_ref() == Some(user))
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
