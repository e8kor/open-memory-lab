//! The `rust` adapter (ADR-0061): one claim per crate and one per module
//! doc of a Cargo workspace.

use super::adapter::{Adapter, Extraction, SetExtraction};

pub struct RustAdapter;

impl Adapter for RustAdapter {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn accepts(&self, path: &str) -> bool {
        path.ends_with(".rs") || path.rsplit('/').next() == Some("Cargo.toml")
    }

    fn extract(&self, path: &str, _bytes: &[u8]) -> Result<Extraction, String> {
        Err(format!("{path}: the rust adapter reads a set of files"))
    }

    fn extract_set(&self, _files: &[(String, Vec<u8>)]) -> SetExtraction {
        SetExtraction::default()
    }
}
