//! The `maintenance` key role's one act of its own (ADR-0097 d18): an admin
//! key converts itself, in place, into a key that reads counts only, so no
//! service needs to hold an admin key.

use serde_json::json;

use oml_domain::admin_act::AdminActCause;
use oml_domain::{Role, SignedEnvelope, event_types};

use super::App;
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::types::{
    AppError, Bearer, Credential, KeyConversionRefusal, KeyRecord, SuspensionCause,
};

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
    /// journals nothing. A key the first start suspended as unlinked still
    /// reaches this route inside its upgrade window, as `link_self` does,
    /// and the conversion lifts that suspension in its own commit,
    /// `key.suspension_lifted {cause: unlinked}` before `key.converted`, so
    /// the maintenance key then reads counts (ADR-0097 d18, the production
    /// migration; the operator's decision D27 of 2026-11-28). A suspension
    /// against an admin (d8) is not lifted and refuses the call.
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
        // The unlinked admission `link_self` makes: an admin key suspended
        // as unlinked, or waiting on its link, reaches this route; any other
        // suspension still refuses it (check_key).
        let caller = match bearer.into() {
            Bearer::Key(value) => {
                let (key, previous_value) = self.key_of_value_admitting(value, true).await?;
                let mut caller = self.caller_of(key).await?;
                caller.previous_value = previous_value;
                caller
            }
            other => self.authenticate_bearer_admitting(other, true).await?,
        };
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
        let links = self.conversion_links(&key, prev.as_ref(), &key.principal, Some(key.key_id), None)?;
        self.commit_chain(links).await?;
        key.suspended = None;
        key.role = Role::Maintenance;
        key.requests_per_minute = Some(super::MAINTENANCE_REQUESTS_PER_MINUTE);
        Ok(key)
    }

    /// The chain that converts `key` to `maintenance`, sealed on `prev`
    /// under `principal` and `actor`: an unlinked suspension lifted first,
    /// then `key.converted`, with `cause` where the server converts it at a
    /// start (ADR-0097 d18; D27 of 2026-11-28).
    pub(crate) fn conversion_links(
        &self,
        key: &KeyRecord,
        prev: Option<&SignedEnvelope>,
        principal: &str,
        actor: Option<uuid::Uuid>,
        cause: Option<AdminActCause>,
    ) -> Result<Vec<(SignedEnvelope, MetadataFold)>, AppError> {
        let mut links: Vec<(SignedEnvelope, MetadataFold)> = Vec::new();
        if key
            .suspended
            .as_ref()
            .is_some_and(|s| s.cause == SuspensionCause::Unlinked)
        {
            let lifted = self.seal(
                prev,
                key.tenant_id,
                event_types::KEY_SUSPENSION_LIFTED,
                principal,
                actor,
                json!({
                    "key_id": key.key_id.to_string(),
                    "cause": SuspensionCause::Unlinked.as_str(),
                    "act_id": null,
                }),
            )?;
            links.push((lifted, MetadataFold::KeySuspensionLifted { key_id: key.key_id }));
        }
        let mut payload = json!({
            "key_id": key.key_id.to_string(),
            "from": Role::Admin.as_str(),
            "to": Role::Maintenance.as_str(),
        });
        if let Some(cause) = cause {
            payload["cause"] = json!(cause.as_str());
        }
        let event = self.seal(
            links.last().map(|(e, _)| e).or(prev),
            key.tenant_id,
            event_types::KEY_CONVERTED,
            principal,
            actor,
            payload,
        )?;
        links.push((event, MetadataFold::KeyConverted { key_id: key.key_id }));
        Ok(links)
    }
}
