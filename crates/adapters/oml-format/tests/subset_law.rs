//! ADR-0056 d6: a format can add a finding and never remove one. A source the
//! scanner refuses as prose is refused in every format that accepts it, and a
//! fragment whose source the scanner passes is refused only for what the
//! format added.

use oml_app::App;
use oml_app::domain::HeldBy;
use oml_app::domain::access::Covers;
use oml_app::domain::name::Name;
use oml_app::domain::{MemoryKind, Role};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::types::{BootstrapRequest, GrantRequest, IssueKeyRequest, SubmitRequest};
use proptest::prelude::*;

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

const SEED: [u8; 32] = [7; 32];

fn app() -> TestApp {
    App::new(
        MemStore::new(),
        MemSigner::new(SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_formats(oml_format::compiled())
}

fn request(idem: &str, body: &str, format: Option<&str>) -> SubmitRequest {
    SubmitRequest {
        on_similar: Default::default(),
        anchor: None,
        idempotency_key: idem.to_owned(),
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workarea: None,
        derives_from: Vec::new(),
        severity: None,
        rule: None,
        valid_from: None,
        valid_until: None,
        format: format.map(str::to_owned),
        title: format.map(|_| "a title".to_owned()),
    }
}

async fn writer(app: &TestApp) -> String {
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("name"),
            admin_key_name: Name::parse("root-admin").expect("name"),
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
    let issued = app
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Writer,
                name: Name::parse("w-key").expect("name"),
                principal: "writer".to_owned(),
                held_by: Some(HeldBy::Agent),
                default_workarea: None,
                crew: None,
                grants: vec![grant(Role::Writer), grant(Role::Reader)],
                connect_code: false,
                user: None,
                guardrail_scope: None,
            },
        )
        .await
        .expect("issue");
    issued.value
}

fn scalar() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("-----BEGIN".to_owned()),
        Just("[needs-human]".to_owned()),
        "[a-z ]{0,12}",
        Just("plain words".to_owned()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn the_scans_findings_on_the_source_alone_are_a_subset_of_its_findings_on_the_source_and_its_views(
        key in "[a-z]{1,6}",
        a in scalar(),
        b in scalar(),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread().build().expect("runtime");
        rt.block_on(async {
            let source = serde_json::json!({ key.clone(): a, "other": [b] }).to_string();
            // The prose scan of the same bytes.
            let prose = app();
            let k = writer(&prose).await;
            let alone = prose.submit_memory(&k, &request("p", &source, None)).await;
            // The same bytes through a format: source scan, then the views.
            let with_views = app();
            let k2 = writer(&with_views).await;
            let both = with_views
                .submit_memory(&k2, &request("f", &source, Some("application/json")))
                .await;
            if alone.is_err() {
                prop_assert!(
                    both.is_err(),
                    "the source alone was refused, the source and its views passed: {source}"
                );
            }
            Ok(())
        })?;
    }
}
