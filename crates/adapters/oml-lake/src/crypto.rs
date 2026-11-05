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
