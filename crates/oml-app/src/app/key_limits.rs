//! A key's limits (ADR-0079): the instant it stops authenticating and the
//! requests it may make in a minute. Both are set at issuance and changed
//! only by an admin key that is not the key itself; the expiry is read
//! against the clock on every request, and the rate is counted in this
//! process's memory.

use serde_json::json;
use uuid::Uuid;

use oml_domain::name::{NamedKind, Reference};
use oml_domain::{EventTime, Role, event_types};

use super::{App, require_role};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::types::{
    AppError, Bearer, Credential, KEY_REQUESTS_PER_MINUTE_MAX, KeyLimitsChange, KeyLimitsRefusal,
    KeyRecord,
};

/// Whether `count` requests already admitted this minute leave room for one
/// more under `limit` (ADR-0079 d4).
fn admits(count: u64, limit: u32) -> bool {
    count < u64::from(limit)
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The key, unless its expiry has come: then
    /// [`AppError::KeyExpired`], which only a caller that presented the
    /// key's own credential is answered (ADR-0079 d2).
    pub fn require_unexpired(&self, key: &KeyRecord) -> Result<(), AppError> {
        match &key.expiry {
            Some(expiry) if key.expired_at(&self.clock.now()) => Err(AppError::KeyExpired {
                expiry: expiry.clone(),
            }),
            _ => Ok(()),
        }
    }

    /// Whether `key`'s expiry has come on this lead's clock, as a key read
    /// serves it (ADR-0079 d3).
    #[must_use]
    pub fn key_expired(&self, key: &KeyRecord) -> bool {
        key.expired_at(&self.clock.now())
    }

    /// An `expiry` a request names is later than the lead's clock, and a
    /// `requests_per_minute` is from 1 to [`KEY_REQUESTS_PER_MINUTE_MAX`];
    /// anything else is refused by the field it names (ADR-0079 d1, d4).
    pub(super) fn check_key_limits(
        &self,
        expiry: Option<&EventTime>,
        requests_per_minute: Option<u32>,
    ) -> Result<(), AppError> {
        if expiry.is_some_and(|e| e.unix_micros() <= self.clock.now().unix_micros()) {
            return Err(AppError::KeyLimits(KeyLimitsRefusal::ExpiryNotLater));
        }
        if requests_per_minute.is_some_and(|n| !(1..=KEY_REQUESTS_PER_MINUTE_MAX).contains(&n)) {
            return Err(AppError::KeyLimits(KeyLimitsRefusal::RateOutOfRange));
        }
        Ok(())
    }

    /// One more request of `key` in its UTC minute under the key's own
    /// rate: past it, `limit.rate` with the seconds left in the minute, and
    /// the refused request is not counted. A key with no rate of its own
    /// passes and is not recorded. The rate is the row's, read with the key
    /// on this request, so a change holds from the next one; the count is
    /// this process's alone (ADR-0079 d4, d5, d6).
    pub(super) fn admit_under_key_rate(&self, key: &KeyRecord) -> Result<(), AppError> {
        let Some(limit) = key.requests_per_minute else {
            return Ok(());
        };
        let now = self.clock.now().unix_micros().div_euclid(1_000_000);
        let minute = now.div_euclid(60);
        let mut windows = self.key_windows.lock().expect("key rate lock");
        let slot = windows.entry(key.key_id).or_insert((minute, 0));
        if slot.0 != minute {
            *slot = (minute, 0);
        }
        if !admits(slot.1, limit) {
            return Err(AppError::KeyRateLimited {
                limit: u64::from(limit),
                retry_after_secs: u64::try_from(60 - now.rem_euclid(60)).unwrap_or(60),
            });
        }
        slot.1 += 1;
        Ok(())
    }

    /// `POST /v2/keys/{key}/limits` (ADR-0079 d5): the key's expiry, its
    /// requests per minute or both, replaced or cleared, journalled as
    /// `key.limits_set` with both values as they stand after the change.
    /// Admin key role only, in the key's own tenant; a key the tenant does
    /// not hold is absent. The calling key itself, a session key and a
    /// revoked key are refused by reason, so no key moves its own date or
    /// raises its own rate (R1). A change that names neither member is
    /// refused, as is an expiry that is not later than now.
    pub async fn set_key_limits(
        &self,
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
        change: KeyLimitsChange,
    ) -> Result<KeyRecord, AppError> {
        let admin = self.authenticate_bearer(bearer.into()).await?.key;
        require_role(&admin, Role::Admin, "set_key_limits")?;
        if change.expiry.is_none() && change.requests_per_minute.is_none() {
            return Err(AppError::KeyLimits(KeyLimitsRefusal::Empty));
        }
        self.check_key_limits(
            change.expiry.as_ref().and_then(Option::as_ref),
            change.requests_per_minute.flatten(),
        )?;
        let tenant_id = admin.tenant_id;
        let key_id = self.resolve(&admin, NamedKind::Key, &key).await?;
        let Some(mut target) = self.journal.key(tenant_id, key_id).await? else {
            return Err(AppError::NotFound);
        };
        if target.key_id == admin.key_id {
            return Err(AppError::KeyLimits(KeyLimitsRefusal::OwnKey));
        }
        if target.credential == Credential::Session {
            return Err(AppError::KeyLimits(KeyLimitsRefusal::SessionKey));
        }
        if target.revoked_at.is_some() {
            return Err(AppError::KeyLimits(KeyLimitsRefusal::Revoked));
        }
        if let Some(expiry) = change.expiry {
            target.expiry = expiry;
        }
        if let Some(rate) = change.requests_per_minute {
            target.requests_per_minute = rate;
        }
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_LIMITS_SET,
            &admin.principal,
            Some(admin.key_id),
            limits_set_payload(&target, admin.key_id),
        )?;
        self.commit(
            event,
            MetadataFold::KeyLimitsSet {
                key_id,
                expiry: target.expiry.clone(),
                requests_per_minute: target.requests_per_minute,
            },
            None,
        )
        .await?;
        Ok(target)
    }
}

/// The catalog payload of `key.limits_set` (event-catalog.md): the key,
/// the admin key that set them, and both limits after the change.
fn limits_set_payload(key: &KeyRecord, set_by: Uuid) -> serde_json::Value {
    json!({
        "key_id": key.key_id.to_string(),
        "set_by_key_id": set_by.to_string(),
        "expiry": key.expiry.as_ref().map(EventTime::as_str),
        "requests_per_minute": key.requests_per_minute,
    })
}

#[cfg(test)]
mod tests {
    use super::admits;

    /// The request that would be one past the limit is the first refused
    /// (ADR-0079 d4).
    #[test]
    fn a_key_is_admitted_until_it_has_made_its_limit_of_requests_this_minute() {
        assert!(admits(0, 1));
        assert!(!admits(1, 1));
        assert!(admits(59, 60));
        assert!(!admits(60, 60));
        assert!(!admits(61, 60));
    }
}
