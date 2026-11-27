//! Acts on an admin person's standing (ADR-0097 d8, d21). An admin person's
//! own un-flag, removal or closed deletion revokes their admin keys at
//! once, `cause: deactivation`. Another admin's un-flag, removal, disable or
//! change of an admin person's entry takes effect at once, is contestable,
//! and suspends the target's admin keys in the act's own commit; the
//! suspension holds while the act is open, a contest included. A contest by
//! the target restores the flag and sign-in only. With three or more admin
//! persons a third admin person who counts settles: confirmed, the
//! suspended keys are revoked, `cause: settlement`; reversed, the
//! suspension lifts and the entry is restored. Below three, the dispute
//! freezes both parties' widening and the initiator's acts against an admin
//! until the operator settles it from the host (d19, another lane's).

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::admin_act::{AdminActCause, AdminActClass, AdminActKind, AdminActState};
use oml_domain::name::{Name, NamedKind};
use oml_domain::web::Email;
use oml_domain::{Role, SignedEnvelope, event_types};

use super::admin_acts::{act_payload, with_act_id};
use super::{App, commit_request, require_role};
use crate::ports::{Clock, IdGen, MetadataFold, Scanner, Signer, WebStore};
use crate::types::{
    AdminActRecord, AdminActRefusal, AppError, Bearer, Credential, KeyRecord, KeySuspension,
    SuspensionCause, WebEmailRequest, WebEntry,
};

/// The links a chain ends with, appended in order.
type Links = Vec<(SignedEnvelope, MetadataFold)>;

/// An open dispute below three admin persons (ADR-0097 d8): the parties of
/// the open contestable act against an admin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Freeze {
    pub act_id: Uuid,
    pub initiator: Option<Name>,
    pub target: Name,
}

/// Who seals a restoration's events: an admin key, or the contesting
/// person with no key acting.
struct Sealer {
    tenant_id: Uuid,
    principal: String,
    key_id: Option<Uuid>,
}

impl Sealer {
    fn of(key: &KeyRecord) -> Self {
        Self {
            tenant_id: key.tenant_id,
            principal: key.principal.clone(),
            key_id: Some(key.key_id),
        }
    }
}

/// The person an act on an admin's standing is against, from its target.
fn target_user(act: &AdminActRecord) -> Option<Name> {
    act.target
        .get("user")
        .and_then(Value::as_str)
        .and_then(|u| Name::parse(u).ok())
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: WebStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The live bearer admin keys belonging to `user`: linked to their root,
    /// or with a link to it pending (ADR-0097 d21).
    pub(crate) async fn admin_keys_of(
        &self,
        tenant_id: Uuid,
        user: &Name,
    ) -> Result<Vec<KeyRecord>, AppError> {
        let Some(root) = self
            .journal
            .resolve_name(tenant_id, NamedKind::Workspace, user)
            .await?
        else {
            return Ok(Vec::new());
        };
        if !self
            .journal
            .workspace(tenant_id, root)
            .await?
            .is_some_and(|w| w.user_root)
        {
            return Ok(Vec::new());
        }
        Ok(self
            .journal
            .web_linked_keys(tenant_id, root)
            .await?
            .into_iter()
            .filter(|k| {
                k.role == Role::Admin
                    && k.credential == Credential::Bearer
                    && k.revoked_at.is_none()
            })
            .collect())
    }

    /// The upgrade window at a start (ADR-0097 d21, d22, the production
    /// migration): in each of `tenants`, every live bearer admin key with no
    /// accepted link is suspended, `key.suspended {cause: unlinked}`, signed
    /// by the server with no key acting, and still reaches `/v2/whoami`
    /// and `POST /v2/keys/self/link`; one suspended so for more than
    /// [`UNLINKED_ADMIN_KEY_WINDOW_SECS`](crate::types::UNLINKED_ADMIN_KEY_WINDOW_SECS) is revoked, `key.revoked {cause:
    /// upgrade}`. One chain per tenant; a tenant with nothing to do
    /// journals nothing, so a later start journals nothing. Answers the
    /// count of keys suspended and revoked.
    ///
    /// # Errors
    /// The store's.
    pub async fn suspend_unlinked_admin_keys(
        &self,
        tenants: &[Uuid],
    ) -> Result<(u64, u64), AppError> {
        let now = self.clock.now();
        let (mut suspended, mut revoked) = (0, 0);
        for &tenant_id in tenants {
            let keys = self.journal.web_unlinked_admin_keys(tenant_id).await?;
            let mut links: Links = Vec::new();
            let mut last = self.head_event(tenant_id).await?;
            for key in keys {
                let (event_type, payload, fold) = match &key.suspended {
                    None => {
                        suspended += 1;
                        (
                            event_types::KEY_SUSPENDED,
                            json!({
                                "key_id": key.key_id.to_string(),
                                "cause": SuspensionCause::Unlinked.as_str(),
                                "act_id": Value::Null,
                            }),
                            MetadataFold::KeySuspended {
                                key_id: key.key_id,
                                suspension: KeySuspension {
                                    cause: SuspensionCause::Unlinked,
                                    act_id: None,
                                    at: now.clone(),
                                },
                            },
                        )
                    }
                    Some(s)
                        if s.cause == SuspensionCause::Unlinked
                            && now.unix_micros() - s.at.unix_micros()
                                >= crate::types::UNLINKED_ADMIN_KEY_WINDOW_SECS * 1_000_000 =>
                    {
                        revoked += 1;
                        (
                            event_types::KEY_REVOKED,
                            json!({
                                "key_id": key.key_id.to_string(),
                                "revoked_by_key_id": Value::Null,
                                "actor_person": Value::Null,
                                "reason": Value::Null,
                                "act_id": Value::Null,
                                "cause": AdminActCause::Upgrade.as_str(),
                            }),
                            MetadataFold::KeyRevoked { key_id: key.key_id },
                        )
                    }
                    Some(_) => continue,
                };
                let event = self.seal(
                    last.as_ref(),
                    tenant_id,
                    event_type,
                    "oml-server",
                    None,
                    payload,
                )?;
                let fold = match fold {
                    MetadataFold::KeySuspended { key_id, suspension } => {
                        MetadataFold::KeySuspended {
                            key_id,
                            suspension: KeySuspension {
                                at: event.envelope.at.clone(),
                                ..suspension
                            },
                        }
                    }
                    other => other,
                };
                last = Some(event.clone());
                links.push((event, fold));
            }
            if !links.is_empty() {
                self.commit_links(links).await?;
            }
        }
        Ok((suspended, revoked))
    }

    /// The act an entry change by `admin` is, as d8 and d21 class it, for
    /// an entry `existing` that holds a live admin flag and that `change`
    /// moves off it (un-flag, a new user) or ends (`None`, a removal): its
    /// own person's own act, or an act against another admin person.
    /// `None` when the entry holds no live admin flag, or the change keeps
    /// its admin person: then the caller's own class applies.
    pub(crate) async fn standing_act(
        &self,
        admin: &KeyRecord,
        existing: Option<&WebEntry>,
        change: Option<&WebEmailRequest>,
    ) -> Result<Option<AdminActKind>, AppError> {
        let Some(entry) = existing.filter(|e| e.admin && !e.pending) else {
            return Ok(None);
        };
        let kind_against = match change {
            None => AdminActKind::RemoveAdmin,
            Some(req) if req.user != entry.user => AdminActKind::ChangeAdmin,
            Some(req) if !req.admin => AdminActKind::UnflagAdmin,
            Some(_) => return Ok(None),
        };
        let own = self.counted_person_of(admin).await?.as_ref() == Some(&entry.user);
        Ok(Some(match (own, kind_against) {
            (true, AdminActKind::RemoveAdmin) => AdminActKind::RemoveOwn,
            // A person moving their own entry to another user leaves the
            // admin standing the keys depend on, as an un-flag does.
            (true, _) => AdminActKind::UnflagOwn,
            (false, kind) => kind,
        }))
    }

    /// The links an act on `user`'s admin standing adds after `prev`, each
    /// naming the act: for the person's own act or a closed deletion, a
    /// `key.revoked` of each of their admin keys, `cause: deactivation`;
    /// for an act against them, a `key.suspended` of each (ADR-0097 d8,
    /// d21).
    pub(crate) async fn standing_links(
        &self,
        admin: &KeyRecord,
        act_id: Uuid,
        kind: AdminActKind,
        user: &Name,
        prev: &SignedEnvelope,
    ) -> Result<Links, AppError> {
        let keys = self.admin_keys_of(admin.tenant_id, user).await?;
        let mut links: Links = Vec::with_capacity(keys.len());
        let mut last = prev.clone();
        for key in keys {
            let (event_type, payload, fold) = if kind.class() == AdminActClass::AgainstAdmin {
                if key.suspended.is_some() {
                    continue;
                }
                let at = self.clock.now();
                (
                    event_types::KEY_SUSPENDED,
                    json!({
                        "key_id": key.key_id.to_string(),
                        "cause": SuspensionCause::AgainstAdmin.as_str(),
                    }),
                    MetadataFold::KeySuspended {
                        key_id: key.key_id,
                        suspension: KeySuspension {
                            cause: SuspensionCause::AgainstAdmin,
                            act_id: Some(act_id),
                            at,
                        },
                    },
                )
            } else {
                (
                    event_types::KEY_REVOKED,
                    json!({
                        "key_id": key.key_id.to_string(),
                        "revoked_by_key_id": admin.key_id.to_string(),
                        "actor_person": Value::Null,
                        "reason": Value::Null,
                        "cause": AdminActCause::Deactivation.as_str(),
                    }),
                    MetadataFold::KeyRevoked { key_id: key.key_id },
                )
            };
            let event = self.seal(
                Some(&last),
                admin.tenant_id,
                event_type,
                &admin.principal,
                Some(admin.key_id),
                with_act_id(payload, act_id),
            )?;
            // A suspension's instant is its event's.
            let fold = match fold {
                MetadataFold::KeySuspended { key_id, suspension } => MetadataFold::KeySuspended {
                    key_id,
                    suspension: KeySuspension {
                        at: event.envelope.at.clone(),
                        ..suspension
                    },
                },
                other => other,
            };
            last = event.clone();
            links.push((event, fold));
        }
        Ok(links)
    }

    /// `POST /v2/admin/acts/{act}/contest` (ADR-0097 d8): the admin person
    /// an open contestable act is against contests it from their own
    /// console session, or from the contest-only sign-in
    /// ([`Self::contest_admin_act_as`]). The act moves to `contested` and
    /// the entry's flag and sign-in come back in the same commit: an
    /// un-flagged or changed entry is flagged again for its user, a removed
    /// one added again, a disabled person enabled. The target's admin keys
    /// stay suspended, and nothing revoked comes back.
    ///
    /// # Errors
    /// `not_contestable`, `not_target`, an act the tenant does not hold,
    /// and the store's.
    pub async fn contest_admin_act(
        &self,
        bearer: impl Into<Bearer<'_>>,
        act_id: Uuid,
    ) -> Result<AdminActRecord, AppError> {
        let caller = self
            .key_act_caller(bearer.into(), "contest_admin_act")
            .await?;
        let Some(user) = caller
            .key
            .session_user
            .clone()
            .filter(|_| caller.key.credential == Credential::Session)
        else {
            return Err(AppError::AdminAct(AdminActRefusal::NotTarget));
        };
        self.contest_admin_act_as(caller.key.tenant_id, &user, act_id)
            .await
    }

    /// The act a contest-only sign-in opened, for its page (ADR-0097 d8):
    /// read under the grant the sign-in matched, so it names only the act
    /// the grant names.
    ///
    /// # Errors
    /// An act the tenant does not hold, and the store's.
    pub async fn contest_act(
        &self,
        grant: &super::ContestGrant,
    ) -> Result<AdminActRecord, AppError> {
        self.journal
            .admin_act(grant.tenant_id, grant.act_id)
            .await?
            .ok_or(AppError::NotFound)
    }

    /// [`Self::contest_admin_act`] for `user`, whom the composition has
    /// identified: their own session, or the contest-only sign-in, whose
    /// provider identity matched the entry as it stood before the act
    /// (ADR-0097 d8).
    ///
    /// # Errors
    /// As [`Self::contest_admin_act`].
    pub async fn contest_admin_act_as(
        &self,
        tenant_id: Uuid,
        user: &Name,
        act_id: Uuid,
    ) -> Result<AdminActRecord, AppError> {
        let mut act = self
            .journal
            .admin_act(tenant_id, act_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if !act.act.contestable() || act.state != AdminActState::Effective {
            return Err(AppError::AdminAct(AdminActRefusal::NotContestable));
        }
        if target_user(&act).as_ref() != Some(user) {
            return Err(AppError::AdminAct(AdminActRefusal::NotTarget));
        }
        // The entry is restored as the act's own admin key set it, sealed
        // as the contesting person with no key acting.
        let actor_key = match act.actor_key_id {
            Some(id) => self.journal.key(tenant_id, id).await?,
            None => None,
        }
        .ok_or_else(|| {
            AppError::StoredResponse(format!("admin act {act_id} names no actor key"))
        })?;
        let by = Sealer {
            tenant_id,
            principal: user.as_str().to_owned(),
            key_id: None,
        };
        act.state = AdminActState::Contested;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::ADMIN_ACT,
            user.as_str(),
            None,
            act_payload(&act),
        )?;
        let mut links: Links = vec![(event, MetadataFold::AdminAct(Box::new(act.clone())))];
        self.restore_standing(&actor_key, &by, &act, &mut links)
            .await?;
        self.commit_links(links).await?;
        Ok(act)
    }

    /// `POST /v2/admin/acts/{act}/settle` (ADR-0097 d8, d11): with three or
    /// more admin persons, a third admin person who counts settles an open
    /// contestable act. `confirm`: the target's suspended admin keys are
    /// revoked, `cause: settlement`, and a contest's restoration is undone
    /// by the act's effect again. Reversed: the suspension lifts and the
    /// entry is restored where no contest restored it. The settled
    /// `admin.act` names the settler as `confirmed_by_person` when
    /// confirmed, and none when reversed. Below three admin persons:
    /// `fewer_than_three`; the operator settles from the host (d19).
    ///
    /// # Errors
    /// `not_contestable`, `fewer_than_three`, `not_settler`, an act the
    /// tenant does not hold, and the store's.
    pub async fn settle_admin_act(
        &self,
        bearer: impl Into<Bearer<'_>>,
        act_id: Uuid,
        confirm: bool,
    ) -> Result<AdminActRecord, AppError> {
        let admin = self
            .key_act_caller(bearer.into(), "settle_admin_act")
            .await?;
        require_role(&admin, Role::Admin, "settle_admin_act")?;
        let admin = admin.key;
        let tenant_id = admin.tenant_id;
        let mut act = self
            .journal
            .admin_act(tenant_id, act_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if !act.act.contestable()
            || !matches!(
                act.state,
                AdminActState::Effective | AdminActState::Contested
            )
        {
            return Err(AppError::AdminAct(AdminActRefusal::NotContestable));
        }
        if self.dispute_admins(tenant_id).await? < 3 {
            return Err(AppError::AdminAct(AdminActRefusal::FewerThanThree));
        }
        let target = target_user(&act);
        let settler = self.counted_person_of(&admin).await?;
        let standing = self.journal.admin_standing(tenant_id).await?;
        let counts = settler.as_ref().is_some_and(|p| {
            Some(p) != act.actor_person.as_ref()
                && Some(p) != target.as_ref()
                && standing.persons.iter().any(|a| {
                    &a.user == p
                        && !a.disabled
                        && oml_domain::admin_act::counts_for_act(
                            a.admin_since.unix_micros(),
                            act.at.unix_micros(),
                        )
                })
        });
        if !counts {
            return Err(AppError::AdminAct(AdminActRefusal::NotSettler));
        }
        let contested = act.state == AdminActState::Contested;
        act.state = AdminActState::Settled;
        act.confirmed_by_person = if confirm { settler } else { None };
        act.settled_by_key_id = Some(admin.key_id);
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::ADMIN_ACT,
            &admin.principal,
            Some(admin.key_id),
            act_payload(&act),
        )?;
        let at = event.envelope.at.clone();
        let mut links: Links = vec![(event, MetadataFold::AdminAct(Box::new(act.clone())))];
        let suspended: Vec<KeyRecord> = match &target {
            Some(user) => self
                .admin_keys_of(tenant_id, user)
                .await?
                .into_iter()
                .filter(|k| {
                    k.suspended
                        .as_ref()
                        .is_some_and(|s| s.act_id == Some(act.act_id))
                })
                .collect(),
            None => Vec::new(),
        };
        for key in suspended {
            let prev = links.last().map(|(e, _)| e.clone()).expect("a link");
            let (event_type, payload, fold) = if confirm {
                (
                    event_types::KEY_REVOKED,
                    json!({
                        "key_id": key.key_id.to_string(),
                        "revoked_by_key_id": admin.key_id.to_string(),
                        "actor_person": act.confirmed_by_person.as_ref().map(Name::as_str),
                        "reason": Value::Null,
                        "cause": AdminActCause::Settlement.as_str(),
                    }),
                    MetadataFold::KeyRevoked { key_id: key.key_id },
                )
            } else {
                (
                    event_types::KEY_SUSPENSION_LIFTED,
                    json!({
                        "key_id": key.key_id.to_string(),
                        "cause": SuspensionCause::AgainstAdmin.as_str(),
                    }),
                    MetadataFold::KeySuspensionLifted { key_id: key.key_id },
                )
            };
            let event = self.seal(
                Some(&prev),
                tenant_id,
                event_type,
                &admin.principal,
                Some(admin.key_id),
                with_act_id(payload, act.act_id),
            )?;
            links.push((event, fold));
        }
        let by = Sealer::of(&admin);
        match (confirm, contested) {
            // A contest restored the entry; the confirmed act applies again.
            (true, true) => self.reapply_standing(&admin, &by, &act, &mut links).await?,
            // Reversed with no contest: the entry comes back now.
            (false, false) => {
                self.restore_standing(&admin, &by, &act, &mut links).await?;
            }
            _ => {}
        }
        self.commit_links(links).await?;
        act.settled_at = Some(at);
        Ok(act)
    }

    /// The entry as the act found it, from its effect (ADR-0097 d8).
    fn before_entry(act: &AdminActRecord) -> Result<(Email, Name, bool), AppError> {
        let bad = || AppError::StoredResponse(format!("admin act {} holds no entry", act.act_id));
        let before = act.effect.get("before").ok_or_else(bad)?;
        let email = before
            .get("email")
            .and_then(Value::as_str)
            .and_then(|e| Email::parse(e).ok())
            .ok_or_else(bad)?;
        let user = before
            .get("user")
            .and_then(Value::as_str)
            .and_then(|u| Name::parse(u).ok())
            .ok_or_else(bad)?;
        let disabled = before
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok((email, user, disabled))
    }

    /// Restores the flag and sign-in the act took (ADR-0097 d8): the entry
    /// flagged again for its user, added again where removed, and the
    /// person enabled where the act disabled them.
    async fn restore_standing(
        &self,
        admin: &KeyRecord,
        by: &Sealer,
        act: &AdminActRecord,
        links: &mut Links,
    ) -> Result<(), AppError> {
        let (email, user, disabled) = Self::before_entry(act)?;
        if act.act == AdminActKind::DisableAdmin {
            if !disabled {
                self.push_link(
                    by,
                    act.act_id,
                    links,
                    event_types::WEB_USER_ENABLED,
                    json!({ "user": user.as_str() }),
                    MetadataFold::UserEnabled { user },
                )?;
            }
            return Ok(());
        }
        let req = WebEmailRequest {
            email,
            user,
            admin: true,
        };
        if let Some((_, _, event_type, fold, payload)) =
            self.entry_change(admin, &req, true).await?
        {
            self.push_link(by, act.act_id, links, event_type, payload, fold)?;
        }
        Ok(())
    }

    /// Applies the act's effect again after a contest restored the entry
    /// (ADR-0097 d8).
    async fn reapply_standing(
        &self,
        admin: &KeyRecord,
        by: &Sealer,
        act: &AdminActRecord,
        links: &mut Links,
    ) -> Result<(), AppError> {
        let (email, user, _) = Self::before_entry(act)?;
        match act.act {
            AdminActKind::DisableAdmin => self.push_link(
                by,
                act.act_id,
                links,
                event_types::WEB_USER_DISABLED,
                json!({ "user": user.as_str(), "by_key_id": admin.key_id.to_string() }),
                MetadataFold::UserDisabled { user },
            ),
            AdminActKind::RemoveAdmin => self.push_link(
                by,
                act.act_id,
                links,
                event_types::WEB_EMAIL_REMOVED,
                json!({
                    "email_digest": email.digest().to_prefixed(),
                    "user": user.as_str(),
                    "admin": true,
                    "by_key_id": admin.key_id.to_string(),
                    "pending": false,
                }),
                MetadataFold::WebEmailRemoved {
                    email_hash: email.digest(),
                    keep_an_admin: true,
                },
            ),
            _ => {
                let after_user = act
                    .effect
                    .get("after")
                    .and_then(|a| a.get("user"))
                    .and_then(Value::as_str)
                    .and_then(|u| Name::parse(u).ok())
                    .unwrap_or(user);
                let req = WebEmailRequest {
                    email,
                    user: after_user,
                    admin: false,
                };
                if let Some((_, _, event_type, fold, payload)) =
                    self.entry_change(admin, &req, true).await?
                {
                    self.push_link(by, act.act_id, links, event_type, payload, fold)?;
                }
                Ok(())
            }
        }
    }

    /// Seals `payload` after the last of `links`, naming `act_id`.
    fn push_link(
        &self,
        by: &Sealer,
        act_id: Uuid,
        links: &mut Links,
        event_type: &str,
        payload: Value,
        fold: MetadataFold,
    ) -> Result<(), AppError> {
        let prev = links.last().map(|(e, _)| e.clone());
        let event = self.seal(
            prev.as_ref(),
            by.tenant_id,
            event_type,
            &by.principal,
            by.key_id,
            with_act_id(payload, act_id),
        )?;
        links.push((event, fold));
        Ok(())
    }

    /// Commits `links` as one chain.
    async fn commit_links(&self, links: Links) -> Result<(), AppError> {
        let reqs = links
            .into_iter()
            .map(|(event, fold)| commit_request(event, fold, None))
            .collect();
        self.commit_requests(reqs).await?;
        Ok(())
    }
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: crate::ports::JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The open contestable acts against an admin (ADR-0097 d8):
    /// `effective` or `contested`.
    pub(crate) async fn open_disputes(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<AdminActRecord>, AppError> {
        let mut open = Vec::new();
        for state in [AdminActState::Effective, AdminActState::Contested] {
            open.extend(
                self.journal
                    .admin_acts(tenant_id, Some(state))
                    .await?
                    .into_iter()
                    .filter(|a| a.act.contestable()),
            );
        }
        Ok(open)
    }

    /// The admin persons a settlement counts (ADR-0097 d8): the tenant's
    /// live admin persons and every open act's target, whom the act took
    /// out of the count.
    async fn dispute_admins(&self, tenant_id: Uuid) -> Result<usize, AppError> {
        let mut persons: std::collections::BTreeSet<Name> = self
            .journal
            .admin_standing(tenant_id)
            .await?
            .persons
            .into_iter()
            .filter(|p| !p.disabled)
            .map(|p| p.user)
            .collect();
        for act in self.open_disputes(tenant_id).await? {
            if let Some(user) = target_user(&act) {
                persons.insert(user);
            }
        }
        Ok(persons.len())
    }

    /// The freeze an open dispute below three admin persons holds, if any
    /// (ADR-0097 d8).
    pub async fn freeze(&self, tenant_id: Uuid) -> Result<Option<Freeze>, AppError> {
        let open = self.open_disputes(tenant_id).await?;
        let Some(act) = open.first() else {
            return Ok(None);
        };
        if self.dispute_admins(tenant_id).await? >= 3 {
            return Ok(None);
        }
        Ok(target_user(act).map(|target| Freeze {
            act_id: act.act_id,
            initiator: act.actor_person.clone(),
            target,
        }))
    }

    /// Refuses `kind` by `person` during a freeze (ADR-0097 d8): both
    /// parties lose widening, and the initiator acts against an admin.
    pub(crate) async fn check_freeze(
        &self,
        tenant_id: Uuid,
        person: Option<&Name>,
        kind: AdminActKind,
    ) -> Result<(), AppError> {
        let Some(person) = person else {
            return Ok(());
        };
        let Some(freeze) = self.freeze(tenant_id).await? else {
            return Ok(());
        };
        let initiator = freeze.initiator.as_ref() == Some(person);
        let party = initiator || &freeze.target == person;
        let refused = match kind.class() {
            AdminActClass::Widening => party,
            AdminActClass::AgainstAdmin => initiator,
            _ => false,
        };
        if refused {
            return Err(AppError::AdminAct(AdminActRefusal::Frozen));
        }
        Ok(())
    }
}
