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
use crate::types::Bearer;
use crate::types::{AppError, WebEmailOutcome, WebEmailRequest, WebEntry};

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

    /// `GET /v2/web/emails`, admin by key role: the tenant's entries in the
    /// order of their email's digest after the digest `after`, at most
    /// `limit` (ADR-0045 d2).
    pub async fn web_emails(
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

    /// A new user's name must not be a shared workarea's, nor compose a
    /// session key name another key holds (ADR-0045 d2). The user's own
    /// root, which a user key naming them created, is theirs, and so are
    /// session keys serving them (ADR-0044 d1, d2).
    async fn require_free_user(&self, tenant_id: Uuid, user: &Name) -> Result<(), AppError> {
        if let Some(id) = self
            .journal
            .resolve_name(tenant_id, NamedKind::Workarea, user)
            .await?
            && !self
                .journal
                .workarea(tenant_id, id)
                .await?
                .is_some_and(|w| w.user_root)
        {
            return Err(AppError::NameTaken {
                kind: NamedKind::Workarea,
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
