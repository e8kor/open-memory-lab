//! `oml workspaces import` (ADR-0084 d5, d7, d15; #426): the bundle is
//! decrypted on the importer's machine, twice, so no plaintext touches the
//! disk. Pass 1 decrypts, verifies the bundle as the server will (d7) and
//! measures it; pass 2 decrypts again and sends it as the fixed parts of
//! one upload session (ADR-0004 d3). The identity is read from a file and
//! never printed.

use std::io::{self, Read};

use sha2::{Digest, Sha256};

use oml_bundle::{BundleError, VerifyOptions};

use crate::client::CliError;

/// The part size of an upload session (ADR-0004 d3): every part is this
/// long but the last.
pub const PART_SIZE: usize = 8 * 1024 * 1024;

/// What pass 1 learned: the plaintext's length and SHA-256 (64 lowercase
/// hex digits, as `POST /v2/uploads` declares it), and the signer's
/// fingerprint, for the message a first import without `--expect-key`
/// prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub size: u64,
    pub sha256: String,
    pub signer_fingerprint: String,
    pub authenticated: bool,
}

/// The identity in an age identity file: its first `AGE-SECRET-KEY-1…`
/// line; comment lines, as `age-keygen` writes them, are skipped. The
/// refusal never quotes the file.
///
/// # Errors
///
/// A local failure when the file holds no identity line.
pub fn identity_of(file_text: &str) -> Result<String, CliError> {
    file_text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("AGE-SECRET-KEY-1"))
        .map(str::to_owned)
        .ok_or_else(|| {
            CliError::local("the identity file holds no age secret key (AGE-SECRET-KEY-1…)")
        })
}

/// A reader that hashes and counts what passes through it.
struct Measured<R> {
    inner: R,
    hash: Sha256,
    size: u64,
}

impl<R: Read> Read for Measured<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.hash.update(&buf[..n]);
        self.size += n as u64;
        Ok(n)
    }
}

/// Pass 1: decrypts `sealed` with `identity`, verifies the plaintext under
/// `options` exactly as the server will (d7), and measures it. Nothing is
/// written anywhere.
///
/// # Errors
///
/// The bundle's refusal, under its code.
pub fn verify_pass(
    sealed: impl Read,
    identity: &str,
    options: &VerifyOptions,
) -> Result<Verified, BundleError> {
    let plain = oml_bundle::crypt::decrypting_reader(sealed, identity)?;
    let mut measured = Measured {
        inner: plain,
        hash: Sha256::new(),
        size: 0,
    };
    let verified = oml_bundle::read_bundle(&mut measured, options)?;
    // The tar's end padding is part of what the upload declares.
    io::copy(&mut measured, &mut io::sink()).map_err(|_| BundleError::DecryptFailed {
        detail: "the ciphertext does not authenticate".to_owned(),
    })?;
    Ok(Verified {
        size: measured.size,
        sha256: hex::encode(measured.hash.finalize()),
        signer_fingerprint: verified.signer_fingerprint,
        authenticated: verified.authenticated,
    })
}

/// Where pass 2 hands each part: its number, counted from 1, and its
/// bytes.
pub type SendPart<'a> = dyn FnMut(u64, &[u8]) -> Result<(), CliError> + 'a;

/// Pass 2: decrypts `sealed` again and hands each part, counted from 1, to
/// `send`, `part_size` bytes each but the last. Returns the bytes sent and
/// their SHA-256, which the caller compares with pass 1's, so a file that
/// changed between the passes is never completed.
///
/// # Errors
///
/// A decryption failure, or the first error `send` returns.
pub fn upload_pass(
    sealed: impl Read,
    identity: &str,
    part_size: usize,
    send: &mut SendPart<'_>,
) -> Result<(u64, String), CliError> {
    let mut plain = oml_bundle::crypt::decrypting_reader(sealed, identity)
        .map_err(|e| CliError::local(e.to_string()))?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buf = vec![0_u8; part_size];
    let mut n = 0_u64;
    loop {
        let mut filled = 0;
        while filled < part_size {
            let read = plain.read(&mut buf[filled..]).map_err(|_| {
                CliError::local("bundle.decrypt_failed: the ciphertext does not authenticate")
            })?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        if filled == 0 {
            break;
        }
        n += 1;
        hash.update(&buf[..filled]);
        size += filled as u64;
        send(n, &buf[..filled])?;
        if filled < part_size {
            break;
        }
    }
    Ok((size, hex::encode(hash.finalize())))
}

/// The line a refused import prints. A first import without
/// `--expect-key` names the flag and the fingerprint pass 1 read, to be
/// checked out of band before it is pinned (ADR-0084 R11).
#[must_use]
pub fn refusal_hint(code: &str, field: Option<&str>, signer_fingerprint: &str) -> Option<String> {
    match (code, field) {
        ("request.validation", Some("/expect_key")) => Some(format!(
            "a first import from this source pins its key: check the source's key fingerprint out of band, then run again with --expect-key {signer_fingerprint} if it matches"
        )),
        ("import.source_key_changed", _) => Some(
            "the target pins another key for this source; import into a new workspace if the source rotated its key".to_owned(),
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../shared/oml-bundle/tests/vectors/"
    );

    fn vector(name: &str) -> Vec<u8> {
        std::fs::read(format!("{VECTORS}{name}")).expect("a committed vector")
    }

    /// The committed TEST-ONLY identity, as the bundle crate's vectors spell
    /// it (never a real key).
    fn test_identity() -> String {
        let hex_bytes = String::from_utf8(vector("TEST-ONLY-age-identity.hex")).expect("text");
        let bytes = hex::decode(hex_bytes.trim()).expect("hex");
        bech32::encode(
            "age-secret-key-",
            bech32::ToBase32::to_base32(&bytes),
            bech32::Variant::Bech32,
        )
        .expect("bech32")
        .to_uppercase()
    }

    #[test]
    fn the_identity_is_the_files_secret_key_line_and_comments_are_skipped() {
        let file = format!(
            "# created: 2026-11-17\n# public key: age1x\n{}\n",
            test_identity()
        );
        assert_eq!(identity_of(&file).expect("an identity"), test_identity());
        assert!(identity_of("# nothing here\n").is_err());
    }

    #[test]
    fn the_two_passes_decrypt_the_same_plaintext_and_parts_are_fixed_but_the_last() {
        let sealed = vector("signed-bundle.tar.age");
        let plain = vector("signed-bundle.tar");
        let fingerprint = String::from_utf8(vector("signing-key.fingerprint")).expect("text");
        let options = VerifyOptions {
            expect_key: Some(fingerprint.trim().to_owned()),
            ..VerifyOptions::default()
        };
        let first = verify_pass(sealed.as_slice(), &test_identity(), &options).expect("pass 1");
        assert_eq!(first.size, plain.len() as u64);
        assert_eq!(first.sha256, hex::encode(Sha256::digest(&plain)));
        assert!(first.authenticated);

        let part_size = 10_000;
        let mut parts: Vec<(u64, Vec<u8>)> = Vec::new();
        let (size, sha256) = upload_pass(
            sealed.as_slice(),
            &test_identity(),
            part_size,
            &mut |n, bytes| {
                parts.push((n, bytes.to_vec()));
                Ok(())
            },
        )
        .expect("pass 2");
        assert_eq!((size, sha256), (first.size, first.sha256));
        assert_eq!(parts.len(), plain.len().div_ceil(part_size));
        for (i, (n, bytes)) in parts.iter().enumerate() {
            assert_eq!(*n, i as u64 + 1);
            if i + 1 < parts.len() {
                assert_eq!(bytes.len(), part_size);
            }
        }
        let joined: Vec<u8> = parts.into_iter().flat_map(|(_, b)| b).collect();
        assert_eq!(joined, plain);
    }

    #[test]
    fn a_bundle_entry_with_dotdot_is_refused_path_unsafe() {
        let plain = vector("signed-bundle.tar");
        let mut archive = tar::Archive::new(plain.as_slice());
        let mut builder = tar::Builder::new(Vec::new());
        for (i, entry) in archive.entries().expect("entries").raw(true).enumerate() {
            let mut entry = entry.expect("entry");
            let path = String::from_utf8_lossy(&entry.header().path_bytes()).into_owned();
            let path = if i == 1 { format!("../{path}") } else { path };
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read");
            let mut header = tar::Header::new_ustar();
            header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append(&header, bytes.as_slice()).expect("append");
        }
        let forged = builder.into_inner().expect("finish");
        let identity = test_identity();
        let recipient = String::from_utf8(vector("test-recipient.txt")).expect("text");
        let sealed = oml_bundle::crypt::encrypt(&forged, &[recipient.trim()]).expect("encrypt");
        let refused = verify_pass(sealed.as_slice(), &identity, &VerifyOptions::default())
            .expect_err("refused");
        assert_eq!(refused.code(), "bundle.path_unsafe");
    }

    #[test]
    fn a_first_import_without_expect_key_names_the_flag_and_the_fingerprint() {
        let hint = refusal_hint("request.validation", Some("/expect_key"), "ab12").expect("a hint");
        assert!(hint.contains("--expect-key ab12"), "{hint}");
        assert!(refusal_hint("request.validation", Some("/target"), "ab12").is_none());
    }
}
