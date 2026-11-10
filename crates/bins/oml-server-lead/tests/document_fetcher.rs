//! The lead's authorize page reads a client ID metadata document through
//! the fetcher its composition wires, behind the fetch caps and the address
//! allow-list (ADR-0031 d5; docs/api/http-api.md, "Client identity"). The
//! lead's start is run over a recording adapter in place of the network, so
//! the test sees that a document client reaches it, and that an answer
//! outside the allow-list is never connected to.

use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt as _;

use oml_app::domain::oauth::Issuer;
use oml_app::ed25519_dalek::SigningKey;
use oml_app::ports::{
    DocumentFetchFailure, DocumentFetcher, FetchGuard, FetchedDocument, IdGen as _,
};
use oml_pg::PgStore;
use oml_server_lead::{OsIds, start_lead_with};
use oml_test_pg::{DB_N, drop_db, mint_database, owner_url, run_tag, serving_url};

const ISSUER: &str = "https://oml.example.com";
const CALLBACK: &str = "https://claude.ai/api/mcp/auth_callback";
/// RFC 7636 appendix B's S256 challenge.
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
/// A public address the allow-list admits.
const PUBLIC: [u8; 4] = [93, 184, 216, 34];

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

/// What the adapter was asked: each resolved host, and each fetch's URL
/// and address.
#[derive(Default)]
struct Asked {
    resolved: Vec<String>,
    fetched: Vec<(String, IpAddr)>,
}

/// The outbound adapter in place of the network: `public.example` resolves
/// to a public address and serves a valid document, `loopback.example`
/// resolves to `127.0.0.1`.
#[derive(Clone, Default)]
struct Recording(Arc<Mutex<Asked>>);

impl DocumentFetcher for Recording {
    async fn resolve(
        &self,
        host: &str,
        _guard: FetchGuard,
    ) -> Result<Vec<IpAddr>, DocumentFetchFailure> {
        self.0
            .lock()
            .expect("unpoisoned")
            .resolved
            .push(host.to_owned());
        match host {
            "public.example" => Ok(vec![IpAddr::from(PUBLIC)]),
            _ => Ok(vec![IpAddr::from([127, 0, 0, 1])]),
        }
    }

    async fn fetch(
        &self,
        url: &str,
        address: IpAddr,
        _guard: FetchGuard,
    ) -> Result<FetchedDocument, DocumentFetchFailure> {
        self.0
            .lock()
            .expect("unpoisoned")
            .fetched
            .push((url.to_owned(), address));
        Ok(FetchedDocument {
            body: json!({
                "client_id": url,
                "client_name": "Claude",
                "redirect_uris": [CALLBACK],
            })
            .to_string()
            .into_bytes(),
            max_age: 300,
        })
    }
}

fn pct(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

async fn authorize(router: &Router, client_id: &str) -> (StatusCode, String) {
    let query = [
        ("client_id", client_id),
        ("redirect_uri", CALLBACK),
        ("response_type", "code"),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
        ("state", "st"),
        ("resource", &format!("{ISSUER}/mcp")),
        ("scope", "mcp"),
    ]
    .iter()
    .map(|(name, value)| format!("{name}={}", pct(value)))
    .collect::<Vec<_>>()
    .join("&");
    let response = router
        .clone()
        .oneshot(
            Request::get(format!("/oauth/authorize?{query}"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("the router answers");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a readable body");
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn the_leads_authorize_page_fetches_a_document_client_through_its_composed_fetcher_under_the_allow_list()
 {
    let db_name = format!(
        "oml_test_docs_{}_{}_{}",
        std::process::id(),
        DB_N.fetch_add(1, Ordering::SeqCst),
        run_tag()
    );
    if !mint_database(&db_name).await {
        return;
    }
    let store = serving_store(&db_name).await;
    let recording = Recording::default();
    let started = start_lead_with(
        &store,
        SigningKey::from_bytes(&OsIds.new_secret()),
        Some(Issuer::parse(ISSUER).expect("a public origin")),
        recording.clone(),
        oml_transport_http::traffic::Traffic::default(),
        None,
        oml_app::domain::brief::BRIEF_MAX_TOKENS_DEFAULT,
        None,
        None,
    )
    .await
    .expect("the start reads the store");

    let public = "https://public.example/oauth/client.json";
    let (status, page) = authorize(&started.router, public).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(
        page.contains(r#"<p class="verified">verified by <b>public.example</b></p>"#),
        "{page}"
    );

    // An answer outside the allow-list is resolved and never connected to.
    let loopback = "https://loopback.example/oauth/client.json";
    let (_, page) = authorize(&started.router, loopback).await;
    assert!(page.contains("oauth.client_invalid"), "{page}");
    {
        let asked = recording.0.lock().expect("unpoisoned");
        assert_eq!(asked.resolved, ["public.example", "loopback.example"]);
        assert_eq!(asked.fetched, [(public.to_owned(), IpAddr::from(PUBLIC))]);
    }

    drop(started);
    drop(store);
    drop_db(&db_name).await;
}
