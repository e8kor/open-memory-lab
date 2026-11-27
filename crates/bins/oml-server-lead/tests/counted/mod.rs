//! A fixture reviewer key that counts as a person: an instruction verdict
//! and a ladder ratification need one (ADR-0097 d12, d17), and a tenant key
//! counts as a person only through an accepted link (ADR-0085 d16). The
//! link is made by an application over the router's own store, from the
//! console sessions of the tenant's admin person and of the person.

#![allow(dead_code, reason = "each suite uses some of these")]

use std::cell::RefCell;

use oml_app::App;
use oml_app::app::Capacity;
use oml_app::domain::name::Name;
use oml_app::domain::web::Email;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::ports::{Clock, ConnectionStore, IdGen, Scanner, Signer, WebStore};
use oml_app::types::{Bearer, WebEmailRequest};
use uuid::Uuid;

thread_local! {
    /// The store the test's doubles router serves, one per test thread.
    static STORE: RefCell<Option<(MemStore, [u8; 32])>> = const { RefCell::new(None) };
}

/// Keeps `store`, which a doubles router over `seed` serves, for
/// [`link_in_router`] on this test's thread.
pub fn remember(store: &MemStore, seed: [u8; 32]) {
    STORE.with(|s| *s.borrow_mut() = Some((store.clone(), seed)));
}

/// [`link`] over the store [`remember`] kept.
pub async fn link_in_router(admin: &str, key_id: &str, person: &str) {
    let (store, seed) = STORE
        .with(|s| s.borrow().clone())
        .expect("a doubles router remembered its store");
    let app = App::new(
        store,
        MemSigner::new(seed),
        FixedClock::new(),
        SeqIdGen::new(),
        MarkerScanner::default(),
    );
    link(
        &app,
        admin,
        Uuid::parse_str(key_id).expect("a key id"),
        person,
    )
    .await;
}

/// `key_id` linked to `person`, a new person of the tenant, by the tenant's
/// admin person from their console session, and accepted from `person`'s.
pub async fn link<J, S, C, I, Sc>(
    app: &App<J, S, C, I, Sc>,
    admin: &str,
    key_id: Uuid,
    person: &str,
) where
    J: WebStore + ConnectionStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    let entries = app
        .list_web_emails(admin, None, 500)
        .await
        .expect("entries");
    let ops = entries
        .iter()
        .find(|e| e.admin && !e.pending)
        .cloned()
        .expect("the tenant's admin person");
    app.web_ensure_keys(&ops).await.expect("session keys");
    let user = Name::parse(person).expect("a person's name");
    let entry = match entries.into_iter().find(|e| e.user == user) {
        Some(entry) => entry,
        None => {
            app.allow_web_email(
                admin,
                &WebEmailRequest {
                    email: Email::parse(&format!("{person}@example.com")).expect("an email"),
                    user: user.clone(),
                    admin: false,
                },
            )
            .await
            .expect("the person's entry")
            .0
        }
    };
    app.web_ensure_keys(&entry).await.expect("session keys");
    let session = |entry: oml_app::types::WebEntry, capacity: Capacity| async move {
        let token = app
            .web_session_open(&entry, 3600, None)
            .await
            .expect("a session")
            .token;
        let session = app
            .web_session(&token)
            .await
            .expect("a read")
            .expect("a live session");
        app.web_caller(&session, capacity)
            .await
            .expect("the session key")
    };
    let by = session(ops, Capacity::Admin).await;
    app.link_key_to_person(
        Bearer::Session(&by),
        key_id.into(),
        &user,
        Some("the person holds this key"),
    )
    .await
    .expect("the key is linked");
    let web = session(entry, Capacity::Web).await;
    app.accept_key_link(Bearer::Session(&web), key_id.into())
        .await
        .expect("the person accepts the link");
}

/// [`link`] for the key whose value is `value`.
pub async fn link_value<J, S, C, I, Sc>(
    app: &App<J, S, C, I, Sc>,
    admin: &str,
    value: &str,
    person: &str,
) where
    J: WebStore + ConnectionStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    let key_id = app.whoami(value).await.expect("the key").key.key_id;
    link(app, admin, key_id, person).await;
}

/// [`link`] for the key whose value is `value`, acting as the bootstrap's
/// founder (`founder@example.com`) through their `<founder>-admin` session,
/// for a suite that keeps no admin key value at hand.
pub async fn link_by_founder<J, S, C, I, Sc, W>(
    app: &App<J, S, C, I, Sc>,
    store: &W,
    value: &str,
    person: &str,
) where
    J: WebStore + ConnectionStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
    W: WebStore,
{
    let key_id = app.whoami(value).await.expect("the key").key.key_id;
    let founder = store
        .web_entry_by_email(
            &Email::parse("founder@example.com")
                .expect("an email")
                .digest(),
        )
        .await
        .expect("a read")
        .expect("the founder's entry");
    app.web_ensure_keys(&founder).await.expect("session keys");
    let token = app
        .web_session_open(&founder, 3600, None)
        .await
        .expect("a session")
        .token;
    let session = app
        .web_session(&token)
        .await
        .expect("a read")
        .expect("a live session");
    let admin = app
        .web_caller(&session, Capacity::Admin)
        .await
        .expect("the founder's admin session");
    let user = Name::parse(person).expect("a person's name");
    let (entry, _) = app
        .allow_web_email(
            Bearer::Session(&admin),
            &WebEmailRequest {
                email: Email::parse(&format!("{person}@example.com")).expect("an email"),
                user: user.clone(),
                admin: false,
            },
        )
        .await
        .expect("the person's entry");
    app.web_ensure_keys(&entry).await.expect("session keys");
    app.link_key_to_person(
        Bearer::Session(&admin),
        key_id.into(),
        &user,
        Some("the person holds this key"),
    )
    .await
    .expect("the key is linked");
    let token = app
        .web_session_open(&entry, 3600, None)
        .await
        .expect("a session")
        .token;
    let session = app
        .web_session(&token)
        .await
        .expect("a read")
        .expect("a live session");
    let web = app
        .web_caller(&session, Capacity::Web)
        .await
        .expect("the person's console");
    app.accept_key_link(Bearer::Session(&web), key_id.into())
        .await
        .expect("the person accepts the link");
}
