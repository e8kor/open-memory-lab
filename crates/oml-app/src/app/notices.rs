//! In-product notices (ADR-0097 d20, d6, d24): an act that must be loud
//! writes a notice, in its own commit, to the persons it concerns, so
//! disabling the reviewers first cannot suppress it. A person reads their
//! notices in To do and dismisses each. An act against an admin notifies
//! every admin person; offensive narrowing notifies every admin person,
//! every reviewer person of the workspace concerned and the person it acted
//! on; a reach into a person's own root taken at once under `sole_admin`
//! notifies that person. A notice to a disabled person waits for their
//! contest-only sign-in.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::access::effective_roles;
use oml_domain::admin_act::AdminActClass;
use oml_domain::name::Name;
use oml_domain::notice::NoticeKind;
use oml_domain::{Role, SignedEnvelope, event_types};

use super::{App, commit_request};
use crate::ports::{
    Clock, CommitRequest, IdGen, JournalStore, MetadataFold, Scanner, Signer, StoreError,
};
use crate::types::{AdminActRecord, AppError, Bearer, NoticeRecord};

/// A notice the act's commit writes: what it tells about, to whom, and what
/// it names besides the act.
pub(crate) struct NoticeSpec {
    pub kind: NoticeKind,
    pub recipients: BTreeSet<Name>,
    pub detail: Value,
}

/// A name in a JSON target, where it parses.
fn name_at(target: &Value, field: &str) -> Option<Name> {
    target
        .get(field)
        .and_then(Value::as_str)
        .and_then(|s| Name::parse(s).ok())
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The notice `act` writes, if any (ADR-0097 d20): an act against an
    /// admin; an act its target marks `offensive` (d24); a widening act
    /// taken at once under `sole_admin` that reaches a root of a person
    /// other than the actor (d6). `workspace` names the workspace an
    /// offensive act concerns, whose reviewer persons are told.
    pub(crate) async fn notice_for(
        &self,
        tenant_id: Uuid,
        act: &AdminActRecord,
        workspace: Option<Uuid>,
    ) -> Result<Option<NoticeSpec>, AppError> {
        let detail = json!({
            "act": act.act.as_str(),
            "target": act.target,
            "actor_person": act.actor_person.as_ref().map(Name::as_str),
        });
        if act.class() == AdminActClass::AgainstAdmin {
            let mut recipients = self.admin_persons(tenant_id).await?;
            recipients.extend(name_at(&act.target, "user"));
            recipients.extend(name_at(&act.target, "person"));
            return Ok(Some(NoticeSpec {
                kind: NoticeKind::ActAgainstAdmin,
                recipients,
                detail,
            }));
        }
        if act.target.get("offensive") == Some(&Value::Bool(true)) {
            return Ok(Some(
                self.offensive_notice(tenant_id, &act.target, workspace, detail)
                    .await?,
            ));
        }
        if act.sole_admin && act.class() == AdminActClass::Widening {
            let mut recipients: BTreeSet<Name> = act
                .target
                .get("root_persons")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v.as_str().and_then(|s| Name::parse(s).ok()))
                .collect();
            recipients.extend(name_at(&act.target, "root_person"));
            if let Some(actor) = &act.actor_person {
                recipients.remove(actor);
            }
            if !recipients.is_empty() {
                return Ok(Some(NoticeSpec {
                    kind: NoticeKind::RootReach,
                    recipients,
                    detail,
                }));
            }
        }
        Ok(None)
    }

    /// An offensive narrowing notice (d24): every admin person and every
    /// reviewer person of `workspace`, the workspace concerned.
    pub(crate) async fn offensive_notice(
        &self,
        tenant_id: Uuid,
        target: &Value,
        workspace: Option<Uuid>,
        detail: Value,
    ) -> Result<NoticeSpec, AppError> {
        let _ = target;
        let mut recipients = self.admin_persons(tenant_id).await?;
        if let Some(workspace) = workspace {
            recipients.extend(self.reviewer_persons_of(tenant_id, workspace).await?);
        }
        Ok(NoticeSpec {
            kind: NoticeKind::OffensiveNarrowing,
            recipients,
            detail,
        })
    }

    /// Every admin person of the tenant, a disabled one included: their
    /// notice waits for their contest-only sign-in (d24).
    pub(crate) async fn admin_persons(&self, tenant_id: Uuid) -> Result<BTreeSet<Name>, AppError> {
        Ok(self
            .journal
            .admin_standing(tenant_id)
            .await?
            .persons
            .into_iter()
            .map(|p| p.user)
            .collect())
    }

    /// The persons a live person's reviewer key of which holds reviewer on
    /// `workspace` counts as (d24).
    pub(crate) async fn reviewer_persons_of(
        &self,
        tenant_id: Uuid,
        workspace: Uuid,
    ) -> Result<BTreeSet<Name>, AppError> {
        let ids: Vec<Uuid> = self
            .journal
            .workspace_ancestry(tenant_id, workspace)
            .await?
            .unwrap_or_default()
            .iter()
            .map(|w| w.workspace_id)
            .collect();
        let now = self.clock.now();
        let mut persons = BTreeSet::new();
        for r in self.journal.web_person_reviewers(tenant_id).await? {
            if r.key.revoked_at.is_some() || r.key.expired_at(&now) {
                continue;
            }
            if effective_roles(&r.grants, &r.denials, &ids).contains(&Role::Reviewer)
                && let Some(person) = self.counted_person_of(&r.key).await?
            {
                persons.insert(person);
            }
        }
        Ok(persons)
    }

    /// `notice.written` for `spec`, sealed after `prev` under the identity
    /// that sealed `prev`, the act's own, naming the act (ADR-0097 d20).
    pub(crate) fn notice_link(
        &self,
        prev: &SignedEnvelope,
        act_id: Option<Uuid>,
        spec: NoticeSpec,
    ) -> Result<(SignedEnvelope, MetadataFold), AppError> {
        let notice_id = self.ids.new_id();
        let recipients: Vec<Name> = spec.recipients.into_iter().collect();
        let detail = spec.detail;
        let env = &prev.envelope;
        let event = self.seal(
            Some(prev),
            env.tenant_id,
            event_types::NOTICE_WRITTEN,
            &env.principal,
            env.api_key_id,
            json!({
                "notice_id": notice_id.to_string(),
                "kind": spec.kind.as_str(),
                "act_id": act_id.map(|a| a.to_string()),
                "recipients": recipients.iter().map(Name::as_str).collect::<Vec<_>>(),
                "critical": spec.kind.critical(),
                "detail": detail,
            }),
        )?;
        Ok((
            event,
            MetadataFold::NoticeWritten {
                notice_id,
                kind: spec.kind,
                act_id,
                recipients,
                detail,
            },
        ))
    }

    /// `reqs` with the notice `act` writes, if any, sealed after their last
    /// event (ADR-0097 d20): one call for every commit an admin act leads.
    /// The workspace an offensive act concerns is the one its target names.
    pub(crate) async fn with_notice(
        &self,
        act: &AdminActRecord,
        reqs: &mut Vec<CommitRequest>,
    ) -> Result<(), AppError> {
        let Some(prev) = reqs.last().map(|r| r.event.clone()) else {
            return Ok(());
        };
        let workspace = ["workspace_node_id", "workspace_id"].iter().find_map(|f| {
            act.target
                .get(*f)
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
        });
        let Some(spec) = self
            .notice_for(prev.envelope.tenant_id, act, workspace)
            .await?
        else {
            return Ok(());
        };
        if spec.recipients.is_empty() {
            return Ok(());
        }
        let (event, fold) = self.notice_link(&prev, Some(act.act_id), spec)?;
        reqs.push(commit_request(event, fold, None));
        Ok(())
    }

    /// `GET /v2/web/notices` (ADR-0097 d20): the notices of the person the
    /// calling key counts as, oldest first, the dismissed ones too when
    /// `dismissed`. A key that counts as no person is
    /// [`AppError::NotFound`].
    ///
    /// # Errors
    /// The above, the caller's authentication, and the store's.
    pub async fn notices(
        &self,
        bearer: impl Into<Bearer<'_>>,
        dismissed: bool,
    ) -> Result<Vec<NoticeRecord>, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?.key;
        let person = self
            .counted_person_of(&caller)
            .await?
            .ok_or(AppError::NotFound)?;
        Ok(self
            .journal
            .notices(caller.tenant_id, &person, dismissed)
            .await?)
    }

    /// `POST /v2/web/notices/{notice}/dismiss` (ADR-0097 d20): the person
    /// dismisses a notice of their own, journalled as `notice.dismissed`.
    /// It is the person's own act, not an admin act. A notice that is not
    /// the person's, or is already dismissed, is [`AppError::NotFound`].
    ///
    /// # Errors
    /// The above, the caller's authentication, and the store's.
    pub async fn dismiss_notice(
        &self,
        bearer: impl Into<Bearer<'_>>,
        notice_id: Uuid,
    ) -> Result<NoticeRecord, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?.key;
        let person = self
            .counted_person_of(&caller)
            .await?
            .ok_or(AppError::NotFound)?;
        let tenant_id = caller.tenant_id;
        let mut notice = self
            .journal
            .notices(tenant_id, &person, false)
            .await?
            .into_iter()
            .find(|n| n.notice_id == notice_id)
            .ok_or(AppError::NotFound)?;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::NOTICE_DISMISSED,
            &caller.principal,
            Some(caller.key_id),
            json!({
                "notice_id": notice_id.to_string(),
                "recipient": person.as_str(),
            }),
        )?;
        let at = event.envelope.at.clone();
        match self
            .commit(
                event,
                MetadataFold::NoticeDismissed {
                    notice_id,
                    recipient: person,
                },
                None,
            )
            .await
        {
            Err(AppError::Store(StoreError::Backend(_))) => return Err(AppError::NotFound),
            other => {
                other?;
            }
        }
        notice.dismissed_at = Some(at);
        Ok(notice)
    }
}
