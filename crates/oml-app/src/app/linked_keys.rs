//! Linked tenant keys (ADR-0085 d16, amended 2026-11-17; ADR-0082 d7): an
//! admin links a tenant key a person holds to that person's sign-in, the
//! person accepts the link from their own console session (the operator's
//! decision of 2026-11-19), and from then on the key acts for them and the
//! console acts with it through the session.

use serde_json::json;
use uuid::Uuid;

use oml_domain::name::{Name, NamedKind, Reference};
use oml_domain::{Role, UsedBy, event_types};

use super::{App, Caller, require_role};
use crate::ports::{Clock, IdGen, MetadataFold, Scanner, Signer, WebStore};
use crate::types::{AppError, Bearer, Credential, KeyRecord, WebSessionRecord};

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: WebStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/keys/{key}/link` (ADR-0085 d16): the tenant key's link to
    /// `user`, journalled as `key.linked_person` and pending until `user`
    /// accepts it from their own console session
    /// ([`Self::accept_key_link`]); until then the key acts for no one (the
    /// operator's decision of 2026-11-19). A console admin session only
    /// (`<user>-admin`). Only a live tenant bearer key used by a person, an
    /// admin key's included (the operator, 2026-11-19), that holds no link
    /// and none pending, is linked, to a user root of the tenant whose
    /// person has a sign-in entry that is not disabled; anything else is
    /// [`AppError::KeyLinkRefused`] naming why, and journals nothing.
    pub async fn link_key_to_person(
        &self,
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
        user: &Name,
    ) -> Result<KeyRecord, AppError> {
        let admin = self.key_act_caller(bearer.into(), "link_key").await?.key;
        require_role(&admin, Role::Admin, "link_key")?;
        let own_person = console_admin_person(&admin)?;
        let tenant_id = admin.tenant_id;
        let mut target = self.linkable_key(&admin, &key).await?;
        let root = self
            .journal
            .resolve_name(tenant_id, NamedKind::Workspace, user)
            .await?;
        let root = match root {
            Some(id) => self.journal.workspace(tenant_id, id).await?,
            None => None,
        }
        .filter(|w| w.user_root)
        .ok_or(AppError::KeyLinkRefused {
            reason: "user_unknown",
        })?;
        if !self.person_signs_in(tenant_id, user).await? {
            return Err(AppError::KeyLinkRefused {
                reason: "user_disabled",
            });
        }
        // No self-link: the console admin's own person, its session user
        // (the operator, 2026-11-17: "No self-link only", "Console only").
        if own_person == user {
            return Err(AppError::KeyLinkRefused {
                reason: "self_link",
            });
        }
        target.link_pending = Some(root.workspace_id);
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_LINKED_PERSON,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "key_id": target.key_id.to_string(),
                "user": user.as_str(),
                "user_root_id": root.workspace_id.to_string(),
                "linked_by_key_id": admin.key_id.to_string(),
                "accepted": false,
            }),
        )?;
        self.commit(
            event,
            MetadataFold::KeyLinked {
                key_id: target.key_id,
                user_root: Some(root.workspace_id),
                accepted: false,
            },
            None,
        )
        .await?;
        Ok(target)
    }

    /// `POST /v2/keys/{key}/unlink` (ADR-0085 d16): the tenant key acts for
    /// no one from this commit on, journalled as `key.unlinked_person`; a
    /// link still pending is withdrawn the same way. Admin key role only; a
    /// key with neither is [`AppError::KeyLinkRefused`] with `not_linked`.
    pub async fn unlink_key_from_person(
        &self,
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
    ) -> Result<KeyRecord, AppError> {
        let admin = self.key_act_caller(bearer.into(), "unlink_key").await?.key;
        require_role(&admin, Role::Admin, "unlink_key")?;
        console_admin_person(&admin)?;
        let tenant_id = admin.tenant_id;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        let mut target = self
            .journal
            .key(tenant_id, key_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if target.linked_user.is_none() && target.link_pending.is_none() {
            return Err(AppError::KeyLinkRefused {
                reason: "not_linked",
            });
        }
        target.linked_user = None;
        target.link_pending = None;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_UNLINKED_PERSON,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "key_id": target.key_id.to_string(),
                "unlinked_by_key_id": admin.key_id.to_string(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::KeyLinked {
                key_id: target.key_id,
                user_root: None,
                accepted: false,
            },
            None,
        )
        .await?;
        Ok(target)
    }

    /// `POST /v2/keys/{key}/accept_link` (ADR-0085 d16, the operator's
    /// decision of 2026-11-19): the person a pending link names accepts it
    /// from their own console session, journalled as `key.link_accepted`;
    /// the key acts for them from this commit on. Refused, journalling
    /// nothing, with [`AppError::KeyLinkRefused`]: a caller that is not a
    /// console session, `session_only`; a key with no link pending,
    /// `not_pending`; a session of any other person, the admin who made
    /// the link included, `not_named_person`; a key no longer live or no
    /// longer one a link may name, `revoked` or `not_used_by_person`.
    /// Returns the key and the person it now acts for.
    pub async fn accept_key_link(
        &self,
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
    ) -> Result<(KeyRecord, Name), AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?.key;
        let Some(session_user) = caller
            .session_user
            .as_ref()
            .filter(|_| caller.credential == Credential::Session)
        else {
            return Err(AppError::KeyLinkRefused {
                reason: "session_only",
            });
        };
        let tenant_id = caller.tenant_id;
        let key_id = self.resolve(&caller, NamedKind::Key, &key).await?;
        let mut target = self
            .journal
            .key(tenant_id, key_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let Some(root_id) = target.link_pending else {
            return Err(AppError::KeyLinkRefused {
                reason: "not_pending",
            });
        };
        let root = self
            .journal
            .workspace(tenant_id, root_id)
            .await?
            .filter(|w| w.user_root);
        let Some(root) = root.filter(|w| &w.name == session_user) else {
            return Err(AppError::KeyLinkRefused {
                reason: "not_named_person",
            });
        };
        if target.revoked_at.is_some() || self.key_expired(&target) {
            return Err(AppError::KeyLinkRefused { reason: "revoked" });
        }
        if target.used_by != UsedBy::Person {
            return Err(AppError::KeyLinkRefused {
                reason: "not_used_by_person",
            });
        }
        target.linked_user = Some(root_id);
        target.link_pending = None;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_LINK_ACCEPTED,
            &caller.principal,
            Some(caller.key_id),
            json!({
                "key_id": target.key_id.to_string(),
                "user": root.name.as_str(),
                "user_root_id": root_id.to_string(),
                "accepted_by_key_id": caller.key_id.to_string(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::KeyLinkAccepted {
                key_id: target.key_id,
                user_root: root_id,
            },
            None,
        )
        .await?;
        Ok((target, root.name))
    }

    /// The key `key` names, if a link may name it: a live tenant bearer key
    /// used by a person, an admin key's included (ADR-0085 d16, the
    /// operator's decision of 2026-11-19).
    async fn linkable_key(
        &self,
        admin: &KeyRecord,
        key: &Reference,
    ) -> Result<KeyRecord, AppError> {
        let key_id = self.resolve(admin, NamedKind::Key, key).await?;
        let target = self
            .journal
            .key(admin.tenant_id, key_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let reason = if target.revoked_at.is_some() || self.key_expired(&target) {
            Some("revoked")
        } else if target.linked_user.is_some() || target.link_pending.is_some() {
            // One link at a time: an unlink comes first, journalled
            // (ADR-0085 d16, amended 2026-11-17).
            Some("already_linked")
        } else if target.user.is_some() || target.credential != Credential::Bearer {
            Some("not_a_tenant_key")
        } else if target.used_by != UsedBy::Person {
            Some("not_used_by_person")
        } else {
            None
        };
        match reason {
            Some(reason) => Err(AppError::KeyLinkRefused { reason }),
            None => Ok(target),
        }
    }

    /// Whether `user` has a sign-in entry in the tenant and none of its
    /// entries is disabled (ADR-0068 d8).
    async fn person_signs_in(&self, tenant_id: Uuid, user: &Name) -> Result<bool, AppError> {
        let mut after = None;
        let mut found = false;
        loop {
            let page = self
                .journal
                .web_entries(tenant_id, after.as_ref(), 500)
                .await?;
            for entry in page.iter().filter(|e| &e.user == user && !e.pending) {
                if entry.disabled_at.is_some() {
                    return Ok(false);
                }
                found = true;
            }
            match page.last() {
                Some(last) if page.len() == 500 => after = Some(last.email.digest()),
                _ => return Ok(found),
            }
        }
    }

    /// The caller a console request acts as when it names `key_id` in
    /// `OML-Acting-Key` beside `session_caller`'s capacity credential
    /// (ADR-0085 d16, amended 2026-11-17): that key, when it is of the
    /// session's tenant, linked to the session's own user, live and used by
    /// a person; otherwise [`AppError::ActingKeyNotLinked`]. Its own
    /// request rate is charged.
    pub async fn acting_caller(
        &self,
        session_caller: &Caller,
        session: &WebSessionRecord,
        key_id: Uuid,
    ) -> Result<Caller, AppError> {
        let tenant_id = session.tenant_id;
        if session_caller.key.tenant_id != tenant_id {
            return Err(AppError::ActingKeyNotLinked);
        }
        let Some(key) = self.journal.key(tenant_id, key_id).await? else {
            return Err(AppError::ActingKeyNotLinked);
        };
        let Some(root) = key.linked_user else {
            return Err(AppError::ActingKeyNotLinked);
        };
        let owner = self.journal.workspace(tenant_id, root).await?;
        let linked_to_session = owner.is_some_and(|w| w.user_root && w.name == session.user);
        // Every condition a link was made under is read again here: a key
        // that is no longer one a link may name acts for no one.
        if !linked_to_session
            || key.revoked_at.is_some()
            || self.key_expired(&key)
            || key.used_by != UsedBy::Person
            || key.role == Role::Admin
            || key.credential != Credential::Bearer
            || key.user.is_some()
        {
            return Err(AppError::ActingKeyNotLinked);
        }
        self.admit_request(&key).await?;
        self.caller_of(key).await
    }
}

/// The person of a console admin session key, `<user>-admin`, the one
/// caller that links and unlinks a key: a bearer admin key names no
/// person, so no self-link could be refused for it (the operator,
/// 2026-11-17: "Console only"; ADR-0085 d16).
fn console_admin_person(admin: &KeyRecord) -> Result<&Name, AppError> {
    match (&admin.credential, &admin.session_user) {
        (Credential::Session, Some(user)) => Ok(user),
        _ => Err(AppError::KeyLinkRefused {
            reason: "session_admin_only",
        }),
    }
}
