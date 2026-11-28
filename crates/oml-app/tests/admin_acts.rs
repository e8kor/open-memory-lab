//! The admin act (ADR-0097 d1, d2, d5, d10, d11, d23): one journalled
//! envelope per act, whose class the act decides; widening at once only
//! under `sole_admin`, and otherwise pending until a second admin person
//! whose flag is seven days older than the act confirms it.

use oml_app::App;
use oml_app::app::Caller;
use oml_app::app::Capacity;
use oml_app::domain::access::Covers;
use oml_app::domain::admin_act::AdminActState;
use oml_app::domain::name::{Name, NamedKind, Reference};
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

    /// An admin entry for `user`, under the bootstrap key. Once the tenant
    /// has had two admin persons the flag is widening and waits (ADR-0097
    /// d5): it is then confirmed by the first admin person whose flag is
    /// seven days old, the clock moved on eight days where none is (d11).
    async fn admin_person(&self, user: &str) {
        let req = WebEmailRequest {
            email: Email::parse(&format!("{user}@example.com")).expect("an email"),
            user: name(user),
            admin: true,
        };
        let entry = match self.app.allow_web_email(self.admin(), &req).await {
            Ok((entry, _)) => entry,
            Err(AppError::AdminActReasonRequired) => {
                // The confirmer's flag must be seven days older than the
                // act, which lives seven days itself (d11, d23).
                self.clock.advance(8 * DAY);
                self.app
                    .allow_web_email_with_reason(self.admin(), &req, true, Some("a test admin"))
                    .await
                    .expect("a pending admin entry");
                let act = self
                    .store
                    .admin_acts(self.boot.tenant_id, None)
                    .await
                    .expect("acts")
                    .into_iter()
                    .rev()
                    .find(|a| a.act.as_str() == "allow_admin")
                    .expect("the pending flag");
                let persons = self
                    .store
                    .admin_standing(self.boot.tenant_id)
                    .await
                    .expect("standing")
                    .persons;
                let confirmer = persons
                    .iter()
                    .find(|p| p.user.as_str() != user && !p.disabled)
                    .expect("an admin person")
                    .user
                    .clone();
                let session = self.admin_session(confirmer.as_str()).await;
                self.app
                    .confirm_admin_act(Bearer::Session(&session), act.act_id, Some("confirmed"))
                    .await
                    .expect("the flag is confirmed");
                self.store
                    .web_entry_by_email(&req.email.digest())
                    .await
                    .expect("a read")
                    .expect("the entry")
            }
            Err(e) => panic!("the admin entry: {e:?}"),
        };
        self.app
            .web_ensure_keys(&entry)
            .await
            .expect("session keys");
    }

    /// A bearer admin key belonging to `user`, an admin person, its link
    /// accepted from `user`'s own session (ADR-0097 d21).
    async fn linked_admin_key(&self, user: &str) -> IssuedKey {
        let issued = self
            .app
            .issue_key(
                self.admin(),
                &IssueKeyRequest {
                    role: Role::Admin,
                    grants: Vec::new(),
                    used_by: Some(UsedBy::Person),
                    person: Some(name(user)),
                    ..agent_req()
                },
            )
            .await
            .expect("an admin key");
        let web = self.person_session(user, Capacity::Web).await;
        self.app
            .accept_key_link(Bearer::Session(&web), issued.key_id.into())
            .await
            .expect("the link is accepted");
        issued
    }

    /// `<user>-admin`, from a session opened now.
    async fn admin_session(&self, user: &str) -> Caller {
        self.person_session(user, Capacity::Admin).await
    }

    /// The session key of `capacity` for `user`, from a session opened now.
    async fn person_session(&self, user: &str, capacity: Capacity) -> Caller {
        let entry = self
            .store
            .web_entry_by_email(
                &Email::parse(&format!("{user}@example.com"))
                    .expect("an email")
                    .digest(),
            )
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
        person: None,
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
        .confirm_admin_act(Bearer::Session(&alice), act_id, Some("confirmed"))
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

/// A confirmation requires a reason, refused by name without one before
/// anything is journalled, and the act keeps the reason it was confirmed
/// with (ADR-0097 d26; amendment of 2026-11-27, answer 18).
#[tokio::test]
async fn a_confirmation_requires_a_reason_and_the_act_keeps_it() {
    let r = two_admins().await;
    let issued = pending_key(&r).await;
    let act_id = issued.pending_act.expect("pending");
    let alice = r.admin_session("alice").await;
    let before = r.journal().await.len();
    for blank in [None, Some("  ")] {
        let err = r
            .app
            .confirm_admin_act(Bearer::Session(&alice), act_id, blank)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::AdminActReasonRequired), "{err:?}");
    }
    assert_eq!(r.journal().await.len(), before, "nothing is journalled");
    let confirmed = r
        .app
        .confirm_admin_act(Bearer::Session(&alice), act_id, Some("checked with ops"))
        .await
        .expect("alice confirms with a reason");
    assert_eq!(
        confirmed.confirm_reason.as_deref(),
        Some("checked with ops")
    );
    let held = r
        .store
        .admin_act(r.boot.tenant_id, act_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(held.confirm_reason.as_deref(), Some("checked with ops"));
    let events = r.journal().await;
    let act = &events[before].envelope.payload;
    assert_eq!(act["state"], "confirmed");
    assert_eq!(act["confirm_reason"], "checked with ops");
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
            Some("confirmed"),
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
            Some("confirmed"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::OwnAct)),
        "{err:?}"
    );
    let bob = r.admin_session("bob").await;
    r.app
        .confirm_admin_act(
            Bearer::Session(&bob),
            issued.pending_act.expect("pending"),
            Some("confirmed"),
        )
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
        .confirm_admin_act(Bearer::Session(&alice), act_id, Some("confirmed"))
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
        .confirm_admin_act(Bearer::Session(&alice), act_id, Some("confirmed"))
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
        .remove_web_email_with_reason(
            r.admin(),
            &Email::parse("bob@example.com").expect("an email"),
            true,
            Some("bob left"),
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
/// admin's revocation of another admin person's key rides a
/// `revoke_admin_key` act against an admin.
#[tokio::test]
async fn one_revoke_verb_ends_any_key_and_cascades_to_none() {
    let r = rig().await;
    r.admin_person("carol").await;
    let second_admin = r.linked_admin_key("carol").await;
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
    // An act against an admin is loud: its notice closes the commit
    // (ADR-0097 d20).
    let notice = &events[events.len() - 1].envelope;
    assert_eq!(notice.event_type, event_types::NOTICE_WRITTEN);
    assert_eq!(notice.payload["kind"], "act_against_admin");
    let last = &events[events.len() - 2].envelope;
    assert_eq!(last.event_type, event_types::KEY_REVOKED);
    assert_eq!(last.payload["reason"], "leaked");
    assert_eq!(
        last.payload["act_id"],
        revoked.act_id.expect("an act").to_string()
    );
    let act = &events[events.len() - 3].envelope;
    assert_eq!(act.event_type, event_types::ADMIN_ACT);
    // Another admin person's key: an act against an admin, final (d8).
    assert_eq!(act.payload["act"], "revoke_admin_key");
    assert_eq!(act.payload["class"], "against_admin");

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
    // An act against an admin carries a reason (d26), so the guard is
    // what refuses it.
    let err = r
        .app
        .remove_web_email_with_reason(r.admin(), &alices, true, Some("a test"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::WebField { field: "admin", .. }),
        "{err:?}"
    );
    let err = r
        .app
        .allow_web_email_with_reason(
            r.admin(),
            &WebEmailRequest {
                email: alices,
                user: name("alice"),
                admin: false,
            },
            true,
            Some("a test"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::WebField { field: "admin", .. }),
        "{err:?}"
    );
}

// -- a link is a consent act (ADR-0097 d7; amendment of 2026-11-27, answer 15) --

/// Linking a key to a person is a consent admin act with a required reason:
/// a link without one is refused before anything is journalled, by an
/// admin's console session and by a key's self-link alike, and one with it
/// journals `admin.act` of class `consent` and then `key.linked_person`
/// naming the act.
#[tokio::test]
async fn a_link_is_a_consent_admin_act_that_requires_a_reason() {
    let r = rig().await;
    r.admin_person("alice").await;
    let rhea = r
        .app
        .issue_key(r.admin(), &person_reviewer_req("rhea"))
        .await
        .expect("rhea's reviewer key");
    let rhea_entry = r
        .app
        .allow_web_email(r.admin(), &entry_of("rhea", false))
        .await
        .expect("rhea's entry")
        .0;
    r.app.web_ensure_keys(&rhea_entry).await.expect("keys");
    let alice = r.admin_session("alice").await;
    let before = r.journal().await.len();
    let err = r
        .app
        .link_key_to_person(
            Bearer::Session(&alice),
            rhea.key_id.into(),
            &name("rhea"),
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::AdminActReasonRequired), "{err:?}");
    let err = r
        .app
        .link_self(r.admin(), &name("alice"), None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::AdminActReasonRequired), "{err:?}");
    assert_eq!(r.journal().await.len(), before, "nothing is journalled");
    for own in [false, true] {
        let (linked, actor) = if own {
            (
                r.app
                    .link_self(r.admin(), &name("alice"), Some("rhea's laptop reviewer"))
                    .await
                    .expect("self-linked"),
                Some("self"),
            )
        } else {
            (
                r.app
                    .link_key_to_person(
                        Bearer::Session(&alice),
                        rhea.key_id.into(),
                        &name("rhea"),
                        Some("rhea's laptop reviewer"),
                    )
                    .await
                    .expect("linked"),
                None,
            )
        };
        let events = r.journal().await;
        let (act, link) = (
            &events[events.len() - 2].envelope,
            &events[events.len() - 1].envelope,
        );
        assert_eq!(act.event_type, event_types::ADMIN_ACT);
        assert_eq!(act.payload["act"], "link_key");
        assert_eq!(act.payload["class"], "consent");
        assert_eq!(act.payload["state"], "effective");
        assert_eq!(act.payload["reason"], "rhea's laptop reviewer");
        assert_eq!(act.payload["target"]["key_id"], linked.key_id.to_string());
        assert_eq!(link.event_type, event_types::KEY_LINKED_PERSON);
        assert_eq!(link.payload["act_id"], act.payload["act_id"]);
        assert_eq!(link.payload["actor"].as_str(), actor);
        assert!(
            linked.link_pending.is_some(),
            "the person's acceptance waits"
        );
    }
}

// -- self-link by possession (ADR-0097 d22) -------------------------------------

/// An admin key links itself to its holder's person by possession, and the
/// link waits on that person: the key acts for no one until alice accepts
/// it from her own console session.
#[tokio::test]
async fn an_admin_key_links_to_its_holders_person_only_once_that_person_accepts() {
    let r = rig().await;
    r.admin_person("alice").await;
    // The bootstrap key stands unlinked, as a key from before v0.8.6 does.
    let linked = r
        .app
        .link_self(r.admin(), &name("alice"), Some("the person holds this key"))
        .await
        .expect("a pending self-link");
    assert!(linked.link_pending.is_some());
    assert_eq!(linked.linked_user, None, "pending: it acts for no one yet");
    let last = r.journal().await.pop().expect("an event").envelope;
    assert_eq!(last.event_type, event_types::KEY_LINKED_PERSON);
    assert_eq!(last.payload["actor"], "self");
    let err = r.app.admin_acts(r.admin(), None).await.unwrap_err();
    assert!(matches!(err, AppError::KeyLinkPending), "{err:?}");
    r.app
        .whoami(r.admin())
        .await
        .expect("whoami still answers a key waiting on its link");
    let web = r.person_session("alice", Capacity::Web).await;
    r.app
        .accept_key_link(Bearer::Session(&web), r.boot.admin_key.key_id.into())
        .await
        .expect("alice accepts");
    let key = r
        .store
        .key(r.boot.tenant_id, r.boot.admin_key.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(key.linked_user.is_some());
    r.app
        .admin_acts(r.admin(), None)
        .await
        .expect("the linked key authenticates");
}

/// An admin key is issued only with a person, `used_by: person`; it comes
/// with a link to that person in the issuance's chain and authenticates
/// nothing until the person accepts it. A `person` on any other role is
/// refused by name (ADR-0097 d21).
#[tokio::test]
async fn an_admin_key_is_refused_without_a_person() {
    let r = rig().await;
    r.admin_person("alice").await;
    let admin_req = |person: Option<&str>, used_by: UsedBy| IssueKeyRequest {
        role: Role::Admin,
        grants: Vec::new(),
        used_by: Some(used_by),
        person: person.map(name),
        ..agent_req()
    };
    for req in [
        admin_req(None, UsedBy::Person),
        admin_req(Some("alice"), UsedBy::Service),
    ] {
        let err = r.app.issue_key(r.admin(), &req).await.unwrap_err();
        assert!(
            matches!(err, AppError::KeyPerson("admin_key_requires_person")),
            "{err:?}"
        );
    }
    let err = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                person: Some(name("alice")),
                ..agent_req()
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::KeyPerson("person_admin_only")),
        "{err:?}"
    );
    let before = r.journal().await.len();
    let cli = r
        .app
        .issue_key(r.admin(), &admin_req(Some("alice"), UsedBy::Person))
        .await
        .expect("alice's admin key");
    let added: Vec<String> = r.journal().await[before..]
        .iter()
        .map(|e| e.envelope.event_type.clone())
        .collect();
    assert_eq!(
        added,
        [
            event_types::ADMIN_ACT,
            event_types::KEY_ISSUED,
            event_types::KEY_LINKED_PERSON
        ]
    );
    let err = r
        .app
        .admin_acts(cli.value.as_str(), None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::KeyLinkPending), "{err:?}");
    let web = r.person_session("alice", Capacity::Web).await;
    r.app
        .accept_key_link(Bearer::Session(&web), cli.key_id.into())
        .await
        .expect("alice accepts");
    r.app
        .admin_acts(cli.value.as_str(), None)
        .await
        .expect("the key authenticates once its person accepts");
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
        .confirm_admin_act(Bearer::Session(&alice), act.act_id, Some("confirmed"))
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
        .confirm_admin_act(Bearer::Session(&bob), act.act_id, Some("confirmed"))
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

// -- declining a sole_admin act undoes it (the operator, 2026-11-27) -------

/// The admin act of `act` journalled last in the tenant.
async fn last_act(r: &Rig, act: &str) -> oml_app::types::AdminActRecord {
    r.store
        .admin_acts(r.boot.tenant_id, None)
        .await
        .expect("acts")
        .into_iter()
        .rev()
        .find(|a| a.act.as_str() == act)
        .expect("the act")
}

/// The events journalled after `before`, by type, and the decline's act id
/// each names.
async fn undo_events(r: &Rig, before: usize) -> Vec<(String, String)> {
    r.journal().await[before..]
        .iter()
        .map(|e| {
            (
                e.envelope.event_type.clone(),
                e.envelope.payload["act_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

/// In a `sole_admin` tenant a declined widening act is undone at once by
/// the matching narrowing effect, each event naming the decline's act: the
/// grant it added is revoked, the limit it raised is set back, the key it
/// issued is revoked and the admin flag it set comes off.
#[tokio::test]
async fn a_declined_sole_admin_act_is_revoked_at_once() {
    let r = rig().await;
    // A person with no admin flag, and their web key's added reader grant.
    let (dana, _) = r
        .app
        .allow_web_email(
            r.admin(),
            &WebEmailRequest {
                email: Email::parse("dana@example.com").expect("an email"),
                user: name("dana"),
                admin: false,
            },
        )
        .await
        .expect("dana's entry");
    r.app.web_ensure_keys(&dana).await.expect("dana's keys");
    r.admin_person("alice").await;
    let alice = r.admin_session("alice").await;
    let granted = r
        .app
        .grant_read_capacity(&alice, &name("dana"), by_name("crew"))
        .await
        .expect("a sole_admin grant at once");
    let grant_id = granted.grants_added[0].grant.grant_id;
    let act = last_act(&r, "add_grant").await;
    assert!(act.sole_admin);
    let before = r.journal().await.len();
    let declined = r
        .app
        .decline_admin_act(r.admin(), act.act_id, Some("not this reach"))
        .await
        .expect("declined");
    assert_eq!(declined.state, AdminActState::Declined);
    let events = undo_events(&r, before).await;
    let decline_id = events[0].1.clone();
    assert_eq!(events[0].0, event_types::ADMIN_ACT);
    assert_eq!(events[2], ("grant.revoked".to_owned(), decline_id));
    let access = r
        .store
        .key_access(r.boot.tenant_id, granted.key.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(
        access.grants.iter().all(|g| g.grant.grant_id != grant_id),
        "the revoked grant confers nothing"
    );
    assert_eq!(access.permission_epoch(), (before + 3) as u64);

    // A raise taken at once is set back to the limits it found.
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
    let (raised, act) = r
        .app
        .set_key_limits_with_reason(
            r.admin(),
            issued.key_id.into(),
            oml_app::types::KeyLimitsChange {
                expiry: None,
                requests_per_minute: Some(Some(600)),
            },
            None,
        )
        .await
        .expect("a sole_admin raise at once");
    assert_eq!(raised.requests_per_minute, Some(600));
    r.app
        .decline_admin_act(Bearer::Session(&alice), act.act_id, None)
        .await
        .expect("declined");
    let held = r
        .store
        .key(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(held.requests_per_minute, Some(60), "the raise is undone");

    // The key a sole_admin issuance issued is revoked.
    let issue = last_act(&r, "issue_key").await;
    r.app
        .decline_admin_act(Bearer::Session(&alice), issue.act_id, None)
        .await
        .expect("declined");
    let err = r.app.whoami(&issued.value).await.unwrap_err();
    assert!(matches!(err, AppError::Unauthorized), "{err:?}");

    // An admin flag set at once comes off its entry.
    r.app
        .allow_web_email(
            r.admin(),
            &WebEmailRequest {
                email: Email::parse("dana@example.com").expect("an email"),
                user: name("dana"),
                admin: true,
            },
        )
        .await
        .expect("a sole_admin flag at once");
    let flag = last_act(&r, "allow_admin").await;
    assert!(flag.sole_admin);
    r.app
        .decline_admin_act(Bearer::Session(&alice), flag.act_id, None)
        .await
        .expect("declined");
    let entry = r
        .store
        .web_entry_by_email(&Email::parse("dana@example.com").expect("an email").digest())
        .await
        .expect("read")
        .expect("dana's entry");
    assert!(!entry.admin, "the flag is undone");
}

/// The administration To do counts "Admin acts to confirm": a pending act
/// is counted for every admin person who may confirm it and never for the
/// person who took it (ADR-0097 d2, d11).
#[tokio::test]
async fn admin_acts_to_confirm_counts_what_another_admin_person_may_confirm() {
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    let bob = r.admin_session("bob").await;
    let counts = |caller| {
        let app = &r.app;
        async move {
            app.admin_todo_counts(Bearer::Session(caller))
                .await
                .expect("the counts")
                .acts_to_confirm
        }
    };
    // The two flags taken at once under `sole_admin` are already counted
    // for each of them (d10); the pending issuance adds to bob's alone.
    let (bobs_before, alices_before) = (counts(&bob).await, counts(&alice).await);
    r.app
        .issue_key_with_reason(Bearer::Session(&alice), &agent_req(), Some("an agent"))
        .await
        .expect("a pending issuance");
    assert_eq!(counts(&bob).await, bobs_before + 1);
    assert_eq!(
        counts(&alice).await,
        alices_before,
        "never the person's own act"
    );
    r.clock.advance(7 * DAY);
    let bob = r.admin_session("bob").await;
    assert_eq!(
        counts(&bob).await,
        bobs_before,
        "an expired act is not counted"
    );
}

// -- acts on an admin person's standing (ADR-0097 d8, d21) -----------------

/// Alice, bob and carol, admin persons accepted eight days ago, and carol's
/// linked bearer admin key.
async fn three_admins() -> (Rig, IssuedKey) {
    let r = rig().await;
    // Carol's key is issued while she is the one admin person, so it takes
    // effect at once (ADR-0097 d10).
    r.admin_person("carol").await;
    let carols = r.linked_admin_key("carol").await;
    for user in ["alice", "bob"] {
        r.admin_person(user).await;
    }
    r.clock.advance(8 * DAY);
    (r, carols)
}

fn entry_of(user: &str, admin: bool) -> WebEmailRequest {
    WebEmailRequest {
        email: Email::parse(&format!("{user}@example.com")).expect("an email"),
        user: name(user),
        admin,
    }
}

/// An admin person's own un-flag revokes their admin keys at once, `cause:
/// deactivation`, in the act's commit (ADR-0097 d21).
#[tokio::test]
async fn an_admin_persons_own_deactivation_revokes_their_admin_keys() {
    let (r, carols) = three_admins().await;
    let carol = r.admin_session("carol").await;
    r.app
        .allow_web_email(Bearer::Session(&carol), &entry_of("carol", false))
        .await
        .expect("carol takes her own flag off");
    let act = last_act(&r, "unflag_own").await;
    assert_eq!(act.class().as_str(), "narrowing");
    let key = r
        .store
        .key(r.boot.tenant_id, carols.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(key.revoked_at.is_some(), "her admin key is revoked at once");
    let revoked = r
        .journal()
        .await
        .into_iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::KEY_REVOKED)
        .expect("the revocation");
    assert_eq!(revoked.envelope.payload["cause"], "deactivation");
    assert_eq!(revoked.envelope.payload["act_id"], act.act_id.to_string());
}

/// Another admin's un-flag takes effect at once and suspends the target's
/// admin keys; with three admin persons a third settles it, and confirmed
/// the keys are revoked (ADR-0097 d8, d21).
#[tokio::test]
async fn an_act_against_another_admin_suspends_the_targets_admin_keys_until_settled() {
    let (r, carols) = three_admins().await;
    let alice = r.admin_session("alice").await;
    let err = r
        .app
        .allow_web_email(Bearer::Session(&alice), &entry_of("carol", false))
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::AdminActReasonRequired), "{err:?}");
    r.app
        .allow_web_email_with_reason(
            Bearer::Session(&alice),
            &entry_of("carol", false),
            true,
            Some("carol's laptop was stolen"),
        )
        .await
        .expect("alice un-flags carol");
    let act = last_act(&r, "unflag_admin").await;
    assert_eq!(act.class().as_str(), "against_admin");
    let err = r
        .app
        .admin_acts(carols.value.as_str(), None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::KeySuspended), "{err:?}");
    // Neither party settles.
    let err = r
        .app
        .settle_admin_act(Bearer::Session(&alice), act.act_id, true)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::NotSettler)),
        "{err:?}"
    );
    let bob = r.admin_session("bob").await;
    let settled = r
        .app
        .settle_admin_act(Bearer::Session(&bob), act.act_id, true)
        .await
        .expect("bob confirms");
    assert_eq!(settled.state, AdminActState::Settled);
    assert_eq!(settled.confirmed_by_person, Some(name("bob")));
    let key = r
        .store
        .key(r.boot.tenant_id, carols.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(
        key.revoked_at.is_some(),
        "confirmed: carol's key is revoked"
    );
}

/// A contest restores the flag and sign-in only: the target's admin keys
/// stay suspended until a reversal lifts the suspension (ADR-0097 d8).
#[tokio::test]
async fn a_contest_never_restores_a_revoked_key() {
    let (r, carols) = three_admins().await;
    let alice = r.admin_session("alice").await;
    r.app
        .allow_web_email_with_reason(
            Bearer::Session(&alice),
            &entry_of("carol", false),
            true,
            Some("a dispute"),
        )
        .await
        .expect("alice un-flags carol");
    let act = last_act(&r, "unflag_admin").await;
    let carol_web = r.person_session("carol", Capacity::Web).await;
    let contested = r
        .app
        .contest_admin_act(Bearer::Session(&carol_web), act.act_id)
        .await
        .expect("carol contests");
    assert_eq!(contested.state, AdminActState::Contested);
    let entry = r
        .store
        .web_entry_by_email(&entry_of("carol", true).email.digest())
        .await
        .expect("read")
        .expect("carol's entry");
    assert!(entry.admin, "the flag comes back");
    let err = r
        .app
        .admin_acts(carols.value.as_str(), None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::KeySuspended),
        "a contest does not lift the suspension: {err:?}"
    );
    let bob = r.admin_session("bob").await;
    r.app
        .settle_admin_act(Bearer::Session(&bob), act.act_id, false)
        .await
        .expect("bob reverses");
    r.app
        .admin_acts(carols.value.as_str(), None)
        .await
        .expect("reversed: the suspension lifts");
}

/// Below three admin persons an act against an admin freezes both sides:
/// neither takes a widening act, and the tenant cannot settle it; the
/// operator does from the host (ADR-0097 d8, d19).
#[tokio::test]
async fn below_three_admins_an_act_against_an_admin_freezes_both_sides() {
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    r.app
        .allow_web_email_with_reason(
            Bearer::Session(&alice),
            &entry_of("bob", false),
            true,
            Some("a dispute"),
        )
        .await
        .expect("alice un-flags bob");
    let act = last_act(&r, "unflag_admin").await;
    let freeze = r
        .app
        .freeze(r.boot.tenant_id)
        .await
        .expect("read")
        .expect("a freeze");
    assert_eq!(freeze.act_id, act.act_id);
    let err = r
        .app
        .issue_key_with_reason(Bearer::Session(&alice), &agent_req(), Some("an agent"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::Frozen)),
        "{err:?}"
    );
    let err = r
        .app
        .settle_admin_act(r.admin(), act.act_id, true)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::FewerThanThree)),
        "{err:?}"
    );
}

/// Two admin persons: bob, with an admin key of his own issued while he
/// was the one admin person, and alice; both counted eight days on.
async fn two_admins_bob_keyed() -> (Rig, IssuedKey) {
    let r = rig().await;
    r.admin_person("bob").await;
    let bobs = r.linked_admin_key("bob").await;
    r.admin_person("alice").await;
    r.clock.advance(8 * DAY);
    (r, bobs)
}

/// Alice un-flags bob below three admin persons: a freeze.
async fn frozen_unflag(r: &Rig) -> oml_app::types::AdminActRecord {
    let alice = r.admin_session("alice").await;
    r.app
        .allow_web_email_with_reason(
            Bearer::Session(&alice),
            &entry_of("bob", false),
            true,
            Some("a dispute"),
        )
        .await
        .expect("alice un-flags bob");
    last_act(r, "unflag_admin").await
}

/// The operator's settlement confirms or reverses the contested act of a
/// freeze open 24 hours, and does nothing else: confirmed, the target's
/// suspended admin keys are revoked and the freeze ends; one commit holds
/// the settled `admin.act`, `operator.settlement`, the revocation and a
/// `settlement` notice to every admin person, and the email goes to both
/// (ADR-0097 d19, d20).
#[tokio::test]
async fn settlement_confirms_or_reverses_the_contested_act_and_nothing_else() {
    let (r, bobs) = two_admins_bob_keyed().await;
    let act = frozen_unflag(&r).await;
    let err = r
        .app
        .operator_settle(r.boot.tenant_id, act.act_id, true)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::FreezeTooRecent)),
        "{err:?}"
    );
    r.clock.advance(DAY + 60);
    let before = r.journal().await.len();
    let settled = r
        .app
        .operator_settle(r.boot.tenant_id, act.act_id, true)
        .await
        .expect("the operator confirms");
    assert_eq!(settled.act.state, AdminActState::Settled);
    assert_eq!(settled.act.confirmed_by_person, None);
    assert_eq!(settled.act.settled_by_key_id, None);
    let events = r.journal().await;
    let types: Vec<&str> = events[before..]
        .iter()
        .map(|e| e.envelope.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        [
            event_types::ADMIN_ACT,
            event_types::OPERATOR_SETTLEMENT,
            event_types::KEY_REVOKED,
            event_types::NOTICE_WRITTEN,
        ],
        "the settlement issues no key and changes nothing else"
    );
    let settlement = &events[before + 1].envelope;
    assert_eq!(settlement.principal, oml_app::app::OPERATOR_PRINCIPAL);
    assert_eq!(settlement.api_key_id, None, "no key acts");
    assert_eq!(settlement.payload["outcome"], "confirmed");
    assert_eq!(settlement.payload["act_id"], act.act_id.to_string());
    let notice = &events[before + 3].envelope.payload;
    assert_eq!(notice["kind"], "settlement");
    assert_eq!(notice["recipients"], serde_json::json!(["alice", "bob"]));
    assert_eq!(
        settled.notified,
        vec![name("alice"), name("bob")],
        "every admin person and both parties"
    );
    let mut to: Vec<String> = settled
        .mail
        .to
        .iter()
        .map(|e| e.as_str().to_owned())
        .collect();
    to.sort();
    assert_eq!(to, ["alice@example.com", "bob@example.com"]);
    let key = r
        .store
        .key(r.boot.tenant_id, bobs.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(key.revoked_at.is_some(), "confirmed: bob's key is revoked");
    assert!(
        r.app
            .freeze(r.boot.tenant_id)
            .await
            .expect("read")
            .is_none(),
        "the freeze ends"
    );
    let err = r
        .app
        .operator_settle(r.boot.tenant_id, act.act_id, false)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::NotFrozen)),
        "a settled act is settled once: {err:?}"
    );
}

/// The operator's reversal gives the target back the flag and lifts the
/// suspension of their admin keys (ADR-0097 d8, d19).
#[tokio::test]
async fn an_operator_reversal_restores_the_flag_and_lifts_the_suspension() {
    let (r, bobs) = two_admins_bob_keyed().await;
    let act = frozen_unflag(&r).await;
    let err = r
        .app
        .admin_acts(bobs.value.as_str(), None)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::KeySuspended), "{err:?}");
    r.clock.advance(DAY + 60);
    r.app
        .operator_settle(r.boot.tenant_id, act.act_id, false)
        .await
        .expect("the operator reverses");
    let entry = r
        .store
        .web_entry_by_email(&entry_of("bob", true).email.digest())
        .await
        .expect("read")
        .expect("bob's entry");
    assert!(entry.admin, "the flag comes back");
    r.app
        .admin_acts(bobs.value.as_str(), None)
        .await
        .expect("reversed: the suspension lifts");
}

/// With three or more admin persons no freeze opens, so the host settles
/// nothing: a third admin person settles in the tenant (ADR-0097 d8, d19).
#[tokio::test]
async fn the_operator_settles_only_a_freeze() {
    let (r, _) = three_admins().await;
    let alice = r.admin_session("alice").await;
    r.app
        .allow_web_email_with_reason(
            Bearer::Session(&alice),
            &entry_of("carol", false),
            true,
            Some("a dispute"),
        )
        .await
        .expect("alice un-flags carol");
    let act = last_act(&r, "unflag_admin").await;
    r.clock.advance(2 * DAY);
    let err = r
        .app
        .operator_settle(r.boot.tenant_id, act.act_id, true)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::AdminAct(AdminActRefusal::NotFrozen)),
        "{err:?}"
    );
    let stats = r.app.fix_stats(r.boot.tenant_id).await.expect("stats");
    assert_eq!(stats.settlements_due, 0);
}

/// Doctor counts a freeze the operator may settle once it is 24 hours old
/// (`operator.settlement_due`, ADR-0097 d19).
#[tokio::test]
async fn a_freeze_open_a_day_is_due_for_the_operators_settlement() {
    let (r, _) = two_admins_bob_keyed().await;
    frozen_unflag(&r).await;
    let stats = r.app.fix_stats(r.boot.tenant_id).await.expect("stats");
    assert_eq!((stats.disputes_open, stats.settlements_due), (1, 0));
    r.clock.advance(DAY);
    let stats = r.app.fix_stats(r.boot.tenant_id).await.expect("stats");
    assert_eq!((stats.disputes_open, stats.settlements_due), (1, 1));
}

/// Doctor flags any key the server issuer issued that is not one of the
/// four session-key shapes, and none of the session keys it issues
/// (`key.issuer_unexpected_shape`, ADR-0097 d18).
#[tokio::test]
async fn doctor_flags_a_server_issuer_key_outside_the_session_key_shapes() {
    let r = rig().await;
    r.admin_person("alice").await;
    let stats = r.app.fix_stats(r.boot.tenant_id).await.expect("stats");
    assert_eq!(stats.issuer_unexpected_shape, 0, "alice's session keys");
    let agent = r
        .app
        .issue_key(r.admin(), &agent_req())
        .await
        .expect("an agent key");
    r.store.set_key_issuer(
        r.boot.tenant_id,
        agent.key_id,
        oml_app::app::server_issuer_id(r.boot.tenant_id),
    );
    let stats = r.app.fix_stats(r.boot.tenant_id).await.expect("stats");
    assert_eq!(stats.issuer_unexpected_shape, 1);
}

/// The upgrade window: an admin key with no accepted link is suspended at
/// the start, still reaches whoami and its self-link, and authenticates
/// again once its person accepts the link; or it converts itself to a
/// maintenance key, which lifts the unlinked suspension in the
/// conversion's commit, so it then reads counts (ADR-0097 d18, d21, d22;
/// the production migration; D27 of 2026-11-28).
#[tokio::test]
async fn a_suspended_unlinked_admin_key_still_links_itself_and_converts_itself() {
    let r = rig().await;
    r.admin_person("alice").await;
    let sweep = r
        .app
        .suspend_unlinked_admin_keys(&[r.boot.tenant_id])
        .await
        .expect("the upgrade window");
    assert_eq!((sweep.suspended, sweep.revoked, sweep.converted), (1, 0, 0));
    let again = r
        .app
        .suspend_unlinked_admin_keys(&[r.boot.tenant_id])
        .await
        .expect("a second start");
    assert_eq!(
        (again.suspended, again.revoked, again.converted),
        (0, 0, 0),
        "a later start journals nothing"
    );
    let err = r.app.admin_acts(r.admin(), None).await.unwrap_err();
    assert!(matches!(err, AppError::KeySuspended), "{err:?}");
    r.app.whoami(r.admin()).await.expect("whoami still answers");
    r.app
        .link_self(r.admin(), &name("alice"), Some("the person holds this key"))
        .await
        .expect("it links itself");
    let web = r.person_session("alice", Capacity::Web).await;
    r.app
        .accept_key_link(Bearer::Session(&web), r.boot.admin_key.key_id.into())
        .await
        .expect("alice accepts");
    r.app
        .admin_acts(r.admin(), None)
        .await
        .expect("linked, it authenticates again");

    // The other way out: the suspended key converts itself.
    let r = rig().await;
    r.admin_person("alice").await;
    r.app
        .suspend_unlinked_admin_keys(&[r.boot.tenant_id])
        .await
        .expect("the upgrade window");
    let err = r.app.rescan(r.admin(), 0, 10).await.unwrap_err();
    assert!(matches!(err, AppError::KeySuspended), "{err:?}");
    let converted = r
        .app
        .convert_self(r.admin(), Role::Maintenance)
        .await
        .expect("the suspended key converts itself");
    assert_eq!(converted.role, Role::Maintenance);
    assert!(converted.suspended.is_none());
    let types: Vec<String> = r
        .journal()
        .await
        .iter()
        .rev()
        .take(2)
        .map(|e| e.envelope.event_type.clone())
        .collect();
    assert_eq!(
        types,
        [event_types::KEY_CONVERTED, event_types::KEY_SUSPENSION_LIFTED],
        "the suspension is lifted in the conversion's commit, before it"
    );
    let held = r
        .store
        .key(r.boot.tenant_id, r.boot.admin_key.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(held.suspended.is_none(), "replayed, the key is not suspended");
    r.app
        .rescan(r.admin(), 0, 10)
        .await
        .expect("the maintenance key reads counts");
}

/// The first start converts a live admin key `used_by: service` to a
/// maintenance key under the server, `key.converted {cause: upgrade}`, and
/// leaves an admin key whose link awaits its person unswept (D27 of
/// 2026-11-28; ADR-0097 d18, the production migration).
#[tokio::test]
async fn the_first_start_converts_service_admin_keys_and_sweeps_no_key_whose_link_is_pending() {
    let r = rig().await;
    r.admin_person("alice").await;
    r.store
        .seed_key_used_by(r.boot.tenant_id, r.boot.admin_key.key_id, UsedBy::Service);
    let pending = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                role: Role::Admin,
                grants: Vec::new(),
                used_by: Some(UsedBy::Person),
                person: Some(name("alice")),
                ..agent_req()
            },
        )
        .await
        .expect("an admin key whose link waits on alice");
    let sweep = r
        .app
        .suspend_unlinked_admin_keys(&[r.boot.tenant_id])
        .await
        .expect("the upgrade window");
    assert_eq!((sweep.suspended, sweep.revoked, sweep.converted), (0, 0, 1));
    let converted = r
        .journal()
        .await
        .pop()
        .expect("an event")
        .envelope;
    assert_eq!(converted.event_type, event_types::KEY_CONVERTED);
    assert_eq!(converted.payload["cause"], "upgrade");
    assert_eq!(converted.api_key_id, None, "the server converts it");
    let held = r
        .store
        .key(r.boot.tenant_id, r.boot.admin_key.key_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(held.role, Role::Maintenance);
    r.app
        .rescan(r.admin(), 0, 10)
        .await
        .expect("the converted key reads counts");
    let waiting = r
        .store
        .key(r.boot.tenant_id, pending.key_id)
        .await
        .expect("read")
        .expect("held");
    assert!(waiting.suspended.is_none(), "a pending link is not swept");
    let again = r
        .app
        .suspend_unlinked_admin_keys(&[r.boot.tenant_id])
        .await
        .expect("a second start");
    assert_eq!(
        (again.suspended, again.revoked, again.converted),
        (0, 0, 0),
        "a later start journals nothing"
    );
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
/// creates it, writes the key's grants on it and sets it as the key's
/// default with `key.default_set`, in one commit (ADR-0097 d30, d23; the
/// operator's decision of 2026-11-27).
#[tokio::test]
async fn a_pending_issuance_creates_no_workspace_until_it_is_confirmed() {
    let r = two_admins().await;
    let issued = r
        .app
        .issue_key_with_reason(r.admin(), &creating_req("beta"), Some("beta's agent"))
        .await
        .expect("a pending issuance");
    let act_id = issued.pending_act.expect("pending");
    let waiting = r
        .store
        .key(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(waiting.default_workspace, None, "nothing created yet");
    let absent = r
        .app
        .workspace_info(r.admin(), Some(by_name("beta")))
        .await
        .unwrap_err();
    assert!(matches!(absent, AppError::NotFound), "{absent:?}");

    let before = r.journal().await.len();
    let alice = r.admin_session("alice").await;
    r.app
        .confirm_admin_act(Bearer::Session(&alice), act_id, Some("confirmed"))
        .await
        .expect("alice confirms");
    assert_eq!(
        added_since(&r, before).await,
        [
            event_types::ADMIN_ACT,
            event_types::WORKSPACE_CREATE,
            event_types::GRANT_ISSUED,
            event_types::GRANT_ISSUED,
            event_types::KEY_DEFAULT_SET,
        ]
    );
    let set = r.journal().await.pop().expect("key.default_set").envelope;
    assert_eq!(set.payload["act_id"], act_id.to_string());
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
    assert_eq!(set.payload["default_workspace_id"], beta.to_string());
    let key = r
        .store
        .key(r.boot.tenant_id, issued.key_id)
        .await
        .expect("read")
        .expect("held");
    assert_eq!(key.default_workspace, Some(beta), "the default is set");
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
        .confirm_admin_act(Bearer::Session(&alice), act_id, Some("confirmed"))
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

/// A flagged child in the shared tree is charged to no account: the
/// `workspaces` meter of the admin person whose act granted the flag does
/// not move, the create's event names the grant that admitted it, and the
/// store counts it per creating key for doctor's `workspace.flagged_creates`
/// (ADR-0097 d31, the operator's decision of 2026-11-27).
#[tokio::test]
async fn a_flagged_child_in_the_shared_tree_is_charged_to_no_account() {
    use oml_app::domain::usage::Meter;
    let r = rig().await;
    r.admin_person("alice").await;
    let alice = r.admin_session("alice").await;
    let flagged = r
        .app
        .issue_key(Bearer::Session(&alice), &flagged_req(Covers::Subtree))
        .await
        .expect("issued at once under sole_admin");
    let root = r
        .store
        .resolve_name(r.boot.tenant_id, NamedKind::Workspace, &name("alice"))
        .await
        .expect("read")
        .expect("alice's root");
    let used = |usage: oml_app::types::AccountUsage| {
        usage
            .meters
            .iter()
            .find(|m| m.meter == Meter::Workspaces)
            .map_or(0, |m| m.used)
    };
    let before = used(r.app.usage_of(r.boot.tenant_id, root).await.expect("usage"));
    r.app
        .create_workspace(
            flagged.value.as_str(),
            &name("c9"),
            Some(by_name("crew")),
            None,
        )
        .await
        .expect("the flag admits the child");
    let event = r.journal().await.pop().expect("workspace.create").envelope;
    assert_eq!(event.event_type, event_types::WORKSPACE_CREATE);
    let grant = flagged
        .access
        .grants
        .iter()
        .find(|g| g.grant.create)
        .expect("the flagged grant");
    assert_eq!(
        event.payload["create_grant_id"],
        grant.grant.grant_id.to_string()
    );
    assert_eq!(
        used(r.app.usage_of(r.boot.tenant_id, root).await.expect("usage")),
        before,
        "no account is charged for the shared tree"
    );
    assert_eq!(
        r.store
            .flagged_creates(r.boot.tenant_id)
            .await
            .expect("the count"),
        [(flagged.key_id, 1)]
    );
}

/// The person on an admin key holds a live admin flag (ADR-0097 d21, the
/// operator's decision of 2026-11-27): an issuance naming a person with no
/// flag is refused `admin_key_requires_admin_person` and journals nothing,
/// and a link to an admin key is refused at its acceptance by a person
/// whose flag came off before they accepted it.
#[tokio::test]
async fn an_admin_keys_person_must_hold_an_admin_flag() {
    let r = rig().await;
    r.admin_person("carol").await;
    let (dana, _) = r
        .app
        .allow_web_email(r.admin(), &entry_of("dana", false))
        .await
        .expect("dana's entry");
    r.app.web_ensure_keys(&dana).await.expect("dana's keys");
    let admin_req = |person: &str| IssueKeyRequest {
        role: Role::Admin,
        grants: Vec::new(),
        used_by: Some(UsedBy::Person),
        person: Some(name(person)),
        ..agent_req()
    };
    let before = r.journal().await.len();
    let err = r
        .app
        .issue_key(r.admin(), &admin_req("dana"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::KeyPerson("admin_key_requires_admin_person")),
        "{err:?}"
    );
    assert_eq!(r.journal().await.len(), before, "nothing is journalled");

    // Carol's key is issued while she holds the flag; she loses it before
    // she accepts the link.
    let issued = r
        .app
        .issue_key(r.admin(), &admin_req("carol"))
        .await
        .expect("carol's admin key");
    r.admin_person("erin").await;
    let erin = r.admin_session("erin").await;
    let web = r.person_session("carol", Capacity::Web).await;
    r.app
        .allow_web_email_with_reason(
            Bearer::Session(&erin),
            &entry_of("carol", false),
            true,
            Some("carol moves teams"),
        )
        .await
        .expect("carol's flag comes off");
    let err = r
        .app
        .accept_key_link(Bearer::Session(&web), issued.key_id.into())
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::KeyLinkRefused {
                reason: "not_admin_person"
            }
        ),
        "{err:?}"
    );
}

/// Every `create_workspaces[]` parent already exists: a creation under
/// another creation of the same request is refused as an unknown
/// workspace, and nothing of the issuance is journalled (ADR-0097 d30, the
/// operator's decision of 2026-11-27).
#[tokio::test]
async fn no_creation_nests_under_another_creation_of_the_same_issuance() {
    let r = rig().await;
    let before = r.journal().await.len();
    let req = IssueKeyRequest {
        create_workspaces: vec![
            new_workspace("crew", "outer"),
            new_workspace("outer", "inner"),
        ],
        ..creating_req("unused")
    };
    let err = r.app.issue_key(r.admin(), &req).await.unwrap_err();
    assert!(matches!(err, AppError::NotFound), "{err:?}");
    assert_eq!(r.journal().await.len(), before, "nothing is journalled");
}

fn grant_on(node: &str, role: Role) -> GrantRequest {
    GrantRequest {
        node: by_name(node),
        role,
        deny: Vec::new(),
        covers: Covers::Subtree,
        create: false,
    }
}

fn person_reviewer_req(principal: &str) -> IssueKeyRequest {
    IssueKeyRequest {
        role: Role::Reviewer,
        principal: principal.to_owned(),
        used_by: Some(UsedBy::Person),
        grants: vec![grant_on("crew", Role::Reviewer)],
        ..agent_req()
    }
}

fn submit_req(idem: &str, kind: oml_app::domain::MemoryKind) -> oml_app::types::SubmitRequest {
    oml_app::types::SubmitRequest {
        on_similar: Default::default(),
        anchor: None,
        origin: None,
        idempotency_key: idem.to_owned(),
        claim_key: Some(format!("claim-{idem}")),
        verb: None,
        kind,
        body: format!("the body of {idem}"),
        workspace: Some(by_name("crew")),
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

async fn held(r: &Rig, writer: &IssuedKey, idem: &str, kind: oml_app::domain::MemoryKind) -> Uuid {
    r.app
        .submit_memory(writer.value.as_str(), &submit_req(idem, kind))
        .await
        .expect("submitted")
        .receipt
        .memory_id
}

/// A minted `used_by: person` reviewer key that counts as no person approves
/// no instruction: d12 closes the minted-pair attack at the verdict, while a
/// knowledge verdict keeps key-level R2 and the same key approves a fact.
#[tokio::test]
async fn an_instruction_verdict_from_a_key_that_counts_as_no_person_is_refused() {
    let r = rig().await;
    let writer = r
        .app
        .issue_key(r.admin(), &agent_req())
        .await
        .expect("a writer");
    let minted = r
        .app
        .issue_key(r.admin(), &person_reviewer_req("minted"))
        .await
        .expect("a minted reviewer");
    let steering = held(&r, &writer, "steer", oml_app::domain::MemoryKind::Steering).await;
    let refused = r
        .app
        .post_verdict(
            minted.value.as_str(),
            steering,
            oml_app::types::Verdict::Approve,
            None,
        )
        .await
        .expect_err("a key that counts as no person approves no instruction");
    assert!(
        matches!(refused, AppError::CountsAsNoPerson { .. }),
        "{refused:?}"
    );
    let fact = held(&r, &writer, "fact", oml_app::domain::MemoryKind::Memory).await;
    r.app
        .post_verdict(
            minted.value.as_str(),
            fact,
            oml_app::types::Verdict::Approve,
            None,
        )
        .await
        .expect("a knowledge verdict keeps key-level R2");
}

/// A crew's agent reviewer key still approves knowledge in production
/// (ADR-0001 d5, "Agents approve knowledge", kept by ADR-0097 d12).
#[tokio::test]
async fn a_crew_agent_reviewer_key_still_approves_knowledge_in_production() {
    let r = rig().await;
    let writer = r
        .app
        .issue_key(r.admin(), &agent_req())
        .await
        .expect("a writer");
    let agent = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                role: Role::Reviewer,
                principal: "crew-reviewer".to_owned(),
                grants: vec![grant_on("crew", Role::Reviewer)],
                ..agent_req()
            },
        )
        .await
        .expect("an agent reviewer");
    let proposal = r
        .app
        .propose_transition(
            r.admin(),
            by_name("crew"),
            oml_app::domain::WorkspaceStatus::Protected,
        )
        .await
        .expect("a proposal");
    r.admin_person("alice").await;
    let admin = r.admin_session("alice").await;
    r.app
        .ratify_transition(
            Bearer::Session(&admin),
            by_name("crew"),
            proposal.transition_id,
        )
        .await
        .expect("a person ratifies the move to protected");
    let fact = held(&r, &writer, "fact", oml_app::domain::MemoryKind::Memory).await;
    r.app
        .post_verdict(
            agent.value.as_str(),
            fact,
            oml_app::types::Verdict::Approve,
            None,
        )
        .await
        .expect("an agent reviewer approves knowledge in a protected workspace");
}

/// The ladder's ratifier counts as a person (d12) other than the
/// proposer's (d17): a key that counts as no one is refused, the
/// proposer's own person is refused outside `sole_admin`, and another
/// admin person ratifies.
#[tokio::test]
async fn a_ladder_ratifier_must_count_as_a_person_other_than_the_proposer() {
    let r = rig().await;
    r.admin_person("alice").await;
    // Alice's own bearer key, linked while she is the only admin person.
    let alice = r.linked_admin_key("alice").await;
    r.admin_person("bob").await;
    r.clock.advance(8 * DAY);
    let minted = r
        .app
        .issue_key_with_reason(
            r.admin(),
            &person_reviewer_req("minted"),
            Some("a reviewer"),
        )
        .await
        .expect("a pending reviewer");
    let bob = r.admin_session("bob").await;
    r.app
        .confirm_admin_act(
            Bearer::Session(&bob),
            minted.pending_act.expect("pending"),
            Some("confirmed"),
        )
        .await
        .expect("bob confirms");
    let proposal = r
        .app
        .propose_transition(
            alice.value.as_str(),
            by_name("crew"),
            oml_app::domain::WorkspaceStatus::Protected,
        )
        .await
        .expect("alice proposes");
    let none = r
        .app
        .ratify_transition(
            minted.value.as_str(),
            by_name("crew"),
            proposal.transition_id,
        )
        .await
        .expect_err("a key that counts as no person ratifies nothing");
    assert!(
        matches!(none, AppError::CountsAsNoPerson { .. }),
        "{none:?}"
    );
    let alice_review = r.admin_session("alice").await;
    let same = r
        .app
        .ratify_transition(
            Bearer::Session(&alice_review),
            by_name("crew"),
            proposal.transition_id,
        )
        .await
        .expect_err("the proposer's own person ratifies nothing");
    assert!(
        matches!(
            same,
            AppError::RatifierSamePerson {
                issued_the_proposer: false
            }
        ),
        "{same:?}"
    );
    r.app
        .ratify_transition(
            Bearer::Session(&bob),
            by_name("crew"),
            proposal.transition_id,
        )
        .await
        .expect("another admin person ratifies");
}

/// One admin cannot propose through an agent key they issued and ratify
/// through their own session (d17, the operator's decision of 2026-11-26).
#[tokio::test]
async fn an_admin_cannot_ratify_a_proposal_from_an_agent_key_they_issued() {
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    let agent = r
        .app
        .issue_key_with_reason(
            Bearer::Session(&alice),
            &IssueKeyRequest {
                role: Role::Reviewer,
                principal: "alice-agent".to_owned(),
                grants: vec![grant_on("crew", Role::Reviewer)],
                ..agent_req()
            },
            Some("an agent reviewer"),
        )
        .await
        .expect("a pending agent key");
    let bob = r.admin_session("bob").await;
    r.app
        .confirm_admin_act(
            Bearer::Session(&bob),
            agent.pending_act.expect("pending"),
            Some("confirmed"),
        )
        .await
        .expect("bob confirms");
    let proposal = r
        .app
        .propose_transition(
            agent.value.as_str(),
            by_name("crew"),
            oml_app::domain::WorkspaceStatus::Protected,
        )
        .await
        .expect("the agent proposes");
    let refused = r
        .app
        .ratify_transition(
            Bearer::Session(&alice),
            by_name("crew"),
            proposal.transition_id,
        )
        .await
        .expect_err("the issuing admin ratifies nothing");
    assert!(
        matches!(
            refused,
            AppError::RatifierSamePerson {
                issued_the_proposer: true
            }
        ),
        "{refused:?}"
    );
    r.app
        .ratify_transition(
            Bearer::Session(&bob),
            by_name("crew"),
            proposal.transition_id,
        )
        .await
        .expect("another admin person ratifies");
}

/// Under `sole_admin` a one-person tenant still moves the ladder: the
/// proposer's own person ratifies, and the transition is marked (d17).
#[tokio::test]
async fn a_sole_admin_ratification_by_the_proposers_own_person_is_admitted_and_marked() {
    let r = rig().await;
    r.admin_person("alice").await;
    let alice = r.linked_admin_key("alice").await;
    let proposal = r
        .app
        .propose_transition(
            alice.value.as_str(),
            by_name("crew"),
            oml_app::domain::WorkspaceStatus::Protected,
        )
        .await
        .expect("alice proposes");
    let again = r.admin_session("alice").await;
    r.app
        .ratify_transition(
            Bearer::Session(&again),
            by_name("crew"),
            proposal.transition_id,
        )
        .await
        .expect("a sole_admin ratification by the proposer's person");
    let events = r.journal().await;
    let ratified = events
        .iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::WORKSPACE_STATUS_TRANSITION)
        .expect("the transition");
    assert_eq!(ratified.envelope.payload["sole_admin"], true);
}

/// The admin read needs a person-linked admin key or a session: a key that
/// counts as no person opens no investigation. Each page it serves counts
/// once against the person whose own root it read, by month, and that
/// person reads the count; a read of the shared tree counts against no one
/// (ADR-0097 d16).
#[tokio::test]
async fn the_admin_read_needs_a_person_and_is_counted_per_person() {
    let r = rig().await;
    let refused = r
        .app
        .open_investigation(r.admin(), "ticket 1")
        .await
        .expect_err("an unlinked admin key opens no investigation");
    assert!(
        matches!(refused, AppError::AdminReadRequiresPerson),
        "{refused:?}"
    );
    r.admin_person("alice").await;
    let carol = r
        .app
        .allow_web_email(r.admin(), &entry_of("carol", false))
        .await
        .expect("carol's entry")
        .0;
    r.app.web_ensure_keys(&carol).await.expect("carol's keys");
    let carols = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                user: Some(name("carol")),
                grants: vec![
                    grant_on("carol", Role::Reader),
                    grant_on("carol", Role::Submitter),
                ],
                ..agent_req()
            },
        )
        .await
        .expect("a key in carol's root");
    let own = r
        .app
        .submit_memory(
            carols.value.as_str(),
            &oml_app::types::SubmitRequest {
                workspace: Some(by_name("carol")),
                ..submit_req("mine", oml_app::domain::MemoryKind::Memory)
            },
        )
        .await
        .expect("carol writes")
        .receipt
        .memory_id;
    let writer = r
        .app
        .issue_key(r.admin(), &agent_req())
        .await
        .expect("a writer");
    let shared = held(&r, &writer, "shared", oml_app::domain::MemoryKind::Memory).await;
    let alice = r.admin_session("alice").await;
    let investigation = r
        .app
        .open_investigation(Bearer::Session(&alice), "ticket 2")
        .await
        .expect("a person's session opens an investigation")
        .investigation_id;
    for id in [own, own, shared] {
        r.app
            .admin_read(Bearer::Session(&alice), id, Some(investigation))
            .await
            .expect("an admin read");
    }
    let session = r.person_session("carol", Capacity::Web).await;
    let (person, counts) = r
        .app
        .own_admin_reads(Bearer::Session(&session))
        .await
        .expect("carol reads her count");
    assert_eq!(person.as_str(), "carol");
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].reads, 2);
    let (alice_person, alice_counts) = r
        .app
        .own_admin_reads(Bearer::Session(&alice))
        .await
        .expect("alice reads hers");
    assert_eq!(alice_person.as_str(), "alice");
    assert!(
        alice_counts.is_empty(),
        "a shared read counts against no one"
    );
}

/// Under `sole_admin` an admin's key in another person's root takes effect
/// at once, marked, and that person is told in-product in the issuance's
/// own commit: a notice in their To do, which they dismiss (ADR-0097 d6,
/// d20).
#[tokio::test]
async fn a_reach_into_a_persons_root_under_sole_admin_takes_effect_at_once_and_notifies_the_person()
{
    let r = rig().await;
    r.admin_person("alice").await;
    let carol = r
        .app
        .allow_web_email(r.admin(), &entry_of("carol", false))
        .await
        .expect("carol's entry")
        .0;
    r.app.web_ensure_keys(&carol).await.expect("carol's keys");
    let alice = r.admin_session("alice").await;
    let before = r.journal().await.len();
    let issued = r
        .app
        .issue_key(
            Bearer::Session(&alice),
            &IssueKeyRequest {
                user: Some(name("carol")),
                grants: vec![
                    grant_on("carol", Role::Reader),
                    grant_on("carol", Role::Submitter),
                ],
                ..agent_req()
            },
        )
        .await
        .expect("a key in carol's root");
    assert_eq!(issued.pending_act, None, "at once under sole_admin");
    let added: Vec<String> = r.journal().await[before..]
        .iter()
        .map(|e| e.envelope.event_type.clone())
        .collect();
    assert_eq!(
        added.last().map(String::as_str),
        Some(event_types::NOTICE_WRITTEN)
    );
    let web = r.person_session("carol", Capacity::Web).await;
    let notices = r
        .app
        .notices(Bearer::Session(&web), false)
        .await
        .expect("carol's notices");
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind.as_str(), "root_reach");
    assert_eq!(
        r.app
            .todo_counts(Bearer::Session(&web), None)
            .await
            .expect("counts")
            .notices,
        1
    );
    assert!(
        r.app
            .notices(Bearer::Session(&alice), false)
            .await
            .expect("alice's notices")
            .is_empty(),
        "the actor is not told of their own reach"
    );
    r.app
        .dismiss_notice(Bearer::Session(&web), notices[0].notice_id)
        .await
        .expect("carol dismisses it");
    assert_eq!(
        r.app
            .todo_counts(Bearer::Session(&web), None)
            .await
            .expect("counts")
            .notices,
        0
    );
    let again = r
        .app
        .dismiss_notice(Bearer::Session(&web), notices[0].notice_id)
        .await
        .expect_err("a dismissed notice is gone");
    assert!(matches!(again, AppError::NotFound), "{again:?}");
}

/// Offensive narrowing is loud: denying a reviewer person's key writes, in
/// the denial's own commit, a notice to every admin person and to every
/// reviewer person of the workspace concerned (ADR-0097 d20, d24).
#[tokio::test]
async fn offensive_narrowing_notifies_every_admin_person_and_the_workspaces_reviewer_persons() {
    let r = rig().await;
    r.admin_person("alice").await;
    let rhea = r
        .app
        .issue_key(r.admin(), &person_reviewer_req("rhea"))
        .await
        .expect("rhea's reviewer key");
    let alice = r.admin_session("alice").await;
    let rhea_entry = r
        .app
        .allow_web_email(r.admin(), &entry_of("rhea", false))
        .await
        .expect("rhea's entry")
        .0;
    r.app.web_ensure_keys(&rhea_entry).await.expect("keys");
    r.app
        .link_key_to_person(
            Bearer::Session(&alice),
            rhea.key_id.into(),
            &name("rhea"),
            Some("the person holds this key"),
        )
        .await
        .expect("linked");
    let rhea_web = r.person_session("rhea", Capacity::Web).await;
    r.app
        .accept_key_link(Bearer::Session(&rhea_web), rhea.key_id.into())
        .await
        .expect("accepted");
    let other = r
        .app
        .issue_key(r.admin(), &person_reviewer_req("other"))
        .await
        .expect("another reviewer key");
    r.app
        .deny_key(
            Bearer::Session(&alice),
            other.key_id.into(),
            by_name("crew"),
        )
        .await
        .expect("the denial takes effect at once");
    let to_rhea = r
        .app
        .notices(Bearer::Session(&rhea_web), false)
        .await
        .expect("rhea's notices");
    assert_eq!(to_rhea.len(), 1, "a reviewer person of crew is told");
    assert_eq!(to_rhea[0].kind.as_str(), "offensive_narrowing");
    let to_alice = r
        .app
        .notices(Bearer::Session(&alice), false)
        .await
        .expect("alice's notices");
    assert_eq!(
        to_alice.len(),
        1,
        "every admin person is told, the actor too"
    );
}

fn boxed_req(hours: u64, r: &Rig) -> IssueKeyRequest {
    let now = r.app.now().unix_micros();
    let until = oml_app::domain::EventTime::from_unix_micros(
        now + i64::try_from(hours * 3_600_000_000).expect("micros"),
    )
    .expect("an instant");
    IssueKeyRequest {
        expiry: Some(until),
        ..agent_req()
    }
}

/// A narrow time box: in a tenant with two admin persons, an admin person
/// issues a reader and submitter key on a shared workspace at once, for at
/// most a day; it is refused past a day, into a user root, and while a box
/// of theirs is open there; one renewal is admitted, a third box waits
/// until a second admin person settles the first; a decline revokes the
/// key (ADR-0097 d13).
#[tokio::test]
async fn a_time_box_admits_reader_and_submitter_grants_in_shared_workspaces_for_at_most_a_day() {
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    let bob = r.admin_session("bob").await;
    let too_long = r
        .app
        .issue_timeboxed_key(
            Bearer::Session(&alice),
            &boxed_req(25, &r),
            Some("incident 4"),
        )
        .await
        .expect_err("a box lasts at most a day");
    assert!(
        matches!(
            too_long,
            AppError::Timebox(oml_app::types::TimeboxRefusal::Until)
        ),
        "{too_long:?}"
    );
    let first = r
        .app
        .issue_timeboxed_key(
            Bearer::Session(&alice),
            &boxed_req(2, &r),
            Some("incident 4"),
        )
        .await
        .expect("a box takes effect at once");
    assert_eq!(first.pending_act, None);
    r.app
        .whoami(&first.value)
        .await
        .expect("the boxed key authenticates at once");
    let open = r
        .app
        .issue_timeboxed_key(
            Bearer::Session(&alice),
            &boxed_req(2, &r),
            Some("incident 4"),
        )
        .await
        .expect_err("one open box per person per workspace");
    assert!(
        matches!(
            open,
            AppError::Timebox(oml_app::types::TimeboxRefusal::Open)
        ),
        "{open:?}"
    );
    r.clock.advance(3 * 3_600);
    let err = r.app.whoami(&first.value).await.expect_err("the box ended");
    assert!(matches!(err, AppError::KeyExpired { .. }), "{err:?}");
    let renewal = r
        .app
        .issue_timeboxed_key(
            Bearer::Session(&alice),
            &boxed_req(2, &r),
            Some("incident 4"),
        )
        .await
        .expect("one renewal");
    r.clock.advance(3 * 3_600);
    let third = r
        .app
        .issue_timeboxed_key(
            Bearer::Session(&alice),
            &boxed_req(2, &r),
            Some("incident 4"),
        )
        .await
        .expect_err("a third box waits on a settlement");
    assert!(
        matches!(
            third,
            AppError::Timebox(oml_app::types::TimeboxRefusal::RenewalsExhausted)
        ),
        "{third:?}"
    );
    let first_act = r
        .store
        .admin_acts(r.boot.tenant_id, Some(AdminActState::Effective))
        .await
        .expect("acts")
        .into_iter()
        .find(|a| a.timebox() && a.target["key_id"] == first.key_id.to_string())
        .expect("the first box");
    let settled = r
        .app
        .confirm_admin_act(Bearer::Session(&bob), first_act.act_id, Some("confirmed"))
        .await
        .expect("bob settles the first box");
    assert_eq!(settled.state, AdminActState::Confirmed);
    let after = r
        .app
        .issue_timeboxed_key(
            Bearer::Session(&alice),
            &boxed_req(2, &r),
            Some("incident 4"),
        )
        .await
        .expect("a box again once the first is settled");
    let renewal_act = r
        .store
        .admin_acts(r.boot.tenant_id, Some(AdminActState::Effective))
        .await
        .expect("acts")
        .into_iter()
        .find(|a| a.timebox() && a.target["key_id"] == after.key_id.to_string())
        .expect("the latest box");
    r.app
        .decline_admin_act(
            Bearer::Session(&bob),
            renewal_act.act_id,
            Some("not needed"),
        )
        .await
        .expect("bob declines it");
    let err = r
        .app
        .whoami(&after.value)
        .await
        .expect_err("declined, revoked");
    assert!(matches!(err, AppError::Unauthorized), "{err:?}");
    let _ = renewal;
    let root = r
        .app
        .issue_timeboxed_key(
            Bearer::Session(&alice),
            &IssueKeyRequest {
                grants: vec![grant_on("alice", Role::Reader)],
                ..boxed_req(2, &r)
            },
            Some("incident 5"),
        )
        .await
        .expect_err("never into a user root");
    assert!(
        matches!(
            root,
            AppError::Timebox(
                oml_app::types::TimeboxRefusal::NotShared | oml_app::types::TimeboxRefusal::Shape
            )
        ),
        "{root:?}"
    );
}

/// No time box while an act against an admin is open (ADR-0097 d13).
#[tokio::test]
async fn a_time_box_is_refused_while_an_act_against_an_admin_is_open() {
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    r.app
        .allow_web_email_with_reason(
            Bearer::Session(&alice),
            &entry_of("bob", false),
            true,
            Some("bob left"),
        )
        .await
        .expect("alice un-flags bob, an act against an admin");
    let refused = r
        .app
        .issue_timeboxed_key(
            Bearer::Session(&alice),
            &boxed_req(2, &r),
            Some("incident 6"),
        )
        .await
        .expect_err("refused while the act is open");
    assert!(
        matches!(
            refused,
            AppError::Timebox(oml_app::types::TimeboxRefusal::AgainstAdminOpen)
        ),
        "{refused:?}"
    );
}

/// A link takes effect when the person it names accepts it, and the
/// acceptance records the session's age and the provider subjects its entry pinned, as digests, so
/// a forged acceptance can be told apart afterwards (ADR-0097 d7).
#[tokio::test]
async fn an_acceptance_records_the_provider_subject_and_the_sessions_age() {
    let r = rig().await;
    r.admin_person("alice").await;
    let web = r.person_session("alice", Capacity::Web).await;
    r.clock.advance(90);
    let issued = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                role: Role::Admin,
                grants: Vec::new(),
                used_by: Some(UsedBy::Person),
                person: Some(name("alice")),
                ..agent_req()
            },
        )
        .await
        .expect("an admin key");
    r.app
        .accept_key_link(Bearer::Session(&web), issued.key_id.into())
        .await
        .expect("alice accepts");
    let accepted = r
        .journal()
        .await
        .into_iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::KEY_LINK_ACCEPTED)
        .expect("the acceptance");
    let evidence = &accepted.envelope.payload["evidence"];
    assert_eq!(evidence["session_age_seconds"], 90);
    assert!(evidence["provider_subject_digests"].is_array());
}

// -- erase: the demotion at once, the shred after a second admin person
// (ADR-0097 d4; amendment of 2026-11-27, answer 5) ---------------------------

/// A writer and a reviewer agent key on `crew`, issued while the tenant is
/// still `sole_admin`, so both take effect at once.
async fn crew_pair(r: &Rig) -> (IssuedKey, IssuedKey) {
    let writer = r
        .app
        .issue_key(r.admin(), &agent_req())
        .await
        .expect("a writer");
    let reviewer = r
        .app
        .issue_key(
            r.admin(),
            &IssueKeyRequest {
                role: Role::Reviewer,
                principal: "rex".to_owned(),
                grants: vec![
                    GrantRequest {
                        node: by_name("crew"),
                        role: Role::Reviewer,
                        deny: Vec::new(),
                        covers: Covers::Subtree,
                        create: false,
                    },
                    GrantRequest {
                        node: by_name("crew"),
                        role: Role::Reader,
                        deny: Vec::new(),
                        covers: Covers::Subtree,
                        create: false,
                    },
                ],
                ..agent_req()
            },
        )
        .await
        .expect("a reviewer");
    (writer, reviewer)
}

fn claim_req(idem: &str, body: &str) -> oml_app::types::SubmitRequest {
    oml_app::types::SubmitRequest {
        on_similar: Default::default(),
        anchor: None,
        origin: None,
        idempotency_key: idem.to_owned(),
        claim_key: Some("vault-token".to_owned()),
        verb: None,
        kind: oml_app::domain::MemoryKind::Memory,
        body: body.to_owned(),
        workspace: Some(by_name("crew")),
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

async fn submitted(r: &Rig, writer: &IssuedKey, idem: &str, body: &str) -> Uuid {
    r.app
        .submit_memory(writer.value.as_str(), &claim_req(idem, body))
        .await
        .expect("submit")
        .receipt
        .memory_id
}

/// A successor of `target`, version `version`, by the contributor key.
async fn superseded(
    r: &Rig,
    writer: &IssuedKey,
    idem: &str,
    target: Uuid,
    version: u32,
    body: &str,
) -> Uuid {
    r.app
        .supersede(
            writer.value.as_str(),
            &oml_app::types::SupersedeRequest {
                on_similar: Default::default(),
                anchor: None,
                origin: None,
                expected_version: version - 1,
                idempotency_key: idem.to_owned(),
                target_id: target,
                body: body.to_owned(),
                kind: None,
                retirement: false,
                derives_from: Vec::new(),
                reason: None,
                workspace: None,
                refuse_vocabulary_target: false,
                severity: None,
                rule: None,
                kind_fields: None,
                valid_from: None,
                valid_until: None,
                format: None,
                title: None,
            },
        )
        .await
        .expect("supersede")
        .receipt
        .memory_id
}

async fn approved(r: &Rig, reviewer: &IssuedKey, id: Uuid) {
    r.app
        .post_verdict(
            reviewer.value.as_str(),
            id,
            oml_app::types::Verdict::Approve,
            None,
        )
        .await
        .expect("approve");
}

/// Two promoted versions of one claim, v1 and its successor v2, the tip;
/// then alice and bob, admin persons accepted eight days ago.
async fn erase_rig() -> (Rig, IssuedKey, Uuid, Uuid) {
    let r = rig().await;
    let (writer, reviewer) = crew_pair(&r).await;
    let v1 = submitted(&r, &writer, "e1", "the vault token is abc").await;
    approved(&r, &reviewer, v1).await;
    let v2 = superseded(&r, &writer, "e2", v1, 2, "the vault token is abcd").await;
    approved(&r, &reviewer, v2).await;
    r.admin_person("alice").await;
    r.admin_person("bob").await;
    r.clock.advance(8 * DAY);
    (r, writer, v1, v2)
}

fn erase(target: Uuid) -> oml_app::app::FixRequest {
    oml_app::app::FixRequest {
        act: oml_app::types::FixAct::Erase,
        target,
        reason: "ticket 9: a secret was written".to_owned(),
        body: None,
        title: None,
    }
}

async fn status_and_body(r: &Rig, id: Uuid) -> (oml_app::domain::MemoryStatus, String) {
    let m = r
        .store
        .memory(r.boot.tenant_id, id)
        .await
        .expect("a read")
        .expect("held");
    (m.status, m.body)
}

/// With two admin persons an erase demotes every promoted version of the
/// claim at once, as a fix, and keeps the bytes; the shred waits, unexpired
/// past a week, until a second admin person confirms it, and the actor's
/// own person cannot.
#[tokio::test]
async fn erase_demotes_at_once_and_shreds_only_after_a_second_admin_person() {
    use oml_app::domain::MemoryStatus;
    let (r, _writer, v1, v2) = erase_rig().await;
    let alice = r.admin_session("alice").await;
    let outcome = r
        .app
        .admin_fix(Bearer::Session(&alice), &erase(v2))
        .await
        .expect("the erase");
    let fix = outcome.fix().expect("a promoted target is a fix").clone();
    assert_eq!(fix.act, oml_app::types::FixAct::Erase);
    for id in [v1, v2] {
        let (status, body) = status_and_body(&r, id).await;
        assert_eq!(status, MemoryStatus::Demoted, "demoted at once");
        assert!(!body.is_empty(), "the bytes are kept");
    }
    let act = last_act(&r, "erase").await;
    assert_eq!(act.act_id, outcome.act().act_id);
    assert_eq!(act.state, AdminActState::PendingConfirmation);
    assert!(!act.sole_admin);
    assert_eq!(act.expires_at(), None, "a pending shred never expires");
    r.clock.advance(8 * DAY);
    assert!(matches!(
        r.app
            .confirm_admin_act(
                Bearer::Session(&r.admin_session("alice").await),
                act.act_id,
                Some("confirmed")
            )
            .await
            .unwrap_err(),
        AppError::AdminAct(AdminActRefusal::OwnAct)
    ));
    let before = r.journal().await.len();
    let confirmed = r
        .app
        .confirm_admin_act(
            Bearer::Session(&r.admin_session("bob").await),
            act.act_id,
            Some("confirmed"),
        )
        .await
        .expect("bob confirms the shred");
    assert_eq!(confirmed.state, AdminActState::Confirmed);
    let events = r.journal().await;
    let added: Vec<&str> = events[before..]
        .iter()
        .map(|e| e.envelope.event_type.as_str())
        .collect();
    assert_eq!(
        added,
        [event_types::ADMIN_ACT, event_types::LIFECYCLE_ERASE]
    );
    assert_eq!(
        events[before + 1].envelope.payload["act_id"],
        act.act_id.to_string()
    );
    for id in [v1, v2] {
        let (status, body) = status_and_body(&r, id).await;
        assert_eq!(status, MemoryStatus::Erased);
        assert!(body.is_empty(), "shredded");
    }
}

/// A declined shred leaves the claim demoted and its bytes kept.
#[tokio::test]
async fn a_declined_shred_leaves_the_claim_demoted_and_its_bytes_kept() {
    use oml_app::domain::MemoryStatus;
    let (r, _writer, _v1, v2) = erase_rig().await;
    let alice = r.admin_session("alice").await;
    let outcome = r
        .app
        .admin_fix(Bearer::Session(&alice), &erase(v2))
        .await
        .expect("the erase");
    r.app
        .decline_admin_act(
            Bearer::Session(&r.admin_session("bob").await),
            outcome.act().act_id,
            Some("the text is no secret"),
        )
        .await
        .expect("bob declines");
    let (status, body) = status_and_body(&r, v2).await;
    assert_eq!(status, MemoryStatus::Demoted);
    assert_eq!(body, "the vault token is abcd");
}

/// An erase whose target is not a promoted version has nothing to demote:
/// it records the pending shred act alone, with no fix record, and the
/// claim's served tip stays.
#[tokio::test]
async fn an_erase_of_a_version_that_is_not_promoted_records_only_the_pending_shred_act() {
    use oml_app::domain::MemoryStatus;
    let (r, writer, _v1, v2) = erase_rig().await;
    let held = superseded(&r, &writer, "e3", v2, 3, "the vault token is abcde").await;
    let alice = r.admin_session("alice").await;
    let before = r.journal().await.len();
    let outcome = r
        .app
        .admin_fix(Bearer::Session(&alice), &erase(held))
        .await
        .expect("the erase");
    assert!(outcome.fix().is_none(), "no fix record");
    assert_eq!(outcome.act().state, AdminActState::PendingConfirmation);
    let added: Vec<String> = r.journal().await[before..]
        .iter()
        .map(|e| e.envelope.event_type.clone())
        .collect();
    assert_eq!(added, [event_types::ADMIN_ACT]);
    assert!(
        r.store
            .fixes(r.boot.tenant_id, false)
            .await
            .expect("fixes")
            .is_empty()
    );
    assert_eq!(status_and_body(&r, v2).await.0, MemoryStatus::Promoted);
    assert_eq!(status_and_body(&r, held).await.0, MemoryStatus::Held);
}

/// In a `sole_admin` tenant the shred runs at once, and the act is marked.
#[tokio::test]
async fn under_sole_admin_an_erase_shreds_at_once_marked() {
    use oml_app::domain::MemoryStatus;
    let r = rig().await;
    let (writer, reviewer) = crew_pair(&r).await;
    let v1 = submitted(&r, &writer, "e1", "the vault token is abc").await;
    approved(&r, &reviewer, v1).await;
    r.admin_person("alice").await;
    let alice = r.admin_session("alice").await;
    let outcome = r
        .app
        .admin_fix(Bearer::Session(&alice), &erase(v1))
        .await
        .expect("the erase");
    assert!(outcome.fix().is_some());
    assert_eq!(outcome.act().state, AdminActState::Effective);
    assert!(outcome.act().sole_admin);
    let (status, body) = status_and_body(&r, v1).await;
    assert_eq!(status, MemoryStatus::Erased);
    assert!(body.is_empty());
}

// -- federation acts in the envelope (ADR-0097 d3, d5) ---------------------

/// An admin's federation acts ride the admin act: declining federation on a
/// workspace is narrowing and takes effect at once, accepting it again is
/// widening and waits, applying only at a second admin person's
/// confirmation.
#[tokio::test]
async fn an_admins_federation_acceptance_waits_on_a_second_admin_person_and_applies_at_confirmation()
 {
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    let declined = r
        .app
        .set_workspace_federation_with_reason(
            Bearer::Session(&alice),
            by_name("crew"),
            false,
            Some("the crew holds drafts"),
        )
        .await
        .expect("a decline at once");
    assert!(!declined.now.accept_federated);
    assert!(declined.pending.is_none());
    let narrowing = last_act(&r, "decline_federation").await;
    assert_eq!(narrowing.state, AdminActState::Effective);
    let events = r.journal().await;
    let set = events
        .iter()
        .rev()
        .find(|e| e.envelope.event_type == event_types::WORKSPACE_FEDERATION_SET)
        .expect("the effect");
    assert_eq!(set.envelope.payload["act_id"], narrowing.act_id.to_string());
    let pending = r
        .app
        .set_workspace_federation_with_reason(
            Bearer::Session(&alice),
            by_name("crew"),
            true,
            Some("the drafts are gone"),
        )
        .await
        .expect("a pending acceptance");
    assert!(!pending.now.accept_federated, "nothing took effect");
    let act = pending.pending.expect("the act waits");
    assert_eq!(act.act.as_str(), "accept_federation");
    assert_eq!(act.state, AdminActState::PendingConfirmation);
    r.app
        .confirm_admin_act(
            Bearer::Session(&r.admin_session("bob").await),
            act.act_id,
            Some("confirmed"),
        )
        .await
        .expect("bob confirms");
    let crew = r
        .store
        .resolve_name(r.boot.tenant_id, NamedKind::Workspace, &name("crew"))
        .await
        .expect("a read")
        .expect("crew");
    assert!(
        r.store
            .federation_declining(r.boot.tenant_id, &[crew])
            .await
            .expect("a read")
            .is_empty(),
        "accepted at the confirmation"
    );
}

/// Widening the tenant's federation policy waits on a second admin person
/// and applies its change at the confirmation; narrowing takes effect at
/// once; a change that does both is refused, as two acts of two classes.
#[tokio::test]
async fn widening_the_federation_policy_waits_and_narrowing_it_takes_effect_at_once() {
    use oml_app::federation::FederationPolicyChange;
    let r = two_admins().await;
    let alice = r.admin_session("alice").await;
    let widen = FederationPolicyChange {
        enabled: Some(true),
        default_sources: None,
        max_sources: Some(8),
    };
    let taken = r
        .app
        .set_federation_policy_with_reason(Bearer::Session(&alice), widen, Some("wider recall"))
        .await
        .expect("a pending widening");
    assert!(!taken.now.0.policy.enabled, "nothing took effect");
    let act = taken.pending.expect("the act waits");
    assert_eq!(act.act.as_str(), "widen_federation_policy");
    r.app
        .confirm_admin_act(
            Bearer::Session(&r.admin_session("bob").await),
            act.act_id,
            Some("confirmed"),
        )
        .await
        .expect("bob confirms");
    let policy = r
        .store
        .federation_policy(r.boot.tenant_id)
        .await
        .expect("the policy");
    assert!(policy.enabled);
    assert_eq!(policy.max_sources, 8);
    let narrowed = r
        .app
        .set_federation_policy_with_reason(
            Bearer::Session(&alice),
            FederationPolicyChange {
                enabled: None,
                default_sources: None,
                max_sources: Some(4),
            },
            None,
        )
        .await
        .expect("a narrowing at once");
    assert!(narrowed.pending.is_none());
    assert_eq!(narrowed.now.0.policy.max_sources, 4);
    assert_eq!(
        last_act(&r, "narrow_federation_policy").await.state,
        AdminActState::Effective
    );
    let mixed = r
        .app
        .set_federation_policy_with_reason(
            Bearer::Session(&alice),
            FederationPolicyChange {
                enabled: Some(false),
                default_sources: None,
                max_sources: Some(16),
            },
            Some("both at once"),
        )
        .await
        .unwrap_err();
    assert!(matches!(mixed, AppError::AdminField { .. }), "{mixed:?}");
}

/// In a `sole_admin` tenant a federation widening takes effect at once,
/// marked, and its decline sets the field back.
#[tokio::test]
async fn a_declined_sole_admin_federation_acceptance_is_undone() {
    let r = rig().await;
    r.admin_person("alice").await;
    let alice = r.admin_session("alice").await;
    r.app
        .set_workspace_federation_with_reason(Bearer::Session(&alice), by_name("crew"), false, None)
        .await
        .expect("declined at once");
    let taken = r
        .app
        .set_workspace_federation_with_reason(Bearer::Session(&alice), by_name("crew"), true, None)
        .await
        .expect("accepted at once");
    assert!(taken.now.accept_federated);
    let act = last_act(&r, "accept_federation").await;
    assert!(act.sole_admin);
    r.app
        .decline_admin_act(Bearer::Session(&alice), act.act_id, Some("too wide"))
        .await
        .expect("declined");
    let crew = r
        .store
        .resolve_name(r.boot.tenant_id, NamedKind::Workspace, &name("crew"))
        .await
        .expect("a read")
        .expect("crew");
    assert_eq!(
        r.store
            .federation_declining(r.boot.tenant_id, &[crew])
            .await
            .expect("a read"),
        vec![crew],
        "the decline set the field back"
    );
}
