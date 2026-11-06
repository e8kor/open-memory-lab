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
use crate::{ApiState, bearer, finish, json_response};

/// openapi.yaml `RescanReport`.
fn report_json(r: &RescanReport) -> Value {
    let action = |veto: bool| if veto { "veto" } else { "hold" };
    json!({
        "scanner_version": r.scanner_version,
        "scanned": r.scanned,
        "findings": r.hits.iter().map(|h| json!({
            "artifact_id": h.artifact_id.to_string(),
            "workarea_id": h.workarea_id.to_string(),
            "version": h.version,
            "state": h.state.as_str(),
            "class": h.finding.class,
            "rule": h.finding.rule,
            "action": action(h.finding.veto),
            "at": h.finding.at,
        })).collect::<Vec<_>>(),
        "by_rule": r.by_rule.iter().map(|(class, rule, veto, count)| json!({
            "class": class, "rule": rule, "action": action(*veto), "count": count,
        })).collect::<Vec<_>>(),
        "next_after_seq": r.next_after_seq,
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
        let result = async {
            let token = bearer(&headers)?;
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
            let report = st
                .app
                .rescan(&token, after_seq, limit)
                .await
                .map_err(|e| map_app_error(e, ErrorContext::Write))?;
            Ok(json_response(StatusCode::OK, &report_json(&report)))
        }
        .await;
        finish(&rid, result)
    }
}
