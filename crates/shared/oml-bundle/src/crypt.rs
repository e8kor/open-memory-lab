//! age encryption of the bundle to named recipients, and decryption with an
//! identity (d5).
//!
//! Recipients are age X25519 public keys (`age1…`); an identity is the
//! matching secret key (`AGE-SECRET-KEY-1…`). The server never holds an
//! identity: the CLI decrypts on the importer's machine (d5). No refusal
//! here names an identity, and nothing here logs.

use std::io::{Read, Write};
use std::str::FromStr;

use age::stream::{StreamReader, StreamWriter};

use crate::{BundleError, sha256_digest};

/// A recipient's fingerprint for the journal (`workspace.exported`, d10):
/// the `sha256:` digest of its canonical `age1…` spelling.
pub fn recipient_fingerprint(recipient: &str) -> Result<String, BundleError> {
    let parsed = parse_recipient(recipient)?;
    Ok(sha256_digest(parsed.to_string().as_bytes()))
}

fn parse_recipient(recipient: &str) -> Result<age::x25519::Recipient, BundleError> {
    age::x25519::Recipient::from_str(recipient.trim()).map_err(|_| BundleError::Unencrypted {
        detail: "a recipient is not an age X25519 public key".to_owned(),
    })
}

/// Wraps `out` so that what is written to it is encrypted to every
/// recipient. No recipient is `bundle.unencrypted` (d5). The caller must
/// call `finish` on the returned writer.
pub fn encrypting_writer<W: Write>(
    recipients: &[&str],
    out: W,
) -> Result<StreamWriter<W>, BundleError> {
    if recipients.is_empty() {
        return Err(BundleError::Unencrypted {
            detail: "an export names at least one recipient".to_owned(),
        });
    }
    let parsed = recipients
        .iter()
        .map(|r| parse_recipient(r))
        .collect::<Result<Vec<_>, _>>()?;
    let encryptor = age::Encryptor::with_recipients(
        parsed.iter().map(|r| r as &dyn age::Recipient),
    )
    .map_err(|_| BundleError::Unencrypted {
        detail: "the recipients could not be encrypted to".to_owned(),
    })?;
    encryptor.wrap_output(out).map_err(|e| BundleError::io(&e))
}

/// Encrypts `plaintext` to every recipient (d5).
pub fn encrypt(plaintext: &[u8], recipients: &[&str]) -> Result<Vec<u8>, BundleError> {
    let mut out = Vec::new();
    let mut writer = encrypting_writer(recipients, &mut out)?;
    writer
        .write_all(plaintext)
        .map_err(|e| BundleError::io(&e))?;
    writer.finish().map_err(|e| BundleError::io(&e))?;
    Ok(out)
}

/// Opens an age file with `identity` and returns a reader of its plaintext.
/// Input that is not an age file is `bundle.unencrypted`; an identity that
/// is no recipient of it, or a passphrase file, is `bundle.decrypt_failed`.
pub fn decrypting_reader<R: Read>(
    input: R,
    identity: &str,
) -> Result<StreamReader<R>, BundleError> {
    let identity = age::x25519::Identity::from_str(identity.trim()).map_err(|_| {
        BundleError::DecryptFailed {
            detail: "the identity is not an age X25519 secret key".to_owned(),
        }
    })?;
    let decryptor = age::Decryptor::new(input).map_err(|_| BundleError::Unencrypted {
        detail: "the input is not an age-encrypted bundle".to_owned(),
    })?;
    if decryptor.is_scrypt() {
        return Err(BundleError::DecryptFailed {
            detail: "a passphrase-encrypted file is not a bundle".to_owned(),
        });
    }
    decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| BundleError::DecryptFailed {
            detail: "the identity is not a recipient of this bundle".to_owned(),
        })
}

/// Decrypts a whole age file with `identity`.
pub fn decrypt(ciphertext: &[u8], identity: &str) -> Result<Vec<u8>, BundleError> {
    let mut reader = decrypting_reader(ciphertext, identity)?;
    let mut out = Vec::new();
    reader
        .read_to_end(&mut out)
        .map_err(|_| BundleError::DecryptFailed {
            detail: "the ciphertext does not authenticate".to_owned(),
        })?;
    Ok(out)
}
