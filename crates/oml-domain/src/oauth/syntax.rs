//! The two pieces of URL grammar the redirect matcher, the document URL and
//! the issuer share. Each admits a subset of what a WHATWG URL parser
//! leaves unchanged, so that an admitted string is already its own
//! serialization (ADR-0031 d5, d6; `docs/operations/configuration.md`,
//! `oauth.issuer`); the one gap is an `xn--` label, below.

/// A port as written after `:`: one to five ASCII digits, no leading zero,
/// a value from 1 to 65535 (ADR-0031 d6).
pub(super) fn port(digits: &str) -> Option<u16> {
    if !(1..=5).contains(&digits.len())
        || digits.starts_with('0')
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    digits.parse::<u16>().ok()
}

/// A lower-case ASCII DNS name in the form a WHATWG parser leaves
/// unchanged: dot-separated labels of 1 to 63 of `[a-z0-9-]`, none
/// beginning or ending with `-`, at most 253 bytes, no empty label (so no
/// trailing dot), and a last label that begins with a letter, so that the
/// parser never reads the name as an IPv4 address. An internationalized
/// name is admitted only in its A-label (`xn--`) form, and that label is
/// not decoded here: one a WHATWG parser would refuse as invalid Punycode
/// passes this check, still in one lower-case spelling. IP literals are not
/// admitted.
pub(super) fn dns_host(host: &str) -> bool {
    let label = |l: &str| {
        (1..=63).contains(&l.len())
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    host.len() <= 253
        && host.split('.').all(label)
        && host
            .rsplit('.')
            .next()
            .is_some_and(|last| last.starts_with(|c: char| c.is_ascii_lowercase()))
}
