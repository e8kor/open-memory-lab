//! The `maintenance` key role's one act of its own (ADR-0097 d18): an admin
//! key converts itself, in place, into a key that reads counts only, so no
//! service needs to hold an admin key.

use serde_json::json;

use oml_domain::{Role, event_types};

use super::App;
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::types::{AppError, Bearer, Credential, KeyConversionRefusal, KeyRecord};

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/keys/self/convert {to: maintenance}` (ADR-0097 d18): the
    /// calling admin bearer key, presented by its current value, becomes a
    /// `maintenance` key in place. It keeps its id, name and hash, takes the
    /// role's request rate, and journals one sealed `key.converted {key_id,
    /// from: admin, to: maintenance}`, which the fold replays; never a
    /// migration's `UPDATE`. Only `admin → maintenance` under the key itself
    /// is admitted; anything else is [`AppError::KeyConversion`] and
    /// journals nothing.
    ///
    /// # Errors
    /// The refusals above, the caller's authentication, and the store's.
    pub async fn convert_self(
        &self,
        bearer: impl Into<Bearer<'_>>,
        to: Role,
    ) -> Result<KeyRecord, AppError> {
        if to != Role::Maintenance {
            return Err(AppError::KeyConversion(KeyConversionRefusal::TargetRole));
        }
        let caller = self
            .authenticate_bearer_admitting(bearer.into(), true)
            .await?;
        if caller.key.role != Role::Admin {
            return Err(AppError::KeyConversion(KeyConversionRefusal::NotAdmin));
        }
        caller.require_current_value("convert_self")?;
        let mut key = caller.key;
        if key.credential != Credential::Bearer {
            return Err(AppError::KeyConversion(KeyConversionRefusal::SessionKey));
        }
        let tenant_id = key.tenant_id;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::KEY_CONVERTED,
            &key.principal,
            Some(key.key_id),
            json!({
                "key_id": key.key_id.to_string(),
                "from": Role::Admin.as_str(),
                "to": Role::Maintenance.as_str(),
            }),
        )?;
        self.commit(
            event,
            MetadataFold::KeyConverted { key_id: key.key_id },
            None,
        )
        .await?;
        key.role = Role::Maintenance;
        key.requests_per_minute = Some(super::MAINTENANCE_REQUESTS_PER_MINUTE);
        Ok(key)
    }
}
