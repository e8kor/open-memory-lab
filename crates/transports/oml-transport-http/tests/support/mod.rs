//! A router over the in-memory doubles: no Postgres, no network.

#![allow(dead_code)]

use std::sync::Arc;

use axum::Router;
use oml_app::App;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_transport_http::{ApiState, DbPing};

pub const SEED: [u8; 32] = [7; 32];

pub type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;
pub type TestState = ApiState<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

pub fn ping() -> DbPing {
    Arc::new(|| Box::pin(async { true }))
}

pub fn app(store: &MemStore) -> TestApp {
    App::new(
        store.clone(),
        MemSigner::new(SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
}

pub fn state(store: &MemStore, app: TestApp) -> TestState {
    ApiState::new(Arc::new(app), store.clone(), ping(), true, None)
}

pub async fn router() -> Router {
    let store = MemStore::new();
    state(&store, app(&store)).into_router()
}

/// Every line the code under test logs, as the lead's JSON subscriber
/// writes it: `tracing-subscriber`'s JSON format, the event's fields at the
/// top level and no span fields (ADR-0042 d1).
#[derive(Clone, Default)]
pub struct Captured(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("unpoisoned").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl Captured {
    /// Captures this thread's lines until the guard drops.
    pub fn start() -> (tracing::subscriber::DefaultGuard, Self) {
        let captured = Self::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_current_span(false)
            .with_span_list(false)
            .with_writer(captured.clone())
            .finish();
        (tracing::subscriber::set_default(subscriber), captured)
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("unpoisoned").clone()).expect("UTF-8 lines")
    }

    /// Each line, parsed: every one is a JSON object.
    pub fn lines(&self) -> Vec<serde_json::Value> {
        self.text()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
            .collect()
    }

    /// The lines whose `event` is `event`.
    pub fn events(&self, event: &str) -> Vec<serde_json::Value> {
        self.lines()
            .into_iter()
            .filter(|l| l["event"] == event)
            .collect()
    }
}
