//! Bulk review over the lead composition against a throwaway database
//! (ADR-0055): the listing a human reviewer key is served, the bulk verdict
//! and its per-item outcomes, the laws the record names — an agent key
//! never reaches it on any transport, an item the reviewer's own key or
//! principal is in the lineage of is skipped and never approved, a stale id
//! is reported and never approved, every approved item carries its own
//! journalled verdict and ratification equal to a single approve's, and a
//! retry replays — and, ignored by default, the scale measurement of the
//! queue at tens of thousands held and of approving 600 items in bulk.

use std::sync::atomic::Ordering;
use std::time::Instant;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::IdGen as _;
use oml_pg::PgStore;
use oml_server_lead::{OsIds, lead_state, pg_ping};
use oml_test_pg::{DB_N, base_url, drop_db, mint_database, owner_url, run_tag, serving_url};

const ENROLLMENT: &str = "enr_test_fixture_token";

// ---------------------------------------------------------------------------
// Throwaway database + rig (the oml-pg test pattern)
// ---------------------------------------------------------------------------

/// `db` through `DATABASE_URL`'s own login, the superuser the role pair
/// was minted by: the journal reads below see every tenant's rows, which
/// row security hides from the owner (ADR-0034 d8).
fn super_url(db: &str) -> String {
    let base = base_url();
    let (head, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{head}/{db}")
}

struct Rig {
    router: Router,
    store: PgStore,
    db_name: String,
}

async fn fresh_rig() -> Option<Rig> {
    let db_name = format!(
        "oml_test_bulk_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db_name).await {
        return None;
    }
    let store = PgStore::connect(&serving_url(&db_name))
        .await
        .expect("connect")
        .with_owner_url(&owner_url(&db_name));
    store
        .migrate(&owner_url(&db_name))
        .await
        .expect("embedded migrations apply");
    let signing = SigningKey::from_bytes(&OsIds.new_secret());
    let router = lead_state(
        &store,
        signing,
        pg_ping(store.pool().clone()),
        true,
        Some(ENROLLMENT.to_owned()),
    )
    .into_router();
    Some(Rig {
        router,
        store,
        db_name,
    })
}

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

async fn send(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    idem: Option<&str>,
    body: Option<Value>,
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
    let bytes = to_bytes(response.into_body(), 1 << 24)
        .await
        .expect("body collects");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("responses are JSON")
    };
    (status, value)
}

fn error_code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("<no code>")
}

/// The bootstrap ceremony; returns (admin token, root workarea id).
async fn bootstrap(router: &Router) -> (String, Uuid) {
    let (status, body) = send(
        router,
        "POST",
        "/v2/bootstrap/claim",
        None,
        None,
        Some(json!({
            "enrollment_token": ENROLLMENT,
            "name": "root-admin",
            "tenant_name": "test-tenant",
            "admin_principal": "platform/admin",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "bootstrap claim: {body}");
    let token = body["value"].as_str().expect("show-once value").to_owned();
    let root = Uuid::parse_str(body["root_workarea_id"].as_str().expect("root id")).expect("uuid");
    (token, root)
}

/// Issues a key named `name` under the admin token; returns the 201 body.
async fn issue(
    router: &Router,
    admin: &str,
    name: &str,
    role: &str,
    principal: &str,
    held_by: &str,
    grants: Value,
) -> Value {
    let (status, body) = send(
        router,
        "POST",
        "/v2/keys",
        Some(admin),
        Some(&format!("issue-{name}")),
        Some(json!({
            "role": role, "name": name, "principal": principal,
            "held_by": held_by, "grants": grants,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "issue {name}: {body}");
    body
}

fn value_of(issued: &Value) -> String {
    issued["value"]
        .as_str()
        .expect("show-once value")
        .to_owned()
}

fn id_of(issued: &Value) -> String {
    issued["key_id"].as_str().expect("key id").to_owned()
}

async fn create_workarea(router: &Router, admin: &str, parent: Uuid, name: &str) -> Uuid {
    let (status, body) = send(
        router,
        "POST",
        "/v2/workareas",
        Some(admin),
        Some(&format!("wa-{name}")),
        Some(json!({ "parent": parent, "name": name })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    Uuid::parse_str(body["id"].as_str().expect("workarea id")).expect("uuid")
}

/// One held write of `claim_key` under `token`; returns its artifact id.
async fn write(
    router: &Router,
    token: &str,
    workarea: Uuid,
    claim_key: &str,
    derives_from: &[&str],
) -> String {
    let mut body = json!({
        "claim_key": claim_key,
        "kind": "memory",
        "body": format!("Decision {claim_key}: the record states it."),
        "workarea": workarea.to_string(),
    });
    if !derives_from.is_empty() {
        body["derives_from"] = json!(derives_from);
    }
    let (status, accepted) = send(
        router,
        "POST",
        "/v1/memories",
        Some(token),
        Some(&format!("w-{claim_key}")),
        Some(body),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "write {claim_key}: {accepted}"
    );
    accepted["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_owned()
}

/// The workarea, an admin, a writer key held by an agent, a reviewer key
/// held by a person, a dreamer key held by an agent, all on one node.
struct World {
    rig: Rig,
    admin: String,
    node: Uuid,
    agent: String,
    agent_issued: Value,
    reviewer: String,
    reviewer_issued: Value,
    agent_dreamer: String,
}

async fn world() -> Option<World> {
    let rig = fresh_rig().await?;
    let (admin, root) = bootstrap(&rig.router).await;
    let node = create_workarea(&rig.router, &admin, root, "adr-import").await;
    let writer_grants = json!([
        { "workarea": node, "role": "writer" },
        { "workarea": node, "role": "reader" },
    ]);
    let dreamer_grants = json!([
        { "workarea": node, "role": "dreamer" },
        { "workarea": node, "role": "reader" },
    ]);
    let agent_issued = issue(
        &rig.router,
        &admin,
        "loader",
        "writer",
        "crew/loader",
        "agent",
        writer_grants,
    )
    .await;
    let reviewer_issued = issue(
        &rig.router,
        &admin,
        "eugene-review",
        "dreamer",
        "eugene-review",
        "human",
        dreamer_grants.clone(),
    )
    .await;
    let agent_dreamer = value_of(
        &issue(
            &rig.router,
            &admin,
            "crew-reviewer",
            "dreamer",
            "crew/reviewer",
            "agent",
            dreamer_grants,
        )
        .await,
    );
    Some(World {
        admin,
        node,
        agent: value_of(&agent_issued),
        agent_issued,
        reviewer: value_of(&reviewer_issued),
        reviewer_issued,
        agent_dreamer,
        rig,
    })
}

async fn held_listing(router: &Router, token: &str, query: &str) -> (StatusCode, Value) {
    send(
        router,
        "GET",
        &format!("/v2/review/held{query}"),
        Some(token),
        None,
        None,
    )
    .await
}

async fn bulk(
    router: &Router,
    token: &str,
    idem: &str,
    verdict: &str,
    ids: &[String],
    reason: Option<&str>,
) -> (StatusCode, Value) {
    let mut body = json!({ "verdict": verdict, "artifact_ids": ids });
    if let Some(reason) = reason {
        body["reason"] = json!(reason);
    }
    send(
        router,
        "POST",
        "/v2/review/verdicts/bulk",
        Some(token),
        Some(idem),
        Some(body),
    )
    .await
}

/// The per-item outcome the bulk answer gives `id`.
fn outcome_of<'a>(answer: &'a Value, id: &str) -> &'a Value {
    answer["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["artifact_id"] == id)
        .unwrap_or_else(|| panic!("no outcome for {id}: {answer}"))
}

/// The journal's events naming `artifact_id`, in seq order, as
/// `(event_type, principal, key_id, payload)`.
async fn journal_of(rig: &Rig, artifact_id: &str) -> Vec<(String, String, Option<Uuid>, Value)> {
    let mut conn = PgConnection::connect(&super_url(&rig.db_name))
        .await
        .expect("owner connect");
    let rows: Vec<(String, String, Option<Uuid>, Value)> = sqlx::query_as(
        "SELECT event_type, principal, api_key_id, payload FROM journal \
         WHERE payload->>'artifact_id' = $1 OR payload->>'new_artifact_id' = $1 \
         ORDER BY seq",
    )
    .bind(artifact_id)
    .fetch_all(&mut conn)
    .await
    .expect("journal read");
    rows
}

/// One approval the single way: the crew's reviewer is served `artifact`
/// next and approves it.
async fn approve_single(router: &Router, dreamer: &str, artifact: &str, tag: &str) -> Value {
    let (status, serve) = send(
        router,
        "POST",
        "/v2/review/next",
        Some(dreamer),
        Some(&format!("next-{tag}")),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    assert_eq!(serve["item"]["artifact_id"], artifact, "{serve}");
    let (status, verdict) = send(
        router,
        "POST",
        "/v2/review/verdicts",
        Some(dreamer),
        Some(&format!("verdict-{tag}")),
        Some(json!({"serve_id": serve["serve_id"], "verdict": "approve"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{verdict}");
    verdict
}

async fn mcp(router: &Router, token: &str, method: &str, params: Value) -> Value {
    let (status, body) = send(
        router,
        "POST",
        "/mcp",
        Some(token),
        None,
        Some(json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{method}: {body}");
    body
}

fn listed_ids(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap_or_else(|| panic!("items: {page}"))
        .iter()
        .map(|i| i["artifact_id"].as_str().expect("id").to_owned())
        .collect()
}

#[tokio::test]
async fn an_agent_key_cannot_list_or_bulk_approve_on_any_transport() {
    let Some(w) = world().await else {
        return;
    };
    let r = &w.rig.router;
    let a = write(r, &w.agent, w.node, "adr-0001-d1", &[]).await;

    // HTTP: the listing and the bulk verdict refuse a writer key held by an
    // agent at the verb ceiling, naming the rule, before any lookup (R1).
    let (status, body) = held_listing(r, &w.agent, "").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "access.verb_ceiling", "{body}");
    let (status, body) = bulk(
        r,
        &w.agent,
        "agent-bulk",
        "approve",
        std::slice::from_ref(&a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(error_code(&body), "access.verb_ceiling", "{body}");

    // MCP: the catalog is the three agent tools, no bulk or review tool is
    // served, and a call naming one is an unknown tool.
    let listed = mcp(r, &w.agent, "tools/list", json!({})).await;
    let mut names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|t| t["name"].as_str().expect("name"))
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["oml_read", "oml_workarea", "oml_write"]);
    // Every key an agent may hold, the writer and the crew's dreamer, and
    // the person's own reviewer key too: /mcp names no review tool for any
    // of them, and a call naming one is an unknown tool.
    for (who, token) in [
        ("writer", &w.agent),
        ("crew dreamer", &w.agent_dreamer),
        ("person's reviewer", &w.reviewer),
    ] {
        let listed = mcp(r, token, "tools/list", json!({})).await;
        if let Some(tools) = listed["result"]["tools"].as_array() {
            assert!(
                tools.iter().all(|t| {
                    let name = t["name"].as_str().unwrap_or_default();
                    !name.contains("review") && !name.contains("approve") && !name.contains("bulk")
                }),
                "{who}: {listed}"
            );
        }
        for tool in ["oml_review_bulk", "oml_review", "oml_approve"] {
            let called = mcp(
                r,
                token,
                "tools/call",
                json!({"name": tool, "arguments": {"artifact_ids": [a], "verdict": "approve"}}),
            )
            .await;
            assert!(
                called["error"]["code"] == -32602 || called["result"]["isError"] == true,
                "{who} {tool}: {called}"
            );
        }
    }

    // The row is still held: the person's reviewer key is listed it.
    let (status, page) = held_listing(r, &w.reviewer, "").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed_ids(&page), vec![a]);
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn a_dreamer_key_held_by_an_agent_is_refused_bulk_review() {
    let Some(w) = world().await else {
        return;
    };
    let r = &w.rig.router;
    let a = write(r, &w.agent, w.node, "adr-0001-d1", &[]).await;
    for (status, body) in [
        held_listing(r, &w.agent_dreamer, "").await,
        bulk(
            r,
            &w.agent_dreamer,
            "crew-bulk",
            "approve",
            std::slice::from_ref(&a),
            None,
        )
        .await,
    ] {
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(error_code(&body), "access.verb_ceiling", "{body}");
        assert_eq!(
            body["error"]["details"]["reason"], "bulk_review_is_human_only",
            "{body}"
        );
    }
    // It still reviews one item per serve.
    approve_single(r, &w.agent_dreamer, &a, "crew").await;
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn an_item_the_reviewer_authored_or_derives_from_is_skipped_not_approved() {
    let Some(w) = world().await else {
        return;
    };
    let r = &w.rig.router;
    // A writer key under the reviewer's own principal.
    let twin = value_of(
        &issue(
            r,
            &w.admin,
            "eugene-loader",
            "writer",
            "eugene-review",
            "agent",
            json!([
                { "workarea": w.node, "role": "writer" },
                { "workarea": w.node, "role": "reader" },
            ]),
        )
        .await,
    );
    // Two promoted rows first, so the crew's reviewer is served each.
    let base = write(r, &w.agent, w.node, "adr-0003-d1", &[]).await;
    approve_single(r, &w.agent_dreamer, &base, "base").await;
    let twin_base = write(r, &twin, w.node, "adr-0004-d1", &[]).await;
    approve_single(r, &w.agent_dreamer, &twin_base, "twin-base").await;
    // Held: a row by the reviewer's principal, the reviewer key's own
    // supersession, a row deriving from the principal's promoted row.
    let by_principal = write(r, &twin, w.node, "adr-0002-d1", &[]).await;
    let (status, proposed) = send(
        r,
        "POST",
        &format!("/v1/memories/{base}/supersede"),
        Some(&w.reviewer),
        Some("reviewer-supersedes"),
        Some(json!({ "expected_version": 1,"body": "Decision adr-0003-d1, as the reviewer revised it."})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{proposed}");
    let by_key = proposed["artifact_id"].as_str().expect("id").to_owned();
    let by_lineage = write(r, &w.agent, w.node, "adr-0004-d2", &[&twin_base]).await;
    let clean = write(r, &w.agent, w.node, "adr-0005-d1", &[]).await;

    // The listing shows only the clean row.
    let (status, page) = held_listing(r, &w.reviewer, "").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(listed_ids(&page), vec![clean.clone()]);

    let ids = vec![
        by_principal.clone(),
        by_key.clone(),
        by_lineage.clone(),
        clean.clone(),
    ];
    let (status, answer) = bulk(r, &w.reviewer, "r2", "approve", &ids, None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    for skipped in [&by_principal, &by_key, &by_lineage] {
        let item = outcome_of(&answer, skipped);
        assert_eq!(item["outcome"], "skipped", "{item}");
        assert_eq!(item["code"], "review.author_in_lineage", "{item}");
        // Nothing promoted it: no ratification names it.
        let journal = journal_of(&w.rig, skipped).await;
        assert!(
            journal
                .iter()
                .all(|(t, ..)| !t.starts_with("lifecycle.") && !t.starts_with("review.")),
            "{skipped}: {journal:?}"
        );
    }
    assert_eq!(outcome_of(&answer, &clean)["outcome"], "approved");
    assert_eq!(answer["summary"]["approved"], 1, "{answer}");
    assert_eq!(answer["summary"]["skipped"], 3, "{answer}");
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn a_stale_id_is_reported_not_approved() {
    let Some(w) = world().await else {
        return;
    };
    let r = &w.rig.router;
    let decided = write(r, &w.agent, w.node, "adr-0001-d1", &[]).await;
    approve_single(r, &w.agent_dreamer, &decided, "decided").await;
    // Two rival successors of one promoted tip; the first approved takes
    // the tip and the second has lost its target.
    let mut successors = Vec::new();
    // A writer supersedes its own claims and a dreamer proposes for any.
    for (token, tag, expected) in [(&w.agent, "one", 1), (&w.agent_dreamer, "two", 1)] {
        let (status, s) = send(
            r,
            "POST",
            &format!("/v1/memories/{decided}/supersede"),
            Some(token),
            Some(&format!("succ-{tag}")),
            Some(json!({ "expected_version": expected,"body": format!("Decision adr-0001-d1, revision {tag}.")})),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{s}");
        successors.push(s["artifact_id"].as_str().expect("id").to_owned());
    }
    let fresh = write(r, &w.agent, w.node, "adr-0002-d1", &[]).await;
    let unknown = OsIds.new_id().to_string();
    let ids = vec![
        decided.clone(),
        successors[0].clone(),
        successors[1].clone(),
        unknown.clone(),
        fresh.clone(),
    ];
    let (status, answer) = bulk(r, &w.reviewer, "stale", "approve", &ids, None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let item = outcome_of(&answer, &decided);
    assert_eq!(item["outcome"], "skipped", "{item}");
    assert_eq!(item["code"], "review.serve_invalid", "{item}");
    assert_eq!(outcome_of(&answer, &successors[0])["outcome"], "approved");
    let item = outcome_of(&answer, &successors[1]);
    assert_eq!(item["outcome"], "skipped", "{item}");
    assert_eq!(item["code"], "submission.tip_conflict", "{item}");
    let item = outcome_of(&answer, &unknown);
    assert_eq!(item["outcome"], "skipped", "{item}");
    assert_eq!(item["code"], "access.not_found", "{item}");
    assert_eq!(outcome_of(&answer, &fresh)["outcome"], "approved");
    // The decided row's journal holds the one approval it had.
    let approvals = journal_of(&w.rig, &decided)
        .await
        .into_iter()
        .filter(|(t, ..)| t == "review.approve")
        .count();
    assert_eq!(approvals, 1);
    drop_db(&w.rig.db_name).await;
}

/// The journal of one approval with the fields that name the item blanked:
/// what must be equal between a single approve and a bulk one.
fn approval_shape(
    events: &[(String, String, Option<Uuid>, Value)],
) -> Vec<(String, String, Option<Uuid>, Value)> {
    events
        .iter()
        .filter(|(t, ..)| t == "review.approve" || t.starts_with("lifecycle."))
        .map(|(t, principal, key, payload)| {
            let mut payload = payload.clone();
            for field in [
                "artifact_id",
                "claim_key",
                "ratification_event_id",
                "served_body_digest",
            ] {
                if let Some(v) = payload.get_mut(field) {
                    *v = json!("<item>");
                }
            }
            (t.clone(), principal.clone(), *key, payload)
        })
        .collect()
}

#[tokio::test]
async fn every_bulk_approved_item_has_its_own_verdict_and_ratification_equal_to_a_single_approve() {
    let Some(w) = world().await else {
        return;
    };
    let r = &w.rig.router;
    let reviewer_id = Uuid::parse_str(&id_of(&w.reviewer_issued)).expect("uuid");
    let single = write(r, &w.agent, w.node, "adr-0001-d1", &[]).await;
    approve_single(r, &w.reviewer, &single, "single").await;
    let a = write(r, &w.agent, w.node, "adr-0002-d1", &[]).await;
    let b = write(r, &w.agent, w.node, "adr-0002-d2", &[]).await;
    let (status, answer) = bulk(
        r,
        &w.reviewer,
        "equal",
        "approve",
        &[a.clone(), b.clone()],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let expected = approval_shape(&journal_of(&w.rig, &single).await);
    assert_eq!(
        expected.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(),
        ["review.approve", "lifecycle.promote"]
    );
    assert_eq!(
        expected[0].2,
        Some(reviewer_id),
        "the verdict is the reviewer key's"
    );
    assert_eq!(expected[1].3["same_user"], false, "{:?}", expected[1]);
    let mut seqs = Vec::new();
    for id in [&a, &b] {
        let journal = journal_of(&w.rig, id).await;
        assert_eq!(approval_shape(&journal), expected, "{id}");
        let item = outcome_of(&answer, id);
        assert_eq!(item["outcome"], "approved", "{item}");
        assert_eq!(item["state"], "promoted", "{item}");
        assert_eq!(item["same_user"], false, "{item}");
        seqs.push(item["journal_seq"].as_u64().expect("seq"));
    }
    assert_ne!(seqs[0], seqs[1], "each item commits its own events");
    // Each event is signed and chained on its own: the chain verifies.
    let mut conn = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("owner connect");
    let unsigned: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM journal WHERE octet_length(sig) <> 64 \
         OR (event_type IN ('review.approve', 'lifecycle.promote') \
             AND payload->>'artifact_id' IS NULL)",
    )
    .fetch_one(&mut conn)
    .await
    .expect("count");
    assert_eq!(unsigned, 0);
    let per_item: Vec<(String, i64)> = sqlx::query_as(
        "SELECT payload->>'artifact_id', count(*) FROM journal \
         WHERE event_type = 'review.approve' GROUP BY 1 ORDER BY 1",
    )
    .fetch_all(&mut conn)
    .await
    .expect("per item");
    assert!(per_item.iter().all(|(_, n)| *n == 1), "{per_item:?}");
    assert_eq!(per_item.len(), 3);
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn a_bulk_retry_replays_every_decided_item() {
    let Some(w) = world().await else {
        return;
    };
    let r = &w.rig.router;
    let a = write(r, &w.agent, w.node, "adr-0001-d1", &[]).await;
    let b = write(r, &w.agent, w.node, "adr-0001-d2", &[]).await;
    let ids = vec![a.clone(), b.clone()];
    let (status, first) = bulk(r, &w.reviewer, "retry", "approve", &ids, None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let events_before = journal_of(&w.rig, &a).await.len() + journal_of(&w.rig, &b).await.len();
    let (status, again) = bulk(r, &w.reviewer, "retry", "approve", &ids, None).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    for id in &ids {
        let (one, two) = (outcome_of(&first, id), outcome_of(&again, id));
        assert_eq!(one["replayed"], false, "{one}");
        assert_eq!(two["replayed"], true, "{two}");
        assert_eq!(two["outcome"], "approved", "{two}");
        assert_eq!(one["journal_seq"], two["journal_seq"]);
    }
    assert_eq!(
        journal_of(&w.rig, &a).await.len() + journal_of(&w.rig, &b).await.len(),
        events_before,
        "a replay journals nothing"
    );
    // The same key with another verdict is a mismatch per item.
    let (status, other) = bulk(r, &w.reviewer, "retry", "reject", &ids, Some("wrong")).await;
    assert_eq!(status, StatusCode::OK, "{other}");
    for id in &ids {
        assert_eq!(
            outcome_of(&other, id)["code"],
            "idempotency.digest_mismatch",
            "{other}"
        );
    }
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn the_listing_serves_only_what_review_next_would_and_claims_nothing() {
    let Some(w) = world().await else {
        return;
    };
    let r = &w.rig.router;
    let other = issue(
        r,
        &w.admin,
        "notes-writer",
        "writer",
        "crew/notes",
        "agent",
        json!([
            { "workarea": w.node, "role": "writer" },
            { "workarea": w.node, "role": "reader" },
        ]),
    )
    .await;
    let a = write(r, &w.agent, w.node, "adr-0001-d1", &[]).await;
    let b = write(r, &w.agent, w.node, "adr-0001-summary", &[]).await;
    let note = write(r, &value_of(&other), w.node, "note-1", &[]).await;
    let c = write(r, &w.agent, w.node, "adr-0002-d1", &[]).await;

    let (status, page) = held_listing(r, &w.reviewer, "").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(
        listed_ids(&page),
        vec![a.clone(), b.clone(), note.clone(), c.clone()]
    );
    let first = &page["items"][0];
    assert_eq!(
        first["text"], "Decision adr-0001-d1: the record states it.",
        "{first}"
    );
    assert_eq!(first["claim_key"], "adr-0001-d1");
    assert!(first["held_since"].is_string(), "{first}");
    assert_eq!(first["same_user"], false);

    let (_, page) = held_listing(r, &w.reviewer, "?claim_key_prefix=adr-0001-").await;
    assert_eq!(listed_ids(&page), vec![a.clone(), b.clone()]);
    let (_, page) = held_listing(r, &w.reviewer, "?author_key=notes-writer").await;
    assert_eq!(listed_ids(&page), vec![note.clone()]);
    let (_, page) = held_listing(
        r,
        &w.reviewer,
        &format!("?author_key={}", id_of(&w.agent_issued)),
    )
    .await;
    assert_eq!(listed_ids(&page), vec![a.clone(), b.clone(), c.clone()]);
    let (_, page) = held_listing(r, &w.reviewer, "?workarea=adr-import&limit=2").await;
    assert_eq!(listed_ids(&page), vec![a.clone(), b.clone()]);
    let cursor = page["next_cursor"]
        .as_str()
        .expect("a next page")
        .to_owned();
    let (_, page) = held_listing(r, &w.reviewer, &format!("?limit=2&cursor={cursor}")).await;
    assert_eq!(listed_ids(&page), vec![note.clone(), c.clone()]);
    assert!(page["next_cursor"].is_null(), "{page}");

    // Listing claimed nothing: another reviewer is served the oldest row.
    let (status, serve) = send(
        r,
        "POST",
        "/v2/review/next",
        Some(&w.agent_dreamer),
        Some("crew-next"),
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{serve}");
    assert_eq!(serve["item"]["artifact_id"], a.as_str());
    // ...and a row another key holds is no longer listed.
    let (_, page) = held_listing(r, &w.reviewer, "").await;
    assert_eq!(listed_ids(&page), vec![b, note, c]);
    for bad in ["?limit=0", "?limit=201", "?colour=red"] {
        let (status, body) = held_listing(r, &w.reviewer, bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
        assert_eq!(error_code(&body), "request.validation", "{bad}: {body}");
    }
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn a_bulk_request_over_the_cap_or_with_a_repeated_id_is_refused() {
    let Some(w) = world().await else {
        return;
    };
    let r = &w.rig.router;
    let a = write(r, &w.agent, w.node, "adr-0001-d1", &[]).await;
    let many: Vec<String> = (0..201).map(|_| OsIds.new_id().to_string()).collect();
    for (tag, ids) in [
        ("empty", Vec::new()),
        ("over", many),
        ("repeat", vec![a.clone(), a.clone()]),
        ("not-an-id", vec!["adr-0001-d1".to_owned()]),
    ] {
        let (status, body) = bulk(r, &w.reviewer, tag, "approve", &ids, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{tag}: {body}");
        assert_eq!(error_code(&body), "request.validation", "{tag}: {body}");
        assert_eq!(
            body["error"]["details"]["field"], "/artifact_ids",
            "{tag}: {body}"
        );
    }
    let (status, body) = bulk(
        r,
        &w.reviewer,
        "no-reason",
        "reject",
        std::slice::from_ref(&a),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(error_code(&body), "request.validation", "{body}");
    // Nothing was judged.
    let (_, page) = held_listing(r, &w.reviewer, "").await;
    assert_eq!(listed_ids(&page), vec![a]);
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn a_bulk_reject_journals_each_item_with_the_one_reason() {
    let Some(w) = world().await else {
        return;
    };
    let r = &w.rig.router;
    let a = write(r, &w.agent, w.node, "adr-0001-d1", &[]).await;
    let b = write(r, &w.agent, w.node, "adr-0001-d2", &[]).await;
    let reason = "loaded from the wrong ADR revision";
    let (status, answer) = bulk(
        r,
        &w.reviewer,
        "reject",
        "reject",
        &[a.clone(), b.clone()],
        Some(reason),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["summary"]["rejected"], 2, "{answer}");
    for id in [&a, &b] {
        assert_eq!(outcome_of(&answer, id)["state"], "rejected");
        let rejects: Vec<Value> = journal_of(&w.rig, id)
            .await
            .into_iter()
            .filter(|(t, ..)| t == "review.reject")
            .map(|(.., p)| p)
            .collect();
        assert_eq!(rejects.len(), 1, "{id}");
        assert_eq!(rejects[0]["reason"], reason);
    }
    drop_db(&w.rig.db_name).await;
}

/// The operator's case: their own agent writes into their user root, and
/// their own reviewer key approves in bulk. Each ratification is marked
/// `same_user`, single and bulk alike, and the answer says so (ADR-0055 d7).
#[tokio::test]
async fn a_bulk_approval_of_the_persons_own_agents_writes_is_marked_same_user() {
    let Some(rig) = fresh_rig().await else {
        return;
    };
    let r = &rig.router;
    let (admin, _) = bootstrap(r).await;
    // The issuance body's `user` is set by a second call shape: issue()
    // sends none, so issue the user keys directly.
    let user_key =
        |name: &'static str, role: &'static str, held_by: &'static str, grants: Value| {
            let admin = admin.clone();
            async move {
                let (status, body) = send(
                    r,
                    "POST",
                    "/v2/keys",
                    Some(&admin),
                    Some(&format!("issue-{name}")),
                    Some(json!({
                        "role": role, "name": name, "principal": format!("eugene/{name}"),
                        "user": "eugene", "held_by": held_by, "grants": grants,
                    })),
                )
                .await;
                assert_eq!(status, StatusCode::CREATED, "{body}");
                value_of(&body)
            }
        };
    let loader = user_key(
        "eugene-agent",
        "writer",
        "agent",
        json!([{ "workarea": "eugene", "role": "writer" }, { "workarea": "eugene", "role": "reader" }]),
    )
    .await;
    let reviewer = user_key(
        "eugene-reviewer",
        "dreamer",
        "human",
        json!([{ "workarea": "eugene", "role": "dreamer" }, { "workarea": "eugene", "role": "reader" }]),
    )
    .await;
    let mut ids = Vec::new();
    for key in ["adr-0001-d1", "adr-0001-summary"] {
        let (status, accepted) = send(
            r,
            "POST",
            "/v1/memories",
            Some(&loader),
            Some(&format!("w-{key}")),
            Some(json!({"claim_key": key, "kind": "memory", "body": format!("Decision {key}.")})),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{accepted}");
        ids.push(accepted["artifact_id"].as_str().expect("id").to_owned());
    }
    let (status, page) = held_listing(r, &reviewer, "").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page["items"]
            .as_array()
            .expect("items")
            .iter()
            .all(|i| i["same_user"] == true),
        "{page}"
    );
    let (status, answer) = bulk(r, &reviewer, "own", "approve", &ids, None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    for id in &ids {
        assert_eq!(outcome_of(&answer, id)["same_user"], true, "{answer}");
        let promote: Vec<Value> = journal_of(&rig, id)
            .await
            .into_iter()
            .filter(|(t, ..)| t == "lifecycle.promote")
            .map(|(.., p)| p)
            .collect();
        assert_eq!(promote.len(), 1);
        assert_eq!(promote[0]["same_user"], true, "{:?}", promote[0]);
        assert_eq!(promote[0]["self_signed"], false, "the mark relaxes nothing");
    }
    drop_db(&rig.db_name).await;
}

// ---------------------------------------------------------------------------
// The scale measurement (ADR-0055 d9): ignored by default
// ---------------------------------------------------------------------------

/// Loads `OML_REVIEW_SCALE_HELD` held rows (45,000 by default) through the
/// write route, then times `review next` with a single approve after it. With `OML_REVIEW_SCALE_BULK`
/// set it also times approving that many items through the bulk route, 200
/// a request. Run with `--ignored --nocapture`.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "a measurement: tens of thousands of writes"]
async fn review_scale_measurement() {
    let held: usize = std::env::var("OML_REVIEW_SCALE_HELD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(45_000);
    let bulk_n: usize = std::env::var("OML_REVIEW_SCALE_BULK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let Some(w) = world().await else {
        return;
    };
    let started = Instant::now();
    let writers = 8;
    let mut tasks = Vec::new();
    for t in 0..writers {
        let router = w.rig.router.clone();
        let agent = w.agent.clone();
        let node = w.node;
        tasks.push(tokio::spawn(async move {
            let mut i = t;
            while i < held {
                let key = format!("adr-{:04}-d{}", i / 10, i % 10);
                let mut tries = 0;
                loop {
                    let (status, body) = send(
                        &router,
                        "POST",
                        "/v1/memories",
                        Some(&agent),
                        Some(&format!("w-{key}")),
                        Some(json!({
                            "claim_key": key, "kind": "memory",
                            "body": format!("Decision {key}: the record states it."),
                            "workarea": node.to_string(),
                        })),
                    )
                    .await;
                    if status == StatusCode::ACCEPTED {
                        break;
                    }
                    tries += 1;
                    assert!(tries < 50, "write {key}: {status} {body}");
                }
                i += writers;
            }
        }));
    }
    for task in tasks {
        task.await.expect("writer");
    }
    println!(
        "loaded {held} held rows in {:.1} s",
        started.elapsed().as_secs_f64()
    );

    let mut conn = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("owner connect");
    conn.execute("ANALYZE").await.expect("analyze");
    let cycles = 50;
    let mut next_ms = Vec::with_capacity(cycles);
    let started = Instant::now();
    for i in 0..cycles {
        let t = Instant::now();
        let (status, serve) = send(
            &w.rig.router,
            "POST",
            "/v2/review/next",
            Some(&w.reviewer),
            Some(&format!("n-{i}")),
            Some(json!({})),
        )
        .await;
        next_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(status, StatusCode::OK, "{serve}");
        let (status, verdict) = send(
            &w.rig.router,
            "POST",
            "/v2/review/verdicts",
            Some(&w.reviewer),
            Some(&format!("v-{i}")),
            Some(json!({"serve_id": serve["serve_id"], "verdict": "approve"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{verdict}");
    }
    let one_by_one = started.elapsed().as_secs_f64();
    next_ms.sort_by(f64::total_cmp);
    println!(
        "review next at {held} held: p50 {:.1} ms, p95 {:.1} ms; next+approve {:.1} rows/s over {cycles}",
        next_ms[cycles / 2],
        next_ms[cycles * 95 / 100],
        cycles as f64 / one_by_one
    );

    if bulk_n > 0 {
        let started = Instant::now();
        let mut listed = Vec::new();
        let mut cursor: Option<String> = None;
        while listed.len() < bulk_n {
            let mut query = format!("?limit={}", 200.min(bulk_n - listed.len()));
            if let Some(c) = &cursor {
                query.push_str(&format!("&cursor={c}"));
            }
            let (status, page) = held_listing(&w.rig.router, &w.reviewer, &query).await;
            assert_eq!(status, StatusCode::OK, "{page}");
            for item in page["items"].as_array().expect("items") {
                listed.push(item["artifact_id"].as_str().expect("id").to_owned());
            }
            match page["next_cursor"].as_str() {
                Some(c) => cursor = Some(c.to_owned()),
                None => break,
            }
        }
        let listing = started.elapsed().as_secs_f64();
        let started = Instant::now();
        let mut approved = 0;
        for (n, chunk) in listed.chunks(200).enumerate() {
            let (status, answer) = bulk(
                &w.rig.router,
                &w.reviewer,
                &format!("bulk-{n}"),
                "approve",
                chunk,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{answer}");
            approved += answer["summary"]["approved"].as_u64().expect("approved");
        }
        let took = started.elapsed().as_secs_f64();
        println!(
            "bulk at {held} held: listed {} in {listing:.2} s; approved {approved} in {took:.2} s ({:.0} rows/s)",
            listed.len(),
            approved as f64 / took
        );
    }
    drop(conn);
    w.rig.store.pool().close().await;
    drop_db(&w.rig.db_name).await;
}
