//! `GET /v2/admin/rescan` (ADR-0065 d13): an admin key rescans the versions
//! its tenant stores, a window of the journal at a time, and reads what the
//! scanner would refuse or hold by rule and place, never by value. It
//! changes nothing and journals nothing.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{Value, json};

use oml_app::app::{RESCAN_MAX_EVENTS, RescanReport};
use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer, WebStore};

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::traffic::Rid;
use crate::{ApiState, bearer, finish, json_response, retry_chain};

/// openapi.yaml `RescanReport`.
fn report_json(r: &RescanReport, fixes: &oml_app::app::FixStats) -> Value {
    let action = |veto: bool| if veto { "veto" } else { "hold" };
    json!({
        "scanner_version": r.scanner_version,
        "scanned": r.scanned,
        "findings": r.hits.iter().map(|h| json!({
            "artifact_id": h.artifact_id.to_string(),
            "workspace_id": h.workspace_id.to_string(),
            "version": h.version,
            "state": h.state.as_str(),
            "class": h.finding.class,
            "rule": h.finding.rule,
            "action": action(h.finding.veto),
            "at": h.finding.at,
            "field": h.field,
            "origin": h.origin,
        })).collect::<Vec<_>>(),
        "scanned_by_origin": r.scanned_by_origin,
        "by_rule": r.by_rule.iter().map(|(class, rule, veto, count)| json!({
            "class": class, "rule": rule, "action": action(*veto), "count": count,
        })).collect::<Vec<_>>(),
        "next_after_seq": r.next_after_seq,
        "recorded": r.recorded,
        "held_unrescanned": r.held_unrescanned,
        "rescan_due": r.rescan_due,
        "findings_unlisted": r.findings_unlisted,
        "promoted_unapproved": r.promoted_unapproved,
        "withdrawn_in_queue": r.withdrawn_in_queue,
        "instructions_without_person_approval": r.instructions_without_person_approval,
        "held_successors_stranded": r.held_successors_stranded,
        "erased_keys_live": r.erased_keys_live,
        "investigations_open_long": r.investigations_open_long,
        "fixes_unconfirmed": fixes.unconfirmed,
        "fix_oldest_unconfirmed_seconds": fixes.oldest_unconfirmed_seconds,
        "fixes_loosening_guardrail": fixes.loosening_guardrail,
        "fixes_without_person_reviewer": fixes.without_person_reviewer,
        "fixes_admin_person_only": fixes.admin_person_only,
        "fixes_reverted": fixes.reverted,
        "key_links_pending": fixes.links_pending,
        "web_emails_pending": fixes.emails_pending,
    })
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + WebStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// `GET /v2/admin/rescan?after_seq=&limit=`, admin by key role.
    pub(crate) async fn rescan_route(
        State(st): State<Self>,
        rid: Rid,
        Query(params): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        Self::rescan_call(&st, &rid, &params, &headers, false).await
    }

    /// `POST /v2/admin/rescan?after_seq=&limit=`, admin by key role: the
    /// read, and the window's held versions' findings recorded under the
    /// server's rule set (ADR-0065 d15; ADR-0074 d28). No Idempotency-Key:
    /// a repeat re-reads and re-journals, and leaves the same state.
    pub(crate) async fn rescan_record_route(
        State(st): State<Self>,
        rid: Rid,
        Query(params): Query<HashMap<String, String>>,
        headers: HeaderMap,
    ) -> Response {
        Self::rescan_call(&st, &rid, &params, &headers, true).await
    }

    async fn rescan_call(
        st: &Self,
        rid: &Rid,
        params: &HashMap<String, String>,
        headers: &HeaderMap,
        record_held: bool,
    ) -> Response {
        let result = async {
            if record_held {
                st.require_writes_open()?;
            }
            let token = bearer(headers)?;
            let after_seq: u64 = match params.get("after_seq") {
                None => 0,
                Some(s) => s.parse().map_err(|_| {
                    ApiError::validation("after_seq must be a non-negative integer")
                })?,
            };
            let limit: usize = match params.get("limit") {
                None => RESCAN_MAX_EVENTS,
                Some(s) => s
                    .parse()
                    .ok()
                    .filter(|l| (1..=RESCAN_MAX_EVENTS).contains(l))
                    .ok_or_else(|| {
                        ApiError::validation(format!(
                            "limit must be between 1 and {RESCAN_MAX_EVENTS}"
                        ))
                    })?,
            };
            let report = if record_held {
                retry_chain(|| st.app.rescan_record_held(&token, after_seq, limit)).await
            } else {
                st.app.rescan(&token, after_seq, limit).await
            }
            .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            let fixes = st
                .app
                .admin_fix_stats(&token)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &report_json(&report, &fixes)))
        }
        .await;
        finish(rid, result)
    }
}
