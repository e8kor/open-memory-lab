//! `oml workspaces export` (ADR-0084 d5, d15; #425): the server's age file
//! written to a new owner-only file. The bundle is encrypted, but its
//! recipients' custody is its only bound once written (d13), so the file is
//! its owner's alone, and an existing file is never overwritten.

use std::fs::OpenOptions;
use std::path::Path;

use serde_json::Value;

use crate::client::{CliError, Client};

/// POSTs `body` to `path` and writes the answer to `out`, created new and
/// owner-only before a byte is written. A refusal, or a failure while
/// copying, removes the file this call created. Returns the bytes written
/// and the manifest digest the server named.
///
/// # Errors
///
/// `out` exists or cannot be made owner-only (nothing is sent), or the
/// server refused or could not be read.
pub fn download(
    client: &Client,
    path: &str,
    body: &Value,
    out: &Path,
) -> Result<(u64, Option<String>), CliError> {
    let shown = out.display().to_string();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    crate::owner_only::open_options(&mut options);
    let mut file = options.open(out).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            CliError::local(format!(
                "refused: {shown} already exists; choose another --out path because existing \
                 files are never overwritten; nothing was sent"
            ))
        } else {
            CliError::local(format!("cannot create {shown}: {e}; nothing was sent"))
        }
    })?;
    if let Err(e) = crate::owner_only::restrict(&file) {
        drop(file);
        let _ = std::fs::remove_file(out);
        return Err(CliError::local(format!(
            "cannot make {shown} owner-only: {e}; nothing was sent"
        )));
    }
    match client.post_download(path, body, &mut file) {
        Ok(done) => Ok(done),
        Err(e) => {
            drop(file);
            let _ = std::fs::remove_file(out);
            Err(e)
        }
    }
}
