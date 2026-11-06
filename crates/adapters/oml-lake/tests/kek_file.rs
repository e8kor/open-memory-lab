//! The lake KEK file (ADR-0060 d19, amending ADR-0005 d1): the writer holds
//! the key-encryption key, so the file is owner-only or the writer does not
//! start, and a rotation adds a version that wraps the data keys while the
//! old version stays until they are re-wrapped.

use std::path::PathBuf;

use oml_lake::crypto::{Kek, Keyring, KeyringError};
use uuid::Uuid;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kek_file");
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

#[cfg(unix)]
#[test]
fn a_kek_file_readable_by_group_or_others_is_refused_by_name() {
    use std::os::unix::fs::PermissionsExt as _;
    let path = scratch("loose");
    std::fs::write(&path, Kek::generate_hex().expect("hex")).expect("write");
    for mode in [0o644, 0o640, 0o604, 0o660] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
        let err = Keyring::load(&path).map(|_| ()).expect_err("refused");
        assert!(
            matches!(err, KeyringError::Permissions { mode: m, .. } if m == mode),
            "{mode:o}: {err}"
        );
        assert!(err.to_string().contains("owner-only"), "{err}");
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    assert_eq!(Keyring::load(&path).expect("loads").len(), 1);
}

#[test]
fn a_rotated_keyring_wraps_new_keys_under_the_new_kek_and_still_opens_the_old_ones() {
    let (t, a) = (Uuid::from_u128(1), Uuid::from_u128(2));
    let old = Keyring::single(Kek::from_file_bytes(&[7_u8; 32]).expect("kek"));
    let old_id = old.current().id().to_owned();
    let (dek, wrapped) = old.current().new_data_key(t, a).expect("dek");
    let sealed = dek.seal_body(t, a, b"canary").expect("seal");

    let both = old.with_new_current().expect("rotate");
    assert_eq!(both.len(), 2);
    let new_id = both.current().id().to_owned();
    assert_ne!(new_id, old_id);
    assert!(both.get(&old_id).is_some());

    let rewrapped = both.rewrap(t, a, &wrapped, &old_id).expect("rewrap");
    let opened = both.current().unwrap(t, a, &rewrapped).expect("opens");
    assert_eq!(opened.open_body(t, a, &sealed).expect("body"), b"canary");

    let only_new = both.only_current();
    assert_eq!(only_new.len(), 1);
    assert!(only_new.get(&old_id).is_none());
    assert!(only_new.rewrap(t, a, &wrapped, &old_id).is_err());
}

#[cfg(unix)]
#[test]
fn a_keyring_written_to_a_file_is_owner_only_and_loads_back_with_every_version() {
    use std::os::unix::fs::PermissionsExt as _;
    let path = scratch("written");
    let ring = Keyring::single(Kek::from_file_bytes(&[9_u8; 32]).expect("kek"))
        .with_new_current()
        .expect("rotate");
    ring.write_atomic(&path).expect("write");
    let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let back = Keyring::load(&path).expect("loads");
    assert_eq!(back.len(), 2);
    assert_eq!(back.current().id(), ring.current().id());
}
