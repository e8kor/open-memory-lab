//! The local credentials file: endpoint + key, owner-only permissions
//! (cli/reference.md "oml context"). The PoC keeps one context, written by
//! `oml bootstrap claim`; storing the key value locally is the documented
//! convenience, and the file is 0600 like the server's keyring.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What `oml bootstrap claim` records. Every field is optional so a partial
/// or hand-edited file still resolves what it can.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Credentials {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_workarea_id: Option<String>,
}

/// `$OML_CLI_CREDENTIALS` (tests and multi-context setups) or
/// `~/.config/oml/credentials`.
pub fn path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("OML_CLI_CREDENTIALS") {
        return Ok(PathBuf::from(p));
    }
    let home = std::env::var("HOME")
        .map_err(|_| "neither OML_CLI_CREDENTIALS nor HOME is set".to_owned())?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("oml")
        .join("credentials"))
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

/// Writes the credentials with owner-only permissions, creating parents.
pub fn store(creds: &Credentials) -> Result<PathBuf, String> {
    let path = path()?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let text = toml::to_string(creds).map_err(|e| format!("serializing credentials: {e}"))?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    {
        use std::io::Write as _;
        let mut file = options
            .open(&path)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        file.write_all(text.as_bytes())
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    // A pre-existing looser file is tightened, not tolerated (the mode above
    // only applies on create).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("cannot chmod {}: {e}", path.display()))?;
    }
    Ok(path)
}
