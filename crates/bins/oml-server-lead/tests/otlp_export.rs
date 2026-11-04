//! OTLP trace export is off unless `server.otlp_endpoint` names a
//! collector: `OTEL_EXPORTER_OTLP_ENDPOINT` alone exports nothing, and set,
//! the lead posts protobuf spans to `<endpoint>/v1/traces` with no
//! authentication header, the request's span the child of a valid
//! `traceparent` and its trace id on the request line (ADR-0042 d13).

use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::Request;
use prost::Message as _;
use serde_json::Value;
use tower::ServiceExt as _;

use oml_app::App;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_server_lead::otlp::{Endpoint, Export};
use oml_server_lead::{Config, exporting_subscriber};
use oml_transport_http::ApiState;
use oml_transport_http::traffic::{ScopeObserver, Traffic, TrustedProxies};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;

const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("unpoisoned").extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn router(traffic: Traffic) -> Router {
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new([5; 32]),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_observer(Arc::new(ScopeObserver));
    ApiState::new(
        Arc::new(app),
        store,
        Arc::new(|| Box::pin(async { true })),
        true,
        None,
    )
    .with_traffic(traffic)
    .into_router()
}

async fn whoami(router: &Router) {
    let response = router
        .clone()
        .oneshot(
            Request::get("/v2/whoami")
                .header("traceparent", format!("00-{TRACE}-00f067aa0ba902b7-01"))
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("the router answers");
    to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
}

/// One HTTP request a collector receives: its head lines and its body.
struct Received {
    head: Vec<String>,
    body: Vec<u8>,
}

/// A collector on an ephemeral port that answers each POST `200` and
/// keeps what it received.
fn collector() -> (String, Arc<Mutex<Vec<Received>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let url = format!("http://{}", listener.local_addr().expect("an address"));
    let received = Arc::new(Mutex::new(Vec::new()));
    let keep = Arc::clone(&received);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let mut reader = BufReader::new(stream.try_clone().expect("a second handle"));
            let mut head = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let line = line.trim_end().to_owned();
                if line.is_empty() {
                    break;
                }
                head.push(line);
            }
            let length = head
                .iter()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                })
                .unwrap_or(0);
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let mut stream = stream;
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            keep.lock()
                .expect("unpoisoned")
                .push(Received { head, body });
        }
    });
    (url, received)
}

#[tokio::test]
async fn otel_exporter_otlp_endpoint_alone_exports_nothing() {
    let (url, received) = collector();
    let env: HashMap<&str, String> = HashMap::from([
        ("OML_POSTGRES__URL", "postgres://serving@env/db".to_owned()),
        (
            "OML_POSTGRES__OWNER_URL",
            "postgres://owner@env/db".to_owned(),
        ),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", url.clone()),
        (
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            format!("{url}/v1/traces"),
        ),
    ]);
    let config = Config::from_sources(None, &|k| env.get(k).cloned()).expect("loads");
    assert_eq!(
        config.otlp_endpoint, None,
        "only OML's setting turns export on"
    );

    // What main composes without the setting: no exporter, no tracer.
    let buf = Buf::default();
    let sink = buf.clone();
    let guard = tracing::subscriber::set_default(exporting_subscriber(move || sink.clone(), None));
    whoami(&router(Traffic::new(TrustedProxies::default()))).await;
    drop(guard);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(received.lock().expect("unpoisoned").is_empty());
    let text = String::from_utf8(buf.0.lock().expect("unpoisoned").clone()).expect("UTF-8");
    assert!(
        !text.contains("trace_id"),
        "export off, no trace_id: {text}"
    );
}

#[tokio::test]
async fn set_spans_go_to_v1_traces_as_protobuf_with_no_auth_header_under_the_callers_trace() {
    let (url, received) = collector();
    let export = Export::start(&Endpoint::parse(&url).expect("a base URL")).expect("starts");
    let buf = Buf::default();
    let sink = buf.clone();
    let guard = tracing::subscriber::set_default(exporting_subscriber(
        move || sink.clone(),
        Some(export.tracer()),
    ));
    let traffic = Traffic::new(TrustedProxies::default()).with_tracer(export.request_tracer());
    whoami(&router(traffic)).await;
    drop(guard);
    export.shutdown();

    let text = String::from_utf8(buf.0.lock().expect("unpoisoned").clone()).expect("UTF-8");
    let line: Value = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|l| l["event"] == "http_request")
        .expect("the request line");
    assert_eq!(line["trace_id"], TRACE, "the caller's trace: {line}");

    let received = received.lock().expect("unpoisoned");
    assert!(!received.is_empty(), "the batch was posted");
    let post = &received[0];
    assert_eq!(post.head[0], "POST /v1/traces HTTP/1.1");
    let lower: Vec<String> = post.head.iter().map(|l| l.to_ascii_lowercase()).collect();
    assert!(
        lower.contains(&"content-type: application/x-protobuf".to_owned()),
        "{lower:?}"
    );
    assert!(
        !lower.iter().any(|l| l.starts_with("authorization")),
        "{lower:?}"
    );
    let request = ExportTraceServiceRequest::decode(post.body.as_slice()).expect("protobuf");
    let spans: Vec<_> = request
        .resource_spans
        .iter()
        .flat_map(|r| &r.scope_spans)
        .flat_map(|s| &s.spans)
        .collect();
    let server = spans
        .iter()
        .find(|s| s.name == "GET /v2/whoami")
        .unwrap_or_else(|| {
            panic!(
                "no request span among {:?}",
                spans.iter().map(|s| &s.name).collect::<Vec<_>>()
            )
        });
    assert_eq!(hex::encode(&server.trace_id), TRACE);
    assert_eq!(hex::encode(&server.parent_span_id), "00f067aa0ba902b7");
    let keys: Vec<&str> = server.attributes.iter().map(|a| a.key.as_str()).collect();
    for expected in [
        "http.request.method",
        "http.route",
        "http.response.status_code",
        "oml.request_id",
    ] {
        assert!(keys.contains(&expected), "{expected} in {keys:?}");
    }
    for personal in ["principal", "key_name", "client_ip", "user_agent"] {
        assert!(
            !keys.iter().any(|k| k.contains(personal)),
            "{personal} in {keys:?}"
        );
    }
}
