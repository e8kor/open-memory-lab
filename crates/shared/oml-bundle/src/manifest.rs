//! `manifest.json`: what the bundle holds, and the signature over it (d1, d5).

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{BundleError, FORMAT, SIGNING_PREFIX, jcs};

/// Which record a bundle carries (d2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// One published release's manifest: promoted versions only.
    Release,
    /// One workspace's record: every version ever promoted, and the
    /// exporter's own held, rejected and demoted versions.
    History,
}

/// The source server and tenant, as signed (d5: verification checks the
/// tenant, since the journal key is deployment-wide).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub server_id: String,
    pub tenant_id: String,
    pub tenant_name: String,
}

/// A release by label and the digest of its manifest (ADR-0085 d1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRef {
    pub label: String,
    pub manifest_digest: String,
}

/// One file of the bundle after the manifest, in layout order: its size, its
/// line count for a JSON Lines file (null for a body), and its digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub path: String,
    pub bytes: u64,
    pub lines: Option<u64>,
    pub sha256: String,
}

/// The manifest's signature: Ed25519 over `oml-bundle/1\n` and the RFC 8785
/// form of the manifest without its `signature` member (d5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSignature {
    /// Always `ed25519`.
    pub alg: String,
    /// Lowercase hex SHA-256 of the 32-byte public key, the fingerprint
    /// `--expect-key` names.
    pub key_fingerprint: String,
    /// The public key, lowercase hex.
    pub public_key: String,
    /// `ed25519:` and 128 lowercase hex characters, as the chain spells it.
    pub sig: String,
}

/// The bundle manifest (d1). Every member is always present; an absent
/// value is `null` or empty (reading R4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub source: Source,
    /// The exported workspace paths, the subtree's roots.
    pub workspaces: Vec<String>,
    pub mode: Mode,
    /// In `release` mode, the release exported.
    pub release: Option<ReleaseRef>,
    /// In `release` mode with `--since`, the base release (d8).
    pub since: Option<ReleaseRef>,
    /// With `since`, the claim keys the exported release dropped (d8).
    pub dropped: Vec<String>,
    /// The export instant, in the chain's `at` form.
    pub exported_at: String,
    /// The exporting principal's pseudonym in this bundle (d4): the only
    /// author whose versions carry their source event (R1, 2026-11-17).
    pub exporter_pseudonym: String,
    /// The journal public keys, lowercase hex, whose signatures the carried
    /// events bear (reading R4: a chain may span a key rotation).
    pub event_keys: Vec<String>,
    /// Every file after the manifest, in layout order.
    pub files: Vec<FileEntry>,
    pub signature: Option<ManifestSignature>,
}

impl Manifest {
    /// The mode's rules on `release`, `since` and `dropped` (d2, d8).
    pub(crate) fn check_mode(&self) -> Result<(), BundleError> {
        let bad = |reason: &str| Err(BundleError::malformed(crate::MANIFEST, 1, reason));
        match self.mode {
            Mode::Release if self.release.is_none() => bad("release mode names no release"),
            Mode::History if self.release.is_some() || self.since.is_some() => {
                bad("history mode names a release")
            }
            _ if self.since.is_none() && !self.dropped.is_empty() => {
                bad("dropped keys without since")
            }
            _ => Ok(()),
        }
    }
}

/// Lowercase hex SHA-256 of a public key's 32 bytes: the journal key's
/// fingerprint as the server and `oml audit verify-chain` print it.
#[must_use]
pub fn key_fingerprint(key: &VerifyingKey) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// The bytes the manifest's signature covers: the domain prefix, then the
/// RFC 8785 form of the manifest without its `signature` member (d5).
pub fn manifest_signed_bytes(manifest: &Manifest) -> Result<Vec<u8>, BundleError> {
    let mut value = serde_json::to_value(manifest)
        .map_err(|_| BundleError::malformed(crate::MANIFEST, 1, "manifest does not serialise"))?;
    if let Value::Object(map) = &mut value {
        map.remove("signature");
    }
    let body = jcs::canonical_bytes(&value)
        .map_err(|reason| BundleError::malformed(crate::MANIFEST, 1, reason))?;
    let mut out = SIGNING_PREFIX.to_vec();
    out.extend_from_slice(&body);
    Ok(out)
}

/// Signs `manifest` with the caller's journal key and returns it with its
/// signature. The key is handed in; this crate reads no key file.
pub fn sign_manifest(manifest: Manifest, key: &SigningKey) -> Result<Manifest, BundleError> {
    sign_manifest_with(manifest, &key.verifying_key(), &|bytes| key.sign(bytes))
}

/// Signs `manifest` through a signer the caller holds, such as the server's
/// signer port, so that the key never leaves it. `sign` receives the
/// prefixed signed bytes and returns pure Ed25519 over them.
pub fn sign_manifest_with(
    mut manifest: Manifest,
    public: &VerifyingKey,
    sign: &dyn Fn(&[u8]) -> Signature,
) -> Result<Manifest, BundleError> {
    manifest.signature = None;
    let bytes = manifest_signed_bytes(&manifest)?;
    let sig = sign(&bytes);
    manifest.signature = Some(ManifestSignature {
        alg: "ed25519".to_owned(),
        key_fingerprint: key_fingerprint(public),
        public_key: hex::encode(public.as_bytes()),
        sig: format!("ed25519:{}", hex::encode(sig.to_bytes())),
    });
    Ok(manifest)
}

/// Verifies the manifest's signature and returns the signer's fingerprint.
/// With `expect_key`, a manifest signed by any other key is
/// `bundle.signature_unexpected` (d5); without it the signature proves
/// integrity only.
pub fn verify_manifest(
    manifest: &Manifest,
    expect_key: Option<&str>,
) -> Result<String, BundleError> {
    if manifest.format != FORMAT {
        return Err(BundleError::FormatUnknown {
            found: manifest.format.clone(),
        });
    }
    let sig = manifest
        .signature
        .as_ref()
        .ok_or_else(|| BundleError::signature("the manifest carries no signature"))?;
    if sig.alg != "ed25519" {
        return Err(BundleError::signature(
            "the signature algorithm is not ed25519",
        ));
    }
    let public = parse_public_key(&sig.public_key)
        .ok_or_else(|| BundleError::signature("the public key is not 32 bytes of hex"))?;
    let fingerprint = key_fingerprint(&public);
    if fingerprint != sig.key_fingerprint {
        return Err(BundleError::signature(
            "the key fingerprint does not name the public key",
        ));
    }
    let signature = parse_signature(&sig.sig)
        .ok_or_else(|| BundleError::signature("the signature is not ed25519: and 128 hex"))?;
    let bytes = manifest_signed_bytes(manifest)?;
    public
        .verify(&bytes, &signature)
        .map_err(|_| BundleError::signature("the manifest signature does not verify"))?;
    if let Some(expected) = expect_key {
        let expected = expected.trim().to_ascii_lowercase();
        if expected != fingerprint {
            return Err(BundleError::SignatureUnexpected {
                found: fingerprint,
                expected,
            });
        }
    }
    Ok(fingerprint)
}

pub(crate) fn parse_public_key(hex_key: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = hex::decode(hex_key).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

pub(crate) fn parse_signature(text: &str) -> Option<Signature> {
    let bytes: [u8; 64] = hex::decode(text.strip_prefix("ed25519:")?)
        .ok()?
        .try_into()
        .ok()?;
    Some(Signature::from_bytes(&bytes))
}
