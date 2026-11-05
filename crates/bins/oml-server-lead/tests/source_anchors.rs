//! Source anchors over the lead composition against a throwaway database
//! (ADR-0057): the anchor is part of the signed submission, immutable per
//! version and inherited by no successor; a malformed or secret-shaped one is
//! refused by name; the origin filters run under row security and never
//! under-return; `stale` comes from source checks of the served version
//! alone, never from a held one; a source check admits only a promoted,
//! anchored tip from its author or a dreamer; and a supersession that names
//! anything but the claim's latest version is refused (d19).

use std::sync::atomic::{AtomicU64, Ordering};

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

const ENROLLMENT: &str = "enr_test_fixture_token";

// ---------------------------------------------------------------------------
// Throwaway database + rig (the oml-pg test pattern)
// ---------------------------------------------------------------------------

/// The throwaway cluster `DATABASE_URL` names; this suite has no default,
/// so it never reaches a cluster nobody named.
fn base_url() -> String {
    std::env::var("DATABASE_URL").expect("DATABASE_URL names a throwaway cluster")
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

fn run_tag() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .subsec_nanos();
    format!("{nanos:08x}")
}

static DB_N: AtomicU64 = AtomicU64::new(0);

/// Mints `db`'s owner/serving role pair and creates `db`; `false`, with
/// `did not run` printed, when `DATABASE_URL`'s role cannot mint, and a
/// failure under `OML_TEST_REQUIRE_SPLIT=1` (ADR-0034 d8).
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

struct Rig {
    router: Router,
    store: PgStore,
    db_name: String,
}

/// A rig whose store chooses the anchor-first plan for at most
/// `anchor_first_max` readable anchored rows, the store's own bound when
/// `None` (ADR-0057 d4).
async fn fresh_rig_with(anchor_first_max: Option<usize>) -> Option<Rig> {
    let db_name = format!(
        "oml_test_anchor_{}_{}_{}",
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
    let store = match anchor_first_max {
        Some(n) => store.with_anchor_first_max(n),
        None => store,
    };
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

/// A fresh Idempotency-Key per request a test means as new.
static IDEM: AtomicU64 = AtomicU64::new(0);

const REPO: &str = "github.com/e8kor/open-memory-lab";
const REV: &str = "ca61c9edd25895702e4ddc89df1848aae696d9ba";
const REV2: &str = "4b7e0c1a9d22f3e8a1c6b5d4e3f2a1b0c9d8e7f6";
const HASH: &str = "sha256:1f3a4f1a7b3d5e8c0f2a4b6d8e0c2f4a6b8d0e2c4f6a8b0d2e4c6f8a0b2d4e6c";
const HASH2: &str = "sha256:9c2e4f1a7b3d5e8c0f2a4b6d8e0c2f4a6b8d0e2c4f6a8b0d2e4c6f8a0b2d4e6c";

fn anchor(path: &str, fragment: &str) -> Value {
    json!({
        "repo": REPO, "rev": REV, "path": path, "fragment": fragment,
        "hash": HASH, "hasher": "oml-md/1",
    })
}

/// A world: an admin; two sibling workareas; a writer and a reader on the
/// first; a writer on the second only; a person's reviewer on both.
struct World {
    rig: Rig,
    node: Uuid,
    other: Uuid,
    writer: String,
    writer2: String,
    reader: String,
    outsider: String,
    reviewer: String,
}

async fn world() -> Option<World> {
    world_with(None).await
}

async fn world_with(anchor_first_max: Option<usize>) -> Option<World> {
    let rig = fresh_rig_with(anchor_first_max).await?;
    let (admin, root) = bootstrap(&rig.router).await;
    let node = create_workarea(&rig.router, &admin, root, "docs").await;
    let other = create_workarea(&rig.router, &admin, root, "elsewhere").await;
    let on = |node: Uuid, roles: &[&str]| {
        Value::from(
            roles
                .iter()
                .map(|r| json!({ "workarea": node, "role": r }))
                .collect::<Vec<_>>(),
        )
    };
    let writer = value_of(
        &issue(
            &rig.router,
            &admin,
            "sync",
            "writer",
            "crew/sync",
            "agent",
            on(node, &["writer", "reader"]),
        )
        .await,
    );
    let writer2 = value_of(
        &issue(
            &rig.router,
            &admin,
            "sync-two",
            "writer",
            "crew/sync-two",
            "agent",
            on(node, &["writer", "reader"]),
        )
        .await,
    );
    let reader = value_of(
        &issue(
            &rig.router,
            &admin,
            "reader",
            "reader",
            "crew/reader",
            "agent",
            on(node, &["reader"]),
        )
        .await,
    );
    let outsider = value_of(
        &issue(
            &rig.router,
            &admin,
            "outsider",
            "writer",
            "crew/outsider",
            "agent",
            on(other, &["writer", "reader"]),
        )
        .await,
    );
    let mut grants = on(node, &["dreamer", "reader"]);
    grants.as_array_mut().expect("array").extend(
        on(other, &["dreamer", "reader"])
            .as_array()
            .expect("array")
            .clone(),
    );
    let reviewer = value_of(
        &issue(
            &rig.router,
            &admin,
            "person-review",
            "dreamer",
            "person-review",
            "human",
            grants,
        )
        .await,
    );
    Some(World {
        rig,
        node,
        other,
        writer,
        writer2,
        reader,
        outsider,
        reviewer,
    })
}

impl World {
    async fn write(
        &self,
        token: &str,
        workarea: Uuid,
        key: &str,
        text: &str,
        anchor: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut body = json!({
            "claim_key": key, "kind": "memory", "body": text, "workarea": workarea,
        });
        if let Some(anchor) = anchor {
            body["anchor"] = anchor;
        }
        send(
            &self.rig.router,
            "POST",
            "/v1/memories",
            Some(token),
            Some(&format!("w-{key}-{text}")),
            Some(body),
        )
        .await
    }

    async fn written(
        &self,
        token: &str,
        workarea: Uuid,
        key: &str,
        text: &str,
        anchor: Option<Value>,
    ) -> String {
        let (status, body) = self.write(token, workarea, key, text, anchor).await;
        assert_eq!(status, StatusCode::ACCEPTED, "write {key}: {body}");
        body["artifact_id"].as_str().expect("id").to_owned()
    }

    async fn approve(&self, ids: &[String]) {
        let (status, body) = send(
            &self.rig.router,
            "POST",
            "/v2/review/verdicts/bulk",
            Some(&self.reviewer),
            Some(&format!("approve-{}", IDEM.fetch_add(1, Ordering::SeqCst))),
            Some(json!({ "verdict": "approve", "artifact_ids": ids })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["summary"]["approved"].as_u64(),
            Some(ids.len() as u64),
            "{body}"
        );
    }

    async fn supersede(
        &self,
        token: &str,
        target: &str,
        text: &str,
        expected: Option<u64>,
        anchor: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut body = json!({ "body": text });
        if let Some(expected) = expected {
            body["expected_version"] = json!(expected);
        }
        if let Some(anchor) = anchor {
            body["anchor"] = anchor;
        }
        send(
            &self.rig.router,
            "POST",
            &format!("/v1/memories/{target}/supersede"),
            Some(token),
            Some(&format!("s-{target}-{text}-{expected:?}")),
            Some(body),
        )
        .await
    }

    async fn check(&self, token: &str, id: &str, body: Value) -> (StatusCode, Value) {
        send(
            &self.rig.router,
            "POST",
            &format!("/v1/memories/{id}/source-check"),
            Some(token),
            Some(&format!("c-{}", IDEM.fetch_add(1, Ordering::SeqCst))),
            Some(body),
        )
        .await
    }

    async fn get(&self, token: &str, id: &str) -> Value {
        let (status, body) = send(
            &self.rig.router,
            "GET",
            &format!("/v1/memories/{id}"),
            Some(token),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn recall(&self, token: &str, body: Value) -> (StatusCode, Value) {
        send(
            &self.rig.router,
            "POST",
            "/v1/memories/recall",
            Some(token),
            None,
            Some(body),
        )
        .await
    }

    async fn list(&self, token: &str, query: &str) -> (StatusCode, Value) {
        send(
            &self.rig.router,
            "GET",
            &format!("/v1/memories?{query}"),
            Some(token),
            None,
            None,
        )
        .await
    }

    async fn done(self) {
        drop(self.rig.store);
        drop_db(&self.rig.db_name).await;
    }
}

fn ids(items: &Value) -> Vec<String> {
    items
        .as_array()
        .expect("items")
        .iter()
        .map(|i| {
            let m = if i["memory"].is_object() {
                &i["memory"]
            } else {
                i
            };
            m["artifact_id"].as_str().expect("id").to_owned()
        })
        .collect()
}

#[tokio::test]
async fn an_anchor_is_optional_on_every_kind_and_journalled_in_the_submission_event() {
    let Some(w) = world().await else { return };
    let a = anchor("docs/adr/ADR-0001-the-constitution.md", "d1");
    let anchored = w
        .written(
            &w.writer,
            w.node,
            "adr-0001-d1",
            "R1 no agent key promotes",
            Some(a.clone()),
        )
        .await;
    let plain = w
        .written(&w.writer, w.node, "plain", "an ordinary memory", None)
        .await;
    let steering = {
        let (status, body) = send(
            &w.rig.router,
            "POST",
            "/v1/memories",
            Some(&w.writer),
            Some("w-steer"),
            Some(
                json!({"kind": "steering", "body": "always cite the record", "workarea": w.node,
                        "anchor": anchor("docs/adr/ADR-0001-the-constitution.md", "d2")}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{body}");
        body["artifact_id"].as_str().expect("id").to_owned()
    };
    assert_eq!(w.get(&w.writer, &anchored).await["anchor"], a);
    assert_eq!(w.get(&w.writer, &plain).await["anchor"], Value::Null);
    assert_eq!(
        w.get(&w.writer, &steering).await["anchor"]["fragment"],
        "d2"
    );
    // The submission event carries the anchor by value: it is signed with
    // the version (ADR-0057 d2).
    let (status, events) = send(
        &w.rig.router,
        "GET",
        "/v2/events?after_seq=0&limit=100",
        Some(&w.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{events}");
    let payload = |id: &str| {
        events["items"]
            .as_array()
            .expect("items")
            .iter()
            .find(|e| e["event_type"] == "submission.remember" && e["payload"]["artifact_id"] == id)
            .map(|e| e["payload"].clone())
            .expect("the submission event")
    };
    assert_eq!(payload(&anchored)["anchor"], a);
    assert_eq!(payload(&plain)["anchor"], Value::Null);
    w.done().await;
}

#[tokio::test]
async fn the_anchor_is_immutable_and_signed_and_a_successor_inherits_none() {
    let Some(w) = world().await else { return };
    let a = anchor("docs/adr/ADR-0002.md", "d1");
    let v1 = w
        .written(
            &w.writer,
            w.node,
            "adr-0002-d1",
            "the first text",
            Some(a.clone()),
        )
        .await;
    w.approve(std::slice::from_ref(&v1)).await;
    // A source check changes no anchor.
    let (status, body) = w
        .check(
            &w.writer,
            &v1,
            json!({"rev": REV2, "result": "changed", "hash": HASH2}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(w.get(&w.reader, &v1).await["anchor"], a);
    // A successor sent without an anchor has none.
    let (status, v2) = w
        .supersede(&w.writer, &v1, "the second text", Some(1), None)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{v2}");
    let v2 = v2["artifact_id"].as_str().expect("id").to_owned();
    assert_eq!(w.get(&w.writer, &v2).await["anchor"], Value::Null);
    assert_eq!(
        w.get(&w.reader, &v1).await["anchor"],
        a,
        "the target's anchor is unchanged"
    );
    // The stored anchor is the signed one: the journal's submission payload
    // and the row agree, read past row security.
    let mut admin = PgConnection::connect(&super_url(&w.rig.db_name))
        .await
        .expect("connect");
    let (repo, path, fragment, hash): (String, String, String, String) = sqlx::query_as(
        "SELECT anchor_repo, anchor_path, anchor_fragment, anchor_hash FROM memories WHERE id = $1",
    )
    .bind(Uuid::parse_str(&v1).expect("uuid"))
    .fetch_one(&mut admin)
    .await
    .expect("row");
    let signed: Value = sqlx::query_scalar(
        "SELECT payload FROM journal WHERE event_type = 'submission.remember' AND payload->>'artifact_id' = $1",
    ).bind(&v1).fetch_one(&mut admin).await.expect("event");
    assert_eq!(
        signed["anchor"],
        json!({"repo": repo, "rev": REV, "path": path, "fragment": fragment, "hash": hash, "hasher": "oml-md/1"})
    );
    drop(admin);
    w.done().await;
}

#[tokio::test]
async fn a_malformed_anchor_is_request_validation_naming_its_field() {
    let Some(w) = world().await else { return };
    for (bad, field) in [
        (
            json!({"repo": "GitHub.com/x", "rev": REV, "path": "a.md"}),
            "/anchor/repo",
        ),
        (
            json!({"repo": REPO, "rev": "HEAD", "path": "a.md"}),
            "/anchor/rev",
        ),
        (
            json!({"repo": REPO, "rev": REV, "path": "../a.md"}),
            "/anchor/path",
        ),
        (
            json!({"repo": REPO, "rev": REV, "path": "a.md", "fragment": "page=2"}),
            "/anchor/fragment",
        ),
        (
            json!({"repo": REPO, "rev": REV, "path": "a.md", "fragment": "L9-L3"}),
            "/anchor/fragment",
        ),
        (
            json!({"repo": REPO, "rev": REV, "path": "a.md", "hash": HASH}),
            "/anchor/hasher",
        ),
        (
            json!({"repo": REPO, "rev": REV, "path": "a.md", "branch": "main"}),
            "/anchor/branch",
        ),
    ] {
        let (status, body) = w
            .write(&w.writer, w.node, "bad", "a claim", Some(bad.clone()))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
        assert_eq!(error_code(&body), "request.validation");
        assert_eq!(body["error"]["details"]["field"], field, "{bad}");
    }
    w.done().await;
}

#[tokio::test]
async fn a_secret_shaped_anchor_path_is_vetoed_by_the_scan() {
    let Some(w) = world().await else { return };
    let (status, body) = w
        .write(
            &w.writer,
            w.node,
            "env",
            "the deploy settings",
            Some(json!({"repo": REPO, "rev": REV, "path": "deploy/.env.production"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "submission.scan_veto");
    assert_eq!(body["error"]["details"]["field"], "/anchor/path");
    let (status, body) = w
        .write(
            &w.writer,
            w.node,
            "marker",
            "a claim",
            Some(json!({"repo": REPO, "rev": REV, "path": "docs/[oml:hold].md"})),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "a hold in an anchor is a veto: {body}"
    );
    w.done().await;
}

#[tokio::test]
async fn anchor_filters_respect_row_security_and_never_serve_anothers_held_rows() {
    let Some(w) = world().await else { return };
    // Promoted, anchored in the reader's workarea.
    let ours = w
        .written(
            &w.writer,
            w.node,
            "ours",
            "canary rollout drains first",
            Some(anchor("docs/ops/canary.md", "drain")),
        )
        .await;
    // Promoted, the same anchor, in a workarea the reader cannot read.
    let theirs = w
        .written(
            &w.outsider,
            w.other,
            "theirs",
            "canary rollout drains first",
            Some(anchor("docs/ops/canary.md", "drain")),
        )
        .await;
    w.approve(&[ours.clone(), theirs.clone()]).await;
    // Held, the same anchor, by another key in the reader's workarea.
    let held = w
        .written(
            &w.writer2,
            w.node,
            "held",
            "canary rollout drains first twice",
            Some(anchor("docs/ops/canary.md", "twice")),
        )
        .await;
    let filter = json!({"query": "canary rollout", "workarea": w.node, "anchor_repo": REPO, "anchor_path_prefix": "docs/ops/"});
    let (status, recall) = w.recall(&w.reader, filter.clone()).await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    assert_eq!(ids(&recall["hits"]), vec![ours.clone()]);
    // The held author sees its own held row with include_unreviewed, and
    // nobody else does.
    let mut own = filter.clone();
    own["include_unreviewed"] = json!(true);
    let (_, mine) = w.recall(&w.writer2, own).await;
    assert!(ids(&mine["hits"]).contains(&held), "{mine}");
    let (_, listed) = w
        .list(
            &w.reader,
            &format!(
                "own=false&workarea={}&anchor_repo={REPO}&anchor_path_prefix=docs/ops/",
                w.node
            ),
        )
        .await;
    assert_eq!(ids(&listed["items"]), vec![ours.clone()]);
    let (_, outside) = w
        .list(
            &w.reader,
            &format!("own=false&workarea={}&anchor_repo={REPO}", w.other),
        )
        .await;
    assert_eq!(
        error_code(&outside),
        "access.not_found",
        "an unreadable workarea serves nothing, {theirs} included: {outside}"
    );
    // The own listing by origin lists the key's own rows in every state.
    let (_, own_rows) = w
        .list(
            &w.writer2,
            &format!("own=true&anchor_repo={REPO}&anchor_path_prefix=docs/ops/"),
        )
        .await;
    assert_eq!(ids(&own_rows["items"]), vec![held]);
    // A filter leaves out unanchored rows, and a malformed one is refused.
    let plain = w
        .written(
            &w.writer,
            w.node,
            "plain",
            "canary rollout without a source",
            None,
        )
        .await;
    w.approve(std::slice::from_ref(&plain)).await;
    let (_, recall) = w.recall(&w.reader, filter).await;
    assert!(!ids(&recall["hits"]).contains(&plain));
    let (status, body) = w
        .recall(
            &w.reader,
            json!({"query": "canary", "anchor_repo": "Not A Repo"}),
        )
        .await;
    assert_eq!(
        (status, body["error"]["details"]["field"].clone()),
        (StatusCode::BAD_REQUEST, json!("/anchor_repo"))
    );
    w.done().await;
}

#[tokio::test]
async fn an_anchor_filtered_recall_never_under_returns() {
    let Some(w) = world().await else { return };
    let mut written = Vec::new();
    // Thirty strong matches outside the prefix, three weak ones inside it.
    for i in 0..30 {
        written.push(
            w.written(
                &w.writer,
                w.node,
                &format!("out-{i}"),
                &format!("rollback rollback rollback plan {i}"),
                Some(anchor(&format!("src/m{i}.rs"), "a")),
            )
            .await,
        );
    }
    let mut inside = Vec::new();
    for i in 0..3 {
        let id = w
            .written(
                &w.writer,
                w.node,
                &format!("in-{i}"),
                &format!("the rollback note number {i} with many other words around it"),
                Some(anchor("docs/runbook.md", &format!("s{i}"))),
            )
            .await;
        inside.push(id);
    }
    written.extend(inside.iter().cloned());
    w.approve(&written).await;
    let (status, recall) = w
        .recall(
            &w.reader,
            json!({"query": "rollback", "workarea": w.node, "k": 3, "anchor_path_prefix": "docs/"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{recall}");
    let mut got = ids(&recall["hits"]);
    got.sort();
    inside.sort();
    assert_eq!(got, inside);
    w.done().await;
}

#[tokio::test]
async fn a_source_check_admits_only_a_promoted_anchored_tip_from_its_author_or_a_dreamer() {
    let Some(w) = world().await else { return };
    let fresh = json!({"rev": REV2, "result": "fresh", "hash": HASH});
    let held = w
        .written(
            &w.writer,
            w.node,
            "k1",
            "a held claim",
            Some(anchor("a.md", "x")),
        )
        .await;
    let (status, body) = w.check(&w.writer, &held, fresh.clone()).await;
    assert_eq!(
        (status, body["error"]["details"]["field"].clone()),
        (StatusCode::BAD_REQUEST, json!("/artifact_id")),
        "held: {body}"
    );
    let plain = w
        .written(&w.writer, w.node, "k2", "an unanchored claim", None)
        .await;
    w.approve(&[held.clone(), plain.clone()]).await;
    let (status, body) = w.check(&w.writer, &plain, fresh.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "unanchored: {body}");
    // Another writer that reads it did not author it.
    let (status, body) = w.check(&w.writer2, &held, fresh.clone()).await;
    assert_eq!(
        (status, error_code(&body)),
        (StatusCode::FORBIDDEN, "access.author_only"),
        "{body}"
    );
    // A key that cannot read it is told nothing.
    let (status, body) = w.check(&w.outsider, &held, fresh.clone()).await;
    assert_eq!(
        (status, error_code(&body)),
        (StatusCode::NOT_FOUND, "access.not_found"),
        "{body}"
    );
    // A reader's role is below the verb.
    let (status, _) = w.check(&w.reader, &held, fresh.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // The author and a dreamer on its workarea may.
    let (status, body) = w.check(&w.writer, &held, fresh.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stale"], Value::Null);
    let (status, body) = w
        .check(
            &w.reviewer,
            &held,
            json!({"rev": REV2, "result": "removed"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stale"], "removed");
    // A superseded version is no longer checked.
    let (status, v2) = w
        .supersede(
            &w.writer,
            &held,
            "the next text",
            Some(1),
            Some(anchor("a.md", "x")),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{v2}");
    w.approve(&[v2["artifact_id"].as_str().expect("id").to_owned()])
        .await;
    let (status, _) = w.check(&w.writer, &held, fresh).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a superseded version");
    w.done().await;
}

#[tokio::test]
async fn stale_is_the_latest_source_check_of_the_served_version() {
    let Some(w) = world().await else { return };
    let v1 = w
        .written(
            &w.writer,
            w.node,
            "k",
            "rollback drains first",
            Some(anchor("docs/a.md", "d1")),
        )
        .await;
    w.approve(std::slice::from_ref(&v1)).await;
    let stale_of = |m: &Value| m["stale"].clone();
    assert_eq!(stale_of(&w.get(&w.reader, &v1).await), Value::Null);
    assert_eq!(w.get(&w.reader, &v1).await["source_checked"], Value::Null);
    let (status, _) = w
        .check(
            &w.writer,
            &v1,
            json!({"rev": REV2, "result": "changed", "hash": HASH2}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let got = w.get(&w.reader, &v1).await;
    assert_eq!(stale_of(&got), "changed");
    assert_eq!(got["source_checked"]["rev"], REV2);
    let (_, recall) = w
        .recall(&w.reader, json!({"query": "rollback", "workarea": w.node}))
        .await;
    assert_eq!(recall["hits"][0]["memory"]["stale"], "changed", "{recall}");
    // The workarea read counts its stale served versions, the doctor's
    // line (ADR-0057 d17).
    let (status, node) = send(
        &w.rig.router,
        "GET",
        &format!("/v2/workareas/{}", w.node),
        Some(&w.reader),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{node}");
    assert_eq!(node["stale_versions"], 1, "{node}");
    // A newer check supersedes the mark: a reverted source reads fresh.
    let (status, _) = w
        .check(
            &w.writer,
            &v1,
            json!({"rev": REV, "result": "fresh", "hash": HASH}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stale_of(&w.get(&w.reader, &v1).await), Value::Null);
    // A moved span reads not stale and says where it went (d18).
    let (status, body) = w.check(&w.writer, &v1, json!({"rev": REV2, "result": "moved", "hash": HASH, "path": "docs/b.md", "fragment": "d1"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got = w.get(&w.reader, &v1).await;
    assert_eq!(stale_of(&got), Value::Null);
    assert_eq!(
        got["source_checked"]["moved_to"],
        json!({"path": "docs/b.md", "fragment": "d1"})
    );
    assert_eq!(
        got["anchor"]["path"], "docs/a.md",
        "a move changes no anchor"
    );
    w.done().await;
}

#[tokio::test]
async fn a_held_successor_never_marks_its_served_version_stale() {
    let Some(w) = world().await else { return };
    let v1 = w
        .written(
            &w.writer,
            w.node,
            "k",
            "rollback drains first",
            Some(anchor("docs/a.md", "d1")),
        )
        .await;
    w.approve(std::slice::from_ref(&v1)).await;
    let (status, v2) = w
        .supersede(
            &w.writer,
            &v1,
            "rollback drains twice",
            Some(1),
            Some(anchor("docs/a.md", "d1")),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{v2}");
    let v2 = v2["artifact_id"].as_str().expect("id").to_owned();
    for token in [&w.reader, &w.writer, &w.reviewer] {
        let served = w.get(token, &v1).await;
        assert_eq!(
            (served["stale"].clone(), served["source_checked"].clone()),
            (Value::Null, Value::Null),
            "{served}"
        );
    }
    let (_, recall) = w
        .recall(&w.reader, json!({"query": "rollback", "workarea": w.node}))
        .await;
    assert_eq!(ids(&recall["hits"]), vec![v1.clone()]);
    assert_eq!(recall["hits"][0]["memory"]["stale"], Value::Null);
    // The held successor is never checked, so it never carries a mark.
    let (status, _) = w
        .check(
            &w.writer,
            &v2,
            json!({"rev": REV2, "result": "changed", "hash": HASH2}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(w.get(&w.writer, &v2).await["stale"], Value::Null);
    w.done().await;
}

#[tokio::test]
async fn a_supersession_naming_an_older_version_is_version_conflict() {
    let Some(w) = world().await else { return };
    let v1 = w
        .written(&w.writer, w.node, "k", "the first text", None)
        .await;
    w.approve(std::slice::from_ref(&v1)).await;
    let (status, body) = w
        .supersede(&w.writer, &v1, "no version named", None, None)
        .await;
    assert_eq!(
        (status, body["error"]["details"]["field"].clone()),
        (StatusCode::BAD_REQUEST, json!("/expected_version")),
        "{body}"
    );
    let (status, first) = w
        .supersede(&w.writer, &v1, "session one", Some(1), None)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(first["version"], 2);
    // A second session that read version 1 is refused, and nothing is
    // captured.
    let (status, body) = w
        .supersede(&w.writer, &v1, "session two", Some(1), None)
        .await;
    assert_eq!(
        (status, error_code(&body)),
        (StatusCode::CONFLICT, "submission.version_conflict"),
        "{body}"
    );
    assert_eq!(body["error"]["details"]["current_version"], 2);
    let (_, own) = w.list(&w.writer, "own=true&claim_key=k").await;
    assert_eq!(own["items"].as_array().expect("items").len(), 2, "{own}");
    // Naming the latest version passes the check and meets the tip rule.
    let (status, body) = w
        .supersede(&w.writer, &v1, "session two again", Some(2), None)
        .await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "rival held successors are allowed once named: {body}"
    );
    w.done().await;
}

#[tokio::test]
async fn mcp_oml_write_and_oml_read_carry_the_anchor_with_no_new_tool() {
    let Some(w) = world().await else { return };
    let a = anchor("docs/adr/ADR-0003.md", "d4");
    let (status, body) = send(
        &w.rig.router,
        "POST",
        "/mcp",
        Some(&w.writer),
        None,
        Some(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "oml_write", "arguments": {
                "text": "rollback is reviewed", "idempotency_key": "mcp-1",
                "claim_key": "adr-0003-d4", "workarea": w.node.to_string(), "anchor": a.clone()}},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["result"]["structuredContent"]["artifact_id"]
        .as_str()
        .expect("id")
        .to_owned();
    w.approve(std::slice::from_ref(&id)).await;
    let (status, body) = send(
        &w.rig.router,
        "POST",
        "/mcp",
        Some(&w.reader),
        None,
        Some(json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "oml_read", "arguments": {
                "query": "rollback", "workarea": w.node.to_string(),
                "anchor_repo": REPO, "anchor_path_prefix": "docs/adr/"}},
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let hit = &body["result"]["structuredContent"]["hits"][0];
    assert_eq!(hit["anchor"], a, "{body}");
    assert_eq!(hit["stale"], Value::Null);
    // In get mode the filters are refused by name.
    let (_, body) = send(
        &w.rig.router,
        "POST",
        "/mcp",
        Some(&w.reader),
        None,
        Some(json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "oml_read", "arguments": {"artifact_id": id, "anchor_repo": REPO}},
        })),
    )
    .await;
    assert_eq!(body["result"]["isError"], true, "{body}");
    // A supersession through oml_write names the version.
    let (_, body) = send(
        &w.rig.router,
        "POST",
        "/mcp",
        Some(&w.writer),
        None,
        Some(json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "oml_write", "arguments": {
                "text": "rollback is reviewed twice", "idempotency_key": "mcp-2", "replaces": id}},
        })),
    )
    .await;
    assert_eq!(
        body["result"]["isError"], true,
        "expected_version is required: {body}"
    );
    let (_, list) = send(
        &w.rig.router,
        "POST",
        "/mcp",
        Some(&w.writer),
        None,
        Some(json!({
        "jsonrpc": "2.0", "id": 5, "method": "tools/list", "params": {}})),
    )
    .await;
    assert_eq!(list["result"]["tools"].as_array().expect("tools").len(), 3);
    w.done().await;
}

#[tokio::test]
async fn a_version_conflict_never_names_a_version_the_caller_cannot_read() {
    let Some(w) = world().await else { return };
    let v1 = w
        .written(&w.writer, w.node, "k", "the first text", None)
        .await;
    w.approve(std::slice::from_ref(&v1)).await;
    // The answer a writer gets before anyone else proposes.
    let (status, before) = w.supersede(&w.writer, &v1, "polling", Some(2), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{before}");
    // A person's edit-as-proposal: held, authored by another key, which the
    // writer cannot read.
    let (status, edit) = w
        .supersede(&w.reviewer, &v1, "the reviewer's edit", Some(1), None)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{edit}");
    let edit = edit["artifact_id"].as_str().expect("id").to_owned();
    let (status, _) = send(
        &w.rig.router,
        "GET",
        &format!("/v1/memories/{edit}"),
        Some(&w.writer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Polling learns nothing: the same refusal, naming the version it can read.
    let (status, after) = w.supersede(&w.writer, &v1, "polling", Some(2), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{after}");
    assert_eq!(after["error"]["details"], before["error"]["details"]);
    assert_eq!(after["error"]["details"]["current_version"], 1);
    // And naming what it read passes the check.
    let (status, body) = w
        .supersede(&w.writer, &v1, "the writer's revision", Some(1), None)
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    // The writer's own held version is its latest from now on (d19).
    let (status, body) = w
        .supersede(&w.writer, &v1, "a second session", Some(1), None)
        .await;
    assert_eq!(
        (status, error_code(&body)),
        (StatusCode::CONFLICT, "submission.version_conflict"),
        "{body}"
    );
    w.done().await;
}

#[tokio::test]
async fn a_moved_checks_path_and_fragment_are_scanned_like_an_anchor() {
    let Some(w) = world().await else { return };
    let v1 = w
        .written(
            &w.writer,
            w.node,
            "k",
            "the deploy runbook",
            Some(anchor("docs/deploy.md", "d1")),
        )
        .await;
    w.approve(std::slice::from_ref(&v1)).await;
    for (body, field) in [
        (
            json!({"rev": REV2, "result": "moved", "hash": HASH, "path": "deploy/.env.production"}),
            "/path",
        ),
        (
            json!({"rev": REV2, "result": "moved", "hash": HASH, "path": "docs/deploy.md", "fragment": "[oml:hold]"}),
            "/fragment",
        ),
    ] {
        let (status, refused) = w.check(&w.writer, &v1, body.clone()).await;
        assert_eq!(
            status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{body}: {refused}"
        );
        assert_eq!(error_code(&refused), "submission.scan_veto");
        assert_eq!(refused["error"]["details"]["field"], field);
    }
    assert_eq!(
        w.get(&w.reader, &v1).await["source_checked"],
        Value::Null,
        "nothing was kept"
    );
    w.done().await;
}

/// The anchor fixture of the regression law: three repositories, nested
/// paths, and words shared across them so every filter cuts a ranked list.
async fn anchor_fixture(w: &World) -> Vec<String> {
    let repos = ["github.com/a/one", "github.com/a/two", "gitlab.com/b/three"];
    let paths = [
        "docs/adr/x.md",
        "docs/ops/y.md",
        "src/store.rs",
        "docs/adr/z.md",
        "README.md",
    ];
    let words = [
        "rollback drains the canary",
        "canary settles before rollback",
        "the store keeps postings",
        "postings rollback budget",
        "a canary for the store",
    ];
    let mut ids = Vec::new();
    for (r, repo) in repos.iter().enumerate() {
        for (p, path) in paths.iter().enumerate() {
            for (i, text) in words.iter().enumerate() {
                let key = format!("f-{r}-{p}-{i}");
                let anchor =
                    json!({"repo": repo, "rev": REV, "path": path, "fragment": format!("s{i}")});
                ids.push(
                    w.written(
                        &w.writer,
                        w.node,
                        &key,
                        &format!("{text} {key}"),
                        Some(anchor),
                    )
                    .await,
                );
            }
        }
    }
    for (i, text) in words.iter().enumerate() {
        ids.push(
            w.written(
                &w.writer,
                w.node,
                &format!("plain-{i}"),
                &format!("{text} plain"),
                None,
            )
            .await,
        );
    }
    w.approve(&ids).await;
    ids
}

/// Every fixture query under every filter shape, for each single-list
/// pipeline the lead serves: the filtered recall equals the unfiltered
/// list filtered then cut, in order and score (ADR-0057 d4).
async fn filtered_equals_filter_then_cut(anchor_first_max: Option<usize>) {
    let Some(w) = world_with(anchor_first_max).await else {
        return;
    };
    anchor_fixture(&w).await;
    let anchors: std::collections::BTreeMap<String, Value> = {
        let (_, page) = w
            .list(
                &w.reader,
                &format!("own=false&workarea={}&limit=200", w.node),
            )
            .await;
        page["items"]
            .as_array()
            .expect("items")
            .iter()
            .map(|m| {
                (
                    m["artifact_id"].as_str().expect("id").to_owned(),
                    m["anchor"].clone(),
                )
            })
            .collect()
    };
    let filters: [(Option<&str>, Option<&str>); 6] = [
        (Some("github.com/a/one"), None),
        (None, Some("docs/")),
        (Some("github.com/a/two"), Some("docs/adr/")),
        (Some("gitlab.com/b/three"), Some("src/store.rs")),
        (None, Some("README")),
        (Some("github.com/none/here"), None),
    ];
    for search in ["bm25-postings", "bm25", "keyword-any"] {
        for query in [
            "rollback",
            "canary rollback",
            "store postings",
            "settles",
            "budget canary store",
        ] {
            let (status, all) = w
                .recall(
                    &w.reader,
                    json!({"query": query, "workarea": w.node, "k": 100, "search": search}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{all}");
            for (repo, prefix) in filters {
                for k in [1, 3, 8] {
                    let mut body =
                        json!({"query": query, "workarea": w.node, "k": k, "search": search});
                    if let Some(repo) = repo {
                        body["anchor_repo"] = json!(repo);
                    }
                    if let Some(prefix) = prefix {
                        body["anchor_path_prefix"] = json!(prefix);
                    }
                    let (status, got) = w.recall(&w.reader, body).await;
                    assert_eq!(status, StatusCode::OK, "{got}");
                    let expected: Vec<(String, Value)> = all["hits"]
                        .as_array()
                        .expect("hits")
                        .iter()
                        .filter(|h| {
                            let a = &anchors[h["memory"]["artifact_id"].as_str().expect("id")];
                            !a.is_null()
                                && repo.is_none_or(|r| a["repo"] == r)
                                && prefix.is_none_or(|p| {
                                    a["path"].as_str().is_some_and(|x| x.starts_with(p))
                                })
                        })
                        .take(k)
                        .map(|h| {
                            (
                                h["memory"]["artifact_id"].as_str().expect("id").to_owned(),
                                h["score"].clone(),
                            )
                        })
                        .collect();
                    let served: Vec<(String, Value)> = got["hits"]
                        .as_array()
                        .expect("hits")
                        .iter()
                        .map(|h| {
                            (
                                h["memory"]["artifact_id"].as_str().expect("id").to_owned(),
                                h["score"].clone(),
                            )
                        })
                        .collect();
                    assert_eq!(
                        served, expected,
                        "{search} {query:?} {repo:?} {prefix:?} k {k}"
                    );
                }
            }
        }
    }
    w.done().await;
}

#[tokio::test]
async fn an_anchor_first_filtered_recall_equals_filter_then_cut() {
    filtered_equals_filter_then_cut(None).await;
}

#[tokio::test]
async fn a_postings_first_filtered_recall_equals_filter_then_cut() {
    filtered_equals_filter_then_cut(Some(0)).await;
}

#[tokio::test]
async fn the_anchor_filter_is_an_index_range_for_the_serving_role_under_row_security() {
    let Some(w) = world().await else { return };
    w.written(
        &w.writer,
        w.node,
        "k",
        "one anchored claim",
        Some(anchor("docs/a.md", "d1")),
    )
    .await;
    let tenant: Uuid = {
        let mut admin = PgConnection::connect(&super_url(&w.rig.db_name))
            .await
            .expect("connect");
        sqlx::query_scalar("SELECT tenant_id FROM memories LIMIT 1")
            .fetch_one(&mut admin)
            .await
            .expect("tenant")
    };
    let mut serving = PgConnection::connect(&serving_url(&w.rig.db_name))
        .await
        .expect("serving");
    let (bypass,): (bool,) =
        sqlx::query_as("SELECT rolbypassrls FROM pg_roles WHERE rolname = current_user")
            .fetch_one(&mut serving)
            .await
            .expect("role");
    assert!(!bypass, "the serving role is bound by row security");
    sqlx::query("SELECT set_config('oml.tenant_id', $1, false)")
        .bind(tenant.to_string())
        .execute(&mut serving)
        .await
        .expect("tenant");
    sqlx::query("SET enable_seqscan = off")
        .execute(&mut serving)
        .await
        .expect("set");
    for (repo, prefix) in [(true, true), (false, true), (true, false)] {
        let sql = oml_pg::anchored_ids_sql(repo, prefix);
        let explained = format!("EXPLAIN {sql}");
        let mut explain = sqlx::query(&explained)
            .bind(tenant)
            .bind(vec![w.node])
            .bind(vec!["memory"])
            .bind(Vec::<Uuid>::new())
            .bind(Vec::<Uuid>::new());
        if repo {
            explain = explain.bind(REPO);
        }
        if prefix {
            explain = explain.bind("docs/").bind("docs0");
        }
        explain = explain.bind(10_i64);
        let plan: Vec<String> = explain
            .fetch_all(&mut serving)
            .await
            .expect("explain")
            .iter()
            .map(|r| sqlx::Row::get::<String, _>(r, 0))
            .collect();
        let plan = plan.join("\n");
        assert!(
            plan.contains("Index") && plan.contains("anchor"),
            "an anchor index serves it: {plan}"
        );
        if prefix {
            assert!(
                plan.contains("Index Cond") && plan.contains("anchor_path ~>=~"),
                "the prefix is an index range, not a heap filter: {plan}"
            );
        }
        if repo {
            assert!(plan.contains("anchor_repo ="), "{plan}");
        }
    }
    drop(serving);
    w.done().await;
}
