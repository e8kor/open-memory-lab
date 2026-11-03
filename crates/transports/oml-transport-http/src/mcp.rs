//! The agent plugin: MCP over streamable HTTP, stateless (docs/api/mcp-plugin.md,
//! ADR-0014). One POST carries one complete JSON-RPC request and receives one
//! complete JSON response; no `Mcp-Session-Id` is issued, and `GET` — the
//! server-initiated stream — is refused.
//!
//! Every tool translates to exactly one call on the HTTP API: the handler
//! builds that call's headers and body and runs the same handler the ring
//! route runs, so validation, idempotency and the typed refusals cannot drift
//! between the two surfaces. A workarea argument is passed through as sent,
//! an id or a name; the facade resolves nothing (mcp-plugin.md; ADR-0026
//! d4). The catalog served is the PoC slice of docs/api/mcp-tools.json — the
//! tools whose ring endpoint exists — and the schemas are read from that
//! file, its one declaration site.

use std::collections::HashMap;

use axum::body::{Body, Bytes, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use oml_app::domain::sha256_digest_string;
use oml_app::ports::{Clock, IdGen, JournalStore, Scanner, Signer};
use oml_app::types::AppError;

use crate::error::{ApiError, ErrorContext, map_app_error};
use crate::{ApiState, bearer, finish, json_response, new_request_id};

/// The MCP specification revision this facade answers `initialize` with.
/// A client asking for another revision is answered with this one, which the
/// specification permits; the client decides whether to proceed.
/// Verified against the published revision on 2026-10-30
/// (modelcontextprotocol.io/specification/2025-06-18, basic/transports and
/// server/tools).
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// `MCP-Protocol-Version` values a request may carry.
const ACCEPTED_PROTOCOL_VERSIONS: [&str; 2] = ["2025-06-18", "2025-03-26"];

/// The normative `tools/list` payload (docs/api/mcp-tools.json).
const CATALOG: &str = include_str!("../../../../docs/api/mcp-tools.json");

/// The catalog tools whose ring endpoint exists in this slice.
pub const SERVED_TOOLS: [&str; 8] = [
    "oml_remember",
    "oml_learn",
    "oml_recall",
    "oml_get",
    "oml_supersede",
    "oml_my_work",
    "oml_workarea_info",
    "oml_events_tail",
];

/// Catalog tools whose ring endpoint is not built yet: named, not unknown
/// (ADR-0025 d12).
const UNBUILT_TOOLS: [&str; 4] = ["oml_attach", "oml_relate_propose", "oml_graph", "oml_vocab"];

/// How a tool's refusal of arguments outside its input schema begins, as
/// distinct from any refusal the ring answers to well-typed arguments.
pub const ARGUMENTS_OUTSIDE_THE_SCHEMA: &str = "the arguments are not this tool's input schema";

// JSON-RPC 2.0 error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// The served subset of the catalog, in catalog order.
#[must_use]
pub fn served_catalog() -> Vec<Value> {
    let catalog: Value =
        serde_json::from_str(CATALOG).expect("docs/api/mcp-tools.json is valid JSON");
    catalog["tools"]
        .as_array()
        .expect("docs/api/mcp-tools.json carries a tools array")
        .iter()
        .filter(|t| {
            t["name"]
                .as_str()
                .is_some_and(|name| SERVED_TOOLS.contains(&name))
        })
        .cloned()
        .collect()
}

#[derive(Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Deserialize)]
struct ToolCall {
    name: String,
    #[serde(default)]
    arguments: Map<String, Value>,
}

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
struct UploadRef {
    upload_session: String,
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
struct WorkareaInfoArgs {
    workarea: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsTailArgs {
    scope: Option<String>,
    after_seq: Option<u64>,
    limit: Option<u64>,
}

impl<J, S, C, I, Sc> ApiState<J, S, C, I, Sc>
where
    J: JournalStore + Clone + 'static,
    S: Signer + 'static,
    C: Clock + 'static,
    I: IdGen + 'static,
    Sc: Scanner + 'static,
{
    pub(crate) async fn mcp(State(st): State<Self>, headers: HeaderMap, body: Bytes) -> Response {
        let rid = new_request_id();
        // No anonymous call, `tools/list` included: the key is resolved
        // before any message is read (mcp-plugin.md "Authentication").
        let token = match Self::mcp_key(&st, &headers).await {
            Ok(token) => token,
            Err(e) => return finish(&rid, Err(e)),
        };
        // A revision the facade does not speak is a 400; an absent header
        // means 2025-03-26, whose stateless JSON shape this one shares.
        if let Some(version) = headers.get("mcp-protocol-version") {
            let known = version
                .to_str()
                .is_ok_and(|v| ACCEPTED_PROTOCOL_VERSIONS.contains(&v));
            if !known {
                return finish(
                    &rid,
                    Err(ApiError::validation(format!(
                        "MCP-Protocol-Version must be one of {ACCEPTED_PROTOCOL_VERSIONS:?}"
                    ))),
                );
            }
        }
        let Ok(message) = serde_json::from_slice::<Value>(&body) else {
            return finish(
                &rid,
                Ok(rpc_error(&Value::Null, PARSE_ERROR, "parse error")),
            );
        };
        if message.is_array() {
            return finish(
                &rid,
                Ok(rpc_error(
                    &Value::Null,
                    INVALID_REQUEST,
                    "one JSON-RPC message per POST; batches are not accepted",
                )),
            );
        }
        let Ok(request) = serde_json::from_value::<RpcRequest>(message) else {
            return finish(
                &rid,
                Ok(rpc_error(
                    &Value::Null,
                    INVALID_REQUEST,
                    "not a JSON-RPC request",
                )),
            );
        };
        if request.jsonrpc != "2.0" {
            let id = request.id.unwrap_or(Value::Null);
            return finish(
                &rid,
                Ok(rpc_error(&id, INVALID_REQUEST, "jsonrpc must be \"2.0\"")),
            );
        }
        // A notification (no id) is acknowledged and nothing is stored.
        let Some(id) = request.id else {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::ACCEPTED;
            return finish(&rid, Ok(response));
        };
        let response = match request.method.as_str() {
            "initialize" => rpc_result(
                &id,
                &json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "oml-server", "version": env!("CARGO_PKG_VERSION") },
                }),
            ),
            "ping" => rpc_result(&id, &json!({})),
            "tools/list" => rpc_result(&id, &json!({ "tools": served_catalog() })),
            "tools/call" => match serde_json::from_value::<ToolCall>(request.params) {
                Err(_) => rpc_error(&id, INVALID_PARAMS, "tools/call takes {name, arguments}"),
                Ok(call) => Self::call_tool(&st, &token, &id, call).await,
            },
            other => rpc_error(&id, METHOD_NOT_FOUND, &format!("method not found: {other}")),
        };
        finish(&rid, Ok(response))
    }

    /// The bearer key, resolved against the journal's key fold.
    async fn mcp_key(st: &Self, headers: &HeaderMap) -> Result<String, ApiError> {
        let token = bearer(headers)?;
        st.store
            .key_by_hash(&sha256_digest_string(token.as_bytes()))
            .await
            .map_err(|e| map_app_error(AppError::Store(e), ErrorContext::Write))?
            .ok_or_else(|| ApiError::auth_invalid("unknown"))?;
        Ok(token)
    }

    async fn call_tool(st: &Self, token: &str, id: &Value, call: ToolCall) -> Response {
        let name = call.name.as_str();
        let args = Value::Object(call.arguments);
        let outcome = match name {
            "oml_remember" => Self::tool_submit(st, token, "remember", args).await,
            "oml_learn" => Self::tool_submit(st, token, "learn", args).await,
            "oml_recall" => Self::tool_recall(st, token, args).await,
            "oml_get" => Self::tool_get(st, token, args).await,
            "oml_supersede" => Self::tool_supersede(st, token, args).await,
            "oml_my_work" => Self::tool_my_work(st, token, args).await,
            "oml_workarea_info" => Self::tool_workarea_info(st, token, args).await,
            "oml_events_tail" => Self::tool_events_tail(st, token, args).await,
            _ if UNBUILT_TOOLS.contains(&name) => {
                return rpc_error(
                    id,
                    INVALID_PARAMS,
                    &format!(
                        "{name} is in the catalog (docs/api/mcp-tools.json) but not built yet; tools/list names the tools this server serves."
                    ),
                );
            }
            _ => {
                return rpc_error(
                    id,
                    INVALID_PARAMS,
                    &format!(
                        "Unknown tool: {name}. This surface carries no promote, sign, demote, delete or admin verb (R1); see tools/list for the complete catalog."
                    ),
                );
            }
        };
        let (structured, is_error) = match outcome {
            Ok(value) => (value, false),
            Err(refusal) => (refusal, true),
        };
        rpc_result(
            id,
            &json!({
                "content": [{ "type": "text", "text": structured.to_string() }],
                "structuredContent": structured,
                "isError": is_error,
            }),
        )
    }

    /// `oml_remember` / `oml_learn`: one `POST /v1/memories`.
    async fn tool_submit(st: &Self, token: &str, verb: &str, args: Value) -> Result<Value, Value> {
        let args: RememberArgs = parse_args(args)?;
        let mut headers = ring_headers(token);
        insert_header(&mut headers, "idempotency-key", &args.idempotency_key)?;
        if let Some(workarea) = &args.workarea {
            insert_header(&mut headers, "x-oml-workarea", workarea)?;
        }
        let mut body = json!({
            "verb": verb,
            "claim_key": args.claim_key,
            "kind": args.kind.unwrap_or_else(|| "memory".to_owned()),
            "body": args.text,
            "attachments": args.attachments.into_iter().map(|a| a.upload_session).collect::<Vec<_>>(),
            "derives_from": args.derives_from,
        });
        if let Some(obj) = body.as_object_mut() {
            if let Some(sim) = args.simulation_id {
                obj.insert("simulation_id".to_owned(), Value::String(sim));
            }
            if let Some(at) = args.simulated_time {
                obj.insert("simulated_time".to_owned(), Value::String(at));
            }
        }
        let response = Self::submit_inner(st, &headers, &Bytes::from(body.to_string())).await;
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

    /// `oml_recall`: one `POST /v1/memories/recall`. `include_own_held`,
    /// `max_tokens` and `k` pass through only when sent, so the ring applies
    /// its defaults, its range checks and its query digest to exactly what
    /// the agent asked (retrieval.md own held rows and token budgets;
    /// ADR-0025 d4, d7, d9, d12).
    async fn tool_recall(st: &Self, token: &str, args: Value) -> Result<Value, Value> {
        let args: RecallArgs = parse_args(args)?;
        let mut headers = ring_headers(token);
        if let Some(scope) = &args.scope {
            insert_header(&mut headers, "x-oml-workarea", scope)?;
        }
        let mut body = Map::new();
        body.insert("query".to_owned(), Value::String(args.query));
        for (name, value) in [
            ("k", args.k.map(Value::from)),
            ("include_own_held", args.include_own_held.map(Value::from)),
            ("max_tokens", args.max_tokens.map(Value::from)),
        ] {
            if let Some(value) = value {
                body.insert(name.to_owned(), value);
            }
        }
        let result = ring_outcome(
            Self::recall_inner(st, &headers, &Bytes::from(Value::Object(body).to_string())).await,
        )
        .await?;
        // Steering items and hits are one claim-item shape with text; only a
        // hit carries a score (mcp-plugin.md, the claim item; ADR-0025 d12).
        let steering: Vec<Value> = result["steering"]
            .as_array()
            .map(|items| items.iter().map(|m| recall_item(m, None)).collect())
            .unwrap_or_default();
        let hits: Vec<Value> = result["hits"]
            .as_array()
            .map(|hits| {
                hits.iter()
                    .map(|hit| recall_item(&hit["memory"], Some(&hit["score"])))
                    .collect()
            })
            .unwrap_or_default();
        let mut out = json!({ "steering": steering, "hits": hits });
        // The ring's budget object, present if and only if max_tokens was
        // sent (retrieval.md token budgets; ADR-0025 d7).
        if let (Some(budget), Some(obj)) = (result.get("budget"), out.as_object_mut()) {
            obj.insert("budget".to_owned(), budget.clone());
        }
        Ok(out)
    }

    /// `oml_get`: one `GET /v1/memories/{artifact_id}`. The item is the
    /// claim item with `text`, `derives_from` and `attachments`, whose
    /// `derives_from` and `replaces` the ring has already limited to what
    /// the key can read (mcp-plugin.md `oml_get`; ADR-0025 d3, d12).
    async fn tool_get(st: &Self, token: &str, args: Value) -> Result<Value, Value> {
        let args: GetArgs = parse_args(args)?;
        let memory =
            ring_outcome(Self::get_memory_inner(st, &args.artifact_id, &ring_headers(token)).await)
                .await?;
        let mut item = claim_item(&memory);
        for (from, to) in [
            ("body", "text"),
            ("derives_from", "derives_from"),
            ("attachments", "attachments"),
        ] {
            item.insert(to.to_owned(), memory[from].clone());
        }
        Ok(Value::Object(item))
    }

    /// `oml_supersede`: one `POST /v1/memories/{artifact_id}/supersede`.
    /// `idempotency_key` is the `Idempotency-Key` header and `text` the
    /// body; `retirement`, `kind`, `derives_from`, `reason` and the
    /// simulation fields pass through only when sent, so the ring applies
    /// its own defaults. The tool carries no vocabulary fields, so a target
    /// of a vocabulary kind is refused at step 5 of the supersede rule
    /// (mcp-plugin.md `oml_supersede`; ADR-0025 d10, d11, d12).
    async fn tool_supersede(st: &Self, token: &str, args: Value) -> Result<Value, Value> {
        let args: SupersedeArgs = parse_args(args)?;
        let mut headers = ring_headers(token);
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
    /// listing. `workarea` is the ring's `workarea_id` subtree filter, and
    /// `state`, `claim_key`, `limit` and the client-held `cursor` pass
    /// through; each item is the claim item without `text`, plus
    /// `held_expires_at` (mcp-plugin.md `oml_my_work`; ADR-0025 d12, d13).
    async fn tool_my_work(st: &Self, token: &str, args: Value) -> Result<Value, Value> {
        let args: MyWorkArgs = parse_args(args)?;
        let mut params = vec![("own".to_owned(), "true".to_owned())];
        for (name, value) in [
            ("workarea_id", args.workarea),
            ("claim_key", args.claim_key),
            ("state", args.state),
            ("limit", args.limit.map(|l| l.to_string())),
            ("cursor", args.cursor),
        ] {
            if let Some(value) = value {
                params.push((name.to_owned(), value));
            }
        }
        let page = ring_outcome(Self::list_memories_inner(st, &params, &ring_headers(token)).await)
            .await?;
        let items: Vec<Value> = page["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|m| {
                        let mut item = claim_item(m);
                        item.insert("held_expires_at".to_owned(), m["held_expires_at"].clone());
                        Value::Object(item)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({ "items": items, "next_cursor": page["next_cursor"] }))
    }

    /// `oml_workarea_info`: one `GET /v2/workareas/{id}`, so the tool answers
    /// exactly what the ring answers; `workarea` is the node's id or name,
    /// which the ring classifies and the app resolves (ADR-0026 d4). An
    /// omitted workarea is the key's journalled default, which the app
    /// resolves from the authenticated key record after step 1 of the
    /// refusal order; a key past step 1 with none is
    /// `request.workarea_unresolved`, and an admin key, which the ring's read
    /// answers with structure by its key role, reads its default the same
    /// way (mcp-plugin.md; roles-and-keys.md refusal order; ADR-0025 d14;
    /// ADR-0026 d15).
    async fn tool_workarea_info(st: &Self, token: &str, args: Value) -> Result<Value, Value> {
        let args: WorkareaInfoArgs = parse_args(args)?;
        let view = ring_outcome(
            Self::get_workarea_inner(st, args.workarea.as_deref(), &ring_headers(token)).await,
        )
        .await?;
        // The plugin's output names the parent `parent`; `crew` is null when
        // the workarea names none (mcp-plugin.md `oml_workarea_info`;
        // ADR-0026 d1, d8).
        let mut out = Map::new();
        for (from, to) in [
            ("id", "id"),
            ("name", "name"),
            ("path", "path"),
            ("status", "status"),
            ("crew", "crew"),
            ("parent_id", "parent"),
            ("children", "children"),
            ("edges", "edges"),
            ("my_effective_verbs", "my_effective_verbs"),
            ("permission_epoch", "permission_epoch"),
        ] {
            if let Some(value) = view.get(from) {
                out.insert(to.to_owned(), value.clone());
            }
        }
        Ok(Value::Object(out))
    }

    /// `oml_events_tail`: one `GET /v2/events`. `scope` is the ring's
    /// `workarea_id`; omitted, the app fills it from the key record with the
    /// journalled default, as the ring does. An omitted `after_seq` is 0, as
    /// on the ring. The page is projected to `{events, head_seq,
    /// next_after_seq, idle}`, idle once the resume point has reached the
    /// head (mcp-plugin.md `oml_events_tail`; ADR-0025 d15).
    async fn tool_events_tail(st: &Self, token: &str, args: Value) -> Result<Value, Value> {
        let args: EventsTailArgs = parse_args(args)?;
        let mut params = HashMap::from([(
            "after_seq".to_owned(),
            args.after_seq.unwrap_or(0).to_string(),
        )]);
        if let Some(scope) = args.scope {
            params.insert("workarea_id".to_owned(), scope);
        }
        if let Some(limit) = args.limit {
            params.insert("limit".to_owned(), limit.to_string());
        }
        let page =
            ring_outcome(Self::events_tail_inner(st, &params, &ring_headers(token)).await).await?;
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

/// `GET /mcp`: the server-initiated stream does not exist in stateless mode.
pub(crate) async fn mcp_get() -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST"));
    response
}

/// The base claim item of a ring `Memory` or `MemorySummary`: the one shape
/// every tool returns a claim version in, the ring's `workarea_id` named
/// `workarea` (mcp-plugin.md, the claim item; ADR-0025 d12).
fn claim_item(m: &Value) -> Map<String, Value> {
    [
        ("artifact_id", "artifact_id"),
        ("claim_key", "claim_key"),
        ("version", "version"),
        ("kind", "kind"),
        ("state", "state"),
        ("tip", "tip"),
        ("retirement", "retirement"),
        ("replaces", "replaces"),
        ("workarea_id", "workarea"),
        ("workarea_path", "workarea_path"),
        ("tokens", "tokens"),
        ("created_at", "created_at"),
        ("promoted_at", "promoted_at"),
    ]
    .into_iter()
    .map(|(from, to)| (to.to_owned(), m[from].clone()))
    .collect()
}

/// A ring `Memory` as an `oml_recall` item: the claim item plus its `body`
/// as `text`, and a hit's `score`; a steering item has none (mcp-plugin.md,
/// the claim item; ADR-0025 d12).
fn recall_item(m: &Value, score: Option<&Value>) -> Value {
    let mut item = claim_item(m);
    item.insert("text".to_owned(), m["body"].clone());
    if let Some(score) = score {
        item.insert("score".to_owned(), score.clone());
    }
    Value::Object(item)
}

fn ring_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
        headers.insert(header::AUTHORIZATION, value);
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers
}

fn insert_header(headers: &mut HeaderMap, name: &'static str, value: &str) -> Result<(), Value> {
    let value = HeaderValue::from_str(value).map_err(|_| {
        refusal(ApiError::validation(format!(
            "{name} must be visible ASCII"
        )))
    })?;
    headers.insert(name, value);
    Ok(())
}

fn parse_args<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, Value> {
    serde_json::from_value(args).map_err(|e| {
        refusal(ApiError::validation(format!(
            "{ARGUMENTS_OUTSIDE_THE_SCHEMA}: {e}"
        )))
    })
}

/// A typed refusal as the tool result's structured content: the same body
/// the HTTP API answers with (errors.md envelope).
fn refusal(error: ApiError) -> Value {
    json!({ "error": error.error_object() })
}

/// The ring handler's response, as the success body or the refusal body.
async fn ring_outcome(response: Result<Response, ApiError>) -> Result<Value, Value> {
    let response = response.map_err(refusal)?;
    let ok = response.status().is_success();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|_| refusal(ApiError::internal("the ring response body was unreadable")))?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| refusal(ApiError::internal("the ring response body was not JSON")))?;
    if ok { Ok(value) } else { Err(value) }
}

fn rpc_result(id: &Value, result: &Value) -> Response {
    json_response(
        StatusCode::OK,
        &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    )
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Response {
    json_response(
        StatusCode::OK,
        &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    )
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
