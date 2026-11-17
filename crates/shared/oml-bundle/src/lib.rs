//! The workspace bundle, format `oml-bundle/1` (ADR-0084).
//!
//! A bundle is a ustar tar with a fixed layout (d1), its manifest signed by
//! the source's journal key under the domain prefix `oml-bundle/1\n` (d5),
//! encrypted with age to one or more named recipients (d5). This crate:
//!
//! - writes a bundle from contents a caller assembled ([`write_bundle`]);
//! - reads and verifies one, failing closed with ADR-0084 d7's codes
//!   ([`read_bundle`]);
//! - encrypts to recipients and decrypts with an identity ([`crypt`]);
//! - plans an import against a caller's view of the target ([`plan`]).
//!
//! It reads no key file, no clock and no environment, and logs nothing: the
//! signing key, the salt and the identity are handed in by the caller.
//! The readings ADR-0084 left open are its notes of 2026-11-17.

mod archive;
pub mod crypt;
mod error;
mod jcs;
mod lines;
mod manifest;
pub mod plan;

pub use archive::{
    BundleContents, Limits, VerifiedBundle, VerifyOptions, read_bundle, write_bundle,
    write_bundle_with,
};
pub use error::BundleError;
pub use lines::{
    Attachment, Author, EdgeKind, EdgeLine, ErasureMarker, EventLine, State, Verdict, VerdictLine,
    VersionEntry, VersionLine, WorkspaceLine, pseudonym,
};
pub use manifest::{
    FileEntry, Manifest, ManifestSignature, Mode, ReleaseRef, Source, is_chain_instant,
    key_fingerprint, manifest_signed_bytes, sign_manifest, sign_manifest_with, verify_manifest,
};

/// The one format this reader knows (d1).
pub const FORMAT: &str = "oml-bundle/1";

/// The domain prefix of the manifest's signed bytes (d5): a manifest
/// signature can never be replayed as a journal event's, or the reverse.
pub const SIGNING_PREFIX: &[u8] = b"oml-bundle/1\n";

/// The manifest's entry name; always the first entry (d1).
pub const MANIFEST: &str = "manifest.json";

/// The JSON Lines files, in their fixed order after the manifest (d1). Each
/// is present even when empty (reading R3).
pub const LINE_FILES: [&str; 5] = [
    "workspaces.jsonl",
    "versions.jsonl",
    "verdicts.jsonl",
    "events.jsonl",
    "edges.jsonl",
];

/// The directory of bodies and attachments, named by plaintext digest (d1).
pub const BODIES_DIR: &str = "bodies/";

/// The longest line a reader takes: twice the 1 MiB field bound (d7).
pub const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;

/// The deepest JSON nesting a reader takes (d7).
pub const MAX_DEPTH: usize = 32;

/// A version's text up to this size rides inline; a larger one is a body
/// under `bodies/` (d1).
pub const INLINE_TEXT_MAX: usize = 64 * 1024;

/// The `sha256:<64 hex>` digest of `bytes`, the spelling the API serves.
#[must_use]
pub fn sha256_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// The idempotency key of an imported version (d8):
/// `import:<target workspace id>:<source server id>:<source tenant id>:<artifact id>:<version>`.
#[must_use]
pub fn import_idempotency_key(
    target_workspace_id: &str,
    source: &Source,
    artifact_id: &str,
    version: u64,
) -> String {
    format!(
        "import:{target_workspace_id}:{}:{}:{artifact_id}:{version}",
        source.server_id, source.tenant_id
    )
}
