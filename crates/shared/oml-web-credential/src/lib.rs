//! What a web session's token derives (ADR-0070 d2, d3; ADR-0045 d8): the
//! capacity credential `oml-web` presents to the API as one of the person's
//! session keys, and the CSRF token every signed-in form carries. The auth
//! context computes the first at session open and the lead checks the
//! second; `oml-web` computes both from the cookie. One implementation, so the two processes
//! cannot derive different values, in a leaf `oml-web` may link without
//! reaching the application core (ADR-0070 d1). The web client secret's
//! rule lives here too, for the same reason: the lead and `oml-web` refuse
//! the same secrets and read a secret file the same way (ADR-0070 d2, #230),
//! and so does the key `oml-web` seals a shown-once secret under (#253).

#![forbid(unsafe_code)]

use sha2::{Digest as _, Sha256};

/// Binds a CSRF token to its purpose, so it is no other digest of the
/// session token (ADR-0045 d8).
const CSRF_PURPOSE: &[u8] = b"oml web csrf\0";

/// The CSRF token of every form of a signed-in page: the lower-case hex of
/// SHA-256 over a fixed purpose and the session token, so it needs no
/// column and is worthless without the cookie (ADR-0045 d8).
#[must_use]
pub fn csrf_token(session_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CSRF_PURPOSE);
    hasher.update(session_token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Whether `sent` is the session's CSRF token, compared in constant time.
#[must_use]
pub fn csrf_matches(session_token: &str, sent: &str) -> bool {
    constant_time_eq(csrf_token(session_token).as_bytes(), sent.as_bytes())
}

/// Binds an act's idempotency key to its purpose, so it is no other digest
/// of the session token.
const ACT_PURPOSE: &[u8] = b"oml web act\0";

/// The `Idempotency-Key` of the API call a page's form makes: the
/// lower-case hex of SHA-256 over a fixed purpose, the session token and
/// the form's random nonce, so the same form sent again replays and no
/// other session's form names the same key (ADR-0070 d3 layer 4).
#[must_use]
pub fn act_idempotency_key(session_token: &str, nonce: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ACT_PURPOSE);
    hasher.update(session_token.as_bytes());
    hasher.update(b"\0");
    hasher.update(nonce.as_bytes());
    hex::encode(hasher.finalize())
}

/// The prefix of a capacity credential, the per-session value `oml-web`
/// presents to the API as one of the person's session keys (ADR-0070 d2).
pub const CAPACITY_CREDENTIAL_PREFIX: &str = "omls_";

/// The capacity credential of one capacity (`web`, `review`, `curate` or
/// `admin`): `omls_` and the base62 of HMAC-SHA256 keyed by the session
/// token over `oml/web-capacity/v1/<capacity>` (ADR-0070 d2). It lives as
/// long as the session, and only its SHA-256 is stored.
#[must_use]
pub fn capacity_credential(session_token: &str, capacity: &str) -> String {
    let mac = hmac_sha256(
        session_token.as_bytes(),
        format!("oml/web-capacity/v1/{capacity}").as_bytes(),
    );
    format!("{CAPACITY_CREDENTIAL_PREFIX}{}", base62(&mac))
}

/// The key a shown-once secret is sealed under (ADR-0070 d3 layer 4): HMAC-
/// SHA256 keyed by the session token over `oml/web-once/v1`, as the
/// capacity credentials are derived, so any `oml-web` replica opens a seal
/// for the same session and no other session can. Only `oml-web` uses it;
/// it lives here beside the session's other derivations so that each
/// purpose string is chosen in one place.
#[must_use]
pub fn once_seal_key(session_token: &str) -> [u8; 32] {
    hmac_sha256(session_token.as_bytes(), b"oml/web-once/v1")
}

/// HMAC-SHA256 (RFC 2104) over `message` under `key`.
#[must_use]
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0_u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

/// `bytes` as a big-endian number written in base62 (`0-9A-Za-z`), with
/// no padding.
#[must_use]
pub fn base62(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut number: Vec<u8> = bytes.iter().copied().skip_while(|b| *b == 0).collect();
    let mut out = Vec::new();
    while !number.is_empty() {
        let mut remainder = 0_u32;
        let mut quotient = Vec::with_capacity(number.len());
        for b in &number {
            let acc = (remainder << 8) | u32::from(*b);
            let q = u8::try_from(acc / 62).expect("a byte's quotient fits a byte");
            remainder = acc % 62;
            if !(quotient.is_empty() && q == 0) {
                quotient.push(q);
            }
        }
        out.push(DIGITS[remainder as usize]);
        number = quotient;
    }
    if out.is_empty() {
        out.push(b'0');
    }
    out.reverse();
    String::from_utf8(out).expect("base62 digits are ASCII")
}

/// Equality of two byte strings with no early exit on the first difference.
#[must_use]
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The shortest web client secret either process accepts: 32 characters,
/// the length of a 192-bit secret in base64, chosen, not measured.
pub const CLIENT_SECRET_MIN: usize = 32;

/// The fewest distinct characters a web client secret holds, so a
/// placeholder of one repeated character or a short repeated pattern is
/// refused; 32 hex digits from a random source hold fewer with a
/// probability below one in ten million. Chosen, not measured.
pub const CLIENT_SECRET_DISTINCT_MIN: usize = 8;

/// Why `secret` is refused as a web client secret, or `None`: shorter than
/// [`CLIENT_SECRET_MIN`], fewer than [`CLIENT_SECRET_DISTINCT_MIN`] distinct
/// characters, or a character that is not visible ASCII, which an
/// `OML-Web-Client` header cannot carry whole (ADR-0070 d2, #230). The
/// reason never quotes the value.
#[must_use]
pub fn client_secret_refusal(secret: &str) -> Option<&'static str> {
    if !secret.bytes().all(|b| b.is_ascii_graphic()) {
        return Some("must be visible ASCII, with no space or line ending");
    }
    if secret.len() < CLIENT_SECRET_MIN {
        return Some("must be at least 32 characters; `openssl rand -hex 32` writes one");
    }
    let mut seen = [false; 128];
    for b in secret.bytes() {
        seen[usize::from(b)] = true;
    }
    if seen.iter().filter(|s| **s).count() < CLIENT_SECRET_DISTINCT_MIN {
        return Some("must hold at least 8 distinct characters; `openssl rand -hex 32` writes one");
    }
    None
}

/// The secret in the file at `path`, without its one trailing line ending
/// (`\n` or `\r\n`). The file is opened once and its owner and mode read
/// from that handle, so what is checked is what is read (#262). On Unix a
/// file group or others may read is refused, naming its mode, as the
/// keyring is (ADR-0023 d4), and so is a file another user owns: a secret
/// file is the process's user's, or root's, as systemd's
/// `LoadCredential=` may give it. The error names the path, never the
/// content.
///
/// # Errors
/// The file cannot be read, others can read it, or another user owns it.
pub fn read_secret_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read as _;
    let shown = path.display();
    let mut file = std::fs::File::open(path).map_err(|e| format!("cannot read {shown}: {e}"))?;
    let meta = file
        .metadata()
        .map_err(|e| format!("cannot read {shown}: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let process = rustix::process::geteuid().as_raw();
        if let Some(why) = ownership_refusal(meta.uid(), meta.mode(), process) {
            return Err(format!("{shown} {why}"));
        }
    }
    #[cfg(not(unix))]
    let _ = &meta;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| format!("cannot read {shown}: {e}"))?;
    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    Ok(text)
}

/// Why a secret file owned by `owner` with `mode` is refused to a process
/// running as `process`, or `None`: another user than the process's or
/// root owns it, or its group or others may read it (#262).
#[cfg(unix)]
fn ownership_refusal(owner: u32, mode: u32, process: u32) -> Option<String> {
    if owner != process && owner != 0 {
        return Some(format!(
            "is owned by uid {owner}, not this process's uid {process} or root; a secret file is its reader's own (chown it)"
        ));
    }
    let mode = mode & 0o777;
    if mode & 0o077 != 0 {
        return Some(format!(
            "has mode {mode:o}; a secret file is readable by its owner alone (chmod 600)"
        ));
    }
    None
}

/// The capacity credential `oml-web --doctor` presents to learn whether the
/// lead accepts its secret: well formed and derived from no session, so
/// the lead resolves it to nothing, and the lead, knowing it, neither logs
/// nor counts the probe once the secret proved `oml-web` (ADR-0070 d2,
/// #262).
pub const DOCTOR_PROBE_CREDENTIAL: &str = "omls_oml-web-doctor-probe-no-session";

#[cfg(test)]
mod tests {
    use super::*;

    /// A file under the system temporary directory with `mode`, removed on
    /// drop.
    struct TempSecret(std::path::PathBuf);

    impl TempSecret {
        fn new(name: &str, text: &str, mode: u32) -> Self {
            let path = std::env::temp_dir()
                .join(format!("oml-web-credential-{}-{name}", std::process::id()));
            std::fs::write(&path, text).expect("write the secret file");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                    .expect("set the mode");
            }
            #[cfg(not(unix))]
            let _ = mode;
            Self(path)
        }
    }

    impl Drop for TempSecret {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    const STRONG: &str = "Zq3vN8xK2mP7rT4wY9bC6dF1gH5jL0sA";

    /// The shown-once seal's key is derived beside the session's other
    /// values (ADR-0070 d3 layer 4, #253): bound to its session, and equal
    /// to none of the session's credentials, its CSRF token or an act's
    /// key, so knowing one of them opens no seal.
    #[test]
    fn a_once_seal_key_is_bound_to_its_session_and_is_no_other_derivation_of_it() {
        let key = once_seal_key("session-a");
        assert_eq!(key, once_seal_key("session-a"));
        assert_ne!(key, once_seal_key("session-b"));
        let hex = hex::encode(key);
        assert_ne!(hex, csrf_token("session-a"));
        assert_ne!(hex, act_idempotency_key("session-a", "n"));
        for capacity in ["web", "review", "curate", "admin"] {
            assert!(!capacity_credential("session-a", capacity).contains(&base62(&key)));
        }
    }

    /// Both processes refuse the same secrets (ADR-0070 d2, #230): shorter
    /// than 32 characters, fewer than 8 distinct characters, or anything
    /// but visible ASCII, which a header cannot carry whole.
    #[test]
    fn a_web_client_secret_is_refused_when_short_repetitive_or_not_visible_ascii() {
        assert_eq!(client_secret_refusal(STRONG), None);
        assert_eq!(
            client_secret_refusal(&"0123456789abcdef".repeat(4)),
            None,
            "openssl rand -hex 32"
        );
        for weak in [
            "changeme".to_owned(),
            STRONG[..31].to_owned(),
            "s".repeat(64),
            "ab".repeat(32),
            "0123456".repeat(6),
        ] {
            assert!(client_secret_refusal(&weak).is_some(), "{weak}");
        }
        for (bad, why) in [
            (format!("{STRONG} "), "a space"),
            (format!("{STRONG}\t"), "a tab"),
            (format!("{STRONG}é"), "non-ASCII"),
            (format!("{STRONG}\n"), "a line ending"),
        ] {
            assert!(client_secret_refusal(&bad).is_some(), "{why}");
        }
    }

    /// A secret file is read only when no one but its owner can read it,
    /// and one line ending is dropped, as `echo` and editors leave one
    /// (ADR-0070 d2, #230).
    #[test]
    fn a_secret_file_is_read_only_when_owner_only_and_its_line_ending_is_dropped() {
        let file = TempSecret::new("owner", &format!("{STRONG}\n"), 0o600);
        assert_eq!(read_secret_file(&file.0).as_deref(), Ok(STRONG));
        let crlf = TempSecret::new("crlf", &format!("{STRONG}\r\n"), 0o400);
        assert_eq!(read_secret_file(&crlf.0).as_deref(), Ok(STRONG));
        #[cfg(unix)]
        for mode in [0o640, 0o604, 0o644, 0o660] {
            let open = TempSecret::new(&format!("open{mode:o}"), STRONG, mode);
            let err = read_secret_file(&open.0).expect_err("readable by others");
            assert!(err.contains(&format!("{mode:o}")), "{err}");
            assert!(!err.contains(STRONG), "{err}");
        }
        let missing = std::env::temp_dir().join("oml-web-credential-no-such-file");
        assert!(read_secret_file(&missing).is_err());
    }

    /// A secret file is owned by the process's user, or by root, which can
    /// rewrite any file anyway; a file another user owns is refused, naming
    /// the owner, whatever its mode (#262).
    #[cfg(unix)]
    #[test]
    fn a_secret_file_another_user_owns_is_refused_and_the_process_or_root_may_own_it() {
        assert_eq!(ownership_refusal(1000, 0o600, 1000), None);
        assert_eq!(ownership_refusal(1000, 0o400, 1000), None);
        assert_eq!(
            ownership_refusal(0, 0o400, 1000),
            None,
            "root's, as systemd's"
        );
        let other = ownership_refusal(1001, 0o600, 1000).expect("another user's file");
        assert!(other.contains("1001"), "{other}");
        let open = ownership_refusal(1000, 0o640, 1000).expect("group-readable");
        assert!(open.contains("640"), "{open}");
    }

    /// The doctor's probe credential is a capacity credential no session
    /// derives, so the lead can tell it from a guess (#262).
    #[test]
    fn the_doctor_probe_credential_is_a_capacity_credential() {
        assert!(DOCTOR_PROBE_CREDENTIAL.starts_with(CAPACITY_CREDENTIAL_PREFIX));
        assert_ne!(DOCTOR_PROBE_CREDENTIAL, capacity_credential("", "web"));
    }

    #[test]
    fn an_acts_key_is_the_same_for_the_same_form_and_differs_by_nonce_and_session() {
        let key = act_idempotency_key("token-a", "nonce-1");
        assert_eq!(key, act_idempotency_key("token-a", "nonce-1"));
        assert_ne!(key, act_idempotency_key("token-a", "nonce-2"));
        assert_ne!(key, act_idempotency_key("token-b", "nonce-1"));
        assert_ne!(key, csrf_token("token-a"));
        assert_eq!(key.len(), 64);
    }
}
