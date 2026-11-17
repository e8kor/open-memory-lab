//! Upload sessions (ADR-0004 d3, as amended 2026-11-17; #451): a key opens
//! a session declaring the size and SHA-256 of what it sends, stores fixed
//! parts that a retry sends again, completes it against the declared digest,
//! or abandons it. Parts are ciphertext under the session's own key, and
//! only the opening key is ever served the session. Nothing is journalled.

use std::sync::Arc;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use oml_domain::upload::{
    self, MAX_SESSIONS_PER_KEY, UploadPurpose, UploadRefusal, UploadState,
};
use oml_domain::{EventTime, Role};

use super::{App, require_role};
use crate::ports::{Clock, IdGen, JournalStore, ObjectStore, Scanner, Signer};
use crate::types::{AppError, Bearer, KeyRecord, UploadSession, UploadStats};

/// The nonce XChaCha20-Poly1305 takes, stored before each part's
/// ciphertext.
const NONCE_LEN: usize = 24;

/// How many missing part numbers `upload.incomplete` names.
const MISSING_NAMED: usize = 100;

/// The object store and the two limits the composition sets (ADR-0004 d3,
/// "Limits").
#[derive(Clone)]
pub struct UploadConfig {
    pub objects: Option<Arc<dyn ObjectStore>>,
    /// `uploads.max_bytes`.
    pub max_bytes: u64,
    /// `uploads.tenant_max_bytes`.
    pub tenant_max_bytes: u64,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            objects: None,
            max_bytes: upload::DEFAULT_MAX_BYTES,
            tenant_max_bytes: upload::DEFAULT_TENANT_MAX_BYTES,
        }
    }
}

/// What one sweep of a tenant's sessions did (ADR-0004 d3, "Expiry").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadSweep {
    /// Sessions this sweep marked expired, their keys nulled.
    pub expired: u64,
    /// Sessions whose objects this sweep removed.
    pub removed: u64,
    /// Removals the object store refused; the next sweep tries again.
    pub failures: u64,
    /// The tenant's sessions after the sweep.
    pub stats: UploadStats,
}

/// The object name of part `n` of a session, and the prefix of all of
/// them.
fn prefix(tenant_id: Uuid, upload_id: Uuid) -> String {
    format!("uploads/{tenant_id}/{upload_id}/")
}

fn part_name(tenant_id: Uuid, upload_id: Uuid, n: u64) -> String {
    format!("{}{n:06}", prefix(tenant_id, upload_id))
}

/// The associated data that binds a part's ciphertext to its session and
/// position.
fn part_aad(upload_id: Uuid, n: u64) -> Vec<u8> {
    let mut aad = upload_id.as_bytes().to_vec();
    aad.extend_from_slice(&n.to_be_bytes());
    aad
}

fn micros_time(micros: i64) -> EventTime {
    EventTime::from_unix_micros(micros).expect("an upload's expiry is a representable instant")
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The app whose upload sessions keep their parts in `objects`, under
    /// `max_bytes` an upload and `tenant_max_bytes` of a tenant's open and
    /// unbound sessions (ADR-0004 d3, as amended 2026-11-17).
    #[must_use]
    pub fn with_uploads(
        self,
        objects: Arc<dyn ObjectStore>,
        max_bytes: u64,
        tenant_max_bytes: u64,
    ) -> Self {
        Self {
            uploads: UploadConfig {
                objects: Some(objects),
                max_bytes,
                tenant_max_bytes,
            },
            ..self
        }
    }

    /// `uploads.max_bytes` and `uploads.tenant_max_bytes` as composed.
    #[must_use]
    pub fn upload_limits(&self) -> (u64, u64) {
        (self.uploads.max_bytes, self.uploads.tenant_max_bytes)
    }

    fn objects(&self) -> Result<&Arc<dyn ObjectStore>, AppError> {
        self.uploads
            .objects
            .as_ref()
            .ok_or(AppError::Upload(UploadRefusal::StorageFailed))
    }

    /// `POST /v2/uploads {purpose, size, sha256}`: a writer key opens a
    /// session for `size` bytes whose SHA-256 is `sha256` (64 lowercase hex
    /// digits, checked by the transport). Over `uploads.max_bytes` is
    /// `upload.too_large`; the key's fifth open or unbound session is
    /// `upload.session_limit`; the tenant's 4 GiB is `upload.tenant_limit`.
    pub async fn open_upload(
        &self,
        bearer: impl Into<Bearer<'_>>,
        purpose: UploadPurpose,
        size: u64,
        sha256: &str,
    ) -> Result<UploadSession, AppError> {
        let key = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&key, Role::Writer, "upload")?;
        if size > self.uploads.max_bytes {
            return Err(AppError::Upload(UploadRefusal::TooLarge {
                max_bytes: self.uploads.max_bytes,
            }));
        }
        self.objects()?;
        let now = self.clock.now();
        let at = now.unix_micros();
        let session = UploadSession {
            tenant_id: key.tenant_id,
            upload_id: self.ids.new_id(),
            key_id: key.key_id,
            purpose,
            size,
            sha256: sha256.to_owned(),
            received: std::collections::BTreeSet::new(),
            state: UploadState::Open,
            part_key: Some(self.ids.new_secret()),
            created_at: now.clone(),
            touched_at: now,
            expires_at: Some(micros_time(upload::open_expiry(at, at))),
            completed_at: None,
            bound_at: None,
        };
        self.journal
            .upload_open(&session, MAX_SESSIONS_PER_KEY, self.uploads.tenant_max_bytes)
            .await?
            .map_err(AppError::Upload)?;
        Ok(session)
    }

    /// `PUT /v2/uploads/{id}/parts/{n}`: stores part `n` of an open session
    /// as ciphertext under the session's key; sending it again replaces it.
    pub async fn put_upload_part(
        &self,
        bearer: impl Into<Bearer<'_>>,
        upload_id: Uuid,
        n: u64,
        bytes: &[u8],
    ) -> Result<UploadSession, AppError> {
        let key = self.authenticate_bearer(bearer.into()).await?.key;
        let session = self.own_upload(&key, upload_id).await?;
        let now = self.clock.now();
        live(&session, &now)?;
        require_state(&session, UploadState::Open)?;
        let parts = upload::part_count(session.size);
        let expected = upload::part_len(session.size, n)
            .ok_or(AppError::Upload(UploadRefusal::PartNumber { parts }))?;
        if bytes.len() as u64 != expected {
            return Err(AppError::Upload(UploadRefusal::PartSize {
                expected,
                got: bytes.len() as u64,
            }));
        }
        let part_key = session
            .part_key
            .ok_or(AppError::Upload(UploadRefusal::SessionInvalid {
                state: UploadState::Expired,
            }))?;
        let sealed = self.seal_part(&part_key, upload_id, n, bytes)?;
        self.objects()?
            .put(&part_name(session.tenant_id, upload_id, n), sealed)
            .await
            .map_err(|_| AppError::Upload(UploadRefusal::StorageFailed))?;
        let at = now.unix_micros();
        let expires = micros_time(upload::open_expiry(session.created_at.unix_micros(), at));
        match self
            .journal
            .upload_part_received(session.tenant_id, upload_id, n, &now, &expires)
            .await?
        {
            Some(session) => Ok(session),
            None => Err(self.state_refusal(&session).await?),
        }
    }

    /// `GET /v2/uploads/{id}`: the session as its opening key sees it; one
    /// past its expiry that no sweep has reached yet reads `expired`.
    pub async fn upload_status(
        &self,
        bearer: impl Into<Bearer<'_>>,
        upload_id: Uuid,
    ) -> Result<UploadSession, AppError> {
        let key = self.authenticate_bearer(bearer.into()).await?.key;
        let mut session = self.own_upload(&key, upload_id).await?;
        if live(&session, &self.clock.now()).is_err() {
            session.state = UploadState::Expired;
        }
        Ok(session)
    }

    /// `POST /v2/uploads/{id}/complete`: reads every part in order and
    /// compares the plaintext's SHA-256 with the declared one. A match
    /// completes the session, unbound for 24 hours; a mismatch fails it,
    /// shreds its key and removes its parts (`upload.digest_mismatch`).
    pub async fn complete_upload(
        &self,
        bearer: impl Into<Bearer<'_>>,
        upload_id: Uuid,
    ) -> Result<UploadSession, AppError> {
        let key = self.authenticate_bearer(bearer.into()).await?.key;
        let session = self.own_upload(&key, upload_id).await?;
        let now = self.clock.now();
        live(&session, &now)?;
        require_state(&session, UploadState::Open)?;
        let missing = session.missing();
        if !missing.is_empty() {
            return Err(AppError::Upload(UploadRefusal::Incomplete {
                missing: missing.into_iter().take(MISSING_NAMED).collect(),
            }));
        }
        let mut hash = Sha256::new();
        for n in 1..=upload::part_count(session.size) {
            hash.update(self.upload_part(&session, n).await?);
        }
        let digest = hex(&hash.finalize());
        if digest != session.sha256 {
            if self
                .journal
                .upload_transition(
                    session.tenant_id,
                    upload_id,
                    &[UploadState::Open],
                    UploadState::Failed,
                    &now,
                    None,
                    true,
                )
                .await?
                .is_some()
            {
                self.remove_objects(&session).await;
            }
            return Err(AppError::Upload(UploadRefusal::DigestMismatch));
        }
        let expires = micros_time(upload::unbound_expiry(now.unix_micros()));
        match self
            .journal
            .upload_transition(
                session.tenant_id,
                upload_id,
                &[UploadState::Open],
                UploadState::Completed,
                &now,
                Some(&expires),
                false,
            )
            .await?
        {
            Some(session) => Ok(session),
            None => Err(self.state_refusal(&session).await?),
        }
    }

    /// `DELETE /v2/uploads/{id}`: abandons a session that is open,
    /// completed or failed, shredding its key and then removing its parts.
    /// A session already abandoned or expired answers as abandoned; a bound
    /// one belongs to the verb that bound it (`upload.session_invalid`).
    pub async fn abandon_upload(
        &self,
        bearer: impl Into<Bearer<'_>>,
        upload_id: Uuid,
    ) -> Result<(), AppError> {
        let key = self.authenticate_bearer(bearer.into()).await?.key;
        let session = self.own_upload(&key, upload_id).await?;
        let now = self.clock.now();
        let moved = self
            .journal
            .upload_transition(
                session.tenant_id,
                upload_id,
                &[UploadState::Open, UploadState::Completed, UploadState::Failed],
                UploadState::Abandoned,
                &now,
                None,
                true,
            )
            .await?;
        match moved {
            Some(_) => {
                self.remove_objects(&session).await;
                Ok(())
            }
            None => {
                let state = self
                    .journal
                    .upload_session(session.tenant_id, upload_id)
                    .await?
                    .map_or(UploadState::Abandoned, |s| s.state);
                match state {
                    UploadState::Abandoned | UploadState::Expired => Ok(()),
                    state => Err(AppError::Upload(UploadRefusal::SessionInvalid { state })),
                }
            }
        }
    }

    /// One sweep of a tenant's sessions at the clock's now (ADR-0004 d3,
    /// "Expiry"): every open or unbound session past its expiry is marked
    /// `expired` with its key nulled, and then the objects of every session
    /// whose key is gone are removed, the key always before the objects.
    pub async fn sweep_uploads(&self, tenant_id: Uuid) -> Result<UploadSweep, AppError> {
        let now = self.clock.now();
        let mut sweep = UploadSweep {
            expired: self.journal.uploads_expire(tenant_id, &now).await?,
            ..UploadSweep::default()
        };
        if let Some(objects) = &self.uploads.objects {
            let mut failed = std::collections::BTreeSet::new();
            loop {
                let page = self.journal.uploads_unswept(tenant_id, 100).await?;
                let fresh: Vec<Uuid> = page
                    .iter()
                    .copied()
                    .filter(|id| !failed.contains(id))
                    .collect();
                if fresh.is_empty() {
                    break;
                }
                for upload_id in fresh {
                    if objects
                        .delete_prefix(&prefix(tenant_id, upload_id))
                        .await
                        .is_ok()
                    {
                        self.journal
                            .upload_objects_removed(tenant_id, upload_id, &now)
                            .await?;
                        sweep.removed += 1;
                    } else {
                        failed.insert(upload_id);
                        sweep.failures += 1;
                    }
                }
            }
        }
        sweep.stats = self.journal.upload_stats(tenant_id).await?;
        Ok(sweep)
    }

    /// The bytes the object store holds under `uploads/`, for the doctor
    /// line; `None` without a store or when it cannot say.
    pub async fn uploads_stored_bytes(&self) -> Option<u64> {
        self.uploads.objects.as_ref()?.stored_bytes("uploads/").await.ok()
    }

    /// The session `upload_id` names, when `key` opened it; any other key,
    /// and a session that does not exist, is `access.not_found`.
    async fn own_upload(&self, key: &KeyRecord, upload_id: Uuid) -> Result<UploadSession, AppError> {
        self.journal
            .upload_session(key.tenant_id, upload_id)
            .await?
            .filter(|s| s.key_id == key.key_id)
            .ok_or(AppError::NotFound)
    }

    /// The refusal for a compare-and-set that found the session moved on.
    async fn state_refusal(&self, session: &UploadSession) -> Result<AppError, AppError> {
        let state = self
            .journal
            .upload_session(session.tenant_id, session.upload_id)
            .await?
            .map_or(UploadState::Abandoned, |s| s.state);
        Ok(AppError::Upload(match state {
            UploadState::Expired => UploadRefusal::Expired,
            state => UploadRefusal::SessionInvalid { state },
        }))
    }

    /// Part `n`'s plaintext; a missing object or one that does not open
    /// under the session's key is `upload.storage_failed`.
    pub(crate) async fn upload_part(
        &self,
        session: &UploadSession,
        n: u64,
    ) -> Result<Vec<u8>, AppError> {
        let failed = || AppError::Upload(UploadRefusal::StorageFailed);
        let part_key = session.part_key.ok_or_else(failed)?;
        let sealed = self
            .objects()?
            .get(&part_name(session.tenant_id, session.upload_id, n))
            .await
            .map_err(|_| failed())?
            .ok_or_else(failed)?;
        if sealed.len() < NONCE_LEN {
            return Err(failed());
        }
        let (nonce, body) = sealed.split_at(NONCE_LEN);
        XChaCha20Poly1305::new((&part_key).into())
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: body,
                    aad: &part_aad(session.upload_id, n),
                },
            )
            .map_err(|_| failed())
    }

    fn seal_part(
        &self,
        part_key: &[u8; 32],
        upload_id: Uuid,
        n: u64,
        bytes: &[u8],
    ) -> Result<Vec<u8>, AppError> {
        let secret = self.ids.new_secret();
        let nonce = XNonce::from_slice(&secret[..NONCE_LEN]);
        let sealed = XChaCha20Poly1305::new(part_key.into())
            .encrypt(
                nonce,
                Payload {
                    msg: bytes,
                    aad: &part_aad(upload_id, n),
                },
            )
            .map_err(|_| AppError::Upload(UploadRefusal::StorageFailed))?;
        let mut out = Vec::with_capacity(NONCE_LEN + sealed.len());
        out.extend_from_slice(nonce);
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    /// Removes a session whose key is already nulled from the object store
    /// and records it; a failure is left to the next sweep, which finds the
    /// session among the unswept.
    async fn remove_objects(&self, session: &UploadSession) {
        let Some(objects) = &self.uploads.objects else {
            return;
        };
        if objects
            .delete_prefix(&prefix(session.tenant_id, session.upload_id))
            .await
            .is_ok()
        {
            let _ = self
                .journal
                .upload_objects_removed(session.tenant_id, session.upload_id, &self.clock.now())
                .await;
        }
    }
}

/// `upload.expired` for a session that counts against the limits and is
/// past its expiry at `now`, though no sweep has marked it yet.
fn live(session: &UploadSession, now: &EventTime) -> Result<(), AppError> {
    match (&session.expires_at, session.state) {
        (_, UploadState::Expired) => Err(AppError::Upload(UploadRefusal::Expired)),
        (Some(expires), state)
            if state.counts_against_limits() && expires.unix_micros() <= now.unix_micros() =>
        {
            Err(AppError::Upload(UploadRefusal::Expired))
        }
        _ => Ok(()),
    }
}

fn require_state(session: &UploadSession, state: UploadState) -> Result<(), AppError> {
    if session.state == state {
        Ok(())
    } else {
        Err(AppError::Upload(UploadRefusal::SessionInvalid {
            state: session.state,
        }))
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}
