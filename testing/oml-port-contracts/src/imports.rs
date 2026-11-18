//! Workspace imports (ADR-0084 d6-d11, R11, I4; #426), stated over the
//! application and run identically over the in-memory double and
//! PostgreSQL: a person's writer key uploads a decrypted bundle and imports
//! it into an in-progress workspace; every version lands held by the
//! importer; a re-import writes nothing; the source's key is pinned at the
//! first import; an older bundle is refused unless allowed; the upload binds
//! once and its key is nulled when the import ends.

use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::SigningKey;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use oml_app::App;
use oml_app::app::{ImportAnswer, ImportRequest, ImportStart};
use oml_app::domain::access::Covers;
use oml_app::domain::import::ImportState;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::upload::{PART_SIZE, UploadPurpose, UploadRefusal, UploadState};
use oml_app::domain::{MemoryStatus, Role, UsedBy};
use oml_app::doubles::{FixedClock, MarkerScanner, MemObjects, MemSigner, SeqIdGen};
use oml_app::ports::JournalStore;
use oml_app::types::{
    AppError, BootstrapRequest, GrantRequest, ImportBegin, ImportRecord, IssueKeyRequest,
};
use oml_bundle::{
    Author, BundleContents, Manifest, Mode, Source, State, VersionEntry, VersionLine,
    key_fingerprint, sha256_digest,
};

const TEST_SEED: [u8; 32] = [9; 32];
const SOURCE_TENANT: &str = "00000000-0000-4000-8000-0000000000aa";
const SOURCE_SERVER: &str = "00000000-0000-4000-8000-0000000000f1";

static NAMES: AtomicU64 = AtomicU64::new(0);

type TestApp<S> = App<S, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

struct Rig<S> {
    app: TestApp<S>,
    store: S,
    objects: std::sync::Arc<MemObjects>,
    clock: FixedClock,
    admin: String,
    tenant_id: Uuid,
    /// A person's writer key in ada's root.
    writer: String,
    writer_id: Uuid,
    /// An agent's writer key in ada's root.
    agent: String,
    /// A person's key of another user, which reads nothing of ada's.
    stranger: String,
}

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

fn fresh(prefix: &str) -> Name {
    name(&format!(
        "{prefix}-{}",
        NAMES.fetch_add(1, Ordering::Relaxed)
    ))
}

async fn rig<S: JournalStore + Clone>(store: S) -> Rig<S> {
    let objects = std::sync::Arc::new(MemObjects::new());
    let clock = FixedClock::default();
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_uploads(
        objects.clone(),
        oml_app::domain::upload::DEFAULT_MAX_BYTES,
        oml_app::domain::upload::DEFAULT_TENANT_MAX_BYTES,
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
    let issue = |user: &str, used_by: UsedBy| IssueKeyRequest {
        profile: None,
        role: Role::Writer,
        name: fresh("import-key"),
        principal: format!("{user}/{}", NAMES.fetch_add(1, Ordering::Relaxed)),
        used_by: Some(used_by),
        default_workspace: None,
        crew: None,
        grants: [Role::Writer, Role::Reader]
            .into_iter()
            .map(|role| GrantRequest {
                node: Reference::parse(user).expect("a reference"),
                role,
                deny: Vec::new(),
                covers: Covers::Subtree,
            })
            .collect(),
        connect_code: false,
        user: Some(name(user)),
        guardrail_scope: None,
        expiry: None,
        requests_per_minute: None,
    };
    let writer = app
        .issue_key(admin.as_str(), &issue("ada", UsedBy::Person))
        .await
        .expect("ada's person key");
    let agent = app
        .issue_key(admin.as_str(), &issue("ada", UsedBy::Agent))
        .await
        .expect("ada's agent key");
    let stranger = app
        .issue_key(admin.as_str(), &issue("bo", UsedBy::Person))
        .await
        .expect("bo's person key");
    Rig {
        app,
        store,
        objects,
        clock,
        admin,
        tenant_id: boot.tenant_id,
        writer_id: writer.key_id,
        writer: writer.value,
        agent: agent.value,
        stranger: stranger.value,
    }
}

fn source_key() -> SigningKey {
    SigningKey::from_bytes(&[42_u8; 32])
}

fn other_key() -> SigningKey {
    SigningKey::from_bytes(&[43_u8; 32])
}

fn fingerprint(key: &SigningKey) -> String {
    key_fingerprint(&key.verifying_key())
}

fn line(artifact_id: &str, version: u64, claim_key: &str, text: &str) -> VersionLine {
    VersionLine {
        artifact_id: artifact_id.to_owned(),
        version,
        claim_key: claim_key.to_owned(),
        workspace: "universe.ada.notes".to_owned(),
        kind: "memory".to_owned(),
        format: None,
        title: None,
        state: State::Promoted,
        retirement: false,
        replaces: None,
        simulation_id: None,
        text: Some(text.to_owned()),
        body: None,
        plaintext_digest: sha256_digest(text.as_bytes()),
        anchor: serde_json::Value::Null,
        valid_from: None,
        valid_until: None,
        skill: None,
        document: None,
        test: None,
        guardrail: None,
        derives_from: Vec::new(),
        origin: json!({"kind": "doc"}),
        attachments: Vec::new(),
        author: Author {
            used_by: "person".to_owned(),
            same_user: false,
            pseudonym: "a".repeat(64),
        },
        created_at: "2026-11-01T10:00:00.000000Z".to_owned(),
        promoted_at: Some("2026-11-01T11:00:00.000000Z".to_owned()),
        source_event: None,
    }
}

/// A history-mode bundle of `lines`, exported at `exported_at`, signed by
/// `key`, as plaintext tar bytes.
fn bundle_of(lines: Vec<VersionLine>, exported_at: &str, key: &SigningKey) -> Vec<u8> {
    let contents = BundleContents {
        manifest: Manifest {
            format: oml_bundle::FORMAT.to_owned(),
            source: Source {
                server_id: SOURCE_SERVER.to_owned(),
                tenant_id: SOURCE_TENANT.to_owned(),
                tenant_name: "source".to_owned(),
            },
            workspaces: vec!["universe.ada.notes".to_owned()],
            mode: Mode::History,
            release: None,
            since: None,
            dropped: Vec::new(),
            exported_at: exported_at.to_owned(),
            exporter_pseudonym: "a".repeat(64),
            event_keys: Vec::new(),
            files: Vec::new(),
            signature: None,
        },
        workspaces: Vec::new(),
        versions: lines
            .into_iter()
            .map(|v| VersionEntry::Version(Box::new(v)))
            .collect(),
        verdicts: Vec::new(),
        events: Vec::new(),
        edges: vec![oml_bundle::EdgeLine {
            kind: oml_bundle::EdgeKind::Claim,
            src: "art-1".to_owned(),
            dst: "art-2".to_owned(),
            label: Some("supports".to_owned()),
        }],
        bodies: std::collections::BTreeMap::new(),
    };
    let mut out = Vec::new();
    oml_bundle::write_bundle(&contents, key, &mut out).expect("the fixture writes");
    out
}

fn first_bundle() -> Vec<u8> {
    bundle_of(
        vec![
            line("art-1", 1, "deploy-day", "Deploys go out on Tuesdays."),
            line("art-2", 1, "rollback", "A rollback is one command."),
        ],
        "2026-11-17T09:00:00.000000Z",
        &source_key(),
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Uploads `bytes` through one session and completes it.
async fn upload<S: JournalStore + Clone>(rig: &Rig<S>, key: &str, bytes: &[u8]) -> Uuid {
    let id = rig
        .app
        .open_upload(
            key,
            UploadPurpose::Import,
            bytes.len() as u64,
            &sha256_hex(bytes),
        )
        .await
        .expect("open")
        .upload_id;
    for (i, part) in bytes.chunks(PART_SIZE as usize).enumerate() {
        rig.app
            .put_upload_part(key, id, i as u64 + 1, part)
            .await
            .expect("part");
    }
    rig.app.complete_upload(key, id).await.expect("complete");
    id
}

fn request(upload_id: Uuid, target: &str, expect_key: Option<String>) -> ImportRequest {
    ImportRequest {
        upload_id,
        target: Reference::parse(target).expect("a target"),
        dry_run: false,
        allow_older: false,
        expect_key,
        expect_tenant: None,
    }
}

fn code_of(e: AppError) -> String {
    format!("{e:?}")
}

async fn import<S: JournalStore + Clone>(
    rig: &Rig<S>,
    key: &str,
    req: &ImportRequest,
) -> Result<ImportAnswer, AppError> {
    rig.app.import_bundle(key, req, &code_of).await
}

fn imported(answer: Result<ImportAnswer, AppError>) -> ImportRecord {
    match answer {
        Ok(ImportAnswer::Imported(record)) => record,
        other => panic!("expected an import, got {other:?}"),
    }
}

fn bundle_code(answer: Result<ImportAnswer, AppError>) -> &'static str {
    match answer {
        Err(AppError::Bundle(e)) => e.code(),
        other => panic!("expected a bundle refusal, got {other:?}"),
    }
}

async fn held_count<S: JournalStore + Clone>(rig: &Rig<S>, workspace_id: Uuid) -> u64 {
    rig.store
        .held_count(rig.tenant_id, &[workspace_id])
        .await
        .expect("held count")
}

/// A first import lands every version held by the importer; importing the
/// same bundle again writes no version (d8).
async fn a_reimport_of_the_same_bundle_writes_nothing<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let bytes = first_bundle();
    let up = upload(&rig, &rig.writer, &bytes).await;
    let first = imported(
        import(
            &rig,
            &rig.writer,
            &request(up, "notes", Some(fingerprint(&source_key()))),
        )
        .await,
    );
    assert_eq!(first.state, ImportState::Done);
    assert_eq!(first.counts.get("new"), Some(&2));
    assert_eq!(first.position, 2);
    assert!(first.authenticated);
    // A writer key proposes no claim edge (its verb ceiling), so the
    // bundle's edge is reported, named with the refusal (d6).
    assert_eq!(first.counts.get("edges_proposed"), Some(&0));
    assert_eq!(first.counts.get("edges_reported"), Some(&1));
    assert_eq!(first.refusals.len(), 1, "{:?}", first.refusals);
    assert_eq!(first.refusals[0].claim_key, "edge:art-1:art-2");
    assert!(first.refusals[0].code.starts_with("VerbCeiling"));
    let target = first.target_workspace_id;
    assert_eq!(held_count(&rig, target).await, 2);

    let again = upload(&rig, &rig.writer, &bytes).await;
    let second = imported(
        import(
            &rig,
            &rig.writer,
            &request(again, "notes", Some(fingerprint(&source_key()))),
        )
        .await,
    );
    assert_eq!(second.counts.get("new"), None);
    assert_eq!(second.counts.get("unchanged"), Some(&2));
    assert_eq!(second.position, 0, "the re-import recorded no version");
    assert_eq!(held_count(&rig, target).await, 2, "nothing was written");
}

/// Every imported version is held, authored by the importing key, and a
/// key outside the target's audience reads none of it (d6; R3).
async fn imported_versions_are_held_and_authored_by_the_importer<S: JournalStore + Clone>(
    store: S,
) {
    let rig = rig(store).await;
    let up = upload(&rig, &rig.writer, &first_bundle()).await;
    let done = imported(
        import(
            &rig,
            &rig.writer,
            &request(up, "notes", Some(fingerprint(&source_key()))),
        )
        .await,
    );
    let view = rig
        .store
        .import_target_view(
            rig.tenant_id,
            done.target_workspace_id,
            SOURCE_SERVER,
            SOURCE_TENANT,
        )
        .await
        .expect("the view");
    assert_eq!(view.tips.len(), 2);
    assert!(view.tips.iter().all(|t| t.written_by_import));
    let mut carried: Vec<&str> = view.imported.iter().map(|(a, _)| a.as_str()).collect();
    carried.sort_unstable();
    assert_eq!(carried, ["art-1", "art-2"]);
    for id in view.tips.iter().map(|t| t.memory_id) {
        let m = rig
            .store
            .memory(rig.tenant_id, id)
            .await
            .expect("read")
            .expect("the row");
        assert_eq!(m.status, MemoryStatus::Held);
        assert_eq!(m.author_key_id, rig.writer_id);
        assert!(
            rig.app.get_memory(rig.stranger.as_str(), id).await.is_err(),
            "another user's key reads no imported held row"
        );
    }
}

/// d6: a target that is not in progress is refused by name.
async fn an_import_into_a_non_in_progress_workspace_is_refused<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    // Another key's standard workspace (ADR-0030 d1, amended 2026-11-18:
    // only the importing key's own empty one is marked on a retry).
    rig.app
        .create_workspace(
            rig.admin.as_str(),
            &name("plain"),
            Some(Reference::parse("ada").expect("a reference")),
            None,
        )
        .await
        .expect("a standard workspace");
    let up = upload(&rig, &rig.writer, &first_bundle()).await;
    let refused = import(
        &rig,
        &rig.writer,
        &request(up, "plain", Some(fingerprint(&source_key()))),
    )
    .await;
    assert!(
        matches!(refused, Err(AppError::ImportTargetNotInProgress { .. })),
        "{refused:?}"
    );
    let session = rig
        .store
        .upload_session(rig.tenant_id, up)
        .await
        .expect("read")
        .expect("the session");
    assert_eq!(session.state, UploadState::Completed, "nothing bound it");
}

/// R11: a first import from a source names its key; a later bundle under
/// another key, or with none named, is `import.source_key_changed`.
async fn the_source_key_is_pinned_at_the_first_import<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let bytes = first_bundle();
    let up = upload(&rig, &rig.writer, &bytes).await;
    assert!(matches!(
        import(&rig, &rig.writer, &request(up, "notes", None)).await,
        Err(AppError::ImportExpectKeyRequired)
    ));
    imported(
        import(
            &rig,
            &rig.writer,
            &request(up, "notes", Some(fingerprint(&source_key()))),
        )
        .await,
    );
    let forged = bundle_of(
        vec![line("art-1", 2, "deploy-day", "Deploys go out on Fridays.")],
        "2026-11-18T09:00:00.000000Z",
        &other_key(),
    );
    let up = upload(&rig, &rig.writer, &forged).await;
    assert_eq!(
        bundle_code(
            import(
                &rig,
                &rig.writer,
                &request(up, "notes", Some(fingerprint(&other_key())))
            )
            .await
        ),
        "import.source_key_changed"
    );
    assert_eq!(
        bundle_code(import(&rig, &rig.writer, &request(up, "notes", None)).await),
        "import.source_key_changed"
    );
    // The bundle names a key other than the one expected.
    assert_eq!(
        bundle_code(
            import(
                &rig,
                &rig.writer,
                &request(up, "notes", Some(fingerprint(&source_key())))
            )
            .await
        ),
        "bundle.signature_unexpected"
    );
}

/// d8: a bundle older than the ledger is refused, and admitted under
/// `allow_older`, which the import records.
async fn an_older_bundle_is_refused_unless_allow_older<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let expect = Some(fingerprint(&source_key()));
    let newer = bundle_of(
        vec![line("art-1", 2, "deploy-day", "Deploys go out on Fridays.")],
        "2026-11-18T09:00:00.000000Z",
        &source_key(),
    );
    let up = upload(&rig, &rig.writer, &newer).await;
    imported(import(&rig, &rig.writer, &request(up, "notes", expect.clone())).await);
    let up = upload(&rig, &rig.writer, &first_bundle()).await;
    assert_eq!(
        bundle_code(import(&rig, &rig.writer, &request(up, "notes", expect.clone())).await),
        "import.bundle_older"
    );
    let mut allowed = request(up, "notes", expect);
    allowed.allow_older = true;
    let done = imported(import(&rig, &rig.writer, &allowed).await);
    assert!(done.allow_older);
    assert_eq!(done.counts.get("new"), Some(&1), "rollback is new here");
    let stats = rig.app.import_stats(rig.tenant_id).await.expect("stats");
    assert_eq!(stats.allow_older, 1);
}

/// d11, I4: only a person's writer key imports; a dry run binds nothing;
/// an import binds its upload once and nulls the upload's key at its end.
async fn the_upload_binds_once_and_its_key_is_nulled_at_the_end<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let expect = Some(fingerprint(&source_key()));
    let bytes = first_bundle();
    let agents = upload(&rig, &rig.agent, &bytes).await;
    assert!(matches!(
        import(&rig, &rig.agent, &request(agents, "notes", expect.clone())).await,
        Err(AppError::ImportPersonOnly { .. })
    ));

    let up = upload(&rig, &rig.writer, &bytes).await;
    // Another key never sees the session.
    assert!(matches!(
        import(&rig, &rig.stranger, &request(up, "notes", expect.clone())).await,
        Err(AppError::NotFound)
    ));
    let mut dry = request(up, "notes", expect.clone());
    dry.dry_run = true;
    match import(&rig, &rig.writer, &dry).await {
        Ok(ImportAnswer::Planned(planned)) => {
            assert_eq!(planned.plan.keys.len(), 2);
            assert_eq!(planned.target_workspace_id, None, "notes would be created");
        }
        other => panic!("expected a plan, got {other:?}"),
    }
    let session = rig
        .store
        .upload_session(rig.tenant_id, up)
        .await
        .expect("read")
        .expect("the session");
    assert_eq!(
        session.state,
        UploadState::Completed,
        "a dry run binds nothing"
    );

    imported(import(&rig, &rig.writer, &request(up, "notes", expect.clone())).await);
    let session = rig
        .store
        .upload_session(rig.tenant_id, up)
        .await
        .expect("read")
        .expect("the session");
    assert_eq!(session.state, UploadState::Bound);
    assert!(
        session.part_key.is_none(),
        "the import's end shreds the key"
    );
    assert!(matches!(
        import(&rig, &rig.writer, &request(up, "notes", expect)).await,
        Err(AppError::Upload(UploadRefusal::SessionInvalid {
            state: UploadState::Bound
        }))
    ));
}

/// d7: the server runs the reader's checks before it writes; a `..` entry
/// is `bundle.path_unsafe`, and nothing is bound or written.
async fn a_bundle_entry_with_dotdot_is_refused_path_unsafe<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let tar = rename_second_entry(&first_bundle(), "../workspaces.jsonl");
    let up = upload(&rig, &rig.writer, &tar).await;
    assert_eq!(
        bundle_code(
            import(
                &rig,
                &rig.writer,
                &request(up, "notes", Some(fingerprint(&source_key())))
            )
            .await
        ),
        "bundle.path_unsafe"
    );
    // A tampered line is the digest check's.
    let mut tar = first_bundle();
    let at = find(&tar, b"Tuesdays").expect("the line");
    tar[at] = b'W';
    let up = upload(&rig, &rig.writer, &tar).await;
    assert_eq!(
        bundle_code(
            import(
                &rig,
                &rig.writer,
                &request(up, "notes", Some(fingerprint(&source_key())))
            )
            .await
        ),
        "bundle.digest_mismatch"
    );
    let session = rig
        .store
        .upload_session(rig.tenant_id, up)
        .await
        .expect("read")
        .expect("the session");
    assert_eq!(session.state, UploadState::Completed);
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The tar with its second entry's name replaced, written raw as a forger
/// would, so the writer's own path checks never see it.
fn rename_second_entry(tar: &[u8], new_name: &str) -> Vec<u8> {
    let mut archive = tar::Archive::new(tar);
    let entries: Vec<(String, Vec<u8>)> = archive
        .entries()
        .expect("entries")
        .raw(true)
        .map(|e| {
            let mut e = e.expect("entry");
            let path = String::from_utf8_lossy(&e.header().path_bytes()).into_owned();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut bytes).expect("read");
            (path, bytes)
        })
        .collect();
    let mut builder = tar::Builder::new(Vec::new());
    for (i, (path, bytes)) in entries.iter().enumerate() {
        let path = if i == 1 { new_name } else { path.as_str() };
        let mut header = tar::Header::new_ustar();
        header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder.append(&header, bytes.as_slice()).expect("append");
    }
    builder.into_inner().expect("finish")
}

/// An import's target: an in-progress workspace `name` the importing key
/// created through a first import of `first_bundle`, and its id.
async fn imported_target<S: JournalStore + Clone>(rig: &Rig<S>, target: &str) -> ImportRecord {
    let up = upload(rig, &rig.writer, &first_bundle()).await;
    imported(
        import(
            rig,
            &rig.writer,
            &request(up, target, Some(fingerprint(&source_key()))),
        )
        .await,
    )
}

/// The import row an `import_begin` law begins: `upload_id` into
/// `target`, signed by `signer`.
fn begun<S>(rig: &Rig<S>, upload_id: Uuid, target: Uuid, signer: &SigningKey) -> ImportRecord {
    ImportRecord {
        tenant_id: rig.tenant_id,
        import_id: Uuid::from_u128(
            0x1_0000_0000 + u128::from(NAMES.fetch_add(1, Ordering::Relaxed)),
        ),
        target_workspace_id: target,
        importer_key_id: rig.writer_id,
        upload_id,
        source_server_id: "00000000-0000-4000-8000-0000000000f2".to_owned(),
        source_tenant_id: SOURCE_TENANT.to_owned(),
        manifest_digest: "sha256:00".to_owned(),
        signer_fingerprint: fingerprint(signer),
        authenticated: true,
        allow_older: false,
        state: ImportState::Running,
        position: 0,
        counts: std::collections::BTreeMap::new(),
        refusals: Vec::new(),
        created_at: rig.clock_now(),
        ended_at: None,
    }
}

impl<S> Rig<S> {
    fn clock_now(&self) -> oml_app::domain::EventTime {
        use oml_app::ports::Clock as _;
        self.clock.now()
    }
}

/// I12: two first imports from one source into one target that begin at
/// once pin one key; the second, under another key, writes nothing.
async fn two_concurrent_first_imports_pin_one_key<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let target = imported_target(&rig, "notes").await.target_workspace_id;
    let a = upload(&rig, &rig.writer, &first_bundle()).await;
    let b = upload(&rig, &rig.writer, &first_bundle()).await;
    let at = rig.clock_now();
    let first = begun(&rig, a, target, &source_key());
    let second = begun(&rig, b, target, &other_key());
    let (one, two) = tokio::join!(
        rig.store.import_begin(&first, &at),
        rig.store.import_begin(&second, &at)
    );
    let outcomes = [one.expect("begin"), two.expect("begin")];
    let begun_count = outcomes
        .iter()
        .filter(|o| **o == ImportBegin::Begun)
        .count();
    assert_eq!(begun_count, 1, "{outcomes:?}");
    assert!(
        outcomes
            .iter()
            .any(|o| matches!(o, ImportBegin::KeyPinned { .. })),
        "{outcomes:?}"
    );
    // The refused begin bound nothing.
    let unbound = if outcomes[0] == ImportBegin::Begun {
        b
    } else {
        a
    };
    let session = rig
        .store
        .upload_session(rig.tenant_id, unbound)
        .await
        .expect("read")
        .expect("the session");
    assert_eq!(session.state, UploadState::Completed);
}

/// I12: a first import that failed still pins the key it named, so a
/// forged bundle under another key is `import.source_key_changed`.
async fn a_failed_first_import_still_pins_its_key_against_a_forged_bundle<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let target = rig
        .app
        .create_workspace(rig.writer.as_str(), &name("landing"), None, None)
        .await
        .expect("a workspace")
        .workspace
        .workspace_id;
    let up = upload(&rig, &rig.writer, &first_bundle()).await;
    let at = rig.clock_now();
    let mut first = begun(&rig, up, target, &source_key());
    first.source_server_id = SOURCE_SERVER.to_owned();
    assert_eq!(
        rig.store.import_begin(&first, &at).await.expect("begin"),
        ImportBegin::Begun
    );
    rig.store
        .import_failed(
            rig.tenant_id,
            first.import_id,
            &rig.clock_now(),
            &std::collections::BTreeMap::new(),
            &[],
        )
        .await
        .expect("failed");
    let forged = bundle_of(
        vec![line("art-1", 2, "deploy-day", "Deploys go out on Fridays.")],
        "2026-11-18T09:00:00.000000Z",
        &other_key(),
    );
    let up = upload(&rig, &rig.writer, &forged).await;
    assert_eq!(
        bundle_code(
            import(
                &rig,
                &rig.writer,
                &request(up, "landing", Some(fingerprint(&other_key())))
            )
            .await
        ),
        "import.source_key_changed"
    );
}

/// ADR-0004 d3, readings of 2026-11-18: an upload expired at the instant
/// of the bind binds nothing (L2).
async fn an_upload_past_its_expiry_at_the_bind_binds_nothing<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let target = imported_target(&rig, "notes").await.target_workspace_id;
    let up = upload(&rig, &rig.writer, &first_bundle()).await;
    // A day and an hour after completion: past the unbound expiry.
    let at = oml_app::domain::EventTime::parse("2026-10-30T13:00:00.000000Z").expect("an instant");
    assert_eq!(
        rig.store
            .import_begin(&begun(&rig, up, target, &source_key()), &at)
            .await
            .expect("begin"),
        ImportBegin::NotBindable
    );
}

/// ADR-0004 d3, readings of 2026-11-18 (M2): bytes changed at rest after
/// completion, sealed under the session's own key, are
/// `upload.digest_mismatch`, and nothing is written.
async fn bytes_changed_at_rest_after_completion_are_refused_digest_mismatch<
    S: JournalStore + Clone,
>(
    store: S,
) {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    use oml_app::ports::ObjectStore as _;
    let rig = rig(store).await;
    let bytes = first_bundle();
    let up = upload(&rig, &rig.writer, &bytes).await;
    let session = rig
        .store
        .upload_session(rig.tenant_id, up)
        .await
        .expect("read")
        .expect("the session");
    let mut changed = bytes.clone();
    let at = changed
        .windows(8)
        .position(|w| w == b"Tuesdays")
        .expect("the line");
    changed[at] = b'W';
    let nonce = [7_u8; 24];
    let mut aad = up.as_bytes().to_vec();
    aad.extend_from_slice(&1_u64.to_be_bytes());
    let sealed = XChaCha20Poly1305::new((&session.part_key.expect("a key")).into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &changed,
                aad: &aad,
            },
        )
        .expect("seals");
    let mut object = nonce.to_vec();
    object.extend_from_slice(&sealed);
    rig.objects
        .put(&format!("uploads/{}/{up}/000001", rig.tenant_id), object)
        .await
        .expect("put");
    assert!(
        matches!(
            import(
                &rig,
                &rig.writer,
                &request(up, "notes", Some(fingerprint(&source_key())))
            )
            .await,
            Err(AppError::Upload(UploadRefusal::DigestMismatch))
        ),
        "the changed bytes are refused"
    );
}

/// d7: a plan over the account's meters is `import.over_quota`, and the
/// pre-pass writes nothing: no workspace, no bind, no version.
async fn the_over_quota_pre_pass_writes_nothing<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let quotas = oml_app::domain::signup::Quotas {
        memories: 1,
        ..oml_app::domain::signup::Quotas::default()
    };
    let Rig {
        app,
        store,
        tenant_id,
        writer,
        ..
    } = rig;
    let app = app
        .with_signup(oml_app::types::SignupPolicy {
            mode: oml_app::domain::signup::SignupMode::Open,
            tenant_id,
            issuer: name("root-admin"),
            terms_version: "2026-11-06".to_owned(),
            terms_url: "https://example.com/terms".to_owned(),
            privacy_url: "https://example.com/privacy".to_owned(),
            per_ip_per_hour: 100,
            quotas,
        })
        .with_quota_profiles(oml_app::types::QuotaProfiles::only(quotas));
    let bytes = first_bundle();
    let id = app
        .open_upload(
            writer.as_str(),
            UploadPurpose::Import,
            bytes.len() as u64,
            &sha256_hex(&bytes),
        )
        .await
        .expect("open")
        .upload_id;
    app.put_upload_part(writer.as_str(), id, 1, &bytes)
        .await
        .expect("part");
    app.complete_upload(writer.as_str(), id)
        .await
        .expect("complete");
    let refused = app
        .import_bundle(
            writer.as_str(),
            &request(id, "notes", Some(fingerprint(&source_key()))),
            &code_of,
        )
        .await;
    assert!(
        matches!(refused, Err(AppError::ImportOverQuota(_))),
        "{refused:?}"
    );
    let session = store
        .upload_session(tenant_id, id)
        .await
        .expect("read")
        .expect("the session");
    assert_eq!(session.state, UploadState::Completed, "nothing bound it");
    assert!(
        matches!(
            app.workspace_info(
                writer.as_str(),
                Some(Reference::parse("notes").expect("ref"))
            )
            .await,
            Err(AppError::NotFound)
        ),
        "no target was created"
    );
}

/// d6, ADR-0080 d16: imported steering, guardrail and skill versions land held by the
/// importer, inside the in-progress target, and no key outside it reads
/// them.
async fn imported_steering_guardrail_and_skill_versions_land_held<S: JournalStore + Clone>(
    store: S,
) {
    let rig = rig(store).await;
    let mut steering = line("art-s", 1, "always-test", "Run the tests before a merge.");
    steering.kind = "steering".to_owned();
    let mut guardrail = line(
        "art-g",
        1,
        "no-force-push",
        "Never force-push a shared branch.",
    );
    guardrail.kind = "guardrail".to_owned();
    guardrail.guardrail = Some(json!({"severity": "should", "rule": null}));
    let mut skill = line(
        "art-k",
        1,
        "skill/release-notes",
        "Write the notes from the changelog.",
    );
    skill.kind = "skill".to_owned();
    skill.skill = Some(json!({"name": "release-notes", "description": "Writes release notes."}));
    let bytes = bundle_of(
        vec![steering, guardrail, skill],
        "2026-11-17T09:00:00.000000Z",
        &source_key(),
    );
    let up = upload(&rig, &rig.writer, &bytes).await;
    let done = imported(
        import(
            &rig,
            &rig.writer,
            &request(up, "notes", Some(fingerprint(&source_key()))),
        )
        .await,
    );
    assert_eq!(done.counts.get("new"), Some(&3), "{:?}", done.refusals);
    // ADR-0080 d10, d16: inside the in-progress target the held guardrail
    // is served unreviewed and asking.
    let served = rig
        .app
        .guardrails(
            rig.writer.as_str(),
            &oml_app::types::GuardrailRequest {
                workspace: Some(Reference::Id(done.target_workspace_id)),
                include_own_held: false,
            },
        )
        .await
        .expect("the guardrails read");
    let row = served
        .guardrails
        .iter()
        .find(|g| g.claim_key == "no-force-push")
        .expect("the imported guardrail is served inside");
    assert!(row.unreviewed);
    assert_eq!(
        row.effect_inside,
        Some(oml_app::domain::guardrail::Effect::Ask)
    );
    let view = rig
        .store
        .import_target_view(
            rig.tenant_id,
            done.target_workspace_id,
            SOURCE_SERVER,
            SOURCE_TENANT,
        )
        .await
        .expect("the view");
    for tip in &view.tips {
        let m = rig
            .store
            .memory(rig.tenant_id, tip.memory_id)
            .await
            .expect("read")
            .expect("the row");
        assert_eq!(m.status, MemoryStatus::Held, "{}", tip.claim_key);
        assert_eq!(m.author_key_id, rig.writer_id);
        assert!(
            rig.app
                .get_memory(rig.stranger.as_str(), tip.memory_id)
                .await
                .is_err(),
            "a key outside the target reads no held {}",
            tip.claim_key
        );
    }
}

/// d4, I14: each imported version carries its provenance from its own
/// commit, `workspace.imported` lists it, and the doctor counts none
/// without provenance.
async fn import_provenance_names_the_source_line_and_the_journal_lists_each_version<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let done = imported_target(&rig, "notes").await;
    let view = rig
        .store
        .import_target_view(
            rig.tenant_id,
            done.target_workspace_id,
            SOURCE_SERVER,
            SOURCE_TENANT,
        )
        .await
        .expect("the view");
    let tip = view
        .tips
        .iter()
        .find(|t| t.claim_key == "deploy-day")
        .expect("the tip");
    let provenance = rig
        .store
        .import_provenance(rig.tenant_id, tip.memory_id)
        .await
        .expect("read")
        .expect("provenance");
    assert_eq!(provenance["import_id"], done.import_id.to_string());
    assert_eq!(provenance["source_server_id"], SOURCE_SERVER);
    assert_eq!(provenance["source_tenant_id"], SOURCE_TENANT);
    assert_eq!(provenance["workspace"], "universe.ada.notes");
    assert_eq!(provenance["artifact_id"], "art-1");
    assert_eq!(provenance["version"], 1);
    assert_eq!(
        provenance["plaintext_digest"],
        sha256_digest(b"Deploys go out on Tuesdays.")
    );
    assert_eq!(provenance["author"]["pseudonym"], "a".repeat(64));
    assert_eq!(provenance["origin"], json!({"kind": "doc"}));
    assert_eq!(provenance["conflict"], false);
    assert_eq!(provenance["state"], "promoted");

    let events = rig
        .store
        .events_range(rig.tenant_id, 1, 10_000)
        .await
        .expect("events");
    let ended = events
        .iter()
        .find(|e| e.envelope.event_type == "workspace.imported")
        .expect("workspace.imported");
    let listed: Vec<&str> = ended.envelope.payload["versions"]
        .as_array()
        .expect("versions")
        .iter()
        .filter_map(|v| v["memory_id"].as_str())
        .collect();
    assert_eq!(listed.len(), 2);
    assert!(listed.contains(&tip.memory_id.to_string().as_str()));
    let stats = rig.app.import_stats(rig.tenant_id).await.expect("stats");
    assert_eq!(stats.unprovenanced, 0);
}

/// d11, I4: an import whose run fails still shreds its upload's key, and
/// the row ends `failed`.
async fn an_import_that_fails_shreds_its_upload_key<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let up = upload(&rig, &rig.writer, &first_bundle()).await;
    let started = rig
        .app
        .import_start(
            &rig.writer,
            &request(up, "notes", Some(fingerprint(&source_key()))),
        )
        .await
        .expect("starts");
    let ImportStart::Started(job) = started else {
        panic!("expected a begun import")
    };
    let id = job.record().import_id;
    // The row is ended under the run, so its last commit is refused.
    rig.store
        .import_failed(
            rig.tenant_id,
            id,
            &rig.clock_now(),
            &std::collections::BTreeMap::new(),
            &[],
        )
        .await
        .expect("ended");
    let finished = rig.app.import_finish(*job, &code_of).await;
    assert!(finished.import.is_err(), "{:?}", finished.import);
    assert_eq!(finished.shred_failure, None);
    let session = rig
        .store
        .upload_session(rig.tenant_id, up)
        .await
        .expect("read")
        .expect("the session");
    assert!(
        session.part_key.is_none(),
        "the failed run shredded the key"
    );
    let record = rig
        .store
        .import_record(rig.tenant_id, id)
        .await
        .expect("read")
        .expect("the row");
    assert_eq!(record.state, ImportState::Failed);
}

/// I11: an import running for more than an hour that no process runs is
/// ended `failed` by the sweep, its key nulled and its objects removed;
/// one this process runs is left running; the doctor counts it stopped
/// before the sweep.
async fn an_import_stopped_for_an_hour_is_ended_and_shredded_by_the_sweep<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let target = imported_target(&rig, "notes").await.target_workspace_id;
    let up = upload(&rig, &rig.writer, &first_bundle()).await;
    let at = rig.clock_now();
    let mut stopped = begun(&rig, up, target, &source_key());
    stopped.source_server_id = SOURCE_SERVER.to_owned();
    assert_eq!(
        rig.store.import_begin(&stopped, &at).await.expect("begin"),
        ImportBegin::Begun
    );
    let live = upload(&rig, &rig.writer, &first_bundle()).await;
    let started = rig
        .app
        .import_start(
            &rig.writer,
            &request(live, "notes", Some(fingerprint(&source_key()))),
        )
        .await;
    // The tenant runs one import at a time (I13).
    assert!(
        matches!(started, Ok(ImportStart::Started(_))),
        "{started:?}"
    );
    let ImportStart::Started(job) = started.expect("started") else {
        unreachable!()
    };
    let other = upload(&rig, &rig.writer, &first_bundle()).await;
    assert!(matches!(
        import(
            &rig,
            &rig.writer,
            &request(other, "notes", Some(fingerprint(&source_key())))
        )
        .await,
        Err(AppError::ImportBusy)
    ));
    rig.clock.advance(3_601);
    let stats = rig.app.import_stats(rig.tenant_id).await.expect("stats");
    assert_eq!(stats.stopped, 2, "both run past the hour before the sweep");
    let sweep = rig.app.sweep_uploads(rig.tenant_id).await.expect("sweep");
    assert_eq!(sweep.imports_stopped, 1, "the live import is left running");
    let record = rig
        .store
        .import_record(rig.tenant_id, stopped.import_id)
        .await
        .expect("read")
        .expect("the row");
    assert_eq!(record.state, ImportState::Failed);
    let session = rig
        .store
        .upload_session(rig.tenant_id, up)
        .await
        .expect("read")
        .expect("the session");
    assert!(session.part_key.is_none(), "its staging key is gone");
    assert!(
        rig.objects
            .names()
            .iter()
            .all(|n| !n.contains(&up.to_string())),
        "its objects are removed"
    );
    let live_row = rig
        .store
        .import_record(rig.tenant_id, job.record().import_id)
        .await
        .expect("read")
        .expect("the row");
    assert_eq!(live_row.state, ImportState::Running);
    let finished = rig.app.import_finish(*job, &code_of).await;
    assert!(finished.import.is_ok(), "{:?}", finished.import);
}

/// I16 (ADR-0030 d1, amended 2026-11-18): a new target is created in
/// progress in one commit; a retry against the importing key's own empty
/// standard workspace marks it.
async fn a_new_target_is_created_in_progress_and_a_retry_marks_its_own_empty_one<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let before = rig
        .store
        .events_range(rig.tenant_id, 1, 10_000)
        .await
        .expect("events")
        .len();
    let done = imported_target(&rig, "notes").await;
    let events = rig
        .store
        .events_range(rig.tenant_id, 1, 10_000)
        .await
        .expect("events");
    let created = events[before..]
        .iter()
        .position(|e| e.envelope.event_type == "workspace.create")
        .expect("the create");
    assert_eq!(
        events[before + created + 1].envelope.event_type,
        "workspace.mode_set",
        "the mark follows the create in the same chain commit"
    );
    let ancestry = rig
        .store
        .workspace_ancestry(rig.tenant_id, done.target_workspace_id)
        .await
        .expect("read")
        .expect("the node");
    assert!(ancestry[0].in_progress);

    // A crash before the mark of an earlier build left a standard, empty
    // workspace the importing key created.
    rig.app
        .create_workspace(rig.writer.as_str(), &name("retry"), None, None)
        .await
        .expect("a standard workspace");
    let up = upload(&rig, &rig.writer, &first_bundle()).await;
    let done = imported(
        import(
            &rig,
            &rig.writer,
            &request(up, "retry", Some(fingerprint(&source_key()))),
        )
        .await,
    );
    assert_eq!(done.counts.get("new"), Some(&2));
}

/// Runs every import law over the store `make` returns.
pub async fn run_import_suite<S, F, Fut>(make: F)
where
    S: JournalStore + Clone,
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = S>,
{
    a_reimport_of_the_same_bundle_writes_nothing(make().await).await;
    imported_versions_are_held_and_authored_by_the_importer(make().await).await;
    an_import_into_a_non_in_progress_workspace_is_refused(make().await).await;
    the_source_key_is_pinned_at_the_first_import(make().await).await;
    an_older_bundle_is_refused_unless_allow_older(make().await).await;
    the_upload_binds_once_and_its_key_is_nulled_at_the_end(make().await).await;
    a_bundle_entry_with_dotdot_is_refused_path_unsafe(make().await).await;
    two_concurrent_first_imports_pin_one_key(make().await).await;
    a_failed_first_import_still_pins_its_key_against_a_forged_bundle(make().await).await;
    an_upload_past_its_expiry_at_the_bind_binds_nothing(make().await).await;
    bytes_changed_at_rest_after_completion_are_refused_digest_mismatch(make().await).await;
    the_over_quota_pre_pass_writes_nothing(make().await).await;
    imported_steering_guardrail_and_skill_versions_land_held(make().await).await;
    import_provenance_names_the_source_line_and_the_journal_lists_each_version(make().await).await;
    an_import_that_fails_shreds_its_upload_key(make().await).await;
    an_import_stopped_for_an_hour_is_ended_and_shredded_by_the_sweep(make().await).await;
    a_new_target_is_created_in_progress_and_a_retry_marks_its_own_empty_one(make().await).await;
}
