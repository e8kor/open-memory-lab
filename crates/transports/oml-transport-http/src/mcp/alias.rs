//! The hidden aliases: the eight tool names v0.3.0 served, callable through
//! `tools/call` and named nowhere in `tools/list`, through v0.4.x as ADR-0039
//! d12 reads its alias window; v0.5.0 removes them (docs/api/mcp-plugin.md,
//! "The v0.3.0 tools, not listed").
//!
//! Each alias takes its v0.3.0 input schema and answers its v0.3.0 output
//! shape and refusal codes, translated at the facade onto the handlers its
//! successor rides: `scope` is the ring's `workarea`, `include_own_held` its
//! `include_unreviewed`, and the caller still sends `claim_key`. Each call
//! logs the event `mcp_alias_called` naming the tool, its successor, the
//! calling key's `key_id` and `tenant_id` and the call's `request_id`, ids
//! only and never the key's value (observability.md, structured logging), so
//! an operator can see whose clients still call an old name before v0.5.0
//! removes it.

use std::collections::HashMap;

use uuid::Uuid;

use axum::body::Bytes;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use oml_app::ports::{Clock, ConnectionStore, IdGen, JournalStore, Scanner, Signer};

use super::{
    Presented, get_item, insert_header, listing_output, parse_args, recall_output, ring_headers,
    ring_outcome,
};
use crate::ApiState;

/// Each v0.3.0 tool name and its successor, which `mcp_alias_called` names
/// (ADR-0039 d12; mcp-plugin.md "What left the catalog"). The events tail
/// left MCP, so its successor is the HTTP route the alias rides.
pub const ALIASES: [(&str, &str); 8] = [
    ("oml_remember", "oml_write"),
    ("oml_learn", "oml_write"),
    ("oml_recall", "oml_read"),
    ("oml_get", "oml_read"),
    ("oml_supersede", "oml_write"),
    ("oml_my_work", "oml_read"),
    ("oml_workarea_info", "oml_workarea"),
    ("oml_events_tail", "GET /v2/events"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RememberArgs {
    workarea: Option<String>,
    claim_key: String,
    text: String,
    kind: Option<String>,
    #[serde(default)]
    attachments: Vec<UploadRef>,
    #[serde(default)]
    derives_from: Vec<String>,
    simulation_id: Option<String>,
    simulated_time: Option<String>,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecallArgs {
    query: String,
    scope: Option<String>,
    k: Option<u64>,
    include_own_held: Option<bool>,
    max_tokens: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArgs {
    artifact_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SupersedeArgs {
    artifact_id: String,
    text: String,
    retirement: Option<bool>,
    kind: Option<String>,
    derives_from: Option<Vec<String>>,
    reason: Option<String>,
    simulation_id: Option<String>,
    simulated_time: Option<String>,
    idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MyWorkArgs {
    workarea: Option<String>,
    claim_key: Option<String>,
    state: Option<String>,
    limit: Option<u64>,
    cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsTailArgs {
    scope: Option<String>,
    after_seq: Option<u64>,
    limit: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadRef {
    upload_session: String,
}

/// The stable `event` of the line each alias call logs (observability.md,
/// structured logging; ADR-0039 d12).
const EVENT_MCP_ALIAS_CALLED: &str = "mcp_alias_called";

/// Who made an alias call, as its log line names them: the resolved key's
/// ids and the call's request id, never the key's value.
pub(super) struct Caller<'a> {
    pub(super) key_id: Uuid,
    pub(super) tenant_id: Uuid,
    pub(super) request_id: &'a str,
}

/// The successor of `name` when it is a v0.3.0 tool name, and `None` for
/// any other name.
pub(super) fn successor(name: &str) -> Option<&'static str> {
    ALIASES
        .into_iter()
        .find_map(|(alias, successor)| (alias == name).then_some(successor))
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + ConnectionStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    /// One call to `name`, a v0.3.0 tool name, logged as `mcp_alias_called`
    /// naming `successor` and `caller` whatever its outcome (ADR-0039 d12).
    pub(super) async fn call_alias(
        st: &Self,
        presented: Presented<'_>,
        caller: &Caller<'_>,
        name: &str,
        successor: &str,
        args: Value,
    ) -> Result<Value, Value> {
        tracing::info!(
            event = EVENT_MCP_ALIAS_CALLED,
            tool = %name,
            successor = %successor,
            key_id = %caller.key_id,
            tenant_id = %caller.tenant_id,
            request_id = %caller.request_id,
            "mcp_alias_called: {name} is a v0.3.0 tool name, which v0.5.0 removes; its successor is {successor}"
        );
        match name {
            "oml_remember" => Self::tool_submit(st, presented, "remember", args).await,
            "oml_learn" => Self::tool_submit(st, presented, "learn", args).await,
            "oml_recall" => Self::tool_recall(st, presented, args).await,
            "oml_get" => Self::tool_get(st, presented, args).await,
            "oml_supersede" => Self::tool_supersede(st, presented, args).await,
            "oml_my_work" => Self::tool_my_work(st, presented, args).await,
            // The v0.3.0 shape: the fields v0.5.2 added to `oml_workarea`
            // stay off the old name (ADR-0039 d12; ADR-0046 d9).
            "oml_workarea_info" => {
                Self::tool_workarea_info(st, presented, args)
                    .await
                    .map(|mut node| {
                        if let Some(obj) = node.as_object_mut() {
                            for added in ["policy", "pending_transitions", "brief"] {
                                obj.remove(added);
                            }
                        }
                        node
                    })
            }
            _ => Self::tool_events_tail(st, presented, args).await,
        }
    }

    /// `oml_remember` / `oml_learn`: one `POST /v1/memories`.
    async fn tool_submit(
        st: &Self,
        presented: Presented<'_>,
        verb: &str,
        args: Value,
    ) -> Result<Value, Value> {
        let args: RememberArgs = parse_args(args)?;
        let mut headers = ring_headers(presented);
        insert_header(&mut headers, "idempotency-key", &args.idempotency_key)?;
        let mut body = json!({
            "verb": verb,
            "claim_key": args.claim_key,
            "kind": args.kind.unwrap_or_else(|| "memory".to_owned()),
            "body": args.text,
            "attachments": args.attachments.into_iter().map(|a| a.upload_session).collect::<Vec<_>>(),
            "derives_from": args.derives_from,
        });
        if let Some(obj) = body.as_object_mut() {
            if let Some(workarea) = args.workarea {
                obj.insert("workarea".to_owned(), Value::String(workarea));
            }
            if let Some(sim) = args.simulation_id {
                obj.insert("simulation_id".to_owned(), Value::String(sim));
            }
            if let Some(at) = args.simulated_time {
                obj.insert("simulated_time".to_owned(), Value::String(at));
            }
        }
        let response = Self::submit_inner(
            st,
            &headers,
            &Bytes::from(body.to_string()),
            presented.connection,
        )
        .await;
        let replayed = response
            .as_ref()
            .is_ok_and(|r| r.headers().contains_key("idempotency-replay"));
        let receipt = ring_outcome(response).await?;
        Ok(json!({
            "artifact_id": receipt["artifact_id"],
            "claim_key": receipt["claim_key"],
            "version": receipt["version"],
            "state": receipt["state"],
            "workarea": receipt["workarea_id"],
            "attachments_bound": 0,
            "idempotent_replay": replayed,
        }))
    }

    /// `oml_recall`: one `POST /v1/memories/recall`. `scope` is the ring's
    /// `workarea` and `include_own_held` its `include_unreviewed` (ADR-0039
    /// d7); they, `max_tokens` and `k` pass through only when sent, so the
    /// ring applies
    /// its defaults, its range checks and its query digest to exactly what
    /// the agent asked (retrieval.md own held rows and token budgets;
    /// ADR-0025 d4, d7, d9, d12).
    async fn tool_recall(st: &Self, presented: Presented<'_>, args: Value) -> Result<Value, Value> {
        let args: RecallArgs = parse_args(args)?;
        let headers = ring_headers(presented);
        let mut body = Map::new();
        body.insert("query".to_owned(), Value::String(args.query));
        for (name, value) in [
            ("workarea", args.scope.map(Value::from)),
            ("k", args.k.map(Value::from)),
            ("include_unreviewed", args.include_own_held.map(Value::from)),
            ("max_tokens", args.max_tokens.map(Value::from)),
        ] {
            if let Some(value) = value {
                body.insert(name.to_owned(), value);
            }
        }
        let result = ring_outcome(
            Self::recall_inner(
                st,
                &headers,
                &Bytes::from(Value::Object(body).to_string()),
                presented.connection,
            )
            .await,
        )
        .await?;
        Ok(recall_output(&result))
    }

    /// `oml_get`: one `GET /v1/memories/{artifact_id}`. The item is the
    /// claim item with `text`, `derives_from` and `attachments`, whose
    /// `derives_from` and `replaces` the ring has already limited to what
    /// the key can read (mcp-plugin.md `oml_get`; ADR-0025 d3, d12).
    async fn tool_get(st: &Self, presented: Presented<'_>, args: Value) -> Result<Value, Value> {
        let args: GetArgs = parse_args(args)?;
        let memory = ring_outcome(
            Self::get_memory_inner(
                st,
                &args.artifact_id,
                &ring_headers(presented),
                presented.connection,
            )
            .await,
        )
        .await?;
        Ok(get_item(&memory))
    }

    /// `oml_supersede`: one `POST /v1/memories/{artifact_id}/supersede`.
    /// `idempotency_key` is the `Idempotency-Key` header and `text` the
    /// body; `retirement`, `kind`, `derives_from`, `reason` and the
    /// simulation fields pass through only when sent, so the ring applies
    /// its own defaults. The tool carries no vocabulary fields, so a target
    /// of a vocabulary kind is refused at step 5 of the supersede rule
    /// (mcp-plugin.md `oml_supersede`; ADR-0025 d10, d11, d12).
    async fn tool_supersede(
        st: &Self,
        presented: Presented<'_>,
        args: Value,
    ) -> Result<Value, Value> {
        let args: SupersedeArgs = parse_args(args)?;
        let mut headers = ring_headers(presented);
        insert_header(&mut headers, "idempotency-key", &args.idempotency_key)?;
        let mut body = Map::new();
        body.insert("body".to_owned(), Value::String(args.text));
        for (name, value) in [
            ("retirement", args.retirement.map(Value::from)),
            ("kind", args.kind.map(Value::from)),
            ("derives_from", args.derives_from.map(Value::from)),
            ("reason", args.reason.map(Value::from)),
            ("simulation_id", args.simulation_id.map(Value::from)),
            ("simulated_time", args.simulated_time.map(Value::from)),
        ] {
            if let Some(value) = value {
                body.insert(name.to_owned(), value);
            }
        }
        let response = Self::supersede_inner(
            st,
            &args.artifact_id,
            &headers,
            &Bytes::from(Value::Object(body).to_string()),
            true,
            presented.connection,
        )
        .await;
        let replayed = response
            .as_ref()
            .is_ok_and(|r| r.headers().contains_key("idempotency-replay"));
        let receipt = ring_outcome(response).await?;
        Ok(json!({
            "artifact_id": receipt["artifact_id"],
            "claim_key": receipt["claim_key"],
            "version": receipt["version"],
            "state": receipt["state"],
            "retirement": receipt["retirement"],
            "replaces": receipt["replaces"],
            "workarea": receipt["workarea_id"],
            "idempotent_replay": replayed,
        }))
    }

    /// `oml_my_work`: one `GET /v1/memories?own=true`, the own-work
    /// listing. `workarea` is the ring's `workarea`, narrowing it to a
    /// subtree, and
    /// `state`, `claim_key`, `limit` and the client-held `cursor` pass
    /// through; each item is the claim item without `text`, plus
    /// `held_expires_at` (mcp-plugin.md `oml_my_work`; ADR-0025 d12, d13).
    async fn tool_my_work(
        st: &Self,
        presented: Presented<'_>,
        args: Value,
    ) -> Result<Value, Value> {
        let args: MyWorkArgs = parse_args(args)?;
        let mut params = vec![("own".to_owned(), "true".to_owned())];
        for (name, value) in [
            ("workarea", args.workarea),
            ("claim_key", args.claim_key),
            ("state", args.state),
            ("limit", args.limit.map(|l| l.to_string())),
            ("cursor", args.cursor),
        ] {
            if let Some(value) = value {
                params.push((name.to_owned(), value));
            }
        }
        let page = ring_outcome(
            Self::list_memories_inner(
                st,
                &params,
                &ring_headers(presented),
                false,
                presented.connection,
            )
            .await,
        )
        .await?;
        Ok(listing_output(&page, false))
    }

    /// `oml_events_tail`: one `GET /v2/events`. `scope` is the ring's
    /// `workarea`; omitted, the app fills it from the key record with the
    /// journalled default, as the ring does. An omitted `after_seq` is 0, as
    /// on the ring. The page is projected to `{events, head_seq,
    /// next_after_seq, idle}`, idle once the resume point has reached the
    /// head (mcp-plugin.md `oml_events_tail`; ADR-0025 d15).
    async fn tool_events_tail(
        st: &Self,
        presented: Presented<'_>,
        args: Value,
    ) -> Result<Value, Value> {
        let args: EventsTailArgs = parse_args(args)?;
        let mut params = HashMap::from([(
            "after_seq".to_owned(),
            args.after_seq.unwrap_or(0).to_string(),
        )]);
        if let Some(scope) = args.scope {
            params.insert("workarea".to_owned(), scope);
        }
        if let Some(limit) = args.limit {
            params.insert("limit".to_owned(), limit.to_string());
        }
        let page = ring_outcome(
            Self::events_tail_inner(st, &params, &ring_headers(presented), presented.connection)
                .await,
        )
        .await?;
        let events: Vec<Value> = page["items"]
            .as_array()
            .map(|items| items.iter().map(tail_item).collect())
            .unwrap_or_default();
        Ok(json!({
            "events": events,
            "head_seq": page["head_seq"],
            "next_after_seq": page["next_after_seq"],
            "idle": page["next_after_seq"] == page["head_seq"],
        }))
    }
}

/// A served ring event as an `oml_events_tail` event (mcp-plugin.md
/// `oml_events_tail`; ADR-0025 d15): its `subject` — `{artifact_id,
/// claim_key, version}` for a claim version, the new version for a
/// `lifecycle.supersede`, `{workarea_id}` for a `workarea.*` node — and
/// `workarea`, the subject's workarea as the served payload names it, null
/// where the naming rule withholds it. Hashes, signatures, payloads and
/// identities are dropped. This build journals no `lifecycle.tip_override`,
/// `lifecycle.reassign` or `workarea.relate_*` event, so none is projected.
fn tail_item(event: &Value) -> Value {
    let payload = &event["payload"];
    let subject = match event["event_type"].as_str() {
        Some(t) if t.starts_with("workarea.") => json!({ "workarea_id": payload["workarea_id"] }),
        Some("lifecycle.supersede") => json!({
            "artifact_id": payload["new_artifact_id"],
            "claim_key": payload["claim_key"],
            "version": payload["new_version"],
        }),
        _ => json!({
            "artifact_id": payload["artifact_id"],
            "claim_key": payload["claim_key"],
            "version": payload["version"],
        }),
    };
    json!({
        "seq": event["seq"],
        "event_type": event["event_type"],
        "at": event["at"],
        "subject": subject,
        "workarea": payload["workarea_id"],
    })
}
