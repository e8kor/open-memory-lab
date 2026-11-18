//! Workspace export (ADR-0084 d2, d10; #425), stated over the application
//! and run identically over every store, the in-memory double and
//! PostgreSQL: the tenant's policy is `own_root` until one is set; a history
//! export reads every promoted version and the exporter's own in any state,
//! never another key's held one; an export is journalled once and its bytes
//! are counted on the exporter's account and on the tenant's.

use std::collections::BTreeSet;

use uuid::Uuid;

use oml_app::App;
use oml_app::app::ExportRequest;
use oml_app::domain::access::Covers;
use oml_app::domain::export::ExportPolicy;
use oml_app::domain::name::{Name, NamedKind, Reference};
use oml_app::domain::{MemoryKind, MemoryStatus, Role, UsedBy, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, SeqIdGen};
use oml_app::ports::{Clock, ConnectionStore, WebStore};
use oml_app::types::{
    BootstrapRequest, GrantRequest, IssueKeyRequest, IssuedKey, SubmitRequest, Verdict,
};

const TEST_SEED: [u8; 32] = [9; 32];

/// An age X25519 recipient for the laws; nothing here decrypts.
const RECIPIENT: &str = "age1c7uc8xfckg2ntq76m2tsazw6xqsm9awvg8v4f4g57xqgcvs3e5pq9963un";

type TestApp<S> = App<S, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

struct Rig<S> {
    app: TestApp<S>,
    store: S,
    tenant_id: Uuid,
    root: Uuid,
    person: IssuedKey,
    agent: IssuedKey,
    review: IssuedKey,
}

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

async fn rig<S: WebStore + ConnectionStore + Clone>(store: S) -> Rig<S> {
    let app: TestApp<S> = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("root-admin"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let admin = boot.admin_key.value.clone();
    let issue = |role: Role, used_by: UsedBy, key_name: &'static str, roles: Vec<Role>| {
        let app = &app;
        let admin = admin.clone();
        async move {
            app.issue_key(
                admin.as_str(),
                &IssueKeyRequest {
                    profile: None,
                    role,
                    name: name(key_name),
                    principal: format!("ada/{key_name}"),
                    used_by: Some(used_by),
                    default_workspace: None,
                    crew: None,
                    grants: roles
                        .into_iter()
                        .map(|r| GrantRequest {
                            node: Reference::parse("ada").expect("a reference"),
                            role: r,
                            deny: Vec::new(),
                            covers: Covers::Subtree,
                        })
                        .collect(),
                    user: Some(name("ada")),
                    connect_code: false,
                    guardrail_scope: None,
                    expiry: None,
                    requests_per_minute: None,
                },
            )
            .await
            .expect("issue a key")
        }
    };
    let person = issue(
        Role::Writer,
        UsedBy::Person,
        "ada-desk",
        vec![Role::Writer, Role::Reader],
    )
    .await;
    let agent = issue(
        Role::Writer,
        UsedBy::Agent,
        "ada-agent",
        vec![Role::Writer, Role::Reader],
    )
    .await;
    let review = issue(
        Role::Reviewer,
        UsedBy::Person,
        "ada-review",
        vec![Role::Reviewer],
    )
    .await;
    let root = store
        .resolve_name(boot.tenant_id, NamedKind::Workspace, &name("ada"))
        .await
        .expect("resolve")
        .expect("ada's root");
    Rig {
        app,
        store,
        tenant_id: boot.tenant_id,
        root,
        person,
        agent,
        review,
    }
}

fn submit(idem: &str, body: &str) -> SubmitRequest {
    SubmitRequest {
        on_similar: Default::default(),
        anchor: None,
        origin: None,
        idempotency_key: idem.to_owned(),
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind: MemoryKind::Memory,
        body: body.to_owned(),
        workspace: None,
        derives_from: Vec::new(),
        severity: None,
        rule: None,
        kind_fields: None,
        valid_from: None,
        valid_until: None,
        format: None,
        title: None,
    }
}

async fn written<S: WebStore + ConnectionStore + Clone>(
    rig: &Rig<S>,
    key: &IssuedKey,
    idem: &str,
    body: &str,
) -> Uuid {
    rig.app
        .submit_memory(key.value.as_str(), &submit(idem, body))
        .await
        .expect("submit")
        .receipt
        .memory_id
}

/// A tenant exports under `own_root` until a policy is set, and no
/// allowance is in force for a workspace no admin allowed (d10).
pub async fn a_tenant_exports_under_own_root_until_a_policy_is_set<
    S: WebStore + ConnectionStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    assert_eq!(
        rig.store
            .export_policy(rig.tenant_id)
            .await
            .expect("policy"),
        ExportPolicy::OwnRoot
    );
    assert!(
        !rig.store
            .export_allowed(rig.tenant_id, rig.root, &FixedClock::new().now())
            .await
            .expect("allowance")
    );
}

/// A history export reads every promoted version and the exporter's own
/// versions in any state, in creation order, and never another key's held
/// or rejected one (d2, R3, reading I6).
pub async fn a_history_export_reads_promoted_and_own_versions_and_never_another_keys_held_ones<
    S: WebStore + ConnectionStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let promoted = written(&rig, &rig.agent, "p", "the kiln fires at 1200 C").await;
    rig.app
        .post_verdict(rig.review.value.as_str(), promoted, Verdict::Approve, None)
        .await
        .expect("approve");
    let foreign_held = written(&rig, &rig.agent, "h", "glaze cracks below 900 C").await;
    let foreign_rejected = written(&rig, &rig.agent, "x", "clay is a metal").await;
    rig.app
        .post_verdict(
            rig.review.value.as_str(),
            foreign_rejected,
            Verdict::Reject,
            None,
        )
        .await
        .expect("reject");
    let own_held = written(&rig, &rig.person, "o", "the wheel turns at 120 rpm").await;
    let nodes: Vec<Uuid> = rig
        .store
        .workspace_subtree(rig.tenant_id, rig.root)
        .await
        .expect("subtree")
        .into_iter()
        .map(|w| w.workspace_id)
        .collect();
    let rows = rig
        .store
        .export_versions(rig.tenant_id, &nodes, &[rig.person.key_id])
        .await
        .expect("export versions");
    let ids: Vec<Uuid> = rows.iter().map(|m| m.memory_id).collect();
    assert_eq!(ids, vec![promoted, own_held], "creation order");
    assert_eq!(rows[0].status, MemoryStatus::Promoted);
    assert_eq!(rows[1].status, MemoryStatus::Held);
    let read: BTreeSet<Uuid> = ids.into_iter().collect();
    assert!(!read.contains(&foreign_held));
    assert!(!read.contains(&foreign_rejected));
    // A node outside the list reads nothing.
    assert!(
        rig.store
            .export_versions(rig.tenant_id, &[Uuid::nil()], &[rig.person.key_id])
            .await
            .expect("export versions")
            .is_empty()
    );
}

/// An export is journalled once as `workspace.exported`, and its bytes are
/// counted on the `exports` meter of the exporter's account (d10, d15).
pub async fn an_export_is_journalled_once_and_counted_on_the_exporters_account<
    S: WebStore + ConnectionStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    written(&rig, &rig.person, "o", "the wheel turns at 120 rpm").await;
    let head = rig
        .store
        .chain_head(rig.tenant_id)
        .await
        .expect("head")
        .expect("a head")
        .seq;
    let exported = rig
        .app
        .export_workspace(
            rig.person.value.as_str(),
            Reference::parse("ada").expect("a reference"),
            &ExportRequest {
                mode: Some("history".to_owned()),
                recipients: vec![RECIPIENT.to_owned()],
                release: None,
                since: None,
            },
        )
        .await
        .expect("export");
    let events = rig
        .store
        .events_range(rig.tenant_id, head + 1, 10)
        .await
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].envelope.event_type,
        event_types::WORKSPACE_EXPORTED
    );
    assert_eq!(
        events[0].envelope.payload["manifest_digest"],
        exported.manifest_digest
    );
    assert_eq!(exported.counts.versions, 1);
    assert_eq!(exported.counts.events, 1);
    assert_eq!(
        rig.store
            .exported_bytes(rig.tenant_id, rig.root, &FixedClock::new().now())
            .await
            .expect("meter"),
        exported.file.len() as u64
    );
}

/// Every export is counted on the tenant's `exports` meter as well, so an
/// export by a key with no account root, which no account meter counts, is
/// counted too; the meter reads the UTC month (ADR-0084 I19).
pub async fn every_export_is_counted_on_the_tenants_exports_meter<
    S: WebStore + ConnectionStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    written(&rig, &rig.person, "o", "the wheel turns at 120 rpm").await;
    let now = FixedClock::new().now();
    assert_eq!(
        rig.store
            .tenant_exported_bytes(rig.tenant_id, &now)
            .await
            .expect("meter"),
        0
    );
    let mut total = 0;
    for _ in 0..2 {
        let exported = rig
            .app
            .export_workspace(
                rig.person.value.as_str(),
                Reference::parse("ada").expect("a reference"),
                &ExportRequest {
                    mode: Some("history".to_owned()),
                    recipients: vec![RECIPIENT.to_owned()],
                    release: None,
                    since: None,
                },
            )
            .await
            .expect("export");
        total += exported.file.len() as u64;
    }
    assert_eq!(
        rig.store
            .tenant_exported_bytes(rig.tenant_id, &now)
            .await
            .expect("meter"),
        total
    );
    // Another tenant's meter reads nothing of this one's.
    assert_eq!(
        rig.store
            .tenant_exported_bytes(Uuid::from_u128(77), &now)
            .await
            .expect("meter"),
        0
    );
}

/// Every export law, each over a fresh store from `make`.
pub async fn run_export_suite<S, F, Fut>(make: F)
where
    S: WebStore + ConnectionStore + Clone,
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = S>,
{
    a_tenant_exports_under_own_root_until_a_policy_is_set(make().await).await;
    a_history_export_reads_promoted_and_own_versions_and_never_another_keys_held_ones(make().await)
        .await;
    an_export_is_journalled_once_and_counted_on_the_exporters_account(make().await).await;
    every_export_is_counted_on_the_tenants_exports_meter(make().await).await;
}
