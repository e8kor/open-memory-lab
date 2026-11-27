//! The admin act (ADR-0097 d1, d2, d5, d10, d11, d23): one journalled
//! envelope per act, whose class the act decides; widening at once only
//! under `sole_admin`, and otherwise pending until a second admin person
//! whose flag is seven days older than the act confirms it.

use oml_app::App;
use oml_app::app::Caller;
use oml_app::app::Capacity;
use oml_app::domain::access::Covers;
use oml_app::domain::admin_act::AdminActState;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::web::Email;
use oml_app::domain::{Role, SignedEnvelope, UsedBy, event_types};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::JournalStore as _;
use oml_app::ports::WebStore as _;
use oml_app::types::{
    AdminActRefusal, AppError, Bearer, BootstrapRequest, BootstrapResult, GrantRequest,
    IssueKeyRequest, IssuedKey, NewWorkspace, WebEmailRequest,
};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

const TEST_SEED: [u8; 32] = [
    0x09, 0x87, 0x45, 0x94, 0xaf, 0xc9, 0x3c, 0x44, 0xaf, 0x9c, 0x48, 0x6d, 0x6d, 0xd4, 0x8b, 0x68,
    0x51, 0x4b, 0x57, 0x36, 0xa7, 0xd6, 0x4c, 0xac, 0x72, 0xd9, 0x07, 0x2f, 0x98, 0xa6, 0x37, 0x1c,
];

const DAY: u64 = 86_400;

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

fn by_name(value: &str) -> Reference {
    Reference::parse(value).expect("a reference")
}

fn fresh_key_name() -> Name {
    static ISSUED: AtomicU64 = AtomicU64::new(0);
    name(&format!(
        "act-key-{}",
        ISSUED.fetch_add(1, Ordering::Relaxed)
    ))
}

async fn rig() -> Rig {
    let store = MemStore::new();
    let clock = FixedClock::new();
    let app = App::new(
        store.clone(),
        MemSigner::new(TEST_SEED),
        clock.clone(),
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
    app.create_workspace(
        boot.admin_key.value.as_str(),
        &name("crew"),
        Some(by_name("universe")),
        None,
    )
    .await
    .expect("crew");
    Rig {
        app,
        store,
        clock,
        boot,
    }
}

impl Rig {
    fn admin(&self) -> &str {
        self.boot.admin_key.value.as_str()
    }

    async fn journal(&self) -> Vec<SignedEnvelope> {
        self.store
            .events_range(self.boot.tenant_id, 1, 10_000)
            .await
            .expect("journal")
    }

    /// An admin entry for `user`, under the bootstrap key.
    async fn admin_person(&self, user: &str) {
        let (entry, _) = self
            .app
            .allow_web_email(
                self.admin(),
                &WebEmailRequest {
                    email: Email::parse(&format!("{user}@example.com")).expect("an email"),
                    user: name(user),
                    admin: true,
                },
            )
            .await
            .expect("the admin entry");
        self.app
            .web_ensure_keys(&entry)
            .await
            .expect("session keys");
    }

    /// `<user>-admin`, from a session opened now.
    async fn admin_session(&self, user: &str) -> Caller {
        let entry = self
            .app
            .list_web_emails(self.admin(), None, 100)
            .await
            .expect("entries")
            .into_iter()
            .find(|e| e.user.as_str() == user)
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
            .web_caller(&session, Capacity::Admin)
            .await
            .expect("the admin session key")
    }
}

fn agent_req() -> IssueKeyRequest {
    IssueKeyRequest {
        profile: None,
        role: Role::Contributor,
        name: fresh_key_name(),
        principal: "ana".to_owned(),
        used_by: Some(UsedBy::Agent),
        default_workspace: None,
        crew: None,
        grants: vec![
            GrantRequest {
                node: by_name("crew"),
                role: Role::Reader,
                deny: Vec::new(),
                covers: Covers::Subtree,
                create: false,
            },
            GrantRequest {
                node: by_name("crew"),
                role: Role::Submitter,
                deny: Vec::new(),
                covers: Covers::Subtree,
                create: false,
            },
        ],
        user: None,
        connect_code: false,
        guardrail_scope: None,
        expiry: None,
        requests_per_minute: None,
        create_workspaces: Vec::new(),
    }
}

/// A tenant with two admin persons, alice and bob, both accepted eight
/// days ago: no longer `sole_admin`, and each counts as a second admin
/// person for an act taken now.
async fn two_admins() -> Rig {
    let r = rig().await;
    r.admin_person("alice").await;
    r.admin_person("bob").await;
    r.clock.advance(8 * DAY);
    r
}

async fn pending_key(r: &Rig) -> IssuedKey {
    r.app
        .issue_key_with_reason(r.admin(), &agent_req(), Some("an agent for the crew"))
        .await
        .expect("a pending issuance")
}

/// In a tenant that never had two admin persons an admin's issuance takes
/// effect at once, marked `sole_admin`: `admin.act` is journalled first in
/// the issuance's commit, its class `widening` fixed by the act, and
/// `key.issued` names its `act_id`.
#[tokio::test]
async fn every_admin_act_journals_one_envelope_whose_class_the_act_decides() {
    let r = rig().await;
    let before = r.journal().await.len();
    let issued = r
        .app
        .issue_key(r.admin(), &agent_req())
        .await
        .expect("issued");
    assert_eq!(issued.pending_act, None);
    let events = r.journal().await;
    let added: Vec<&str> = events[before..]
        .iter()
        .map(|e| e.envelope.event_type.as_str())
        .collect();
    assert_eq!(
        added,
        [
            event_types::ADMIN_ACT,
            event_types::KEY_ISSUED,
            event_types::GRANT_ISSUED,
            event_types::GRANT_ISSUED
        ]
    );
    let act = &events[before].envelope.payload;
    assert_eq!(act["act"], "issue_key");
    assert_eq!(act["class"], "widening");
    assert_eq!(act["state"], "effective");
    assert_eq!(act["sole_admin"], true);
    let act_id = act["act_id"].as_str().expect("an act id");
    for effect in &events[before + 1..] {
        assert_eq!(effect.envelope.payload["act_id"], act_id);
    }
    r.app
        .whoami(&issued.value)
        .await
        .expect("a sole_admin issuance authenticates at once");
    let held = r
        .store
        .admin_act(r.boot.tenant_id, Uuid::parse_str(act_id).expect("a uuid"))
        .await
        .expect("read")
        .expect("held");
    assert!(held.sole_admin);
    assert_eq!(held.state, AdminActState::Effective);
}

/// With two admin persons, an admin's issuance waits: the value is shown
/// once and authenticates nothing, no grant is in force, and a second
/// admin person's confirmation applies the grants and lets the key in.
#[tokio::test]
async fn a_widening_act_takes_effect_only_after_a_second_admin_person_confirms_it() {
    let r = two_admins().await;
    let issued = pending_key(&r).await;
    let act_id = issued.pending_act.expect("pending");
    assert!(
        !issued.value.is_empty(),
        "the value is shown once at issuance"
    );
    assert!(issued.access.grants.is_empty());
    let err = r.app.whoami(&issued.value).await.unwrap_err();
    assert!(matches!(err, AppError::KeyPendingConfirmation), "{err:?}");
    let access = r
        .store
        .key_access(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(access.grants.is_empty(), "no pending grant is in force");

    let alice = r.admin_session("alice").await;
    let confirmed = r
        .app
        .confirm_admin_act(Bearer::Session(&alice), act_id)
        .await
        .expect("alice confirms");
    assert_eq!(confirmed.state, AdminActState::Confirmed);
    assert_eq!(confirmed.confirmed_by_person, Some(name("alice")));
    r.app
        .whoami(&issued.value)
        .await
        .expect("the confirmed key authenticates");
    let access = r
        .store
        .key_access(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(access.grants.len(), 2);
}

/// A pending key authenticates on no path: not by its value, and a
/// pending widening requires a reason.
#[tokio::test]
async fn a_pending_issuance_requires_a_reason() {
    let r = two_admins().await;
    let err = r.app.issue_key(r.admin(), &agent_req()).await.unwrap_err();
    assert!(matches!(err, AppError::AdminActReasonRequired), "{err:?}");
}

/// An admin person whose flag is younger than seven days at the act
/// confirms nothing, and nor does the person who took the act.
#[tokio::test]
async fn an_admin_person_accepted_less_than_seven_days_ago_confirms_nothing() {
    let r = rig().await;
    r.admin_person("alice").await;
    r.admin_person("bob").await;
    r.clock.advance(6 * DAY);
    let issued = pending_key(&r).await;
    let alice = r.admin_session("alice").await;
    let err = r
        .app
        .confirm_admin_act(
            Bearer::Session(&alice),
            issued.pending_act.expect("pending"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::TooNew)),
        "{err:?}"
    );

    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    let issued = r
        .app
        .issue_key_with_reason(Bearer::Session(&alice), &agent_req(), Some("alice's agent"))
        .await
        .expect("pending");
    let alice = r.admin_session("alice").await;
    let err = r
        .app
        .confirm_admin_act(
            Bearer::Session(&alice),
            issued.pending_act.expect("pending"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::OwnAct)),
        "{err:?}"
    );
    let bob = r.admin_session("bob").await;
    r.app
        .confirm_admin_act(Bearer::Session(&bob), issued.pending_act.expect("pending"))
        .await
        .expect("bob confirms alice's act");
}

/// A declined pending act never takes effect: its key is revoked in the
/// decline's commit, and a later confirm is refused; one past its seven
/// days reads `expired` and is never confirmed either.
#[tokio::test]
async fn a_declined_or_expired_widening_never_takes_effect() {
    let r = two_admins().await;
    let issued = pending_key(&r).await;
    let act_id = issued.pending_act.expect("pending");
    let bob = r.admin_session("bob").await;
    let declined = r
        .app
        .decline_admin_act(Bearer::Session(&bob), act_id, Some("not this agent"))
        .await
        .expect("declined");
    assert_eq!(declined.state, AdminActState::Declined);
    let key = r
        .store
        .key(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(key.revoked_at.is_some(), "the declined key is revoked");
    let alice = r.admin_session("alice").await;
    let err = r
        .app
        .confirm_admin_act(Bearer::Session(&alice), act_id)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::NotPending)),
        "{err:?}"
    );
    let decline = r
        .journal()
        .await
        .into_iter()
        .rev()
        .find(|e| {
            e.envelope.event_type == event_types::ADMIN_ACT
                && e.envelope.payload["act"] == "decline_act"
        })
        .expect("the decline is an admin act of its own");
    assert_eq!(decline.envelope.payload["class"], "narrowing");

    let issued = pending_key(&r).await;
    let act_id = issued.pending_act.expect("pending");
    r.clock.advance(7 * DAY);
    let alice = r.admin_session("alice").await;
    let err = r
        .app
        .confirm_admin_act(Bearer::Session(&alice), act_id)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::Expired)),
        "{err:?}"
    );
    let listed = r
        .app
        .admin_acts(Bearer::Session(&alice), Some(AdminActState::Expired))
        .await
        .expect("the listing");
    assert!(listed.iter().any(|a| a.act_id == act_id));
    let err = r.app.whoami(&issued.value).await.unwrap_err();
    assert!(matches!(err, AppError::KeyPendingConfirmation), "{err:?}");
}

/// A tenant that once had two live admin persons stays out of
/// `sole_admin` when one entry is removed.
#[tokio::test]
async fn a_tenant_that_had_two_admin_persons_never_returns_to_sole_admin_on_a_removal() {
    let r = two_admins().await;
    r.app
        .remove_web_email(
            r.admin(),
            &Email::parse("bob@example.com").expect("an email"),
        )
        .await
        .expect("bob's entry is removed");
    let standing = r
        .store
        .admin_standing(r.boot.tenant_id)
        .await
        .expect("read");
    assert!(!standing.sole_admin);
    let issued = pending_key(&r).await;
    assert!(issued.pending_act.is_some(), "widening still waits");
}

// -- the plain revoke verb (ADR-0097 d14) -----------------------------------

/// One revoke ends one key on every path and nothing it issued: the key
/// authenticates nothing afterwards, a key it issued keeps working, and an
/// admin's revocation rides a narrowing `revoke_key` act.
#[tokio::test]
async fn one_revoke_verb_ends_any_key_and_cascades_to_none() {
    let r = rig().await;
    let second_admin = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                role: Role::Admin,
                grants: Vec::new(),
                used_by: Some(UsedBy::Person),
                ..agent_req()
            },
        )
        .await
        .expect("an admin key");
    let issued = r
        .app
        .issue_key(second_admin.value.as_str(), &agent_req())
        .await
        .expect("an agent key the second admin issued");
    let revoked = r
        .app
        .revoke_key(r.admin(), second_admin.key_id.into(), Some("leaked"))
        .await
        .expect("revoked");
    assert!(revoked.act_id.is_some());
    let err = r.app.whoami(&second_admin.value).await.unwrap_err();
    assert!(matches!(err, AppError::Unauthorized), "{err:?}");
    r.app
        .whoami(&issued.value)
        .await
        .expect("no cascade: a key it issued keeps working");
    let events = r.journal().await;
    let last = &events[events.len() - 1].envelope;
    assert_eq!(last.event_type, event_types::KEY_REVOKED);
    assert_eq!(last.payload["reason"], "leaked");
    assert_eq!(
        last.payload["act_id"],
        revoked.act_id.expect("an act").to_string()
    );
    let act = &events[events.len() - 2].envelope;
    assert_eq!(act.event_type, event_types::ADMIN_ACT);
    assert_eq!(act.payload["act"], "revoke_key");
    assert_eq!(act.payload["class"], "narrowing");

    let err = r
        .app
        .revoke_key(r.admin(), second_admin.key_id.into(), None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::KeyRevocation(oml_app::types::KeyRevocationRefusal::Revoked)
        ),
        "{err:?}"
    );
    let err = r
        .app
        .revoke_key(issued.value.as_str(), second_admin.key_id.into(), None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::VerbCeiling { .. }), "{err:?}");
}

/// A session key ends with its entry's session, never by revocation.
#[tokio::test]
async fn a_session_key_is_refused_revocation_by_name() {
    let r = rig().await;
    r.admin_person("alice").await;
    let alice = r.admin_session("alice").await;
    let err = r
        .app
        .revoke_key(r.admin(), alice.key().key_id.into(), None)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::KeyRevocation(oml_app::types::KeyRevocationRefusal::SessionKey)
        ),
        "{err:?}"
    );
}

/// A person's `<user>-web` session revokes the person's own keys as their
/// own act, no admin act, and finds no one else's.
#[tokio::test]
async fn a_persons_web_session_revokes_the_persons_own_keys() {
    let r = rig().await;
    r.admin_person("alice").await;
    let entry = r
        .app
        .list_web_emails(r.admin(), None, 100)
        .await
        .expect("entries")
        .into_iter()
        .find(|e| e.user.as_str() == "alice")
        .expect("alice");
    let token = r
        .app
        .web_session_open(&entry, 3600, None)
        .await
        .expect("a session")
        .token;
    let session = r
        .app
        .web_session(&token)
        .await
        .expect("read")
        .expect("live");
    let web = r
        .app
        .web_caller(&session, Capacity::Web)
        .await
        .expect("alice-web");
    let own = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                user: Some(name("alice")),
                grants: Vec::new(),
                role: Role::Reader,
                ..agent_req()
            },
        )
        .await
        .expect("a key in alice's root, issued by the admin");
    // An admin-issued key in alice's root counts as the admin's person,
    // no one, so alice's session finds it not hers.
    let err = r
        .app
        .revoke_key(Bearer::Session(&web), own.key_id.into(), None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    let hers = r
        .app
        .issue_own_key(
            &web,
            "laptop",
            Role::Reader,
            false,
            &oml_app::app::OwnKeyShape {
                grants: Vec::new(),
                default_workspace: None,
                guardrail_scope: None,
                expiry: None,
                requests_per_minute: None,
                create_workspaces: Vec::new(),
            },
        )
        .await
        .expect("alice's own key");
    let before = r.journal().await.len();
    let revoked = r
        .app
        .revoke_key(Bearer::Session(&web), hers.key_id.into(), None)
        .await
        .expect("alice revokes her own key");
    assert_eq!(revoked.act_id, None, "a person's own act is no admin act");
    let added = r.journal().await;
    assert_eq!(added.len(), before + 1);
    assert_eq!(added[before].envelope.payload["actor_person"], "alice");
}

// -- the last live admin person (ADR-0097 d25) --------------------------------

/// The guard counts live admin persons and leaves a disabled entry out:
/// with bob's admin entry disabled, alice is the last, and neither removing
/// her entry nor un-flagging it commits.
#[tokio::test]
async fn removing_the_last_live_admin_person_is_refused_on_every_surface() {
    let r = rig().await;
    r.admin_person("alice").await;
    let bob = WebEmailRequest {
        email: Email::parse("bob@example.com").expect("an email"),
        user: name("bob"),
        admin: false,
    };
    r.app
        .allow_web_email(r.admin(), &bob)
        .await
        .expect("bob's entry");
    let alice = r.admin_session("alice").await;
    r.app
        .disable_web_user(&alice, &name("bob"))
        .await
        .expect("bob is disabled");
    r.app
        .allow_web_email(r.admin(), &WebEmailRequest { admin: true, ..bob })
        .await
        .expect("bob's disabled entry takes the flag");
    let alices = Email::parse("alice@example.com").expect("an email");
    let err = r
        .app
        .remove_web_email(r.admin(), &alices)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::WebField { field: "admin", .. }),
        "{err:?}"
    );
    let err = r
        .app
        .allow_web_email(
            r.admin(),
            &WebEmailRequest {
                email: alices,
                user: name("alice"),
                admin: false,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::WebField { field: "admin", .. }),
        "{err:?}"
    );
}

// -- self-link by possession (ADR-0097 d22) -------------------------------------

/// An admin key links itself to its holder's person by possession, and the
/// link waits on that person: the key acts for no one until alice accepts
/// it from her own console session.
#[tokio::test]
async fn an_admin_key_links_to_its_holders_person_only_once_that_person_accepts() {
    let r = rig().await;
    r.admin_person("alice").await;
    let cli = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                role: Role::Admin,
                grants: Vec::new(),
                used_by: Some(UsedBy::Person),
                ..agent_req()
            },
        )
        .await
        .expect("the CLI's admin key");
    let linked = r
        .app
        .link_self(cli.value.as_str(), &name("alice"))
        .await
        .expect("a pending self-link");
    assert!(linked.link_pending.is_some());
    assert_eq!(linked.linked_user, None, "pending: it acts for no one yet");
    let last = r.journal().await.pop().expect("an event").envelope;
    assert_eq!(last.event_type, event_types::KEY_LINKED_PERSON);
    assert_eq!(last.payload["actor"], "self");
    let entry = r
        .app
        .list_web_emails(r.admin(), None, 100)
        .await
        .expect("entries")
        .into_iter()
        .find(|e| e.user.as_str() == "alice")
        .expect("alice");
    let token = r
        .app
        .web_session_open(&entry, 3600, None)
        .await
        .expect("a session")
        .token;
    let session = r
        .app
        .web_session(&token)
        .await
        .expect("read")
        .expect("live");
    let web = r
        .app
        .web_caller(&session, Capacity::Web)
        .await
        .expect("alice-web");
    r.app
        .accept_key_link(Bearer::Session(&web), cli.key_id.into())
        .await
        .expect("alice accepts");
    let key = r
        .store
        .key(r.boot.tenant_id, cli.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(key.linked_user.is_some());
}

// -- limits (ADR-0097 d15) and the admin flag (d5) ----------------------------

/// A raise waits on a second admin person and changes nothing until then;
/// a lower rate takes effect at once as narrowing.
#[tokio::test]
async fn raising_a_limit_waits_on_a_second_admin_and_lowering_it_takes_effect_at_once() {
    let r = rig().await;
    let issued = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                requests_per_minute: Some(60),
                ..agent_req()
            },
        )
        .await
        .expect("a key at 60 a minute");
    r.admin_person("alice").await;
    r.admin_person("bob").await;
    r.clock.advance(8 * DAY);
    let (key, act) = r
        .app
        .set_key_limits_with_reason(
            r.admin(),
            issued.key_id.into(),
            oml_app::types::KeyLimitsChange {
                expiry: None,
                requests_per_minute: Some(Some(600)),
            },
            Some("a busier agent"),
        )
        .await
        .expect("a pending raise");
    assert_eq!(act.state, AdminActState::PendingConfirmation);
    assert_eq!(key.requests_per_minute, Some(60), "nothing changes yet");
    let alice = r.admin_session("alice").await;
    r.app
        .confirm_admin_act(Bearer::Session(&alice), act.act_id)
        .await
        .expect("alice confirms");
    let held = r
        .store
        .key(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(held.requests_per_minute, Some(600));
    let (key, act) = r
        .app
        .set_key_limits_with_reason(
            r.admin(),
            issued.key_id.into(),
            oml_app::types::KeyLimitsChange {
                expiry: None,
                requests_per_minute: Some(Some(10)),
            },
            None,
        )
        .await
        .expect("a lower rate");
    assert_eq!(act.state, AdminActState::Effective);
    assert_eq!(act.class().as_str(), "narrowing");
    assert_eq!(key.requests_per_minute, Some(10));
}

/// Once two admin persons exist, a third admin flag waits on a second admin
/// person, and its confirmation adds the entry.
#[tokio::test]
async fn a_new_admin_person_waits_on_a_second_admin_person() {
    let r = two_admins().await;
    let carol = WebEmailRequest {
        email: Email::parse("carol@example.com").expect("an email"),
        user: name("carol"),
        admin: true,
    };
    let alice = r.admin_session("alice").await;
    let err = r
        .app
        .allow_web_email(Bearer::Session(&alice), &carol)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::AdminActReasonRequired), "{err:?}");
    let (_, outcome) = r
        .app
        .allow_web_email_with_reason(Bearer::Session(&alice), &carol, true, Some("a third admin"))
        .await
        .expect("a pending flag");
    assert_eq!(
        outcome,
        oml_app::types::WebEmailOutcome::PendingConfirmation
    );
    let held = r
        .store
        .web_entry_by_email(&carol.email.digest())
        .await
        .expect("read");
    assert!(held.is_none(), "nothing changes until confirmed");
    let act = r
        .app
        .admin_acts(Bearer::Session(&alice), None)
        .await
        .expect("acts")
        .pop()
        .expect("the act");
    let bob = r.admin_session("bob").await;
    r.app
        .confirm_admin_act(Bearer::Session(&bob), act.act_id)
        .await
        .expect("bob confirms");
    let held = r
        .store
        .web_entry_by_email(&carol.email.digest())
        .await
        .expect("read")
        .expect("carol's entry");
    assert!(held.admin);
}

/// A workspace an issuance creates, `name` under `parent` (ADR-0097 d30).
fn new_workspace(parent: &str, child: &str) -> NewWorkspace {
    NewWorkspace {
        parent: by_name(parent),
        name: name(child),
    }
}

/// [`agent_req`] with no grant of its own, creating `child` under `crew`.
fn creating_req(child: &str) -> IssueKeyRequest {
    IssueKeyRequest {
        grants: Vec::new(),
        create_workspaces: vec![new_workspace("crew", child)],
        ..agent_req()
    }
}

/// [`agent_req`] whose submitter grant on `crew` carries `create` and
/// covers `covers` (ADR-0097 d31).
fn flagged_req(covers: Covers) -> IssueKeyRequest {
    let mut req = agent_req();
    for grant in &mut req.grants {
        grant.covers = covers;
    }
    req.grants[1].create = true;
    req
}

/// The event types the journal gained since `before`.
async fn added_since(r: &Rig, before: usize) -> Vec<String> {
    r.journal().await[before..]
        .iter()
        .map(|e| e.envelope.event_type.clone())
        .collect()
}

/// An issuance that creates a workspace commits the creation, the key and
/// the key's role's default grants on it in one chain, the creation first,
/// each naming the act; the key's default is the new workspace. A second
/// issuance naming the same workspace is `name.taken` and journals nothing,
/// its key included (ADR-0097 d30).
#[tokio::test]
async fn an_issuance_with_new_workspaces_creates_them_and_grants_the_key_in_one_commit_or_not_at_all()
 {
    let r = rig().await;
    let before = r.journal().await.len();
    let issued = r
        .app
        .issue_key(r.admin(), &creating_req("alpha"))
        .await
        .expect("issued with its workspace");
    assert_eq!(
        added_since(&r, before).await,
        [
            event_types::ADMIN_ACT,
            event_types::WORKSPACE_CREATE,
            event_types::KEY_ISSUED,
            event_types::GRANT_ISSUED,
            event_types::GRANT_ISSUED,
        ]
    );
    let events = r.journal().await;
    let act_id = events[before].envelope.payload["act_id"].clone();
    for effect in &events[before + 1..] {
        assert_eq!(effect.envelope.payload["act_id"], act_id);
    }
    let alpha = Uuid::parse_str(
        events[before + 1].envelope.payload["workspace_id"]
            .as_str()
            .expect("an id"),
    )
    .expect("a uuid");
    assert_eq!(issued.default_workspace, Some(alpha));
    let roles: Vec<Role> = issued
        .access
        .grants
        .iter()
        .inspect(|g| assert_eq!(g.grant.node, alpha))
        .map(|g| g.grant.role)
        .collect();
    assert_eq!(roles, [Role::Reader, Role::Submitter]);

    let before = r.journal().await.len();
    let refused = r
        .app
        .issue_key(r.admin(), &creating_req("alpha"))
        .await
        .unwrap_err();
    assert!(matches!(refused, AppError::NameTaken { .. }), "{refused:?}");
    assert_eq!(r.journal().await.len(), before, "nothing of the act lands");
}

/// A pending issuance creates nothing and reserves no name: the workspace
/// is absent until a second admin person confirms, and the confirmation
/// creates it and writes the key's grants on it in one commit (ADR-0097
/// d30, d23).
#[tokio::test]
async fn a_pending_issuance_creates_no_workspace_until_it_is_confirmed() {
    let r = two_admins().await;
    let issued = r
        .app
        .issue_key_with_reason(r.admin(), &creating_req("beta"), Some("beta's agent"))
        .await
        .expect("a pending issuance");
    let act_id = issued.pending_act.expect("pending");
    let absent = r
        .app
        .workspace_info(r.admin(), Some(by_name("beta")))
        .await
        .unwrap_err();
    assert!(matches!(absent, AppError::NotFound), "{absent:?}");

    let before = r.journal().await.len();
    let alice = r.admin_session("alice").await;
    r.app
        .confirm_admin_act(Bearer::Session(&alice), act_id)
        .await
        .expect("alice confirms");
    assert_eq!(
        added_since(&r, before).await,
        [
            event_types::ADMIN_ACT,
            event_types::WORKSPACE_CREATE,
            event_types::GRANT_ISSUED,
            event_types::GRANT_ISSUED,
        ]
    );
    let beta = r
        .app
        .workspace_info(r.admin(), Some(by_name("beta")))
        .await
        .expect("created at the confirmation")
        .workspace
        .workspace_id;
    let access = r
        .store
        .key_access(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(access.grants.iter().all(|g| g.grant.node == beta));
    assert_eq!(access.grants.len(), 2);
}

/// The admin's session key creates a workspace directly under `universe`,
/// a neutral admin act: at once, journalled as `admin.act` with nothing to
/// settle (ADR-0097 d9, d29).
#[tokio::test]
async fn the_admin_session_creates_a_workspace_under_universe_as_a_neutral_act() {
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    let before = r.journal().await.len();
    r.app
        .create_workspace(
            Bearer::Session(&alice),
            &name("projects"),
            Some(by_name("universe")),
            None,
        )
        .await
        .expect("created at once");
    let events = r.journal().await;
    let act = &events[before].envelope.payload;
    assert_eq!(act["act"], "create_workspace");
    assert_eq!(act["class"], "neutral");
    assert_eq!(act["state"], "effective");
    assert_eq!(
        events[before + 1].envelope.event_type,
        event_types::WORKSPACE_CREATE
    );
}

/// A key creates a child only where a grant carrying `create` reaches: an
/// unflagged key is refused by its role, and a flagged subtree grant admits
/// the child, already covered, so no grant is written (ADR-0097 d31).
#[tokio::test]
async fn a_key_creates_a_child_only_under_a_grant_that_carries_create() {
    let r = rig().await;
    let plain = r
        .app
        .issue_key(r.admin(), &agent_req())
        .await
        .expect("issued");
    let refused = r
        .app
        .create_workspace(
            plain.value.as_str(),
            &name("c1"),
            Some(by_name("crew")),
            None,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(refused, AppError::VerbCeiling { .. }),
        "{refused:?}"
    );

    let flagged = r
        .app
        .issue_key(r.admin(), &flagged_req(Covers::Subtree))
        .await
        .expect("issued");
    assert!(flagged.access.grants.iter().any(|g| g.grant.create));
    let before = r.journal().await.len();
    r.app
        .create_workspace(
            flagged.value.as_str(),
            &name("c2"),
            Some(by_name("crew")),
            None,
        )
        .await
        .expect("the flag admits the child");
    assert_eq!(
        added_since(&r, before).await,
        [event_types::WORKSPACE_CREATE]
    );
}

/// Under a node grant carrying `create`, the child gets the same grant,
/// role, coverage and flag, journalled in the create's commit, and no wider
/// role (ADR-0097 d31).
#[tokio::test]
async fn a_child_under_a_node_grant_carries_the_same_grant_and_no_wider_role() {
    let r = rig().await;
    let flagged = r
        .app
        .issue_key(r.admin(), &flagged_req(Covers::Node))
        .await
        .expect("issued");
    let before = r.journal().await.len();
    let child = r
        .app
        .create_workspace(
            flagged.value.as_str(),
            &name("c3"),
            Some(by_name("crew")),
            None,
        )
        .await
        .expect("created");
    assert_eq!(
        added_since(&r, before).await,
        [event_types::WORKSPACE_CREATE, event_types::GRANT_ISSUED]
    );
    let grant = &r.journal().await[before + 1].envelope.payload;
    assert_eq!(grant["role"], "submitter");
    assert_eq!(grant["covers"], "node");
    assert_eq!(grant["create"], true);
    assert_eq!(
        grant["workspace_node_id"],
        child.workspace.workspace_id.to_string()
    );
    let access = r
        .store
        .key_access(r.boot.tenant_id, flagged.key_id)
        .await
        .expect("read")
        .expect("held");
    let on_child: Vec<Role> = access
        .grants
        .iter()
        .filter(|g| g.grant.node == child.workspace.workspace_id)
        .map(|g| g.grant.role)
        .collect();
    assert_eq!(
        on_child,
        [Role::Submitter],
        "no wider role than the parent's"
    );
}

/// `oml_write`'s create reaches a shared parent only through the flag: a
/// tenant key without it is `create_is_own_root_only`, and one whose grant
/// carries it creates there (ADR-0097 d31; ADR-0095 d3).
#[tokio::test]
async fn oml_write_create_workspace_reaches_a_shared_parent_only_through_the_create_flag() {
    let r = rig().await;
    let plain = r
        .app
        .issue_key(r.admin(), &agent_req())
        .await
        .expect("issued");
    let refused = r
        .app
        .create_own_workspace(
            plain.value.as_str(),
            &name("m1"),
            Some(by_name("crew")),
            None,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(refused, AppError::CreateIsOwnRootOnly { .. }),
        "{refused:?}"
    );
    let flagged = r
        .app
        .issue_key(r.admin(), &flagged_req(Covers::Subtree))
        .await
        .expect("issued");
    r.app
        .create_own_workspace(
            flagged.value.as_str(),
            &name("m2"),
            Some(by_name("crew")),
            None,
        )
        .await
        .expect("the flag admits the shared parent");
}

/// Granting `create` is widening: with two admin persons a flagged key
/// waits on the second one's confirmation, and the flag is in force only
/// after it (ADR-0097 d31, d5).
#[tokio::test]
async fn granting_create_waits_on_a_second_admin_person() {
    let r = two_admins().await;
    let issued = r
        .app
        .issue_key_with_reason(r.admin(), &flagged_req(Covers::Subtree), Some("a builder"))
        .await
        .expect("a pending issuance");
    let act_id = issued.pending_act.expect("pending");
    let alice = r.admin_session("alice").await;
    r.app
        .confirm_admin_act(Bearer::Session(&alice), act_id)
        .await
        .expect("alice confirms");
    let access = r
        .store
        .key_access(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(
        access.grants.iter().any(|g| g.grant.create),
        "the confirmed effect keeps the flag"
    );
}
