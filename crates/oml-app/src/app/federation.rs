//! Federated recall, phase 1 (ADR-0072): the tenant's `federation` policy
//! and a workarea's `accept_federated`, each a person's or an admin's
//! journalled act; and the plan a recall runs under its `sources` — the
//! candidates a key reads, the reference set whose statistics every hit is
//! scored with, the selection above the cap, and the widen hint. Selection
//! reads exact counts and names alone: no clock, no randomness and no model
//! (d9). Every name and count it serves is of a node the key reads (d6).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::federation::{
    CANDIDATES_DOCTOR, MAX_SOURCES_CEILING, SourceCounts, estimate, is_trusted_source,
    matches_at_least, order_by_estimate, rate, richer,
};
use oml_domain::in_progress::may_set_mode;
use oml_domain::name::{NamedKind, Reference};
use oml_domain::{MemoryKind, Role, event_types};

use super::{App, Caller, derived_path, require_role};
use crate::federation::{
    CountsScope, CutReport, DefaultSources, FederationPolicy, FederationPolicyChange,
    FederationRecord, FederationSettings, SearchedSource, SourceRefusal, Sources, SourcesReport,
    Via, Widen, WorkareaFederation,
};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer, StoreError};
use crate::search::{Fusion, MatchMode, Pipeline, RetrieverKind};
use crate::types::{AppError, Bearer, RecallRequest};

/// A key that met more than [`CANDIDATES_DOCTOR`] candidate sources, as
/// `GET /v2/federation` names it to a tenant admin (d12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WideKey {
    pub key_id: Uuid,
    pub candidates: usize,
}

/// What a recall searches under its `sources` (d4, d5, d7, d8): the
/// promoted nodes the pipeline ranks, the own held nodes it may add, the
/// reference set the statistics come from, and what the answer and its
/// event say of them.
#[derive(Debug, Default)]
pub(super) struct SourcePlan {
    /// The nodes whose promoted tips are ranked.
    pub promoted: Vec<Uuid>,
    /// Under a list, the listed nodes own held rows are confined to.
    pub held_within: Option<BTreeSet<Uuid>>,
    /// The reference set, under `wide` and a list.
    pub reference: Option<Vec<Uuid>>,
    /// The federated nodes searched: a hit on one is `via: federated`.
    pub federated: BTreeSet<Uuid>,
    pub report: Option<SourcesReport>,
    pub hint: Option<HintInput>,
    pub record: Option<FederationRecord>,
}

/// The counts the widen hint is decided from, read before the search and
/// applied once the answer's size is known (d8).
#[derive(Debug)]
pub(super) struct HintInput {
    words: usize,
    reference: SourceCounts,
    scope: SourceCounts,
    candidates: Vec<SourceCounts>,
}

impl HintInput {
    /// `search.widen` (d8, as amended by the phase 1 eval): a candidate at
    /// least twice as rich per readable row as the scope, with a kept word
    /// on two of its rows. The thin-answer rule is dropped: on the eval's
    /// scopes of 12 to 25 memories it fired on 84% of the queries the scope
    /// answered, past d11's 10% (ADR-0072 Phase 1 as built).
    pub(super) fn widen(&self) -> Option<Widen> {
        let scope_rate = rate(&self.scope, &self.reference);
        self.candidates
            .iter()
            .any(|c| richer(c, scope_rate, &self.reference))
            .then(|| Widen {
                candidates: self
                    .candidates
                    .iter()
                    .filter(|c| estimate(c, &self.reference) > 0.0)
                    .count(),
                matches_at_least: matches_at_least(self.words, &self.candidates),
            })
    }
}

/// A candidate source: a trusted node outside the scope the key reads.
#[derive(Debug, Clone)]
struct Candidate {
    id: Uuid,
    name: String,
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `GET /v2/federation`, admin by key role (d5): the tenant's policy,
    /// and the keys that met more than 1,000 candidate sources since the
    /// lead started, for doctor (d12).
    pub async fn federation_settings(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<(FederationSettings, Vec<WideKey>), AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "federation_settings")?;
        let policy = self.journal.federation_policy(admin.tenant_id).await?;
        Ok((
            FederationSettings { policy },
            self.wide_keys_of(admin.tenant_id),
        ))
    }

    /// `POST /v2/federation`, admin by key role (d5): each field sent
    /// replaces its value; journalled as `federation.policy_set` with the
    /// policy whole, the previous one and the admin key. A change that
    /// changes nothing journals nothing.
    pub async fn set_federation_policy(
        &self,
        bearer: impl Into<Bearer<'_>>,
        change: FederationPolicyChange,
    ) -> Result<(FederationSettings, Vec<WideKey>), AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "federation_policy")?;
        if change.max_sources.is_some_and(|m| m > MAX_SOURCES_CEILING) {
            return Err(AppError::SourcesInvalid {
                field: "/max_sources",
                message: format!("max_sources must be between 0 and {MAX_SOURCES_CEILING}"),
            });
        }
        let tenant_id = admin.tenant_id;
        let from = self.journal.federation_policy(tenant_id).await?;
        let policy = FederationPolicy {
            enabled: change.enabled.unwrap_or(from.enabled),
            default_sources: change.default_sources.unwrap_or(from.default_sources),
            max_sources: change.max_sources.unwrap_or(from.max_sources),
        };
        if policy != from {
            let prev = self.head_event(tenant_id).await?;
            let event = self.seal(
                prev.as_ref(),
                tenant_id,
                event_types::FEDERATION_POLICY_SET,
                &admin.principal,
                Some(admin.key_id),
                json!({
                    "policy": policy_json(&policy),
                    "from": policy_json(&from),
                    "by_key_id": admin.key_id.to_string(),
                }),
            )?;
            self.commit(event, MetadataFold::FederationPolicySet(policy), None)
                .await?;
        }
        Ok((FederationSettings { policy }, self.wide_keys_of(tenant_id)))
    }

    /// `POST /v2/workareas/{workarea}/federation` (d5): a dreamer key a
    /// person holds, with dreamer effective access on the node, or a tenant
    /// admin sets `accept_federated`; an agent's key is refused before any
    /// lookup. Journalled as `workarea.federation_set {workarea_id, field,
    /// value, from}`; setting the value the node holds journals nothing.
    pub async fn set_workarea_federation(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workarea: Reference,
        accept_federated: bool,
    ) -> Result<WorkareaFederation, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        let admin = key.role == Role::Admin;
        if !admin && !may_set_mode(key.role, key.held_by, key.profile) {
            return Err(AppError::FederationHumanOnly {
                role: key.role,
                held_by: key.held_by,
            });
        }
        let tenant_id = key.tenant_id;
        let workarea_id = self.resolve(key, NamedKind::Workarea, &workarea).await?;
        if admin {
            self.journal
                .workarea(tenant_id, workarea_id)
                .await?
                .ok_or(AppError::NotFound)?;
        } else {
            self.require_access(&caller, workarea_id, Role::Dreamer, "workarea_federation")
                .await?;
        }
        self.observer.workarea(workarea_id);
        let from = self
            .journal
            .federation_declining(tenant_id, &[workarea_id])
            .await?
            .is_empty();
        if from == accept_federated {
            return Ok(WorkareaFederation { accept_federated });
        }
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::WORKAREA_FEDERATION_SET,
            &key.principal,
            Some(key.key_id),
            json!({
                "workarea_id": workarea_id.to_string(),
                "field": "accept_federated",
                "value": accept_federated,
                "from": from,
            }),
        )?;
        self.commit(
            event,
            MetadataFold::WorkareaFederationSet {
                workarea_id,
                accept_federated,
            },
            None,
        )
        .await?;
        Ok(WorkareaFederation { accept_federated })
    }

    /// The keys of `tenant_id` the doctor line names (d12).
    fn wide_keys_of(&self, tenant_id: Uuid) -> Vec<WideKey> {
        self.wide_keys
            .lock()
            .map(|seen| {
                seen.iter()
                    .filter(|((t, _), _)| *t == tenant_id)
                    .map(|((_, key_id), n)| WideKey {
                        key_id: *key_id,
                        candidates: *n,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// What a recall at `scope` searches under its `sources` (d4, d5): the
    /// refusals in d5's order — the list's shape, federation on, a pipeline
    /// that is not `Cc`, then each name in index order — and then the
    /// candidates, the reference set's counts and the selection. On a
    /// tenant with federation off, `scope` reads nothing more and the plan
    /// is today's (d8).
    #[allow(clippy::too_many_arguments, reason = "one recall's inputs")]
    pub(super) async fn plan_sources(
        &self,
        caller: &Caller,
        req: &RecallRequest,
        scope: Uuid,
        hit_nodes: &[Uuid],
        kinds: &[MemoryKind],
        pipeline: Pipeline,
        query: &str,
        expired_at: Option<i64>,
    ) -> Result<SourcePlan, AppError> {
        let tenant_id = caller.key.tenant_id;
        let policy = self.journal.federation_policy(tenant_id).await?;
        let sources = match &req.sources {
            Some(sources) => sources.clone(),
            None if policy.enabled && policy.default_sources == DefaultSources::Wide => {
                Sources::Wide
            }
            None => Sources::Scope,
        };
        // (1) The list's shape.
        if let Sources::List(names) = &sources {
            if names.is_empty() {
                return Err(sources_invalid("/sources", "sources is an empty list"));
            }
            let distinct: BTreeSet<&String> = names.iter().collect();
            if distinct.len() != names.len() {
                return Err(sources_invalid(
                    "/sources",
                    "sources names a workarea twice",
                ));
            }
            if names.len() > policy.max_sources as usize {
                return Err(sources_invalid(
                    "/sources",
                    &format!(
                        "sources lists {} workareas, more than the tenant's max_sources of {}",
                        names.len(),
                        policy.max_sources
                    ),
                ));
            }
        }
        // (2) Federation on, else today's answer for `scope` and a refusal
        // for anything wider, never a silent `scope` (d5).
        if !policy.enabled {
            if sources != Sources::Scope {
                return Err(sources_invalid(
                    "/sources",
                    "federation is off on this tenant: sources is scope until a tenant admin enables it",
                ));
            }
            return Ok(SourcePlan {
                promoted: hit_nodes.to_vec(),
                ..SourcePlan::default()
            });
        }
        // (3) A `Cc` pipeline normalises each list by its own bounds, which
        // an added source moves (d7.2).
        if sources != Sources::Scope && matches!(pipeline.fusion(), Fusion::Cc { .. }) {
            return Err(AppError::SourcesUnderCc {
                pipeline: pipeline.as_str().to_owned(),
            });
        }
        // Only `bm25_postings` prunes, and never under `match: all`
        // (ADR-0052 d4, d6).
        let prunes = pipeline.retrievers().contains(&RetrieverKind::Bm25Postings)
            && req.match_mode == MatchMode::Any;
        let counts_scope = |nodes: Vec<Uuid>| CountsScope {
            nodes,
            kinds: kinds.to_vec(),
            expired_at,
            prunes,
        };
        match sources {
            Sources::List(names) => {
                // (4) Each name in index order, the first unusable one
                // deciding; the list is answered whole or refused.
                let listed = self.listed_sources(caller, scope, &names).await?;
                let counts = self
                    .journal
                    .reference_counts(tenant_id, &counts_scope(listed.clone()), query)
                    .await?;
                let kept = Kept::of(&counts);
                let reference = kept.sum(listed.iter());
                let federated: BTreeSet<Uuid> = listed
                    .iter()
                    .filter(|n| !hit_nodes.contains(n))
                    .copied()
                    .collect();
                let mut searched = Vec::with_capacity(listed.len());
                for node in &listed {
                    let via = if federated.contains(node) {
                        Via::Federated
                    } else {
                        Via::Scope
                    };
                    searched.push(self.searched(caller, *node, via, &kept, &reference).await?);
                }
                Ok(SourcePlan {
                    promoted: listed.clone(),
                    held_within: Some(listed.iter().copied().collect()),
                    reference: Some(listed.clone()),
                    report: Some(SourcesReport {
                        mode: "list",
                        considered: listed.len(),
                        searched,
                        cut: CutReport::default(),
                    }),
                    record: Some(FederationRecord {
                        sources: Value::from(
                            listed.iter().map(Uuid::to_string).collect::<Vec<_>>(),
                        ),
                        federated_workarea_ids: federated.iter().copied().collect(),
                        reference_workarea_ids: listed,
                    }),
                    federated,
                    hint: None,
                })
            }
            Sources::Scope | Sources::Wide => {
                let wide = sources == Sources::Wide;
                let candidates = self.candidates(caller, scope, hit_nodes).await?;
                if candidates.len() > CANDIDATES_DOCTOR
                    && let Ok(mut seen) = self.wide_keys.lock()
                {
                    let n = seen.entry((tenant_id, caller.key.key_id)).or_default();
                    *n = (*n).max(candidates.len());
                }
                // R = served(scope) ∪ C, fixed before any selection (d4.2).
                let mut r: Vec<Uuid> = hit_nodes.to_vec();
                r.extend(candidates.iter().map(|c| c.id));
                let counts = self
                    .journal
                    .reference_counts(tenant_id, &counts_scope(r.clone()), query)
                    .await?;
                let kept = Kept::of(&counts);
                let reference = kept.sum(r.iter());
                let scope_counts = kept.sum(hit_nodes.iter());
                let candidate_counts: Vec<SourceCounts> =
                    candidates.iter().map(|c| kept.of_node(&c.id)).collect();
                if !wide {
                    return Ok(SourcePlan {
                        promoted: hit_nodes.to_vec(),
                        hint: Some(HintInput {
                            words: kept.words.len(),
                            reference,
                            scope: scope_counts,
                            candidates: candidate_counts,
                        }),
                        record: Some(FederationRecord {
                            sources: Value::from("scope"),
                            federated_workarea_ids: Vec::new(),
                            reference_workarea_ids: r,
                        }),
                        ..SourcePlan::default()
                    });
                }
                // Below the cap the union; above it the estimate's order,
                // ties by name, the rest the tail (d4.3, d4.4, d4.6).
                let mut ordered: Vec<(SourceCounts, String, f64)> = candidates
                    .iter()
                    .zip(candidate_counts)
                    .map(|(c, counts)| {
                        let e = estimate(&counts, &reference);
                        (counts, c.name.clone(), e)
                    })
                    .collect();
                let ids: BTreeMap<String, Uuid> =
                    candidates.iter().map(|c| (c.name.clone(), c.id)).collect();
                order_by_estimate(&mut ordered);
                let cap = policy.max_sources as usize;
                let (chosen, tail) = if ordered.len() <= cap {
                    (ordered.as_slice(), &[][..])
                } else {
                    ordered.split_at(cap)
                };
                let cut: Vec<&SourceCounts> =
                    tail.iter().filter(|t| t.2 > 0.0).map(|t| &t.0).collect();
                let selected: Vec<Uuid> = chosen.iter().map(|c| ids[&c.1]).collect();
                let mut searched = Vec::with_capacity(hit_nodes.len() + selected.len());
                for node in hit_nodes {
                    searched.push(
                        self.searched(caller, *node, Via::Scope, &kept, &reference)
                            .await?,
                    );
                }
                for node in &selected {
                    searched.push(
                        self.searched(caller, *node, Via::Federated, &kept, &reference)
                            .await?,
                    );
                }
                let mut promoted = hit_nodes.to_vec();
                promoted.extend(selected.iter().copied());
                Ok(SourcePlan {
                    promoted,
                    held_within: None,
                    reference: Some(r.clone()),
                    report: Some(SourcesReport {
                        mode: "wide",
                        considered: candidates.len(),
                        searched,
                        cut: CutReport {
                            count: cut.len(),
                            matches_at_least: matches_at_least(kept.words.len(), cut),
                        },
                    }),
                    record: Some(FederationRecord {
                        sources: Value::from("wide"),
                        federated_workarea_ids: selected.clone(),
                        reference_workarea_ids: r,
                    }),
                    federated: selected.into_iter().collect(),
                    hint: None,
                })
            }
        }
    }

    /// C (d4.1): every node outside subtree(scope) and outside the scope's
    /// served nodes where reader is effective for the key — the union over
    /// its reader grants' subtrees — that is a trusted source (d1) and
    /// accepts federated reads (d5). Computed per request from the grants
    /// and denials this request read, cached nowhere; ordered by name.
    async fn candidates(
        &self,
        caller: &Caller,
        scope: Uuid,
        hit_nodes: &[Uuid],
    ) -> Result<Vec<Candidate>, AppError> {
        let tenant_id = caller.key.tenant_id;
        let mut outside: BTreeSet<Uuid> = self
            .journal
            .workarea_subtree(tenant_id, scope)
            .await?
            .into_iter()
            .map(|w| w.workarea_id)
            .collect();
        outside.extend(hit_nodes.iter().copied());
        let roots: BTreeSet<Uuid> = caller
            .grants
            .iter()
            .filter(|g| g.role == Role::Reader)
            .map(|g| g.node)
            .collect();
        let mut found: BTreeMap<Uuid, String> = BTreeMap::new();
        for root in roots {
            let readable: BTreeSet<Uuid> = self
                .nodes_with(caller, root, Role::Reader)
                .await?
                .into_iter()
                .collect();
            if readable.is_empty() {
                continue;
            }
            let in_progress = self.subtree_roots(tenant_id, root).await?;
            for node in self.journal.workarea_subtree(tenant_id, root).await? {
                let id = node.workarea_id;
                if outside.contains(&id) || !readable.contains(&id) || found.contains_key(&id) {
                    continue;
                }
                let inside = in_progress.get(&id).copied().flatten().is_some();
                if is_trusted_source(node.status, inside) {
                    found.insert(id, node.name.as_str().to_owned());
                }
            }
        }
        let ids: Vec<Uuid> = found.keys().copied().collect();
        let declining: BTreeSet<Uuid> = self
            .journal
            .federation_declining(tenant_id, &ids)
            .await?
            .into_iter()
            .collect();
        let mut candidates: Vec<Candidate> = found
            .into_iter()
            .filter(|(id, _)| !declining.contains(id))
            .map(|(id, name)| Candidate { id, name })
            .collect();
        candidates.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
        Ok(candidates)
    }

    /// A list's nodes, each checked in index order (d5): a name the key
    /// cannot read and one that does not exist are the same
    /// `access.not_found` at `/sources/<i>`; a readable name that is not a
    /// trusted source, or that lies outside subtree(scope) on a workarea
    /// declining federated reads, is refused with its reason. Only after
    /// the read check passes is a status or a policy consulted (d6).
    async fn listed_sources(
        &self,
        caller: &Caller,
        scope: Uuid,
        names: &[String],
    ) -> Result<Vec<Uuid>, AppError> {
        let tenant_id = caller.key.tenant_id;
        let inside_scope: BTreeSet<Uuid> = self
            .journal
            .workarea_subtree(tenant_id, scope)
            .await?
            .into_iter()
            .map(|w| w.workarea_id)
            .collect();
        let mut listed = Vec::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            let not_found = AppError::SourceNotFound { index };
            let Ok(reference) = Reference::parse(name) else {
                return Err(not_found);
            };
            let id = match self
                .resolve(&caller.key, NamedKind::Workarea, &reference)
                .await
            {
                Ok(id) => id,
                Err(AppError::Store(e)) => return Err(AppError::Store(e)),
                Err(_) => return Err(not_found),
            };
            let ancestry = self
                .journal
                .workarea_ancestry(tenant_id, id)
                .await?
                .unwrap_or_default();
            let ids: Vec<Uuid> = ancestry.iter().map(|w| w.workarea_id).collect();
            if ids.is_empty() || !caller.effective(&ids).contains(&Role::Reader) {
                return Err(not_found);
            }
            if listed.contains(&id) {
                return Err(sources_invalid(
                    "/sources",
                    "sources names a workarea twice",
                ));
            }
            let status = ancestry[0].status;
            let inside = self.in_progress_root(tenant_id, id).await?.is_some();
            if !is_trusted_source(status, inside) {
                return Err(AppError::SourceRefused {
                    index,
                    reason: SourceRefusal::Untrusted,
                });
            }
            if !inside_scope.contains(&id)
                && !self
                    .journal
                    .federation_declining(tenant_id, &[id])
                    .await?
                    .is_empty()
            {
                return Err(AppError::SourceRefused {
                    index,
                    reason: SourceRefusal::DeclinesFederation,
                });
            }
            listed.push(id);
        }
        Ok(listed)
    }

    /// One `search.sources.searched` entry (d8): the node's path, its
    /// estimate, and for a federated source the kept words its rows hold.
    async fn searched(
        &self,
        caller: &Caller,
        node: Uuid,
        via: Via,
        kept: &Kept,
        reference: &SourceCounts,
    ) -> Result<SearchedSource, AppError> {
        let ancestry = self
            .journal
            .workarea_ancestry(caller.key.tenant_id, node)
            .await?
            .ok_or_else(|| StoreError::Backend(format!("searched node {node} has no ancestry")))?;
        let counts = kept.of_node(&node);
        let matched_words = match via {
            Via::Scope => Vec::new(),
            Via::Federated => kept
                .words
                .iter()
                .zip(&counts.df)
                .filter(|(_, d)| **d > 0)
                .map(|(w, _)| w.clone())
                .collect(),
        };
        Ok(SearchedSource {
            workarea_id: node,
            workarea_path: derived_path(&ancestry),
            via,
            estimate: estimate(&counts, reference),
            matched_words,
        })
    }
}

/// The reference counts restricted to the kept words, those the pipeline
/// does not prune over R (d4.2, d4.4).
struct Kept {
    words: Vec<String>,
    nodes: BTreeMap<Uuid, SourceCounts>,
}

impl Kept {
    fn of(counts: &crate::federation::ReferenceCounts) -> Self {
        let keep: Vec<usize> = counts
            .words
            .iter()
            .enumerate()
            .filter(|(_, w)| !counts.pruned.contains(w))
            .map(|(i, _)| i)
            .collect();
        Kept {
            words: keep.iter().map(|&i| counts.words[i].clone()).collect(),
            nodes: counts
                .nodes
                .iter()
                .map(|(id, c)| {
                    (
                        *id,
                        SourceCounts {
                            rows: c.rows,
                            df: keep
                                .iter()
                                .map(|&i| c.df.get(i).copied().unwrap_or(0))
                                .collect(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn of_node(&self, node: &Uuid) -> SourceCounts {
        self.nodes
            .get(node)
            .cloned()
            .unwrap_or_else(|| SourceCounts {
                rows: 0,
                df: vec![0; self.words.len()],
            })
    }

    fn sum<'a>(&self, nodes: impl Iterator<Item = &'a Uuid>) -> SourceCounts {
        let of: Vec<SourceCounts> = nodes.map(|n| self.of_node(n)).collect();
        SourceCounts::sum(self.words.len(), &of)
    }
}

fn sources_invalid(field: &'static str, message: &str) -> AppError {
    AppError::SourcesInvalid {
        field,
        message: message.to_owned(),
    }
}

fn policy_json(policy: &FederationPolicy) -> Value {
    json!({
        "enabled": policy.enabled,
        "default_sources": policy.default_sources.as_str(),
        "max_sources": policy.max_sources,
    })
}
