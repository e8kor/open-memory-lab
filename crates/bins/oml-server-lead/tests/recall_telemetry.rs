//! The recall telemetry over the real lead and a migrated Postgres store
//! under its role pair (ADR-0054 d2, d3, d4, d5, d6, d11, d13, as
//! accepted; ADR-0034 d8). The laws of who may read an event and when a
//! query's text is kept each carry their own control: the same probe, run
//! against a deliberately broken clause, finds the leak, so a green law is
//! never a probe that could not see one. Each test is named after the
//! sentence it proves.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnection, PgPool};
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::{HeldBy, MemoryKind, Role};
use oml_app::ed25519_dalek::SigningKey;
use oml_app::types::{
    BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, SubmitRequest, Verdict,
};
use oml_fetch::PinnedFetcher;
use oml_pg::PgStore;
use oml_server_lead::{LeadApp, lead_app, start_lead_with};
use oml_transport_http::traffic::{Traffic, TrustedProxies, metrics};

fn base_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev".to_owned())
}

const PAIR_PASSWORD: &str = "oml_test_pair";

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

fn owner_url(db: &str) -> String {
    role_url(db, &format!("{db}_owner"))
}

fn serving_url(db: &str) -> String {
    role_url(db, &format!("{db}_serving"))
}

static DB_N: AtomicU64 = AtomicU64::new(0);

/// A fresh id from the lead's own generator (the workspace's uuid has no
/// v4 feature).
fn fresh_id() -> Uuid {
    use oml_app::ports::IdGen as _;
    oml_server_lead::OsIds.new_id()
}

fn fresh_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!(
        "oml_test_tele_{}_{}_{nanos:08x}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
    )
}

/// Mints `db`'s role pair and the database, as every split suite does
/// (ADR-0034 d8); `false` where the login cannot create roles, which
/// `OML_TEST_REQUIRE_SPLIT=1` turns into a failure.
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

async fn call(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a readable body");
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, body)
}

fn post(path: &str, key: &str, body: &Value) -> Request<Body> {
    Request::post(path)
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("idempotency-key", fresh_id().to_string())
        .body(Body::from(body.to_string()))
        .expect("a request")
}

fn get(path: &str, key: &str) -> Request<Body> {
    Request::get(path)
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .body(Body::empty())
        .expect("a request")
}

/// One tenant on the lead: its admin, a writer that also reads, a reader,
/// and a dreamer that reviews.
struct Tenant {
    boot: BootstrapResult,
    writer: String,
    reader: String,
    dreamer: String,
}

struct World {
    db: String,
    store: PgStore,
    side: LeadApp,
    router: Router,
    traffic: Traffic,
    a: Tenant,
    b: Tenant,
}

async fn issue(
    app: &LeadApp,
    boot: &BootstrapResult,
    name: &str,
    role: Role,
    grants: &[Role],
) -> String {
    app.issue_key(
        &boot.admin_key.value,
        &IssueKeyRequest {
            role,
            name: Name::parse(name).expect("a name"),
            principal: format!("crew/{name}"),
            held_by: Some(HeldBy::Agent),
            default_workarea: Some(boot.root_workarea_id.into()),
            crew: None,
            grants: grants
                .iter()
                .map(|r| GrantRequest {
                    node: boot.root_workarea_id.into(),
                    role: *r,
                    deny: Vec::new(),
                    covers: Covers::Subtree,
                })
                .collect(),
            connect_code: false,
            user: None,
            guardrail_scope: None,
        },
    )
    .await
    .expect("a key")
    .value
}

async fn tenant(app: &LeadApp, name: &str) -> Tenant {
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse(name).expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: format!("{name}-admin"),
        })
        .await
        .expect("bootstrap");
    let writer = issue(
        app,
        &boot,
        "writer",
        Role::Writer,
        &[Role::Writer, Role::Reader],
    )
    .await;
    let reader = issue(app, &boot, "reader", Role::Reader, &[Role::Reader]).await;
    let dreamer = issue(
        app,
        &boot,
        "dreamer",
        Role::Dreamer,
        &[Role::Dreamer, Role::Reader],
    )
    .await;
    Tenant {
        boot,
        writer,
        reader,
        dreamer,
    }
}

async fn world() -> Option<World> {
    let db = fresh_name();
    if !mint_database(&db).await {
        return None;
    }
    let store = PgStore::connect(&serving_url(&db))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db));
    store
        .migrate(&owner_url(&db))
        .await
        .expect("embedded migrations apply");
    let key = SigningKey::from_bytes(&[11; 32]);
    let side = lead_app(&store, key.clone());
    let a = tenant(&side, "acme").await;
    let b = tenant(&side, "globex").await;
    let traffic = Traffic::new(TrustedProxies::default());
    let router = start_lead_with(
        &store,
        key,
        None,
        PinnedFetcher,
        traffic.clone(),
        None,
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
    )
    .await
    .expect("the lead starts")
    .router;
    Some(World {
        db,
        store,
        side,
        router,
        traffic,
        a,
        b,
    })
}

impl World {
    /// A memory of `t` written by its writer and approved by its dreamer.
    async fn promoted(&self, t: &Tenant, body: &str) -> Uuid {
        let id = self
            .side
            .submit_memory(
                &t.writer,
                &SubmitRequest {
                    idempotency_key: fresh_id().to_string(),
                    claim_key: None,
                    verb: None,
                    kind: MemoryKind::Memory,
                    body: body.to_owned(),
                    workarea: None,
                    derives_from: Vec::new(),
                    severity: None,
                    rule: None,
                },
            )
            .await
            .expect("submit")
            .receipt
            .memory_id;
        self.side
            .post_verdict(&t.dreamer, id, Verdict::Approve, None)
            .await
            .expect("approve");
        id
    }

    async fn recall(&self, key: &str, query: &str) -> Value {
        let (status, body) = call(
            &self.router,
            post("/v1/memories/recall", key, &json!({ "query": query })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// The tenant's events as its admin reads them, which writes the queue
    /// first.
    async fn events(&self, t: &Tenant) -> Vec<Value> {
        let (status, body) = call(
            &self.router,
            get("/v2/usage/events?limit=200", &t.boot.admin_key.value),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["items"].as_array().cloned().unwrap_or_default()
    }

    async fn set_query_text(&self, t: &Tenant, on: bool) {
        let (status, body) = call(
            &self.router,
            post(
                "/v2/usage/settings",
                &t.boot.admin_key.value,
                &json!({ "query_text": on }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["query_text"], on, "{body}");
    }

    async fn serving(&self) -> PgPool {
        PgPool::connect(&serving_url(&self.db))
            .await
            .expect("serving")
    }

    async fn owner(&self) -> PgConnection {
        PgConnection::connect(&owner_url(&self.db))
            .await
            .expect("owner")
    }
}

/// Rows of `recall_events` the serving role reads with `oml.tenant_id` set
/// to `tenant` (or unset), and of them those holding query text.
async fn visible(pool: &PgPool, tenant: Option<Uuid>) -> (i64, i64) {
    let mut tx = pool.begin().await.expect("tx");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.map(|t| t.to_string()).unwrap_or_default())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let row: (i64, i64) = sqlx::query_as("SELECT count(*), count(query_text) FROM recall_events")
        .fetch_one(&mut *tx)
        .await
        .expect("count");
    tx.rollback().await.expect("rollback");
    row
}

#[tokio::test]
async fn a_recall_answers_its_recall_id_and_its_event_records_the_digest_and_the_served_hits() {
    let Some(w) = world().await else { return };
    let id = w.promoted(&w.a, "the deploy runs on fridays").await;
    let answer = w.recall(&w.a.reader, "deploy fridays").await;
    let recall_id = answer["recall_id"]
        .as_str()
        .expect("a recall_id")
        .to_owned();
    let events = w.events(&w.a).await;
    assert_eq!(events.len(), 1, "{events:?}");
    let e = &events[0];
    assert_eq!(e["recall_id"], recall_id.as_str());
    assert_eq!(e["query_digest"], answer["query_digest"]);
    assert_eq!(e["hits"][0]["artifact_id"], id.to_string());
    assert_eq!(e["pipeline"], answer["search"]["pipeline"]);
    assert_eq!(e["query_text"], Value::Null, "no text without the opt-in");
    drop_db(&w.db).await;
}

#[tokio::test]
async fn query_text_is_null_unless_the_tenant_opted_in() {
    let Some(w) = world().await else { return };
    w.promoted(&w.a, "the deploy runs on fridays").await;
    w.recall(&w.a.reader, "deploy before opt-in").await;
    w.events(&w.a).await;
    let pool = w.serving().await;
    assert_eq!(visible(&pool, Some(w.a.boot.tenant_id)).await, (1, 0));

    w.set_query_text(&w.a, true).await;
    w.recall(&w.a.reader, "deploy after opt-in").await;
    let events = w.events(&w.a).await;
    assert_eq!(events[0]["query_text"], "deploy after opt-in");
    assert_eq!(
        events[1]["query_text"],
        Value::Null,
        "an earlier event keeps no text"
    );
    assert_eq!(visible(&pool, Some(w.a.boot.tenant_id)).await, (2, 1));

    // Off stops the text and clears what was kept, in the same commit.
    w.set_query_text(&w.a, false).await;
    w.recall(&w.a.reader, "deploy after opt-out").await;
    w.events(&w.a).await;
    assert_eq!(visible(&pool, Some(w.a.boot.tenant_id)).await, (3, 0));

    // The control: the same probe counts text the moment a row holds it,
    // as a writer that ignored the opt-in would leave it.
    let mut tx = pool.begin().await.expect("tx");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(w.a.boot.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set");
    tx.execute("UPDATE recall_events SET query_text = 'leaked'")
        .await
        .expect("broken clause");
    tx.commit().await.expect("commit");
    assert_eq!(
        visible(&pool, Some(w.a.boot.tenant_id)).await,
        (3, 3),
        "the probe cannot see text"
    );
    drop_db(&w.db).await;
}

#[tokio::test]
async fn no_agent_writer_reader_or_dreamer_key_reads_a_recall_event_on_any_transport() {
    let Some(w) = world().await else { return };
    w.promoted(&w.a, "the deploy runs on fridays").await;
    w.recall(&w.a.reader, "deploy fridays").await;
    for key in [&w.a.writer, &w.a.reader, &w.a.dreamer] {
        for request in [
            get("/v2/usage/events", key),
            get("/v2/usage/settings", key),
            post("/v2/usage/settings", key, &json!({ "query_text": true })),
            post(
                "/v2/usage/eval",
                key,
                &json!({ "pipelines": ["bm25-postings"] }),
            ),
        ] {
            let path = request.uri().to_string();
            let (status, body) = call(&w.router, request).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
            assert!(!body.to_string().contains("deploy"), "{path}: {body}");
        }
        // /mcp serves no tool that reaches an event, and oml_read has no
        // mode that does.
        let (_, body) = call(
            &w.router,
            post(
                "/mcp",
                key,
                &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                        "params": {"name": "oml_usage", "arguments": {}}}),
            ),
        )
        .await;
        assert!(
            body.get("error").is_some() || body["result"]["isError"] == true,
            "{body}"
        );
        let (_, body) = call(
            &w.router,
            post(
                "/mcp",
                key,
                &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                        "params": {"name": "oml_read", "arguments": {"events": true}}}),
            ),
        )
        .await;
        assert_eq!(body["result"]["isError"], true, "{body}");
    }
    // The tenant's admin reads them.
    assert_eq!(w.events(&w.a).await.len(), 1);
    drop_db(&w.db).await;
}

#[tokio::test]
async fn no_tenant_reads_another_tenants_recall_events_and_the_probe_finds_a_leak_without_the_fence()
 {
    let Some(w) = world().await else { return };
    w.promoted(&w.a, "the deploy runs on fridays").await;
    w.recall(&w.a.reader, "deploy fridays").await;
    assert_eq!(w.events(&w.a).await.len(), 1);
    assert!(w.events(&w.b).await.is_empty(), "b's admin reads a's event");
    let pool = w.serving().await;
    assert_eq!(visible(&pool, Some(w.a.boot.tenant_id)).await.0, 1);
    assert_eq!(visible(&pool, Some(w.b.boot.tenant_id)).await.0, 0);
    assert_eq!(
        visible(&pool, None).await.0,
        0,
        "no tenant set reads nothing"
    );

    // The control: the fence broken on the parent and on every partition,
    // the same probe as b now reads a's event.
    let mut owner = w.owner().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT relname::text FROM pg_class WHERE relname LIKE 'recall\\_events%' AND relkind IN ('r', 'p')",
    )
    .fetch_all(&mut owner)
    .await
    .expect("tables");
    for t in &tables {
        owner
            .execute(format!("ALTER TABLE {t} DISABLE ROW LEVEL SECURITY").as_str())
            .await
            .expect("broken clause");
    }
    assert_eq!(
        visible(&pool, Some(w.b.boot.tenant_id)).await.0,
        1,
        "the probe cannot see a leak"
    );
    drop_db(&w.db).await;
}

#[tokio::test]
async fn feedback_for_another_keys_recall_or_an_unserved_id_is_refused_and_the_read_not_run() {
    let Some(w) = world().await else { return };
    let used = w.promoted(&w.a, "the deploy runs on fridays").await;
    let other = w.promoted(&w.a, "lunch is at noon").await;
    let answer = w.recall(&w.a.reader, "deploy fridays").await;
    let recall_id = answer["recall_id"]
        .as_str()
        .expect("a recall_id")
        .to_owned();
    let recall_with = |key: &str, feedback: Value| {
        post(
            "/v1/memories/recall",
            key,
            &json!({ "query": "lunch", "feedback": feedback }),
        )
    };
    for (key, feedback, field) in [
        // Another key's recall.
        (
            &w.a.writer,
            json!({ "recall_id": recall_id, "useful": [used] }),
            "/feedback/recall_id",
        ),
        // A version that recall did not serve.
        (
            &w.a.reader,
            json!({ "recall_id": recall_id, "useful": [other] }),
            "/feedback/useful/0",
        ),
        // A recall that does not exist.
        (
            &w.a.reader,
            json!({ "recall_id": fresh_id(), "useful": [used] }),
            "/feedback/recall_id",
        ),
        // Not the member's shape.
        (
            &w.a.reader,
            json!({ "recall_id": recall_id, "usefull": [used] }),
            "/feedback",
        ),
    ] {
        let (status, body) = call(&w.router, recall_with(key, feedback)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"]["code"], "request.validation", "{body}");
        assert_eq!(body["error"]["details"]["field"], field, "{body}");
    }
    // None of the refused reads ran: the tenant holds the first recall alone.
    assert_eq!(w.events(&w.a).await.len(), 1);

    // Valid feedback rides the next read and labels the first recall.
    let (status, body) = call(
        &w.router,
        recall_with(
            &w.a.reader,
            json!({ "recall_id": recall_id, "useful": [used] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let events = w.events(&w.a).await;
    let first = events
        .iter()
        .find(|e| e["recall_id"] == recall_id.as_str())
        .expect("the first event");
    assert_eq!(first["labels"][0]["label"], "feedback_useful", "{first}");
    assert_eq!(first["labels"][0]["artifact_id"], used.to_string());
    assert_eq!(first["labels"][0]["source"], "explicit");
    drop_db(&w.db).await;
}

#[tokio::test]
async fn a_get_a_citation_and_a_second_recall_in_other_words_label_the_keys_recall() {
    let Some(w) = world().await else { return };
    let id = w.promoted(&w.a, "the deploy runs on fridays").await;
    let first = w.recall(&w.a.writer, "deploy fridays").await;
    let (status, _) = call(&w.router, get(&format!("/v1/memories/{id}"), &w.a.writer)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        &w.router,
        post(
            "/v1/memories",
            &w.a.writer,
            &json!({ "body": "deploys move to thursdays", "derives_from": [id] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // Another key's get labels nothing of this key's recall.
    call(&w.router, get(&format!("/v1/memories/{id}"), &w.a.reader)).await;
    // A second recall in other words with no positive: requery.
    let unhelped = w.recall(&w.a.writer, "release calendar").await;
    w.recall(&w.a.writer, "shipping schedule").await;
    let events = w.events(&w.a).await;
    let labels = |recall: &Value| -> Vec<String> {
        let e = events
            .iter()
            .find(|e| e["recall_id"] == recall["recall_id"])
            .expect("its event");
        let mut l: Vec<String> = e["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|l| l["label"].as_str().unwrap_or_default().to_owned())
            .collect();
        l.sort();
        l
    };
    assert_eq!(labels(&first), vec!["derived", "get"]);
    assert_eq!(labels(&unhelped), vec!["requery"]);
    drop_db(&w.db).await;
}

#[tokio::test]
async fn a_replay_scores_the_labelled_queries_and_its_answer_holds_no_text_and_no_id() {
    let Some(w) = world().await else { return };
    let used = w.promoted(&w.a, "the deploy runs on fridays").await;
    w.promoted(&w.a, "the deploy freeze starts in december")
        .await;
    w.set_query_text(&w.a, true).await;
    let answer = w.recall(&w.a.reader, "deploy fridays").await;
    let feedback = json!({ "recall_id": answer["recall_id"], "useful": [used] });
    let (status, body) = call(
        &w.router,
        post(
            "/v1/memories/recall",
            &w.a.reader,
            &json!({ "query": "zzz", "feedback": feedback }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, report) = call(
        &w.router,
        post(
            "/v2/usage/eval",
            &w.a.boot.admin_key.value,
            &json!({ "pipelines": ["bm25-postings", "keyword-any"] }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["labelled"], 1, "{report}");
    assert_eq!(report["pipelines"][0]["queries"], 1, "{report}");
    assert_eq!(report["pipelines"][0]["success_at_1"], 1.0, "{report}");
    let text = report.to_string();
    for leak in [
        "deploy",
        "fridays",
        &used.to_string(),
        answer["recall_id"].as_str().unwrap_or("-"),
    ] {
        assert!(!text.contains(leak), "the replay names {leak}: {text}");
    }
    // Numbers, counts and pipeline names alone.
    let uuid_like = text
        .split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .any(|t| t.len() == 36 && t.matches('-').count() == 4);
    assert!(!uuid_like, "{text}");
    // The replay records no event of its own.
    assert_eq!(w.events(&w.a).await.len(), 2);
    drop_db(&w.db).await;
}

#[tokio::test]
async fn no_row_older_than_30_days_is_read_and_an_overdue_partition_is_emptied_and_dropped() {
    let Some(w) = world().await else { return };
    let tenant = w.a.boot.tenant_id;
    let hex = tenant.simple().to_string();
    let mut owner = w.owner().await;
    // Two week partitions behind the rotation's: one that ended 35 days
    // before this week, and one from then to this week, as an outage of the
    // rotation leaves them; rows dated 40, 31 and 1 days ago.
    let week = |weeks_back: i64| {
        format!(
            "SELECT to_char(date_trunc('week', now() AT TIME ZONE 'UTC') - interval '{} days', 'YYYYMMDD')",
            7 * weeks_back
        )
    };
    let mut bounds = Vec::new();
    for weeks_back in [6, 5, 0] {
        let d: String = sqlx::query_scalar(&week(weeks_back))
            .fetch_one(&mut owner)
            .await
            .expect("a monday");
        bounds.push(d);
    }
    for (from, to) in [(&bounds[0], &bounds[1]), (&bounds[1], &bounds[2])] {
        for table in ["recall_events", "recall_outcomes"] {
            let p = format!("{table}_t_{hex}_w{from}");
            for s in [
                format!("CREATE TABLE {p} (LIKE {table} INCLUDING ALL)"),
                format!("ALTER TABLE {p} ENABLE ROW LEVEL SECURITY"),
                format!("ALTER TABLE {p} FORCE ROW LEVEL SECURITY"),
                format!(
                    "CREATE POLICY tenant_isolation ON {p} USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)"
                ),
                format!(
                    "ALTER TABLE {table}_t_{hex} ATTACH PARTITION {p} FOR VALUES FROM ('{from} 00:00:00+00') TO ('{to} 00:00:00+00')"
                ),
            ] {
                owner.execute(s.as_str()).await.expect("an old week");
            }
        }
    }
    let attached_weeks: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_inherits WHERE inhparent = $1::regclass")
            .bind(format!("recall_events_t_{hex}"))
            .fetch_one(&mut owner)
            .await
            .expect("weeks");
    let pool = w.serving().await;
    let insert = |days: i64| {
        format!(
            "INSERT INTO recall_events (tenant_id, recall_id, created_at, key_id, principal, \
               scope_workarea_id, pipeline, access, query_digest, hit_ids, hit_scores) \
             VALUES ('{tenant}', gen_random_uuid(), now() - interval '{days} days', gen_random_uuid(), \
               'p', gen_random_uuid(), 'bm25', 'scan', decode(repeat('00', 32), 'hex'), '{{}}', '{{}}')"
        )
    };
    let mut tx = pool.begin().await.expect("tx");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set");
    for days in [40, 31, 1] {
        tx.execute(insert(days).as_str()).await.expect("a row");
    }
    tx.commit().await.expect("commit");
    // No read serves a row past the 30 days, purged or not.
    assert_eq!(w.events(&w.a).await.len(), 1);
    assert_eq!(visible(&pool, Some(tenant)).await.0, 3);

    // A writer's first pass purges the rows; the rotation drops the week.
    let (_t, writer) = oml_pg::telemetry::PgTelemetry::spawn(
        pool.clone(),
        Arc::clone(w.traffic.metrics()) as Arc<dyn oml_app::ports::TelemetryMeter>,
    );
    let mut purged = false;
    for _ in 0..50 {
        if visible(&pool, Some(tenant)).await.0 == 1 {
            purged = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    writer.abort();
    assert!(purged, "the pass left rows past the 30 days");
    let rotated = w
        .store
        .rotate_telemetry()
        .await
        .expect("an owner URL")
        .expect("the rotation");
    assert_eq!(rotated.dropped, 2, "{rotated:?}");
    let weeks_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_inherits WHERE inhparent = $1::regclass")
            .bind(format!("recall_events_t_{hex}"))
            .fetch_one(&mut owner)
            .await
            .expect("weeks");
    assert_eq!(weeks_after, attached_weeks - 1);
    drop_db(&w.db).await;
}

#[tokio::test]
async fn an_erased_version_leaves_no_text_no_hit_and_no_label_in_the_telemetry() {
    let Some(w) = world().await else { return };
    let erased = w.promoted(&w.a, "the deploy runs on fridays").await;
    let kept = w
        .promoted(&w.a, "the deploy freeze starts in december")
        .await;
    w.set_query_text(&w.a, true).await;
    let answer = w.recall(&w.a.writer, "deploy").await;
    call(
        &w.router,
        get(&format!("/v1/memories/{erased}"), &w.a.writer),
    )
    .await;
    let events = w.events(&w.a).await;
    assert_eq!(events[0]["hits"].as_array().map(Vec::len), Some(2));
    let pool = w.serving().await;
    let mut tx = pool.begin().await.expect("tx");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(w.a.boot.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set");
    let touched: i64 = sqlx::query_scalar("SELECT oml_telemetry_erase($1, $2)")
        .bind(w.a.boot.tenant_id)
        .bind(vec![erased])
        .fetch_one(&mut *tx)
        .await
        .expect("erase");
    tx.commit().await.expect("commit");
    assert_eq!(touched, 1);
    let events = w.events(&w.a).await;
    let e = events
        .iter()
        .find(|e| e["recall_id"] == answer["recall_id"])
        .expect("the event");
    assert_eq!(e["query_text"], Value::Null);
    assert_eq!(e["hits"].as_array().map(Vec::len), Some(1));
    assert_eq!(e["hits"][0]["artifact_id"], kept.to_string());
    assert!(!e.to_string().contains(&erased.to_string()), "{e}");
    drop_db(&w.db).await;
}

#[tokio::test]
async fn every_quality_metric_label_is_from_its_declared_set() {
    let Some(w) = world().await else { return };
    let id = w.promoted(&w.a, "the deploy runs on fridays").await;
    w.recall(&w.a.reader, "deploy").await;
    w.recall(&w.a.reader, "nothing matches this").await;
    call(&w.router, get(&format!("/v1/memories/{id}"), &w.a.reader)).await;
    let (status, body) = call(
        &w.router,
        post(
            "/v1/memories",
            &w.a.writer,
            &json!({ "body": "the deploy runs on fridays", "claim_key": "deploy-day-again" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    w.events(&w.a).await;
    let scrape = metrics::router(Arc::clone(w.traffic.metrics()));
    let (_, text) = call(
        &scrape,
        Request::get("/metrics")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    let text = text.as_str().unwrap_or_default().to_owned();
    let declared: &[(&str, &str, &[&str])] = &[
        (
            "oml_recall_answers_total",
            "result",
            &["hits", "empty", "below_floor"],
        ),
        (
            "oml_recall_outcomes_total",
            "label",
            &[
                "get",
                "derived",
                "superseded",
                "feedback_useful",
                "feedback_not_useful",
                "requery",
                "no_use",
            ],
        ),
        (
            "oml_recall_events_total",
            "outcome",
            &[
                "written",
                "dropped_queue_full",
                "dropped_store_error",
                "dropped_no_partition",
            ],
        ),
        (
            "oml_recall_labels_dropped_total",
            "reason",
            &["rate_limited", "queue_full", "store_error"],
        ),
        (
            "oml_remember_duplicates_total",
            "relation",
            &["same_text", "none"],
        ),
        (
            "oml_review_wait_seconds_count",
            "verdict",
            &["approve", "reject"],
        ),
        (
            "oml_memories_unrecalled",
            "promoted_for",
            &["7d", "30d", "90d", "older"],
        ),
    ];
    for (family, label, values) in declared {
        for line in text
            .lines()
            .filter(|l| l.starts_with(&format!("{family}{{")))
        {
            let set = &line[family.len() + 1..line.find('}').expect("a label set")];
            for pair in set.split(',') {
                let (k, v) = pair.split_once('=').expect("k=v");
                let v = v.trim_matches('"');
                if k == *label {
                    assert!(values.contains(&v), "{family} {k}={v}");
                } else {
                    assert_eq!(k, "pipeline", "{family} carries an undeclared label {k}");
                }
            }
        }
    }
    for expected in [
        r#"oml_recall_answers_total{pipeline="bm25-postings",result="hits"} 1"#,
        r#"oml_recall_answers_total{pipeline="bm25-postings",result="empty"} 1"#,
        r#"oml_recall_outcomes_total{label="get"} 1"#,
        r#"oml_recall_events_total{outcome="written"} 2"#,
        r#"oml_remember_duplicates_total{relation="same_text"} 1"#,
        "oml_recall_duration_seconds_count{pipeline=\"bm25-postings\"} 2",
    ] {
        assert!(text.contains(expected), "no {expected} in:\n{text}");
    }
    // No sample names a tenant, a key, a principal or a query word; the
    // help lines may say what a family counts.
    let samples: String = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "tenant",
        "key_id",
        "principal",
        "deploy",
        &w.a.boot.tenant_id.to_string(),
    ] {
        assert!(!samples.contains(forbidden), "/metrics names {forbidden}");
    }
    drop_db(&w.db).await;
}

/// The event write's cost on a recall: the same recalls, through the
/// router, with the writer on and with it off, p50 and p95 printed
/// (FR-104). Run by hand: `cargo test -p oml-server-lead --test
/// recall_telemetry -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a measurement, run by hand"]
async fn the_event_writes_cost_on_a_recalls_p95() {
    let Some(w) = world().await else { return };
    for i in 0..200 {
        w.promoted(
            &w.a,
            &format!("note {i} on the deploy of service {}", i % 17),
        )
        .await;
    }
    let off = lead_app(&w.store, SigningKey::from_bytes(&[11; 32]));
    let rounds = 2_000;
    let mut on_ms = Vec::with_capacity(rounds);
    let mut off_ms = Vec::with_capacity(rounds);
    let req = |i: usize| oml_app::types::RecallRequest {
        query: format!("deploy service {}", i % 17),
        k: 8,
        workarea: None,
        kinds: Vec::new(),
        include_text: true,
        include_unreviewed: false,
        max_tokens: None,
        search: None,
        match_mode: oml_app::search::MatchMode::Any,
    };
    // The router's app holds the writer; the side app holds none. Both
    // run the same recall against the same store, interleaved.
    let (telemetry, _writer) = oml_pg::telemetry::PgTelemetry::spawn(
        w.serving().await,
        Arc::clone(w.traffic.metrics()) as Arc<dyn oml_app::ports::TelemetryMeter>,
    );
    let on = lead_app(&w.store, SigningKey::from_bytes(&[11; 32])).with_telemetry(telemetry);
    for i in 0..rounds {
        let t = std::time::Instant::now();
        on.recall(w.a.reader.as_str(), &req(i))
            .await
            .expect("recall");
        on_ms.push(t.elapsed().as_secs_f64() * 1e3);
        let t = std::time::Instant::now();
        off.recall(w.a.reader.as_str(), &req(i))
            .await
            .expect("recall");
        off_ms.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let pct = |v: &mut Vec<f64>, p: f64| {
        v.sort_by(f64::total_cmp);
        v[((v.len() as f64 - 1.0) * p).round() as usize]
    };
    println!(
        "recall over {rounds} rounds, 200 memories: writer on p50 {:.3} ms p95 {:.3} ms; off p50 {:.3} ms p95 {:.3} ms",
        pct(&mut on_ms, 0.5),
        pct(&mut on_ms, 0.95),
        pct(&mut off_ms, 0.5),
        pct(&mut off_ms, 0.95),
    );
    drop_db(&w.db).await;
}
