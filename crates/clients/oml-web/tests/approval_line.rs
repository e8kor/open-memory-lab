//! What a memory's card says of its approval (ADR-0077): when it was
//! approved and whether a person or a reviewer agent approved it, a
//! self-review marked as one, and nothing where the API serves nothing,
//! against an API double that lists the memories a test names.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use http::{Request, Response, StatusCode, header};
use oml_sdk::{Sending, Transport};
use oml_web::middleware::body_bytes;
use oml_web::{Layers, SESSION_COOKIE, Web, api, router};
use serde_json::{Value, json};
use tower::ServiceExt as _;

const SECRET: &str = "0123456789abcdef0123456789abcdef";
const TOKEN: &str = "the-session-token";
const ORIGIN: &str = "https://memory.example";
const ROOT: &str = "6f1c2d4e-0000-4000-8000-000000000001";

struct Double {
    memories: Value,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Transport for Double {
    fn send(&self, request: Request<Vec<u8>>) -> Sending<'_> {
        let path = request.uri().to_string();
        self.seen.lock().unwrap().push(path.clone());
        let body = if path.starts_with("/v2/whoami") {
            json!({
                "key_id": "k1", "name": "alice-web", "role": "user", "principal": "alice",
                "held_by": "human", "tenant": {"id": "t1", "name": "oml"},
                "default_workarea": null, "crew": null,
                "user": {"id": ROOT, "name": "alice"}, "grants": [],
            })
        } else if path.starts_with("/v1/memories") {
            json!({"items": self.memories, "next_cursor": null})
        } else {
            json!({"items": [], "next_cursor": null})
        };
        let response = Response::builder()
            .status(200)
            .body(body.to_string().into_bytes())
            .unwrap();
        Box::pin(async move { Ok(response) })
    }
}

/// A served version as `GET /v1/memories` lists it.
fn memory(id: &str, state: &str, principal: Option<&str>, approval: Option<(&str, bool)>) -> Value {
    json!({
        "artifact_id": id, "claim_key": format!("claim-{id}"), "version": 1, "tip": true,
        "workarea_path": "universe.alice", "state": state, "principal": principal,
        "author_key_id": null, "created_at": "2026-11-09T08:00:00.000000Z",
        "promoted_at": (state == "promoted").then_some("2026-11-10T09:30:00.000000Z"),
        "text": format!("text of {id}"),
        "approved_at": approval.map(|_| "2026-11-10T09:30:00.000000Z"),
        "approver": approval.map(|(held_by, same_user)| json!({
            "held_by": held_by, "same_user": same_user,
        })),
    })
}

/// The page of `/web/memories` over `memories`.
async fn list(memories: Value) -> String {
    let app = router(Web {
        layers: Layers {
            public_origin: Some(ORIGIN.to_owned()),
            log: Arc::new(|_| {}),
        },
        api: api(
            Arc::new(Double {
                memories,
                seen: Arc::new(Mutex::new(Vec::new())),
            }),
            SECRET,
        )
        .unwrap(),
    });
    let response = app
        .oneshot(
            Request::get("/web/memories")
                .header("x-request-id", "page-7")
                .header(header::COOKIE, format!("{SESSION_COOKIE}={TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    String::from_utf8_lossy(&body_bytes(response.into_body()).await.unwrap()).into_owned()
}

/// The card of the memory `id`, up to the next card.
fn card<'a>(page: &'a str, id: &str) -> &'a str {
    let start = page
        .find(&format!(r#"id="m-{id}""#))
        .unwrap_or_else(|| panic!("no card of {id} in {page}"));
    let rest = &page[start + 1..];
    let end = rest.find(r#"<li><details"#).unwrap_or(rest.len());
    &rest[..end]
}

/// The review step's sentence on the card of `id`.
fn review_step(page: &str, id: &str) -> String {
    let card = card(page, id);
    let marker = r#"<span class="step">Reviewed</span><span>"#;
    let from = card
        .find(marker)
        .unwrap_or_else(|| panic!("no review step in {card}"))
        + marker.len();
    let to = card[from..].find("</span>").unwrap() + from;
    card[from..to].to_owned()
}

#[tokio::test]
async fn a_card_says_when_a_person_or_a_reviewer_agent_approved_it() {
    let page = list(json!([
        memory(
            "by-person",
            "promoted",
            Some("alice"),
            Some(("human", false))
        ),
        memory(
            "by-agent",
            "promoted",
            Some("alice"),
            Some(("agent", false))
        ),
    ]))
    .await;
    assert_eq!(
        review_step(&page, "by-person"),
        "approved by a person on 2026-11-10"
    );
    assert_eq!(
        review_step(&page, "by-agent"),
        "approved by a reviewer agent on 2026-11-10"
    );
    assert!(!page.contains("self-review"), "{page}");
    assert!(!page.contains("outside its lineage"), "{page}");
}

#[tokio::test]
async fn a_person_approving_their_own_agents_write_is_marked_a_self_review() {
    let page = list(json!([memory(
        "own",
        "promoted",
        Some("alice"),
        Some(("human", true))
    )]))
    .await;
    let step = review_step(&page, "own");
    assert!(
        step.starts_with("approved by a person on 2026-11-10 · self-review"),
        "{step}"
    );
}

#[tokio::test]
async fn a_card_the_api_serves_no_approval_for_says_approved_and_nothing_more() {
    // A `universe` row served to a reader outside its authors has no
    // principal, no approved_at and no approver (ADR-0077 d4).
    let page = list(json!([memory("shared", "promoted", None, None)])).await;
    assert_eq!(review_step(&page, "shared"), "approved");
    assert!(!page.contains("self-review"), "{page}");
}

#[tokio::test]
async fn an_unreviewed_card_names_no_approval() {
    let page = list(json!([memory("waiting", "held", Some("alice"), None)])).await;
    assert_eq!(
        review_step(&page, "waiting"),
        "held until someone else reviews it"
    );
    assert!(!page.contains("approved by"), "{page}");
}
