//! The fetch caps and the address allow-list of a client ID metadata
//! document fetch (ADR-0031 d5; `docs/api/http-api.md`, "Client
//! identity"), over a resolver and fetcher double: no test here touches a
//! network.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Semaphore;

use oml_app::App;
use oml_app::domain::EventTime;
use oml_app::domain::Sha256Hash;
use oml_app::domain::oauth::{CLAUDE_CALLBACK, ClientInvalid, Issuer, Refusal};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, PlainSealer, SeqIdGen};
use oml_app::oauth_fetch::{CappedFetcher, FETCH_SLOTS};
use oml_app::ports::{
    ClientDocumentFetcher as _, ConnectionStore as _, DocumentFetchFailure, DocumentFetcher,
    FetchedDocument,
};
use oml_app::types::{AuthorizeOutcome, AuthorizeParams, ClientDescription};

const ISSUER: &str = "https://oml.example.com";
const DOC_URL: &str = "https://client.example.com/oauth/metadata.json";
const DOC_HOST: &str = "client.example.com";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
/// The first clock reading of a fresh app.
const T0: &str = "2026-10-29T12:00:00.000000Z";
/// Long enough that a fetch which waited for a slot fails the test rather
/// than hanging it.
const NO_WAIT: Duration = Duration::from_secs(5);

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn app(store: MemStore) -> TestApp {
    App::new(
        store,
        MemSigner::new([7; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_oauth_issuer(Issuer::parse(ISSUER).expect("a public origin"))
}

fn ip(address: &str) -> IpAddr {
    address.parse().expect("an address")
}

/// A public address no test answer shares with another, `n` below 250.
fn public(n: usize) -> IpAddr {
    ip(&format!("93.184.216.{n}"))
}

fn document(url: &str) -> Vec<u8> {
    format!(
        r#"{{"client_id":"{url}","client_name":"Claude","redirect_uris":["{CLAUDE_CALLBACK}"]}}"#
    )
    .into_bytes()
}

/// Resolver answers by host and a fetch that serves each URL's document,
/// recording every connection; a fetch waits for a permit on `gate` when
/// one is set, so a test holds slots for as long as it likes.
#[derive(Default)]
struct Outbound {
    answers: Mutex<BTreeMap<String, Vec<IpAddr>>>,
    resolved: AtomicUsize,
    connected: Mutex<Vec<(String, IpAddr)>>,
    gate: Option<Arc<Semaphore>>,
}

impl Outbound {
    fn gated(gate: &Arc<Semaphore>) -> Self {
        Self {
            gate: Some(Arc::clone(gate)),
            ..Self::default()
        }
    }

    fn answer(&self, host: &str, answers: &[IpAddr]) {
        self.answers
            .lock()
            .expect("answers")
            .insert(host.to_owned(), answers.to_vec());
    }

    fn connections(&self) -> Vec<(String, IpAddr)> {
        self.connected.lock().expect("connections").clone()
    }
}

/// The fetcher the caps hold: a handle on one [`Outbound`] the test keeps.
struct Shared(Arc<Outbound>);

impl DocumentFetcher for Shared {
    async fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, DocumentFetchFailure> {
        let outbound = &self.0;
        outbound.resolved.fetch_add(1, Ordering::SeqCst);
        outbound
            .answers
            .lock()
            .expect("answers")
            .get(host)
            .cloned()
            .ok_or(DocumentFetchFailure::Failed)
    }

    async fn fetch(
        &self,
        url: &str,
        address: IpAddr,
    ) -> Result<FetchedDocument, DocumentFetchFailure> {
        let outbound = &self.0;
        outbound
            .connected
            .lock()
            .expect("connections")
            .push((url.to_owned(), address));
        if let Some(gate) = &outbound.gate {
            gate.acquire().await.expect("gate open").forget();
        }
        Ok(FetchedDocument {
            body: document(url),
            max_age: 3_600,
        })
    }
}

/// A valid authorize request from the document client at `DOC_URL`.
fn params() -> AuthorizeParams {
    AuthorizeParams {
        client_id: Some(DOC_URL.to_owned()),
        redirect_uri: Some(CLAUDE_CALLBACK.to_owned()),
        response_type: Some("code".to_owned()),
        code_challenge: Some(CHALLENGE.to_owned()),
        code_challenge_method: Some("S256".to_owned()),
        state: Some("opaque state".to_owned()),
        resource: Some(format!("{ISSUER}/mcp")),
        scope: Some("mcp".to_owned()),
        repeated: false,
    }
}

async fn authorize(app: &TestApp, fetcher: &CappedFetcher<Shared>) -> AuthorizeOutcome {
    app.authorize_request(params(), fetcher, &PlainSealer)
        .await
        .expect("an authorize request is answered")
}

fn refused(reason: ClientInvalid) -> AuthorizeOutcome {
    AuthorizeOutcome::PageRefusal(Refusal::ClientInvalid(reason))
}

/// Waits until `count` fetches have connected and are held at the gate.
async fn held(outbound: &Outbound, count: usize) {
    tokio::time::timeout(NO_WAIT, async {
        while outbound.connections().len() < count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the held fetches connect");
}

#[tokio::test]
async fn a_document_host_that_resolves_to_a_loopback_address_is_refused_before_any_connection() {
    for loopback in [ip("127.0.0.1"), ip("::1")] {
        let outbound = Arc::new(Outbound::default());
        outbound.answer(DOC_HOST, &[loopback]);
        let fetcher = CappedFetcher::new(Shared(Arc::clone(&outbound)));

        assert_eq!(
            fetcher.fetch(DOC_URL).await,
            Err(DocumentFetchFailure::Failed),
            "{loopback}"
        );
        assert_eq!(
            authorize(&app(MemStore::new()), &fetcher).await,
            refused(ClientInvalid::Fetch)
        );
        assert_eq!(outbound.connections(), [], "{loopback}");
    }
}

#[tokio::test]
async fn one_non_admitted_answer_among_admitted_ones_refuses_the_fetch() {
    for stray in [ip("10.0.0.1"), ip("169.254.169.254"), ip("fd00::1")] {
        let outbound = Arc::new(Outbound::default());
        outbound.answer(
            DOC_HOST,
            &[public(1), ip("2606:4700:4700::1111"), stray, public(2)],
        );
        let fetcher = CappedFetcher::new(Shared(Arc::clone(&outbound)));

        assert_eq!(
            fetcher.fetch(DOC_URL).await,
            Err(DocumentFetchFailure::Failed),
            "{stray}"
        );
        assert_eq!(outbound.connections(), [], "{stray}");
    }
    // The same answers without the stray one are fetched from the first.
    let outbound = Arc::new(Outbound::default());
    outbound.answer(DOC_HOST, &[public(1), public(2)]);
    let fetcher = CappedFetcher::new(Shared(Arc::clone(&outbound)));
    assert!(fetcher.fetch(DOC_URL).await.is_ok());
    assert_eq!(outbound.connections(), [(DOC_URL.to_owned(), public(1))]);
}

#[tokio::test]
async fn a_seventeenth_concurrent_fetch_is_fetch_busy_when_no_cached_copy_exists() {
    let gate = Arc::new(Semaphore::new(0));
    let outbound = Arc::new(Outbound::gated(&gate));
    outbound.answer(DOC_HOST, &[public(200)]);
    let fetcher = Arc::new(CappedFetcher::new(Shared(Arc::clone(&outbound))));
    let mut running = Vec::new();
    for n in 0..FETCH_SLOTS {
        let host = format!("client{n}.example.com");
        outbound.answer(&host, &[public(n + 1)]);
        let fetcher = Arc::clone(&fetcher);
        running.push(tokio::spawn(async move {
            fetcher
                .fetch(&format!("https://{host}/metadata.json"))
                .await
        }));
    }
    held(&outbound, FETCH_SLOTS).await;
    let resolved = outbound.resolved.load(Ordering::SeqCst);

    let app = app(MemStore::new());
    let busy = tokio::time::timeout(NO_WAIT, authorize(&app, &fetcher))
        .await
        .expect("a seventeenth fetch waits for no slot");
    assert_eq!(busy, refused(ClientInvalid::FetchBusy));
    // Refused before its host was resolved, so it never connected.
    assert_eq!(outbound.resolved.load(Ordering::SeqCst), resolved);
    assert_eq!(outbound.connections().len(), FETCH_SLOTS);

    gate.add_permits(FETCH_SLOTS + 1);
    for fetch in running {
        assert!(fetch.await.expect("joined").is_ok());
    }
    // The slots are given back: the same request now fetches.
    assert!(matches!(
        authorize(&app, &fetcher).await,
        AuthorizeOutcome::Page(_)
    ));
}

#[tokio::test]
async fn a_second_fetch_to_one_host_waits_for_no_slot_and_is_fetch_busy() {
    let gate = Arc::new(Semaphore::new(0));
    let outbound = Arc::new(Outbound::gated(&gate));
    outbound.answer(DOC_HOST, &[public(1)]);
    let fetcher = Arc::new(CappedFetcher::new(Shared(Arc::clone(&outbound))));
    let first = {
        let fetcher = Arc::clone(&fetcher);
        tokio::spawn(async move { fetcher.fetch(DOC_URL).await })
    };
    held(&outbound, 1).await;

    let other_path = "https://client.example.com/other/metadata.json";
    for url in [DOC_URL, other_path] {
        let second = tokio::time::timeout(NO_WAIT, fetcher.fetch(url))
            .await
            .expect("a second fetch to one host waits for no slot");
        assert_eq!(second, Err(DocumentFetchFailure::Busy), "{url}");
    }
    assert_eq!(outbound.connections().len(), 1);

    gate.add_permits(1);
    assert!(first.await.expect("joined").is_ok());
}

#[tokio::test]
async fn two_host_names_on_one_address_hold_one_slot_and_the_second_is_fetch_busy() {
    let gate = Arc::new(Semaphore::new(0));
    let outbound = Arc::new(Outbound::gated(&gate));
    outbound.answer("slow-a.example.com", &[public(9)]);
    outbound.answer("slow-b.example.com", &[public(9)]);
    let fetcher = Arc::new(CappedFetcher::new(Shared(Arc::clone(&outbound))));
    let first = {
        let fetcher = Arc::clone(&fetcher);
        tokio::spawn(async move { fetcher.fetch("https://slow-a.example.com/doc.json").await })
    };
    held(&outbound, 1).await;

    let second = tokio::time::timeout(
        NO_WAIT,
        fetcher.fetch("https://slow-b.example.com/doc.json"),
    )
    .await
    .expect("a second fetch to one address waits for no slot");
    assert_eq!(second, Err(DocumentFetchFailure::Busy));
    assert_eq!(outbound.connections().len(), 1);

    gate.add_permits(2);
    assert!(first.await.expect("joined").is_ok());
    // Its host slot was given back with its address slot.
    assert!(
        fetcher
            .fetch("https://slow-b.example.com/doc.json")
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn a_stale_cached_copy_is_served_when_the_fetch_fails_within_24_hours() {
    // Fetched two hours before T0 under an hour's lifetime: stale, kept.
    let t0 = EventTime::parse(T0).expect("a time");
    let fetched_at = EventTime::from_unix_micros(t0.unix_micros() - 2 * 3_600 * 1_000_000)
        .expect("a time before T0");
    let body = r#"{"cached":true}"#;
    let store = MemStore::new();
    store
        .upsert_cimd(
            ClientDescription {
                client_id: DOC_URL.to_owned(),
                client_name: "Cached Claude".to_owned(),
                redirect_uris: vec![CLAUDE_CALLBACK.to_owned()],
                metadata_digest: Sha256Hash::digest(body.as_bytes()),
            },
            &fetched_at,
            3_600,
        )
        .await
        .expect("a cached copy");
    // The host now answers loopback, so the fetch fails within its bounds.
    let outbound = Arc::new(Outbound::default());
    outbound.answer(DOC_HOST, &[ip("127.0.0.1")]);
    let fetcher = CappedFetcher::new(Shared(Arc::clone(&outbound)));

    match authorize(&app(store.clone()), &fetcher).await {
        AuthorizeOutcome::Page(page) => assert_eq!(page.client_name, "Cached Claude"),
        other => panic!("the stale copy is served, not {other:?}"),
    }
    assert_eq!(outbound.resolved.load(Ordering::SeqCst), 1);
    assert_eq!(outbound.connections(), []);
    let row = store.oauth_client(DOC_URL).await.expect("read");
    assert_eq!(row.expect("kept").fetched_at, Some(fetched_at));
}
