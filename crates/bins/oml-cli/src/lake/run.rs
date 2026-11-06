//! The run of `oml sync --adapter delta` (ADR-0062 d2, d8, d11).

use std::io::Write;
use std::path::PathBuf;

use crate::client::CliError;
use crate::sync::api::SyncApi;
use crate::sync::run::Report;

/// What a lake run was asked to do.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub root: PathBuf,
    pub uri_base: Option<String>,
    pub workarea: String,
    pub dry_run: bool,
    pub check: bool,
    pub record: bool,
    pub json: bool,
    pub max_writes: usize,
    pub depth: usize,
    /// Read every table at this version (a test's lake at an earlier moment).
    pub at_version: Option<u64>,
}

/// # Errors
/// A call fails without a typed refusal.
pub fn execute(
    _api: &dyn SyncApi,
    _opts: &Options,
    _out: &mut dyn Write,
) -> Result<Report, CliError> {
    Err(CliError::local("not built"))
}
