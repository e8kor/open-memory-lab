//! The calls a run makes, behind a port so the diff and the run are tested
//! against a fake (ADR-0057 d8–d11). [`HttpApi`] speaks the HTTP API under
//! the caller's key, one request per memory, honouring `Retry-After` (d6).

use serde_json::{Value, json};

use crate::client::{CliError, Client, encode, segment};

/// A version as the listing serves it, with the anchor and source-check
/// fields of ADR-0057 d5.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub artifact_id: String,
    pub claim_key: String,
    pub version: u64,
    pub tip: bool,
    pub state: String,
    pub retirement: bool,
    pub workarea_id: Option<String>,
    pub plaintext_digest: String,
    pub anchor: Option<ListedAnchor>,
    pub source_checked: Option<Checked>,
}

/// An anchor as served; a `uri` anchor has no `repo`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListedAnchor {
    pub repo: Option<String>,
    pub uri: Option<String>,
    pub rev: Option<String>,
    pub path: Option<String>,
    pub fragment: Option<String>,
    pub hash: Option<String>,
}

/// A version's latest source check (ADR-0057 d5, d18).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    pub rev: String,
    pub result: String,
    pub moved_to: Option<(String, Option<String>)>,
}

impl Listed {
    /// A listing item, read leniently: an absent field is its empty value.
    #[must_use]
    pub fn of(v: &Value) -> Self {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
        let anchor = v["anchor"].as_object().map(|_| {
            let a = &v["anchor"];
            let o = |k: &str| a[k].as_str().map(ToOwned::to_owned);
            ListedAnchor {
                repo: o("repo"),
                uri: o("uri"),
                rev: o("rev"),
                path: o("path"),
                fragment: o("fragment"),
                hash: o("hash"),
            }
        });
        let checked = v["source_checked"].as_object().map(|_| {
            let c = &v["source_checked"];
            Checked {
                rev: c["rev"].as_str().unwrap_or_default().to_owned(),
                result: c["result"].as_str().unwrap_or_default().to_owned(),
                moved_to: c["moved_to"]["path"].as_str().map(|p| {
                    (
                        p.to_owned(),
                        c["moved_to"]["fragment"].as_str().map(ToOwned::to_owned),
                    )
                }),
            }
        });
        Self {
            artifact_id: s("artifact_id"),
            claim_key: s("claim_key"),
            version: v["version"].as_u64().unwrap_or(0),
            tip: v["tip"].as_bool().unwrap_or(false),
            state: s("state"),
            retirement: v["retirement"].as_bool().unwrap_or(false),
            workarea_id: v["workarea_id"].as_str().map(ToOwned::to_owned),
            plaintext_digest: s("plaintext_digest"),
            anchor,
            source_checked: checked,
        }
    }
}

/// One listing: the key's own rows in every state, or the promoted served
/// tips, of a workarea's subtree, optionally filtered by origin (d4).
#[derive(Clone, Debug, Default)]
pub struct ListQuery {
    pub workarea: String,
    pub own: bool,
    pub anchor_repo: Option<String>,
    pub anchor_path_prefix: Option<String>,
}

/// A claim edge as `GET /v2/edges` serves it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeRow {
    pub edge_id: String,
    pub from_artifact_id: String,
    pub to_artifact_id: String,
    pub label: String,
    pub state: String,
    pub proposer_key_id: String,
}

impl EdgeRow {
    #[must_use]
    pub fn of(v: &Value) -> Self {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_owned();
        Self {
            edge_id: s("edge_id"),
            from_artifact_id: s("from_artifact_id"),
            to_artifact_id: s("to_artifact_id"),
            label: s("label"),
            state: s("state"),
            proposer_key_id: s("proposer_key_id"),
        }
    }
}

/// The calling key, from `GET /v2/whoami`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Me {
    pub key_id: String,
    pub name: String,
}

/// The calls of a run.
pub trait SyncApi {
    /// # Errors
    /// The server refuses or cannot be reached.
    fn me(&self) -> Result<Me, CliError>;
    /// The id of a workarea named by id or name.
    /// # Errors
    /// As [`SyncApi::me`].
    fn workarea_id(&self, workarea: &str) -> Result<String, CliError>;
    /// Every page of one listing.
    /// # Errors
    /// As [`SyncApi::me`].
    fn list(&self, query: &ListQuery) -> Result<Vec<Listed>, CliError>;
    /// `POST /v1/memories`.
    /// # Errors
    /// As [`SyncApi::me`].
    fn submit(&self, body: &Value, idempotency_key: &str) -> Result<Value, CliError>;
    /// `POST /v1/memories/{artifact_id}/supersede`.
    /// # Errors
    /// As [`SyncApi::me`].
    fn supersede(
        &self,
        artifact_id: &str,
        body: &Value,
        idempotency_key: &str,
    ) -> Result<Value, CliError>;
    /// `POST /v1/memories/{artifact_id}/source-check` (ADR-0057 d5).
    /// # Errors
    /// As [`SyncApi::me`].
    fn source_check(
        &self,
        artifact_id: &str,
        body: &Value,
        idempotency_key: &str,
    ) -> Result<Value, CliError>;
    /// Each source check of a batch, answered in order. The default sends
    /// them one by one; [`HttpApi`] sends up to [`CHECK_WORKERS`] at once.
    fn source_checks(&self, batch: &[(String, Value, String)]) -> Vec<Result<Value, CliError>> {
        batch
            .iter()
            .map(|(a, body, key)| self.source_check(a, body, key))
            .collect()
    }
    /// Every claim edge in a workarea's subtree.
    /// # Errors
    /// As [`SyncApi::me`].
    fn claim_edges(&self, workarea_id: &str) -> Result<Vec<EdgeRow>, CliError>;
    /// `POST /v2/edges {kind: claim}`.
    /// # Errors
    /// As [`SyncApi::me`].
    fn propose_edge(
        &self,
        from_artifact_id: &str,
        to_artifact_id: &str,
        label: &str,
        idempotency_key: &str,
    ) -> Result<Value, CliError>;
    /// `POST /v2/edges/{edge_id}/ratify`.
    /// # Errors
    /// As [`SyncApi::me`].
    fn ratify_edge(&self, edge_id: &str, idempotency_key: &str) -> Result<Value, CliError>;
}

/// The HTTP API under the caller's key.
pub struct HttpApi {
    pub client: Client,
}

/// How many times a write is sent while the server answers 429 or 503.
const ATTEMPTS: u32 = 6;

/// The most source checks in flight at once (ADR-0057 d5): a re-sync of
/// 600 records checks 600 tips, one request each.
pub const CHECK_WORKERS: usize = 4;

impl HttpApi {
    fn post(&self, path: &str, body: &Value, key: &str) -> Result<Value, CliError> {
        self.client
            .post_keyed_honouring_retry_after(path, body, key, ATTEMPTS)
            .map(|(_, v)| v)
    }

    fn pages(&self, path: &str) -> Result<Vec<Value>, CliError> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let url = match &cursor {
                Some(c) => format!("{path}&cursor={}", encode(c)),
                None => path.to_owned(),
            };
            let (_, page) = self.client.get(&url)?;
            out.extend(page["items"].as_array().cloned().unwrap_or_default());
            match page["next_cursor"].as_str() {
                Some(c) if !c.is_empty() => cursor = Some(c.to_owned()),
                _ => return Ok(out),
            }
        }
    }
}

impl SyncApi for HttpApi {
    fn me(&self) -> Result<Me, CliError> {
        let me = self.client.whoami()?;
        Ok(Me {
            key_id: me["key_id"].as_str().unwrap_or_default().to_owned(),
            name: me["name"]
                .as_str()
                .or_else(|| me["key_id"].as_str())
                .unwrap_or_default()
                .to_owned(),
        })
    }

    fn workarea_id(&self, workarea: &str) -> Result<String, CliError> {
        let (_, node) = self
            .client
            .get(&format!("/v2/workareas/{}", segment(workarea)?))?;
        node["id"].as_str().map(ToOwned::to_owned).ok_or_else(|| {
            CliError::local(format!("the workarea {workarea} was served without an id"))
        })
    }

    fn list(&self, query: &ListQuery) -> Result<Vec<Listed>, CliError> {
        let mut path = format!(
            "/v1/memories?own={}&limit=200&workarea={}",
            query.own,
            encode(&query.workarea)
        );
        if let Some(repo) = &query.anchor_repo {
            path.push_str(&format!("&anchor_repo={}", encode(repo)));
        }
        if let Some(prefix) = &query.anchor_path_prefix {
            path.push_str(&format!("&anchor_path_prefix={}", encode(prefix)));
        }
        Ok(self.pages(&path)?.iter().map(Listed::of).collect())
    }

    fn submit(&self, body: &Value, key: &str) -> Result<Value, CliError> {
        self.post("/v1/memories", body, key)
    }

    fn supersede(&self, artifact_id: &str, body: &Value, key: &str) -> Result<Value, CliError> {
        self.post(
            &format!("/v1/memories/{}/supersede", segment(artifact_id)?),
            body,
            key,
        )
    }

    fn source_check(&self, artifact_id: &str, body: &Value, key: &str) -> Result<Value, CliError> {
        self.post(
            &format!("/v1/memories/{}/source-check", segment(artifact_id)?),
            body,
            key,
        )
    }

    fn source_checks(&self, batch: &[(String, Value, String)]) -> Vec<Result<Value, CliError>> {
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let slots: Vec<Mutex<Option<Result<Value, CliError>>>> =
            batch.iter().map(|_| Mutex::new(None)).collect();
        let next = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..CHECK_WORKERS.min(batch.len()) {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some((a, body, key)) = batch.get(i) else {
                            break;
                        };
                        let answer = self.source_check(a, body, key);
                        if let Ok(mut slot) = slots[i].lock() {
                            *slot = Some(answer);
                        }
                    }
                });
            }
        });
        slots
            .into_iter()
            .map(|m| {
                m.into_inner().ok().flatten().unwrap_or_else(|| {
                    Err(CliError::Unanswered(
                        "a source check was not sent".to_owned(),
                    ))
                })
            })
            .collect()
    }

    fn claim_edges(&self, workarea_id: &str) -> Result<Vec<EdgeRow>, CliError> {
        let path = format!(
            "/v2/edges?kind=claim&limit=200&workarea_id={}",
            encode(workarea_id)
        );
        Ok(self.pages(&path)?.iter().map(EdgeRow::of).collect())
    }

    fn propose_edge(
        &self,
        from: &str,
        to: &str,
        label: &str,
        key: &str,
    ) -> Result<Value, CliError> {
        self.post(
            "/v2/edges",
            &json!({ "kind": "claim", "from_artifact_id": from, "to_artifact_id": to, "label": label }),
            key,
        )
    }

    fn ratify_edge(&self, edge_id: &str, key: &str) -> Result<Value, CliError> {
        self.post(
            &format!("/v2/edges/{}/ratify", segment(edge_id)?),
            &json!({}),
            key,
        )
    }
}
