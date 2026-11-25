//! The Keys view of the user page and of the admin page (ADR-0045 d9,
//! d13), moved from the lead to paths of their own, `/web/keys` and
//! `/web/admin/keys` (ADR-0070 d7 (b)), with its assertions unchanged but
//! for the path (`tests/web_pages.rs`).
//!
//! On the user page the person's `<user>-web` key lists their keys, issues
//! a contributor or reader key or a connector key on their root, mints a
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
    ADMIN_KEYS_PATH, TODO_PATH, USER_KEYS_PATH, attr, form, hidden, input, select, shell, short,
    state_tag, text,
};
use oml_sdk::{ListedKey, encode};

use crate::once::{self, ONCE_COOKIE, Secret};
use crate::pages::{Fields, WalkEnd, refused, walk};
use crate::session::{PageCalls, Session, call, cookie};
use crate::{Capacity, Web, acts, outcome, usage};

/// The user page's acts, each the act of one documented operation.
pub const USER_ACTIONS: &[&str] = &[
    "build_key",
    "rotate_key",
    "issue_key",
    "connect_key",
    "connect_code",
    "cut_off",
    "accept_link",
    "accept_email",
    "decline_email",
];

/// The admin page's acts: the cut-off, and a tenant key's link to the
/// listed person and its end (ADR-0085 d16).
pub const ADMIN_ACTIONS: &[&str] = &[
    "cut_off",
    "rotate_key",
    "link_key",
    "unlink_key",
    "set_limits",
    "deny_on",
];

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

/// The mark of a contributor key that holds no reader grant (ADR-0093 d3).
const NO_RECALL: &str = "no recall";

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

/// A key's end as the lists show it (ADR-0096 d10): an instant at
/// `23:59:59` UTC, the shape the console sends, reads "works through" the
/// day before it, in every time zone, beside the stored instant; any other
/// instant shows alone, since a computed date could fall before the key
/// was issued. Never a date the server did not store.
pub(crate) fn ends(at: &str, expired: bool) -> String {
    let end_of_day = unix_of(at).is_some_and(|unix| unix.rem_euclid(86_400) == 86_399);
    if end_of_day && let Some(unix) = unix_of(at) {
        let through = &rfc3339(unix - 86_400)[..10];
        let verb = if expired { "expired: worked" } else { "works" };
        return format!(
            "{verb} through {through}, every time zone · ends {}",
            instant(at)
        );
    }
    let verb = if expired { "ended" } else { "ends" };
    format!("{verb} {}", instant(at))
}

/// The instant a form's date `YYYY-MM-DD` is sent as: the next UTC day's
/// `23:59:59Z`, so the key works through the chosen date in every time
/// zone (ADR-0096 d10); a day its month does not hold rolls over
/// (ADR-0079's answer 3). `None` for any other shape.
pub(crate) fn expiry_sent(day: &str) -> Option<String> {
    let b = day.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = &day[r];
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(rfc3339((days_from_civil(y, m, d) + 2) * 86_400 - 1))
}

/// Whether the session's key carries a denial, as `GET /v2/whoami` says of
/// a person's `<user>-web` and of no other key; the admin's page is never
/// drawn for one, since an admin key takes no denial.
pub(crate) fn denied(session: &Session) -> bool {
    session.whoami.denied == Some(true)
}

/// The line beside the input: a person sets the date once (ADR-0079 d5).
const EXPIRES_NOTE: &str =
    "A key's expiry date and rate are optional; only an admin can change them later.";

/// The key builder's page (ADR-0096 d1, D5).
pub const NEW_KEY_PATH: &str = "/web/keys/new";

/// The notice a `done` the view's own acts name shows; any other is none.
fn notice(done: Option<&str>) -> Option<&'static str> {
    match done? {
        "cut off" => Some("cut off"),
        "linked" => Some("linked: the key acts for this person once they accept the link"),
        "accepted" => Some("accepted: the console acts with the key for this account"),
        "email_accepted" => Some("accepted: the email signs in to this account"),
        "email_removed" => Some("removed: the email signs in to nothing"),
        "unlinked" => Some("unlinked: the console no longer acts with the key"),
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
    // Each grant with its coverage: a node grant is marked "this
    // workspace only" (ADR-0096 d8).
    let access: Vec<String> = key
        .grants
        .iter()
        .map(|g| {
            if g.covers == "node" {
                format!(
                    "{} on <code>{}</code> (this workspace only)",
                    g.role,
                    short(&g.path)
                )
            } else {
                format!("{} on <code>{}</code> and below", g.role, short(&g.path))
            }
        })
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
    // A contributor without recall, as the API serves it: a submitter grant
    // on a node no reader grant of the key reaches, such as a writer key
    // issued write-only before v0.8.4, which works as before and is
    // replaced by a submitter key (ADR-0093 d3). The listing's grants carry
    // no ancestry, so the page decides nothing of its own.
    if key.without_recall {
        marks.push(NO_RECALL);
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
        let _ = write!(
            limits,
            r#"<br><span class="dim">{}</span>"#,
            ends(at, key.expired)
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
            // The one-key read and its limits (ADR-0096 d4); never on an
            // admin key, which takes no limit.
            let _ = write!(
                acts,
                r#"<a href="{ADMIN_KEYS_PATH}?key={k}">Open</a> "#,
                k = attr(&encode(name))
            );
            if key.role != "admin" {
                let _ = write!(
                    acts,
                    r#"<a href="{ADMIN_KEYS_PATH}?key={k}&amp;op=limits">Limits</a> "#,
                    k = attr(&encode(name))
                );
            }
            hidden("user", user)
        } else {
            String::new()
        };
        // Rotate, beside Cut off, on a live bearer key that has not
        // expired: a connector key takes a new connect code instead
        // (ADR-0096 d13). Never on the admin side, whose list is one
        // person's keys, which their person rotates and an admin does not
        // (the operator's decision of 2026-11-25).
        if side == Side::User && key.credential == "bearer" && !key.expired {
            acts.push_str(&form(
                side.path(),
                "rotate_key",
                csrf,
                &format!("{}{listed}", hidden("key", name)),
                "Rotate",
                false,
            ));
        }
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
        attr(&key.used_by),
        access.join("<br>"),
        acts
    )
}

/// What an issuance shows once: the value, or the connect code; `None`
/// for an answer with neither that is no replay.
async fn secret_of(web: &Web, issued: oml_sdk::KeyIssued) -> Option<Secret> {
    let code = issued
        .connect_code
        .and_then(|c| Some((c.code?, c.expires_at, c.connector_url)));
    match (issued.value, code) {
        (Some(value), _) => Some(Secret::Key {
            name: issued.name,
            value,
            server: server(web).await,
        }),
        (None, Some((code, expires_at, connector_url))) => Some(Secret::Code {
            name: issued.name,
            code,
            expires_at,
            connector_url,
        }),
        (None, None) => None,
    }
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
pub(crate) fn unix_now() -> i64 {
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

/// The civil date of `days` after 1970-01-01 (proleptic Gregorian), the
/// inverse of [`days_from_civil`].
const fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`, the instant a query sends.
pub(crate) fn rfc3339(unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(86_400));
    let secs = unix.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
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
        Secret::Rotated {
            name,
            value,
            server,
            previous_valid_until,
            back,
        } => {
            let url = if server.is_empty() {
                String::new()
            } else {
                format!("OML_URL={}\n", attr(server))
            };
            let previous = previous_valid_until.as_deref().map_or_else(
                || "The previous value stopped working now.".to_owned(),
                |at| {
                    format!(
                        "The previous value works until {} UTC, then stops.",
                        text(
                            &at.replacen('T', " ", 1)
                                .chars()
                                .take(16)
                                .collect::<String>()
                        )
                    )
                },
            );
            (
                "Key rotated",
                format!(
                    r#"<p class="label">Key rotated</p><h1>{}</h1><p class="desc">Shown once: copy it now. {previous}</p><div class="card secret"><p class="label">For the agent</p><pre>{url}OML_API_KEY={}</pre></div><p><a href="{}">Back to Keys</a></p>"#,
                    short(name),
                    attr(value),
                    attr(back)
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
    let (card, linked) = match person {
        Ok(found) => (
            found.person.as_ref().and_then(usage::card),
            found
                .person
                .map(|p| (p.linked_keys, p.pending_links, p.pending_emails))
                .unwrap_or_default(),
        ),
        Err(_) => (
            Some(usage::unavailable(
                usage_ids.request_id.as_deref().unwrap_or_default(),
            )),
            (Vec::new(), Vec::new(), Vec::new()),
        ),
    };
    let (linked, pending, emails) = linked;
    let csrf = session.csrf();
    // For a session whose key carries a denial the page opens on the
    // sentence, above the table, and draws none of the three forms: the API
    // refuses each one for this session's key, now and for good.
    let denied = denied(session).then_some(session.whoami.key_id.as_str());
    let mut body = format!(
        r#"<h1>Keys</h1><p class="desc">Keys acting for <code>{}</code>.</p>{}{}{}{}{}{}"#,
        short(&user),
        denied.map(|_| denied_notice()).unwrap_or_default(),
        card.unwrap_or_default(),
        table(&listed, Side::User, &csrf, "", denied),
        pending_emails_section(&emails, &csrf),
        pending_section(&pending, &csrf),
        linked_section(&linked)
    );
    if denied.is_some() {
        return outcome::html(StatusCode::OK, user_shell(session, fields, &body));
    }
    // The key builder is the page's one way to a new key (ADR-0096 d1, D5):
    // the kind, the grants, the date and the rate are chosen there.
    let _ = write!(
        body,
        r#"<p><a class="button" href="{NEW_KEY_PATH}">New key</a></p><p class="desc">{EXPIRES_NOTE}</p>"#
    );
    outcome::html(StatusCode::OK, user_shell(session, fields, &body))
}

/// The tenant keys an admin linked to the person, each with the link that
/// opens Review judging as it in the shared workspaces it is granted;
/// nothing where none is linked (ADR-0085 d16).
fn linked_section(linked: &[oml_sdk::LinkedKey]) -> String {
    if linked.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        r#"<h2>Linked keys</h2><p class="desc">Tenant keys an admin linked to this account. Each judges in the shared workspaces it is granted.</p><ul>"#,
    );
    for key in linked {
        let _ = write!(
            out,
            r#"<li><code>{}</code> · <a href="{TODO_PATH}?as={}">Judge as this key in To do</a></li>"#,
            short(&key.name),
            attr(&encode(&key.id))
        );
    }
    out.push_str("</ul>");
    out
}

/// The links an admin made of tenant keys to this account that wait on its
/// acceptance, each with its **Accept link** act; nothing where none waits.
/// Until accepted, a key acts for no one (ADR-0085 d16, the operator's
/// decision of 2026-11-19).
fn pending_section(pending: &[oml_sdk::LinkedKey], csrf: &str) -> String {
    if pending.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        r#"<h2>Links to accept</h2><p class="desc">An admin linked these tenant keys to this account. A key acts for this account only once the link is accepted here; until then it acts for no one.</p><ul>"#,
    );
    for key in pending {
        let _ = write!(
            out,
            "<li><code>{}</code> {}</li>",
            short(&key.name),
            form(
                USER_KEYS_PATH,
                "accept_link",
                csrf,
                &format!("{}{}", hidden("key", &key.name), acts::nonce_field()),
                "Accept link",
                false
            )
        );
    }
    out.push_str("</ul>");
    out
}

/// The sign-in emails an admin added for this account that wait on its
/// acceptance, each with whether it signs in as an admin and the admin key
/// that added it, and its **Accept sign-in email** and **Remove** acts;
/// nothing where none waits. Until accepted an email signs in to nothing.
/// The accept names back the admin flag shown, so a flag changed since the
/// page was drawn is refused (ADR-0045 d2, the operator's decisions of
/// 2026-11-19 and 2026-11-20).
fn pending_emails_section(emails: &[oml_sdk::PendingEmail], csrf: &str) -> String {
    if emails.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        r#"<h2>Sign-in emails to accept</h2><p class="desc">An admin added these sign-in emails for this account. An email signs in to this account only once it is accepted here; until then it signs in to nothing. An email not asked for is removed here.</p><ul>"#,
    );
    for pending in emails {
        let email = pending.email.as_str();
        let admin = if pending.admin { "true" } else { "false" };
        let accept = format!(
            "{}{}{}",
            hidden("email", email),
            hidden("admin", admin),
            acts::nonce_field()
        );
        let remove = format!("{}{}", hidden("email", email), acts::nonce_field());
        let adder = pending
            .added_by
            .name
            .as_deref()
            .unwrap_or(pending.added_by.id.as_str());
        let _ = write!(
            out,
            "<li><code>{}</code> · {} · added by <code>{}</code> {} {}</li>",
            text(email),
            if pending.admin {
                "signs in as an admin"
            } else {
                "signs in without admin"
            },
            text(adder),
            form(
                USER_KEYS_PATH,
                "accept_email",
                csrf,
                &accept,
                "Accept sign-in email",
                false
            ),
            form(
                USER_KEYS_PATH,
                "decline_email",
                csrf,
                &remove,
                "Remove",
                false
            )
        );
    }
    out.push_str("</ul>");
    out
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
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(refusal) = admin_only(&session, &calls) {
        return refusal;
    }
    // A rotated key's value, shown here once and cleared in the same answer
    // (ADR-0096 d13); a seal that does not open is cleared unshown.
    if let Some(sealed) = cookie(&headers, ONCE_COOKIE) {
        if let Some(secret) = once::open(session.token(), ADMIN_KEYS_PATH, &sealed) {
            let (title, body) = secret_card(&secret);
            return clearing(outcome::html(
                StatusCode::OK,
                shell(
                    title,
                    &session.administration(),
                    ADMIN_KEYS_PATH,
                    None,
                    &body,
                ),
            ));
        }
        return clearing(admin_view(web, session, calls, uri).await);
    }
    admin_view(web, session, calls, uri).await
}

async fn admin_view(web: Web, session: Session, calls: Arc<PageCalls>, uri: Uri) -> Response {
    let fields = match Fields::of(&uri) {
        Ok(fields) => fields,
        Err(message) => return acts::invalid(&message, &calls),
    };
    let next = uri
        .path_and_query()
        .map_or(ADMIN_KEYS_PATH, |pq| pq.as_str());
    if let Some(key) = fields.given("key") {
        return one_key(&web, &session, &calls, &fields, key, next).await;
    }
    let user = fields.given("user");
    let mut body = format!(
        r#"<h1>Keys</h1><p class="desc">One person's agent and connector keys; open one to read its record, set its limits or deny it on a workspace. No key is issued here.</p><form class="row" method="get" action="{ADMIN_KEYS_PATH}">{}<button type="submit">Show</button></form>"#,
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
        body.push_str(&link_forms(&session.csrf(), user));
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

/// The admin's link forms for the listed person: a tenant key the person
/// holds, by name, linked so the console acts with it for them in shared
/// workspaces, or its link ended (ADR-0085 d16). The API judges the key.
/// `/web/admin/keys?key=<key>`: one key's full record through `GET
/// /v2/keys/{key}` as `<user>-admin`, with **Set limits**, **Deny on a
/// workspace** and **Cut off**; `op=limits` and `op=deny` draw their forms,
/// `step=review` the denial's confirmation (ADR-0096 d4). The read journals
/// nothing, and the page says nothing of an audit trail.
async fn one_key(
    web: &Web,
    session: &Session,
    calls: &PageCalls,
    fields: &Fields,
    key: &str,
    next: &str,
) -> Response {
    let credential = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    let record = match web.api.get_key(&call(&credential, &ids), key).await {
        Ok(record) => record,
        Err(e) => return outcome::page(&e, calls, next),
    };
    let csrf = session.csrf();
    let name = record.name.clone().unwrap_or_else(|| record.key_id.clone());
    let acting = session.whoami.user.as_ref().map_or_else(
        || "the admin key".to_owned(),
        |u| format!("{}-admin", u.name),
    );
    let enc = encode(&name);
    let mut body = format!(
        r#"<header class="oml-section-head"><div class="top"><h1>{}</h1></div><p class="desc">One key's record, read as <code>{}</code>.</p></header>"#,
        short(&name),
        short(&acting)
    );
    let revoked = record.revoked_at.is_some();
    let state = if revoked {
        "revoked"
    } else if record.expired {
        "expired"
    } else {
        "live"
    };
    let none = || "none".to_owned();
    let facts: Vec<(&str, String)> = vec![
        ("State", state_tag(state)),
        (
            "Whose",
            record
                .user
                .as_ref()
                .and_then(|u| u.name.clone())
                .map_or_else(none, |u| format!("<code>{}</code>", short(&u))),
        ),
        (
            "Role",
            format!(
                "{} · {} · {}",
                text(&record.role),
                text(&record.credential),
                text(record.used_by.as_deref().unwrap_or("agent"))
            ),
        ),
        (
            "Crew",
            record
                .crew_id
                .as_deref()
                .map_or_else(none, |c| format!("<code>{}</code>", short(c))),
        ),
        ("Principal", format!("<code>{}</code>", short(&record.principal))),
        (
            "Label",
            record.label.as_deref().map_or_else(none, text),
        ),
        (
            "Profile",
            record.profile.as_deref().map_or_else(none, text),
        ),
        ("Guardrails loaded", text(&record.guardrail_scope)),
        (
            "Default workspace",
            record
                .default_workspace_id
                .as_deref()
                .map_or_else(none, |w| format!("<code>{}</code>", short(w))),
        ),
        (
            "Expiry",
            record
                .expiry
                .as_deref()
                .map_or_else(|| "does not expire".to_owned(), |at| ends(at, record.expired)),
        ),
        (
            "Rate",
            record.requests_per_minute.map_or_else(
                || "no rate of its own; under the account's quota, the account's holds".to_owned(),
                |n| format!("{n} requests a minute; under the account's quota, the lower of the two holds"),
            ),
        ),
        (
            "Connections",
            if record.connections.is_empty() {
                none()
            } else {
                record
                    .connections
                    .iter()
                    .map(|c| text(&c.state))
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        ),
    ];
    body.push_str(r#"<table class="oml-table"><tbody>"#);
    for (label, value) in facts {
        let _ = write!(
            body,
            r#"<tr><th scope="row">{label}</th><td class="c-meta wrap prose">{value}</td></tr>"#
        );
    }
    body.push_str("</tbody></table><h2>Grants</h2>");
    body.push_str(
        r#"<table class="oml-table"><tr><th>Workspace</th><th>Grant</th><th>Covers</th></tr>"#,
    );
    for g in &record.grants {
        let _ = write!(
            body,
            r#"<tr><td class="mono">{}</td><td class="c-grant">{}</td><td>{}</td></tr>"#,
            short(&g.workspace_node_id),
            text(&g.role),
            if g.covers == "node" {
                "this workspace only"
            } else {
                "this workspace and below"
            }
        );
    }
    body.push_str("</table>");
    if !record.denials.is_empty() {
        body.push_str("<h2>Except</h2><ul>");
        for d in &record.denials {
            let _ = write!(
                body,
                r#"<li>denied on <code>{}</code></li>"#,
                short(&d.workspace_node_id)
            );
        }
        body.push_str("</ul>");
    }
    if revoked {
        body.push_str(r#"<p class="desc">A revoked key takes no act.</p>"#);
    } else {
        match fields.given("op") {
            Some("limits") => body.push_str(&limits_form(&csrf, &name)),
            Some("deny") => body.push_str(&deny_form(&csrf, &name, fields)),
            _ => {
                // A session key and an admin key take neither: the API
                // refuses limits on them (`session_key`, `admin_key`) and a
                // denial cuts off no session or admin key (ADR-0096 d4;
                // ADR-0079 d5).
                if record.credential != "session" && record.role != "admin" {
                    let _ = write!(
                        body,
                        r#"<p><a class="button primary" href="{ADMIN_KEYS_PATH}?key={enc}&amp;op=limits">Set limits</a> <a class="button" href="{ADMIN_KEYS_PATH}?key={enc}&amp;op=deny">Deny on a workspace</a></p>"#,
                        enc = attr(&enc)
                    );
                }
                // Rotate where an admin may (ADR-0096 d13, the operator's
                // decision of 2026-11-25): a live tenant bearer key that is
                // not an admin key and names no person. The API refuses a
                // key linked to a person by name, which the read serves no
                // field for.
                if record.credential == "bearer"
                    && record.role != "admin"
                    && record.user.is_none()
                    && !record.expired
                {
                    body.push_str(&form(
                        ADMIN_KEYS_PATH,
                        "rotate_key",
                        &csrf,
                        &hidden("key", &name),
                        "Rotate",
                        false,
                    ));
                }
                if record.credential != "session" && record.role != "admin" {
                    body.push_str(&form(
                        ADMIN_KEYS_PATH,
                        "cut_off",
                        &csrf,
                        &format!("{}{}", hidden("key", &name), acts::nonce_field()),
                        "Cut off",
                        false,
                    ));
                }
            }
        }
    }
    let _ = write!(
        body,
        r#"<p><a href="{ADMIN_KEYS_PATH}?user={}">Back to the keys</a></p>"#,
        attr(&encode(
            record
                .user
                .as_ref()
                .and_then(|u| u.name.as_deref())
                .unwrap_or("")
        ))
    );
    outcome::html(
        StatusCode::OK,
        shell(
            "Key",
            &session.administration(),
            ADMIN_KEYS_PATH,
            admin_notice(fields.get("done")),
            &body,
        ),
    )
}

/// The notices the one-key page's acts name.
fn admin_notice(done: Option<&str>) -> Option<&'static str> {
    match done? {
        "limits" => Some("limits set"),
        "denied" => Some("denied on the workspace"),
        other => notice(Some(other)),
    }
}

/// The limits form (ADR-0096 d4, d10): the date as the next UTC day's end,
/// or no expiry; a rate, or none of its own.
fn limits_form(csrf: &str, key: &str) -> String {
    format!(
        r#"<h2>Set limits</h2><p class="desc">An expired key moved to a later date works again. The key works through the chosen date in every time zone.</p>{}"#,
        form(
            ADMIN_KEYS_PATH,
            "set_limits",
            csrf,
            &format!(
                r#"{}<label>Expires<input type="date" name="expires"></label><label class="oml-check"><input type="checkbox" name="no_expiry" value="yes"> No expiry</label><label>Requests a minute<input name="rate" inputmode="numeric"></label><label class="oml-check"><input type="checkbox" name="no_rate" value="yes"> No rate of its own</label>{}"#,
                hidden("key", key),
                acts::nonce_field()
            ),
            "Set limits",
            true,
        )
    )
}

/// The denial's form, then with `step=review` its confirmation: one
/// workspace of the tenant and a reason (ADR-0096 d4).
fn deny_form(csrf: &str, key: &str, fields: &Fields) -> String {
    let workspace = fields.given("workspace");
    let reason = fields.given("reason");
    if let (Some("review"), Some(workspace), Some(reason)) =
        (fields.given("step"), workspace, reason)
    {
        return format!(
            r#"<section class="card"><h2>Deny {} on {}?</h2><p class="oml-consequence">The key stops working on <code>{}</code> and below at once. Its memories stay.</p><p class="desc">Reason: {}</p>{}</section>"#,
            short(key),
            short(workspace),
            short(workspace),
            text(reason),
            form(
                ADMIN_KEYS_PATH,
                "deny_on",
                csrf,
                &format!(
                    "{}{}{}{}",
                    hidden("key", key),
                    hidden("workspace", workspace),
                    hidden("reason", reason),
                    acts::nonce_field()
                ),
                "Deny",
                true,
            )
        );
    }
    format!(
        r#"<h2>Deny on a workspace</h2><form class="oml-form" method="get" action="{ADMIN_KEYS_PATH}">{}{}{}{}<button type="submit" name="step" value="review">Review</button></form>"#,
        hidden("key", key),
        hidden("op", "deny"),
        input("workspace", "Workspace", workspace.unwrap_or(""), true),
        input("reason", "Reason", reason.unwrap_or(""), true),
    )
}

fn link_forms(csrf: &str, user: &str) -> String {
    let fields = |placeholder: &str| {
        format!(
            "{}{}{}",
            input("key", placeholder, "", true),
            hidden("user", user),
            acts::nonce_field()
        )
    };
    format!(
        r#"<h2>Linked tenant key</h2><p class="desc">A live tenant key used by a person that {} works with in shared workspaces. The link waits until {} accepts it on their own Keys page; accepted, the key acts for them.</p>{}{}"#,
        short(user),
        short(user),
        form(
            ADMIN_KEYS_PATH,
            "link_key",
            csrf,
            &fields("tenant key name"),
            "Link key",
            false
        ),
        form(
            ADMIN_KEYS_PATH,
            "unlink_key",
            csrf,
            &fields("linked key name"),
            "Unlink key",
            false
        )
    )
}

/// The fields each action defines on `side`, beside `action`,
/// `csrf_token` and `nonce`: the admin's cut-off also names the listed
/// `user`.
fn defined(side: Side, action: &str, name: &str) -> bool {
    match action {
        "issue_key" | "connect_key" => matches!(name, "name" | "role" | "expires"),
        "build_key" => crate::builder::defined(name),
        "cut_off" => matches!(name, "key" | "confirm") || (side == Side::Admin && name == "user"),
        "rotate_key" => {
            matches!(name, "key" | "confirm" | "grace") || (side == Side::Admin && name == "user")
        }
        "link_key" | "unlink_key" => matches!(name, "key" | "user"),
        "set_limits" => matches!(name, "key" | "expires" | "no_expiry" | "rate" | "no_rate"),
        "deny_on" => matches!(name, "key" | "workspace" | "reason"),
        "accept_email" => matches!(name, "email" | "admin"),
        "decline_email" => name == "email",
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
        r#"<h1>Cut off {}?</h1><p class="oml-consequence">It stops working on every workspace at once. Its memories stay.</p>"#,
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

/// Where a rotation goes back to: the personal Keys page, or on the admin
/// side the listed person's keys, else the one key's page.
fn rotated_back(side: Side, key: &str, user: Option<&str>) -> String {
    match (side, user) {
        (Side::User, _) => side.path().to_owned(),
        (Side::Admin, Some(u)) => format!("{ADMIN_KEYS_PATH}?user={}", encode(u)),
        (Side::Admin, None) => format!("{ADMIN_KEYS_PATH}?key={}", encode(key)),
    }
}

/// Rotate's confirmation, which records nothing: what a rotation does, the
/// grace offered (none, one hour, 24 hours), and a form carrying
/// `confirm=yes`, the listed `user` on the admin page, and a fresh nonce,
/// so a resubmitted confirmation replays one rotation (ADR-0096 d13).
fn rotate_confirmation(session: &Session, side: Side, key: &str, user: Option<&str>) -> Response {
    let page = side.path();
    let listed = user.map_or_else(String::new, |u| hidden("user", u));
    let back = rotated_back(side, key, user);
    let mut body = format!(
        r#"<h1>Rotate {}?</h1><p class="oml-consequence">The key gets a new value, shown once. Its name, grants and limits stay. Every client that holds the previous value needs the new one.</p>"#,
        short(key)
    );
    body.push_str(&form(
        page,
        "rotate_key",
        &session.csrf(),
        &format!(
            "{}{listed}{}{}{}",
            hidden("key", key),
            select(
                "grace",
                "previous value works for",
                &[
                    ("0", "no time: it stops now"),
                    ("3600", "1 hour"),
                    ("86400", "24 hours")
                ],
            ),
            hidden("confirm", "yes"),
            acts::nonce_field()
        ),
        "Rotate it",
        true,
    ));
    let _ = write!(
        body,
        r#"<p><a href="{}">Keep the value</a></p>"#,
        attr(&back)
    );
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
        "issue_key" | "connect_key" | "build_key" => &["name"],
        "link_key" => &["key", "user"],
        "deny_on" => &["key", "workspace", "reason"],
        "accept_email" => &["email", "admin"],
        "decline_email" => &["email"],
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
    if action == "rotate_key" && fields.given("confirm") != Some("yes") {
        return rotate_confirmation(
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
        "build_key" => {
            // The builder's review posts the draft it showed; the API judges
            // it and answers any refusal the page could not (ADR-0096 d1).
            let draft = match crate::builder::of_form(&fields) {
                Ok(draft) => draft,
                Err(message) => return acts::invalid(&message, &calls),
            };
            let user = session.whoami.user.as_ref().map_or("", |u| u.name.as_str());
            match web.api.issue_key_with(&call, &key, &draft.body(user)).await {
                Ok(issued) if issued.replayed() => return acts::see_other(USER_KEYS_PATH, &[]),
                Ok(issued) => match secret_of(&web, issued).await {
                    Some(secret) => secret,
                    None => return no_secret(&calls),
                },
                // A refusal only the server decides goes back to the
                // review step with its notice, the draft kept (D12).
                Err(oml_sdk::Error::Api(e))
                    if crate::builder::server_refusal(&e.code).is_some() =>
                {
                    let mut back: Vec<String> = draft
                        .fields()
                        .into_iter()
                        .map(|(n, v)| format!("{n}={}", encode(&v)))
                        .collect();
                    back.push("step=review".to_owned());
                    back.push(format!("refused={}", encode(&e.code)));
                    return acts::see_other(
                        &format!("{}?{}", crate::keys::NEW_KEY_PATH, back.join("&")),
                        &[],
                    );
                }
                Err(e) => return outcome::page(&e, &calls, crate::keys::NEW_KEY_PATH),
            }
        }
        "issue_key" | "connect_key" => {
            // The form offers a contributor, the default, and a reader; a
            // submitter key is an act of the CLI and the admin (ADR-0093 d6).
            let role = match fields.given("role").unwrap_or("contributor") {
                role @ ("contributor" | "reader") => role,
                _ => return acts::invalid("role is contributor or reader", &calls),
            };
            let user = session.whoami.user.as_ref().map_or("", |u| u.name.as_str());
            let name = format!("{user}-{}", fields.given("name").unwrap_or_default());
            let connector = action == "connect_key";
            // The form's date is the end of that day in UTC. The API judges
            // the instant, so a value that is not `YYYY-MM-DD`, or a day that
            // has passed, is refused by it and shown as it answers; a day its
            // month does not hold is read as that many days on (ADR-0079 d1).
            let expiry = match fields.given("expires") {
                Some(day) => match expiry_sent(day) {
                    Some(at) => Some(at),
                    None => return acts::invalid("expires is a date, YYYY-MM-DD", &calls),
                },
                None => None,
            };
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
        "rotate_key" => {
            let name = fields.given("key").unwrap_or_default();
            let grace = match fields.given("grace").unwrap_or("0") {
                "0" => None,
                g @ ("3600" | "86400") => g.parse().ok(),
                _ => return acts::invalid("grace is 0, 3600 or 86400 seconds", &calls),
            };
            match web.api.rotate_key(&call, &key, name, grace).await {
                Ok(rotated) => match rotated.value {
                    Some(value) => Secret::Rotated {
                        name: name.to_owned(),
                        value,
                        server: server(&web).await,
                        previous_valid_until: rotated.previous_valid_until,
                        back: rotated_back(side, name, fields.given("user")),
                    },
                    // A replay, which shows no value: as for an issue.
                    None => return acts::see_other(page, &[]),
                },
                Err(e) => return outcome::page(&e, &calls, page),
            }
        }
        "accept_email" | "decline_email" => {
            let email = fields.given("email").unwrap_or_default();
            let (answer, done) = if action == "accept_email" {
                let admin = fields.given("admin") == Some("true");
                (
                    web.api.accept_web_email(&call, &key, email, admin).await,
                    "email_accepted",
                )
            } else {
                (
                    web.api.decline_web_email(&call, &key, email).await,
                    "email_removed",
                )
            };
            return match answer {
                Ok(_) => acts::see_other(&format!("{page}?done={done}"), &[]),
                Err(e) => outcome::page(&e, &calls, page),
            };
        }
        "accept_link" => {
            let name = fields.given("key").unwrap_or_default();
            return match web.api.accept_key_link(&call, &key, name).await {
                Ok(_) => acts::see_other(&format!("{page}?done=accepted"), &[]),
                Err(e) => outcome::page(&e, &calls, page),
            };
        }
        "set_limits" => {
            let name = fields.given("key").unwrap_or_default();
            let mut change = serde_json::Map::new();
            if fields.given("no_expiry") == Some("yes") {
                change.insert("expiry".to_owned(), serde_json::Value::Null);
            } else if let Some(day) = fields.given("expires") {
                match expiry_sent(day) {
                    Some(at) => {
                        change.insert("expiry".to_owned(), at.into());
                    }
                    None => return acts::invalid("expires is a date, YYYY-MM-DD", &calls),
                }
            }
            if fields.given("no_rate") == Some("yes") {
                change.insert("requests_per_minute".to_owned(), serde_json::Value::Null);
            } else if let Some(rate) = fields.given("rate") {
                match rate.parse::<u64>() {
                    Ok(n) => {
                        change.insert("requests_per_minute".to_owned(), n.into());
                    }
                    Err(_) => {
                        return acts::invalid(
                            "requests a minute is a whole number from 1 to 1,000,000",
                            &calls,
                        );
                    }
                }
            }
            let back = format!("{page}?key={}", encode(name));
            return match web
                .api
                .set_key_limits(&call, &key, name, &serde_json::Value::Object(change))
                .await
            {
                Ok(_) => acts::see_other(&format!("{back}&done=limits"), &[]),
                Err(e) => outcome::page(&e, &calls, &back),
            };
        }
        "deny_on" => {
            let name = fields.given("key").unwrap_or_default();
            let workspace = fields.given("workspace").unwrap_or_default();
            let reason = fields.given("reason").unwrap_or_default();
            let back = format!("{page}?key={}", encode(name));
            return match web.api.deny_key(&call, &key, name, workspace, reason).await {
                Ok(_) => acts::see_other(&format!("{back}&done=denied"), &[]),
                Err(e) => outcome::page(&e, &calls, &back),
            };
        }
        "link_key" | "unlink_key" => {
            let name = fields.given("key").unwrap_or_default();
            let user = fields.given("user");
            let linked_to = (action == "link_key").then_some(user).flatten();
            let listed = user.map_or_else(String::new, |u| format!("user={}&", encode(u)));
            let done = if linked_to.is_some() {
                "linked"
            } else {
                "unlinked"
            };
            return match web.api.link_key(&call, &key, name, linked_to).await {
                Ok(_) => acts::see_other(&format!("{page}?{listed}done={done}"), &[]),
                Err(e) => outcome::page(&e, &calls, page),
            };
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
    // The secret opens on the page that asked for it: a rotation from the
    // admin Keys page shows on that page.
    let shown_on = if matches!(secret, Secret::Rotated { .. }) {
        page
    } else {
        USER_KEYS_PATH
    };
    let sealed = once::seal(session.token(), shown_on, &secret);
    acts::see_other(shown_on, &[once::set_cookie(&sealed)])
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
    fn an_instant_sent_in_a_query_reads_back_as_the_same_unix_second() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_794_392_400), "2026-11-11T10:20:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        for unix in [-86_401, 59, 951_868_799, 1_794_392_400, 4_102_444_799] {
            assert_eq!(unix_of(&rfc3339(unix)), Some(unix), "{unix}");
        }
    }

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

    /// A contributor without recall is marked "no recall" on both sides as
    /// `GET /v2/keys` serves it: a migrated v0.8.3 write-only user key
    /// holding submitter on its root and the `universe` node reader grant is
    /// marked though it holds a reader grant; a contributor that recalls and
    /// a submitter are not (ADR-0093 d3, as read on 2026-11-20).
    #[test]
    fn a_contributor_key_served_without_recall_is_marked_no_recall() {
        let listed = |role: &str, grants: &[(&str, &str)], without_recall: bool| -> ListedKey {
            serde_json::from_value(serde_json::json!({
                "id": "k1",
                "name": "alice-ingest",
                "role": role,
                "used_by": "agent",
                "credential": "bearer",
                "revoked": false,
                "cut_off": false,
                "without_recall": without_recall,
                "grants": grants.iter().map(|(r, path)| serde_json::json!({
                    "role": r, "workspace_id": "w", "path": path,
                })).collect::<Vec<_>>(),
            }))
            .expect("a listed key")
        };
        for side in [Side::User, Side::Admin] {
            let migrated = row(
                &listed(
                    "contributor",
                    &[("submitter", "universe.alice"), ("reader", "universe")],
                    true,
                ),
                side,
                "csrf",
                "alice",
                None,
            );
            assert!(migrated.contains(NO_RECALL), "{migrated}");
            for (role, grants) in [
                (
                    "contributor",
                    &[
                        ("submitter", "universe.alice"),
                        ("reader", "universe.alice"),
                    ][..],
                ),
                ("submitter", &[("submitter", "universe.alice")][..]),
            ] {
                let shown = row(&listed(role, grants, false), side, "csrf", "alice", None);
                assert!(!shown.contains(NO_RECALL), "{role}: {shown}");
            }
        }
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
