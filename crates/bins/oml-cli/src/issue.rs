//! The key tool's local half (ADR-0028; cli/reference.md `oml keys`): the
//! reader grant the issuing verbs send beside a writer grant, the shell
//! quoting of the two export lines `oml keys issue-agent` prints, and the
//! owner-only file `--out` writes the key value to. Everything here runs on
//! the operator's machine; the server takes exactly the grants it is sent
//! (ADR-0025 d2).

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use uuid::Uuid;

/// The grants an issuing verb sends: each requested grant in order, and
/// beside every writer grant a reader grant on the same workarea, unless
/// `write_only` or the request already names that reader grant (ADR-0028
/// d2). A default of the CLI's alone; nothing here widens what the server
/// would accept.
#[must_use]
pub fn with_reader_beside_writers(
    grants: &[(Uuid, String)],
    write_only: bool,
) -> Vec<(Uuid, String)> {
    let mut sent: Vec<(Uuid, String)> = Vec::with_capacity(grants.len() * 2);
    for (node, role) in grants {
        sent.push((*node, role.clone()));
        let reads = |(n, r): &(Uuid, String)| n == node && r == "reader";
        if role == "writer" && !write_only && !grants.iter().chain(&sent).any(reads) {
            sent.push((*node, "reader".to_owned()));
        }
    }
    sent
}

/// POSIX single quotes: the text verbatim, each `'` written as `'\''`, so a
/// printed `export` line means exactly the text whatever it holds.
#[must_use]
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The file `oml keys issue-agent --out` writes the key value to: created
/// exclusively and owner-only before anything is sent, so a path that cannot
/// hold the value fails while no key exists yet, and no file is ever
/// overwritten (ADR-0028 d1; roles-and-keys.md "Key format").
#[derive(Debug)]
pub struct KeyFile {
    path: PathBuf,
    shown: String,
    file: File,
}

impl KeyFile {
    /// Creates `path` with mode 0600, refusing a path that exists, is not
    /// UTF-8, or holds a quote or a newline (the printed line reads the file
    /// back inside single quotes, so either would make it unsafe to run).
    ///
    /// # Errors
    ///
    /// The refusal, naming its reason, when the file is not created.
    pub fn create(path: &Path) -> Result<Self, String> {
        let path = std::path::absolute(path)
            .map_err(|e| format!("cannot resolve the --out path {}: {e}", path.display()))?;
        let shown = path
            .to_str()
            .ok_or_else(|| {
                format!(
                    "refused: the --out path {} is not UTF-8, so the line that reads it back \
                     cannot be printed; nothing was sent (ADR-0028 d1)",
                    path.display()
                )
            })?
            .to_owned();
        if shown.contains(['\'', '"', '\n', '\r']) {
            return Err(format!(
                "refused: the --out path {shown:?} holds a quote or a newline, so the line that \
                 reads it back would not be safe to run; nothing was sent (ADR-0028 d1)"
            ));
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                format!(
                    "refused: {shown} already exists, and --out never overwrites a file; \
                     nothing was sent (ADR-0028 d1)"
                )
            } else {
                format!("cannot create {shown}: {e}; nothing was sent")
            }
        })?;
        let key_file = Self { path, shown, file };
        // The umask can only narrow the creation mode; set it exactly.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if let Err(e) = key_file
                .file
                .set_permissions(std::fs::Permissions::from_mode(0o600))
            {
                let shown = key_file.shown.clone();
                // The refusal is the chmod failure; a leftover empty file
                // would only be reported, never retried.
                let _ = key_file.discard();
                return Err(format!(
                    "cannot make {shown} owner-only: {e}; nothing was sent"
                ));
            }
        }
        Ok(key_file)
    }

    /// The absolute path, as the printed line names it.
    #[must_use]
    pub fn shown(&self) -> &str {
        &self.shown
    }

    /// Writes the value and one newline and syncs it to disk. On failure the
    /// partial file is removed: the value is then lost, and the caller names
    /// the issued key so it can be revoked.
    ///
    /// # Errors
    ///
    /// The write or sync failure.
    pub fn write_value(mut self, value: &str) -> Result<(), String> {
        let written = self
            .file
            .write_all(format!("{value}\n").as_bytes())
            .and_then(|()| self.file.sync_all());
        if let Err(e) = written {
            drop(self.file);
            let _ = std::fs::remove_file(&self.path);
            return Err(e.to_string());
        }
        Ok(())
    }

    /// Removes the empty file after a refused issuance.
    ///
    /// # Errors
    ///
    /// The removal failure, for the caller to report beside the refusal.
    pub fn discard(self) -> Result<(), String> {
        drop(self.file);
        std::fs::remove_file(&self.path)
            .map_err(|e| format!("the empty key file {} was not removed: {e}", self.shown))
    }
}
