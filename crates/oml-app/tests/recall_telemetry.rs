//! A recall hands its event to the telemetry port and never waits on it:
//! a port that drops everything, as a full queue does, leaves the answer
//! the same (ADR-0054 d2, d13).

use std::sync::{Arc, Mutex};

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::{EventTime, HeldBy, MemoryKind, Role};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::search::MatchMode;
use oml_app::telemetry::{
    LabelledQuery, ServedRecall, StoredEvent, Telemetry, TelemetryFuture, TelemetryItem,
};
use oml_app::types::{
    BootstrapRequest, GrantRequest, IssueKeyRequest, RecallRequest, SubmitRequest,
};
use uuid::Uuid;

/// The port of a queue that is always full: every item is dropped, and
/// counted here.
#[derive(Default)]
struct Full(Mutex<u64>);

impl Telemetry for Full {
    fn offer(&self, _item: TelemetryItem) {
        *self.0.lock().expect("count") += 1;
    }

    fn flush(&self) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }

    fn served(&self, _: Uuid, _: Uuid, _: Uuid) -> TelemetryFuture<'_, Option<ServedRecall>> {
        Box::pin(async { Ok(None) })
    }

    fn explicit_labels_today(&self, _: Uuid, _: Uuid) -> TelemetryFuture<'_, u64> {
        Box::pin(async { Ok(0) })
    }

    fn events(
        &self,
        _: Uuid,
        _: Option<(EventTime, Uuid)>,
        _: u32,
    ) -> TelemetryFuture<'_, Vec<StoredEvent>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn labelled_queries(&self, _: Uuid) -> TelemetryFuture<'_, Vec<LabelledQuery>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[tokio::test]
async fn a_recall_answers_the_same_when_the_event_queue_is_full() {
    let store = MemStore::new();
    let app = |telemetry: Option<Arc<dyn Telemetry>>| {
        let app = App::new(
            store.clone(),
            MemSigner::new([7; 32]),
            FixedClock::new(),
            SeqIdGen::new(),
            MarkerScanner::default(),
        );
        match telemetry {
            Some(t) => app.with_telemetry(t),
            None => app,
        }
    };
    let plain = app(None);
    let boot = plain
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "alice-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let grant = |role| GrantRequest {
        node: boot.root_workarea_id.into(),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    };
    let writer = plain
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                role: Role::Writer,
                name: Name::parse("agent-7").expect("a name"),
                principal: "crew/agent-7".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: Some(boot.root_workarea_id.into()),
                crew: None,
                grants: vec![grant(Role::Writer), grant(Role::Reader)],
                connect_code: false,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("a key")
        .value;
    plain
        .submit_memory(
            writer.as_str(),
            &SubmitRequest {
                on_similar: Default::default(),
                anchor: None,
                idempotency_key: "i-1".to_owned(),
                claim_key: None,
                verb: None,
                kind: MemoryKind::Memory,
                body: "the deploy runs on fridays".to_owned(),
                workarea: None,
                derives_from: Vec::new(),
                severity: None,
                rule: None,
            },
        )
        .await
        .expect("submit");
    let req = RecallRequest {
        timings: false,
        anchor: oml_app::types::AnchorFilter::default(),
        query: "deploy fridays".to_owned(),
        k: 8,
        workarea: None,
        kinds: Vec::new(),
        include_text: true,
        include_unreviewed: true,
        max_tokens: None,
        search: None,
        match_mode: MatchMode::Any,
    };
    let full = Arc::new(Full::default());
    let without = plain.recall(writer.as_str(), &req).await.expect("recall");
    let with = app(Some(Arc::clone(&full) as Arc<dyn Telemetry>))
        .recall(writer.as_str(), &req)
        .await
        .expect("recall");
    assert_eq!(*full.0.lock().expect("count"), 1, "the event was offered");
    let ids = |r: &oml_app::types::RecallResult| {
        r.hits
            .iter()
            .map(|h| (h.served.memory.memory_id, h.score.to_bits()))
            .collect::<Vec<_>>()
    };
    assert!(!with.hits.is_empty());
    assert_eq!(ids(&with), ids(&without));
    assert_eq!(with.query_digest, without.query_digest);
    assert_eq!(with.pipeline, without.pipeline);
}
