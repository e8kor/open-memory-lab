//! The key tool's local half (ADR-0028; cli/reference.md `oml keys`): the
//! reader grant the issuing verbs send beside a writer grant, the shell
//! quoting of the two export lines `oml keys issue-agent` prints, and the
//! owner-only file `--out` writes the key value to. Everything here runs on
//! the operator's machine; the server takes exactly the grants it is sent
//! (ADR-0025 d2).

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// The grants an issuing verb sends: each requested grant in order, and
/// beside every writer grant a reader grant on the same workarea, unless
/// `write_only` or the request already names that reader grant (ADR-0028
/// d2). A default of the CLI's alone; nothing here widens what the server
/// would accept. No issuing verb takes a per-grant deny mask yet, so neither
/// grant carries one; a verb that takes one copies the writer grant's masks
/// onto its reader grant (ADR-0028 d2; cli/reference.md `oml keys`).
///
/// Each workarea is the id or the name the operator typed, and the added
/// reader grant spells it the same way. The CLI resolves no name (ADR-0026
/// d4), so "already names that reader grant" compares spellings: a reader
/// grant given by a workarea's id does not count for a writer grant given
/// by its name.
#[must_use]
pub fn with_reader_beside_writers(
    grants: &[(String, String)],
    write_only: bool,
) -> Vec<(String, String)> {
    let mut sent: Vec<(String, String)> = Vec::with_capacity(grants.len() * 2);
    for (node, role) in grants {
        sent.push((node.clone(), role.clone()));
        let reads = |(n, r): &(String, String)| n == node && r == "reader";
        if role == "writer" && !write_only && !grants.iter().chain(&sent).any(reads) {
            sent.push((node.clone(), "reader".to_owned()));
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
    /// Creates `path` owner-only ([`crate::owner_only`]), refusing a path that exists, is not
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
        crate::owner_only::open_options(&mut options);
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
        // The umask can only narrow the creation mode, and on Windows the
        // file was created with the DACL it inherited: set it exactly.
        if let Err(e) = crate::owner_only::restrict(&key_file.file) {
            let shown = key_file.shown.clone();
            // The refusal is the restriction's failure; a leftover empty
            // file would only be reported, never retried.
            let _ = key_file.discard();
            return Err(format!(
                "cannot make {shown} owner-only: {e}; nothing was sent"
            ));
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::KeyFile;

    /// An empty directory of this test's own, under the OS temp directory.
    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oml-cli-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_key_file_is_owner_only_before_its_value_is_written() {
        let dir = scratch("key-file-owner-only");
        let path = dir.join("crew-agent-7");
        let key_file = KeyFile::create(&path).expect("created");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path)
                .expect("exists")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "owner-only while still empty");
        }
        #[cfg(windows)]
        assert_eq!(
            crate::owner_only::dacl_of(&path).expect("its DACL reads back"),
            crate::owner_only::Dacl::owner_only(),
            "one protected entry, the current user's, while still empty"
        );
        key_file.write_value("oml_value").expect("written");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(text, "oml_value\n");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }

    #[test]
    fn a_key_file_never_overwrites_a_file_that_exists() {
        let dir = scratch("key-file-exists");
        let path = dir.join("taken");
        std::fs::write(&path, "kept\n").expect("an existing file");
        let refusal = KeyFile::create(&path).expect_err("refused");
        assert!(refusal.contains("already exists"), "{refusal}");
        let text = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(text, "kept\n", "the existing file is untouched");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }

    #[cfg(windows)]
    #[test]
    fn no_other_open_of_a_key_file_succeeds_while_the_cli_holds_it() {
        let dir = scratch("key-file-unshared");
        let path = dir.join("crew-agent-7");
        let key_file = KeyFile::create(&path).expect("created");
        let refused = std::fs::File::open(&path).expect_err("a second open is refused");
        assert_eq!(
            refused.raw_os_error(),
            Some(
                i32::try_from(windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION)
                    .expect("fits")
            ),
            "{refused}"
        );
        key_file.discard().expect("discarded");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }
}
