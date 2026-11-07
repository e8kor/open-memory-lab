//! The agent-only-memory scenario of M2 (ADR-0025) over the lead composition against a throwaway database: an
//! agent that stores only to OML reads back what it wrote, searches it under a
//! budget, updates it and proposes retiring it — holding one key and calling
//! only the three tools the agent plugin lists, oml_write, oml_read and
//! oml_workarea (docs/api/mcp-plugin.md, ADR-0025, ADR-0039 d2). A dreamer
//! ratifies over the review ring, never over MCP, and a second agent key on
//! the same workarea under the same principal holds R3, since own is the
//! authoring key and never the principal (ADR-0025 d3): the first agent's held
//! rows never reach it, and its submission never reaches the first agent's
//! events tail, read over `GET /v2/events` since the tail left MCP.

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
        .expect("DATABASE_URL names a throwaway PostgreSQL cluster; no suite defaults to one, so none reaches a live server")
}

/// The password of every minted test role; the roles live only as long as
/// their throwaway database.
const PAIR_PASSWORD: &str = "oml_test_pair";

/// `db` at `DATABASE_URL`'s host, logged in as `role`.
fn role_url(db: &str, role: &str) -> String {
    let base = base_url();
    let (scheme, rest) = base
        .split_once("://")
        .expect("DATABASE_URL is a postgres:// URL");
    let (_, host_path) = rest.rsplit_once('@').expect("DATABASE_URL names its user");
    let (host, _) = host_path
        .split_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{scheme}://{role}:{PAIR_PASSWORD}@{host}/{db}")
}

/// `db` through the owner login of its role pair (ADR-0034 d2, d8).
fn owner_url(db: &str) -> String {
    role_url(db, &format!("{db}_owner"))
}

/// `db` through the serving login of its role pair (ADR-0034 d2, d8).
fn serving_url(db: &str) -> String {
    role_url(db, &format!("{db}_serving"))
}

/// Eight hex digits of the clock, so a name a crashed earlier run left
/// behind on the cluster under the same pid and counter is not reused.
fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

/// Mints `db`'s owner/serving role pair, `LOGIN` and no other attribute,
/// and creates `db` owned by the owner (ADR-0034 d2, d8); `false`, with
/// `did not run: <role> cannot create roles` printed, when `DATABASE_URL`'s
/// role is not the superuser minting takes, as the maintainer's local `oml`
/// (CREATEDB alone) is not. Under `OML_TEST_REQUIRE_SPLIT=1`, which CI sets,
/// the same condition fails the test, so no run passes vacuously.
async fn mint_database(db: &str) -> bool {
    let mut admin = PgConnection::connect(&base_url())
        .await
        .expect("admin connect");
    let (role, mints): (String, bool) = sqlx::query_as(
        "SELECT current_user::text, rolsuper FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&mut admin)
    .await
    .expect("role attributes");
    if !mints {
        assert!(
            std::env::var("OML_TEST_REQUIRE_SPLIT").as_deref() != Ok("1"),
            "{role} cannot create roles, and OML_TEST_REQUIRE_SPLIT=1 requires the role pair (ADR-0034 d8)"
        );
        println!("did not run: {role} cannot create roles");
        return false;
    }
    for role in ["owner", "serving"] {
        admin
            .execute(format!("CREATE ROLE {db}_{role} LOGIN PASSWORD '{PAIR_PASSWORD}'").as_str())
            .await
            .expect("mint the role pair");
    }
    admin
        .execute(format!("CREATE DATABASE {db} OWNER {db}_owner").as_str())
        .await
        .expect("create throwaway database");
    true
}

/// A store over `db`'s serving login, migrated through its owner login with
/// `oml.serving_role` set (ADR-0034 d4, d8).
async fn serving_store(db: &str) -> PgStore {
    let store = PgStore::connect(&serving_url(db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(db));
    store
        .migrate(&owner_url(db))
        .await
        .expect("embedded migrations apply");
    store
}

static DB_N: AtomicU64 = AtomicU64::new(0);

/// The lead's router over the serving login of a fresh database and its
/// name, or `None` when this run cannot mint the role pair (ADR-0034 d8).
async fn fresh_router() -> Option<(Router, String)> {
    let db_name = format!(
        "oml_test_aom_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db_name).await {
        return None;
    }
    let store = serving_store(&db_name).await;
    let signing = SigningKey::from_bytes(&OsIds.new_secret());
    let router = lead_state(
        &store,
        signing,
        pg_ping(store.pool().clone()),
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    Some((router, db_name))
}

async fn drop_db(name: &str) {
    if let Ok(mut admin) = PgConnection::connect(&base_url()).await {
        let _ = admin
            .execute(format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)").as_str())
            .await;
        for role in ["serving", "owner"] {
            let _ = admin
                .execute(format!("DROP ROLE IF EXISTS {name}_{role}").as_str())
                .await;
        }
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
    send_with(router, method, path, token, idem, body, &[]).await
}

/// [`send`] with `extra` headers beside the authorization and the
/// idempotency key.
async fn send_with(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    idem: Option<&str>,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
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
    /// The tenant's first admin key, from the bootstrap claim.
    admin: String,
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
        Some(&json!({"parent": root, "name": "metals-crew"})),
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
            .map(|grant| json!({"workarea": workarea, "role": grant}))
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
        admin,
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

/// Every page of the key's events tail from the beginning, over
/// `GET /v2/events`, following `next_after_seq` until it reaches `head_seq`
/// (ADR-0025 d15). The tail is not on MCP (ADR-0039 d2).
async fn tail(key: &McpKey) -> Vec<Value> {
    let mut events = Vec::new();
    let mut after_seq = 0;
    for _ in 0..100 {
        let (status, page) = send(
            &key.router,
            "GET",
            &format!("/v2/events?after_seq={after_seq}&limit=50"),
            Some(&key.token),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{page}");
        events.extend(page["items"].as_array().expect("items").iter().cloned());
        let next = page["next_after_seq"].as_u64().expect("next_after_seq");
        if next == page["head_seq"].as_u64().expect("head_seq") {
            return events;
        }
        assert!(
            next > after_seq,
            "a page short of head_seq advances: {page}"
        );
        after_seq = next;
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
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let crew = crew(&router).await;
    let (agent, other, wa) = (&crew.agent, &crew.other, crew.workarea.as_str());
    let mut texts: HashMap<String, &str> = HashMap::new();

    // 1. One connection lists the three tools, oml_write, oml_read and
    // oml_workarea, and the journey calls only those: Claude Code offers the
    // model nothing else (ADR-0025 d12; ADR-0039 d2).
    let listed = agent.rpc("tools/list", json!({})).await;
    let names = set(listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|t| t["name"].as_str().expect("name")));
    assert_eq!(names, set(["oml_write", "oml_read", "oml_workarea"]));

    // 2. On its default workarea the one key both remembers and recalls
    // (d1, d14; roles-and-keys.md, effective verbs).
    let info = agent.ok("oml_workarea", json!({})).await;
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
                "oml_write",
                json!({
                    "claim_key": claim_key,
                    "text": text,
                    "kind": kind,
                    "idempotency_key": format!("agent/run-1/{claim_key}"),
                }),
            )
            .await;
        assert_eq!(receipt["mode"], "submit", "{receipt}");
        assert_eq!(receipt["state"], "held", "{receipt}");
        assert_eq!(receipt["version"], 1, "{receipt}");
        assert_eq!(receipt["workarea"], wa, "{receipt}");
        texts.insert(id(&receipt), text);
        held.push(id(&receipt));
    }
    let [quote, alloy, steering]: [String; 3] = held.try_into().expect("three receipts");

    // 4. The own-work listing, oml_read with no mode argument, shows all
    // three held, each a claim item with its text and count (d6, d13;
    // ADR-0039 d4).
    let work = agent.ok("oml_read", json!({})).await;
    assert_eq!(work["mode"], "listing", "{work}");
    let items = work["items"].as_array().expect("items");
    assert_eq!(set(items.iter().map(id)), set([&quote, &alloy, &steering]));
    for item in items {
        assert_eq!(item["state"], "held", "{item}");
        assert_eq!(item["tip"], false, "{item}");
        assert_eq!(item["workarea"], wa, "{item}");
        assert_eq!(item["text"], texts[&id(item)], "{item}");
        assert_eq!(item["tokens"], tokens(texts[&id(item)]), "{item}");
    }

    // 5. A read by artifact_id returns a held row to its authoring key, and
    // to no other key, not even one of the same principal (d3).
    let got = agent.ok("oml_read", json!({"artifact_id": quote})).await;
    assert_eq!(got["mode"], "get", "{got}");
    let got = &got["items"][0];
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
        .refused(
            "oml_read",
            json!({"artifact_id": quote}),
            "access.not_found",
        )
        .await;

    // 6. include_unreviewed: the author's held memories join the hits,
    // flagged held; held steering is not in force, and no other key sees
    // them, the same principal's included (d3, d4; ADR-0039 d4).
    let own = agent
        .ok(
            "oml_read",
            json!({"query": QUERY, "include_unreviewed": true}),
        )
        .await;
    assert_eq!(own["mode"], "recall", "{own}");
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
    let promoted_only = agent.ok("oml_read", json!({"query": QUERY})).await;
    assert_eq!(promoted_only["hits"], json!([]), "{promoted_only}");
    let theirs = other
        .ok(
            "oml_read",
            json!({"query": QUERY, "include_unreviewed": true}),
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
    let open = agent.ok("oml_read", json!({"query": QUERY})).await;
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
    let budgeted = agent.ok("oml_read", budgeted_request.clone()).await;
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
    let mut again = agent.ok("oml_read", budgeted_request).await;
    // Each recall has an id of its own (ADR-0054 d2); the rest is the same.
    let mut budgeted = budgeted;
    for answer in [&mut again, &mut budgeted] {
        answer.as_object_mut().map(|o| o.remove("recall_id"));
    }
    assert_eq!(again, budgeted, "the cut is deterministic");

    // 9. A write with replaces enters admission held and replays once; the
    // tip moves only when another identity ratifies it (d10; ADR-0039 d3).
    let update = json!({
        "expected_version": 1,
        "replaces": quote,
        "text": QUOTE_REVISED,
        "reason": "supplier issued a revised quote",
        "idempotency_key": "agent/run-1/revise",
    });
    let successor = agent.ok("oml_write", update.clone()).await;
    assert_eq!(successor["mode"], "supersede", "{successor}");
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
    let replay = agent.ok("oml_write", update).await;
    assert_eq!(replay["idempotent_replay"], true, "{replay}");
    assert_eq!(id(&replay), revised, "{replay}");

    let before = other.ok("oml_read", json!({"query": QUERY})).await;
    assert_eq!(set(ids(&before["hits"])), set([&quote, &alloy]), "{before}");
    let author_view = agent
        .ok(
            "oml_read",
            json!({"query": QUERY, "include_unreviewed": true}),
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
    let after = other.ok("oml_read", json!({"query": QUERY})).await;
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
            "oml_write",
            json!({
                "expected_version": 2,
                "replaces": revised,
                "text": RETIREMENT,
                "retirement": true,
                "idempotency_key": "agent/run-1/retire",
            }),
        )
        .await;
    assert_eq!(retire["mode"], "supersede", "{retire}");
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
        let result = key.ok("oml_read", json!({"query": QUERY})).await;
        assert_eq!(ids(&result["hits"]), [alloy.as_str()], "{result}");
        assert_eq!(ids(&result["steering"]), [steering.as_str()], "{result}");
    }
    for history in [&quote, &revised] {
        let version = agent.ok("oml_read", json!({"artifact_id": history})).await;
        let version = &version["items"][0];
        assert_eq!(version["state"], "promoted", "{version}");
        assert_eq!(version["tip"], false, "{version}");
    }
    let retired = agent
        .ok("oml_read", json!({"artifact_id": retirement}))
        .await;
    let retired = &retired["items"][0];
    assert_eq!(retired["state"], "promoted", "{retired}");
    assert_eq!(retired["tip"], true, "{retired}");
    assert_eq!(retired["retirement"], true, "{retired}");
    assert_eq!(retired["text"], RETIREMENT, "{retired}");

    // The agent learns that every write it made was approved from its own
    // listing's state filter, with no events cursor (ADR-0039 d2).
    let approved = agent.ok("oml_read", json!({"state": "promoted"})).await;
    assert_eq!(
        set(ids(&approved["items"])),
        set([&quote, &alloy, &steering, &revised, &retirement]),
        "{approved}"
    );
    let waiting = agent.ok("oml_read", json!({"state": "held"})).await;
    assert_eq!(waiting["items"], json!([]), "{waiting}");

    // 11. R3 on recall and on the stream: the second key's held submission
    // reaches neither the agent's recall nor its tail, though both keys share
    // a principal, and no review event reaches the content tail at all (d3,
    // d4, d15). The tail is read over GET /v2/events, since it left MCP
    // (ADR-0039 d2).
    let foreign = other
        .ok(
            "oml_write",
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
            "oml_read",
            json!({"query": QUERY, "include_unreviewed": true}),
        )
        .await;
    assert!(!ids(&own["hits"]).contains(&foreign), "{own}");

    let events = tail(agent).await;
    // The version an event is about: the new version of a supersession, the
    // payload's artifact otherwise (event-catalog.md).
    let subject = |event: &Value| -> String {
        let payload = &event["payload"];
        payload["new_artifact_id"]
            .as_str()
            .or_else(|| payload["artifact_id"].as_str())
            .unwrap_or_else(|| panic!("a subject: {event}"))
            .to_owned()
    };
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
            assert_ne!(subject(event), foreign, "{event}");
            assert_eq!(event["payload"]["workarea_id"], wa, "{event}");
        }
    }
    let subjects = |event_type: &str| -> Vec<String> {
        events
            .iter()
            .filter(|e| e["event_type"] == event_type)
            .map(subject)
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

/// The key's own id, read from `GET /v2/whoami` (ADR-0039 d15).
async fn key_id(router: &Router, key: &McpKey) -> uuid::Uuid {
    let (status, me) = send(router, "GET", "/v2/whoami", Some(&key.token), None, None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    me["key_id"]
        .as_str()
        .and_then(|id| id.parse().ok())
        .expect("whoami names the key id")
}

/// One `POST /v1/memories` under `key`.
async fn submit(router: &Router, key: &McpKey, idem: &str, body: &Value) -> (StatusCode, Value) {
    send(
        router,
        "POST",
        "/v1/memories",
        Some(&key.token),
        Some(idem),
        Some(body),
    )
    .await
}

/// The event type the key's content tail journals for `artifact_id`'s
/// capture (ADR-0025 d15).
async fn capture_event(router: &Router, key: &McpKey, artifact_id: &str) -> String {
    let (status, events) = send(
        router,
        "GET",
        "/v2/events?after_seq=0&limit=100",
        Some(&key.token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{events}");
    let types: Vec<&str> = events["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|e| e["payload"]["artifact_id"] == artifact_id)
        .filter_map(|e| e["event_type"].as_str())
        .filter(|t| t.starts_with("submission."))
        .collect();
    assert_eq!(types.len(), 1, "one capture event: {events}");
    types[0].to_owned()
}

#[tokio::test]
async fn a_write_without_claim_key_is_stored_under_the_key_derived_from_its_author_and_text() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let Crew { agent, .. } = crew(&router).await;
    let author = key_id(&router, &agent).await;

    // No claim_key, verb or kind: the server derives the key, kind is
    // memory and the capture is remember (ADR-0039 d3, d6).
    let (status, receipt) = submit(&router, &agent, "derive-1", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
    let expected = oml_app::domain::derive_claim_key(author, QUOTE);
    assert_eq!(receipt["claim_key"], expected.as_str(), "{receipt}");
    assert!(
        expected.starts_with("auto-") && expected.len() == 21,
        "{expected}"
    );
    let artifact = receipt["artifact_id"].as_str().expect("artifact id");
    let (status, row) = send(
        &router,
        "GET",
        &format!("/v1/memories/{artifact}"),
        Some(&agent.token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["claim_key"], expected.as_str(), "{row}");
    assert_eq!(row["kind"], "memory", "{row}");
    assert_eq!(
        capture_event(&router, &agent, artifact).await,
        "submission.remember"
    );

    // A replay under the same Idempotency-Key answers the stored receipt.
    let (status, replay) = submit(&router, &agent, "derive-1", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{replay}");
    assert_eq!(replay["artifact_id"], artifact, "{replay}");

    drop_db(&db).await;
}

#[tokio::test]
async fn a_derived_claim_key_conflict_reveals_no_other_keys_text() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let Crew { agent, other, .. } = crew(&router).await;

    // Two keys record one text: two claims under two derived keys, and the
    // second writer learns nothing of the first's held row (ADR-0039 d6).
    let (status, first) = submit(&router, &agent, "same-1", &json!({ "body": FOREIGN })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let (status, second) = submit(&router, &other, "same-2", &json!({ "body": FOREIGN })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    assert_ne!(first["claim_key"], second["claim_key"]);
    assert_ne!(first["artifact_id"], second["artifact_id"]);

    // The conflict a derived key can raise is over the writer's own row.
    let (status, again) = submit(&router, &agent, "same-3", &json!({ "body": FOREIGN })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(
        again["error"]["code"], "submission.claim_key_conflict",
        "{again}"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn an_explicit_claim_key_starting_auto_is_refused_naming_claim_key() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let Crew { agent, .. } = crew(&router).await;

    let (status, refused) = submit(
        &router,
        &agent,
        "auto-explicit",
        &json!({ "claim_key": "auto-0123456789abcdef", "body": QUOTE }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["error"]["code"], "request.validation", "{refused}");
    assert_eq!(
        refused["error"]["details"]["field"], "/claim_key",
        "{refused}"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn a_write_with_derives_from_and_no_verb_is_journalled_submission_learn() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let Crew { agent, .. } = crew(&router).await;

    let (status, ancestor) = submit(&router, &agent, "anc-1", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{ancestor}");
    let (status, derived) = submit(
        &router,
        &agent,
        "learn-1",
        &json!({ "body": ALLOY, "derives_from": [ancestor["artifact_id"]] }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{derived}");
    let artifact = derived["artifact_id"].as_str().expect("artifact id");
    assert_eq!(
        capture_event(&router, &agent, artifact).await,
        "submission.learn"
    );

    drop_db(&db).await;
}

#[tokio::test]
async fn an_explicit_verb_keeps_its_journal_event() {
    let Some((router, db)) = fresh_router().await else {
        return;
    };
    let Crew { agent, .. } = crew(&router).await;

    let (status, ancestor) = submit(&router, &agent, "anc-2", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{ancestor}");
    for (idem, verb, text) in [
        ("verb-remember", "remember", ALLOY),
        ("verb-learn", "learn", STEERING),
    ] {
        let (status, receipt) = submit(
            &router,
            &agent,
            idem,
            &json!({
                "verb": verb,
                "body": text,
                "derives_from": [ancestor["artifact_id"]],
            }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{receipt}");
        let artifact = receipt["artifact_id"].as_str().expect("artifact id");
        assert_eq!(
            capture_event(&router, &agent, artifact).await,
            format!("submission.{verb}")
        );
    }

    drop_db(&db).await;
}

// ---------------------------------------------------------------------------
// One workarea field on the memory routes (ADR-0039 d4, d7). These run over
// the in-memory doubles, so they need no role pair: what they prove is the
// transport's and the app's, not the store's.
// ---------------------------------------------------------------------------

/// The lead's router over in-memory doubles, its bootstrap open under
/// [`ENROLLMENT`].
fn doubles_router() -> Router {
    use oml_app::App;
    use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new([7; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let ping: oml_transport_http::DbPing = std::sync::Arc::new(|| Box::pin(async { true }));
    oml_transport_http::ApiState::new(
        std::sync::Arc::new(app),
        store,
        ping,
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router()
}

/// A child of the crew workarea, `child`, and a writer key, `scoped`, with
/// writer and reader grants on the crew workarea and `child` as its default
/// workarea, so a read that defaults reads `child` alone (ADR-0039 d4).
async fn scoped_key(router: &Router, crew: &Crew) -> (McpKey, String) {
    let (status, child) = send(
        router,
        "POST",
        "/v2/workareas",
        Some(&crew.admin),
        Some("wa-child"),
        Some(&json!({"parent": crew.workarea, "name": "metals-child"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{child}");
    let child = child["id"].as_str().expect("child id").to_owned();
    let grants: Vec<Value> = ["writer", "reader"]
        .into_iter()
        .map(|role| json!({"workarea": crew.workarea, "role": role}))
        .collect();
    let (status, key) = send(
        router,
        "POST",
        "/v2/keys",
        Some(&crew.admin),
        Some("issue-scoped"),
        Some(&json!({
            "role": "writer",
            "name": "issue-scoped",
            "principal": "crew/agent-9",
            "grants": grants,
            "default_workarea": child,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");
    assert_eq!(key["default_workarea_id"], child.as_str(), "{key}");
    let value = key["value"].as_str().expect("show-once value");
    (McpKey::new(router, value), child)
}

/// The refusal of a v0.3.0 spelling: `request.validation` naming what was
/// sent and what to send (ADR-0039 d7, d11).
fn assert_renamed(status: StatusCode, body: &Value, field: &str, renamed_to: &str) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "request.validation", "{body}");
    assert_eq!(body["error"]["details"]["field"], field, "{body}");
    assert_eq!(body["error"]["details"]["renamed_to"], renamed_to, "{body}");
}

/// The artifact ids of a recall's hits.
fn hit_ids(recall: &Value) -> BTreeSet<String> {
    recall["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|hit| id(&hit["memory"]))
        .collect()
}

#[tokio::test]
async fn the_x_oml_workarea_header_is_refused_naming_workarea() {
    let router = doubles_router();
    let crew = crew(&router).await;
    let token = crew.agent.token.as_str();
    let header = [("x-oml-workarea", crew.workarea.as_str())];

    let (status, written) = submit(&router, &crew.agent, "hdr-0", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{written}");
    let target = id(&written);

    // A body route names the field by its JSON pointer, a query route by
    // its parameter name; the header itself is named by its name.
    for (method, path, idem, body, renamed_to) in [
        (
            "POST",
            "/v1/memories".to_owned(),
            Some("hdr-1"),
            Some(json!({ "body": ALLOY })),
            "/workarea",
        ),
        (
            "POST",
            format!("/v1/memories/{target}/supersede"),
            Some("hdr-2"),
            Some(json!({ "expected_version": 1, "body": QUOTE_REVISED })),
            "/workarea",
        ),
        (
            "POST",
            "/v1/memories/recall".to_owned(),
            None,
            Some(json!({ "query": QUERY })),
            "/workarea",
        ),
        ("GET", "/v1/memories".to_owned(), None, None, "workarea"),
        (
            "GET",
            "/v1/memories?own=true".to_owned(),
            None,
            None,
            "workarea",
        ),
        ("GET", "/v2/events".to_owned(), None, None, "workarea"),
    ] {
        let (status, refused) = send_with(
            &router,
            method,
            &path,
            Some(token),
            idem,
            body.as_ref(),
            &header,
        )
        .await;
        assert_renamed(status, &refused, "X-OML-Workarea", renamed_to);
    }

    // The same target in the field is honoured.
    let (status, accepted) = submit(
        &router,
        &crew.agent,
        "hdr-3",
        &json!({ "body": ALLOY, "workarea": crew.workarea }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
    assert_eq!(
        accepted["workarea_id"],
        crew.workarea.as_str(),
        "{accepted}"
    );
}

#[tokio::test]
async fn a_listing_naming_workarea_id_is_refused_naming_workarea() {
    let router = doubles_router();
    let crew = crew(&router).await;
    let token = crew.agent.token.as_str();
    for path in [
        format!("/v1/memories?workarea_id={}", crew.workarea),
        format!("/v1/memories?own=true&workarea_id={}", crew.workarea),
        format!("/v2/events?workarea_id={}", crew.workarea),
    ] {
        let (status, refused) = send(&router, "GET", &path, Some(token), None, None).await;
        assert_renamed(status, &refused, "workarea_id", "workarea");
    }
    for path in [
        format!("/v1/memories?workarea={}", crew.workarea),
        format!("/v1/memories?own=true&workarea={}", crew.workarea),
        format!("/v2/events?workarea={}", crew.workarea),
    ] {
        let (status, page) = send(&router, "GET", &path, Some(token), None, None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {page}");
    }
}

#[tokio::test]
async fn recall_without_workarea_reads_the_keys_default_workarea() {
    let router = doubles_router();
    let crew = crew(&router).await;
    let (scoped, child) = scoped_key(&router, &crew).await;

    // One claim in the key's default workarea, the child, by omission, and
    // one in its parent, by the field.
    let (status, in_child) = submit(&router, &scoped, "dflt-1", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{in_child}");
    assert_eq!(in_child["workarea_id"], child.as_str(), "{in_child}");
    let (status, in_parent) = submit(
        &router,
        &scoped,
        "dflt-2",
        &json!({ "body": ALLOY, "workarea": crew.workarea }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{in_parent}");
    assert_eq!(
        in_parent["workarea_id"],
        crew.workarea.as_str(),
        "{in_parent}"
    );
    approve(
        &router,
        &crew.dreamer,
        &[&id(&in_child), &id(&in_parent)],
        "dflt",
    )
    .await;

    let recall = |body: Value| {
        let router = router.clone();
        let token = scoped.token.clone();
        async move {
            let (status, recall) = send(
                &router,
                "POST",
                "/v1/memories/recall",
                Some(&token),
                None,
                Some(&body),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{recall}");
            hit_ids(&recall)
        }
    };
    assert_eq!(
        recall(json!({ "query": QUERY })).await,
        set([id(&in_child)]),
        "an omitted workarea reads the key's default, the child"
    );
    assert_eq!(
        recall(json!({ "query": QUERY, "workarea": crew.workarea })).await,
        set([id(&in_child), id(&in_parent)]),
        "a named workarea reads its subtree"
    );

    // The scoped listing defaults the same way.
    let (status, page) = send(
        &router,
        "GET",
        "/v1/memories",
        Some(&scoped.token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(set(ids(&page["items"])), set([id(&in_child)]), "{page}");
}

#[tokio::test]
async fn the_own_work_listing_without_workarea_lists_everything_the_key_wrote() {
    let router = doubles_router();
    let crew = crew(&router).await;
    let (scoped, child) = scoped_key(&router, &crew).await;

    let (status, in_child) = submit(&router, &scoped, "own-1", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{in_child}");
    let (status, in_parent) = submit(
        &router,
        &scoped,
        "own-2",
        &json!({ "body": ALLOY, "workarea": crew.workarea }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{in_parent}");
    // Another key's row is never own (R3).
    let (status, foreign) =
        submit(&router, &crew.agent, "own-3", &json!({ "body": FOREIGN })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{foreign}");

    let list = |path: String| {
        let router = router.clone();
        let token = scoped.token.clone();
        async move {
            let (status, page) = send(&router, "GET", &path, Some(&token), None, None).await;
            assert_eq!(status, StatusCode::OK, "{page}");
            set(ids(&page["items"]))
        }
    };
    assert_eq!(
        list("/v1/memories?own=true".to_owned()).await,
        set([id(&in_child), id(&in_parent)]),
        "the own-work listing reads every workarea the key wrote in"
    );
    assert_eq!(
        list(format!("/v1/memories?own=true&workarea={child}")).await,
        set([id(&in_child)]),
        "a named workarea narrows it to that subtree"
    );
}

#[tokio::test]
async fn include_unreviewed_returns_the_keys_own_held_rows_and_no_other_keys() {
    let router = doubles_router();
    let crew = crew(&router).await;

    let (status, own) = submit(&router, &crew.agent, "unrev-1", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{own}");
    let (status, foreign) =
        submit(&router, &crew.other, "unrev-2", &json!({ "body": FOREIGN })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{foreign}");

    let recall = |body: Value| {
        let router = router.clone();
        let token = crew.agent.token.clone();
        async move {
            send(
                &router,
                "POST",
                "/v1/memories/recall",
                Some(&token),
                None,
                Some(&body),
            )
            .await
        }
    };
    let (status, held) = recall(json!({ "query": QUERY, "include_unreviewed": true })).await;
    assert_eq!(status, StatusCode::OK, "{held}");
    assert_eq!(hit_ids(&held), set([id(&own)]), "{held}");
    assert_eq!(held["hits"][0]["memory"]["state"], "held", "{held}");

    // The default is false: nothing is promoted yet, so nothing is served.
    let (status, none) = recall(json!({ "query": QUERY })).await;
    assert_eq!(status, StatusCode::OK, "{none}");
    assert!(hit_ids(&none).is_empty(), "{none}");

    // The v0.3.0 spellings are refused naming their successors.
    let (status, refused) = recall(json!({ "query": QUERY, "include_own_held": true })).await;
    assert_renamed(status, &refused, "/include_own_held", "/include_unreviewed");
    let (status, refused) = recall(json!({ "query": QUERY, "include_bodies": true })).await;
    assert_renamed(status, &refused, "/include_bodies", "/include_text");
}

#[tokio::test]
async fn a_memory_response_carries_text_and_no_body() {
    let router = doubles_router();
    let crew = crew(&router).await;
    let token = crew.agent.token.as_str();

    let (status, written) = submit(&router, &crew.agent, "text-1", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{written}");
    let artifact = id(&written);

    let (status, row) = send(
        &router,
        "GET",
        &format!("/v1/memories/{artifact}"),
        Some(token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{row}");
    assert_eq!(row["text"], QUOTE, "{row}");
    assert!(row.get("body").is_none(), "{row}");

    let (status, recall) = send(
        &router,
        "POST",
        "/v1/memories/recall",
        Some(token),
        None,
        Some(&json!({ "query": QUERY, "include_unreviewed": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    let memory = &recall["hits"][0]["memory"];
    assert_eq!(memory["text"], QUOTE, "{recall}");
    assert!(memory.get("body").is_none(), "{recall}");

    // include_text false serves the item without its text.
    let (status, bare) = send(
        &router,
        "POST",
        "/v1/memories/recall",
        Some(token),
        None,
        Some(&json!({ "query": QUERY, "include_unreviewed": true, "include_text": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bare}");
    let memory = &bare["hits"][0]["memory"];
    assert!(
        memory.get("text").is_none() && memory.get("body").is_none(),
        "{bare}"
    );
}

// ---------------------------------------------------------------------------
// A verdict without serve_id applies to the reviewer's one open item
// (ADR-0039 d10). These run over the in-memory doubles, as above.
// ---------------------------------------------------------------------------

/// A dreamer key on the crew workarea with dreamer and reader grants, so
/// it may propose an edit as well as review (ADR-0025 d10).
async fn reviewer(router: &Router, crew: &Crew, name: &str, principal: &str) -> String {
    let grants: Vec<Value> = ["dreamer", "reader"]
        .into_iter()
        .map(|role| json!({"workarea": crew.workarea, "role": role}))
        .collect();
    let (status, key) = send(
        router,
        "POST",
        "/v2/keys",
        Some(&crew.admin),
        Some(name),
        Some(&json!({"role": "dreamer", "name": name, "principal": principal, "grants": grants})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");
    key["value"].as_str().expect("show-once value").to_owned()
}

/// One `POST /v2/review/next` under `token`: the served item's artifact id.
async fn claim_next(router: &Router, token: &str, idem: &str) -> String {
    let (status, served) = send(
        router,
        "POST",
        "/v2/review/next",
        Some(token),
        Some(idem),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{served}");
    id(&served["item"])
}

/// One `POST /v2/review/verdicts` under `token`.
async fn verdict(router: &Router, token: &str, idem: &str, body: &Value) -> (StatusCode, Value) {
    send(
        router,
        "POST",
        "/v2/review/verdicts",
        Some(token),
        Some(idem),
        Some(body),
    )
    .await
}

/// The refusal of a verdict that names no serve where the reviewer holds
/// none or several: `request.validation` naming `/serve_id`.
fn assert_serve_id_named(status: StatusCode, body: &Value) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "request.validation", "{body}");
    assert_eq!(body["error"]["details"]["field"], "/serve_id", "{body}");
}

/// The state the author reads its row in.
async fn state(router: &Router, author: &McpKey, artifact: &str) -> String {
    let (status, row) = send(
        router,
        "GET",
        &format!("/v1/memories/{artifact}"),
        Some(&author.token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{row}");
    row["state"].as_str().expect("state").to_owned()
}

#[tokio::test]
async fn a_verdict_without_serve_id_applies_to_the_reviewers_one_open_item_and_is_refused_with_two()
{
    one_open_item_then_two(&doubles_router()).await;
    // The store's claimed-row query, where this run can mint the role pair.
    if let Some((router, db)) = fresh_router().await {
        one_open_item_then_two(&router).await;
        drop_db(&db).await;
    }
}

/// A verdict without serve_id applies to the one item `router`'s reviewer
/// claimed, and is refused naming serve_id once it claims two (ADR-0039 d10).
async fn one_open_item_then_two(router: &Router) {
    let crew = crew(router).await;
    let rev = reviewer(router, &crew, "issue-rev", "crew/reviewer-5").await;

    // A promoted tip, a held successor of it by the agent, and the
    // reviewer's own edit proposal of it: the rival makes the agent's
    // successor one the reviewer is not served (ADR-0025 d10).
    let (status, tip) = submit(router, &crew.agent, "two-0", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{tip}");
    let tip = id(&tip);
    approve(router, &crew.dreamer, &[tip.as_str()], "two-tip").await;
    let (status, successor) = send(
        router,
        "POST",
        &format!("/v1/memories/{tip}/supersede"),
        Some(&crew.agent.token),
        Some("two-1"),
        Some(&json!({ "expected_version": 1, "body": QUOTE_REVISED })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{successor}");
    let successor = id(&successor);
    let (status, proposal) = send(
        router,
        "POST",
        &format!("/v1/memories/{tip}/supersede"),
        Some(&rev),
        Some("two-2"),
        Some(&json!({ "expected_version": 1, "body": "Accra Metals quote AM-7741 is 410 USD/t.", "reason": "the quote moved" })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{proposal}");
    let proposal = id(&proposal);
    let (status, first) = submit(router, &crew.agent, "two-3", &json!({ "body": ALLOY })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let first = id(&first);
    let (status, second) = submit(router, &crew.agent, "two-4", &json!({ "body": FOREIGN })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    let second = id(&second);

    // One open item: the verdict without serve_id applies to it.
    assert_eq!(claim_next(router, &rev, "two-next-1").await, first);
    let (status, applied) = verdict(router, &rev, "two-v-1", &json!({"verdict": "approve"})).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["artifact_id"], first.as_str(), "{applied}");
    assert_eq!(applied["state"], "promoted", "{applied}");

    // The reviewer claims the next item; once its proposal is rejected the
    // agent's older successor is servable, so it holds two.
    assert_eq!(claim_next(router, &rev, "two-next-2").await, second);
    let (status, rejected) = verdict(
        router,
        &crew.dreamer,
        "two-v-2",
        &json!({
            "serve_id": format!("srv_{}", proposal.replace('-', "")),
            "verdict": "reject",
            "reason": "the agent's successor carries the revision",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rejected}");
    assert_eq!(claim_next(router, &rev, "two-next-3").await, successor);

    // Two open items: refused naming serve_id, and neither moves.
    let (status, refused) = verdict(router, &rev, "two-v-3", &json!({"verdict": "approve"})).await;
    assert_serve_id_named(status, &refused);
    assert_eq!(state(router, &crew.agent, &second).await, "held");
    assert_eq!(state(router, &crew.agent, &successor).await, "held");
}

#[tokio::test]
async fn a_verdict_without_serve_id_is_refused_naming_serve_id_when_the_reviewer_holds_none() {
    let router = doubles_router();
    let crew = crew(&router).await;
    let (status, written) = submit(&router, &crew.agent, "none-0", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{written}");
    let written = id(&written);

    // A held row waits, but the reviewer claimed nothing.
    let (status, refused) = verdict(
        &router,
        &crew.dreamer,
        "none-v-1",
        &json!({"verdict": "approve"}),
    )
    .await;
    assert_serve_id_named(status, &refused);
    assert_eq!(state(&router, &crew.agent, &written).await, "held");

    // The row is still served whole to the next claim.
    assert_eq!(
        claim_next(&router, &crew.dreamer, "none-next").await,
        written
    );
}

#[tokio::test]
async fn a_verdict_without_serve_id_never_applies_to_an_item_another_reviewer_claimed() {
    let router = doubles_router();
    let crew = crew(&router).await;
    let rev = reviewer(&router, &crew, "issue-rev", "crew/reviewer-5").await;
    let (status, written) =
        submit(&router, &crew.agent, "other-0", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{written}");
    let written = id(&written);
    assert_eq!(claim_next(&router, &rev, "other-next").await, written);

    // The claim is rev's: crew.dreamer holds none, so its default is none.
    let (status, refused) = verdict(
        &router,
        &crew.dreamer,
        "other-v-1",
        &json!({"verdict": "approve"}),
    )
    .await;
    assert_serve_id_named(status, &refused);
    assert_eq!(state(&router, &crew.agent, &written).await, "held");

    // The claimant's own default applies to it.
    let (status, applied) =
        verdict(&router, &rev, "other-v-2", &json!({"verdict": "approve"})).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["artifact_id"], written.as_str(), "{applied}");
}

/// One `POST /v2/review/verdicts` under `token`: the status, whether it
/// answered `Idempotency-Replay: true`, and the body.
async fn verdict_replay(
    router: &Router,
    token: &str,
    idem: &str,
    body: &Value,
) -> (StatusCode, bool, Value) {
    let request = Request::builder()
        .method("POST")
        .uri("/v2/review/verdicts")
        .header("authorization", format!("Bearer {token}"))
        .header("idempotency-key", idem)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds");
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("infallible router");
    let status = response.status();
    let replayed = response
        .headers()
        .get("idempotency-replay")
        .is_some_and(|v| v == "true");
    let bytes = to_bytes(response.into_body(), 1 << 22)
        .await
        .expect("body collects");
    (
        status,
        replayed,
        serde_json::from_slice(&bytes).expect("json body"),
    )
}

#[tokio::test]
async fn a_retried_verdict_without_serve_id_replays_its_stored_response_and_never_applies_to_the_next_claim()
 {
    let router = doubles_router();
    let crew = crew(&router).await;
    let (status, first) = submit(&router, &crew.agent, "rep-0", &json!({ "body": QUOTE })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    let first = id(&first);
    let (status, second) = submit(&router, &crew.agent, "rep-1", &json!({ "body": ALLOY })).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{second}");
    let second = id(&second);

    // The approve commits; its response is taken to be lost.
    assert_eq!(
        claim_next(&router, &crew.dreamer, "rep-next-1").await,
        first
    );
    let request = json!({"verdict": "approve"});
    let (status, replayed, applied) =
        verdict_replay(&router, &crew.dreamer, "rep-v-1", &request).await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert!(!replayed, "{applied}");
    assert_eq!(applied["artifact_id"], first.as_str(), "{applied}");

    // The same key claims the next item, then retries the lost request.
    assert_eq!(
        claim_next(&router, &crew.dreamer, "rep-next-2").await,
        second
    );
    let (status, replayed, replay) =
        verdict_replay(&router, &crew.dreamer, "rep-v-1", &request).await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert!(replayed, "{replay}");
    assert_eq!(replay, applied);
    assert_eq!(state(&router, &crew.agent, &second).await, "held");

    // The key names one request: another body under it is refused.
    let (status, mismatch) = verdict(
        &router,
        &crew.dreamer,
        "rep-v-1",
        &json!({"verdict": "reject", "reason": "a different request"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{mismatch}");
    assert_eq!(
        mismatch["error"]["code"], "idempotency.digest_mismatch",
        "{mismatch}"
    );
    assert_eq!(state(&router, &crew.agent, &second).await, "held");
}
