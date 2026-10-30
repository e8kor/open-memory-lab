//! Signing-key identity and rotation (chain-format.md, "The signing key,
//! rotation, and the verifier's key set").

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// A signing key's id IS its fingerprint: the lowercase hex SHA-256 of the
/// 32 raw public key bytes (chain-format.md).
#[must_use]
pub fn key_fingerprint(key: &VerifyingKey) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// The ASCII bytes a planned rotation's `cross_sig` covers:
/// `oml-signing-key-rotation:<new_key_id>:<new_public_key_hex>`.
#[must_use]
pub fn rotation_message(new_key: &VerifyingKey) -> Vec<u8> {
    format!(
        "oml-signing-key-rotation:{}:{}",
        key_fingerprint(new_key),
        hex::encode(new_key.as_bytes())
    )
    .into_bytes()
}

/// The old key attests the new key: `cross_sig` for a planned
/// `system.signing_key_rotated` event (event-catalog.md).
#[must_use]
pub fn cross_sign_rotation(old_key: &SigningKey, new_key: &VerifyingKey) -> Signature {
    old_key.sign(&rotation_message(new_key))
}

/// A cross-signature that does not extend trust from the old key to the new.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "cross-signature does not verify: the old key did not attest this new key (chain-format.md rotation rule)"
)]
pub struct RotationVerifyError;

/// Verifies a planned rotation's `cross_sig`: pure Ed25519 by the old key
/// over the rotation message naming the new key.
pub fn verify_rotation_cross_sig(
    old_key: &VerifyingKey,
    new_key: &VerifyingKey,
    cross_sig: &Signature,
) -> Result<(), RotationVerifyError> {
    old_key
        .verify(&rotation_message(new_key), cross_sig)
        .map_err(|_| RotationVerifyError)
}
