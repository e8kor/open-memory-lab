//! Body sealing for the lake (ADR-0005 d1, ADR-0060 d6): each materialized
//! claim version gets its own 256-bit data key; the body is sealed under it
//! with ChaCha20-Poly1305, and the data key is wrapped by the lake's
//! key-encryption key (KEK). Erase destroys the wrapped data key in
//! Postgres, after which every copy of the ciphertext the lake still holds,
//! old table versions included, opens under no key that exists.
//!
//! Sealed bytes are `nonce (12) || ciphertext || tag (16)`; the associated
//! data binds a sealed body to its tenant and version, so a ciphertext moved
//! to another row does not open.

use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

/// Bytes in a data key and in the KEK.
pub const KEY_LEN: usize = 32;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SealError {
    /// The KEK file is not 32 raw bytes or 64 hex characters.
    #[error("the lake KEK is not 32 bytes (raw) or 64 hex characters")]
    KekMalformed,
    /// The operating system's random source failed.
    #[error("the random source failed")]
    Random,
    /// The bytes do not open under the key: wrong key, wrong row, or altered.
    #[error("the sealed bytes do not open under this key and row")]
    Opens,
}

/// The lake's key-encryption key, read from an owner-only file. Its id is
/// the first 8 bytes of its SHA-256, hex: enough to name which KEK wrapped a
/// data key, never enough to recover it.
pub struct Kek {
    key: [u8; KEY_LEN],
    id: String,
}

impl std::fmt::Debug for Kek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kek")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Kek {
    /// A KEK from a file's contents: 32 raw bytes, or 64 hex characters with
    /// optional surrounding whitespace.
    pub fn from_file_bytes(bytes: &[u8]) -> Result<Self, SealError> {
        let key: [u8; KEY_LEN] = if bytes.len() == KEY_LEN {
            bytes.try_into().map_err(|_| SealError::KekMalformed)?
        } else {
            let text = std::str::from_utf8(bytes).map_err(|_| SealError::KekMalformed)?;
            let raw = hex::decode(text.trim()).map_err(|_| SealError::KekMalformed)?;
            raw.as_slice()
                .try_into()
                .map_err(|_| SealError::KekMalformed)?
        };
        let id = hex::encode(&Sha256::digest(key)[..8]);
        Ok(Self { key, id })
    }

    /// A fresh random KEK, as 64 hex characters for its file.
    pub fn generate_hex() -> Result<String, SealError> {
        Ok(hex::encode(random::<KEY_LEN>()?))
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// A fresh data key and its wrapping under this KEK.
    pub fn new_data_key(
        &self,
        tenant: Uuid,
        artifact: Uuid,
    ) -> Result<(DataKey, Vec<u8>), SealError> {
        let dek = random::<KEY_LEN>()?;
        let wrapped = seal(&self.key, &aad(b"dek", tenant, artifact), &dek)?;
        Ok((DataKey(dek), wrapped))
    }

    /// The data key a wrapped key holds, for that tenant and artifact.
    pub fn unwrap(
        &self,
        tenant: Uuid,
        artifact: Uuid,
        wrapped: &[u8],
    ) -> Result<DataKey, SealError> {
        let raw = open(&self.key, &aad(b"dek", tenant, artifact), wrapped)?;
        let dek: [u8; KEY_LEN] = raw.as_slice().try_into().map_err(|_| SealError::Opens)?;
        Ok(DataKey(dek))
    }
}

/// Why a KEK file did not load.
#[derive(Debug, thiserror::Error)]
pub enum KeyringError {
    /// The file cannot be read.
    #[error("lake KEK file {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    /// The file is readable by group or others; it holds the key that wraps
    /// every data key, so the writer does not start on it.
    #[error(
        "lake KEK file {path} has mode {mode:o}: the file must be owner-only (chmod 600), \
         because the writer holds the key that wraps every body key"
    )]
    Permissions { path: String, mode: u32 },
    /// The contents are not KEKs.
    #[error("lake KEK file {path}: {source}")]
    Malformed { path: String, source: SealError },
}

/// The lake's KEK versions, from one owner-only file (ADR-0060 d19). One
/// version is the normal state. A rotation adds a version, re-wraps every
/// live data key under it, then retires the older ones. The last version is
/// current; a data key names the version that wrapped it by id.
#[derive(Debug)]
pub struct Keyring {
    keys: Vec<Kek>,
}

impl Keyring {
    /// A keyring of one version.
    #[must_use]
    pub fn single(kek: Kek) -> Self {
        Self { keys: vec![kek] }
    }

    /// A keyring from file contents: 32 raw bytes, or one 64-hex KEK per
    /// line, oldest first, `#` lines ignored.
    ///
    /// # Errors
    /// The contents hold no KEK, or a line is not one.
    pub fn from_file_bytes(bytes: &[u8]) -> Result<Self, SealError> {
        if bytes.len() == KEY_LEN {
            return Ok(Self::single(Kek::from_file_bytes(bytes)?));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| SealError::KekMalformed)?;
        let keys = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| Kek::from_file_bytes(l.as_bytes()))
            .collect::<Result<Vec<_>, _>>()?;
        if keys.is_empty() {
            return Err(SealError::KekMalformed);
        }
        Ok(Self { keys })
    }

    /// Loads the KEK file, refusing one that group or others can read.
    ///
    /// # Errors
    /// The file is unreadable, too open, or not KEKs.
    pub fn load(path: &std::path::Path) -> Result<Self, KeyringError> {
        let shown = path.display().to_string();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let meta = std::fs::metadata(path).map_err(|source| KeyringError::Read {
                path: shown.clone(),
                source,
            })?;
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                return Err(KeyringError::Permissions { path: shown, mode });
            }
        }
        let bytes = std::fs::read(path).map_err(|source| KeyringError::Read {
            path: shown.clone(),
            source,
        })?;
        Self::from_file_bytes(&bytes).map_err(|source| KeyringError::Malformed {
            path: shown,
            source,
        })
    }

    /// The version that wraps new data keys.
    #[must_use]
    pub fn current(&self) -> &Kek {
        &self.keys[self.keys.len() - 1]
    }

    /// The version a data key names.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Kek> {
        self.keys.iter().find(|k| k.id() == id)
    }

    /// Versions held.
    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Ids of every version, oldest first.
    #[must_use]
    pub fn ids(&self) -> Vec<String> {
        self.keys.iter().map(|k| k.id().to_owned()).collect()
    }

    /// This keyring plus a fresh random current version.
    ///
    /// # Errors
    /// The random source fails.
    pub fn with_new_current(mut self) -> Result<Self, SealError> {
        let hex = Kek::generate_hex()?;
        self.keys.push(Kek::from_file_bytes(hex.as_bytes())?);
        Ok(self)
    }

    /// This keyring with only the current version.
    #[must_use]
    pub fn only_current(mut self) -> Self {
        self.keys.drain(..self.keys.len() - 1);
        self
    }

    /// A data key wrapped by the version `from_id`, wrapped again by the
    /// current version.
    ///
    /// # Errors
    /// `from_id` is not in this keyring, or the wrapped key does not open
    /// under it for this tenant and artifact.
    pub fn rewrap(
        &self,
        tenant: Uuid,
        artifact: Uuid,
        wrapped: &[u8],
        from_id: &str,
    ) -> Result<Vec<u8>, SealError> {
        let from = self.get(from_id).ok_or(SealError::Opens)?;
        let raw = open(&from.key, &aad(b"dek", tenant, artifact), wrapped)?;
        seal(&self.current().key, &aad(b"dek", tenant, artifact), &raw)
    }

    /// Writes the file atomically and owner-only: a temporary file beside
    /// it, then a rename, so the file holds the old versions or the new
    /// ones and never a prefix of either.
    ///
    /// # Errors
    /// The write or the rename fails.
    pub fn write_atomic(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write as _;
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".new");
        let tmp = std::path::PathBuf::from(tmp);
        let _ = std::fs::remove_file(&tmp);
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut f = opts.open(&tmp)?;
        let mut text = String::new();
        for k in &self.keys {
            text.push_str(&hex::encode(k.key));
            text.push('\n');
        }
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    }
}

/// One claim version's data key, held only while its body is sealed.
pub struct DataKey([u8; KEY_LEN]);

impl DataKey {
    /// Seals `body` for (`tenant`, `artifact`).
    pub fn seal_body(
        &self,
        tenant: Uuid,
        artifact: Uuid,
        body: &[u8],
    ) -> Result<Vec<u8>, SealError> {
        seal(&self.0, &aad(b"body", tenant, artifact), body)
    }

    /// Opens a sealed body of (`tenant`, `artifact`).
    pub fn open_body(
        &self,
        tenant: Uuid,
        artifact: Uuid,
        sealed: &[u8],
    ) -> Result<Vec<u8>, SealError> {
        open(&self.0, &aad(b"body", tenant, artifact), sealed)
    }
}

/// Lowercase hex SHA-256: the lake's digest spelling (delta-tables.md type
/// conventions).
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn aad(purpose: &[u8], tenant: Uuid, artifact: Uuid) -> Vec<u8> {
    let mut out = Vec::with_capacity(purpose.len() + 32);
    out.extend_from_slice(purpose);
    out.extend_from_slice(tenant.as_bytes());
    out.extend_from_slice(artifact.as_bytes());
    out
}

fn random<const N: usize>() -> Result<[u8; N], SealError> {
    let mut out = [0_u8; N];
    getrandom::getrandom(&mut out).map_err(|_| SealError::Random)?;
    Ok(out)
}

fn seal(key: &[u8; KEY_LEN], aad_bytes: &[u8], plain: &[u8]) -> Result<Vec<u8>, SealError> {
    let key =
        LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, key).map_err(|_| SealError::Opens)?);
    let nonce_bytes = random::<NONCE_LEN>()?;
    let mut buf = plain.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce_bytes),
        Aad::from(aad_bytes),
        &mut buf,
    )
    .map_err(|_| SealError::Opens)?;
    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&buf);
    Ok(out)
}

fn open(key: &[u8; KEY_LEN], aad_bytes: &[u8], sealed: &[u8]) -> Result<Vec<u8>, SealError> {
    if sealed.len() < NONCE_LEN {
        return Err(SealError::Opens);
    }
    let key =
        LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, key).map_err(|_| SealError::Opens)?);
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| SealError::Opens)?;
    let mut buf = body.to_vec();
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad_bytes),
            &mut buf,
        )
        .map_err(|_| SealError::Opens)?;
    Ok(plain.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_opens_under_its_own_key_and_row_and_under_no_other() {
        let kek = Kek::from_file_bytes(Kek::generate_hex().expect("hex").as_bytes()).expect("kek");
        let (t, a, b) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let (dek, wrapped) = kek.new_data_key(t, a).expect("dek");
        let sealed = dek.seal_body(t, a, b"the canary").expect("seal");
        assert!(!sealed.windows(10).any(|w| w == b"the canary"));
        let again = kek.unwrap(t, a, &wrapped).expect("unwrap");
        assert_eq!(again.open_body(t, a, &sealed).expect("open"), b"the canary");
        assert_eq!(again.open_body(t, b, &sealed), Err(SealError::Opens));
        assert!(kek.unwrap(t, b, &wrapped).is_err());
        let other = Kek::from_file_bytes(&[7_u8; 32]).expect("kek");
        assert!(other.unwrap(t, a, &wrapped).is_err());
    }

    #[test]
    fn a_kek_file_of_the_wrong_length_is_refused() {
        assert_eq!(
            Kek::from_file_bytes(b"abcd").map(|_| ()),
            Err(SealError::KekMalformed)
        );
    }
}
