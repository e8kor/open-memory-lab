//! A lake's canonical location, the anchor's `uri` (ADR-0062 d5).
//!
//! `s3://<bucket>/<path>`, `gs://<bucket>/<path>`, an `https` URL, or
//! `file:///<path>` for a lake read from a disk that names no other location.
//! The scheme and host are lower case and the path keeps its case; there is no
//! user information, `?` or `#`, no trailing `/`, at most 2,048 characters. An
//! ADLS `abfss://<container>@<account>.dfs.core.windows.net/<path>` is written
//! as `https://<account>.dfs.core.windows.net/<container>/<path>`, the same
//! object's endpoint, so `@` stays refused. A bare local path is refused: a
//! mirror is read with `--uri-base <canonical uri>`.

use unicode_normalization::UnicodeNormalization;

/// The longest `uri` the server admits.
pub const URI_MAX: usize = 2048;

/// The canonical form of `location`.
///
/// # Errors
/// Why a location is not one an anchor may carry.
pub fn canonical(location: &str) -> Result<String, String> {
    if location
        .chars()
        .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("a location has no whitespace or control character".into());
    }
    if location.contains(['?', '#']) {
        return Err("a location has no query and no fragment: ? and # are refused".into());
    }
    let Some((scheme, rest)) = location.split_once("://") else {
        return Err(format!(
            "invalid --uri-base {location:?}: use an s3://, gs://, https://, or file:// \
             location; local paths are not accepted"
        ));
    };
    let scheme = scheme.to_ascii_lowercase();
    let out = match scheme.as_str() {
        "s3" | "gs" => {
            let (bucket, path) = split_host(rest)?;
            if !dns_safe(&bucket) {
                return Err(format!(
                    "{bucket:?} is not a DNS-safe bucket name (lower case, digits, - and ., 3 to 63 characters)"
                ));
            }
            format!("{scheme}://{bucket}{path}")
        }
        "https" => {
            let (host, path) = split_host(rest)?;
            format!("https://{host}{path}")
        }
        "abfss" => {
            let (authority, path) = split_host_raw(rest);
            let Some((container, host)) = authority.split_once('@') else {
                return Err("an abfss location is abfss://<container>@<account>.dfs.core.windows.net/<path>".into());
            };
            if container.is_empty() || host.contains('@') {
                return Err("an abfss location names one container and no user information".into());
            }
            let host = host.to_ascii_lowercase();
            if !host.ends_with(".dfs.core.windows.net") {
                return Err("an abfss location is on <account>.dfs.core.windows.net".into());
            }
            format!("https://{host}/{container}{path}")
        }
        "file" => {
            if !rest.starts_with('/') {
                return Err("a file location is file:///<absolute path>, with no host".into());
            }
            format!("file://{rest}")
        }
        other => {
            return Err(format!(
                "{other}:// is not a lake location: s3, gs, abfss, https and file are"
            ));
        }
    };
    check(&out)
}

fn check(uri: &str) -> Result<String, String> {
    let nfc: String = uri.nfc().collect();
    if nfc != uri {
        return Err("a location is written in Unicode NFC".into());
    }
    if uri.len() > URI_MAX {
        return Err(format!("a location is at most {URI_MAX} characters"));
    }
    if uri.ends_with('/') && !uri.ends_with("://") {
        return Err("a location has no trailing /".into());
    }
    if uri.split("://").nth(1).is_some_and(|r| r.contains("//")) {
        return Err("a location has no empty path segment".into());
    }
    Ok(uri.to_owned())
}

/// `<host><path>` split at the first `/`, the host lower-cased and without
/// user information; the path starts with `/` or is empty.
fn split_host(rest: &str) -> Result<(String, String), String> {
    let (host, path) = split_host_raw(rest);
    if host.contains('@') {
        return Err("a location carries no user information".into());
    }
    if host.is_empty() {
        return Err("a location names a host or bucket".into());
    }
    Ok((host.to_ascii_lowercase(), path.to_owned()))
}

fn split_host_raw(rest: &str) -> (&str, &str) {
    match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    }
}

fn dns_safe(bucket: &str) -> bool {
    let b = bucket.as_bytes();
    (3..=63).contains(&b.len())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'.')
        && b[0].is_ascii_alphanumeric()
        && b[b.len() - 1].is_ascii_alphanumeric()
}

/// `base` and the table's path under it.
///
/// # Errors
/// The joined location is not canonical.
pub fn join(base: &str, rel: &str) -> Result<String, String> {
    check(&format!("{base}/{rel}"))
}
