//! Grant admissibility, effective access, the refusal order, effective
//! verbs and content-tail visibility — pure functions over journalled grant
//! and denial records, computed on every request with no cache
//! (ADR-0025 d1, d2, d14, d15; ADR-0019 d1) — and the dreamer grant narrowed
//! by `edge_labels`, with the member profile an ontology crew's keys carry
//! (ADR-0027 d3, d4, d5).
//!
//! The tables are declared on `docs/architecture/roles-and-keys.md`
//! (grant admissibility and refusal order, effective verbs, narrowed
//! dreamer grants, member profiles), `docs/architecture/workareas.md`
//! (effective access) and `docs/api/http-api.md` (events tail); this module
//! mirrors them and adds nothing.
//!
//! The ADR-0027 refusals answer these codes of `docs/api/errors.md`:
//!
//! | Refusal | Code |
//! |---|---|
//! | [`EdgeLabelsRefusal`], every variant | `request.validation` |
//! | [`VerbRefusal::VerbCeiling`] | `access.verb_ceiling` |
//! | [`VerbRefusal::GrantRefused`] | `access.grant_refused`, `details` `{workarea_id, needed_role}` plus `edge_labels` where dreamer is narrowed |
//! | [`VerbRefusal::NotFound`] | `access.not_found` |
//! | [`ProfileMismatch`] | `access.profile_mismatch` (403), `details` `{profile, clause, grant_index}` |

use std::collections::BTreeSet;

use uuid::Uuid;

use crate::model::{HeldBy, Role};
use crate::vocab::OmlLabel;

/// One grant on a key: one role over the subtree of one workarea node, with
/// its own deny entries, each a subtree masked within this grant only
/// (workareas.md, per-grant attenuation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub grant_id: Uuid,
    /// The granted node id, never a path (workareas.md, grants bind to node ids).
    pub node: Uuid,
    pub role: Role,
    /// Subtree roots this grant does not cover.
    pub deny: Vec<Uuid>,
}

/// A global per-key denial: a subtree the key cannot reach whatever its
/// grants say — deny-wins (workareas.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDenial {
    pub denial_id: Uuid,
    pub node: Uuid,
}

/// The grant roles a key of `key_role` may carry, in table order
/// (roles-and-keys.md, grant admissibility; ADR-0025 d1). Admin and
/// operator keys carry no grant, and no key carries an admin or operator
/// grant.
#[must_use]
pub fn admissible_grant_roles(key_role: Role) -> &'static [Role] {
    match key_role {
        Role::Reader => &[Role::Reader],
        Role::Writer => &[Role::Reader, Role::Writer],
        Role::Dreamer => &[Role::Reader, Role::Dreamer],
        Role::Admin | Role::Operator => &[],
    }
}

/// The key roles a key of `issuer_role` may issue (roles-and-keys.md,
/// issuer ceiling): a tenant admin issues every tenant role, the operator
/// issues operator keys and a tenant's first admin key, and no other role
/// issues at all.
#[must_use]
pub fn issuable_key_roles(issuer_role: Role) -> &'static [Role] {
    match issuer_role {
        Role::Admin => &[Role::Reader, Role::Writer, Role::Dreamer, Role::Admin],
        Role::Operator => &[Role::Admin, Role::Operator],
        Role::Reader | Role::Writer | Role::Dreamer => &[],
    }
}

/// A refused issuance, in the issuance validation order of
/// roles-and-keys.md (ADR-0025 d2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IssuanceRefusal {
    /// Step 1, `access.role_exclusive`: an admin key never carries a dreamer
    /// grant — the two-key rule.
    #[error(
        "an admin key never carries a dreamer grant (roles-and-keys.md two-key rule, access.role_exclusive)"
    )]
    RoleExclusive,
    /// Steps 2 and 3, `access.issuer_ceiling`. With no `grant_index`, `role`
    /// is the requested key role and `admissible` the key roles the issuer
    /// may issue; with one, `role` is the first refused grant's role,
    /// `admissible` the key role's admissible grant roles, and `grant_index`
    /// that grant's 0-based position.
    #[error(
        "{role:?} is outside the admissible set {admissible:?} (roles-and-keys.md issuer ceiling, access.issuer_ceiling)"
    )]
    IssuerCeiling {
        role: Role,
        admissible: &'static [Role],
        grant_index: Option<usize>,
    },
}

/// Steps 1–3 of the issuance validation order (roles-and-keys.md; ADR-0025
/// d2): the two-key rule, the issuer ceiling on the key role, then grant
/// admissibility, first failure answering. The caller has already refused an
/// issuer outside the issue verb (`access.verb_ceiling`) and checked
/// `tenant_id` (step 0); node existence, deny containment and the default's
/// coverage (steps 4–6) need the store and follow this check.
pub fn check_issuance(
    issuer_role: Role,
    key_role: Role,
    grants: &[Grant],
) -> Result<(), IssuanceRefusal> {
    if key_role == Role::Admin && grants.iter().any(|g| g.role == Role::Dreamer) {
        return Err(IssuanceRefusal::RoleExclusive);
    }
    let issuable = issuable_key_roles(issuer_role);
    if !issuable.contains(&key_role) {
        return Err(IssuanceRefusal::IssuerCeiling {
            role: key_role,
            admissible: issuable,
            grant_index: None,
        });
    }
    let admissible = admissible_grant_roles(key_role);
    if let Some((index, refused)) = grants
        .iter()
        .enumerate()
        .find(|(_, g)| !admissible.contains(&g.role))
    {
        return Err(IssuanceRefusal::IssuerCeiling {
            role: refused.role,
            admissible,
            grant_index: Some(index),
        });
    }
    Ok(())
}

/// Whether a global per-key denial names the node whose ancestry is
/// `ancestry`, or an ancestor of it — deny-wins (workareas.md).
fn globally_denied(denials: &[KeyDenial], ancestry: &[Uuid]) -> bool {
    denials.iter().any(|d| ancestry.contains(&d.node))
}

/// Whether `grant` survives at the node whose ancestry is `ancestry`: it is
/// on the node or an ancestor, and its own deny list names neither
/// (workareas.md, per-grant attenuation).
fn survives(grant: &Grant, ancestry: &[Uuid]) -> bool {
    ancestry.contains(&grant.node) && !grant.deny.iter().any(|d| ancestry.contains(d))
}

/// `effective_roles(N)` (workareas.md, effective access; ADR-0025 d1):
/// every grant on N or an ancestor of N, less each grant whose own deny list
/// names N or an ancestor of N, unioned; the empty set when any global
/// denial names N or an ancestor of N. `ancestry` runs from N to the root;
/// an empty ancestry — an unknown node — confers nothing. It is
/// [`effective_access`]'s `roles` where no grant is narrowed (ADR-0027 d5).
#[must_use]
pub fn effective_roles(
    grants: &[Grant],
    denials: &[KeyDenial],
    ancestry: &[Uuid],
) -> BTreeSet<Role> {
    if globally_denied(denials, ancestry) {
        return BTreeSet::new();
    }
    grants
        .iter()
        .filter(|g| survives(g, ancestry))
        .map(|g| g.role)
        .collect()
}

/// A refused workarea-scoped verb, by step of the refusal order
/// (roles-and-keys.md; errors.md `access`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AccessRefusal {
    /// Step 1, `access.verb_ceiling`: the key's role does not admit the
    /// needed grant role, whatever its grants.
    #[error(
        "a {role:?} key never holds {needed:?} on any node (roles-and-keys.md refusal order step 1, access.verb_ceiling)"
    )]
    VerbCeiling { role: Role, needed: Role },
    /// Step 3, `access.grant_refused`: the node is readable but its
    /// effective access does not confer the needed role. The caller adds the
    /// `workarea_id` detail.
    #[error(
        "the node is readable but effective access does not confer {needed_role:?} (roles-and-keys.md refusal order step 3, access.grant_refused)"
    )]
    GrantRefused { needed_role: Role },
    /// Step 4, `access.not_found`: neither the needed role nor reader.
    #[error("not found (roles-and-keys.md refusal order step 4, access.not_found)")]
    NotFound,
}

/// The refusal order for a verb needing grant role `needed` on a node whose
/// [`effective_roles`] are `effective` (roles-and-keys.md; ADR-0025 d1):
/// verb ceiling, allowed, grant refused, not found. Step 1 reads only the
/// key's role, so a caller may also check it through
/// [`admissible_grant_roles`] before it resolves the node. It is
/// [`resolve_verb`] where no grant is narrowed (ADR-0027 d5).
pub fn resolve(
    key_role: Role,
    effective: &BTreeSet<Role>,
    needed: Role,
) -> Result<(), AccessRefusal> {
    refusal_order(key_role, effective, None, Verb::Needs(needed)).map_err(|refusal| match refusal {
        VerbRefusal::VerbCeiling { role, needed } => AccessRefusal::VerbCeiling { role, needed },
        VerbRefusal::GrantRefused { needed_role, .. } => {
            AccessRefusal::GrantRefused { needed_role }
        }
        VerbRefusal::NotFound => AccessRefusal::NotFound,
    })
}

/// The verbs each effective grant role confers, in table order
/// (roles-and-keys.md, effective verbs; ADR-0025 d14).
const VERBS_CONFERRED: [(Role, &[&str]); 3] = [
    (
        Role::Reader,
        &["recall", "read", "graph_read", "events_tail"],
    ),
    (Role::Writer, &["remember", "learn", "attach", "supersede"]),
    (Role::Dreamer, &["propose", "review", "supersede"]),
];

/// `my_effective_verbs` (roles-and-keys.md, effective verbs; ADR-0025 d14):
/// the union of the verbs the `effective` roles confer, in table order
/// without duplicates, and without `review` for a `held_by: service` key,
/// which is refused every review-class verb. It states the access
/// conferred, not what a server build serves.
#[must_use]
pub fn effective_verbs(effective: &BTreeSet<Role>, held_by: HeldBy) -> Vec<&'static str> {
    let mut verbs: Vec<&'static str> = Vec::new();
    for (role, conferred) in VERBS_CONFERRED {
        if !effective.contains(&role) {
            continue;
        }
        for &verb in conferred {
            if verb == "review" && held_by == HeldBy::Service {
                continue;
            }
            if !verbs.contains(&verb) {
                verbs.push(verb);
            }
        }
    }
    verbs
}

// ---- Narrowed dreamer grants: `edge_labels` (ADR-0027 d5) ----
//
// Kept beside `Grant`, `effective_roles`, `resolve` and `AccessRefusal`
// rather than inside them, so that their callers are unchanged, and computed
// by the same steps: `effective_roles` and `resolve` are `effective_access`
// and `resolve_verb` where no grant is narrowed, which keeps ADR-0008's one
// access computation. A narrowed grant never reaches them, since
// `LabelledGrant::unnarrowed` is `None` for it: a caller still on the plain
// functions refuses a narrowed dreamer's verbs instead of conferring the
// whole dreamer row.

/// A refused `edge_labels` on a grant request (roles-and-keys.md, narrowed
/// dreamer grants; ADR-0027 d5). Every variant answers `request.validation`,
/// checked with the rest of the body before step 0 of the issuance order.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EdgeLabelsRefusal {
    /// `edge_labels` on a grant whose role is not `dreamer`.
    #[error(
        "edge_labels is admitted on a dreamer grant only, not on {role:?} (request.validation)"
    )]
    NotDreamer { role: Role },
    /// An empty list: an absent `edge_labels` is the unnarrowed grant.
    #[error("edge_labels is a non-empty list (request.validation)")]
    Empty,
    /// A label that is not one of the `oml:` upper vocabulary's.
    #[error("{label} is not a label of the oml: upper vocabulary (request.validation)")]
    OutsideOml { label: String },
    /// A label the list already holds.
    #[error("{label} is listed twice in edge_labels (request.validation)")]
    Repeated { label: String },
}

/// One grant with its `edge_labels` attribute: absent, the grant means what
/// [`Grant`] says; present, a dreamer grant narrowed to claim edges carrying
/// a listed label — a second attenuation inside the grant, beside its deny
/// masks (roles-and-keys.md, narrowed dreamer grants; ADR-0027 d5). Built
/// only through [`LabelledGrant::new`] or from a [`Grant`], so a narrowed
/// grant is always a dreamer grant with a non-empty set of `oml:` labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelledGrant {
    grant: Grant,
    edge_labels: Option<BTreeSet<OmlLabel>>,
}

impl LabelledGrant {
    /// `grant` with the `edge_labels` a request or a stored grant row
    /// carries, refusing a non-dreamer grant first, then an empty list, then
    /// each label in list order: one outside `oml:`, then one listed before.
    // reading: the docs name the four refusals but not their order; the
    // grant's role is read before the list, and the list in its own order.
    pub fn new(grant: Grant, edge_labels: Option<&[&str]>) -> Result<Self, EdgeLabelsRefusal> {
        let Some(list) = edge_labels else {
            return Ok(Self::from(grant));
        };
        if grant.role != Role::Dreamer {
            return Err(EdgeLabelsRefusal::NotDreamer { role: grant.role });
        }
        if list.is_empty() {
            return Err(EdgeLabelsRefusal::Empty);
        }
        let mut set = BTreeSet::new();
        for &text in list {
            let label = OmlLabel::parse(text).ok_or_else(|| EdgeLabelsRefusal::OutsideOml {
                label: text.to_owned(),
            })?;
            if !set.insert(label) {
                return Err(EdgeLabelsRefusal::Repeated {
                    label: text.to_owned(),
                });
            }
        }
        Ok(Self {
            grant,
            edge_labels: Some(set),
        })
    }

    /// The grant as [`Grant`] says it, for [`effective_roles`], [`resolve`]
    /// and [`TailCaller`], where it is not narrowed; `None` for a narrowed
    /// grant, which those cannot read without dropping its `edge_labels`.
    /// A narrowed grant is always a dreamer grant, so leaving it out fails
    /// closed: where it is the only dreamer grant, they refuse every
    /// dreamer-class verb, the claim edges its labels list included, and they
    /// confer every other role as [`effective_access`] does
    /// (roles-and-keys.md, narrowed dreamer grants; ADR-0027 d5).
    #[must_use]
    pub fn unnarrowed(&self) -> Option<&Grant> {
        self.edge_labels.is_none().then_some(&self.grant)
    }

    /// The narrowed set, in the register's order; `None` for a grant that
    /// is not narrowed.
    #[must_use]
    pub fn edge_labels(&self) -> Option<&BTreeSet<OmlLabel>> {
        self.edge_labels.as_ref()
    }

    /// Whether this grant is narrowed to exactly `oml:about`, the shape the
    /// member-profile table names (ADR-0027 d3).
    fn about_only(&self) -> bool {
        self.edge_labels
            .as_ref()
            .is_some_and(|set| set.len() == 1 && set.contains(&OmlLabel::About))
    }
}

impl From<Grant> for LabelledGrant {
    /// A grant without `edge_labels`, whatever its role.
    fn from(grant: Grant) -> Self {
        Self {
            grant,
            edge_labels: None,
        }
    }
}

/// Effective access at one node once a grant may be narrowed
/// (roles-and-keys.md, narrowed dreamer grants; workareas.md, effective
/// access; ADR-0027 d5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectiveAccess {
    /// `effective_roles(N)`, unchanged by narrowing.
    pub roles: BTreeSet<Role>,
    /// Where dreamer is **narrowed** at N — every dreamer grant surviving
    /// there carries `edge_labels` — the union of their lists; `None` where
    /// dreamer is unnarrowed or absent.
    pub edge_labels: Option<BTreeSet<OmlLabel>>,
}

/// Effective access at the node whose ancestry, from the node to the root,
/// is `ancestry` (roles-and-keys.md, narrowed dreamer grants; ADR-0027 d5).
/// `roles` is computed as [`effective_roles`] computes it, narrowed grants
/// included: narrowing cuts verbs out of a grant, never its role. Of the
/// dreamer grants that survive at N — on N or an ancestor, not masked at N by their
/// own deny list — dreamer is unnarrowed if any carries no `edge_labels`, and
/// otherwise narrowed to the union of their lists. A narrowed grant
/// attenuates only itself, so an unnarrowed grant beside it widens it, and a
/// global denial covering N empties every role.
#[must_use]
pub fn effective_access(
    grants: &[LabelledGrant],
    denials: &[KeyDenial],
    ancestry: &[Uuid],
) -> EffectiveAccess {
    if globally_denied(denials, ancestry) {
        return EffectiveAccess::default();
    }
    let surviving: Vec<&LabelledGrant> = grants
        .iter()
        .filter(|g| survives(&g.grant, ancestry))
        .collect();
    let roles = surviving.iter().map(|g| g.grant.role).collect();
    let dreamer: Vec<&LabelledGrant> = surviving
        .into_iter()
        .filter(|g| g.grant.role == Role::Dreamer)
        .collect();
    let edge_labels = if dreamer.is_empty() || dreamer.iter().any(|g| g.edge_labels.is_none()) {
        None
    } else {
        Some(
            dreamer
                .iter()
                .filter_map(|g| g.edge_labels.as_ref())
                .flatten()
                .copied()
                .collect(),
        )
    };
    EffectiveAccess { roles, edge_labels }
}

/// `my_edge_labels` on the workarea read (roles-and-keys.md, effective
/// verbs; ADR-0027 d5): the narrowed set as `oml:` CURIEs in the register's
/// order where dreamer is narrowed, `None` — served null — where it is
/// unnarrowed or absent. A narrowed dreamer still contributes `propose` and
/// `review` to [`effective_verbs`] over `access.roles`.
#[must_use]
pub fn my_edge_labels(access: &EffectiveAccess) -> Option<Vec<&'static str>> {
    access
        .edge_labels
        .as_ref()
        .map(|set| set.iter().map(|label| label.as_str()).collect())
}

/// A workarea-scoped verb as the refusal order reads it once a dreamer may
/// be narrowed (roles-and-keys.md, refusal order step 2; ADR-0027 d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb<'a> {
    /// Any verb but a claim-edge act, needing this grant role. For dreamer:
    /// a serve, a verdict, a reopen, edit-as-proposal, a workarea-edge act, a
    /// status-transition act or a dead-letter re-enqueue.
    Needs(Role),
    /// Proposing, ratifying or rejecting a claim edge with this label, which
    /// needs dreamer. Claim-edge labels are open, so any label is taken; a
    /// narrowed set lists `oml:` labels only.
    ClaimEdge(&'a str),
}

impl Verb<'_> {
    fn needed_role(self) -> Role {
        match self {
            Verb::Needs(role) => role,
            Verb::ClaimEdge(_) => Role::Dreamer,
        }
    }
}

/// A refused workarea-scoped verb, by step of the refusal order, once a
/// dreamer may be narrowed (roles-and-keys.md; errors.md `access`). It is
/// [`AccessRefusal`] with the `edge_labels` detail of step 3.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerbRefusal {
    /// Step 1, `access.verb_ceiling`.
    #[error(
        "a {role:?} key never holds {needed:?} on any node (roles-and-keys.md refusal order step 1, access.verb_ceiling)"
    )]
    VerbCeiling { role: Role, needed: Role },
    /// Step 3, `access.grant_refused`. `edge_labels` is the narrowed set at
    /// the node where dreamer is narrowed there, else `None`. The caller
    /// adds the `workarea_id` detail.
    #[error(
        "the node is readable but effective access does not confer {needed_role:?} for this verb (roles-and-keys.md refusal order step 3, access.grant_refused)"
    )]
    GrantRefused {
        needed_role: Role,
        edge_labels: Option<BTreeSet<OmlLabel>>,
    },
    /// Step 4, `access.not_found`.
    #[error("not found (roles-and-keys.md refusal order step 4, access.not_found)")]
    NotFound,
}

/// The refusal order for `verb` on a node whose [`effective_access`] is
/// `access` (roles-and-keys.md; ADR-0025 d1; ADR-0027 d5): verb ceiling;
/// allowed where the needed role is effective and, for a narrowed dreamer,
/// only for a claim-edge act whose label the narrowed set lists; grant
/// refused, naming the narrowed set, where reader is effective; else not
/// found. Where nothing is narrowed it answers as [`resolve`] does.
pub fn resolve_verb(
    key_role: Role,
    access: &EffectiveAccess,
    verb: Verb<'_>,
) -> Result<(), VerbRefusal> {
    refusal_order(key_role, &access.roles, access.edge_labels.as_ref(), verb)
}

/// The one refusal order [`resolve`] and [`resolve_verb`] answer:
/// `edge_labels` is the narrowed set at the node, `None` where dreamer is
/// unnarrowed or absent (roles-and-keys.md; ADR-0025 d1; ADR-0027 d5).
fn refusal_order(
    key_role: Role,
    roles: &BTreeSet<Role>,
    edge_labels: Option<&BTreeSet<OmlLabel>>,
    verb: Verb<'_>,
) -> Result<(), VerbRefusal> {
    let needed = verb.needed_role();
    if !admissible_grant_roles(key_role).contains(&needed) {
        return Err(VerbRefusal::VerbCeiling {
            role: key_role,
            needed,
        });
    }
    let narrowing_admits = match (edge_labels, verb) {
        (Some(listed), Verb::ClaimEdge(label)) => {
            OmlLabel::parse(label).is_some_and(|l| listed.contains(&l))
        }
        (Some(_), Verb::Needs(role)) => role != Role::Dreamer,
        (None, _) => true,
    };
    if roles.contains(&needed) && narrowing_admits {
        Ok(())
    } else if roles.contains(&Role::Reader) {
        Err(VerbRefusal::GrantRefused {
            needed_role: needed,
            edge_labels: edge_labels.cloned(),
        })
    } else {
        Err(VerbRefusal::NotFound)
    }
}

// ---- Member profiles (ADR-0027 d1, d3, d4) ----

/// A crew record's kind, given at creation and never changed; an absent
/// kind, and every crew recorded before ADR-0027, is `general`
/// (workareas.md, crews; ADR-0027 d1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CrewKind {
    General,
    Ontology,
}

impl CrewKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CrewKind::General => "general",
            CrewKind::Ontology => "ontology",
        }
    }
}

/// The member profile a key naming an ontology crew carries: a fixed grant
/// shape over one of the five roles, never a role (roles-and-keys.md,
/// member profiles; ADR-0027 d3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberProfile {
    Classifier,
    TermCreator,
    Curator,
}

impl MemberProfile {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MemberProfile::Classifier => "classifier",
            MemberProfile::TermCreator => "term_creator",
            MemberProfile::Curator => "curator",
        }
    }

    /// The key role the profile's row names.
    #[must_use]
    pub fn key_role(self) -> Role {
        match self {
            MemberProfile::Classifier | MemberProfile::Curator => Role::Dreamer,
            MemberProfile::TermCreator => Role::Writer,
        }
    }
}

/// The clause of step 7 that refused, in its checking order
/// (roles-and-keys.md, member profiles; ADR-0027 d4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileClause {
    CrewKind,
    ProfileRequired,
    KeyRole,
    HeldBy,
    Grant,
    RequiredGrant,
}

impl ProfileClause {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileClause::CrewKind => "crew_kind",
            ProfileClause::ProfileRequired => "profile_required",
            ProfileClause::KeyRole => "key_role",
            ProfileClause::HeldBy => "held_by",
            ProfileClause::Grant => "grant",
            ProfileClause::RequiredGrant => "required_grant",
        }
    }
}

/// `access.profile_mismatch` (403), `details` `{profile, clause,
/// grant_index}`: `profile` as sent, `grant_index` the first offending
/// grant's 0-based index for the `grant` clause and `None` for every other
/// (roles-and-keys.md, member profiles; ADR-0027 d4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "the key does not match its member profile on {clause:?} (roles-and-keys.md member profiles, access.profile_mismatch)"
)]
pub struct ProfileMismatch {
    pub profile: Option<MemberProfile>,
    pub clause: ProfileClause,
    pub grant_index: Option<usize>,
}

/// The cell of a profile's row a grant falls in (roles-and-keys.md, member
/// profiles). `None` is a grant the row names nowhere, which no profile
/// admits either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Must,
    May,
    Never,
}

// reading: `edge_labels: ["oml:about"]` in the table is read as exactly
// that list, so a dreamer grant narrowed to `oml:about` and another label
// lies in no cell; deny masks never move a grant between cells.
fn cell(profile: MemberProfile, grant: &LabelledGrant, on_vocabulary: bool) -> Option<Cell> {
    let narrowed = grant.edge_labels.is_some();
    match (profile, grant.grant.role) {
        (MemberProfile::Classifier, Role::Reader) | (MemberProfile::Curator, Role::Reader) => {
            Some(Cell::May)
        }
        (MemberProfile::Classifier, Role::Dreamer) => {
            if !narrowed || !on_vocabulary {
                Some(Cell::Never)
            } else if grant.about_only() {
                Some(Cell::Must)
            } else {
                None
            }
        }
        (MemberProfile::TermCreator, _) if !on_vocabulary => Some(Cell::Never),
        (MemberProfile::TermCreator, Role::Writer) => Some(Cell::Must),
        (MemberProfile::TermCreator, Role::Reader) => Some(Cell::May),
        (MemberProfile::Curator, Role::Dreamer) => match (narrowed, on_vocabulary) {
            (false, true) => Some(Cell::Must),
            (true, false) if grant.about_only() => Some(Cell::May),
            (true, false) => None,
            (false, false) | (true, true) => Some(Cell::Never),
        },
        _ => None,
    }
}

/// The `grant` clause: the first grant outside the row's "must" and "may"
/// cells, or within "never", answers with its 0-based index.
fn check_profile_grants(
    profile: MemberProfile,
    grants: &[LabelledGrant],
    is_vocabulary_node: &impl Fn(Uuid) -> bool,
) -> Result<(), ProfileMismatch> {
    let offending = grants.iter().position(|g| {
        !matches!(
            cell(profile, g, is_vocabulary_node(g.grant.node)),
            Some(Cell::Must | Cell::May)
        )
    });
    match offending {
        Some(index) => Err(ProfileMismatch {
            profile: Some(profile),
            clause: ProfileClause::Grant,
            grant_index: Some(index),
        }),
        None => Ok(()),
    }
}

/// Step 7 of the issuance order (roles-and-keys.md, member profiles;
/// ADR-0027 d3, d4): when the key names a crew of kind `ontology`, or the
/// body carries a profile, the clauses in order — the crew is of kind
/// `ontology`, a profile is sent, the key role is the profile's, `held_by`
/// is `agent` or `human`, every grant lies within the row's "must" or "may"
/// cells and none within "never", and a "must" grant is present — the first
/// failure answering. A key naming a `general` crew, or none, without a
/// profile is not checked. `crew` is the named crew's kind, `None` for no
/// crew; `is_vocabulary_node` says whether a grant's node lies in a
/// vocabulary workarea's subtree. Deny masks and global denials only narrow,
/// so neither is read.
// reading: a profile sent without a crew is `request.validation` before
// step 0; reaching this step regardless, it fails `crew_kind`, since no
// crew is of kind `ontology`.
// reading: `held_by` is read as the attestation the body sent, so an absent
// one — journalled null, stored as the schema default — fails `held_by`:
// the journal must prove the attestation the profile requires.
pub fn check_member_profile(
    crew: Option<CrewKind>,
    profile: Option<MemberProfile>,
    key_role: Role,
    held_by: Option<HeldBy>,
    grants: &[LabelledGrant],
    is_vocabulary_node: impl Fn(Uuid) -> bool,
) -> Result<(), ProfileMismatch> {
    let ontology = crew == Some(CrewKind::Ontology);
    if !ontology && profile.is_none() {
        return Ok(());
    }
    let refuse = |clause| {
        Err(ProfileMismatch {
            profile,
            clause,
            grant_index: None,
        })
    };
    if !ontology {
        return refuse(ProfileClause::CrewKind);
    }
    let Some(profile) = profile else {
        return refuse(ProfileClause::ProfileRequired);
    };
    if key_role != profile.key_role() {
        return refuse(ProfileClause::KeyRole);
    }
    if !matches!(held_by, Some(HeldBy::Agent | HeldBy::Human)) {
        return refuse(ProfileClause::HeldBy);
    }
    check_profile_grants(profile, grants, &is_vocabulary_node)?;
    let required = grants
        .iter()
        .any(|g| cell(profile, g, is_vocabulary_node(g.grant.node)) == Some(Cell::Must));
    if !required {
        return refuse(ProfileClause::RequiredGrant);
    }
    Ok(())
}

/// A grant added to a profiled key (`POST /v2/keys/{key_id}/grants`),
/// checked against `grants` — the key's grants and the new one together —
/// and refused with clause `grant`, journalling nothing (roles-and-keys.md,
/// member profiles; ADR-0027 d4).
// reading: only the `grant` clause is re-run. The crew kind, the profile,
// the key role and `held_by` are fixed for the key's life, and
// `required_grant` is not re-checked, since a revocation may leave a
// profiled key short of its required grant and adding authority within
// the row must not then be refused.
pub fn check_profile_amendment(
    profile: MemberProfile,
    grants: &[LabelledGrant],
    is_vocabulary_node: impl Fn(Uuid) -> bool,
) -> Result<(), ProfileMismatch> {
    check_profile_grants(profile, grants, &is_vocabulary_node)
}

/// What the journal up to an event says about its subject, as the content
/// tail judges it (http-api.md#events-tail; ADR-0025 d15).
#[derive(Debug, Clone, Copy)]
pub struct TailSubject<'a> {
    /// The subject's workareas as the subject table lists them, each as its
    /// ancestry from the node to the root: one for most types; for
    /// `lifecycle.reassign`, `from_workarea_id` and, when `re_enters_held` is
    /// false, `to_workarea_id`; for `workarea.relate_*`, both endpoints. A
    /// count the event type does not take hides the event.
    pub workareas: &'a [Vec<Uuid>],
    /// The subject version's authoring key, as its submission event's
    /// envelope `api_key_id` names it; `None` for a `workarea.*` subject.
    pub author_key: Option<Uuid>,
    /// The reader clause's *promoted* condition: in the journal up to and
    /// including the event — before it, for `lifecycle.reassign` — the
    /// version's latest transition between held and promoted is a promotion.
    pub promoted: bool,
    /// The subject is `self_signed`, so the reader clause shows it only on a
    /// tail whose `workarea_id` is the subject's own workarea.
    pub self_signed: bool,
    /// The subject is a simulation row or sim-derived version outside the
    /// calling key's audience: no clause shows it.
    pub outside_audience: bool,
}

/// The tailing key: its own key set (roles-and-keys.md#own), its grants and
/// global denials, and the tail's `workarea_id`.
#[derive(Debug, Clone, Copy)]
pub struct TailCaller<'a> {
    pub own_keys: &'a [Uuid],
    pub grants: &'a [Grant],
    pub denials: &'a [KeyDenial],
    pub scope: Uuid,
}

impl TailCaller<'_> {
    fn reads(&self, ancestry: &[Uuid]) -> bool {
        effective_roles(self.grants, self.denials, ancestry).contains(&Role::Reader)
    }

    fn within(&self, ancestry: &[Uuid]) -> bool {
        ancestry.contains(&self.scope)
    }

    fn denied(&self, ancestry: &[Uuid]) -> bool {
        self.denials.iter().any(|d| ancestry.contains(&d.node))
    }

    /// The reader clause (a): reader on a listed workarea within the tail's
    /// subtree, promoted, inside the audience, and a self-signed subject
    /// only on a tail scoped to that same workarea.
    fn reader_clause(&self, subject: &TailSubject<'_>) -> bool {
        subject.promoted
            && !subject.outside_audience
            && subject.workareas.iter().any(|w| {
                self.reads(w)
                    && self.within(w)
                    && (!subject.self_signed || w.first() == Some(&self.scope))
            })
    }

    /// The author clause (b), and the whole rule for the submission family:
    /// the authoring key is in the caller's own key set, a listed workarea
    /// lies within the tail's subtree minus the global denials, and the
    /// subject is inside the audience. No reader grant is needed.
    fn author_clause(&self, subject: &TailSubject<'_>) -> bool {
        subject
            .author_key
            .is_some_and(|k| self.own_keys.contains(&k))
            && !subject.outside_audience
            && subject
                .workareas
                .iter()
                .any(|w| self.within(w) && !self.denied(w))
    }
}

/// Whether the content tail shows an event of `event_type` to `caller`
/// (http-api.md#events-tail, the single declaration site; ADR-0025 d15).
/// `submission.*` and `simulation.write` reach only the author;
/// `lifecycle.*` reaches a reader of a promoted subject or its author;
/// `workarea.*` reaches readers of the node, or of both endpoints with one
/// inside the tail's subtree. `review.*`, `claim_edge.*`, `key.*`,
/// `grant.*`, `tenant.*`, `system.*` and every type the tables do not map
/// are never shown.
#[must_use]
pub fn tail_visible(event_type: &str, subject: &TailSubject<'_>, caller: &TailCaller<'_>) -> bool {
    let count = subject.workareas.len();
    match event_type {
        "submission.remember" | "submission.learn" | "submission.attach" | "simulation.write" => {
            count == 1 && caller.author_clause(subject)
        }
        "lifecycle.promote"
        | "lifecycle.demote"
        | "lifecycle.erase"
        | "lifecycle.supersede"
        | "lifecycle.tip_override" => {
            count == 1 && (caller.reader_clause(subject) || caller.author_clause(subject))
        }
        "lifecycle.reassign" => {
            (1..=2).contains(&count)
                && (caller.reader_clause(subject) || caller.author_clause(subject))
        }
        "workarea.create"
        | "workarea.reparent"
        | "workarea.status_proposed"
        | "workarea.status_transition"
        | "workarea.crew_updated" => {
            count == 1
                && subject
                    .workareas
                    .iter()
                    .all(|w| caller.reads(w) && caller.within(w))
        }
        "workarea.relate_proposed" | "workarea.relate_ratified" => {
            count == 2
                && subject.workareas.iter().all(|w| caller.reads(w))
                && subject.workareas.iter().any(|w| caller.within(w))
        }
        _ => false,
    }
}
