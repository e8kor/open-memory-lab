//! Vocabulary releases: the snapshot a `vocab_release` ratification
//! materializes — the term tips, the ratified `is_a` and `replaced_by` edges
//! and the `is_a` closure — as one RFC 8785 canonical document with its
//! SHA-256 digest, and the diff between two releases that a release's serve
//! shows its reviewer — pure functions over tips and edges the caller has
//! already read (ADR-0021 d4, d5).
//!
//! The rules are declared on `docs/architecture/ontologies.md` ("Releases
//! and the materialized closure"), `docs/data/postgres-schema.md`
//! (`vocab_closure`) and, for the canonical form and the digest's spelling,
//! `docs/data/chain-format.md`; this module mirrors them and adds nothing.
//!
//! Each refusal answers exactly one code of `docs/api/errors.md`:
//!
//! | Refusal | Code |
//! |---|---|
//! | [`ReleaseRefusal::Cycle`] | `vocab.cycle_refused` |
//! | [`ReleaseRefusal::TwoTips`] | none declared — see the variant |
//! | [`ReleaseDiffRefusal`], every variant | none declared — see the enum |

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::envelope::Sha256Hash;
use crate::jcs::canonical_bytes;
use crate::vocab::{Curie, OmlLabel, Successors};
use crate::vocab_edges::{ClosureRow, CycleRefused, TermEdge, TermTip, TipState};

/// A row of a release's materialized `is_a` closure as `vocab_closure`
/// stores it: `descendant` lies `depth` ratified `is_a` edges below
/// `ancestor` (ADR-0021 d4; postgres-schema.md).
// reading: `vocab_closure` keys one row per (ancestor, descendant) and names
// a depth without saying which path it measures; it is the shortest, so a
// depth-1 row is exactly a ratified edge and the hierarchy's `parents` and
// `children` (openapi.yaml `getTermHierarchy`) are depth-1 lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReleaseClosureRow {
    pub ancestor: Curie,
    pub descendant: Curie,
    pub depth: u32,
}

/// A refused release snapshot.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReleaseRefusal {
    /// `vocab.cycle_refused`: the ratified edges of one label close a cycle,
    /// which the invariant at each edge's ratification refuses (ADR-0021 d5;
    /// [`crate::vocab_edges::check_acyclic`]).
    #[error(transparent)]
    Cycle(#[from] CycleRefused),
    /// Two tips name one CURIE, where a term has one served tip
    /// (audit-and-lineage.md, "Supersession chains, claim_key and the tip").
    // reading: the docs declare no code for an input the read model cannot
    // produce; this refusal names none, and a caller must not invent one.
    #[error("{curie} has two tips in one release's input (no declared code)")]
    TwoTips { curie: Curie },
}

/// The materialized state of one vocabulary release: each term's tip state,
/// the ratified `is_a` and `replaced_by` edges, the `is_a` closure, and the
/// canonical document and digest over the three (ADR-0021 d4).
///
/// The document is the RFC 8785 form (chain-format.md, "signed_bytes") of
///
/// ```text
/// {"closure": [{"ancestor", "depth", "descendant"}, …],
///  "edges":   [{"from", "label", "to"}, …],
///  "terms":   [{"curie", "successors": [{"curie", "relation"}, …], "term_status"}, …]}
/// ```
///
/// with the field names of `vocab_closure` (postgres-schema.md), of a term
/// edge, and of openapi.yaml's `VocabTerm`. Each array is in code-point order
/// of its members' strings: closure rows by `ancestor` then `descendant`,
/// edges by `label`, `from` then `to`, terms by `curie`, and a `consider`
/// term's successors by `curie`; an active term's `successors` is `[]`. The
/// digest is SHA-256 of those bytes, spelled `sha256:` + 64 lowercase hex
/// ([`Sha256Hash`]).
// reading: ADR-0021 d4 pins a release by a Merkle root over the active term
// set, whose construction is RS-4's and still open (roadmap.md); this digest
// is not that root. It covers what the ratification materializes, and no
// term version or plaintext digest, which the Merkle leaf adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseSnapshot {
    terms: BTreeMap<Curie, TipState>,
    edges: BTreeSet<TermEdge>,
    closure: BTreeSet<ReleaseClosureRow>,
    document: Vec<u8>,
    digest: Sha256Hash,
}

impl ReleaseSnapshot {
    /// Each term's tip state, by CURIE; a `consider` list sorted and once
    /// each.
    #[must_use]
    pub fn terms(&self) -> &BTreeMap<Curie, TipState> {
        &self.terms
    }

    /// The release's ratified `is_a` and `replaced_by` edges, once each.
    #[must_use]
    pub fn edges(&self) -> &BTreeSet<TermEdge> {
        &self.edges
    }

    /// The materialized `is_a` closure, one row per pair: the transitive
    /// closure, so no term lies below itself (ADR-0021 d4, d10).
    #[must_use]
    pub fn closure(&self) -> &BTreeSet<ReleaseClosureRow> {
        &self.closure
    }

    /// The closure as the rows [`crate::vocab_edges::expand_terms`]
    /// consumes, in the closure's order.
    #[must_use]
    pub fn closure_rows(&self) -> Vec<ClosureRow> {
        self.closure
            .iter()
            .map(|row| ClosureRow {
                ancestor: row.ancestor.clone(),
                descendant: row.descendant.clone(),
            })
            .collect()
    }

    /// The release's canonical document, RFC 8785 bytes.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.document
    }

    /// SHA-256 of [`Self::canonical_bytes`].
    #[must_use]
    pub fn digest(&self) -> Sha256Hash {
        self.digest
    }
}

/// Builds the snapshot a `vocab_release` ratification materializes from the
/// term tips and the ratified edges (ADR-0021 d4): `edges` may hold edges of
/// any label, of which `is_a` and `replaced_by` enter the release, a CURIE
/// pair once however many version pairs carry it. Refuses two tips for one
/// CURIE, then an `is_a` cycle, then a `replaced_by` cycle (ADR-0021 d5).
/// Neither the order of `tips` nor that of `edges` reaches the snapshot.
// reading: the docs do not confine an `is_a` or `replaced_by` edge to one
// vocabulary, so the snapshot covers exactly the edges the caller read and
// asks no endpoint to be among the tips; and `consider` names one or more
// successors with no order, so the snapshot holds them sorted and once each.
pub fn build_release(
    tips: &[TermTip],
    edges: &[TermEdge],
) -> Result<ReleaseSnapshot, ReleaseRefusal> {
    let mut terms = BTreeMap::new();
    for tip in tips {
        let state = match &tip.state {
            TipState::Deprecated(Successors::Consider(curies)) => {
                let once: BTreeSet<&Curie> = curies.iter().collect();
                TipState::Deprecated(Successors::Consider(once.into_iter().cloned().collect()))
            }
            state => state.clone(),
        };
        if terms.insert(tip.curie.clone(), state).is_some() {
            return Err(ReleaseRefusal::TwoTips {
                curie: tip.curie.clone(),
            });
        }
    }
    let edges: BTreeSet<TermEdge> = edges
        .iter()
        .filter(|e| matches!(e.label, OmlLabel::IsA | OmlLabel::ReplacedBy))
        .cloned()
        .collect();
    let closure = closure_over(&edges, OmlLabel::IsA)?;
    closure_over(&edges, OmlLabel::ReplacedBy)?;
    let document = document(&terms, &edges, &closure);
    let digest = Sha256Hash::digest(&document);
    Ok(ReleaseSnapshot {
        terms,
        edges,
        closure,
        document,
        digest,
    })
}

/// The transitive closure of the `label` edges among `edges`, each row at
/// the fewest edges joining its pair, refusing a cycle (a self-edge among
/// them).
fn closure_over(
    edges: &BTreeSet<TermEdge>,
    label: OmlLabel,
) -> Result<BTreeSet<ReleaseClosureRow>, CycleRefused> {
    let mut up: BTreeMap<&Curie, Vec<&Curie>> = BTreeMap::new();
    for edge in edges.iter().filter(|e| e.label == label) {
        up.entry(&edge.from).or_default().push(&edge.to);
    }
    let mut rows = BTreeSet::new();
    // Every term on a cycle has an edge out, so a walk from each source
    // meets every cycle; breadth first, each term is met at its fewest
    // edges.
    for &descendant in up.keys() {
        let mut met: BTreeSet<&Curie> = BTreeSet::new();
        let mut frontier = vec![descendant];
        let mut depth = 0;
        while !frontier.is_empty() {
            depth += 1;
            let mut next = Vec::new();
            for node in frontier {
                for &ancestor in up.get(node).into_iter().flatten() {
                    if ancestor == descendant {
                        return Err(CycleRefused { label });
                    }
                    if met.insert(ancestor) {
                        next.push(ancestor);
                        rows.insert(ReleaseClosureRow {
                            ancestor: ancestor.clone(),
                            descendant: descendant.clone(),
                            depth,
                        });
                    }
                }
            }
            frontier = next;
        }
    }
    Ok(rows)
}

/// The canonical document of [`ReleaseSnapshot`]. The ordered collections
/// give each array its order; the edges hold only `oml:is_a` and
/// `oml:replaced_by`, whose label order is their strings'.
fn document(
    terms: &BTreeMap<Curie, TipState>,
    edges: &BTreeSet<TermEdge>,
    closure: &BTreeSet<ReleaseClosureRow>,
) -> Vec<u8> {
    let terms: Vec<Value> = terms
        .iter()
        .map(|(curie, state)| {
            let successors: Vec<Value> = match state {
                TipState::Active => Vec::new(),
                TipState::Deprecated(successors) => successors
                    .curies()
                    .iter()
                    .map(|c| json!({"curie": c.as_str(), "relation": successors.relation().as_str()}))
                    .collect(),
            };
            json!({
                "curie": curie.as_str(),
                "successors": successors,
                "term_status": state.status().as_str(),
            })
        })
        .collect();
    let edges: Vec<Value> = edges
        .iter()
        .map(|e| json!({"from": e.from.as_str(), "label": e.label.as_str(), "to": e.to.as_str()}))
        .collect();
    let closure: Vec<Value> = closure
        .iter()
        .map(|row| {
            json!({
                "ancestor": row.ancestor.as_str(),
                "depth": row.depth,
                "descendant": row.descendant.as_str(),
            })
        })
        .collect();
    let value = json!({"closure": closure, "edges": edges, "terms": terms});
    // JCS here refuses only a non-integer or an integer at or above 2^53
    // (chain-format.md number rule); the document holds strings and u32
    // depths, so it cannot refuse.
    canonical_bytes(&value).expect("strings and u32 depths canonicalize")
}

/// The diff a `vocab_release`'s serve shows against the previous release:
/// terms added, deprecations with their successors, `is_a` and
/// `replaced_by` edges added and removed, and closure rows added and removed
/// (ADR-0021 d4; ontologies.md). Every collection is ordered, so one diff
/// computed twice is equal value for value, and d4's counts are their
/// lengths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleaseDiff {
    /// Terms the next release holds and the previous does not, each with
    /// its tip state — deprecated, with its successors, when it was first
    /// released so.
    pub terms_added: BTreeMap<Curie, TipState>,
    /// Terms of both releases whose next tip is a deprecation the previous
    /// did not hold: an active term deprecated, or a deprecated term's
    /// successors changed.
    pub deprecations: BTreeMap<Curie, Successors>,
    pub edges_added: BTreeSet<TermEdge>,
    pub edges_removed: BTreeSet<TermEdge>,
    /// Closure rows the next release holds and the previous does not; a
    /// pair whose depth changed is removed at the old depth and added at the
    /// new.
    pub closure_added: BTreeSet<ReleaseClosureRow>,
    pub closure_removed: BTreeSet<ReleaseClosureRow>,
}

impl ReleaseDiff {
    /// Whether the diff names no change.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A refused diff between two releases: the next release changes a term in
/// a way a diff does not serve, since a diff serves terms added and
/// deprecations (ADR-0021 d4).
// reading: the docs declare no code for either change, both of which the
// term lifecycle never makes; these refusals name none, and a caller must
// not invent one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReleaseDiffRefusal {
    /// The next release lacks a term the previous holds: a minted CURIE
    /// resolves forever (ADR-0021 d2).
    #[error("{curie} is in the previous release and not in the next (no declared code)")]
    TermDropped { curie: Curie },
    /// A term deprecated in the previous release is active in the next: the
    /// term lifecycle has no move from deprecated back to active
    /// (ontologies.md, "The term lifecycle").
    #[error(
        "{curie} is deprecated in the previous release and active in the next (no declared code)"
    )]
    TermReinstated { curie: Curie },
}

/// The diff from `previous` to `next` (ADR-0021 d4), refusing, at the first
/// of `previous`'s terms in CURIE order that meets one, a term `next` drops
/// or reinstates.
pub fn diff_releases(
    previous: &ReleaseSnapshot,
    next: &ReleaseSnapshot,
) -> Result<ReleaseDiff, ReleaseDiffRefusal> {
    for (curie, before) in &previous.terms {
        match (before, next.terms.get(curie)) {
            (_, None) => {
                return Err(ReleaseDiffRefusal::TermDropped {
                    curie: curie.clone(),
                });
            }
            (TipState::Deprecated(_), Some(TipState::Active)) => {
                return Err(ReleaseDiffRefusal::TermReinstated {
                    curie: curie.clone(),
                });
            }
            _ => {}
        }
    }
    let mut diff = ReleaseDiff::default();
    for (curie, after) in &next.terms {
        match (previous.terms.get(curie), after) {
            (None, _) => {
                diff.terms_added.insert(curie.clone(), after.clone());
            }
            (Some(before), TipState::Deprecated(successors)) if before != after => {
                diff.deprecations.insert(curie.clone(), successors.clone());
            }
            _ => {}
        }
    }
    diff.edges_added = next.edges.difference(&previous.edges).cloned().collect();
    diff.edges_removed = previous.edges.difference(&next.edges).cloned().collect();
    diff.closure_added = next
        .closure
        .difference(&previous.closure)
        .cloned()
        .collect();
    diff.closure_removed = previous
        .closure
        .difference(&next.closure)
        .cloned()
        .collect();
    Ok(diff)
}
