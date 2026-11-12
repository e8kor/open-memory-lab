//! Open sign-up's use cases (ADR-0068): a person's own sign-up after the
//! terms page, the quotas of the sign-up tenant's people and the per-key
//! request rate, an admin's quota override and disable switch, the
//! deletion request and its closure, the onboarding status, and the
//! person's export. The pages are the transport's.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde_json::{Value, json};
use uuid::Uuid;

use oml_domain::name::{Name, NamedKind, Reference, UNIVERSE};
use oml_domain::signup::{QUOTA_LIMIT_MAX, Quota, Quotas};
use oml_domain::{Role, event_types};

use super::{App, Caller, require_role};
use crate::ports::{
    Clock, ConnectionStore, IdGen, MetadataFold, Scanner, Signer, StoreError, WebStore,
};
use crate::types::{
    AdminSearch, AppError, Bearer, Credential, DeletionRequest, KeyRecord, SignupPolicy, UserUsage,
    Whoami,
};
use oml_domain::MemoryStatus;

/// How many times a sign-up draws a user name before it is refused
/// `name.taken` (ADR-0068 d2).
pub const DRAWS_MAX: usize = 3;

/// The most memories the export and the status read (ADR-0068 d6); a
/// person's quota is below it unless an admin raised it past.
pub const EXPORT_MAX: usize = 100_000;

/// How many times one commit of a multi-commit act that follows the act's
/// first commit retries after a chain-head conflict: the transport's own
/// retry count (#266).
const STEP_CHAIN_RETRIES: usize = 16;

/// One commit of a multi-commit act after the act's first commit, retried
/// alone on a chain-head conflict, so a retry never re-runs, and so never
/// journals twice, a commit that already landed (#266). A conflict that
/// outlasts the retries is answered as the store unavailable, which the
/// transport does not retry: what landed stands, the claim on its
/// `Idempotency-Key` is kept, and the act finishes when sent again under a
/// new key.
async fn later_step<T, F, Fut>(mut step: F) -> Result<T, AppError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    let mut attempt = 0;
    loop {
        match step().await {
            Err(AppError::Store(
                StoreError::SeqConflict { .. } | StoreError::PrevHashConflict { .. },
            )) => {
                attempt += 1;
                if attempt >= STEP_CHAIN_RETRIES {
                    return Err(AppError::Store(StoreError::Backend(
                        "chain head contention after the act's first commit; what it journalled stands, and the act finishes when sent again under a new Idempotency-Key"
                            .to_owned(),
                    )));
                }
            }
            other => return other,
        }
    }
}

/// Open sign-up's configuration and the process's counters: each key's
/// requests in its current minute, and the keys of the sign-up tenant
/// that authenticated since the process started (ADR-0068 d5, d10).
#[derive(Debug)]
pub struct SignupState {
    policy: SignupPolicy,
    /// Per key: the UTC minute, the requests in it, and the limit read at
    /// its start.
    windows: Mutex<BTreeMap<Uuid, (i64, u64, u64)>>,
}

impl SignupState {
    #[must_use]
    pub fn new(policy: SignupPolicy) -> Self {
        Self {
            policy,
            windows: Mutex::new(BTreeMap::new()),
        }
    }
}

/// What disabling a person did (ADR-0068 d8): the bearer and connector
/// keys cut off on `universe`, the keys of the root skipped because they
/// were gone by the time their denial read them, and the seq of
/// `web.user_disabled`. The HTTP answer carries the count and the seq; a
/// skipped key is left out of the count and named nowhere else (#277).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDisabled {
    pub keys_cut_off: usize,
    pub keys_skipped: Vec<Uuid>,
    pub journal_seq: u64,
}

/// What `GET /v2/whoami` reads of the person beside the key's record: the
/// session check of every console page reads none of it (#286); the
/// default is the block; the Start page asks for its onboarding status too
/// (ADR-0070 d5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonAsk {
    /// `person=false`: no person block, and none of its reads.
    Omit,
    /// The person block, as `whoami` has served it since v0.7.0.
    Person,
    /// `start=true`: the person block with `start`.
    WithStart,
}

/// The person a key names, as `GET /v2/whoami` serves it (ADR-0070 d5):
/// `limits` is their usage and quotas where they are under the sign-up
/// quotas.
#[derive(Debug, Clone)]
pub struct Person {
    pub user: Name,
    pub limits: Option<(UserUsage, Quotas)>,
    pub deletion_requested_at: Option<oml_domain::EventTime>,
    /// The onboarding status, read only when asked (ADR-0070 d5).
    pub start: Option<StartStatus>,
}

/// The least per-minute request rate a person's session keys get, whatever
/// their quota says: a page view of the console spends two to four
/// requests, so sixty keep a person on their own pages (ADR-0070, the
/// seventh table of phase (b); chosen, not measured, #170).
pub const SESSION_REQUESTS_PER_MINUTE_MIN: u64 = 60;

/// The requests a minute a key of `credential` gets under a quota of
/// `quota`: a person's own console keeps a floor of its own, so a limit set
/// below it binds their agent keys alone and no quota locks a person out of
/// the page that shows it (ADR-0070, the seventh table of phase (b)).
fn request_limit(credential: Credential, quota: u64) -> u64 {
    if credential == Credential::Session {
        quota.max(SESSION_REQUESTS_PER_MINUTE_MIN)
    } else {
        quota
    }
}

/// One person of the sign-up tenant as the admin's users page lists them
/// (ADR-0068 d5, d6, d8).
#[derive(Debug, Clone)]
pub struct UserRow {
    pub user: Name,
    pub root: Option<Uuid>,
    pub signed_up: bool,
    pub disabled: bool,
    /// Whether the tenant is the sign-up tenant, whose people are under
    /// quotas (ADR-0068 d5).
    pub under_quotas: bool,
    pub usage: UserUsage,
    pub quotas: Quotas,
    /// Each live admin key whose row names the person's root, by name (by
    /// id for a key that predates names): a corrupt row no verb makes,
    /// which refuses the person's disable and which `oml doctor` warns
    /// about (ADR-0068 d8, d11; #277).
    pub admin_keys: Vec<String>,
}

/// The onboarding page's state (ADR-0068 d10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartStatus {
    pub connected: bool,
    /// The newest memory's state: held or promoted; `None` before one.
    pub first_memory: Option<MemoryStatus>,
}

impl<J, S, C, I, Sc> App<J, S, C, I, Sc>
where
    J: WebStore + ConnectionStore,
    S: Signer,
    C: Clock,
    I: IdGen,
    Sc: Scanner,
{
    // -- the admin's users page ---------------------------------------------

    /// `GET /v2/web/users`'s people (ADR-0070 d5): the sign-up tenant's people,
    /// as `<admin>-admin` reads them: each user with an entry, whether they
    /// signed up themselves, whether disabled, what they hold and their limits
    /// (ADR-0068 d5).
    pub async fn list_web_users(&self, admin: &Caller) -> Result<Vec<UserRow>, AppError> {
        require_role(&admin.key, Role::Admin, "web_users")?;
        let tenant_id = admin.key.tenant_id;
        let mut by_user: BTreeMap<Name, (bool, bool)> = BTreeMap::new();
        let mut after = None;
        loop {
            let page = self
                .journal
                .web_entries(tenant_id, after.as_ref(), 500)
                .await?;
            let Some(last) = page.last() else { break };
            after = Some(last.email.digest());
            for e in &page {
                let row = by_user.entry(e.user.clone()).or_insert((false, false));
                row.0 |= e.terms_version.is_some();
                row.1 |= e.disabled_at.is_some();
            }
            if page.len() < 500 {
                break;
            }
        }
        let under_quotas = self
            .signup_policy()
            .is_some_and(|p| p.tenant_id == tenant_id);
        let mut out = Vec::new();
        for (user, (signed_up, disabled)) in by_user {
            let root = self.signup_root(tenant_id, &user).await?;
            let (usage, quotas, admin_keys) = match root {
                Some(root) => (
                    self.journal
                        .user_usage(tenant_id, root, &self.clock.now())
                        .await?,
                    self.quotas_of(tenant_id, root).await?,
                    self.admin_keys_under(tenant_id, root).await?,
                ),
                None => (UserUsage::default(), self.default_quotas(), Vec::new()),
            };
            out.push(UserRow {
                user,
                root,
                signed_up,
                disabled,
                under_quotas,
                usage,
                quotas,
                admin_keys,
            });
        }
        Ok(out)
    }

    /// The live admin keys whose row names `root`, by name or else by id:
    /// rows no verb makes, since issuance never gives an admin key a user
    /// (ADR-0068 d8, d11; #277).
    async fn admin_keys_under(&self, tenant_id: Uuid, root: Uuid) -> Result<Vec<String>, AppError> {
        Ok(self
            .journal
            .web_user_keys(tenant_id, root)
            .await?
            .into_iter()
            .filter(|k| k.role == Role::Admin && k.revoked_at.is_none())
            .map(|k| {
                k.name
                    .map_or_else(|| k.key_id.to_string(), |n| n.as_str().to_owned())
            })
            .collect())
    }

    /// `POST /v2/web/users/{user}/quota` (ADR-0070 d5 #8; ADR-0068 d5): one
    /// person's override of one quota, or with `None` the default again,
    /// journalled as `web.quota_set`.
    pub async fn set_web_user_quota(
        &self,
        admin: &Caller,
        user: &Name,
        quota: Quota,
        limit: Option<u64>,
    ) -> Result<(), AppError> {
        require_role(&admin.key, Role::Admin, "web_users")?;
        if limit.is_some_and(|l| l > quota.max_limit()) {
            return Err(AppError::WebField {
                field: "limit",
                message: if quota.max_limit() == QUOTA_LIMIT_MAX {
                    "a quota's limit is a whole number from 0 to 1000000"
                } else {
                    "a byte limit is a whole number from 0 to 1000000000000"
                },
            });
        }
        let tenant_id = admin.key.tenant_id;
        let root = self
            .signup_root(tenant_id, user)
            .await?
            .ok_or(AppError::NotFound)?;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::WEB_QUOTA_SET,
            &admin.key.principal,
            Some(admin.key.key_id),
            json!({
                "user": user.as_str(),
                "user_root": root.to_string(),
                "quota": quota.as_str(),
                "limit": limit,
            }),
        )?;
        self.commit(
            event,
            MetadataFold::QuotaSet {
                user_root: root,
                quota,
                limit,
            },
            None,
        )
        .await?;
        if quota == Quota::RequestsPerMinute {
            self.forget_windows();
        }
        Ok(())
    }

    /// `POST /v2/web/users/{user}/disable` (ADR-0070 d5 #9; ADR-0068 d8): for a
    /// non-admin person the tenant holds, `web.user_disabled`, which disables
    /// every entry of the user and ends their sessions, then one `key.denied`
    /// on `universe` for each live bearer and connector key of the root.
    /// Session keys are reached only from sessions, which ended. Each
    /// `key.denied` retries alone on a chain-head conflict (`later_step`), so a
    /// conflict never journals `web.user_disabled` twice in one call (#266).
    pub async fn disable_web_user(
        &self,
        admin: &Caller,
        user: &Name,
    ) -> Result<UserDisabled, AppError> {
        require_role(&admin.key, Role::Admin, "web_users")?;
        let tenant_id = admin.key.tenant_id;
        // Only a person the tenant holds, and never an admin: a name no
        // entry holds, or an admin's, journals nothing (ADR-0068 d8).
        let mut held = false;
        let mut after = None;
        loop {
            let page = self
                .journal
                .web_entries(tenant_id, after.as_ref(), 500)
                .await?;
            let Some(last) = page.last() else { break };
            after = Some(last.email.digest());
            for e in page.iter().filter(|e| &e.user == user) {
                if e.admin {
                    return Err(AppError::NotFound);
                }
                held = true;
            }
            if page.len() < 500 {
                break;
            }
        }
        if !held {
            return Err(AppError::NotFound);
        }
        // An admin key under the root is a corrupt row no denial can cut
        // off, so the disable refuses by name before its first commit and
        // journals nothing (ADR-0068 d8, d11; #277).
        let root = self.signup_root(tenant_id, user).await?;
        if let Some(root) = root
            && let Some(key) = self
                .admin_keys_under(tenant_id, root)
                .await?
                .into_iter()
                .next()
        {
            return Err(AppError::AdminKeyUnderUserRoot {
                user: user.clone(),
                key,
            });
        }
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::WEB_USER_DISABLED,
            &admin.key.principal,
            Some(admin.key.key_id),
            json!({ "user": user.as_str(), "by_key_id": admin.key.key_id.to_string() }),
        )?;
        let receipt = self
            .commit(
                event,
                MetadataFold::UserDisabled { user: user.clone() },
                None,
            )
            .await?;
        let mut cut = 0;
        let mut skipped = Vec::new();
        if let Some(root) = root {
            let universe_id = self.universe(tenant_id).await?;
            let universe = Reference::Name(Name::parse(UNIVERSE).expect("universe is a name"));
            for key in self.journal.web_user_keys(tenant_id, root).await? {
                if key.revoked_at.is_some() || key.credential == Credential::Session {
                    continue;
                }
                let denied = self
                    .journal
                    .key_access(tenant_id, key.key_id)
                    .await?
                    .is_some_and(|a| a.denials.iter().any(|d| d.denial.node == universe_id));
                if denied {
                    continue;
                }
                // After the first commit no refusal may end the act, so its
                // claim is never released (#266): a key gone since the
                // listing, or one a direct row edit made an admin key since
                // the check above, is skipped, left out of `keys_cut_off`
                // and kept in `keys_skipped`, which the HTTP answer does not
                // carry; `oml doctor` names the second (#277).
                match later_step(|| {
                    self.deny_key_by(&admin.key, Reference::Id(key.key_id), universe.clone())
                })
                .await
                {
                    Ok(_) => cut += 1,
                    Err(AppError::NotFound | AppError::DenialOnAdminKey) => {
                        skipped.push(key.key_id);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(UserDisabled {
            keys_cut_off: cut,
            keys_skipped: skipped,
            journal_seq: receipt.seq,
        })
    }

    /// `POST /v2/web/deletion-requests` (ADR-0070 d5 #7; ADR-0068 d6), as
    /// `<user>-web`: the pending request, journalled as
    /// `web.deletion_requested`. It ends nothing and changes no access.
    pub async fn request_web_deletion(&self, web: &Caller) -> Result<u64, AppError> {
        let user = web
            .key
            .session_user
            .clone()
            .ok_or(AppError::WebKeyMissing { capacity: "web" })?;
        let tenant_id = web.key.tenant_id;
        let prev = self.head_event(tenant_id).await?;
        let event = self.seal(
            prev.as_ref(),
            tenant_id,
            event_types::WEB_DELETION_REQUESTED,
            &web.key.principal,
            Some(web.key.key_id),
            json!({ "user": user.as_str() }),
        )?;
        Ok(self
            .commit(event, MetadataFold::DeletionRequested { user }, None)
            .await?
            .seq)
    }

    /// The tenant's pending deletion requests, oldest first (ADR-0068 d6).
    async fn pending_deletions(&self, admin: &Caller) -> Result<Vec<DeletionRequest>, AppError> {
        require_role(&admin.key, Role::Admin, "web_users")?;
        Ok(self
            .journal
            .web_deletion_requests(admin.key.tenant_id)
            .await?)
    }

    /// `POST /v2/web/users/{user}/deletion/close` (ADR-0070 d5 #10; ADR-0068
    /// d6): the person disabled (d8), then the request closed, journalled as
    /// `web.deletion_closed`. The bytes wait for erasure. The closing commit
    /// retries alone on a chain-head conflict, as the disable's denials do
    /// (#266).
    pub async fn close_web_deletion(&self, admin: &Caller, user: &Name) -> Result<u64, AppError> {
        require_role(&admin.key, Role::Admin, "web_users")?;
        let tenant_id = admin.key.tenant_id;
        if !self
            .journal
            .web_deletion_requests(tenant_id)
            .await?
            .iter()
            .any(|r| &r.user == user)
        {
            return Err(AppError::NotFound);
        }
        self.disable_web_user(admin, user).await?;
        later_step(|| async {
            let prev = self.head_event(tenant_id).await?;
            let event = self.seal(
                prev.as_ref(),
                tenant_id,
                event_types::WEB_DELETION_CLOSED,
                &admin.key.principal,
                Some(admin.key.key_id),
                json!({ "user": user.as_str(), "by_key_id": admin.key.key_id.to_string() }),
            )?;
            Ok(self
                .commit(
                    event,
                    MetadataFold::DeletionClosed { user: user.clone() },
                    None,
                )
                .await?
                .seq)
        })
        .await
    }

    /// `GET /v2/web/users`, admin by key role (ADR-0068 d5, d6, d11): the
    /// people of the tenant with their usage and limits, and the pending
    /// deletion requests, for the admin page's CLI twin and `oml doctor`.
    pub async fn signup_overview(
        &self,
        bearer: impl Into<crate::types::Bearer<'_>>,
    ) -> Result<(Vec<UserRow>, Vec<DeletionRequest>), AppError> {
        let caller = self.authenticate_bearer(bearer.into()).await?;
        Ok((
            self.list_web_users(&caller).await?,
            self.pending_deletions(&caller).await?,
        ))
    }

    // -- the onboarding page ---------------------------------------------------

    /// `GET /v2/whoami?start=true`'s `person.start` (ADR-0070 d5), the
    /// onboarding page's state for `<user>-web` (ADR-0068 d10): connected once
    /// any of the person's keys authenticated since the process started or a
    /// connector key has an open connection; the newest memory's state.
    async fn whoami_start(&self, key: &KeyRecord) -> Result<StartStatus, AppError> {
        let tenant_id = key.tenant_id;
        let Some(root) = key.user else {
            return Ok(StartStatus {
                connected: false,
                first_memory: None,
            });
        };
        let keys = self.journal.web_user_keys(tenant_id, root).await?;
        let agent_keys: Vec<&KeyRecord> = keys
            .iter()
            .filter(|k| k.credential != Credential::Session)
            .collect();
        let seen = {
            let windows = self.signup_windows();
            agent_keys.iter().any(|k| windows.contains(&k.key_id))
        };
        let connected = seen
            || self
                .journal
                .user_usage(tenant_id, root, &self.clock.now())
                .await?
                .connections
                > 0;
        let newest = self.root_memories(tenant_id, root, 1).await?;
        Ok(StartStatus {
            connected,
            first_memory: newest.first().map(|m| m.status),
        })
    }

    /// `GET /v2/whoami`'s `person` usage and limits (ADR-0070 d5); `None` where
    /// they are under no quota (ADR-0068 d5; ADR-0071 d5).
    async fn whoami_limits(
        &self,
        key: &KeyRecord,
    ) -> Result<Option<(UserUsage, Quotas)>, AppError> {
        let Some(root) = self.quota_root(key) else {
            return Ok(None);
        };
        let tenant_id = key.tenant_id;
        let Some(quotas) = self.account_limits(tenant_id, root).await?.quotas else {
            return Ok(None);
        };
        Ok(Some((
            self.journal
                .user_usage(tenant_id, root, &self.clock.now())
                .await?,
            quotas,
        )))
    }

    /// `GET /v2/whoami` for whichever credential presented the key: a key
    /// value, a connection's caller, or a session key's caller from a
    /// capacity credential (ADR-0070 d5). The credential is resolved once:
    /// a key value's lookup, then the key's access read once, which is the
    /// read every verb makes, and one request of the key's rate for a key
    /// value or a connection, none again for a session key the gate admitted
    /// (#253). The person block is read only as `person` asks (#286).
    pub async fn whoami_served(
        &self,
        bearer: Bearer<'_>,
        person: PersonAsk,
    ) -> Result<(Whoami, Option<Person>), AppError> {
        let (key, admit) = match bearer {
            Bearer::Key(value) => (self.key_of_value(value).await?, true),
            Bearer::Connection(caller) => (caller.key.clone(), true),
            Bearer::Session(caller) => (caller.key.clone(), false),
        };
        let access = self.access_of(&key).await?;
        if admit {
            // A person of the sign-up tenant's key under its request rate
            // (ADR-0068 d5), after the access read, as `authenticate` does.
            self.admit_request(&key).await?;
        }
        let whoami = self.whoami_of_key(key, access).await?;
        let person = match (person, &whoami.user) {
            (PersonAsk::Omit, _) | (_, None) => None,
            (ask, Some(user)) => Some(
                self.person_block(&whoami.key, user.name.clone(), ask == PersonAsk::WithStart)
                    .await?,
            ),
        };
        Ok((whoami, person))
    }

    /// The person a key names, for `GET /v2/whoami`'s `person` (ADR-0070
    /// d5; ADR-0068 d5, d6, d10): the user of its root, whether it is
    /// under the sign-up quotas, its usage and limits there, and when the
    /// person asked for deletion. `None` for a key that names no user.
    pub async fn whoami_person(
        &self,
        bearer: Bearer<'_>,
        start: bool,
    ) -> Result<Option<Person>, AppError> {
        // One authentication, so one request of the key's rate, however
        // much the answer names (ADR-0068 d5; ADR-0070 d5).
        let caller = self.authenticate_bearer(bearer).await?;
        let tenant_id = caller.key.tenant_id;
        let Some(root) = caller.key.user else {
            return Ok(None);
        };
        let Some(user) = self
            .journal
            .workarea(tenant_id, root)
            .await?
            .map(|w| w.name)
        else {
            return Ok(None);
        };
        self.person_block(&caller.key, user, start).await.map(Some)
    }

    /// The person block of `key`, whose root is named `user`: its limits,
    /// its pending deletion request and, where `start`, its onboarding
    /// status, each read once (ADR-0070 d5; #286).
    pub(crate) async fn person_block(
        &self,
        key: &KeyRecord,
        user: Name,
        start: bool,
    ) -> Result<Person, AppError> {
        let limits = self.whoami_limits(key).await?;
        let deletion_requested_at = self
            .journal
            .web_deletion_requests(key.tenant_id)
            .await?
            .into_iter()
            .find(|r| r.user == user)
            .map(|r| r.requested_at);
        let start = if start {
            Some(self.whoami_start(key).await?)
        } else {
            None
        };
        Ok(Person {
            user,
            limits,
            deletion_requested_at,
            start,
        })
    }

    /// `GET /v2/web/export` (ADR-0070 d5 #6; ADR-0068 d6), as `<user>-web`: the
    /// person's export, every memory in their root and below, every state,
    /// their keys and their entry's terms version. Nothing a second person
    /// wrote lies in the root.
    pub async fn export_web_person(&self, web: &Caller) -> Result<Value, AppError> {
        let tenant_id = web.key.tenant_id;
        let root = web
            .key
            .user
            .ok_or(AppError::WebKeyMissing { capacity: "web" })?;
        let nodes = self.journal.workarea_subtree(tenant_id, root).await?;
        let paths: BTreeMap<Uuid, String> = {
            let by_id: BTreeMap<Uuid, &crate::types::WorkareaRecord> =
                nodes.iter().map(|w| (w.workarea_id, w)).collect();
            nodes
                .iter()
                .map(|w| {
                    let mut parts = vec![w.name.as_str().to_owned()];
                    let mut at = w.parent_id;
                    while let Some(p) = at.and_then(|id| by_id.get(&id)) {
                        parts.push(p.name.as_str().to_owned());
                        at = p.parent_id;
                    }
                    parts.push(UNIVERSE.to_owned());
                    parts.reverse();
                    (w.workarea_id, parts.join("."))
                })
                .collect()
        };
        let memories = self.root_memories(tenant_id, root, EXPORT_MAX).await?;
        let keys = self.journal.web_user_keys(tenant_id, root).await?;
        Ok(json!({
            "format": "oml-export/personal-1",
            "user": web.key.session_user.as_ref().map(Name::as_str),
            "exported_at": self.clock.now().as_str(),
            "memories": memories.iter().map(|m| json!({
                "memory_id": m.memory_id.to_string(),
                "workarea": paths.get(&m.workarea_id),
                "claim_key": m.claim_key,
                "version": m.version,
                "status": m.status.as_str(),
                "text": m.body,
                "created_at": m.created_at.as_str(),
                "promoted_at": m.promoted_at.as_ref().map(oml_domain::EventTime::as_str),
            })).collect::<Vec<_>>(),
            "keys": keys.iter().map(|k| json!({
                "name": k.name.as_ref().map(Name::as_str),
                "role": k.role.as_str(),
                "credential": k.credential.as_str(),
                "revoked": k.revoked_at.is_some(),
            })).collect::<Vec<_>>(),
        }))
    }

    /// The root's memories, every state, newest first, at most `limit`.
    async fn root_memories(
        &self,
        tenant_id: Uuid,
        root: Uuid,
        limit: usize,
    ) -> Result<Vec<crate::types::MemoryRecord>, AppError> {
        let nodes: Vec<Uuid> = self
            .journal
            .workarea_subtree(tenant_id, root)
            .await?
            .into_iter()
            .map(|w| w.workarea_id)
            .collect();
        Ok(self
            .journal
            .web_admin_search(
                tenant_id,
                &AdminSearch {
                    query: None,
                    nodes: Some(nodes),
                    state: None,
                    origin_kinds: Vec::new(),
                    author: None,
                    after: None,
                    limit,
                },
            )
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
    /// The app with open sign-up configured (ADR-0068 d1); without it the
    /// deployment has no sign-up tenant and no quotas.
    #[must_use]
    pub fn with_signup(self, policy: SignupPolicy) -> Self {
        Self {
            signup: Some(std::sync::Arc::new(SignupState::new(policy))),
            ..self
        }
    }

    /// The configured sign-up, if any.
    #[must_use]
    pub fn signup_policy(&self) -> Option<&SignupPolicy> {
        self.signup.as_deref().map(|s| &s.policy)
    }

    /// The sign-up profile's limits, what a person with no root yet would
    /// start on (ADR-0071 d5).
    fn default_quotas(&self) -> Quotas {
        self.profiles
            .get(&self.profiles.signup_profile)
            .unwrap_or(Quotas::ZERO)
    }

    /// The account a key belongs to: a user key, or a person's session key
    /// (ADR-0071 d1). Whether it is under limits is its profile's.
    fn quota_root(&self, key: &KeyRecord) -> Option<Uuid> {
        key.user
    }

    /// A person's limits as the users listing serves them: their profile's
    /// with their overrides; for an account under no limits, the sign-up
    /// profile's with their overrides, which hold nothing until a profile
    /// does, so a quota an admin sets reads back in the row either way
    /// (ADR-0068 d5; ADR-0070 d5; ADR-0071 d5).
    pub(crate) async fn quotas_of(&self, tenant_id: Uuid, root: Uuid) -> Result<Quotas, AppError> {
        if let Some(quotas) = self.account_limits(tenant_id, root).await?.quotas {
            return Ok(quotas);
        }
        let overrides = self.journal.user_quota_overrides(tenant_id, root).await?;
        Ok(self.default_quotas().with_overrides(&overrides))
    }

    /// One more of `quota` for `key`'s person, or `limit.quota` naming
    /// what was hit (ADR-0068 d5); a key under no limit passes. The
    /// commit's ceiling is the check that holds (ADR-0071 d3).
    pub(crate) async fn require_quota(
        &self,
        key: &KeyRecord,
        quota: Quota,
    ) -> Result<(), AppError> {
        self.require_charge(key, &[(quota, 1)]).await
    }

    /// The key's request in its UTC minute (ADR-0068 d5): past its limit,
    /// `limit.rate` with the seconds left in the minute. The limit is read
    /// at each minute's first request, so an override holds from the next
    /// minute. Every key of the sign-up tenant is recorded, which is the
    /// onboarding page's "connected" (d10).
    pub(crate) async fn admit_request(&self, key: &KeyRecord) -> Result<(), AppError> {
        let Some(state) = self.signup.as_deref() else {
            return Ok(());
        };
        let Some(root) = self.quota_root(key) else {
            return Ok(());
        };
        let now = self.clock.now().unix_micros().div_euclid(1_000_000);
        let minute = now.div_euclid(60);
        let current = {
            let windows = state.windows.lock().expect("rate lock");
            windows.get(&key.key_id).copied()
        };
        let limit = match current {
            Some((m, _, limit)) if m == minute => limit,
            _ => match self.account_limits(key.tenant_id, root).await?.quotas {
                Some(quotas) => request_limit(key.credential, quotas.requests_per_minute),
                None => return Ok(()),
            },
        };
        let mut windows = state.windows.lock().expect("rate lock");
        let slot = windows.entry(key.key_id).or_insert((minute, 0, limit));
        if slot.0 != minute {
            *slot = (minute, 0, limit);
        }
        if slot.1 >= slot.2 {
            let retry = u64::try_from(60 - now.rem_euclid(60)).unwrap_or(60);
            return Err(AppError::KeyRateLimited {
                limit: slot.2,
                retry_after_secs: retry,
            });
        }
        slot.1 += 1;
        Ok(())
    }

    /// The keys whose requests this process counted: the "connected" set.
    fn signup_windows(&self) -> std::collections::BTreeSet<Uuid> {
        self.signup.as_deref().map_or_else(Default::default, |s| {
            s.windows
                .lock()
                .expect("rate lock")
                .keys()
                .copied()
                .collect()
        })
    }

    /// Every key's window restarts, so a changed rate holds at once; the
    /// keys stay recorded as seen.
    fn forget_windows(&self) {
        if let Some(state) = self.signup.as_deref() {
            for slot in state.windows.lock().expect("rate lock").values_mut() {
                slot.0 = i64::MIN;
            }
        }
    }

    /// The root a user's name names, if it is a user root.
    pub(super) async fn signup_root(
        &self,
        tenant_id: Uuid,
        user: &Name,
    ) -> Result<Option<Uuid>, AppError> {
        let Some(id) = self
            .journal
            .resolve_name(tenant_id, NamedKind::Workarea, user)
            .await?
        else {
            return Ok(None);
        };
        Ok(self
            .journal
            .workarea(tenant_id, id)
            .await?
            .filter(|w| w.user_root)
            .map(|w| w.workarea_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A person's session keys keep the floor whatever their quota; every
    /// other key takes the quota as set (ADR-0070, the seventh table of
    /// phase (b); #253).
    #[test]
    fn a_session_key_is_held_to_its_quota_or_the_floor_whichever_is_more() {
        let floor = SESSION_REQUESTS_PER_MINUTE_MIN;
        for (quota, session) in [(0, floor), (4, floor), (floor, floor), (600, 600)] {
            assert_eq!(
                request_limit(Credential::Session, quota),
                session,
                "{quota}"
            );
            assert_eq!(request_limit(Credential::Bearer, quota), quota, "{quota}");
            assert_eq!(
                request_limit(Credential::Connection, quota),
                quota,
                "{quota}"
            );
        }
    }
}
