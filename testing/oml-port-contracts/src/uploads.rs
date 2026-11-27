//! Upload sessions (ADR-0004 d3, as amended 2026-11-17; #451), stated over
//! the application and run identically over every store, the in-memory
//! double and PostgreSQL: a bundle of several parts uploads, resumes after an
//! interruption and verifies at completion; an unbound upload expires;
//! another key can neither read nor bind it; every refusal is named.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use oml_app::App;
use oml_app::domain::access::Covers;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::upload::{PART_SIZE, UploadPurpose, UploadRefusal, UploadState};
use oml_app::domain::{EventTime, Role, UsedBy};
use oml_app::doubles::{MarkerScanner, MemObjects, MemSigner, SeqIdGen};
use oml_app::ports::{Clock, JournalStore};
use oml_app::types::{AppError, BootstrapRequest, GrantRequest, IssueKeyRequest};

const TEST_SEED: [u8; 32] = [7; 32];
const HOUR: i64 = 3_600_000_000;

/// A clock the test moves by any amount, days included: the fixed clock
/// of the doubles stays inside one day, and expiry is counted in days.
#[derive(Clone)]
struct MovableClock(Arc<AtomicI64>);

impl MovableClock {
    fn new() -> Self {
        Self(Arc::new(AtomicI64::new(
            EventTime::parse("2026-11-17T09:00:00.000000Z")
                .expect("a fixed instant")
                .unix_micros(),
        )))
    }

    fn advance_hours(&self, hours: i64) {
        self.0.fetch_add(hours * HOUR, Ordering::SeqCst);
    }
}

impl Clock for MovableClock {
    fn now(&self) -> EventTime {
        EventTime::from_unix_micros(self.0.fetch_add(1, Ordering::SeqCst)).expect("an instant")
    }
}

type TestApp<S> = App<S, MemSigner, MovableClock, SeqIdGen, MarkerScanner>;

struct Rig<S> {
    app: TestApp<S>,
    store: S,
    objects: Arc<MemObjects>,
    clock: MovableClock,
    tenant_id: Uuid,
    admin: String,
}

async fn rig_with<S: JournalStore + Clone>(
    store: S,
    max_bytes: u64,
    tenant_max_bytes: u64,
) -> Rig<S> {
    let objects = Arc::new(MemObjects::new());
    let clock = MovableClock::new();
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
    .with_uploads(objects.clone(), max_bytes, tenant_max_bytes);
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("root-admin"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    Rig {
        app,
        store,
        objects,
        clock,
        tenant_id: boot.tenant_id,
        admin: boot.admin_key.value,
    }
}

async fn rig<S: JournalStore + Clone>(store: S) -> Rig<S> {
    rig_with(
        store,
        oml_app::domain::upload::DEFAULT_MAX_BYTES,
        oml_app::domain::upload::DEFAULT_TENANT_MAX_BYTES,
    )
    .await
}

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

async fn key<S: JournalStore + Clone>(rig: &Rig<S>, key_name: &str, role: Role) -> String {
    rig.app
        .issue_key(
            &rig.admin,
            &IssueKeyRequest {
                profile: None,
                role,
                name: name(key_name),
                principal: format!("{key_name}/laptop"),
                used_by: Some(UsedBy::Person),
                default_workspace: None,
                crew: None,
                grants: vec![GrantRequest {
                    node: Reference::parse("universe").expect("a reference"),
                    role,
                    deny: Vec::new(),
                    covers: Covers::Subtree,
                    create: false,
                }],
                user: None,
                connect_code: false,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("issue a key")
        .value
}

/// `len` bytes no two parts of which are alike.
fn bundle(len: u64) -> Vec<u8> {
    (0..len)
        .map(|i| (i % 251) as u8 ^ (i / 8_388_608) as u8)
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn part(bytes: &[u8], n: u64) -> &[u8] {
    let start = ((n - 1) * PART_SIZE) as usize;
    let end = (start + PART_SIZE as usize).min(bytes.len());
    &bytes[start..end]
}

fn refusal(result: Result<impl std::fmt::Debug, AppError>) -> UploadRefusal {
    match result {
        Err(AppError::Upload(refusal)) => refusal,
        other => panic!("expected an upload refusal, got {other:?}"),
    }
}

async fn a_bundle_of_several_parts_uploads_resumes_after_an_interruption_and_verifies_at_completion<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let alice = key(&rig, "alice-writer", Role::Submitter).await;
    let bytes = bundle(2 * PART_SIZE + 4_000);
    let opened = rig
        .app
        .open_upload(
            alice.as_str(),
            UploadPurpose::Import,
            bytes.len() as u64,
            &sha256_hex(&bytes),
        )
        .await
        .expect("open");
    assert_eq!(opened.state, UploadState::Open);
    assert_eq!(opened.missing(), vec![1, 2, 3]);
    let id = opened.upload_id;

    // The first part lands, then the connection drops before the second.
    rig.app
        .put_upload_part(alice.as_str(), id, 1, part(&bytes, 1))
        .await
        .expect("part 1");
    // The store fails the next write: the part is refused by name and the
    // session still says what is missing.
    rig.objects.set_failing(true);
    assert_eq!(
        refusal(
            rig.app
                .put_upload_part(alice.as_str(), id, 2, part(&bytes, 2))
                .await
        ),
        UploadRefusal::StorageFailed
    );
    rig.objects.set_failing(false);
    let status = rig
        .app
        .upload_status(alice.as_str(), id)
        .await
        .expect("status");
    assert_eq!(status.missing(), vec![2, 3]);

    // The retry resumes where the status says, and part 1 sent again
    // replaces itself.
    for n in [3, 2, 1] {
        rig.app
            .put_upload_part(alice.as_str(), id, n, part(&bytes, n))
            .await
            .expect("resumed part");
    }
    let done = rig
        .app
        .complete_upload(alice.as_str(), id)
        .await
        .expect("complete");
    assert_eq!(done.state, UploadState::Completed);
    assert!(done.missing().is_empty());
    // An unbound completed upload expires 24 hours after completion.
    let completed = done
        .completed_at
        .as_ref()
        .expect("completed at")
        .unix_micros();
    assert_eq!(
        done.expires_at.as_ref().expect("an expiry").unix_micros(),
        completed + 24 * HOUR
    );
}

async fn a_part_is_stored_as_ciphertext_and_nothing_about_an_upload_is_journalled<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let alice = key(&rig, "alice-writer", Role::Submitter).await;
    let head = rig.store.chain_head(rig.tenant_id).await.expect("head");
    let bytes = b"a decrypted bundle, in plaintext".to_vec();
    let id = rig
        .app
        .open_upload(
            alice.as_str(),
            UploadPurpose::Import,
            bytes.len() as u64,
            &sha256_hex(&bytes),
        )
        .await
        .expect("open")
        .upload_id;
    rig.app
        .put_upload_part(alice.as_str(), id, 1, &bytes)
        .await
        .expect("part");
    rig.app
        .complete_upload(alice.as_str(), id)
        .await
        .expect("complete");
    let names = rig.objects.names();
    assert_eq!(
        names,
        vec![format!("uploads/{}/{id}/000001", rig.tenant_id)]
    );
    let stored = rig.objects.raw(&names[0]).expect("stored");
    assert!(
        !stored.windows(bytes.len()).any(|w| w == bytes.as_slice()),
        "the stored part holds the plaintext"
    );
    assert_eq!(
        rig.store.chain_head(rig.tenant_id).await.expect("head"),
        head,
        "an upload journalled an event"
    );
}

async fn another_key_can_neither_read_nor_touch_an_upload<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let alice = key(&rig, "alice-writer", Role::Submitter).await;
    let bob = key(&rig, "bob-writer", Role::Submitter).await;
    let bytes = b"alice's bundle".to_vec();
    let id = rig
        .app
        .open_upload(
            alice.as_str(),
            UploadPurpose::Import,
            bytes.len() as u64,
            &sha256_hex(&bytes),
        )
        .await
        .expect("open")
        .upload_id;
    assert!(matches!(
        rig.app.upload_status(bob.as_str(), id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        rig.app.put_upload_part(bob.as_str(), id, 1, &bytes).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        rig.app.complete_upload(bob.as_str(), id).await,
        Err(AppError::NotFound)
    ));
    assert!(matches!(
        rig.app.abandon_upload(bob.as_str(), id).await,
        Err(AppError::NotFound)
    ));
    // A session that does not exist answers the same.
    assert!(matches!(
        rig.app.upload_status(alice.as_str(), Uuid::nil()).await,
        Err(AppError::NotFound)
    ));
    // Alice's own session is untouched.
    assert_eq!(
        rig.app
            .upload_status(alice.as_str(), id)
            .await
            .expect("status")
            .state,
        UploadState::Open
    );
}

/// A contributor and a submitter key open an upload; a reader, a reviewer
/// and an admin key are refused by their role (ADR-0004 d3; ADR-0093 d6).
async fn only_a_contributor_or_a_submitter_key_opens_an_upload<S: JournalStore + Clone>(store: S) {
    let rig = rig(store).await;
    let on_universe = |role| GrantRequest {
        node: Reference::parse("universe").expect("a reference"),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
        create: false,
    };
    let contributor = rig
        .app
        .issue_key(
            &rig.admin,
            &IssueKeyRequest {
                profile: None,
                role: Role::Contributor,
                name: name("erin-contributor"),
                principal: "erin/laptop".to_owned(),
                used_by: Some(UsedBy::Person),
                default_workspace: None,
                crew: None,
                grants: vec![on_universe(Role::Submitter), on_universe(Role::Reader)],
                user: None,
                connect_code: false,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("a contributor key")
        .value;
    let submitter = key(&rig, "sam-submitter", Role::Submitter).await;
    for opener in [&contributor, &submitter] {
        rig.app
            .open_upload(opener.as_str(), UploadPurpose::Import, 10, &"a".repeat(64))
            .await
            .expect("a contributor or a submitter opens an upload");
    }
    let reader = key(&rig, "rhea-reader", Role::Reader).await;
    assert!(matches!(
        rig.app
            .open_upload(reader.as_str(), UploadPurpose::Import, 10, &"a".repeat(64))
            .await,
        Err(AppError::VerbCeiling {
            role: Role::Reader,
            ..
        })
    ));
    let reviewer = key(&rig, "carol-reviewer", Role::Reviewer).await;
    assert!(matches!(
        rig.app
            .open_upload(
                reviewer.as_str(),
                UploadPurpose::Import,
                10,
                &"a".repeat(64)
            )
            .await,
        Err(AppError::VerbCeiling { .. })
    ));
    assert!(matches!(
        rig.app
            .open_upload(
                rig.admin.as_str(),
                UploadPurpose::Import,
                10,
                &"a".repeat(64)
            )
            .await,
        Err(AppError::VerbCeiling { .. })
    ));
}

async fn an_unbound_upload_expires_and_its_key_is_shredded_before_its_objects<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let alice = key(&rig, "alice-writer", Role::Submitter).await;
    let bytes = b"a bundle nobody imports".to_vec();
    let id = rig
        .app
        .open_upload(
            alice.as_str(),
            UploadPurpose::Import,
            bytes.len() as u64,
            &sha256_hex(&bytes),
        )
        .await
        .expect("open")
        .upload_id;
    rig.app
        .put_upload_part(alice.as_str(), id, 1, &bytes)
        .await
        .expect("part");
    rig.app
        .complete_upload(alice.as_str(), id)
        .await
        .expect("complete");

    // 23 hours on, the sweep leaves it.
    rig.clock.advance_hours(23);
    let sweep = rig.app.sweep_uploads(rig.tenant_id).await.expect("sweep");
    assert_eq!((sweep.expired, sweep.removed), (0, 0));

    // Past 24 hours, with the object store down: the key is shredded and
    // the objects stay, ciphertext no key opens, counted as unswept.
    rig.clock.advance_hours(2);
    assert_eq!(
        refusal(rig.app.put_upload_part(alice.as_str(), id, 1, &bytes).await),
        UploadRefusal::Expired
    );
    rig.objects.set_failing(true);
    let sweep = rig.app.sweep_uploads(rig.tenant_id).await.expect("sweep");
    assert_eq!((sweep.expired, sweep.removed, sweep.failures), (1, 0, 1));
    assert_eq!(sweep.stats.unswept, 1);
    let row = rig
        .store
        .upload_session(rig.tenant_id, id)
        .await
        .expect("read")
        .expect("the row stays");
    assert_eq!(row.state, UploadState::Expired);
    assert_eq!(row.part_key, None);
    assert_eq!(rig.objects.names().len(), 1);

    // The next sweep removes the objects.
    rig.objects.set_failing(false);
    let sweep = rig.app.sweep_uploads(rig.tenant_id).await.expect("sweep");
    assert_eq!((sweep.expired, sweep.removed, sweep.failures), (0, 1, 0));
    assert_eq!(sweep.stats.unswept, 0);
    assert!(rig.objects.names().is_empty());
    assert_eq!(
        rig.app
            .upload_status(alice.as_str(), id)
            .await
            .expect("status")
            .state,
        UploadState::Expired
    );
    rig.app
        .abandon_upload(alice.as_str(), id)
        .await
        .expect("abandoning an expired upload answers as abandoned and changes nothing");
    assert_eq!(
        rig.app
            .upload_status(alice.as_str(), id)
            .await
            .expect("status")
            .state,
        UploadState::Expired
    );
}

async fn an_open_session_slides_with_each_part_and_expires_72_hours_after_it_opened<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let alice = key(&rig, "alice-writer", Role::Submitter).await;
    let bytes = bundle(4 * PART_SIZE);
    let id = rig
        .app
        .open_upload(
            alice.as_str(),
            UploadPurpose::Import,
            bytes.len() as u64,
            &sha256_hex(&bytes),
        )
        .await
        .expect("open")
        .upload_id;
    // A part every 20 hours keeps it open for three of them...
    for n in 1..=3 {
        rig.clock.advance_hours(20);
        rig.app
            .put_upload_part(alice.as_str(), id, n, part(&bytes, n))
            .await
            .expect("a part inside the window");
    }
    // ...and at 73 hours from its opening it is expired however recent its
    // last part.
    rig.clock.advance_hours(13);
    assert_eq!(
        refusal(
            rig.app
                .put_upload_part(alice.as_str(), id, 4, part(&bytes, 4))
                .await
        ),
        UploadRefusal::Expired
    );
}

async fn every_upload_refusal_is_named<S: JournalStore + Clone>(store: S) {
    let rig = rig_with(store, 3 * PART_SIZE, 5 * PART_SIZE).await;
    let alice = key(&rig, "alice-writer", Role::Submitter).await;
    let bob = key(&rig, "bob-writer", Role::Submitter).await;
    let digest = |b: &[u8]| sha256_hex(b);

    // Over uploads.max_bytes.
    assert_eq!(
        refusal(
            rig.app
                .open_upload(
                    alice.as_str(),
                    UploadPurpose::Import,
                    3 * PART_SIZE + 1,
                    &"a".repeat(64)
                )
                .await
        ),
        UploadRefusal::TooLarge {
            max_bytes: 3 * PART_SIZE
        }
    );

    let bytes = bundle(PART_SIZE + 10);
    let id = rig
        .app
        .open_upload(
            alice.as_str(),
            UploadPurpose::Import,
            bytes.len() as u64,
            &digest(&bytes),
        )
        .await
        .expect("open")
        .upload_id;
    // A part of the wrong length, and a part number past the count.
    assert_eq!(
        refusal(
            rig.app
                .put_upload_part(alice.as_str(), id, 2, &bytes[..9])
                .await
        ),
        UploadRefusal::PartSize {
            expected: 10,
            got: 9
        }
    );
    assert_eq!(
        refusal(
            rig.app
                .put_upload_part(alice.as_str(), id, 3, &bytes[..10])
                .await
        ),
        UploadRefusal::PartNumber { parts: 2 }
    );
    assert_eq!(
        refusal(
            rig.app
                .put_upload_part(alice.as_str(), id, 0, &bytes[..10])
                .await
        ),
        UploadRefusal::PartNumber { parts: 2 }
    );
    // Completion with a part missing names it.
    rig.app
        .put_upload_part(alice.as_str(), id, 1, part(&bytes, 1))
        .await
        .expect("part 1");
    assert_eq!(
        refusal(rig.app.complete_upload(alice.as_str(), id).await),
        UploadRefusal::Incomplete { missing: vec![2] }
    );
    // The wrong bytes fail the session, shred its key and remove its parts.
    let mut wrong = part(&bytes, 2).to_vec();
    wrong[0] ^= 1;
    rig.app
        .put_upload_part(alice.as_str(), id, 2, &wrong)
        .await
        .expect("part 2");
    assert_eq!(
        refusal(rig.app.complete_upload(alice.as_str(), id).await),
        UploadRefusal::DigestMismatch
    );
    let failed = rig
        .app
        .upload_status(alice.as_str(), id)
        .await
        .expect("status");
    assert_eq!(failed.state, UploadState::Failed);
    assert_eq!(failed.part_key, None);
    assert!(rig.objects.names().is_empty());
    // A failed session takes no part.
    assert_eq!(
        refusal(
            rig.app
                .put_upload_part(alice.as_str(), id, 1, part(&bytes, 1))
                .await
        ),
        UploadRefusal::SessionInvalid {
            state: UploadState::Failed
        }
    );

    // The tenant's 5 parts' worth: Alice holds 3, so Bob's 3 more pass it.
    let three = 3 * PART_SIZE;
    rig.app
        .open_upload(
            alice.as_str(),
            UploadPurpose::Import,
            three,
            &"b".repeat(64),
        )
        .await
        .expect("open within the tenant's bytes");
    assert_eq!(
        refusal(
            rig.app
                .open_upload(bob.as_str(), UploadPurpose::Import, three, &"c".repeat(64))
                .await
        ),
        UploadRefusal::TenantLimit {
            max_bytes: 5 * PART_SIZE
        }
    );
    // A key's fifth open or unbound session.
    for _ in 0..3 {
        rig.app
            .open_upload(alice.as_str(), UploadPurpose::Import, 1, &"d".repeat(64))
            .await
            .expect("open");
    }
    assert_eq!(
        refusal(
            rig.app
                .open_upload(alice.as_str(), UploadPurpose::Import, 1, &"d".repeat(64))
                .await
        ),
        UploadRefusal::SessionLimit { limit: 4 }
    );
}

/// An app without an object store opens no upload: the refusal is
/// `upload.storage_failed`, and nothing is stored.
async fn without_an_object_store_no_upload_opens<S: JournalStore + Clone>(store: S) {
    let bare: TestApp<S> = App::new(
        store,
        MemSigner::new(TEST_SEED),
        MovableClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let boot = bare
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("bare"),
            admin_key_name: name("bare-admin"),
            admin_principal: "bare-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let writer = bare
        .issue_key(
            &boot.admin_key.value,
            &IssueKeyRequest {
                profile: None,
                role: Role::Submitter,
                name: name("dan-writer"),
                principal: "dan".to_owned(),
                used_by: Some(UsedBy::Person),
                default_workspace: None,
                crew: None,
                grants: Vec::new(),
                user: None,
                connect_code: false,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("issue")
        .value;
    assert_eq!(
        refusal(
            bare.open_upload(writer.as_str(), UploadPurpose::Import, 1, &"e".repeat(64))
                .await
        ),
        UploadRefusal::StorageFailed
    );
}

async fn an_abandoned_upload_is_shredded_and_abandoning_it_again_answers_the_same<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let alice = key(&rig, "alice-writer", Role::Submitter).await;
    let bytes = b"second thoughts".to_vec();
    let id = rig
        .app
        .open_upload(
            alice.as_str(),
            UploadPurpose::Import,
            bytes.len() as u64,
            &sha256_hex(&bytes),
        )
        .await
        .expect("open")
        .upload_id;
    rig.app
        .put_upload_part(alice.as_str(), id, 1, &bytes)
        .await
        .expect("part");
    rig.app
        .abandon_upload(alice.as_str(), id)
        .await
        .expect("abandon");
    let row = rig
        .app
        .upload_status(alice.as_str(), id)
        .await
        .expect("status");
    assert_eq!(row.state, UploadState::Abandoned);
    assert_eq!(row.part_key, None);
    assert!(rig.objects.names().is_empty());
    rig.app
        .abandon_upload(alice.as_str(), id)
        .await
        .expect("abandoning again answers as abandoned");
    assert_eq!(
        refusal(rig.app.complete_upload(alice.as_str(), id).await),
        UploadRefusal::SessionInvalid {
            state: UploadState::Abandoned
        }
    );
}

/// A part the store no longer holds, as after a pod lost its emptyDir, is
/// named at completion as `upload.incomplete`, never `storage_failed`, and
/// sending it again completes the session (ADR-0004 d3, note of
/// 2026-11-18).
async fn a_part_the_store_lost_is_named_incomplete_at_completion_and_sending_it_again_completes<
    S: JournalStore + Clone,
>(
    store: S,
) {
    let rig = rig(store).await;
    let alice = key(&rig, "alice-writer", Role::Submitter).await;
    let bytes = bundle(PART_SIZE + 4_000);
    let id = rig
        .app
        .open_upload(
            alice.as_str(),
            UploadPurpose::Import,
            bytes.len() as u64,
            &sha256_hex(&bytes),
        )
        .await
        .expect("open")
        .upload_id;
    for n in [1, 2] {
        rig.app
            .put_upload_part(alice.as_str(), id, n, part(&bytes, n))
            .await
            .expect("part");
    }
    let lost = rig
        .objects
        .names()
        .into_iter()
        .find(|n| n.ends_with("000002"))
        .expect("part 2's object");
    rig.objects.forget(&lost);
    assert_eq!(
        refusal(rig.app.complete_upload(alice.as_str(), id).await),
        UploadRefusal::Incomplete { missing: vec![2] }
    );
    // The status agrees with the refusal.
    let status = rig
        .app
        .upload_status(alice.as_str(), id)
        .await
        .expect("status");
    assert_eq!(status.missing(), vec![2]);
    rig.app
        .put_upload_part(alice.as_str(), id, 2, part(&bytes, 2))
        .await
        .expect("part 2 again");
    let done = rig
        .app
        .complete_upload(alice.as_str(), id)
        .await
        .expect("complete");
    assert_eq!(done.state, UploadState::Completed);
}

/// Every law of upload sessions.
pub async fn run_upload_suite<S, F, Fut>(make: F)
where
    S: JournalStore + Clone,
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = S>,
{
    a_bundle_of_several_parts_uploads_resumes_after_an_interruption_and_verifies_at_completion(
        make().await,
    )
    .await;
    a_part_is_stored_as_ciphertext_and_nothing_about_an_upload_is_journalled(make().await).await;
    another_key_can_neither_read_nor_touch_an_upload(make().await).await;
    only_a_contributor_or_a_submitter_key_opens_an_upload(make().await).await;
    an_unbound_upload_expires_and_its_key_is_shredded_before_its_objects(make().await).await;
    an_open_session_slides_with_each_part_and_expires_72_hours_after_it_opened(make().await).await;
    every_upload_refusal_is_named(make().await).await;
    without_an_object_store_no_upload_opens(make().await).await;
    an_abandoned_upload_is_shredded_and_abandoning_it_again_answers_the_same(make().await).await;
    a_part_the_store_lost_is_named_incomplete_at_completion_and_sending_it_again_completes(
        make().await,
    )
    .await;
}
