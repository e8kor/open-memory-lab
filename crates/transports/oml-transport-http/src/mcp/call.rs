//! The `mcp_call` line: one per `/mcp` POST that carries a JSON-RPC
//! message, written before that request's `http_request` line, under its
//! ids, with closed values only. Neither the JSON-RPC `id` nor any argument
//! is logged, and a caller-chosen name the facade does not recognize logs
//! `unknown` (ADR-0042 d10).

use std::time::Instant;

use serde_json::Value;
use uuid::Uuid;

use super::{DESIGNED_TOOLS, SERVED_TOOLS, alias};
use crate::traffic::{RequestIds, user_agent};

/// The stable `event` of the line (observability.md, the MCP call line).
pub const EVENT_MCP_CALL: &str = "mcp_call";

/// The methods the line names; any other logs `unknown`.
const RPC_METHODS: [&str; 8] = [
    "initialize",
    "ping",
    "tools/list",
    "tools/call",
    "notifications/initialized",
    "notifications/cancelled",
    "notifications/progress",
    "notifications/roots/list_changed",
];

const UNKNOWN: &str = "unknown";

/// How the call ended (ADR-0042 d10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A result, `isError` unset.
    Result,
    /// A tool result with `isError` set.
    ToolError,
    /// A JSON-RPC error.
    RpcError,
}

impl Outcome {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Result => "result",
            Self::ToolError => "tool_error",
            Self::RpcError => "rpc_error",
        }
    }
}

/// One call's closed values, filled as the facade answers it.
#[derive(Debug)]
pub(crate) struct McpCall {
    started: Instant,
    pub(crate) rpc_method: &'static str,
    pub(crate) tool: Option<&'static str>,
    pub(crate) alias_of: Option<&'static str>,
    pub(crate) mode: Option<&'static str>,
    pub(crate) outcome: Outcome,
    pub(crate) error_code: Option<String>,
    pub(crate) client: Option<(String, String)>,
}

/// The method's name as the line logs it: one of [`RPC_METHODS`], or
/// `unknown`.
fn rpc_method(method: &str) -> &'static str {
    RPC_METHODS
        .into_iter()
        .find(|m| *m == method)
        .unwrap_or(UNKNOWN)
}

/// The tool's name as the line logs it: served, a v0.3.0 alias or
/// designed, else `unknown`.
fn tool(name: &str) -> &'static str {
    SERVED_TOOLS
        .into_iter()
        .chain(DESIGNED_TOOLS)
        .chain(alias::ALIASES.into_iter().map(|(alias, _)| alias))
        .find(|t| *t == name)
        .unwrap_or(UNKNOWN)
}

/// The mode the arguments choose, as the tool itself chooses it:
/// `oml_write` supersedes with `replaces` and submits otherwise; `oml_read`
/// gets with `artifact_id`, recalls with `query` and lists otherwise
/// (ADR-0039 d3, d4).
fn mode(tool: &str, arguments: Option<&Value>) -> Option<&'static str> {
    let has = |field: &str| arguments.and_then(|a| a.get(field)).is_some();
    match tool {
        "oml_write" if has("replaces") => Some("supersede"),
        "oml_write" => Some("submit"),
        "oml_read" if has("artifact_id") => Some("get"),
        "oml_read" if has("query") => Some("recall"),
        "oml_read" => Some("listing"),
        _ => None,
    }
}

impl McpCall {
    pub(crate) fn new(started: Instant) -> Self {
        Self {
            started,
            rpc_method: UNKNOWN,
            tool: None,
            alias_of: None,
            mode: None,
            outcome: Outcome::Result,
            error_code: None,
            client: None,
        }
    }

    /// The message's method, and for `initialize` its `clientInfo`, cut
    /// and replaced as the `User-Agent` is (ADR-0042 d9, d10).
    pub(crate) fn method(&mut self, method: &str, params: &Value) {
        self.rpc_method = rpc_method(method);
        if self.rpc_method == "initialize" {
            let info = |field: &str| {
                params["clientInfo"][field]
                    .as_str()
                    .map(|v| user_agent(v.as_bytes()))
            };
            if let (Some(name), version) = (info("name"), info("version")) {
                self.client = Some((name, version.unwrap_or_default()));
            }
        }
    }

    /// A `tools/call` of `name` with `arguments`.
    pub(crate) fn tool_call(&mut self, name: &str, arguments: Option<&Value>) {
        let logged = tool(name);
        self.tool = Some(logged);
        self.alias_of = alias::successor(name);
        self.mode = mode(logged, arguments);
    }

    /// A JSON-RPC error with `code`.
    pub(crate) fn rpc_error(&mut self, code: i64) {
        self.outcome = Outcome::RpcError;
        self.error_code = Some(code.to_string());
    }

    /// A tool result, refused under the envelope's `error.code` or not.
    pub(crate) fn tool_result(&mut self, refusal: Option<&Value>) {
        if let Some(envelope) = refusal {
            self.outcome = Outcome::ToolError;
            self.error_code = envelope["error"]["code"].as_str().map(ToOwned::to_owned);
        }
    }

    /// The call's time inside the facade, in seconds.
    pub(crate) fn seconds(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Writes the line under the request's ids (ADR-0042 d10).
    pub(crate) fn log(&self, ids: &RequestIds, tenant_id: Uuid, key_id: Uuid) {
        let duration_ms = (self.seconds() * 1_000_000.0).round() / 1000.0;
        tracing::info!(
            event = EVENT_MCP_CALL,
            request_id = %ids.request_id,
            correlation_id = %ids.correlation_id,
            trace_id = crate::traffic::trace_id().as_deref(),
            tenant_id = %tenant_id,
            key_id = %key_id,
            rpc_method = self.rpc_method,
            tool = self.tool,
            alias_of = self.alias_of,
            mode = self.mode,
            outcome = self.outcome.as_str(),
            error_code = self.error_code.as_deref(),
            duration_ms,
            client_name = self.client.as_ref().map(|(name, _)| name.as_str()),
            client_version = self.client.as_ref().map(|(_, version)| version.as_str()),
        );
    }
}
