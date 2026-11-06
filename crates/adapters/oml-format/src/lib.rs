//! `oml-format` — the compiled-in adapters of the Format port: Mermaid, JSON,
//! YAML and code snippets, with the identity adapter of prose (ADR-0056 d3).
//! Pure: no I/O, clock or randomness, so the server and the CLI compute the
//! same search text and the same diff from the same code.

#![forbid(unsafe_code)]

mod code;
mod common;
mod fragment;
mod json;
mod mermaid;
mod yaml;

use std::sync::Arc;

use oml_app::ports::{Format, Formats, IdentityFormat, ResolvedFormat};

pub use code::LANGUAGES;

pub const MERMAID: &str = "text/vnd.oml.mermaid";
pub const JSON: &str = "application/json";
pub const YAML: &str = "application/yaml";
pub const CODE: &str = "text/vnd.oml.code";

/// The closed list of formats this build serves (ADR-0056 d1).
pub struct CompiledFormats;

impl CompiledFormats {
    /// `text/vnd.oml.code; lang=<tag>` in its one canonical spelling.
    fn code(media_type: &str) -> Option<String> {
        let (name, params) = media_type.split_once(';')?;
        let lang = params.trim().strip_prefix("lang=")?.trim();
        (name.trim() == CODE && LANGUAGES.contains(&lang)).then(|| format!("{CODE}; lang={lang}"))
    }
}

impl Formats for CompiledFormats {
    fn resolve(&self, media_type: &str) -> Result<ResolvedFormat, Vec<String>> {
        let adapter: Arc<dyn Format> = match media_type {
            "text/plain" | "text/markdown" => {
                return Ok(ResolvedFormat {
                    media_type: media_type.to_owned(),
                    prose: true,
                    adapter: Arc::new(IdentityFormat),
                });
            }
            MERMAID => Arc::new(mermaid::Mermaid),
            JSON => Arc::new(json::Json),
            YAML => Arc::new(yaml::Yaml),
            other => match Self::code(other) {
                Some(canonical) => {
                    return Ok(ResolvedFormat {
                        media_type: canonical,
                        prose: false,
                        adapter: Arc::new(code::Code),
                    });
                }
                None => return Err(self.allowed()),
            },
        };
        Ok(ResolvedFormat {
            media_type: media_type.to_owned(),
            prose: false,
            adapter,
        })
    }

    fn allowed(&self) -> Vec<String> {
        let mut all: Vec<String> = ["text/plain", "text/markdown", MERMAID, JSON, YAML]
            .into_iter()
            .map(str::to_owned)
            .collect();
        all.extend(LANGUAGES.iter().map(|l| format!("{CODE}; lang={l}")));
        all
    }

    fn adapters(&self) -> Vec<&'static str> {
        vec!["identity/1", "mermaid/1", "json/1", "yaml/1", "code/1"]
    }
}

/// The registry the lead and the CLI compose.
#[must_use]
pub fn compiled() -> Arc<dyn Formats> {
    Arc::new(CompiledFormats)
}
