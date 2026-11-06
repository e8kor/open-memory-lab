//! The typed error envelope of docs/api/errors.md: every non-2xx response is
//! `{error: {code, message, rule, details?, retryable}, request_id}`, codes
//! from the closed taxonomy, `rule` stable per code.

