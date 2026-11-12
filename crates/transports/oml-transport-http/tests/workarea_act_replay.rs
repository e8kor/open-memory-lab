//! The four workarea acts the web console runs, over the in-memory
//! doubles: creating a workarea, proposing a transition, ratifying one and
//! setting the mode each
//! replay the recorded outcome under the same `Idempotency-Key` and request,
//! with `Idempotency-Replay: true` and nothing acted again, and refuse the
//! same key on another request with `idempotency.digest_mismatch`
//! (ADR-0070 d3 layer 4, d5). Each act claims its key before it acts, so two
//! sends of one form act once, a send waits on another attempt's claim and
//! answers its outcome, and a claim whose record was lost holds the key
//! rather than letting a retry act again (#233). Each test is named after
//! the sentence it proves.

mod support;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use oml_app::domain::access::Covers;
use oml_app::domain::name::{Name, NamedKind};
use oml_app::domain::{HeldBy, Role, Sha256Hash};
use oml_app::doubles::MemStore;
use oml_app::ports::{
    IdempotencyClaim, IdempotencyPut, IdempotencyRecord, IdempotencyScope, JournalStore as _,
};
use oml_app::types::{BootstrapRequest, GrantRequest, IssueKeyRequest};

use support::{app, state};

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

async fn send(
    router: &Router,
    path: &str,
    bearer: &str,
    key: &str,
    body: Option<Value>,
) -> (StatusCode, HeaderMap, Value) {
    let builder = Request::post(path)
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .header("idempotency-key", key);
    let request = match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string())),
        None => builder.body(Body::empty()),
    }
    .expect("a request");
    let response = router.clone().oneshot(request).await.expect("an answer");
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn replayed(headers: &HeaderMap) -> bool {
    headers
        .get("idempotency-replay")
        .is_some_and(|v| v == "true")
}

struct World {
    store: MemStore,
    router: Router,
    tenant: Uuid,
    admin: String,
    ann: String,
    proj: Uuid,
    draft: Uuid,
}

/// `universe` → `proj` and `draft`; the admin key, and ann, a person's
/// dreamer key on both.
async fn world() -> World {
    let store = MemStore::new();
    let app = app(&store);
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("acme-admin"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    let mut nodes = Vec::new();
    for n in ["proj", "draft"] {
        nodes.push(
            app.create_workarea(&admin, &name(n), Some(boot.root_workarea_id.into()), None)
                .await
                .expect("a workarea")
                .workarea
                .workarea_id,
        );
    }
    let grants = nodes
        .iter()
        .flat_map(|node| {
            [Role::Dreamer, Role::Reader].map(|role| GrantRequest {
                node: (*node).into(),
                role,
                deny: Vec::new(),
                covers: Covers::Subtree,
            })
        })
        .collect();
    let ann = app
        .issue_key(
            &admin,
            &IssueKeyRequest {
                profile: None,
                role: Role::Dreamer,
                name: name("ann-review"),
                principal: "ann".to_owned(),
                held_by: Some(HeldBy::Human),
                default_workarea: None,
                crew: None,
                grants,
                connect_code: false,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("ann's key")
        .value;
    World {
        router: state(&store, app).into_router(),
        tenant: boot.tenant_id,
        store,
        admin,
        ann,
        proj: nodes[0],
        draft: nodes[1],
    }
}

#[tokio::test]
async fn a_proposal_replays_under_the_same_key_and_request_and_refuses_the_key_on_another() {
    let w = world().await;
    let path = format!("/v2/workareas/{}/transitions", w.proj);
    let body = json!({"to": "committed"});
    let (status, headers, first) = send(&w.router, &path, &w.ann, "p-1", Some(body.clone())).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{first}");
    assert!(!replayed(&headers));
    let (status, headers, again) = send(&w.router, &path, &w.ann, "p-1", Some(body)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{again}");
    assert!(replayed(&headers), "the resubmission is a replay");
    assert_eq!(again, first);
    let pending = w.store.pending_transitions(w.tenant, w.proj).await.unwrap();
    assert_eq!(pending.len(), 1, "proposed once: {pending:?}");
    let (status, _, refused) = send(
        &w.router,
        &path,
        &w.ann,
        "p-1",
        Some(json!({"to": "validated"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
}

#[tokio::test]
async fn a_ratification_replays_under_the_same_key_and_refuses_the_key_on_another_transition() {
    let w = world().await;
    let propose = format!("/v2/workareas/{}/transitions", w.proj);
    let (status, _, proposal) = send(
        &w.router,
        &propose,
        &w.ann,
        "p-1",
        Some(json!({"to": "committed"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{proposal}");
    let ratify = |t: &str| format!("/v2/workareas/{}/transitions/{t}/ratify", w.proj);
    let first_id = proposal["transition_id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let (status, headers, first) = send(&w.router, &ratify(&first_id), &w.admin, "r-1", None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert!(!replayed(&headers));
    // Without the replay a second ratification fails as no longer live;
    // with it, the person sees the act that already succeeded.
    let (status, headers, again) = send(&w.router, &ratify(&first_id), &w.admin, "r-1", None).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert!(replayed(&headers), "the resubmission is a replay");
    assert_eq!(again, first);
    let node = w.store.workarea(w.tenant, w.proj).await.unwrap().unwrap();
    assert_eq!(node.status.as_str(), "committed");
    // The same key on another transition is refused by name.
    let (status, _, next) = send(
        &w.router,
        &propose,
        &w.ann,
        "p-2",
        Some(json!({"to": "validated"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{next}");
    let next_id = next["transition_id"].as_str().expect("an id").to_owned();
    let (status, _, refused) = send(&w.router, &ratify(&next_id), &w.admin, "r-1", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
    let node = w.store.workarea(w.tenant, w.proj).await.unwrap().unwrap();
    assert_eq!(node.status.as_str(), "committed", "nothing ratified");
}

#[tokio::test]
async fn a_mode_change_replays_under_the_same_key_and_request_and_refuses_the_key_on_another() {
    let w = world().await;
    let path = format!("/v2/workareas/{}/mode", w.draft);
    let body = json!({"mode": "in_progress"});
    let (status, headers, first) = send(&w.router, &path, &w.ann, "m-1", Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert!(!replayed(&headers));
    let (status, headers, again) = send(&w.router, &path, &w.ann, "m-1", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert!(replayed(&headers), "the resubmission is a replay");
    assert_eq!(again, first);
    let (status, _, refused) = send(
        &w.router,
        &path,
        &w.ann,
        "m-1",
        Some(json!({"mode": "standard"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
    let node = w.store.workarea(w.tenant, w.draft).await.unwrap().unwrap();
    assert!(node.in_progress, "the refused request committed nothing");
}

#[tokio::test]
async fn a_creation_replays_under_the_same_key_and_request_and_refuses_the_key_on_another() {
    let w = world().await;
    let body = json!({"name": "fresh", "parent": "universe"});
    let (status, headers, first) = send(
        &w.router,
        "/v2/workareas",
        &w.admin,
        "c-1",
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert!(!replayed(&headers));
    // Without the replay a second creation answers name.taken; with it, the
    // person sees the creation that already succeeded.
    let (status, headers, again) =
        send(&w.router, "/v2/workareas", &w.admin, "c-1", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert!(replayed(&headers), "the resubmission is a replay");
    assert_eq!(again, first);
    let (status, _, refused) = send(
        &w.router,
        "/v2/workareas",
        &w.admin,
        "c-1",
        Some(json!({"name": "other", "parent": "universe"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
    let root = w
        .store
        .workarea_children(
            w.tenant,
            first["parent_id"].as_str().unwrap().parse().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        root.iter().filter(|n| n.name.as_str() == "fresh").count(),
        1,
        "created once"
    );
    assert!(root.iter().all(|n| n.name.as_str() != "other"), "{root:?}");
}

// -- each act costs one request of the person's rate (#253, #281) ---------

/// ann, a person of the sign-up tenant at one request a minute: her root
/// `universe.ann` → `proj` and `draft`, her dreamer key on both, and the
/// router over an app with sign-up on and a clock that never leaves its
/// minute, so each test spends one window (ADR-0068 d5).
async fn world_at_one_request_a_minute() -> World {
    let store = MemStore::new();
    let app = app(&store);
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("acme-admin"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    let person_key = |key_name: &str, grants: Vec<GrantRequest>| IssueKeyRequest {
        profile: None,
        role: Role::Dreamer,
        name: name(key_name),
        principal: "ann".to_owned(),
        held_by: Some(HeldBy::Human),
        default_workarea: None,
        crew: None,
        grants,
        connect_code: false,
        user: Some(name("ann")),
        guardrail_scope: None,
    };
    // The first key naming ann creates her root (ADR-0044 d3).
    let first = app
        .issue_key(&admin, &person_key("ann-first", Vec::new()))
        .await
        .expect("ann's first key");
    let root = store
        .key(boot.tenant_id, first.key_id)
        .await
        .unwrap()
        .and_then(|k| k.user)
        .expect("ann's root");
    let mut nodes = Vec::new();
    for n in ["proj", "draft"] {
        nodes.push(
            app.create_workarea(&admin, &name(n), Some(root.into()), None)
                .await
                .expect("a workarea")
                .workarea
                .workarea_id,
        );
    }
    let grants = nodes
        .iter()
        .flat_map(|node| {
            [Role::Dreamer, Role::Reader].map(|role| GrantRequest {
                node: (*node).into(),
                role,
                deny: Vec::new(),
                covers: Covers::Subtree,
            })
        })
        .collect();
    let ann = app
        .issue_key(&admin, &person_key("ann-review", grants))
        .await
        .expect("ann's key")
        .value;
    let app = app.with_signup(oml_app::types::SignupPolicy {
        mode: oml_app::domain::signup::SignupMode::Allowlist,
        tenant_id: boot.tenant_id,
        issuer: name("acme-admin"),
        terms_version: "2026-11-06".to_owned(),
        terms_url: "https://oml.example.com/terms".to_owned(),
        privacy_url: "https://oml.example.com/privacy".to_owned(),
        per_ip_per_hour: 10,
        quotas: oml_app::domain::signup::Quotas {
            requests_per_minute: 1,
            ..oml_app::domain::signup::Quotas::default()
        },
    });
    World {
        router: state(&store, app).into_router(),
        tenant: boot.tenant_id,
        store,
        admin,
        ann,
        proj: nodes[0],
        draft: nodes[1],
    }
}

/// ann's next request in the minute is refused `limit.rate`: her one
/// request was spent, and spent once.
async fn the_minute_is_spent(w: &World) {
    let (status, _, refused) = send(
        &w.router,
        &format!("/v2/workareas/{}/mode", w.draft),
        &w.ann,
        "spent",
        Some(json!({"mode": "standard"})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{refused}");
    assert_eq!(refused["error"]["code"], "limit.rate", "{refused}");
}

/// A proposal resolves its caller once, so at one request a minute the
/// person's first request proposes (#253).
#[tokio::test]
async fn a_proposal_by_a_persons_key_costs_one_request_of_its_rate() {
    let w = world_at_one_request_a_minute().await;
    let path = format!("/v2/workareas/{}/transitions", w.proj);
    let (status, _, proposal) = send(
        &w.router,
        &path,
        &w.ann,
        "p-1",
        Some(json!({"to": "committed"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{proposal}");
    the_minute_is_spent(&w).await;
}

/// A ratification resolves its caller once, so at one request a minute
/// the person's first request ratifies (#253).
#[tokio::test]
async fn a_ratification_by_a_persons_key_costs_one_request_of_its_rate() {
    let w = world_at_one_request_a_minute().await;
    let propose = format!("/v2/workareas/{}/transitions", w.proj);
    let (status, _, proposal) = send(
        &w.router,
        &propose,
        &w.admin,
        "p-1",
        Some(json!({"to": "committed"})),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{proposal}");
    let id = proposal["transition_id"].as_str().expect("an id");
    let (status, _, ratified) = send(
        &w.router,
        &format!("/v2/workareas/{}/transitions/{id}/ratify", w.proj),
        &w.ann,
        "r-1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{ratified}");
    the_minute_is_spent(&w).await;
}

/// A mode change resolves its caller once, so at one request a minute the
/// person's first request sets the mode (#253).
#[tokio::test]
async fn a_mode_change_by_a_persons_key_costs_one_request_of_its_rate() {
    let w = world_at_one_request_a_minute().await;
    let path = format!("/v2/workareas/{}/mode", w.draft);
    let (status, _, set) = send(
        &w.router,
        &path,
        &w.ann,
        "m-1",
        Some(json!({"mode": "in_progress"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{set}");
    the_minute_is_spent(&w).await;
}

// -- the claim an act takes on its key before it acts (#233) ---------------

/// The claim scope and request digest `POST /v2/workareas` takes for
/// `body` under `idem` as the admin key (ADR-0070 d5): the act's name
/// beside the key, no workarea, and the digest over the empty path user
/// and the body's bytes.
async fn create_claim(w: &World, idem: &str, body: &Value) -> (IdempotencyScope, String) {
    let key_id = w
        .store
        .resolve_name(w.tenant, NamedKind::Key, &name("acme-admin"))
        .await
        .expect("a lookup")
        .expect("the admin key");
    let mut bytes = b"\n".to_vec();
    bytes.extend_from_slice(body.to_string().as_bytes());
    (
        IdempotencyScope {
            tenant_id: w.tenant,
            key_id,
            workarea_id: Uuid::nil(),
            idem_key: format!("workarea.create:{idem}"),
        },
        Sha256Hash::digest(&bytes).to_prefixed(),
    )
}

async fn created(w: &World, name: &str) -> usize {
    let root = w
        .store
        .resolve_name(w.tenant, NamedKind::Workarea, &self::name("universe"))
        .await
        .expect("a lookup")
        .expect("the root");
    w.store
        .workarea_children(w.tenant, root)
        .await
        .expect("children")
        .iter()
        .filter(|n| n.name.as_str() == name)
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_creates_under_one_key_create_once_and_answer_alike() {
    let w = world().await;
    let body = json!({"name": "twice", "parent": "universe"});
    let ((s1, h1, b1), (s2, h2, b2)) = tokio::join!(
        send(
            &w.router,
            "/v2/workareas",
            &w.admin,
            "c-2",
            Some(body.clone())
        ),
        send(
            &w.router,
            "/v2/workareas",
            &w.admin,
            "c-2",
            Some(body.clone())
        ),
    );
    assert_eq!(
        (s1, s2),
        (StatusCode::CREATED, StatusCode::CREATED),
        "{b1} {b2}"
    );
    assert_eq!(
        [replayed(&h1), replayed(&h2)]
            .iter()
            .filter(|r| **r)
            .count(),
        1,
        "one send acts and the other replays it"
    );
    assert_eq!(b1, b2, "both answers agree");
    assert_eq!(created(&w, "twice").await, 1, "created once");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_send_whose_key_another_attempt_holds_waits_and_answers_that_attempts_outcome() {
    let w = world().await;
    let body = json!({"name": "held", "parent": "universe"});
    let (scope, digest) = create_claim(&w, "c-3", &body).await;
    assert_eq!(
        w.store
            .idempotency_claim(&scope, &digest, Uuid::from_u128(0xC3))
            .await
            .unwrap(),
        IdempotencyClaim::Claimed,
        "another attempt, in another process, holds the key"
    );
    let settle = {
        let store = w.store.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            store
                .idempotency_settle(
                    IdempotencyPut {
                        scope,
                        record: IdempotencyRecord {
                            request_digest: digest,
                            response: json!({"status": 201, "body": {"answered": "first"}}),
                        },
                    },
                    Uuid::from_u128(0xC3),
                )
                .await
                .unwrap()
        })
    };
    let (status, headers, again) =
        send(&w.router, "/v2/workareas", &w.admin, "c-3", Some(body)).await;
    assert!(settle.await.unwrap(), "the other attempt settled");
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert!(replayed(&headers));
    assert_eq!(
        again,
        json!({"answered": "first"}),
        "the first attempt's answer"
    );
    assert_eq!(created(&w, "held").await, 0, "this send acted nothing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_send_whose_key_a_stopped_attempt_holds_answers_in_progress_and_acts_nothing() {
    let w = world().await;
    let body = json!({"name": "stuck", "parent": "universe"});
    let (scope, digest) = create_claim(&w, "c-4", &body).await;
    w.store
        .idempotency_claim(&scope, &digest, Uuid::from_u128(0xC4))
        .await
        .unwrap();
    let (status, headers, refused) =
        send(&w.router, "/v2/workareas", &w.admin, "c-4", Some(body)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.in_progress");
    assert!(headers.contains_key(header::RETRY_AFTER), "{headers:?}");
    assert_eq!(created(&w, "stuck").await, 0, "nothing acted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_act_whose_record_is_lost_still_answers_and_its_retry_acts_nothing() {
    let w = world().await;
    let body = json!({"name": "lost", "parent": "universe"});
    w.store.fail_next_idempotency_settle();
    let (status, headers, first) = send(
        &w.router,
        "/v2/workareas",
        &w.admin,
        "c-5",
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "the act committed: {first}");
    assert!(!replayed(&headers));
    let (status, _, retry) = send(&w.router, "/v2/workareas", &w.admin, "c-5", Some(body)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{retry}");
    assert_eq!(retry["error"]["code"], "idempotency.in_progress");
    assert_eq!(created(&w, "lost").await, 1, "the retry acted nothing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_act_frees_its_key_so_the_resubmission_acts_again() {
    let w = world().await;
    let body = json!({"name": "later", "parent": "nowhere"});
    let (status, _, first) = send(
        &w.router,
        "/v2/workareas",
        &w.admin,
        "c-6",
        Some(body.clone()),
    )
    .await;
    assert!(status.is_client_error(), "{first}");
    let code = first["error"]["code"].clone();
    assert_ne!(code, "idempotency.in_progress");
    let (status, _, again) = send(&w.router, "/v2/workareas", &w.admin, "c-6", Some(body)).await;
    assert!(status.is_client_error(), "{again}");
    assert_eq!(
        again["error"]["code"], code,
        "the resubmission acted again and was refused alike"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_dropped_after_its_act_committed_never_lets_the_resubmission_act_again() {
    let w = world().await;
    let body = json!({"name": "dropped", "parent": "universe"});
    let park = w.store.park_after_next_commit();
    let first = {
        let router = w.router.clone();
        let admin = w.admin.clone();
        let body = body.clone();
        tokio::spawn(async move { send(&router, "/v2/workareas", &admin, "c-7", Some(body)).await })
    };
    while !park.reached() {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    // The act committed; the client gives up, as oml-web's call timeout
    // does, and its request is dropped before the outcome is recorded.
    first.abort();
    assert!(first.await.is_err(), "the request was dropped");
    park.open();
    let (status, headers, again) =
        send(&w.router, "/v2/workareas", &w.admin, "c-7", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{again}");
    assert!(
        replayed(&headers),
        "the resubmission replays the act: {again}"
    );
    assert_eq!(created(&w, "dropped").await, 1, "created once");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_step_that_fails_after_the_act_committed_holds_the_key_so_the_resubmission_acts_nothing()
{
    let w = world().await;
    let path = format!("/v2/workareas/{}/mode", w.draft);
    let body = json!({"mode": "in_progress"});
    w.store.fail_workarea_children_after_next_commit();
    let (status, _, first) = send(&w.router, &path, &w.ann, "m-9", Some(body.clone())).await;
    assert!(
        status.is_server_error(),
        "the read after the commit failed: {first}"
    );
    let node = w.store.workarea(w.tenant, w.draft).await.unwrap().unwrap();
    assert!(node.in_progress, "the act committed");
    let head = w.store.chain_head(w.tenant).await.unwrap();
    let (status, _, again) = send(&w.router, &path, &w.ann, "m-9", Some(body)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["error"]["code"], "idempotency.in_progress");
    assert_eq!(
        w.store.chain_head(w.tenant).await.unwrap(),
        head,
        "the resubmission journalled nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_refused_before_its_act_runs_frees_its_key() {
    let w = world().await;
    // `prefix` is refused after the key is claimed and before the act runs.
    let body = json!({"name": "early", "parent": "universe", "prefix": "ex"});
    let (status, _, first) = send(
        &w.router,
        "/v2/workareas",
        &w.admin,
        "c-8",
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{first}");
    let started = std::time::Instant::now();
    let (status, _, again) = send(&w.router, "/v2/workareas", &w.admin, "c-8", Some(body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{again}");
    assert_eq!(again["error"]["code"], first["error"]["code"]);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "the claim was released, so the resubmission did not wait it out"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_panic_after_the_act_committed_is_answered_as_a_handler_panic_and_holds_the_key() {
    let w = world().await;
    let body = json!({"name": "panicked", "parent": "universe"});
    w.store.panic_after_next_commit();
    let (status, _, first) = send(
        &w.router,
        "/v2/workareas",
        &w.admin,
        "c-9",
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{first}");
    assert_eq!(first["error"]["details"]["reason"], "handler_panic");
    let (status, _, again) = send(&w.router, "/v2/workareas", &w.admin, "c-9", Some(body)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["error"]["code"], "idempotency.in_progress");
    assert_eq!(created(&w, "panicked").await, 1, "created once");
}

/// How many allowlist events the journal holds.
async fn email_events(w: &World) -> usize {
    w.store
        .events_range(w.tenant, 1, 100_000)
        .await
        .expect("the journal reads")
        .iter()
        .filter(|e| e.envelope.event_type.starts_with("web.email"))
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_allowlist_adds_under_one_key_add_once_and_answer_alike() {
    // The allowlist acts claim their key before they act, as every console
    // act does (#233; ADR-0070 d3 layer 4, the seventh table).
    let w = world().await;
    let before = email_events(&w).await;
    let body = json!({"email": "carol@example.com", "user": "carol", "admin": false});
    let ((s1, h1, b1), (s2, h2, b2)) = tokio::join!(
        send(
            &w.router,
            "/v2/web/emails",
            &w.admin,
            "e-1",
            Some(body.clone())
        ),
        send(
            &w.router,
            "/v2/web/emails",
            &w.admin,
            "e-1",
            Some(body.clone())
        ),
    );
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK), "{b1} {b2}");
    assert_eq!(
        [replayed(&h1), replayed(&h2)]
            .iter()
            .filter(|r| **r)
            .count(),
        1,
        "one send acts and the other replays it"
    );
    assert_eq!(b1, b2, "both answers agree");
    assert_eq!(email_events(&w).await, before + 1, "added once");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_removals_under_one_key_remove_once_and_answer_alike() {
    let w = world().await;
    let (status, _, added) = send(
        &w.router,
        "/v2/web/emails",
        &w.admin,
        "e-2",
        Some(json!({"email": "dave@example.com", "user": "dave", "admin": false})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    let before = email_events(&w).await;
    let body = json!({"email": "dave@example.com"});
    let ((s1, h1, b1), (s2, h2, b2)) = tokio::join!(
        send(
            &w.router,
            "/v2/web/emails/remove",
            &w.admin,
            "r-1",
            Some(body.clone())
        ),
        send(
            &w.router,
            "/v2/web/emails/remove",
            &w.admin,
            "r-1",
            Some(body.clone())
        ),
    );
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK), "{b1} {b2}");
    assert_eq!(
        [replayed(&h1), replayed(&h2)]
            .iter()
            .filter(|r| **r)
            .count(),
        1,
        "one send acts and the other replays it"
    );
    assert_eq!(b1, b2, "both answers agree");
    assert_eq!(email_events(&w).await, before + 1, "removed once");
    // The key on another body is refused, never acted.
    let (status, _, refused) = send(
        &w.router,
        "/v2/web/emails/remove",
        &w.admin,
        "r-1",
        Some(json!({"email": "erin@example.com"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{refused}");
    assert_eq!(refused["error"]["code"], "idempotency.digest_mismatch");
}
