//! The thin HTTP client: every verb is a request to the lab's own API and
//! nothing more — the CLI holds no authority of its own (ADR-0015,
//! cli/reference.md). Refusals arrive as the typed error envelope of
//! docs/api/errors.md and map to exit code 1; local failures map to 2.

use std::time::Duration;

use serde_json::Value;

/// Why a verb did not succeed, split exactly along the CLI reference's exit
/// codes: a typed server refusal (exit 1) versus a local failure (exit 2).
#[derive(Debug)]
pub enum CliError {
    /// The server refused with the typed envelope; `envelope` is the whole
    /// response document (for `--json`).
    Refused {
        status: u16,
        code: String,
        message: String,
        rule: String,
        envelope: Value,
    },
    /// No context, unreachable server, bad arguments.
    Local(String),
}

impl CliError {
    pub fn local(msg: impl Into<String>) -> Self {
        Self::Local(msg.into())
    }
}

/// Options for one call.
#[derive(Default)]
pub struct CallOpts {
    /// Send `Authorization: Bearer <key>` (all verbs but bootstrap claim).
    pub auth: bool,
    /// Mint and send an `Idempotency-Key` (required on every write,
    /// http-api.md idempotency table).
    pub idempotency: bool,
    /// The `X-OML-Workarea` per-request override (ADR-0022 d4).
    pub workarea: Option<String>,
}

pub struct Client {
    agent: ureq::Agent,
    server: String,
    key: Option<String>,
}

impl Client {
    #[must_use]
    pub fn new(server: String, key: Option<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        Self {
            agent,
            server: server.trim_end_matches('/').to_owned(),
            key,
        }
    }

    #[must_use]
    pub fn server(&self) -> &str {
        &self.server
    }

    fn bearer(&self) -> Result<String, CliError> {
        self.key.as_deref().map(|k| format!("Bearer {k}")).ok_or_else(|| {
            CliError::local(
                "no API key: pass --key, set OML_CLI_KEY, or claim one with `oml bootstrap claim`",
            )
        })
    }

    /// GET; returns (status, parsed body).
    pub fn get(&self, path: &str) -> Result<(u16, Value), CliError> {
        let request = self
            .agent
            .get(&format!("{}{path}", self.server))
            .set("Authorization", &self.bearer()?);
        finish(request.call())
    }

    /// GET without credentials, tolerating any status: `/readyz` answers 503
    /// with a diagnostic body, which is the probe's signal, not a refusal.
    pub fn probe(&self, path: &str) -> Result<(u16, Value), CliError> {
        match self.agent.get(&format!("{}{path}", self.server)).call() {
            Ok(response) => {
                let status = response.status();
                let text = response
                    .into_string()
                    .map_err(|e| CliError::local(format!("reading the response body: {e}")))?;
                Ok((status, parse_lenient(&text)))
            }
            Err(ureq::Error::Status(status, response)) => {
                let text = response.into_string().unwrap_or_default();
                Ok((status, parse_lenient(&text)))
            }
            Err(e) => Err(CliError::local(format!("cannot reach the server: {e}"))),
        }
    }

    /// POST a JSON body; returns (status, parsed body).
    pub fn post(
        &self,
        path: &str,
        body: &Value,
        opts: &CallOpts,
    ) -> Result<(u16, Value), CliError> {
        let mut request = self.agent.post(&format!("{}{path}", self.server));
        if opts.auth {
            request = request.set("Authorization", &self.bearer()?);
        }
        if opts.idempotency {
            request = request.set("Idempotency-Key", &new_idempotency_key());
        }
        if let Some(wa) = &opts.workarea {
            request = request.set("X-OML-Workarea", wa);
        }
        finish(request.send_json(body.clone()))
    }
}

/// One idempotency key per invocation: a re-run is a new act, a transport
/// retry inside the call is not (ureq does not retry POSTs).
fn new_idempotency_key() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source is available");
    format!("oml-cli-{}", hex::encode(bytes))
}

fn parse_lenient(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
}

fn finish(result: Result<ureq::Response, ureq::Error>) -> Result<(u16, Value), CliError> {
    match result {
        Ok(response) => {
            let status = response.status();
            let text = response
                .into_string()
                .map_err(|e| CliError::local(format!("reading the response body: {e}")))?;
            if text.is_empty() {
                // 204: nothing in scope waits.
                return Ok((status, Value::Null));
            }
            let value: Value = serde_json::from_str(&text)
                .map_err(|e| CliError::local(format!("the response is not JSON ({e}): {text}")))?;
            Ok((status, value))
        }
        Err(ureq::Error::Status(status, response)) => {
            let text = response.into_string().unwrap_or_default();
            match serde_json::from_str::<Value>(&text) {
                Ok(envelope) if envelope["error"]["code"].is_string() => Err(CliError::Refused {
                    status,
                    code: envelope["error"]["code"].as_str().unwrap_or("").to_owned(),
                    message: envelope["error"]["message"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                    rule: envelope["error"]["rule"].as_str().unwrap_or("").to_owned(),
                    envelope,
                }),
                _ => Err(CliError::local(format!(
                    "the server answered {status} without the typed error envelope: {text}"
                ))),
            }
        }
        Err(e) => Err(CliError::local(format!("cannot reach the server: {e}"))),
    }
}
