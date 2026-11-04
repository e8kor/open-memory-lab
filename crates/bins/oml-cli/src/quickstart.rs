//! `oml quickstart`'s local half (ADR-0041 d9): the compose file it writes,
//! embedded in the binary and pinned to the CLI's own version, the
//! owner-only `.env` of generated passwords beside it, the files whose
//! presence means a stack is already there, and the enrollment token read
//! from the lead's log until the designed `oml-bootstrap` service exists.

use std::fmt::Write as _;
use std::path::Path;

/// The compose file, with `@@PROJECT@@`, `@@VERSION@@` and `@@PORT@@` to
/// fill.
const COMPOSE: &str = include_str!("quickstart-compose.yaml");

/// The image's repository; the tag is the CLI's own version.
pub const IMAGE: &str = "ghcr.io/e8kor/open-memory-lab";

/// The `.env` variables, one generated password each: the cluster's
/// superuser, the owner role the lead migrates as, and the serving role it
/// serves as (ADR-0034 d2).
pub const PASSWORDS: [&str; 3] = [
    "OML_POSTGRES_PASSWORD",
    "OML_OWNER_PASSWORD",
    "OML_SERVER_PASSWORD",
];

/// What a directory holds once a quickstart wrote into it; any one present
/// refuses a second run as `quickstart.exists`.
pub const WRITTEN: [&str; 4] = ["compose.yaml", ".env", "keys", "credentials"];

/// The compose project name for the stack in `dir`, an absolute path:
/// `oml-` and the first 12 hex digits of the path's SHA-256. Compose would
/// otherwise name the project after the directory's basename, so two stacks
/// in two `oml` directories would share one project, its containers and
/// its volumes.
#[must_use]
pub fn project_name(dir: &Path) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(dir.as_os_str().as_encoded_bytes());
    format!("oml-{}", &hex::encode(digest)[..12])
}

/// The compose file of project `project` for `version`, its lead published
/// on `127.0.0.1:port`.
#[must_use]
pub fn compose(project: &str, version: &str, port: u16) -> String {
    // A Windows checkout may hold the template with CRLF endings; the file
    // written is LF on every platform.
    COMPOSE
        .replace("\r\n", "\n")
        .replace("@@PROJECT@@", project)
        .replace("@@VERSION@@", version)
        .replace("@@PORT@@", &port.to_string())
}

/// 32 hex digits from the OS random source: a password that needs no
/// quoting in a URL, in SQL or in `.env`.
#[must_use]
pub fn password() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source is available");
    hex::encode(bytes)
}

/// The `.env` text: each of [`PASSWORDS`] with a fresh [`password`].
#[must_use]
pub fn env_file() -> String {
    let mut text = String::new();
    for name in PASSWORDS {
        let _ = writeln!(text, "{name}={}", password());
    }
    text
}

/// The first of [`WRITTEN`] that `dir` holds, if any.
#[must_use]
pub fn existing(dir: &Path) -> Option<&'static str> {
    WRITTEN
        .into_iter()
        .find(|name| dir.join(name).symlink_metadata().is_ok())
}

/// The last enrollment token in `logs`: `enr_` and 48 lower-case hex
/// digits, as the lead mints it, with no hex digit on either side.
#[must_use]
pub fn enrollment_token(logs: &str) -> Option<&str> {
    const DIGITS: usize = 48;
    let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let bytes = logs.as_bytes();
    let mut found = None;
    for (at, _) in logs.match_indices("enr_") {
        let start = at + 4;
        let end = start + DIGITS;
        if end <= bytes.len()
            && bytes[start..end].iter().all(|&b| hex(b))
            && bytes.get(end).is_none_or(|&b| !b.is_ascii_alphanumeric())
            && (at == 0 || !bytes[at - 1].is_ascii_alphanumeric())
        {
            found = Some(&logs[at..end]);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compose_file_pins_the_image_to_the_given_version_and_publishes_on_loopback_alone() {
        let text = compose("oml-0123456789ab", "0.4.0", 18080);
        assert!(text.contains("\nname: oml-0123456789ab\n"), "{text}");
        assert!(text.contains(&format!("image: {IMAGE}:0.4.0\n")), "{text}");
        assert!(text.contains("\"127.0.0.1:18080:8080\""), "{text}");
        assert!(!text.contains("@@"), "every placeholder is filled");
        assert!(!text.contains("latest") && !text.contains("oml_dev_only"));
        for name in PASSWORDS {
            assert!(
                text.contains(&format!("${{{name}")),
                "{name} is read from .env"
            );
        }
    }

    #[test]
    fn two_directories_with_the_same_basename_get_distinct_compose_project_names() {
        let a = project_name(Path::new("/home/u/a/oml"));
        let b = project_name(Path::new("/home/u/b/oml"));
        assert_ne!(a, b);
        assert_eq!(a, project_name(Path::new("/home/u/a/oml")), "stable");
        for name in [&a, &b] {
            assert_eq!(name.len(), 16, "{name}");
            assert!(
                name.strip_prefix("oml-")
                    .is_some_and(|h| h.bytes().all(|c| c.is_ascii_hexdigit())),
                "{name}"
            );
        }
    }

    #[test]
    fn the_env_file_holds_a_distinct_generated_password_per_role() {
        let text = env_file();
        let values: Vec<&str> = text
            .lines()
            .map(|l| l.split_once('=').expect("k=v").1)
            .collect();
        assert_eq!(values.len(), 3);
        assert!(values.iter().all(|v| v.len() == 32));
        assert!(values[0] != values[1] && values[1] != values[2] && values[0] != values[2]);
    }

    #[test]
    fn the_enrollment_token_is_the_last_whole_one_in_the_log() {
        let first = format!("enr_{}", "a".repeat(48));
        let last = format!("enr_{}", "0".repeat(48));
        let logs = format!(
            "x | token {first}\nx | enr_{} too long\nx | enr_{} short\nx | {last}\n",
            "b".repeat(49),
            "c".repeat(47)
        );
        assert_eq!(enrollment_token(&logs), Some(last.as_str()));
        assert_eq!(enrollment_token("no token"), None);
        assert_eq!(enrollment_token(&format!("enr_{}", "A".repeat(48))), None);
    }
}
