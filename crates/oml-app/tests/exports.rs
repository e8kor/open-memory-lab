//! Workspace export in `history` mode (ADR-0084 d1-d5, d10, d13; readings
//! I1-I9; #425), over the in-memory doubles. Each test is named after the
//! sentence it proves.

use std::io::Read as _;

use oml_app::App;
use oml_app::app::ExportRequest;
use oml_app::domain::access::Covers;
use oml_app::domain::export::ExportPolicy;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{MemoryKind, Role, UsedBy, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::{JournalStore as _, Signer as _};
use oml_app::types::{
    AppError, BootstrapRequest, BootstrapResult, GrantRequest, IssueKeyRequest, IssuedKey,
    SubmitRequest, Verdict,
};
use oml_bundle::{State, VerifyOptions, VersionEntry, crypt};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

const TEST_SEED: [u8; 32] = [
    0x09, 0x87, 0x45, 0x94, 0xaf, 0xc9, 0x3c, 0x44, 0xaf, 0x9c, 0x48, 0x6d, 0x6d, 0xd4, 0x8b, 0x68,
    0x51, 0x4b, 0x57, 0x36, 0xa7, 0xd6, 0x4c, 0xac, 0x72, 0xd9, 0x07, 0x2f, 0x98, 0xa6, 0x37, 0x1c,
];

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

fn vectors() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../shared/oml-bundle/tests/vectors")
}

/// The TEST-ONLY recipient of the bundle crate's vectors.
fn recipient() -> String {
    std::fs::read_to_string(vectors().join("test-recipient.txt"))
        .expect("the test recipient")
        .trim()
        .to_owned()
}

/// The TEST-ONLY identity that opens what is encrypted to [`recipient`].
fn identity() -> String {
    let hex_bytes = std::fs::read_to_string(vectors().join("TEST-ONLY-age-identity.hex"))
        .expect("the test identity");
    let bytes: Vec<u8> = (0..hex_bytes.trim().len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_bytes.trim()[i..i + 2], 16).expect("hex"))
        .collect();
    bech32::encode(
        "age-secret-key-",
        bech32::ToBase32::to_base32(&bytes),
        bech32::Variant::Bech32,
    )
    .expect("bech32")
    .to_uppercase()
}

fn rig() -> (TestApp, MemStore) {
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    (app, store)
}

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

fn by_name(value: &str) -> Reference {
    Reference::parse(value).expect("a reference")
}

fn fresh_key_name() -> Name {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    name(&format!(
        "export-key-{}",
        ISSUED.fetch_add(1, Ordering::Relaxed)
    ))
}

async fn bootstrap(app: &TestApp) -> BootstrapResult {
    app.bootstrap_tenant(&BootstrapRequest {
        tenant_name: name("acme"),
        admin_key_name: name("root-admin"),
        admin_principal: "acme-admin".to_owned(),
    })
    .await
    .expect("bootstrap")
}

fn grant(node: &str, role: Role) -> GrantRequest {
    GrantRequest {
        node: by_name(node),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
    }
}

async fn issue(
    app: &TestApp,
    admin: &str,
    role: Role,
    used_by: UsedBy,
    user: Option<&str>,
    principal: &str,
    grants: Vec<GrantRequest>,
) -> IssuedKey {
    app.issue_key(
        admin,
        &IssueKeyRequest {
            profile: None,
            role,
            name: fresh_key_name(),
            principal: principal.to_owned(),
            used_by: Some(used_by),
            default_workspace: None,
            crew: None,
            grants,
            user: user.map(name),
            connect_code: false,
            guardrail_scope: None,
            expiry: None,
            requests_per_minute: None,
        },
    )
    .await
    .expect("issue a key")
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

async fn written(app: &TestApp, key: &str, req: &SubmitRequest) -> Uuid {
    app.submit_memory(key, req)
        .await
        .expect("submit")
        .receipt
        .memory_id
}

fn history() -> ExportRequest {
    ExportRequest {
        mode: Some("history".to_owned()),
        recipients: vec![recipient()],
        release: None,
        since: None,
    }
}

/// Alice's root: her person writer key (the exporter), her agent's writer
/// key and her reviewer key, each `alice`'s.
struct Alice {
    boot: BootstrapResult,
    person: IssuedKey,
    agent: IssuedKey,
    review: IssuedKey,
}

async fn alice(app: &TestApp) -> Alice {
    let boot = bootstrap(app).await;
    let admin = &boot.admin_key.value;
    let rw = || vec![grant("alice", Role::Writer), grant("alice", Role::Reader)];
    let person = issue(
        app,
        admin,
        Role::Writer,
        UsedBy::Person,
        Some("alice"),
        "alice/desk",
        rw(),
    )
    .await;
    let agent = issue(
        app,
        admin,
        Role::Writer,
        UsedBy::Agent,
        Some("alice"),
        "alice/agent",
        rw(),
    )
    .await;
    let review = issue(
        app,
        admin,
        Role::Reviewer,
        UsedBy::Person,
        Some("alice"),
        "alice/review",
        vec![grant("alice", Role::Reviewer)],
    )
    .await;
    Alice {
        boot,
        person,
        agent,
        review,
    }
}

#[tokio::test]
async fn an_export_without_an_age_recipient_is_refused_unencrypted() {
    let (app, _) = rig();
    let a = alice(&app).await;
    // No recipient is refused before anything is read: even a workspace
    // that does not exist answers bundle.unencrypted.
    for workspace in ["alice", "nowhere"] {
        let err = app
            .export_workspace(
                a.person.value.as_str(),
                by_name(workspace),
                &ExportRequest {
                    recipients: Vec::new(),
                    ..history()
                },
            )
            .await
            .expect_err("no recipient");
        assert!(matches!(err, AppError::BundleUnencrypted { .. }), "{err:?}");
    }
    let err = app
        .export_workspace(
            a.person.value.as_str(),
            by_name("alice"),
            &ExportRequest {
                recipients: vec!["age1notakey".to_owned()],
                ..history()
            },
        )
        .await
        .expect_err("a recipient that does not parse");
    assert!(matches!(err, AppError::BundleUnencrypted { .. }), "{err:?}");
}

#[tokio::test]
async fn an_agent_key_is_refused_export() {
    let (app, _) = rig();
    let a = alice(&app).await;
    for (key, role, used_by) in [
        (&a.agent.value, Role::Writer, UsedBy::Agent),
        (&a.boot.admin_key.value, Role::Admin, UsedBy::Person),
    ] {
        let err = app
            .export_workspace(key.as_str(), by_name("alice"), &history())
            .await
            .expect_err("not a person's non-admin key");
        match err {
            AppError::ExportPersonOnly {
                role: r,
                used_by: u,
            } => assert_eq!((r, u.as_str()), (role, used_by.as_str())),
            other => panic!("{other:?}"),
        }
    }
}

#[tokio::test]
async fn release_mode_and_since_are_refused_until_releases_are_built() {
    let (app, _) = rig();
    let a = alice(&app).await;
    for (req, field) in [
        (
            ExportRequest {
                mode: Some("release".to_owned()),
                ..history()
            },
            "mode",
        ),
        (
            ExportRequest {
                release: Some("v1".to_owned()),
                ..history()
            },
            "release",
        ),
        (
            ExportRequest {
                since: Some("v1".to_owned()),
                ..history()
            },
            "since",
        ),
    ] {
        let err = app
            .export_workspace(a.person.value.as_str(), by_name("alice"), &req)
            .await
            .expect_err("releases are not built");
        match err {
            AppError::ExportModeRefused { field: f, reason } => {
                assert_eq!((f, reason), (field, "releases_unbuilt"));
            }
            other => panic!("{other:?}"),
        }
    }
}

#[tokio::test]
async fn a_tenant_whose_export_policy_is_off_refuses_every_export() {
    let (app, store) = rig();
    let a = alice(&app).await;
    store.set_export_policy(a.boot.tenant_id, ExportPolicy::Off);
    let err = app
        .export_workspace(a.person.value.as_str(), by_name("alice"), &history())
        .await
        .expect_err("policy off");
    assert!(
        matches!(err, AppError::ExportDisabled { reason: None }),
        "{err:?}"
    );
}

/// A shared workspace `crew` with a child `inner`, and a person's tenant
/// reader key on `crew`, `inner` denied when `deny_inner`.
async fn shared(app: &TestApp, boot: &BootstrapResult, deny_inner: bool) -> (IssuedKey, Uuid) {
    let admin = &boot.admin_key.value;
    let crew = app
        .create_workspace(admin, &name("crew"), Some(by_name("universe")), None)
        .await
        .expect("crew");
    let inner = app
        .create_workspace(admin, &name("inner"), Some(by_name("crew")), None)
        .await
        .expect("inner");
    let reader = issue(
        app,
        admin,
        Role::Reader,
        UsedBy::Person,
        None,
        "carol/desk",
        vec![GrantRequest {
            deny: if deny_inner {
                vec![inner.workspace.workspace_id.into()]
            } else {
                Vec::new()
            },
            ..grant("crew", Role::Reader)
        }],
    )
    .await;
    (reader, crew.workspace.workspace_id)
}

#[tokio::test]
async fn under_own_root_a_shared_workspace_exports_only_with_an_allowance_in_force() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    let (carol, crew) = shared(&app, &boot, false).await;
    let err = app
        .export_workspace(carol.value.as_str(), by_name("crew"), &history())
        .await
        .expect_err("outside the exporter's own root");
    assert!(
        matches!(
            err,
            AppError::ExportDisabled {
                reason: Some("own_root")
            }
        ),
        "{err:?}"
    );
    // A shared workspace's default mode is release, which is not built.
    store.allow_export(boot.tenant_id, crew, None);
    let err = app
        .export_workspace(
            carol.value.as_str(),
            by_name("crew"),
            &ExportRequest {
                mode: None,
                ..history()
            },
        )
        .await
        .expect_err("the default mode outside a user root is release");
    assert!(
        matches!(err, AppError::ExportModeRefused { field: "mode", .. }),
        "{err:?}"
    );
    let exported = app
        .export_workspace(carol.value.as_str(), by_name("crew"), &history())
        .await
        .expect("an allowance in force");
    assert_eq!(exported.counts.workspaces, 2);
}

#[tokio::test]
async fn a_node_the_exporter_cannot_read_refuses_the_whole_export() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    store.set_export_policy(boot.tenant_id, ExportPolicy::Allowed);
    let (carol, _) = shared(&app, &boot, true).await;
    let err = app
        .export_workspace(carol.value.as_str(), by_name("crew"), &history())
        .await
        .expect_err("inner is denied");
    assert!(matches!(err, AppError::ExportNotReadable), "{err:?}");
}

#[tokio::test]
async fn a_history_export_decrypts_verifies_and_carries_only_what_may_leave() {
    let (app, store) = rig();
    let a = alice(&app).await;
    let tenant_id = a.boot.tenant_id;
    // Promoted, by the agent: travels, without an event.
    let promoted = written(
        &app,
        &a.agent.value,
        &submit("p", "the kiln fires at 1200 C"),
    )
    .await;
    app.post_verdict(a.review.value.as_str(), promoted, Verdict::Approve, None)
        .await
        .expect("approve");
    // Held, by the agent: another key's held row never leaves.
    let foreign_held = written(
        &app,
        &a.agent.value,
        &submit("h", "glaze cracks below 900 C"),
    )
    .await;
    // The exporter's own held and rejected versions travel, with their events.
    let own_held = written(
        &app,
        &a.person.value,
        &submit("o", "the wheel turns at 120 rpm"),
    )
    .await;
    let own_rejected = written(&app, &a.person.value, &submit("r", "clay is a metal")).await;
    app.post_verdict(a.review.value.as_str(), own_rejected, Verdict::Reject, None)
        .await
        .expect("reject");

    let head_before = store.chain_head(tenant_id).await.unwrap().unwrap().seq;
    let exported = app
        .export_workspace(a.person.value.as_str(), by_name("alice"), &history())
        .await
        .expect("export");

    // The age file opens with the identity, and the bundle verifies under
    // the server's journal key, which is also the source server id (I5).
    let mut tar = Vec::new();
    crypt::decrypting_reader(exported.file.as_slice(), &identity())
        .expect("a recipient's identity opens it")
        .read_to_end(&mut tar)
        .expect("plaintext");
    let fingerprint = MemSigner::new(TEST_SEED).fingerprint();
    let verified = oml_bundle::read_bundle(
        tar.as_slice(),
        &VerifyOptions {
            expect_key: Some(fingerprint.clone()),
            expect_tenant: Some(tenant_id.to_string()),
            ..VerifyOptions::default()
        },
    )
    .expect("the bundle verifies");
    assert!(verified.authenticated);
    let manifest = &verified.contents.manifest;
    assert_eq!(manifest.source.server_id, fingerprint);
    assert_eq!(manifest.workspaces, vec!["universe.alice".to_owned()]);
    assert_eq!(exported.signer_fingerprint, fingerprint);

    let states: std::collections::BTreeMap<String, State> = verified
        .contents
        .versions
        .iter()
        .filter_map(|e| match e {
            VersionEntry::Version(v) => Some((v.artifact_id.clone(), v.state)),
            VersionEntry::Erased(_) => None,
        })
        .collect();
    assert_eq!(states.get(&promoted.to_string()), Some(&State::Promoted));
    assert_eq!(states.get(&own_held.to_string()), Some(&State::Held));
    assert_eq!(
        states.get(&own_rejected.to_string()),
        Some(&State::Rejected)
    );
    assert!(!states.contains_key(&foreign_held.to_string()));
    assert_eq!(states.len(), 3);
    // Only the exporter's own writes carry events (R1, R12).
    assert_eq!(verified.contents.events.len(), 2);
    assert_eq!(exported.counts.events, 2);
    // The approval and the rejection travel as verdicts.
    assert_eq!(verified.contents.verdicts.len(), 2);
    // No line names a key or a person.
    let text = String::from_utf8_lossy(&tar).into_owned();
    assert!(!text.contains(&a.agent.key_id.to_string()));
    assert!(!text.contains(&a.review.key_id.to_string()));
    assert!(!text.contains("alice/review"));

    // Journalled once, with the manifest digest and the recipients'
    // fingerprints; metered in bytes on the exporter's account.
    let events = store
        .events_range(tenant_id, head_before + 1, 10)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    let e = &events[0].envelope;
    assert_eq!(e.event_type, event_types::WORKSPACE_EXPORTED);
    assert_eq!(e.payload["manifest_digest"], exported.manifest_digest);
    assert_eq!(
        e.payload["recipients"][0],
        crypt::recipient_fingerprint(&recipient()).unwrap()
    );
    assert_eq!(e.payload["mode"], "history");
    let root = store
        .resolve_name(
            tenant_id,
            oml_app::domain::name::NamedKind::Workspace,
            &name("alice"),
        )
        .await
        .unwrap()
        .expect("alice's root");
    let at = oml_app::ports::Clock::now(&FixedClock::new());
    assert_eq!(
        store.exported_bytes(tenant_id, root, &at).await.unwrap(),
        exported.file.len() as u64
    );
}

#[tokio::test]
async fn an_erasure_marker_carries_no_digest() {
    let (app, store) = rig();
    let a = alice(&app).await;
    let erased = written(
        &app,
        &a.agent.value,
        &submit("e", "the studio code is 4417"),
    )
    .await;
    app.post_verdict(a.review.value.as_str(), erased, Verdict::Approve, None)
        .await
        .expect("approve");
    store.set_status(
        a.boot.tenant_id,
        erased,
        oml_app::domain::MemoryStatus::Erased,
    );
    let exported = app
        .export_workspace(a.person.value.as_str(), by_name("alice"), &history())
        .await
        .expect("export");
    assert_eq!(exported.counts.erased, 1);
    let mut tar = Vec::new();
    crypt::decrypting_reader(exported.file.as_slice(), &identity())
        .expect("opens")
        .read_to_end(&mut tar)
        .expect("plaintext");
    let verified =
        oml_bundle::read_bundle(tar.as_slice(), &VerifyOptions::default()).expect("verifies");
    let marker = verified
        .contents
        .versions
        .iter()
        .find_map(|e| match e {
            VersionEntry::Erased(m) => Some(m.clone()),
            VersionEntry::Version(_) => None,
        })
        .expect("a marker");
    assert_eq!(marker.artifact_id, erased.to_string());
    // d13, R7: the marker is the id, the version and the fact; the erased
    // text's digest appears nowhere in the bundle.
    let digest = oml_bundle::sha256_digest("the studio code is 4417".as_bytes());
    let text = String::from_utf8_lossy(&tar).into_owned();
    assert!(!text.contains(digest.trim_start_matches("sha256:")));
    assert!(!text.contains("4417"));
}
