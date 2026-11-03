//! The agent-only-memory scenario of M2 (docs/product/critical-path.md;
//! FR-10, FR-11) over the lead composition against a throwaway database: an
//! agent that stores only to OML reads back what it wrote, searches it under a
//! budget, updates it and proposes retiring it — holding one key and speaking
//! only the agent plugin (docs/api/mcp-plugin.md, ADR-0025). A dreamer
//! ratifies over the review ring, never over MCP, and a second agent key on
//! the same workarea under the same principal holds R3, since own is the
//! authoring key and never the principal (ADR-0025 d3): the first agent's held
//! rows never reach it, and its submission never reaches the first agent's
//! events tail.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;

use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::IdGen as _;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_state, pg_ping};

const ENROLLMENT: &str = "enr_test_fixture_token";

// Every text below is ASCII, so its NFC form is itself and its `body_bytes`
// is its length (ADR-0025 d6). The query matches every memory text and the
// retirement text, and not the steering text: steering applies by scope,
// never by the query (retrieval.md, the recall-ordering contract).
const QUERY: &str = "aluminium price";
const QUOTE_KEY: &str = "accra-metals-aluminium-price";
const QUOTE: &str =
    "Accra Metals aluminium price is 412 USD/t for Q1-2027 delivery, quote AM-7741.";
const QUOTE_REVISED: &str =
    "Accra Metals revised its aluminium price to 408 USD/t for Q1-2027 delivery, quote AM-7741-r2.";
const RETIREMENT: &str = "Retired: the Accra Metals aluminium price quote AM-7741-r2 lapsed on 2026-12-01 and was not renewed.";
const ALLOY_KEY: &str = "tema-alloys-aluminium-price";
const ALLOY: &str =
    "Tema Alloys aluminium price is 409 USD/t for Q1-2027 delivery, valid until 2026-12-15.";
const STEERING_KEY: &str = "quote-validity-steering";
const STEERING: &str = "Treat any supplier quote without an explicit validity date as expired.";
const FOREIGN: &str = "Kumasi Metals aluminium price is 415 USD/t, quote KM-3310.";

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev".to_owned())
}

fn db_url(name: &str) -> String {
    let base = base_url();
    let (prefix, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{prefix}/{name}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

async fn fresh_router() -> (Router, String) {
    let db_name = format!(
        "oml_test_aom_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst)
    );
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    admin
        .execute(format!("CREATE DATABASE {db_name}").as_str())
        .await
        .expect("create throwaway database");
    let store = PgStore::connect(&db_url(&db_name)).await.expect("connect");
    store.migrate().await.expect("embedded migrations apply");
    let signing = SigningKey::from_bytes(&OsIds.new_secret());
    let router = lead_state(
        &store,
        signing,
        pg_ping(store.pool().clone()),
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    (router, db_name)
}

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
    }
}

async fn send(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    idem: Option<&str>,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some(idem) = idem {
        builder = builder.header("idempotency-key", idem);
    }
    let body = match body {
        None => Body::empty(),
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
    };
    let response = router
        .clone()
        .oneshot(builder.body(body).expect("request builds"))
        .await
        .expect("infallible router");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("body collects");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("responses are JSON")
    };
    (status, value)
}

/// One key's view of `/mcp`: every call is one independent POST (stateless).
struct McpKey {
    router: Router,
    token: String,
    next_id: AtomicU64,
}

impl McpKey {
    fn new(router: &Router, token: &str) -> Self {
        Self {
            router: router.clone(),
            token: token.to_owned(),
            next_id: AtomicU64::new(1),
        }
    }

    async fn rpc(&self, method: &str, params: Value) -> Value {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (status, body) = send(
            &self.router,
            "POST",
            "/mcp",
            Some(&self.token),
            None,
            Some(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{method}: {body}");
        assert_eq!(body["id"], id, "{body}");
        body
    }

    /// A tool call that succeeds: its structured content.
    async fn ok(&self, tool: &str, args: Value) -> Value {
        let body = self
            .rpc("tools/call", json!({"name": tool, "arguments": args}))
            .await;
        assert_eq!(body["result"]["isError"], false, "{tool}: {body}");
        body["result"]["structuredContent"].clone()
    }

    /// A tool call the ring refuses with `code` (docs/api/errors.md).
    async fn refused(&self, tool: &str, args: Value, code: &str) {
        let body = self
            .rpc("tools/call", json!({"name": tool, "arguments": args}))
            .await;
        assert_eq!(body["result"]["isError"], true, "{tool}: {body}");
        assert_eq!(
            body["result"]["structuredContent"]["error"]["code"], code,
            "{tool}: {body}"
        );
    }
}

struct Crew {
    /// The agent: one writer-role key with writer and reader grants.
    agent: McpKey,
    /// A second agent key of the same shape on the same workarea and under
    /// the same principal: principal equality never makes a row own (R3, d3).
    other: McpKey,
    /// The reviewer, on the review ring only.
    dreamer: String,
    workarea: String,
}

/// Bootstrap, one crew workarea, two one-key agents of one principal and a
/// dreamer on it (ADR-0025 d1, d2): a writer-role key admits reader grants,
/// and a key's default workarea is its first grant's node.
async fn crew(router: &Router) -> Crew {
    let (status, boot) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(&json!({"enrollment_token": ENROLLMENT, "name": "root-admin", "tenant_name": "aom-tenant"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{boot}");
    let admin = boot["value"].as_str().expect("admin key").to_owned();
    let root = boot["root_workarea_id"].as_str().expect("root").to_owned();
    let (status, wa) = send(
        router,
        "POST",
        "/v2/workareas",
        Some(&admin),
        Some("wa-1"),
        Some(&json!({"parent_id": root, "name": "metals-crew"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{wa}");
    let workarea = wa["id"].as_str().expect("workarea id").to_owned();

    let mut values = Vec::new();
    for (idem, role, principal, grants) in [
        (
            "issue-agent",
            "writer",
            "crew/agent-7",
            vec!["writer", "reader"],
        ),
        (
            "issue-other",
            "writer",
            "crew/agent-7",
            vec!["writer", "reader"],
        ),
        (
            "issue-dreamer",
            "dreamer",
            "crew/reviewer-3",
            vec!["dreamer"],
        ),
    ] {
        let grants: Vec<Value> = grants
            .into_iter()
            .map(|grant| json!({"workarea_node_id": workarea, "role": grant}))
            .collect();
        let (status, key) = send(
            router,
            "POST",
            "/v2/keys",
            Some(&admin),
            Some(idem),
            Some(&json!({"role": role, "name": idem, "principal": principal, "grants": grants})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{key}");
        assert_eq!(key["default_workarea_id"], workarea, "{key}");
        values.push(key["value"].as_str().expect("show-once value").to_owned());
    }
    let dreamer = values.pop().expect("dreamer");
    let other = values.pop().expect("other");
    let agent = values.pop().expect("agent");
    Crew {
        agent: McpKey::new(router, &agent),
        other: McpKey::new(router, &other),
        dreamer,
        workarea,
    }
}

/// The dreamer approves exactly `rows` over the review ring, in whatever
/// order review next serves them.
async fn approve(router: &Router, dreamer: &str, rows: &[&str], tag: &str) {
    let mut pending: Vec<&str> = rows.to_vec();
    for n in 0..rows.len() {
        let (status, served) = send(
            router,
            "POST",
            "/v2/review/next",
            Some(dreamer),
            Some(&format!("{tag}-next-{n}")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{served}");
        let artifact = served["item"]["artifact_id"].as_str().expect("served row");
        let at = pending
            .iter()
            .position(|row| *row == artifact)
            .unwrap_or_else(|| panic!("served {artifact}, expected one of {pending:?}"));
        pending.swap_remove(at);
        let (status, verdict) = send(
            router,
            "POST",
            "/v2/review/verdicts",
            Some(dreamer),
            Some(&format!("{tag}-verdict-{n}")),
            Some(&json!({"serve_id": served["serve_id"], "verdict": "approve"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{verdict}");
        assert_eq!(verdict["state"], "promoted", "{verdict}");
    }
}

/// Every page of the key's events tail from the beginning, following
/// `next_after_seq` until `idle` (ADR-0025 d15).
async fn tail(key: &McpKey) -> Vec<Value> {
    let mut events = Vec::new();
    // An omitted after_seq starts from the beginning, as on the ring.
    let mut args = json!({"limit": 50});
    let mut after_seq = 0;
    for _ in 0..100 {
        let page = key.ok("oml_events_tail", args).await;
        events.extend(page["events"].as_array().expect("events").iter().cloned());
        let next = page["next_after_seq"].as_u64().expect("next_after_seq");
        if page["idle"] == true {
            assert_eq!(page["head_seq"], next, "{page}");
            return events;
        }
        assert!(next > after_seq, "a page that is not idle advances: {page}");
        after_seq = next;
        args = json!({"after_seq": after_seq, "limit": 50});
    }
    panic!("the tail never reached head_seq");
}

/// Budget tokens under counter `oml-tc/1`: ⌈body_bytes ÷ 4⌉ (retrieval.md,
/// token budgets).
fn tokens(text: &str) -> u64 {
    u64::try_from(text.len().div_ceil(4)).expect("a count fits u64")
}

fn id(item: &Value) -> String {
    item["artifact_id"]
        .as_str()
        .expect("artifact_id")
        .to_owned()
}

fn ids(items: &Value) -> Vec<String> {
    items.as_array().expect("items").iter().map(id).collect()
}

fn set<I: IntoIterator<Item = S>, S: Into<String>>(ids: I) -> BTreeSet<String> {
    ids.into_iter().map(Into::into).collect()
}

#[tokio::test]
async fn an_agent_holding_one_key_reads_back_searches_under_a_budget_updates_and_proposes_retiring_its_memory()
 {
    let (router, db) = fresh_router().await;
    let crew = crew(&router).await;
    let (agent, other, wa) = (&crew.agent, &crew.other, crew.workarea.as_str());
    let mut texts: HashMap<String, &str> = HashMap::new();

    // 1. One connection serves the eight built tools (ADR-0025 d12).
    let listed = agent.rpc("tools/list", json!({})).await;
    let names = set(listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|t| t["name"].as_str().expect("name")));
    assert_eq!(
        names,
        set([
            "oml_remember",
            "oml_learn",
            "oml_recall",
            "oml_get",
            "oml_supersede",
            "oml_my_work",
            "oml_workarea_info",
            "oml_events_tail",
        ])
    );

    // 2. On its default workarea the one key both remembers and recalls
    // (d1, d14; roles-and-keys.md, effective verbs).
    let info = agent.ok("oml_workarea_info", json!({})).await;
    assert_eq!(info["id"], wa, "{info}");
    assert_eq!(
        info["my_effective_verbs"],
        json!([
            "recall",
            "read",
            "graph_read",
            "events_tail",
            "remember",
            "learn",
            "attach",
            "supersede"
        ])
    );
    assert!(info["permission_epoch"].is_u64(), "{info}");

    // 3. Three memories, one of them steering, each held on arrival.
    let mut held = Vec::new();
    for (claim_key, text, kind) in [
        (QUOTE_KEY, QUOTE, "memory"),
        (ALLOY_KEY, ALLOY, "memory"),
        (STEERING_KEY, STEERING, "steering"),
    ] {
        let receipt = agent
            .ok(
                "oml_remember",
                json!({
                    "claim_key": claim_key,
                    "text": text,
                    "kind": kind,
                    "idempotency_key": format!("agent/run-1/{claim_key}"),
                }),
            )
            .await;
        assert_eq!(receipt["state"], "held", "{receipt}");
        assert_eq!(receipt["version"], 1, "{receipt}");
        assert_eq!(receipt["workarea"], wa, "{receipt}");
        texts.insert(id(&receipt), text);
        held.push(id(&receipt));
    }
    let [quote, alloy, steering]: [String; 3] = held.try_into().expect("three receipts");

    // 4. The own-work listing shows all three held, with their counts and
    // without their text (d6, d13).
    let work = agent.ok("oml_my_work", json!({})).await;
    let items = work["items"].as_array().expect("items");
    assert_eq!(set(items.iter().map(id)), set([&quote, &alloy, &steering]));
    for item in items {
        assert_eq!(item["state"], "held", "{item}");
        assert_eq!(item["tip"], false, "{item}");
        assert_eq!(item["workarea"], wa, "{item}");
        assert_eq!(item["tokens"], tokens(texts[&id(item)]), "{item}");
        assert!(item.get("text").is_none(), "{item}");
    }

    // 5. Get reads a held row back to its authoring key, and to no other key,
    // not even one of the same principal (d3).
    let got = agent.ok("oml_get", json!({"artifact_id": quote})).await;
    assert_eq!(got["text"], QUOTE, "{got}");
    assert_eq!(got["claim_key"], QUOTE_KEY, "{got}");
    assert_eq!(got["version"], 1, "{got}");
    assert_eq!(got["kind"], "memory", "{got}");
    assert_eq!(got["state"], "held", "{got}");
    assert_eq!(got["tip"], false, "{got}");
    assert_eq!(got["retirement"], false, "{got}");
    assert_eq!(got["replaces"], Value::Null, "{got}");
    assert_eq!(got["workarea"], wa, "{got}");
    assert_eq!(got["tokens"], tokens(QUOTE), "{got}");
    assert_eq!(got["derives_from"], json!([]), "{got}");
    assert_eq!(got["attachments"], json!([]), "{got}");
    other
        .refused("oml_get", json!({"artifact_id": quote}), "access.not_found")
        .await;

    // 6. include_own_held: the author's held memories join the hits, flagged
    // held; held steering is not in force, and no other key sees them, the
    // same principal's included (d3, d4).
    let own = agent
        .ok(
            "oml_recall",
            json!({"query": QUERY, "include_own_held": true}),
        )
        .await;
    assert_eq!(own["steering"], json!([]), "{own}");
    assert_eq!(set(ids(&own["hits"])), set([&quote, &alloy]), "{own}");
    for hit in own["hits"].as_array().expect("hits") {
        assert_eq!(hit["state"], "held", "{hit}");
        assert_eq!(hit["tip"], false, "{hit}");
        assert_eq!(hit["text"], texts[&id(hit)], "{hit}");
        assert_eq!(hit["tokens"], tokens(texts[&id(hit)]), "{hit}");
        assert!(hit["score"].is_number(), "{hit}");
    }
    assert!(
        own.get("budget").is_none(),
        "no max_tokens, no budget: {own}"
    );
    let promoted_only = agent.ok("oml_recall", json!({"query": QUERY})).await;
    assert_eq!(promoted_only["hits"], json!([]), "{promoted_only}");
    let theirs = other
        .ok(
            "oml_recall",
            json!({"query": QUERY, "include_own_held": true}),
        )
        .await;
    assert_eq!(theirs["hits"], json!([]), "{theirs}");
    assert_eq!(theirs["steering"], json!([]), "{theirs}");

    // 7. Another identity ratifies all three over the review ring.
    approve(
        &router,
        &crew.dreamer,
        &[&quote, &alloy, &steering],
        "first",
    )
    .await;

    // 8. Under a budget that fits the steering and one hit: steering first and
    // never cut, the longest rank prefix of hits, every row counted, the cut
    // reported, and the same answer on repeat (d6, d7, d8).
    let open = agent.ok("oml_recall", json!({"query": QUERY})).await;
    assert_eq!(ids(&open["steering"]), [steering.as_str()], "{open}");
    let segment = &open["steering"][0];
    assert_eq!(segment["kind"], "steering", "{open}");
    assert_eq!(segment["text"], STEERING, "{open}");
    assert!(segment.get("score").is_none(), "{open}");
    let ranked = ids(&open["hits"]);
    assert_eq!(set(ranked.clone()), set([&quote, &alloy]), "{open}");
    for item in open["steering"]
        .as_array()
        .into_iter()
        .chain(open["hits"].as_array())
        .flatten()
    {
        assert_eq!(item["state"], "promoted", "{item}");
        assert_eq!(item["tip"], true, "{item}");
        assert_eq!(item["tokens"], tokens(texts[&id(item)]), "{item}");
    }
    assert!(open.get("budget").is_none(), "{open}");
    let steering_tokens = tokens(STEERING);
    let first = tokens(texts[&ranked[0]]);
    let max_tokens = steering_tokens + first;
    let budgeted_request = json!({"query": QUERY, "max_tokens": max_tokens});
    let budgeted = agent.ok("oml_recall", budgeted_request.clone()).await;
    assert_eq!(
        ids(&budgeted["steering"]),
        [steering.as_str()],
        "{budgeted}"
    );
    assert_eq!(ids(&budgeted["hits"]), [ranked[0].as_str()], "{budgeted}");
    assert_eq!(
        budgeted["budget"],
        json!({
            "max_tokens": max_tokens,
            "counter": "oml-tc/1",
            "steering_tokens": steering_tokens,
            "used_tokens": steering_tokens + first,
            "truncated": true,
            "hits_omitted": 1,
            "next_hit_tokens": tokens(texts[&ranked[1]]),
        })
    );
    let again = agent.ok("oml_recall", budgeted_request).await;
    assert_eq!(again, budgeted, "the cut is deterministic");

    // 9. A supersession enters admission held and replays once; the tip moves
    // only when another identity ratifies it (d10).
    let update = json!({
        "artifact_id": quote,
        "text": QUOTE_REVISED,
        "reason": "supplier issued a revised quote",
        "idempotency_key": "agent/run-1/revise",
    });
    let successor = agent.ok("oml_supersede", update.clone()).await;
    assert_eq!(successor["state"], "held", "{successor}");
    assert_eq!(successor["claim_key"], QUOTE_KEY, "{successor}");
    assert_eq!(successor["version"], 2, "{successor}");
    assert_eq!(successor["retirement"], false, "{successor}");
    assert_eq!(
        successor["replaces"],
        json!({"artifact_id": quote, "version": 1})
    );
    assert_eq!(successor["workarea"], wa, "{successor}");
    assert_eq!(successor["idempotent_replay"], false, "{successor}");
    let revised = id(&successor);
    texts.insert(revised.clone(), QUOTE_REVISED);
    let replay = agent.ok("oml_supersede", update).await;
    assert_eq!(replay["idempotent_replay"], true, "{replay}");
    assert_eq!(id(&replay), revised, "{replay}");

    let before = other.ok("oml_recall", json!({"query": QUERY})).await;
    assert_eq!(set(ids(&before["hits"])), set([&quote, &alloy]), "{before}");
    let author_view = agent
        .ok(
            "oml_recall",
            json!({"query": QUERY, "include_own_held": true}),
        )
        .await;
    let hits = author_view["hits"].as_array().expect("hits");
    assert_eq!(
        set(hits.iter().map(id)),
        set([&quote, &alloy, &revised]),
        "the held successor and the tip it would replace are never merged: {author_view}"
    );
    let pending = hits.iter().find(|h| id(h) == revised).expect("successor");
    assert_eq!(pending["state"], "held", "{pending}");
    assert_eq!(pending["tip"], false, "{pending}");
    assert_eq!(
        pending["replaces"],
        json!({"artifact_id": quote, "version": 1})
    );

    approve(&router, &crew.dreamer, &[&revised], "revision").await;
    let after = other.ok("oml_recall", json!({"query": QUERY})).await;
    assert_eq!(set(ids(&after["hits"])), set([&revised, &alloy]), "{after}");
    let tip = after["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .find(|h| id(h) == revised)
        .expect("the new tip");
    assert_eq!(tip["tip"], true, "{tip}");
    assert_eq!(tip["version"], 2, "{tip}");
    assert_eq!(tip["text"], QUOTE_REVISED, "{tip}");
    assert_eq!(tip["replaces"], json!({"artifact_id": quote, "version": 1}));

    // 10. The author proposes retiring the claim; once ratified, recall
    // serves nothing of it and every version stays readable as history (d11).
    let retire = agent
        .ok(
            "oml_supersede",
            json!({
                "artifact_id": revised,
                "text": RETIREMENT,
                "retirement": true,
                "idempotency_key": "agent/run-1/retire",
            }),
        )
        .await;
    assert_eq!(retire["state"], "held", "{retire}");
    assert_eq!(retire["version"], 3, "{retire}");
    assert_eq!(retire["retirement"], true, "{retire}");
    assert_eq!(
        retire["replaces"],
        json!({"artifact_id": revised, "version": 2})
    );
    let retirement = id(&retire);
    texts.insert(retirement.clone(), RETIREMENT);

    approve(&router, &crew.dreamer, &[&retirement], "retirement").await;
    for key in [agent, other] {
        let result = key.ok("oml_recall", json!({"query": QUERY})).await;
        assert_eq!(ids(&result["hits"]), [alloy.as_str()], "{result}");
        assert_eq!(ids(&result["steering"]), [steering.as_str()], "{result}");
    }
    for history in [&quote, &revised] {
        let version = agent.ok("oml_get", json!({"artifact_id": history})).await;
        assert_eq!(version["state"], "promoted", "{version}");
        assert_eq!(version["tip"], false, "{version}");
    }
    let retired = agent
        .ok("oml_get", json!({"artifact_id": retirement}))
        .await;
    assert_eq!(retired["state"], "promoted", "{retired}");
    assert_eq!(retired["tip"], true, "{retired}");
    assert_eq!(retired["retirement"], true, "{retired}");
    assert_eq!(retired["text"], RETIREMENT, "{retired}");

    // 11. R3 on recall and on the stream: the second key's held submission
    // reaches neither the agent's recall nor its tail, though both keys share
    // a principal, and no review event reaches the content tail at all (d3,
    // d4, d15).
    let foreign = other
        .ok(
            "oml_remember",
            json!({
                "claim_key": "kumasi-metals-aluminium-price",
                "text": FOREIGN,
                "idempotency_key": "other/run-1/kumasi",
            }),
        )
        .await;
    assert_eq!(foreign["state"], "held", "{foreign}");
    let foreign = id(&foreign);
    let own = agent
        .ok(
            "oml_recall",
            json!({"query": QUERY, "include_own_held": true}),
        )
        .await;
    assert!(!ids(&own["hits"]).contains(&foreign), "{own}");

    let events = tail(agent).await;
    let mut last_seq = 0;
    for event in &events {
        let event_type = event["event_type"].as_str().expect("event_type");
        assert!(
            ["submission.", "lifecycle.", "workarea."]
                .iter()
                .any(|family| event_type.starts_with(family)),
            "{event_type} is not on the content tail: {event}"
        );
        let seq = event["seq"].as_u64().expect("seq");
        assert!(seq > last_seq, "seq ascending: {event}");
        last_seq = seq;
        if !event_type.starts_with("workarea.") {
            assert_ne!(event["subject"]["artifact_id"], foreign.as_str(), "{event}");
            assert_eq!(event["workarea"], wa, "{event}");
        }
    }
    let subjects = |event_type: &str| -> Vec<String> {
        events
            .iter()
            .filter(|e| e["event_type"] == event_type)
            .map(|e| id(&e["subject"]))
            .collect()
    };
    assert_eq!(
        set(subjects("submission.remember")),
        set([&quote, &alloy, &steering, &revised, &retirement])
    );
    assert_eq!(
        set(subjects("lifecycle.promote")),
        set([&quote, &alloy, &steering])
    );
    assert_eq!(
        subjects("lifecycle.supersede"),
        [revised.as_str(), retirement.as_str()]
    );

    drop_db(&db).await;
}
