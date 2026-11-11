//! The Atlas panel's workarea dropdown (ADR-0045 d28): one `select` of
//! exactly the acts the acting keys may take there now, computed by the
//! verbs' own checks. Apply is a `GET` of the Workareas view, which
//! `oml-web` serves at its own path and where the act is confirmed and run
//! (ADR-0070 d7 (b)); the lead renders neither the view nor its acts.

use std::fmt::Write as _;

use uuid::Uuid;

use oml_app::app::{WorkareaOffer, WorkareaOffers};
use oml_page::web::{ADMIN_WORKAREAS_PATH, USER_WORKAREAS_PATH};

use super::page::{attr, text};

/// The value an option carries.
#[must_use]
pub(crate) fn offer_value(offer: &WorkareaOffer) -> String {
    match offer {
        WorkareaOffer::Propose(to) => format!("propose:{}", to.as_str()),
        WorkareaOffer::Ratify { transition_id, .. } => format!("ratify:{transition_id}"),
        WorkareaOffer::InProgress => "mode:in_progress".to_owned(),
        WorkareaOffer::Commit => "mode:standard".to_owned(),
    }
}

pub(crate) fn offer_text(offer: &WorkareaOffer) -> String {
    match offer {
        WorkareaOffer::Propose(to) => format!("propose {}", to.as_str()),
        WorkareaOffer::Ratify { from, to, .. } => {
            format!("ratify {} → {}", from.as_str(), to.as_str())
        }
        WorkareaOffer::InProgress => "put in progress".to_owned(),
        WorkareaOffer::Commit => "commit".to_owned(),
    }
}

/// A workarea's dropdown, `base` the page it is on (`/web` or
/// `/web/admin`): its offered acts, a withheld commit shown
/// disabled with its count, and Apply only where something is offered.
pub(crate) fn dropdown(base: &str, node: Uuid, offers: &WorkareaOffers) -> String {
    if offers.offers.is_empty() && offers.commit_withheld.is_none() {
        return r#"<span class="dim">nothing to do here now</span>"#.to_owned();
    }
    let action = if base == "/web/admin" {
        ADMIN_WORKAREAS_PATH
    } else {
        USER_WORKAREAS_PATH
    };
    let mut out = format!(
        r#"<form class="row act" method="get" action="{action}"><input type="hidden" name="workarea" value="{node}"><label>act<select name="op">"#
    );
    for offer in &offers.offers {
        let _ = write!(
            out,
            r#"<option value="{}">{}</option>"#,
            attr(&offer_value(offer)),
            text(&offer_text(offer))
        );
    }
    if let Some(held) = offers.commit_withheld {
        let _ = write!(
            out,
            r#"<option disabled selected>commit — {held} {} still held</option>"#,
            if held == 1 { "memory" } else { "memories" }
        );
    }
    out.push_str("</select></label>");
    if !offers.offers.is_empty() {
        out.push_str(r#"<button type="submit">Apply</button>"#);
    }
    out.push_str("</form>");
    out
}
