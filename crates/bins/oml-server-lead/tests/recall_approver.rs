//! A served memory says when it was approved and by what kind of reviewer,
//! and never names the reviewer (#175; ADR-0077), over the real lead and a
//! migrated Postgres store under its role pair (ADR-0034 d8): the same
//! `approver` and `approved_at` on recall and get, over HTTP and MCP; a
//! crew's reviewer, a person and a person approving their own agent's
//! write each read as what they are; an unreviewed hit and an anonymous
//! `universe` hit carry neither; the reviewing key differs from the author
//! on every hit that was reviewed; and the rows migration 0044 fills from
//! the journal equal the rows the promotion's fold writes. Each test is
//! named after the sentence it proves.

use std::sync::atomic::Ordering;

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

/// Migration 0044 as shipped: the fill the last test runs again.
const MIGRATION_0044: &str = include_str!("../../../../migrations/0044_memory_approval.sql");

// ---------------------------------------------------------------------------
// The rig (the bulk_review.rs pattern)
// ---------------------------------------------------------------------------

/// `db` through `DATABASE_URL`'s own login, the minting superuser, which
/// reads the journal whatever the tenant.
fn super_url(db: &str) -> String {
    let base = base_url();
    let (head, _) = base
        .rsplit_once('/')
        .expect("DATABASE_URL names a database path");
    format!("{head}/{db}")
}

struct Rig {
    router: Router,
    db_name: String,
}

async fn fresh_rig() -> Option<Rig> {
    let db_name = format!(
        "oml_test_approver_{}_{}_{}",
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
    Some(Rig { router, db_name })
}

async fn send(
    router: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let body = match body {
        None => Body::empty(),
        Some(v) => {
            builder = builder
                .header("content-type", "application/json")
                .header("idempotency-key", OsIds.new_id().to_string());
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

/// A request that must answer `expected`; its body.
async fn ok(
    router: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
    expected: StatusCode,
) -> Value {
    let (status, answer) = send(router, method, path, Some(token), body).await;
    assert_eq!(status, expected, "{method} {path}: {answer}");
    answer
}

/// A key as issued: its show-once value, its id, its name and principal.
struct Key {
    value: String,
    id: String,
    name: String,
    principal: String,
}

/// Issues a key named `name` with `more` merged into the issuance body.
async fn issue(router: &Router, admin: &str, name: &str, role: &str, more: Value) -> Key {
    let principal = format!("people/{name}");
    let mut body = json!({ "role": role, "name": name, "principal": principal });
    if let (Some(body), Some(more)) = (body.as_object_mut(), more.as_object()) {
        body.extend(more.clone());
    }
    let issued = ok(
        router,
        "POST",
        "/v2/keys",
        admin,
        Some(body),
        StatusCode::CREATED,
    )
    .await;
    Key {
        value: issued["value"]
            .as_str()
            .expect("show-once value")
            .to_owned(),
        id: issued["key_id"].as_str().expect("key id").to_owned(),
        name: name.to_owned(),
        principal,
    }
}

fn grants(workarea: &str, roles: &[&str]) -> Value {
    Value::Array(
        roles
            .iter()
            .map(|role| json!({ "workarea": workarea, "role": role }))
            .collect(),
    )
}

/// Writes one memory as `writer` into `workarea`; it is held.
async fn remember(
    router: &Router,
    writer: &Key,
    workarea: &str,
    claim_key: &str,
    text: &str,
) -> String {
    let accepted = ok(
        router,
        "POST",
        "/v1/memories",
        &writer.value,
        Some(json!({
            "verb": "remember", "claim_key": claim_key, "kind": "memory", "body": text,
            "workarea": workarea,
        })),
        StatusCode::ACCEPTED,
    )
    .await;
    accepted["artifact_id"].as_str().expect("id").to_owned()
}

/// `reviewer` is served its next held item, which is `id`, and approves it.
async fn approve(router: &Router, reviewer: &Key, id: &str) {
    let serve = ok(
        router,
        "POST",
        "/v2/review/next",
        &reviewer.value,
        Some(json!({})),
        StatusCode::OK,
    )
    .await;
    assert_eq!(serve["item"]["artifact_id"], id, "{serve}");
    ok(
        router,
        "POST",
        "/v2/review/verdicts",
        &reviewer.value,
        Some(json!({ "serve_id": serve["serve_id"], "verdict": "approve" })),
        StatusCode::OK,
    )
    .await;
}

async fn recall(router: &Router, key: &Key, body: Value) -> Value {
    ok(
        router,
        "POST",
        "/v1/memories/recall",
        &key.value,
        Some(body),
        StatusCode::OK,
    )
    .await
}

async fn get(router: &Router, key: &Key, id: &str) -> Value {
    ok(
        router,
        "GET",
        &format!("/v1/memories/{id}"),
        &key.value,
        None,
        StatusCode::OK,
    )
    .await
}

/// `oml_read` over `/mcp` with `arguments`: its structured content.
async fn mcp_read(router: &Router, key: &Key, arguments: Value) -> Value {
    let rpc = ok(
        router,
        "POST",
        "/mcp",
        &key.value,
        Some(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "oml_read", "arguments": arguments },
        })),
        StatusCode::OK,
    )
    .await;
    assert_eq!(rpc["result"]["isError"], false, "{rpc}");
    rpc["result"]["structuredContent"].clone()
}

/// The hit of `answer` whose artifact is `id`: its memory.
fn hit<'a>(answer: &'a Value, id: &str) -> &'a Value {
    answer["hits"]
        .as_array()
        .expect("hits")
        .iter()
        .map(|h| &h["memory"])
        .find(|m| m["artifact_id"] == id)
        .unwrap_or_else(|| panic!("{id} is a hit of {answer}"))
}

/// What the journal and the row hold of one version's review, read by the
/// minting superuser: the approving verdict's key, principal and time, and
/// the authoring key and principal.
struct Reviewed {
    reviewer_key: Uuid,
    reviewer_principal: String,
    verdict_at: String,
    author_key: Uuid,
    author_principal: String,
}

async fn reviewed(db: &str, artifact_id: &str) -> Reviewed {
    let mut conn = PgConnection::connect(&super_url(db))
        .await
        .expect("superuser connect");
    let (reviewer_key, reviewer_principal, verdict_at): (Uuid, String, String) = sqlx::query_as(
        "SELECT api_key_id, principal, \
                to_char(at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
           FROM journal WHERE event_type = 'review.approve' AND payload->>'artifact_id' = $1",
    )
    .bind(artifact_id)
    .fetch_one(&mut conn)
    .await
    .expect("the version's one approving verdict");
    let (author_key, author_principal): (Uuid, String) =
        sqlx::query_as("SELECT author_key_id, author_principal FROM memories WHERE id = $1::uuid")
            .bind(artifact_id)
            .fetch_one(&mut conn)
            .await
            .expect("the version's row");
    Reviewed {
        reviewer_key,
        reviewer_principal,
        verdict_at,
        author_key,
        author_principal,
    }
}

// ---------------------------------------------------------------------------
// The world: a crew workarea with a crew's reviewer and a person, a user
// root with a person's own agent and reviewer, `universe`, and a workarea
// in progress
// ---------------------------------------------------------------------------

struct World {
    rig: Rig,
    /// The tenant's admin key's value.
    admin: String,
    ann: Key,
    reader: Key,
    eve_agent: Key,
    reviewers: Vec<Key>,
    crew: String,
    proj: String,
    by_crew: String,
    by_person: String,
    by_self: String,
    in_universe: String,
    unreviewed: String,
}

impl World {
    async fn boot() -> Option<Self> {
        let rig = fresh_rig().await?;
        let r = &rig.router;
        let (status, claimed) = send(
            r,
            "POST",
            "/v2/bootstrap/claim",
            None,
            Some(json!({
                "enrollment_token": ENROLLMENT, "name": "root-admin",
                "tenant_name": "approver", "admin_principal": "platform/admin",
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{claimed}");
        let admin = claimed["value"].as_str().expect("admin key").to_owned();
        let root = claimed["root_workarea_id"]
            .as_str()
            .expect("root")
            .to_owned();
        let workarea = |parent: String, name: &'static str| {
            let admin = admin.clone();
            async move {
                ok(
                    r,
                    "POST",
                    "/v2/workareas",
                    &admin,
                    Some(json!({ "parent": parent, "name": name })),
                    StatusCode::CREATED,
                )
                .await["id"]
                    .as_str()
                    .expect("workarea id")
                    .to_owned()
            }
        };
        let crew = workarea(root.clone(), "crew").await;
        let proj = workarea(crew.clone(), "proj").await;

        let ann = issue(
            r,
            &admin,
            "ann-writer",
            "writer",
            json!({ "held_by": "agent", "default_workarea": crew,
                    "grants": grants(&root, &["writer", "reader"]) }),
        )
        .await;
        let reader = issue(
            r,
            &admin,
            "ray-reader",
            "reader",
            json!({ "held_by": "agent", "default_workarea": crew,
                    "grants": grants(&root, &["reader"]) }),
        )
        .await;
        // A crew's reviewer: a reviewer key an agent holds.
        let bot = issue(
            r,
            &admin,
            "zed-crew-reviewer",
            "dreamer",
            json!({ "held_by": "agent", "grants": grants(&crew, &["dreamer", "reader"]) }),
        )
        .await;
        // A person, who also reviews `universe` and puts `proj` in progress.
        let pat = issue(
            r,
            &admin,
            "pat-person-reviewer",
            "dreamer",
            json!({ "held_by": "human", "grants": grants(&root, &["dreamer", "reader"]) }),
        )
        .await;
        // One person's agent and reviewer keys, in that person's own root.
        let eve_agent = issue(
            r,
            &admin,
            "eve-agent",
            "writer",
            json!({ "held_by": "agent", "user": "eve",
                    "grants": grants("eve", &["writer", "reader"]) }),
        )
        .await;
        let eve_review = issue(
            r,
            &admin,
            "eve-own-reviewer",
            "dreamer",
            json!({ "held_by": "human", "user": "eve",
                    "grants": grants("eve", &["dreamer", "reader"]) }),
        )
        .await;

        // Each write is approved before the next, so each reviewer is
        // served the row meant for it.
        let by_crew = remember(
            r,
            &ann,
            &crew,
            "by-crew",
            "The gantry crane lifts ten tonnes.",
        )
        .await;
        approve(r, &bot, &by_crew).await;
        let by_person = remember(
            r,
            &ann,
            &crew,
            "by-person",
            "The gantry crane is inspected in May.",
        )
        .await;
        approve(r, &pat, &by_person).await;
        let by_self = remember(
            r,
            &eve_agent,
            "eve",
            "by-self",
            "The gantry crane notes live in my drawer.",
        )
        .await;
        approve(r, &eve_review, &by_self).await;
        let in_universe = remember(
            r,
            &ann,
            &root,
            "in-universe",
            "Every gantry crane carries its load plate.",
        )
        .await;
        approve(r, &pat, &in_universe).await;
        ok(
            r,
            "POST",
            &format!("/v2/workareas/{proj}/mode"),
            &pat.value,
            Some(json!({ "mode": "in_progress" })),
            StatusCode::OK,
        )
        .await;
        let unreviewed = remember(
            r,
            &ann,
            &proj,
            "unreviewed",
            "The gantry crane rail is being re-laid.",
        )
        .await;
        Some(Self {
            rig,
            admin,
            ann,
            reader,
            eve_agent,
            reviewers: vec![bot, pat, eve_review],
            crew,
            proj,
            by_crew,
            by_person,
            by_self,
            in_universe,
            unreviewed,
        })
    }

    /// Every recall the laws read: the crew scope, the workarea in
    /// progress, the person's own root (which also searches `universe`),
    /// the author's own held rows, and the author's recall from
    /// `universe` down, each with the key that sent it.
    async fn recalls(&self) -> Vec<(&Key, Value)> {
        let r = &self.rig.router;
        let query = "gantry crane";
        vec![
            (
                &self.reader,
                recall(
                    r,
                    &self.reader,
                    json!({ "query": query, "workarea": self.crew }),
                )
                .await,
            ),
            (
                &self.reader,
                recall(
                    r,
                    &self.reader,
                    json!({ "query": query, "workarea": self.proj }),
                )
                .await,
            ),
            (
                &self.eve_agent,
                recall(r, &self.eve_agent, json!({ "query": query })).await,
            ),
            (
                &self.ann,
                recall(
                    r,
                    &self.ann,
                    json!({ "query": query, "workarea": self.proj, "include_unreviewed": true }),
                )
                .await,
            ),
            (
                &self.ann,
                recall(
                    r,
                    &self.ann,
                    json!({ "query": query, "workarea": "universe" }),
                )
                .await,
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// The laws
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_hit_says_when_it_was_approved_and_whether_a_crew_a_person_or_the_authors_own_person_approved_it()
 {
    let Some(w) = World::boot().await else {
        return;
    };
    let r = &w.rig.router;
    let answer = recall(
        r,
        &w.reader,
        json!({ "query": "gantry crane", "workarea": w.crew }),
    )
    .await;

    // A crew's reviewer and a person, each as what it is.
    assert_eq!(
        hit(&answer, &w.by_crew)["approver"],
        json!({ "held_by": "agent", "same_user": false }),
        "{answer}"
    );
    assert_eq!(
        hit(&answer, &w.by_person)["approver"],
        json!({ "held_by": "human", "same_user": false }),
        "{answer}"
    );
    // A person approving their own agent's write is a self-review.
    let own = recall(r, &w.eve_agent, json!({ "query": "gantry crane" })).await;
    assert_eq!(
        hit(&own, &w.by_self)["approver"],
        json!({ "held_by": "human", "same_user": true }),
        "{own}"
    );

    // The time is the approving verdict's own, at or before the promotion.
    for id in [&w.by_crew, &w.by_person] {
        let memory = hit(&answer, id);
        let verdict = reviewed(&w.rig.db_name, id).await;
        assert_eq!(
            memory["approved_at"],
            verdict.verdict_at.as_str(),
            "{memory}"
        );
        assert!(
            memory["approved_at"].as_str() <= memory["promoted_at"].as_str(),
            "the verdict precedes the promotion it ratifies: {memory}"
        );
    }
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn get_and_recall_serve_the_same_approval_over_http_and_over_mcp() {
    let Some(w) = World::boot().await else {
        return;
    };
    let r = &w.rig.router;
    let answer = recall(
        r,
        &w.reader,
        json!({ "query": "gantry crane", "workarea": w.crew }),
    )
    .await;
    let over_mcp = mcp_read(
        r,
        &w.reader,
        json!({ "query": "gantry crane", "workarea": w.crew }),
    )
    .await;
    for id in [&w.by_crew, &w.by_person] {
        let recalled = hit(&answer, id);
        assert!(recalled["approver"].is_object(), "{recalled}");
        let fetched = get(r, &w.reader, id).await;
        let mcp_hit = over_mcp["hits"]
            .as_array()
            .expect("hits")
            .iter()
            .find(|h| h["artifact_id"] == id.as_str())
            .unwrap_or_else(|| panic!("{id} is an MCP hit of {over_mcp}"));
        let mcp_got = mcp_read(r, &w.reader, json!({ "artifact_id": id })).await;
        let mcp_item = &mcp_got["items"][0];
        for field in ["approver", "approved_at"] {
            assert_eq!(
                fetched[field], recalled[field],
                "get and recall agree on {field}"
            );
            assert_eq!(
                mcp_hit[field], recalled[field],
                "MCP recall agrees on {field}"
            );
            assert_eq!(
                mcp_item[field], recalled[field],
                "MCP get agrees on {field}"
            );
        }
    }
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn the_reviewer_differs_from_the_author_on_every_hit_that_is_not_served_unreviewed() {
    let Some(w) = World::boot().await else {
        return;
    };
    let (mut checked, mut unreviewed, mut anonymous) = (0, 0, 0);
    for (key, answer) in w.recalls().await {
        for h in answer["hits"].as_array().expect("hits") {
            let memory = &h["memory"];
            let id = memory["artifact_id"].as_str().expect("id");
            if memory["unreviewed"] == true {
                // Served before any review: nobody approved it.
                assert_eq!(memory["approver"], Value::Null, "{memory}");
                assert_eq!(memory["approved_at"], Value::Null, "{memory}");
                unreviewed += 1;
                continue;
            }
            assert_eq!(memory["state"], "promoted", "{memory}");
            // Whatever the reader is shown, the journal's verdict on a
            // reviewed hit is another key's and another principal's.
            let verdict = reviewed(&w.rig.db_name, id).await;
            assert_ne!(verdict.reviewer_key, verdict.author_key, "{memory}");
            assert_ne!(
                verdict.reviewer_principal, verdict.author_principal,
                "{memory}"
            );
            if memory["principal"].is_null() {
                // An anonymous `universe` row keeps ADR-0044 d9's list.
                assert_eq!(memory["approver"], Value::Null, "{memory}");
                assert_eq!(memory["approved_at"], Value::Null, "{memory}");
                anonymous += 1;
                continue;
            }
            assert!(
                memory["approver"].is_object(),
                "served to {}: {memory}",
                key.name
            );
            assert_eq!(
                memory["approved_at"],
                verdict.verdict_at.as_str(),
                "{memory}"
            );
            checked += 1;
        }
    }
    // Each kind of hit was met, so the law is not vacuous.
    assert!(checked >= 5, "reviewed hits checked: {checked}");
    assert!(unreviewed >= 2, "unreviewed hits met: {unreviewed}");
    assert!(anonymous >= 1, "anonymous hits met: {anonymous}");
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn no_read_of_a_memory_names_its_reviewer() {
    let Some(w) = World::boot().await else {
        return;
    };
    let r = &w.rig.router;
    let mut served = Vec::new();
    for (_, answer) in w.recalls().await {
        served.push(answer);
    }
    for id in [&w.by_crew, &w.by_person, &w.in_universe] {
        served.push(get(r, &w.ann, id).await);
        served.push(mcp_read(r, &w.ann, json!({ "artifact_id": id })).await);
    }
    served.push(get(r, &w.eve_agent, &w.by_self).await);
    served.push(
        mcp_read(
            r,
            &w.reader,
            json!({ "query": "gantry crane", "workarea": w.crew }),
        )
        .await,
    );
    for answer in &served {
        let text = answer.to_string();
        for reviewer in &w.reviewers {
            for secret in [&reviewer.id, &reviewer.name, &reviewer.principal] {
                assert!(
                    !text.contains(secret.as_str()),
                    "an answer names a reviewer ({secret}): {text}"
                );
            }
        }
    }
    // The approver object holds its two members and no other.
    let answer = recall(
        r,
        &w.reader,
        json!({ "query": "gantry crane", "workarea": w.crew }),
    )
    .await;
    let approver = hit(&answer, &w.by_crew)["approver"]
        .as_object()
        .expect("an approver");
    let mut members: Vec<&str> = approver.keys().map(String::as_str).collect();
    members.sort_unstable();
    assert_eq!(members, ["held_by", "same_user"]);
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn a_universe_hit_carries_no_approval_for_any_key_but_its_authors() {
    let Some(w) = World::boot().await else {
        return;
    };
    let r = &w.rig.router;
    // A person's recall in their own root also searches `universe`.
    let own = recall(r, &w.eve_agent, json!({ "query": "gantry crane" })).await;
    let memory = hit(&own, &w.in_universe);
    assert_eq!(memory["principal"], Value::Null, "{memory}");
    assert_eq!(memory["approver"], Value::Null, "{memory}");
    assert_eq!(memory["approved_at"], Value::Null, "{memory}");
    assert!(
        memory["promoted_at"].is_string(),
        "the row keeps its promotion time: {memory}"
    );
    let fetched = get(r, &w.reader, &w.in_universe).await;
    assert_eq!(fetched["approver"], Value::Null, "{fetched}");
    assert_eq!(fetched["approved_at"], Value::Null, "{fetched}");
    // Its author is served its own row as every author is.
    let authors = get(r, &w.ann, &w.in_universe).await;
    assert_eq!(
        authors["approver"],
        json!({ "held_by": "human", "same_user": false }),
        "{authors}"
    );
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn an_unreviewed_hit_carries_no_approval_and_gains_one_when_it_is_approved() {
    let Some(w) = World::boot().await else {
        return;
    };
    let r = &w.rig.router;
    let scoped = json!({ "query": "gantry crane", "workarea": w.proj });
    let before = recall(r, &w.reader, scoped.clone()).await;
    let memory = hit(&before, &w.unreviewed);
    assert_eq!(memory["unreviewed"], true, "{memory}");
    assert_eq!(memory["approver"], Value::Null, "{memory}");
    assert_eq!(memory["approved_at"], Value::Null, "{memory}");

    approve(r, &w.reviewers[1], &w.unreviewed).await;
    let after = recall(r, &w.reader, scoped).await;
    let memory = hit(&after, &w.unreviewed);
    assert_eq!(memory["unreviewed"], false, "{memory}");
    assert_eq!(
        memory["approver"],
        json!({ "held_by": "human", "same_user": false }),
        "{memory}"
    );
    drop_db(&w.rig.db_name).await;
}

#[tokio::test]
async fn a_successor_carries_its_own_approval_and_the_version_it_replaced_keeps_its_own() {
    let Some(w) = World::boot().await else {
        return;
    };
    let r = &w.rig.router;
    // Review serves the oldest held row first, so the one waiting in
    // `proj` is judged before the successor is written.
    approve(r, &w.reviewers[1], &w.unreviewed).await;
    let successor = ok(
        r,
        "POST",
        &format!("/v1/memories/{}/supersede", w.by_crew),
        &w.ann.value,
        Some(json!({ "body": "The gantry crane lifts twelve tonnes.", "expected_version": 1 })),
        StatusCode::ACCEPTED,
    )
    .await["artifact_id"]
        .as_str()
        .expect("id")
        .to_owned();
    // The person approves what the crew's reviewer approved the first of.
    approve(r, &w.reviewers[1], &successor).await;
    let answer = recall(
        r,
        &w.reader,
        json!({ "query": "gantry crane", "workarea": w.crew }),
    )
    .await;
    assert_eq!(
        hit(&answer, &successor)["approver"],
        json!({ "held_by": "human", "same_user": false }),
        "{answer}"
    );
    let replaced = get(r, &w.reader, &w.by_crew).await;
    assert_eq!(replaced["tip"], false, "{replaced}");
    assert_eq!(
        replaced["approver"],
        json!({ "held_by": "agent", "same_user": false }),
        "{replaced}"
    );
    drop_db(&w.rig.db_name).await;
}

/// Doctor's count of promoted versions with no approval (#311): the rescan
/// read an admin key makes carries it, as a count and never a list, and a
/// key that is not an admin's is refused the read.
#[tokio::test]
async fn the_rescan_read_counts_the_promoted_versions_that_carry_no_approval() {
    let Some(w) = World::boot().await else {
        return;
    };
    let r = &w.rig.router;
    let count = |answer: &Value| answer["promoted_unapproved"].clone();
    let before = ok(
        r,
        "GET",
        "/v2/admin/rescan?limit=1",
        &w.admin,
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        count(&before),
        json!(0),
        "every promoted version holds its approval, and the held ones are not counted: {before}"
    );
    // Two promoted versions as they were before the fill, and a held one
    // that never had an approval.
    let mut owner = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("superuser connect");
    for id in [&w.by_crew, &w.by_person] {
        sqlx::query(
            "UPDATE memories SET approver_held_by = NULL, approver_same_user = NULL, \
             approved_at = NULL WHERE id = $1::uuid",
        )
        .bind(id)
        .execute(&mut owner)
        .await
        .expect("clear one approval");
    }
    owner.close().await.expect("close");
    let after = ok(
        r,
        "GET",
        "/v2/admin/rescan?limit=1",
        &w.admin,
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(count(&after), json!(2), "{after}");
    assert!(
        !after.to_string().contains(&w.by_crew),
        "a count and never a list of the versions: {after}"
    );
    // The fill matches what the journal names, so running it again heals both.
    run_the_fill(&w.rig.db_name).await;
    let healed = ok(
        r,
        "GET",
        "/v2/admin/rescan?limit=1",
        &w.admin,
        None,
        StatusCode::OK,
    )
    .await;
    assert_eq!(count(&healed), json!(0), "{healed}");
    // Only an admin reads it.
    let (status, refused) = send(
        r,
        "GET",
        "/v2/admin/rescan?limit=1",
        Some(&w.ann.value),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{refused}");
    assert!(refused.get("promoted_unapproved").is_none(), "{refused}");
    drop_db(&w.rig.db_name).await;
}

/// The remedy doctor names for `approval_unmatched` (#311): supersede the
/// version and have a reviewer approve the successor. The count is of
/// served tips, so the remedy brings it to zero; the replaced version
/// stays readable with no approval.
#[tokio::test]
async fn superseding_an_unapproved_version_and_approving_its_successor_brings_the_count_to_zero() {
    let Some(w) = World::boot().await else {
        return;
    };
    let r = &w.rig.router;
    let count = |answer: Value| answer["promoted_unapproved"].clone();
    let read = || {
        ok(
            r,
            "GET",
            "/v2/admin/rescan?limit=1",
            &w.admin,
            None,
            StatusCode::OK,
        )
    };
    // Review serves the oldest held row first, so the one waiting is
    // judged before the successor is written.
    approve(r, &w.reviewers[1], &w.unreviewed).await;
    let mut owner = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("superuser connect");
    sqlx::query(
        "UPDATE memories SET approver_held_by = NULL, approver_same_user = NULL, \
         approved_at = NULL WHERE id = $1::uuid",
    )
    .bind(&w.by_crew)
    .execute(&mut owner)
    .await
    .expect("clear one approval");
    owner.close().await.expect("close");
    assert_eq!(count(read().await), json!(1));

    let successor = ok(
        r,
        "POST",
        &format!("/v1/memories/{}/supersede", w.by_crew),
        &w.ann.value,
        Some(json!({ "body": "The gantry crane lifts twelve tonnes.", "expected_version": 1 })),
        StatusCode::ACCEPTED,
    )
    .await["artifact_id"]
        .as_str()
        .expect("id")
        .to_owned();
    assert_eq!(
        count(read().await),
        json!(1),
        "a held successor changes nothing: the version is still the served tip"
    );
    approve(r, &w.reviewers[1], &successor).await;
    assert_eq!(count(read().await), json!(0), "the remedy is done");
    let replaced = get(r, &w.reader, &w.by_crew).await;
    assert_eq!(replaced["tip"], false, "{replaced}");
    assert_eq!(replaced["approver"], Value::Null, "{replaced}");
    drop_db(&w.rig.db_name).await;
}

/// Runs migration 0044's fill, as shipped, under the owner login the
/// migration runs under.
async fn run_the_fill(db: &str) {
    let fill = MIGRATION_0044
        .split_once("DO $$")
        .map(|(_, block)| format!("DO $${block}"))
        .expect("migration 0044 carries its fill");
    let mut owner = PgConnection::connect(&owner_url(db))
        .await
        .expect("owner connect");
    let mut tx = owner.begin().await.expect("begin");
    tx.execute(fill.as_str()).await.expect("the fill runs");
    tx.commit().await.expect("commit");
    owner.close().await.expect("close");
}

/// Every version's stored approval, by id, as text.
async fn stored_approvals(db: &str) -> Vec<(String, Option<String>)> {
    let mut conn = PgConnection::connect(&super_url(db))
        .await
        .expect("superuser connect");
    sqlx::query_as(
        "SELECT id::text, \
                approver_held_by || ' ' || approver_same_user::text || ' ' || \
                approved_at::text \
           FROM memories ORDER BY id",
    )
    .fetch_all(&mut conn)
    .await
    .expect("read the approvals")
}

#[tokio::test]
async fn migration_0044_fills_from_the_journal_exactly_what_a_promotion_stores() {
    let Some(w) = World::boot().await else {
        return;
    };
    let r = &w.rig.router;
    // Review serves the oldest held row first, so the one waiting in
    // `proj` is judged before the successor is written.
    approve(r, &w.reviewers[1], &w.unreviewed).await;
    // A supersession too, so the fill reads lifecycle.supersede as well.
    let successor = ok(
        r,
        "POST",
        &format!("/v1/memories/{}/supersede", w.by_person),
        &w.ann.value,
        Some(json!({ "body": "The gantry crane is inspected in June.", "expected_version": 1 })),
        StatusCode::ACCEPTED,
    )
    .await["artifact_id"]
        .as_str()
        .expect("id")
        .to_owned();
    approve(r, &w.reviewers[0], &successor).await;
    let held = remember(
        r,
        &w.ann,
        &w.crew,
        "still-held",
        "The gantry crane hook is new.",
    )
    .await;

    let folded = stored_approvals(&w.rig.db_name).await;
    assert_eq!(
        folded
            .iter()
            .filter(|(_, approval)| approval.is_some())
            .count(),
        6,
        "six versions were promoted, each with its approval: {folded:?}"
    );
    assert!(
        folded
            .iter()
            .any(|(id, approval)| *id == held && approval.is_none()),
        "the held version holds none: {folded:?}"
    );

    // The rows as they were before the migration: no approval on any.
    let mut owner = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("superuser connect");
    owner
        .execute(
            "UPDATE memories SET approver_held_by = NULL, approver_same_user = NULL, \
             approved_at = NULL",
        )
        .await
        .expect("clear the approvals");
    owner.close().await.expect("close");
    assert!(
        stored_approvals(&w.rig.db_name)
            .await
            .iter()
            .all(|(_, approval)| approval.is_none()),
        "the control: the rows hold no approval before the fill"
    );

    run_the_fill(&w.rig.db_name).await;

    assert_eq!(
        stored_approvals(&w.rig.db_name).await,
        folded,
        "the fill writes what the fold wrote, and nothing on the held version"
    );
    drop_db(&w.rig.db_name).await;
}

/// v0.1.0's `lifecycle.promote` named its verdict `verdict_event_id` and
/// carried no `same_user`. The fill reads that name too, takes a version's
/// latest promotion, and leaves the mark null where the event has none.
#[tokio::test]
async fn migration_0044_reads_a_promotion_that_names_its_verdict_as_v0_1_0_did() {
    let Some(w) = World::boot().await else {
        return;
    };
    let persons = reviewed(&w.rig.db_name, &w.by_person).await;
    // A later promotion of the crew-approved version, in v0.1.0's shape,
    // naming the person's verdict: appended by the minting superuser, since
    // no build writes that shape now. The chain is not read again.
    let mut conn = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("superuser connect");
    sqlx::query(
        "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, \
                              at, payload, signed_bytes_hash, sig, signed_bytes) \
         SELECT a.tenant_id, (SELECT max(seq) + 1 FROM journal), gen_random_uuid(), \
                'lifecycle.promote', decode(repeat('00', 32), 'hex'), 'oml-server', now(), \
                jsonb_build_object('artifact_id', $1::text, 'verdict_event_id', a.event_id::text, \
                                   'policy', jsonb_build_object('k', 1)), \
                decode(repeat('00', 32), 'hex'), decode(repeat('00', 64), 'hex'), \
                decode('00', 'hex') \
           FROM journal a \
          WHERE a.event_type = 'review.approve' AND a.payload->>'artifact_id' = $2",
    )
    .bind(&w.by_crew)
    .bind(&w.by_person)
    .execute(&mut conn)
    .await
    .expect("append the v0.1.0-shaped promotion");
    conn.close().await.expect("close");

    run_the_fill(&w.rig.db_name).await;

    let mut conn = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("superuser connect");
    let (held_by, same_user, at): (String, Option<bool>, String) = sqlx::query_as(
        "SELECT approver_held_by, approver_same_user, \
                to_char(approved_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
           FROM memories WHERE id = $1::uuid",
    )
    .bind(&w.by_crew)
    .fetch_one(&mut conn)
    .await
    .expect("the version's row");
    // The crew's reviewer approved it; the appended promotion names the
    // person's verdict, and the row now reads as that one.
    assert_eq!(
        (held_by.as_str(), same_user, at),
        ("human", None, persons.verdict_at),
        "the row holds the verdict the v0.1.0-shaped promotion names"
    );
    drop_db(&w.rig.db_name).await;
}

/// A version's latest promotion decides its approval: where that promotion
/// names a verdict the journal does not hold, the row is left null, and an
/// earlier promotion's verdict is not read in its place.
#[tokio::test]
async fn migration_0044_leaves_a_version_null_when_its_latest_promotion_names_an_unknown_verdict() {
    let Some(w) = World::boot().await else {
        return;
    };
    let before = stored_approvals(&w.rig.db_name).await;
    assert!(
        before
            .iter()
            .any(|(id, approval)| *id == w.by_crew && approval.is_some()),
        "the control: the crew-approved version holds its first promotion's approval: {before:?}"
    );
    // A later promotion of the same version, naming a verdict that is not in
    // the journal: appended by the minting superuser, since no build writes
    // one. The first promotion, with its real verdict, stays in the chain.
    let mut conn = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("superuser connect");
    sqlx::query(
        "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, \
                              at, payload, signed_bytes_hash, sig, signed_bytes) \
         SELECT a.tenant_id, (SELECT max(seq) + 1 FROM journal), gen_random_uuid(), \
                'lifecycle.promote', decode(repeat('00', 32), 'hex'), 'oml-server', now(), \
                jsonb_build_object('artifact_id', $1::text, \
                                   'ratification_event_id', gen_random_uuid()::text, \
                                   'policy', jsonb_build_object('k', 1)), \
                decode(repeat('00', 32), 'hex'), decode(repeat('00', 64), 'hex'), \
                decode('00', 'hex') \
           FROM journal a \
          WHERE a.event_type = 'review.approve' AND a.payload->>'artifact_id' = $1 \
          LIMIT 1",
    )
    .bind(&w.by_crew)
    .execute(&mut conn)
    .await
    .expect("append the later promotion");
    conn.close().await.expect("close");

    run_the_fill(&w.rig.db_name).await;

    let after = stored_approvals(&w.rig.db_name).await;
    assert!(
        after
            .iter()
            .any(|(id, approval)| *id == w.by_crew && approval.is_none()),
        "the version whose latest promotion names no verdict holds none: {after:?}"
    );
    let others_before: Vec<_> = before.iter().filter(|(id, _)| *id != w.by_crew).collect();
    let others_after: Vec<_> = after.iter().filter(|(id, _)| *id != w.by_crew).collect();
    assert_eq!(
        others_after, others_before,
        "no other version's approval changed"
    );
    drop_db(&w.rig.db_name).await;
}
