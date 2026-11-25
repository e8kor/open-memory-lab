//! An admin's investigations (ADR-0089 d10, d15): an admin key reads claim
//! text only under an open investigation of its own, which carries the
//! reason once for every read made under it. The reason is stored in clear
//! and scanned as a verdict's is. An investigation stays open until its key
//! closes it (the operator, 2026-11-17: no idle expiry); `oml doctor` names
//! one open for more than seven days.

use serde_json::json;
use uuid::Uuid;

use oml_domain::event_types;

use super::App;
use crate::ports::{Clock, IdGen, JournalStore, MetadataFold, Scanner, Signer};
use crate::types::{AppError, Bearer, Investigation, KeyRecord};

/// The longest a reason is, in bytes: one sentence or a ticket's title.
pub const INVESTIGATION_REASON_MAX_BYTES: usize = 1024;

/// How long an investigation may stay open before `oml doctor` names it.
pub const INVESTIGATION_OPEN_LONG_SECONDS: i64 = 7 * 24 * 3600;

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// `POST /v2/admin/investigations {reason}`: opens an investigation of
    /// the admin key, journalled as `admin.investigation_opened` with its
    /// reason in clear. A reason that is empty or longer than
    /// [`INVESTIGATION_REASON_MAX_BYTES`] is [`AppError::AdminField`]; one
    /// the scan vetoes is refused as a verdict's reason is.
    ///
    /// # Errors
    /// The refusals above, a key that is not an admin's, and the store's.
    pub async fn open_investigation(
        &self,
        bearer: impl Into<Bearer<'_>>,
        reason: &str,
    ) -> Result<Investigation, AppError> {
        let admin = self.admin_caller(bearer.into(), "investigate").await?.key;
        let reason = reason.trim();
        if reason.is_empty() || reason.len() > INVESTIGATION_REASON_MAX_BYTES {
            return Err(AppError::AdminField {
                field: "/reason",
                message: format!(
                    "an investigation's reason is 1 to {INVESTIGATION_REASON_MAX_BYTES} bytes"
                ),
            });
        }
        self.scan_veto_only(admin.tenant_id, "reason", reason)?;
        let investigation_id = self.ids.new_id();
        let prev = self.head_event(admin.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            admin.tenant_id,
            event_types::ADMIN_INVESTIGATION_OPENED,
            &admin.principal,
            Some(admin.key_id),
            json!({
                "investigation_id": investigation_id.to_string(),
                "reason": reason,
            }),
        )?;
        let opened_at = event.envelope.at.clone();
        self.commit(
            event,
            MetadataFold::InvestigationOpened {
                investigation_id,
                key_id: admin.key_id,
                reason: reason.to_owned(),
            },
            None,
        )
        .await?;
        Ok(Investigation {
            investigation_id,
            key_id: admin.key_id,
            reason: reason.to_owned(),
            opened_at,
            reads: 0,
            closed_at: None,
        })
    }

    /// `POST /v2/admin/investigations/{id}/close`: the key closes its own
    /// open investigation, journalled as `admin.investigation_closed`. One
    /// the tenant does not hold, or another key's, is
    /// [`AppError::NotFound`]; one already closed is answered as it is,
    /// journalling nothing.
    ///
    /// # Errors
    /// The refusals above, a key that is not an admin's, and the store's.
    pub async fn close_investigation(
        &self,
        bearer: impl Into<Bearer<'_>>,
        investigation_id: Uuid,
    ) -> Result<Investigation, AppError> {
        let admin = self.admin_caller(bearer.into(), "investigate").await?.key;
        let found = self
            .journal
            .investigation(admin.tenant_id, investigation_id)
            .await?
            .filter(|i| i.key_id == admin.key_id)
            .ok_or(AppError::NotFound)?;
        if found.closed_at.is_some() {
            return Ok(found);
        }
        let prev = self.head_event(admin.tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            admin.tenant_id,
            event_types::ADMIN_INVESTIGATION_CLOSED,
            &admin.principal,
            Some(admin.key_id),
            json!({ "investigation_id": investigation_id.to_string() }),
        )?;
        let closed_at = event.envelope.at.clone();
        self.commit(
            event,
            MetadataFold::InvestigationClosed {
                investigation_id,
                key_id: admin.key_id,
            },
            None,
        )
        .await?;
        Ok(Investigation {
            closed_at: Some(closed_at),
            ..found
        })
    }

    /// `GET /v2/admin/investigations?state=open|closed`: the admin key's own
    /// investigations in that state, oldest first, each with its reason,
    /// read count and age (ADR-0089 d15).
    ///
    /// # Errors
    /// A key that is not an admin's, and the store's.
    pub async fn investigations(
        &self,
        bearer: impl Into<Bearer<'_>>,
        open: bool,
    ) -> Result<Vec<Investigation>, AppError> {
        let admin = self.admin_caller(bearer.into(), "investigate").await?.key;
        let all = self
            .journal
            .investigations(admin.tenant_id, Some(admin.key_id), open)
            .await?;
        Ok(all
            .into_iter()
            .filter(|i| open || i.closed_at.is_some())
            .collect())
    }

    /// The server's clock now, for an investigation's age.
    #[must_use]
    pub fn now(&self) -> oml_domain::EventTime {
        self.clock.now()
    }

    /// The investigation an admin read names, checked open and the key's
    /// own, else [`AppError::InvestigationRequired`] naming `field`. The
    /// read's commit checks it again (ADR-0089 d10).
    pub(crate) async fn require_investigation(
        &self,
        admin: &KeyRecord,
        investigation: Option<Uuid>,
        field: &'static str,
    ) -> Result<Uuid, AppError> {
        let id = investigation.ok_or(AppError::InvestigationRequired { field })?;
        self.journal
            .investigation(admin.tenant_id, id)
            .await?
            .filter(|i| i.key_id == admin.key_id && i.closed_at.is_none())
            .map(|i| i.investigation_id)
            .ok_or(AppError::InvestigationRequired { field })
    }
}
