//! A fixture reviewer key that counts as a person: an instruction verdict
//! and a ladder ratification need one (ADR-0097 d12, d17). A tenant key
//! counts as a person only through an accepted link (ADR-0085 d16).

// Each suite uses some of these.
#![allow(dead_code)]

use oml_app::App;
use oml_app::app::{Caller, Capacity};
use oml_app::domain::name::Name;
use oml_app::domain::web::Email;
use oml_app::doubles::{FixedClock, MarkerScanner, MemSigner, MemStore, SeqIdGen};
use oml_app::types::{Bearer, WebEmailRequest, WebEntry};
use uuid::Uuid;

type TestApp = App<MemStore, MemSigner, FixedClock, SeqIdGen, MarkerScanner>;

/// A person's name from a principal: lower case, every other character
/// a hyphen.
fn name(value: &str) -> Name {
    let value: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    Name::parse(&value).expect("a fixture name")
}

/// A session of `capacity` for `entry`, opened now.
pub async fn session_of(app: &TestApp, entry: &WebEntry, capacity: Capacity) -> Caller {
    let token = app
        .web_session_open(entry, 3600, None)
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
}

/// The entry of `user`, added under `admin` where the tenant holds none,
/// with its session keys.
pub async fn entry_of(app: &TestApp, admin: &str, user: &str, is_admin: bool) -> WebEntry {
    let user = name(user);
    let user = user.as_str();
    let held = app
        .list_web_emails(admin, None, 500)
        .await
        .expect("entries")
        .into_iter()
        .find(|e| e.user.as_str() == user);
    let entry = match held {
        Some(entry) => entry,
        None => {
            app.allow_web_email(
                admin,
                &WebEmailRequest {
                    email: Email::parse(&format!("{user}@example.com")).expect("an email"),
                    user: name(user),
                    admin: is_admin,
                },
            )
            .await
            .expect("the entry")
            .0
        }
    };
    app.web_ensure_keys(&entry).await.expect("session keys");
    entry
}

/// The tenant's admin person who links fixture keys: an existing admin
/// entry, else `ops`, so a one-admin tenant stays `sole_admin`.
pub async fn linker(app: &TestApp, admin: &str) -> WebEntry {
    if let Some(entry) = app
        .list_web_emails(admin, None, 500)
        .await
        .expect("entries")
        .into_iter()
        .find(|e| e.admin && !e.pending)
    {
        app.web_ensure_keys(&entry).await.expect("session keys");
        return entry;
    }
    entry_of(app, admin, "ops", true).await
}

/// `key_id` linked to the person `person` and accepted from that person's
/// own session, so the key counts as `person`.
pub async fn link_to(app: &TestApp, admin: &str, key_id: Uuid, person: &str) {
    let ops = linker(app, admin).await;
    let entry = entry_of(app, admin, person, false).await;
    let by = session_of(app, &ops, Capacity::Admin).await;
    app.link_key_to_person(
        Bearer::Session(&by),
        key_id.into(),
        &name(person),
        Some("the person holds this key"),
    )
    .await
    .expect("the key is linked");
    let web = session_of(app, &entry, Capacity::Web).await;
    app.accept_key_link(Bearer::Session(&web), key_id.into())
        .await
        .expect("the person accepts the link");
}

/// The bootstrap admin key `admin` linked to the admin person `ops` by
/// possession and accepted from ops's session (ADR-0097 d21, d22), so the
/// keys it issues count as ops; a key already linked is left as it is.
pub async fn admin_counts(app: &TestApp, admin: &str) {
    let ops = linker(app, admin).await;
    match app
        .link_self(admin, &ops.user, Some("the person holds this key"))
        .await
    {
        Ok(key) => {
            let web = session_of(app, &ops, Capacity::Web).await;
            app.accept_key_link(Bearer::Session(&web), key.key_id.into())
                .await
                .expect("ops accepts the link");
        }
        Err(oml_app::types::AppError::KeyLinkRefused { .. }) => {}
        Err(e) => panic!("the self-link: {e:?}"),
    }
}
