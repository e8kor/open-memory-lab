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

use oml_domain::access::{Covers, check_issuance, effective_roles};
use oml_domain::name::{Name, NamedKind, Reference, UNIVERSE};
use oml_domain::web::SESSION_KEY_SUFFIXES;
use oml_domain::{EventTime, Role, Sha256Hash, SignedEnvelope, UsedBy, event_types};

use super::{App, Caller, Head, Issuance, issuance_refusal, require_role};
use crate::ports::{Clock, ConnectionStore, IdGen, MetadataFold, Scanner, Signer, WebStore};
use oml_domain::in_progress::WorkspaceMode;
use oml_domain::origin::OriginKind;
use oml_domain::{MemoryKind, MemoryStatus, WorkspaceStatus};

use crate::cursor::ListingCursor;
use crate::types::{
    AdminSearch, AdminState, AppError, Bearer, ConnectCodeIssued, Credential, DenialRecord,
    GrantRecord, GrantRequest, IssueKeyRequest, IssuedKey, KeyAccess, KeyRecord, MemoryRecord,
    QueueDepth, WorkspaceRecord,
};
use crate::types::{
    AnchorFilter, ListRequest, ListingPosition, MemoryPage, ServedMemory, StateClause,
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
    /// The capacity a suffix names; `None` for any other word.
    #[must_use]
    pub fn parse(suffix: &str) -> Option<Self> {
        [
            Capacity::Web,
            Capacity::Review,
            Capacity::Curate,
            Capacity::Admin,
        ]
        .into_iter()
        .find(|c| c.suffix() == suffix)
    }

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

/// What a capacity designation did (ADR-0045 d10; ADR-0070 d5 #3): the
/// session key that holds the capacity and the grants it gained, each
/// with its epoch, the seq of its `grant.issued`; none when it held them
/// all already.
#[derive(Debug, Clone)]
pub struct CapacityGranted {
    pub key: KeyRecord,
    pub grants_added: Vec<GrantRecord>,
}

/// One of the person's keys as the user page lists it (ADR-0045 d9).
#[derive(Debug, Clone)]
pub struct OwnKey {
    pub key: KeyRecord,
    pub access: Option<KeyAccess>,
    /// A global denial on `universe` cuts it off.
    pub cut_off: bool,
    /// A contributor without recall, which the console marks "no recall"
    /// (ADR-0093 d3, as read on 2026-11-20).
    pub without_recall: bool,
}

/// What a person's web session sends beside a key's name, role and kind
/// (ADR-0096 d1, d2): the grants, taken as sent where any are; the default
/// workspace; the guardrail scope, `ancestors` when omitted; the expiry and
/// the key's own rate.
#[derive(Debug, Clone, Default)]
pub struct OwnKeyShape {
    pub grants: Vec<GrantRequest>,
    pub default_workspace: Option<Reference>,
    pub guardrail_scope: Option<oml_domain::guardrail::GuardrailScope>,
    pub expiry: Option<EventTime>,
    pub requests_per_minute: Option<u32>,
    /// The workspaces the issuance creates inside the person's root, each
    /// granted to the key by its role's default grants (ADR-0097 d30).
    pub create_workspaces: Vec<crate::types::NewWorkspace>,
}

/// One workspace's queue with its path (ADR-0045 d10).
#[derive(Debug, Clone)]
pub struct QueueRow {
    pub depth: QueueDepth,
    pub path: String,
    /// The live person's reviewer keys whose effective access confers
    /// reviewer on the workspace, so may approve its held instructions
    /// (ADR-0080 d16; #435). A count, never a key. It overcounts: a key
    /// whose person also wrote the held rows is counted, though outside that
    /// person's own root it cannot approve them (`review.author_in_lineage`,
    /// same_person; ADR-0082 d7).
    pub person_reviewers: u64,
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
    pub workspace_path: String,
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
    pub workspace: Option<Reference>,
    pub state: Option<AdminState>,
    pub origin: Option<OriginKind>,
    /// The authoring key, by name or id.
    pub author: Option<Reference>,
    /// Only rows of these kinds; empty narrows nothing (ADR-0083 d5;
    /// ADR-0082 d8 item 3).
    pub kinds: Vec<MemoryKind>,
    pub after: Option<(EventTime, Uuid)>,
    pub limit: usize,
    /// The open investigation of the admin key the read is made under
    /// (ADR-0089 d10).
    pub investigation: Option<Uuid>,
}

/// The user page's Memories list as the person filters it (ADR-0045 d22,
/// d23); the states are the admin read's words.
#[derive(Debug, Clone, Default)]
pub struct WebListRequest {
    pub workspace: Option<Reference>,
    /// Rows in any of these states; empty narrows nothing (ADR-0083 d3).
    pub states: Vec<AdminState>,
    /// Rows of any of these kinds; empty narrows nothing (ADR-0082 d8
    /// item 3).
    pub kinds: Vec<MemoryKind>,
    /// `mine=true`: the caller's own key set alone, with no scoped part
    /// (ADR-0083 d3, d14).
    pub mine: bool,
    /// Only the `promoted` rows approved at or after this instant; the
    /// other states stay whole (ADR-0083 d3).
    pub promoted_since: Option<EventTime>,
    pub origin: Option<OriginKind>,
    /// One of the person's own keys, by name.
    pub author: Option<Name>,
    pub cursor: Option<String>,
    pub limit: usize,
}

/// How many rows the Atlas counts per view before it says "at least"
/// (ADR-0045 d30): ten pages of the largest list page, chosen, not measured.
pub const REGION_COUNT_MAX: usize = 2_000;

/// What claiming a web console act's `Idempotency-Key` found
/// ([`App::claim_act`]).
#[derive(Debug)]
pub enum ActClaim {
    /// No outcome is recorded and the claim is this attempt's: act, then
    /// [`App::settle_act`] or, on a refusal, [`App::release_act`].
    Act(ActTicket),
    /// The outcome recorded under the key, `{status, body}` as the act
    /// recorded it.
    Replay(serde_json::Value),
    /// Another attempt holds the claim and has recorded nothing yet: it is
    /// still acting, or it stopped between its act and its record.
    InProgress,
}

/// The claim an attempt holds on its act's `Idempotency-Key` until it
/// settles or releases it.
#[derive(Debug)]
pub struct ActTicket {
    scope: crate::ports::IdempotencyScope,
    request_digest: String,
    claim_id: Uuid,
}

impl ActTicket {
    /// The claim's id, which the lead's log names for an operator.
    #[must_use]
    pub fn claim_id(&self) -> Uuid {
        self.claim_id
    }

    /// The acting key whose claim this is.
    #[must_use]
    pub fn key_id(&self) -> Uuid {
        self.scope.key_id
    }
}

/// One workspace of the Atlas (ADR-0045 d30): the node, its path, and its
/// rows the viewer may read by state: approved (promoted tips), held,
/// unreviewed inside an in-progress scope, and every other state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub workspace: WorkspaceRecord,
    pub path: String,
    pub approved: u64,
    pub held: u64,
    pub in_progress: u64,
    pub other: u64,
}

/// The Atlas's workspaces and whether the count stopped at
/// [`REGION_COUNT_MAX`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regions {
    pub regions: Vec<Region>,
    pub partial: bool,
    /// The first region's ancestors, `universe` first, each its id and
    /// name: the breadcrumb out of a zoomed Atlas.
    pub ancestors: Vec<(Uuid, Name)>,
}

/// One act a workspace's dropdown may offer (ADR-0045 d28).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceOffer {
    /// Propose the next rung (ADR-0009 d3).
    Propose(WorkspaceStatus),
    /// Ratify a live proposal another key made.
    Ratify {
        transition_id: Uuid,
        from: WorkspaceStatus,
        to: WorkspaceStatus,
    },
    /// Put the workspace in progress (ADR-0030 d1).
    InProgress,
    /// Commit an in-progress root (ADR-0030 d6).
    Commit,
}

/// What a workspace's dropdown shows: the node's status and mode, the acts
/// its verbs admit now, and a commit withheld with the held rows that stop
/// it (ADR-0045 d28).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceOffers {
    pub workspace_id: Uuid,
    pub status: WorkspaceStatus,
    pub in_progress: bool,
    pub offers: Vec<WorkspaceOffer>,
    /// `Some(held)` when the node is an in-progress root whose commit the
    /// held rows refuse.
    pub commit_withheld: Option<u64>,
}

/// Counts `m` on its region by state; a held row is in progress when its
/// node or an ancestor on the page's tree is in progress (ADR-0030 d1). A
/// row on a node the tree did not serve is not counted.
fn count_into(regions: &mut [Region], m: &MemoryRecord) {
    let inside = {
        let mut node = Some(m.workspace_id);
        let mut found = false;
        while let Some(id) = node {
            match regions.iter().find(|r| r.workspace.workspace_id == id) {
                Some(r) if r.workspace.in_progress => {
                    found = true;
                    break;
                }
                Some(r) => node = r.workspace.parent_id,
                None => break,
            }
        }
        found
    };
    let Some(region) = regions
        .iter_mut()
        .find(|r| r.workspace.workspace_id == m.workspace_id)
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

pub(super) fn key_name(user: &Name, suffix: &str) -> Result<Name, AppError> {
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
    // -- the web console's acts, replayed by their Idempotency-Key ----------

    /// The scope a web console act records its outcome under: the acting
    /// key, no workspace, and the act's name beside the client's key, so
    /// one key's value names one act (ADR-0070 d3 layer 4, d5).
    fn act_scope(caller: &Caller, act: &str, idem_key: &str) -> crate::ports::IdempotencyScope {
        crate::ports::IdempotencyScope {
            tenant_id: caller.key.tenant_id,
            key_id: caller.key.key_id,
            workspace_id: Uuid::nil(),
            idem_key: format!("{act}:{idem_key}"),
        }
    }

    /// Claims this act's `Idempotency-Key` for the caller before the act
    /// runs (http-api.md idempotency; ADR-0070 d3 layer 4, d5; #233):
    /// [`ActClaim::Act`] to act, holding the claim, [`ActClaim::Replay`]
    /// with the outcome recorded under the key, or [`ActClaim::InProgress`]
    /// while another attempt holds the claim, which the caller waits on and
    /// claims again. A key held by another request is
    /// [`AppError::IdempotencyDigestMismatch`]. `claim_id` names this
    /// attempt's claim; the caller draws a fresh one per attempt.
    pub async fn claim_act(
        &self,
        caller: &Caller,
        act: &str,
        idem_key: &str,
        request_digest: &str,
        claim_id: Uuid,
    ) -> Result<ActClaim, AppError> {
        let scope = Self::act_scope(caller, act, idem_key);
        match self
            .journal
            .idempotency_claim(&scope, request_digest, claim_id)
            .await?
        {
            crate::ports::IdempotencyClaim::Claimed => Ok(ActClaim::Act(ActTicket {
                scope,
                request_digest: request_digest.to_owned(),
                claim_id,
            })),
            crate::ports::IdempotencyClaim::Recorded(record) => {
                if record.request_digest != request_digest {
                    return Err(AppError::IdempotencyDigestMismatch);
                }
                Ok(ActClaim::Replay(record.response))
            }
            crate::ports::IdempotencyClaim::InProgress {
                request_digest: held,
            } => {
                if held != request_digest {
                    return Err(AppError::IdempotencyDigestMismatch);
                }
                Ok(ActClaim::InProgress)
            }
        }
    }

    /// Records the act's outcome into its claim, so a resubmission under
    /// the same key answers it instead of acting again. `false` when the
    /// claim was no longer held, which records nothing.
    pub async fn settle_act(
        &self,
        ticket: ActTicket,
        response: serde_json::Value,
    ) -> Result<bool, AppError> {
        Ok(self
            .journal
            .idempotency_settle(
                crate::ports::IdempotencyPut {
                    scope: ticket.scope,
                    record: crate::ports::IdempotencyRecord {
                        request_digest: ticket.request_digest,
                        response,
                    },
                },
                ticket.claim_id,
            )
            .await?)
    }

    /// Drops the claim of an act that was refused before anything
    /// committed, so a resubmission acts again: a refused request records
    /// nothing (http-api.md idempotency). Never called once the act may
    /// have committed (#233).
    pub async fn release_act(&self, ticket: ActTicket) -> Result<(), AppError> {
        Ok(self
            .journal
            .idempotency_release(&ticket.scope, ticket.claim_id)
            .await?)
    }

    // -- the user page's keys ---------------------------------------------

    /// The session key the gate resolved, its access read now, when the act
    /// runs: the gate read it before the request's body arrived (ADR-0070
    /// d8). A key that carries a denial runs none of its key acts, since
    /// each hands on access the key no longer holds whole: a key it issues
    /// is granted the person's whole root, a connect code opens a
    /// connection, and a cut-off is journalled under it
    /// ([`AppError::SessionKeyDenied`]; ADR-0070 d2; #325). With the
    /// caller comes the chain head, read before the access, for the act to
    /// seal on ([`Head::BeforeAccess`]): no denial of the key commits
    /// between the access the act decided on and the act's own commit.
    async fn undenied(
        &self,
        gate: &Caller,
        verb: &'static str,
    ) -> Result<(Caller, Option<SignedEnvelope>), AppError> {
        let head = self.head_event(gate.key.tenant_id).await?;
        let now = self.authenticate_bearer(Bearer::Session(gate)).await?;
        now.require_current_value(verb)?;
        if now.denials.is_empty() {
            Ok((now, head))
        } else {
            Err(AppError::SessionKeyDenied {
                role: now.key.role,
                verb,
            })
        }
    }

    /// `GET /v2/keys` for a session user key (ADR-0070 d5 #1; ADR-0045 d9):
    /// every key naming the person's user, with its grants and whether a denial
    /// on `universe` cuts it off.
    pub async fn list_own_keys(&self, web: &Caller) -> Result<Vec<OwnKey>, AppError> {
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
            let without_recall = self.without_recall(&key, access.as_ref()).await?;
            out.push(OwnKey {
                key,
                access,
                cut_off,
                without_recall,
            });
        }
        Ok(out)
    }

    /// `POST /v2/keys` as `<user>-web` (ADR-0070 d5; ADR-0045 d9, d16;
    /// ADR-0096 d1 to d3), the console's key builder: a contributor, reader
    /// or submitter key naming the person's user, named `<user>-<name>` with
    /// exactly the four session suffixes reserved, issued by `<user>-web`;
    /// with `connector`, a connector key and its connect code.
    ///
    /// Without `shape.grants` the key takes today's shape on the whole
    /// root: a contributor submitter and reader, a reader reader, a
    /// submitter submitter, each covering the subtree. With them the grants
    /// are taken exactly as sent (ADR-0025 d2), each inside the person's
    /// root (ADR-0044 d3), and the issuance adds only `universe`'s reader
    /// node grant, which a submitter key never gets (ADR-0093 d2).
    ///
    /// This role check is the one guard that keeps a reviewer key out of
    /// the console: the issuance runs under `Issuance::PersonsOwn`, whose
    /// ceiling is the admin role (ADR-0096 d1; R1). Issuance never
    /// amplifies authority, so a `<user>-web` that carries a denial issues
    /// none (#325). The person may end the key on a date and set its rate
    /// here; both change afterwards only by an admin key (ADR-0079 d1, d5;
    /// ADR-0096 d2).
    pub async fn issue_own_key(
        &self,
        web: &Caller,
        suffix: &str,
        role: Role,
        connector: bool,
        shape: &OwnKeyShape,
    ) -> Result<IssuedKey, AppError> {
        let (web, head) = &self.undenied(web, "issue_key").await?;
        let user_name = self.web_user_of(web).await?;
        // A person of the sign-up tenant holds at most their keys quota
        // (ADR-0068 d5).
        self.require_quota(&web.key, oml_domain::signup::Quota::Keys)
            .await?;
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
            create: false,
        };
        let whole_root = match role {
            Role::Contributor => vec![grant(Role::Submitter), grant(Role::Reader)],
            Role::Reader => vec![grant(Role::Reader)],
            Role::Submitter => vec![grant(Role::Submitter)],
            _ => {
                return Err(field(
                    "role",
                    "an issued key is a contributor, a reader or a submitter",
                ));
            }
        };
        // The whole root only for a key sent with neither grants nor new
        // workspaces (ADR-0096 d1; ADR-0097 d30).
        let grants = if shape.grants.is_empty() && shape.create_workspaces.is_empty() {
            whole_root
        } else {
            shape.grants.clone()
        };
        let req = IssueKeyRequest {
            profile: None,
            guardrail_scope: shape.guardrail_scope,
            role,
            principal: name.as_str().to_owned(),
            name,
            used_by: Some(UsedBy::Agent),
            default_workspace: shape.default_workspace.clone(),
            crew: None,
            grants,
            connect_code: connector,
            user: Some(user_name),
            expiry: shape.expiry.clone(),
            requests_per_minute: shape.requests_per_minute,
            person: None,
            create_workspaces: shape.create_workspaces.clone(),
        };
        self.issue_key_by(&web.key, &req, Issuance::PersonsOwn(head))
            .await
    }

    /// `POST /v2/keys/{key}/connect-codes` as `<user>-web` (ADR-0070 d5;
    /// ADR-0045 d9): a fresh code for one of the person's connector keys,
    /// minted by `<user>-web`, and by none that carries a denial (#325).
    pub async fn mint_own_connect_code(
        &self,
        web: &Caller,
        key: &Name,
    ) -> Result<ConnectCodeIssued, AppError> {
        let (web, head) = &self.undenied(web, "mint_connect_code").await?;
        let target = self.own_key(web, key).await?;
        self.mint_connect_code_by(
            &web.key,
            Reference::Id(target.key_id),
            Head::BeforeAccess(head),
        )
        .await
    }

    /// `POST /v2/keys/{key}/denials` as `<user>-web` or `<user>-admin`
    /// (ADR-0070 d5; ADR-0045 d9, d13): one of the person's keys denied on
    /// `universe`, journalled as `key.denied`, by `<user>-web` on the user page
    /// or `<user>-admin` on the admin page; the only cut-off while revocation
    /// is not built. A session key is never cut off here, and one that
    /// carries a denial cuts nothing off (#325).
    ///
    /// `<user>-admin` may name any workspace of the tenant instead of
    /// `universe` (ADR-0096 d4, decided by the operator, 2026-11-24);
    /// `<user>-web` keeps `universe` alone, which the transport checks.
    pub async fn cut_off_key(&self, acting: &Caller, key: &Name) -> Result<DenialRecord, AppError> {
        let universe = Reference::Name(Name::parse(UNIVERSE).expect("universe is a name"));
        self.deny_key_on(acting, key, universe, None).await
    }

    /// [`App::cut_off_key`] on `node`: a session key's denial of one key
    /// on one workspace, never of a session key or an admin key.
    pub async fn deny_key_on(
        &self,
        acting: &Caller,
        key: &Name,
        node: Reference,
        reason: Option<&str>,
    ) -> Result<DenialRecord, AppError> {
        let (acting, head) = &self.undenied(acting, "deny_key").await?;
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
        self.deny_key_by(
            &acting.key,
            Reference::Id(target.key_id),
            node,
            Head::BeforeAccess(head),
            reason,
        )
        .await
    }

    /// The user page's user: the name of the root `<user>-web` names.
    async fn web_user_of(&self, web: &Caller) -> Result<Name, AppError> {
        let root = web
            .key
            .user
            .ok_or(AppError::WebKeyMissing { capacity: "web" })?;
        self.journal
            .workspace(web.key.tenant_id, root)
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
        if let Some(ancestry) = self.journal.workspace_ancestry(tenant_id, node).await? {
            return Ok(ancestry.last().map(|w| w.workspace_id));
        }
        Ok(self
            .journal
            .resolve_name(
                tenant_id,
                NamedKind::Workspace,
                &Name::parse(UNIVERSE).expect("universe is a name"),
            )
            .await?)
    }

    // -- the admin page's capacities ----------------------------------------

    /// `GET /v2/keys?user=` for the admin role (ADR-0070 d5 #1; ADR-0045 d13),
    /// the removal confirmation's list: the keys naming that user, session keys
    /// aside.
    pub async fn list_keys_of_user(
        &self,
        admin: &Caller,
        user: &Name,
    ) -> Result<Vec<OwnKey>, AppError> {
        require_role(admin, Role::Admin, "web_keys")?;
        let tenant_id = admin.key.tenant_id;
        let Some(root) = self
            .journal
            .resolve_name(tenant_id, NamedKind::Workspace, user)
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
            let without_recall = self.without_recall(&key, access.as_ref()).await?;
            out.push(OwnKey {
                key,
                access,
                cut_off,
                without_recall,
            });
        }
        Ok(out)
    }

    /// `POST /v2/web/users/{user}/capacities` with `capacity: review` (ADR-0070
    /// d5 #3; ADR-0045 d10): the user's `<user>-review` gains reviewer and
    /// reader on `workspace`, journalled as `grant.issued` by `<user>-admin`.
    /// The key exists from the user's first sign-in.
    pub async fn grant_review_capacity(
        &self,
        admin: &Caller,
        user: &Name,
        workspace: Reference,
    ) -> Result<CapacityGranted, AppError> {
        self.grant_review_capacity_with_reason(admin, user, workspace, None)
            .await
    }

    /// [`Self::grant_review_capacity`] with the admin's reason, which a grant that waits on
    /// a second admin person requires (ADR-0097 d5, d26).
    pub async fn grant_review_capacity_with_reason(
        &self,
        admin: &Caller,
        user: &Name,
        workspace: Reference,
        reason: Option<&str>,
    ) -> Result<CapacityGranted, AppError> {
        require_role(admin, Role::Admin, "grant_review")?;
        let review = self
            .session_key_of(admin.key.tenant_id, user, Capacity::Review)
            .await?
            .ok_or(AppError::WebKeyMissing { capacity: "review" })?;
        let grants = [Role::Reviewer, Role::Reader].map(|role| GrantRequest {
            node: workspace.clone(),
            role,
            deny: Vec::new(),
            covers: Covers::Subtree,
            create: false,
        });
        let added = self
            .add_grants(&admin.key, &review, &grants, reason)
            .await?;
        Ok(CapacityGranted {
            key: review,
            grants_added: added,
        })
    }

    /// `POST /v2/web/users/{user}/capacities` with `capacity: read` (ADR-0096,
    /// the operator's decision of 2026-11-25): the user's `<user>-web` gains
    /// reader, covering the subtree, on `workspace`, journalled as
    /// `grant.issued` by `<user>-admin`, so the person's Memories and
    /// Catalog list what is approved there. No verb beyond reading.
    pub async fn grant_read_capacity(
        &self,
        admin: &Caller,
        user: &Name,
        workspace: Reference,
    ) -> Result<CapacityGranted, AppError> {
        self.grant_read_capacity_with_reason(admin, user, workspace, None)
            .await
    }

    /// [`Self::grant_read_capacity`] with the admin's reason, which a grant that waits on
    /// a second admin person requires (ADR-0097 d5, d26).
    pub async fn grant_read_capacity_with_reason(
        &self,
        admin: &Caller,
        user: &Name,
        workspace: Reference,
        reason: Option<&str>,
    ) -> Result<CapacityGranted, AppError> {
        require_role(admin, Role::Admin, "grant_read")?;
        let web = self
            .session_key_of(admin.key.tenant_id, user, Capacity::Web)
            .await?
            .ok_or(AppError::WebKeyMissing { capacity: "web" })?;
        let reader = GrantRequest {
            node: workspace,
            role: Role::Reader,
            deny: Vec::new(),
            covers: Covers::Subtree,
            create: false,
        };
        let added = self.add_grants(&admin.key, &web, &[reader], reason).await?;
        Ok(CapacityGranted {
            key: web,
            grants_added: added,
        })
    }

    /// `POST /v2/web/users/{user}/capacities` with `capacity: curate` (ADR-0070
    /// d5 #3; ADR-0045 d10, ADR-0044 d7): the user's `<user>-curate`, a tenant
    /// contributor key with submitter on the `universe` node alone and reader on
    /// `workspace`, created by `<user>-admin` at the first designation and
    /// extended by a reader grant after it.
    pub async fn grant_curation_capacity(
        &self,
        admin: &Caller,
        user: &Name,
        workspace: Reference,
    ) -> Result<CapacityGranted, AppError> {
        self.grant_curation_capacity_with_reason(admin, user, workspace, None)
            .await
    }

    /// [`Self::grant_curation_capacity`] with the admin's reason, which a grant that waits on
    /// a second admin person requires (ADR-0097 d5, d26).
    pub async fn grant_curation_capacity_with_reason(
        &self,
        admin: &Caller,
        user: &Name,
        workspace: Reference,
        reason: Option<&str>,
    ) -> Result<CapacityGranted, AppError> {
        require_role(admin, Role::Admin, "grant_curation")?;
        let tenant_id = admin.key.tenant_id;
        let reader = GrantRequest {
            node: workspace,
            role: Role::Reader,
            deny: Vec::new(),
            covers: Covers::Subtree,
            create: false,
        };
        if let Some(curate) = self
            .session_key_of(tenant_id, user, Capacity::Curate)
            .await?
        {
            let added = self
                .add_grants(&admin.key, &curate, &[reader], reason)
                .await?;
            return Ok(CapacityGranted {
                key: curate,
                grants_added: added,
            });
        }
        let name = key_name(user, Capacity::Curate.suffix())?;
        let req = IssueKeyRequest {
            profile: None,
            // The issuance default; a page offers no scope (ADR-0037
            // d17, d18 Q5).
            guardrail_scope: None,
            role: Role::Contributor,
            principal: name.as_str().to_owned(),
            name,
            used_by: Some(UsedBy::Person),
            default_workspace: None,
            crew: None,
            grants: vec![
                GrantRequest {
                    node: Reference::Name(Name::parse(UNIVERSE).expect("universe is a name")),
                    role: Role::Submitter,
                    deny: Vec::new(),
                    covers: Covers::Node,
                    create: false,
                },
                reader,
            ],
            connect_code: false,
            user: None,
            expiry: None,
            requests_per_minute: None,
            person: None,
            create_workspaces: Vec::new(),
        };
        let issued = self
            .issue_key_by(&admin.key, &req, Issuance::Session(user))
            .await?;
        let key = self
            .session_key_of(tenant_id, user, Capacity::Curate)
            .await?
            .ok_or(AppError::WebKeyMissing { capacity: "curate" })?;
        Ok(CapacityGranted {
            key,
            grants_added: issued.access.grants,
        })
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
        reason: Option<&str>,
    ) -> Result<Vec<GrantRecord>, AppError> {
        let tenant_id = admin.tenant_id;
        let held = self
            .journal
            .key_access(tenant_id, key.key_id)
            .await?
            .map(|a| a.grants)
            .unwrap_or_default();
        let mut resolved = Vec::new();
        for g in grants {
            let node = self.resolve(admin, NamedKind::Workspace, &g.node).await?;
            if self.journal.workspace(tenant_id, node).await?.is_none() {
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
                // A capacity grant carries no create flag (ADR-0097 d31).
                create: false,
            });
        }
        // Step 3 applies to added grants; a contributor's shape was checked
        // when it was issued, and adding a grant cannot undo it (ADR-0093 d2).
        check_issuance(Role::Admin, key.role, &resolved).map_err(issuance_refusal)?;
        if resolved.is_empty() {
            return Ok(Vec::new());
        }
        // A pending key takes no added grant (ADR-0097 d23).
        if key.pending_act.is_some() {
            return Err(AppError::AdminAct(
                crate::types::AdminActRefusal::KeyPending,
            ));
        }
        // Adding a grant or a capacity is widening, a person's root
        // included (ADR-0097 d5, d6): pending, the grants wait in the act.
        let mut root_persons = BTreeSet::new();
        for grant in &resolved {
            if let Some(person) = self.root_person_of(tenant_id, grant.node).await? {
                root_persons.insert(person.as_str().to_owned());
            }
        }
        let prev = self.head_event(tenant_id).await?;
        let act = self
            .begin_admin_act(
                admin,
                oml_domain::admin_act::AdminActKind::AddGrant,
                serde_json::json!({
                    "key_id": key.key_id.to_string(),
                    "name": key.name.as_ref().map(Name::as_str),
                    "role": key.role.as_str(),
                    "principal": key.principal,
                    // The roots' persons the grants reach, which the confirm
                    // screen names and a grant at once under `sole_admin`
                    // notifies (ADR-0097 d6).
                    "root_persons": root_persons,
                }),
                reason,
                serde_json::json!({
                    "key_id": key.key_id.to_string(),
                    "grants": resolved.iter().map(super::admin_acts::grant_json).collect::<Vec<_>>(),
                }),
                prev.as_ref(),
            )
            .await?;
        if act.pending() {
            self.commit_with_act(Some(act), Vec::new()).await?;
            return Ok(Vec::new());
        }
        let mut last = act.event.clone();
        let mut links = Vec::with_capacity(resolved.len());
        let mut added = Vec::with_capacity(resolved.len());
        for grant in resolved {
            let event = self.grant_event(&last, admin, key.key_id, &grant)?;
            let event = self.seal(
                Some(&last),
                tenant_id,
                &event.envelope.event_type,
                &event.envelope.principal,
                event.envelope.api_key_id,
                super::admin_acts::with_act_id(event.envelope.payload, act.act_id()),
            )?;
            added.push(GrantRecord {
                grant: grant.clone(),
                epoch: event.envelope.seq,
            });
            last = event.clone();
            links.push((
                event,
                MetadataFold::GrantIssued {
                    key_id: key.key_id,
                    grant,
                },
            ));
        }
        self.commit_with_act(Some(act), links).await?;
        Ok(added)
    }

    /// `GET /v2/review/queue` (ADR-0070 d5 #2): the review queue's depth and
    /// age per workspace, tenant-wide, no content (ADR-0045 d10, amending
    /// ADR-0026 d15), with each workspace's held instructions and the
    /// person's reviewer keys that may approve them: a live key whose grants
    /// and denials confer reviewer there, as `require_access` reads one key
    /// (ADR-0080 d16; #435). Counts only, never a key or a memory.
    pub async fn review_queue(&self, admin: &Caller) -> Result<Vec<QueueRow>, AppError> {
        // Counts per workspace, no content: an admin key's or a maintenance
        // key's (ADR-0097 d18).
        if !matches!(admin.key.role, Role::Admin | Role::Maintenance) {
            return Err(AppError::VerbCeiling {
                role: admin.key.role,
                verb: "review_queue",
            });
        }
        let tenant_id = admin.key.tenant_id;
        let depths = self.journal.web_queue(tenant_id).await?;
        let reviewers: Vec<_> = if depths.is_empty() {
            Vec::new()
        } else {
            let now = self.clock.now();
            self.journal
                .web_person_reviewers(tenant_id)
                .await?
                .into_iter()
                .filter(|r| !r.key.expired_at(&now))
                .collect()
        };
        let mut out = Vec::new();
        for depth in depths {
            let Some(ancestry) = self
                .journal
                .workspace_ancestry(tenant_id, depth.workspace_id)
                .await?
            else {
                out.push(QueueRow {
                    depth,
                    path: String::new(),
                    person_reviewers: 0,
                });
                continue;
            };
            let path = super::derived_path(&ancestry);
            let ids: Vec<Uuid> = ancestry.iter().map(|w| w.workspace_id).collect();
            let person_reviewers = reviewers
                .iter()
                .filter(|r| effective_roles(&r.grants, &r.denials, &ids).contains(&Role::Reviewer))
                .count() as u64;
            out.push(QueueRow {
                depth,
                path,
                person_reviewers,
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// A workspace's path, by which `GET /v2/keys` names a grant's node.
    pub async fn workspace_path(&self, caller: &Caller, node: Uuid) -> Result<String, AppError> {
        self.path_of(caller.key.tenant_id, node).await
    }

    async fn path_of(&self, tenant_id: Uuid, node: Uuid) -> Result<String, AppError> {
        Ok(self
            .journal
            .workspace_ancestry(tenant_id, node)
            .await?
            .map(|a| super::derived_path(&a))
            .unwrap_or_default())
    }

    // -- the Memories list and the workspace dropdowns ------------------------

    /// `GET /v1/memories?with_own=true` (ADR-0070 d5; ADR-0045 d22, d23), the
    /// Memories list: the scoped listing `<user>-web` is served at `universe`
    /// (or at the filter's workspace) and the own-work listing of every key
    /// naming the user, each read after the same position and merged newest
    /// first, a row in both shown once. `author` narrows to one of the person's
    /// keys and drops the scoped listing; a key that is not theirs is not
    /// found.
    pub async fn list_memories_with_own(
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
        // `promoted_since` with no state names every state, so it narrows
        // the promoted rows alone (ADR-0083 d3).
        let states: &[AdminState] = if req.states.is_empty() && req.promoted_since.is_some() {
            &AdminState::ALL
        } else {
            &req.states
        };
        let states: Vec<StateClause> = states
            .iter()
            .map(|s| {
                let (status, tip) = match s {
                    AdminState::Held => (MemoryStatus::Held, None),
                    AdminState::Promoted => (MemoryStatus::Promoted, Some(true)),
                    AdminState::Superseded => (MemoryStatus::Promoted, Some(false)),
                    AdminState::Rejected => (MemoryStatus::Rejected, None),
                    AdminState::Demoted => (MemoryStatus::Demoted, None),
                    AdminState::Withdrawn => (MemoryStatus::Withdrawn, None),
                };
                StateClause {
                    status,
                    tip,
                    promoted_since: req
                        .promoted_since
                        .clone()
                        .filter(|_| *s == AdminState::Promoted),
                }
            })
            .collect();
        let listing = |own: bool, workspace: Option<Reference>| ListRequest {
            own,
            workspace,
            states: states.clone(),
            kinds: req.kinds.clone(),
            claim_key: None,
            prefix: None,
            counts: false,
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
            as_of: None,
        };
        let own = self
            .list_memories(Bearer::Session(web), &listing(true, req.workspace.clone()))
            .await?;
        let scoped = if author.is_some() || req.mine {
            None
        } else {
            let scope = req.workspace.clone().unwrap_or_else(|| {
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
            counts: None,
            marks: std::collections::BTreeMap::new(),
        })
    }

    /// `GET /v2/workspaces/{workspace}/tree?counts=true` for a user key (ADR-0070
    /// d5; ADR-0045 d30), the Atlas's workspaces: the nodes the tree read serves
    /// `<user>-web` under `workspace` (by default `universe`, which a user key
    /// reads as its root and the `universe` node), each with the rows of
    /// [`Self::list_memories_with_own`] on it counted by state, so a region and
    /// a count exist only where the list would show the row; a held row outside
    /// an in-progress scope counts only for its author's person.
    pub async fn user_regions(
        &self,
        web: &Caller,
        workspace: Option<Reference>,
    ) -> Result<Regions, AppError> {
        let scope = workspace
            .clone()
            .unwrap_or_else(|| Reference::Name(Name::parse(UNIVERSE).expect("universe is a name")));
        let (mut regions, tree_cut) = self.tree_regions(web, scope).await?;
        // The page's own scope first; then each in-progress root on the
        // tree read as its own scope, where every author's held rows are
        // served (ADR-0030 d3), its subtree's counts replaced by that read.
        let mut partial = self
            .count_listing(web, workspace, &mut regions, &BTreeSet::new())
            .await?;
        partial |= tree_cut;
        let roots: Vec<Uuid> = regions
            .iter()
            .filter(|r| r.workspace.in_progress)
            .map(|r| r.workspace.workspace_id)
            .collect();
        for root in roots {
            let subtree: BTreeSet<Uuid> = self
                .journal
                .workspace_subtree(web.key.tenant_id, root)
                .await?
                .into_iter()
                .map(|w| w.workspace_id)
                .collect();
            for r in regions
                .iter_mut()
                .filter(|r| subtree.contains(&r.workspace.workspace_id))
            {
                (r.approved, r.held, r.in_progress, r.other) = (0, 0, 0, 0);
            }
            let outside: BTreeSet<Uuid> = regions
                .iter()
                .map(|r| r.workspace.workspace_id)
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
            .workspace_ancestry(tenant_id, top.workspace.workspace_id)
            .await?
            .unwrap_or_default();
        Ok(ancestry
            .iter()
            .skip(1)
            .rev()
            .map(|w| (w.workspace_id, w.name.clone()))
            .collect())
    }

    /// Counts the rows [`Self::list_memories_with_own`] lists under `workspace`
    /// onto `regions`, skipping rows on `skip`; whether it stopped at
    /// [`REGION_COUNT_MAX`].
    async fn count_listing(
        &self,
        web: &Caller,
        workspace: Option<Reference>,
        regions: &mut [Region],
        skip: &BTreeSet<Uuid>,
    ) -> Result<bool, AppError> {
        let mut cursor = None;
        let mut counted = 0;
        loop {
            let page = self
                .list_memories_with_own(
                    web,
                    &WebListRequest {
                        workspace: workspace.clone(),
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
                .filter(|m| !skip.contains(&m.memory.workspace_id))
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

    /// The admin page's Atlas workspaces (ADR-0045 d30): the tenant's tree
    /// under `workspace`, which an admin key reads as structure, with every
    /// row on each node counted by state, the admin read's scope and no
    /// wider. Counts carry no content, so they are read as the queue's
    /// depth is (d16) and journal nothing; the page of rows beside them is
    /// the one `memory.admin_read`.
    pub async fn admin_regions(
        &self,
        admin: &Caller,
        workspace: Option<Reference>,
    ) -> Result<Regions, AppError> {
        require_role(admin, Role::Admin, "admin_read")?;
        let tenant_id = admin.key.tenant_id;
        let scope = workspace
            .unwrap_or_else(|| Reference::Name(Name::parse(UNIVERSE).expect("universe is a name")));
        let (mut regions, tree_cut) = self.tree_regions(admin, scope).await?;
        let nodes: Vec<Uuid> = regions.iter().map(|r| r.workspace.workspace_id).collect();
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
                .workspace_tree(
                    Bearer::Session(caller),
                    scope.clone(),
                    cursor.as_deref(),
                    200,
                )
                .await?;
            regions.extend(page.items.into_iter().map(|node| Region {
                workspace: node.workspace,
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

    /// The acts a workspace's dropdown offers (ADR-0045 d28): a status
    /// transition as `transitions`, the mode and the commit as `mode`, each
    /// offered only where the verb's own check admits it now; a commit the
    /// held rows refuse is withheld with their count. A refusal hides an
    /// act; a store failure is answered.
    pub async fn workspace_offers(
        &self,
        transitions: &Caller,
        mode: &Caller,
        workspace_id: Uuid,
    ) -> Result<WorkspaceOffers, AppError> {
        let tenant_id = transitions.key.tenant_id;
        let node = self
            .journal
            .workspace(tenant_id, workspace_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let admitted = |result: Result<(), AppError>| match result {
            Ok(()) => Ok(true),
            Err(AppError::Store(e)) => Err(AppError::Store(e)),
            Err(_) => Ok(false),
        };
        let mut offers = Vec::new();
        for to in [WorkspaceStatus::Protected, WorkspaceStatus::Frozen] {
            let check = self
                .propose_admitted(transitions, workspace_id, to)
                .await
                .map(|_| ());
            if admitted(check)? {
                offers.push(WorkspaceOffer::Propose(to));
            }
        }
        for t in self
            .journal
            .pending_transitions(tenant_id, workspace_id)
            .await?
            .into_iter()
            .filter(|t| t.from == node.status)
        {
            let check = self
                .ratify_admitted(transitions, workspace_id, t.transition_id)
                .await
                .map(|_| ());
            if admitted(check)? {
                offers.push(WorkspaceOffer::Ratify {
                    transition_id: t.transition_id,
                    from: t.from,
                    to: t.to,
                });
            }
        }
        let mut commit_withheld = None;
        let wanted = if node.in_progress {
            WorkspaceMode::Standard
        } else {
            WorkspaceMode::InProgress
        };
        match self.mode_admitted(mode, workspace_id, wanted).await {
            Ok(super::ModeAdmitted::Change { .. }) => offers.push(if node.in_progress {
                WorkspaceOffer::Commit
            } else {
                WorkspaceOffer::InProgress
            }),
            Ok(super::ModeAdmitted::Already(_)) => {}
            Err(AppError::HeldRemaining { .. }) => {
                let nodes: Vec<Uuid> = self
                    .journal
                    .workspace_subtree(tenant_id, workspace_id)
                    .await?
                    .iter()
                    .map(|w| w.workspace_id)
                    .collect();
                commit_withheld = Some(self.journal.held_count(tenant_id, &nodes).await?);
            }
            Err(e) => {
                admitted(Err(e))?;
            }
        }
        Ok(WorkspaceOffers {
            workspace_id,
            status: node.status,
            in_progress: node.in_progress,
            offers,
            commit_withheld,
        })
    }

    /// Whether one act of the dropdown is admitted now, by the same check
    /// as [`Self::workspace_offers`]: `Ok` exactly when it is offered, else
    /// the refusal its verb would answer (ADR-0045 d28). Setting the mode a
    /// node already holds is no act, refused naming `op`.
    pub async fn workspace_offer_check(
        &self,
        transitions: &Caller,
        mode: &Caller,
        workspace_id: Uuid,
        offer: WorkspaceOffer,
    ) -> Result<(), AppError> {
        match offer {
            WorkspaceOffer::Propose(to) => self
                .propose_admitted(transitions, workspace_id, to)
                .await
                .map(|_| ()),
            WorkspaceOffer::Ratify { transition_id, .. } => self
                .ratify_admitted(transitions, workspace_id, transition_id)
                .await
                .map(|_| ()),
            WorkspaceOffer::InProgress | WorkspaceOffer::Commit => {
                let wanted = if offer == WorkspaceOffer::Commit {
                    WorkspaceMode::Standard
                } else {
                    WorkspaceMode::InProgress
                };
                match self.mode_admitted(mode, workspace_id, wanted).await? {
                    super::ModeAdmitted::Change { .. } => Ok(()),
                    super::ModeAdmitted::Already(_) => Err(field(
                        "op",
                        "the workspace is already in that mode, so there is nothing to do",
                    )),
                }
            }
        }
    }

    /// [`Self::workspace_offers`] for one key acting in both capacities, by
    /// its value or a session: what a dropdown would offer that key.
    pub async fn workspace_offers_for(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
    ) -> Result<WorkspaceOffers, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let workspace_id = self
            .resolve(&caller.key, NamedKind::Workspace, &workspace)
            .await?;
        self.workspace_offers(&caller, &caller, workspace_id).await
    }

    /// [`Self::workspace_offers_for`], and beside the offers each act a
    /// dropdown names that is not offered now with the refusal its verb
    /// would answer: proposing either rung, putting the workspace in
    /// progress and committing it. A confirmation reads the refusal off
    /// the wire, as the lead's page read it from
    /// [`Self::workspace_offer_check`] (ADR-0045 d28; ADR-0070 d5).
    pub async fn workspace_offers_explained_for(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
    ) -> Result<(WorkspaceOffers, Vec<(WorkspaceOffer, AppError)>), AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let workspace_id = self
            .resolve(&caller.key, NamedKind::Workspace, &workspace)
            .await?;
        let offers = self
            .workspace_offers(&caller, &caller, workspace_id)
            .await?;
        let mut refused = Vec::new();
        for act in [
            WorkspaceOffer::Propose(WorkspaceStatus::Protected),
            WorkspaceOffer::Propose(WorkspaceStatus::Frozen),
            WorkspaceOffer::InProgress,
            WorkspaceOffer::Commit,
        ] {
            if offers.offers.contains(&act) {
                continue;
            }
            match self
                .workspace_offer_check(&caller, &caller, workspace_id, act)
                .await
            {
                // Offered by the same check, so not reached; a page that
                // meets an act neither offered nor refused says so.
                Ok(()) => {}
                Err(AppError::Store(e)) => return Err(AppError::Store(e)),
                Err(e) => refused.push((act, e)),
            }
        }
        Ok((offers, refused))
    }

    // -- the admin read -----------------------------------------------------

    /// `POST /v2/admin/memories/search` (ADR-0045 d21): admin key role
    /// alone; every memory of the tenant the search names, any state, with
    /// its text and author, newest first. Journalled as one
    /// `memory.admin_read` naming the ids served and a digest of the query.
    #[allow(
        clippy::too_many_arguments,
        reason = "the admin search's filters and the investigation it reads under"
    )]
    pub async fn admin_search(
        &self,
        bearer: impl Into<Bearer<'_>>,
        query: Option<&str>,
        workspace: Option<Reference>,
        state: Option<AdminState>,
        after: Option<(EventTime, Uuid)>,
        limit: usize,
        investigation: Option<Uuid>,
    ) -> Result<AdminPage, AppError> {
        self.admin_search_filtered(
            bearer,
            &AdminSearchRequest {
                query,
                workspace,
                state,
                origin: None,
                author: None,
                kinds: Vec::new(),
                after,
                limit,
                investigation,
            },
        )
        .await
    }

    /// The admin read's search with the admin page's every filter
    /// (ADR-0045 d22, d24): [`Self::admin_search`] narrowed besides to one
    /// `origin.kind`, one authoring key, by name or id, of the tenant, and a
    /// set of kinds (ADR-0083 d5). One
    /// `memory.admin_read` per page served, as there.
    pub async fn admin_search_filtered(
        &self,
        bearer: impl Into<Bearer<'_>>,
        req: &AdminSearchRequest<'_>,
    ) -> Result<AdminPage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller, Role::Admin, "admin_read")?;
        let tenant_id = caller.key.tenant_id;
        let AdminSearchRequest {
            query,
            workspace,
            state,
            origin,
            author,
            kinds,
            after,
            limit,
            investigation,
        } = req.clone();
        let investigation = self
            .require_investigation(&caller.key, investigation, "/investigation")
            .await?;
        let author = match &author {
            None => None,
            Some(reference) => Some(self.resolve(&caller.key, NamedKind::Key, reference).await?),
        };
        let nodes = match &workspace {
            None => None,
            Some(reference) => {
                let node = self
                    .resolve(&caller.key, NamedKind::Workspace, reference)
                    .await?;
                let subtree = self.journal.workspace_subtree(tenant_id, node).await?;
                if subtree.is_empty() {
                    return Err(AppError::NotFound);
                }
                Some(subtree.into_iter().map(|w| w.workspace_id).collect())
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
            kinds,
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
            investigation,
            items
                .iter()
                .map(|m| (m.memory.memory_id, m.memory.workspace_id))
                .collect(),
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
        investigation: Option<Uuid>,
    ) -> Result<AdminMemory, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller, Role::Admin, "admin_read")?;
        let investigation = self
            .require_investigation(&caller.key, investigation, "investigation")
            .await?;
        let tenant_id = caller.key.tenant_id;
        let memory = self
            .journal
            .memory(tenant_id, memory_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let read = self.admin_memory(tenant_id, memory, true).await?;
        self.journal_admin_read(
            &caller.key,
            investigation,
            vec![(memory_id, read.memory.workspace_id)],
            None,
        )
        .await?;
        Ok(read)
    }

    async fn admin_memory(
        &self,
        tenant_id: Uuid,
        memory: MemoryRecord,
        with_verdicts: bool,
    ) -> Result<AdminMemory, AppError> {
        let workspace_path = self.path_of(tenant_id, memory.workspace_id).await?;
        let author = self.journal.key(tenant_id, memory.author_key_id).await?;
        let author_user = match author.as_ref().and_then(|k| k.user) {
            Some(root) => self
                .journal
                .workspace(tenant_id, root)
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
            workspace_path,
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

    /// One `memory.admin_read` for a page served, and one count per person
    /// whose own root the page read, for the month of the read, which that
    /// person sees on their own page (ADR-0097 d16).
    async fn journal_admin_read(
        &self,
        admin: &KeyRecord,
        investigation_id: Uuid,
        read: Vec<(Uuid, Uuid)>,
        query: Option<&str>,
    ) -> Result<(), AppError> {
        let ids: BTreeSet<String> = read.iter().map(|(id, _)| id.to_string()).collect();
        let tenant_id = admin.tenant_id;
        let mut persons = BTreeSet::new();
        let workspaces: BTreeSet<Uuid> = read.iter().map(|(_, w)| *w).collect();
        for workspace in workspaces {
            if let Some(person) = self.root_person_of(tenant_id, workspace).await? {
                persons.insert(person);
            }
        }
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
                "investigation_id": investigation_id.to_string(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::AdminMemoryRead {
                investigation_id,
                key_id: admin.key_id,
                persons: persons.into_iter().collect(),
            },
            None,
        )
        .await?;
        Ok(())
    }
}
