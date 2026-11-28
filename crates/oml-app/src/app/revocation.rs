//! One plain revoke verb (ADR-0097 d14): `POST /v2/keys/{key}/revoke
//! {reason}` ends one key, and nothing more. It authenticates nothing
//! afterwards on any path, bearer, OAuth token and refresh, and connect-code
//! redemption, since every one resolves only a key with no `revoked_at`;
//! its unredeemed codes are voided in the same fold. No cascade: the keys
//! it issued keep working. An admin's revocation is an admin act: of a key
//! of another admin person, an act against an admin (d8), and otherwise
//! narrowing (d3). A person's `<user>-web` session revokes the person's own
//! keys, those that count as the person, as a person's own act, not an
//! admin act (the operator's decision of 2026-11-25).

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::admin_act::AdminActKind;
use oml_domain::name::{NamedKind, Reference};
use oml_domain::{Role, event_types};

use super::admin_acts::BegunAct;
use super::auth::is_web_session_key;
use super::fixes::{
    SERVER_ISSUER_PRINCIPAL, SERVER_KEY_PRINCIPAL, server_issuer_id, server_key_id,
};
use super::{App, commit_request};
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::types::{AppError, Bearer, Credential, KeyRecord, KeyRevocationRefusal, RevokedKey};

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/keys/{key}/revoke {reason}` (ADR-0097 d14). Who: an admin
    /// key on any key of the tenant; a person's `<user>-web` session on a
    /// key that counts as that person, any other key finding none. Refused
    /// by name, `request.validation` with `details.field: key`: a session
    /// key (`session_key`; a session ends with its entry), the server's
    /// reserved key (`reserved`), an already revoked key (`revoked`). Any
    /// other caller is `access.verb_ceiling`. Journals `key.revoked {key_id,
    /// revoked_by_key_id, actor_person, reason, act_id, cause}`, an admin's
    /// behind its `admin.act` in one commit.
    ///
    /// # Errors
    /// The refusals above, the caller's authentication, a reason the scan
    /// vetoes, and the store's.
    pub async fn revoke_key(
        &self,
        bearer: impl Into<Bearer<'_>>,
        key: Reference,
        reason: Option<&str>,
    ) -> Result<RevokedKey, AppError> {
        let caller = self.key_act_caller(bearer.into(), "revoke_key").await?.key;
        let tenant_id = caller.tenant_id;
        let persons_own = is_web_session_key(&caller);
        if caller.role != Role::Admin && !persons_own {
            return Err(AppError::VerbCeiling {
                role: caller.role,
                verb: "revoke_key",
            });
        }
        let key_id = self.resolve(&caller, NamedKind::Key, &key).await?;
        let target = self
            .journal
            .key(tenant_id, key_id)
            .await?
            .ok_or(AppError::NotFound)?;
        let target_person = self.counted_person_of(&target).await?;
        if persons_own && (target_person != caller.session_user || target.key_id == caller.key_id) {
            return Err(AppError::NotFound);
        }
        if target.credential == Credential::Session {
            return Err(AppError::KeyRevocation(KeyRevocationRefusal::SessionKey));
        }
        if (target.key_id == server_key_id(tenant_id) && target.principal == SERVER_KEY_PRINCIPAL)
            || (target.key_id == server_issuer_id(tenant_id)
                && target.principal == SERVER_ISSUER_PRINCIPAL)
        {
            return Err(AppError::KeyRevocation(KeyRevocationRefusal::Reserved));
        }
        if target.revoked_at.is_some() {
            return Err(AppError::KeyRevocation(KeyRevocationRefusal::Revoked));
        }
        if let Some(reason) = reason {
            self.scan_veto_only(tenant_id, "reason", reason)?;
        }
        let prev = self.head_event(tenant_id).await?;
        let act: Option<BegunAct> = if persons_own {
            None
        } else {
            let actor = self.counted_person_of(&caller).await?;
            let against_admin = target.role == Role::Admin
                && target_person.is_some()
                && target_person != actor
                && self
                    .is_admin_person(tenant_id, target_person.as_ref())
                    .await?;
            let kind = if against_admin {
                AdminActKind::RevokeAdminKey
            } else {
                AdminActKind::RevokeKey
            };
            Some(
                self.begin_admin_act(
                    &caller,
                    kind,
                    json!({
                        "key_id": key_id.to_string(),
                        "name": target.name.as_ref().map(|n| n.as_str()),
                        "role": target.role.as_str(),
                        "person": target_person.as_ref().map(|p| p.as_str()),
                        // Revoking a person's key is offensive narrowing,
                        // loud at once (ADR-0097 d24).
                        "offensive": target_person.is_some(),
                    }),
                    reason,
                    json!({}),
                    prev.as_ref(),
                )
                .await?,
            )
        };
        let actor_person = match &act {
            Some(act) => act.record.actor_person.clone(),
            None => caller.session_user.clone(),
        };
        let event = self.seal(
            act.as_ref().map(|a| &a.event).or(prev.as_ref()),
            tenant_id,
            event_types::KEY_REVOKED,
            &caller.principal,
            Some(caller.key_id),
            json!({
                "key_id": key_id.to_string(),
                "revoked_by_key_id": caller.key_id.to_string(),
                "actor_person": actor_person.as_ref().map(|p| p.as_str()),
                "reason": reason,
                "act_id": act.as_ref().map(|a| a.act_id().to_string()),
                "cause": Value::Null,
            }),
        )?;
        let revoked_at = event.envelope.at.clone();
        let mut reqs = Vec::with_capacity(2);
        if let Some(act) = &act {
            let (event, fold) = act.link();
            reqs.push(commit_request(event, fold, None));
        }
        reqs.push(commit_request(
            event,
            MetadataFold::KeyRevoked { key_id },
            None,
        ));
        // Revoking a person's key, or another admin's, is loud (ADR-0097
        // d20, d24); the answer names the revocation's own seq.
        let revoked_seq = reqs.last().map(|r| r.event.envelope.seq);
        if let Some(act) = &act {
            self.with_notice(&act.record, &mut reqs).await?;
        }
        let mut receipt = self.commit_requests(reqs).await?;
        if let Some(seq) = revoked_seq {
            receipt.seq = seq;
        }
        Ok(RevokedKey {
            key: KeyRecord {
                revoked_at: Some(revoked_at.clone()),
                ..target
            },
            revoked_at,
            act_id: act.map(|a| a.act_id()),
            journal_seq: receipt.seq,
        })
    }

    /// Whether `person` is a live admin person of the tenant (ADR-0097 d8,
    /// d10).
    pub(crate) async fn is_admin_person(
        &self,
        tenant_id: Uuid,
        person: Option<&oml_domain::name::Name>,
    ) -> Result<bool, AppError> {
        let Some(person) = person else {
            return Ok(false);
        };
        Ok(self
            .journal
            .admin_standing(tenant_id)
            .await?
            .persons
            .iter()
            .any(|p| &p.user == person))
    }
}
