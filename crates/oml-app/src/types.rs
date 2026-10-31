//! Application-layer records and use-case request/response shapes.

use oml_domain::access::{Grant, KeyDenial};
use oml_domain::budget::Budget;
use oml_domain::name::{Name, NamedKind, Naming, Reference};
use oml_domain::{
    ChainError, EventTime, HeldBy, JcsError, LineageClosure, MemoryKind, MemoryStatus, Role,
    SignedEnvelope, TransitionError, VerdictError, VerifyError,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cursor::CursorError;
use crate::ports::StoreError;

/// The tenant a provider-level key resolves under. Its row names no tenant
/// (postgres-schema.md `api_keys.tenant_id` NULL), and no tenant holds the
/// nil id, so every tenant-scoped read under it answers nothing and no
/// grant or denial can name the key (ADR-0025 d13).
pub const PROVIDER_TENANT: Uuid = Uuid::nil();

/// A stored API key row: everything about a key is looked up by the value's
/// hash; the value itself is never stored (roles-and-keys.md key format).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRecord {
    pub key_id: Uuid,
    /// The owning tenant; [`PROVIDER_TENANT`] for a provider-level key.
    pub tenant_id: Uuid,
    pub role: Role,
    pub principal: String,
    pub held_by: HeldBy,
    pub default_workarea: Option<Uuid>,
    /// The key that issued this one; `None` for the bootstrap ceremony's
    /// first admin key, which no key issues (postgres-schema.md
    /// `api_keys.issued_by`).
    pub issued_by: Option<Uuid>,
    /// `sha256:<hex>` of the full key string, prefix included.
    pub key_hash: String,
    /// The key's name: unique in its tenant, operator keys sharing the
    /// provider's one namespace, and never released, a revoked key's
    /// included (roles-and-keys.md key names; ADR-0026 d1, d9, d10). `None`
    /// on a key issued before names, until the lead names it (ADR-0026 d11),
    /// and on a key issued without one, which the HTTP issuance body cannot
    /// send yet ([`IssueKeyRequest::name`]).
    pub name: Option<Name>,
    /// The crew record named at issuance, fixed for the key's life; `None`
    /// for a key with no crew (ADR-0026 d8).
    pub crew_id: Option<Uuid>,
}

/// A stored grant row: the domain grant and its epoch, the journal seq of
/// the `grant.issued` that wrote it (postgres-schema.md `grants`;
/// workareas.md permission epoch; ADR-0025 d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRecord {
    pub grant: Grant,
    pub epoch: u64,
}

/// A stored global per-key denial: the domain denial, the key that
/// journalled it, and its epoch, the journal seq of its `key.denied`
/// (postgres-schema.md `key_denials`; ADR-0025 d2, d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenialRecord {
    pub denial: KeyDenial,
    pub created_by: Uuid,
    pub epoch: u64,
}

/// What effective access is computed from for one key, read in one
/// snapshot: the floor of its permission epoch, its grants, its global
/// denials in force and the epochs of the lifts that ended the others, each
/// list in epoch order (workareas.md effective access and permission epoch;
/// ADR-0025 d1, d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAccess {
    /// The journal seq of the key's `key.issued`.
    pub issued_seq: u64,
    pub grants: Vec<GrantRecord>,
    /// The denials no `key.denial_lifted` has lifted: only these mask.
    pub denials: Vec<DenialRecord>,
    /// The journal seq of each `key.denial_lifted` on the key. A lifted
    /// denial masks nothing and its row stays as history; its lift still
    /// moves the permission epoch (event-catalog.md `key.denial_lifted`).
    pub lift_epochs: Vec<u64>,
}

impl KeyAccess {
    /// The key's permission epoch over what is built: the latest of its
    /// `key.issued`, `grant.issued`, `key.denied` and `key.denial_lifted`
    /// seqs (workareas.md permission epoch; ADR-0025 d14).
    #[must_use]
    pub fn permission_epoch(&self) -> u64 {
        self.grants
            .iter()
            .map(|g| g.epoch)
            .chain(self.denials.iter().map(|d| d.epoch))
            .chain(self.lift_epochs.iter().copied())
            .fold(self.issued_seq, u64::max)
    }
}

/// A workarea's crew: recorded, not operated — OML never contacts the
/// external manager (openapi.yaml `CrewRecord`; event-catalog.md
/// `workarea.create`). An absent member is stored and served absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crew {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charter_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_manager: Option<String>,
}

/// A workarea node (the PoC slice: id, parent, name and crew — no status
/// ladder yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkareaRecord {
    pub workarea_id: Uuid,
    pub tenant_id: Uuid,
    pub parent_id: Option<Uuid>,
    /// Unique in the tenant across the whole tree and never changed; the
    /// root's is `universe` (ADR-0026 d1, d6).
    pub name: Name,
    /// The crew object a PoC `workarea.create` copied in, kept as the
    /// history the naming of rows that predate names reads it from; `None`
    /// on every workarea created since crews are records (ADR-0026 d8, d11).
    pub crew: Option<Crew>,
    /// The crew record the workarea names; `None` when it names none
    /// (ADR-0026 d8).
    pub crew_id: Option<Uuid>,
}

/// A crew record: named, recorded and not operated — OML never contacts the
/// external manager (workareas.md crews; postgres-schema.md `crews`;
/// ADR-0026 d8). The name never changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrewRecord {
    pub crew_id: Uuid,
    pub tenant_id: Uuid,
    /// Unique in the tenant (ADR-0026 d1).
    pub name: Name,
    pub charter_uri: Option<String>,
    pub external_manager: Option<String>,
}

impl WorkareaRecord {
    /// The node's rung on the status ladder (workareas.md the status
    /// ladder). No status transition is built, so a node keeps the status it
    /// was created with: the universe root is permanently `committed`, and
    /// every other node is created in `development` (http-api.md workareas).
    #[must_use]
    pub fn status(&self) -> &'static str {
        if self.parent_id.is_none() {
            "committed"
        } else {
            "development"
        }
    }
}

/// A node as `workarea.create` committed it, with the path derived from the
/// stored ancestry: the same derivation the workarea read serves, so the
/// create and the read never name one node two ways (ADR-0025 d14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedWorkarea {
    pub workarea: WorkareaRecord,
    pub path: String,
    /// The crew record the create named, as the workarea read serves it.
    pub crew: Option<CrewRecord>,
}

/// One workarea as the calling key may see it (http-api.md workareas;
/// ADR-0025 d14): the node, its path derived from the `parent_id` names
/// from `universe`, the crew record it names, the immediate children the
/// key can read, the verbs its effective access confers there, and its
/// permission epoch. No workarea edge can exist in this build, so the read
/// serves no edges. An admin key reads it by its key role: every child, no
/// effective verb (ADR-0026 d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkareaInfo {
    pub workarea: WorkareaRecord,
    pub path: String,
    /// The crew record the workarea names, read from the store; `None` when
    /// it names none (ADR-0026 d8).
    pub crew: Option<CrewRecord>,
    /// Ordered by name in byte order, then id; a child the key cannot read
    /// is absent, not marked.
    pub children: Vec<WorkareaRecord>,
    /// The effective-verbs table of roles-and-keys.md over the roles
    /// effective on the node, in table order; empty for an admin key, which
    /// holds no grant role.
    pub my_effective_verbs: Vec<&'static str>,
    /// The key's permission epoch as its access was read for this request
    /// (workareas.md permission epoch).
    pub permission_epoch: u64,
}

/// One node of a workarea tree read, with its derived path (http-api.md
/// workareas, `GET /v2/workareas/{id}/tree`; ADR-0026 d15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub workarea: WorkareaRecord,
    pub path: String,
}

/// The rows of one tenant's chain that have no name yet: the tenant's own
/// row, workareas and keys. Every row a creation writes is named, so on the
/// names schema these are the rows that predate names, and keys issued
/// without one (ADR-0026 d11; postgres-schema.md names and tenants).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnnamedRows {
    pub tenant: bool,
    /// In id order.
    pub workareas: Vec<Uuid>,
    /// In id order.
    pub keys: Vec<Uuid>,
}

impl UnnamedRows {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.tenant && self.workareas.is_empty() && self.keys.is_empty()
    }
}

/// One chain the lead named at its start: the seq of its
/// `system.names_assigned`, what that event assigns, and the denials on
/// admin keys rule 5 lifted in the same commit, in journal order (ADR-0026
/// d11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignedNames {
    pub tenant_id: Uuid,
    pub journal_seq: u64,
    pub naming: Naming,
    pub lifted_denials: Vec<Uuid>,
}

/// Why the lead did not name a chain's rows that predate names; the chain
/// journals nothing (http-api.md rows created before names; ADR-0026 d11).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NamingRefusal {
    /// The chain already journals `system.names_assigned` at `seq`, yet a
    /// row has no name: the names of a chain are assigned once.
    #[error(
        "the chain journals system.names_assigned at seq {seq}, yet a row has no name (postgres-schema.md names and tenants)"
    )]
    AlreadyAssigned { seq: u64 },
    /// A row with no name has no creation event in the chain to name it by.
    #[error("the {} row {id} has no name and no creation event in the chain", .kind.as_str())]
    NoCreationEvent { kind: NamedKind, id: Uuid },
}

/// The metadata row of one memory version. The body lives here, never in the
/// journal — journal payloads are digest-only (ADR-0002 d1, admission.md).
/// The claim fields follow postgres-schema.md `memories` (ADR-0025 d6, d10,
/// d11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRecord {
    pub memory_id: Uuid,
    pub tenant_id: Uuid,
    pub workarea_id: Uuid,
    /// Names one live claim per workarea; its versions share it
    /// (audit-and-lineage.md supersession chains).
    pub claim_key: String,
    /// The ordinal within the `claim_key`: 1 for a new claim, 1 + the
    /// greatest version of the key in any state for a successor (ADR-0025
    /// d10).
    pub version: u32,
    pub kind: MemoryKind,
    /// The NFC-normalized claim text.
    pub body: String,
    pub body_digest: String,
    /// UTF-8 octets of the NFC body, the count budget tokens derive from
    /// (retrieval.md token budgets; ADR-0025 d6).
    pub body_bytes: u64,
    pub status: MemoryStatus,
    /// Whether this version is its claim's served tip: set when a version
    /// is promoted, cleared when a ratified successor replaces it. A held,
    /// rejected or replaced version is never the tip (ADR-0025 d10).
    pub is_tip: bool,
    /// The `at` of the event that promoted this version; `None` until then.
    pub promoted_at: Option<EventTime>,
    /// A retirement version: ratified as tip, it retires the claim (ADR-0025
    /// d11).
    pub retirement: bool,
    /// The version a supersession replaces; `None` for a version 1.
    pub replaces: Option<Uuid>,
    pub author_principal: String,
    pub author_key_id: Uuid,
    pub derives_from: Vec<Uuid>,
    /// The row's lineage closure, flattened: the author plus every declared
    /// ancestor's closure (roles-and-keys.md lineage-closure exclusion).
    pub lineage_principals: Vec<String>,
    pub lineage_key_ids: Vec<Uuid>,
    pub scan_finding: Option<String>,
    pub created_at: EventTime,
}

/// What a capture reads of one `claim_key` in one workarea before it commits
/// (audit-and-lineage.md supersession chains; ADR-0025 d10, d18).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClaimKeyState {
    /// The greatest `version` of any row of the key, whatever its state;
    /// `None` when no row carries the key.
    pub greatest_version: Option<u32>,
    /// Whether a held, promoted or demoted version holds the key: a new
    /// claim (version 1) collides then. A key whose every version is
    /// rejected or erased is released.
    pub live: bool,
}

/// A version a supersession replaces, as served: its id and its ordinal
/// (openapi.yaml `MemorySummary.replaces`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRef {
    pub artifact_id: Uuid,
    pub version: u32,
}

/// One memory version as the calling key is served it (http-api.md get and
/// recall; ADR-0025 d3, d12): the row with `derives_from` cut to the
/// ancestors the key can read, the version it replaces only where the key
/// can read that version too, and the workarea's derived path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedMemory {
    pub memory: MemoryRecord,
    pub replaces: Option<VersionRef>,
    /// The `parent_id` names from the root to the row's workarea, joined
    /// with `.` (ADR-0025 d14); `None` for a node the tenant does not hold.
    pub workarea_path: Option<String>,
}

impl MemoryRecord {
    /// The domain closure used by `check_reviewer` (ADR-0001 R2).
    #[must_use]
    pub fn lineage_closure(&self) -> LineageClosure {
        let mut closure = LineageClosure::new();
        for principal in &self.lineage_principals {
            closure.add_principal(principal.clone());
        }
        for key_id in &self.lineage_key_ids {
            closure.add_key_id(*key_id);
        }
        closure
    }
}

// ---------------------------------------------------------------------------
// Use-case requests and responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BootstrapRequest {
    /// The tenant's name, unique in the deployment (ADR-0026 d1).
    pub tenant_name: Name,
    pub admin_principal: String,
}

/// Show-once: the value is returned exactly here and never stored
/// (roles-and-keys.md key lifecycle), and only after the key's last
/// `grant.issued` committed. The rest is the key as `key.issued` journalled
/// it and its grants as stored, ids and epochs included (ADR-0025 d2).
#[derive(Debug, Clone)]
pub struct IssuedKey {
    pub key_id: Uuid,
    pub value: String,
    pub role: Role,
    pub principal: String,
    /// The issuer's attestation as journalled; `None` when none was sent.
    pub held_by: Option<HeldBy>,
    /// The sent default, else the first grant's node; `None` for a
    /// zero-grant key.
    pub default_workarea: Option<Uuid>,
    /// The key's name as journalled ([`IssueKeyRequest::name`]).
    pub name: Option<Name>,
    /// The crew record named at issuance, fixed for the key's life
    /// (ADR-0026 d8).
    pub crew_id: Option<Uuid>,
    pub access: KeyAccess,
}

#[derive(Debug, Clone)]
pub struct BootstrapResult {
    pub tenant_id: Uuid,
    pub root_workarea_id: Uuid,
    pub admin_key: IssuedKey,
}

/// `POST /v2/keys`, validated in the issuance order (roles-and-keys.md
/// grant admissibility and refusal order; ADR-0025 d2).
///
/// Every workarea and crew reference takes an id or a name, resolved in the
/// issuer's tenant after the issuer's role is checked and before step 1
/// (roles-and-keys.md issuance order; ADR-0026 d4).
#[derive(Debug, Clone)]
pub struct IssueKeyRequest {
    pub role: Role,
    /// The key's name, distinct from its principal and its label, checked
    /// after every other check (step 7, `name.taken`; ADR-0026 d5, d9).
    /// The HTTP issuance body does not carry one yet, so `None` issues the
    /// key unnamed and journals `name: null`, as every key before names was.
    pub name: Option<Name>,
    pub principal: String,
    /// The issuing admin's attestation; `None` journals null and stores the
    /// schema default, `agent`.
    pub held_by: Option<HeldBy>,
    /// `None` takes the first grant's node; a key never inherits its
    /// issuer's default.
    pub default_workarea: Option<Reference>,
    /// The key's crew record, fixed for its life; one the tenant does not
    /// hold is absent at step 4 (ADR-0026 d8).
    pub crew: Option<Reference>,
    pub grants: Vec<GrantRequest>,
}

/// One requested grant: a role over the subtree of `node`, less the
/// subtrees of `deny`, each of which must lie inside it (workareas.md
/// per-grant attenuation), every node by id or name.
#[derive(Debug, Clone)]
pub struct GrantRequest {
    pub node: Reference,
    pub role: Role,
    pub deny: Vec<Reference>,
}

/// `POST /v2/crews` (http-api.md crews; ADR-0026 d8): every field but the
/// name optional.
#[derive(Debug, Clone)]
pub struct CrewCreateRequest {
    pub name: Name,
    pub charter_uri: Option<String>,
    pub external_manager: Option<String>,
}

/// `POST /v2/crews/{crew_id}/update`: the fields to change, at least one;
/// a field not sent keeps its value, and the name never changes
/// (http-api.md crews; ADR-0026 d8).
#[derive(Debug, Clone, Default)]
pub struct CrewUpdate {
    pub charter_uri: Option<String>,
    pub external_manager: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SubmitRequest {
    pub idempotency_key: String,
    pub claim_key: String,
    pub kind: MemoryKind,
    /// The claim text as sent; capture normalizes it to NFC (admission.md).
    pub body: String,
    /// `X-OML-Workarea`, by id or name; `None` resolves to the key's
    /// default workarea (http-api.md, ADR-0022 d4; ADR-0026 d4).
    pub workarea_id: Option<Reference>,
    pub derives_from: Vec<Uuid>,
}

/// The stored idempotent response: a replayed submit returns this byte for
/// byte (http-api.md idempotency record).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitReceipt {
    pub memory_id: Uuid,
    pub claim_key: String,
    pub version: u32,
    pub workarea_id: Uuid,
    pub state: String,
    pub journal_seq: u64,
    pub body_digest: String,
}

#[derive(Debug, Clone)]
pub struct SubmitOutcome {
    pub receipt: SubmitReceipt,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Reject,
}

#[derive(Debug, Clone)]
pub struct VerdictOutcome {
    pub memory_id: Uuid,
    pub version: u32,
    pub state: MemoryStatus,
    pub journal_seq: u64,
}

/// The kinds recall serves as hits. Vocabulary kinds join them only by a
/// workarea's journalled opt-in, which is unbuilt (retrieval.md, the
/// recall-ordering contract; ADR-0021 d9, ADR-0025 d8).
pub const HIT_KINDS: [MemoryKind; 2] = [MemoryKind::Memory, MemoryKind::DerivedFact];

#[derive(Debug, Clone)]
pub struct RecallRequest {
    pub query: String,
    pub k: usize,
    /// The scope node, by id or name; `None` resolves to the key's default
    /// workarea, and a key with neither is refused — there is no tenant-wide
    /// fallback (ADR-0025 d1; ADR-0026 d4).
    pub workarea_id: Option<Reference>,
    /// The hit kinds the request admits; empty when it sent no filter. It
    /// narrows the hits only, never the steering (retrieval.md, the
    /// recall-ordering contract; ADR-0025 d8).
    pub kinds: Vec<MemoryKind>,
    /// Whether the served rows carry their bodies; a budget counts bodies,
    /// so `max_tokens` without them is refused (retrieval.md token budgets;
    /// ADR-0025 d7).
    pub include_bodies: bool,
    /// The calling key's own held rows join the ranked hits, flagged by
    /// state (retrieval.md, own held rows in recall; ADR-0025 d4).
    pub include_own_held: bool,
    /// The budget in budget tokens under counter `oml-tc/1`; `None` is no
    /// budget (retrieval.md token budgets; ADR-0025 d7).
    pub max_tokens: Option<u64>,
}

/// One recall candidate as the store ranks it, before the `k` and budget
/// cuts: its id, the clear `body_bytes` its budget-token count derives
/// from, and the store's score. It carries no body, so a row the cuts drop
/// is never hydrated; the served rows hydrate in one set-wise read
/// (retrieval.md token budgets; ADR-0024 d4, ADR-0025 d7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecallCandidate {
    pub memory_id: Uuid,
    pub body_bytes: u64,
    /// The store's rank: `ts_rank` in Postgres, the count of distinct query
    /// terms matched in the in-memory store (retrieval.md, the
    /// recall-ordering contract).
    pub score: f32,
}

/// One recall hit: the row as the calling key is served it, and the store's
/// score for it (openapi.yaml `RecallResponse.hits`; ADR-0025 d8). A hit
/// reads as the served row it carries.
#[derive(Debug, Clone)]
pub struct RecallHit {
    pub served: ServedMemory,
    /// The store's rank: `ts_rank` in Postgres, the count of distinct query
    /// terms matched in the in-memory store (retrieval.md, the
    /// recall-ordering contract).
    pub score: f32,
}

impl core::ops::Deref for RecallHit {
    type Target = ServedMemory;

    fn deref(&self) -> &ServedMemory {
        &self.served
    }
}

/// Applicable steering, ordered structurally and never ranked, precedes the
/// hits, ordered by score descending then `artifact_id` ascending
/// (retrieval.md, the recall-ordering contract; ADR-0025 d8), cut to `k`
/// and then to the budget (ADR-0025 d7).
#[derive(Debug, Clone)]
pub struct RecallResult {
    pub steering: Vec<ServedMemory>,
    pub hits: Vec<RecallHit>,
    /// How the `max_tokens` cut was applied; `None` when no budget was sent,
    /// and the response then carries no budget object (retrieval.md token
    /// budgets).
    pub budget: Option<Budget>,
    /// The artifact ids served after the `k` and budget cuts, steering then
    /// hits, own held rows included: what the read log records, never the
    /// candidates before the cuts (retrieval.md, the read log; ADR-0025 d9).
    pub served_ids: Vec<Uuid>,
    /// SHA-256 over the RFC 8785 JSON of the whole request, every member
    /// present with its default applied (retrieval.md, the read log;
    /// ADR-0025 d9).
    pub query_digest: String,
}

/// Whose rows a listing reads: the store's own predicate, so no page
/// reaches a row outside it (http-api.md search; ADR-0025 d13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingScope {
    /// The own-work listing: every row the key set authored, in every
    /// state, except the rows on a `masked` node — the subtrees the key's
    /// global per-key denials cover (roles-and-keys.md#own). The key set is
    /// the calling key alone until rotation is built.
    Own {
        key_ids: Vec<Uuid>,
        masked: Vec<Uuid>,
    },
    /// The scoped listing: the promoted served tips on exactly these nodes,
    /// subtree(scope) ∩ reader effective access as the use-case computes
    /// it; an empty set lists nothing (ADR-0025 d1, d13).
    Tips { nodes: Vec<Uuid> },
}

/// A listing's predicates, re-applied on every page: a cursor carries a
/// position only (http-api.md pagination; ADR-0025 d13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingFilter {
    pub scope: ListingScope,
    /// Only rows on these nodes — the own-work listing's `workarea_id`
    /// subtree; `None` narrows nothing.
    pub subtree: Option<Vec<Uuid>>,
    pub state: Option<MemoryStatus>,
    pub kind: Option<MemoryKind>,
    pub claim_key: Option<String>,
    /// `created_at` at or after this instant.
    pub since: Option<EventTime>,
    /// `created_at` before this instant.
    pub until: Option<EventTime>,
}

/// Where a listing page resumes: the `created_at` and id of the last row
/// the previous page served, in the order `created_at` descending, then
/// `artifact_id` descending (http-api.md search).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingPosition {
    pub created_at: EventTime,
    pub artifact_id: Uuid,
}

/// `GET /v1/memories` as the use-case takes it (http-api.md search;
/// ADR-0025 d13).
#[derive(Debug, Clone)]
pub struct ListRequest {
    /// `own=true`: the own-work listing, which resolves no workarea, so it
    /// reads no `scope` (the transport refuses an `X-OML-Workarea` sent
    /// with it).
    pub own: bool,
    /// The scoped listing's scope node, by id or name; `None` resolves to
    /// the key's default workarea, and a key with neither is refused.
    pub scope: Option<Reference>,
    /// The `workarea_id` subtree filter, by id or name.
    pub workarea_id: Option<Reference>,
    pub state: Option<MemoryStatus>,
    pub kind: Option<MemoryKind>,
    pub claim_key: Option<String>,
    pub since: Option<EventTime>,
    pub until: Option<EventTime>,
    /// The page size, which the transport bounds by `api.page_limit_max`.
    pub limit: usize,
    /// The opaque cursor of the previous page.
    pub cursor: Option<String>,
}

/// One listing page: summaries as the calling key is served them, and the
/// cursor of the next page, `None` once the listing is exhausted.
#[derive(Debug, Clone)]
pub struct MemoryPage {
    pub items: Vec<ServedMemory>,
    pub next_cursor: Option<String>,
}

/// `POST /v1/memories/{artifact_id}/supersede` (http-api.md supersede;
/// ADR-0025 d10, d11). The successor's `claim_key` and workarea are always
/// the target's.
#[derive(Debug, Clone)]
pub struct SupersedeRequest {
    pub idempotency_key: String,
    pub target_id: Uuid,
    /// The successor's text as sent; capture normalizes it to NFC. With
    /// `retirement`, why the claim is retired.
    pub body: String,
    /// `None` takes the target's kind, the one field a successor inherits.
    pub kind: Option<MemoryKind>,
    /// A retire-proposal: ratified, the successor retires the claim.
    pub retirement: bool,
    /// Ancestors beside the target, which is always one.
    pub derives_from: Vec<Uuid>,
    /// Digested with the request, never journalled.
    pub reason: Option<String>,
    /// `X-OML-Workarea`, by id or name: when sent, it must be the target's
    /// workarea.
    pub workarea_id: Option<Reference>,
    /// Set by `oml_supersede`, which carries no vocabulary fields: a target
    /// of a vocabulary kind is refused, and superseded on the HTTP API
    /// instead (mcp-plugin.md `oml_supersede`; ADR-0025 d12).
    pub refuse_vocabulary_target: bool,
}

/// The stored idempotent response of a supersession: a replay returns this
/// byte for byte, naming only the caller's own successor (openapi.yaml
/// `SubmitAccepted`; ADR-0025 d10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersedeReceipt {
    pub memory_id: Uuid,
    pub claim_key: String,
    pub version: u32,
    pub workarea_id: Uuid,
    pub state: String,
    pub retirement: bool,
    pub replaces: VersionRef,
    pub journal_seq: u64,
    pub body_digest: String,
}

#[derive(Debug, Clone)]
pub struct SupersedeOutcome {
    pub receipt: SupersedeReceipt,
    pub replayed: bool,
}

/// One event as the content tail serves it to the calling key
/// (http-api.md#events-tail; ADR-0025 d15).
#[derive(Debug, Clone)]
pub struct TailedEvent {
    /// The journalled event, its hash and signature as stored.
    pub event: SignedEnvelope,
    /// Whether the event's actor, its envelope `api_key_id`, is in the
    /// calling key's own key set; outside it the tail serves `principal` and
    /// `api_key_id` as null.
    pub names_actor: bool,
    /// The payload as served: outside the actor's own key set every field
    /// naming a key is null, and a claim-subject workarea id the naming rule
    /// withholds is null (roles-and-keys.md#own).
    pub payload: serde_json::Value,
}

/// One page of the events tail: what one bounded scan let the key see, the
/// head read with it, and where the next page resumes
/// (http-api.md#events-tail; ADR-0025 d15).
#[derive(Debug, Clone)]
pub struct TailPage {
    pub events: Vec<TailedEvent>,
    pub head_seq: u64,
    pub next_after_seq: u64,
}

/// A typed application refusal; every variant names the rule it enforces.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unknown api key (reads fail closed — http-api.md authentication)")]
    Unauthorized,
    #[error(
        "{verb} is outside the {role:?} verb ceiling (roles-and-keys.md permission matrix, R1)"
    )]
    VerbCeiling { role: Role, verb: &'static str },
    /// Issuance steps 2 and 3: with no `grant_index`, `role` is the requested
    /// key role and `admissible` the key roles the issuer may issue; with
    /// one, `role` is the first refused grant's role and `admissible` the key
    /// role's admissible grant roles (roles-and-keys.md issuance order).
    #[error(
        "{role:?} is outside the admissible set {admissible:?} (roles-and-keys.md issuer ceiling, access.issuer_ceiling)"
    )]
    IssuerCeiling {
        role: Role,
        admissible: &'static [Role],
        grant_index: Option<usize>,
    },
    #[error(
        "an admin key never carries a dreamer grant (roles-and-keys.md two-key rule, access.role_exclusive)"
    )]
    RoleExclusive,
    #[error(
        "the node is readable but effective access does not confer {needed_role:?} (roles-and-keys.md refusal order, access.grant_refused)"
    )]
    GrantRefused {
        workarea_id: Uuid,
        needed_role: Role,
    },
    /// The deny node as the request spelled it (http-api.md names and ids).
    #[error(
        "deny node {subtree_node} lies outside grant {grant_index}'s subtree (roles-and-keys.md issuance order, request.validation)"
    )]
    DenyOutsideGrant {
        grant_index: usize,
        subtree_node: Reference,
    },
    /// The default as the request spelled it.
    #[error(
        "no grant of the key covers default workarea {workarea} (roles-and-keys.md issuance order, request.validation)"
    )]
    DefaultUncovered { workarea: Reference },
    #[error(
        "no workarea: the key has no default and the request names none (http-api.md request.workarea_unresolved)"
    )]
    WorkareaUnresolved,
    #[error(
        "max_tokens counts bodies that include_bodies false would not send (retrieval.md token budgets, request.validation)"
    )]
    BudgetWithoutBodies,
    #[error(
        "after_seq {after_seq} is beyond head_seq {head_seq} (http-api.md events tail, request.validation)"
    )]
    AfterSeqBeyondHead { after_seq: u64, head_seq: u64 },
    #[error(
        "not found — absence and no-access are indistinguishable (http-api.md access.not_found)"
    )]
    NotFound,
    #[error(
        "a writer may supersede only the claims its own key authored (roles-and-keys.md#own, access.author_only)"
    )]
    AuthorOnly,
    /// The target is not the current served tip, at supersede or at the
    /// ratifying verdict; `current_tip` only when the caller can read it
    /// (errors.md `submission.tip_conflict`; ADR-0025 d10, d16).
    #[error(
        "the supersede target is not the current promoted tip (http-api.md submission.tip_conflict)"
    )]
    TipConflict { current_tip: Option<VersionRef> },
    /// The header as the request spelled it.
    #[error(
        "X-OML-Workarea names {workarea}, not the target's workarea: a supersession is captured in its target's (http-api.md supersede, request.validation)"
    )]
    WorkareaNotTarget { workarea: Reference },
    #[error(
        "a {target:?} target admits no {sent:?} successor: a kind changes only within its class, and a retirement keeps the target's (audit-and-lineage.md, request.validation)"
    )]
    KindChangeRefused {
        target: MemoryKind,
        sent: MemoryKind,
    },
    #[error(
        "a term is never retired: terms deprecate (ADR-0021 d3; ADR-0025 d11, request.validation)"
    )]
    TermRetirement,
    #[error(
        "oml_supersede carries no vocabulary fields, so a {0:?} target is superseded on the HTTP API (mcp-plugin.md oml_supersede; ADR-0025 d12, request.validation)"
    )]
    VocabularyTarget(MemoryKind),
    #[error(
        "a held, promoted or demoted version holds this claim_key in the workarea (audit-and-lineage.md, submission.claim_key_conflict)"
    )]
    ClaimKeyConflict,
    #[error("deterministic scan veto: {0} (admission.md — a refusal that journals nothing)")]
    ScanVeto(String),
    #[error(
        "derives_from names {0}, which the key cannot read or no row is (fail closed, access.not_found)"
    )]
    LineageUnknown(Uuid),
    #[error(
        "idempotency key reused with a different request (http-api.md idempotency.digest_mismatch)"
    )]
    IdempotencyDigestMismatch,
    #[error("stored idempotent response is unreadable: {0}")]
    StoredResponse(String),
    /// A creation's name its kind already holds in the namespace, a revoked
    /// key's included; nothing was journalled. Only admin and operator keys
    /// hold a creating verb, and the bootstrap claim checks its token first,
    /// so no agent key ever receives it (errors.md `name.taken`; ADR-0026
    /// d5, d10).
    #[error("the {} name {} is already taken in its namespace (errors.md name.taken)", .kind.as_str(), .name.as_str())]
    NameTaken { kind: NamedKind, name: Name },
    /// Denials attenuate grants and an admin key holds none, so no denial on
    /// one is journalled (http-api.md keys, grants, denials; ADR-0026 d15).
    #[error(
        "a global denial on an admin key is refused: an admin key carries no grant for it to attenuate (roles-and-keys.md permission matrix footnote 6, request.validation, field key_id)"
    )]
    DenialOnAdminKey,
    #[error(
        "a crew update sends charter_uri or external_manager, at least one (http-api.md crews, request.validation)"
    )]
    EmptyCrewUpdate,
    /// The lead's naming of a chain's rows that predate names refused; the
    /// chain journals nothing (ADR-0026 d11).
    #[error("the rows of tenant {tenant_id} that predate names were not named: {refusal}")]
    NamingRefused {
        tenant_id: Uuid,
        refusal: NamingRefusal,
    },
    /// A listing cursor this server will not resume from (http-api.md
    /// pagination, request.validation).
    #[error(transparent)]
    Cursor(#[from] CursorError),
    #[error(transparent)]
    Verdict(#[from] VerdictError),
    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error(transparent)]
    Chain(#[from] ChainError),
    #[error(transparent)]
    ChainVerify(#[from] VerifyError),
    #[error(transparent)]
    Jcs(#[from] JcsError),
    #[error(transparent)]
    Store(StoreError),
}

/// A store refusal as the application answers it: the unique index's
/// refusal of a creation's name is [`AppError::NameTaken`], whichever
/// commit carried it; every other refusal passes through (ADR-0026 d5).
impl From<StoreError> for AppError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NameTaken { kind, name } => AppError::NameTaken { kind, name },
            other => AppError::Store(other),
        }
    }
}
