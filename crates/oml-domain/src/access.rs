//! Grant admissibility, effective access, the refusal order, effective
//! verbs and content-tail visibility — pure functions over journalled grant
//! and denial records, computed on every request with no cache
//! (ADR-0025 d1, d2, d14, d15; ADR-0019 d1).
//!
//! The tables are declared on `docs/architecture/roles-and-keys.md`
//! (grant admissibility and refusal order, effective verbs),
//! `docs/architecture/workareas.md` (effective access) and
//! `docs/api/http-api.md` (events tail); this module mirrors them and adds
//! nothing.

use std::collections::BTreeSet;

use uuid::Uuid;

use crate::model::{HeldBy, Role};

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

/// `effective_roles(N)` (workareas.md, effective access; ADR-0025 d1):
/// every grant on N or an ancestor of N, less each grant whose own deny list
/// names N or an ancestor of N, unioned; the empty set when any global
/// denial names N or an ancestor of N. `ancestry` runs from N to the root;
/// an empty ancestry — an unknown node — confers nothing.
#[must_use]
pub fn effective_roles(
    grants: &[Grant],
    denials: &[KeyDenial],
    ancestry: &[Uuid],
) -> BTreeSet<Role> {
    if denials.iter().any(|d| ancestry.contains(&d.node)) {
        return BTreeSet::new();
    }
    grants
        .iter()
        .filter(|g| ancestry.contains(&g.node))
        .filter(|g| !g.deny.iter().any(|d| ancestry.contains(d)))
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
/// [`admissible_grant_roles`] before it resolves the node.
pub fn resolve(
    key_role: Role,
    effective: &BTreeSet<Role>,
    needed: Role,
) -> Result<(), AccessRefusal> {
    if !admissible_grant_roles(key_role).contains(&needed) {
        return Err(AccessRefusal::VerbCeiling {
            role: key_role,
            needed,
        });
    }
    if effective.contains(&needed) {
        Ok(())
    } else if effective.contains(&Role::Reader) {
        Err(AccessRefusal::GrantRefused {
            needed_role: needed,
        })
    } else {
        Err(AccessRefusal::NotFound)
    }
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
