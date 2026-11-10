//! The scaffold's layers through the real router (ADR-0070 d3).

use std::sync::{Arc, Mutex};

use axum::body::Body;
use http::{Request, StatusCode, header};
use oml_sdk::{Sending, Transport, TransportError};
use oml_web::middleware::{HUMAN_ONLY, body_bytes, content_security_policy};
use oml_web::{HEALTH_PATH, Layers, SESSION_COOKIE, Web, api, router};
use tower::ServiceExt;

const ORIGIN: &str = "https://memory.example";
const SECRET: &str = "0123456789abcdef0123456789abcdef";

/// An API no call reaches: these layers answer before any page calls it.
struct Nowhere;

impl Transport for Nowhere {
    fn send(&self, _: http::Request<Vec<u8>>) -> Sending<'_> {
        Box::pin(async {
            Err(TransportError::Unreachable(
                "no API in this test".to_owned(),
            ))
        })
    }
}

fn web(layers: Layers) -> axum::Router {
    router(Web {
        layers,
        api: api(Arc::new(Nowhere), SECRET).unwrap(),
    })
}

fn rig() -> (axum::Router, Arc<Mutex<Vec<String>>>) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = lines.clone();
    let layers = Layers {
        public_origin: Some(ORIGIN.to_owned()),
        log: Arc::new(move |line| sink.lock().unwrap().push(line)),
    };
    (web(layers), lines)
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&body_bytes(response.into_body()).await.unwrap()).unwrap()
}

#[tokio::test]
async fn the_health_probe_answers_ok_without_calling_the_api() {
    let (app, _) = rig();
    let response = app
        .oneshot(Request::get(HEALTH_PATH).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "oml-web");
}

#[tokio::test]
async fn a_request_id_the_rule_admits_is_echoed_and_one_it_refuses_is_replaced() {
    let (app, _) = rig();
    let response = app
        .clone()
        .oneshot(
            Request::get(HEALTH_PATH)
                .header("x-request-id", "page-42")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["x-request-id"], "page-42");
    assert_eq!(response.headers()["x-correlation-id"], "page-42");

    let response = app
        .oneshot(
            Request::get(HEALTH_PATH)
                .header("x-request-id", "two words")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let id = response.headers()["x-request-id"].to_str().unwrap();
    assert!(id.starts_with("req_") && id.len() == 36, "{id}");
}

#[tokio::test]
async fn every_request_writes_one_line_naming_the_route_template_and_no_header_value() {
    let (app, lines) = rig();
    app.oneshot(
        Request::get(HEALTH_PATH)
            .header("x-request-id", "line-1")
            .header(header::COOKIE, "__Host-oml_session=secret-token")
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    let lines = lines.lock().unwrap();
    assert_eq!(lines.len(), 1);
    let line: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(line["msg"], "http_request");
    assert_eq!(line["route"], HEALTH_PATH);
    assert_eq!(line["request_id"], "line-1");
    assert_eq!(line["status"], 200);
    assert!(!lines[0].contains("secret-token"));
}

#[tokio::test]
async fn an_authorization_header_on_a_page_route_is_refused_as_human_only_and_still_logged() {
    let (app, lines) = rig();
    let response = app
        .oneshot(
            Request::get("/web")
                .header(header::AUTHORIZATION, "Bearer oml_agent_key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let rid = response.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let body = json(response).await;
    assert_eq!(body["error"]["code"], "access.verb_ceiling");
    assert_eq!(body["error"]["details"]["reason"], HUMAN_ONLY);
    assert_eq!(body["request_id"], rid);
    let lines = lines.lock().unwrap();
    assert_eq!(lines.len(), 1);
    assert!(!lines[0].contains("oml_agent_key"));
}

#[tokio::test]
async fn a_post_from_another_origin_or_with_none_is_refused() {
    let (app, _) = rig();
    for origin in [Some("https://evil.example"), None] {
        let mut req = Request::post("/web");
        if let Some(origin) = origin {
            req = req.header(header::ORIGIN, origin);
        }
        let response = app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{origin:?}");
        assert_eq!(
            json(response).await["error"]["code"],
            "oauth.origin_refused"
        );
    }
}

fn form_post(origin: &str, cookie: Option<&str>, body: &str) -> Request<Body> {
    let mut req = Request::post("/web/nothing").header(header::ORIGIN, origin);
    if let Some(cookie) = cookie {
        req = req.header(header::COOKIE, format!("{SESSION_COOKIE}={cookie}"));
    }
    req.body(Body::from(body.to_owned())).unwrap()
}

#[tokio::test]
async fn a_post_from_the_public_origin_with_the_sessions_csrf_token_passes_layer_four() {
    let (app, _) = rig();
    let csrf = oml_web_credential::csrf_token("tok");
    let response = app
        .oneshot(form_post(
            ORIGIN,
            Some("tok"),
            &format!("action=x&csrf_token={csrf}"),
        ))
        .await
        .unwrap();
    // No page takes this post, so the request reaches the fallback.
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_post_without_the_sessions_csrf_token_is_refused_as_the_lead_refuses_it() {
    let (app, _) = rig();
    let other = oml_web_credential::csrf_token("another-session");
    for body in [
        "action=x".to_owned(),
        format!("csrf_token={other}"),
        format!(
            "csrf_token={}&csrf_token={}",
            oml_web_credential::csrf_token("tok"),
            oml_web_credential::csrf_token("tok")
        ),
    ] {
        let response = app
            .clone()
            .oneshot(form_post(ORIGIN, Some("tok"), &body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        let answer = json(response).await;
        assert_eq!(answer["error"]["code"], "request.validation");
        assert_eq!(answer["error"]["details"]["field"], "csrf_token");
    }
}

#[tokio::test]
async fn a_post_without_a_session_goes_to_sign_in() {
    let (app, _) = rig();
    let response = app.oneshot(form_post(ORIGIN, None, "")).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    // A form's page is its path: the query a GET carried is not resent.
    assert_eq!(
        response.headers()[header::LOCATION],
        "/web/sign-in?next=%2Fweb%2Fnothing"
    );
}

#[tokio::test]
async fn sec_fetch_site_same_origin_is_admitted_as_the_lead_admits_it() {
    let (app, _) = rig();
    let csrf = oml_web_credential::csrf_token("tok");
    let response = app
        .oneshot(
            Request::post("/web/nothing")
                .header("sec-fetch-site", "same-origin")
                .header(header::COOKIE, format!("{SESSION_COOKIE}=tok"))
                .body(Body::from(format!("csrf_token={csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_page_route_without_a_session_cookie_goes_to_sign_in_without_calling_the_api() {
    let (app, lines) = rig();
    let response = app
        .oneshot(
            Request::get("/web?view=guardrails")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers()[header::LOCATION],
        "/web/sign-in?next=%2Fweb%3Fview%3Dguardrails"
    );
    let line: serde_json::Value = serde_json::from_str(&lines.lock().unwrap()[0]).unwrap();
    assert!(line.get("api_request_ids").is_none(), "{line}");
}

#[tokio::test]
async fn while_the_public_origin_is_unknown_every_post_is_refused() {
    let layers = Layers {
        public_origin: None,
        log: Arc::new(|_| {}),
    };
    let response = web(layers)
        .oneshot(
            Request::post("/web")
                .header(header::ORIGIN, ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_csp_names_the_stylesheets_digest_and_loads_nothing_else() {
    let csp = content_security_policy();
    assert!(
        csp.starts_with("default-src 'none'; style-src 'sha256-"),
        "{csp}"
    );
    assert!(csp.contains(&oml_page::web::style_source()), "{csp}");
    for absent in ["script-src", "frame-src", "https://"] {
        assert!(!csp.contains(absent), "{absent}: {csp}");
    }
    for present in [
        "form-action 'self';",
        "frame-ancestors 'none'",
        "base-uri 'none'",
    ] {
        assert!(csp.contains(present), "{present}: {csp}");
    }
}

#[tokio::test]
async fn every_answer_carries_the_csp_and_the_no_store_headers_refusals_included() {
    let (app, _) = rig();
    for req in [
        Request::get(HEALTH_PATH).body(Body::empty()).unwrap(),
        Request::get("/web")
            .header(header::AUTHORIZATION, "Bearer x")
            .body(Body::empty())
            .unwrap(),
        Request::get("/web/nothing").body(Body::empty()).unwrap(),
    ] {
        let response = app.clone().oneshot(req).await.unwrap();
        let h = response.headers();
        assert_eq!(
            h[header::CONTENT_SECURITY_POLICY],
            content_security_policy().as_str()
        );
        assert_eq!(h[header::CACHE_CONTROL], "no-store");
        assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(h[header::REFERRER_POLICY], "same-origin");
    }
}
