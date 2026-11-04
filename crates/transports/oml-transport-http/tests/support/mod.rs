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
