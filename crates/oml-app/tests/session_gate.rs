//! The session gate's reads (ADR-0070 d2, d8; #297), over the in-memory
//! doubles: a capacity credential resolves to its session key's caller on
//! rows read in one store call, and a request without a body decides its
//! first resolution of the caller on the access read there, where every
//! later resolution, and every resolution of a request with a body, reads
//! the access when it runs. Each test is named after the sentence it
//! proves.

use oml_app::App;
use oml_app::domain::name::{Name, Reference};
use oml_app::domain::web::{Email, capacity_credential};
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::types::{AppError, Bearer, BootstrapRequest, WebEmailRequest};

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

/// A tenant, its admin key's value and the session token of `alice`, an
/// admin entry whose session lives an hour.
struct Rig {
    app: TestApp,
    clock: FixedClock,
    admin: String,
    token: String,
}

async fn rig() -> Rig {
    let clock = FixedClock::new();
    let app = App::new(
        MemStore::new(),
        MemSigner::new([3; 32]),
        clock.clone(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    let boot = app
        .bootstrap_tenant(&BootstrapRequest {
            tenant_name: Name::parse("acme").expect("a name"),
            admin_key_name: Name::parse("root-admin").expect("a name"),
            admin_principal: "root".to_owned(),
        })
        .await
        .expect("bootstrap");
    let (entry, _) = app
        .allow_web_email(
            boot.admin_key.value.as_str(),
            &WebEmailRequest {
                email: Email::parse("alice@example.com").expect("an email"),
                user: Name::parse("alice").expect("a user"),
                admin: true,
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
    Rig {
        app,
        clock,
        admin: boot.admin_key.value,
        token,
    }
}

impl Rig {
    /// The tenant's admin cuts `alice-web` off on `universe`: a change of
    /// the key's access after the gate read it.
    async fn cut_off(&self) {
        self.app
            .deny_key(
                self.admin.as_str(),
                Reference::parse("alice-web").expect("a reference"),
                Reference::parse("universe").expect("a reference"),
            )
            .await
            .expect("the denial is recorded");
    }
}

#[tokio::test]
async fn a_request_without_a_body_decides_its_first_resolution_on_the_access_the_gate_read_and_every_later_one_on_the_access_read_then()
 {
    let rig = rig().await;
    let credential = capacity_credential(&rig.token, "web");
    let (gate, _) = rig
        .app
        .web_credential_caller(&credential, true)
        .await
        .expect("the credential resolves");
    assert_eq!(
        gate.key().name.as_ref().map(Name::as_str),
        Some("alice-web")
    );
    assert!(gate.denials().is_empty());
    rig.cut_off().await;

    let first = rig
        .app
        .caller(Bearer::Session(&gate))
        .await
        .expect("the first resolution");
    assert!(
        first.denials().is_empty(),
        "the first resolution decides on the gate's read"
    );
    assert_eq!(first.grants(), gate.grants());
    assert_eq!(first.permission_epoch(), gate.permission_epoch());

    let later = rig
        .app
        .caller(Bearer::Session(&gate))
        .await
        .expect("a later resolution");
    assert_eq!(
        later.denials().len(),
        1,
        "a later resolution reads the access again"
    );
    // The caller the first resolution made holds no read of its own: an
    // act that resolves it again, as after a claim's wait, reads now.
    let again = rig
        .app
        .caller(Bearer::Session(&first))
        .await
        .expect("a resolution of the resolved caller");
    assert_eq!(again.denials().len(), 1);
}

#[tokio::test]
async fn a_request_with_a_body_reads_its_access_when_its_verb_runs() {
    let rig = rig().await;
    let credential = capacity_credential(&rig.token, "web");
    let (gate, _) = rig
        .app
        .web_credential_caller(&credential, false)
        .await
        .expect("the credential resolves");
    rig.cut_off().await;
    let first = rig
        .app
        .caller(Bearer::Session(&gate))
        .await
        .expect("the first resolution");
    assert_eq!(
        first.denials().len(),
        1,
        "the verb reads the access when it runs"
    );
}

#[tokio::test]
async fn whoami_serves_a_session_key_the_grants_the_gate_read_once_and_the_grants_read_then_after()
{
    let rig = rig().await;
    let credential = capacity_credential(&rig.token, "web");
    let (gate, _) = rig
        .app
        .web_credential_caller(&credential, true)
        .await
        .expect("the credential resolves");
    let (first, _) = rig
        .app
        .whoami_served(Bearer::Session(&gate), oml_app::app::PersonAsk::Omit)
        .await
        .expect("whoami");
    let (later, _) = rig
        .app
        .whoami_served(Bearer::Session(&gate), oml_app::app::PersonAsk::Omit)
        .await
        .expect("whoami again");
    assert_eq!(first.key.name.as_ref().map(Name::as_str), Some("alice-web"));
    assert_eq!(first.grants, later.grants);
    assert_eq!(
        first
            .grants
            .iter()
            .map(|g| g.grant.clone())
            .collect::<Vec<_>>(),
        gate.grants()
    );
}

#[tokio::test]
async fn each_capacity_of_a_session_resolves_to_its_own_session_key_as_the_separate_reads_resolve_it()
 {
    let rig = rig().await;
    for capacity in ["web", "review", "admin"] {
        let credential = capacity_credential(&rig.token, capacity);
        let (caller, session) = rig
            .app
            .web_credential_caller(&credential, true)
            .await
            .expect("the credential resolves");
        assert_eq!(
            caller.key().name.as_ref().map(Name::as_str),
            Some(format!("alice-{capacity}").as_str())
        );
        assert_eq!(session.user.as_str(), "alice");
        // The gate's caller and the caller the separate reads make agree.
        let apart = rig
            .app
            .web_caller(
                &session,
                oml_app::app::Capacity::parse(capacity).expect("a capacity"),
            )
            .await
            .expect("the separate reads resolve");
        assert_eq!(caller.key(), apart.key());
        assert_eq!(caller.grants(), apart.grants());
        assert_eq!(caller.denials(), apart.denials());
        assert_eq!(caller.permission_epoch(), apart.permission_epoch());
    }
}

#[tokio::test]
async fn a_credential_no_session_holds_one_past_its_sessions_expiry_and_a_capacity_the_session_never_reached_are_each_unauthorized()
 {
    let rig = rig().await;
    for first_resolution in [true, false] {
        let unknown = rig
            .app
            .web_credential_caller("omls_not-a-credential", first_resolution)
            .await;
        assert!(
            matches!(unknown, Err(AppError::Unauthorized)),
            "{unknown:?}"
        );
        // No curator key was designated, so the session holds no `curate`
        // credential.
        let curate = rig
            .app
            .web_credential_caller(&capacity_credential(&rig.token, "curate"), first_resolution)
            .await;
        assert!(matches!(curate, Err(AppError::Unauthorized)), "{curate:?}");
    }
    // Past its hour the session is ended at the gate, and stays ended.
    rig.clock.advance(3601);
    for _ in 0..2 {
        let expired = rig
            .app
            .web_credential_caller(&capacity_credential(&rig.token, "web"), true)
            .await;
        assert!(
            matches!(expired, Err(AppError::Unauthorized)),
            "{expired:?}"
        );
    }
    assert!(
        rig.app
            .web_session(&rig.token)
            .await
            .expect("a read")
            .is_none(),
        "the expired session's row is gone"
    );
}
