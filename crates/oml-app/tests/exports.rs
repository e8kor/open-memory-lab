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
        create: false,
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
            person: None,
            create_workspaces: Vec::new(),
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
    // alice's sign-in entry first: an address added for a person a key
    // names waits on her (the operator's decision of 2026-11-20, ADR-0045
    // d2's note).
    let (entry, _) = app
        .allow_web_email(
            admin.as_str(),
            &oml_app::types::WebEmailRequest {
                email: oml_app::domain::web::Email::parse("alice@example.com").expect("an email"),
                user: name("alice"),
                admin: false,
            },
        )
        .await
        .expect("alice's entry");
    app.web_ensure_keys(&entry).await.expect("her session keys");
    let rw = || {
        vec![
            grant("alice", Role::Submitter),
            grant("alice", Role::Reader),
        ]
    };
    let person = issue(
        app,
        admin,
        Role::Contributor,
        UsedBy::Person,
        Some("alice"),
        "alice/desk",
        rw(),
    )
    .await;
    let agent = issue(
        app,
        admin,
        Role::Contributor,
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
        (&a.agent.value, Role::Contributor, UsedBy::Agent),
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
    // Held, by the person's own agent key: hers by I6 as written, so it
    // travels, without its event (R12).
    let agent_held = written(
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
    assert_eq!(states.get(&agent_held.to_string()), Some(&State::Held));
    assert_eq!(states.len(), 4);
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

/// Exports `alice` as her person key and opens the bundle.
async fn exported_bundle(app: &TestApp, a: &Alice) -> oml_bundle::BundleContents {
    let exported = app
        .export_workspace(a.person.value.as_str(), by_name("alice"), &history())
        .await
        .expect("export");
    let mut tar = Vec::new();
    crypt::decrypting_reader(exported.file.as_slice(), &identity())
        .expect("opens")
        .read_to_end(&mut tar)
        .expect("plaintext");
    oml_bundle::read_bundle(tar.as_slice(), &VerifyOptions::default())
        .expect("verifies")
        .contents
}

#[tokio::test]
async fn verdicts_jsonl_names_no_erasure_marker() {
    let (app, store) = rig();
    let a = alice(&app).await;
    let erased = written(&app, &a.agent.value, &submit("e", "the gate code is 9921")).await;
    app.post_verdict(a.review.value.as_str(), erased, Verdict::Approve, None)
        .await
        .expect("approve");
    store.set_status(
        a.boot.tenant_id,
        erased,
        oml_app::domain::MemoryStatus::Erased,
    );
    let contents = exported_bundle(&app, &a).await;
    assert!(
        contents
            .versions
            .iter()
            .any(|e| matches!(e, VersionEntry::Erased(m) if m.artifact_id == erased.to_string()))
    );
    // I17: the approval of an erased version stays on the server.
    assert!(
        contents
            .verdicts
            .iter()
            .all(|v| v.artifact_id != erased.to_string()),
        "{:?}",
        contents.verdicts
    );
}

#[tokio::test]
async fn a_crew_key_is_refused_export_as_person_only() {
    let (app, _) = rig();
    let a = alice(&app).await;
    let admin = &a.boot.admin_key.value;
    let crew = app
        .create_crew(
            admin,
            &oml_app::types::CrewCreateRequest {
                name: name("kiln-crew"),
                charter_uri: None,
                external_manager: None,
            },
        )
        .await
        .expect("create crew");
    let crew_key = app
        .issue_key(
            admin,
            &IssueKeyRequest {
                profile: None,
                role: Role::Reader,
                name: fresh_key_name(),
                principal: "kiln-crew/desk".to_owned(),
                used_by: Some(UsedBy::Person),
                default_workspace: None,
                crew: Some(by_name("kiln-crew")),
                grants: vec![grant("alice", Role::Reader)],
                user: Some(name("alice")),
                connect_code: false,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                person: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("issue a crew key");
    assert_eq!(crew_key.crew_id, Some(crew.crew_id));
    let err = app
        .export_workspace(crew_key.value.as_str(), by_name("alice"), &history())
        .await
        .expect_err("a crew key never exports");
    assert!(matches!(err, AppError::ExportPersonOnly { .. }), "{err:?}");
}

#[tokio::test]
async fn a_bearer_person_key_exports_its_persons_console_writes() {
    let (app, _) = rig();
    let a = alice(&app).await;
    let admin = a.boot.admin_key.value.as_str();
    let (entry, _) = app
        .allow_web_email(
            admin,
            &oml_app::types::WebEmailRequest {
                email: oml_app::domain::web::Email::parse("alice@example.com").expect("an email"),
                user: name("alice"),
                admin: false,
            },
        )
        .await
        .expect("the entry is added");
    app.web_ensure_keys(&entry).await.expect("the session keys");
    let token = app
        .web_session_open(&entry, 3600, None)
        .await
        .expect("the session opens")
        .token;
    let session = app
        .web_session(&token)
        .await
        .expect("a read")
        .expect("a live session");
    let web = app
        .web_caller(&session, oml_app::app::Capacity::Web)
        .await
        .expect("alice-web");
    let console = app
        .submit_memory(
            oml_app::types::Bearer::Session(&web),
            &SubmitRequest {
                workspace: Some(by_name("alice")),
                ..submit("console", "the kiln door sticks in winter")
            },
        )
        .await
        .expect("a console write")
        .receipt
        .memory_id;
    let contents = exported_bundle(&app, &a).await;
    // I6 as written: the person's own set is every key of the person, so
    // the console's held write travels with the bearer key's export, and
    // its event does not (R12: only the exporting key's own writes).
    let line = contents
        .versions
        .iter()
        .find_map(|e| match e {
            VersionEntry::Version(v) if v.artifact_id == console.to_string() => Some(v.clone()),
            _ => None,
        })
        .expect("the console write travels");
    assert_eq!(line.state, State::Held);
    assert_eq!(line.source_event, None);
}

#[tokio::test]
async fn a_bundle_over_uploads_max_bytes_is_export_too_large_and_journals_and_meters_nothing() {
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_uploads(
        std::sync::Arc::new(oml_app::doubles::MemObjects::new()),
        256,
        1 << 20,
    );
    let a = alice(&app).await;
    let tenant_id = a.boot.tenant_id;
    written(&app, &a.person.value, &submit("big", &"glaze ".repeat(200))).await;
    let head_before = store.chain_head(tenant_id).await.unwrap().unwrap().seq;
    let err = app
        .export_workspace(a.person.value.as_str(), by_name("alice"), &history())
        .await
        .expect_err("over uploads.max_bytes");
    assert!(
        matches!(err, AppError::ExportTooLarge { max_bytes: 256 }),
        "{err:?}"
    );
    // I13: refused before anything is journalled or metered.
    assert_eq!(
        store.chain_head(tenant_id).await.unwrap().unwrap().seq,
        head_before
    );
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
    assert_eq!(store.exported_bytes(tenant_id, root, &at).await.unwrap(), 0);
}

#[tokio::test]
async fn a_versions_derives_from_keeps_only_the_ids_the_bundle_exports() {
    let (app, store) = rig();
    let a = alice(&app).await;
    let mut promoted = Vec::new();
    for (idem, body) in [
        ("kept", "the kiln vents north"),
        ("gone", "the vent code is 5531"),
    ] {
        let id = written(&app, &a.agent.value, &submit(idem, body)).await;
        app.post_verdict(a.review.value.as_str(), id, Verdict::Approve, None)
            .await
            .expect("approve");
        promoted.push(id);
    }
    let (kept, gone) = (promoted[0], promoted[1]);
    let child = written(
        &app,
        &a.person.value,
        &SubmitRequest {
            derives_from: vec![kept, gone],
            ..submit("child", "the north vent needs a damper")
        },
    )
    .await;
    store.set_status(
        a.boot.tenant_id,
        gone,
        oml_app::domain::MemoryStatus::Erased,
    );
    let contents = exported_bundle(&app, &a).await;
    let line = contents
        .versions
        .iter()
        .find_map(|e| match e {
            VersionEntry::Version(v) if v.artifact_id == child.to_string() => Some(v.clone()),
            _ => None,
        })
        .expect("the child travels");
    // I17: an erased ancestor is a marker, not an exported version.
    assert_eq!(line.derives_from, vec![kept.to_string()]);
}

#[tokio::test]
async fn an_export_by_a_key_with_no_account_root_is_counted_on_the_tenants_meter() {
    let (app, store) = rig();
    let boot = bootstrap(&app).await;
    store.set_export_policy(boot.tenant_id, ExportPolicy::Allowed);
    let (carol, _) = shared(&app, &boot, false).await;
    let exported = app
        .export_workspace(carol.value.as_str(), by_name("crew"), &history())
        .await
        .expect("a shared workspace under the allowed policy");
    let at = oml_app::ports::Clock::now(&FixedClock::new());
    // I19: no account meter counts carol's export; the tenant's does.
    assert_eq!(
        store
            .tenant_exported_bytes(boot.tenant_id, &at)
            .await
            .unwrap(),
        exported.file.len() as u64
    );
}

/// I6 and R3: a tenant key linked to a person (ADR-0085 d16) owns itself
/// alone. Exporting that person's root, it carries none of the person's
/// other keys' held rows, which the person's own export carries.
#[tokio::test]
async fn a_linked_key_exports_none_of_its_persons_other_keys_held_rows() {
    let (app, _) = rig();
    let a = alice(&app).await;
    let admin = a.boot.admin_key.value.as_str();
    let held = written(
        &app,
        a.agent.value.as_str(),
        &SubmitRequest {
            workspace: Some(by_name("alice")),
            ..submit("agent-held", "the glaze cracks below 900 degrees")
        },
    )
    .await;
    // Bob's tenant key, linked to alice by a console admin.
    let (ops_entry, _) = app
        .allow_web_email(
            admin,
            &oml_app::types::WebEmailRequest {
                email: oml_app::domain::web::Email::parse("ops@example.com").expect("an email"),
                user: name("ops"),
                admin: true,
            },
        )
        .await
        .expect("the admin entry");
    app.web_ensure_keys(&ops_entry).await.expect("the keys");
    let token = app
        .web_session_open(&ops_entry, 3600, None)
        .await
        .expect("opens")
        .token;
    let session = app
        .web_session(&token)
        .await
        .expect("a read")
        .expect("live");
    let ops = app
        .web_caller(&session, oml_app::app::Capacity::Admin)
        .await
        .expect("the console admin");
    let (alice_entry, _) = app
        .allow_web_email(
            admin,
            &oml_app::types::WebEmailRequest {
                email: oml_app::domain::web::Email::parse("alice@example.com").expect("an email"),
                user: name("alice"),
                admin: false,
            },
        )
        .await
        .expect("alice signs in");
    app.web_ensure_keys(&alice_entry).await.expect("the keys");
    let bob = issue(
        &app,
        admin,
        Role::Reader,
        UsedBy::Person,
        None,
        "bob/laptop",
        vec![grant("alice", Role::Reader)],
    )
    .await;
    app.link_key_to_person(
        oml_app::types::Bearer::Session(&ops),
        bob.key_id.into(),
        &name("alice"),
        Some("the person holds this key"),
    )
    .await
    .expect("linked, pending");
    // Alice accepts the link from her own console session (ADR-0085 d16,
    // the operator's decision of 2026-11-19).
    let alice_token = app
        .web_session_open(&alice_entry, 3600, None)
        .await
        .expect("opens")
        .token;
    let alice_session = app
        .web_session(&alice_token)
        .await
        .expect("a read")
        .expect("live");
    let alice_web = app
        .web_caller(&alice_session, oml_app::app::Capacity::Web)
        .await
        .expect("alice's console");
    app.accept_key_link(
        oml_app::types::Bearer::Session(&alice_web),
        bob.key_id.into(),
    )
    .await
    .expect("accepted");
    let exported = app
        .export_workspace(bob.value.as_str(), by_name("alice"), &history())
        .await
        .expect("export");
    let mut tar = Vec::new();
    crypt::decrypting_reader(exported.file.as_slice(), &identity())
        .expect("opens")
        .read_to_end(&mut tar)
        .expect("plaintext");
    let contents = oml_bundle::read_bundle(tar.as_slice(), &VerifyOptions::default())
        .expect("verifies")
        .contents;
    let leaked: Vec<String> = contents
        .versions
        .iter()
        .filter_map(|e| match e {
            VersionEntry::Version(v) if v.state != State::Promoted => Some(v.artifact_id.clone()),
            _ => None,
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "a linked key exported held rows: {leaked:?}"
    );
    // The person's own export carries the agent's held row.
    let own = exported_bundle(&app, &a).await;
    assert!(
        own.versions
            .iter()
            .any(|e| matches!(e, VersionEntry::Version(v) if v.artifact_id == held.to_string()))
    );
}

/// I13: the stored-bytes estimate does not stand in for the bundle's size:
/// a bundle whose bodies fit but whose lines do not is still refused once
/// built, and nothing is journalled.
#[tokio::test]
async fn a_bundle_whose_bodies_fit_but_whose_lines_do_not_is_still_export_too_large() {
    let store = MemStore::new();
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_uploads(
        std::sync::Arc::new(oml_app::doubles::MemObjects::new()),
        256,
        1 << 20,
    );
    let a = alice(&app).await;
    let tenant_id = a.boot.tenant_id;
    written(&app, &a.person.value, &submit("small", "kiln")).await;
    let head_before = store.chain_head(tenant_id).await.unwrap().unwrap().seq;
    let err = app
        .export_workspace(a.person.value.as_str(), by_name("alice"), &history())
        .await
        .expect_err("the built bundle is over 256 bytes");
    assert!(
        matches!(err, AppError::ExportTooLarge { max_bytes: 256 }),
        "{err:?}"
    );
    assert_eq!(
        store.chain_head(tenant_id).await.unwrap().unwrap().seq,
        head_before
    );
}
