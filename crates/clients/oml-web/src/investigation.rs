//! The console's admin investigation (ADR-0089 d10): an admin page that
//! reads claim text names the browser session's open investigation, kept in
//! the `__Host-oml_investigation` cookie; with none, or one closed since,
//! the page asks for a reason and opens one through
//! `POST /v2/admin/investigations` before it reads.

use std::sync::Arc;

use axum::extract::rejection::FormRejection;
use axum::extract::{Extension, Form, State};
use axum::response::Response;
use http::StatusCode;

use oml_page::web::{attr, hidden, shell};

use crate::pages::refused;
use crate::session::{PageCalls, Session, call};
use crate::{Capacity, Web, acts, outcome};

/// The cookie that holds the browser session's open investigation id.
pub const INVESTIGATION_COOKIE: &str = "__Host-oml_investigation";

/// The console's act that opens or closes one.
pub const INVESTIGATION_PATH: &str = "/web/admin/investigation";

/// The code the API refuses an admin read with when it names no open
/// investigation of its key.
pub const REQUIRED_CODE: &str = "admin.investigation_required";

fn set_cookie(id: &str) -> String {
    format!("{INVESTIGATION_COOKIE}={id}; Path=/; Secure; HttpOnly; SameSite=Strict")
}

fn clear_cookie() -> String {
    format!("{INVESTIGATION_COOKIE}=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0")
}

/// A return path the form may redirect to: an admin page of the console.
fn safe_next(next: Option<&str>) -> &str {
    match next {
        Some(n) if n.starts_with("/web/admin/") && !n.contains("//") && !n.contains('\\') => n,
        _ => "/web/admin/memories",
    }
}

/// The page that asks for the reason an investigation is opened under,
/// returning to `next` once it is open.
#[must_use]
pub fn reason_page(calls: &PageCalls, next: &str) -> Response {
    let Some(bar) = calls.bar() else {
        return outcome::sign_in(next);
    };
    let csrf = calls.csrf().unwrap_or_default();
    let body = format!(
        r#"<h1>Open an investigation</h1><p class="desc">Claim text is read under one reason per investigation. The reason is stored in clear, scanned, and named by every read made under it, each journalled.</p><form method="post" action="{INVESTIGATION_PATH}"><input type="hidden" name="action" value="open"><input type="hidden" name="csrf_token" value="{}">{}<label>Reason <input name="reason" required maxlength="1024"></label><button type="submit" class="primary">Open and read</button></form>"#,
        attr(csrf),
        hidden("next", safe_next(Some(next))),
    );
    let mut response = outcome::html(
        StatusCode::FORBIDDEN,
        shell("Investigation", &bar, "/web/admin/memories", None, &body),
    );
    response.headers_mut().append(
        http::header::SET_COOKIE,
        http::HeaderValue::from_str(&clear_cookie()).expect("a cookie value"),
    );
    response
}

/// The line an admin page shows above what it read: the reason, and the
/// control that closes the investigation.
#[must_use]
pub fn line(csrf: &str) -> String {
    format!(
        r#"<form class="row" method="post" action="{INVESTIGATION_PATH}"><input type="hidden" name="action" value="close"><input type="hidden" name="csrf_token" value="{}"><span class="desc">Reads on this page are journalled under the open investigation.</span><button type="submit">Close investigation</button></form>"#,
        attr(csrf)
    )
}

/// `POST /web/admin/investigation`: `open` with a reason, or `close`.
pub async fn act(
    State(web): State<Web>,
    Extension(session): Extension<Session>,
    Extension(calls): Extension<Arc<PageCalls>>,
    form: Result<Form<Vec<(String, String)>>, FormRejection>,
) -> Response {
    if !session.admin {
        return refused(
            StatusCode::NOT_FOUND,
            "access.not_found",
            "no such page here",
            &calls,
        );
    }
    let (fields, action) = match acts::read(
        form,
        &["open", "close"],
        |action, name| matches!((action, name), ("open", "reason" | "next")),
        &calls,
    ) {
        Ok(read) => read,
        Err(refusal) => return *refusal,
    };
    let admin = session.credential(Capacity::Admin);
    let ids = calls.next_ids();
    match action {
        "open" => {
            let next = safe_next(fields.get("next")).to_owned();
            let reason = fields.get("reason").unwrap_or_default();
            match web
                .api
                .open_investigation(&call(&admin, &ids), reason)
                .await
            {
                Ok(opened) => acts::see_other(&next, &[set_cookie(&opened.investigation_id)]),
                Err(e) => outcome::page(&e, &calls, &next),
            }
        }
        _ => {
            if let Some(id) = session.investigation.as_deref() {
                // A close of one the lead does not hold leaves the cookie
                // to clear alone; any other failure keeps the cookie and
                // shows the refusal, so the investigation is not taken
                // for closed (ADR-0089 d17).
                match web.api.close_investigation(&call(&admin, &ids), id).await {
                    Err(e) if close_keeps_cookie(&e) => {
                        return outcome::page(&e, &calls, "/web/admin/memories");
                    }
                    _ => {}
                }
            }
            acts::see_other("/web/admin/memories", &[clear_cookie()])
        }
    }
}

/// Whether a failed close keeps the investigation cookie: every failure
/// but an investigation the lead does not hold, which clears it alone
/// (ADR-0089 d17).
fn close_keeps_cookie(e: &oml_sdk::Error) -> bool {
    !crate::picker::is_not_found(e)
}

#[cfg(test)]
mod tests {
    use super::close_keeps_cookie;

    fn api(code: &str, status: u16) -> oml_sdk::Error {
        oml_sdk::Error::Api(Box::new(oml_sdk::ApiError {
            status,
            code: code.to_owned(),
            message: String::new(),
            details: serde_json::Value::Null,
            request_id: None,
            retry_after: None,
        }))
    }

    /// A close the lead could not run keeps the cookie, so the console does
    /// not take the investigation for closed; one the lead does not hold
    /// clears it (ADR-0089 d17).
    #[test]
    fn a_failed_close_keeps_the_cookie_unless_the_lead_holds_no_such_investigation() {
        assert!(close_keeps_cookie(&api("service.unavailable", 503)));
        assert!(close_keeps_cookie(&api("auth.invalid", 401)));
        assert!(close_keeps_cookie(&oml_sdk::Error::Decode {
            status: 502,
            why: "no envelope".to_owned(),
        }));
        assert!(!close_keeps_cookie(&api("access.not_found", 404)));
    }
}
