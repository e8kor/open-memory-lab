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
    ADMIN_KEYS_PATH, USER_KEYS_PATH, attr, form, hidden, input, select, shell, short, state_tag,
    text,
};
use oml_sdk::{ListedKey, encode};

use crate::once::{self, ONCE_COOKIE, Secret};
use crate::pages::{Fields, WalkEnd, refused, walk};
use crate::session::{PageCalls, Session, call, cookie};
use crate::{Capacity, Web, acts, outcome, usage};

/// The user page's acts, each the act of one documented operation.
pub const USER_ACTIONS: &[&str] = &["issue_key", "connect_key", "connect_code", "cut_off"];

/// The admin page's one act.
pub const ADMIN_ACTIONS: &[&str] = &["cut_off"];

/// The most keys one listing call reads, the API's maximum.
const KEY_PAGE: u32 = 200;

/// The most pages of keys a view reads: 10,000 keys, past which it is
/// refused by name. A constant of the build, unmeasured (#170).
pub const KEY_PAGES_MAX: usize = 50;

/// Which page the view is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
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

/// What the Keys page and the Start page say to a person whose session key
/// carries a denial, in place of the forms for the three acts that key is
/// refused for good: issuing a key, minting a connect code and cutting off a
/// key (`access.verb_ceiling`, reason `session_key_denied`; ADR-0070 d2).
/// Only an admin key denies a session key, and an admin key still does the
/// three for the person.
pub(crate) const DENIED_SESSION: &str = "This web session is restricted: it cannot issue keys, get connect codes or cut off keys. An administrator can.";

/// The muted line under it: the export, the deletion request and sign-out
/// read nothing of the key, so they still work (#335).
const DENIED_STILL_WORKS: &str =
    "Exporting memories, requesting deletion and signing out still work.";

/// The word [`DENIED_SESSION`] uses, which marks the session's own key in
/// the table.
const RESTRICTED: &str = "restricted";

/// The sentence and its muted line as the pages draw them.
pub(crate) fn denied_notice() -> String {
    format!(
        r#"<p class="notice" role="status">{}</p><p class="desc">{}</p>"#,
        text(DENIED_SESSION),
        text(DENIED_STILL_WORKS)
    )
}

/// A served instant to the second, in UTC by name, inside a `<time>` whose
/// `datetime` is the instant as served: `2026-10-29 12:30:00 UTC`. An
/// instant in any other shape is shown as served. Every page shows an
/// instant this way.
pub(crate) fn instant(at: &str) -> String {
    let shown = match unix_of(at) {
        Some(_) => format!("{} {} UTC", &at[..10], &at[11..19]),
        None => at.to_owned(),
    };
    format!(r#"<time datetime="{}">{}</time>"#, attr(at), short(&shown))
}

/// Whether the session's key carries a denial, as `GET /v2/whoami` says of
/// a person's `<user>-web` and of no other key; the admin's page is never
/// drawn for one, since an admin key takes no denial.
pub(crate) fn denied(session: &Session) -> bool {
    session.whoami.denied == Some(true)
}

/// The one optional input of a person's issue form: the day the key ends,
/// which a browser sends as `YYYY-MM-DD` with no script. The label names
/// the instant, so no zone is left to guess (ADR-0079 d1, the operator's
/// answer 3).
const EXPIRES_INPUT: &str = r#"<label>Key expires (optional; end of that day, UTC)<input type="date" name="expires"></label>"#;

/// What the form's day becomes: its last second in UTC.
const END_OF_DAY: &str = "T23:59:59Z";

/// The line beside the input: a person sets the date once (ADR-0079 d5).
const EXPIRES_NOTE: &str = "A key's expiry date is optional; only an admin can change it later.";

/// The notice a `done` the view's own acts name shows; any other is none.
fn notice(done: Option<&str>) -> Option<&'static str> {
    match done? {
        "cut off" => Some("cut off"),
        _ => None,
    }
}

/// Every key `user` names (the session's own where `None`), a page at a
/// time, or the page that says why the list could not be read whole, so
/// no view shows it cut short (#308): the API's refusal, `unavailable`
/// with 502 at a cursor the API repeats, the API's fault, and
/// `limit.list_too_long` past [`KEY_PAGES_MAX`] pages. `next` is the page
/// a sign-in returns to.
pub(crate) async fn keys(
    web: &Web,
    credential: &str,
    calls: &PageCalls,
    user: Option<&str>,
    next: &str,
) -> Result<Vec<ListedKey>, acts::Refusal> {
    walk(KEY_PAGES_MAX, |cursor| async move {
        let ids = calls.next_ids();
        let page = web
            .api
            .list_keys(&call(credential, &ids), user, KEY_PAGE, cursor.as_deref())
            .await?;
        Ok((page.items, page.next_cursor))
    })
    .await
    .map_err(|end| {
        Box::new(match end {
            WalkEnd::Failed(e) => outcome::page(&e, calls, next),
            WalkEnd::Repeated => refused(
                StatusCode::BAD_GATEWAY,
                "unavailable",
                "the server repeated a cursor of the key list, so the list cannot be read whole; try again",
                calls,
            ),
            WalkEnd::TooLong => refused(
                StatusCode::UNPROCESSABLE_ENTITY,
                "limit.list_too_long",
                &format!(
                    "the key list is longer than this page reads ({KEY_PAGES_MAX} pages of {KEY_PAGE}); use the API or the command line"
                ),
                calls,
            ),
        })
    })
}

/// One key's row, with the acts the page allows on it: a key that is not a
/// session key and is neither revoked nor cut off can be cut off, and on the
/// user page such a connector key gets a new connect code unless it has
/// expired. On the admin page the cut-off carries the
/// listed `user`, so the act lands back on the same list. `denied` is the id
/// of the session's own key where that key carries a denial: no row then
/// offers an act, and that key's row is marked.
fn row(key: &ListedKey, side: Side, csrf: &str, user: &str, denied: Option<&str>) -> String {
    let name = key.name.as_deref().unwrap_or("");
    let access: Vec<String> = key
        .grants
        .iter()
        .map(|g| format!("{} on {}", g.role, short(&g.path)))
        .collect();
    // The marks a key carries, in the place and style of `revoked` and `cut
    // off`; a key with none is live. An expired key stays cut-off-able (a
    // denial is not refused on it) and takes no new connect code, which the
    // API refuses with `oauth.connect_refused` and reason `expired`
    // (ADR-0079 d2, d3).
    let mut marks = Vec::new();
    if key.revoked {
        marks.push("revoked");
    } else if key.cut_off {
        marks.push("cut off");
    }
    if key.expired {
        marks.push("expired");
    }
    // The session's own key, where it carries a denial, in the word the
    // page's sentence uses: `GET /v2/keys` says `cut_off` for a denial on
    // `universe` alone, so the row would read live under that sentence
    // (#343). The page knows it of this one key and marks no other.
    if denied == Some(key.id.as_str()) {
        marks.push(RESTRICTED);
    }
    let working = !key.revoked && !key.cut_off;
    if marks.is_empty() {
        marks.push("live");
    }
    // Apart in the markup and to a reader that speaks it: "cut off,
    // expired", never one word.
    let marks = marks
        .into_iter()
        .map(state_tag)
        .collect::<Vec<_>>()
        .join(r#"<span class="sr-only">,</span> "#);
    let mut limits = String::new();
    if let Some(at) = &key.expiry {
        let verb = if key.expired { "expired" } else { "expires" };
        let _ = write!(
            limits,
            r#"<br><span class="dim">{verb} {}</span>"#,
            instant(at)
        );
    }
    if let Some(n) = key.requests_per_minute {
        let _ = write!(
            limits,
            r#"<br><span class="dim">{n} {} a minute</span>"#,
            if n == 1 { "request" } else { "requests" }
        );
    }
    let mut acts = String::new();
    if key.credential != "session" && working && denied.is_none() {
        if key.credential == "connection" && side == Side::User {
            if key.expired {
                acts.push_str(
                    r#"<p class="dim">Expired: no new connect code until an admin sets a later date.</p>"#,
                );
            } else {
                acts.push_str(&form(
                    side.path(),
                    "connect_code",
                    csrf,
                    &format!("{}{}", hidden("key", name), acts::nonce_field()),
                    "New connect code",
                    false,
                ));
            }
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
        r#"<tr><td class="mono">{}</td><td>{}<br><span class="dim">{} · {}</span>{limits}</td><td>{}</td><td>{marks}</td><td>{}</td></tr>"#,
        short(name),
        attr(&key.role),
        attr(&key.credential),
        attr(&key.held_by),
        access.join("<br>"),
        acts
    )
}

/// The keys as a table. `denied`: the id of the session's own key where it
/// carries a denial, so no row offers an act that key is refused and its
/// own row says so.
pub(crate) fn table(
    keys: &[ListedKey],
    side: Side,
    csrf: &str,
    user: &str,
    denied: Option<&str>,
) -> String {
    let mut out = String::from(
        "<table class=\"oml-stack t-keys\"><tr><th>Key</th><th>Role</th><th>Access</th><th>State</th><th></th></tr>",
    );
    for key in keys {
        out.push_str(&row(key, side, csrf, user, denied));
    }
    out.push_str("</table>");
    out
}

/// This process's clock, in Unix seconds.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Days from 1970-01-01 to the civil date `y-m-d` (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DDTHH:MM:SS[.fraction]Z` as Unix seconds; `None` for any other
/// shape, an offset other than `Z` included.
fn unix_of(at: &str) -> Option<i64> {
    let b = at.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let rest = at.get(19..)?;
    let fraction = rest.strip_suffix('Z')?;
    if !(fraction.is_empty()
        || (fraction.len() > 1
            && fraction.starts_with('.')
            && fraction[1..].bytes().all(|c| c.is_ascii_digit())))
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = at.get(r)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (y, mo, d, h, mi, sec) = (
        n(0..4)?,
        n(5..7)?,
        n(8..10)?,
        n(11..13)?,
        n(14..16)?,
        n(17..19)?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3_600 + mi * 60 + sec)
}

/// How long a connect code still works, read at `now`: "expires in 10
/// minutes (10:20 UTC)", "has expired (10:20 UTC)" once past, or the
/// instant as served when it is not a UTC instant this page reads.
fn expiry(expires_at: &str, now: i64) -> String {
    let Some(at) = unix_of(expires_at) else {
        return format!("is good until {expires_at}");
    };
    let clock = &expires_at[11..16];
    let left = at - now;
    if left <= 0 {
        return format!("has expired ({clock} UTC)");
    }
    let minutes = (left + 59) / 60;
    let unit = if minutes == 1 { "minute" } else { "minutes" };
    format!("expires in {minutes} {unit} ({clock} UTC)")
}

/// The card a shown-once secret is drawn on, as the lead's page drew it.
fn secret_card(secret: &Secret) -> (&'static str, String) {
    match secret {
        Secret::Key {
            name,
            value,
            server,
        } => {
            let (url, note) = if server.is_empty() {
                (String::new(), format!(r#"<p class="desc">{NO_SERVER}</p>"#))
            } else {
                (format!("OML_URL={}\n", attr(server)), String::new())
            };
            (
                "Key issued",
                format!(
                    r#"<h1>{}</h1><p class="desc">Shown once: copy it now. Then <a href="/web/setup">set up</a> the agent.</p>{note}<div class="card secret"><p class="label">For the agent</p><pre>{url}OML_API_KEY={}</pre></div><p><a href="{USER_KEYS_PATH}">Back to Keys</a></p>"#,
                    short(name),
                    attr(value)
                ),
            )
        }
        Secret::Code {
            name,
            code,
            expires_at,
            connector_url,
        } => (
            "Connect code",
            format!(
                r#"<p class="label">Connect code</p><h1>{}</h1><p class="desc">Shown once; works once and {}. Add a custom connector in Claude with this URL, then enter the code.</p><div class="card secret"><p class="label">Connector URL</p><pre>{}</pre><p class="label">Connect code</p><pre>{}</pre></div><p><a href="{USER_KEYS_PATH}">Back to Keys</a></p>"#,
                short(name),
                text(&expiry(expires_at, unix_now())),
                attr(connector_url),
                attr(code)
            ),
        ),
    }
}

/// The page with the `Set-Cookie` that clears a sealed secret, whether or
/// not it opened.
pub(crate) fn clearing(mut response: Response) -> Response {
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
                shell(title, &session.personal(), USER_KEYS_PATH, None, &body),
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
    // The person's usage is the page's secondary call, read beside the key
    // list in the same round (ADR-0070 d8): a failure marks the card
    // unavailable and the keys are shown.
    let credential = session.credential(Capacity::Web);
    let usage_ids = calls.next_ids();
    let usage_call = call(&credential, &usage_ids);
    let (listed, person) = tokio::join!(
        keys(web, &credential, calls, None, next),
        web.api.whoami_person(&usage_call),
    );
    let listed = match listed {
        Ok(listed) => listed,
        Err(response) => return *response,
    };
    let card = match person {
        Ok(found) => found.person.as_ref().and_then(usage::card),
        Err(_) => Some(usage::unavailable(
            usage_ids.request_id.as_deref().unwrap_or_default(),
        )),
    };
    let csrf = session.csrf();
    // For a session whose key carries a denial the page opens on the
    // sentence, above the table, and draws none of the three forms: the API
    // refuses each one for this session's key, now and for good.
    let denied = denied(session).then_some(session.whoami.key_id.as_str());
    let mut body = format!(
        r#"<h1>Keys</h1><p class="desc">Keys acting for <code>{}</code>.</p>{}{}{}"#,
        short(&user),
        denied.map(|_| denied_notice()).unwrap_or_default(),
        card.unwrap_or_default(),
        table(&listed, Side::User, &csrf, "", denied)
    );
    if denied.is_some() {
        return outcome::html(StatusCode::OK, user_shell(session, fields, &body));
    }
    let prefix = format!("{user}-");
    let issue = |action: &str, button: &str, primary: bool| {
        form(
            USER_KEYS_PATH,
            action,
            &csrf,
            &format!(
                "{}{}{EXPIRES_INPUT}{}",
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
        "<h2>Agent key</h2><p class=\"desc\">The user's workarea only. The value is shown once. {EXPIRES_NOTE}</p>{}<h2>Connector key</h2><p class=\"desc\">For Claude on the web or a phone; its connect code is shown once. {EXPIRES_NOTE}</p>{}",
        issue("issue_key", "Issue key", true),
        issue("connect_key", "Issue connector key", false)
    );
    outcome::html(StatusCode::OK, user_shell(session, fields, &body))
}

/// The user page's frame around `body`.
fn user_shell(session: &Session, fields: &Fields, body: &str) -> String {
    shell(
        "",
        &session.personal(),
        USER_KEYS_PATH,
        notice(fields.get("done")),
        body,
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
            next,
        )
        .await
        {
            Ok(listed) => listed,
            Err(response) => return *response,
        };
        body.push_str(&table(&listed, Side::Admin, &session.csrf(), user, None));
    }
    outcome::html(
        StatusCode::OK,
        shell(
            "",
            &session.administration(),
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
        "issue_key" | "connect_key" => matches!(name, "name" | "role" | "expires"),
        "cut_off" => matches!(name, "key" | "confirm") || (side == Side::Admin && name == "user"),
        _ => name == "key",
    }
}

/// The cut-off's confirmation, which records nothing: the key it names,
/// what cutting it off does, and a form carrying `confirm=yes`, the listed
/// `user` on the admin page, and a fresh nonce, so a resubmitted
/// confirmation replays one cut-off (ADR-0045 d13, as the lead confirmed a
/// removal; #253).
fn cut_off_confirmation(session: &Session, side: Side, key: &str, user: Option<&str>) -> Response {
    let page = side.path();
    let listed = user.map_or_else(String::new, |u| hidden("user", u));
    let back = user.map_or_else(|| page.to_owned(), |u| format!("{page}?user={}", encode(u)));
    let mut body = format!(
        r#"<h1>Cut off {}?</h1><p class="oml-consequence">It stops working on every workarea at once. Its memories stay.</p>"#,
        short(key)
    );
    body.push_str(&form(
        page,
        "cut_off",
        &session.csrf(),
        &format!(
            "{}{listed}{}{}",
            hidden("key", key),
            hidden("confirm", "yes"),
            acts::nonce_field()
        ),
        "Cut it off",
        true,
    ));
    let _ = write!(body, r#"<p><a href="{}">Keep it</a></p>"#, attr(&back));
    let bar = match side {
        Side::User => session.personal(),
        Side::Admin => session.administration(),
    };
    outcome::html(StatusCode::OK, shell("Confirm", &bar, page, None, &body))
}

/// An issue or a new connect code the API answered without the secret and
/// not as a replay: the page cannot show what it was never sent, so it says
/// so by name and sets no cookie, never a redirect that looks like a replay.
pub(crate) fn no_secret(calls: &PageCalls) -> Response {
    refused(
        StatusCode::BAD_GATEWAY,
        "unavailable",
        "the server answered the act without the value or the code it shows once; cut the key off and issue another",
        calls,
    )
}

/// The server URL an agent sets beside its key: the API's issuer, else
/// `oml-web`'s own public origin, else empty, and the page then names
/// `no_server` (ADR-0076 d6, d8).
pub(crate) async fn server(web: &Web) -> String {
    server_from(
        web.api.issuer().await.ok(),
        web.layers.public_origin.as_deref(),
    )
}

/// The API's issuer, else the public origin, else empty: never another
/// server's address.
fn server_from(issuer: Option<String>, public_origin: Option<&str>) -> String {
    issuer
        .or_else(|| public_origin.map(ToOwned::to_owned))
        .unwrap_or_default()
}

/// The line a page shows when no server address is known (ADR-0076 d8).
pub(crate) const NO_SERVER: &str = "This server's address is not configured (no_server: set \
                                    web.public_url, or oauth.issuer on the lead); ask its \
                                    operator for the URL to give the agents.";

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
    // A cut-off is confirmed first, and fails closed: only the
    // confirmation's own field acts (#253).
    if action == "cut_off" && fields.given("confirm") != Some("yes") {
        return cut_off_confirmation(
            &session,
            side,
            fields.given("key").unwrap_or_default(),
            fields.given("user"),
        );
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
            // The form's date is the end of that day in UTC. The API judges
            // the instant, so a value that is not `YYYY-MM-DD`, or a day that
            // has passed, is refused by it and shown as it answers; a day its
            // month does not hold is read as that many days on (ADR-0079 d1).
            let expiry = fields
                .given("expires")
                .map(|day| format!("{day}{END_OF_DAY}"));
            match web
                .api
                .issue_key(&call, &key, &name, role, connector, expiry.as_deref())
                .await
            {
                // A replay, which shows neither: the first answer's cookie
                // holds the secret, and this one sets none over it.
                Ok(issued) if issued.replayed() => return acts::see_other(USER_KEYS_PATH, &[]),
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
                        (None, None) => return no_secret(&calls),
                    }
                }
                Err(e) => return outcome::page(&e, &calls, page),
            }
        }
        "connect_code" => {
            let name = fields.given("key").unwrap_or_default();
            match web.api.mint_connect_code(&call, &key, name).await {
                // A replay, which shows no code: as for an issue.
                Ok(minted) if minted.replayed() => return acts::see_other(USER_KEYS_PATH, &[]),
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
                Ok(_) => return no_secret(&calls),
                Err(e) => return outcome::page(&e, &calls, page),
            }
        }
        _ => {
            let name = fields.given("key").unwrap_or_default();
            let listed = fields
                .given("user")
                .map_or_else(String::new, |u| format!("user={}&", encode(u)));
            return match web.api.add_denial(&call, &key, name, "universe").await {
                Ok(_) => acts::see_other(&format!("{page}?{listed}done=cut+off"), &[]),
                Err(e) => outcome::page(&e, &calls, page),
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

    #[test]
    fn a_connect_codes_expiry_reads_as_minutes_left_and_its_utc_clock_time() {
        let at = "2026-11-11T10:20:00Z";
        let expires = unix_of(at).unwrap();
        assert_eq!(expires, 1_794_392_400);
        assert_eq!(
            expiry(at, expires - 600),
            "expires in 10 minutes (10:20 UTC)"
        );
        assert_eq!(
            expiry(at, expires - 599),
            "expires in 10 minutes (10:20 UTC)"
        );
        assert_eq!(expiry(at, expires - 30), "expires in 1 minute (10:20 UTC)");
        assert_eq!(expiry(at, expires), "has expired (10:20 UTC)");
        assert_eq!(
            expiry("2026-11-11T10:20:00.123456Z", expires - 120),
            "expires in 2 minutes (10:20 UTC)"
        );
        // Not a UTC instant this page reads: shown as served.
        assert_eq!(
            expiry("2026-11-11T11:20:00+01:00", expires - 600),
            "is good until 2026-11-11T11:20:00+01:00"
        );
        assert_eq!(expiry("soon", 0), "is good until soon");
    }

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
                 oauth.issuer on the lead); ask its operator for the URL to give the agents."
            ),
            "{unknown}"
        );
        assert!(unknown.contains("OML_API_KEY=oml_value"), "{unknown}");
    }
}
