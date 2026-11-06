//! The thin HTTP client: every verb is a request to the lab's own API and
//! nothing more — the CLI holds no authority of its own (ADR-0015,
//! cli/reference.md). Refusals arrive as the typed error envelope of
//! docs/api/errors.md and map to exit code 1; local failures map to 2.

use std::time::Duration;

use serde_json::Value;

/// Why a verb did not succeed, split along the CLI reference's exit codes: a
/// typed server refusal (exit 1) versus a local failure (exit 2), and among
/// the local failures, one where the server may have acted from one where
/// nothing was sent.
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
    /// No context, unreachable server, bad arguments, or a 4xx without the
    /// typed envelope: the server did not act on the request.
    Local(String),
    /// The request may have reached the server, but no answer the CLI can
    /// use came back: the exchange failed after connecting, a success body
    /// could not be read or parsed, or a 5xx came without the typed
    /// envelope, as from a gateway. The server may have acted on it. The
    /// message never carries a success body, which can hold a key value
    /// (cli/reference.md `oml keys issue-agent`, ADR-0028 d1).
    Unanswered(String),
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
}

/// What the CLI sends as `User-Agent` in place of ureq's default: the
/// server's request line names the client by it (ADR-0042 d9).
pub const USER_AGENT: &str = concat!("oml-cli/", env!("CARGO_PKG_VERSION"));

pub struct Client {
    agent: ureq::Agent,
    server: String,
    key: Option<String>,
}

impl Client {
    #[must_use]
    pub fn new(server: String, key: Option<String>) -> Self {
        Self::with_timeout(server, key, Duration::from_secs(30))
    }

    /// A client whose every request ends after `timeout`, its TCP connect
    /// included. The DNS lookup has no deadline here: ureq gives it none, so
    /// a caller that must end in time bounds the whole call itself, as `oml
    /// hook session-start` does (ADR-0029 d5).
    #[must_use]
    pub fn with_timeout(server: String, key: Option<String>, timeout: Duration) -> Self {
        Self::named(server, key, timeout, USER_AGENT.to_owned())
    }

    /// [`Client::with_timeout`] for one of the plugin's hooks, which names
    /// itself `oml-cli/<version> hook/<hook>` (ADR-0042 d9).
    #[must_use]
    pub fn for_hook(server: String, key: Option<String>, timeout: Duration, hook: &str) -> Self {
        Self::named(server, key, timeout, format!("{USER_AGENT} hook/{hook}"))
    }

    fn named(server: String, key: Option<String>, timeout: Duration, user_agent: String) -> Self {
        // `timeout` alone leaves ureq's own 30-second connect timeout in force.
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout(timeout)
            .user_agent(&user_agent)
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
                "no API key: pass --api-key, set OML_API_KEY, or claim one with `oml bootstrap claim`",
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

    /// `GET /v2/whoami`: the calling key's own record, from which a client
    /// derives everything beyond its URL and key (ADR-0039 d15).
    pub fn whoami(&self) -> Result<Value, CliError> {
        self.get("/v2/whoami").map(|(_, me)| me)
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

    /// PUT a JSON body under the key; returns (status, parsed body).
    ///
    /// # Errors
    ///
    /// The server refused, or could not be reached.
    pub fn put(&self, path: &str, body: &Value) -> Result<(u16, Value), CliError> {
        let request = self
            .agent
            .put(&format!("{}{path}", self.server))
            .set("Authorization", &self.bearer()?);
        finish(request.send_json(body.clone()))
    }

    /// POST a JSON body; returns (status, parsed body).
    pub fn post(
        &self,
        path: &str,
        body: &Value,
        opts: &CallOpts,
    ) -> Result<(u16, Value), CliError> {
        finish(self.send_post(path, body, opts)?)
    }

    /// POST a JSON body; returns the parsed success body and whether it is an
    /// idempotent replay (`Idempotency-Replay: true`, http-api.md
    /// idempotency), which a connect-code verb reads because a replayed
    /// answer carries no code (ADR-0031 d7).
    pub fn post_replayable(
        &self,
        path: &str,
        body: &Value,
        opts: &CallOpts,
    ) -> Result<(Value, bool), CliError> {
        let result = self.send_post(path, body, opts)?;
        let replayed = matches!(
            &result,
            Ok(response) if response.header("Idempotency-Replay") == Some("true")
        );
        finish(result).map(|(_, value)| (value, replayed))
    }

    fn send_post(
        &self,
        path: &str,
        body: &Value,
        opts: &CallOpts,
    ) -> Result<Result<ureq::Response, ureq::Error>, CliError> {
        let mut request = self.agent.post(&format!("{}{path}", self.server));
        if opts.auth {
            request = request.set("Authorization", &self.bearer()?);
        }
        if opts.idempotency {
            request = request.set("Idempotency-Key", &new_idempotency_key());
        }
        Ok(request.send_json(body.clone()))
    }

    /// POST a JSON body under the key, with `idempotency_key` as its
    /// `Idempotency-Key`, so a re-run naming the same key replays the first
    /// answer (http-api.md idempotency; ADR-0039 d3).
    pub fn post_keyed(
        &self,
        path: &str,
        body: &Value,
        idempotency_key: &str,
    ) -> Result<(u16, Value), CliError> {
        let request = self
            .agent
            .post(&format!("{}{path}", self.server))
            .set("Authorization", &self.bearer()?)
            .set("Idempotency-Key", idempotency_key);
        finish(request.send_json(body.clone()))
    }

    /// [`Client::post_keyed`], retried under the same key while the server
    /// answers 429 or 503 with `Retry-After`, at most `attempts` times, so a
    /// long run waits as the server asks instead of failing (ADR-0057 d6).
    pub fn post_keyed_honouring_retry_after(
        &self,
        path: &str,
        body: &Value,
        idempotency_key: &str,
        attempts: u32,
    ) -> Result<(u16, Value), CliError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let result = self
                .agent
                .post(&format!("{}{path}", self.server))
                .set("Authorization", &self.bearer()?)
                .set("Idempotency-Key", idempotency_key)
                .send_json(body.clone());
            if let Err(ureq::Error::Status(429 | 503, response)) = &result
                && attempt < attempts
                && let Some(wait) = response
                    .header("Retry-After")
                    .and_then(|s| s.trim().parse::<u64>().ok())
            {
                std::thread::sleep(Duration::from_secs(wait.clamp(1, 60)));
                continue;
            }
            return finish(result);
        }
    }
}

/// One path segment carrying a reference exactly as the operator typed it,
/// [`encode`]d, so a `/`, a `?` or a `%` stays inside the segment and
/// reaches the server, which alone classifies the value as an id, a name or
/// neither (`request.validation`). Three spellings no URL path carries as
/// one segment are refused here instead, before anything is sent: the empty
/// value, which leaves no segment, and `.` and `..`, which the URL parser
/// ureq sends through removes as dot segments (url 2.5.8 `src/parser.rs`,
/// after the URL Standard's single-dot and double-dot path segments, which
/// count an escaped `%2e` as a dot too, so no escape carries them). No id
/// or name is spelled that way (ADR-0026 d2, d3), and the refusal turns away
/// no value the server would resolve; beyond it the CLI classifies and
/// resolves nothing (cli/reference.md conventions; ADR-0026 d4).
///
/// # Errors
///
/// A local failure (exit 2) naming the value, when it is ``, `.` or `..`.
pub fn segment(text: &str) -> Result<String, CliError> {
    if matches!(text, "" | "." | "..") {
        return Err(CliError::local(format!(
            "{text:?} is neither an id nor a name and cannot travel as one URL path segment (an \
             empty value leaves none, and a URL parser removes `.` and `..` as dot segments); \
             nothing was sent (cli/reference.md conventions)"
        )));
    }
    Ok(encode(text))
}

/// `text` with every byte outside RFC 3986's unreserved set percent-encoded,
/// for a path segment ([`segment`]) or a query value.
#[must_use]
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// One idempotency key per invocation: a re-run is a new act, a transport
/// retry inside the call is not (ureq does not retry POSTs).
///
/// # Panics
///
/// When the OS random source is unavailable.
#[must_use]
pub fn new_idempotency_key() -> String {
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
            // A success body is never echoed: an issuance's carries the key
            // value, which reaches no stream but the one the verb prints it
            // to (ADR-0028 d1).
            let text = response.into_string().map_err(|e| {
                CliError::Unanswered(format!(
                    "the server answered {status}, but its body could not be read ({e}); the \
                     server may have acted on the request"
                ))
            })?;
            if text.is_empty() {
                // 204: nothing in scope waits.
                return Ok((status, Value::Null));
            }
            let value: Value = serde_json::from_str(&text).map_err(|e| {
                CliError::Unanswered(format!(
                    "the server answered {status}, but its body is not JSON ({e}) and is not \
                     shown; the server may have acted on the request"
                ))
            })?;
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
                // A gateway's 5xx can follow a request the server committed.
                _ if status >= 500 => Err(CliError::Unanswered(format!(
                    "the server answered {status} without the typed error envelope, so the \
                     server behind it may have acted on the request: {text}"
                ))),
                _ => Err(CliError::local(format!(
                    "the server answered {status} without the typed error envelope: {text}"
                ))),
            }
        }
        Err(e) if nothing_was_sent(e.kind()) => {
            Err(CliError::local(format!("cannot reach the server: {e}")))
        }
        Err(e) => Err(CliError::Unanswered(format!(
            "the exchange with the server failed after connecting ({e}); the server may have \
             acted on the request"
        ))),
    }
}

/// The transport failures that end a call before any request byte leaves:
/// a bad URL, DNS, the connection or the proxy. Every other failure may
/// follow a request the server received, so it is not one of these.
fn nothing_was_sent(kind: ureq::ErrorKind) -> bool {
    use ureq::ErrorKind as K;
    matches!(
        kind,
        K::InvalidUrl
            | K::UnknownScheme
            | K::Dns
            | K::InsecureRequestHttpsOnly
            | K::ConnectionFailed
            | K::InvalidProxyUrl
            | K::ProxyConnect
            | K::ProxyUnauthorized
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::net::TcpListener;

    /// The `User-Agent` of the one request `send` makes to a listener that
    /// answers `200 {}`.
    fn user_agent_of(send: impl FnOnce(String)) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
        let url = format!("http://{}", listener.local_addr().expect("an address"));
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("one connection");
            let mut reader = BufReader::new(stream.try_clone().expect("a second handle"));
            let mut agent = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("a request line");
                if line.trim().is_empty() {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("user-agent")
                {
                    agent = value.trim().to_owned();
                }
            }
            let mut stream = stream;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .expect("the answer");
            agent
        });
        send(url);
        server.join().expect("the listener thread")
    }

    #[test]
    fn the_cli_sends_oml_cli_and_its_version_and_a_hook_adds_its_name() {
        let version = env!("CARGO_PKG_VERSION");
        let agent = user_agent_of(|url| {
            Client::new(url, None).probe("/readyz").expect("an answer");
        });
        assert_eq!(agent, format!("oml-cli/{version}"));
        let agent = user_agent_of(|url| {
            Client::for_hook(url, None, Duration::from_secs(5), "session-start")
                .probe("/readyz")
                .expect("an answer");
        });
        assert_eq!(agent, format!("oml-cli/{version} hook/session-start"));
    }
}
