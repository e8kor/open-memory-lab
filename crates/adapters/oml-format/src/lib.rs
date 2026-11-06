//! `oml-format` — the compiled-in adapters of the Format port (ADR-0056 d3).
//! Red stub: only prose resolves, so every format test fails until the
//! adapters land.

#![forbid(unsafe_code)]

use std::sync::Arc;

use oml_app::ports::{Formats, ProseFormats, ResolvedFormat};

/// The closed list of formats this build serves.
pub struct CompiledFormats;

impl Formats for CompiledFormats {
    fn resolve(&self, media_type: &str) -> Result<ResolvedFormat, Vec<String>> {
        ProseFormats.resolve(media_type)
    }
    fn allowed(&self) -> Vec<String> {
        ProseFormats.allowed()
    }
    fn adapters(&self) -> Vec<&'static str> {
        ProseFormats.adapters()
    }
}

/// The registry the lead and the CLI compose.
#[must_use]
pub fn compiled() -> Arc<dyn Formats> {
    Arc::new(CompiledFormats)
}
