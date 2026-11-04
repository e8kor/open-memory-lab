//! `oml-fetch` — the outbound fetch of a client ID metadata document, the
//! lead's [`DocumentFetcher`] (ADR-0031 d5; `docs/api/http-api.md`,
//! "Client identity"; ADR-0019: outbound I/O lives in an adapter).
//!
//! It resolves a host's A and AAAA answers and fetches from one address it
//! is handed, and it decides nothing: the 16-slot, per-host and
//! per-address caps and the address allow-list are
//! `oml_app::oauth_fetch::CappedFetcher`'s, run before this adapter is
//! asked to connect. Each blocking call owns the [`FetchGuard`] it is
//! handed, so its slots stay taken until the thread returns: dropping a
//! `spawn_blocking` handle, or the resolve timeout firing, stops nothing.
//! The fetch connects to the given address with no
//! second resolution, through a `ureq` agent whose resolver answers only
//! that address; it reads no proxy from the environment, follows no
//! redirect, sends `Accept-Encoding: identity`, and is cut at 5 seconds
//! overall and at 16 KiB of body. TLS verifies the certificate against the
//! web PKI roots for the URL's host, which `ureq` also sends as SNI.
//!
//! `ureq` is built with its default `gzip` feature in this workspace, so a
//! host that answers `Content-Encoding: gzip` against `identity` is decoded
//! and the 16 KiB cut applies to the decoded bytes; the 5-second cut bounds
//! such a host as any other.

pub mod signin;

use std::io::Read as _;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs as _};
use std::time::Duration;

use oml_app::ports::{DocumentFetchFailure, DocumentFetcher, FetchGuard, FetchedDocument};

/// The cut on a document's body (ADR-0031 d5).
pub const BODY_MAX_BYTES: usize = 16 * 1024;

/// The cut on a resolution, and on a fetch from connect to last byte.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// The port of every document URL: the admitted form carries none, so
/// `https`'s default (ADR-0031 d5).
const HTTPS_PORT: u16 = 443;

/// The lead's document fetcher. It holds nothing; one instance serves
/// every fetch, behind the one `CappedFetcher` the lead composes.
#[derive(Debug, Default, Clone, Copy)]
pub struct PinnedFetcher;

impl DocumentFetcher for PinnedFetcher {
    async fn resolve(
        &self,
        host: &str,
        guard: FetchGuard,
    ) -> Result<Vec<IpAddr>, DocumentFetchFailure> {
        let host = host.to_owned();
        let lookup = tokio::task::spawn_blocking(move || {
            let _guard = guard;
            (host.as_str(), HTTPS_PORT)
                .to_socket_addrs()
                .map(|answers| answers.map(|answer| answer.ip()).collect::<Vec<_>>())
        });
        match tokio::time::timeout(FETCH_TIMEOUT, lookup).await {
            Ok(Ok(Ok(answers))) => Ok(answers),
            _ => Err(DocumentFetchFailure::Failed),
        }
    }

    async fn fetch(
        &self,
        url: &str,
        address: IpAddr,
        guard: FetchGuard,
    ) -> Result<FetchedDocument, DocumentFetchFailure> {
        let url = url.to_owned();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            fetch_pinned(&url, SocketAddr::new(address, HTTPS_PORT))
        })
        .await
        .unwrap_or(Err(DocumentFetchFailure::Failed))
    }
}

/// `GET url` over a connection to `address` alone, blocking: the one code
/// path [`PinnedFetcher::fetch`] runs. It checks nothing about the URL or
/// the address — the caller has — so a test can point it at a plain-HTTP
/// stub on loopback. Anything but a `200` whose body is within
/// [`BODY_MAX_BYTES`] and arrives within [`FETCH_TIMEOUT`] is
/// [`DocumentFetchFailure::Failed`].
pub fn fetch_pinned(
    url: &str,
    address: SocketAddr,
) -> Result<FetchedDocument, DocumentFetchFailure> {
    let agent = ureq::AgentBuilder::new()
        .resolver(move |_: &str| Ok(vec![address]))
        .try_proxy_from_env(false)
        .redirects(0)
        .timeout(FETCH_TIMEOUT)
        .build();
    let response = agent
        .get(url)
        .set("Accept-Encoding", "identity")
        .call()
        .map_err(|_| DocumentFetchFailure::Failed)?;
    if response.status() != 200 {
        return Err(DocumentFetchFailure::Failed);
    }
    let max_age = max_age(response.header("Cache-Control"));
    let mut body = Vec::new();
    response
        .into_reader()
        .take(BODY_MAX_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|_| DocumentFetchFailure::Failed)?;
    if body.len() > BODY_MAX_BYTES {
        return Err(DocumentFetchFailure::Failed);
    }
    Ok(FetchedDocument { body, max_age })
}

/// A `Cache-Control` header's `max-age` in seconds, `0` when it is absent
/// or unreadable or the header also says `no-store` or `no-cache`; a value
/// past `u64` saturates (RFC 9111 §1.2.2). The store caps what it keeps at
/// 24 hours.
#[must_use]
pub fn max_age(cache_control: Option<&str>) -> u64 {
    let mut age = None;
    for directive in cache_control.unwrap_or("").split(',') {
        let directive = directive.trim().to_ascii_lowercase();
        if directive == "no-store" || directive == "no-cache" {
            return 0;
        }
        if let Some(seconds) = directive.strip_prefix("max-age=") {
            age = seconds_of(seconds);
        }
    }
    age.unwrap_or(0)
}

/// RFC 9111 delta-seconds: one or more digits, saturating.
fn seconds_of(digits: &str) -> Option<u64> {
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(digits.parse().unwrap_or(u64::MAX))
}
