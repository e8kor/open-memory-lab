//! `oml sync --adapter delta` (ADR-0062): a Delta Lake's tables read from
//! their `_delta_log` alone and mapped to anchored claims. Compiled only under
//! the `lake-sync` feature, which the default `oml` never enables.

pub mod extract;
pub mod plan;
pub mod read;
pub mod run;
pub mod uri;

/// The directory a `<lake uri>` argument names: a path or a `file://` URL. An
/// object-storage location is read in the next phase (ADR-0062 d21, v0.6.2).
///
/// # Errors
/// An object-storage URL, or a path that is not a directory.
pub fn local_root(source: &str) -> Result<std::path::PathBuf, String> {
    let path = if let Some(path) = source.strip_prefix("file://") {
        std::path::PathBuf::from(path)
    } else {
        if let Some((scheme, _)) = source.split_once("://") {
            return Err(format!(
                "{scheme}:// lakes are not supported yet; pass a local directory and its canonical \
                 location with --uri-base"
            ));
        }
        std::path::PathBuf::from(source)
    };
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!("{source} is not a directory"))
    }
}
