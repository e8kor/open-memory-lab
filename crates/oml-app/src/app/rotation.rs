//! Key rotation (ADR-0096 d13, the operator's decision of 2026-11-25): a
//! bearer key gets a new value and keeps everything else, its id, name,
//! grants, denials, limits, links, crew and history. The previous value
//! stops at once, or with a grace of at most 24 hours keeps authenticating
//! until its end. The value is shown once, as at issuance, and journalled
//! never: `key.rotated` names the key, the rotating key and the grace's end,
//! and the new hash rides the commit's fold as an issuance's does.

use serde_json::json;
use uuid::Uuid;

use oml_domain::name::{Name, NamedKind, Reference};
use oml_domain::{EventTime, Role, event_types};

use super::{App, base62_43, sha256_digest_string};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::types::{AppError, Bearer, Credential, KeyRecord, KeyRotationRefusal, RotatedKey};

/// The longest grace a rotation gives the previous value: 24 hours.
pub const ROTATION_GRACE_MAX_SECS: u32 = 86_400;

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/keys/{key}/rotate` (ADR-0096 d13): a new value for `key`,
    /// by an admin key (a `<user>-admin` session's included) for any key of
    /// the tenant, by a person's `<user>-web` session for that person's own
    /// keys, or by the key itself. A session key, a connector key, a revoked
    /// key, a cut-off key and an expired key are refused by name, and so is
    /// a grace above [`ROTATION_GRACE_MAX_SECS`]. Any other caller is
    /// `access.verb_ceiling`; a person's session naming another's key finds
    /// none.
    ///
    /// # Errors
    /// The refusals above, the caller's authentication, and the store's.
    pub async fn rotate_key(
        &self,
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
        grace_seconds: Option<u32>,
    ) -> Result<RotatedKey, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?.key;
        if grace_seconds.is_some_and(|s| s > ROTATION_GRACE_MAX_SECS) {
            return Err(AppError::KeyRotation(KeyRotationRefusal::GraceTooLong));
        }
        let tenant_id = caller.tenant_id;
        let key_id = self.resolve(&caller, NamedKind::Key, &key).await?;
        let Some(target) = self.journal.key(tenant_id, key_id).await? else {
            return Err(AppError::NotFound);
        };
        let own_session = caller.credential == Credential::Session
            && caller.role == Role::Contributor
            && caller.user.is_some();
        if caller.role != Role::Admin && target.key_id != caller.key_id {
            if !own_session {
                return Err(AppError::VerbCeiling {
                    role: caller.role,
                    verb: "rotate_key",
                });
            }
            // A person's web session reaches that person's keys alone, its
            // session keys among them, which are then refused by name.
            let own = target.user == caller.user
                || (target.session_user.is_some() && target.session_user == caller.session_user);
            if !own {
                return Err(AppError::NotFound);
            }
        }
        self.require_rotatable(&target).await?;
        let now = self.clock.now();
        let secret = self.ids.new_secret();
        let value = format!("oml_{}", base62_43(&secret));
        let hash = sha256_digest_string(value.as_bytes());
        let previous = match (grace_seconds.filter(|s| *s > 0), target.key_hash.clone()) {
            (Some(grace), Some(old)) => {
                let until =
                    EventTime::from_unix_micros(now.unix_micros() + i64::from(grace) * 1_000_000)
                        .ok_or(AppError::KeyRotation(KeyRotationRefusal::GraceTooLong))?;
                Some((old, until))
            }
            _ => None,
        };
        let previous_valid_until = previous.as_ref().map(|(_, until)| until.clone());
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_ROTATED,
            &caller.principal,
            Some(caller.key_id),
            rotated_payload(key_id, caller.key_id, previous_valid_until.as_ref()),
        )?;
        let receipt = self
            .commit(
                event,
                MetadataFold::KeyRotated {
                    key_id,
                    key_hash: hash.clone(),
                    previous,
                },
                None,
            )
            .await?;
        let mut key = target;
        key.key_hash = Some(hash);
        Ok(RotatedKey {
            key,
            value,
            rotated_at: now,
            previous_valid_until,
            journal_seq: receipt.seq,
        })
    }

    /// Whether `target` takes a new value: a live bearer key that is not a
    /// session key, not cut off and not expired (ADR-0096 d13).
    async fn require_rotatable(&self, target: &KeyRecord) -> Result<(), AppError> {
        let refuse = |r| Err(AppError::KeyRotation(r));
        match target.credential {
            Credential::Session => return refuse(KeyRotationRefusal::SessionKey),
            Credential::Connection => return refuse(KeyRotationRefusal::ConnectorKey),
            Credential::Bearer => {}
        }
        if target.revoked_at.is_some() {
            return refuse(KeyRotationRefusal::Revoked);
        }
        if target.expired_at(&self.clock.now()) {
            return refuse(KeyRotationRefusal::Expired);
        }
        // A key cut off, by a denial on `universe`, stays cut off: a new
        // value would only hand it out again.
        let universe = self
            .journal
            .resolve_name(
                target.tenant_id,
                NamedKind::Workspace,
                &Name::parse("universe").expect("universe is a name"),
            )
            .await?;
        let cut_off = self
            .journal
            .key_access(target.tenant_id, target.key_id)
            .await?
            .is_some_and(|a| a.denials.iter().any(|d| Some(d.denial.node) == universe));
        if cut_off {
            return refuse(KeyRotationRefusal::CutOff);
        }
        Ok(())
    }

    /// The instant until which `key`'s previous value still authenticates,
    /// where a rotation's grace has not ended (ADR-0096 d13): what `whoami`
    /// serves and `oml doctor` reports as `key.rotation_grace_open`.
    ///
    /// # Errors
    /// The store's.
    pub async fn rotation_grace(&self, key: &KeyRecord) -> Result<Option<EventTime>, AppError> {
        let until = self
            .journal
            .key_previous_valid_until(key.tenant_id, key.key_id)
            .await?;
        let now = self.clock.now();
        Ok(until.filter(|u| u.unix_micros() > now.unix_micros()))
    }
}

/// The catalog payload of `key.rotated` (event-catalog.md): never a value
/// or a hash.
fn rotated_payload(key_id: Uuid, by: Uuid, until: Option<&EventTime>) -> serde_json::Value {
    json!({
        "key_id": key_id.to_string(),
        "rotated_by_key_id": by.to_string(),
        "previous_valid_until": until.map(EventTime::as_str),
    })
}
