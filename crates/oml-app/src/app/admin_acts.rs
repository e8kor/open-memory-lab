//! The admin act (ADR-0097 d1, d2, d3, d5, d9, d10, d11, d23): every act an
//! admin key takes outside content review journals one `admin.act` envelope
//! in the commit of its effect, first in the chain, and the effect's own
//! events name its `act_id`. The act decides its class. Narrowing and
//! neutral acts take effect at once; a widening act takes effect at once
//! only in a tenant that never had two live admin persons, marked
//! `sole_admin` (d10), and otherwise waits, `pending_confirmation`, until a
//! second admin person who counts confirms it (d5, d11). Confirming applies
//! the effect the act recorded, in the confirmation's own commit; declining
//! or expiry is final (d23).

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::access::{Covers, Grant};
use oml_domain::admin_act::{AdminActClass, AdminActKind, AdminActState, counts_for_act};
use oml_domain::{EventTime, Role, Sha256Hash, SignedEnvelope, event_types};

use super::{App, code_issued_payload, commit_request, require_role};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::types::{AdminActRecord, AdminActRefusal, AppError, Bearer, ConnectCode, KeyRecord};

/// An admin act sealed and not yet committed: its envelope, first in the
/// chain of its effect, and its record.
pub(crate) struct BegunAct {
    pub event: SignedEnvelope,
    pub record: AdminActRecord,
}

impl BegunAct {
    pub(crate) fn act_id(&self) -> Uuid {
        self.record.act_id
    }

    /// Whether the act waits on a second admin person (ADR-0097 d5).
    pub(crate) fn pending(&self) -> bool {
        self.record.state == AdminActState::PendingConfirmation
    }

    /// The envelope and its fold, the chain's first link.
    pub(crate) fn link(&self) -> (SignedEnvelope, MetadataFold) {
        (
            self.event.clone(),
            MetadataFold::AdminAct(Box::new(self.record.clone())),
        )
    }
}

/// What a decline's undo names: the declining admin key, its act, its
/// person and its reason.
struct Undo<'a> {
    admin: &'a KeyRecord,
    decline_id: Uuid,
    actor_person: Option<String>,
    reason: Option<&'a str>,
}

/// `payload` naming the admin act that caused it (ADR-0097 d2: the effect's
/// own event names the `act_id`).
pub(crate) fn with_act_id(mut payload: Value, act_id: Uuid) -> Value {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("act_id".to_owned(), json!(act_id.to_string()));
    }
    payload
}

/// `payload` naming `act`'s id where there is one.
pub(crate) fn named(payload: Value, act: Option<&BegunAct>) -> Value {
    match act {
        Some(act) => with_act_id(payload, act.act_id()),
        None => payload,
    }
}

/// A grant as a pending act's effect holds it (ADR-0097 d23).
pub(crate) fn grant_json(grant: &Grant) -> Value {
    json!({
        "grant_id": grant.grant_id.to_string(),
        "node": grant.node.to_string(),
        "role": grant.role.as_str(),
        "deny": grant.deny.iter().map(Uuid::to_string).collect::<Vec<_>>(),
        "covers": grant.covers.as_str(),
    })
}

fn grant_from_json(value: &Value) -> Result<Grant, AppError> {
    let bad = || AppError::StoredResponse(format!("admin act effect grant {value}"));
    let id = |field: &str| -> Result<Uuid, AppError> {
        value
            .get(field)
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(bad)
    };
    let role = value.get("role").and_then(Value::as_str).ok_or_else(bad)?;
    let role = [Role::Reader, Role::Submitter, Role::Reviewer]
        .into_iter()
        .find(|r| r.as_str() == role)
        .ok_or_else(bad)?;
    let deny = value
        .get("deny")
        .and_then(Value::as_array)
        .ok_or_else(bad)?
        .iter()
        .map(|d| {
            d.as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or_else(bad)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let covers = value
        .get("covers")
        .and_then(Value::as_str)
        .and_then(Covers::parse)
        .ok_or_else(bad)?;
    Ok(Grant {
        grant_id: id("grant_id")?,
        node: id("node")?,
        role,
        deny,
        covers,
    })
}

/// The `admin.act` payload of `record` (ADR-0097 d2).
pub(crate) fn act_payload(record: &AdminActRecord) -> Value {
    json!({
        "act_id": record.act_id.to_string(),
        "act": record.act.as_str(),
        "class": record.class().as_str(),
        "target": record.target,
        "reason": record.reason,
        "actor_person": record.actor_person.as_ref().map(|p| p.as_str()),
        "state": record.state.as_str(),
        "confirmed_by_person": record.confirmed_by_person.as_ref().map(|p| p.as_str()),
        "sole_admin": record.sole_admin,
        "cause": record.cause.map(oml_domain::admin_act::AdminActCause::as_str),
    })
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// Opens an admin act by `admin`, sealed on `prev`: its class is the
    /// act's (d2), its actor the person the key counts as, and its state
    /// `effective` for every act but widening, which is `effective`, marked
    /// `sole_admin`, only in a tenant that never had two live admin persons
    /// while no act against an admin is open, and `pending_confirmation`
    /// otherwise (d5, d10). The reason is scanned as a verdict's is and
    /// required for widening, consent and acts against an admin, except a
    /// widening act taken at once under `sole_admin` (d26). `effect` is what
    /// a confirmation applies (d23).
    pub(crate) async fn begin_admin_act(
        &self,
        admin: &KeyRecord,
        act: AdminActKind,
        target: Value,
        reason: Option<&str>,
        effect: Value,
        prev: Option<&SignedEnvelope>,
    ) -> Result<BegunAct, AppError> {
        let tenant_id = admin.tenant_id;
        let (state, sole_admin) = if act.class() == AdminActClass::Widening {
            let standing = self.journal.admin_standing(tenant_id).await?;
            if standing.sole_admin && !self.act_against_admin_open(tenant_id).await? {
                (AdminActState::Effective, true)
            } else {
                (AdminActState::PendingConfirmation, false)
            }
        } else {
            (AdminActState::Effective, false)
        };
        if reason.is_none() && act.requires_reason() && !sole_admin {
            return Err(AppError::AdminActReasonRequired);
        }
        // During a freeze both parties lose widening and the initiator acts
        // against an admin (ADR-0097 d8).
        self.check_freeze(
            tenant_id,
            self.counted_person_of(admin).await?.as_ref(),
            act,
        )
        .await?;
        if let Some(reason) = reason {
            self.scan_veto_only(tenant_id, "reason", reason)?;
        }
        let actor_person = self.counted_person_of(admin).await?;
        let mut record = AdminActRecord {
            act_id: self.ids.new_id(),
            act,
            target,
            reason: reason.map(str::to_owned),
            actor_key_id: Some(admin.key_id),
            actor_person,
            state,
            confirmed_by_person: None,
            settled_by_key_id: None,
            sole_admin,
            cause: None,
            at: self.clock.now(),
            settled_at: None,
            // A widening act keeps what it applies, pending or taken at once
            // under `sole_admin`, so a confirmation applies it and a decline
            // undoes it (d10, d23; the operator's decision of 2026-11-27).
            // An act against an admin keeps the entry it found, which a
            // contest or a reversal restores (d8).
            effect: if matches!(
                act.class(),
                AdminActClass::Widening | AdminActClass::AgainstAdmin
            ) {
                effect
            } else {
                json!({})
            },
        };
        let event = self.seal(
            prev,
            tenant_id,
            event_types::ADMIN_ACT,
            &admin.principal,
            Some(admin.key_id),
            act_payload(&record),
        )?;
        record.at = event.envelope.at.clone();
        Ok(BegunAct { event, record })
    }

    /// The admin act `caller` takes by an act at once (narrowing, neutral):
    /// `None` for a caller that is no admin key, or that is a person's own
    /// session acting as the person (ADR-0097 d1: persons' own acts are not
    /// admin acts). A `<user>-admin` session key is an admin key.
    pub(crate) async fn admin_act_of(
        &self,
        caller: &KeyRecord,
        act: AdminActKind,
        target: Value,
        reason: Option<&str>,
        prev: Option<&SignedEnvelope>,
    ) -> Result<Option<BegunAct>, AppError> {
        if caller.role != Role::Admin {
            return Ok(None);
        }
        Ok(Some(
            self.begin_admin_act(caller, act, target, reason, json!({}), prev)
                .await?,
        ))
    }

    /// One effect event under an admin act of `kind` taken at once, in one
    /// chain led by the act's envelope, the effect naming its `act_id`; with
    /// no admin act where `caller` is no admin key (ADR-0097 d2). For the
    /// acts whose class is narrowing or neutral: crews, a workspace's crew,
    /// a rotation, a rescan, a connection's revocation (d3, d9).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn commit_as_act(
        &self,
        caller: &KeyRecord,
        kind: AdminActKind,
        target: Value,
        reason: Option<&str>,
        event_type: &str,
        payload: Value,
        fold: MetadataFold,
    ) -> Result<crate::ports::CommitReceipt, AppError> {
        let prev = self.head_event(caller.tenant_id).await?;
        let act = self
            .admin_act_of(caller, kind, target, reason, prev.as_ref())
            .await?;
        let event = self.seal(
            act.as_ref().map(|a| &a.event).or(prev.as_ref()),
            caller.tenant_id,
            event_type,
            &caller.principal,
            Some(caller.key_id),
            named(payload, act.as_ref()),
        )?;
        self.commit_with_act(act, vec![(event, fold)]).await
    }

    /// Commits `links`, led by `act`'s envelope where there is one, as one
    /// chain (ADR-0097 d2).
    pub(crate) async fn commit_with_act(
        &self,
        act: Option<BegunAct>,
        links: Vec<(SignedEnvelope, MetadataFold)>,
    ) -> Result<crate::ports::CommitReceipt, AppError> {
        let mut reqs = Vec::with_capacity(links.len() + 1);
        if let Some(act) = &act {
            let (event, fold) = act.link();
            reqs.push(commit_request(event, fold, None));
        }
        reqs.extend(
            links
                .into_iter()
                .map(|(event, fold)| commit_request(event, fold, None)),
        );
        Ok(self.commit_requests(reqs).await?)
    }

    /// Whether an act against an admin is open in the tenant: then widening
    /// is confirm-first even with one live admin person (ADR-0097 d10).
    pub(crate) async fn act_against_admin_open(&self, tenant_id: Uuid) -> Result<bool, AppError> {
        for state in [AdminActState::Effective, AdminActState::Contested] {
            if self
                .journal
                .admin_acts(tenant_id, Some(state))
                .await?
                .iter()
                .any(|a| a.act.contestable())
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `GET /v2/admin/acts` (ADR-0097 d2, d23): the tenant's admin acts,
    /// oldest first, in `state` when one is named, each with its state at
    /// now, so a pending act past its life reads `expired`. Admin key role
    /// only.
    ///
    /// # Errors
    /// The caller's authentication and role, and the store's.
    pub async fn admin_acts(
        &self,
        bearer: impl Into<Bearer<'_>>,
        state: Option<AdminActState>,
    ) -> Result<Vec<AdminActRecord>, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?;
        require_role(&admin, Role::Admin, "admin_acts")?;
        let now = self.clock.now();
        // An expired act is stored pending; it is listed as expired.
        let stored = match state {
            Some(AdminActState::Expired) => Some(AdminActState::PendingConfirmation),
            other => other,
        };
        let mut acts = self.journal.admin_acts(admin.key.tenant_id, stored).await?;
        for act in &mut acts {
            act.state = act.state_at(&now);
        }
        if let Some(wanted) = state {
            acts.retain(|a| a.state == wanted);
        }
        Ok(acts)
    }
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: crate::ports::ConnectionStore + crate::ports::WebStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/admin/acts/{act}/decline` (ADR-0097 d3, d10, d23; the
    /// operator's decision of 2026-11-27): any admin key declines a pending
    /// widening act, which then never takes effect, or one a `sole_admin`
    /// tenant took at once, which the decline undoes in its own commit by
    /// the matching narrowing effect: the key the act issued is revoked, the
    /// grants it added are revoked, the connect code it minted is voided or
    /// the connection it opened revoked, a raised limit is set back to the
    /// limits the act found, and an admin flag is taken off its entry under
    /// the last-admin guard (d25). A pending issuance's key is revoked too.
    /// Declining is itself narrowing, journalled as its own `decline_act`
    /// act before the declined act's envelope. Refused: an act that took
    /// effect and is not widening under `sole_admin` (`not_pending`), one
    /// past its life (`expired`, already final).
    ///
    /// # Errors
    /// The refusals above, an act the tenant does not hold, and the store's.
    pub async fn decline_admin_act(
        &self,
        bearer: impl Into<Bearer<'_>>,
        act_id: Uuid,
        reason: Option<&str>,
    ) -> Result<AdminActRecord, AppError> {
        let admin = self
            .key_act_caller(bearer.into(), "decline_admin_act")
            .await?;
        require_role(&admin, Role::Admin, "decline_admin_act")?;
        let admin = admin.key;
        let tenant_id = admin.tenant_id;
        let mut act = self
            .journal
            .admin_act(tenant_id, act_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let undo = match act.state_at(&self.clock.now()) {
            AdminActState::PendingConfirmation => false,
            AdminActState::Expired => return Err(AppError::AdminAct(AdminActRefusal::Expired)),
            AdminActState::Effective
                if act.sole_admin && act.class() == AdminActClass::Widening =>
            {
                true
            }
            _ => return Err(AppError::AdminAct(AdminActRefusal::NotPending)),
        };
        let prev = self.head_event(tenant_id).await?;
        let decline = self
            .begin_admin_act(
                &admin,
                AdminActKind::DeclineAct,
                json!({ "act_id": act_id.to_string() }),
                reason,
                json!({}),
                prev.as_ref(),
            )
            .await?;
        act.state = AdminActState::Declined;
        act.settled_by_key_id = Some(admin.key_id);
        let event = self.seal(
            Some(&decline.event),
            tenant_id,
            event_types::ADMIN_ACT,
            &admin.principal,
            Some(admin.key_id),
            act_payload(&act),
        )?;
        let at = event.envelope.at.clone();
        let mut links = vec![
            decline.link(),
            (event, MetadataFold::AdminAct(Box::new(act.clone()))),
        ];
        let undo_ctx = Undo {
            admin: &admin,
            decline_id: decline.act_id(),
            actor_person: decline
                .record
                .actor_person
                .as_ref()
                .map(|p| p.as_str().to_owned()),
            reason,
        };
        // A pending issuance's key never authenticated; it is revoked so
        // its value ends with the act (d23).
        if act.act == AdminActKind::IssueKey || undo {
            self.undo_effect(&undo_ctx, &act, undo, &mut links).await?;
        }
        let reqs = links
            .into_iter()
            .map(|(event, fold)| commit_request(event, fold, None))
            .collect();
        self.commit_requests(reqs).await?;
        act.settled_at = Some(at);
        Ok(act)
    }

    /// The narrowing effect that undoes `act`, sealed after the last of
    /// `links`, each event naming the decline's `act_id` (ADR-0097 d10, the
    /// operator's decision of 2026-11-27). With `took_effect` false only a
    /// pending issuance's key is revoked: nothing else applied.
    async fn undo_effect(
        &self,
        undo: &Undo<'_>,
        act: &AdminActRecord,
        took_effect: bool,
        links: &mut Vec<(SignedEnvelope, MetadataFold)>,
    ) -> Result<(), AppError> {
        let admin = undo.admin;
        let tenant_id = admin.tenant_id;
        let effect = &act.effect;
        let uuid_at = |v: &Value, field: &str| {
            v.get(field)
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
        };
        let key_id = uuid_at(effect, "key_id").or_else(|| uuid_at(&act.target, "key_id"));
        let key = match key_id {
            Some(id) => self.journal.key(tenant_id, id).await?,
            None => None,
        };
        let live = key.as_ref().filter(|k| k.revoked_at.is_none());
        match act.act {
            AdminActKind::IssueKey => {
                if let Some(key) = live {
                    let payload = json!({
                        "key_id": key.key_id.to_string(),
                        "revoked_by_key_id": admin.key_id.to_string(),
                        "actor_person": undo.actor_person,
                        "reason": undo.reason,
                        "cause": null,
                    });
                    self.push_undo(undo, links, event_types::KEY_REVOKED, payload, |_| {
                        MetadataFold::KeyRevoked { key_id: key.key_id }
                    })?;
                }
            }
            _ if !took_effect => {}
            AdminActKind::AddGrant => {
                let Some(key) = live else { return Ok(()) };
                let held = self
                    .journal
                    .key_access(tenant_id, key.key_id)
                    .await?
                    .map(|a| a.grants)
                    .unwrap_or_default();
                for grant in effect
                    .get("grants")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    let grant = grant_from_json(grant)?;
                    if !held.iter().any(|h| h.grant.grant_id == grant.grant_id) {
                        continue;
                    }
                    let grant_id = grant.grant_id;
                    let payload = json!({
                        "key_id": key.key_id.to_string(),
                        "grant_id": grant_id.to_string(),
                        "revoked_by_key_id": admin.key_id.to_string(),
                    });
                    self.push_undo(undo, links, event_types::GRANT_REVOKED, payload, |_| {
                        MetadataFold::GrantRevoked {
                            key_id: key.key_id,
                            grant_id,
                        }
                    })?;
                }
            }
            AdminActKind::MintConnectCode => {
                let Some(key) = live else { return Ok(()) };
                let Some(code_id) = effect.get("code").and_then(|c| uuid_at(c, "code_id")) else {
                    return Ok(());
                };
                let opened = self
                    .journal
                    .connections(tenant_id, key.key_id)
                    .await?
                    .into_iter()
                    .find(|c| {
                        c.code_id == code_id && c.state == oml_domain::oauth::ConnectionState::Open
                    });
                if let Some(connection) = opened {
                    let connection_id = connection.connection_id;
                    let payload = json!({
                        "connection_id": connection_id.to_string(),
                        "key_id": key.key_id.to_string(),
                        "revoked_by_key_id": admin.key_id.to_string(),
                        "cause": oml_domain::oauth::RevokedCause::Admin.as_str(),
                        "reason_digest": undo.reason.map(|r| oml_domain::sha256_digest_string(r.as_bytes())),
                    });
                    self.push_undo(
                        undo,
                        links,
                        event_types::CONNECTION_REVOKED,
                        payload,
                        |_| MetadataFold::ConnectionRevoked {
                            connection_id,
                            cause: oml_domain::oauth::RevokedCause::Admin,
                        },
                    )?;
                } else if self
                    .journal
                    .connect_codes(tenant_id, key.key_id)
                    .await?
                    .iter()
                    .any(|c| {
                        c.code_id == code_id && c.redeemed_at.is_none() && c.voided_seq.is_none()
                    })
                {
                    let payload = json!({
                        "code_id": code_id.to_string(),
                        "key_id": key.key_id.to_string(),
                        "voided_by_key_id": admin.key_id.to_string(),
                    });
                    self.push_undo(
                        undo,
                        links,
                        event_types::CONNECTION_CODE_VOIDED,
                        payload,
                        |_| MetadataFold::ConnectCodeVoided {
                            key_id: key.key_id,
                            code_id,
                        },
                    )?;
                }
            }
            AdminActKind::RaiseLimits => {
                let Some(key) = live else { return Ok(()) };
                let before = effect.get("before").cloned().unwrap_or(Value::Null);
                let expiry = before
                    .get("expiry")
                    .and_then(Value::as_str)
                    .map(EventTime::parse)
                    .transpose()
                    .map_err(|e| AppError::StoredResponse(e.to_string()))?;
                let requests_per_minute = before
                    .get("requests_per_minute")
                    .and_then(Value::as_u64)
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|e| AppError::StoredResponse(e.to_string()))?;
                let payload = json!({
                    "key_id": key.key_id.to_string(),
                    "set_by_key_id": admin.key_id.to_string(),
                    "expiry": expiry.as_ref().map(EventTime::as_str),
                    "requests_per_minute": requests_per_minute,
                });
                self.push_undo(undo, links, event_types::KEY_LIMITS_SET, payload, |_| {
                    MetadataFold::KeyLimitsSet {
                        key_id: key.key_id,
                        expiry,
                        requests_per_minute,
                    }
                })?;
            }
            AdminActKind::AllowAdmin => {
                let bad = || AppError::StoredResponse(format!("admin act {} entry", act.act_id));
                let text = |field: &str| {
                    effect
                        .get(field)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .ok_or_else(bad)
                };
                let email = oml_domain::web::Email::parse(&text("email")?).map_err(|_| bad())?;
                // The entry as it stands now: a flag already off changes
                // nothing; the flag comes off its user as the entry names
                // it today, under the last-admin guard (d25).
                let Some(entry) = self.journal.web_entry_by_email(&email.digest()).await? else {
                    return Ok(());
                };
                if !entry.admin || entry.tenant_id != tenant_id {
                    return Ok(());
                }
                let req = crate::types::WebEmailRequest {
                    email,
                    user: entry.user.clone(),
                    admin: false,
                };
                if let Some((_, _, event_type, fold, payload)) =
                    self.entry_change(admin, &req, true).await?
                {
                    self.push_undo(undo, links, event_type, payload, |_| fold)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Seals `payload` as `event_type` after the last of `links`, naming the
    /// decline's `act_id`, and pushes it with its fold.
    fn push_undo(
        &self,
        undo: &Undo<'_>,
        links: &mut Vec<(SignedEnvelope, MetadataFold)>,
        event_type: &str,
        payload: Value,
        fold: impl FnOnce(&SignedEnvelope) -> MetadataFold,
    ) -> Result<(), AppError> {
        let prev = links.last().map(|(e, _)| e.clone()).expect("a link");
        let event = self.seal(
            Some(&prev),
            undo.admin.tenant_id,
            event_type,
            &undo.admin.principal,
            Some(undo.admin.key_id),
            with_act_id(payload, undo.decline_id),
        )?;
        let fold = fold(&event);
        links.push((event, fold));
        Ok(())
    }

    /// `POST /v2/admin/acts/{act}/confirm` (ADR-0097 d5, d11, d23): a
    /// second admin person confirms a pending widening act, and its effect
    /// applies in this commit: a key's grants and connect code, a grant, a
    /// raised limit. Refused, journalling nothing: an act that waits on no
    /// one (`not_pending`), one past its life (`expired`; a late confirm
    /// never revives it), a key that counts as the act's own person
    /// (`own_act`), one that counts as no live admin person
    /// (`not_admin_person`), and an admin person whose flag was accepted
    /// less than seven days before the act (`too_new`).
    ///
    /// # Errors
    /// The refusals above, an act the tenant does not hold, and the store's.
    pub async fn confirm_admin_act(
        &self,
        bearer: impl Into<Bearer<'_>>,
        act_id: Uuid,
    ) -> Result<AdminActRecord, AppError> {
        let admin = self
            .key_act_caller(bearer.into(), "confirm_admin_act")
            .await?;
        require_role(&admin, Role::Admin, "confirm_admin_act")?;
        let admin = admin.key;
        let tenant_id = admin.tenant_id;
        let mut act = self
            .journal
            .admin_act(tenant_id, act_id)
            .await?
            .ok_or(AppError::NotFound)?;
        match act.state_at(&self.clock.now()) {
            AdminActState::PendingConfirmation => {}
            AdminActState::Expired => return Err(AppError::AdminAct(AdminActRefusal::Expired)),
            _ => return Err(AppError::AdminAct(AdminActRefusal::NotPending)),
        }
        let Some(person) = self.counted_person_of(&admin).await? else {
            return Err(AppError::AdminAct(AdminActRefusal::NotAdminPerson));
        };
        if act.actor_person.as_ref() == Some(&person) {
            return Err(AppError::AdminAct(AdminActRefusal::OwnAct));
        }
        let standing = self.journal.admin_standing(tenant_id).await?;
        let Some(counted) = standing
            .persons
            .iter()
            .find(|p| p.user == person && !p.disabled)
        else {
            return Err(AppError::AdminAct(AdminActRefusal::NotAdminPerson));
        };
        if !counts_for_act(counted.admin_since.unix_micros(), act.at.unix_micros()) {
            return Err(AppError::AdminAct(AdminActRefusal::TooNew));
        }
        act.state = AdminActState::Confirmed;
        act.confirmed_by_person = Some(person);
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
        let mut links = vec![(event, MetadataFold::AdminAct(Box::new(act.clone())))];
        self.confirmed_effect(&admin, &act, &mut links).await?;
        let reqs = links
            .into_iter()
            .map(|(event, fold)| commit_request(event, fold, None))
            .collect();
        self.commit_requests(reqs).await?;
        act.settled_at = Some(at);
        Ok(act)
    }

    /// The effect a confirmed act applies, sealed after the last of `links`
    /// (ADR-0097 d5, d6, d15, d23).
    async fn confirmed_effect(
        &self,
        admin: &KeyRecord,
        act: &AdminActRecord,
        links: &mut Vec<(SignedEnvelope, MetadataFold)>,
    ) -> Result<(), AppError> {
        let tenant_id = admin.tenant_id;
        let effect = &act.effect;
        let key_id = effect
            .get("key_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok());
        let last = |links: &Vec<(SignedEnvelope, MetadataFold)>| {
            links
                .last()
                .map(|(e, _)| e.clone())
                .expect("the act's own link")
        };
        match act.act {
            AdminActKind::IssueKey | AdminActKind::AddGrant => {
                let key_id = key_id.ok_or_else(|| {
                    AppError::StoredResponse(format!("admin act {} names no key", act.act_id))
                })?;
                for grant in effect
                    .get("grants")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    let grant = grant_from_json(grant)?;
                    let prev = last(links);
                    let event = self.grant_event(&prev, admin, key_id, &grant)?;
                    let event = self.reseal_with_act(&prev, event, act.act_id)?;
                    links.push((event, MetadataFold::GrantIssued { key_id, grant }));
                }
                if let Some(code) = effect.get("code").filter(|c| !c.is_null()) {
                    self.confirmed_code(admin, act, key_id, code, links).await?;
                }
            }
            AdminActKind::MintConnectCode => {
                let key_id = key_id.ok_or_else(|| {
                    AppError::StoredResponse(format!("admin act {} names no key", act.act_id))
                })?;
                let code = effect.get("code").ok_or_else(|| {
                    AppError::StoredResponse(format!("admin act {} holds no code", act.act_id))
                })?;
                self.confirmed_code(admin, act, key_id, code, links).await?;
            }
            AdminActKind::RaiseLimits => {
                let key_id = key_id.ok_or_else(|| {
                    AppError::StoredResponse(format!("admin act {} names no key", act.act_id))
                })?;
                let expiry = effect
                    .get("expiry")
                    .and_then(Value::as_str)
                    .map(EventTime::parse)
                    .transpose()
                    .map_err(|e| AppError::StoredResponse(e.to_string()))?;
                let requests_per_minute = effect
                    .get("requests_per_minute")
                    .and_then(Value::as_u64)
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|e| AppError::StoredResponse(e.to_string()))?;
                let prev = last(links);
                let event = self.seal(
                    Some(&prev),
                    tenant_id,
                    event_types::KEY_LIMITS_SET,
                    &admin.principal,
                    Some(admin.key_id),
                    with_act_id(
                        json!({
                            "key_id": key_id.to_string(),
                            "set_by_key_id": admin.key_id.to_string(),
                            "expiry": expiry.as_ref().map(EventTime::as_str),
                            "requests_per_minute": requests_per_minute,
                        }),
                        act.act_id,
                    ),
                )?;
                links.push((
                    event,
                    MetadataFold::KeyLimitsSet {
                        key_id,
                        expiry,
                        requests_per_minute,
                    },
                ));
            }
            AdminActKind::AllowAdmin => {
                let bad = || AppError::StoredResponse(format!("admin act {} entry", act.act_id));
                let text = |field: &str| {
                    effect
                        .get(field)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .ok_or_else(bad)
                };
                let req = crate::types::WebEmailRequest {
                    email: oml_domain::web::Email::parse(&text("email")?).map_err(|_| bad())?,
                    user: oml_domain::name::Name::parse(&text("user")?).map_err(|_| bad())?,
                    admin: true,
                };
                // The entry as it stands at the confirmation: a change since
                // the act is read again, and one that already holds the flag
                // changes nothing.
                if let Some((_, _, event_type, fold, payload)) =
                    self.entry_change(admin, &req, true).await?
                {
                    let prev = last(links);
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
            }
            _ => {
                return Err(AppError::StoredResponse(format!(
                    "admin act {} ({}) applies nothing at confirmation",
                    act.act_id,
                    act.act.as_str()
                )));
            }
        }
        Ok(())
    }

    /// A pending connect code's `connection.code_issued`, at confirmation:
    /// its ten minutes run from now, and it voids the key's earlier codes
    /// here, not at the mint (ADR-0097 d6, the operator's decision of
    /// 2026-11-26).
    async fn confirmed_code(
        &self,
        admin: &KeyRecord,
        act: &AdminActRecord,
        key_id: Uuid,
        code: &Value,
        links: &mut Vec<(SignedEnvelope, MetadataFold)>,
    ) -> Result<(), AppError> {
        let bad = || AppError::StoredResponse(format!("admin act {} code {code}", act.act_id));
        let code_id = code
            .get("code_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(bad)?;
        let code_hash = code
            .get("code_hash")
            .and_then(Value::as_str)
            .and_then(|s| Sha256Hash::from_prefixed(s).ok())
            .ok_or_else(bad)?;
        let ttl = code
            .get("ttl_secs")
            .and_then(Value::as_i64)
            .ok_or_else(bad)?;
        // The key's live code is voided now, at the confirmation, not at
        // the mint (ADR-0097 d6).
        let voids = self
            .journal
            .connect_codes(admin.tenant_id, key_id)
            .await?
            .into_iter()
            .find(|c| c.redeemed_at.is_none() && c.voided_seq.is_none())
            .map(|c| c.code_id);
        let expires_at =
            EventTime::from_unix_micros(self.clock.now().unix_micros() + ttl * 1_000_000)
                .ok_or_else(bad)?;
        let code = ConnectCode {
            code_id,
            key_id,
            code_hash,
            expires_at,
        };
        let prev = links.last().map(|(e, _)| e.clone()).expect("a link");
        let event = self.seal(
            Some(&prev),
            admin.tenant_id,
            event_types::CONNECTION_CODE_ISSUED,
            &admin.principal,
            Some(admin.key_id),
            with_act_id(code_issued_payload(&code, admin.key_id, voids), act.act_id),
        )?;
        links.push((event, MetadataFold::ConnectCodeIssued { code, voids }));
        Ok(())
    }

    /// `event` sealed again on `prev` with the `act_id` in its payload.
    fn reseal_with_act(
        &self,
        prev: &SignedEnvelope,
        event: SignedEnvelope,
        act_id: Uuid,
    ) -> Result<SignedEnvelope, AppError> {
        let env = event.envelope;
        self.seal(
            Some(prev),
            env.tenant_id,
            &env.event_type,
            &env.principal,
            env.api_key_id,
            with_act_id(env.payload, act_id),
        )
    }
}
