//! The local credentials file: the one URL-and-key pair the CLI starts from
//! when no flag and no environment variable sets either (ADR-0039 d15;
//! cli/reference.md "Start settings"). `oml login` and `oml bootstrap claim`
//! write `url` and `api_key` and nothing else; storing the key value locally is the
//! documented convenience, and the file is owner-only like the server's
//! keyring: 0600 on Unix, one-entry protected DACL on Windows
//! ([`crate::owner_only`]).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The stored pair. Either field may be absent in a hand-edited file; the
/// caller refuses a file that sets one alone. A v0.3.0 file's `server`,
/// `key`, `key_id`, `tenant_id` and `root_workarea_id` are not read: its
/// `server` and `key` were read through v0.5.0, and v0.5.1 removed them
/// (ADR-0039 d12, d15).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Credentials {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// `$OML_CLI_CREDENTIALS` (tests and multi-context setups) or
/// `.config/oml/credentials` under the home directory [`default_path`]
/// names.
pub fn path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("OML_CLI_CREDENTIALS") {
        return Ok(PathBuf::from(p));
    }
    default_path(
        std::env::var("HOME").ok(),
        std::env::var("USERPROFILE").ok(),
    )
    .ok_or_else(|| {
        if cfg!(windows) {
            "none of OML_CLI_CREDENTIALS, HOME and USERPROFILE is set".to_owned()
        } else {
            "neither OML_CLI_CREDENTIALS nor HOME is set".to_owned()
        }
    })
}

/// `.config/oml/credentials` under `home` (`$HOME`); on Windows, where
/// `HOME` is usually unset, under `profile` (`%USERPROFILE%`), the user's
/// profile directory (cli/reference.md "Owner-only files").
#[must_use]
pub fn default_path(home: Option<String>, profile: Option<String>) -> Option<PathBuf> {
    let base = home.or(if cfg!(windows) { profile } else { None })?;
    Some(
        PathBuf::from(base)
            .join(".config")
            .join("oml")
            .join("credentials"),
    )
}

/// Loads the file when present and parseable; anything else is an empty
/// context (the caller's flags and environment still apply).
#[must_use]
pub fn load() -> Credentials {
    let Ok(path) = path() else {
        return Credentials::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Credentials::default();
    };
    toml::from_str(&text).unwrap_or_default()
}

/// Writes the credentials owner-only, creating parents.
pub fn store(creds: &Credentials) -> Result<PathBuf, String> {
    let path = path()?;
    store_at(&path, creds)?;
    Ok(path)
}

/// Writes the credentials to `path`, made owner-only before a byte is
/// written ([`crate::owner_only`]). A pre-existing looser file is
/// tightened, not tolerated: the creation mode applies only on create, so
/// the file is restricted on every write.
fn store_at(path: &Path, creds: &Credentials) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let text = toml::to_string(creds).map_err(|e| format!("serializing credentials: {e}"))?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true);
    crate::owner_only::open_options(&mut options);
    let mut file = options
        .open(path)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    crate::owner_only::restrict(&file)
        .map_err(|e| format!("cannot make {} owner-only: {e}", path.display()))?;
    {
        use std::io::Write as _;
        // Truncated only once restricted, so an older value is never left
        // readable under a looser mode while the new one is written.
        file.set_len(0)
            .and_then(|()| file.write_all(text.as_bytes()))
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Credentials, default_path, store_at};

    #[test]
    fn stored_credentials_are_owner_only_and_replace_what_was_there() {
        let dir = std::env::temp_dir().join(format!("oml-cli-creds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("oml").join("credentials");
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a scratch directory");
        std::fs::write(
            &path,
            "server = \"https://old.example.com\"\nkey = \"oml_old_and_longer\"\n",
        )
        .expect("an older file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .expect("a looser mode");
        }
        let creds = Credentials {
            url: Some("https://oml.example.com".to_owned()),
            api_key: Some("oml_new".to_owned()),
        };
        store_at(&path, &creds).expect("stored");
        let text = std::fs::read_to_string(&path).expect("read back");
        let read: Credentials = toml::from_str(&text).expect("parses");
        assert_eq!(read.url.as_deref(), Some("https://oml.example.com"));
        assert_eq!(read.api_key.as_deref(), Some("oml_new"));
        assert!(
            !text.contains("old"),
            "nothing of the older file is left: {text}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path)
                .expect("exists")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "tightened, not tolerated");
        }
        #[cfg(windows)]
        assert_eq!(
            crate::owner_only::dacl_of(&path).expect("its DACL reads back"),
            crate::owner_only::Dacl::owner_only()
        );
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }

    #[test]
    fn a_v0_3_0_credentials_files_server_and_key_are_not_read() {
        // Read through v0.5.0 and removed in v0.5.1 with the other v0.3.0
        // spellings: the file still parses, and sets no pair, so the CLI
        // asks for `oml login` (ADR-0039 d12, d15).
        let v0_3_0 = "server = \"https://oml.example.com\"\nkey = \"oml_admin\"\n\
                      key_id = \"0b7e9d5c-0000-4000-8000-000000000001\"\n\
                      tenant_id = \"0b7e9d5c-0000-4000-8000-000000000002\"\n\
                      root_workarea_id = \"0b7e9d5c-0000-4000-8000-000000000003\"\n";
        let read: Credentials = toml::from_str(v0_3_0).expect("a v0.3.0 file parses");
        assert_eq!(read.url, None);
        assert_eq!(read.api_key, None);
    }

    #[test]
    fn the_credentials_are_under_home_and_on_windows_under_the_profile_when_home_is_unset() {
        let under = |home: &str| {
            Some(
                Path::new(home)
                    .join(".config")
                    .join("oml")
                    .join("credentials"),
            )
        };
        assert_eq!(
            default_path(Some("/home/op".to_owned()), None),
            under("/home/op")
        );
        assert_eq!(
            default_path(
                Some("/home/op".to_owned()),
                Some("C:\\Users\\op".to_owned())
            ),
            under("/home/op"),
            "HOME, when set, wins on every platform"
        );
        let profile: Option<PathBuf> = default_path(None, Some("C:\\Users\\op".to_owned()));
        if cfg!(windows) {
            assert_eq!(profile, under("C:\\Users\\op"));
        } else {
            assert_eq!(profile, None, "USERPROFILE is a Windows variable alone");
        }
        assert_eq!(default_path(None, None), None);
    }
}
