//! The `maintenance` key role and the conversion of an admin key into one
//! (ADR-0097 d18): a service's key that reads counts only and takes the
//! rescan, in place of an admin key.

use oml_app::App;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::{Role, UsedBy, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::types::{AppError, BootstrapRequest, BootstrapResult, IssueKeyRequest, IssuedKey};

const TEST_SEED: [u8; 32] = [
    0x09, 0x87, 0x45, 0x94, 0xaf, 0xc9, 0x3c, 0x44, 0xaf, 0x9c, 0x48, 0x6d, 0x6d, 0xd4, 0x8b, 0x68,
    0x51, 0x4b, 0x57, 0x36, 0xa7, 0xd6, 0x4c, 0xac, 0x72, 0xd9, 0x07, 0x2f, 0x98, 0xa6, 0x37, 0x1c,
];

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

struct Rig {
    app: TestApp,
    store: MemStore,
    boot: BootstrapResult,
}

fn name(value: &str) -> Name {
    Name::parse(value).expect("a fixture name")
}

async fn rig() -> Rig {
    let store = MemStore::new();
    let app = App::new(
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
    Rig { app, store, boot }
}

fn maintenance_req(key: &str) -> IssueKeyRequest {
    IssueKeyRequest {
        profile: None,
        role: Role::Maintenance,
        name: name(key),
        principal: key.to_owned(),
        used_by: Some(UsedBy::Service),
        default_workspace: None,
        crew: None,
        grants: Vec::new(),
        connect_code: false,
        user: None,
        guardrail_scope: None,
        expiry: None,
        requests_per_minute: None,
        person: None,
        create_workspaces: Vec::new(),
    }
}

async fn maintenance_key(r: &Rig) -> IssuedKey {
    r.app
        .issue_key(
            r.boot.admin_key.value.as_str(),
            &maintenance_req("deployer"),
        )
        .await
        .expect("an admin issues a maintenance key")
}

fn refused_outside_counts(err: &AppError) -> bool {
    matches!(
        err,
        AppError::VerbCeiling {
            role: Role::Maintenance,
            ..
        }
    )
}

/// An admin issues a maintenance key, a widening act like every issuance,
/// with no grant and the role's request rate, 60 a minute (a placeholder).
#[tokio::test]
async fn an_admin_issues_a_maintenance_key_with_no_grant_and_its_request_rate() {
    let r = rig().await;
    let before = r
        .store
        .events_range(r.boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .len();
    let issued = maintenance_key(&r).await;
    assert!(issued.access.grants.is_empty());
    let held = r
        .store
        .key(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(held.role, Role::Maintenance);
    assert_eq!(held.requests_per_minute, Some(60));
    let events = r
        .store
        .events_range(r.boot.tenant_id, 1, 10_000)
        .await
        .expect("journal");
    assert_eq!(events[before].envelope.event_type, event_types::ADMIN_ACT);
    assert_eq!(events[before].envelope.payload["act"], "issue_key");
    assert_eq!(events[before].envelope.payload["class"], "widening");
    let mut connect = maintenance_req("with-code");
    connect.connect_code = true;
    let err = r
        .app
        .issue_key(r.boot.admin_key.value.as_str(), &connect)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::OAuth(_)),
        "a maintenance key holds no connection: {err:?}"
    );
}

/// A maintenance key reads what `oml doctor` reads under an admin key, in
/// counts-only form, and takes the rescan: whoami, the rescan window and
/// its record, the usage drift, the search and federation settings with no
/// key listed, the tree, the events head with no event, and the sign-up
/// overview with no key listed and no deletion named.
#[tokio::test]
async fn a_maintenance_key_reads_the_doctors_counts_and_takes_the_rescan() {
    let r = rig().await;
    let key = maintenance_key(&r).await;
    let value = key.value.as_str();
    let me = r.app.whoami(value).await.expect("whoami");
    assert_eq!(me.key.role, Role::Maintenance);
    r.app.rescan(value, 0, 10).await.expect("the rescan window");
    r.app
        .rescan_record_held(value, 0, 10)
        .await
        .expect("the rescan's record");
    r.app.admin_usage_drift(value).await.expect("the drift");
    r.app.search_settings(value).await.expect("search settings");
    let (_, wide) = r
        .app
        .federation_settings(value)
        .await
        .expect("federation settings");
    assert!(wide.is_empty(), "no key list");
    r.app
        .workspace_tree(
            value,
            Reference::parse("universe").expect("a ref"),
            None,
            50,
        )
        .await
        .expect("the tree");
    let tail = r
        .app
        .tail_events(value, None, 0, 10)
        .await
        .expect("the head");
    assert!(tail.events.is_empty(), "no event is served");
    assert!(tail.head_seq > 0);
    let (users, deletions) = r.app.signup_overview(value).await.expect("the overview");
    assert!(users.iter().all(|u| u.admin_keys.is_empty()), "no key list");
    assert!(deletions.is_empty(), "no person named");
}

/// A maintenance key reaches nothing else: no claim, no key, no admin act,
/// no fix and no investigation, each refused `access.verb_ceiling`.
#[tokio::test]
async fn a_maintenance_key_is_refused_every_other_verb() {
    let r = rig().await;
    let key = maintenance_key(&r).await;
    let value = key.value.as_str();
    let errs = [
        r.app
            .issue_key(value, &maintenance_req("another"))
            .await
            .map(|_| ())
            .unwrap_err(),
        r.app.admin_acts(value, None).await.map(|_| ()).unwrap_err(),
        r.app
            .admin_fixes(value, false)
            .await
            .map(|_| ())
            .unwrap_err(),
        r.app.crews(value, None, 10).await.map(|_| ()).unwrap_err(),
    ];
    for err in errs {
        assert!(refused_outside_counts(&err), "{err:?}");
    }
}

/// An admin key converts itself into a maintenance key in place: one sealed
/// `key.converted {from: admin, to: maintenance}`, the same id and hash, and
/// from then on it reads counts only. Only `admin → maintenance` under the
/// key itself is admitted.
#[tokio::test]
async fn an_admin_key_converts_itself_into_a_maintenance_key_in_place() {
    let r = rig().await;
    let admin = r.boot.admin_key.value.as_str();
    let before = r
        .store
        .key(r.boot.tenant_id, r.boot.admin_key.key_id)
        .await
        .expect("read")
        .expect("held");
    let maint = maintenance_key(&r).await;
    let err = r
        .app
        .convert_self(maint.value.as_str(), Role::Maintenance)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::KeyConversion(_)), "{err:?}");
    let err = r.app.convert_self(admin, Role::Reader).await.unwrap_err();
    assert!(matches!(err, AppError::KeyConversion(_)), "{err:?}");
    let converted = r
        .app
        .convert_self(admin, Role::Maintenance)
        .await
        .expect("the admin key converts itself");
    assert_eq!(converted.role, Role::Maintenance);
    let after = r
        .store
        .key(r.boot.tenant_id, r.boot.admin_key.key_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(after.role, Role::Maintenance);
    assert_eq!(after.key_hash, before.key_hash, "the hash is kept");
    let last = r
        .store
        .events_range(r.boot.tenant_id, 1, 10_000)
        .await
        .expect("journal")
        .pop()
        .expect("an event")
        .envelope;
    assert_eq!(last.event_type, event_types::KEY_CONVERTED);
    assert_eq!(last.payload["from"], "admin");
    assert_eq!(last.payload["to"], "maintenance");
    assert_eq!(last.payload["key_id"], r.boot.admin_key.key_id.to_string());
    r.app.rescan(admin, 0, 10).await.expect("counts still read");
    let err = r.app.admin_acts(admin, None).await.unwrap_err();
    assert!(refused_outside_counts(&err), "{err:?}");
}

/// A person's session keys are issued by the tenant's reserved server
/// issuer, never by the admin key that added the entry: the issuer is
/// issued and revoked at once, authenticates nothing, and no revoke verb
/// reaches it (ADR-0097 d18).
#[tokio::test]
async fn the_server_issuer_issues_the_session_keys_in_place_of_the_admin_who_added_the_entry() {
    use oml_app::domain::web::Email;
    use oml_app::ports::WebStore as _;
    use oml_app::types::{Bearer, WebEmailRequest};
    let r = rig().await;
    let admin = r.boot.admin_key.value.as_str();
    let (entry, _) = r
        .app
        .allow_web_email(
            admin,
            &WebEmailRequest {
                email: Email::parse("rhea@example.com").expect("an email"),
                user: name("rhea"),
                admin: false,
            },
        )
        .await
        .expect("rhea's entry");
    r.app
        .web_ensure_keys(&entry)
        .await
        .expect("the session keys");
    let keys = r
        .store
        .web_session_keys(r.boot.tenant_id, &name("rhea"))
        .await
        .expect("read");
    assert_eq!(keys.len(), 2, "{keys:?}");
    let issuer = keys[0].issued_by.expect("an issuer");
    assert!(keys.iter().all(|k| k.issued_by == Some(issuer)));
    assert_ne!(issuer, r.boot.admin_key.key_id, "never the admin key");
    let held = r
        .store
        .key(r.boot.tenant_id, issuer)
        .await
        .expect("read")
        .expect("the server issuer");
    assert_eq!(held.principal, "oml:server-issuer");
    assert!(held.revoked_at.is_some(), "it authenticates nothing");
    let err = r
        .app
        .revoke_key(Bearer::Key(admin), issuer.into(), Some("try"))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::KeyRevocation(_)), "{err:?}");
}
