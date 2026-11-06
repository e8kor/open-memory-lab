//! Per-account usage metering's use cases (ADR-0071): an account's limits
//! from its quota profile and overrides, the ceiling a metered commit
//! carries, the pre-check before a write is signed, the usage reads, the
//! ledger export, a profile's assignment and the drift report. The counters
//! are the store's; the pages and the endpoints are the transport's.

use serde_json::json;
use uuid::Uuid;

use oml_domain::name::{Name, NamedKind};
use oml_domain::signup::{Quota, Quotas, check_charge};
use oml_domain::usage::{Meter, Month, meter_state, percent};
use oml_domain::{Role, SignedEnvelope, event_types};

use super::{App, Caller, require_role};
use crate::ports::{
    Ceiling, Clock, CommitReceipt, IdGen, IdempotencyPut, JournalStore, MetadataFold, Scanner,
    Signer,
};
use crate::types::{
    AccountUsage, AppError, Bearer, KeyRecord, LedgerRow, MeterDrift, MeterReading,
    ProfileAssignment, QuotaProfiles, UserUsage,
};

/// The drift report (ADR-0071 d3, d5): each meter whose counter differs
/// from its rows, and each account on an undeclared profile, with its
/// user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageDriftReport {
    pub drift: Vec<(MeterDrift, Option<Name>)>,
    pub undeclared: Vec<(Uuid, Option<Name>, String)>,
}

/// The limits an account is held to, and where they come from
/// (ADR-0071 d5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLimits {
    /// The profile, `None` for an account under no limits.
    pub profile: Option<String>,
    /// Whether the configuration declares that profile.
    pub declared: bool,
    /// The limits with the account's overrides; `None` under no limits.
    pub quotas: Option<Quotas>,
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: JournalStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    /// The app with the deployment's quota profiles (ADR-0071 d5); without
    /// them, `default` alone, with ADR-0068's limits.
    #[must_use]
    pub fn with_quota_profiles(self, profiles: QuotaProfiles) -> Self {
        Self {
            profiles: std::sync::Arc::new(profiles),
            ..self
        }
    }

    /// The deployment's quota profiles.
    #[must_use]
    pub fn quota_profiles(&self) -> &QuotaProfiles {
        &self.profiles
    }

    /// An account's limits (ADR-0071 d5): its assigned profile's, an
    /// undeclared one's being zero; else, in the sign-up tenant,
    /// `quota.signup_profile`'s; else none. The account's overrides apply
    /// to whichever profile holds.
    pub async fn account_limits(
        &self,
        tenant_id: Uuid,
        root: Uuid,
    ) -> Result<AccountLimits, AppError> {
        let assigned = self.journal.account_profile(tenant_id, root).await?;
        let in_signup_tenant = self
            .signup_policy()
            .is_some_and(|p| p.tenant_id == tenant_id);
        let (profile, declared, base) = match assigned {
            Some(name) => match self.profiles.get(&name) {
                Some(quotas) => (Some(name), true, Some(quotas)),
                None => (Some(name), false, Some(Quotas::ZERO)),
            },
            None if in_signup_tenant => {
                let name = self.profiles.signup_profile.clone();
                let quotas = self.profiles.get(&name);
                let declared = quotas.is_some();
                (Some(name), declared, Some(quotas.unwrap_or(Quotas::ZERO)))
            }
            None => (None, true, None),
        };
        let quotas = match base {
            Some(base) if declared => Some(
                base.with_overrides(&self.journal.user_quota_overrides(tenant_id, root).await?),
            ),
            other => other,
        };
        Ok(AccountLimits {
            profile,
            declared,
            quotas,
        })
    }

    /// The ceiling a commit charging `root`'s account carries, `None` for
    /// no account or one under no limits (ADR-0071 d3).
    pub(crate) async fn ceiling_for(
        &self,
        tenant_id: Uuid,
        root: Option<Uuid>,
    ) -> Result<Option<Ceiling>, AppError> {
        let Some(root) = root else { return Ok(None) };
        Ok(self
            .account_limits(tenant_id, root)
            .await?
            .quotas
            .map(|quotas| Ceiling {
                user_root: root,
                quotas,
            }))
    }

    /// A commit that charges an account, carrying its ceiling: the store
    /// refuses it, writing nothing, past a limit (ADR-0071 d3).
    pub(crate) async fn commit_charged(
        &self,
        event: SignedEnvelope,
        fold: MetadataFold,
        idempotency: Option<IdempotencyPut>,
        ceiling: Option<Ceiling>,
    ) -> Result<CommitReceipt, AppError> {
        let mut req = super::commit_request(event, fold, idempotency);
        req.ceiling = ceiling;
        Ok(self.journal.commit(req).await?)
    }

    /// The pre-check before a write is signed: each `(quota, delta)` fits
    /// beside what the key's account holds, or `limit.quota` naming the
    /// first that does not. The commit's ceiling is the check that holds;
    /// this one spares the signature (ADR-0071 d3).
    pub(crate) async fn require_charge(
        &self,
        key: &KeyRecord,
        charges: &[(Quota, u64)],
    ) -> Result<(), AppError> {
        let Some(root) = key.user else { return Ok(()) };
        let Some(quotas) = self.account_limits(key.tenant_id, root).await?.quotas else {
            return Ok(());
        };
        let usage = self
            .journal
            .user_usage(key.tenant_id, root, &self.clock.now())
            .await?;
        for (quota, delta) in charges {
            check_charge(*quota, quotas.get(*quota), usage.get(*quota), *delta)
                .map_err(AppError::QuotaReached)?;
        }
        Ok(())
    }

    /// The usage of `root`'s account: each meter against its limit, this
    /// month (ADR-0071 d5, d6).
    pub async fn usage_of(&self, tenant_id: Uuid, root: Uuid) -> Result<AccountUsage, AppError> {
        let now = self.clock.now();
        let limits = self.account_limits(tenant_id, root).await?;
        let usage = self.journal.user_usage(tenant_id, root, &now).await?;
        let user = self
            .journal
            .workarea(tenant_id, root)
            .await?
            .filter(|w| w.user_root)
            .map(|w| w.name);
        Ok(account_usage(root, user, &limits, &usage, Month::of(&now)))
    }

    /// `GET /v1/account/usage`: the caller's own account, by its key and
    /// never by a parameter, so no request names another account; a tenant
    /// key belongs to none (ADR-0071 d1, d6).
    pub async fn account_usage(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<AccountUsage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        self.own_usage(&caller).await
    }

    /// The account usage of a caller already authenticated, as the pages
    /// read it.
    pub async fn own_usage(&self, caller: &Caller) -> Result<AccountUsage, AppError> {
        let root = caller.key.user.ok_or(AppError::NotFound)?;
        self.usage_of(caller.key.tenant_id, root).await
    }

    /// The account `account` names, by root id or user name, in the admin's
    /// tenant: a user root, or `access.not_found`.
    async fn account_named(&self, tenant_id: Uuid, account: &str) -> Result<Uuid, AppError> {
        let id = match Uuid::parse_str(account) {
            Ok(id) => Some(id),
            Err(_) => match Name::parse(account) {
                Ok(name) => {
                    self.journal
                        .resolve_name(tenant_id, NamedKind::Workarea, &name)
                        .await?
                }
                Err(_) => None,
            },
        };
        let Some(id) = id else {
            return Err(AppError::NotFound);
        };
        self.journal
            .workarea(tenant_id, id)
            .await?
            .filter(|w| w.user_root)
            .map(|w| w.workarea_id)
            .ok_or(AppError::NotFound)
    }

    /// `GET /v2/admin/accounts/{account}/usage` (ADR-0071 d6).
    pub async fn admin_account_usage(
        &self,
        bearer: impl Into<Bearer<'_>>,
        account: &str,
    ) -> Result<AccountUsage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller.key, Role::Admin, "account_usage")?;
        let root = self.account_named(caller.key.tenant_id, account).await?;
        self.usage_of(caller.key.tenant_id, root).await
    }

    /// Every account of the admin's tenant with its usage this month, as
    /// `/web/admin/users` lists them (ADR-0071 d6).
    pub async fn admin_accounts(&self, admin: &Caller) -> Result<Vec<AccountUsage>, AppError> {
        require_role(&admin.key, Role::Admin, "account_usage")?;
        let tenant_id = admin.key.tenant_id;
        let now = self.clock.now();
        let month = Month::of(&now);
        let mut out = Vec::new();
        for row in self.journal.usage_accounts(tenant_id, month).await? {
            let limits = self.account_limits(tenant_id, row.user_root).await?;
            let user = self
                .journal
                .workarea(tenant_id, row.user_root)
                .await?
                .map(|w| w.name);
            out.push(account_usage(
                row.user_root,
                user,
                &limits,
                &row.usage,
                month,
            ));
        }
        Ok(out)
    }

    /// `GET /v2/admin/usage?period=YYYY-MM`: the month's ledger rows, every
    /// account, each with its user (ADR-0071 d4, d6).
    pub async fn admin_usage_ledger(
        &self,
        bearer: impl Into<Bearer<'_>>,
        month: Month,
    ) -> Result<Vec<(LedgerRow, Option<Name>)>, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller.key, Role::Admin, "usage_ledger")?;
        let tenant_id = caller.key.tenant_id;
        let rows = self.journal.usage_ledger(tenant_id, month).await?;
        let mut names: std::collections::BTreeMap<Uuid, Option<Name>> = Default::default();
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let name = match names.get(&row.user_root) {
                Some(name) => name.clone(),
                None => {
                    let name = self
                        .journal
                        .workarea(tenant_id, row.user_root)
                        .await?
                        .map(|w| w.name);
                    names.insert(row.user_root, name.clone());
                    name
                }
            };
            out.push((row, name));
        }
        Ok(out)
    }

    /// `GET /v2/admin/usage/drift`: each meter whose counter differs from
    /// its rows this month, and each account on a profile the configuration
    /// does not declare (ADR-0071 d3, d5).
    pub async fn admin_usage_drift(
        &self,
        bearer: impl Into<Bearer<'_>>,
    ) -> Result<UsageDriftReport, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller.key, Role::Admin, "usage_drift")?;
        let tenant_id = caller.key.tenant_id;
        let month = Month::of(&self.clock.now());
        let name_of = |root: Uuid| async move {
            Ok::<_, AppError>(
                self.journal
                    .workarea(tenant_id, root)
                    .await?
                    .map(|w| w.name),
            )
        };
        let mut report = UsageDriftReport::default();
        for drift in self.journal.usage_drift(tenant_id, month).await? {
            let name = name_of(drift.user_root).await?;
            report.drift.push((drift, name));
        }
        for row in self.journal.usage_accounts(tenant_id, month).await? {
            if let Some(profile) = row.profile.filter(|p| self.profiles.get(p).is_none()) {
                let name = name_of(row.user_root).await?;
                report.undeclared.push((row.user_root, name, profile));
            }
        }
        Ok(report)
    }

    /// `PUT /v2/admin/accounts/{account}/profile` (ADR-0071 d5): a declared
    /// profile and overrides within each quota's range, journalled as
    /// `account.profile_set`, replacing the account's; then its usage.
    pub async fn admin_set_profile(
        &self,
        bearer: impl Into<Bearer<'_>>,
        account: &str,
        assignment: &ProfileAssignment,
    ) -> Result<AccountUsage, AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        require_role(&caller.key, Role::Admin, "account_profile")?;
        if self.profiles.get(&assignment.profile).is_none() {
            return Err(AppError::WebField {
                field: "profile",
                message: "the configuration declares no profile of that name",
            });
        }
        if assignment
            .overrides
            .iter()
            .any(|(quota, limit)| *limit > quota.max_limit())
        {
            return Err(AppError::WebField {
                field: "overrides",
                message: "a limit is a whole number from 0 to its quota's maximum",
            });
        }
        let tenant_id = caller.key.tenant_id;
        let root = self.account_named(tenant_id, account).await?;
        let prev = self.head_event(tenant_id).await?;
        let overrides: serde_json::Map<String, serde_json::Value> = assignment
            .overrides
            .iter()
            .map(|(q, l)| (q.as_str().to_owned(), json!(l)))
            .collect();
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::ACCOUNT_PROFILE_SET,
            &caller.key.principal,
            Some(caller.key.key_id),
            json!({
                "user_root": root.to_string(),
                "profile": assignment.profile,
                "overrides": overrides,
            }),
        )?;
        self.commit(
            event,
            MetadataFold::AccountProfileSet {
                user_root: root,
                profile: Some(assignment.profile.clone()),
                overrides: assignment.overrides.clone(),
            },
            None,
        )
        .await?;
        self.usage_of(tenant_id, root).await
    }
}

/// An account's meters against its limits, in [`Meter::ALL`]'s order.
fn account_usage(
    root: Uuid,
    user: Option<Name>,
    limits: &AccountLimits,
    usage: &UserUsage,
    month: Month,
) -> AccountUsage {
    let meters = Meter::ALL
        .into_iter()
        .map(|meter| {
            let limit = limits
                .quotas
                .zip(meter.quota())
                .map(|(q, quota)| q.get(quota))
                .filter(|l| *l != oml_domain::signup::UNLIMITED);
            let used = usage.meter(meter);
            MeterReading {
                meter,
                used,
                limit,
                percent: percent(limit, used),
                state: meter_state(limit, used),
            }
        })
        .collect();
    AccountUsage {
        user_root: root,
        user,
        profile: limits.profile.clone(),
        profile_declared: limits.declared,
        month,
        meters,
    }
}
