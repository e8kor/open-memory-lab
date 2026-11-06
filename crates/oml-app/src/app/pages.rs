//! The acts of the web pages through the person's session keys (ADR-0045
//! d6, d9–d13, d16, d21): the keys a first sign-in issues, the caller each
//! capacity acts as, the user page's own-key acts, the admin page's
//! capacities and queue, and the admin read of every memory's text and
//! attribution, journalled.
//!
//! A session key has no value: the transport never holds one. It asks for
//! the [`Caller`] of a capacity from a live session, re-checked on every
//! request, and acts through [`Bearer::Session`]; every act lands in the
//! journal under the session key, as a CLI act lands under its key.

use std::collections::BTreeSet;

use serde_json::json;
use uuid::Uuid;

use oml_domain::access::{Covers, check_issuance};
use oml_domain::name::{Name, NamedKind, Reference, UNIVERSE};
use oml_domain::web::SESSION_KEY_SUFFIXES;
use oml_domain::{EventTime, HeldBy, Role, Sha256Hash, event_types};

use super::{App, Caller, Issuance, issuance_refusal, require_role};
use crate::ports::{Clock, ConnectionStore, IdGen, MetadataFold, Scanner, Signer, WebStore};
use oml_domain::in_progress::WorkareaMode;
use oml_domain::origin::OriginKind;
use oml_domain::{MemoryStatus, WorkareaStatus};

use crate::cursor::ListingCursor;
use crate::types::{
    AdminSearch, AdminState, AppError, Bearer, ConnectCodeIssued, Credential, DenialRecord,
    GrantRequest, IssueKeyRequest, IssuedKey, KeyAccess, KeyRecord, MemoryRecord, QueueDepth,
    WebEntry, WebSessionRecord, WorkareaRecord,
};
use crate::types::{AnchorFilter, ListRequest, ListingPosition, MemoryPage, ServedMemory};

/// The capacity a page act needs: one session key each (ADR-0045 d6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capacity {
    /// `<user>-web`: the user page.
    Web,
    /// `<user>-review`: review and supersession proposals.
    Review,
    /// `<user>-curate`: curation into `universe`.
    Curate,
    /// `<user>-admin`: the allowlist, the capacities, the queue and the
    /// admin read.
    Admin,
}

impl Capacity {
    /// The suffix of the key's name, `<user>-<suffix>`.
    #[must_use]
    pub fn suffix(self) -> &'static str {
        match self {
            Capacity::Web => "web",
            Capacity::Review => "review",
            Capacity::Curate => "curate",
            Capacity::Admin => "admin",
        }
    }
}

/// One of the person's keys as the user page lists it (ADR-0045 d9).
#[derive(Debug, Clone)]
pub struct OwnKey {
    pub key: KeyRecord,
    pub access: Option<KeyAccess>,
    /// A global denial on `universe` cuts it off.
    pub cut_off: bool,
}

/// One workarea's queue with its path (ADR-0045 d10).
#[derive(Debug, Clone)]
pub struct QueueRow {
    pub depth: QueueDepth,
    pub path: String,
}

/// One verdict on a memory, as the journal holds it (ADR-0045 d21).
#[derive(Debug, Clone)]
pub struct VerdictTrail {
    pub verdict: String,
    pub key_id: Option<Uuid>,
    pub key_name: Option<Name>,
    pub principal: String,
    pub reason: Option<String>,
    pub at: EventTime,
}

/// A memory as the admin read serves it: its text, where it lives, and who
/// wrote it, and for one memory who reviewed it (ADR-0045 d21). ADR-0044
/// d9's anonymous view does not apply.
#[derive(Debug, Clone)]
pub struct AdminMemory {
    pub memory: MemoryRecord,
    pub state: AdminState,
    pub workarea_path: String,
    pub author: Option<KeyRecord>,
    pub author_user: Option<Name>,
    pub verdicts: Vec<VerdictTrail>,
}

/// The admin read's answer: the page and the cursor of the next.
#[derive(Debug, Clone)]
pub struct AdminPage {
    pub items: Vec<AdminMemory>,
    pub next: Option<(EventTime, Uuid)>,
}

/// The admin read's search as the admin page asks it (ADR-0045 d22, d24).
#[derive(Debug, Clone)]
pub struct AdminSearchRequest<'a> {
    pub query: Option<&'a str>,
    pub workarea: Option<Reference>,
    pub state: Option<AdminState>,
    pub origin: Option<OriginKind>,
    /// The authoring key, by name or id.
    pub author: Option<Reference>,
    pub after: Option<(EventTime, Uuid)>,
    pub limit: usize,
}

/// The user page's Memories list as the person filters it (ADR-0045 d22,
/// d23); the states are the admin read's words.
#[derive(Debug, Clone, Default)]
pub struct WebListRequest {
    pub workarea: Option<Reference>,
    pub state: Option<AdminState>,
    pub origin: Option<OriginKind>,
    /// One of the person's own keys, by name.
    pub author: Option<Name>,
    pub cursor: Option<String>,
    pub limit: usize,
}

/// How many rows the Atlas counts per view before it says "at least"
/// (ADR-0045 d30): ten pages of the largest list page, chosen, not measured.
pub const REGION_COUNT_MAX: usize = 2_000;

/// One workarea of the Atlas (ADR-0045 d30): the node, its path, and its
/// rows the viewer may read by state: approved (promoted tips), held,
/// unreviewed inside an in-progress scope, and every other state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub workarea: WorkareaRecord,
    pub path: String,
    pub approved: u64,
    pub held: u64,
    pub in_progress: u64,
    pub other: u64,
}

/// The Atlas's workareas and whether the count stopped at
/// [`REGION_COUNT_MAX`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regions {
    pub regions: Vec<Region>,
    pub partial: bool,
    /// The first region's ancestors, `universe` first, each its id and
    /// name: the breadcrumb out of a zoomed Atlas.
    pub ancestors: Vec<(Uuid, Name)>,
}

/// One act a workarea's dropdown may offer (ADR-0045 d28).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkareaOffer {
    /// Propose the next rung (ADR-0009 d3).
    Propose(WorkareaStatus),
    /// Ratify a live proposal another key made.
    Ratify {
        transition_id: Uuid,
        from: WorkareaStatus,
        to: WorkareaStatus,
    },
    /// Put the workarea in progress (ADR-0030 d1).
    InProgress,
    /// Commit an in-progress root (ADR-0030 d6).
    Commit,
}

/// What a workarea's dropdown shows: the node's status and mode, the acts
/// its verbs admit now, and a commit withheld with the held rows that stop
/// it (ADR-0045 d28).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkareaOffers {
    pub workarea_id: Uuid,
    pub status: WorkareaStatus,
    pub in_progress: bool,
    pub offers: Vec<WorkareaOffer>,
    /// `Some(held)` when the node is an in-progress root whose commit the
    /// held rows refuse.
    pub commit_withheld: Option<u64>,
}

/// Counts `m` on its region by state; a held row is in progress when its
/// node or an ancestor on the page's tree is in progress (ADR-0030 d1). A
/// row on a node the tree did not serve is not counted.
fn count_into(regions: &mut [Region], m: &MemoryRecord) {
    let inside = {
        let mut node = Some(m.workarea_id);
        let mut found = false;
        while let Some(id) = node {
            match regions.iter().find(|r| r.workarea.workarea_id == id) {
                Some(r) if r.workarea.in_progress => {
                    found = true;
                    break;
                }
                Some(r) => node = r.workarea.parent_id,
                None => break,
            }
        }
        found
    };
    let Some(region) = regions
        .iter_mut()
        .find(|r| r.workarea.workarea_id == m.workarea_id)
    else {
        return;
    };
    match AdminState::of(m) {
        AdminState::Promoted => region.approved += 1,
        AdminState::Held if inside => region.in_progress += 1,
        AdminState::Held => region.held += 1,
        _ => region.other += 1,
    }
}

fn field(field: &'static str, message: &'static str) -> AppError {
    AppError::WebField { field, message }
}

fn key_name(user: &Name, suffix: &str) -> Result<Name, AppError> {
    Name::parse(&format!("{}-{suffix}", user.as_str()))
        .map_err(|_| field("name", "the key's name is not a name"))
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: WebStore + ConnectionStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    // -- session keys -----------------------------------------------------

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
                // The issuance default; a page offers no scope (proposed ADR-0037
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
    /// the key.
    pub async fn web_caller(
        &self,
        session: &WebSessionRecord,
        capacity: Capacity,
    ) -> Result<Caller, AppError> {
        let missing = AppError::WebKeyMissing {
            capacity: capacity.suffix(),
        };
        if capacity == Capacity::Admin && !session.admin {
            return Err(AppError::NotFound);
        }
        let name = key_name(&session.user, capacity.suffix())?;
        let key = match self
            .journal
            .resolve_name(session.tenant_id, NamedKind::Key, &name)
            .await?
        {
            Some(id) => self.journal.key(session.tenant_id, id).await?,
            None => None,
        }
        .filter(|k| {
            k.credential == Credential::Session
                && k.session_user.as_ref() == Some(&session.user)
                && k.revoked_at.is_none()
        })
        .ok_or(missing)?;
        self.observer.credential(Some(&key), None);
        let mut caller = self.caller_of(key).await?;
        if capacity == Capacity::Web
            && let Some(root) = caller.key.user
        {
            caller.own_keys = self
                .journal
                .web_user_keys(session.tenant_id, root)
                .await?
                .into_iter()
                .map(|k| k.key_id)
                .collect();
            if !caller.own_keys.contains(&caller.key.key_id) {
                caller.own_keys.push(caller.key.key_id);
            }
        }
        Ok(caller)
    }

    // -- the user page's keys ---------------------------------------------

    /// The person's keys (ADR-0045 d9): every key naming their user, with
    /// its grants and whether a denial on `universe` cuts it off.
    pub async fn web_own_keys(&self, web: &Caller) -> Result<Vec<OwnKey>, AppError> {
        let tenant_id = web.key.tenant_id;
        let Some(root) = web.key.user else {
            return Ok(Vec::new());
        };
        let universe = self.universe_of(tenant_id, root).await?;
        let mut keys = self.journal.web_user_keys(tenant_id, root).await?;
        // The person's tenant session keys too: `<user>-review`, and
        // `<user>-curate` and `<user>-admin` where they exist (ADR-0045 d6).
        if let Some(user) = &web.key.session_user {
            for key in self.journal.web_session_keys(tenant_id, user).await? {
                if !keys.iter().any(|k| k.key_id == key.key_id) {
                    keys.push(key);
                }
            }
        }
        let mut out = Vec::new();
        for key in keys {
            let access = self.journal.key_access(tenant_id, key.key_id).await?;
            let cut_off = access
                .as_ref()
                .is_some_and(|a| a.denials.iter().any(|d| Some(d.denial.node) == universe));
            out.push(OwnKey {
                key,
                access,
                cut_off,
            });
        }
        Ok(out)
    }

    /// `issue_key` and `connect_key` (ADR-0045 d9, d16): a reader or writer
    /// key naming the person's user, granted on their root and nothing
    /// else, named `<user>-<name>` with the session suffixes reserved,
    /// issued by `<user>-web`; with `connector`, a connector key and its
    /// connect code. Issuance never amplifies authority: `<user>-web` holds
    /// writer there, and the key it issues holds no more.
    pub async fn web_issue_key(
        &self,
        web: &Caller,
        suffix: &str,
        role: Role,
        connector: bool,
    ) -> Result<IssuedKey, AppError> {
        let user_name = self.web_user_of(web).await?;
        if SESSION_KEY_SUFFIXES.contains(&suffix) {
            return Err(field(
                "name",
                "the names ending in web, review, curate and admin are the session keys'",
            ));
        }
        let name = Name::parse(&format!("{}-{suffix}", user_name.as_str())).map_err(|_| {
            field(
                "name",
                "a key's name is lower-case letters, digits and hyphens",
            )
        })?;
        let root = Reference::Name(user_name.clone());
        let grant = |role: Role| GrantRequest {
            node: root.clone(),
            role,
            deny: Vec::new(),
            covers: Covers::Subtree,
        };
        let grants = match role {
            Role::Writer => vec![grant(Role::Writer), grant(Role::Reader)],
            Role::Reader => vec![grant(Role::Reader)],
            _ => return Err(field("role", "a key you issue is a reader or a writer")),
        };
        let req = IssueKeyRequest {
            profile: None,
            // The issuance default; a page offers no scope (proposed ADR-0037
            // d17, d18 Q5).
            guardrail_scope: None,
            role,
            principal: name.as_str().to_owned(),
            name,
            held_by: Some(HeldBy::Agent),
            default_workarea: None,
            crew: None,
            grants,
            connect_code: connector,
            user: Some(user_name),
        };
        self.issue_key_by(&web.key, &req, Issuance::PersonsOwn)
            .await
    }

    /// `connect_code` (ADR-0045 d9): a fresh code for one of the person's
    /// connector keys, minted by `<user>-web`.
    pub async fn web_connect_code(
        &self,
        web: &Caller,
        key: &Name,
    ) -> Result<ConnectCodeIssued, AppError> {
        let target = self.own_key(web, key).await?;
        self.mint_connect_code_by(&web.key, Reference::Id(target.key_id))
            .await
    }

    /// `cut_off` (ADR-0045 d9, d13): one of the person's keys denied on
    /// `universe`, journalled as `key.denied`, by `<user>-web` on the user
    /// page or `<user>-admin` on the admin page; the only cut-off while
    /// revocation is not built. A session key is never cut off here.
    pub async fn web_cut_off(&self, acting: &Caller, key: &Name) -> Result<DenialRecord, AppError> {
        let target = match acting.key.role {
            Role::Admin => self.named_key(acting.key.tenant_id, key).await?,
            _ => self.own_key(acting, key).await?,
        };
        if target.credential == Credential::Session {
            return Err(field(
                "key",
                "a session key ends with its email, not by a cut-off",
            ));
        }
        if target.role == Role::Admin {
            return Err(field("key", "an admin key carries no grant to cut off"));
        }
        let universe = Reference::Name(Name::parse(UNIVERSE).expect("universe is a name"));
        self.deny_key_by(&acting.key, Reference::Id(target.key_id), universe)
            .await
    }

    /// The user page's user: the name of the root `<user>-web` names.
    async fn web_user_of(&self, web: &Caller) -> Result<Name, AppError> {
        let root = web
            .key
            .user
            .ok_or(AppError::WebKeyMissing { capacity: "web" })?;
        self.journal
            .workarea(web.key.tenant_id, root)
            .await?
            .map(|w| w.name)
            .ok_or(AppError::NotFound)
    }

    /// One of the person's keys by name: its user root is theirs.
    async fn own_key(&self, web: &Caller, key: &Name) -> Result<KeyRecord, AppError> {
        let target = self.named_key(web.key.tenant_id, key).await?;
        if web.key.user.is_none() || target.user != web.key.user {
            return Err(AppError::NotFound);
        }
        Ok(target)
    }

    async fn named_key(&self, tenant_id: Uuid, key: &Name) -> Result<KeyRecord, AppError> {
        let id = self
            .journal
            .resolve_name(tenant_id, NamedKind::Key, key)
            .await?
            .ok_or(AppError::NotFound)?;
        self.journal
            .key(tenant_id, id)
            .await?
            .ok_or(AppError::NotFound)
    }

    /// The tenant's `universe`, read off any node's ancestry.
    async fn universe_of(&self, tenant_id: Uuid, node: Uuid) -> Result<Option<Uuid>, AppError> {
        if let Some(ancestry) = self.journal.workarea_ancestry(tenant_id, node).await? {
            return Ok(ancestry.last().map(|w| w.workarea_id));
        }
        Ok(self
            .journal
            .resolve_name(
                tenant_id,
                NamedKind::Workarea,
                &Name::parse(UNIVERSE).expect("universe is a name"),
            )
            .await?)
    }

    // -- the admin page's capacities ----------------------------------------

    /// A user's keys, for the removal confirmation (ADR-0045 d13): the keys
    /// naming that user, session keys aside.
    pub async fn web_keys_of_user(
        &self,
        admin: &Caller,
        user: &Name,
    ) -> Result<Vec<OwnKey>, AppError> {
        require_role(&admin.key, Role::Admin, "web_keys")?;
        let tenant_id = admin.key.tenant_id;
        let Some(root) = self
            .journal
            .resolve_name(tenant_id, NamedKind::Workarea, user)
            .await?
        else {
            return Ok(Vec::new());
        };
        let universe = self.universe_of(tenant_id, root).await?;
        let mut out = Vec::new();
        for key in self.journal.web_user_keys(tenant_id, root).await? {
            if key.credential == Credential::Session {
                continue;
            }
            let access = self.journal.key_access(tenant_id, key.key_id).await?;
            let cut_off = access
                .as_ref()
                .is_some_and(|a| a.denials.iter().any(|d| Some(d.denial.node) == universe));
            out.push(OwnKey {
                key,
                access,
                cut_off,
            });
        }
        Ok(out)
    }

    /// `grant_review` (ADR-0045 d10): the user's `<user>-review` gains
    /// dreamer and reader on `workarea`, journalled as `grant.issued` by
    /// `<user>-admin`. The key exists from the user's first sign-in.
    pub async fn web_grant_review(
        &self,
        admin: &Caller,
        user: &Name,
        workarea: Reference,
    ) -> Result<(), AppError> {
        require_role(&admin.key, Role::Admin, "grant_review")?;
        let review = self
            .session_key_of(admin.key.tenant_id, user, Capacity::Review)
            .await?
            .ok_or(AppError::WebKeyMissing { capacity: "review" })?;
        let grants = [Role::Dreamer, Role::Reader].map(|role| GrantRequest {
            node: workarea.clone(),
            role,
            deny: Vec::new(),
            covers: Covers::Subtree,
        });
        self.add_grants(&admin.key, &review, &grants).await
    }

    /// `grant_curation` (ADR-0045 d10, ADR-0044 d7): the user's
    /// `<user>-curate`, a tenant writer key with writer on the `universe`
    /// node alone and reader on `workarea`, created by `<user>-admin` at the
    /// first designation and extended by a reader grant after it.
    pub async fn web_grant_curation(
        &self,
        admin: &Caller,
        user: &Name,
        workarea: Reference,
    ) -> Result<(), AppError> {
        require_role(&admin.key, Role::Admin, "grant_curation")?;
        let tenant_id = admin.key.tenant_id;
        let reader = GrantRequest {
            node: workarea,
            role: Role::Reader,
            deny: Vec::new(),
            covers: Covers::Subtree,
        };
        if let Some(curate) = self
            .session_key_of(tenant_id, user, Capacity::Curate)
            .await?
        {
            return self.add_grants(&admin.key, &curate, &[reader]).await;
        }
        let name = key_name(user, Capacity::Curate.suffix())?;
        let req = IssueKeyRequest {
            profile: None,
            // The issuance default; a page offers no scope (proposed ADR-0037
            // d17, d18 Q5).
            guardrail_scope: None,
            role: Role::Writer,
            principal: name.as_str().to_owned(),
            name,
            held_by: Some(HeldBy::Human),
            default_workarea: None,
            crew: None,
            grants: vec![
                GrantRequest {
                    node: Reference::Name(Name::parse(UNIVERSE).expect("universe is a name")),
                    role: Role::Writer,
                    deny: Vec::new(),
                    covers: Covers::Node,
                },
                reader,
            ],
            connect_code: false,
            user: None,
        };
        self.issue_key_by(&admin.key, &req, Issuance::Session(user))
            .await
            .map(|_| ())
    }

    /// The live session key of `capacity` serving `user`.
    async fn session_key_of(
        &self,
        tenant_id: Uuid,
        user: &Name,
        capacity: Capacity,
    ) -> Result<Option<KeyRecord>, AppError> {
        let name = key_name(user, capacity.suffix())?;
        Ok(self
            .journal
            .web_session_keys(tenant_id, user)
            .await?
            .into_iter()
            .find(|k| k.revoked_at.is_none() && k.name.as_ref() == Some(&name)))
    }

    /// Grants added to an existing key by an admin key, each a
    /// `grant.issued`: the key's role admits every grant role, as at
    /// issuance (roles-and-keys.md admissibility). A grant the key already
    /// holds on the node is not added twice.
    async fn add_grants(
        &self,
        admin: &KeyRecord,
        key: &KeyRecord,
        grants: &[GrantRequest],
    ) -> Result<(), AppError> {
        let tenant_id = admin.tenant_id;
        let held = self
            .journal
            .key_access(tenant_id, key.key_id)
            .await?
            .map(|a| a.grants)
            .unwrap_or_default();
        let mut resolved = Vec::new();
        for g in grants {
            let node = self.resolve(admin, NamedKind::Workarea, &g.node).await?;
            if self.journal.workarea(tenant_id, node).await?.is_none() {
                return Err(AppError::NotFound);
            }
            if held.iter().any(|h| {
                h.grant.node == node && h.grant.role == g.role && h.grant.covers == g.covers
            }) {
                continue;
            }
            resolved.push(oml_domain::access::Grant {
                grant_id: self.ids.new_id(),
                node,
                role: g.role,
                deny: Vec::new(),
                covers: g.covers,
            });
        }
        check_issuance(Role::Admin, key.role, &resolved).map_err(issuance_refusal)?;
        for grant in resolved {
            let prev = self
                .head_event(tenant_id)
                .await?
                .ok_or(AppError::NotFound)?;
            self.commit_grant(&prev, admin, key.key_id, grant).await?;
        }
        Ok(())
    }

    /// The review queue's depth and age per workarea, tenant-wide, no
    /// content (ADR-0045 d10, amending ADR-0026 d15).
    pub async fn web_queue(&self, admin: &Caller) -> Result<Vec<QueueRow>, AppError> {
        require_role(&admin.key, Role::Admin, "review_queue")?;
        let tenant_id = admin.key.tenant_id;
        let mut out = Vec::new();
        for depth in self.journal.web_queue(tenant_id).await? {
            let path = self.path_of(tenant_id, depth.workarea_id).await?;
            out.push(QueueRow { depth, path });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// A workarea's path, for a page to name a grant's node by.
    pub async fn web_path(&self, caller: &Caller, node: Uuid) -> Result<String, AppError> {
        self.path_of(caller.key.tenant_id, node).await
    }

    async fn path_of(&self, tenant_id: Uuid, node: Uuid) -> Result<String, AppError> {
        Ok(self
            .journal
            .workarea_ancestry(tenant_id, node)
            .await?
            .map(|a| super::derived_path(&a))
            .unwrap_or_default())
    }

    /// Whether the tenant's list keeps an admin entry beside `email_hash`:
    /// the page never removes or un-flags the last (ADR-0045 d10).
    pub async fn web_other_admin(
        &self,
        admin: &Caller,
        email_hash: &Sha256Hash,
    ) -> Result<bool, AppError> {
        let tenant_id = admin.key.tenant_id;
        let mut after: Option<Sha256Hash> = None;
        loop {
            let page = self
                .journal
                .web_entries(tenant_id, after.as_ref(), 200)
                .await?;
            if page
                .iter()
                .any(|e| e.admin && e.email.digest() != *email_hash)
            {
                return Ok(true);
            }
            match page.last() {
                Some(last) if page.len() == 200 => after = Some(last.email.digest()),
                _ => return Ok(false),
            }
        }
    }

    /// Every entry of the tenant (ADR-0045 d10), for the admin page.
    pub async fn web_all_entries(&self, admin: &Caller) -> Result<Vec<WebEntry>, AppError> {
        require_role(&admin.key, Role::Admin, "web_emails")?;
        let mut entries = Vec::new();
        let mut after: Option<Sha256Hash> = None;
        loop {
            let page = self
                .journal
                .web_entries(admin.key.tenant_id, after.as_ref(), 200)
                .await?;
            let more = page.len() == 200;
            after = page.last().map(|e| e.email.digest());
            entries.extend(page);
            if !more {
                break;
            }
        }
        entries.sort_by(|a, b| a.email.cmp(&b.email));
        Ok(entries)
    }

    // -- the Memories list and the workarea dropdowns ------------------------

    /// The user page's Memories list (ADR-0045 d22, d23): the scoped
    /// listing `<user>-web` is served at `universe` (or at the filter's
    /// workarea) and the own-work listing of every key naming the user,
    /// each read after the same position and merged newest first, a row in
    /// both shown once. `author` narrows to one of the person's keys and
    /// drops the scoped listing; a key that is not theirs is not found.
    pub async fn web_memories(
        &self,
        web: &Caller,
        req: &WebListRequest,
    ) -> Result<MemoryPage, AppError> {
        let tenant_id = web.key.tenant_id;
        let author = match &req.author {
            None => None,
            Some(name) => Some(
                self.journal
                    .resolve_name(tenant_id, NamedKind::Key, name)
                    .await?
                    .filter(|id| web.own_key_ids().contains(id))
                    .ok_or(AppError::NotFound)?,
            ),
        };
        let (state, tip) = match req.state {
            None => (None, None),
            Some(AdminState::Held) => (Some(MemoryStatus::Held), None),
            Some(AdminState::Promoted) => (Some(MemoryStatus::Promoted), Some(true)),
            Some(AdminState::Superseded) => (Some(MemoryStatus::Promoted), Some(false)),
            Some(AdminState::Rejected) => (Some(MemoryStatus::Rejected), None),
            Some(AdminState::Demoted) => (Some(MemoryStatus::Demoted), None),
        };
        let listing = |own: bool, workarea: Option<Reference>| ListRequest {
            own,
            workarea,
            state,
            kind: None,
            claim_key: None,
            since: None,
            until: None,
            limit: req.limit,
            cursor: req.cursor.clone(),
            anchor: AnchorFilter {
                repo: None,
                path_prefix: None,
                origin_kinds: req.origin.into_iter().collect(),
            },
            author,
            tip,
        };
        let own = self
            .list_memories(Bearer::Session(web), &listing(true, req.workarea.clone()))
            .await?;
        let scoped = if author.is_some() {
            None
        } else {
            let scope = req.workarea.clone().unwrap_or_else(|| {
                Reference::Name(Name::parse(UNIVERSE).expect("universe is a name"))
            });
            Some(
                self.list_memories(Bearer::Session(web), &listing(false, Some(scope)))
                    .await?,
            )
        };
        let more =
            own.next_cursor.is_some() || scoped.as_ref().is_some_and(|p| p.next_cursor.is_some());
        let mut rows: Vec<ServedMemory> = own.items;
        rows.extend(scoped.into_iter().flat_map(|p| p.items));
        let position =
            |m: &ServedMemory| (m.memory.created_at.as_str().to_owned(), m.memory.memory_id);
        rows.sort_by_key(|m| std::cmp::Reverse(position(m)));
        rows.dedup_by_key(|m| m.memory.memory_id);
        let more = more || rows.len() > req.limit;
        rows.truncate(req.limit);
        let next_cursor = rows.last().filter(|_| more).map(|last| {
            ListingCursor {
                key_id: web.key.key_id,
                position: ListingPosition {
                    created_at: last.memory.created_at.clone(),
                    artifact_id: last.memory.memory_id,
                },
                issued_at: self.clock.now(),
            }
            .encode()
        });
        Ok(MemoryPage {
            items: rows,
            next_cursor,
        })
    }

    /// The user page's Atlas workareas (ADR-0045 d30): the nodes the
    /// tree read serves `<user>-web` under `workarea` (by default
    /// `universe`, which a user key reads as its root and the `universe`
    /// node), each with the rows of [`Self::web_memories`] on it counted by
    /// state, so a region and a count exist only where the list would show
    /// the row; a held row outside an in-progress scope counts only for its
    /// author's person.
    pub async fn web_regions(
        &self,
        web: &Caller,
        workarea: Option<Reference>,
    ) -> Result<Regions, AppError> {
        let scope = workarea
            .clone()
            .unwrap_or_else(|| Reference::Name(Name::parse(UNIVERSE).expect("universe is a name")));
        let (mut regions, tree_cut) = self.tree_regions(web, scope).await?;
        // The page's own scope first; then each in-progress root on the
        // tree read as its own scope, where every author's held rows are
        // served (ADR-0030 d3), its subtree's counts replaced by that read.
        let mut partial = self
            .count_listing(web, workarea, &mut regions, &BTreeSet::new())
            .await?;
        partial |= tree_cut;
        let roots: Vec<Uuid> = regions
            .iter()
            .filter(|r| r.workarea.in_progress)
            .map(|r| r.workarea.workarea_id)
            .collect();
        for root in roots {
            let subtree: BTreeSet<Uuid> = self
                .journal
                .workarea_subtree(web.key.tenant_id, root)
                .await?
                .into_iter()
                .map(|w| w.workarea_id)
                .collect();
            for r in regions
                .iter_mut()
                .filter(|r| subtree.contains(&r.workarea.workarea_id))
            {
                (r.approved, r.held, r.in_progress, r.other) = (0, 0, 0, 0);
            }
            let outside: BTreeSet<Uuid> = regions
                .iter()
                .map(|r| r.workarea.workarea_id)
                .filter(|id| !subtree.contains(id))
                .collect();
            partial |= self
                .count_listing(web, Some(Reference::Id(root)), &mut regions, &outside)
                .await?;
        }
        let ancestors = self.ancestors_of(web.key.tenant_id, &regions).await?;
        Ok(Regions {
            regions,
            partial,
            ancestors,
        })
    }

    /// The ancestors of the first region, `universe` first.
    async fn ancestors_of(
        &self,
        tenant_id: Uuid,
        regions: &[Region],
    ) -> Result<Vec<(Uuid, Name)>, AppError> {
        let Some(top) = regions.first() else {
            return Ok(Vec::new());
        };
        let ancestry = self
            .journal
            .workarea_ancestry(tenant_id, top.workarea.workarea_id)
            .await?
            .unwrap_or_default();
        Ok(ancestry
            .iter()
            .skip(1)
            .rev()
            .map(|w| (w.workarea_id, w.name.clone()))
            .collect())
    }

    /// Counts the rows [`Self::web_memories`] lists under `workarea` onto
    /// `regions`, skipping rows on `skip`; whether it stopped at
    /// [`REGION_COUNT_MAX`].
    async fn count_listing(
        &self,
        web: &Caller,
        workarea: Option<Reference>,
        regions: &mut [Region],
        skip: &BTreeSet<Uuid>,
    ) -> Result<bool, AppError> {
        let mut cursor = None;
        let mut counted = 0;
        loop {
            let page = self
                .web_memories(
                    web,
                    &WebListRequest {
                        workarea: workarea.clone(),
                        cursor: cursor.take(),
                        limit: 200,
                        ..WebListRequest::default()
                    },
                )
                .await?;
            counted += page.items.len();
            for m in page
                .items
                .iter()
                .filter(|m| !skip.contains(&m.memory.workarea_id))
            {
                count_into(regions, &m.memory);
            }
            match page.next_cursor {
                Some(next) if counted < REGION_COUNT_MAX => cursor = Some(next),
                Some(_) => return Ok(true),
                None => return Ok(false),
            }
        }
    }

    /// The admin page's Atlas workareas (ADR-0045 d30): the tenant's tree
    /// under `workarea`, which an admin key reads as structure, with every
    /// row on each node counted by state, the admin read's scope and no
    /// wider. Counts carry no content, so they are read as the queue's
    /// depth is (d16) and journal nothing; the page of rows beside them is
    /// the one `memory.admin_read`.
    pub async fn admin_regions(
        &self,
        admin: &Caller,
        workarea: Option<Reference>,
    ) -> Result<Regions, AppError> {
        require_role(&admin.key, Role::Admin, "admin_read")?;
        let tenant_id = admin.key.tenant_id;
        let scope = workarea
            .unwrap_or_else(|| Reference::Name(Name::parse(UNIVERSE).expect("universe is a name")));
        let (mut regions, tree_cut) = self.tree_regions(admin, scope).await?;
        let nodes: Vec<Uuid> = regions.iter().map(|r| r.workarea.workarea_id).collect();
        let mut after = None;
        let mut counted = 0;
        let partial = loop {
            let rows = self
                .journal
                .web_admin_search(
                    tenant_id,
                    &AdminSearch {
                        nodes: Some(nodes.clone()),
                        after: after.take(),
                        limit: 201,
                        ..AdminSearch::default()
                    },
                )
                .await?;
            let more = rows.len() > 200;
            for m in rows.iter().take(200) {
                count_into(&mut regions, m);
            }
            counted += rows.len().min(200);
            match rows.get(199).filter(|_| more) {
                Some(last) if counted < REGION_COUNT_MAX => {
                    after = Some(ListingPosition {
                        created_at: last.created_at.clone(),
                        artifact_id: last.memory_id,
                    });
                }
                Some(_) => break true,
                None => break false,
            }
        };
        let ancestors = self.ancestors_of(tenant_id, &regions).await?;
        Ok(Regions {
            regions,
            partial: partial || tree_cut,
            ancestors,
        })
    }

    /// The nodes the tree read serves `caller` under `scope`, uncounted.
    async fn tree_regions(
        &self,
        caller: &Caller,
        scope: Reference,
    ) -> Result<(Vec<Region>, bool), AppError> {
        let mut regions = Vec::new();
        let mut truncated = false;
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .workarea_tree(
                    Bearer::Session(caller),
                    scope.clone(),
                    cursor.as_deref(),
                    200,
                )
                .await?;
            regions.extend(page.items.into_iter().map(|node| Region {
                workarea: node.workarea,
                path: node.path,
                approved: 0,
                held: 0,
                in_progress: 0,
                other: 0,
            }));
            match page.next_cursor {
                Some(next) if regions.len() < REGION_COUNT_MAX => cursor = Some(next),
                Some(_) => {
                    truncated = true;
                    break;
                }
                None => break,
            }
        }
        Ok((regions, truncated))
    }

    /// The acts a workarea's dropdown offers (ADR-0045 d28): a status
    /// transition as `transitions`, the mode and the commit as `mode`, each
    /// offered only where the verb's own check admits it now; a commit the
    /// held rows refuse is withheld with their count. A refusal hides an
    /// act; a store failure is answered.
    pub async fn workarea_offers(
        &self,
        transitions: &Caller,
        mode: &Caller,
        workarea_id: Uuid,
    ) -> Result<WorkareaOffers, AppError> {
        let tenant_id = transitions.key.tenant_id;
        let node = self
            .journal
            .workarea(tenant_id, workarea_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let admitted = |result: Result<(), AppError>| match result {
            Ok(()) => Ok(true),
            Err(AppError::Store(e)) => Err(AppError::Store(e)),
            Err(_) => Ok(false),
        };
        let mut offers = Vec::new();
        for to in [WorkareaStatus::Committed, WorkareaStatus::Validated] {
            let check = self
                .propose_admitted(transitions, workarea_id, to)
                .await
                .map(|_| ());
            if admitted(check)? {
                offers.push(WorkareaOffer::Propose(to));
            }
        }
        for t in self
            .journal
            .pending_transitions(tenant_id, workarea_id)
            .await?
            .into_iter()
            .filter(|t| t.from == node.status)
        {
            let check = self
                .ratify_admitted(transitions, workarea_id, t.transition_id)
                .await
                .map(|_| ());
            if admitted(check)? {
                offers.push(WorkareaOffer::Ratify {
                    transition_id: t.transition_id,
                    from: t.from,
                    to: t.to,
                });
            }
        }
        let mut commit_withheld = None;
        let wanted = if node.in_progress {
            WorkareaMode::Standard
        } else {
            WorkareaMode::InProgress
        };
        match self.mode_admitted(mode, workarea_id, wanted).await {
            Ok(super::ModeAdmitted::Change { .. }) => offers.push(if node.in_progress {
                WorkareaOffer::Commit
            } else {
                WorkareaOffer::InProgress
            }),
            Ok(super::ModeAdmitted::Already(_)) => {}
            Err(AppError::HeldRemaining { .. }) => {
                let nodes: Vec<Uuid> = self
                    .journal
                    .workarea_subtree(tenant_id, workarea_id)
                    .await?
                    .iter()
                    .map(|w| w.workarea_id)
                    .collect();
                commit_withheld = Some(self.journal.held_count(tenant_id, &nodes).await?);
            }
            Err(e) => {
                admitted(Err(e))?;
            }
        }
        Ok(WorkareaOffers {
            workarea_id,
            status: node.status,
            in_progress: node.in_progress,
            offers,
            commit_withheld,
        })
    }

    /// Whether one act of the dropdown is admitted now, by the same check
    /// as [`Self::workarea_offers`]: `Ok` exactly when it is offered, else
    /// the refusal its verb would answer (ADR-0045 d28). Setting the mode a
    /// node already holds is no act, refused naming `op`.
    pub async fn workarea_offer_check(
        &self,
        transitions: &Caller,
        mode: &Caller,
        workarea_id: Uuid,
        offer: WorkareaOffer,
    ) -> Result<(), AppError> {
        match offer {
            WorkareaOffer::Propose(to) => self
                .propose_admitted(transitions, workarea_id, to)
                .await
                .map(|_| ()),
            WorkareaOffer::Ratify { transition_id, .. } => self
                .ratify_admitted(transitions, workarea_id, transition_id)
                .await
                .map(|_| ()),
            WorkareaOffer::InProgress | WorkareaOffer::Commit => {
                let wanted = if offer == WorkareaOffer::Commit {
                    WorkareaMode::Standard
                } else {
                    WorkareaMode::InProgress
                };
                match self.mode_admitted(mode, workarea_id, wanted).await? {
                    super::ModeAdmitted::Change { .. } => Ok(()),
                    super::ModeAdmitted::Already(_) => Err(field(
                        "op",
                        "the workarea is already in that mode, so there is nothing to do",
                    )),
                }
            }
        }
    }

    /// [`Self::workarea_offers`] for one key acting in both capacities, by
    /// its value or a session: what a dropdown would offer that key.
    pub async fn workarea_offers_for(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workarea: Reference,
    ) -> Result<WorkareaOffers, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let workarea_id = self
            .resolve(&caller.key, NamedKind::Workarea, &workarea)
            .await?;
        self.workarea_offers(&caller, &caller, workarea_id).await
    }

    // -- the admin read -----------------------------------------------------

    /// `POST /v2/admin/memories/search` (ADR-0045 d21): admin key role
    /// alone; every memory of the tenant the search names, any state, with
    /// its text and author, newest first. Journalled as one
    /// `memory.admin_read` naming the ids served and a digest of the query.
    pub async fn admin_search(
        &self,
        bearer: impl Into<Bearer<'_>>,
        query: Option<&str>,
        workarea: Option<Reference>,
        state: Option<AdminState>,
        after: Option<(EventTime, Uuid)>,
        limit: usize,
    ) -> Result<AdminPage, AppError> {
        self.admin_search_filtered(
            bearer,
            &AdminSearchRequest {
                query,
                workarea,
                state,
                origin: None,
                author: None,
                after,
                limit,
            },
        )
        .await
    }

    /// The admin read's search with the admin page's every filter
    /// (ADR-0045 d22, d24): [`Self::admin_search`] narrowed besides to one
    /// `origin.kind` and one authoring key, by name or id, of the tenant. One
    /// `memory.admin_read` per page served, as there.
    pub async fn admin_search_filtered(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &AdminSearchRequest<'_>,
    ) -> Result<AdminPage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller.key, Role::Admin, "admin_read")?;
        let tenant_id = caller.key.tenant_id;
        let AdminSearchRequest {
            query,
            workarea,
            state,
            origin,
            author,
            after,
            limit,
        } = req.clone();
        let author = match &author {
            None => None,
            Some(reference) => Some(self.resolve(&caller.key, NamedKind::Key, reference).await?),
        };
        let nodes = match &workarea {
            None => None,
            Some(reference) => {
                let node = self
                    .resolve(&caller.key, NamedKind::Workarea, reference)
                    .await?;
                let subtree = self.journal.workarea_subtree(tenant_id, node).await?;
                if subtree.is_empty() {
                    return Err(AppError::NotFound);
                }
                Some(subtree.into_iter().map(|w| w.workarea_id).collect())
            }
        };
        let search = AdminSearch {
            query: query
                .map(str::trim)
                .filter(|q| !q.is_empty())
                .map(ToOwned::to_owned),
            nodes,
            state,
            origin_kinds: origin.into_iter().collect(),
            author,
            after: after.map(|(created_at, artifact_id)| crate::types::ListingPosition {
                created_at,
                artifact_id,
            }),
            limit: limit.saturating_add(1),
        };
        let mut rows = self.journal.web_admin_search(tenant_id, &search).await?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        let next = rows
            .last()
            .filter(|_| more)
            .map(|m| (m.created_at.clone(), m.memory_id));
        let mut items = Vec::with_capacity(rows.len());
        for memory in rows {
            items.push(self.admin_memory(tenant_id, memory, false).await?);
        }
        self.journal_admin_read(
            &caller.key,
            items.iter().map(|m| m.memory.memory_id).collect(),
            search.query.as_deref(),
        )
        .await?;
        Ok(AdminPage { items, next })
    }

    /// `GET /v2/admin/memories/{artifact_id}` (ADR-0045 d21): one memory
    /// with its text and full attribution, a universe claim with its
    /// source; journalled as `memory.admin_read` with its one id.
    pub async fn admin_read(
        &self,
        bearer: impl Into<Bearer<'_>>,
        memory_id: Uuid,
    ) -> Result<AdminMemory, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller.key, Role::Admin, "admin_read")?;
        let tenant_id = caller.key.tenant_id;
        let memory = self
            .journal
            .memory(tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let read = self.admin_memory(tenant_id, memory, true).await?;
        self.journal_admin_read(&caller.key, vec![memory_id], None)
            .await?;
        Ok(read)
    }

    async fn admin_memory(
        &self,
        tenant_id: Uuid,
        memory: MemoryRecord,
        with_verdicts: bool,
    ) -> Result<AdminMemory, AppError> {
        let workarea_path = self.path_of(tenant_id, memory.workarea_id).await?;
        let author = self.journal.key(tenant_id, memory.author_key_id).await?;
        let author_user = match author.as_ref().and_then(|k| k.user) {
            Some(root) => self
                .journal
                .workarea(tenant_id, root)
                .await?
                .map(|w| w.name),
            None => None,
        };
        let verdicts = if with_verdicts {
            self.verdicts_on(tenant_id, memory.memory_id).await?
        } else {
            Vec::new()
        };
        Ok(AdminMemory {
            state: AdminState::of(&memory),
            memory,
            workarea_path,
            author,
            author_user,
            verdicts,
        })
    }

    /// The journal's verdicts on one memory, in chain order.
    async fn verdicts_on(
        &self,
        tenant_id: Uuid,
        memory_id: Uuid,
    ) -> Result<Vec<VerdictTrail>, AppError> {
        let id = memory_id.to_string();
        let mut out = Vec::new();
        let mut from = 1;
        loop {
            let page = self.journal.events_range(tenant_id, from, 500).await?;
            let Some(last) = page.last() else { break };
            from = last.envelope.seq + 1;
            for event in &page {
                let e = &event.envelope;
                let verdict = match e.event_type.as_str() {
                    event_types::REVIEW_APPROVE => "approve",
                    event_types::REVIEW_REJECT => "reject",
                    _ => continue,
                };
                if e.payload["artifact_id"].as_str() != Some(id.as_str()) {
                    continue;
                }
                let key = match e.api_key_id {
                    Some(k) => self.journal.key(tenant_id, k).await?,
                    None => None,
                };
                out.push(VerdictTrail {
                    verdict: verdict.to_owned(),
                    key_id: e.api_key_id,
                    key_name: key.and_then(|k| k.name),
                    principal: e.principal.clone(),
                    reason: e.payload["reason"].as_str().map(ToOwned::to_owned),
                    at: e.at.clone(),
                });
            }
            if page.len() < 500 {
                break;
            }
        }
        Ok(out)
    }

    async fn journal_admin_read(
        &self,
        admin: &KeyRecord,
        artifact_ids: Vec<Uuid>,
        query: Option<&str>,
    ) -> Result<(), AppError> {
        let ids: BTreeSet<String> = artifact_ids.iter().map(Uuid::to_string).collect();
        let tenant_id = admin.tenant_id;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::MEMORY_ADMIN_READ,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "artifact_ids": ids.into_iter().collect::<Vec<_>>(),
                "query_digest": query.map(|q| Sha256Hash::digest(q.as_bytes()).to_prefixed()),
            }),
        )?;
        self.commit(event, MetadataFold::AdminRead, None).await?;
        Ok(())
    }
}
