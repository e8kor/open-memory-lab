//! The email allowlist an admin key keeps over `/v2/web/emails`
//! (ADR-0045 d2, d10): adding, changing and removing an entry, and the
//! listing. Signing in against it is the auth context's (`auth.rs`).

use serde_json::json;
use uuid::Uuid;

use oml_domain::admin_act::{AdminActClass, AdminActKind};
use oml_domain::name::{Name, NamedKind};
use oml_domain::web::{Email, check_user, session_key_names};
use oml_domain::{Sha256Hash, event_types};

use super::App;
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
        // The last live admin entry is kept on every surface, a bearer admin
        // key's included (ADR-0097 d25, d27 item 4, reversing ADR-0045 d10's
        // CLI allowance).
        self.allow_web_email_keeping(bearer, req, true).await
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
        self.allow_web_email_with_reason(bearer, req, keep_an_admin, None)
            .await
    }

    /// [`Self::allow_web_email_keeping`] with the admin's reason, which an
    /// admin flag that waits on a second admin person requires (ADR-0097
    /// d26).
    ///
    /// # Errors
    /// As [`Self::allow_web_email`], and a pending flag sent no reason.
    pub async fn allow_web_email_with_reason(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &WebEmailRequest,
        keep_an_admin: bool,
        reason: Option<&str>,
    ) -> Result<(WebEntry, WebEmailOutcome), AppError> {
        let admin = self.admin_caller(bearer.into(), "web_emails").await?.key;
        let Some((entry, outcome, event_type, fold, payload)) =
            self.entry_change(&admin, req, keep_an_admin).await?
        else {
            let held = self
                .journal
                .web_entry_by_email(&req.email.digest())
                .await?
                .ok_or(AppError::NotFound)?;
            return Ok((held, WebEmailOutcome::Unchanged));
        };
        let tenant_id = admin.tenant_id;
        let existing = self.journal.web_entry_by_email(&req.email.digest()).await?;
        // A new admin person, or the admin flag on an existing one, is
        // widening: outside `sole_admin` it waits on a second admin person
        // and nothing changes until then (ADR-0097 d5). An entry for a new
        // person with no flag is neutral (d9).
        let flagged = req.admin && !existing.as_ref().is_some_and(|e| e.admin);
        // Moving a live admin flag off its person is that person's own act,
        // which revokes their admin keys, or an act against another admin
        // person, which suspends them (ADR-0097 d8, d21).
        let standing = self
            .standing_act(&admin, existing.as_ref(), Some(req))
            .await?;
        let kind = if let Some(kind) = standing {
            kind
        } else if flagged {
            AdminActKind::AllowAdmin
        } else if outcome == WebEmailOutcome::Allowed {
            AdminActKind::AllowPerson
        } else {
            AdminActKind::ChangePerson
        };
        let (target, effect) = match (standing, &existing) {
            (Some(_), Some(e)) => (
                json!({ "user": e.user.as_str(), "admin": req.admin, "to_user": req.user.as_str(),
                        "email_digest": req.email.digest().to_prefixed() }),
                json!({
                    "before": { "email": req.email.as_str(), "user": e.user.as_str(),
                                "admin": true, "disabled": e.disabled_at.is_some() },
                    "after": { "user": req.user.as_str(), "admin": req.admin },
                }),
            ),
            _ => (
                json!({ "user": req.user.as_str(), "admin": req.admin,
                        "email_digest": req.email.digest().to_prefixed() }),
                json!({ "email": req.email.as_str(), "user": req.user.as_str(), "admin": req.admin }),
            ),
        };
        let prev = self.head_event(tenant_id).await?;
        let act = self
            .begin_admin_act(&admin, kind, target, reason, effect, prev.as_ref())
            .await?;
        if act.pending() {
            self.commit_with_act(Some(act), Vec::new()).await?;
            return Ok((
                existing.unwrap_or(entry),
                WebEmailOutcome::PendingConfirmation,
            ));
        }
        let event = self.seal(
            Some(&act.event),
            tenant_id,
            event_type,
            &admin.principal,
            Some(admin.key_id),
            super::admin_acts::with_act_id(payload, act.act_id()),
        )?;
        let mut links = vec![(event.clone(), fold)];
        if let (Some(kind), Some(e)) = (standing, &existing) {
            links.extend(
                self.standing_links(&admin, act.act_id(), kind, &e.user, &event)
                    .await?,
            );
        }
        self.commit_with_act(Some(act), links).await?;
        Ok((entry, outcome))
    }

    /// The change `req` makes to the allowlist under `admin`: the entry as
    /// it will stand, the outcome, and the event and fold that journal it,
    /// or `None` for a request that changes nothing. Shared by the act and
    /// by a confirmation of a pending admin flag (ADR-0097 d5).
    pub(crate) async fn entry_change(
        &self,
        admin: &KeyRecord,
        req: &WebEmailRequest,
        keep_an_admin: bool,
    ) -> Result<
        Option<(
            WebEntry,
            WebEmailOutcome,
            &'static str,
            MetadataFold,
            serde_json::Value,
        )>,
        AppError,
    > {
        check_user(&req.user)?;
        let tenant_id = admin.tenant_id;
        let email_hash = req.email.digest();
        let existing = self.journal.web_entry_by_email(&email_hash).await?;
        let outcome = match &existing {
            Some(entry) if entry.tenant_id != tenant_id => return Err(AppError::WebEmailTaken),
            Some(entry) if entry.user == req.user && entry.admin == req.admin => {
                return Ok(None);
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
            // A change that leaves or makes an entry wait on its person
            // records the admin key that made it, the one the person is
            // shown as having last set it (the operator's decision of
            // 2026-11-20); an accepted entry keeps the key that added it.
            added_by: match &existing {
                Some(e) if !pending => e.added_by,
                _ => admin.key_id,
            },
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
        let payload = json!({
            "email_digest": email_hash.to_prefixed(),
            "user": entry.user.as_str(),
            "admin": entry.admin,
            "by_key_id": admin.key_id.to_string(),
            "pending": entry.pending,
        });
        Ok(Some((entry, outcome, event_type, fold, payload)))
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
        // As in [`Self::allow_web_email`]: never the last live admin entry
        // (ADR-0097 d25, d27 item 4).
        self.remove_web_email_keeping(bearer, email, true).await
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
        self.remove_web_email_with_reason(bearer, email, keep_an_admin, None)
            .await
    }

    /// [`Self::remove_web_email_keeping`] with the admin's reason, which
    /// removing another admin person's entry requires (ADR-0097 d8, d26).
    ///
    /// # Errors
    /// As [`Self::remove_web_email`], and an act against an admin sent no
    /// reason.
    pub async fn remove_web_email_with_reason(
        &self,
        bearer: impl Into<Bearer<'_>>,
        email: &Email,
        keep_an_admin: bool,
        reason: Option<&str>,
    ) -> Result<(WebEntry, u64, Uuid), AppError> {
        let admin = self.admin_caller(bearer.into(), "web_emails").await?.key;
        let tenant_id = admin.tenant_id;
        let email_hash = email.digest();
        let entry = self
            .journal
            .web_entry_by_email(&email_hash)
            .await?
            .filter(|e| e.tenant_id == tenant_id)
            .ok_or(AppError::NotFound)?;
        // Removing an entry is narrowing; an admin person's own removal
        // revokes their admin keys, and another admin person's is an act
        // against an admin that suspends them (ADR-0097 d3, d8, d21).
        let standing = self.standing_act(&admin, Some(&entry), None).await?;
        let kind = standing.unwrap_or(AdminActKind::RemovePerson);
        let prev = self.head_event(tenant_id).await?;
        let act = self
            .admin_act_of(
                &admin,
                kind,
                json!({ "user": entry.user.as_str(), "email_digest": email_hash.to_prefixed() }),
                reason,
                prev.as_ref(),
            )
            .await?;
        let act = match act {
            Some(act) if kind.class() == AdminActClass::AgainstAdmin => {
                let mut act = act;
                // The entry as it was, its pinned subjects included, so the
                // contest-only sign-in matches a removed admin as their entry
                // stood (ADR-0097 d8, the operator's decision of 2026-11-27).
                act.record.effect = json!({
                    "before": { "email": email.as_str(), "user": entry.user.as_str(),
                                "admin": entry.admin, "disabled": entry.disabled_at.is_some(),
                                "google_subject": entry.google_subject,
                                "github_subject": entry.github_subject },
                });
                Some(act)
            }
            other => other,
        };
        let prev = act.as_ref().map(|a| a.event.clone()).or(prev);
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::WEB_EMAIL_REMOVED,
            &admin.principal,
            Some(admin.key_id),
            super::admin_acts::named(
                json!({
                    "email_digest": email_hash.to_prefixed(),
                    "user": entry.user.as_str(),
                    "admin": entry.admin,
                    "by_key_id": admin.key_id.to_string(),
                    "pending": entry.pending,
                }),
                act.as_ref(),
            ),
        )?;
        let event_id = event.envelope.event_id;
        let mut links = vec![(
            event.clone(),
            MetadataFold::WebEmailRemoved {
                email_hash,
                keep_an_admin,
            },
        )];
        if let (Some(kind), Some(act)) = (standing, &act) {
            links.extend(
                self.standing_links(&admin, act.act_id(), kind, &entry.user, &event)
                    .await?,
            );
        }
        let receipt = self.commit_with_act(act, links).await?;
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
    /// included, `not_named_person`; an entry whose admin flag is not
    /// `admin`, the flag the person reviewed, `changed` (the operator's
    /// decision of 2026-11-20). An email no entry of the caller's tenant
    /// holds is [`AppError::NotFound`]. The fold matches the flag too, so a
    /// flip between this read and the commit fails the commit.
    pub async fn accept_web_email(
        &self,
        bearer: impl Into<Bearer<'_>>,
        email: &Email,
        admin: bool,
    ) -> Result<WebEntry, AppError> {
        let (caller, mut entry) = self.pending_entry_of_caller(bearer.into(), email).await?;
        if entry.admin != admin {
            return Err(AppError::WebEmailRefused { reason: "changed" });
        }
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
                "admin": entry.admin,
                "accepted_by_key_id": caller.key_id.to_string(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::WebEmailAccepted {
                email_hash,
                user: entry.user.clone(),
                admin: entry.admin,
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

    /// Whether `user` is an existing person: an entry of the tenant names
    /// them, their session keys exist, which their first sign-in made and
    /// nothing deletes (ADR-0045 d6), or a key names their user root as its
    /// `user_root_id`, its `linked_user_id` or its `link_pending_user_id`
    /// (ADR-0044 d2, ADR-0085 d16). Such a person's first email an admin adds
    /// waits on them; a user root no key names, and a name no root holds,
    /// take their first email at once (the operator's decisions of
    /// 2026-11-19 and 2026-11-20, ADR-0045 d2's note).
    async fn person_exists(&self, tenant_id: Uuid, user: &Name) -> Result<bool, AppError> {
        if self.web_user_known(tenant_id, user).await? {
            return Ok(true);
        }
        if !self
            .journal
            .web_session_keys(tenant_id, user)
            .await?
            .is_empty()
        {
            return Ok(true);
        }
        let Some(id) = self
            .journal
            .resolve_name(tenant_id, NamedKind::Workspace, user)
            .await?
        else {
            return Ok(false);
        };
        if !self
            .journal
            .workspace(tenant_id, id)
            .await?
            .is_some_and(|w| w.user_root)
        {
            return Ok(false);
        }
        if !self.journal.web_user_keys(tenant_id, id).await?.is_empty() {
            return Ok(true);
        }
        Ok(!self
            .journal
            .web_linked_keys(tenant_id, id)
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
        let admin = self.admin_caller(bearer.into(), "web_emails").await?.key;
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
