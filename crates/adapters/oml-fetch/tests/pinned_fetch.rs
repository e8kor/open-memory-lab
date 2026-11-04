//! The pinned fetch against a plain-TCP stub on loopback (ADR-0031 d5;
//! `docs/api/http-api.md`, "Client identity"). The stub speaks HTTP/1.1
//! without TLS, so these tests prove the pin, the redirect refusal, the
//! encoding, the cuts and the `Cache-Control` reading of the one code path
//! the adapter runs; a real TLS fetch to a public host is not exercised by
//! any test.

use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oml_app::ports::{DocumentFetchFailure, FetchedDocument};
use oml_fetch::{BODY_MAX_BYTES, fetch_pinned, max_age};

/// A stub that answers every connection with `respond`, recording each
/// request's head.
struct Stub {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    fn serve(respond: impl Fn(&mut std::net::TcpStream) + Send + 'static) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind loopback");
        let address = listener.local_addr().expect("address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    match stream.read(&mut byte) {
                        Ok(1) => head.push(byte[0]),
                        _ => break,
                    }
                }
                recorded
                    .lock()
                    .expect("requests")
                    .push(String::from_utf8_lossy(&head).into_owned());
                respond(&mut stream);
            }
        });
        Self { address, requests }
    }

    fn answering(response: Vec<u8>) -> Self {
        Self::serve(move |stream| {
            let _ = stream.write_all(&response);
        })
    }

    /// A URL on a host no resolver knows, so a fetch that resolved it
    /// again could not connect.
    fn url(&self) -> String {
        format!("http://doc.invalid:{}/metadata.json", self.address.port())
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }
}

fn ok(headers: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

#[test]
fn the_connection_goes_to_the_pinned_address_and_the_host_is_never_resolved() {
    let stub = Stub::answering(ok("Cache-Control: max-age=600\r\n", b"{}"));

    assert_eq!(
        fetch_pinned(&stub.url(), stub.address),
        Ok(FetchedDocument {
            body: b"{}".to_vec(),
            max_age: 600,
        })
    );
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    // The host is the URL's, though the connection went to the pin.
    assert!(
        requests[0]
            .to_ascii_lowercase()
            .contains(&format!("host: doc.invalid:{}\r\n", stub.address.port())),
        "{}",
        requests[0]
    );
}

#[test]
fn the_request_asks_for_identity_encoding() {
    let stub = Stub::answering(ok("", b"{}"));

    assert!(fetch_pinned(&stub.url(), stub.address).is_ok());
    let head = stub.requests()[0].to_ascii_lowercase();
    assert!(head.contains("accept-encoding: identity\r\n"), "{head}");
    assert!(!head.contains("gzip"), "{head}");
}

#[test]
fn a_body_past_16_kib_is_refused() {
    assert_eq!(BODY_MAX_BYTES, 16 * 1024);
    let at_bound = Stub::answering(ok("", &vec![b' '; BODY_MAX_BYTES]));
    assert_eq!(
        fetch_pinned(&at_bound.url(), at_bound.address).map(|f| f.body.len()),
        Ok(BODY_MAX_BYTES)
    );

    let past = Stub::answering(ok("", &vec![b' '; BODY_MAX_BYTES + 1]));
    assert_eq!(
        fetch_pinned(&past.url(), past.address),
        Err(DocumentFetchFailure::Failed)
    );
    // Without a length the cut is on the bytes received.
    let mut unsized_body = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    unsized_body.extend(vec![b' '; BODY_MAX_BYTES + 1]);
    let streamed = Stub::answering(unsized_body);
    assert_eq!(
        fetch_pinned(&streamed.url(), streamed.address),
        Err(DocumentFetchFailure::Failed)
    );
}

#[test]
fn a_redirect_is_not_followed() {
    let stub = Stub::answering(
        b"HTTP/1.1 302 Found\r\nLocation: http://doc.invalid/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
    );

    assert_eq!(
        fetch_pinned(&stub.url(), stub.address),
        Err(DocumentFetchFailure::Failed)
    );
    assert_eq!(stub.requests().len(), 1);
}

#[test]
fn an_answer_other_than_200_is_a_failed_fetch() {
    for status in [
        "204 No Content",
        "404 Not Found",
        "500 Internal Server Error",
    ] {
        let stub = Stub::answering(
            format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .into_bytes(),
        );
        assert_eq!(
            fetch_pinned(&stub.url(), stub.address),
            Err(DocumentFetchFailure::Failed),
            "{status}"
        );
    }
}

#[test]
fn a_body_dripped_past_5_seconds_is_cut_though_every_read_is_quick() {
    let stub = Stub::serve(|stream| {
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\n\r\n");
        for _ in 0..64 {
            if stream.write_all(b" ").is_err() {
                return;
            }
            thread::sleep(Duration::from_millis(250));
        }
    });

    let started = Instant::now();
    assert_eq!(
        fetch_pinned(&stub.url(), stub.address),
        Err(DocumentFetchFailure::Failed)
    );
    let took = started.elapsed();
    assert!(
        took >= Duration::from_secs(4) && took < Duration::from_secs(8),
        "{took:?}"
    );
}

#[test]
fn cache_control_gives_the_max_age_and_no_store_or_no_cache_give_none() {
    assert_eq!(max_age(Some("max-age=600")), 600);
    assert_eq!(max_age(Some("public, MAX-AGE=86400")), 86_400);
    assert_eq!(max_age(Some("max-age=99999999999999999999")), u64::MAX);
    assert_eq!(max_age(Some("no-store, max-age=600")), 0);
    assert_eq!(max_age(Some("max-age=600, no-cache")), 0);
    assert_eq!(max_age(Some("max-age=soon")), 0);
    assert_eq!(max_age(Some("private")), 0);
    assert_eq!(max_age(None), 0);
}
