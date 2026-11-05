//! The measurement half of ADR-0054 as accepted: the `feedback` member a
//! read carries, and a tenant admin's three acts on the recall telemetry:
//! the query-text setting, the event listing and the real-usage replay.
//! No agent, writer, reader or dreamer key reads an event, a label or a
//! count here (d2); every act but feedback is admin by key role.

use std::collections::BTreeMap;

use serde_json::json;
use uuid::Uuid;

use oml_domain::{EventTime, Role, event_types};

use super::{App, require_admissible, require_role};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::search::{MatchMode, Pipeline};
use crate::telemetry::{
    EXPLICIT_LABELS_PER_DAY, FEEDBACK_IDS_MAX, FEEDBACK_PER_KEY_PER_DAY, Feedback, FeedbackLabels,
    Label, REPLAY_DEADLINE_SECS, REPLAY_QUERIES_MAX, RETENTION_DAYS, ReplayReport, ReplayScores,
    StoredEvent, TelemetryItem, score_query,
};
use crate::types::{AnchorFilter, AppError, Bearer, RecallRequest};

/// What `GET /v2/usage/settings` answers (ADR-0054 d3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSettings {
    /// Whether this deployment records recalls at all.
    pub recording: bool,
    /// Since when the tenant's events keep their query text; `None` is off.
    pub query_text_since: Option<EventTime>,
    pub retention_days: i64,
}

/// A recall the replay runs: `k` 10, no text, no own held rows.
const REPLAY_K: usize = 10;

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The `feedback` member of a read, checked before the read runs: at
    /// most [`FEEDBACK_IDS_MAX`] ids, none named twice, a `recall_id` this
    /// key issued in the last 24 hours, and only ids that recall served to
    /// it; past [`EXPLICIT_LABELS_PER_DAY`] of the key's explicit labels
    /// today it is [`AppError::FeedbackRateLimited`]. A valid member is
    /// queued to the writer and changes nothing the read answers
    /// (ADR-0054 d6).
    pub async fn accept_feedback(
        &self,
        bearer: impl Into<Bearer<'_>>,
        feedback: &Feedback,
    ) -> Result<(), AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        let key = &caller.key;
        require_admissible(key, Role::Reader, "feedback")?;
        // The per-key cap first, refused members counted, so a stream of
        // made-up recall ids costs no lookup past it.
        let day = self.clock.now().unix_micros().div_euclid(86_400_000_000);
        {
            let mut counts = self
                .feedback_counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            counts.retain(|_, (d, _)| *d == day);
            let slot = counts.entry(key.key_id).or_insert((day, 0));
            if slot.1 >= FEEDBACK_PER_KEY_PER_DAY {
                return Err(AppError::FeedbackRateLimited);
            }
            slot.1 += 1;
        }
        let refuse = |field: String, message: &str| AppError::Feedback {
            field,
            message: message.to_owned(),
        };
        if feedback.useful.len() + feedback.not_useful.len() > FEEDBACK_IDS_MAX {
            return Err(refuse(
                "/feedback".to_owned(),
                "feedback names at most 20 ids, useful and not_useful together",
            ));
        }
        if feedback.useful.is_empty() && feedback.not_useful.is_empty() {
            return Err(refuse(
                "/feedback".to_owned(),
                "feedback names at least one id in useful or not_useful",
            ));
        }
        let Some(telemetry) = self.telemetry.as_ref() else {
            return Err(refuse(
                "/feedback/recall_id".to_owned(),
                "this server records no recall, so it takes no feedback",
            ));
        };
        let mut served = telemetry
            .served(key.tenant_id, key.key_id, feedback.recall_id)
            .await?;
        if served.is_none() {
            // Feedback sent right after its recall may find the event still
            // queued: the queue is written once, then read again.
            telemetry.flush().await;
            served = telemetry
                .served(key.tenant_id, key.key_id, feedback.recall_id)
                .await?;
        }
        let Some(served) = served else {
            return Err(refuse(
                "/feedback/recall_id".to_owned(),
                "recall_id names no recall this key made in the last 24 hours",
            ));
        };
        let mut seen = std::collections::BTreeSet::new();
        for (list, ids) in [
            ("useful", &feedback.useful),
            ("not_useful", &feedback.not_useful),
        ] {
            for (i, id) in ids.iter().enumerate() {
                if !seen.insert(*id) {
                    return Err(refuse(
                        format!("/feedback/{list}/{i}"),
                        "an id is named once in feedback",
                    ));
                }
                if !served.hit_ids.contains(id) {
                    return Err(refuse(
                        format!("/feedback/{list}/{i}"),
                        "the id is not a hit that recall served to this key",
                    ));
                }
            }
        }
        let today = telemetry
            .explicit_labels_today(key.tenant_id, key.key_id)
            .await?;
        if today + seen.len() as u64 > EXPLICIT_LABELS_PER_DAY {
            return Err(AppError::FeedbackRateLimited);
        }
        telemetry.offer(TelemetryItem::Feedback(FeedbackLabels {
            tenant_id: key.tenant_id,
            recall_id: feedback.recall_id,
            created_at: served.created_at,
            useful: feedback.useful.clone(),
            not_useful: feedback.not_useful.clone(),
            at: self.clock.now(),
        }));
        Ok(())
    }

    /// `GET /v2/usage/settings`, admin by key role (ADR-0054 d3).
    pub async fn usage_settings(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<UsageSettings, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "usage_settings")?;
        self.usage_settings_of(admin.tenant_id).await
    }

    /// `POST /v2/usage/settings`, admin by key role: the tenant's events
    /// keep their query text from now (`true`) or no longer (`false`),
    /// journalled as `telemetry.settings_set` with the admin key; turning it
    /// off nulls the text of the tenant's events in the same commit. Setting
    /// what is already set journals nothing (ADR-0054 d3).
    pub async fn set_usage_settings(
        &self,
        bearer: impl Into<Bearer<'_>>,
        query_text: bool,
    ) -> Result<UsageSettings, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "usage_settings")?;
        let tenant_id = admin.tenant_id;
        let current = self.journal.tenant_query_text_since(tenant_id).await?;
        if current.is_some() == query_text {
            return self.usage_settings_of(tenant_id).await;
        }
        let since = query_text.then(|| self.clock.now());
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::TELEMETRY_SETTINGS_SET,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "query_text": query_text,
                "by_key_id": admin.key_id.to_string(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::TelemetrySettingsSet {
                query_text_since: since,
            },
            None,
        )
        .await?;
        self.usage_settings_of(tenant_id).await
    }

    async fn usage_settings_of(&self, tenant_id: Uuid) -> Result<UsageSettings, AppError> {
        Ok(UsageSettings {
            recording: self.telemetry.is_some(),
            query_text_since: self.journal.tenant_query_text_since(tenant_id).await?,
            retention_days: RETENTION_DAYS,
        })
    }

    /// `GET /v2/usage/events`, admin by key role: the tenant's events of the
    /// last 30 days with their labels, newest first; the query text only
    /// where it was stored by opt-in (ADR-0054 d2).
    pub async fn usage_events(
        &self,
        bearer: impl Into<Bearer<'_>>,
        before: Option<(EventTime, Uuid)>,
        limit: u32,
    ) -> Result<Vec<StoredEvent>, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "usage_events")?;
        let telemetry = self.telemetry.as_ref().ok_or(AppError::TelemetryOff)?;
        // What the queue holds is written first, so a listing shows every
        // recall answered before it.
        telemetry.flush().await;
        let events = telemetry.events(admin.tenant_id, before, limit).await?;
        // An admin reading other keys' recalls, and their text where kept,
        // is journalled, as an admin read of a memory is (ADR-0045 d21;
        // ADR-0054 d2, as amended): counts and bounds, never a query.
        let prev = self.head_event(admin.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            admin.tenant_id,
            event_types::TELEMETRY_EVENTS_READ,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "events": events.len(),
                "with_text": events.iter().filter(|e| e.query_text.is_some()).count(),
                "newest": events.first().map(|e| e.created_at.as_str().to_owned()),
                "oldest": events.last().map(|e| e.created_at.as_str().to_owned()),
            }),
        )?;
        self.commit(event, MetadataFold::AdminRead, None).await?;
        Ok(events)
    }

    /// `POST /v2/usage/eval`, admin by key role: each labelled query of the
    /// last 30 days, the newest [`REPLAY_QUERIES_MAX`] and for at most
    /// [`REPLAY_DEADLINE_SECS`], replayed through each of `pipelines`, none of
    /// them a reranking one, under its original key's scope as that key reads
    /// now: the key's record is read again, a revoked or vanished key's
    /// queries are skipped and counted, and the recall runs as that key's
    /// caller with its grants and denials of now, with no own held rows and
    /// no text, inside the lead, never presenting the key's secret; the answer holds numbers and
    /// counts alone, never a query, a hit or an id, and the replay records
    /// no event (ADR-0054 d11).
    pub async fn usage_eval(
        &self,
        bearer: impl Into<Bearer<'_>>,
        pipelines: &[String],
    ) -> Result<ReplayReport, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "usage_eval")?;
        let telemetry = self.telemetry.as_ref().ok_or(AppError::TelemetryOff)?;
        let mut chosen = Vec::with_capacity(pipelines.len());
        for name in pipelines {
            let pipeline = Pipeline::parse(name)
                .filter(|p| self.search.serves(*p))
                .ok_or_else(|| AppError::SearchUnavailable {
                    name: name.clone(),
                    served: self.served_pipelines(),
                })?;
            if pipeline.reranks() {
                return Err(AppError::ReplayReranks { name: name.clone() });
            }
            if !chosen.contains(&pipeline) {
                chosen.push(pipeline);
            }
        }
        telemetry.flush().await;
        let mut queries = telemetry.labelled_queries(admin.tenant_id).await?;
        let labelled = queries.len() as u64;
        // The newest REPLAY_QUERIES_MAX, oldest first as read.
        let mut capped = queries.len() > REPLAY_QUERIES_MAX;
        if capped {
            queries.drain(..queries.len() - REPLAY_QUERIES_MAX);
        }
        let considered = queries.len() as u64;
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(REPLAY_DEADLINE_SECS);
        let mut callers = BTreeMap::new();
        let mut runnable = Vec::new();
        for q in queries {
            if let std::collections::btree_map::Entry::Vacant(slot) = callers.entry(q.key_id) {
                let key = self.journal.key(admin.tenant_id, q.key_id).await?;
                let caller = match key {
                    Some(key) if key.revoked_at.is_none() => Some(self.caller_of(key).await?),
                    _ => None,
                };
                slot.insert(caller);
            }
            if callers.get(&q.key_id).is_some_and(Option::is_some) {
                runnable.push(q);
            }
        }
        let skipped_revoked = considered - runnable.len() as u64;
        let mut scores = Vec::with_capacity(chosen.len());
        for pipeline in chosen {
            let mut sums = [0.0_f64; 4];
            let mut ran = 0_u64;
            for q in &runnable {
                if std::time::Instant::now() >= deadline {
                    capped = true;
                    break;
                }
                let Some(Some(caller)) = callers.get(&q.key_id) else {
                    continue;
                };
                let req = RecallRequest {
                    query: q.query_text.clone(),
                    k: REPLAY_K,
                    workarea: Some(q.scope_workarea_id.into()),
                    kinds: Vec::new(),
                    include_text: false,
                    include_unreviewed: false,
                    max_tokens: None,
                    search: Some(pipeline.as_str().to_owned()),
                    match_mode: MatchMode::Any,
                    anchor: AnchorFilter::default(),
                };
                // A scope the key no longer reads answers nothing, which
                // the replay scores as a miss.
                let ranked: Vec<Uuid> = match self.recall_as(caller, &req).await {
                    Ok(result) => result
                        .hits
                        .iter()
                        .map(|h| h.served.memory.memory_id)
                        .collect(),
                    Err(AppError::Store(e)) => return Err(AppError::Store(e)),
                    Err(_) => Vec::new(),
                };
                for (sum, score) in sums.iter_mut().zip(score_query(&ranked, &q.grades)) {
                    *sum += score;
                }
                ran += 1;
            }
            let mean = |sum: f64| if ran == 0 { 0.0 } else { sum / ran as f64 };
            scores.push(ReplayScores {
                pipeline,
                queries: ran,
                success_at_1: mean(sums[0]),
                success_at_5: mean(sums[1]),
                ndcg_at_10: mean(sums[2]),
                recall_at_10: mean(sums[3]),
            });
        }
        // The replay is journalled: who ran it, through which pipelines, and
        // how many recalls; never a query (ADR-0054 d11, as amended).
        let prev = self.head_event(admin.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            admin.tenant_id,
            event_types::TELEMETRY_REPLAYED,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "pipelines": scores.iter().map(|p| p.pipeline.as_str()).collect::<Vec<_>>(),
                "labelled": labelled,
                "ran": scores.first().map_or(0, |p| p.queries),
                "capped": capped,
            }),
        )?;
        self.commit(event, MetadataFold::AdminRead, None).await?;
        Ok(ReplayReport {
            labelled,
            capped,
            skipped_revoked,
            window_days: RETENTION_DAYS,
            pipelines: scores,
        })
    }

    /// Hands a committed act that may label the acting key's recent recall
    /// to the writer, without waiting (ADR-0054 d5).
    pub(super) fn offer_act(&self, tenant_id: Uuid, key_id: Uuid, artifact_id: Uuid, label: Label) {
        if let Some(telemetry) = &self.telemetry {
            telemetry.offer(TelemetryItem::Act(crate::telemetry::Act {
                tenant_id,
                key_id,
                artifact_id,
                label,
                at: self.clock.now(),
            }));
        }
    }
}
