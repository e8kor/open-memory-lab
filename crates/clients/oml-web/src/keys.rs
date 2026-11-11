//! The Keys view of the user page and of the admin page (ADR-0045 d9,
//! d13), moved from the lead to paths of their own, `/web/keys` and
//! `/web/admin/keys` (ADR-0070 d7 (b)), with its assertions unchanged but
//! for the path (`tests/web_pages.rs`).
//!
//! On the user page the person's `<user>-web` key lists their keys, issues
//! a reader or writer key or a connector key on their root, mints a
//! connector key's connect code and cuts a key off, each through its
//! documented operation (`listKeys`, `issueKey`, `mintConnectCode`,
//! `addDenial`). On the admin page `<user>-admin` lists one person's keys,
//! session keys aside, and cuts any of them off.
//!
//! A key's value and a connect code are shown once, on the page the act
//! redirects to, carried there sealed in a cookie the page clears
//! ([`crate::once`]): never in a URL, never in a line, never kept here.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Extension;
use axum::extract::rejection::FormRejection;
use axum::extract::{Form, State};
use axum::response::Response;
use http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use oml_page::web::{
    ADMIN_KEYS_PATH, ADMIN_TABS, USER_KEYS_PATH, attr, form, hidden, input, select, shell, short,
    state_tag, user_tabs,
};
use oml_sdk::{ListedKey, encode};

use crate::once::{self, ONCE_COOKIE, Secret};
use crate::pages::{Fields, refused};
use crate::session::{PageCalls, Session, call, cookie};
use crate::{Capacity, Web, acts, outcome};

/// The public server the plugins and the agent kit connect to unless told
/// otherwise (ADR-0043), shown beside a key's value where the API names no
/// issuer.
pub const DEFAULT_SERVER: &str = "https://memory.doublegate.io";

/// The user page's acts, each the act of one documented operation.
pub const USER_ACTIONS: &[&str] = &["issue_key", "connect_key", "connect_code", "cut_off"];

/// The admin page's one act.
pub const ADMIN_ACTIONS: &[&str] = &["cut_off"];

/// The most keys one listing call reads, the API's maximum.
const KEY_PAGE: u32 = 200;

/// The most pages of keys a view reads: 10,000 keys. A constant of the
/// build, unmeasured (#170).
const KEY_PAGES_MAX: usize = 50;

/// Which page the view is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    User,
    Admin,
}

impl Side {
    fn path(self) -> &'static str {
        match self {
            Self::User => USER_KEYS_PATH,
            Self::Admin => ADMIN_KEYS_PATH,
        }
    }

    fn capacity(self) -> Capacity {
        match self {
            Self::User => Capacity::Web,
            Self::Admin => Capacity::Admin,
        }
    }
}

/// The notice a `done` the view's own acts name shows; any other is none.
fn notice(done: Option<&str>) -> Option<&'static str> {
    match done? {
        "cut off" => Some("cut off"),
        _ => None,
    }
}

/// Every key `user` names (the session's own where `None`), a page at a
/// time.
async fn keys(
    web: &Web,
    credential: &str,
    calls: &PageCalls,
    user: Option<&str>,
) -> Result<Vec<ListedKey>, oml_sdk::Error> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..KEY_PAGES_MAX {
        let ids = calls.next_ids();
        let page = web
            .api
            .list_keys(&call(credential, &ids), user, KEY_PAGE, cursor.as_deref())
            .await?;
        out.extend(page.items);
        match page.next_cursor {
            Some(next) if Some(&next) != cursor.as_ref() => cursor = Some(next),
            _ => break,
        }
    }
    Ok(out)
}

/// One key's row, with the acts the page allows on it: a live key that is
/// not a session key can be cut off, and on the user page a live connector
/// key gets a new connect code. On the admin page the cut-off carries the
/// listed `user`, so the act lands back on the same list.
fn row(key: &ListedKey, side: Side, csrf: &str, user: &str) -> String {
    let name = key.name.as_deref().unwrap_or("");
    let access: Vec<String> = key
        .grants
        .iter()
        .map(|g| format!("{} on {}", g.role, short(&g.path)))
        .collect();
    let state = if key.revoked {
        "revoked"
    } else if key.cut_off {
        "cut off"
    } else {
        "live"
    };
    let mut acts = String::new();
    if key.credential != "session" && state == "live" {
        if key.credential == "connection" && side == Side::User {
            acts.push_str(&form(
                side.path(),
                "connect_code",
                csrf,
                &format!("{}{}", hidden("key", name), acts::nonce_field()),
                "New connect code",
                false,
            ));
        }
        let listed = if side == Side::Admin {
            hidden("user", user)
        } else {
            String::new()
        };
        acts.push_str(&form(
            side.path(),
            "cut_off",
            csrf,
            &format!("{}{listed}{}", hidden("key", name), acts::nonce_field()),
            "Cut off",
            false,
        ));
    }
    format!(
        r#"<tr><td class="mono">{}</td><td>{}<br><span class="dim">{} · {}</span></td><td>{}</td><td>{}</td><td>{}</td></tr>"#,
        short(name),
        attr(&key.role),
        attr(&key.credential),
        attr(&key.held_by),
        access.join("<br>"),
        state_tag(state),
        acts
    )
}

fn table(keys: &[ListedKey], side: Side, csrf: &str, user: &str) -> String {
    let mut out = String::from(
        "<table><tr><th>Key</th><th>Role</th><th>Access</th><th>State</th><th></th></tr>",
    );
    for key in keys {
        out.push_str(&row(key, side, csrf, user));
    }
    out.push_str("</table>");
    out
}

/// The card a shown-once secret is drawn on, as the lead's page drew it.
fn secret_card(secret: &Secret) -> (&'static str, String) {
    match secret {
        Secret::Key {
            name,
            value,
            server,
        } => (
            "Key issued",
            format!(
                r#"<h1>{}</h1><p class="desc">Shown once: copy it now. Then <a href="/web?view=setup">set up</a> your agent.</p><div class="card secret"><p class="label">For your agent</p><pre>OML_URL={}
OML_API_KEY={}</pre></div><p><a href="{USER_KEYS_PATH}">Back to your keys</a></p>"#,
                short(name),
                attr(server),
                attr(value)
            ),
        ),
        Secret::Code {
            name,
            code,
            expires_at,
            connector_url,
        } => (
            "Connect code",
            format!(
                r#"<p class="label">Connect code</p><h1>{}</h1><p class="desc">Shown once; works once, until {}. Add a custom connector in Claude with this URL, then enter the code.</p><div class="card secret"><p class="label">Connector URL</p><pre>{}</pre><p class="label">Connect code</p><pre>{}</pre></div><p><a href="{USER_KEYS_PATH}">Back to your keys</a></p>"#,
                short(name),
                short(expires_at),
                attr(connector_url),
                attr(code)
            ),
        ),
    }
}

/// The page with the `Set-Cookie` that clears a sealed secret, whether or
/// not it opened.
fn clearing(mut response: Response) -> Response {
    if let Ok(value) = HeaderValue::from_str(&once::clear_cookie()) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

/// Any session but an admin entry's answers `access.not_found` on the
/// admin page (ADR-0045 d10).
fn admin_only(session: &Session, calls: &PageCalls) -> Option<Response> {
    (!session.admin).then(|| {
        refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            calls,
        )
    })
}

/// `GET /web/keys`: the person's keys and the issue forms, or once, after
/// an issue or a new connect code, the secret it produced.
pub async fn user_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    if let Some(sealed) = cookie(&headers, ONCE_COOKIE) {
        // Shown here once and cleared in the same answer; a seal that does
        // not open for this session and page is cleared unshown.
        if let Some(secret) = once::open(session.token(), USER_KEYS_PATH, &sealed) {
            let (title, body) = secret_card(&secret);
            return clearing(outcome::html(
                StatusCode::OK,
                shell(
                    title,
                    &session.who(),
                    &user_tabs(session.admin),
                    USER_KEYS_PATH,
                    None,
                    &body,
                ),
            ));
        }
        return clearing(user_view(&web, &session, &calls, &fields, &uri).await);
    }
    user_view(&web, &session, &calls, &fields, &uri).await
}

async fn user_view(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    fields: &Fields,
    uri: &Uri,
) -> Response {
    let next = uri
        .path_and_query()
        .map_or(USER_KEYS_PATH, |pq| pq.as_str());
    let Some(user) = session.whoami.user.as_ref().map(|u| u.name.clone()) else {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            calls,
        );
    };
    let listed = match keys(web, &session.credential(Capacity::Web), calls, None).await {
        Ok(listed) => listed,
        Err(e) => return outcome::page(&e, calls.page_id(), next),
    };
    let csrf = session.csrf();
    let mut body = format!(
        r#"<h1>Keys</h1><p class="desc">Keys acting for <code>{}</code>.</p>{}"#,
        short(&user),
        table(&listed, Side::User, &csrf, "")
    );
    let prefix = format!("{user}-");
    let issue = |action: &str, button: &str, primary: bool| {
        form(
            USER_KEYS_PATH,
            action,
            &csrf,
            &format!(
                "{}{}{}",
                input("name", &format!("name, after {prefix}"), "", true),
                select(
                    "role",
                    "role",
                    &[("writer", "writer"), ("reader", "reader")]
                ),
                acts::nonce_field()
            ),
            button,
            primary,
        )
    };
    let _ = write!(
        body,
        "<h2>Agent key</h2><p class=\"desc\">Your workarea only. The value is shown once.</p>{}<h2>Connector key</h2><p class=\"desc\">For Claude on the web or a phone; its connect code is shown once.</p>{}",
        issue("issue_key", "Issue key", true),
        issue("connect_key", "Issue connector key", false)
    );
    outcome::html(
        StatusCode::OK,
        shell(
            "Your page",
            &session.who(),
            &user_tabs(session.admin),
            USER_KEYS_PATH,
            notice(fields.get("done")),
            &body,
        ),
    )
}

/// `GET /web/admin/keys`: one person's keys, named by `user`, with a
/// cut-off beside each; session keys are not listed.
pub async fn admin_page(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    uri: Uri,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let next = uri
        .path_and_query()
        .map_or(ADMIN_KEYS_PATH, |pq| pq.as_str());
    let user = fields.given("user");
    let mut body = format!(
        r#"<h1>Keys</h1><p class="desc">One person's agent and connector keys; cut one off here.</p><form class="row" method="get" action="{ADMIN_KEYS_PATH}">{}<button type="submit">Show</button></form>"#,
        input("user", "user", user.unwrap_or(""), true)
    );
    if let Some(user) = user {
        let listed = match keys(
            &web,
            &session.credential(Capacity::Admin),
            &calls,
            Some(user),
        )
        .await
        {
            Ok(listed) => listed,
            Err(e) => return outcome::page(&e, calls.page_id(), next),
        };
        body.push_str(&table(&listed, Side::Admin, &session.csrf(), user));
    }
    outcome::html(
        StatusCode::OK,
        shell(
            "Administration",
            &session.who(),
            &ADMIN_TABS,
            ADMIN_KEYS_PATH,
            notice(fields.get("done")),
            &body,
        ),
    )
}

/// The fields each action defines on `side`, beside `action`,
/// `csrf_token` and `nonce`: the admin's cut-off also names the listed
/// `user`.
fn defined(side: Side, action: &str, name: &str) -> bool {
    match action {
        "issue_key" | "connect_key" => matches!(name, "name" | "role"),
        _ => name == "key" || (side == Side::Admin && name == "user"),
    }
}

/// The server URL an agent sets beside its key: the API's issuer.
async fn server(web: &Web) -> String {
    match web.api.issuer().await {
        Ok(issuer) => issuer,
        Err(_) => web
            .layers
            .public_origin
            .clone()
            .unwrap_or_else(|| DEFAULT_SERVER.to_owned()),
    }
}

/// One act: the form checked as every act's is ([`acts::read`]), then its
/// operation under the session's capacity credential and the form's
/// idempotency key; a secret it produced is sealed into the cookie the
/// redirect target opens.
async fn act(
    web: Web,
    session: Session,
    calls: Arc<PageCalls>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
    side: Side,
) -> Response {
    let page = side.path();
    let actions = match side {
        Side::User => USER_ACTIONS,
        Side::Admin => ADMIN_ACTIONS,
    };
    let (fields, action) = match acts::read(form, actions, |a, n| defined(side, a, n), &calls) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let required: &[&str] = match action {
        "issue_key" | "connect_key" => &["name"],
        _ => &["key"],
    };
    if let Some(missing) = required.iter().find(|n| fields.given(n).is_none()) {
        return acts::invalid(&format!("{missing} is required"), &calls);
    }
    let key = match acts::key(&session, &fields, &calls) {
        Ok(key) => key,
        Err(refusal) => return *refusal,
    };
    let _in_flight = acts::hold(&key).await;
    let credential = session.credential(side.capacity());
    let ids = calls.next_ids();
    let call = call(&credential, &ids);
    let secret = match action {
        "issue_key" | "connect_key" => {
            let role = match fields.given("role").unwrap_or("writer") {
                role @ ("reader" | "writer") => role,
                _ => return acts::invalid("role is reader or writer", &calls),
            };
            let user = session.whoami.user.as_ref().map_or("", |u| u.name.as_str());
            let name = format!("{user}-{}", fields.given("name").unwrap_or_default());
            let connector = action == "connect_key";
            match web.api.issue_key(&call, &key, &name, role, connector).await {
                Ok(issued) => {
                    let code = issued
                        .connect_code
                        .and_then(|c| Some((c.code?, c.expires_at, c.connector_url)));
                    match (issued.value, code) {
                        (Some(value), _) => Secret::Key {
                            name: issued.name,
                            value,
                            server: server(&web).await,
                        },
                        (None, Some((code, expires_at, connector_url))) => Secret::Code {
                            name: issued.name,
                            code,
                            expires_at,
                            connector_url,
                        },
                        // A replay, which shows neither: the first answer's
                        // cookie holds the secret, and this one sets none
                        // over it.
                        (None, None) => return acts::see_other(USER_KEYS_PATH, &[]),
                    }
                }
                Err(e) => return outcome::page(&e, calls.page_id(), page),
            }
        }
        "connect_code" => {
            let name = fields.given("key").unwrap_or_default();
            match web.api.mint_connect_code(&call, &key, name).await {
                Ok(oml_sdk::ConnectCode {
                    code: Some(code),
                    expires_at,
                    connector_url,
                    ..
                }) => Secret::Code {
                    name: name.to_owned(),
                    code,
                    expires_at,
                    connector_url,
                },
                // A replay, which shows no code: as for an issue.
                Ok(_) => return acts::see_other(USER_KEYS_PATH, &[]),
                Err(e) => return outcome::page(&e, calls.page_id(), page),
            }
        }
        _ => {
            let name = fields.given("key").unwrap_or_default();
            let listed = fields
                .given("user")
                .map_or_else(String::new, |u| format!("user={}&", encode(u)));
            return match web.api.add_denial(&call, &key, name, "universe").await {
                Ok(_) => acts::see_other(&format!("{page}?{listed}done=cut+off"), &[]),
                Err(e) => outcome::page(&e, calls.page_id(), page),
            };
        }
    };
    let sealed = once::seal(session.token(), USER_KEYS_PATH, &secret);
    acts::see_other(USER_KEYS_PATH, &[once::set_cookie(&sealed)])
}

/// `POST /web/keys`: an act of the user page's view.
pub async fn user_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    act(web, session, calls, form, Side::User).await
}

/// `POST /web/admin/keys`: the admin page's cut-off.
pub async fn admin_act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    act(web, session, calls, form, Side::Admin).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A new key is shown beside the API's issuer, else beside `oml-web`'s
    /// own public origin, and with neither beside the named `no_server`
    /// line and no URL at all: the page never names another server
    /// (ADR-0076 d6, d8).
    #[test]
    fn a_new_key_is_shown_beside_the_apis_issuer_and_with_none_beside_a_named_no_server_line() {
        let issuer = "https://oml.example.com";
        assert_eq!(
            server_from(Some(issuer.to_owned()), Some("https://web.example.com")),
            issuer
        );
        assert_eq!(server_from(None, Some(issuer)), issuer);
        assert_eq!(server_from(None, None), "");

        let card = |server: &str| {
            secret_card(&Secret::Key {
                name: "alice-laptop".to_owned(),
                value: "oml_value".to_owned(),
                server: server.to_owned(),
            })
            .1
        };
        let shown = card(issuer);
        assert!(shown.contains(&format!("OML_URL={issuer}")), "{shown}");
        let unknown = card("");
        assert!(!unknown.contains("OML_URL="), "{unknown}");
        assert!(
            unknown.contains(
                "This server's address is not configured (no_server: set web.public_url, or \
                 oauth.issuer on the lead); ask its operator for the URL to give your agents."
            ),
            "{unknown}"
        );
        assert!(unknown.contains("OML_API_KEY=oml_value"), "{unknown}");
    }
}
