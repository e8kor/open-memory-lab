//! Federated recall, phase 1 (ADR-0072): the tenant's `federation` policy
//! and a workspace's `accept_federated`, each a person's or an admin's
//! journalled act; and the plan a recall runs under its `sources` — the
//! candidates a key reads, the reference set whose statistics every hit is
//! scored with and the selection above the cap. Selection
//! reads exact counts and names alone: no clock, no randomness and no model
//! (d9). Every name and count it serves is of a node the key reads (d6).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::federation::{
    CANDIDATES_DOCTOR, MAX_SOURCES_CEILING, SourceCounts, estimate, is_trusted_source,
    matches_at_least, order_by_estimate,
};
use oml_domain::in_progress::may_set_mode;
use oml_domain::name::{NamedKind, Reference};
use oml_domain::{MemoryKind, Role, WorkspaceStatus, event_types};

use oml_domain::admin_act::AdminActKind;

use super::{App, Caller, derived_path, served_path};
use crate::federation::{
    CountsScope, CutReport, DefaultSources, FederationActTaken, FederationPolicy,
    FederationPolicyChange, FederationRecord, FederationSettings, SearchedSource, SourceRefusal,
    Sources, SourcesReport, Via, WorkspaceFederation,
};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer, StoreError};
use crate::search::{Fusion, MatchMode, Pipeline, RetrieverKind};
use crate::types::WorkspaceRecord;
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
    /// The public nodes outside the scope searched: a hit on one is `via:
    /// public` (ADR-0098 d3).
    pub public: BTreeSet<Uuid>,
    pub report: Option<SourcesReport>,
    pub record: Option<FederationRecord>,
}

/// The wide keys' counts (ADR-0097 d18).
fn wide_counts(wide: &[WideKey]) -> crate::federation::WideKeyCounts {
    crate::federation::WideKeyCounts {
        keys: wide.len(),
        most_candidates: wide.iter().map(|k| k.candidates).max().unwrap_or(0),
    }
}

/// A candidate source: a trusted node outside the scope the key reads.
#[derive(Debug, Clone)]
pub(super) struct Candidate {
    pub(super) id: Uuid,
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
        // An admin key's, or a maintenance key's with no key listed
        // (ADR-0097 d18).
        let admin = self.counts_caller(bearer, "federation_settings").await?.key;
        let policy = self.journal.federation_policy(admin.tenant_id).await?;
        let wide = self.wide_keys_of(admin.tenant_id);
        let wide_counts = wide_counts(&wide);
        // A maintenance key reads the counts and never the list.
        let wide = if admin.role == Role::Maintenance {
            Vec::new()
        } else {
            wide
        };
        Ok((
            FederationSettings {
                policy,
                widen_signal: self.widen.health_of(admin.tenant_id),
                wide_counts,
            },
            wide,
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
        Ok(self
            .set_federation_policy_with_reason(bearer, change, None)
            .await?
            .now)
    }

    /// [`Self::set_federation_policy`] as an admin act (ADR-0097 d3, d5): a
    /// change that searches less (federation off, the `scope` default,
    /// fewer sources) is narrowing and takes effect at once; one that
    /// searches more is widening, takes effect at once marked under
    /// `sole_admin`, and otherwise waits on a second admin person, whose
    /// confirmation applies the change over the policy as it stands then.
    /// A change that does both is two acts of two classes and is refused,
    /// [`AppError::AdminField`] at `/`, journalling nothing.
    ///
    /// # Errors
    /// As [`Self::set_federation_policy`], the refusal above, and a pending
    /// widening sent no reason.
    pub async fn set_federation_policy_with_reason(
        &self,
        bearer: impl Into<Bearer<'_>>,
        change: FederationPolicyChange,
        reason: Option<&str>,
    ) -> Result<FederationActTaken<(FederationSettings, Vec<WideKey>)>, AppError> {
        let admin = self
            .admin_caller(bearer.into(), "federation_policy")
            .await?
            .key;
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
        let mut pending = None;
        let mut now = from;
        if policy != from {
            let widens = policy.enabled && !from.enabled
                || policy.default_sources == DefaultSources::Wide
                    && from.default_sources != DefaultSources::Wide
                || policy.max_sources > from.max_sources;
            let narrows = !policy.enabled && from.enabled
                || policy.default_sources != DefaultSources::Wide
                    && from.default_sources == DefaultSources::Wide
                || policy.max_sources < from.max_sources;
            if widens && narrows {
                return Err(AppError::AdminField {
                    field: "/",
                    message: "a change that widens and narrows the federation policy is two acts: send the narrowing first, then the widening".to_owned(),
                });
            }
            let kind = if widens {
                AdminActKind::WidenFederationPolicy
            } else {
                AdminActKind::NarrowFederationPolicy
            };
            let prev = self.head_event(tenant_id).await?;
            let act = self
                .begin_admin_act(
                    &admin,
                    kind,
                    json!({ "policy": policy_json(&policy), "from": policy_json(&from) }),
                    reason,
                    json!({ "change": change_json(&change), "from": policy_json(&from) }),
                    prev.as_ref(),
                )
                .await?;
            if act.pending() {
                pending = Some(act.record.clone());
                self.commit_with_act(Some(act), Vec::new()).await?;
            } else {
                let event = self.policy_event(&act.event, &admin, policy, from, act.act_id())?;
                self.commit_with_act(Some(act), vec![event]).await?;
                now = policy;
            }
        }
        let wide = self.wide_keys_of(tenant_id);
        Ok(FederationActTaken {
            now: (
                FederationSettings {
                    policy: now,
                    widen_signal: self.widen.health_of(tenant_id),
                    wide_counts: wide_counts(&wide),
                },
                wide,
            ),
            pending,
        })
    }

    /// `federation.policy_set` of `policy` over `from`, sealed on `prev` and
    /// naming the admin act that caused it (ADR-0097 d2).
    pub(crate) fn policy_event(
        &self,
        prev: &oml_domain::SignedEnvelope,
        admin: &crate::types::KeyRecord,
        policy: FederationPolicy,
        from: FederationPolicy,
        act_id: Uuid,
    ) -> Result<(oml_domain::SignedEnvelope, MetadataFold), AppError> {
        let event = self.seal(
            Some(prev),
            admin.tenant_id,
            event_types::FEDERATION_POLICY_SET,
            &admin.principal,
            Some(admin.key_id),
            super::admin_acts::with_act_id(
                json!({
                    "policy": policy_json(&policy),
                    "from": policy_json(&from),
                    "by_key_id": admin.key_id.to_string(),
                }),
                act_id,
            ),
        )?;
        Ok((event, MetadataFold::FederationPolicySet(policy)))
    }

    /// `workspace.federation_set` of `workspace_id` to `accept_federated`,
    /// sealed on `prev` under `key` and naming `act_id` where an admin act
    /// caused it (ADR-0097 d2).
    pub(crate) fn workspace_federation_event(
        &self,
        prev: Option<&oml_domain::SignedEnvelope>,
        key: &crate::types::KeyRecord,
        workspace_id: Uuid,
        accept_federated: bool,
        act_id: Option<Uuid>,
    ) -> Result<(oml_domain::SignedEnvelope, MetadataFold), AppError> {
        let payload = json!({
            "workspace_id": workspace_id.to_string(),
            "field": "accept_federated",
            "value": accept_federated,
            "from": !accept_federated,
        });
        let event = self.seal(
            prev,
            key.tenant_id,
            event_types::WORKSPACE_FEDERATION_SET,
            &key.principal,
            Some(key.key_id),
            match act_id {
                Some(act_id) => super::admin_acts::with_act_id(payload, act_id),
                None => payload,
            },
        )?;
        Ok((
            event,
            MetadataFold::WorkspaceFederationSet {
                workspace_id,
                accept_federated,
            },
        ))
    }

    /// `POST /v2/workspaces/{workspace}/federation` (d5): a reviewer key a
    /// person holds, with reviewer effective access on the node, or a tenant
    /// admin sets `accept_federated`; an agent's key is refused before any
    /// lookup. Journalled as `workspace.federation_set {workspace_id, field,
    /// value, from}`; setting the value the node holds journals nothing.
    pub async fn set_workspace_federation(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
        accept_federated: bool,
    ) -> Result<WorkspaceFederation, AppError> {
        Ok(self
            .set_workspace_federation_with_reason(bearer, workspace, accept_federated, None)
            .await?
            .now)
    }

    /// [`Self::set_workspace_federation`], an admin's as an admin act
    /// (ADR-0097 d3, d5): turning `accept_federated` off is narrowing, at
    /// once; turning it on is widening, at once marked under `sole_admin`,
    /// and otherwise pending until a second admin person confirms it, which
    /// applies it then. A person's reviewer key acts as the person, never
    /// as an admin act (d1).
    ///
    /// # Errors
    /// As [`Self::set_workspace_federation`], and a pending widening sent no
    /// reason.
    pub async fn set_workspace_federation_with_reason(
        &self,
        bearer: impl Into<Bearer<'_>>,
        workspace: Reference,
        accept_federated: bool,
        reason: Option<&str>,
    ) -> Result<FederationActTaken<WorkspaceFederation>, AppError> {
        let caller = self
            .key_act_caller(bearer.into(), "set_workspace_federation")
            .await?;
        let key = &caller.key;
        let admin = key.role == Role::Admin;
        if !admin && !may_set_mode(key.role, key.used_by, key.profile) {
            return Err(AppError::FederationHumanOnly {
                role: key.role,
                used_by: key.used_by,
            });
        }
        let tenant_id = key.tenant_id;
        let workspace_id = self.resolve(key, NamedKind::Workspace, &workspace).await?;
        if admin {
            self.journal
                .workspace(tenant_id, workspace_id)
                .await?
                .ok_or(AppError::NotFound)?;
        } else {
            self.require_access(
                &caller,
                workspace_id,
                Role::Reviewer,
                "workspace_federation",
            )
            .await?;
        }
        self.observer.workspace(workspace_id);
        let from = self
            .journal
            .federation_declining(tenant_id, &[workspace_id])
            .await?
            .is_empty();
        let done = |accept_federated, pending| FederationActTaken {
            now: WorkspaceFederation { accept_federated },
            pending,
        };
        if from == accept_federated {
            return Ok(done(accept_federated, None));
        }
        let prev = self.head_event(tenant_id).await?;
        let act = if admin {
            let act = self
                .begin_admin_act(
                    key,
                    if accept_federated {
                        AdminActKind::AcceptFederation
                    } else {
                        AdminActKind::DeclineFederation
                    },
                    json!({
                        "workspace_id": workspace_id.to_string(),
                        "accept_federated": accept_federated,
                    }),
                    reason,
                    json!({
                        "workspace_id": workspace_id.to_string(),
                        "accept_federated": accept_federated,
                    }),
                    prev.as_ref(),
                )
                .await?;
            if act.pending() {
                let record = act.record.clone();
                self.commit_with_act(Some(act), Vec::new()).await?;
                return Ok(done(from, Some(record)));
            }
            Some(act)
        } else {
            None
        };
        let event = self.workspace_federation_event(
            act.as_ref().map(|a| &a.event).or(prev.as_ref()),
            key,
            workspace_id,
            accept_federated,
            act.as_ref().map(super::admin_acts::BegunAct::act_id),
        )?;
        self.commit_with_act(act, vec![event]).await?;
        Ok(done(accept_federated, None))
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
        policy: Option<FederationPolicy>,
        scope: Uuid,
        hit_nodes: &[Uuid],
        public: &BTreeSet<Uuid>,
        kinds: &[MemoryKind],
        pipeline: Pipeline,
        query: &str,
        expired_at: Option<i64>,
    ) -> Result<SourcePlan, AppError> {
        let tenant_id = caller.key.tenant_id;
        let sources = match (&req.sources, &policy) {
            (Some(sources), _) => sources.clone(),
            (None, Some(p)) if p.enabled && p.default_sources == DefaultSources::Wide => {
                Sources::Wide
            }
            (None, _) => Sources::Scope,
        };
        // `scope` is today's answer, and it reads nothing more: no policy,
        // no candidate and no count, federation on or off (d12, as amended).
        if sources == Sources::Scope && public.is_empty() {
            return Ok(SourcePlan {
                promoted: hit_nodes.to_vec(),
                ..SourcePlan::default()
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
        // A default recall that searched a public node outside the scope
        // lists each one in `search.sources`, so every hit still says where
        // it came from; the statistics are the hit nodes', public ones
        // among them, as the scope's own (ADR-0098 d3, d7). Federation is
        // neither read nor needed.
        if sources == Sources::Scope {
            let counts = self
                .journal
                .reference_counts(tenant_id, &counts_scope(hit_nodes.to_vec()), query)
                .await?;
            let kept = Kept::of(&counts);
            let reference = kept.sum(hit_nodes.iter());
            let mut searched = Vec::with_capacity(public.len());
            for node in hit_nodes.iter().filter(|n| public.contains(n)) {
                searched.push(
                    self.searched(caller, *node, None, Via::Public, &kept, &reference)
                        .await?,
                );
            }
            return Ok(SourcePlan {
                promoted: hit_nodes.to_vec(),
                public: public.clone(),
                report: Some(SourcesReport {
                    mode: "scope",
                    considered: searched.len(),
                    searched,
                    cut: CutReport::default(),
                }),
                ..SourcePlan::default()
            });
        }
        let policy = match policy {
            Some(policy) => policy,
            None => self.journal.federation_policy(tenant_id).await?,
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
                    "sources names a workspace twice",
                ));
            }
            if names.len() > policy.max_sources as usize {
                return Err(sources_invalid(
                    "/sources",
                    &format!(
                        "sources lists {} workspaces, more than the tenant's max_sources of {}",
                        names.len(),
                        policy.max_sources
                    ),
                ));
            }
        }
        // (2) Federation on, else today's answer for `scope` and a refusal
        // for anything wider, never a silent `scope` (d5).
        if !policy.enabled {
            return Err(sources_invalid(
                "/sources",
                "federation is off on this tenant: sources is scope until a tenant admin enables it",
            ));
        }
        // (3) A `Cc` pipeline normalises each list by its own bounds, which
        // an added source moves (d7.2).
        if matches!(pipeline.fusion(), Fusion::Cc { .. }) {
            return Err(AppError::SourcesUnderCc {
                pipeline: pipeline.as_str().to_owned(),
            });
        }
        match sources {
            Sources::List(names) => {
                // (4) Each name in index order, the first unusable one
                // deciding; the list is answered whole or refused.
                let (listed, paths): (Vec<Uuid>, BTreeMap<Uuid, String>) = {
                    let named = self.listed_sources(caller, scope, &names).await?;
                    (
                        named.iter().map(|(id, _)| *id).collect(),
                        named.into_iter().collect(),
                    )
                };
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
                    let via = if public.contains(node) {
                        Via::Public
                    } else if federated.contains(node) {
                        Via::Federated
                    } else {
                        Via::Scope
                    };
                    searched.push(
                        self.searched(caller, *node, paths.get(node), via, &kept, &reference)
                            .await?,
                    );
                }
                let listed_public: BTreeSet<Uuid> = listed
                    .iter()
                    .filter(|n| public.contains(n))
                    .copied()
                    .collect();
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
                        federated_workspace_ids: federated.iter().copied().collect(),
                        reference_workspace_ids: listed,
                    }),
                    federated,
                    public: listed_public,
                })
            }
            Sources::Scope | Sources::Wide => {
                let (candidates, paths) = self.candidates(caller, scope, hit_nodes).await?;
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
                let candidate_counts: Vec<SourceCounts> =
                    candidates.iter().map(|c| kept.of_node(&c.id)).collect();
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
                        self.searched(
                            caller,
                            *node,
                            paths.get(node),
                            if public.contains(node) {
                                Via::Public
                            } else {
                                Via::Scope
                            },
                            &kept,
                            &reference,
                        )
                        .await?,
                    );
                }
                for node in &selected {
                    searched.push(
                        self.searched(
                            caller,
                            *node,
                            paths.get(node),
                            Via::Federated,
                            &kept,
                            &reference,
                        )
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
                        federated_workspace_ids: selected.clone(),
                        reference_workspace_ids: r,
                    }),
                    federated: selected.into_iter().collect(),
                    public: public.clone(),
                })
            }
        }
    }

    /// C (d4.1): every node outside subtree(scope) and outside the scope's
    /// served nodes where reader is effective for the key — the union over
    /// its reader grants' subtrees — that is a trusted source (d1) and
    /// accepts federated reads (d5). Computed per request from the grants
    /// and denials this request read, cached nowhere; ordered by name. With
    /// it, the path of every node of subtree(scope) and of every candidate,
    /// derived from the subtrees read, so no node costs a read of its own.
    pub(super) async fn candidates(
        &self,
        caller: &Caller,
        scope: Uuid,
        hit_nodes: &[Uuid],
    ) -> Result<(Vec<Candidate>, BTreeMap<Uuid, String>), AppError> {
        let (inside, mut paths) = self.subtree_paths(caller.key.tenant_id, scope).await?;
        let mut outside: BTreeSet<Uuid> = inside.iter().map(|w| w.workspace_id).collect();
        outside.extend(hit_nodes.iter().copied());
        let candidates = self.trusted_outside(caller, &outside, &mut paths).await?;
        Ok((candidates, paths))
    }

    /// The trusted sources the key reads that `outside` does not hold,
    /// accepting federated reads, ordered by name; each one's path joins
    /// `paths`.
    async fn trusted_outside(
        &self,
        caller: &Caller,
        outside: &BTreeSet<Uuid>,
        paths: &mut BTreeMap<Uuid, String>,
    ) -> Result<Vec<Candidate>, AppError> {
        let tenant_id = caller.key.tenant_id;
        let roots: BTreeSet<Uuid> = caller
            .grants
            .iter()
            .filter(|g| g.role == Role::Reader)
            .map(|g| g.node)
            .collect();
        let mut found: BTreeMap<Uuid, String> = BTreeMap::new();
        // One ancestry and one subtree read per grant root: each node's
        // effective access, in-progress root and path follow from its
        // parent's, parent before child (ADR-0025 d1; ADR-0030 d1).
        for root in roots {
            let Some(above) = self.journal.workspace_ancestry(tenant_id, root).await? else {
                continue;
            };
            let root_ip = oml_domain::in_progress::in_progress_root_of(
                &above
                    .iter()
                    .map(|w| {
                        (
                            w.workspace_id,
                            w.in_progress,
                            w.status == WorkspaceStatus::Development,
                        )
                    })
                    .collect::<Vec<_>>(),
            );
            let mut walk: BTreeMap<Uuid, (Vec<Uuid>, Option<Uuid>, String)> = BTreeMap::new();
            walk.insert(
                root,
                (
                    above.iter().map(|w| w.workspace_id).collect(),
                    root_ip,
                    derived_path(&above),
                ),
            );
            for node in self.journal.workspace_subtree(tenant_id, root).await? {
                let id = node.workspace_id;
                if id != root {
                    let Some((ancestry, ip, path)) =
                        node.parent_id.and_then(|p| walk.get(&p)).cloned()
                    else {
                        continue;
                    };
                    // A node that is not `development` is never inside, nor
                    // is anything beneath it (ADR-0030 d3).
                    let ip = if node.status != WorkspaceStatus::Development {
                        None
                    } else if node.in_progress {
                        Some(id)
                    } else {
                        ip
                    };
                    let ancestry = std::iter::once(id).chain(ancestry).collect();
                    walk.insert(id, (ancestry, ip, format!("{path}.{}", node.name.as_str())));
                }
                let (ancestry, ip, path) = &walk[&id];
                if outside.contains(&id)
                    || found.contains_key(&id)
                    || !caller.effective(ancestry).contains(&Role::Reader)
                {
                    continue;
                }
                if is_trusted_source(node.status, ip.is_some()) {
                    found.insert(id, node.name.as_str().to_owned());
                    paths.insert(id, path.clone());
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

    /// The subtree at `top`, parent before child, and each node's path: the
    /// top's from its ancestry, every other node's its parent's and its own
    /// name joined with `.` (ADR-0025 d14).
    async fn subtree_paths(
        &self,
        tenant_id: Uuid,
        top: Uuid,
    ) -> Result<(Vec<WorkspaceRecord>, BTreeMap<Uuid, String>), AppError> {
        let mut paths = BTreeMap::new();
        if let Some(ancestry) = self.journal.workspace_ancestry(tenant_id, top).await? {
            paths.insert(top, derived_path(&ancestry));
        }
        let nodes = self.journal.workspace_subtree(tenant_id, top).await?;
        for node in &nodes {
            if node.workspace_id == top {
                continue;
            }
            if let Some(parent) = node.parent_id.and_then(|p| paths.get(&p)).cloned() {
                paths.insert(
                    node.workspace_id,
                    format!("{parent}.{}", node.name.as_str()),
                );
            }
        }
        Ok((nodes, paths))
    }

    /// For the candidate-count report (d4.1, #205): how many trusted
    /// sources accepting federated reads the key reads, |C| before any
    /// scope is set aside; `None` for a key the tenant does not hold. It
    /// reads the key's grants and denials now, as a recall would, and is
    /// served on no route: an operator's one-off read of a tenant's keys.
    pub async fn federation_candidate_count(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Option<usize>, AppError> {
        let Some(key) = self.journal.key(tenant_id, key_id).await? else {
            return Ok(None);
        };
        let caller = self.caller_of(key).await?;
        let mut paths = BTreeMap::new();
        Ok(Some(
            self.trusted_outside(&caller, &BTreeSet::new(), &mut paths)
                .await?
                .len(),
        ))
    }

    /// A list's nodes, each checked in index order (d5): a name the key
    /// cannot read and one that does not exist are the same
    /// `access.not_found` at `/sources/<i>`; a readable name that is not a
    /// trusted source, or that lies outside subtree(scope) on a workspace
    /// declining federated reads, is refused with its reason. Only after
    /// the read check passes is a status or a policy consulted (d6).
    async fn listed_sources(
        &self,
        caller: &Caller,
        scope: Uuid,
        names: &[String],
    ) -> Result<Vec<(Uuid, String)>, AppError> {
        let tenant_id = caller.key.tenant_id;
        let inside_scope: BTreeSet<Uuid> = self
            .journal
            .workspace_subtree(tenant_id, scope)
            .await?
            .into_iter()
            .map(|w| w.workspace_id)
            .collect();
        let mut listed = Vec::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            let not_found = AppError::SourceNotFound { index };
            let Ok(reference) = Reference::parse(name) else {
                return Err(not_found);
            };
            let id = match self
                .resolve(&caller.key, NamedKind::Workspace, &reference)
                .await
            {
                Ok(id) => id,
                Err(AppError::Store(e)) => return Err(AppError::Store(e)),
                Err(_) => return Err(not_found),
            };
            let ancestry = self
                .journal
                .workspace_ancestry(tenant_id, id)
                .await?
                .unwrap_or_default();
            let ids: Vec<Uuid> = ancestry.iter().map(|w| w.workspace_id).collect();
            if ids.is_empty() || !caller.effective(&ids).contains(&Role::Reader) {
                return Err(not_found);
            }
            if listed.iter().any(|(listed, _)| *listed == id) {
                return Err(sources_invalid(
                    "/sources",
                    "sources names a workspace twice",
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
            listed.push((id, derived_path(&ancestry)));
        }
        Ok(listed)
    }

    /// One `search.sources.searched` entry (d8): the node's path, its
    /// estimate, and for a federated source the kept words its rows hold.
    async fn searched(
        &self,
        caller: &Caller,
        node: Uuid,
        path: Option<&String>,
        via: Via,
        kept: &Kept,
        reference: &SourceCounts,
    ) -> Result<SearchedSource, AppError> {
        // The path the subtrees already gave, else the node's ancestry: a
        // scope's `universe` inside a user root (ADR-0044 d10).
        // A public node searched is served with its private ancestors as
        // placeholders (ADR-0098, answer 4 of 2026-11-28): its ancestry is
        // read again whenever the node is public, whatever path the walk
        // gave.
        // Boxed: the read sits in every recall's future, which stays small.
        let ancestry = Box::pin(self.journal.workspace_ancestry(caller.key.tenant_id, node))
            .await?
            .ok_or_else(|| StoreError::Backend(format!("searched node {node} has no ancestry")))?;
        let workspace_path = match path {
            Some(path) if !ancestry.first().is_some_and(|w| w.public) => path.clone(),
            _ => served_path(caller, &ancestry),
        };
        let counts = kept.of_node(&node);
        let matched_words = match via {
            Via::Scope => Vec::new(),
            Via::Federated | Via::Public => kept
                .words
                .iter()
                .zip(&counts.df)
                .filter(|(_, d)| **d > 0)
                .map(|(w, _)| w.clone())
                .collect(),
        };
        Ok(SearchedSource {
            workspace_id: node,
            workspace_path,
            via,
            estimate: estimate(&counts, reference),
            matched_words,
        })
    }
}

/// The reference counts restricted to the kept words, those the pipeline
/// does not prune over R (d4.2, d4.4).
pub(super) struct Kept {
    pub(super) words: Vec<String>,
    nodes: BTreeMap<Uuid, SourceCounts>,
}

impl Kept {
    pub(super) fn of(counts: &crate::federation::ReferenceCounts) -> Self {
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

    pub(super) fn of_node(&self, node: &Uuid) -> SourceCounts {
        self.nodes
            .get(node)
            .cloned()
            .unwrap_or_else(|| SourceCounts {
                rows: 0,
                df: vec![0; self.words.len()],
            })
    }

    pub(super) fn sum<'a>(&self, nodes: impl Iterator<Item = &'a Uuid>) -> SourceCounts {
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

/// The fields a policy change sends, as its admin act's effect holds them.
fn change_json(change: &FederationPolicyChange) -> Value {
    json!({
        "enabled": change.enabled,
        "default_sources": change.default_sources.map(DefaultSources::as_str),
        "max_sources": change.max_sources,
    })
}

/// The policy `base` with the fields of `change`, as an effect holds it.
pub(crate) fn apply_change_json(base: FederationPolicy, change: &Value) -> FederationPolicy {
    FederationPolicy {
        enabled: change
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(base.enabled),
        default_sources: change
            .get("default_sources")
            .and_then(Value::as_str)
            .and_then(DefaultSources::parse)
            .unwrap_or(base.default_sources),
        max_sources: change
            .get("max_sources")
            .and_then(Value::as_u64)
            .and_then(|m| u32::try_from(m).ok())
            .unwrap_or(base.max_sources),
    }
}

fn policy_json(policy: &FederationPolicy) -> Value {
    json!({
        "enabled": policy.enabled,
        "default_sources": policy.default_sources.as_str(),
        "max_sources": policy.max_sources,
    })
}
