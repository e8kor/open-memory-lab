//! The committed test vectors under `tests/vectors/`, for the server and
//! CLI lanes to reuse (ADR-0084):
//!
//! - `signed-bundle.tar`: the fixture of `common`, plaintext, signed by the
//!   test key of seed `[42; 32]`;
//! - `signed-bundle.tar.age`: the same tar encrypted to the recipient in
//!   `test-recipient.txt`;
//! - `test-identity.txt`: that recipient's age identity, a throwaway made for
//!   these vectors and never used for anything else;
//! - `signing-key.fingerprint`: the test key's fingerprint, for
//!   `--expect-key`.
//!
//! `OML_BUNDLE_WRITE_VECTORS=1 cargo test -p oml-bundle --test vectors`
//! rewrites them after a deliberate format change.

mod common;

use std::path::PathBuf;

use common::*;
use oml_bundle::{VerifyOptions, crypt, key_fingerprint, read_bundle};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

fn writing() -> bool {
    std::env::var_os("OML_BUNDLE_WRITE_VECTORS").is_some()
}

fn write_fingerprint() {
    std::fs::write(
        dir().join("signing-key.fingerprint"),
        format!("{}\n", key_fingerprint(&test_key().verifying_key())),
    )
    .unwrap();
}

#[test]
fn the_committed_plaintext_vector_is_what_the_writer_makes() {
    let path = dir().join("signed-bundle.tar");
    if writing() {
        std::fs::write(&path, tar_bytes()).unwrap();
        write_fingerprint();
    }
    assert_eq!(std::fs::read(path).unwrap(), tar_bytes());
}

#[test]
fn the_committed_encrypted_vector_opens_with_the_test_identity_and_verifies_under_the_test_key() {
    use age::secrecy::ExposeSecret;
    let identity_path = dir().join("test-identity.txt");
    let sealed_path = dir().join("signed-bundle.tar.age");
    if writing() {
        write_fingerprint();
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public().to_string();
        std::fs::write(
            &identity_path,
            format!("{}\n", identity.to_string().expose_secret()),
        )
        .unwrap();
        std::fs::write(dir().join("test-recipient.txt"), format!("{recipient}\n")).unwrap();
        std::fs::write(
            &sealed_path,
            crypt::encrypt(&tar_bytes(), &[&recipient]).unwrap(),
        )
        .unwrap();
    }
    let identity = std::fs::read_to_string(identity_path).unwrap();
    let opened = crypt::decrypt(&std::fs::read(sealed_path).unwrap(), &identity).unwrap();
    let fingerprint = std::fs::read_to_string(dir().join("signing-key.fingerprint")).unwrap();
    let options = VerifyOptions {
        expect_key: Some(fingerprint.trim().to_owned()),
        ..VerifyOptions::default()
    };
    let verified = read_bundle(&opened[..], &options).unwrap();
    assert!(verified.authenticated);
    assert_eq!(verified.contents.versions, contents().versions);
}
