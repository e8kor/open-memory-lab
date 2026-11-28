//! Public workspaces (ADR-0098): one flag, `public`, on a `protected` or
//! `frozen` node outside every user root. Set, every key of the tenant whose
//! role reads reads the node's approved items without a grant, and every
//! default recall searches it (d1, d3). Setting it is an admin act of class
//! widening, which waits on a second admin person outside `sole_admin`;
//! clearing it is narrowing, at once, and declines a set that waits (d6).
//! Only an admin person sets or clears it (d6). The effect is
//! `workspace.public_set`, journalled in the commit where the act takes
//! effect (d9).

use std::collections::BTreeMap;

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::admin_act::{AdminActKind, AdminActState};
use oml_domain::name::{NamedKind, Reference};
use oml_domain::{Role, SignedEnvelope, WorkspaceStatus, event_types};

use super::admin_acts::act_payload;
use super::{App, Caller};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::types::{
    AdminActRecord, AppError, Bearer, KeyRecord, PublicFlag, PublicOutcome, PublicRefusal,
    WorkspaceRecord,
};

/// The `workspace.public_set` payload (ADR-0098 d9).
fn public_set_payload(workspace_id: Uuid, value: bool, act_id: Uuid, from: bool) -> Value {
    json!({
        "workspace_id": workspace_id.to_string(),
        "value": value,
        "act_id": act_id.to_string(),
        "from": from,
    })
}

/// Whether `act` is a set of `public` on `workspace_id`.
fn sets_public_on(act: &AdminActRecord, workspace_id: Uuid) -> bool {
    act.act == AdminActKind::WorkspacePublicSet
        && act
            .target
            .get("workspace_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            == Some(workspace_id)
}

/// The doctor's public lines (ADR-0098 d9): each public node, since when,
/// its act, whether that act took effect under `sole_admin`, and its served
/// rows, the load it adds to every default recall; and the public nodes no
/// act could have made public, which only a direct store edit produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublicStats {
    pub public: Vec<PublicNodeStats>,
    /// Public nodes that are `development` or inside a user root
    /// (`workspace.public_ineligible`, critical).
    pub ineligible: Vec<Uuid>,
}

/// One public node as `workspace.public` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicNodeStats {
    pub workspace_id: Uuid,
    pub name: String,
    pub flag: PublicFlag,
    /// The claims on the node whose promoted tip is not a retirement: the
    /// rows every default recall of the tenant now searches.
    pub served_rows: u64,
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/workspaces/{workspace}/public` (ADR-0098 d5, d6, d9): an
    /// admin person sets or clears the node's `public` flag, with a reason.
    /// A set is a widening act: at once under `sole_admin`, marked, and
    /// otherwise `pending_confirmation` until a second admin person
    /// confirms it with `confirmAdminAct`. A clear is narrowing, at once,
    /// and declines a set that waits on the node. Refused, journalling
    /// nothing: every key that is no admin person
    /// ([`AppError::PublicIsAdminOnly`]), a node the tenant does not hold,
    /// and `workspace.public_refused` for a user root or a node under one,
    /// a `development` node, a node already public, a second set while one
    /// waits, and a clear of a node neither public nor waiting.
    ///
    /// # Errors
    /// The refusals above, a widening act sent no reason outside
    /// `sole_admin`, and the store's.
    pub async fn set_workspace_public(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
        public: bool,
        reason: Option<&str>,
    ) -> Result<PublicOutcome, AppError> {
        let caller = self
            .key_act_caller(bearer.into(), "set_workspace_public")
            .await?;
        let admin = self.public_admin(&caller).await?;
        let tenant_id = admin.tenant_id;
        let workspace_id = self
            .resolve(&admin, NamedKind::Workspace, &workspace)
            .await?;
        let node = self
            .journal
            .workspace(tenant_id, workspace_id)
            .await?
            .ok_or(AppError::NotFound)?;
        self.observer.workspace(workspace_id);
        let pending = self.pending_public_set(tenant_id, workspace_id).await?;
        let refused = |reason: PublicRefusal, act_id: Option<Uuid>| {
            Err(AppError::WorkspacePublicRefused {
                workspace_id,
                reason,
                act_id,
            })
        };
        let target = json!({ "workspace_id": workspace_id.to_string() });
        let prev = self.head_event(tenant_id).await?;
        if public {
            // A person's own work never becomes public, and its cap at
            // `development` is why (d5); then the status, the flag and a
            // set that waits.
            if node.user().is_some() {
                return refused(PublicRefusal::UserRoot, None);
            }
            if node.status == WorkspaceStatus::Development {
                return refused(PublicRefusal::Status, None);
            }
            if node.public {
                return refused(PublicRefusal::AlreadyPublic, None);
            }
            if let Some(act) = &pending {
                return refused(PublicRefusal::Pending, Some(act.act_id));
            }
            let act = self
                .begin_admin_act(
                    &admin,
                    AdminActKind::WorkspacePublicSet,
                    target.clone(),
                    reason,
                    target,
                    prev.as_ref(),
                )
                .await?;
            let act_id = act.act_id();
            if act.pending() {
                self.commit_with_act(Some(act), Vec::new()).await?;
                return Ok(PublicOutcome {
                    workspace_id,
                    public: false,
                    act_id,
                    state: AdminActState::PendingConfirmation,
                });
            }
            let link =
                self.public_set_link(&admin, &act.event, workspace_id, true, act_id, false)?;
            self.commit_with_act(Some(act), vec![link]).await?;
            return Ok(PublicOutcome {
                workspace_id,
                public: true,
                act_id,
                state: AdminActState::Effective,
            });
        }
        if !node.public && pending.is_none() {
            return refused(PublicRefusal::NotPublic, None);
        }
        let act = self
            .begin_admin_act(
                &admin,
                AdminActKind::WorkspacePublicClear,
                target,
                reason,
                json!({}),
                prev.as_ref(),
            )
            .await?;
        let act_id = act.act_id();
        let mut last: SignedEnvelope = act.event.clone();
        let mut links = Vec::new();
        // A clear declines the set that waits on the node (d6).
        if let Some(mut waiting) = pending {
            waiting.state = AdminActState::Declined;
            waiting.settled_by_key_id = Some(admin.key_id);
            let event = self.seal(
                Some(&last),
                tenant_id,
                event_types::ADMIN_ACT,
                &admin.principal,
                Some(admin.key_id),
                act_payload(&waiting),
            )?;
            last = event.clone();
            links.push((event, MetadataFold::AdminAct(Box::new(waiting))));
        }
        if node.public {
            links.push(self.public_set_link(&admin, &last, workspace_id, false, act_id, true)?);
        }
        self.commit_with_act(Some(act), links).await?;
        Ok(PublicOutcome {
            workspace_id,
            public: false,
            act_id,
            state: AdminActState::Effective,
        })
    }

    /// The admin person `caller` acts as, or [`AppError::PublicIsAdminOnly`]:
    /// a linked admin key or `<user>-admin`, never a reviewer key, an agent
    /// or an admin key that counts as no person (ADR-0098 d6).
    async fn public_admin(&self, caller: &Caller) -> Result<KeyRecord, AppError> {
        let key = caller.key();
        if key.role != Role::Admin || self.counted_person_of(key).await?.is_none() {
            return Err(AppError::PublicIsAdminOnly { role: key.role });
        }
        Ok(key.clone())
    }

    /// The set of `public` on `workspace_id` that still waits on a second
    /// admin person, at most one (ADR-0098 d6); one past its life is
    /// expired, final, and waits on no one (ADR-0097 d23).
    pub(crate) async fn pending_public_set(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<AdminActRecord>, AppError> {
        let now = self.clock.now();
        Ok(self
            .journal
            .admin_acts(tenant_id, Some(AdminActState::PendingConfirmation))
            .await?
            .into_iter()
            .filter(|a| sets_public_on(a, workspace_id))
            .find(|a| a.state_at(&now) == AdminActState::PendingConfirmation))
    }

    /// The `workspace.public_set` link sealed on `prev`.
    pub(crate) fn public_set_link(
        &self,
        admin: &KeyRecord,
        prev: &SignedEnvelope,
        workspace_id: Uuid,
        value: bool,
        act_id: Uuid,
        from: bool,
    ) -> Result<(SignedEnvelope, MetadataFold), AppError> {
        let event = self.seal(
            Some(prev),
            admin.tenant_id,
            event_types::WORKSPACE_PUBLIC_SET,
            &admin.principal,
            Some(admin.key_id),
            public_set_payload(workspace_id, value, act_id, from),
        )?;
        Ok((
            event,
            MetadataFold::WorkspacePublicSet {
                workspace_id,
                public: value,
                act_id,
            },
        ))
    }

    /// The effect of a confirmed set (ADR-0098 d6, d9): the flag, in the
    /// confirmation's commit. A node made public since by a direct edit
    /// changes nothing.
    pub(crate) async fn confirmed_public_set(
        &self,
        admin: &KeyRecord,
        act: &AdminActRecord,
        links: &mut Vec<(SignedEnvelope, MetadataFold)>,
    ) -> Result<(), AppError> {
        let workspace_id = act
            .effect
            .get("workspace_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .ok_or_else(|| {
                AppError::StoredResponse(format!("admin act {} names no workspace", act.act_id))
            })?;
        let node = self
            .journal
            .workspace(admin.tenant_id, workspace_id)
            .await?
            .ok_or_else(|| {
                AppError::StoredResponse(format!(
                    "admin act {} names workspace {workspace_id}, which the tenant does not hold",
                    act.act_id
                ))
            })?;
        if node.public {
            return Ok(());
        }
        let prev = links
            .last()
            .map(|(e, _)| e.clone())
            .expect("the act's link");
        links.push(self.public_set_link(admin, &prev, workspace_id, true, act.act_id, false)?);
        Ok(())
    }

    /// Each public node of `nodes` with its flag as the reads serve it: since
    /// the act that set it took effect, and whether it did under
    /// `sole_admin` (ADR-0098 d9). A node not public reads no act, so a
    /// tenant with no public node reads nothing more.
    pub(crate) async fn public_flags(
        &self,
        tenant_id: Uuid,
        nodes: &[&WorkspaceRecord],
    ) -> Result<BTreeMap<Uuid, PublicFlag>, AppError> {
        let mut flags = BTreeMap::new();
        for node in nodes.iter().filter(|n| n.public) {
            let act = match node.public_act_id {
                Some(act_id) => self.journal.admin_act(tenant_id, act_id).await?,
                None => None,
            };
            flags.insert(
                node.workspace_id,
                PublicFlag {
                    value: true,
                    since: act.as_ref().map(|a| match a.state {
                        AdminActState::Confirmed => a.settled_at.clone().unwrap_or(a.at.clone()),
                        _ => a.at.clone(),
                    }),
                    act_id: node.public_act_id,
                    sole_admin: act.as_ref().is_some_and(|a| a.sole_admin),
                    pending_act_id: None,
                },
            );
        }
        Ok(flags)
    }

    /// The doctor's `workspace.public` and `workspace.public_ineligible`
    /// (ADR-0098 d9): the tenant's public nodes with their flags and served
    /// rows, and the ones no act could have made public. Reads no claim
    /// text and journals nothing.
    ///
    /// # Errors
    /// The store's.
    pub async fn public_stats(&self, tenant_id: Uuid) -> Result<PublicStats, AppError> {
        let nodes = self.journal.public_workspaces(tenant_id).await?;
        let refs: Vec<&WorkspaceRecord> = nodes.iter().collect();
        let flags = self.public_flags(tenant_id, &refs).await?;
        let mut stats = PublicStats::default();
        for node in &nodes {
            if node.status == WorkspaceStatus::Development || node.user().is_some() {
                stats.ineligible.push(node.workspace_id);
            }
            let served_rows = self
                .journal
                .approved_by_kind(tenant_id, node.workspace_id)
                .await?
                .iter()
                .map(|(_, n)| n)
                .sum();
            stats.public.push(PublicNodeStats {
                workspace_id: node.workspace_id,
                name: node.name.as_str().to_owned(),
                flag: flags.get(&node.workspace_id).cloned().unwrap_or_default(),
                served_rows,
            });
        }
        Ok(stats)
    }

    /// [`Self::public_stats`] for the admin key `bearer` names, as the
    /// admin rescan window serves it.
    ///
    /// # Errors
    /// The caller's authentication and role, and the store's.
    pub async fn admin_public_stats(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<PublicStats, AppError> {
        let admin = self.admin_caller(bearer.into(), "rescan").await?.key;
        self.public_stats(admin.tenant_id).await
    }

    /// Doctor's `workspace.flagged_creates` (ADR-0097 d31, the operator's
    /// decision of 2026-11-27): per creating key, the workspaces it created
    /// in the shared tree under a grant carrying `create`, which no meter
    /// counts, for the admin key `bearer` names, as the admin rescan window
    /// serves it.
    ///
    /// # Errors
    /// The caller's authentication and role, and the store's.
    pub async fn admin_flagged_creates(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<Vec<(Uuid, u64)>, AppError> {
        let admin = self.admin_caller(bearer.into(), "rescan").await?.key;
        Ok(self.journal.flagged_creates(admin.tenant_id).await?)
    }

    /// The public nodes `key` reads through the flag, apart from its grants,
    /// a global deny applied (ADR-0098 d9, the one-key read).
    pub(crate) async fn public_reach_of(
        &self,
        key: &KeyRecord,
        access: &crate::types::KeyAccess,
    ) -> Result<Vec<Uuid>, AppError> {
        if !oml_domain::access::reads_public(key.role) {
            return Ok(Vec::new());
        }
        let mut reach = Vec::new();
        for &node in &access.public_nodes {
            let ancestry = self.ancestry(key.tenant_id, node).await?;
            if !access
                .denials
                .iter()
                .any(|d| ancestry.contains(&d.denial.node))
            {
                reach.push(node);
            }
        }
        Ok(reach)
    }
}

/// When a declined `sole_admin` set made the node public, the clear its
/// decline journals (ADR-0097 d10: declining a `sole_admin` act revokes it).
pub(crate) fn revokes_public(act: &AdminActRecord, node: &WorkspaceRecord) -> bool {
    node.public && node.public_act_id == Some(act.act_id)
}
