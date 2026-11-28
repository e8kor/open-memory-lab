//! The operator's recovery of a dormant tenant (ADR-0097 d19, d20; the
//! operator's answers 21, 22 and 24): dormancy read from the server
//! authentication table, failing closed when the table does not reach back
//! 30 days; a 72-hour wait any admin sign-in cancels; a sweep that
//! completes it once, whichever replica or start runs it; and the dormant
//! admins keep their flags.

use oml_app::App;
use oml_app::app::Capacity;
use oml_app::domain::name::Name;
use oml_app::domain::web::Email;
use oml_app::domain::{EventTime, Role, SignedEnvelope, UsedBy, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::{Clock as _, JournalStore as _, WebStore as _};
use oml_app::types::{
    AppError, Bearer, BootstrapRequest, BootstrapResult, IssueKeyRequest, IssuedKey,
    RecoverRefusal, RecoveryState, WebEmailRequest,
};
use uuid::Uuid;

const TEST_SEED: [u8; 32] = [
    0x19, 0x87, 0x45, 0x94, 0xaf, 0xc9, 0x3c, 0x44, 0xaf, 0x9c, 0x48, 0x6d, 0x6d, 0xd4, 0x8b, 0x68,
    0x51, 0x4b, 0x57, 0x36, 0xa7, 0xd6, 0x4c, 0xac, 0x72, 0xd9, 0x07, 0x2f, 0x98, 0xa6, 0x37, 0x1c,
];

const DAY: u64 = 86_400;
const HOUR: u64 = 3_600;

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

struct Rig {
    app: TestApp,
    store: MemStore,
    clock: FixedClock,
    boot: BootstrapResult,
}

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

fn email(user: &str) -> Email {
    Email::parse(&format!("{user}@example.com")).expect("an email")
}

fn app_over(store: &MemStore, clock: &FixedClock) -> TestApp {
    App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    )
}

/// A tenant whose one admin person is bob, with a bearer admin key linked
/// to him, and the bootstrap's own admin key.
async fn rig() -> (Rig, IssuedKey) {
    let store = MemStore::new();
    let clock = FixedClock::new();
    let app = app_over(&store, &clock);
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: name("acme"),
            admin_key_name: name("root-admin"),
            admin_principal: "acme-admin".to_owned(),
        })
        .await
        .expect("bootstrap");
    let r = Rig {
        app,
        store,
        clock,
        boot,
    };
    let (entry, _) = r
        .app
        .allow_web_email(
            r.admin(),
            &WebEmailRequest {
                email: email("bob"),
                user: name("bob"),
                admin: true,
            },
        )
        .await
        .expect("bob's admin entry");
    r.app.web_ensure_keys(&entry).await.expect("bob's keys");
    let bobs = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                profile: None,
                role: Role::Admin,
                name: name("bob-cli"),
                principal: "bob".to_owned(),
                used_by: Some(UsedBy::Person),
                default_workspace: None,
                crew: None,
                grants: Vec::new(),
                user: None,
                connect_code: false,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                person: Some(name("bob")),
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("bob's admin key");
    let web = r.session("bob", Capacity::Web).await;
    r.app
        .accept_key_link(Bearer::Session(&web), bobs.key_id.into())
        .await
        .expect("bob accepts the link");
    (r, bobs)
}

impl Rig {
    fn admin(&self) -> &str {
        self.boot.admin_key.value.as_str()
    }

    fn tenant(&self) -> Uuid {
        self.boot.tenant_id
    }

    async fn journal(&self) -> Vec<SignedEnvelope> {
        self.store
            .events_range(self.tenant(), 1, 10_000)
            .await
            .expect("journal")
    }

    /// The session key of `capacity` for `user`, from a sign-in now.
    async fn session(&self, user: &str, capacity: Capacity) -> oml_app::app::Caller {
        let entry = self
            .store
            .web_entry_by_email(&email(user).digest())
            .await
            .expect("a read")
            .expect("the entry");
        let token = self
            .app
            .web_session_open(&entry, 3600, None)
            .await
            .expect("a session")
            .token;
        let session = self
            .app
            .web_session(&token)
            .await
            .expect("a read")
            .expect("a live session");
        self.app
            .web_caller(&session, capacity)
            .await
            .expect("the session key")
    }

    async fn recover(&self) -> Result<oml_app::types::OperatorRecovery, AppError> {
        self.app
            .operator_recover(self.tenant(), name("carol"), email("carol"))
            .await
    }
}

fn refusal(result: Result<oml_app::types::OperatorRecovery, AppError>) -> RecoverRefusal {
    match result {
        Err(AppError::RecoverRefused(r)) => r,
        other => panic!("a refused recovery, not {other:?}"),
    }
}

/// A key's authentications of one UTC day are one row, written once and
/// coalesced in memory, and the next day is a row of its own; a person's
/// session key and an admin person's sign-in write theirs (ADR-0097,
/// answer 24).
#[tokio::test]
async fn a_key_authenticates_into_one_row_a_day() {
    let (r, bobs) = rig().await;
    let today = r.clock.now().as_str()[..10].to_owned();
    for _ in 0..3 {
        r.app.whoami(bobs.value.as_str()).await.expect("whoami");
    }
    let rows = r.store.authentication_rows(r.tenant());
    let bobs_rows: Vec<_> = rows.iter().filter(|(k, _, _)| *k == bobs.key_id).collect();
    assert_eq!(
        bobs_rows,
        [&(bobs.key_id, today.clone(), 1)],
        "one write a day"
    );
    r.clock.advance(DAY);
    r.app.whoami(bobs.value.as_str()).await.expect("whoami");
    let days = r
        .store
        .authentication_rows(r.tenant())
        .into_iter()
        .filter(|(k, _, _)| *k == bobs.key_id)
        .count();
    assert_eq!(days, 2, "the next day is a row of its own");
    // bob-admin authenticated at bob's sign-in, whichever capacity he used.
    let admin_session = r.session("bob", Capacity::Admin).await;
    let admin_rows = r
        .store
        .authentication_rows(r.tenant())
        .into_iter()
        .filter(|(k, _, _)| *k == admin_session.key().key_id)
        .count();
    assert!(admin_rows >= 1, "an admin person's sign-in is written");
}

/// The purge sweep deletes rows of a day more than 30 days back and keeps
/// the rest (answer 24).
#[tokio::test]
async fn the_purge_sweep_keeps_thirty_days_of_authentications() {
    let (r, bobs) = rig().await;
    let before = r.store.authentication_rows(r.tenant()).len();
    assert!(before > 0);
    r.clock.advance(30 * DAY);
    r.app.whoami(bobs.value.as_str()).await.expect("whoami");
    assert_eq!(
        r.app
            .purge_authentications(r.tenant())
            .await
            .expect("purge"),
        0,
        "a day 30 days back is kept"
    );
    r.clock.advance(DAY);
    let purged = r
        .app
        .purge_authentications(r.tenant())
        .await
        .expect("purge");
    assert_eq!(purged as usize, before, "a day 31 days back is purged");
    assert_eq!(r.store.authentication_rows(r.tenant()).len(), 1);
}

/// Recover fails closed with `log_too_short` when the authentication table
/// does not reach back 30 days, from a fresh install or purged rows, and
/// never reads a missing row as dormancy (answers 21, 24).
#[tokio::test]
async fn recovery_is_refused_when_the_authentication_table_does_not_reach_back_thirty_days() {
    let (r, _) = rig().await;
    assert_eq!(refusal(r.recover().await), RecoverRefusal::LogTooShort);
    // Installed ten days ago: the old rows count from the install only.
    r.clock.advance(31 * DAY);
    let installed = EventTime::from_unix_micros(
        r.clock.now().unix_micros() - 10 * i64::try_from(DAY).expect("a day") * 1_000_000,
    )
    .expect("a time");
    r.store.set_authentications_installed_at(installed);
    assert_eq!(refusal(r.recover().await), RecoverRefusal::LogTooShort);
    // Every row purged: nothing is covered, so nothing is dormant.
    r.store
        .set_authentications_installed_at(EventTime::from_unix_micros(0).expect("the epoch"));
    r.app
        .purge_authentications(r.tenant())
        .await
        .expect("purge");
    assert!(r.store.authentication_rows(r.tenant()).is_empty());
    assert_eq!(refusal(r.recover().await), RecoverRefusal::LogTooShort);
    let stats = r.app.fix_stats(r.tenant()).await.expect("stats");
    assert_eq!(stats.authentication_coverage_days, 0);
}

/// Recovery runs only after 30 days in which no admin key of the tenant
/// authenticated and no admin person signed in, and any admin sign-in in
/// its 72 hours cancels it (ADR-0097 d19; answers 21, 22).
#[tokio::test]
async fn recovery_runs_only_after_thirty_days_without_an_admin_person_and_any_sign_in_cancels_it() {
    let (r, bobs) = rig().await;
    r.clock.advance(31 * DAY);
    r.app.whoami(bobs.value.as_str()).await.expect("bob's key");
    assert_eq!(refusal(r.recover().await), RecoverRefusal::AdminActive);
    r.clock.advance(31 * DAY);
    let before = r.journal().await.len();
    let requested = r.recover().await.expect("dormant: requested");
    assert_eq!(requested.recovery.state, RecoveryState::Requested);
    let events = r.journal().await;
    let types: Vec<&str> = events[before..]
        .iter()
        .map(|e| e.envelope.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        [
            event_types::OPERATOR_RECOVERY_REQUESTED,
            event_types::NOTICE_WRITTEN
        ]
    );
    let request = &events[before].envelope;
    assert_eq!(request.principal, oml_app::app::OPERATOR_PRINCIPAL);
    assert_eq!(request.api_key_id, None, "no key acts");
    assert_eq!(request.payload["person"], "carol");
    assert_eq!(request.payload["window"]["days"], 30);
    assert!(
        !request.payload.to_string().contains("carol@example.com"),
        "the journal carries the address's digest alone"
    );
    assert_eq!(events[before + 1].envelope.payload["kind"], "recovery");
    assert_eq!(
        events[before + 1].envelope.payload["recipients"],
        serde_json::json!(["bob"])
    );
    assert_eq!(requested.notified, vec![name("bob")]);
    assert_eq!(
        requested.mail.to,
        vec![email("bob")],
        "the dormant admin is emailed"
    );
    assert_eq!(refusal(r.recover().await), RecoverRefusal::Pending);
    let stats = r.app.fix_stats(r.tenant()).await.expect("stats");
    assert_eq!((stats.recoveries_pending, stats.recoveries_overdue), (1, 0));
    // Within the 72 hours the sweep does nothing; bob's sign-in cancels it.
    r.clock.advance(HOUR);
    let sweep = r.app.recovery_sweep(r.tenant()).await.expect("sweep");
    assert_eq!((sweep.completed.len(), sweep.cancelled), (0, 0));
    r.session("bob", Capacity::Web).await;
    let sweep = r.app.recovery_sweep(r.tenant()).await.expect("sweep");
    assert_eq!((sweep.completed.len(), sweep.cancelled), (0, 1));
    let last = r.journal().await.pop().expect("an event");
    assert_eq!(
        last.envelope.event_type,
        event_types::OPERATOR_RECOVERY_CANCELLED
    );
    assert_eq!(last.envelope.payload["cause"], "admin_sign_in");
    r.clock.advance(3 * DAY);
    let sweep = r.app.recovery_sweep(r.tenant()).await.expect("sweep");
    assert_eq!(
        (sweep.completed.len(), sweep.cancelled),
        (0, 0),
        "cancelled for good"
    );
    let key = r
        .store
        .key(r.tenant(), bobs.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(
        key.revoked_at.is_none(),
        "a cancelled recovery revokes nothing"
    );
}

/// The completion, 72 hours after the request with no admin sign-in, is
/// one commit: every admin key of the tenant revoked, `cause: recovery`,
/// the new admin person's entry, `operator.recovery` with the window and
/// the revoked keys, and a notice. The dormant admin keeps their flag, so
/// the tenant leaves `sole_admin` (ADR-0097 d19, d10; decided 2026-11-25).
#[tokio::test]
async fn recovery_keeps_the_dormant_admins_flags() {
    let (r, bobs) = rig().await;
    r.clock.advance(31 * DAY);
    let requested = r.recover().await.expect("requested");
    r.clock.advance(72 * HOUR);
    let before = r.journal().await.len();
    let sweep = r.app.recovery_sweep(r.tenant()).await.expect("sweep");
    assert_eq!(sweep.completed.len(), 1);
    let done = &sweep.completed[0];
    assert_eq!(done.recovery.recovery_id, requested.recovery.recovery_id);
    assert_eq!(done.recovery.state, RecoveryState::Completed);
    let events = r.journal().await;
    let types: Vec<&str> = events[before..]
        .iter()
        .map(|e| e.envelope.event_type.as_str())
        .filter(|t| *t != event_types::KEY_ISSUED)
        .collect();
    let revoked = done.revoked_keys.len();
    assert!(revoked >= 3, "root-admin, bob-cli and bob-admin: {revoked}");
    let tail = &types[types.len() - 3..];
    assert_eq!(
        tail,
        [
            event_types::WEB_EMAIL_ALLOWED,
            event_types::OPERATOR_RECOVERY,
            event_types::NOTICE_WRITTEN
        ]
    );
    let recovery = events
        .iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::OPERATOR_RECOVERY)
        .expect("operator.recovery");
    assert_eq!(
        recovery.envelope.principal,
        oml_app::app::OPERATOR_PRINCIPAL
    );
    assert_eq!(
        recovery.envelope.payload["revoked_keys"]
            .as_array()
            .map(Vec::len),
        Some(revoked)
    );
    assert!(
        recovery.envelope.payload["revoked_keys"]
            .to_string()
            .contains(&bobs.key_id.to_string())
    );
    for id in [bobs.key_id, r.boot.admin_key.key_id] {
        let key = r
            .store
            .key(r.tenant(), id)
            .await
            .expect("read")
            .expect("held");
        assert!(key.revoked_at.is_some(), "every admin key is revoked");
    }
    let bob = r
        .store
        .web_entry_by_email(&email("bob").digest())
        .await
        .expect("read")
        .expect("bob's entry");
    assert!(bob.admin, "the dormant admin keeps the flag");
    let carol = r
        .store
        .web_entry_by_email(&email("carol").digest())
        .await
        .expect("read")
        .expect("carol's entry");
    assert!(carol.admin && !carol.pending, "the new admin person");
    let standing = r.store.admin_standing(r.tenant()).await.expect("standing");
    assert!(!standing.sole_admin, "two admin persons: not sole_admin");
    assert_eq!(done.notified, vec![name("bob"), name("carol")]);
    let mut to: Vec<&str> = done.mail.to.iter().map(Email::as_str).collect();
    to.sort_unstable();
    assert_eq!(to, ["bob@example.com", "carol@example.com"]);
    let stats = r.app.fix_stats(r.tenant()).await.expect("stats");
    assert_eq!((stats.recoveries_pending, stats.recoveries_overdue), (0, 0));
}

/// The sweep completes a request once: a second pass, a second replica
/// over the same store and a restart journal nothing more, and a start
/// after the request completes it (answer 22).
#[tokio::test]
async fn a_recovery_completes_once_across_replicas_and_restarts() {
    let (r, _) = rig().await;
    r.clock.advance(31 * DAY);
    r.recover().await.expect("requested");
    r.clock.advance(72 * HOUR);
    // A start after the request: a fresh app over the same store.
    let restarted = app_over(&r.store, &r.clock);
    let sweep = restarted.recovery_sweep(r.tenant()).await.expect("sweep");
    assert_eq!(sweep.completed.len(), 1);
    let after = r.journal().await.len();
    let replica = app_over(&r.store, &r.clock);
    for app in [&r.app, &replica, &restarted] {
        let sweep = app.recovery_sweep(r.tenant()).await.expect("sweep");
        assert_eq!((sweep.completed.len(), sweep.cancelled), (0, 0));
    }
    assert_eq!(r.journal().await.len(), after, "one journalled completion");
    let completions = r
        .journal()
        .await
        .iter()
        .filter(|e| e.envelope.event_type == event_types::OPERATOR_RECOVERY)
        .count();
    assert_eq!(completions, 1);
}

/// A request past its 72 hours that the sweep cannot complete stays
/// requested, named by the refusal, and doctor reports it overdue
/// (`admin.recovery_overdue`, answer 22).
#[tokio::test]
async fn a_recovery_past_its_seventy_two_hours_and_not_completed_is_overdue() {
    let (r, _) = rig().await;
    r.clock.advance(31 * DAY);
    r.recover().await.expect("requested");
    // carol's name is taken while the request waits; the request is made
    // as the operator, so no admin key authenticates.
    r.store.insert_web_entry(oml_app::types::WebEntry {
        tenant_id: r.tenant(),
        email: Email::parse("carol.other@example.com").expect("an email"),
        user: name("carol"),
        admin: false,
        google_subject: None,
        github_subject: None,
        added_by: r.boot.admin_key.key_id,
        terms_version: None,
        disabled_at: None,
        pending: false,
    });
    r.clock.advance(72 * HOUR);
    let err = r.app.recovery_sweep(r.tenant()).await.unwrap_err();
    assert!(
        matches!(err, AppError::RecoverRefused(RecoverRefusal::PersonTaken)),
        "{err:?}"
    );
    let stats = r.app.fix_stats(r.tenant()).await.expect("stats");
    assert_eq!((stats.recoveries_pending, stats.recoveries_overdue), (1, 1));
}

/// Recovery's dormancy gate never opens while an act against an admin is
/// open (ADR-0097 Consequences, Q5(a) with Q14).
#[tokio::test]
async fn recovery_is_refused_while_an_act_against_an_admin_is_open() {
    let (r, _) = rig().await;
    let (alice, _) = r
        .app
        .allow_web_email(
            r.admin(),
            &WebEmailRequest {
                email: email("alice"),
                user: name("alice"),
                admin: true,
            },
        )
        .await
        .expect("alice's admin entry, under sole_admin at once");
    r.app.web_ensure_keys(&alice).await.expect("alice's keys");
    r.clock.advance(8 * DAY);
    let session = r.session("alice", Capacity::Admin).await;
    r.app
        .allow_web_email_with_reason(
            Bearer::Session(&session),
            &WebEmailRequest {
                email: email("bob"),
                user: name("bob"),
                admin: false,
            },
            true,
            Some("a dispute"),
        )
        .await
        .expect("alice un-flags bob");
    r.clock.advance(31 * DAY);
    assert_eq!(refusal(r.recover().await), RecoverRefusal::DisputeOpen);
}

/// While this process counts an authentication row it could not write
/// since it started, recover is refused `log_write_failed`, since the
/// missing row would read as dormancy; the count is the process's own, so
/// the refusal is per replica (ADR-0097, answer 26 reading 2).
#[tokio::test]
async fn recovery_is_refused_while_this_process_counts_an_authentication_row_it_could_not_write() {
    let (r, bobs) = rig().await;
    r.clock.advance(31 * DAY);
    r.store.fail_authentication_writes(true);
    r.app
        .whoami(bobs.value.as_str())
        .await
        .expect("bob's key still authenticates");
    r.store.fail_authentication_writes(false);
    assert_eq!(r.app.authentication_write_failures(), 1);
    assert_eq!(refusal(r.recover().await), RecoverRefusal::LogWriteFailed);
    let stats = r.app.fix_stats(r.tenant()).await.expect("stats");
    assert_eq!(stats.authentication_write_failures, 1);
    // Another replica, which counted no failure, admits the request.
    let replica = app_over(&r.store, &r.clock);
    replica
        .operator_recover(r.tenant(), name("carol"), email("carol"))
        .await
        .expect("a replica with no failure counted admits it");
}

/// A sign-in whose authentication row could not be written does not read
/// as dormancy at the completion: while the row stays unwritten the sweep
/// refuses the completion `log_write_failed` and the request stays, and
/// once the store takes writes again the sweep writes the row first and the
/// sign-in cancels the recovery (ADR-0097 d19, answer 26; review finding
/// store-5).
#[tokio::test]
async fn a_sign_in_whose_row_failed_still_cancels_a_recovery_and_never_reads_as_dormancy() {
    let (r, bobs) = rig().await;
    r.clock.advance(31 * DAY);
    r.recover().await.expect("requested");
    r.clock.advance(HOUR);
    r.store.fail_authentication_writes(true);
    r.app
        .whoami(bobs.value.as_str())
        .await
        .expect("bob signs in; his row is not written");
    r.clock.advance(72 * HOUR);
    let err = r.app.recovery_sweep(r.tenant()).await.unwrap_err();
    assert!(
        matches!(
            err,
            AppError::RecoverRefused(RecoverRefusal::LogWriteFailed)
        ),
        "{err:?}"
    );
    let key = r
        .store
        .key(r.tenant(), bobs.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(key.revoked_at.is_none(), "nothing is revoked");
    r.store.fail_authentication_writes(false);
    let sweep = r.app.recovery_sweep(r.tenant()).await.expect("sweep");
    assert_eq!(
        (sweep.completed.len(), sweep.cancelled),
        (0, 1),
        "the written row cancels the recovery"
    );
}

/// A completed recovery revokes every live admin key of the tenant,
/// whatever its link: a key whose link to its person is still pending
/// included (ADR-0097 d19; review finding store-4).
#[tokio::test]
async fn a_completed_recovery_leaves_no_live_admin_key_in_the_tenant() {
    let (r, _) = rig().await;
    let pending = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                profile: None,
                role: Role::Admin,
                name: name("bob-laptop"),
                principal: "bob".to_owned(),
                used_by: Some(UsedBy::Person),
                default_workspace: None,
                crew: None,
                grants: Vec::new(),
                user: None,
                connect_code: false,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                person: Some(name("bob")),
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("an admin key whose link waits on bob");
    r.clock.advance(31 * DAY);
    r.recover().await.expect("requested");
    r.clock.advance(72 * HOUR);
    let sweep = r.app.recovery_sweep(r.tenant()).await.expect("sweep");
    assert_eq!(sweep.completed.len(), 1);
    let held = r
        .store
        .key(r.tenant(), pending.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(held.revoked_at.is_some(), "the pending-link key is revoked");
    let live = r.store.live_admin_keys(r.tenant()).await.expect("read");
    assert!(
        live.iter().all(|k| k
            .name
            .as_ref()
            .is_some_and(|n| n.as_str().starts_with("carol-"))),
        "only the new admin person's keys are live: {:?}",
        live.iter().map(|k| k.name.clone()).collect::<Vec<_>>()
    );
}

/// The double's authentication table counts as installed at the clock's
/// start unless a test sets an older install, as a fresh Postgres's 0076
/// does, so a recovery right after the install meets `log_too_short`
/// (ADR-0097, answer 24; review finding store-10).
#[tokio::test]
async fn the_doubles_authentication_table_is_installed_when_the_store_is_made() {
    let store = MemStore::new();
    let coverage = store.authentication_coverage().await.expect("coverage");
    assert_eq!(
        coverage.installed_at,
        EventTime::parse(oml_app::doubles::FIXED_CLOCK_START).expect("a time")
    );
}
