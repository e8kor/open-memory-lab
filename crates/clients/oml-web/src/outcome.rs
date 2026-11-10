//! Layer 7 of ADR-0070 d3: the API's error envelope becomes a page outcome.
//! A pure table, so every code the pages can meet is answered in one place
//! and a code the table does not name is the generic refusal.

use http::StatusCode;

/// What a page shows for an API refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageOutcome {
    /// `access.not_found`: the not-found page.
    NotFound,
    /// `auth.invalid`: the session is gone, so the sign-in page.
    SignIn,
    /// `request.validation`: the form again, with `details.field` marked.
    FormAgain,
    /// `limit.*`: the retry page, with the API's `Retry-After`.
    Retry,
    /// `review.*` and `workarea.*`: the page itself, with the API's message.
    Message,
    /// Any other code: the generic refusal page, with the request id.
    Refused,
}

impl PageOutcome {
    /// The outcome of the envelope's `code`.
    #[must_use]
    pub fn of_code(code: &str) -> Self {
        match code {
            "access.not_found" => Self::NotFound,
            "auth.invalid" => Self::SignIn,
            "request.validation" => Self::FormAgain,
            _ if code.starts_with("limit.") => Self::Retry,
            _ if code.starts_with("review.") || code.starts_with("workarea.") => Self::Message,
            _ => Self::Refused,
        }
    }

    /// The status the page answers with. The sign-in page and a form shown
    /// again are pages the browser should render, not errors it should
    /// retry, so they answer with their own codes rather than the API's.
    #[must_use]
    pub fn status(self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::SignIn => StatusCode::UNAUTHORIZED,
            Self::FormAgain => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Retry => StatusCode::TOO_MANY_REQUESTS,
            Self::Message => StatusCode::CONFLICT,
            Self::Refused => StatusCode::FORBIDDEN,
        }
    }
}

/// The outcome of a whole envelope body, `{"error": {"code": …}}`. A body
/// that is not an envelope is the generic refusal: the page never guesses.
#[must_use]
pub fn of_envelope(body: &[u8]) -> PageOutcome {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["code"].as_str().map(PageOutcome::of_code))
        .unwrap_or(PageOutcome::Refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_code_of_the_d3_table_maps_to_its_page_outcome() {
        let table = [
            ("access.not_found", PageOutcome::NotFound),
            ("auth.invalid", PageOutcome::SignIn),
            ("request.validation", PageOutcome::FormAgain),
            ("limit.rate", PageOutcome::Retry),
            ("limit.quota", PageOutcome::Retry),
            ("review.self_review", PageOutcome::Message),
            ("workarea.not_empty", PageOutcome::Message),
            ("access.verb_ceiling", PageOutcome::Refused),
            ("something.new", PageOutcome::Refused),
        ];
        for (code, want) in table {
            assert_eq!(PageOutcome::of_code(code), want, "{code}");
        }
    }

    #[test]
    fn a_body_that_is_not_an_envelope_is_the_generic_refusal() {
        assert_eq!(of_envelope(b"<html>"), PageOutcome::Refused);
        assert_eq!(
            of_envelope(br#"{"code":"auth.invalid"}"#),
            PageOutcome::Refused
        );
        assert_eq!(
            of_envelope(br#"{"error":{"code":"auth.invalid"},"request_id":"req_1"}"#),
            PageOutcome::SignIn
        );
    }

    #[test]
    fn the_sign_in_outcome_answers_401_and_the_generic_refusal_403() {
        assert_eq!(PageOutcome::SignIn.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(PageOutcome::Refused.status(), StatusCode::FORBIDDEN);
    }
}
