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
use crate::types::{
    AdminSearch, AdminState, AppError, Bearer, ConnectCodeIssued, Credential, DenialRecord,
    GrantRequest, IssueKeyRequest, IssuedKey, KeyAccess, KeyRecord, MemoryRecord, QueueDepth,
    WebEntry, WebSessionRecord,
};

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
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller.key, Role::Admin, "admin_read")?;
        let tenant_id = caller.key.tenant_id;
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
