//! The operator's recovery of a dormant tenant (ADR-0097 d19, d20; the
//! operator's answers 21, 22 and 24). `oml operator recover <tenant>
//! --person <name> --email <address>`, run on the host with the tenant's
//! operator token, is admitted only when the server authentication table
//! reaches back 30 days and shows no admin key of the tenant authenticated
//! in them, an admin person's sign-in included, and no act against an
//! admin is open. Its request is journalled as
//! `operator.recovery_requested` with a `recovery` notice to every admin
//! person, emailed too, since a dormant admin sees nothing in-product.
//!
//! The server's sweep then completes it once 72 hours pass with no admin
//! sign-in: in one commit, every admin key of the tenant revoked, `cause:
//! recovery`, the named admin person's entry added, `operator.recovery`
//! with the window and the revoked keys, and a `recovery` notice. The
//! dormant admins keep their flags (decided by the operator, 2026-11-25),
//! so the tenant leaves `sole_admin`. Any admin authentication in the 72
//! hours cancels it, `operator.recovery_cancelled`. Each pass reads the
//! chain head before the requested row, and the row moves out of
//! `requested` once, so a second completion, from another replica or after
//! a restart, commits nothing.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::admin_act::AdminActCause;
use oml_domain::name::Name;
use oml_domain::notice::NoticeKind;
use oml_domain::usage::day_of;
use oml_domain::web::Email;
use oml_domain::{EventTime, Role, SignedEnvelope, event_types};

use super::super::notices::NoticeSpec;
use super::super::{App, commit_request};
use super::{Links, OPERATOR_PRINCIPAL, Sealer};
use crate::ports::{
    Clock, ConnectionStore, IdGen, MetadataFold, Scanner, Signer, StoreError, WebStore,
};
use crate::types::{
    AUTHENTICATION_RETENTION_DAYS, AppError, DAY_US, KeyRecord, OperatorRecovery, OutboundMail,
    RECOVERY_DORMANCY_DAYS, RecoverRefusal, RecoveryRecord, RecoveryState, RecoverySweep, WebEntry,
};

/// The UTC day `days` whole days before `now`.
fn day_before(now: &EventTime, days: i64) -> Result<String, AppError> {
    EventTime::from_unix_micros(
        now.unix_micros()
            .saturating_sub(days.saturating_mul(DAY_US)),
    )
    .map(|t| day_of(&t).to_owned())
    .ok_or_else(|| {
        AppError::Store(StoreError::Backend(format!(
            "{} less {days} days is not an event time",
            now.as_str()
        )))
    })
}

/// `payload` naming the recovery's id.
fn with_recovery_id(mut payload: Value, recovery_id: Uuid) -> Value {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("recovery_id".to_owned(), json!(recovery_id.to_string()));
    }
    payload
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: WebStore + ConnectionStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/operator/tenants/{tenant}/recover {person, email}`
    /// (ADR-0097 d19): the composition has checked the host's per-tenant
    /// token; no key acts. Refused `operator.recover_refused` with
    /// `dispute_open` while an act against an admin is open, `pending` while
    /// a recovery waits, `log_too_short` when the authentication table does
    /// not reach back 30 days, `admin_active` when an admin key of the
    /// tenant authenticated in them, and `person_taken` or `email_taken`.
    /// Then, in one commit, `operator.recovery_requested` and a `recovery`
    /// notice to every admin person; the answer carries the email for the
    /// composition to send.
    ///
    /// # Errors
    /// The refusals above, a tenant the store does not hold, and the
    /// store's.
    pub async fn operator_recover(
        &self,
        tenant_id: Uuid,
        person: Name,
        email: Email,
    ) -> Result<OperatorRecovery, AppError> {
        let refused = |r| Err(AppError::RecoverRefused(r));
        let prev = self
            .head_event(tenant_id)
            .await?
            .ok_or(AppError::NotFound)?;
        // Recovery's dormancy gate never opens during a live dispute
        // (Consequences: Q5(a) needs Q14's settlement verb).
        if !self.open_disputes(tenant_id).await?.is_empty() {
            return refused(RecoverRefusal::DisputeOpen);
        }
        if !self
            .journal
            .operator_recoveries(tenant_id, Some(RecoveryState::Requested))
            .await?
            .is_empty()
        {
            return refused(RecoverRefusal::Pending);
        }
        // Fail closed: a table that does not reach back 30 days, a fresh
        // install or purged rows, is never read as dormancy (answers 21,
        // 24).
        let now = self.clock.now();
        let coverage = self.journal.authentication_coverage().await?;
        if !coverage.covers(&now, RECOVERY_DORMANCY_DAYS) {
            return refused(RecoverRefusal::LogTooShort);
        }
        let since_day = day_before(&now, RECOVERY_DORMANCY_DAYS)?;
        if self
            .journal
            .admin_keys_authenticated_since(tenant_id, &since_day)
            .await?
            > 0
        {
            return refused(RecoverRefusal::AdminActive);
        }
        self.require_person_free(tenant_id, &person, &email).await?;
        let recovery_id = self.ids.new_id();
        let mut record = RecoveryRecord {
            recovery_id,
            person,
            email,
            state: RecoveryState::Requested,
            requested_at: now.clone(),
            settled_at: None,
        };
        let completes_after = record
            .completes_after()
            .map(|t| t.as_str().to_owned())
            .unwrap_or_default();
        let event = self.seal(
            Some(&prev),
            tenant_id,
            event_types::OPERATOR_RECOVERY_REQUESTED,
            OPERATOR_PRINCIPAL,
            None,
            with_recovery_id(
                json!({
                    "state": RecoveryState::Requested.as_str(),
                    "person": record.person.as_str(),
                    "email_digest": record.email.digest().to_prefixed(),
                    "window": {
                        "days": RECOVERY_DORMANCY_DAYS,
                        "since_day": since_day,
                        "coverage_since": coverage.since().map(|t| t.as_str().to_owned()),
                    },
                    "completes_after": completes_after,
                }),
                recovery_id,
            ),
        )?;
        record.requested_at = event.envelope.at.clone();
        let completes_after = record
            .completes_after()
            .map(|t| t.as_str().to_owned())
            .unwrap_or_default();
        let recipients = self.admin_persons(tenant_id).await?;
        let notified: Vec<Name> = recipients.iter().cloned().collect();
        let notice = self.notice_link(
            &event,
            None,
            NoticeSpec {
                kind: NoticeKind::Recovery,
                recipients,
                detail: json!({
                    "recovery_id": recovery_id.to_string(),
                    "state": RecoveryState::Requested.as_str(),
                    "person": record.person.as_str(),
                    "completes_after": completes_after,
                }),
            },
        )?;
        let links: Links = vec![
            (event, MetadataFold::RecoveryRequested(record.clone())),
            notice,
        ];
        self.commit_links(links).await?;
        let to = self.entry_emails(tenant_id, &notified).await?;
        let mail = OutboundMail {
            to,
            subject: "OML: the operator asked to recover a dormant tenant".to_owned(),
            body: format!(
                "The operator asked, from the host, to recover a tenant in which no admin key \
                 authenticated and no admin person signed in for {RECOVERY_DORMANCY_DAYS} days.\n\n\
                 Tenant: {tenant_id}\n\
                 Recovery: {recovery_id}\n\
                 New admin person: {person}\n\
                 Completes after: {completes_after}\n\n\
                 Any admin person's sign-in before then cancels it. Once it completes, every \
                 admin key of the tenant is revoked and the new admin person is added; the \
                 admin persons of today keep their flags.\n\n\
                 The request is journalled as operator.recovery_requested, and each admin person \
                 finds its notice in To do on the console.\n",
                person = record.person.as_str(),
            ),
        };
        Ok(OperatorRecovery {
            recovery: record,
            notified,
            revoked_keys: Vec::new(),
            mail,
        })
    }

    /// One pass of the recovery sweep in one tenant (ADR-0097 d19, answer
    /// 22). The requested recovery, if any, is cancelled when an admin key
    /// of the tenant authenticated on its request day or since, and
    /// completed once its 72 hours have passed with none. The chain head is
    /// read before the row, so a commit another replica made in between
    /// fails this one's chain and the pass is run again, finding the row
    /// settled. A completion whose person or address was taken in the
    /// meantime is refused by name and the request stays, which doctor
    /// reports as overdue.
    ///
    /// # Errors
    /// `person_taken` or `email_taken` for a completion, and the store's.
    pub async fn recovery_sweep(&self, tenant_id: Uuid) -> Result<RecoverySweep, AppError> {
        let mut sweep = RecoverySweep::default();
        let Some(prev) = self.head_event(tenant_id).await? else {
            return Ok(sweep);
        };
        let Some(recovery) = self
            .journal
            .operator_recoveries(tenant_id, Some(RecoveryState::Requested))
            .await?
            .into_iter()
            .next()
        else {
            return Ok(sweep);
        };
        let by = Sealer {
            tenant_id,
            principal: OPERATOR_PRINCIPAL.to_owned(),
            key_id: None,
        };
        if self
            .journal
            .admin_keys_authenticated_since(tenant_id, day_of(&recovery.requested_at))
            .await?
            > 0
        {
            let event = self.seal(
                Some(&prev),
                tenant_id,
                event_types::OPERATOR_RECOVERY_CANCELLED,
                &by.principal,
                None,
                with_recovery_id(
                    json!({
                        "state": RecoveryState::Cancelled.as_str(),
                        "person": recovery.person.as_str(),
                        "cause": "admin_sign_in",
                    }),
                    recovery.recovery_id,
                ),
            )?;
            self.commit_links(vec![(
                event,
                MetadataFold::RecoverySettled {
                    recovery_id: recovery.recovery_id,
                    state: RecoveryState::Cancelled,
                },
            )])
            .await?;
            sweep.cancelled = 1;
            return Ok(sweep);
        }
        let due = recovery
            .completes_after()
            .is_some_and(|t| self.clock.now().unix_micros() >= t.unix_micros());
        if !due {
            return Ok(sweep);
        }
        sweep
            .completed
            .push(self.complete_recovery(&by, prev, recovery).await?);
        Ok(sweep)
    }

    /// The completion's one commit: the server issuer, where the tenant has
    /// none yet; every admin key of the tenant revoked, `cause: recovery`;
    /// the named admin person's entry, flagged and accepted, added by the
    /// server issuer; `operator.recovery` with the window and the revoked
    /// keys; and a `recovery` notice to every admin person, the new one
    /// included. The dormant admins' entries are not touched.
    async fn complete_recovery(
        &self,
        by: &Sealer,
        prev: SignedEnvelope,
        mut recovery: RecoveryRecord,
    ) -> Result<OperatorRecovery, AppError> {
        let tenant_id = by.tenant_id;
        let recovery_id = recovery.recovery_id;
        self.require_person_free(tenant_id, &recovery.person, &recovery.email)
            .await?;
        let (issuer, mut reqs) = self.server_issuer(tenant_id, Some(&prev)).await?;
        let first = reqs.last().map_or(prev, |r| r.event.clone());
        let revoked = self.tenant_admin_keys(tenant_id).await?;
        let mut links: Links = Vec::new();
        let push = |links: &mut Links, event_type: &str, payload: Value, fold: MetadataFold| {
            let last = links.last().map_or(&first, |(e, _)| e);
            let event = self.seal(
                Some(last),
                tenant_id,
                event_type,
                &by.principal,
                by.key_id,
                with_recovery_id(payload, recovery_id),
            )?;
            links.push((event, fold));
            Ok::<(), AppError>(())
        };
        for key in &revoked {
            push(
                &mut links,
                event_types::KEY_REVOKED,
                json!({
                    "key_id": key.key_id.to_string(),
                    "revoked_by_key_id": Value::Null,
                    "actor_person": Value::Null,
                    "reason": Value::Null,
                    "cause": AdminActCause::Recovery.as_str(),
                }),
                MetadataFold::KeyRevoked { key_id: key.key_id },
            )?;
        }
        let entry = WebEntry {
            tenant_id,
            email: recovery.email.clone(),
            user: recovery.person.clone(),
            admin: true,
            google_subject: None,
            github_subject: None,
            added_by: issuer.key_id,
            terms_version: None,
            disabled_at: None,
            pending: false,
        };
        push(
            &mut links,
            event_types::WEB_EMAIL_ALLOWED,
            json!({
                "email_digest": entry.email.digest().to_prefixed(),
                "user": entry.user.as_str(),
                "admin": true,
                "by_key_id": issuer.key_id.to_string(),
                "pending": false,
                "cause": AdminActCause::Recovery.as_str(),
            }),
            MetadataFold::WebEmailAllowed(entry),
        )?;
        let revoked_ids: Vec<Uuid> = revoked.iter().map(|k| k.key_id).collect();
        push(
            &mut links,
            event_types::OPERATOR_RECOVERY,
            json!({
                "state": RecoveryState::Completed.as_str(),
                "person": recovery.person.as_str(),
                "email_digest": recovery.email.digest().to_prefixed(),
                "window": {
                    "days": RECOVERY_DORMANCY_DAYS,
                    "requested_at": recovery.requested_at.as_str(),
                    "completes_after": recovery.completes_after().map(|t| t.as_str().to_owned()),
                },
                "revoked_keys": revoked_ids.iter().map(Uuid::to_string).collect::<Vec<_>>(),
            }),
            MetadataFold::RecoverySettled {
                recovery_id,
                state: RecoveryState::Completed,
            },
        )?;
        let at = links.last().map(|(e, _)| e.envelope.at.clone());
        let mut recipients = self.admin_persons(tenant_id).await?;
        recipients.insert(recovery.person.clone());
        let notified: Vec<Name> = recipients.iter().cloned().collect();
        let last = links.last().map(|(e, _)| e.clone()).expect("a link");
        links.push(self.notice_link(
            &last,
            None,
            NoticeSpec {
                kind: NoticeKind::Recovery,
                recipients,
                detail: json!({
                    "recovery_id": recovery_id.to_string(),
                    "state": RecoveryState::Completed.as_str(),
                    "person": recovery.person.as_str(),
                    "revoked_keys": revoked_ids.len(),
                }),
            },
        )?);
        reqs.extend(
            links
                .into_iter()
                .map(|(event, fold)| commit_request(event, fold, None)),
        );
        self.commit_requests(reqs).await?;
        recovery.state = RecoveryState::Completed;
        recovery.settled_at = at;
        let mut to = self.entry_emails(tenant_id, &notified).await?;
        if !to.contains(&recovery.email) {
            to.push(recovery.email.clone());
        }
        let mail = OutboundMail {
            to,
            subject: "OML: the operator's recovery of a dormant tenant completed".to_owned(),
            body: format!(
                "No admin person signed in within the 72 hours after the operator's request, so \
                 the server completed the recovery.\n\n\
                 Tenant: {tenant_id}\n\
                 Recovery: {recovery_id}\n\
                 New admin person: {person}\n\
                 Admin keys revoked: {revoked}\n\n\
                 The admin persons before the recovery keep their flags, so the new admin \
                 person's widening acts wait on one of them to confirm. The recovery is \
                 journalled as operator.recovery.\n",
                person = recovery.person.as_str(),
                revoked = revoked_ids.len(),
            ),
        };
        Ok(OperatorRecovery {
            recovery,
            notified,
            revoked_keys: revoked_ids,
            mail,
        })
    }

    /// Refuses a person whose name the tenant holds, as a workspace, a
    /// session key or an entry's user, and an address an entry holds.
    async fn require_person_free(
        &self,
        tenant_id: Uuid,
        person: &Name,
        email: &Email,
    ) -> Result<(), AppError> {
        if !self.user_name_free(tenant_id, person).await?
            || self.entry_of_user(tenant_id, person).await?.is_some()
        {
            return Err(AppError::RecoverRefused(RecoverRefusal::PersonTaken));
        }
        if self
            .journal
            .web_entry_by_email(&email.digest())
            .await?
            .is_some()
        {
            return Err(AppError::RecoverRefused(RecoverRefusal::EmailTaken));
        }
        Ok(())
    }

    /// The tenant's entry whose user is `person`, if any.
    async fn entry_of_user(
        &self,
        tenant_id: Uuid,
        person: &Name,
    ) -> Result<Option<WebEntry>, AppError> {
        const PAGE: usize = 200;
        let mut after = None;
        loop {
            let page = self
                .journal
                .web_entries(tenant_id, after.as_ref(), PAGE)
                .await?;
            if let Some(found) = page.iter().find(|e| &e.user == person) {
                return Ok(Some(found.clone()));
            }
            match page.last() {
                Some(last) if page.len() == PAGE => after = Some(last.email.digest()),
                _ => return Ok(None),
            }
        }
    }

    /// Every live admin key of the tenant: each admin person's linked
    /// bearer keys and admin session keys, and every bearer admin key with
    /// no accepted link.
    async fn tenant_admin_keys(&self, tenant_id: Uuid) -> Result<Vec<KeyRecord>, AppError> {
        let mut keys: BTreeMap<Uuid, KeyRecord> = BTreeMap::new();
        let live_admin = |k: &KeyRecord| k.role == Role::Admin && k.revoked_at.is_none();
        for person in self.admin_persons(tenant_id).await? {
            for key in self.admin_keys_of(tenant_id, &person).await? {
                keys.insert(key.key_id, key);
            }
            for key in self.journal.web_session_keys(tenant_id, &person).await? {
                if live_admin(&key) {
                    keys.insert(key.key_id, key);
                }
            }
        }
        for key in self.journal.web_unlinked_admin_keys(tenant_id).await? {
            if live_admin(&key) {
                keys.insert(key.key_id, key);
            }
        }
        Ok(keys.into_values().collect())
    }

    /// Deletes the tenant's authentication rows of a day more than 30 days
    /// back (answer 24's purge sweep), and answers how many.
    ///
    /// # Errors
    /// The store's.
    pub async fn purge_authentications(&self, tenant_id: Uuid) -> Result<u64, AppError> {
        let before = day_before(&self.clock.now(), AUTHENTICATION_RETENTION_DAYS)?;
        Ok(self
            .journal
            .purge_authentications(tenant_id, &before)
            .await?)
    }
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: crate::ports::JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The tenant's requested recoveries, and those of them past their 72
    /// hours (`admin.recovery_pending`, critical, and
    /// `admin.recovery_overdue`, ADR-0097 d19, answer 22).
    pub(crate) async fn recovery_counts(&self, tenant_id: Uuid) -> Result<(u64, u64), AppError> {
        let requested = self
            .journal
            .operator_recoveries(tenant_id, Some(RecoveryState::Requested))
            .await?;
        let now = self.clock.now().unix_micros();
        let overdue = requested
            .iter()
            .filter(|r| r.completes_after().is_some_and(|t| now >= t.unix_micros()))
            .count();
        Ok((requested.len() as u64, overdue as u64))
    }

    /// The whole days the server authentication table covers now, deployment
    /// wide (`auth.log_coverage`, answer 24): 0 with no row held.
    ///
    /// # Errors
    /// The store's.
    pub async fn authentication_coverage_days(&self) -> Result<i64, AppError> {
        Ok(self
            .journal
            .authentication_coverage()
            .await?
            .days(&self.clock.now()))
    }
}
