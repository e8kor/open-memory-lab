//! `oml-pg` — the real journal-and-metadata store adapter over PostgreSQL
//! (workspace-plan.md M1 step 3), implementing `oml_app::ports::JournalStore`
//! over the committed migrations.
//!
//! The commit point is one transaction (ADR-0002 d1): the signed event, the
//! chain-head upsert, the metadata fold, the outbox row and the idempotency
//! record commit together or not at all. Chain assignment serializes on a
//! transaction-scoped advisory lock per tenant (ADR-0002 d3); the journal is
//! append-only by mechanism (ADR-0002 d4, enforced in `migrations/0001_init.sql`).
//! Every query is the sqlx runtime API — no compile-time checked macros, so
//! no `DATABASE_URL` at build (workspace-plan.md).

pub mod recall_meter;
pub mod telemetry;

use oml_app::domain::access::{Covers, Grant, KeyDenial};
use oml_app::domain::anchor::{Anchor, CheckResult, Origin};
use oml_app::domain::budget::hit_order;
use oml_app::domain::guardrail::{GuardrailScope, Severity};
use oml_app::domain::in_progress::KeyProfile;
use oml_app::domain::name::{Name, NamedKind, Naming, UNIVERSE};
use oml_app::domain::oauth::{
    CACHED_DOCUMENTS_MAX, ConnectionState, KeyState, RefreshDecision, RevokedCause, TokenKind,
    UNUSED_REGISTRATION_SECONDS, UNUSED_REGISTRATIONS_MAX, cache_lifetime,
};
use oml_app::domain::origin::OriginKind;
use oml_app::domain::signup::{Quota, QuotaReached};
use oml_app::domain::usage::{Meter, Month};
use oml_app::domain::valid_time::{ValidEnd, ValidTime};
use oml_app::domain::vocab_edges::EdgeState;
use oml_app::domain::web::{Email, Provider};
use oml_app::domain::{
    ChainHead, Envelope, EventTime, HeldBy, KeySet, MemoryKind, MemoryStatus, Role, Sha256Hash,
    SignedEnvelope, VerifyError, WorkareaStatus, ratifier_is_proposer, verify_chain,
};
use oml_app::ed25519_dalek::Signature;
use oml_app::ports::{
    Ceiling, CommitReceipt, CommitRequest, ConnectionStore, IdempotencyClaim, IdempotencyPut,
    IdempotencyRecord, IdempotencyScope, JournalStore, MetadataFold, Retriever, ScanBacklog,
    StoreError, SubjectRow, TailEvent, TailScan, WebStore, Written, claim_subject,
};
use oml_app::search::{
    Access, Candidate, CandidateScope, MatchMode, MatchedToken, OwnHeldScope, PostingsHealth,
    RetrievalQuery, Retrieved, RetrieverKind, SparsePending, SparsePosting, SparseTenant,
};
use oml_app::types::{
    AdminSearch, AdminState, Approval, AuthorizationRecord, BriefEdgeRecord, ClaimEdgePosition,
    ClaimEdgeRecord, ClaimEdgeSelection, ClaimKeyState, ClientDescription, ClientKind, ConnectCode,
    ConnectCodeRecord, ConnectGrant, ConnectOutcome, ConnectionRecord, Credential, CrewRecord,
    DenialRecord, DocumentCaching, FormatFields, GrantRecord, GuardrailFields, HIT_KINDS,
    HeldFilter, HeldRow, KeyAccess, KeyRecord, KeyReferences, ListingFilter, ListingPosition,
    ListingScope, MemoryRecord, NamedRef, OAuthClientRecord, PROVIDER_TENANT, PinOutcome,
    QueueDepth, RefreshFacts, RefreshOutcome, SourceCheck, SweepReport, TenantRow, TokenPair,
    TokenRecord, TransitionRecord, TransitionState, UnnamedRows, VersionFinding, WebEntry,
    WebSessionRecord, WorkareaRecord, WorkareaRef,
};
use serde_json::{Value, json};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions, PgRow};
use sqlx::{Connection, Postgres, QueryBuilder, Row, Transaction};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// The committed migrations, embedded at build time. The migrations are
/// normative in bytes (postgres-schema.md "Migrations"); the server/bootstrap
/// calls [`PgStore::migrate`] (or runs this migrator, as the owner with
/// `oml.serving_role` set) before serving.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../../migrations");

/// Stores each workarea's status as the one v0.5.1 and earlier served — the
/// universe root `committed`, every other node `development`, derived from
/// `parent_id` whatever the column held — on a database where migration 0017
/// is not yet applied, and returns the rows it changed. A v0.1.0 build stored
/// every node `development`, its root included, and later builds never read
/// the column back, so a deployment created then holds a root 0017's
/// `workareas_universe_committed` refuses (docs/operations/upgrades.md
/// "Upgrading from v0.5.1 or earlier"). 0017 keeps its released bytes, so a
/// database that applied it keeps its checksum; this runs before it instead,
/// in one transaction, under each tenant's row-level-security context as the
/// 0003 startup policy enumerates them. The served status does not change, so
/// nothing is journalled. A fresh database, or one past 0017, is left alone,
/// and a second run changes nothing.
pub async fn align_pre_0017_statuses(owner: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = owner.begin().await?;
    let pending: bool = sqlx::query_scalar(
        "SELECT to_regclass('_sqlx_migrations') IS NOT NULL \
            AND to_regclass('workareas') IS NOT NULL",
    )
    .fetch_one(&mut *tx)
    .await?;
    if !pending {
        return Ok(0);
    }
    let past_0017: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version >= 17)")
            .fetch_one(&mut *tx)
            .await?;
    if past_0017 {
        return Ok(0);
    }
    sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
        .execute(&mut *tx)
        .await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM tenants ORDER BY id")
        .fetch_all(&mut *tx)
        .await?;
    let mut aligned = 0;
    for tenant in tenants {
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        aligned += sqlx::query(
            "UPDATE workareas \
                SET status = CASE WHEN parent_id IS NULL THEN 'committed' ELSE 'development' END \
              WHERE tenant_id = $1 \
                AND status <> CASE WHEN parent_id IS NULL THEN 'committed' ELSE 'development' END",
        )
        .bind(tenant)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    }
    tx.commit().await?;
    Ok(aligned)
}

const FORK_CHECK_PAGE: usize = 1024;

/// A fork-check refusal (ADR-0002 d7): a verification failure here means the
/// stored head does not verify against the stored events, and the caller
/// must refuse to write onto the fork.
#[derive(Debug, thiserror::Error)]
pub enum ForkCheckError {
    #[error("no chain head is stored for this tenant")]
    EmptyChain,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Verify(#[from] VerifyError),
}

/// Why the lead refuses to serve over its two URLs (ADR-0034 d5): the
/// `details.reason` of its `postgres.role_split` exit, before any listener
/// binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleSplitReason {
    /// `postgres.owner_url` is unset; the lead reads its configuration and
    /// names this one itself.
    OwnerUrlMissing,
    /// Both URLs log in as one role, compared by login, not URL.
    SameRole,
    /// The two logins see different databases.
    DifferentDatabases,
    /// The serving role holds an attribute or membership d2 refuses, can
    /// create in the database or `public`, or, after migrating, holds
    /// `TRUNCATE`, `REFERENCES`, `TRIGGER` or `MAINTAIN` on a table.
    ServingRolePrivileged,
    /// The serving role owns the database or an object in it, as in every
    /// unsplit install.
    ServingRoleOwns,
    /// The serving role holds a privilege on `_sqlx_migrations` (d6).
    ServingRoleReadsMigrations,
    /// The serving role has no `SELECT` on `journal`: the migrations did
    /// not grant the role `postgres.url` names.
    ServingRoleUngranted,
    /// A table but `_sqlx_migrations`, a partition included, is not the
    /// owner's, lacks `ENABLE` or `FORCE ROW LEVEL SECURITY`, or has no
    /// policy.
    TableUnfenced,
    /// A function in `public` is `SECURITY DEFINER`, which runs a serving
    /// call as the owner.
    DefinerFunction,
}

impl RoleSplitReason {
    /// The reason as the lead's exit names it (ADR-0034 d5).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OwnerUrlMissing => "owner_url_missing",
            Self::SameRole => "same_role",
            Self::DifferentDatabases => "different_databases",
            Self::ServingRolePrivileged => "serving_role_privileged",
            Self::ServingRoleOwns => "serving_role_owns",
            Self::ServingRoleReadsMigrations => "serving_role_reads_migrations",
            Self::ServingRoleUngranted => "serving_role_ungranted",
            Self::TableUnfenced => "table_unfenced",
            Self::DefinerFunction => "definer_function",
        }
    }
}

impl std::fmt::Display for RoleSplitReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A refusal of the role split (ADR-0034 d5): `postgres.role_split` and its
/// reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("postgres.role_split: {reason}")]
pub struct RoleSplitError {
    pub reason: RoleSplitReason,
}

/// A boot check's failure: the split refused by name, or a catalog read
/// that did not complete, which proves nothing and refuses as well.
#[derive(Debug, thiserror::Error)]
pub enum RoleSplitCheckError {
    #[error(transparent)]
    Refused(#[from] RoleSplitError),
    #[error("postgres.role_split: the check could not read the catalog: {0}")]
    Backend(#[from] sqlx::Error),
}

impl RoleSplitCheckError {
    /// The refusal's reason, `None` for a catalog read that failed.
    #[must_use]
    pub fn reason(&self) -> Option<RoleSplitReason> {
        match self {
            Self::Refused(e) => Some(e.reason),
            Self::Backend(_) => None,
        }
    }
}

/// Who a pool logs in as and where: `session_user`, `current_user` and
/// `current_database()`.
async fn login_of(pool: &PgPool) -> Result<(String, String, String), sqlx::Error> {
    sqlx::query_as("SELECT session_user::text, current_user::text, current_database()::text")
        .fetch_one(pool)
        .await
}

/// Whether `sql` returns true, read on the owner pool with the logins in
/// scope as `split.serving` and `split.owner`.
async fn holds(
    owner: &PgPool,
    sql: &str,
    serving: &str,
    owner_role: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(&format!(
        "WITH split (serving, owner) AS (VALUES ($1::text, $2::text)) {sql}"
    ))
    .bind(serving)
    .bind(owner_role)
    .fetch_one(owner)
    .await
}

/// `pg_depend` marks `<alias>.oid`, of catalog `<catalog>`, as an
/// extension's member (`deptype` `'e'`), such as 0006's `ltree` objects,
/// which stay with the bootstrap superuser (ADR-0034 d2).
macro_rules! not_extension_member {
    ($catalog:literal, $alias:literal) => {
        concat!(
            "NOT EXISTS (SELECT 1 FROM pg_depend dep WHERE dep.classid = '",
            $catalog,
            "'::regclass AND dep.objid = ",
            $alias,
            ".oid AND dep.deptype = 'e')"
        )
    };
}

/// The serving role owns the database or a relation, function or type in
/// it, extension members skipped.
const SERVING_ROLE_OWNS: &str = concat!(
    "SELECT EXISTS (SELECT 1 FROM pg_database d JOIN pg_roles r ON r.oid = d.datdba \
                    WHERE d.datname = current_database() AND r.rolname = split.serving) \
         OR EXISTS (SELECT 1 FROM pg_class c JOIN pg_roles r ON r.oid = c.relowner \
                    WHERE r.rolname = split.serving AND ",
    not_extension_member!("pg_class", "c"),
    ") OR EXISTS (SELECT 1 FROM pg_proc p JOIN pg_roles r ON r.oid = p.proowner \
                    WHERE r.rolname = split.serving AND ",
    not_extension_member!("pg_proc", "p"),
    ") OR EXISTS (SELECT 1 FROM pg_type t JOIN pg_roles r ON r.oid = t.typowner \
                    WHERE r.rolname = split.serving AND ",
    not_extension_member!("pg_type", "t"),
    ") FROM split"
);

/// The serving role holds an attribute d2 refuses, is a member of any role,
/// or can create in the database or `public`.
const SERVING_ROLE_PRIVILEGED_BEFORE: &str = "\
    SELECT r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls \
        OR EXISTS (SELECT 1 FROM pg_auth_members m WHERE m.member = r.oid) \
        OR has_database_privilege(r.oid, current_database(), 'CREATE') \
        OR has_schema_privilege(r.oid, 'public', 'CREATE') \
    FROM split JOIN pg_roles r ON r.rolname = split.serving";

/// The tables of `public` the checks after migrating read: plain and
/// partitioned tables, partitions included, extension members skipped.
macro_rules! public_tables {
    () => {
        concat!(
            "FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND ",
            not_extension_member!("pg_class", "c")
        )
    };
}

/// The serving role holds a privilege row security does not bind on a
/// table but `_sqlx_migrations`, granted to it, a role it is in or
/// `PUBLIC`. `MAINTAIN` needs PostgreSQL 17, the pin.
const SERVING_ROLE_PRIVILEGED_AFTER: &str = concat!(
    "SELECT EXISTS (SELECT 1 ",
    public_tables!(),
    " AND c.relname <> '_sqlx_migrations' \
       AND has_table_privilege(split.serving, c.oid, 'TRUNCATE, REFERENCES, TRIGGER, MAINTAIN')) \
     FROM split"
);

/// The serving role holds any privilege on `_sqlx_migrations` (ADR-0034 d6).
const SERVING_ROLE_READS_MIGRATIONS: &str = "\
    SELECT has_table_privilege(split.serving, 'public._sqlx_migrations', \
        'SELECT, INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER, MAINTAIN') FROM split";

/// The serving role cannot read `journal`: the migrations did not grant it.
const SERVING_ROLE_UNGRANTED: &str =
    "SELECT NOT has_table_privilege(split.serving, 'public.journal', 'SELECT') FROM split";

/// A table but `_sqlx_migrations` is not the owner's, lacks `ENABLE` or
/// `FORCE ROW LEVEL SECURITY`, or has no policy.
const TABLE_UNFENCED: &str = concat!(
    "SELECT EXISTS (SELECT 1 ",
    public_tables!(),
    " AND c.relname <> '_sqlx_migrations' \
       AND (c.relowner <> (SELECT oid FROM pg_roles WHERE rolname = split.owner) \
            OR NOT c.relrowsecurity OR NOT c.relforcerowsecurity \
            OR NOT EXISTS (SELECT 1 FROM pg_policy pol WHERE pol.polrelid = c.oid))) \
     FROM split"
);

/// A function in `public` is `SECURITY DEFINER`.
const DEFINER_FUNCTION: &str = concat!(
    "SELECT EXISTS (SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                    WHERE n.nspname = 'public' AND p.prosecdef AND ",
    not_extension_member!("pg_proc", "p"),
    ") FROM split"
);

/// Runs `checks` in order on the owner pool and refuses with the first
/// that holds.
async fn refuse_first(
    owner: &PgPool,
    serving: &str,
    owner_role: &str,
    checks: &[(RoleSplitReason, &str)],
) -> Result<(), RoleSplitCheckError> {
    for (reason, sql) in checks {
        if holds(owner, sql, serving, owner_role).await? {
            return Err(RoleSplitError { reason: *reason }.into());
        }
    }
    Ok(())
}

/// The checks before migrating (ADR-0034 d5; postgres-schema.md
/// "Privileges"), over the owner pool and the serving pool, in this order:
/// both pools log in as one role, by `session_user` or `current_user`, never
/// by URL (`same_role`); they see two databases (`different_databases`);
/// the serving role owns the database or a relation, function or type in
/// it (`serving_role_owns`), checked before the next since a database's
/// owner can create in it; the serving role holds `SUPERUSER`, `CREATEDB`,
/// `CREATEROLE`, `REPLICATION` or `BYPASSRLS`, is a member of any role, or
/// holds `CREATE` on the database or `public` (`serving_role_privileged`).
/// Extension members, `ltree`'s owned by the bootstrap superuser, are
/// skipped (d2). A catalog read that fails refuses as well.
pub async fn role_split_preflight(
    owner: &PgPool,
    serving: &PgPool,
) -> Result<(), RoleSplitCheckError> {
    let (owner_session, owner_role, owner_db) = login_of(owner).await?;
    let (serving_session, serving_role, serving_db) = login_of(serving).await?;
    if owner_role == serving_role || owner_session == serving_session {
        return Err(RoleSplitError {
            reason: RoleSplitReason::SameRole,
        }
        .into());
    }
    if owner_db != serving_db {
        return Err(RoleSplitError {
            reason: RoleSplitReason::DifferentDatabases,
        }
        .into());
    }
    refuse_first(
        owner,
        &serving_role,
        &owner_role,
        &[
            (RoleSplitReason::ServingRoleOwns, SERVING_ROLE_OWNS),
            (
                RoleSplitReason::ServingRolePrivileged,
                SERVING_ROLE_PRIVILEGED_BEFORE,
            ),
        ],
    )
    .await
}

/// The checks after migrating (ADR-0034 d5; postgres-schema.md
/// "Privileges"), by `has_table_privilege`, so a grant through `PUBLIC` or
/// a role counts, in this order: the serving role holds `TRUNCATE`,
/// `REFERENCES`, `TRIGGER` or `MAINTAIN` on a table but `_sqlx_migrations`
/// (`serving_role_privileged`); any of those or `SELECT`, `INSERT`,
/// `UPDATE` or `DELETE` on `_sqlx_migrations` (`serving_role_reads_migrations`,
/// d6); no `SELECT` on `journal` (`serving_role_ungranted`); a table but
/// `_sqlx_migrations`, partitions included, is not the owner's, lacks
/// `ENABLE` or `FORCE ROW LEVEL SECURITY`, or has no policy
/// (`table_unfenced`); a function in `public` is `SECURITY DEFINER`
/// (`definer_function`). Extension members are skipped (d2). A catalog read
/// that fails refuses as well.
pub async fn role_split_postflight(
    owner: &PgPool,
    serving: &PgPool,
) -> Result<(), RoleSplitCheckError> {
    let (_, owner_role, _) = login_of(owner).await?;
    let (_, serving_role, _) = login_of(serving).await?;
    refuse_first(
        owner,
        &serving_role,
        &owner_role,
        &[
            (
                RoleSplitReason::ServingRolePrivileged,
                SERVING_ROLE_PRIVILEGED_AFTER,
            ),
            (
                RoleSplitReason::ServingRoleReadsMigrations,
                SERVING_ROLE_READS_MIGRATIONS,
            ),
            (
                RoleSplitReason::ServingRoleUngranted,
                SERVING_ROLE_UNGRANTED,
            ),
            (RoleSplitReason::TableUnfenced, TABLE_UNFENCED),
            (RoleSplitReason::DefinerFunction, DEFINER_FUNCTION),
        ],
    )
    .await
}

/// The owner's URL, held for d11's partition DDL alone; its `Debug` never
/// prints it, since it carries the owner's password (ADR-0034 d4).
#[derive(Clone)]
struct OwnerUrl(std::sync::Arc<str>);

impl std::fmt::Debug for OwnerUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OwnerUrl(<redacted>)")
    }
}

/// The lock timeout of a tenant's partition DDL: a `55P03` past it answers
/// `system.unavailable`, `partition_lock_timeout` (ADR-0034 d11).
const PARTITION_LOCK_TIMEOUT: &str = "5s";

/// The partition `memories` holds `tenant_id`'s rows in: the tenant's id in
/// 32 hex digits, never its name, which would put every tenant's name in
/// `pg_class` for every role (postgres-schema.md, the per-tenant
/// partitions; ADR-0034 d11). 0009 names each existing tenant's so.
fn memories_partition(tenant_id: Uuid) -> String {
    format!("memories_t_{}", tenant_id.simple())
}

/// The partition `search_postings` holds `tenant_id`'s postings in, named as
/// [`memories_partition`] names the tenant's memories (ADR-0052 d1).
fn postings_partition(tenant_id: Uuid) -> String {
    format!("search_postings_t_{}", tenant_id.simple())
}

/// A partition DDL failure as the port answers it: a lock contest the DDL
/// lost, a wait past the timeout (`55P03`) or a deadlock the detector broke
/// by aborting it (`40P01`), is [`StoreError::PartitionLockTimeout`],
/// retryable; anything else [`StoreError::Partition`] (ADR-0034 d11).
fn partition_error(e: &sqlx::Error) -> StoreError {
    match e.as_database_error().and_then(|db| db.code()) {
        Some(code) if code == "55P03" || code == "40P01" => StoreError::PartitionLockTimeout,
        _ => StoreError::Partition(e.to_string()),
    }
}

/// `tenant_id`'s partition DDL, in one owner transaction under `SET LOCAL
/// lock_timeout`: its `search_postings` partition, fenced alike and attached
/// first (ADR-0052 d1), then the table `LIKE memories INCLUDING ALL`, its `ENABLE` and
/// `FORCE ROW LEVEL SECURITY` and its parent's `tenant_isolation` policy,
/// then `ATTACH PARTITION`, which takes `ShareUpdateExclusiveLock` on the
/// parent where `CREATE TABLE … PARTITION OF` would take
/// `AccessExclusiveLock` and stop every tenant's reads and writes. The
/// attach clones `memories`' foreign keys, so it also takes
/// `ShareRowExclusiveLock` on `journal`, `tenants` and `review_queue`:
/// every tenant's commits, each of which inserts into `journal`, wait for
/// it, bounded by the timeout, and a commit that inserts into `tenants`
/// then `journal`, as a genesis does, can deadlock with it
/// ([`partition_error`]; postgres-schema.md, the per-tenant partitions;
/// ADR-0034 d11). The identifiers and the bound derive from the id alone,
/// which is why they are spelled into the DDL, which takes no parameters.
async fn create_partition_on(owner: &mut PgConnection, tenant_id: Uuid) -> Result<(), sqlx::Error> {
    let partition = memories_partition(tenant_id);
    let policy = "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) \
                  WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)";
    let mut tx = owner.begin().await?;
    sqlx::query(&format!(
        "SET LOCAL lock_timeout = '{PARTITION_LOCK_TIMEOUT}'"
    ))
    .persistent(false)
    .execute(&mut *tx)
    .await?;
    // The postings partition first, so the tenant's first write finds it
    // when its trigger writes postings (ADR-0052 d1). A schema before 0021,
    // as an upgrade test builds, has no postings table; 0021 then creates
    // the partition of every tenant it finds.
    let postings_table: bool =
        sqlx::query_scalar("SELECT to_regclass('search_postings') IS NOT NULL")
            .persistent(false)
            .fetch_one(&mut *tx)
            .await?;
    let mut statements = Vec::new();
    if postings_table {
        statements.extend(postings_partition_ddl(tenant_id));
    }
    // The similar keys partition likewise, so the tenant's first write
    // finds it when its trigger writes keys (ADR-0050 d17).
    let similar_table: bool = sqlx::query_scalar("SELECT to_regclass('similar_keys') IS NOT NULL")
        .persistent(false)
        .fetch_one(&mut *tx)
        .await?;
    if similar_table {
        statements.extend(similar_partition_ddl(tenant_id));
    }
    statements.extend([
        format!("CREATE TABLE {partition} (LIKE memories INCLUDING ALL)"),
        format!("ALTER TABLE {partition} ENABLE ROW LEVEL SECURITY"),
        format!("ALTER TABLE {partition} FORCE ROW LEVEL SECURITY"),
        format!("CREATE POLICY tenant_isolation ON {partition} {policy}"),
        format!("ALTER TABLE memories ATTACH PARTITION {partition} FOR VALUES IN ('{tenant_id}')"),
    ]);
    for statement in statements {
        sqlx::query(&statement)
            .persistent(false)
            .execute(&mut *tx)
            .await?;
    }
    // The tenant's recall telemetry partitions, so its first recall is
    // recorded (ADR-0054 d2). A schema before 0023 has none to create.
    let telemetry_tables: bool =
        sqlx::query_scalar("SELECT to_regclass('recall_events') IS NOT NULL")
            .persistent(false)
            .fetch_one(&mut *tx)
            .await?;
    if telemetry_tables {
        telemetry::ensure_tenant(&mut tx, tenant_id).await?;
    }
    tx.commit().await
}

/// The DDL of `tenant_id`'s `search_postings` partition, fenced before it is
/// attached, as 0021 builds every existing tenant's (ADR-0052 d1).
fn postings_partition_ddl(tenant_id: Uuid) -> Vec<String> {
    let postings = postings_partition(tenant_id);
    let policy = "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) \
                  WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)";
    vec![
        format!("CREATE TABLE {postings} (LIKE search_postings INCLUDING ALL)"),
        format!("ALTER TABLE {postings} ENABLE ROW LEVEL SECURITY"),
        format!("ALTER TABLE {postings} FORCE ROW LEVEL SECURITY"),
        format!("CREATE POLICY tenant_isolation ON {postings} {policy}"),
        format!(
            "ALTER TABLE search_postings ATTACH PARTITION {postings} FOR VALUES IN ('{tenant_id}')"
        ),
    ]
}

/// `tenant_id`'s `similar_keys` partition (ADR-0050 d17).
fn similar_partition(tenant_id: Uuid) -> String {
    format!("similar_keys_t_{}", tenant_id.simple())
}

/// The DDL of `tenant_id`'s `similar_keys` partition, fenced before it is
/// attached, as 0025 builds every existing tenant's (ADR-0050 d17).
fn similar_partition_ddl(tenant_id: Uuid) -> Vec<String> {
    let similar = similar_partition(tenant_id);
    let policy = "USING (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid) \
                  WITH CHECK (tenant_id = NULLIF(current_setting('oml.tenant_id', true), '')::uuid)";
    vec![
        format!("CREATE TABLE {similar} (LIKE similar_keys INCLUDING ALL)"),
        format!("ALTER TABLE {similar} ENABLE ROW LEVEL SECURITY"),
        format!("ALTER TABLE {similar} FORCE ROW LEVEL SECURITY"),
        format!("CREATE POLICY tenant_isolation ON {similar} {policy}"),
        format!(
            "ALTER TABLE similar_keys ATTACH PARTITION {similar} FOR VALUES IN ('{tenant_id}')"
        ),
    ]
}

/// Creates and fills the `similar_keys` partition of every tenant whose
/// memories partition exists and whose similar keys partition does not, as
/// [`ensure_postings_partitions`] does for postings: a lead built before
/// 0027 creates a tenant without it, and that tenant's writes fail until it
/// exists (ADR-0050 d17). Returns the tenants repaired.
async fn ensure_similar_partitions(owner: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = owner.begin().await?;
    sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
        .execute(&mut *tx)
        .await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM tenants ORDER BY id")
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut repaired = 0;
    for tenant_id in tenants {
        let mut tx = owner.begin().await?;
        let missing: bool = sqlx::query_scalar(
            "SELECT to_regclass('similar_keys') IS NOT NULL \
                AND to_regclass($1) IS NOT NULL AND to_regclass($2) IS NULL",
        )
        .bind(memories_partition(tenant_id))
        .bind(similar_partition(tenant_id))
        .fetch_one(&mut *tx)
        .await?;
        if !missing {
            tx.rollback().await?;
            continue;
        }
        sqlx::query(&format!(
            "SET LOCAL lock_timeout = '{PARTITION_LOCK_TIMEOUT}'"
        ))
        .persistent(false)
        .execute(&mut *tx)
        .await?;
        for statement in similar_partition_ddl(tenant_id) {
            sqlx::query(&statement)
                .persistent(false)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT oml_similar_keys_refill($1)")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        repaired += 1;
    }
    Ok(repaired)
}

/// Creates and fills the `search_postings` partition of every tenant whose
/// memories partition exists and whose postings partition does not, each
/// in an owner transaction of its own: a lead built before 0021 creates a
/// tenant's memories partition alone, and that tenant's memory writes fail
/// until the postings partition exists (ADR-0052 d1, d12; review F4). The
/// lead runs it at every start, after migrating; it creates nothing where
/// nothing is missing. Returns the tenants repaired.
async fn ensure_postings_partitions(owner: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = owner.begin().await?;
    sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
        .execute(&mut *tx)
        .await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM tenants ORDER BY id")
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let mut repaired = 0;
    for tenant_id in tenants {
        let mut tx = owner.begin().await?;
        let missing: bool =
            sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL AND to_regclass($2) IS NULL")
                .bind(memories_partition(tenant_id))
                .bind(postings_partition(tenant_id))
                .fetch_one(&mut *tx)
                .await?;
        if !missing {
            tx.rollback().await?;
            continue;
        }
        sqlx::query(&format!(
            "SET LOCAL lock_timeout = '{PARTITION_LOCK_TIMEOUT}'"
        ))
        .persistent(false)
        .execute(&mut *tx)
        .await?;
        for statement in postings_partition_ddl(tenant_id) {
            sqlx::query(&statement)
                .persistent(false)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT oml_search_postings_refill($1)")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        repaired += 1;
    }
    Ok(repaired)
}

/// The full-text recall statement `recall_fts` runs, binding `$1` the
/// tenant, `$2` the readable nodes, `$3` the query, `$4` the kinds and `$5`
/// `k`. Its `m.tenant_id = $1` is the tenant `oml.tenant_id` names, so the
/// plan keeps that tenant's partition alone, or drops the rest as it
/// starts; row security stays the fence (ADR-0034 d11). Exported so the
/// suite plans the statement the adapter runs.
pub const RECALL_FTS: &str = "SELECT m.id, m.body_bytes, \
            ts_rank(to_tsvector('simple', COALESCE(m.search_text, m.claim)), \
                    plainto_tsquery('simple', $3), 0) AS score \
     FROM memories m \
     WHERE m.tenant_id = $1 AND m.status = 'promoted' AND m.is_tip \
       AND NOT m.retirement AND m.kind = ANY($4::text[]) \
       AND m.workarea_id = ANY($2::uuid[]) /*anchor*/ \
       AND to_tsvector('simple', COALESCE(m.search_text, m.claim)) @@ plainto_tsquery('simple', $3) \
     ORDER BY score DESC, m.id ASC \
     LIMIT $5";

/// The own held recall statement `recall_own_held` runs, binding `$1` the
/// tenant, `$2` the author keys (the own key set, and inside an in-progress
/// scope the mark of the server's rule set), `$3` the readable nodes, `$4`
/// the query, `$5` the kinds and `$6` `k`; pruned to the tenant's partition
/// as [`RECALL_FTS`] is (ADR-0034 d11).
pub const RECALL_OWN_HELD: &str = "SELECT m.id, m.body_bytes, \
            ts_rank(to_tsvector('simple', COALESCE(m.search_text, m.claim)), \
                    plainto_tsquery('simple', $4), 0) AS score \
     FROM memories m \
     WHERE m.tenant_id = $1 AND m.status = 'held' \
       AND (m.author_key_id = ANY($2::uuid[]) \
         OR (m.scan_finding IS NULL AND encode(substring(sha256(convert_to(COALESCE(m.scanned_under, ''), 'UTF8')) FROM 1 FOR 16), 'hex')::uuid = ANY($2::uuid[]))) \
       AND NOT m.retirement AND m.kind = ANY($5::text[]) \
       AND m.workarea_id = ANY($3::uuid[]) /*anchor*/ \
       AND to_tsvector('simple', COALESCE(m.search_text, m.claim)) @@ plainto_tsquery('simple', $4) \
     ORDER BY score DESC, m.id ASC \
     LIMIT $6";

/// The lineage widening a `claim_edge.proposed` fold runs after its
/// INSERT, binding `$1` the tenant, `$2` the edge's source and `$3` its
/// target (the reading [`MetadataFold::ClaimEdgeProposed`] names; ADR-0001
/// d2). `component` is every version connected to either endpoint through
/// the tenant's claim edges, followed from either end, the new edge
/// included; `reached` is `component` and every version that reaches it
/// through `derives_from`, child to parent, and claim edges; each version
/// in `reached` gains the union of `component`'s flattened closures,
/// sorted and deduplicated: principals in byte order, as the application
/// sorts them, and key ids in uuid order. Every scan of `memories` names
/// `m.tenant_id = $1`, so the plan keeps the tenant's partition alone
/// (ADR-0034 d11, d13); row security stays the fence. The fold runs it
/// under a custom plan, which prunes the UPDATE's targets too. Exported so
/// the suite plans the statement the adapter runs.
pub const WIDEN_LINEAGE: &str = "WITH RECURSIVE \
     edge_adjacent(a, b) AS ( \
         SELECT e.src_artifact_id, e.dst_artifact_id FROM claim_edges e \
          WHERE e.tenant_id = $1 \
         UNION ALL \
         SELECT e.dst_artifact_id, e.src_artifact_id FROM claim_edges e \
          WHERE e.tenant_id = $1), \
     component(id) AS ( \
         SELECT unnest(ARRAY[$2::uuid, $3::uuid]) \
         UNION \
         SELECT j.b FROM component c JOIN edge_adjacent j ON j.a = c.id), \
     joined AS ( \
         SELECT \
           ARRAY(SELECT p FROM memories m, unnest(m.lineage_principals) p \
                  WHERE m.tenant_id = $1 AND m.id IN (SELECT id FROM component)) \
             AS principals, \
           ARRAY(SELECT k FROM memories m, unnest(m.lineage_key_ids) k \
                  WHERE m.tenant_id = $1 AND m.id IN (SELECT id FROM component)) \
             AS key_ids), \
     reach_adjacent(a, b) AS ( \
         SELECT parent, m.id FROM memories m, unnest(m.derives_from) parent \
          WHERE m.tenant_id = $1 \
         UNION ALL \
         SELECT a, b FROM edge_adjacent), \
     reached(id) AS ( \
         SELECT id FROM component \
         UNION \
         SELECT j.b FROM reached r JOIN reach_adjacent j ON j.a = r.id) \
     UPDATE memories m SET \
       lineage_principals = ARRAY( \
           SELECT p FROM unnest(m.lineage_principals || joined.principals) p \
            GROUP BY p ORDER BY p COLLATE \"C\"), \
       lineage_key_ids = ARRAY( \
           SELECT k FROM unnest(m.lineage_key_ids || joined.key_ids) k \
            GROUP BY k ORDER BY k) \
     FROM joined \
     WHERE m.tenant_id = $1 AND m.id IN (SELECT id FROM reached)";

/// The candidate scope every search statement but `keyword-all`'s reads,
/// binding `$1` the tenant, `$2` the promoted nodes, `$3` the kinds, `$4`
/// the own held author keys ([`held_authors`]) and `$5` their nodes:
/// promoted served tips on
/// the readable nodes, and the own held rows of the keys on theirs, never a
/// retirement version (ADR-0025 d4, d8; ADR-0049 d2). Row security stays the
/// fence beneath it (ADR-0034 d11). A version whose valid time ended at or
/// before the request's now, bound as `$NOW` (an `int8`; the least `int8`
/// under `include_expired`), is outside it: a leakproof comparison of a
/// column with a parameter (proposed ADR-0059 d3). Each statement names its
/// own number for `$NOW` with [`with_now`].
const SEARCH_SCOPE: &str = "m.tenant_id = $1 AND NOT m.retirement \
       AND (m.valid_until_us IS NULL OR m.valid_until_us > $NOW::int8) \
       AND m.kind = ANY($3::text[]) \
       AND ((m.status = 'promoted' AND m.is_tip AND m.workarea_id = ANY($2::uuid[])) \
         OR (m.status = 'held' AND m.workarea_id = ANY($5::uuid[]) \
             AND (m.author_key_id = ANY($4::uuid[]) \
               OR (m.scan_finding IS NULL \
                   AND encode(substring(sha256(convert_to(COALESCE(m.scanned_under, ''), 'UTF8')) FROM 1 FOR 16), 'hex')::uuid = ANY($4::uuid[])))))";

/// The author keys a held search statement binds: the own key set, and,
/// inside an in-progress scope, the mark of the server's rule set too
/// ([`scanner_mark`]), which admits every author's held row the scan did
/// not hold and last read under that rule set (ADR-0030 d3; ADR-0065 d15).
/// The statements compare a row's own mark, computed from its
/// `scanned_under`, only beside `m.scan_finding IS NULL`, so a row the
/// scan held, or one an older rule set passed, stays its author's.
fn held_authors(own: &OwnHeldScope) -> Vec<Uuid> {
    let mut keys = own.author_key_ids.clone();
    if own.any_author {
        keys.push(scanner_mark(own.current_scanner.as_deref()));
    }
    keys
}

/// A rule-set version as an id the held statements' author array can carry:
/// the first 16 bytes of SHA-256 over the version, the empty string for
/// none, as the statements compute it from `m.scanned_under`. A key id
/// equal to one is a 2^-122 chance at most (ids are minted v4 or v7).
fn scanner_mark(version: Option<&str>) -> Uuid {
    let digest = Sha256Hash::digest(version.unwrap_or("").as_bytes()).0;
    let mut first = [0_u8; 16];
    first.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(first)
}

/// `sql` with its `$NOW` placeholder numbered `$n`: the request's now as
/// a bound `int8`, so the expiry cut compares a column with a parameter,
/// leakproof, and an index serves it under row security (proposed ADR-0059
/// d3; review M3).
fn with_now(sql: &str, n: usize) -> String {
    sql.replace("$NOW", &format!("${n}"))
}

/// The promoted part of [`SEARCH_SCOPE`] today: the served tips on the
/// readable nodes.
const TIPS_TODAY: &str = "(m.status = 'promoted' AND m.is_tip AND m.workarea_id = ANY($2::uuid[]))";

/// The held part of [`SEARCH_SCOPE`], up to its author predicate.
const HELD_TODAY: &str = "OR (m.status = 'held' AND m.workarea_id = ANY($5::uuid[])";

/// Whether `m` was its claim's tip at the instant `at`, an SQL
/// `timestamptz` expression (ADR-0059 d15, d16, d20): promoted at or before
/// it, and the greatest version of its `workarea_id` and `claim_key`, in any
/// state today, promoted at or before it. Today's tip promoted by then was
/// the tip then, since a tip moves only forward (ADR-0059 d16), so the
/// probe runs only for a replaced version: correlated, once per such row,
/// it reads `memories_tip_moves` by an index condition on the claim, never
/// a join the planner could unparameterize. Exported so the suite plans
/// the probe the adapter runs.
#[must_use]
pub fn tip_at_sql(at: &str) -> String {
    // `m.is_tip` short-cuts the probe for today's tip: sound only while a
    // tip moves forward alone. An act that makes a replaced version the tip
    // again (a demotion returning it) must add ADR-0059 d16's interval fold
    // and drop this short cut in the same change; the in-memory store's
    // `tip_at` makes the same reading.
    format!(
        "m.status = 'promoted' AND m.promoted_at <= {at} \
         AND (m.is_tip OR m.version = (SELECT max(s.version) FROM memories s \
           WHERE s.tenant_id = m.tenant_id AND s.workarea_id = m.workarea_id \
             AND s.claim_key = m.claim_key AND s.promoted_at <= {at}))"
    )
}

/// An instant as an SQL literal. Safe because an [`EventTime`] exists only
/// through `EventTime::parse`, which admits the fixed 27-byte form of
/// digits, `-`, `:`, `T`, `.` and `Z` alone, so no quote can reach the
/// statement; the assertion restates it. It is inlined where binding it
/// would renumber every search statement's parameters.
fn instant_literal(at: &EventTime) -> String {
    let t = at.as_str();
    assert!(
        t.bytes()
            .all(|b| b.is_ascii_digit() || b"-:T.Z".contains(&b)),
        "an EventTime is fixed-width digits and separators"
    );
    format!("'{t}'::timestamptz")
}

/// `sql`, a statement that reads [`SEARCH_SCOPE`], read at the instant `at`
/// (ADR-0059 d15, d18): its promoted part is the tips at `at` on the
/// readable nodes and its held part the rows recorded by `at`. `None` keeps
/// today's statement.
fn scope_at(sql: &str, at: Option<&EventTime>) -> String {
    let Some(at) = at else {
        return sql.to_owned();
    };
    // Both parts are rewritten or the statement is refused: a held part
    // left as today's would count held rows recorded after the instant in
    // bm25's statistics.
    assert!(
        sql.contains(TIPS_TODAY) && sql.contains(HELD_TODAY),
        "the statement reads the search scope's promoted and held parts"
    );
    sql.replace(
        TIPS_TODAY,
        &format!(
            "({} AND m.workarea_id = ANY($2::uuid[]))",
            tip_at_sql(&instant_literal(at))
        ),
    )
    .replace(
        HELD_TODAY,
        &format!(
            "OR (m.status = 'held' AND m.created_at <= {} AND m.workarea_id = ANY($5::uuid[])",
            instant_literal(at)
        ),
    )
}

/// The versions of the tenant `$1` whose valid time ended at or before the
/// request's now, `$NOW`, read by the `memories_valid_until` index's
/// condition; the postings statements leave them out (proposed ADR-0059 d3).
const EXPIRED_IDS: &str = "expired AS (SELECT m.id FROM memories m \
     WHERE m.tenant_id = $1 AND m.valid_until_us <= $NOW::int8)";

/// The `time` statement (proposed ADR-0059 d7): of the rows the text list
/// found, `$10` by id, those in the scope whose declared valid time
/// overlaps the window [`$6`, `$7`), either side open when null; a row with
/// no `valid_from` never matches. A row that starts inside the window
/// scores 1, one that started before it `1 / (1 + days from its start to
/// the window's)`; ties by id; at most `$8`. `$9` the request's now. The
/// read is a primary-key probe bounded by the text list, and every
/// predicate on the bounds is a leakproof `int8` comparison. Exported so
/// the suite plans the statement the adapter runs.
#[must_use]
pub fn time_sql() -> String {
    with_now(
        &format!(
            "SELECT m.id, m.body_bytes, \
                    (1.0 / (1.0 + GREATEST(COALESCE($6::int8, m.valid_from_us) - m.valid_from_us, 0)::float8 \
                                    / 86400000000.0))::float4 AS score, \
                    '{{}}'::text[] AS matched \
             FROM memories m \
             WHERE {SEARCH_SCOPE} AND m.id = ANY($10::uuid[]) AND m.valid_from_us IS NOT NULL \
               AND ($7::int8 IS NULL OR m.valid_from_us < $7::int8) \
               AND ($6::int8 IS NULL OR m.valid_until_us IS NULL OR m.valid_until_us > $6::int8) \
             ORDER BY score DESC, m.id ASC \
             LIMIT $8"
        ),
        9,
    )
}

/// The `keyword-any` statement (ADR-0048 S1): the query's `simple` lexemes
/// joined by OR on the existing GIN index, `ts_rank` normalization 0, ties
/// by id; `$6` the query, `$7` `k`, `$8` whether every term must match,
/// `$9` the request's now, the anchor's binds from `$10`.
fn keyword_any_sql(anchor: &str) -> String {
    with_now(
        &format!(
            "WITH q AS (SELECT NULLIF(replace(plainto_tsquery('simple', $6)::text, ' & ', ' | '), '')::tsquery AS tq, \
                    tsvector_to_array(to_tsvector('simple', $6)) AS terms) \
         SELECT m.id, m.body_bytes, ts_rank(to_tsvector('simple', COALESCE(m.search_text, m.claim)), q.tq, 0) AS score, \
                ARRAY(SELECT t FROM unnest(q.terms) t \
                      WHERE t = ANY(tsvector_to_array(to_tsvector('simple', COALESCE(m.search_text, m.claim)))) ORDER BY t) AS matched \
         FROM memories m CROSS JOIN q \
         WHERE {SEARCH_SCOPE}{anchor} \
           AND to_tsvector('simple', COALESCE(m.search_text, m.claim)) @@ q.tq \
           AND (NOT $8 OR to_tsvector('simple', COALESCE(m.search_text, m.claim)) @@ plainto_tsquery('simple', $6)) \
         ORDER BY score DESC, m.id ASC \
         LIMIT $7"
        ),
        9,
    )
}

/// The query words the `bm25` and `trigram` retrievers read, `$1` the
/// query: the `simple` words of the normalised query (identifier parts
/// beside the whole), less its English stop words when it has any other
/// word, in byte order (ADR-0049 d12). A stop word scores near nothing
/// under BM25 and nearly every row of English text holds one, so reading
/// it would make nearly every row a candidate.
const SEARCH_WORDS: &str = "words AS (SELECT DISTINCT w, ts_lexize('english_stem', w) = '{}'::text[] AS stop \
                   FROM unnest(tsvector_to_array(to_tsvector('simple', oml_search_text($1)))) w), \
     kept AS (SELECT w FROM words WHERE NOT stop OR NOT EXISTS (SELECT 1 FROM words WHERE NOT stop))";

/// The statement answering a `bm25` or `trigram` list's `terms`: the
/// [`SEARCH_WORDS`] of `$1`.
fn search_terms_sql() -> String {
    format!(
        "WITH {SEARCH_WORDS} \
         SELECT COALESCE(array_agg(w ORDER BY w COLLATE \"C\"), '{{}}'::text[]) FROM kept"
    )
}

/// The `bm25` statement (ADR-0048 S2; ADR-0049 d12): each kept query word
/// (`SEARCH_WORDS`) in its exact form and its English stem, scored with
/// BM25, k1 1.2 and b 0.75, against each row's stored `search_vector`,
/// whose length is its stored `search_len`; a word scores its best form,
/// so a row holding both a word and its stem counts it once. The row count,
/// the average length and each form's document frequency are computed in
/// this statement over the candidate scope alone, the frequencies from the
/// rows that hold a form, which are every scope row holding it, so a row
/// the key cannot read never moves another row's rank (ADR-0048 d3). No
/// row's text is read and no vector is built from it. `$6` the query, `$7`
/// `k`, `$8` whether every kept word must match. Exported so the suite
/// plans the statement the adapter runs.
#[must_use]
pub fn bm25_sql() -> String {
    bm25_sql_within("")
}

/// [`bm25_sql`] with `anchor`, a predicate on `m` from `$10` on (`$9` the
/// request's now), narrowing
/// the scored rows and never the statistics or the document frequencies,
/// so a filtered row scores as it does unfiltered (ADR-0057 d4).
fn bm25_sql_within(anchor: &str) -> String {
    // The filter narrows the scored rows, never the term frequencies the
    // document frequencies count.
    let within = if anchor.is_empty() {
        String::new()
    } else {
        format!(
            "WHERE EXISTS (SELECT 1 FROM memories m WHERE m.tenant_id = $1 AND m.id = tf.id{anchor})"
        )
    };
    let words = SEARCH_WORDS.replace("$1", "$6");
    with_now(
        &format!(
            "WITH {words}, \
         forms AS (SELECT DISTINCT kept.w, f FROM kept \
                   CROSS JOIN LATERAL unnest(array_append(COALESCE(ts_lexize('english_stem', kept.w), '{{}}'::text[]), kept.w)) f), \
         q AS (SELECT string_agg(DISTINCT '''' || replace(replace(f, '\\', '\\\\'), '''', '''''') || '''', ' | ')::tsquery AS tq, \
                      array_agg(DISTINCT f) AS fa FROM forms), \
         stats AS (SELECT count(*)::float8 AS n, GREATEST(COALESCE(avg(m.search_len), 1), 1)::float8 AS avgdl \
                   FROM memories m WHERE {SEARCH_SCOPE}), \
         tf AS MATERIALIZED (SELECT m.id, m.body_bytes, m.search_len::float8 AS dl, u.lexeme, \
                                    GREATEST(cardinality(u.positions), 1)::float8 AS tf \
                             FROM memories m CROSS JOIN q \
                             CROSS JOIN LATERAL unnest(ts_filter(setweight(m.search_vector, 'A', q.fa), '{{a}}')) u \
                             WHERE {SEARCH_SCOPE} AND m.search_vector @@ q.tq), \
         df AS (SELECT lexeme, count(*)::float8 AS df FROM tf GROUP BY lexeme), \
         per AS (SELECT tf.id, tf.body_bytes, forms.w, \
                        max(ln(1 + (s.n - df.df + 0.5) / (df.df + 0.5)) \
                            * tf.tf * 2.2 / (tf.tf + 1.2 * (0.25 + 0.75 * tf.dl / s.avgdl))) AS c \
                 FROM tf JOIN df USING (lexeme) JOIN forms ON forms.f = tf.lexeme CROSS JOIN stats s \
                 {within} \
                 GROUP BY tf.id, tf.body_bytes, forms.w), \
         scored AS (SELECT id, body_bytes, sum(c) AS score, array_agg(w ORDER BY w COLLATE \"C\") AS words \
                    FROM per GROUP BY id, body_bytes) \
         SELECT id, body_bytes, score::float4 AS score, words AS matched FROM scored \
         WHERE (NOT $8 OR cardinality(words) = (SELECT count(*) FROM kept)) \
         ORDER BY score DESC, id ASC \
         LIMIT $7"
        ),
        9,
    )
}

/// The served versions on the reference set `$10` of a federated recall
/// (ADR-0072 d4.2, d7): promoted tips of the kinds `$3`, unexpired at
/// `$NOW`, never a held row. Every BM25 statistic is taken over these rows,
/// never over the searched ones.
const REFERENCE_SCOPE: &str = "m.tenant_id = $1 AND NOT m.retirement \
       AND (m.valid_until_us IS NULL OR m.valid_until_us > $NOW::int8) \
       AND m.kind = ANY($3::text[]) AND m.status = 'promoted' AND m.is_tip \
       AND m.workarea_id = ANY($10::uuid[])";

/// `bm25_sql_within` under a reference set (ADR-0072 d7): the row count,
/// the mean length and each form's document frequency are taken over
/// `REFERENCE_SCOPE`, `$10` the reference nodes, and the anchor's binds
/// start at `$11`; the scored rows are the candidate scope's, so a row's
/// score is a function of the row, the query and the reference set alone.
/// A form no reference row holds scores nothing, as `bm25_postings` leaves
/// it out, so an own held row ranks alike on both. Exported so the suite
/// plans the statement the adapter runs.
#[must_use]
pub fn bm25_reference_sql_within(anchor: &str) -> String {
    // The filter narrows the scored rows, never the term frequencies the
    // document frequencies count.
    let within = if anchor.is_empty() {
        String::new()
    } else {
        format!(
            "WHERE EXISTS (SELECT 1 FROM memories m WHERE m.tenant_id = $1 AND m.id = tf.id{anchor})"
        )
    };
    let words = SEARCH_WORDS.replace("$1", "$6");
    with_now(
        &format!(
            "WITH {words}, \
         forms AS (SELECT DISTINCT kept.w, f FROM kept \
                   CROSS JOIN LATERAL unnest(array_append(COALESCE(ts_lexize('english_stem', kept.w), '{{}}'::text[]), kept.w)) f), \
         q AS (SELECT string_agg(DISTINCT '''' || replace(replace(f, '\\', '\\\\'), '''', '''''') || '''', ' | ')::tsquery AS tq, \
                      array_agg(DISTINCT f) AS fa FROM forms), \
         stats AS (SELECT count(*)::float8 AS n, GREATEST(COALESCE(avg(m.search_len), 1), 1)::float8 AS avgdl \
                   FROM memories m WHERE {REFERENCE_SCOPE}), \
         df AS (SELECT u.lexeme, count(*)::float8 AS df \
                FROM memories m CROSS JOIN q \
                CROSS JOIN LATERAL unnest(ts_filter(setweight(m.search_vector, 'A', q.fa), '{{a}}')) u \
                WHERE {REFERENCE_SCOPE} AND m.search_vector @@ q.tq GROUP BY u.lexeme), \
         tf AS MATERIALIZED (SELECT m.id, m.body_bytes, m.search_len::float8 AS dl, u.lexeme, \
                                    GREATEST(cardinality(u.positions), 1)::float8 AS tf \
                             FROM memories m CROSS JOIN q \
                             CROSS JOIN LATERAL unnest(ts_filter(setweight(m.search_vector, 'A', q.fa), '{{a}}')) u \
                             WHERE {SEARCH_SCOPE} AND m.search_vector @@ q.tq), \
         per AS (SELECT tf.id, tf.body_bytes, forms.w, \
                        max(ln(1 + (s.n - df.df + 0.5) / (df.df + 0.5)) \
                            * tf.tf * 2.2 / (tf.tf + 1.2 * (0.25 + 0.75 * tf.dl / s.avgdl))) AS c \
                 FROM tf JOIN df USING (lexeme) JOIN forms ON forms.f = tf.lexeme CROSS JOIN stats s \
                 {within} \
                 GROUP BY tf.id, tf.body_bytes, forms.w), \
         scored AS (SELECT id, body_bytes, sum(c) AS score, array_agg(w ORDER BY w COLLATE \"C\") AS words \
                    FROM per GROUP BY id, body_bytes) \
         SELECT id, body_bytes, score::float4 AS score, words AS matched FROM scored \
         WHERE (NOT $8 OR cardinality(words) = (SELECT count(*) FROM kept)) \
         ORDER BY score DESC, id ASC \
         LIMIT $7"
        ),
        9,
    )
}

/// The forms the `bm25` and `bm25_postings` retrievers read for `$1`: each
/// kept query word ([`SEARCH_WORDS`]) with its exact form and its English
/// stem, one row per distinct (word, form), in byte order (ADR-0049 d12;
/// ADR-0052 d4).
fn search_forms_sql() -> String {
    format!(
        "WITH {SEARCH_WORDS}, \
         forms AS (SELECT DISTINCT kept.w, f FROM kept \
                   CROSS JOIN LATERAL unnest(array_append(COALESCE(ts_lexize('english_stem', kept.w), '{{}}'::text[]), kept.w)) f) \
         SELECT w, f FROM forms ORDER BY w COLLATE \"C\", f COLLATE \"C\""
    )
}

/// The readable counts, one statement (ADR-0052 d5): a first row with no
/// token, the served versions on the readable nodes `$2` of the memory kinds
/// `$3` and their summed length; then each token of `$4` with its document
/// frequency among them. Both sum the per-workarea counts, so they are the
/// readable rows', never the tenant's.
const POSTINGS_COUNTS: &str = "SELECT NULL::int8 AS token_id, COALESCE(sum(c.rows), 0)::int8 AS n, \
            COALESCE(sum(c.total_len), 0)::int8 AS len \
     FROM search_workarea_rows c \
     WHERE c.tenant_id = $1 AND c.workarea_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) \
     UNION ALL \
     SELECT c.token_id, sum(c.rows)::int8, 0::int8 \
     FROM search_term_rows c \
     WHERE c.tenant_id = $1 AND c.workarea_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) AND c.token_id = ANY($4::int8[]) \
     GROUP BY c.token_id";

/// The reference set's counts per node (ADR-0072 d4.2, d12): for each node
/// of `$2`, a row with no token holding its served versions of the memory
/// kinds `$3`; then each token of `$4` with its document frequency there.
/// Read by the counts' primary keys, `workarea_id = ANY` and `token_id =
/// ANY` being leakproof equalities.
const REFERENCE_COUNTS: &str = "SELECT c.workarea_id, NULL::int8 AS token_id, \
            COALESCE(sum(c.rows), 0)::int8 AS n \
     FROM search_workarea_rows c \
     WHERE c.tenant_id = $1 AND c.workarea_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) \
     GROUP BY c.workarea_id \
     UNION ALL \
     SELECT c.workarea_id, c.token_id, sum(c.rows)::int8 \
     FROM search_term_rows c \
     WHERE c.tenant_id = $1 AND c.workarea_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) AND c.token_id = ANY($4::int8[]) \
     GROUP BY c.workarea_id, c.token_id";

/// The served versions on the reference nodes `$2` of the kinds `$3` whose
/// valid time ended at or before the request's now `$4`, with their node:
/// the rows the counts hold that the reference set does not (ADR-0072
/// d4.2). Read by the `memories_valid_until` index's condition.
const REFERENCE_EXPIRED: &str = "SELECT m.id, m.workarea_id FROM memories m \
     WHERE m.tenant_id = $1 AND m.valid_until_us <= $4::int8 AND m.status = 'promoted' \
       AND m.is_tip AND NOT m.retirement AND m.workarea_id = ANY($2::uuid[]) \
       AND m.kind = ANY($3::text[])";

/// Each node of `$2`'s served versions of the memory kinds `$3`: the widen
/// signal's rows (ADR-0072 d8, as built by #244).
const VOCABULARY_ROWS: &str = "SELECT c.workarea_id, COALESCE(sum(c.rows), 0)::int8 \
     FROM search_workarea_rows c \
     WHERE c.tenant_id = $1 AND c.workarea_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) \
     GROUP BY c.workarea_id";

/// Every token's count per node of `$2`, [`REFERENCE_COUNTS`]' second half
/// without its token filter, cut at `$4` rows so a vocabulary past the
/// signal's cap is never read whole: the widen signal's read, made off the
/// request path (ADR-0072 d8, as built by #244).
const VOCABULARY_TERMS: &str = "SELECT c.workarea_id, c.token_id, sum(c.rows)::int8 \
     FROM search_term_rows c \
     WHERE c.tenant_id = $1 AND c.workarea_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) \
     GROUP BY c.workarea_id, c.token_id \
     LIMIT $4";

/// Every token each expired version `$2` holds, from its postings by the
/// `(memory_id)` index.
const VOCABULARY_EXPIRED_TOKENS: &str = "SELECT DISTINCT p.memory_id, p.token_id \
     FROM search_postings p \
     WHERE p.tenant_id = $1 AND p.memory_id = ANY($2::uuid[]) AND p.kind = 1";

/// Which tokens of `$3` each expired version `$2` holds, from its postings
/// by the `(memory_id)` index.
const REFERENCE_EXPIRED_TOKENS: &str = "SELECT DISTINCT p.memory_id, p.token_id \
     FROM search_postings p \
     WHERE p.tenant_id = $1 AND p.memory_id = ANY($2::uuid[]) AND p.kind = 1 \
       AND p.token_id = ANY($3::int8[])";

/// The own held rows a recall may rank: the held versions of the keys `$2`
/// ([`held_authors`]) on their nodes `$3` of the kinds `$4`, never a
/// retirement version (ADR-0025 d4, d5), with their lengths; read by the
/// `memories_author` index (ADR-0052 d5, open question 7).
const POSTINGS_OWN_HELD: &str = "SELECT m.id, m.search_len FROM memories m \
     WHERE m.tenant_id = $1 AND m.status = 'held' \
        AND (m.author_key_id = ANY($2::uuid[]) \
          OR (m.scan_finding IS NULL AND encode(substring(sha256(convert_to(COALESCE(m.scanned_under, ''), 'UTF8')) FROM 1 FOR 16), 'hex')::uuid = ANY($2::uuid[]))) \
        AND m.workarea_id = ANY($3::uuid[]) AND m.kind = ANY($4::text[]) AND NOT m.retirement \
        AND (m.valid_until_us IS NULL OR m.valid_until_us > $5::int8)";

/// The served versions on the readable nodes `$2` of the kinds `$3` whose
/// valid time ended at or before the request's now `$4`, with their
/// lengths: the rows the per-workarea counts hold that the scope does not,
/// taken out of the statistics so an expired version counts in none, as in
/// `bm25`'s scan (proposed ADR-0059 d3; review M5). Read by the
/// `memories_valid_until` index's condition.
const POSTINGS_EXPIRED: &str = "SELECT m.id, m.search_len FROM memories m \
     WHERE m.tenant_id = $1 AND m.valid_until_us <= $4::int8 AND m.status = 'promoted' \
       AND m.is_tip AND NOT m.retirement AND m.workarea_id = ANY($2::uuid[]) \
       AND m.kind = ANY($3::text[])";

/// Each token of `$3`'s document frequency among the own held rows `$2`,
/// from their postings by the `(memory_id)` index.
const POSTINGS_OWN_HELD_DF: &str = "SELECT p.token_id, count(DISTINCT p.memory_id)::int8 AS df \
     FROM search_postings p \
     WHERE p.tenant_id = $1 AND p.memory_id = ANY($2::uuid[]) AND p.kind = 1 \
       AND p.token_id = ANY($3::int8[]) \
     GROUP BY p.token_id";

/// The BM25 contribution of a posting `p` for the query form `q`, written as
/// the `bm25` statement writes it (k1 1.2, b 0.75; [`bm25_sql`]), so equal
/// inputs give equal floats.
const POSTING_CONTRIBUTION: &str = "q.idf * p.weight::float8 * 2.2 \
     / (p.weight::float8 + 1.2 * (0.25 + 0.75 * p.doc_len::float8 / $9))";

/// The `bm25_postings` statement (ADR-0052 d4): the postings of the
/// generating tokens `$10` read by the `(kind, token_id)` index, kept when
/// served on a readable node `$2` of a memory kind `$3` or one of the own
/// held rows `$4`; each candidate's partial BM25 score over those tokens;
/// the best `$11` candidates (all when NULL), ordered as the answer is, by
/// the `float4` score then id, so a cut at `k` keeps the rows `bm25` keeps,
/// rescored with every form of
/// every word, `$5`/`$6`/`$7` (word, token, readable document frequency),
/// by the `(memory_id)` index, so each returned row's score is its full
/// BM25 score; under `$12` (`match: all`) only rows holding all `$13` words.
/// `$8` the readable row count, `$9` their mean length, `$14` `k`. Every
/// predicate on the postings is a leakproof equality, so the indexes serve
/// the serving role under row security. Exported so the suite plans the
/// statement the adapter runs.
#[must_use]
pub fn bm25_postings_sql() -> String {
    bm25_postings_sql_within("")
}

/// [`bm25_postings_sql`] with `anchor`, a predicate on `p.memory_id` from
/// `$16` on (`$15` the request's now), in both parts of the generating postings, so the readable
/// counts and every score are the unfiltered ones (ADR-0057 d4).
fn bm25_postings_sql_within(anchor: &str) -> String {
    with_now(
        &format!(
            "WITH q AS (SELECT q.w, q.token_id, ln(1 + ($8 - q.df + 0.5) / (q.df + 0.5)) AS idf \
                    FROM unnest($5::text[], $6::int8[], $7::float8[]) AS q(w, token_id, df)), \
         p AS (SELECT p.memory_id, p.token_id, p.weight, p.doc_len FROM search_postings p \
               WHERE p.tenant_id = $1 AND p.kind = 1 AND p.token_id = ANY($10::int8[]) \
                 AND p.workarea_id = ANY($2::uuid[]) AND p.state = 1 \
                 AND p.memory_kind = ANY($3::int2[]){anchor} \
               UNION ALL \
               SELECT p.memory_id, p.token_id, p.weight, p.doc_len FROM search_postings p \
               WHERE p.tenant_id = $1 AND p.memory_id = ANY($4::uuid[]) AND p.kind = 1 \
                 AND p.token_id = ANY($10::int8[]){anchor}), \
         gen AS (SELECT p.memory_id, q.w, max({POSTING_CONTRIBUTION}) AS c \
                 FROM p JOIN q ON q.token_id = p.token_id GROUP BY p.memory_id, q.w), \
         {EXPIRED_IDS}, \
         top AS (SELECT memory_id FROM gen \
                 WHERE memory_id NOT IN (SELECT id FROM expired) GROUP BY memory_id \
                 ORDER BY sum(c)::float4 DESC, memory_id ASC LIMIT $11), \
         scored AS (SELECT p.memory_id, q.w, max({POSTING_CONTRIBUTION}) AS c \
                    FROM top JOIN search_postings p ON p.memory_id = top.memory_id \
                    JOIN q ON q.token_id = p.token_id \
                    WHERE p.tenant_id = $1 AND p.kind = 1 AND p.token_id = ANY($6::int8[]) \
                    GROUP BY p.memory_id, q.w), \
         ranked AS (SELECT memory_id, sum(c) AS score, array_agg(w ORDER BY w COLLATE \"C\") AS words \
                    FROM scored GROUP BY memory_id) \
         SELECT r.memory_id AS id, m.body_bytes, r.score::float4 AS score, r.words AS matched \
         FROM ranked r JOIN memories m ON m.tenant_id = $1 AND m.id = r.memory_id \
         WHERE NOT $12 OR cardinality(r.words) = $13 \
         ORDER BY score DESC, id ASC \
         LIMIT $14"
        ),
        15,
    )
}

/// The `sparse` statement (ADR-0052 d16): the kind 2 postings of the query
/// tokens `$5` read by the served index, kept when served on a readable node
/// `$2` of a memory kind `$3`, and the own held rows `$4`'s by the
/// `(memory_id)` index; each candidate scored `sum(idf × weight)` with the
/// query weights `$6`, which the model ships, so no corpus statistic is
/// read; under `$8` (`match: all`) only rows holding all `$9` tokens; the
/// best `$10`, by the `float4` score then id. Each returned row names its
/// pieces (`$7`), their contributions, largest first, and whether its text
/// holds each (`doc_len` 1; d17). Every predicate on the postings is a
/// leakproof equality.
#[must_use]
pub fn sparse_sql() -> String {
    sparse_sql_within("")
}

/// [`sparse_sql`] with `anchor`, a predicate on `p.memory_id` from `$12`
/// on (ADR-0057 d4); `$11` the request's now.
fn sparse_sql_within(anchor: &str) -> String {
    with_now(&format!("WITH q AS (SELECT * FROM unnest($5::int8[], $6::float8[], $7::text[]) AS q(token_id, w, piece)), \
     p AS (SELECT p.memory_id, p.token_id, p.weight, p.doc_len FROM search_postings p \
           WHERE p.tenant_id = $1 AND p.kind = 2 AND p.token_id = ANY($5::int8[]) \
             AND p.workarea_id = ANY($2::uuid[]) AND p.state = 1 \
             AND p.memory_kind = ANY($3::int2[]) /*anchor*/ \
           UNION ALL \
           SELECT p.memory_id, p.token_id, p.weight, p.doc_len FROM search_postings p \
           WHERE p.tenant_id = $1 AND p.memory_id = ANY($4::uuid[]) AND p.kind = 2 \
             AND p.token_id = ANY($5::int8[]) /*anchor*/), \
     hit AS (SELECT p.memory_id, q.piece, q.w * p.weight::float8 AS c, p.doc_len = 1 AS in_text \
             FROM p JOIN q ON q.token_id = p.token_id), \
     {EXPIRED_IDS}, \
     top AS (SELECT memory_id, sum(c) AS score FROM hit \
             WHERE memory_id NOT IN (SELECT id FROM expired) GROUP BY memory_id \
             HAVING NOT $8 OR count(*) = $9 \
             ORDER BY sum(c)::float4 DESC, memory_id ASC LIMIT $10) \
     SELECT t.memory_id AS id, m.body_bytes, t.score::float4 AS score, \
            array_agg(h.piece ORDER BY h.c DESC, h.piece COLLATE \"C\") AS pieces, \
            array_agg(h.c::float4 ORDER BY h.c DESC, h.piece COLLATE \"C\") AS contributions, \
            array_agg(h.in_text ORDER BY h.c DESC, h.piece COLLATE \"C\") AS in_text \
     FROM top t JOIN hit h ON h.memory_id = t.memory_id \
     JOIN memories m ON m.tenant_id = $1 AND m.id = t.memory_id \
     GROUP BY t.memory_id, m.body_bytes, t.score \
     ORDER BY score DESC, id ASC"), 11)
        .replace("/*anchor*/", anchor)
}

/// The sparse queue's health for one tenant (ADR-0052 d15): its entries
/// waiting, those whose last encode failed, the parked ones, and whether
/// its state is on.
const SPARSE_QUEUE_HEALTH: &str = "SELECT count(*) FILTER (WHERE parked_at IS NULL)::int8, \
            count(*) FILTER (WHERE parked_at IS NULL AND attempts > 0)::int8, \
            count(*) FILTER (WHERE parked_at IS NOT NULL)::int8, \
            EXISTS (SELECT 1 FROM search_sparse_state s WHERE s.tenant_id = $1 AND s.enabled) \
     FROM search_sparse_queue WHERE tenant_id = $1";

/// The tenants the encoder's pass has work for, every tenant in one read
/// (under `oml.startup_check`): a due entry, by the oldest-first index, or
/// a sparse state not on yet or a backfill unfinished; with whether to run
/// a backfill step (ADR-0052 d15).
const SPARSE_TENANTS: &str = "SELECT t.id, \
            (s.tenant_id IS NULL OR NOT s.enabled OR s.backfilled_at IS NULL) AS backfill \
       FROM tenants t LEFT JOIN search_sparse_state s ON s.tenant_id = t.id \
      WHERE s.tenant_id IS NULL OR NOT s.enabled OR s.backfilled_at IS NULL \
         OR EXISTS (SELECT 1 FROM search_sparse_queue q \
                     WHERE q.tenant_id = t.id AND q.parked_at IS NULL \
                       AND q.next_at <= clock_timestamp()) \
      ORDER BY t.id";

/// One bounded backfill step (ADR-0052 d15): the tenant `$1`'s next `$3`
/// versions after `$2` by id, the ones with postings and no kind 2 queued;
/// answers the last id read, how many were read and how many queued, so
/// the next step resumes there.
const SPARSE_BACKFILL_STEP: &str = "WITH batch AS ( \
         SELECT m.id, m.created_at, \
                oml_search_state(m.status, m.is_tip, m.retirement, m.kind) AS state \
           FROM memories m WHERE m.tenant_id = $1 AND ($2::uuid IS NULL OR m.id > $2) \
          ORDER BY m.id LIMIT $3), \
     ins AS (INSERT INTO search_sparse_queue (tenant_id, memory_id, enqueued_at, next_at) \
             SELECT $1, b.id, b.created_at, clock_timestamp() FROM batch b \
              WHERE b.state IS NOT NULL \
                AND NOT EXISTS (SELECT 1 FROM search_postings p \
                                 WHERE p.tenant_id = $1 AND p.memory_id = b.id AND p.kind = 2) \
             ON CONFLICT (tenant_id, memory_id) DO NOTHING RETURNING 1) \
     SELECT (SELECT id FROM batch ORDER BY id DESC LIMIT 1), \
            (SELECT count(*) FROM batch)::int8, (SELECT count(*) FROM ins)::int8";

/// Up to `$2` due entries of tenant `$1`, oldest first, skipping any
/// another pass holds, leased for `$3` seconds so another lead's pass skips
/// them until then; each with its version's claim (ADR-0052 d15).
const SPARSE_PENDING: &str = "WITH due AS ( \
         SELECT q.memory_id FROM search_sparse_queue q \
          WHERE q.tenant_id = $1 AND q.parked_at IS NULL AND q.next_at <= clock_timestamp() \
          ORDER BY q.enqueued_at, q.memory_id LIMIT $2 FOR UPDATE SKIP LOCKED), \
     leased AS (UPDATE search_sparse_queue q \
                   SET next_at = clock_timestamp() + make_interval(secs => $3) \
                  FROM due WHERE q.tenant_id = $1 AND q.memory_id = due.memory_id \
                RETURNING q.memory_id, q.enqueued_at) \
     SELECT l.memory_id, COALESCE(m.search_text, m.claim), (extract(epoch FROM l.enqueued_at) * 1000000)::int8 \
       FROM leased l JOIN memories m ON m.tenant_id = $1 AND m.id = l.memory_id \
      ORDER BY l.enqueued_at, l.memory_id";

/// The postings health statement (ADR-0052 d12), as the serving role under
/// `$1`'s row security, `$2` its partition's name: whether the partition
/// exists; the versions that should have postings (a hit kind, no
/// retirement, not erased, some lexeme) and have none, the fill
/// incomplete; the served versions with no served posting, behind; the
/// per-workarea counts that disagree with the served versions; and, of
/// 200 count rows and as many served (workarea, kind, token)
/// triples, the ones whose count disagrees with the served postings or is
/// missing. The 200 start at a random token each read and wrap round, so
/// repeated reads cover every count; each side's read is bounded by its
/// index (review F7). The version checks read every version of the tenant
/// once with an index probe each: doctor's cost, not a recall's.
const POSTINGS_HEALTH: &str = "WITH v AS ( \
         SELECT m.id, m.workarea_id, oml_search_memory_kind(m.kind) AS mk, m.search_len, \
                oml_search_state(m.status, m.is_tip, m.retirement, m.kind) AS state \
         FROM memories m WHERE m.tenant_id = $1), \
     want AS (SELECT * FROM v WHERE state IS NOT NULL AND search_len > 0), \
     served AS (SELECT workarea_id, mk, count(*)::int8 AS rows, sum(search_len)::int8 AS len \
                FROM v WHERE state = 1 GROUP BY workarea_id, mk), \
     kept AS (SELECT workarea_id, memory_kind AS mk, rows::int8 AS rows, total_len AS len \
              FROM search_workarea_rows WHERE tenant_id = $1 AND kind = 1), \
     start AS MATERIALIZED (SELECT ((random() - 0.5) * 1.8e19)::int8 AS s), \
     sampled AS (SELECT * FROM ( \
         (SELECT 0 AS lap, c.workarea_id, c.memory_kind, c.token_id, c.rows \
            FROM search_term_rows c, start WHERE c.tenant_id = $1 AND c.token_id >= start.s \
           ORDER BY c.token_id LIMIT 200) \
         UNION ALL \
         (SELECT 1, c.workarea_id, c.memory_kind, c.token_id, c.rows \
            FROM search_term_rows c, start WHERE c.tenant_id = $1 AND c.token_id < start.s \
           ORDER BY c.token_id LIMIT 200)) x \
       WHERE x.token_id IS NOT NULL ORDER BY lap, token_id LIMIT 200), \
     posted AS (SELECT DISTINCT y.workarea_id, y.memory_kind, y.token_id FROM ( \
         (SELECT 0 AS lap, p.workarea_id, p.memory_kind, p.token_id FROM search_postings p, start \
           WHERE p.tenant_id = $1 AND p.kind = 1 AND p.state = 1 AND p.token_id >= start.s \
           ORDER BY p.token_id LIMIT 200) \
         UNION ALL \
         (SELECT 1, p.workarea_id, p.memory_kind, p.token_id FROM search_postings p, start \
           WHERE p.tenant_id = $1 AND p.kind = 1 AND p.state = 1 AND p.token_id < start.s \
           ORDER BY p.token_id LIMIT 200)) y) \
     SELECT to_regclass($2) IS NOT NULL AS partition, \
       (SELECT count(*) FROM want w WHERE NOT EXISTS ( \
            SELECT 1 FROM search_postings p WHERE p.tenant_id = $1 AND p.memory_id = w.id))::int8 \
         AS unfilled, \
       (SELECT count(*) FROM want w WHERE w.state = 1 AND NOT EXISTS ( \
            SELECT 1 FROM search_postings p WHERE p.tenant_id = $1 AND p.memory_id = w.id \
                                          AND p.state = 1))::int8 AS served_behind, \
       (SELECT count(*) FROM served s FULL JOIN kept k USING (workarea_id, mk) \
         WHERE s.rows IS DISTINCT FROM k.rows OR s.len IS DISTINCT FROM k.len)::int8 \
         AS workarea_drift, \
       ((SELECT count(*) FROM sampled c WHERE c.rows <> ( \
            SELECT count(DISTINCT p.memory_id) FROM search_postings p \
             WHERE p.tenant_id = $1 AND p.kind = 1 AND p.token_id = c.token_id \
               AND p.workarea_id = c.workarea_id AND p.state = 1 \
               AND p.memory_kind = c.memory_kind)) \
        + (SELECT count(*) FROM posted t WHERE NOT EXISTS ( \
            SELECT 1 FROM search_term_rows c WHERE c.tenant_id = $1 AND c.kind = 1 \
               AND c.workarea_id = t.workarea_id AND c.memory_kind = t.memory_kind \
               AND c.token_id = t.token_id)))::int8 AS term_drift, \
       ((SELECT count(*) FROM sampled) + (SELECT count(*) FROM posted))::int8 AS terms_checked";

/// A lexeme's posting id, as `oml_token_id` computes it in SQL: the first
/// eight bytes of SHA-256 over its UTF-8 bytes, a signed big-endian integer
/// (ADR-0052 d2). No table maps a word to an id, so a write assigns
/// nothing and no shared row holds a tenant's word.
#[must_use]
pub fn token_id(lexeme: &str) -> i64 {
    let digest = Sha256Hash::digest(lexeme.as_bytes()).0;
    let mut first = [0_u8; 8];
    first.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(first)
}

/// The share of the readable rows above which a word with a form held by
/// more is frequent, and pruned whole once the query's postings pass
/// [`POSTING_BUDGET`] (ADR-0052 d6); the sweep that set it is
/// eval/bench/results/prune-sweep.json (ADR-0052 open question 2).
pub const FREQUENT_SHARE: f64 = 0.1;

/// The most postings the kept words may hold among the readable rows; past
/// it the kept word with the most postings is pruned too, and below it
/// nothing is (ADR-0052 d6): at 10,000 a universe recall over 99,721
/// readable rows measured 25.5 ms p95 (bench/scale/REPORT.md; ADR-0052
/// open question 2).
pub const POSTING_BUDGET: u64 = 10_000;

/// The candidates rescored with every form after a pruned read, as a
/// multiple of `k` (ADR-0052 d4 step 5).
pub const RESCORE_DEPTH: usize = 4;

/// The two constants of ADR-0052 d6 a store prunes by. The lead serves
/// [`PruneLimits::SERVED`]; the suite names others to prove the laws on a
/// small fixture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PruneLimits {
    /// [`FREQUENT_SHARE`]: a form whose readable document frequency is above
    /// this share of the readable rows is pruned.
    pub frequent_share: f64,
    /// [`POSTING_BUDGET`]: the kept forms' summed readable frequencies stay
    /// at or below it.
    pub posting_budget: u64,
}

impl PruneLimits {
    /// The constants the lead serves.
    pub const SERVED: Self = Self {
        frequent_share: FREQUENT_SHARE,
        posting_budget: POSTING_BUDGET,
    };

    /// No pruning: every form is read, so the list is `bm25`'s.
    pub const NONE: Self = Self {
        frequent_share: f64::INFINITY,
        posting_budget: u64::MAX,
    };

    /// The query words pruned, by index into `words`, each word its present
    /// forms (token, readable document frequency), over `rows` readable
    /// rows. A word is pruned whole, every form of it, so a word the answer
    /// does not name in `pruned` is read in full and every row holding it
    /// is a candidate (ADR-0052 d6, d11). None while the distinct forms'
    /// postings together fit the budget, so a scope that can be read whole
    /// is; past it, every frequent word, one with a form held by more than
    /// the share of the readable rows; then, while the kept words' distinct
    /// forms sum above the budget, the kept word with the most postings,
    /// ties by index. A word with no present form is neither kept nor
    /// pruned. It reads the readable counts alone, so which word is pruned
    /// depends only on rows the key may read.
    #[must_use]
    pub fn pruned(&self, words: &[Vec<(i64, u64)>], rows: u64) -> BTreeSet<usize> {
        let postings = |chosen: &mut dyn Iterator<Item = &Vec<(i64, u64)>>| -> u64 {
            let distinct: BTreeMap<i64, u64> = chosen.flatten().copied().collect();
            distinct.values().sum()
        };
        if postings(&mut words.iter()) <= self.posting_budget {
            return BTreeSet::new();
        }
        #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
        let bound = self.frequent_share * rows as f64;
        let mut pruned = BTreeSet::new();
        for (i, forms) in words.iter().enumerate() {
            #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
            let frequent = forms.iter().any(|(_, df)| *df as f64 > bound);
            if !forms.is_empty() && frequent {
                pruned.insert(i);
            }
        }
        loop {
            let kept: Vec<usize> = (0..words.len())
                .filter(|i| !pruned.contains(i) && !words[*i].is_empty())
                .collect();
            if postings(&mut kept.iter().map(|i| &words[*i])) <= self.posting_budget {
                break;
            }
            let Some(&heaviest) = kept.iter().max_by(|a, b| {
                let sum = |i: usize| words[i].iter().map(|(_, df)| df).sum::<u64>();
                sum(**a).cmp(&sum(**b)).then(b.cmp(a))
            }) else {
                break;
            };
            pruned.insert(heaviest);
        }
        pruned
    }
}

/// The `trigram` statement (ADR-0048 S3): `pg_trgm` word similarity of the
/// query to the normalised text above 0.3, the threshold `SET LOCAL` gives
/// `<%`. No index serves it: under row security `<%` is not leakproof, so it
/// scans the scope (ADR-0034 d13; ADR-0049 d12). `$6` the query, `$7` `k`,
/// `$8` the request's now, the anchor's binds from `$9`.
fn trigram_sql(anchor: &str) -> String {
    with_now(
        &format!(
            "SELECT m.id, m.body_bytes, word_similarity($6, oml_search_text(COALESCE(m.search_text, m.claim))) AS score \
         FROM memories m \
         WHERE {SEARCH_SCOPE}{anchor} AND $6 <% oml_search_text(COALESCE(m.search_text, m.claim)) \
         ORDER BY score DESC, m.id ASC \
         LIMIT $7"
        ),
        8,
    )
}

/// The `did_you_mean` statement (ADR-0049 d6, d12): words of the
/// maintained vocabulary of the readable nodes alone, the words of their
/// promoted served tips, similar to a query word of four or more letters
/// that is not an English stop word, above `pg_trgm` similarity 0.4. No row
/// is read: the vocabulary is kept by the write that promotes, replaces or
/// retires a row (migration 0020). `$1` the tenant, `$2` the nodes, `$3`
/// the words, `$4` the limit.
const SUGGEST: &str = "WITH q AS (SELECT w FROM unnest($3::text[]) w \
                WHERE char_length(w) >= 4 AND ts_lexize('english_stem', w) <> '{}'::text[]) \
     SELECT word FROM (SELECT v.word, max(similarity(v.word, q.w)) AS s \
                         FROM search_vocabulary v CROSS JOIN q \
                        WHERE v.tenant_id = $1 AND v.workarea_id = ANY($2::uuid[]) \
                          AND v.word <> q.w AND similarity(v.word, q.w) > 0.4 \
                        GROUP BY v.word) x \
     ORDER BY s DESC, word ASC \
     LIMIT $4";

/// The similar check's candidates (ADR-0050 d17), binding `$1` the tenant,
/// `$2` the write's keys, `$3` the nodes whose served tips it reads, `$4`
/// the own keys, `$5` the nodes of its own held rows and `$6` the limit.
/// Only bigint and uuid equalities, which are leakproof, reach
/// `similar_keys_key` under row security, and the index carries every
/// column the statement reads. Exported so the suite plans the statement
/// the adapter runs.
pub const SIMILAR_CANDIDATES: &str = "SELECT s.memory_id FROM similar_keys s \
     WHERE s.tenant_id = $1 AND s.key = ANY($2::int8[]) \
       AND ((s.state = 1 AND s.workarea_id = ANY($3::uuid[])) \
         OR (s.state = 0 AND s.author_key_id = ANY($4::uuid[]) \
             AND s.workarea_id = ANY($5::uuid[]))) \
     GROUP BY s.memory_id \
     ORDER BY count(*) DESC, s.memory_id ASC \
     LIMIT $6";

/// The most versions the similar keys' health reads per call (ADR-0050 d15).
const SIMILAR_HEALTH_SAMPLE: i64 = 2000;

/// The similar keys' health for doctor (ADR-0050 d15, d17), over a sample:
/// whether the tenant's partition exists, and, of [`SIMILAR_HEALTH_SAMPLE`]
/// versions in id order from a random id, wrapping round, the versions that
/// should hold keys and hold none, and those whose keys hold a state other
/// than theirs or that hold keys and should not. `$1` is the tenant, `$2`
/// the partition's name and `$3` the sample's size; the random id it starts
/// at is drawn in the statement. Repeated reads cover every version; each is bounded by the
/// primary key's order, so the cost is the sample's, never the tenant's, and
/// the word split runs on the sample alone. A key whose version is gone
/// altogether is outside the sample; the delete trigger removes it.
const SIMILAR_HEALTH: &str = "WITH start AS MATERIALIZED (SELECT gen_random_uuid() AS s), v AS ( \
         SELECT x.id, x.state, cardinality(oml_similar_words(coalesce(x.search_text, x.claim))) > 0 AS worded FROM ( \
           (SELECT 0 AS lap, m.id, m.claim, m.search_text, \
                   oml_similar_state(m.status, m.is_tip, m.retirement, m.kind) AS state \
              FROM memories m WHERE m.tenant_id = $1 AND m.id >= (SELECT s FROM start) ORDER BY m.id LIMIT $3) \
           UNION ALL \
           (SELECT 1, m.id, m.claim, m.search_text, \
                   oml_similar_state(m.status, m.is_tip, m.retirement, m.kind) \
              FROM memories m WHERE m.tenant_id = $1 AND m.id < (SELECT s FROM start) ORDER BY m.id LIMIT $3)) x \
         ORDER BY x.lap, x.id LIMIT $3) \
     SELECT to_regclass($2) IS NOT NULL, \
       (SELECT count(*) FROM v WHERE v.state IS NOT NULL AND v.worded AND NOT EXISTS ( \
            SELECT 1 FROM similar_keys s WHERE s.tenant_id = $1 AND s.memory_id = v.id))::int8, \
       (SELECT count(DISTINCT s.memory_id) FROM similar_keys s JOIN v ON v.id = s.memory_id \
         WHERE s.tenant_id = $1 AND s.state IS DISTINCT FROM v.state)::int8";

/// The Postgres `JournalStore`.
#[derive(Debug, Clone)]
pub struct PgStore {
    pool: PgPool,
    owner_url: Option<OwnerUrl>,
    /// Whether `pg_trgm` was found by the last probe, at connect and after
    /// each migration: the trigram retriever is served only then (ADR-0049
    /// d8). Shared by every clone of the store, as the database is.
    trigram: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Whether the database holds 0045's meters, once a commit found them:
    /// a database not yet migrated to 0045, which only an upgrade test
    /// writes to, commits unmetered and is filled by 0045 (ADR-0071
    /// Consequences). Shared by every clone, as the database is.
    metered: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The constants `bm25_postings` prunes by (ADR-0052 d6).
    prune: PruneLimits,
    /// The most readable anchored rows a filtered list reads anchor-first
    /// ([`ANCHOR_FIRST_MAX`]; ADR-0057 d4).
    anchor_first_max: usize,
}

impl PgStore {
    /// Connects a pool to `database_url` (no migration side effect; call
    /// [`Self::migrate`] where bootstrap wants it), with
    /// [`Self::with_search_probe`] run, so the trigram retriever is served
    /// where `pg_trgm` exists. The store creates no tenant's partitions until
    /// [`Self::with_owner_url`] gives it the owner's URL.
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(database_url)
            .await?;
        Self {
            pool,
            owner_url: None,
            trigram: std::sync::Arc::default(),
            metered: std::sync::Arc::default(),
            prune: PruneLimits::SERVED,
            anchor_first_max: ANCHOR_FIRST_MAX,
        }
        .with_search_probe()
        .await
    }

    #[must_use]
    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            pool,
            owner_url: None,
            trigram: std::sync::Arc::default(),
            metered: std::sync::Arc::default(),
            prune: PruneLimits::SERVED,
            anchor_first_max: ANCHOR_FIRST_MAX,
        }
    }

    /// The store pruning `bm25_postings` by `limits` rather than
    /// [`PruneLimits::SERVED`]; the lead never calls it, the suite does, to
    /// prove the pruning laws on a small fixture (ADR-0052 d12).
    #[must_use]
    pub fn with_prune_limits(mut self, limits: PruneLimits) -> Self {
        self.prune = limits;
        self
    }

    /// The store choosing the anchor-first plan for at most `max` readable
    /// anchored rows, and the filter joined on the indexed columns above it
    /// (ADR-0057 d4): [`ANCHOR_FIRST_MAX`] unless set; 0 always joins.
    #[must_use]
    pub fn with_anchor_first_max(mut self, max: usize) -> Self {
        self.anchor_first_max = max;
        self
    }

    /// The store with the retrievers its database can run: the trigram
    /// retriever only where `pg_trgm` is installed (ADR-0049 d8). Without
    /// the probe the store serves the keyword and BM25 retrievers alone.
    pub async fn with_search_probe(self) -> Result<Self, sqlx::Error> {
        self.probe_search().await?;
        Ok(self)
    }

    async fn probe_search(&self) -> Result<(), sqlx::Error> {
        let trigram: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_trgm')",
        )
        .fetch_one(&self.pool)
        .await?;
        self.trigram
            .store(trigram, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// The owner's URL, which the store opens a connection to for each
    /// tenant's partition DDL alone and closes after it (ADR-0034 d4, d11);
    /// without it, [`JournalStore::create_tenant_partitions`] refuses.
    #[must_use]
    pub fn with_owner_url(self, owner_url: &str) -> Self {
        Self {
            owner_url: Some(OwnerUrl(owner_url.into())),
            ..self
        }
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Opens the owner's connection at `owner_url` (ADR-0034 d2, d4): one
    /// connection whose `after_connect` sets `oml.serving_role` to this
    /// store's login, its pool's `current_user`, so a granting migration names
    /// it (d3); no role name is read from configuration (d2). It is not a
    /// long-lived pool: the caller runs [`role_split_preflight`],
    /// [`Self::migrate_as`] and [`role_split_postflight`] on it, then closes
    /// it before serving (d4), and no owner connection outlives the boot but
    /// d11's partition DDL (d8).
    pub async fn connect_owner(&self, owner_url: &str) -> Result<PgPool, sqlx::Error> {
        let serving: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&self.pool)
            .await?;
        PgPoolOptions::new()
            .max_connections(1)
            .after_connect(move |conn, _meta| {
                let serving = serving.clone();
                Box::pin(async move {
                    sqlx::query("SELECT set_config('oml.serving_role', $1, false)")
                        .bind(serving)
                        .execute(conn)
                        .await
                        .map(|_| ())
                })
            })
            .connect(owner_url)
            .await
    }

    /// Applies any pending committed migrations (forward-only,
    /// postgres-schema.md "Migrations") on `owner`, a pool from
    /// [`Self::connect_owner`] (ADR-0034 d2, d3, d4), so each table's
    /// migration grants this store's login the DML the adapter's statements
    /// use (postgres-schema.md "Privileges"). A migration that grants, run
    /// without `oml.serving_role`, fails at `current_setting` (`42704`).
    ///
    /// Before 0017 is applied, [`align_pre_0017_statuses`] stores each
    /// workarea's status as the one v0.5.1 and earlier served, so 0017's
    /// `workareas_universe_committed` holds over a database whose root a
    /// v0.1.0 build stored `development` (docs/operations/upgrades.md
    /// "Upgrading from v0.5.1 or earlier"). Returns the rows it aligned.
    pub async fn migrate_as(&self, owner: &PgPool) -> Result<u64, sqlx::migrate::MigrateError> {
        let aligned = align_pre_0017_statuses(owner)
            .await
            .map_err(sqlx::migrate::MigrateError::Execute)?;
        MIGRATOR.run(owner).await?;
        // A tenant a lead before 0021 created has no postings partition, and
        // its writes fail until it does (ADR-0052 d1).
        ensure_postings_partitions(owner)
            .await
            .map_err(sqlx::migrate::MigrateError::Execute)?;
        // A tenant a lead before 0027 created has no similar keys
        // partition, and its writes fail until it does (ADR-0050 d17).
        ensure_similar_partitions(owner)
            .await
            .map_err(sqlx::migrate::MigrateError::Execute)?;
        // A tenant a lead before 0023 created has no telemetry partitions,
        // and its recalls are not recorded until it does (ADR-0054 d2).
        telemetry::ensure_partitions(owner)
            .await
            .map_err(sqlx::migrate::MigrateError::Execute)?;
        // 0020 may have created pg_trgm, which serves bm25-trigram.
        self.probe_search().await?;
        Ok(aligned)
    }

    /// [`Self::migrate_as`] over an owner connection opened at `owner_url`
    /// by [`Self::connect_owner`] and closed before this returns (ADR-0034
    /// d4), with no boot check run around it.
    pub async fn migrate(&self, owner_url: &str) -> Result<u64, sqlx::migrate::MigrateError> {
        let owner = self.connect_owner(owner_url).await?;
        let applied = self.migrate_as(&owner).await;
        owner.close().await;
        applied
    }

    /// The startup fork check (ADR-0002 d7): pages the stored events and
    /// runs the domain verification procedure (chain-format.md steps 1–7
    /// and 9) against the stored head. A caller that gets an error must
    /// refuse to sign on top of this chain.
    pub async fn verify_stored_chain(
        &self,
        tenant_id: Uuid,
        keys: &KeySet,
    ) -> Result<ChainHead, ForkCheckError> {
        let stated = self
            .chain_head(tenant_id)
            .await?
            .ok_or(ForkCheckError::EmptyChain)?;
        let mut events = Vec::new();
        let mut from_seq = 1_u64;
        loop {
            let page = self
                .events_range(tenant_id, from_seq, FORK_CHECK_PAGE)
                .await?;
            let n = page.len();
            events.extend(page);
            if n < FORK_CHECK_PAGE {
                break;
            }
            from_seq += n as u64;
        }
        Ok(verify_chain(&events, keys, Some(&stated))?)
    }

    /// The rows of `tenants` that predate names, per table, the tables that
    /// hold none left out: each tenant's own `tenants` row and its
    /// `workareas` and `api_keys` rows whose `name` is NULL. 0006 adds the
    /// column NULL on every row it finds and every creation since names its
    /// row, an issuance's key included, so a NULL here is a row the lead
    /// names before it serves (ADR-0026 d9, d11; http-api.md "Rows created
    /// before names").
    pub async fn rows_without_names(
        &self,
        tenants: &[Uuid],
    ) -> Result<Vec<(&'static str, u64)>, StoreError> {
        let (mut tenant_rows, mut workarea_rows, mut key_rows) = (0_i64, 0_i64, 0_i64);
        for tenant_id in tenants {
            let mut tx = self.pool.begin().await.map_err(backend)?;
            set_tenant(&mut tx, *tenant_id).await?;
            let row = sqlx::query(
                "SELECT (SELECT count(*) FROM tenants WHERE id = $1 AND name IS NULL) \
                        AS tenants, \
                        (SELECT count(*) FROM workareas WHERE tenant_id = $1 AND name IS NULL) \
                        AS workareas, \
                        (SELECT count(*) FROM api_keys WHERE tenant_id = $1 AND name IS NULL) \
                        AS api_keys",
            )
            .bind(tenant_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
            tenant_rows += row.try_get::<i64, _>("tenants").map_err(backend)?;
            workarea_rows += row.try_get::<i64, _>("workareas").map_err(backend)?;
            key_rows += row.try_get::<i64, _>("api_keys").map_err(backend)?;
        }
        [
            ("tenants", tenant_rows),
            ("workareas", workarea_rows),
            ("api_keys", key_rows),
        ]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(table, n)| Ok((table, u64::try_from(n).map_err(backend)?)))
        .collect()
    }

    /// One workarea read under the tenant's RLS context: `sql` binds the
    /// tenant as `$1` and a node id as `$2` and selects [`WORKAREA_COLUMNS`].
    async fn workarea_rows(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        sql: &str,
    ) -> Result<Vec<WorkareaRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(sql)
            .bind(tenant_id)
            .bind(workarea_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(workarea_from_row).collect()
    }

    /// One transition read under the tenant's RLS context: `sql` binds the
    /// tenant as `$1` and one id as `$2` and selects [`TRANSITION_COLUMNS`].
    async fn transition_rows(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        sql: &str,
    ) -> Result<Vec<TransitionRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(sql)
            .bind(tenant_id)
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(transition_from_row).collect()
    }
}

// ---------------------------------------------------------------------------
// Small conversions between the port's types and the schema's columns
// ---------------------------------------------------------------------------

/// The `workarea_transitions` columns a transition read selects.
const TRANSITION_COLUMNS: &str = "id, tenant_id, workarea_id, from_status, to_status, state, \
     proposer_key_id, proposer_principal, ratifier_key_id";

/// A `workarea_transitions` row as the port's record; a status or state
/// word the record does not spell fails closed.
fn transition_from_row(row: &PgRow) -> Result<TransitionRecord, StoreError> {
    let transition_id: Uuid = row.try_get("id").map_err(backend)?;
    let status = |column: &str| -> Result<WorkareaStatus, StoreError> {
        let word: String = row.try_get(column).map_err(backend)?;
        WorkareaStatus::parse(&word).ok_or_else(|| {
            StoreError::Backend(format!("transition {transition_id} has {column} {word:?}"))
        })
    };
    let state: String = row.try_get("state").map_err(backend)?;
    Ok(TransitionRecord {
        transition_id,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        workarea_id: row.try_get("workarea_id").map_err(backend)?,
        from: status("from_status")?,
        to: status("to_status")?,
        state: TransitionState::parse(&state).ok_or_else(|| {
            StoreError::Backend(format!("transition {transition_id} has state {state:?}"))
        })?,
        proposer_key_id: row.try_get("proposer_key_id").map_err(backend)?,
        proposer_principal: row.try_get("proposer_principal").map_err(backend)?,
        ratifier_key_id: row.try_get("ratifier_key_id").map_err(backend)?,
    })
}

fn backend<E: std::fmt::Display>(e: E) -> StoreError {
    StoreError::Backend(e.to_string())
}

/// A creation's insert as the store answers it: a refusal by `constraint`,
/// the unique index on the kind's name, is the name the creation lost,
/// [`StoreError::NameTaken`]; the index is the authority `name.taken`
/// reports, a lost race included (postgres-schema.md names and tenants;
/// ADR-0026 d5). Any other failure is the backend's.
fn name_taken_or_backend(
    e: sqlx::Error,
    constraint: &str,
    kind: NamedKind,
    name: &Name,
) -> StoreError {
    match &e {
        sqlx::Error::Database(db) if db.constraint() == Some(constraint) => StoreError::NameTaken {
            kind,
            name: name.clone(),
        },
        _ => backend(e),
    }
}

/// A `name` column as the port's name: the `oml_name` domain checks the
/// reference grammar, so a stored name reads back under it, the derived
/// form included (ADR-0026 d2). NULL on a row that predates names.
fn stored_name(row: &PgRow, column: &str) -> Result<Option<Name>, StoreError> {
    let name: Option<String> = row.try_get(column).map_err(backend)?;
    name.as_deref()
        .map(Name::parse_reference)
        .transpose()
        .map_err(backend)
}

/// Stable advisory-lock key for a tenant's chain head: the first eight bytes
/// of SHA-256 over the tenant uuid bytes, big-endian (ADR-0002 d3 — the lock
/// is per tenant; a cross-tenant collision only over-serializes, never
/// under-serializes).
fn chain_lock_key(tenant_id: Uuid) -> i64 {
    let digest = Sha256Hash::digest(tenant_id.as_bytes()).0;
    let mut first = [0_u8; 8];
    first.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(first)
}

/// `SET LOCAL oml.tenant_id` via `set_config(..., is_local = true)` — the
/// RLS fence of 0001_init.sql, scoped to this transaction. (`SET LOCAL`
/// itself cannot take a bind parameter.)
async fn set_tenant(tx: &mut Transaction<'_, Postgres>, tenant_id: Uuid) -> Result<(), StoreError> {
    sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

/// The 0001 idempotency PK is `(tenant_id, key)`; the port scope's
/// `(key_id, workarea_id, idem_key)` packs into the key column. Uuids
/// contain no `:`, so the packing is injective.
fn scope_key(scope: &IdempotencyScope) -> String {
    format!("{}:{}:{}", scope.key_id, scope.workarea_id, scope.idem_key)
}

/// The port's key hash is `sha256:<hex>`; `api_keys.hash` stores the raw
/// bytes. `None` means the string cannot name any stored row.
fn key_hash_bytes(key_hash: &str) -> Option<Vec<u8>> {
    hex::decode(key_hash.strip_prefix("sha256:")?).ok()
}

fn hash32(bytes: &[u8]) -> Result<Sha256Hash, StoreError> {
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| StoreError::Backend("stored hash is not 32 bytes".to_owned()))?;
    Ok(Sha256Hash(arr))
}

/// The states 0005's CHECK admits (postgres-schema.md `memories`); demote
/// and erase are unbuilt, so no fold writes 'demoted' or 'erased' yet.
fn status_from_str(s: &str) -> Result<MemoryStatus, StoreError> {
    match s {
        "held" => Ok(MemoryStatus::Held),
        "promoted" => Ok(MemoryStatus::Promoted),
        "demoted" => Ok(MemoryStatus::Demoted),
        "rejected" => Ok(MemoryStatus::Rejected),
        "erased" => Ok(MemoryStatus::Erased),
        other => Err(StoreError::Backend(format!(
            "unknown memory status {other:?}"
        ))),
    }
}

/// An integer column as the port's unsigned value; a negative one is not a
/// row this adapter wrote, so it fails closed.
fn unsigned<T: TryFrom<i32>>(row: &PgRow, column: &str) -> Result<T, StoreError> {
    let value: i32 = row.try_get(column).map_err(backend)?;
    T::try_from(value)
        .map_err(|_| StoreError::Backend(format!("stored {column} {value} is negative")))
}

fn role_from_str(s: &str) -> Result<Role, StoreError> {
    match s {
        "reader" => Ok(Role::Reader),
        "writer" => Ok(Role::Writer),
        "dreamer" => Ok(Role::Dreamer),
        "admin" => Ok(Role::Admin),
        "operator" => Ok(Role::Operator),
        other => Err(StoreError::Backend(format!("unknown role {other:?}"))),
    }
}

fn held_by_from_str(s: &str) -> Result<HeldBy, StoreError> {
    match s {
        "agent" => Ok(HeldBy::Agent),
        "human" => Ok(HeldBy::Human),
        "service" => Ok(HeldBy::Service),
        other => Err(StoreError::Backend(format!("unknown held_by {other:?}"))),
    }
}

/// The m-qualified memory column list every memory read shares.
/// `created_at` and `promoted_at` come back through `to_char` in the
/// fixed-width signed form so `EventTime` round-trips exactly
/// (timestamptz(6) holds the microseconds).
const MEMORY_COLUMNS: &str = "m.id, m.tenant_id, m.workarea_id, m.claim_key, m.version, \
     m.kind, m.claim, m.claim_digest, m.body_bytes, m.status, m.is_tip, m.retirement, \
     m.replaces_artifact_id, m.author_principal, m.author_key_id, m.derives_from, \
     m.lineage_principals, m.lineage_key_ids, m.scan_finding, m.scanned_under, \
     m.severity, m.rule, \
     m.valid_from, m.valid_until, m.valid_from_us, m.valid_until_us, \
     m.anchor_repo, m.anchor_uri, m.anchor_rev, m.anchor_path, m.anchor_fragment, \
     m.anchor_hash, m.anchor_hasher, m.source_check_rev, m.source_check_result, \
     m.source_check_hash, m.source_check_path, m.source_check_fragment, m.source_check_at, \
     m.format, m.title, m.search_text, m.search_text_by, m.origin_kind, \
     to_char(m.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS created_at_signed, \
     to_char(m.promoted_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS promoted_at_signed, \
     m.approver_held_by, m.approver_same_user, \
     to_char(m.approved_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS approved_at_signed";

fn memory_from_row(row: &PgRow) -> Result<MemoryRecord, StoreError> {
    let status: String = row.try_get("status").map_err(backend)?;
    let kind: String = row.try_get("kind").map_err(backend)?;
    let claim_digest: Vec<u8> = row.try_get("claim_digest").map_err(backend)?;
    let created: String = row.try_get("created_at_signed").map_err(backend)?;
    let promoted: Option<String> = row.try_get("promoted_at_signed").map_err(backend)?;
    let severity: Option<String> = row.try_get("severity").map_err(backend)?;
    let guardrail = severity
        .map(|word| {
            let severity = Severity::parse(&word)
                .ok_or_else(|| StoreError::Backend(format!("unknown severity {word:?}")))?;
            let rule: Option<Value> = row.try_get("rule").map_err(backend)?;
            Ok::<_, StoreError>(Box::new(GuardrailFields { severity, rule }))
        })
        .transpose()?;
    Ok(MemoryRecord {
        memory_id: row.try_get("id").map_err(backend)?,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        workarea_id: row.try_get("workarea_id").map_err(backend)?,
        claim_key: row.try_get("claim_key").map_err(backend)?,
        version: unsigned(row, "version")?,
        kind: MemoryKind::parse(&kind)
            .ok_or_else(|| StoreError::Backend(format!("unknown memory kind {kind:?}")))?,
        body: row.try_get("claim").map_err(backend)?,
        body_digest: hash32(&claim_digest)?.to_prefixed(),
        body_bytes: unsigned(row, "body_bytes")?,
        status: status_from_str(&status)?,
        is_tip: row.try_get("is_tip").map_err(backend)?,
        promoted_at: promoted
            .as_deref()
            .map(EventTime::parse)
            .transpose()
            .map_err(backend)?,
        retirement: row.try_get("retirement").map_err(backend)?,
        replaces: row.try_get("replaces_artifact_id").map_err(backend)?,
        author_principal: row.try_get("author_principal").map_err(backend)?,
        author_key_id: row.try_get("author_key_id").map_err(backend)?,
        derives_from: row.try_get("derives_from").map_err(backend)?,
        lineage_principals: row.try_get("lineage_principals").map_err(backend)?,
        lineage_key_ids: row.try_get("lineage_key_ids").map_err(backend)?,
        scan_finding: row.try_get("scan_finding").map_err(backend)?,
        scanned_under: row.try_get("scanned_under").map_err(backend)?,
        created_at: EventTime::parse(&created).map_err(backend)?,
        guardrail,
        valid: Box::new(valid_from_row(row)?),
        anchor: anchor_from_row(row)?.map(Box::new),
        origin_kind: origin_kind_from_row(row)?,
        source_check: source_check_from_row(row)?.map(Box::new),
        approval: approval_from_row(row)?.map(Box::new),
        format: Box::new(FormatFields {
            media_type: row.try_get("format").map_err(backend)?,
            title: row.try_get("title").map_err(backend)?,
            search_text: row.try_get("search_text").map_err(backend)?,
            search_text_by: row.try_get("search_text_by").map_err(backend)?,
        }),
    })
}

/// A row's `origin_kind`, `None` on versions written before ADR-0063.
fn origin_kind_from_row(row: &PgRow) -> Result<Option<OriginKind>, StoreError> {
    let kind: Option<String> = row.try_get("origin_kind").map_err(backend)?;
    kind.map(|k| {
        OriginKind::parse(&k)
            .ok_or_else(|| StoreError::Backend(format!("unknown origin kind {k:?}")))
    })
    .transpose()
}

/// A row's declared valid time: each end's signed text and its bound
/// (proposed ADR-0059 d2).
fn valid_from_row(row: &PgRow) -> Result<ValidTime, StoreError> {
    let end = |text: &str, micros: &str| -> Result<Option<ValidEnd>, StoreError> {
        let text: Option<String> = row.try_get(text).map_err(backend)?;
        let micros: Option<i64> = row.try_get(micros).map_err(backend)?;
        Ok(text.zip(micros).map(|(t, m)| ValidEnd::from_stored(t, m)))
    };
    Ok(ValidTime {
        from: end("valid_from", "valid_from_us")?,
        until: end("valid_until", "valid_until_us")?,
    })
}

/// A row's anchor columns, as the author declared them (ADR-0057 d1).
fn anchor_from_row(row: &PgRow) -> Result<Option<Anchor>, StoreError> {
    let text =
        |name: &str| -> Result<Option<String>, StoreError> { row.try_get(name).map_err(backend) };
    let origin = match (text("anchor_repo")?, text("anchor_uri")?) {
        (Some(repo), None) => Origin::Repo {
            repo,
            rev: text("anchor_rev")?
                .ok_or_else(|| StoreError::Backend("a repo anchor without a rev".to_owned()))?,
            path: text("anchor_path")?
                .ok_or_else(|| StoreError::Backend("a repo anchor without a path".to_owned()))?,
        },
        (None, Some(uri)) => Origin::Uri {
            uri,
            rev: text("anchor_rev")?,
        },
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => {
            return Err(StoreError::Backend(
                "an anchor with both repo and uri".to_owned(),
            ));
        }
    };
    let hash = match (text("anchor_hash")?, text("anchor_hasher")?) {
        (Some(hash), Some(hasher)) => Some((hash, hasher)),
        _ => None,
    };
    Ok(Some(Anchor {
        origin,
        fragment: text("anchor_fragment")?,
        hash,
    }))
}

/// A row's latest source check (ADR-0057 d5, d18).
fn source_check_from_row(row: &PgRow) -> Result<Option<SourceCheck>, StoreError> {
    let text =
        |name: &str| -> Result<Option<String>, StoreError> { row.try_get(name).map_err(backend) };
    let Some(result) = text("source_check_result")? else {
        return Ok(None);
    };
    let result = CheckResult::parse(&result)
        .ok_or_else(|| StoreError::Backend(format!("unknown source check {result:?}")))?;
    let at = text("source_check_at")?
        .ok_or_else(|| StoreError::Backend("a source check without its time".to_owned()))?;
    Ok(Some(SourceCheck {
        rev: text("source_check_rev")?
            .ok_or_else(|| StoreError::Backend("a source check without its rev".to_owned()))?,
        result,
        hash: text("source_check_hash")?,
        moved_to: text("source_check_path")?
            .map(|path| Ok::<_, StoreError>((path, text("source_check_fragment")?)))
            .transpose()?,
        at: EventTime::parse(&at).map_err(backend)?,
    }))
}

/// The approving verdict stored on a version's row, `None` where it holds
/// none; a row that holds its time without its attestation fails closed
/// (ADR-0077 d6).
fn approval_from_row(row: &PgRow) -> Result<Option<Approval>, StoreError> {
    let Some(at) = row
        .try_get::<Option<String>, _>("approved_at_signed")
        .map_err(backend)?
    else {
        return Ok(None);
    };
    let held_by: Option<String> = row.try_get("approver_held_by").map_err(backend)?;
    let held_by = held_by
        .ok_or_else(|| StoreError::Backend("an approval without its attestation".to_owned()))?;
    Ok(Some(Approval {
        held_by: held_by_from_str(&held_by)?,
        same_user: row.try_get("approver_same_user").map_err(backend)?,
        at: EventTime::parse(&at).map_err(backend)?,
    }))
}

/// A recall candidate from a row of `id`, `body_bytes` and `score`, and
/// `matched` when the statement selects it.
fn candidate_from_row(row: &PgRow) -> Result<Candidate, StoreError> {
    let matched: Option<Vec<String>> = row.try_get("matched").ok();
    Ok(Candidate {
        memory_id: row.try_get("id").map_err(backend)?,
        body_bytes: unsigned(row, "body_bytes")?,
        score: row.try_get("score").map_err(backend)?,
        matched: matched.unwrap_or_default(),
        tokens: Vec::new(),
    })
}

/// A `sparse` row: its matched pieces, their contributions and whether the
/// text holds each, become `matched` (the pieces the text holds) and
/// `tokens`, largest first (ADR-0052 d17).
fn sparse_candidate_from_row(row: &PgRow) -> Result<Candidate, StoreError> {
    let pieces: Vec<String> = row.try_get("pieces").map_err(backend)?;
    let weights: Vec<f32> = row.try_get("contributions").map_err(backend)?;
    let in_text: Vec<bool> = row.try_get("in_text").map_err(backend)?;
    let tokens: Vec<MatchedToken> = pieces
        .into_iter()
        .zip(weights)
        .zip(in_text)
        .map(|((token, weight), in_text)| MatchedToken {
            token,
            weight,
            expanded: !in_text,
        })
        .collect();
    let mut matched: Vec<String> = tokens
        .iter()
        .filter(|t| !t.expanded)
        .map(|t| t.token.clone())
        .collect();
    matched.sort_unstable();
    matched.dedup();
    Ok(Candidate {
        memory_id: row.try_get("id").map_err(backend)?,
        body_bytes: unsigned(row, "body_bytes")?,
        score: row.try_get("score").map_err(backend)?,
        matched,
        tokens,
    })
}

/// The kinds of `kinds` a recall serves as hits: steering and the
/// vocabulary kinds never are, whatever the filter names (retrieval.md, the
/// recall-ordering contract).
fn hit_kinds(kinds: &[MemoryKind]) -> Vec<&'static str> {
    kinds
        .iter()
        .filter(|kind| HIT_KINDS.contains(kind))
        .map(|kind| kind.as_str())
        .collect()
}

/// The workarea columns every workarea read selects (unqualified: the
/// recursive walks select them from their CTE).
const WORKAREA_COLUMNS: &str =
    "id, tenant_id, parent_id, name, status, crew, crew_id, user_root, namespace_root_id, mode";

/// A workarea row as the port's record. A row with no name yet predates
/// names and is served to no one: the lead names every such row before it
/// serves (ADR-0026 d11), and until that naming is built a lead whose store
/// holds one serves no request ([`PgStore::rows_without_names`]), so one
/// found here fails closed.
fn workarea_from_row(row: &PgRow) -> Result<WorkareaRecord, StoreError> {
    let workarea_id: Uuid = row.try_get("id").map_err(backend)?;
    let crew: Option<Value> = row.try_get("crew").map_err(backend)?;
    Ok(WorkareaRecord {
        workarea_id,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        parent_id: row.try_get("parent_id").map_err(backend)?,
        name: stored_name(row, "name")?.ok_or_else(|| {
            StoreError::Backend(format!(
                "workarea {workarea_id} has no name: the lead names every row that predates \
                 names before it serves (ADR-0026 d11)"
            ))
        })?,
        status: {
            let status: String = row.try_get("status").map_err(backend)?;
            WorkareaStatus::parse(&status).ok_or_else(|| {
                StoreError::Backend(format!("workarea {workarea_id} has status {status:?}"))
            })?
        },
        crew: crew
            .map(serde_json::from_value)
            .transpose()
            .map_err(backend)?,
        crew_id: row.try_get("crew_id").map_err(backend)?,
        user_root: row.try_get("user_root").map_err(backend)?,
        namespace: row.try_get("namespace_root_id").map_err(backend)?,
        in_progress: row.try_get::<String, _>("mode").map_err(backend)? == "in_progress",
    })
}

/// The `api_keys` columns a key read selects. `revoked_at` comes back
/// through `to_char` in the fixed-width signed form, as a memory's
/// `promoted_at` does, so `EventTime` round-trips exactly.
const KEY_COLUMNS: &str = "id, tenant_id, role, principal, held_by, default_workarea_id, \
     issued_by, hash, credential, name, crew_id, user_root_id, session_for, guardrail_scope, \
     profile, to_char(revoked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS revoked_at_signed";

/// A claim edge's columns, `e` the edge and `k` its proposer's key row;
/// `created_at` comes back through `to_char` in the fixed-width signed
/// form, as a memory's does.
const CLAIM_EDGE_COLUMNS: &str = "e.id, e.tenant_id, e.src_artifact_id, e.dst_artifact_id, \
     e.label, e.state, e.proposer_key_id, k.principal AS proposer_principal, \
     e.ratifier_key_id, \
     to_char(e.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS created_at_signed";

/// A claim-edge row as the port's record; a state outside the schema's
/// CHECK is the backend's refusal.
fn claim_edge_from_row(row: &PgRow) -> Result<ClaimEdgeRecord, StoreError> {
    let state: String = row.try_get("state").map_err(backend)?;
    let created: String = row.try_get("created_at_signed").map_err(backend)?;
    Ok(ClaimEdgeRecord {
        edge_id: row.try_get("id").map_err(backend)?,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        src_artifact_id: row.try_get("src_artifact_id").map_err(backend)?,
        dst_artifact_id: row.try_get("dst_artifact_id").map_err(backend)?,
        label: row.try_get("label").map_err(backend)?,
        state: match state.as_str() {
            "pending" => EdgeState::Pending,
            "ratified" => EdgeState::Ratified,
            other => {
                return Err(StoreError::Backend(format!(
                    "unknown claim edge state {other:?}"
                )));
            }
        },
        proposer_key_id: row.try_get("proposer_key_id").map_err(backend)?,
        proposer_principal: row.try_get("proposer_principal").map_err(backend)?,
        ratifier_key_id: row.try_get("ratifier_key_id").map_err(backend)?,
        created_at: EventTime::parse(&created).map_err(backend)?,
    })
}

const BRIEF_EDGE_COLUMNS: &str = "e.id, e.tenant_id, e.artifact_id, e.workarea_id, e.state, \
     e.proposer_key_id, k.principal AS proposer_principal, e.ratifier_key_id, e.ratified_seq, \
     to_char(e.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS created_at_signed";

/// A brief-edge row as the port's record.
fn brief_edge_from_row(row: &PgRow) -> Result<BriefEdgeRecord, StoreError> {
    let state: String = row.try_get("state").map_err(backend)?;
    let created: String = row.try_get("created_at_signed").map_err(backend)?;
    let ratified_seq: Option<i64> = row.try_get("ratified_seq").map_err(backend)?;
    Ok(BriefEdgeRecord {
        edge_id: row.try_get("id").map_err(backend)?,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        artifact_id: row.try_get("artifact_id").map_err(backend)?,
        workarea_id: row.try_get("workarea_id").map_err(backend)?,
        state: match state.as_str() {
            "pending" => EdgeState::Pending,
            "ratified" => EdgeState::Ratified,
            other => {
                return Err(StoreError::Backend(format!(
                    "unknown brief edge state {other:?}"
                )));
            }
        },
        proposer_key_id: row.try_get("proposer_key_id").map_err(backend)?,
        proposer_principal: row.try_get("proposer_principal").map_err(backend)?,
        ratifier_key_id: row.try_get("ratifier_key_id").map_err(backend)?,
        created_at: EventTime::parse(&created).map_err(backend)?,
        ratified_seq: ratified_seq
            .map(|s| u64::try_from(s).map_err(backend))
            .transpose()?,
    })
}

/// A key row as the port's record. A provider-level row names no tenant
/// (postgres-schema.md api_keys): it resolves under the provider tenant, so
/// its refusals are the use-cases' own, never a storage failure (ADR-0025
/// d13).
fn key_from_row(row: &PgRow) -> Result<KeyRecord, StoreError> {
    let tenant_id: Option<Uuid> = row.try_get("tenant_id").map_err(backend)?;
    let role: String = row.try_get("role").map_err(backend)?;
    let held_by: String = row.try_get("held_by").map_err(backend)?;
    let stored_hash: Option<Vec<u8>> = row.try_get("hash").map_err(backend)?;
    let credential: String = row.try_get("credential").map_err(backend)?;
    let revoked: Option<String> = row.try_get("revoked_at_signed").map_err(backend)?;
    let scope: String = row.try_get("guardrail_scope").map_err(backend)?;
    Ok(KeyRecord {
        key_id: row.try_get("id").map_err(backend)?,
        tenant_id: tenant_id.unwrap_or(PROVIDER_TENANT),
        role: role_from_str(&role)?,
        principal: row.try_get("principal").map_err(backend)?,
        held_by: held_by_from_str(&held_by)?,
        default_workarea: row.try_get("default_workarea_id").map_err(backend)?,
        issued_by: row.try_get("issued_by").map_err(backend)?,
        credential: Credential::parse(&credential).ok_or_else(|| {
            StoreError::Backend(format!(
                "api_keys.credential {credential:?} is not a credential"
            ))
        })?,
        key_hash: stored_hash.map(|h| format!("sha256:{}", hex::encode(h))),
        name: stored_name(row, "name")?,
        crew_id: row.try_get("crew_id").map_err(backend)?,
        user: row.try_get("user_root_id").map_err(backend)?,
        revoked_at: revoked
            .as_deref()
            .map(EventTime::parse)
            .transpose()
            .map_err(backend)?,
        session_user: stored_name(row, "session_for")?,
        guardrail_scope: GuardrailScope::parse(&scope)
            .ok_or_else(|| StoreError::Backend(format!("unknown guardrail_scope {scope:?}")))?,
        profile: row
            .try_get::<Option<String>, _>("profile")
            .map_err(backend)?
            .map(|p| {
                KeyProfile::parse(&p)
                    .ok_or_else(|| StoreError::Backend(format!("unknown key profile {p:?}")))
            })
            .transpose()?,
    })
}

/// A crew row as the port's record; every crew record is created named
/// (ADR-0026 d8, d11).
fn crew_from_row(row: &PgRow) -> Result<CrewRecord, StoreError> {
    let crew_id: Uuid = row.try_get("id").map_err(backend)?;
    Ok(CrewRecord {
        crew_id,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        name: stored_name(row, "name")?
            .ok_or_else(|| StoreError::Backend(format!("crew {crew_id} has no name")))?,
        charter_uri: row.try_get("charter_uri").map_err(backend)?,
        external_manager: row.try_get("external_manager").map_err(backend)?,
    })
}

/// A grant's deny list as `grants.deny` stores it: the `grant.issued`
/// spelling, `[{"subtree_node_id": …}]`, in the grant's order.
fn deny_to_json(deny: &[Uuid]) -> Value {
    Value::Array(
        deny.iter()
            .map(|node| json!({ "subtree_node_id": node.to_string() }))
            .collect(),
    )
}

fn deny_from_json(deny: &Value) -> Result<Vec<Uuid>, StoreError> {
    let malformed = || StoreError::Backend(format!("stored grant deny list is malformed: {deny}"));
    deny.as_array()
        .ok_or_else(malformed)?
        .iter()
        .map(|entry| {
            let node = entry
                .get("subtree_node_id")
                .and_then(Value::as_str)
                .ok_or_else(malformed)?;
            Uuid::parse_str(node).map_err(backend)
        })
        .collect()
}

fn seq_from_column(row: &PgRow, column: &str) -> Result<u64, StoreError> {
    let seq: i64 = row.try_get(column).map_err(backend)?;
    u64::try_from(seq).map_err(backend)
}

fn json_str<'a>(v: &'a Value, name: &str) -> Result<&'a str, StoreError> {
    v.get(name).and_then(Value::as_str).ok_or_else(|| {
        StoreError::Backend(format!("stored signed_bytes field {name} is not a string"))
    })
}

/// Rebuilds the served envelope by parsing the STORED canonical bytes — the
/// stored bytes are authoritative (chain-format.md), so verification steps 6
/// and 7 hold by construction instead of trusting a jsonb round-trip.
fn envelope_from_signed_bytes(signed_bytes: &[u8]) -> Result<Envelope, StoreError> {
    let v: Value = serde_json::from_slice(signed_bytes).map_err(backend)?;
    let tenant_id = Uuid::parse_str(json_str(&v, "tenant_id")?).map_err(backend)?;
    let seq = v
        .get("seq")
        .and_then(Value::as_u64)
        .ok_or_else(|| StoreError::Backend("stored signed_bytes seq is not a u64".to_owned()))?;
    let event_id = Uuid::parse_str(json_str(&v, "event_id")?).map_err(backend)?;
    let event_type = json_str(&v, "event_type")?.to_owned();
    let prev_hash = Sha256Hash::from_prefixed(json_str(&v, "prev_hash")?).map_err(backend)?;
    let principal = json_str(&v, "principal")?.to_owned();
    let api_key_id = match v.get("api_key_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Uuid::parse_str(s).map_err(backend)?),
        Some(other) => {
            return Err(StoreError::Backend(format!(
                "stored api_key_id is neither null nor a string: {other}"
            )));
        }
    };
    let at = EventTime::parse(json_str(&v, "at")?).map_err(backend)?;
    let payload = v
        .get("payload")
        .cloned()
        .ok_or_else(|| StoreError::Backend("stored signed_bytes has no payload".to_owned()))?;
    Ok(Envelope {
        tenant_id,
        seq,
        event_id,
        event_type,
        prev_hash,
        principal,
        api_key_id,
        at,
        payload,
    })
}

fn signed_envelope_from_row(row: &PgRow) -> Result<SignedEnvelope, StoreError> {
    let signed_bytes: Vec<u8> = row.try_get("signed_bytes").map_err(backend)?;
    let hash: Vec<u8> = row.try_get("signed_bytes_hash").map_err(backend)?;
    let sig: Vec<u8> = row.try_get("sig").map_err(backend)?;
    let sig_arr: [u8; 64] = sig
        .as_slice()
        .try_into()
        .map_err(|_| StoreError::Backend("stored sig is not 64 bytes".to_owned()))?;
    let envelope = envelope_from_signed_bytes(&signed_bytes)?;
    Ok(SignedEnvelope {
        envelope,
        signed_bytes,
        hash: hash32(&hash)?,
        sig: Signature::from_bytes(&sig_arr),
    })
}

// ---------------------------------------------------------------------------
// The metadata fold inside the commit transaction
// ---------------------------------------------------------------------------

/// The findings lists of `lists` under the commit's `seq`, each in its
/// order (ADR-0074 d23). A finding the version's list already holds by
/// class, rule and field is kept as first recorded (`scan_findings_one`).
/// A finding migration 0043 could not place, with no field, stands for one
/// place of its class and rule: the first of `list`'s finds of them, in
/// order, is taken as that place and not added, and the rest are, so a
/// finding is never listed twice and a second place is never lost.
async fn insert_findings(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    seq: i64,
    lists: &[(Uuid, Vec<VersionFinding>)],
) -> Result<(), StoreError> {
    let rows: Vec<(Uuid, i32, &VersionFinding)> = lists
        .iter()
        .flat_map(|(id, list)| list.iter().zip(0_i32..).map(move |(f, ord)| (*id, ord, f)))
        .collect();
    if rows.is_empty() {
        return Ok(());
    }
    let int = |v: Option<u64>| v.map(i64::try_from).transpose().map_err(backend);
    let mut ids = Vec::with_capacity(rows.len());
    let mut ords = Vec::with_capacity(rows.len());
    let mut classes = Vec::with_capacity(rows.len());
    let mut rules = Vec::with_capacity(rows.len());
    let mut verdicts = Vec::with_capacity(rows.len());
    let mut rule_sets = Vec::with_capacity(rows.len());
    let mut fields = Vec::with_capacity(rows.len());
    let mut lines = Vec::with_capacity(rows.len());
    let mut vias = Vec::with_capacity(rows.len());
    let mut starts = Vec::with_capacity(rows.len());
    let mut ends = Vec::with_capacity(rows.len());
    let mut found_by = Vec::with_capacity(rows.len());
    let mut migrated = Vec::with_capacity(rows.len());
    for (id, ord, f) in rows {
        ids.push(id);
        ords.push(ord);
        classes.push(f.class.clone());
        rules.push(f.rule.clone());
        verdicts.push(f.verdict.as_str());
        rule_sets.push(f.rule_set.clone());
        fields.push(f.field.clone());
        lines.push(f.line.map(i32::try_from).transpose().map_err(backend)?);
        vias.push(f.via.clone());
        starts.push(int(f.span.map(|s| s.0))?);
        ends.push(int(f.span.map(|s| s.1))?);
        found_by.push(f.found_by.as_str());
        migrated.push(f.migrated_from.clone());
    }
    sqlx::query(
        "INSERT INTO scan_findings (tenant_id, memory_id, event_seq, ord, class, rule, verdict, \
         rule_set, field, line, via, span_start, span_end, found_by, migrated_from) \
         SELECT $1, u.id, $2, u.ord, u.class, u.rule, u.verdict, u.rule_set, u.field, u.line, \
                u.via, u.span_start, u.span_end, u.found_by, u.migrated_from \
         FROM (SELECT u.*, row_number() OVER (PARTITION BY u.id, u.class, u.rule \
                                              ORDER BY u.ord) AS k \
               FROM unnest($3::uuid[], $4::int4[], $5::text[], $6::text[], $7::text[], \
                           $8::text[], $9::text[], $10::int4[], $11::text[], $12::int8[], \
                           $13::int8[], $14::text[], $15::text[]) \
                 AS u(id, ord, class, rule, verdict, rule_set, field, line, via, span_start, \
                      span_end, found_by, migrated_from)) u \
         WHERE u.k > (SELECT count(*) FROM scan_findings f \
                       WHERE f.tenant_id = $1 AND f.memory_id = u.id \
                         AND f.class = u.class AND f.rule = u.rule AND f.field IS NULL) \
         ON CONFLICT ON CONSTRAINT scan_findings_one DO NOTHING",
    )
    .bind(tenant_id)
    .bind(seq)
    .bind(ids)
    .bind(ords)
    .bind(classes)
    .bind(rules)
    .bind(verdicts)
    .bind(rule_sets)
    .bind(fields)
    .bind(lines)
    .bind(vias)
    .bind(starts)
    .bind(ends)
    .bind(found_by)
    .bind(migrated)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(())
}

async fn insert_memory(
    tx: &mut Transaction<'_, Postgres>,
    created_event_id: Uuid,
    m: &MemoryRecord,
) -> Result<(), StoreError> {
    // claim_digest stores the raw 32 bytes of the record's prefixed digest
    // string; reads reconstruct the `sha256:<hex>` spelling.
    let digest = Sha256Hash::from_prefixed(&m.body_digest).map_err(backend)?;
    // Integer columns hold the ordinal and the byte count; a value they
    // cannot hold is refused, never truncated.
    let version = i32::try_from(m.version).map_err(backend)?;
    let body_bytes = i32::try_from(m.body_bytes).map_err(backend)?;
    let promoted_at = m.promoted_at.as_ref().map(EventTime::as_str);
    let anchor = m.anchor.as_deref();
    // An anchored row names its anchor columns in the same INSERT; an
    // unanchored one names none, so a schema before 0025 still takes it
    // (ADR-0057 d2).
    let (columns, values) = if anchor.is_some() {
        (
            ", anchor_repo, anchor_uri, anchor_rev, anchor_path, anchor_fragment, \
             anchor_hash, anchor_hasher",
            ", $25, $26, $27, $28, $29, $30, $31",
        )
    } else {
        ("", "")
    };
    // A version with a kind names `origin_kind` after the anchor's columns
    // (ADR-0063 d2); one without, folded from an event before it, names
    // none.
    let origin_n = if anchor.is_some() { 32 } else { 25 };
    let (origin_column, origin_value) = if m.origin_kind.is_some() {
        (", origin_kind".to_owned(), format!(", ${origin_n}"))
    } else {
        (String::new(), String::new())
    };
    // A version a versioned scanner read names `scanned_under` last
    // (ADR-0065 d15); one without, as a test double's, names none.
    let scanner_n = origin_n + usize::from(m.origin_kind.is_some());
    let (scanner_column, scanner_value) = if m.scanned_under.is_some() {
        (", scanned_under".to_owned(), format!(", ${scanner_n}"))
    } else {
        (String::new(), String::new())
    };
    let sql = format!(
        "INSERT INTO memories (id, tenant_id, workarea_id, claim, claim_digest, status, \
         created_event_id, created_at, claim_key, author_principal, \
         author_key_id, derives_from, lineage_principals, lineage_key_ids, scan_finding, \
         version, kind, body_bytes, is_tip, promoted_at, retirement, replaces_artifact_id, \
         severity, rule{columns}{origin_column}{scanner_column}) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, ($8)::timestamptz, $9, $10, $11, $12, $13, $14, \
         $15, $16, $17, $18, $19, ($20)::timestamptz, $21, $22, $23, $24{values}{origin_value}\
         {scanner_value})"
    );
    let mut insert = sqlx::query(&sql)
        .bind(m.memory_id)
        .bind(m.tenant_id)
        .bind(m.workarea_id)
        .bind(&m.body)
        .bind(digest.0.to_vec())
        .bind(m.status.as_str())
        .bind(created_event_id)
        .bind(m.created_at.as_str())
        .bind(&m.claim_key)
        .bind(&m.author_principal)
        .bind(m.author_key_id)
        .bind(m.derives_from.clone())
        .bind(m.lineage_principals.clone())
        .bind(m.lineage_key_ids.clone())
        .bind(m.scan_finding.as_deref())
        .bind(version)
        .bind(m.kind.as_str())
        .bind(body_bytes)
        .bind(m.is_tip)
        .bind(promoted_at)
        .bind(m.retirement)
        .bind(m.replaces)
        .bind(m.guardrail.as_ref().map(|g| g.severity.as_str()))
        .bind(m.guardrail.as_ref().and_then(|g| g.rule.clone()));
    if let Some(anchor) = anchor {
        insert = insert
            .bind(anchor.repo())
            .bind(match &anchor.origin {
                Origin::Uri { uri, .. } => Some(uri.as_str()),
                Origin::Repo { .. } => None,
            })
            .bind(match &anchor.origin {
                Origin::Repo { rev, .. } => Some(rev.as_str()),
                Origin::Uri { rev, .. } => rev.as_deref(),
            })
            .bind(anchor.path())
            .bind(anchor.fragment.as_deref())
            .bind(anchor.hash.as_ref().map(|(h, _)| h.as_str()))
            .bind(anchor.hash.as_ref().map(|(_, h)| h.as_str()));
    }
    if let Some(kind) = m.origin_kind {
        insert = insert.bind(kind.as_str());
    }
    if let Some(scanner) = m.scanned_under.as_deref() {
        insert = insert.bind(scanner);
    }
    insert.execute(&mut **tx).await.map_err(backend)?;
    // The format, the title and the derived search text in the same
    // transaction, only when the version carries any, so a schema before 0032
    // still takes every prose row without one; the lexemes and the postings
    // follow the update (ADR-0056 d4).
    if *m.format != FormatFields::default() {
        sqlx::query(
            "UPDATE memories SET format = $3, title = $4, search_text = $5, search_text_by = $6 \
             WHERE tenant_id = $1 AND id = $2",
        )
        .bind(m.tenant_id)
        .bind(m.memory_id)
        .bind(&m.format.media_type)
        .bind(m.format.title.as_deref())
        .bind(m.format.search_text.as_deref())
        .bind(m.format.search_text_by.as_deref())
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
    // Declared valid time in the same transaction, only when declared, so a
    // schema before 0026 still takes every row that declares none (proposed
    // ADR-0059 d11).
    if !m.valid.is_open() {
        sqlx::query(
            "UPDATE memories SET valid_from = $3, valid_until = $4, valid_from_us = $5, \
             valid_until_us = $6 WHERE tenant_id = $1 AND id = $2",
        )
        .bind(m.tenant_id)
        .bind(m.memory_id)
        .bind(m.valid.from.as_ref().map(ValidEnd::as_str))
        .bind(m.valid.until.as_ref().map(ValidEnd::as_str))
        .bind(m.valid.from.as_ref().map(ValidEnd::unix_micros))
        .bind(m.valid.until.as_ref().map(ValidEnd::unix_micros))
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
    // A version stored already promoted keeps the approval it carries, as
    // its promotion's fold would have written it (ADR-0077 d6).
    if let Some(approval) = m.approval.as_deref() {
        sqlx::query(
            "UPDATE memories SET approver_held_by = $3, approver_same_user = $4, \
             approved_at = ($5)::timestamptz WHERE tenant_id = $1 AND id = $2",
        )
        .bind(m.tenant_id)
        .bind(m.memory_id)
        .bind(approval.held_by.as_str())
        .bind(approval.same_user)
        .bind(approval.at.as_str())
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
    Ok(())
}

async fn enqueue_review(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    memory_id: Uuid,
) -> Result<(), StoreError> {
    sqlx::query("INSERT INTO review_queue (memory_id, tenant_id) VALUES ($1, $2)")
        .bind(memory_id)
        .bind(tenant_id)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

async fn set_memory_status(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    memory_id: Uuid,
    status: MemoryStatus,
) -> Result<(), StoreError> {
    sqlx::query("UPDATE memories SET status = $3 WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id)
        .bind(memory_id)
        .bind(status.as_str())
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    sqlx::query("DELETE FROM review_queue WHERE tenant_id = $1 AND memory_id = $2")
        .bind(tenant_id)
        .bind(memory_id)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

/// Whether `memories` has the approval columns of migration 0044. Read only
/// on a promotion that carries no approval, which the application never
/// sends.
async fn schema_keeps_approval(tx: &mut Transaction<'_, Postgres>) -> Result<bool, StoreError> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_attribute \
         WHERE attrelid = 'memories'::regclass AND attname = 'approved_at' \
           AND NOT attisdropped)",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(backend)
}

/// Ratification of a first version: it is promoted, made the tip and
/// stamped with the event's `at`; `memories_served_tip` refuses a second tip
/// of its claim (ADR-0025 d10).
async fn promote_memory(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    memory_id: Uuid,
    at: &EventTime,
    approval: Option<&Approval>,
) -> Result<(), StoreError> {
    // The row keeps the approving verdict that ratified it, in the one
    // statement that promotes it. A promotion that carries none names no
    // approval column, so a schema before 0044 still takes it; a schema that
    // keeps the column refuses it, and the commit rolls back (ADR-0077 d6).
    if approval.is_none() && schema_keeps_approval(tx).await? {
        return Err(StoreError::ApprovalMissing { memory_id });
    }
    let promote = match approval {
        Some(_) => {
            "UPDATE memories SET status = 'promoted', is_tip = true, \
             promoted_at = ($3)::timestamptz, approver_held_by = $4, \
             approver_same_user = $5, approved_at = ($6)::timestamptz \
             WHERE tenant_id = $1 AND id = $2"
        }
        None => {
            "UPDATE memories SET status = 'promoted', is_tip = true, \
             promoted_at = ($3)::timestamptz WHERE tenant_id = $1 AND id = $2"
        }
    };
    let mut promote = sqlx::query(promote)
        .bind(tenant_id)
        .bind(memory_id)
        .bind(at.as_str());
    if let Some(approval) = approval {
        promote = promote
            .bind(approval.held_by.as_str())
            .bind(approval.same_user)
            .bind(approval.at.as_str());
    }
    promote.execute(&mut **tx).await.map_err(backend)?;
    sqlx::query("DELETE FROM review_queue WHERE tenant_id = $1 AND memory_id = $2")
        .bind(tenant_id)
        .bind(memory_id)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

/// Ratification of a supersession: the compare-and-set takes the tip off
/// `replaced` only while it is the promoted tip under the held successor's
/// own workarea and `claim_key` and the successor replaces it, reading the
/// successor's claim identity in the same statement; a zero row count
/// refuses the commit, whose transaction then rolls back, journal row
/// included. Only then is the successor promoted and made the tip
/// (audit-and-lineage.md supersession chains; ADR-0025 d10).
async fn supersede_tip(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    successor: Uuid,
    replaced: Uuid,
    replaced_valid_until: Option<&ValidEnd>,
    at: &EventTime,
    approval: Option<&Approval>,
) -> Result<(), StoreError> {
    // The ratification's end for the replaced version, when it gives one
    // (proposed ADR-0059 d4); null leaves the end as it was.
    let flipped = sqlx::query(
        "UPDATE memories t SET is_tip = false FROM memories s \
         WHERE s.tenant_id = $1 AND s.id = $2 AND s.status = 'held' \
           AND s.replaces_artifact_id = $3 \
           AND t.tenant_id = $1 AND t.id = $3 \
           AND t.workarea_id = s.workarea_id AND t.claim_key = s.claim_key \
           AND t.is_tip AND t.status = 'promoted'",
    )
    .bind(tenant_id)
    .bind(successor)
    .bind(replaced)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    if flipped.rows_affected() != 1 {
        return Err(StoreError::TipMoved { replaced });
    }
    if let Some(end) = replaced_valid_until {
        sqlx::query(
            "UPDATE memories SET valid_until = $3, valid_until_us = $4 \
             WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant_id)
        .bind(replaced)
        .bind(end.as_str())
        .bind(end.unix_micros())
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
    promote_memory(tx, tenant_id, successor, at, approval).await
}

/// The fold of `claim_edge.proposed`: the edge row, then the lineage
/// widening, [`WIDEN_LINEAGE`], in the same transaction, so no edge is
/// stored that the review queue's exclusion would not see (ADR-0001 d2).
/// An edge that is not pending with no ratifier, or whose `created_at` is
/// not its event's `at`, is refused before any write. An id the store
/// holds, another tenant's row (row security's WITH CHECK), and an endpoint
/// or proposer key the tenant does not hold (23503 from the tenant-keyed
/// foreign keys) are the backend's refusals, as any fold's are (ADR-0034
/// d12); the application has read both endpoints, so the last does not
/// occur.
async fn propose_claim_edge(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    edge: &ClaimEdgeRecord,
    at: &EventTime,
) -> Result<(), StoreError> {
    if edge.state != EdgeState::Pending || edge.ratifier_key_id.is_some() {
        return Err(StoreError::Backend(format!(
            "claim edge {} is proposed pending, with no ratifier",
            edge.edge_id
        )));
    }
    if edge.created_at != *at {
        return Err(StoreError::Backend(format!(
            "claim edge {} is created at its claim_edge.proposed event's at",
            edge.edge_id
        )));
    }
    sqlx::query(
        "INSERT INTO claim_edges (id, tenant_id, src_artifact_id, dst_artifact_id, label, \
         state, proposer_key_id, ratifier_key_id, created_at) \
         VALUES ($1, $2, $3, $4, $5, 'pending', $6, NULL, ($7)::timestamptz)",
    )
    .bind(edge.edge_id)
    .bind(edge.tenant_id)
    .bind(edge.src_artifact_id)
    .bind(edge.dst_artifact_id)
    .bind(&edge.label)
    .bind(edge.proposer_key_id)
    .bind(at.as_str())
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    // A custom plan prunes the UPDATE's targets to the tenant's partition
    // at planning on every PostgreSQL version; a generic one, which the
    // pool's prepared statement turns to after five runs, still names every
    // partition as a target on PostgreSQL 17, opening them though it scans
    // none (ADR-0034 d11).
    sqlx::query("SET LOCAL plan_cache_mode = force_custom_plan")
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    sqlx::query(WIDEN_LINEAGE)
        .bind(tenant_id)
        .bind(edge.src_artifact_id)
        .bind(edge.dst_artifact_id)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    sqlx::query("SET LOCAL plan_cache_mode = DEFAULT")
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

/// The fold of a claim-edge `review.ratify`: the compare-and-set turns a
/// pending edge of this tenant ratified by `ratifier_key_id`; a zero row
/// count refuses the commit with [`StoreError::EdgeNotPending`], and its
/// transaction rolls back, journal row included, as [`supersede_tip`]'s
/// does. A ratifier equal to the proposer (`claim_edges_ratifier_is_not_proposer`)
/// and a key the tenant does not hold (23503) are the backend's refusals
/// (postgres-schema.md `claim_edges`; ADR-0021 d5; ADR-0012).
async fn ratify_claim_edge(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    edge_id: Uuid,
    ratifier_key_id: Uuid,
) -> Result<(), StoreError> {
    let ratified = sqlx::query(
        "UPDATE claim_edges SET state = 'ratified', ratifier_key_id = $3 \
         WHERE tenant_id = $1 AND id = $2 AND state = 'pending'",
    )
    .bind(tenant_id)
    .bind(edge_id)
    .bind(ratifier_key_id)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    if ratified.rows_affected() != 1 {
        return Err(StoreError::EdgeNotPending { edge_id });
    }
    Ok(())
}

/// The fold of `brief_edge.proposed`: the pending row, created at its
/// event's `at`; the keys refuse a claim, workarea or proposer of another
/// tenant (postgres-schema.md `brief_edges`; ADR-0046 d7).
async fn propose_brief_edge(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    edge: &BriefEdgeRecord,
    at: &EventTime,
) -> Result<(), StoreError> {
    if edge.state != EdgeState::Pending
        || edge.ratifier_key_id.is_some()
        || edge.ratified_seq.is_some()
        || edge.created_at != *at
    {
        return Err(StoreError::Backend(format!(
            "brief edge {} is proposed pending at its event's at",
            edge.edge_id
        )));
    }
    sqlx::query(
        "INSERT INTO brief_edges (id, tenant_id, artifact_id, workarea_id, state, \
         proposer_key_id, ratifier_key_id, ratified_seq, created_at) \
         VALUES ($1, $2, $3, $4, 'pending', $5, NULL, NULL, ($6)::timestamptz)",
    )
    .bind(edge.edge_id)
    .bind(tenant_id)
    .bind(edge.artifact_id)
    .bind(edge.workarea_id)
    .bind(edge.proposer_key_id)
    .bind(at.as_str())
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(())
}

/// The fold of a brief edge's `review.ratify`: the compare-and-set turns a
/// pending edge of this tenant ratified at `seq`; a zero row count refuses
/// the commit with [`StoreError::EdgeNotPending`] (ADR-0046 d8).
async fn ratify_brief_edge(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    edge_id: Uuid,
    ratifier_key_id: Uuid,
    seq: i64,
) -> Result<(), StoreError> {
    let ratified = sqlx::query(
        "UPDATE brief_edges SET state = 'ratified', ratifier_key_id = $3, ratified_seq = $4 \
         WHERE tenant_id = $1 AND id = $2 AND state = 'pending'",
    )
    .bind(tenant_id)
    .bind(edge_id)
    .bind(ratifier_key_id)
    .bind(seq)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    if ratified.rows_affected() != 1 {
        return Err(StoreError::EdgeNotPending { edge_id });
    }
    Ok(())
}

/// The fold of `system.names_assigned` into the columns 0006 added: each
/// crew record inserted, each listed row — the tenant, its workareas, its
/// keys — named where its `name` is still NULL, each listed workarea
/// pointed at its crew record, and every workarea path that is still NULL
/// derived from the names, root first. A listed row that already has a
/// name, or that the tenant does not hold, refuses the commit; a name its
/// kind already holds is [`StoreError::NameTaken`] (postgres-schema.md
/// folds; ADR-0026 d11).
async fn fold_names_assigned(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    at: &EventTime,
    naming: &Naming,
) -> Result<(), StoreError> {
    for crew in &naming.crews {
        sqlx::query(
            "INSERT INTO crews (id, tenant_id, name, charter_uri, external_manager, \
             created_at) VALUES ($1, $2, $3, $4, $5, ($6)::timestamptz)",
        )
        .bind(crew.crew_id)
        .bind(tenant_id)
        .bind(crew.name.as_str())
        .bind(crew.charter_uri.as_deref())
        .bind(crew.external_manager.as_deref())
        .bind(at.as_str())
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            name_taken_or_backend(e, "crews_tenant_id_name", NamedKind::Crew, &crew.name)
        })?;
    }
    for assignment in &naming.names {
        let (sql, constraint) = match assignment.kind {
            NamedKind::Tenant if assignment.id == tenant_id => (
                "UPDATE tenants SET name = $3 WHERE id = $1 AND id = $2 AND name IS NULL",
                "tenants_name",
            ),
            // A non-root `universe` meets the CHECK that ties the name to
            // the root; the naming never assigns one (ADR-0026 d6).
            NamedKind::Workarea => (
                "UPDATE workareas SET name = $3 WHERE tenant_id = $1 AND id = $2 \
                 AND name IS NULL",
                if assignment.name.as_str() == UNIVERSE {
                    "workareas_universe_is_root"
                } else {
                    // Every row that predates names is shared (ADR-0044 d12).
                    "workareas_shared_name"
                },
            ),
            NamedKind::Key => (
                "UPDATE api_keys SET name = $3 WHERE tenant_id = $1 AND id = $2 \
                 AND name IS NULL",
                "api_keys_tenant_id_name",
            ),
            kind => {
                return Err(StoreError::Backend(format!(
                    "{} {} is not a row of this tenant's chain to name",
                    kind.as_str(),
                    assignment.id
                )));
            }
        };
        let named = sqlx::query(sql)
            .bind(tenant_id)
            .bind(assignment.id)
            .bind(assignment.name.as_str())
            .execute(&mut **tx)
            .await
            .map_err(|e| name_taken_or_backend(e, constraint, assignment.kind, &assignment.name))?;
        if named.rows_affected() != 1 {
            return Err(StoreError::Backend(format!(
                "{} {} is not a row without a name",
                assignment.kind.as_str(),
                assignment.id
            )));
        }
    }
    for crew in &naming.crews {
        let pointed =
            sqlx::query("UPDATE workareas SET crew_id = $3 WHERE tenant_id = $1 AND id = ANY($2)")
                .bind(tenant_id)
                .bind(&crew.workarea_ids)
                .bind(crew.crew_id)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
        if pointed.rows_affected() != u64::try_from(crew.workarea_ids.len()).map_err(backend)? {
            return Err(StoreError::Backend(format!(
                "crew {} names a workarea this tenant does not hold",
                crew.crew_id
            )));
        }
    }
    // Each path is its parent's with the node's name appended, the root's
    // its name alone (ADR-0026 d7).
    sqlx::query(
        "WITH RECURSIVE tree (id, path) AS ( \
             SELECT id, text2ltree(name) FROM workareas \
              WHERE tenant_id = $1 AND parent_id IS NULL AND name IS NOT NULL \
             UNION ALL \
             SELECT w.id, tree.path || text2ltree(w.name) FROM workareas w \
               JOIN tree ON w.parent_id = tree.id \
              WHERE w.tenant_id = $1 AND w.name IS NOT NULL \
         ) UPDATE workareas w SET path = tree.path FROM tree \
            WHERE w.tenant_id = $1 AND w.id = tree.id AND w.path IS NULL",
    )
    .bind(tenant_id)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(())
}

/// Applies the metadata fold rows of one commit (ADR-0002 d1: same
/// transaction as the journal insert; `memories.created_event_id` references
/// the journal row, so this runs after it). `seq` and `at` are the commit's
/// event seq and time: a key's `issued_seq` and `revoked_at`, a grant's or
/// denial's epoch and a memory's `promoted_at`.
async fn apply_fold(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    event_id: Uuid,
    seq: i64,
    at: &EventTime,
    fold: MetadataFold,
) -> Result<(), StoreError> {
    match fold {
        // The tenants row is upserted before the journal insert (FK order);
        // a verdict short of the policy's k is journalled, metadata unchanged.
        MetadataFold::TenantGenesis { .. }
        | MetadataFold::VerdictRecorded { .. }
        | MetadataFold::AdminRead => {}
        MetadataFold::FederationPolicySet(policy) => {
            sqlx::query(
                "UPDATE tenants SET federation_enabled = $2, federation_default_sources = $3, \
                 federation_max_sources = $4 WHERE id = $1",
            )
            .bind(tenant_id)
            .bind(policy.enabled)
            .bind(policy.default_sources.as_str())
            .bind(i32::try_from(policy.max_sources).map_err(backend)?)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::WorkareaFederationSet {
            workarea_id,
            accept_federated,
        } => {
            let updated = sqlx::query(
                "UPDATE workareas SET accept_federated = $3 WHERE tenant_id = $1 AND id = $2",
            )
            .bind(tenant_id)
            .bind(workarea_id)
            .bind(accept_federated)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "workarea {workarea_id} is not held by this tenant"
                )));
            }
        }
        MetadataFold::SearchDefaultSet { pipeline } => {
            sqlx::query("UPDATE tenants SET search_default = $2 WHERE id = $1")
                .bind(tenant_id)
                .bind(pipeline)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
        }
        // The opt-in's instant, or off; off nulls the text the tenant's
        // events hold, in this commit (ADR-0054 d3, as accepted).
        MetadataFold::TelemetrySettingsSet { query_text_since } => {
            sqlx::query(
                "UPDATE tenants SET telemetry_query_text_since = $2::timestamptz WHERE id = $1",
            )
            .bind(tenant_id)
            .bind(query_text_since.as_ref().map(EventTime::as_str))
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if query_text_since.is_none() {
                sqlx::query(
                    "UPDATE recall_events SET query_text = NULL \
                     WHERE tenant_id = $1 AND query_text IS NOT NULL",
                )
                .bind(tenant_id)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            }
        }
        MetadataFold::WorkareaCreated(w) => {
            // The status stored is the record's, the one column every
            // workarea read serves (ADR-0009 d1). The path is the parent's
            // with this node's name appended, the root's its name alone
            // (ADR-0026 d7); a parent the lead has not named yet has no
            // path, and neither does its child until the naming fold. A
            // parent the tenant does not hold, another tenant's node
            // included, meets the key (tenant_id, parent_id) and is the
            // backend's refusal, as any 23503 is (ADR-0034 d12).
            let crew = w
                .crew
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .map_err(backend)?;
            sqlx::query(
                "INSERT INTO workareas (id, tenant_id, parent_id, name, path, status, crew, \
                 crew_id, user_root, namespace_root_id) VALUES ($1, $2, $3, $4, \
                 CASE WHEN $3::uuid IS NULL THEN text2ltree($4) \
                      ELSE (SELECT p.path FROM workareas p \
                             WHERE p.tenant_id = $2 AND p.id = $3) || text2ltree($4) END, \
                 $5, $6, $7, $8, $9)",
            )
            .bind(w.workarea_id)
            .bind(w.tenant_id)
            .bind(w.parent_id)
            .bind(w.name.as_str())
            .bind(w.status.as_str())
            .bind(crew)
            .bind(w.crew_id)
            .bind(w.user_root)
            .bind(w.namespace)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                // A non-root `universe` meets the CHECK that ties the name
                // to the root before the unique index, and errors.md
                // answers it as `name.taken`; a root of another name lost
                // no name, so its refusal stays the backend's
                // (postgres-schema.md `workareas`; ADR-0026 d5, d6). A name
                // is unique in its namespace, the shared one or its user
                // root's (ADR-0044 d5).
                let constraint = if w.parent_id.is_some() && w.name.as_str() == UNIVERSE {
                    "workareas_universe_is_root"
                } else if w.namespace.is_some() {
                    "workareas_user_name"
                } else {
                    "workareas_shared_name"
                };
                name_taken_or_backend(e, constraint, NamedKind::Workarea, &w.name)
            })?;
        }
        MetadataFold::WebEmailAllowed(entry) => {
            sqlx::query(
                "INSERT INTO web_emails (tenant_id, email_hash, email, user_name, admin, \
                 created_at, added_by, terms_version) \
                 VALUES ($1, $2, $3, $4, $5, ($6)::timestamptz, $7, $8)",
            )
            .bind(tenant_id)
            .bind(entry.email.digest().0.to_vec())
            .bind(entry.email.as_str())
            .bind(entry.user.as_str())
            .bind(entry.admin)
            .bind(at.as_str())
            .bind(entry.added_by)
            .bind(entry.terms_version.as_deref())
            .execute(&mut **tx)
            .await
            .map_err(|e| match &e {
                // The deployment-wide index on the digest: another
                // tenant's entry, invisible here, holds the email
                // (ADR-0045 d2).
                sqlx::Error::Database(db)
                    if db.constraint() == Some("web_emails_email_hash")
                        || db.constraint() == Some("web_emails_pkey") =>
                {
                    StoreError::EmailTaken
                }
                _ => backend(e),
            })?;
        }
        MetadataFold::WebEmailChanged {
            entry,
            keep_an_admin,
        } => {
            let hash = entry.email.digest().0.to_vec();
            if keep_an_admin && !entry.admin && last_admin_entry(tx, tenant_id, &hash).await? {
                return Err(StoreError::LastAdminEntry);
            }
            end_entry_sessions(tx, tenant_id, &hash).await?;
            let changed = sqlx::query(
                "UPDATE web_emails SET user_name = $3, admin = $4 \
                 WHERE tenant_id = $1 AND email_hash = $2",
            )
            .bind(tenant_id)
            .bind(&hash)
            .bind(entry.user.as_str())
            .bind(entry.admin)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if changed.rows_affected() != 1 {
                return Err(StoreError::Backend(
                    "web.email_changed names an entry the tenant does not hold".to_owned(),
                ));
            }
        }
        MetadataFold::WebEmailRemoved {
            email_hash,
            keep_an_admin,
        } => {
            let hash = email_hash.0.to_vec();
            if keep_an_admin && last_admin_entry(tx, tenant_id, &hash).await? {
                return Err(StoreError::LastAdminEntry);
            }
            end_entry_sessions(tx, tenant_id, &hash).await?;
            let removed =
                sqlx::query("DELETE FROM web_emails WHERE tenant_id = $1 AND email_hash = $2")
                    .bind(tenant_id)
                    .bind(&hash)
                    .execute(&mut **tx)
                    .await
                    .map_err(backend)?;
            if removed.rows_affected() != 1 {
                return Err(StoreError::Backend(
                    "web.email_removed names an entry the tenant does not hold".to_owned(),
                ));
            }
        }
        MetadataFold::QuotaSet {
            user_root,
            quota,
            limit,
        } => {
            match limit {
                Some(limit) => sqlx::query(
                    "INSERT INTO user_quotas (tenant_id, user_root, quota, max_count) \
                     VALUES ($1, $2, $3, $4) ON CONFLICT (tenant_id, user_root, quota) \
                     DO UPDATE SET max_count = EXCLUDED.max_count",
                )
                .bind(tenant_id)
                .bind(user_root)
                .bind(quota.as_str())
                .bind(i64::try_from(limit).map_err(backend)?),
                None => sqlx::query(
                    "DELETE FROM user_quotas WHERE tenant_id = $1 AND user_root = $2 \
                     AND quota = $3",
                )
                .bind(tenant_id)
                .bind(user_root)
                .bind(quota.as_str()),
            }
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::AccountProfileSet {
            user_root,
            profile,
            overrides,
        } => {
            match profile {
                Some(profile) => sqlx::query(
                    "INSERT INTO account_profiles (tenant_id, user_root, profile) \
                     VALUES ($1, $2, $3) ON CONFLICT (tenant_id, user_root) \
                     DO UPDATE SET profile = EXCLUDED.profile",
                )
                .bind(tenant_id)
                .bind(user_root)
                .bind(profile),
                None => sqlx::query(
                    "DELETE FROM account_profiles WHERE tenant_id = $1 AND user_root = $2",
                )
                .bind(tenant_id)
                .bind(user_root),
            }
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            // The overrides are replaced whole (ADR-0071 d5).
            sqlx::query("DELETE FROM user_quotas WHERE tenant_id = $1 AND user_root = $2")
                .bind(tenant_id)
                .bind(user_root)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            for (quota, limit) in overrides {
                sqlx::query(
                    "INSERT INTO user_quotas (tenant_id, user_root, quota, max_count) \
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(tenant_id)
                .bind(user_root)
                .bind(quota.as_str())
                .bind(i64::try_from(limit).map_err(backend)?)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            }
        }
        MetadataFold::UserDisabled { user } => {
            sqlx::query("DELETE FROM web_sessions WHERE tenant_id = $1 AND user_name = $2")
                .bind(tenant_id)
                .bind(user.as_str())
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            sqlx::query(
                "UPDATE web_emails SET disabled_at = ($3)::timestamptz \
                 WHERE tenant_id = $1 AND user_name = $2",
            )
            .bind(tenant_id)
            .bind(user.as_str())
            .bind(at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::DeletionRequested { user } => {
            sqlx::query(
                "INSERT INTO web_deletion_requests (tenant_id, user_name, requested_at) \
                 VALUES ($1, $2, ($3)::timestamptz) \
                 ON CONFLICT (tenant_id, user_name) WHERE closed_at IS NULL DO NOTHING",
            )
            .bind(tenant_id)
            .bind(user.as_str())
            .bind(at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::DeletionClosed { user } => {
            sqlx::query(
                "UPDATE web_deletion_requests SET closed_at = ($3)::timestamptz \
                 WHERE tenant_id = $1 AND user_name = $2 AND closed_at IS NULL",
            )
            .bind(tenant_id)
            .bind(user.as_str())
            .bind(at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::CrewCreated(c) => {
            sqlx::query(
                "INSERT INTO crews (id, tenant_id, name, charter_uri, external_manager, \
                 created_at) VALUES ($1, $2, $3, $4, $5, ($6)::timestamptz)",
            )
            .bind(c.crew_id)
            .bind(c.tenant_id)
            .bind(c.name.as_str())
            .bind(c.charter_uri.as_deref())
            .bind(c.external_manager.as_deref())
            .bind(at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                name_taken_or_backend(e, "crews_tenant_id_name", NamedKind::Crew, &c.name)
            })?;
        }
        MetadataFold::CrewUpdated(c) => {
            // The name is matched, never written: a crew record's name never
            // changes (ADR-0026 d8).
            let updated = sqlx::query(
                "UPDATE crews SET charter_uri = $3, external_manager = $4 \
                 WHERE tenant_id = $1 AND id = $2 AND name = $5",
            )
            .bind(tenant_id)
            .bind(c.crew_id)
            .bind(c.charter_uri.as_deref())
            .bind(c.external_manager.as_deref())
            .bind(c.name.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "crew {} named {} is not held by this tenant",
                    c.crew_id,
                    c.name.as_str()
                )));
            }
        }
        // The tenant-scoped foreign key onto crews refuses another tenant's
        // crew (0006_names.sql `workareas_crew`).
        MetadataFold::WorkareaCrewUpdated {
            workarea_id,
            crew_id,
        } => {
            let updated =
                sqlx::query("UPDATE workareas SET crew_id = $3 WHERE tenant_id = $1 AND id = $2")
                    .bind(tenant_id)
                    .bind(workarea_id)
                    .bind(crew_id)
                    .execute(&mut **tx)
                    .await
                    .map_err(backend)?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "workarea {workarea_id} is not held by this tenant"
                )));
            }
        }
        MetadataFold::WorkareaModeSet {
            workarea_id,
            in_progress,
            held_free,
        } => {
            // The commit's zero-held test, in the committing transaction
            // under the chain lock (ADR-0030 d6).
            if !held_free.is_empty() {
                let held: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM memories WHERE tenant_id = $1 \
                     AND workarea_id = ANY($2::uuid[]) AND status = 'held')",
                )
                .bind(tenant_id)
                .bind(&held_free)
                .fetch_one(&mut **tx)
                .await
                .map_err(backend)?;
                if held {
                    return Err(StoreError::HeldRemaining);
                }
            }
            let updated =
                sqlx::query("UPDATE workareas SET mode = $3 WHERE tenant_id = $1 AND id = $2")
                    .bind(tenant_id)
                    .bind(workarea_id)
                    .bind(if in_progress {
                        "in_progress"
                    } else {
                        "standard"
                    })
                    .execute(&mut **tx)
                    .await
                    .map_err(backend)?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "workarea {workarea_id} is not held by this tenant"
                )));
            }
        }
        MetadataFold::NamesAssigned(naming) => {
            fold_names_assigned(tx, tenant_id, at, &naming).await?;
        }
        MetadataFold::KeyIssued(k) => {
            let hash = k
                .key_hash
                .as_deref()
                .map(|h| {
                    key_hash_bytes(h).ok_or_else(|| {
                        StoreError::Backend(format!("key_hash {h:?} is not a sha256:<hex> string"))
                    })
                })
                .transpose()?;
            sqlx::query(
                // A connector key has no hash, which
                // api_keys_connector_has_no_hash holds (0012; ADR-0031 d7).
                "INSERT INTO api_keys (id, tenant_id, hash, credential, role, principal, \
                 held_by, default_workarea_id, issued_by, issued_seq, name, crew_id, \
                 user_root_id, session_for, guardrail_scope, profile) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
            )
            .bind(k.key_id)
            .bind(k.tenant_id)
            .bind(hash)
            .bind(k.credential.as_str())
            .bind(k.role.as_str())
            .bind(&k.principal)
            .bind(k.held_by.as_str())
            .bind(k.default_workarea)
            .bind(k.issued_by)
            .bind(seq)
            .bind(k.name.as_ref().map(Name::as_str))
            .bind(k.crew_id)
            .bind(k.user)
            .bind(k.session_user.as_ref().map(Name::as_str))
            .bind(k.guardrail_scope.as_str())
            .bind(k.profile.map(KeyProfile::as_str))
            .execute(&mut **tx)
            .await
            .map_err(|e| match &k.name {
                Some(name) => {
                    name_taken_or_backend(e, "api_keys_tenant_id_name", NamedKind::Key, name)
                }
                None => backend(e),
            })?;
        }
        // The schema's role CHECK and its foreign keys refuse a grant or
        // denial on a key or node the tenant does not hold; the error aborts
        // the commit. The keys name the tenant with the id, (tenant_id,
        // key_id) and (tenant_id, workarea_node_id), because PostgreSQL runs
        // a foreign-key check without row-level security: a reference on id
        // alone would accept another tenant's key or node (0004_grants.sql).
        MetadataFold::GrantIssued { key_id, grant } => {
            sqlx::query(
                "INSERT INTO grants (id, tenant_id, key_id, role, workarea_node_id, deny, epoch, \
                 covers) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(grant.grant_id)
            .bind(tenant_id)
            .bind(key_id)
            .bind(grant.role.as_str())
            .bind(grant.node)
            .bind(deny_to_json(&grant.deny))
            .bind(seq)
            .bind(grant.covers.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::KeyDenied {
            key_id,
            denial,
            created_by,
        } => {
            sqlx::query(
                "INSERT INTO key_denials (id, tenant_id, key_id, workarea_node_id, epoch, \
                 created_by) VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(denial.denial_id)
            .bind(tenant_id)
            .bind(key_id)
            .bind(denial.node)
            .bind(seq)
            .bind(created_by)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::KeyDenialLifted { key_id, denial_id } => {
            // The row stays as history, its lifted_epoch this commit's seq;
            // the guard matches only a denial in force on this key of this
            // tenant, so a lift of anything else aborts the commit rather
            // than journalling a lift of nothing (0006_names.sql
            // `key_denials.lifted_epoch`).
            let lifted = sqlx::query(
                "UPDATE key_denials SET lifted_epoch = $4 \
                 WHERE tenant_id = $1 AND id = $2 AND key_id = $3 AND lifted_epoch IS NULL",
            )
            .bind(tenant_id)
            .bind(denial_id)
            .bind(key_id)
            .bind(seq)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if lifted.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "denial {denial_id} is not a denial in force on key {key_id} of this tenant"
                )));
            }
        }
        MetadataFold::KeyRevoked { key_id } => {
            // The row stays as history, stamped with the event's `at`, which
            // the key read serves (ADR-0026 d10); key_by_hash stops resolving
            // it. A key the tenant does not hold matches no row under
            // tenant_isolation, and that aborts the commit rather than
            // journalling a revocation of nothing.
            let revoked = sqlx::query(
                "UPDATE api_keys SET revoked_at = ($3)::timestamptz \
                 WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL",
            )
            .bind(tenant_id)
            .bind(key_id)
            .bind(at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if revoked.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "key {key_id} is not a live key of this tenant"
                )));
            }
            // Its unredeemed codes are voided in this commit (ADR-0031 d8).
            sqlx::query(
                "UPDATE connect_codes SET voided_seq = $3 \
                 WHERE tenant_id = $1 AND key_id = $2 \
                 AND redeemed_at IS NULL AND voided_seq IS NULL",
            )
            .bind(tenant_id)
            .bind(key_id)
            .bind(seq)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::ConnectCodeIssued { code, voids } => {
            fold_code_issued(tx, tenant_id, seq, code, voids).await?;
        }
        MetadataFold::ConnectionOpened {
            connection_id,
            replaces,
            auth_code_hash,
            tokens,
        } => {
            fold_connection_opened(
                tx,
                tenant_id,
                (seq, at),
                connection_id,
                replaces,
                &auth_code_hash,
            )
            .await?;
            insert_tokens(tx, tenant_id, connection_id, tokens, at).await?;
        }
        MetadataFold::ConnectionRevoked {
            connection_id,
            cause,
        } => {
            fold_connection_revoked(tx, tenant_id, seq, connection_id, cause).await?;
        }
        MetadataFold::MemoryHeld {
            memory,
            enqueue,
            findings,
        } => {
            insert_memory(tx, event_id, &memory).await?;
            insert_findings(
                tx,
                tenant_id,
                seq,
                &[(memory.memory_id, findings.into_vec())],
            )
            .await?;
            if enqueue {
                enqueue_review(tx, tenant_id, memory.memory_id).await?;
            }
        }
        MetadataFold::MemoryPromoted {
            memory_id,
            approval,
        } => {
            promote_memory(tx, tenant_id, memory_id, at, approval.as_deref()).await?;
        }
        MetadataFold::MemoryRejected { memory_id } => {
            set_memory_status(tx, tenant_id, memory_id, MemoryStatus::Rejected).await?;
        }
        MetadataFold::SourceChecked { memory_id, check } => {
            // The version's latest check; the journal keeps every one
            // (ADR-0057 d5). The application admitted a promoted tip.
            let (path, fragment) = check.moved_to.map_or((None, None), |(p, f)| (Some(p), f));
            sqlx::query(
                "UPDATE memories SET source_check_rev = $3, source_check_result = $4, \
                 source_check_hash = $5, source_check_path = $6, source_check_fragment = $7, \
                 source_check_at = $8 \
                 WHERE tenant_id = $1 AND id = $2 AND status = 'promoted'",
            )
            .bind(tenant_id)
            .bind(memory_id)
            .bind(&check.rev)
            .bind(check.result.as_str())
            .bind(check.hash.as_deref())
            .bind(path)
            .bind(fragment)
            .bind(check.at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::FindingsRecorded {
            scanned_under,
            versions,
        } => {
            // Held versions alone; a finding already on the row stays
            // (ADR-0065 d13, d15).
            let ids: Vec<Uuid> = versions.iter().map(|v| v.memory_id).collect();
            let summaries: Vec<Option<String>> =
                versions.iter().map(|v| v.summary.clone()).collect();
            // The versions still held: a list is kept for those alone.
            let held: Vec<Uuid> = sqlx::query_scalar(
                "UPDATE memories m SET scanned_under = $2, \
                 scan_finding = COALESCE(m.scan_finding, u.finding) \
                 FROM unnest($3::uuid[], $4::text[]) AS u(id, finding) \
                 WHERE m.tenant_id = $1 AND m.id = u.id AND m.status = 'held' \
                 RETURNING m.id",
            )
            .bind(tenant_id)
            .bind(scanned_under)
            .bind(ids)
            .bind(summaries)
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?;
            let lists: Vec<(Uuid, Vec<VersionFinding>)> = versions
                .into_iter()
                .filter(|v| held.contains(&v.memory_id))
                .map(|v| (v.memory_id, v.findings))
                .collect();
            insert_findings(tx, tenant_id, seq, &lists).await?;
        }
        MetadataFold::TipSuperseded {
            successor,
            replaced,
            replaced_valid_until,
            approval,
        } => {
            supersede_tip(
                tx,
                tenant_id,
                successor,
                replaced,
                replaced_valid_until.as_deref(),
                at,
                approval.as_deref(),
            )
            .await?;
        }
        MetadataFold::TransitionProposed(p) => {
            // The proposer principal must be the one the proposing key's
            // row holds, or the ratifying commit's R2 check would compare
            // against a principal no key holds (ADR-0010 R2). The schema
            // refuses a step other than one rung forward, a ratified row
            // with no ratifier or no seq, and a node or key of another
            // tenant (0017_workarea_transitions.sql); the record's state is
            // written as carried, so a record that is not pending meets
            // those checks rather than being coerced.
            let principal: Option<String> = sqlx::query_scalar(
                "SELECT principal FROM api_keys WHERE tenant_id = $1 AND id = $2",
            )
            .bind(tenant_id)
            .bind(p.proposer_key_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(backend)?;
            if principal.as_deref() != Some(p.proposer_principal.as_str()) {
                return Err(StoreError::Backend(format!(
                    "transition {}'s proposer principal is not its proposing key's",
                    p.transition_id
                )));
            }
            sqlx::query(
                "INSERT INTO workarea_transitions (id, tenant_id, workarea_id, from_status, \
                 to_status, state, proposer_key_id, proposer_principal, ratifier_key_id, \
                 proposed_seq) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(p.transition_id)
            .bind(tenant_id)
            .bind(p.workarea_id)
            .bind(p.from.as_str())
            .bind(p.to.as_str())
            .bind(p.state.as_str())
            .bind(p.proposer_key_id)
            .bind(&p.proposer_principal)
            .bind(p.ratifier_key_id)
            .bind(seq)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::TransitionRatified {
            transition_id,
            workarea_id,
            to,
            ratifier_key_id,
        } => {
            ratify_transition(
                tx,
                tenant_id,
                seq,
                transition_id,
                workarea_id,
                to,
                ratifier_key_id,
            )
            .await?;
        }
        MetadataFold::ClaimEdgeProposed(edge) => {
            propose_claim_edge(tx, tenant_id, &edge, at).await?;
        }
        MetadataFold::ClaimEdgeRatified {
            edge_id,
            ratifier_key_id,
        } => {
            ratify_claim_edge(tx, tenant_id, edge_id, ratifier_key_id).await?;
        }
        MetadataFold::BriefEdgeProposed(edge) => {
            propose_brief_edge(tx, tenant_id, &edge, at).await?;
        }
        MetadataFold::BriefEdgeRatified {
            edge_id,
            ratifier_key_id,
        } => {
            ratify_brief_edge(tx, tenant_id, edge_id, ratifier_key_id, seq).await?;
        }
    }
    Ok(())
}

/// `workarea.status_transition`'s fold, under the chain lock the commit
/// already holds. The node row and the proposal row are locked and read
/// first, so nothing moves either between the re-checks and the writes
/// (workareas.md, ratification re-checks). A row that is not a proposal on
/// this node with this `to`, and a ratifier key the tenant does not hold,
/// are defects; a row already ratified or whose `from` is no longer the
/// node's status is [`StoreError::TransitionStale`]; a ratifier that is the
/// proposing key or holds the proposer's principal, read from its key row
/// here, is [`StoreError::RatifierIsProposer`]; and a step to `validated`
/// that finds a direct child not `validated` is
/// [`StoreError::ChildrenNotValidated`]. Any refusal refuses the commit,
/// which writes nothing. The schema still refuses a ratifier that is the
/// proposing key and a universe root moved off `committed` (ADR-0009 d1,
/// d3, d4; ADR-0010 R2).
async fn ratify_transition(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    seq: i64,
    transition_id: Uuid,
    workarea_id: Uuid,
    to: WorkareaStatus,
    ratifier_key_id: Uuid,
) -> Result<(), StoreError> {
    let defect = || {
        StoreError::Backend(format!(
            "transition {transition_id} is not a proposal on workarea {workarea_id} to {}",
            to.as_str()
        ))
    };
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM workareas WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(workarea_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    let Some(current) = status else {
        return Err(defect());
    };
    let row: Option<(Uuid, String, String, String, Uuid, String)> = sqlx::query_as(
        "SELECT workarea_id, from_status, to_status, state, proposer_key_id, \
         proposer_principal FROM workarea_transitions \
         WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(transition_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    let Some((row_workarea, from, row_to, state, proposer_key_id, proposer_principal)) = row else {
        return Err(defect());
    };
    if row_workarea != workarea_id || row_to != to.as_str() {
        return Err(defect());
    }
    if state != TransitionState::Pending.as_str() || from != current {
        let from = WorkareaStatus::parse(&from).ok_or_else(|| {
            StoreError::Backend(format!("transition {transition_id} has from {from:?}"))
        })?;
        return Err(StoreError::TransitionStale {
            transition_id,
            workarea_id,
            from,
            to,
        });
    }
    let ratifier_principal: Option<String> =
        sqlx::query_scalar("SELECT principal FROM api_keys WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(ratifier_key_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(backend)?;
    let Some(ratifier_principal) = ratifier_principal else {
        return Err(StoreError::Backend(format!(
            "key {ratifier_key_id} is not held by this tenant"
        )));
    };
    if ratifier_is_proposer(
        proposer_key_id,
        &proposer_principal,
        ratifier_key_id,
        &ratifier_principal,
    ) {
        return Err(StoreError::RatifierIsProposer { transition_id });
    }
    if to == WorkareaStatus::Validated {
        // Under the chain lock every child create and reparent waits for
        // this commit, so the read sees the subtree as it is (ADR-0009 d4).
        let short: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM workareas WHERE tenant_id = $1 AND parent_id = $2 \
             AND status <> 'validated' LIMIT 1",
        )
        .bind(tenant_id)
        .bind(workarea_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(backend)?;
        if short.is_some() {
            return Err(StoreError::ChildrenNotValidated {
                transition_id,
                workarea_id,
            });
        }
    }
    sqlx::query(
        "UPDATE workarea_transitions SET state = 'ratified', ratifier_key_id = $3, \
         ratified_seq = $4 WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(transition_id)
    .bind(ratifier_key_id)
    .bind(seq)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    sqlx::query("UPDATE workareas SET status = $3 WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id)
        .bind(workarea_id)
        .bind(to.as_str())
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

/// Voids every code of `key_id` from which no connection has opened, spent
/// or not, their `voided_seq` this commit's seq: the mint's and the
/// revocation's voiding (roles-and-keys.md connect codes; ADR-0031 d8).
async fn void_unopened_codes(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    key_id: Uuid,
    seq: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE connect_codes c SET voided_seq = $3 \
         WHERE c.tenant_id = $1 AND c.key_id = $2 AND c.voided_seq IS NULL \
         AND NOT EXISTS (SELECT 1 FROM oauth_connections o \
                          WHERE o.tenant_id = c.tenant_id AND o.code_id = c.id \
                            AND o.opened_seq IS NOT NULL)",
    )
    .bind(tenant_id)
    .bind(key_id)
    .bind(seq)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(())
}

/// `connection.code_issued`: the mint names the key's one unredeemed,
/// unvoided code as the one it voids, or none when the key has none; it
/// voids every code of the key from which no connection opened and inserts
/// the new one, which `connect_codes_live` admits only once they are void.
/// A key the tenant does not hold meets the key (tenant_id, key_id) and is
/// refused as any `23503` is (event-catalog.md `connection.code_issued`;
/// ADR-0031 d8; ADR-0034 d12).
async fn fold_code_issued(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    seq: i64,
    code: ConnectCode,
    voids: Option<Uuid>,
) -> Result<(), StoreError> {
    let unredeemed: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM connect_codes \
         WHERE tenant_id = $1 AND key_id = $2 AND redeemed_at IS NULL AND voided_seq IS NULL \
         FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(code.key_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    if unredeemed != voids {
        return Err(StoreError::ConnectCodeMoved {
            key_id: code.key_id,
        });
    }
    void_unopened_codes(tx, tenant_id, code.key_id, seq).await?;
    sqlx::query(
        "INSERT INTO connect_codes (id, tenant_id, key_id, hash, issued_seq, expires_at) \
         VALUES ($1, $2, $3, $4, $5, ($6)::timestamptz)",
    )
    .bind(code.code_id)
    .bind(tenant_id)
    .bind(code.key_id)
    .bind(code.code_hash.0.to_vec())
    .bind(seq)
    .bind(code.expires_at.as_str())
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(())
}

/// `connection.opened`, under the chain lock: the key's open connection,
/// which `replaces` must name, is revoked as `replaced` first, then the
/// exchange's conditional update opens the connection only while it is
/// `authorized`, its authorization code unredeemed and live at `at`, its
/// connect code unvoided and its key unrevoked, stamping `opened_seq` and
/// `auth_code_redeemed_at`. A second open connection of the key, an opening
/// that names none while one is open, is refused by
/// `oauth_connections_open`. The update matches the row by `presented`,
/// the exchange's code digest, so a digest that is not the connection's is
/// refused as the rest are (postgres-schema.md `oauth_connections`, and
/// redemption and rotation are conditional updates; ADR-0031 d7, d8, d10,
/// d11, d12).
async fn fold_connection_opened(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    (seq, at): (i64, &EventTime),
    connection_id: Uuid,
    replaces: Option<Uuid>,
    presented: &Sha256Hash,
) -> Result<(), StoreError> {
    let key_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT key_id FROM oauth_connections WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(connection_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    // Every refusal of an opening is one typed refusal, so the exchange can
    // tell a lost race from a backend failure (ADR-0031 d10).
    let not_openable = StoreError::ConnectionNotOpenable { connection_id };
    let Some(key_id) = key_id else {
        return Err(not_openable);
    };
    if let Some(replaced) = replaces {
        let revoked = sqlx::query(
            "UPDATE oauth_connections \
             SET state = 'revoked', revoked_cause = 'replaced', revoked_seq = $4 \
             WHERE tenant_id = $1 AND id = $2 AND key_id = $3 AND state = 'open'",
        )
        .bind(tenant_id)
        .bind(replaced)
        .bind(key_id)
        .bind(seq)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
        if revoked.rows_affected() != 1 {
            return Err(not_openable);
        }
    }
    let opened = sqlx::query(
        "UPDATE oauth_connections o \
         SET state = 'open', opened_seq = $3, auth_code_redeemed_at = ($4)::timestamptz \
         WHERE o.tenant_id = $1 AND o.id = $2 AND o.auth_code_hash = $5 \
         AND o.state = 'authorized' \
         AND o.auth_code_redeemed_at IS NULL AND o.auth_code_expires_at > ($4)::timestamptz \
         AND EXISTS (SELECT 1 FROM connect_codes c \
                      WHERE c.tenant_id = o.tenant_id AND c.id = o.code_id \
                        AND c.voided_seq IS NULL) \
         AND EXISTS (SELECT 1 FROM api_keys k \
                      WHERE k.tenant_id = o.tenant_id AND k.id = o.key_id \
                        AND k.revoked_at IS NULL)",
    )
    .bind(tenant_id)
    .bind(connection_id)
    .bind(seq)
    .bind(at.as_str())
    .bind(presented.0.to_vec())
    .execute(&mut **tx)
    .await
    .map_err(|e| match &e {
        // The key has an open connection the opening does not name.
        sqlx::Error::Database(db) if db.constraint() == Some("oauth_connections_open") => {
            not_openable.clone()
        }
        _ => backend(e),
    })?;
    // Not authorized, its authorization code redeemed or expired, its
    // connect code voided or its key revoked.
    if opened.rows_affected() != 1 {
        return Err(not_openable);
    }
    Ok(())
}

/// Inserts the access and refresh rows of `connection_id`, created at `at`,
/// in the caller's transaction; a digest held already meets the primary
/// key and aborts it (postgres-schema.md `oauth_tokens`; ADR-0031 d10).
async fn insert_tokens(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    connection_id: Uuid,
    tokens: TokenPair,
    at: &EventTime,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO oauth_tokens (hash, tenant_id, connection_id, kind, expires_at, created_at) \
         VALUES ($1, $3, $4, 'access', ($2)::timestamptz, ($7)::timestamptz), \
                ($5, $3, $4, 'refresh', ($6)::timestamptz, ($7)::timestamptz)",
    )
    .bind(tokens.access.token_hash.0.to_vec())
    .bind(tokens.access.expires_at.as_str())
    .bind(tenant_id)
    .bind(connection_id)
    .bind(tokens.refresh.token_hash.0.to_vec())
    .bind(tokens.refresh.expires_at.as_str())
    .bind(at.as_str())
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(())
}

/// `connection.revoked`: the open connection is revoked with `cause`, its
/// `revoked_seq` this commit's seq, and every code of its key from which no
/// connection opened is voided. A connection that is not open, and the
/// cause `replaced`, which only a newer `connection.opened` gives, are
/// refused (event-catalog.md `connection.revoked`; ADR-0031 d8, d12).
async fn fold_connection_revoked(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    seq: i64,
    connection_id: Uuid,
    cause: RevokedCause,
) -> Result<(), StoreError> {
    if cause == RevokedCause::Replaced {
        return Err(StoreError::Backend(
            "only a newer connection.opened replaces a connection".to_owned(),
        ));
    }
    let key_id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE oauth_connections \
         SET state = 'revoked', revoked_cause = $3, revoked_seq = $4 \
         WHERE tenant_id = $1 AND id = $2 AND state = 'open' RETURNING key_id",
    )
    .bind(tenant_id)
    .bind(connection_id)
    .bind(cause.as_str())
    .bind(seq)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    let Some(key_id) = key_id else {
        return Err(StoreError::Backend(format!(
            "connection {connection_id} is not an open connection of this tenant"
        )));
    };
    void_unopened_codes(tx, tenant_id, key_id, seq).await
}

// ---------------------------------------------------------------------------
// Usage metering (ADR-0071): the counters a metered fold moves, in the
// commit's own transaction, and the ceiling the commit carries
// ---------------------------------------------------------------------------

/// An account's counters as `usage_accounts` reads them: root, profile and
/// six counts.
type CounterRow = (Uuid, Option<String>, i64, i64, i64, i64, i64, i64);
/// A ledger row as read: root, day, profile and six counts.
type LedgerTuple = (Uuid, String, Option<String>, i64, i64, i64, i64, i64, i64);
/// A drift row: root, then each compared meter's counter and source value.
type DriftRow = (Uuid, i64, i64, i64, i64, i64, i64, i64, i64, i64, i64);

/// Six counts in [`Meter::ALL`]'s order as a usage; a negative count,
/// which only a drifted counter holds, reads as zero.
fn usage_of(
    (memories, text_bytes, writes, recalls, keys, connections): (i64, i64, i64, i64, i64, i64),
) -> oml_app::types::UserUsage {
    let n = |v: i64| u64::try_from(v).unwrap_or(0);
    oml_app::types::UserUsage {
        memories: n(memories),
        text_bytes: n(text_bytes),
        writes: n(writes),
        recalls: n(recalls),
        keys: n(keys),
        connections: n(connections),
    }
}

/// What a fold moves on the account meters, read before the fold
/// (ADR-0071 d2, d3): each direct charge as (account, meter, delta), the
/// clock read a held version's write is counted on, and the chain of an
/// account whose liveness the fold may change.
#[derive(Debug, Default)]
struct MeterPlan {
    charges: Vec<(Uuid, Meter, i64)>,
    /// A held version's own `created_at`: its write is counted on that UTC
    /// day, the one clock read the drift report also reads.
    written_at: Option<EventTime>,
    chain: Option<TrackedChain>,
}

/// A chain whose liveness a fold may change, with its liveness before the
/// fold.
#[derive(Debug)]
struct TrackedChain {
    workarea_id: Uuid,
    claim_key: String,
    /// The chain's account; `None` for a chain whose first version the
    /// fold itself may store, so its account is read once the fold has,
    /// and only when its liveness changed.
    account: Option<Uuid>,
    live_before: bool,
}

/// The account a key belongs to, if any (ADR-0071 d1).
async fn author_account(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    key_id: Uuid,
) -> Result<Option<Uuid>, StoreError> {
    let root: Option<Option<Uuid>> =
        sqlx::query_scalar("SELECT user_root_id FROM api_keys WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(key_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(backend)?;
    Ok(root.flatten())
}

/// Whether the chain is live by ADR-0071 d2, through the one SQL
/// definition 0045 holds.
async fn chain_live(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workarea_id: Uuid,
    claim_key: &str,
) -> Result<bool, StoreError> {
    sqlx::query_scalar("SELECT oml_chain_live($1, $2, $3)")
        .bind(tenant_id)
        .bind(workarea_id)
        .bind(claim_key)
        .fetch_one(&mut **tx)
        .await
        .map_err(backend)
}

/// The chain as the meters track it: its account and its liveness now, in
/// one statement; `None` for a chain that is no account's, which no meter
/// counts.
async fn tracked_chain(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workarea_id: Uuid,
    claim_key: &str,
) -> Result<Option<TrackedChain>, StoreError> {
    let (account, live): (Option<Uuid>, bool) =
        sqlx::query_as("SELECT oml_chain_account($1, $2, $3), oml_chain_live($1, $2, $3)")
            .bind(tenant_id)
            .bind(workarea_id)
            .bind(claim_key)
            .fetch_one(&mut **tx)
            .await
            .map_err(backend)?;
    Ok(account.map(|account| TrackedChain {
        workarea_id,
        claim_key: claim_key.to_owned(),
        account: Some(account),
        live_before: live,
    }))
}

/// [`tracked_chain`] of the chain a version belongs to, in one statement.
async fn tracked_chain_of(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    memory_id: Uuid,
) -> Result<Option<TrackedChain>, StoreError> {
    let row: Option<(Uuid, String, Option<Uuid>, bool)> = sqlx::query_as(
        "SELECT m.workarea_id, m.claim_key, \
                oml_chain_account(m.tenant_id, m.workarea_id, m.claim_key), \
                oml_chain_live(m.tenant_id, m.workarea_id, m.claim_key) \
           FROM memories m WHERE m.tenant_id = $1 AND m.id = $2",
    )
    .bind(tenant_id)
    .bind(memory_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(row.and_then(|(workarea_id, claim_key, account, live)| {
        account.map(|account| TrackedChain {
            workarea_id,
            claim_key,
            account: Some(account),
            live_before: live,
        })
    }))
}

/// The account a connection counts on, read before its fold: its key's,
/// while the key is live and no session key and, with `open`, the
/// connection is open (ADR-0071 d2).
async fn connection_account(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    connection_id: Uuid,
    open: bool,
) -> Result<Option<Uuid>, StoreError> {
    let root: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT k.user_root_id FROM oauth_connections c \
         JOIN api_keys k ON k.tenant_id = c.tenant_id AND k.id = c.key_id \
         WHERE c.tenant_id = $1 AND c.id = $2 AND (NOT $3 OR c.state = 'open') \
           AND k.revoked_at IS NULL AND k.credential <> 'session'",
    )
    .bind(tenant_id)
    .bind(connection_id)
    .bind(open)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(root.flatten())
}

/// What `fold` moves on the meters, read before it is applied. `written`
/// is what the caller already read of a held version; without it the store
/// reads the author's key and the chain. A fold that charges no account
/// and touches no account's chain reads nothing: a tenant key's version
/// that starts its chain, and every fold no meter counts (ADR-0071 d3).
async fn meter_plan(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    fold: &MetadataFold,
    written: Option<Written>,
) -> Result<MeterPlan, StoreError> {
    let mut plan = MeterPlan::default();
    match fold {
        MetadataFold::MemoryHeld { memory, .. } => {
            let author = match written {
                Some(written) => written.author_account,
                None => author_account(tx, tenant_id, memory.author_key_id).await?,
            };
            if let Some(root) = author {
                let bytes = i64::try_from(memory.body_bytes).map_err(backend)?;
                plan.charges.push((root, Meter::TextBytes, bytes));
                plan.charges.push((root, Meter::Writes, 1));
                plan.written_at = Some(memory.created_at.clone());
            }
            match written {
                // The version starts its chain: the chain is its author's
                // account's, was not live, and a held version makes it
                // live, so nothing is read.
                Some(Written {
                    starts_chain: true, ..
                }) => {
                    if let Some(root) = author {
                        plan.charges.push((root, Meter::Memories, 1));
                    }
                }
                // A revision, or a claim captured under a released key:
                // the chain has rows, and its first names its account.
                Some(_) => {
                    plan.chain =
                        tracked_chain(tx, tenant_id, memory.workarea_id, &memory.claim_key).await?;
                }
                // Nothing was read for the store: the chain may be this
                // version's to start, so its account is read after the
                // fold, when its liveness changed.
                None => {
                    plan.chain = Some(TrackedChain {
                        workarea_id: memory.workarea_id,
                        claim_key: memory.claim_key.clone(),
                        account: None,
                        live_before: chain_live(
                            tx,
                            tenant_id,
                            memory.workarea_id,
                            &memory.claim_key,
                        )
                        .await?,
                    });
                }
            }
        }
        MetadataFold::MemoryPromoted { memory_id, .. }
        | MetadataFold::MemoryRejected { memory_id } => {
            plan.chain = tracked_chain_of(tx, tenant_id, *memory_id).await?;
        }
        MetadataFold::TipSuperseded { successor, .. } => {
            plan.chain = tracked_chain_of(tx, tenant_id, *successor).await?;
        }
        MetadataFold::KeyIssued(k) => {
            if let Some(root) = k.user
                && k.credential != Credential::Session
            {
                plan.charges.push((root, Meter::Keys, 1));
            }
        }
        MetadataFold::KeyRevoked { key_id } => {
            // The key leaves the keys meter, and its open connection the
            // connections meter with it (ADR-0071 d2).
            let row: Option<(Option<Uuid>, bool, bool)> = sqlx::query_as(
                "SELECT k.user_root_id, k.revoked_at IS NULL AND k.credential <> 'session', \
                        EXISTS (SELECT 1 FROM oauth_connections c \
                                 WHERE c.tenant_id = k.tenant_id AND c.key_id = k.id \
                                   AND c.state = 'open') \
                   FROM api_keys k WHERE k.tenant_id = $1 AND k.id = $2",
            )
            .bind(tenant_id)
            .bind(key_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(backend)?;
            if let Some((Some(root), true, connected)) = row {
                plan.charges.push((root, Meter::Keys, -1));
                if connected {
                    plan.charges.push((root, Meter::Connections, -1));
                }
            }
        }
        MetadataFold::ConnectionOpened {
            connection_id,
            replaces,
            ..
        } => {
            // A connection that replaces the key's open one takes its
            // place on the meter.
            if replaces.is_none()
                && let Some(root) = connection_account(tx, tenant_id, *connection_id, false).await?
            {
                plan.charges.push((root, Meter::Connections, 1));
            }
        }
        MetadataFold::ConnectionRevoked { connection_id, .. } => {
            if let Some(root) = connection_account(tx, tenant_id, *connection_id, true).await? {
                plan.charges.push((root, Meter::Connections, -1));
            }
        }
        // No meter counts these. The match names each, so a fold added
        // later, a demotion or an erasure among them, does not compile
        // until it says what it moves (ADR-0071 d2).
        MetadataFold::TenantGenesis { .. }
        | MetadataFold::WorkareaCreated(_)
        | MetadataFold::CrewCreated(_)
        | MetadataFold::CrewUpdated(_)
        | MetadataFold::WorkareaCrewUpdated { .. }
        | MetadataFold::WorkareaModeSet { .. }
        | MetadataFold::NamesAssigned(_)
        | MetadataFold::GrantIssued { .. }
        | MetadataFold::KeyDenied { .. }
        | MetadataFold::KeyDenialLifted { .. }
        | MetadataFold::ConnectCodeIssued { .. }
        | MetadataFold::VerdictRecorded { .. }
        | MetadataFold::SourceChecked { .. }
        | MetadataFold::FindingsRecorded { .. }
        | MetadataFold::WebEmailAllowed(_)
        | MetadataFold::WebEmailChanged { .. }
        | MetadataFold::WebEmailRemoved { .. }
        | MetadataFold::AdminRead
        | MetadataFold::QuotaSet { .. }
        | MetadataFold::AccountProfileSet { .. }
        | MetadataFold::UserDisabled { .. }
        | MetadataFold::DeletionRequested { .. }
        | MetadataFold::DeletionClosed { .. }
        | MetadataFold::TransitionProposed(_)
        | MetadataFold::TransitionRatified { .. }
        | MetadataFold::ClaimEdgeProposed(_)
        | MetadataFold::ClaimEdgeRatified { .. }
        | MetadataFold::BriefEdgeProposed(_)
        | MetadataFold::BriefEdgeRatified { .. }
        | MetadataFold::SearchDefaultSet { .. }
        | MetadataFold::TelemetrySettingsSet { .. }
        | MetadataFold::FederationPolicySet(_)
        | MetadataFold::WorkareaFederationSet { .. } => {}
    }
    Ok(plan)
}

/// One account's deltas, in the order the ceiling is held:
/// memories, text bytes, writes, keys, connections.
type Deltas = [i64; 5];

/// After the fold: the tracked chain's liveness again, then each charged
/// account's stocks in one statement and its day's writes in another, and
/// the ceiling the commit carries held against each meter it raised. A
/// stock never goes below zero: a fold that takes one off a counter that
/// is already there, which only a drifted counter is, leaves it, and the
/// drift report names it. A meter past its limit is returned, and the
/// caller refuses the commit with it, which rolls back with the journal
/// row (ADR-0071 d3).
async fn apply_meters(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    mut plan: MeterPlan,
    ceiling: Option<&Ceiling>,
) -> Result<Option<QuotaReached>, StoreError> {
    if let Some(chain) = plan.chain.take() {
        let after = chain_live(tx, tenant_id, chain.workarea_id, &chain.claim_key).await?;
        if after != chain.live_before {
            let account = match chain.account {
                Some(account) => Some(account),
                None => sqlx::query_scalar("SELECT oml_chain_account($1, $2, $3)")
                    .bind(tenant_id)
                    .bind(chain.workarea_id)
                    .bind(&chain.claim_key)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(backend)?,
            };
            if let Some(account) = account {
                plan.charges
                    .push((account, Meter::Memories, if after { 1 } else { -1 }));
            }
        }
    }
    let mut accounts: BTreeMap<Uuid, Deltas> = BTreeMap::new();
    for (root, meter, delta) in plan.charges {
        let slot = match meter {
            Meter::Memories => 0,
            Meter::TextBytes => 1,
            Meter::Writes => 2,
            Meter::Keys => 3,
            Meter::Connections => 4,
            Meter::Recalls => unreachable!("no fold counts a recall"),
        };
        accounts.entry(root).or_default()[slot] += delta;
    }
    for (root, deltas) in accounts {
        let [memories, text_bytes, writes, keys, connections] = deltas;
        let mut used: Deltas = [0; 5];
        if [memories, text_bytes, keys, connections] != [0; 4] {
            let (m, b, k, c): (i64, i64, i64, i64) = sqlx::query_as(
                "INSERT INTO account_meters \
                        (tenant_id, user_root, memories, text_bytes, keys, connections) \
                 VALUES ($1, $2, greatest($3::bigint, 0), greatest($4::bigint, 0), \
                         greatest($5::bigint, 0), greatest($6::bigint, 0)) \
                 ON CONFLICT (tenant_id, user_root) DO UPDATE SET \
                     memories = greatest(account_meters.memories + $3::bigint, 0), \
                     text_bytes = greatest(account_meters.text_bytes + $4::bigint, 0), \
                     keys = greatest(account_meters.keys + $5::bigint, 0), \
                     connections = greatest(account_meters.connections + $6::bigint, 0) \
                 RETURNING memories, text_bytes, keys, connections",
            )
            .bind(tenant_id)
            .bind(root)
            .bind(memories)
            .bind(text_bytes)
            .bind(keys)
            .bind(connections)
            .fetch_one(&mut **tx)
            .await
            .map_err(backend)?;
            (used[0], used[1], used[3], used[4]) = (m, b, k, c);
        }
        if writes != 0 {
            let at = plan
                .written_at
                .as_ref()
                .ok_or_else(|| backend("a write is counted on its version's day"))?;
            let month = Month::of(at);
            // The day's row and the month's sum in one statement: the
            // other days' rows as they stood, and this day's as written.
            used[2] = sqlx::query_scalar(
                "WITH day AS ( \
                     INSERT INTO account_activity (tenant_id, user_root, day, writes) \
                     VALUES ($1, $2, ($3)::date, $4) \
                     ON CONFLICT (tenant_id, user_root, day) \
                     DO UPDATE SET writes = account_activity.writes + EXCLUDED.writes \
                     RETURNING writes) \
                 SELECT ((SELECT writes FROM day) + coalesce(( \
                     SELECT sum(a.writes) FROM account_activity a \
                      WHERE a.tenant_id = $1 AND a.user_root = $2 \
                        AND a.day >= ($5)::date AND a.day < ($6)::date \
                        AND a.day <> ($3)::date), 0))::bigint",
            )
            .bind(tenant_id)
            .bind(root)
            .bind(oml_app::domain::usage::day_of(at))
            .bind(writes)
            .bind(month.first_day())
            .bind(month.next().first_day())
            .fetch_one(&mut **tx)
            .await
            .map_err(backend)?;
        }
        if let Some(ceiling) = ceiling.filter(|c| c.user_root == root) {
            for (quota, delta, used) in [
                (Quota::Memories, memories, used[0]),
                (Quota::TextBytes, text_bytes, used[1]),
                (Quota::WritesPerMonth, writes, used[2]),
                (Quota::Keys, keys, used[3]),
                (Quota::Connections, connections, used[4]),
            ] {
                let limit = ceiling.quotas.get(quota);
                let used = u64::try_from(used).unwrap_or(0);
                if delta > 0 && used > limit {
                    return Ok(Some(QuotaReached {
                        quota,
                        limit,
                        used: used.saturating_sub(delta.unsigned_abs()),
                    }));
                }
            }
        }
    }
    Ok(None)
}

/// Appends one event inside the commit's transaction if it extends
/// `expected`, the (seq, prev_hash) the head calls for: the journal row, the
/// chain-head upsert, the metadata fold, the outbox row and the idempotency
/// record (ADR-0002 d1).
async fn append_event(
    tx: &mut Transaction<'_, Postgres>,
    (expected_seq, expected_prev): (u64, Sha256Hash),
    req: CommitRequest,
    metered: bool,
) -> Result<CommitReceipt, StoreError> {
    let env = &req.event.envelope;
    let tenant_id = env.tenant_id;
    // The caller pre-signed the envelope; a commit that does not extend
    // the head is a typed refusal, never a silent overwrite.
    if env.seq != expected_seq {
        return Err(StoreError::SeqConflict {
            expected_seq,
            found_seq: env.seq,
        });
    }
    if env.prev_hash != expected_prev {
        return Err(StoreError::PrevHashConflict { seq: env.seq });
    }

    // The tenants row must exist before the journal FK: genesis creates
    // it, named. A tenant that has its row keeps it, name included (the rig
    // also reuses this fold as a no-op); a name another tenant holds is
    // refused by `tenants_name` (ADR-0026 d1).
    if let MetadataFold::TenantGenesis { name } = &req.fold {
        sqlx::query("INSERT INTO tenants (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(tenant_id)
            .bind(name.as_str())
            .execute(&mut **tx)
            .await
            .map_err(|e| name_taken_or_backend(e, "tenants_name", NamedKind::Tenant, name))?;
    }

    let seq = i64::try_from(env.seq).map_err(backend)?;
    sqlx::query(
        "INSERT INTO journal (tenant_id, seq, event_id, event_type, prev_hash, principal, \
         api_key_id, at, payload, signed_bytes, signed_bytes_hash, sig) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, ($8)::timestamptz, $9, $10, $11, $12)",
    )
    .bind(tenant_id)
    .bind(seq)
    .bind(env.event_id)
    .bind(&env.event_type)
    .bind(env.prev_hash.0.to_vec())
    .bind(&env.principal)
    .bind(env.api_key_id)
    .bind(env.at.as_str())
    .bind(&env.payload)
    .bind(&req.event.signed_bytes)
    .bind(req.event.hash.0.to_vec())
    .bind(req.event.sig.to_bytes().to_vec())
    .execute(&mut **tx)
    .await
    .map_err(backend)?;

    sqlx::query(
        "INSERT INTO journal_chain_head (tenant_id, seq, head_hash) VALUES ($1, $2, $3) \
         ON CONFLICT (tenant_id) DO UPDATE SET seq = EXCLUDED.seq, \
         head_hash = EXCLUDED.head_hash",
    )
    .bind(tenant_id)
    .bind(seq)
    .bind(req.event.hash.0.to_vec())
    .execute(&mut **tx)
    .await
    .map_err(backend)?;

    let plan = if metered {
        Some(meter_plan(tx, tenant_id, &req.fold, req.written).await?)
    } else {
        None
    };
    apply_fold(tx, tenant_id, env.event_id, seq, &env.at, req.fold).await?;
    // A meter past its ceiling refuses the commit below, after the
    // idempotency record: an attempt whose key is already recorded is the
    // replay it is, at the limit too (ADR-0071 d3).
    let over_ceiling = match plan {
        Some(plan) => apply_meters(tx, tenant_id, plan, req.ceiling.as_ref()).await?,
        None => None,
    };

    // Outbox: kind 'bronze-event' (0001's drain-kind set); the port
    // row's event id, topic and digest-only payload ride in the jsonb
    // payload (storage.md outbox protocol).
    let outbox_payload = serde_json::json!({
        "event_id": req.outbox.event_id,
        "topic": req.outbox.topic,
        "payload": req.outbox.payload,
    });
    sqlx::query(
        "INSERT INTO outbox (tenant_id, tenant_seq, kind, payload) \
         VALUES ($1, $2, 'bronze-event', $3)",
    )
    .bind(tenant_id)
    .bind(seq)
    .bind(outbox_payload)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;

    if let Some(put) = req.idempotency {
        // request_digest stores the digest string's bytes verbatim —
        // the PoC digest is an opaque comparand (http-api.md idempotency).
        // A scope already recorded is a concurrent attempt that committed
        // first; returning before COMMIT rolls this whole commit back.
        let inserted = sqlx::query(
            "INSERT INTO idempotency (tenant_id, key, request_digest, response, \
             committed_at) VALUES ($1, $2, $3, $4, now()) \
             ON CONFLICT (tenant_id, key) DO NOTHING",
        )
        .bind(put.scope.tenant_id)
        .bind(scope_key(&put.scope))
        .bind(put.record.request_digest.into_bytes())
        .bind(put.record.response)
        .execute(&mut **tx)
        .await
        .map_err(backend)?
        .rows_affected();
        if inserted == 0 {
            return Err(StoreError::IdempotencyRecorded);
        }
    }
    // Returning before COMMIT rolls back the counters with the journal row.
    if let Some(reached) = over_ceiling {
        return Err(StoreError::QuotaReached(reached));
    }

    Ok(CommitReceipt {
        seq: env.seq,
        hash: req.event.hash,
    })
}

// ---------------------------------------------------------------------------
// The port implementation
// ---------------------------------------------------------------------------

impl PgStore {
    /// The usage rollup of one tenant at `at` (ADR-0071 d4): one ledger row
    /// per account for every closed UTC day after its last one, at most 62
    /// days back and from yesterday for an account with none, holding the
    /// stocks as read now, that day's flows and the profile the account is
    /// on: the one an admin assigned, else `unassigned`, the profile an
    /// account of this tenant is on with none assigned, `None` for a tenant
    /// whose accounts are then under no limits (d5). Then the rows past
    /// their retention are deleted. Rows already appended stay as they are,
    /// so two leads append each once. Returns the rows appended.
    pub async fn roll_up_usage(
        &self,
        tenant_id: Uuid,
        at: &EventTime,
        unassigned: Option<&str>,
    ) -> Result<u64, StoreError> {
        let today = oml_app::domain::usage::day_of(at);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let appended = sqlx::query(
            "INSERT INTO usage_daily (tenant_id, user_root, day, profile, memories, text_bytes, \
                                      writes, recalls, keys, connections, recorded_at) \
             SELECT m.tenant_id, m.user_root, d.day::date, coalesce(p.profile, $5), \
                    m.memories, m.text_bytes, \
                    coalesce(a.writes, 0), coalesce(a.recalls, 0), m.keys, m.connections, \
                    ($3)::timestamptz \
               FROM account_meters m \
               CROSS JOIN LATERAL generate_series( \
                   greatest(coalesce((SELECT max(u.day) + 1 FROM usage_daily u \
                                       WHERE u.tenant_id = m.tenant_id \
                                         AND u.user_root = m.user_root), \
                                     ($2)::date - 1), \
                            ($2)::date - $4::int)::timestamp, \
                   (($2)::date - 1)::timestamp, interval '1 day') AS d(day) \
               LEFT JOIN account_profiles p \
                 ON p.tenant_id = m.tenant_id AND p.user_root = m.user_root \
               LEFT JOIN account_activity a \
                 ON a.tenant_id = m.tenant_id AND a.user_root = m.user_root \
                AND a.day = d.day::date \
              WHERE m.tenant_id = $1 \
             ON CONFLICT (tenant_id, user_root, day) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(today)
        .bind(at.as_str())
        .bind(i32::try_from(oml_app::domain::usage::ACTIVITY_RETENTION_DAYS).map_err(backend)?)
        .bind(unassigned)
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        sqlx::query(
            "DELETE FROM usage_daily WHERE tenant_id = $1 \
             AND day < ($2)::date - make_interval(months => $3)",
        )
        .bind(tenant_id)
        .bind(today)
        .bind(i32::try_from(oml_app::domain::usage::LEDGER_RETENTION_MONTHS).map_err(backend)?)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query(
            "DELETE FROM account_activity WHERE tenant_id = $1 AND day < ($2)::date - $3::int",
        )
        .bind(tenant_id)
        .bind(today)
        .bind(i32::try_from(oml_app::domain::usage::ACTIVITY_RETENTION_DAYS).map_err(backend)?)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(appended)
    }

    /// `keyword-all`: the v0.5.0 statements, [`RECALL_FTS`] and
    /// [`RECALL_OWN_HELD`], unchanged, their lists merged under the total
    /// order of hits and cut to `k`, as the core merged them before
    /// (ADR-0049 d7). Every term matched, so `matched` is the terms.
    async fn keyword_all(
        tx: &mut Transaction<'_, Postgres>,
        tenant_id: Uuid,
        scope: &CandidateScope,
        kinds: &[&str],
        query: RetrievalQuery<'_>,
        terms: &[String],
        anchor: &AnchorRead,
    ) -> Result<Vec<Candidate>, StoreError> {
        let k = i64::try_from(query.k).unwrap_or(i64::MAX);
        let fts = RECALL_FTS.replace("/*anchor*/", &anchor.row("m", 6));
        let mut rows = anchor
            .bind(
                sqlx::query(&fts)
                    .bind(tenant_id)
                    .bind(&scope.promoted_nodes)
                    .bind(query.query)
                    .bind(kinds)
                    .bind(k),
            )
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?;
        if let Some(own) = &scope.own_held {
            let held = RECALL_OWN_HELD.replace("/*anchor*/", &anchor.row("m", 7));
            rows.extend(
                anchor
                    .bind(
                        sqlx::query(&held)
                            .bind(tenant_id)
                            .bind(held_authors(own))
                            .bind(&own.nodes)
                            .bind(query.query)
                            .bind(kinds)
                            .bind(k),
                    )
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(backend)?,
            );
        }
        let mut candidates = rows
            .iter()
            .map(candidate_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        for c in &mut candidates {
            c.matched = terms.to_vec();
        }
        hit_order(&mut candidates, |c| (c.score, c.memory_id));
        candidates.truncate(query.k);
        Ok(candidates)
    }
}

/// The most readable anchored rows a filtered recall reads anchor-first:
/// their ids from the anchor index, then their postings by id; above it the
/// filter is joined to the postings on the indexed columns (ADR-0057 d4;
/// #138 measures both).
pub const ANCHOR_FIRST_MAX: usize = 2_000;

/// How a filtered list narrows its candidates (ADR-0057 d4).
#[derive(Debug, Clone, PartialEq, Eq)]
enum AnchorRead {
    /// No filter: the statement is the unfiltered one.
    Unfiltered,
    /// A narrow filter: the readable anchored ids, read by the index.
    Ids(Vec<Uuid>),
    /// A broad filter: the repository by equality and the prefix as a
    /// range on the leakproof `text_pattern_ops` operators, on the indexed
    /// columns of the row.
    Columns {
        repo: Option<String>,
        lo: Option<String>,
        hi: Option<String>,
        /// `origin_kinds` (ADR-0063 d3), empty when unset.
        kinds: Vec<String>,
    },
}

/// The kinds a filter names, as the column stores them.
fn origin_kind_strings(kinds: &[OriginKind]) -> Vec<String> {
    kinds.iter().map(|k| k.as_str().to_owned()).collect()
}

/// The anchor predicate on the row `m` from `$first` on: the repository by
/// equality and the prefix as the range `[lo, hi)`, each present only when
/// bound, so the index serves the serving role under row security
/// (ADR-0057 d4).
fn anchor_columns_sql(m: &str, repo: bool, lo: bool, hi: bool, first: usize) -> String {
    origin_columns_sql(m, repo, lo, hi, false, first)
}

/// [`anchor_columns_sql`] and, when `kinds`, the kind filter's predicate
/// last, `{m}.origin_kind = ANY($n::text[])` (ADR-0063 d3).
fn origin_columns_sql(
    m: &str,
    repo: bool,
    lo: bool,
    hi: bool,
    kinds: bool,
    first: usize,
) -> String {
    let mut n = first;
    let mut sql = String::new();
    if repo {
        sql.push_str(&format!(" AND {m}.anchor_repo = ${n}"));
        n += 1;
    }
    if lo {
        sql.push_str(&format!(" AND {m}.anchor_path ~>=~ ${n}"));
        n += 1;
    }
    if hi {
        sql.push_str(&format!(" AND {m}.anchor_path ~<~ ${n}"));
        n += 1;
    }
    if kinds {
        sql.push_str(&format!(" AND {m}.origin_kind = ANY(${n}::text[])"));
    }
    sql
}

/// The statement a filtered list chooses its plan by: the readable anchored
/// ids of the candidate scope, `$1`..`$5` as the search scope binds them,
/// `$6` the request's now, then the repository, the prefix's two bounds, and the limit. Exported so
/// the suite plans the statement the adapter runs.
#[must_use]
pub fn anchored_ids_sql(repo: bool, prefix: bool) -> String {
    let columns = anchor_columns_sql("m", repo, prefix, prefix, 7);
    let limit = 7 + usize::from(repo) + 2 * usize::from(prefix);
    with_now(
        &format!("SELECT m.id FROM memories m WHERE {SEARCH_SCOPE}{columns} LIMIT ${limit}"),
        6,
    )
}

/// The least string greater than every string that starts with `prefix`:
/// its last character replaced by the next scalar value, a character with
/// none dropped first. UTF-8 orders as code points do, so `[prefix,
/// upper)` is exactly the strings that start with `prefix` under
/// `text_pattern_ops`. `None` when no such string exists.
#[must_use]
pub fn prefix_upper(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        let mut next = u32::from(last) + 1;
        if (0xD800..=0xDFFF).contains(&next) {
            next = 0xE000;
        }
        if let Some(c) = char::from_u32(next) {
            chars.push(c);
            return Some(chars.into_iter().collect());
        }
    }
    None
}

impl AnchorRead {
    /// The predicate on the row `m`, its binds from `$first` on.
    fn row(&self, m: &str, first: usize) -> String {
        match self {
            Self::Unfiltered => String::new(),
            Self::Ids(_) => format!(" AND {m}.id = ANY(${first}::uuid[])"),
            Self::Columns {
                repo,
                lo,
                hi,
                kinds,
            } => origin_columns_sql(
                m,
                repo.is_some(),
                lo.is_some(),
                hi.is_some(),
                !kinds.is_empty(),
                first,
            ),
        }
    }

    /// The predicate on a version id `id` of the postings, its binds from
    /// `$first` on: the ids, or the row's columns by its primary key.
    fn posting(&self, id: &str, first: usize) -> String {
        match self {
            Self::Unfiltered => String::new(),
            Self::Ids(_) => format!(" AND {id} = ANY(${first}::uuid[])"),
            Self::Columns { .. } => format!(
                " AND EXISTS (SELECT 1 FROM memories a WHERE a.tenant_id = $1 AND a.id = {id}{})",
                self.row("a", first)
            ),
        }
    }

    /// Binds the predicate's values, in the order its placeholders number.
    fn bind<'q>(
        &'q self,
        mut q: sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>,
    ) -> sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments> {
        match self {
            Self::Unfiltered => q,
            Self::Ids(ids) => q.bind(ids),
            Self::Columns {
                repo,
                lo,
                hi,
                kinds,
            } => {
                for value in [repo, lo, hi].into_iter().flatten() {
                    q = q.bind(value.as_str());
                }
                if !kinds.is_empty() {
                    q = q.bind(kinds);
                }
                q
            }
        }
    }
}

/// The candidate scope's binds every search statement takes: the tenant,
/// the promoted nodes, the kinds, and the own held keys and their nodes
/// (ADR-0049 d2).
struct ScopeBinds<'a> {
    tenant_id: Uuid,
    nodes: &'a [Uuid],
    kinds: &'a [&'static str],
    /// The held rows' author keys ([`held_authors`]): inside an in-progress
    /// scope it names the server's rule-set mark too, every author's
    /// unflagged held rows scanned under it on `held_nodes` (ADR-0030 d3;
    /// ADR-0065 d15); empty, no held row.
    authors: Option<&'a [Uuid]>,
    held_nodes: &'a [Uuid],
    /// How the origin filters narrow the candidates (ADR-0057 d4).
    anchor: &'a AnchorRead,
    /// The request's now, the least `int8` under `include_expired`: every
    /// statement's expiry bound (proposed ADR-0059 d3).
    expired_at: i64,
    /// A federated recall's reference set: every BM25 statistic and the
    /// pruned words are taken over its served rows alone (ADR-0072 d4.2,
    /// d7).
    reference: Option<&'a [Uuid]>,
}

/// What the `bm25_postings` retriever read: the query's words, its list,
/// how, and the words it pruned (ADR-0052 d6, d11).
struct PostingsRead {
    terms: Vec<String>,
    candidates: Vec<Candidate>,
    access: Access,
    pruned: Vec<String>,
}

impl PgStore {
    /// How a list narrows by the origin filters (ADR-0057 d4): none
    /// without them; with them, the readable anchored ids of the candidate
    /// scope read by the anchor index, up to `anchor_first_max`, and when
    /// more rows match, the filter itself, joined on the indexed columns.
    async fn anchor_read(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        tenant_id: Uuid,
        kinds: &[&'static str],
        scope: &CandidateScope,
        authors: Option<&[Uuid]>,
        held_nodes: &[Uuid],
    ) -> Result<AnchorRead, StoreError> {
        if scope.anchor.is_empty() {
            return Ok(AnchorRead::Unfiltered);
        }
        let lo = scope.anchor.path_prefix.clone();
        let hi = lo.as_deref().and_then(prefix_upper);
        let columns = AnchorRead::Columns {
            repo: scope.anchor.repo.clone(),
            lo,
            hi,
            kinds: origin_kind_strings(&scope.anchor.origin_kinds),
        };
        let sql = with_now(
            &format!(
                "SELECT m.id FROM memories m WHERE {SEARCH_SCOPE}{} LIMIT ${}",
                columns.row("m", 7),
                7 + match &columns {
                    AnchorRead::Columns {
                        repo,
                        lo,
                        hi,
                        kinds,
                    } => {
                        usize::from(repo.is_some())
                            + usize::from(lo.is_some())
                            + usize::from(hi.is_some())
                            + usize::from(!kinds.is_empty())
                    }
                    _ => 0,
                }
            ),
            6,
        );
        let sql = scope_at(&sql, scope.as_of.as_ref());
        let limit = i64::try_from(self.anchor_first_max.saturating_add(1)).unwrap_or(i64::MAX);
        let ids: Vec<Uuid> = columns
            .bind(
                sqlx::query(&sql)
                    .persistent(scope.as_of.is_none())
                    .bind(tenant_id)
                    .bind(&scope.promoted_nodes)
                    .bind(kinds)
                    .bind(authors)
                    .bind(held_nodes)
                    .bind(scope.expired_at.unwrap_or(i64::MIN)),
            )
            .bind(limit)
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?
            .iter()
            .map(|row| row.try_get("id").map_err(backend))
            .collect::<Result<_, _>>()?;
        Ok(if ids.len() <= self.anchor_first_max {
            AnchorRead::Ids(ids)
        } else {
            columns
        })
    }

    /// `bm25`'s scan statement over the scope ([`bm25_sql`]): the fallback
    /// `bm25_postings` runs when it pruned every form (ADR-0052 d6).
    async fn bm25_scan(
        tx: &mut Transaction<'_, Postgres>,
        scope: &ScopeBinds<'_>,
        query: RetrievalQuery<'_>,
    ) -> Result<Vec<Candidate>, StoreError> {
        // Under a reference set its nodes are `$10` and the anchor's binds
        // follow (ADR-0072 d7).
        let sql = match scope.reference {
            None => bm25_sql_within(&scope.anchor.row("m", 10)),
            Some(_) => bm25_reference_sql_within(&scope.anchor.row("m", 11)),
        };
        let mut statement = sqlx::query(&sql)
            .bind(scope.tenant_id)
            .bind(scope.nodes)
            .bind(scope.kinds)
            .bind(scope.authors)
            .bind(scope.held_nodes)
            .bind(query.query)
            .bind(i64::try_from(query.k).unwrap_or(i64::MAX))
            .bind(query.match_mode == MatchMode::All)
            .bind(scope.expired_at);
        if let Some(reference) = scope.reference {
            statement = statement.bind(reference);
        }
        scope
            .anchor
            .bind(statement)
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?
            .iter()
            .map(candidate_from_row)
            .collect()
    }

    /// The `bm25_postings` list (ADR-0052 d4, d5, d6): the readable counts
    /// of every form, read before any posting; the forms pruned by them;
    /// then the kept forms' postings, the best candidates rescored with
    /// every form. Under `match: all` it reads only the rarest word's
    /// forms and keeps the rows holding every word, pruning nothing. When
    /// every form present is pruned, `bm25`'s scan answers and says so.
    async fn bm25_postings(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        scope: &ScopeBinds<'_>,
        query: RetrievalQuery<'_>,
    ) -> Result<PostingsRead, StoreError> {
        let forms: Vec<(String, String)> = sqlx::query_as(&search_forms_sql())
            .bind(query.query)
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?;
        // The kept words are the forms' words, in byte order, as `bm25`'s
        // terms statement reads them.
        let mut terms: Vec<String> = forms.iter().map(|(w, _)| w.clone()).collect();
        terms.dedup();
        let empty = |terms: Vec<String>| PostingsRead {
            terms,
            candidates: Vec::new(),
            access: Access::Postings,
            pruned: Vec::new(),
        };
        if forms.is_empty() {
            return Ok(empty(terms));
        }
        let memory_kinds: Vec<i16> = scope
            .kinds
            .iter()
            .filter_map(|k| match *k {
                "memory" => Some(1_i16),
                "derived_fact" => Some(2_i16),
                _ => None,
            })
            .collect();
        let tokens: Vec<i64> = {
            let mut t: Vec<i64> = forms.iter().map(|(_, f)| token_id(f)).collect();
            t.sort_unstable();
            t.dedup();
            t
        };
        // The readable rows: the served versions' counts on the readable
        // nodes, and the own held rows read by the author index; under a
        // reference set, its served versions' counts alone (ADR-0072 d4.2).
        let counted_nodes = scope.reference.unwrap_or(scope.nodes);
        let counted: Vec<(Option<i64>, i64, i64)> = sqlx::query_as(POSTINGS_COUNTS)
            .bind(scope.tenant_id)
            .bind(counted_nodes)
            .bind(&memory_kinds)
            .bind(&tokens)
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?;
        let mut rows = 0_u64;
        let mut total_len = 0_u64;
        let mut df: BTreeMap<i64, u64> = tokens.iter().map(|t| (*t, 0)).collect();
        for (token, n, len) in counted {
            let n = u64::try_from(n).map_err(backend)?;
            match token {
                None => {
                    rows = n;
                    total_len = u64::try_from(len).map_err(backend)?;
                }
                Some(token) => *df.entry(token).or_default() += n,
            }
        }
        // An expired version counts in no statistic, as in bm25's scan: the
        // per-workarea counts hold every served version, so the expired ones
        // on the readable nodes are taken out by id (proposed ADR-0059 d3).
        let expired: Vec<(Uuid, i32)> = sqlx::query_as(POSTINGS_EXPIRED)
            .bind(scope.tenant_id)
            .bind(counted_nodes)
            .bind(scope.kinds)
            .bind(scope.expired_at)
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?;
        if !expired.is_empty() {
            rows = rows.saturating_sub(expired.len() as u64);
            total_len = total_len.saturating_sub(
                expired
                    .iter()
                    .map(|(_, len)| u64::try_from(*len).unwrap_or(0))
                    .sum::<u64>(),
            );
            let ids: Vec<Uuid> = expired.iter().map(|(id, _)| *id).collect();
            let gone: Vec<(i64, i64)> = sqlx::query_as(POSTINGS_OWN_HELD_DF)
                .bind(scope.tenant_id)
                .bind(&ids)
                .bind(&tokens)
                .fetch_all(&mut **tx)
                .await
                .map_err(backend)?;
            for (token, n) in gone {
                if let Some(df) = df.get_mut(&token) {
                    *df = df.saturating_sub(u64::try_from(n).map_err(backend)?);
                }
            }
        }
        let held: Vec<(Uuid, i32)> =
            if scope.authors.is_some_and(<[Uuid]>::is_empty) || scope.held_nodes.is_empty() {
                Vec::new()
            } else {
                sqlx::query_as(POSTINGS_OWN_HELD)
                    .bind(scope.tenant_id)
                    .bind(scope.authors)
                    .bind(scope.held_nodes)
                    .bind(scope.kinds)
                    .bind(scope.expired_at)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(backend)?
            };
        // Under a reference set the own held rows are scored and never
        // counted: R holds served rows alone (ADR-0072 d7).
        if scope.reference.is_none() {
            rows += held.len() as u64;
            total_len += held
                .iter()
                .map(|(_, len)| u64::try_from(*len).unwrap_or(0))
                .sum::<u64>();
        }
        if rows == 0 {
            return Ok(empty(terms));
        }
        let held_ids: Vec<Uuid> = held.iter().map(|(id, _)| *id).collect();
        if !held_ids.is_empty() && scope.reference.is_none() {
            let held_counted: Vec<(i64, i64)> = sqlx::query_as(POSTINGS_OWN_HELD_DF)
                .bind(scope.tenant_id)
                .bind(&held_ids)
                .bind(&tokens)
                .fetch_all(&mut **tx)
                .await
                .map_err(backend)?;
            for (token, n) in held_counted {
                *df.entry(token).or_default() += u64::try_from(n).map_err(backend)?;
            }
        }
        // Each word's present forms, and the words the query has.
        let mut words: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
        for (w, f) in &forms {
            let present = words.entry(w.as_str()).or_default();
            let t = token_id(f);
            if df.get(&t).copied().unwrap_or(0) > 0 && !present.contains(&t) {
                present.push(t);
            }
        }
        let all = query.match_mode == MatchMode::All;
        let (generators, depth, pruned_words) = if all {
            // Every answer holds the rarest word, so its forms generate the
            // candidates and nothing is pruned (ADR-0052 d4).
            if words.values().any(Vec::is_empty) {
                return Ok(empty(terms));
            }
            let Some(rarest) = words
                .values()
                .min_by_key(|ts| ts.iter().map(|t| df[t]).sum::<u64>())
            else {
                return Ok(empty(terms));
            };
            if rarest.iter().map(|t| df[t]).sum::<u64>() > self.prune.posting_budget {
                return Ok(PostingsRead {
                    terms,
                    candidates: Self::bm25_scan(tx, scope, query).await?,
                    access: Access::Scan,
                    pruned: Vec::new(),
                });
            }
            (rarest.clone(), None, Vec::new())
        } else {
            let entries: Vec<(&str, Vec<(i64, u64)>)> = words
                .iter()
                .map(|(w, ts)| (*w, ts.iter().map(|t| (*t, df[t])).collect()))
                .collect();
            let forms: Vec<Vec<(i64, u64)>> = entries.iter().map(|(_, f)| f.clone()).collect();
            let pruned = self.prune.pruned(&forms, rows);
            let mut kept: Vec<i64> = entries
                .iter()
                .enumerate()
                .filter(|(i, _)| !pruned.contains(i))
                .flat_map(|(_, (_, f))| f.iter().map(|(t, _)| *t))
                .collect();
            kept.sort_unstable();
            kept.dedup();
            if kept.is_empty() && !pruned.is_empty() {
                return Ok(PostingsRead {
                    terms,
                    candidates: Self::bm25_scan(tx, scope, query).await?,
                    access: Access::Scan,
                    pruned: Vec::new(),
                });
            }
            let pruned_words: Vec<String> =
                pruned.iter().map(|i| entries[*i].0.to_owned()).collect();
            let depth = if pruned.is_empty() {
                Some(query.k)
            } else if *scope.anchor == AnchorRead::Unfiltered {
                Some(query.k.saturating_mul(RESCORE_DEPTH))
            } else {
                // A filtered list rescores every row its kept forms
                // generate inside the filter, at most the posting budget,
                // so it ranks as the unfiltered list filtered then cut
                // (ADR-0057 d4).
                None
            };
            (kept, depth, pruned_words)
        };
        if generators.is_empty() {
            return Ok(empty(terms));
        }
        // Every (word, form) a row may hold, with the form's readable
        // frequency; a form no readable row holds scores nothing.
        let mut q_words = Vec::new();
        let mut q_tokens = Vec::new();
        let mut q_df = Vec::new();
        for (w, f) in &forms {
            let t = token_id(f);
            let n = df.get(&t).copied().unwrap_or(0);
            if n > 0 {
                q_words.push(w.clone());
                q_tokens.push(t);
                #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
                q_df.push(n as f64);
            }
        }
        #[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
        let (n, avgdl) = (rows as f64, (total_len as f64 / rows as f64).max(1.0));
        let sql = bm25_postings_sql_within(&scope.anchor.posting("p.memory_id", 16));
        let candidates = scope
            .anchor
            .bind(
                sqlx::query(&sql)
                    .bind(scope.tenant_id)
                    .bind(scope.nodes)
                    .bind(&memory_kinds)
                    .bind(&held_ids)
                    .bind(&q_words)
                    .bind(&q_tokens)
                    .bind(&q_df)
                    .bind(n)
                    .bind(avgdl)
                    .bind(&generators)
                    .bind(depth.map(|d| i64::try_from(d).unwrap_or(i64::MAX)))
                    .bind(all)
                    .bind(i64::try_from(words.len()).unwrap_or(i64::MAX))
                    .bind(i64::try_from(query.k).unwrap_or(i64::MAX))
                    .bind(scope.expired_at),
            )
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?
            .iter()
            .map(candidate_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PostingsRead {
            terms,
            candidates,
            access: Access::Postings,
            pruned: pruned_words,
        })
    }
}

impl Retriever for PgStore {
    fn retrievers(&self) -> Vec<RetrieverKind> {
        let mut served = vec![
            RetrieverKind::KeywordAll,
            RetrieverKind::KeywordAny,
            RetrieverKind::Bm25,
            RetrieverKind::Bm25Postings,
            RetrieverKind::Sparse,
            RetrieverKind::Time,
        ];
        if self.trigram.load(std::sync::atomic::Ordering::SeqCst) {
            served.push(RetrieverKind::Trigram);
        }
        served
    }

    async fn retrieve(
        &self,
        tenant_id: Uuid,
        kind: RetrieverKind,
        scope: &CandidateScope,
        query: RetrievalQuery<'_>,
    ) -> Result<Retrieved, StoreError> {
        if !self.retrievers().contains(&kind) {
            return Err(StoreError::Backend(format!(
                "this store serves no {} retriever (pg_trgm absent)",
                kind.as_str()
            )));
        }
        // At an instant the postings lists, whose counts and states are
        // today's served versions, are read by bm25's scan over the scope at
        // the instant, and keyword-all's v0.5.0 statements by keyword-any's
        // with every word (ADR-0059 d18).
        let as_of = scope.as_of.as_ref();
        let every_word = as_of.is_some() && kind == RetrieverKind::KeywordAll;
        let kind = match (as_of, kind) {
            (Some(_), RetrieverKind::Bm25Postings | RetrieverKind::Sparse) => RetrieverKind::Bm25,
            (Some(_), RetrieverKind::KeywordAll) => RetrieverKind::KeywordAny,
            (_, kind) => kind,
        };
        let kinds = hit_kinds(&scope.kinds);
        let (authors, held_nodes): (Option<Vec<Uuid>>, Vec<Uuid>) = scope
            .own_held
            .as_ref()
            .map_or((Some(Vec::new()), Vec::new()), |own| {
                (Some(held_authors(own)), own.nodes.clone())
            });
        let k = i64::try_from(query.k).unwrap_or(i64::MAX);
        let all = every_word || query.match_mode == MatchMode::All;
        let mut tx = self.pool.begin().await.map_err(backend)?;
        // One snapshot for every statement of the list, so counts and
        // postings read together; nothing here writes (review F6).
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The expiry cut every statement but keyword-all's binds (proposed
        // ADR-0059 d3): the request's now, or the least int8 under
        // include_expired, so the cut keeps every row.
        let expired_at = scope.expired_at.unwrap_or(i64::MIN);
        // The terms as the retriever reads them: the query's `simple`
        // lexemes, of the normalised query for bm25, bm25_postings and
        // trigram.
        // `bm25_postings` reads them with its forms, one statement fewer.
        let terms_sql = match kind {
            RetrieverKind::KeywordAll | RetrieverKind::KeywordAny => {
                Some("SELECT tsvector_to_array(to_tsvector('simple', $1))".to_owned())
            }
            // A sparse list reads the query's words as `bm25` does, so the
            // answer's terms and did_you_mean never hold a word piece
            // (ADR-0052 d17; L2 on review).
            RetrieverKind::Bm25 | RetrieverKind::Trigram | RetrieverKind::Sparse => {
                Some(search_terms_sql())
            }
            RetrieverKind::Bm25Postings | RetrieverKind::Time => None,
        };
        let mut terms: Vec<String> = match terms_sql {
            Some(sql) => sqlx::query_scalar(&sql)
                .bind(query.query)
                .fetch_one(&mut *tx)
                .await
                .map_err(backend)?,
            None => Vec::new(),
        };
        // The origin filters are a predicate of the candidate statement,
        // before scoring and the cut, so a filtered list is the unfiltered
        // one filtered then cut (ADR-0057 d4).
        let anchor = self
            .anchor_read(
                &mut tx,
                tenant_id,
                &kinds,
                scope,
                authors.as_deref(),
                &held_nodes,
            )
            .await?;
        let scoped = ScopeBinds {
            tenant_id,
            nodes: &scope.promoted_nodes,
            kinds: &kinds,
            authors: authors.as_deref(),
            held_nodes: &held_nodes,
            anchor: &anchor,
            expired_at,
            reference: scope.reference.as_deref(),
        };
        let mut access = Access::Scan;
        let mut pruned = Vec::new();
        let candidates = match kind {
            RetrieverKind::KeywordAll => {
                Self::keyword_all(&mut tx, tenant_id, scope, &kinds, query, &terms, &anchor).await?
            }
            RetrieverKind::Trigram if all => Vec::new(),
            // The window's rows, by leakproof int8 comparisons; no window,
            // no list (proposed ADR-0059 d7).
            RetrieverKind::Time => match query.window {
                None => Vec::new(),
                Some(window) => sqlx::query(&scope_at(&time_sql(), as_of))
                    .persistent(as_of.is_none())
                    .bind(tenant_id)
                    .bind(&scope.promoted_nodes)
                    .bind(&kinds)
                    .bind(&authors)
                    .bind(&held_nodes)
                    .bind(window.from_us)
                    .bind(window.until_us)
                    .bind(k)
                    .bind(expired_at)
                    .bind(query.within)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(backend)?
                    .iter()
                    .map(candidate_from_row)
                    .collect::<Result<Vec<_>, _>>()?,
            },
            RetrieverKind::Bm25Postings => {
                let read = self.bm25_postings(&mut tx, &scoped, query).await?;
                terms = read.terms;
                access = read.access;
                pruned = read.pruned;
                read.candidates
            }
            RetrieverKind::Sparse => {
                access = Access::Postings;
                let mut list = self.sparse_list(&mut tx, &scoped, query).await?;
                for c in &mut list {
                    c.matched.retain(|piece| terms.contains(piece));
                }
                list
            }
            // A federated recall's `bm25` takes its statistics over the
            // reference set (ADR-0072 d7).
            RetrieverKind::Bm25 if scope.reference.is_some() => {
                Self::bm25_scan(&mut tx, &scoped, query).await?
            }
            RetrieverKind::KeywordAny | RetrieverKind::Bm25 | RetrieverKind::Trigram => {
                if kind == RetrieverKind::Trigram {
                    sqlx::query("SET LOCAL pg_trgm.word_similarity_threshold = 0.3")
                        .execute(&mut *tx)
                        .await
                        .map_err(backend)?;
                }
                let sql = scope_at(
                    &match kind {
                        RetrieverKind::KeywordAny => keyword_any_sql(&anchor.row("m", 10)),
                        RetrieverKind::Bm25 => bm25_sql_within(&anchor.row("m", 10)),
                        _ => trigram_sql(&anchor.row("m", 9)),
                    },
                    as_of,
                );
                // A statement at an instant is planned once: its literal
                // instant would fill the statement cache (ADR-0059 d20).
                let mut statement = sqlx::query(&sql)
                    .persistent(as_of.is_none())
                    .bind(tenant_id)
                    .bind(&scope.promoted_nodes)
                    .bind(&kinds)
                    .bind(&authors)
                    .bind(&held_nodes)
                    .bind(query.query)
                    .bind(k);
                if kind != RetrieverKind::Trigram {
                    statement = statement.bind(all);
                }
                statement = statement.bind(expired_at);
                anchor
                    .bind(statement)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(backend)?
                    .iter()
                    .map(candidate_from_row)
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        tx.commit().await.map_err(backend)?;
        Ok(Retrieved {
            terms,
            candidates,
            access,
            pruned,
        })
    }

    async fn suggest(
        &self,
        tenant_id: Uuid,
        scope: &CandidateScope,
        words: &[String],
        limit: usize,
    ) -> Result<Vec<String>, StoreError> {
        if !self.trigram.load(std::sync::atomic::Ordering::SeqCst) || words.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let found: Vec<String> = sqlx::query_scalar(SUGGEST)
            .bind(tenant_id)
            .bind(&scope.promoted_nodes)
            .bind(words)
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(found)
    }

    async fn similar_candidates(
        &self,
        tenant_id: Uuid,
        scope: &CandidateScope,
        keys: &[i64],
        limit: usize,
    ) -> Result<Vec<Uuid>, StoreError> {
        let (authors, held_nodes) = scope
            .own_held
            .as_ref()
            .map_or((Vec::new(), Vec::new()), |own| {
                (own.author_key_ids.clone(), own.nodes.clone())
            });
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let found: Vec<Uuid> = sqlx::query_scalar(SIMILAR_CANDIDATES)
            .bind(tenant_id)
            .bind(keys)
            .bind(&scope.promoted_nodes)
            .bind(&authors)
            .bind(&held_nodes)
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(found)
    }

    async fn valid_time_counts(
        &self,
        tenant_id: Uuid,
        now_us: i64,
    ) -> Result<Option<oml_app::search::ValidTimeCounts>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let (declared, expired, untimed): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE m.is_tip AND NOT m.retirement \
                                       AND m.valid_from_us IS NOT NULL)::int8, \
                    count(*) FILTER (WHERE m.is_tip AND NOT m.retirement \
                                       AND m.valid_until_us <= $2)::int8, \
                    count(*) FILTER (WHERE m.promoted_at IS NULL)::int8 \
             FROM memories m WHERE m.tenant_id = $1 AND m.status = 'promoted'",
        )
        .bind(tenant_id)
        .bind(now_us)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(Some(oml_app::search::ValidTimeCounts {
            declared: u64::try_from(declared).map_err(backend)?,
            expired: u64::try_from(expired).map_err(backend)?,
            untimed: u64::try_from(untimed).map_err(backend)?,
        }))
    }

    async fn postings_health(&self, tenant_id: Uuid) -> Result<Option<PostingsHealth>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(POSTINGS_HEALTH)
            .bind(tenant_id)
            .bind(postings_partition(tenant_id))
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
        let queued: (i64, i64, i64, bool) = sqlx::query_as(SPARSE_QUEUE_HEALTH)
            .bind(tenant_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
        let similar: (bool, i64, i64) = sqlx::query_as(SIMILAR_HEALTH)
            .bind(tenant_id)
            .bind(similar_partition(tenant_id))
            .bind(SIMILAR_HEALTH_SAMPLE)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let count = |column: &str| -> Result<u64, StoreError> {
            let value: i64 = row.try_get(column).map_err(backend)?;
            u64::try_from(value).map_err(backend)
        };
        Ok(Some(PostingsHealth {
            partition: row.try_get("partition").map_err(backend)?,
            unfilled: count("unfilled")?,
            served_behind: count("served_behind")?,
            workarea_drift: count("workarea_drift")?,
            term_drift: count("term_drift")?,
            terms_checked: count("terms_checked")?,
            sparse_backlog: u64::try_from(queued.0).map_err(backend)?,
            sparse_failing: u64::try_from(queued.1).map_err(backend)?,
            sparse_parked: u64::try_from(queued.2).map_err(backend)?,
            sparse_enabled: queued.3,
            similar_partition: similar.0,
            similar_unfilled: u64::try_from(similar.1).map_err(backend)?,
            similar_drift: u64::try_from(similar.2).map_err(backend)?,
        }))
    }

    async fn sparse_tenants(&self) -> Result<Vec<SparseTenant>, StoreError> {
        // One statement for every tenant, under the transaction-local
        // marker that lets the serving role read ids and times across
        // tenants (0003, 0022): an idle pass costs this read alone.
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        let rows: Vec<(Uuid, bool)> = sqlx::query_as(SPARSE_TENANTS)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(|(tenant_id, backfill)| SparseTenant {
                tenant_id,
                backfill,
            })
            .collect())
    }

    async fn sparse_backfill(
        &self,
        tenant_id: Uuid,
        limit: usize,
    ) -> Result<(u64, bool), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // Turning the state on races a write in flight: its trigger read the
        // state off, and the step's read, taken before it commits, misses
        // it, so neither queues it. SHARE on memories waits for every write
        // in flight and holds new ones until this commits, when their
        // triggers read the state on; a state already on takes no lock
        // (ADR-0052 d15).
        let on: Option<bool> =
            sqlx::query_scalar("SELECT enabled FROM search_sparse_state WHERE tenant_id = $1")
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        if on != Some(true) {
            sqlx::query("LOCK TABLE memories IN SHARE MODE")
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
        }
        // On, from now: a state turned on again starts its backfill over.
        let (after,): (Option<Uuid>,) = sqlx::query_as(
            "INSERT INTO search_sparse_state (tenant_id, enabled) VALUES ($1, true) \
             ON CONFLICT (tenant_id) DO UPDATE SET enabled = true, \
                 backfill_after = CASE WHEN search_sparse_state.enabled \
                                       THEN search_sparse_state.backfill_after END, \
                 backfilled_at = CASE WHEN search_sparse_state.enabled \
                                      THEN search_sparse_state.backfilled_at END \
             RETURNING backfill_after",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        let (last, read, queued): (Option<Uuid>, i64, i64) = sqlx::query_as(SPARSE_BACKFILL_STEP)
            .bind(tenant_id)
            .bind(after)
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
        let done = usize::try_from(read).unwrap_or(0) < limit;
        sqlx::query(
            "UPDATE search_sparse_state SET backfill_after = COALESCE($2, backfill_after), \
                    backfilled_at = CASE WHEN $3 THEN clock_timestamp() END \
              WHERE tenant_id = $1",
        )
        .bind(tenant_id)
        .bind(last)
        .bind(done)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok((u64::try_from(queued).map_err(backend)?, done))
    }

    async fn sparse_off(&self) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        let on: Vec<Uuid> = sqlx::query_scalar(
            "SELECT s.tenant_id FROM search_sparse_state s WHERE s.enabled \
             UNION SELECT DISTINCT q.tenant_id FROM search_sparse_queue q",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        for tenant_id in &on {
            let mut tx = self.pool.begin().await.map_err(backend)?;
            set_tenant(&mut tx, *tenant_id).await?;
            sqlx::query(
                "UPDATE search_sparse_state SET enabled = false, backfill_after = NULL, \
                        backfilled_at = NULL WHERE tenant_id = $1",
            )
            .bind(tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
            sqlx::query("DELETE FROM search_sparse_queue WHERE tenant_id = $1")
                .bind(tenant_id)
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
        }
        Ok(on.len() as u64)
    }

    async fn sparse_pending(
        &self,
        tenant_id: Uuid,
        limit: usize,
    ) -> Result<Vec<SparsePending>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows: Vec<(Uuid, String, i64)> = sqlx::query_as(SPARSE_PENDING)
            .bind(tenant_id)
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .bind(SPARSE_LEASE_SECS as f64)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(|(memory_id, claim, enqueued_at_us)| SparsePending {
                memory_id,
                claim,
                enqueued_at_us,
            })
            .collect())
    }

    async fn sparse_write(
        &self,
        tenant_id: Uuid,
        pending: &SparsePending,
        postings: &[SparsePosting],
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let mut version: Option<(Uuid, Option<i16>, Option<i16>)> = None;
        for step in SPARSE_WRITE_ORDER {
            match step {
                // The entry read, and only it: one enqueued again since is
                // left for the next pass, so the newer claim is the one
                // encoded.
                SparseWriteStep::TakeEntry => {
                    let taken: Option<i32> = sqlx::query_scalar(SPARSE_TAKE_ENTRY)
                        .bind(tenant_id)
                        .bind(pending.memory_id)
                        .bind(pending.enqueued_at_us)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(backend)?;
                    if taken.is_none() {
                        tx.rollback().await.map_err(backend)?;
                        return Ok(false);
                    }
                }
                // The version's state now, held FOR SHARE so a flip waits
                // for this write or precedes it (ADR-0052 d8, d15).
                SparseWriteStep::HoldVersion => {
                    version = sqlx::query_as(SPARSE_HOLD_VERSION)
                        .bind(tenant_id)
                        .bind(pending.memory_id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(backend)?;
                }
            }
        }
        let Some((workarea_id, Some(memory_kind), Some(state))) = version else {
            tx.commit().await.map_err(backend)?;
            return Ok(true);
        };
        sqlx::query(
            "DELETE FROM search_postings WHERE tenant_id = $1 AND memory_id = $2 AND kind = 2",
        )
        .bind(tenant_id)
        .bind(pending.memory_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let tokens: Vec<i64> = postings.iter().map(|p| i64::from(p.token_id)).collect();
        let weights: Vec<f32> = postings.iter().map(|p| p.weight).collect();
        let in_text: Vec<i32> = postings.iter().map(|p| i32::from(p.in_text)).collect();
        sqlx::query(
            "INSERT INTO search_postings (tenant_id, memory_id, workarea_id, token_id, weight, \
                                          doc_len, kind, memory_kind, state) \
             SELECT $1, $2, $3, t, w, d, 2, $4, $5 \
               FROM unnest($6::int8[], $7::float4[], $8::int4[]) AS x(t, w, d)",
        )
        .bind(tenant_id)
        .bind(pending.memory_id)
        .bind(workarea_id)
        .bind(memory_kind)
        .bind(state)
        .bind(&tokens)
        .bind(&weights)
        .bind(&in_text)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(true)
    }

    async fn sparse_failed(
        &self,
        tenant_id: Uuid,
        pending: &SparsePending,
        reason: &str,
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let parked: Option<bool> = sqlx::query_scalar(
            "UPDATE search_sparse_queue \
                SET attempts = attempts + 1, last_error = left($3, 500), \
                    next_at = clock_timestamp() \
                      + make_interval(secs => least(power(2, least(attempts + 1, 20)), $4)), \
                    parked_at = CASE WHEN attempts + 1 >= $5 THEN clock_timestamp() END \
              WHERE tenant_id = $1 AND memory_id = $2 \
             RETURNING parked_at IS NOT NULL",
        )
        .bind(tenant_id)
        .bind(pending.memory_id)
        .bind(reason)
        .bind(SPARSE_RETRY_MAX_SECS as f64)
        .bind(SPARSE_MAX_ATTEMPTS)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(parked.unwrap_or(false))
    }

    async fn sparse_backlog(&self) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM search_sparse_queue WHERE parked_at IS NULL")
                .fetch_one(&mut *tx)
                .await
                .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        u64::try_from(n).map_err(backend)
    }
}

impl PgStore {
    /// The `sparse` list (ADR-0052 d16): the own held rows read as
    /// `bm25_postings` reads them, then [`sparse_sql`].
    async fn sparse_list(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        scope: &ScopeBinds<'_>,
        query: RetrievalQuery<'_>,
    ) -> Result<Vec<Candidate>, StoreError> {
        if query.sparse.is_empty()
            || (scope.nodes.is_empty() && scope.authors.is_some_and(<[Uuid]>::is_empty))
        {
            return Ok(Vec::new());
        }
        let memory_kinds: Vec<i16> = scope
            .kinds
            .iter()
            .filter_map(|k| match *k {
                "memory" => Some(1_i16),
                "derived_fact" => Some(2_i16),
                _ => None,
            })
            .collect();
        let held: Vec<Uuid> =
            if scope.authors.is_some_and(<[Uuid]>::is_empty) || scope.held_nodes.is_empty() {
                Vec::new()
            } else {
                sqlx::query_as::<_, (Uuid, i32)>(POSTINGS_OWN_HELD)
                    .bind(scope.tenant_id)
                    .bind(scope.authors)
                    .bind(scope.held_nodes)
                    .bind(scope.kinds)
                    .bind(scope.expired_at)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(backend)?
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect()
            };
        let tokens: Vec<i64> = query.sparse.iter().map(|t| i64::from(t.token_id)).collect();
        let weights: Vec<f64> = query.sparse.iter().map(|t| f64::from(t.weight)).collect();
        let pieces: Vec<&str> = query.sparse.iter().map(|t| t.piece.as_str()).collect();
        let sql = sparse_sql_within(&scope.anchor.posting("p.memory_id", 12));
        scope
            .anchor
            .bind(
                sqlx::query(&sql)
                    .bind(scope.tenant_id)
                    .bind(scope.nodes)
                    .bind(&memory_kinds)
                    .bind(&held)
                    .bind(&tokens)
                    .bind(&weights)
                    .bind(&pieces)
                    .bind(query.match_mode == MatchMode::All)
                    .bind(i64::try_from(tokens.len()).unwrap_or(i64::MAX))
                    .bind(i64::try_from(query.k).unwrap_or(i64::MAX))
                    .bind(scope.expired_at),
            )
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?
            .iter()
            .map(sparse_candidate_from_row)
            .collect()
    }
}

/// The drain's guarded take of a queue entry: `$1` tenant, `$2` version,
/// `$3` the `enqueued_at` it read, in microseconds (ADR-0052 d15).
pub const SPARSE_TAKE_ENTRY: &str = "DELETE FROM search_sparse_queue \
      WHERE tenant_id = $1 AND memory_id = $2 \
        AND (extract(epoch FROM enqueued_at) * 1000000)::int8 = $3 \
     RETURNING 1";

/// The drain's read of the version it writes, held `FOR SHARE`: `$1`
/// tenant, `$2` version (ADR-0052 d15).
pub const SPARSE_HOLD_VERSION: &str = "SELECT m.workarea_id, oml_search_memory_kind(m.kind), \
            oml_search_state(m.status, m.is_tip, m.retirement, m.kind) \
       FROM memories m WHERE m.tenant_id = $1 AND m.id = $2 FOR SHARE";

/// The two locking steps of a drain's write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SparseWriteStep {
    /// [`SPARSE_TAKE_ENTRY`].
    TakeEntry,
    /// [`SPARSE_HOLD_VERSION`].
    HoldVersion,
}

/// The order a drain's write takes its locks in; exported so the suite
/// interleaves a version's removal with exactly this order.
/// The trigger's order: the version's row first (an UPDATE or DELETE of
/// it holds it before its trigger touches the queue), then the queue
/// entry, so the drain and a version's removal never wait on each other in
/// opposite orders (ADR-0052 d15; MEDIUM-1 on review).
pub const SPARSE_WRITE_ORDER: [SparseWriteStep; 2] =
    [SparseWriteStep::HoldVersion, SparseWriteStep::TakeEntry];

/// The longest a failed encode waits before its next try (ADR-0052 d15).
pub const SPARSE_RETRY_MAX_SECS: i64 = 3_600;

/// The attempts after which a failing entry is parked, a dead letter kept
/// with its reason for doctor and never taken again until its version is
/// written again (ADR-0052 d15; HIGH-2 on review).
pub const SPARSE_MAX_ATTEMPTS: i32 = 5;

/// How long a pass's lease on the entries it took lasts: another lead's
/// pass skips them meanwhile (ADR-0052 d15; L1 on review).
pub const SPARSE_LEASE_SECS: i64 = 300;

impl PgStore {
    /// A query's counts from per-(node, form) counts (ADR-0072 d4.2): each
    /// word's frequency is its most frequent form's, and the words a
    /// pruning pipeline drops are decided over the nodes' sums as
    /// `bm25_postings` decides them (ADR-0052 d6). Shared by the reference
    /// counts and the widen signal's lookup, so the two cannot disagree.
    fn counts_of_forms(
        &self,
        forms: &[(String, String)],
        rows: &BTreeMap<Uuid, u64>,
        df: &BTreeMap<(Uuid, i64), u64>,
        prunes: bool,
    ) -> oml_app::federation::ReferenceCounts {
        let mut words: Vec<String> = forms.iter().map(|(w, _)| w.clone()).collect();
        words.dedup();
        // Each word's forms, in the words' order.
        let word_forms: Vec<Vec<i64>> = words
            .iter()
            .map(|w| {
                let mut f: Vec<i64> = forms
                    .iter()
                    .filter(|(fw, _)| fw == w)
                    .map(|(_, f)| token_id(f))
                    .collect();
                f.sort_unstable();
                f.dedup();
                f
            })
            .collect();
        let nodes: BTreeMap<Uuid, oml_app::domain::federation::SourceCounts> = rows
            .iter()
            .map(|(node, n)| {
                (
                    *node,
                    oml_app::domain::federation::SourceCounts {
                        rows: *n,
                        df: word_forms
                            .iter()
                            .map(|fs| {
                                fs.iter()
                                    .map(|t| df.get(&(*node, *t)).copied().unwrap_or(0))
                                    .max()
                                    .unwrap_or(0)
                            })
                            .collect(),
                    },
                )
            })
            .collect();
        // The words a pruning pipeline drops, decided over the set's sums
        // as `bm25_postings` decides them (ADR-0052 d6; ADR-0072 d4.2).
        let pruned = if prunes {
            let total_rows: u64 = rows.values().sum();
            let entries: Vec<Vec<(i64, u64)>> = word_forms
                .iter()
                .map(|fs| {
                    fs.iter()
                        .map(|t| {
                            let n: u64 = df
                                .iter()
                                .filter(|((_, tok), _)| tok == t)
                                .map(|(_, n)| *n)
                                .sum();
                            (*t, n)
                        })
                        .filter(|(_, n)| *n > 0)
                        .collect()
                })
                .collect();
            self.prune
                .pruned(&entries, total_rows)
                .into_iter()
                .map(|i| words[i].clone())
                .collect()
        } else {
            Vec::new()
        };
        oml_app::federation::ReferenceCounts {
            words,
            pruned,
            nodes,
        }
    }
}

impl JournalStore for PgStore {
    async fn user_usage(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        at: &EventTime,
    ) -> Result<oml_app::types::UserUsage, StoreError> {
        let month = Month::of(at);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT coalesce(m.memories, 0), coalesce(m.text_bytes, 0), \
                    coalesce(a.writes, 0), coalesce(a.recalls, 0), \
                    coalesce(m.keys, 0), coalesce(m.connections, 0) \
               FROM (SELECT 1) one \
               LEFT JOIN account_meters m ON m.tenant_id = $1 AND m.user_root = $2 \
               LEFT JOIN (SELECT sum(writes)::bigint AS writes, sum(recalls)::bigint AS recalls \
                            FROM account_activity \
                           WHERE tenant_id = $1 AND user_root = $2 \
                             AND day >= ($3)::date AND day < ($4)::date) a ON true",
        )
        .bind(tenant_id)
        .bind(user_root)
        .bind(month.first_day())
        .bind(month.next().first_day())
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(usage_of(row))
    }

    async fn account_profile(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> Result<Option<String>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let profile = sqlx::query_scalar(
            "SELECT profile FROM account_profiles WHERE tenant_id = $1 AND user_root = $2",
        )
        .bind(tenant_id)
        .bind(user_root)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(profile)
    }

    async fn meter_recalls(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        day: &str,
        count: u64,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The account's counter row, so the ledger rolls a person up who
        // has only read (ADR-0071 d4).
        sqlx::query(
            "INSERT INTO account_meters (tenant_id, user_root) VALUES ($1, $2) \
             ON CONFLICT (tenant_id, user_root) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(user_root)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query(
            "INSERT INTO account_activity (tenant_id, user_root, day, recalls) \
             VALUES ($1, $2, ($3)::date, $4) ON CONFLICT (tenant_id, user_root, day) \
             DO UPDATE SET recalls = account_activity.recalls + EXCLUDED.recalls",
        )
        .bind(tenant_id)
        .bind(user_root)
        .bind(day)
        .bind(i64::try_from(count).map_err(backend)?)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)
    }

    async fn usage_accounts(
        &self,
        tenant_id: Uuid,
        month: Month,
    ) -> Result<Vec<oml_app::types::AccountCounters>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows: Vec<CounterRow> = sqlx::query_as(
            "SELECT m.user_root, p.profile, m.memories, m.text_bytes, \
                    coalesce(a.writes, 0), coalesce(a.recalls, 0), m.keys, m.connections \
               FROM account_meters m \
               LEFT JOIN account_profiles p \
                 ON p.tenant_id = m.tenant_id AND p.user_root = m.user_root \
               LEFT JOIN (SELECT user_root, sum(writes)::bigint AS writes, \
                                 sum(recalls)::bigint AS recalls \
                            FROM account_activity \
                           WHERE tenant_id = $1 AND day >= ($2)::date AND day < ($3)::date \
                           GROUP BY user_root) a ON a.user_root = m.user_root \
              WHERE m.tenant_id = $1 ORDER BY m.user_root",
        )
        .bind(tenant_id)
        .bind(month.first_day())
        .bind(month.next().first_day())
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(
                |(user_root, profile, a, b, c, d, e, f)| oml_app::types::AccountCounters {
                    user_root,
                    profile,
                    usage: usage_of((a, b, c, d, e, f)),
                },
            )
            .collect())
    }

    async fn usage_ledger(
        &self,
        tenant_id: Uuid,
        month: Month,
    ) -> Result<Vec<oml_app::types::LedgerRow>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows: Vec<LedgerTuple> = sqlx::query_as(
            "SELECT user_root, day::text, profile, memories, text_bytes, writes, recalls, \
                        keys, connections \
                   FROM usage_daily \
                  WHERE tenant_id = $1 AND day >= ($2)::date AND day < ($3)::date \
                  ORDER BY day, user_root",
        )
        .bind(tenant_id)
        .bind(month.first_day())
        .bind(month.next().first_day())
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(
                |(user_root, day, profile, a, b, c, d, e, f)| oml_app::types::LedgerRow {
                    user_root,
                    day,
                    profile,
                    usage: usage_of((a, b, c, d, e, f)),
                },
            )
            .collect())
    }

    async fn usage_drift(
        &self,
        tenant_id: Uuid,
        month: Month,
    ) -> Result<Vec<oml_app::types::MeterDrift>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        // One snapshot for the counters and the rows they are compared with.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows: Vec<DriftRow> = sqlx::query_as(
            "WITH src AS (SELECT * FROM oml_account_source($1)), \
                  held AS (SELECT k.user_root_id AS user_root, count(*)::bigint AS writes \
                             FROM memories m JOIN api_keys k \
                               ON k.tenant_id = m.tenant_id AND k.id = m.author_key_id \
                            WHERE m.tenant_id = $1 AND k.user_root_id IS NOT NULL \
                              AND m.created_at >= ($2)::date::timestamp AT TIME ZONE 'UTC' \
                              AND m.created_at < ($3)::date::timestamp AT TIME ZONE 'UTC' \
                            GROUP BY 1), \
                  act AS (SELECT user_root, sum(writes)::bigint AS writes FROM account_activity \
                           WHERE tenant_id = $1 AND day >= ($2)::date AND day < ($3)::date \
                           GROUP BY 1), \
                  roots AS (SELECT user_root FROM src UNION SELECT user_root FROM account_meters \
                             WHERE tenant_id = $1 UNION SELECT user_root FROM held \
                             UNION SELECT user_root FROM act) \
             SELECT r.user_root, \
                    coalesce(c.memories, 0), coalesce(src.memories, 0), \
                    coalesce(c.text_bytes, 0), coalesce(src.text_bytes, 0), \
                    coalesce(act.writes, 0), coalesce(held.writes, 0), \
                    coalesce(c.keys, 0), coalesce(src.keys, 0), \
                    coalesce(c.connections, 0), coalesce(src.connections, 0) \
               FROM roots r \
               LEFT JOIN account_meters c ON c.tenant_id = $1 AND c.user_root = r.user_root \
               LEFT JOIN src ON src.user_root = r.user_root \
               LEFT JOIN held ON held.user_root = r.user_root \
               LEFT JOIN act ON act.user_root = r.user_root \
              ORDER BY r.user_root",
        )
        .bind(tenant_id)
        .bind(month.first_day())
        .bind(month.next().first_day())
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let mut drift = Vec::new();
        for (root, cm, sm, cb, sb, cw, sw, ck, sk, cc, sc) in rows {
            for (meter, counter, source) in [
                (Meter::Memories, cm, sm),
                (Meter::TextBytes, cb, sb),
                (Meter::Writes, cw, sw),
                (Meter::Keys, ck, sk),
                (Meter::Connections, cc, sc),
            ] {
                if counter != source {
                    drift.push(oml_app::types::MeterDrift {
                        user_root: root,
                        meter,
                        counter,
                        rows: source,
                    });
                }
            }
        }
        Ok(drift)
    }

    async fn user_quota_overrides(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> Result<Vec<(oml_app::domain::signup::Quota, u64)>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT quota, max_count FROM user_quotas WHERE tenant_id = $1 AND user_root = $2",
        )
        .bind(tenant_id)
        .bind(user_root)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.into_iter()
            .map(|(quota, limit)| {
                let quota = oml_app::domain::signup::Quota::parse(&quota).ok_or_else(|| {
                    StoreError::Backend(format!("user_quotas.quota {quota:?} is not a quota"))
                })?;
                Ok((quota, u64::try_from(limit).map_err(backend)?))
            })
            .collect()
    }

    async fn commit_chain(&self, reqs: Vec<CommitRequest>) -> Result<CommitReceipt, StoreError> {
        // Everything networked happened before BEGIN (ADR-0024): the events
        // arrive signed; inside the transaction nothing waits on anything
        // but Postgres.
        let mut reqs = reqs.into_iter();
        let Some(first) = reqs.next() else {
            return Err(StoreError::Backend(
                "an empty chain commits nothing".to_owned(),
            ));
        };
        let tenant_id = first.event.envelope.tenant_id;
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;

        // ADR-0002 d3: chain assignment is serialized by an advisory lock on
        // the tenant's chain head, held to commit.
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(chain_lock_key(tenant_id))
            .execute(&mut *tx)
            .await
            .map_err(backend)?;

        let head =
            sqlx::query("SELECT seq, head_hash FROM journal_chain_head WHERE tenant_id = $1")
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        let expected = match &head {
            None => (1_u64, Sha256Hash::ZERO),
            Some(row) => {
                let seq: i64 = row.try_get("seq").map_err(backend)?;
                let head_hash: Vec<u8> = row.try_get("head_hash").map_err(backend)?;
                let seq = u64::try_from(seq).map_err(backend)?;
                (seq + 1, hash32(&head_hash)?)
            }
        };
        // Each link extends the one before it, under the one lock; a
        // refused link returns before COMMIT, so the transaction rolls back
        // every earlier link with it (ADR-0025 d10).
        // The meters exist from 0045 on; once found they stay (ADR-0071).
        let metered = self.metered.load(std::sync::atomic::Ordering::Relaxed) || {
            let found: bool = sqlx::query_scalar("SELECT to_regproc('oml_chain_live') IS NOT NULL")
                .fetch_one(&mut *tx)
                .await
                .map_err(backend)?;
            if found {
                self.metered
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            found
        };
        // Each append is boxed: its future holds the fold's and the meters'
        // state, and a commit's future, which every caller holds inline,
        // stays a pointer wide for it.
        let mut receipt = Box::pin(append_event(&mut tx, expected, first, metered)).await?;
        for req in reqs {
            if req.event.envelope.tenant_id != tenant_id {
                return Err(StoreError::Backend(
                    "a chain commit names one tenant".to_owned(),
                ));
            }
            receipt = Box::pin(append_event(
                &mut tx,
                (receipt.seq + 1, receipt.hash),
                req,
                metered,
            ))
            .await?;
        }

        tx.commit().await.map_err(backend)?;
        Ok(receipt)
    }

    async fn create_tenant_partitions(&self, tenant_id: Uuid) -> Result<(), StoreError> {
        let Some(OwnerUrl(owner_url)) = &self.owner_url else {
            return Err(StoreError::Partition(
                "this store holds no owner URL for the partition DDL (ADR-0034 d4)".to_owned(),
            ));
        };
        // An owner connection opened for this call alone and closed after
        // it, so none outlives the DDL (ADR-0034 d4, d8).
        let mut owner = PgConnection::connect(owner_url)
            .await
            .map_err(|e| partition_error(&e))?;
        let created = create_partition_on(&mut owner, tenant_id).await;
        let _ = owner.close().await;
        created.map_err(|e| partition_error(&e))
    }

    async fn chain_head(&self, tenant_id: Uuid) -> Result<Option<ChainHead>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query("SELECT seq, head_hash FROM journal_chain_head WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        match row {
            None => Ok(None),
            Some(row) => {
                let seq: i64 = row.try_get("seq").map_err(backend)?;
                let head_hash: Vec<u8> = row.try_get("head_hash").map_err(backend)?;
                Ok(Some(ChainHead {
                    tenant_id,
                    seq: u64::try_from(seq).map_err(backend)?,
                    hash: hash32(&head_hash)?,
                }))
            }
        }
    }

    async fn events_range(
        &self,
        tenant_id: Uuid,
        from_seq: u64,
        limit: usize,
    ) -> Result<Vec<SignedEnvelope>, StoreError> {
        let from = i64::try_from(from_seq).unwrap_or(i64::MAX);
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(
            "SELECT signed_bytes, signed_bytes_hash, sig FROM journal \
             WHERE tenant_id = $1 AND seq >= $2 ORDER BY seq ASC LIMIT $3",
        )
        .bind(tenant_id)
        .bind(from)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(signed_envelope_from_row).collect()
    }

    async fn events_with_subjects(
        &self,
        tenant_id: Uuid,
        from_seq: u64,
        limit: usize,
    ) -> Result<TailScan, StoreError> {
        let from = i64::try_from(from_seq).unwrap_or(i64::MAX);
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        // One snapshot for the head, the scan and the subject rows, so
        // head_seq is read where the scan reads (http-api.md events tail).
        // SET TRANSACTION must precede every other statement.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let head = sqlx::query("SELECT seq FROM journal_chain_head WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        let rows = sqlx::query(
            "SELECT signed_bytes, signed_bytes_hash, sig FROM journal \
             WHERE tenant_id = $1 AND seq >= $2 ORDER BY seq ASC LIMIT $3",
        )
        .bind(tenant_id)
        .bind(from)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let events = rows
            .iter()
            .map(signed_envelope_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let ids: Vec<Uuid> = events
            .iter()
            .filter_map(|e| claim_subject(&e.envelope.event_type, &e.envelope.payload))
            .collect();
        let subjects = sqlx::query(
            "SELECT id, workarea_id, author_key_id FROM memories \
             WHERE tenant_id = $1 AND id = ANY($2::uuid[])",
        )
        .bind(tenant_id)
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let subjects = subjects
            .iter()
            .map(|row| {
                let subject = SubjectRow {
                    artifact_id: row.try_get("id").map_err(backend)?,
                    workarea_id: row.try_get("workarea_id").map_err(backend)?,
                    author_key_id: row.try_get("author_key_id").map_err(backend)?,
                };
                Ok((subject.artifact_id, subject))
            })
            .collect::<Result<std::collections::HashMap<_, _>, StoreError>>()?;
        Ok(TailScan {
            head_seq: head
                .as_ref()
                .map(|row| seq_from_column(row, "seq"))
                .transpose()?
                .unwrap_or(0),
            events: events
                .into_iter()
                .map(|event| TailEvent {
                    subject: claim_subject(&event.envelope.event_type, &event.envelope.payload)
                        .and_then(|id| subjects.get(&id).copied()),
                    event,
                })
                .collect(),
        })
    }

    async fn memory(
        &self,
        tenant_id: Uuid,
        memory_id: Uuid,
    ) -> Result<Option<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let sql =
            format!("SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = $1 AND m.id = $2");
        let row = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(memory_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(memory_from_row).transpose()
    }

    async fn memories(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // One set-wise statement, in the order the ids are named (ADR-0024
        // d4).
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m \
             WHERE m.tenant_id = $1 AND m.id = ANY($2::uuid[]) \
             ORDER BY array_position($2::uuid[], m.id)"
        );
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(memory_ids)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(memory_from_row).collect()
    }

    async fn tenant_search_default(&self, tenant_id: Uuid) -> Result<Option<String>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let value: Option<Option<String>> =
            sqlx::query_scalar("SELECT search_default FROM tenants WHERE id = $1")
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(value.flatten())
    }

    async fn tenant_query_text_since(
        &self,
        tenant_id: Uuid,
    ) -> Result<Option<EventTime>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let value: Option<Option<String>> = sqlx::query_scalar(
            "SELECT to_char(telemetry_query_text_since AT TIME ZONE 'UTC', \
                            'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
             FROM tenants WHERE id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        value
            .flatten()
            .as_deref()
            .map(EventTime::parse)
            .transpose()
            .map_err(backend)
    }

    /// The tenant's default pipeline and federation policy off one row, the
    /// row `tenant_search_default` reads, so a recall naming neither pays
    /// no read for federation (ADR-0072 d12).
    async fn recall_defaults(
        &self,
        tenant_id: Uuid,
    ) -> Result<(Option<String>, oml_app::federation::FederationPolicy), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row: Option<(Option<String>, bool, String, i32)> = sqlx::query_as(
            "SELECT search_default, federation_enabled, federation_default_sources, \
             federation_max_sources FROM tenants WHERE id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let Some((pipeline, enabled, default_sources, max_sources)) = row else {
            return Ok((None, oml_app::federation::FederationPolicy::default()));
        };
        Ok((
            pipeline,
            oml_app::federation::FederationPolicy {
                enabled,
                default_sources: oml_app::federation::DefaultSources::parse(&default_sources)
                    .ok_or_else(|| {
                        StoreError::Backend(format!(
                            "default_sources {default_sources:?} is not a mode"
                        ))
                    })?,
                max_sources: u32::try_from(max_sources).map_err(backend)?,
            },
        ))
    }

    async fn federation_policy(
        &self,
        tenant_id: Uuid,
    ) -> Result<oml_app::federation::FederationPolicy, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row: Option<(bool, String, i32)> = sqlx::query_as(
            "SELECT federation_enabled, federation_default_sources, federation_max_sources \
             FROM tenants WHERE id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let Some((enabled, default_sources, max_sources)) = row else {
            return Ok(oml_app::federation::FederationPolicy::default());
        };
        Ok(oml_app::federation::FederationPolicy {
            enabled,
            default_sources: oml_app::federation::DefaultSources::parse(&default_sources)
                .ok_or_else(|| {
                    StoreError::Backend(format!(
                        "default_sources {default_sources:?} is not a mode"
                    ))
                })?,
            max_sources: u32::try_from(max_sources).map_err(backend)?,
        })
    }

    async fn federation_declining(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
    ) -> Result<Vec<Uuid>, StoreError> {
        if nodes.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let declining: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM workareas WHERE tenant_id = $1 AND id = ANY($2::uuid[]) \
             AND NOT accept_federated ORDER BY id",
        )
        .bind(tenant_id)
        .bind(nodes)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(declining)
    }

    /// The reference set's counts (ADR-0072 d4.2, d12): the query's forms,
    /// then per node the served versions' counts and each form's document
    /// frequency, read by the counts' primary keys with leakproof
    /// equalities; then the versions expired at the request's now taken
    /// out by id. A word's frequency is its most frequent form's.
    async fn reference_counts(
        &self,
        tenant_id: Uuid,
        scope: &oml_app::federation::CountsScope,
        query: &str,
    ) -> Result<oml_app::federation::ReferenceCounts, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let forms: Vec<(String, String)> = sqlx::query_as(&search_forms_sql())
            .bind(query)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        let kinds = hit_kinds(&scope.kinds);
        let memory_kinds: Vec<i16> = kinds
            .iter()
            .filter_map(|k| match *k {
                "memory" => Some(1_i16),
                "derived_fact" => Some(2_i16),
                _ => None,
            })
            .collect();
        let tokens: Vec<i64> = {
            let mut t: Vec<i64> = forms.iter().map(|(_, f)| token_id(f)).collect();
            t.sort_unstable();
            t.dedup();
            t
        };
        let mut rows: BTreeMap<Uuid, u64> = scope.nodes.iter().map(|n| (*n, 0)).collect();
        let mut df: BTreeMap<(Uuid, i64), u64> = BTreeMap::new();
        if !scope.nodes.is_empty() {
            let counted: Vec<(Uuid, Option<i64>, i64)> = sqlx::query_as(REFERENCE_COUNTS)
                .bind(tenant_id)
                .bind(&scope.nodes)
                .bind(&memory_kinds)
                .bind(&tokens)
                .fetch_all(&mut *tx)
                .await
                .map_err(backend)?;
            for (node, token, n) in counted {
                let n = u64::try_from(n).map_err(backend)?;
                match token {
                    None => *rows.entry(node).or_default() += n,
                    Some(token) => *df.entry((node, token)).or_default() += n,
                }
            }
            if let Some(now) = scope.expired_at {
                let expired: Vec<(Uuid, Uuid)> = sqlx::query_as(REFERENCE_EXPIRED)
                    .bind(tenant_id)
                    .bind(&scope.nodes)
                    .bind(&kinds)
                    .bind(now)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(backend)?;
                if !expired.is_empty() {
                    let home: BTreeMap<Uuid, Uuid> = expired.iter().copied().collect();
                    for node in home.values() {
                        if let Some(r) = rows.get_mut(node) {
                            *r = r.saturating_sub(1);
                        }
                    }
                    let ids: Vec<Uuid> = home.keys().copied().collect();
                    let held: Vec<(Uuid, i64)> = sqlx::query_as(REFERENCE_EXPIRED_TOKENS)
                        .bind(tenant_id)
                        .bind(&ids)
                        .bind(&tokens)
                        .fetch_all(&mut *tx)
                        .await
                        .map_err(backend)?;
                    for (memory_id, token) in held {
                        if let Some(node) = home.get(&memory_id)
                            && let Some(d) = df.get_mut(&(*node, token))
                        {
                            *d = d.saturating_sub(1);
                        }
                    }
                }
            }
        }
        tx.commit().await.map_err(backend)?;
        Ok(self.counts_of_forms(&forms, &rows, &df, scope.prunes))
    }

    async fn vocabulary_counts(
        &self,
        tenant_id: Uuid,
        scope: &oml_app::federation::CountsScope,
        max_pairs: usize,
    ) -> Result<Option<oml_app::federation::VocabularyCounts>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The database cancels each statement of the read at 25 s. The
        // limit is per statement and the read runs up to four, so a build
        // the lead stops at 30 s can leave its last statement running for
        // at most 25 s more, one at a time (ADR-0072 d12; #268).
        sqlx::query(&format!(
            "SET LOCAL statement_timeout = '{}s'",
            oml_app::app::WIDEN_SIGNAL_STATEMENT_SECS
        ))
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let kinds = hit_kinds(&scope.kinds);
        let memory_kinds: Vec<i16> = kinds
            .iter()
            .filter_map(|k| match *k {
                "memory" => Some(1_i16),
                "derived_fact" => Some(2_i16),
                _ => None,
            })
            .collect();
        let mut rows: BTreeMap<Uuid, u64> = scope.nodes.iter().map(|n| (*n, 0)).collect();
        let mut counted: Vec<(Uuid, i64, i64)> = Vec::new();
        // Each expired version's (node, token), subtracted from the counts.
        let mut expired_pairs: BTreeMap<(Uuid, i64), u64> = BTreeMap::new();
        if !scope.nodes.is_empty() {
            let per_node: Vec<(Uuid, i64)> = sqlx::query_as(VOCABULARY_ROWS)
                .bind(tenant_id)
                .bind(&scope.nodes)
                .bind(&memory_kinds)
                .fetch_all(&mut *tx)
                .await
                .map_err(backend)?;
            for (node, n) in per_node {
                *rows.entry(node).or_default() += u64::try_from(n).map_err(backend)?;
            }
            // At most one row past the cap is read, so a vocabulary too large
            // to hold is never read whole.
            counted = sqlx::query_as(VOCABULARY_TERMS)
                .bind(tenant_id)
                .bind(&scope.nodes)
                .bind(&memory_kinds)
                .bind(i64::try_from(max_pairs.saturating_add(1)).unwrap_or(i64::MAX))
                .fetch_all(&mut *tx)
                .await
                .map_err(backend)?;
            if counted.len() > max_pairs {
                tx.commit().await.map_err(backend)?;
                return Ok(None);
            }
            if let Some(now) = scope.expired_at {
                let expired: Vec<(Uuid, Uuid)> = sqlx::query_as(REFERENCE_EXPIRED)
                    .bind(tenant_id)
                    .bind(&scope.nodes)
                    .bind(&kinds)
                    .bind(now)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(backend)?;
                if !expired.is_empty() {
                    let home: BTreeMap<Uuid, Uuid> = expired.iter().copied().collect();
                    for node in home.values() {
                        if let Some(r) = rows.get_mut(node) {
                            *r = r.saturating_sub(1);
                        }
                    }
                    let ids: Vec<Uuid> = home.keys().copied().collect();
                    let held: Vec<(Uuid, i64)> = sqlx::query_as(VOCABULARY_EXPIRED_TOKENS)
                        .bind(tenant_id)
                        .bind(&ids)
                        .fetch_all(&mut *tx)
                        .await
                        .map_err(backend)?;
                    for (memory_id, token) in held {
                        if let Some(node) = home.get(&memory_id) {
                            *expired_pairs.entry((*node, token)).or_default() += 1;
                        }
                    }
                }
            }
        }
        tx.commit().await.map_err(backend)?;
        // Taken out of the rows as read, with no second copy: the read's
        // 32 bytes a row and the signal's 16 a pair are the build's peak.
        Ok(Some(oml_app::federation::VocabularyCounts::from_counts(
            rows,
            counted.into_iter().map(|(node, token, n)| {
                let less = expired_pairs.get(&(node, token)).copied().unwrap_or(0);
                (
                    node,
                    token,
                    u64::try_from(n).unwrap_or(0).saturating_sub(less),
                )
            }),
        )))
    }

    /// The query's forms, one statement over no table, then their counts out
    /// of the vocabulary as [`Self::reference_counts`] reads them.
    async fn counts_from_vocabulary(
        &self,
        vocabulary: &oml_app::federation::VocabularyCounts,
        query: &str,
        prunes: bool,
    ) -> Result<oml_app::federation::ReferenceCounts, StoreError> {
        let forms: Vec<(String, String)> = sqlx::query_as(&search_forms_sql())
            .bind(query)
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;
        let mut df: BTreeMap<(Uuid, i64), u64> = BTreeMap::new();
        for (_, form) in &forms {
            let token = token_id(form);
            for node in vocabulary.rows.keys() {
                let n = vocabulary.df(node, token);
                if n > 0 {
                    df.insert((*node, token), n);
                }
            }
        }
        Ok(self.counts_of_forms(&forms, &vocabulary.rows, &df, prunes))
    }

    async fn steering_in_force(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // Promoted served tips of kind steering on the named nodes, never a
        // retirement version; a node's position in the array is its depth
        // from the root (retrieval.md, the recall-ordering contract;
        // ADR-0025 d8).
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m \
             WHERE m.tenant_id = $1 AND m.status = 'promoted' AND m.is_tip \
               AND NOT m.retirement AND m.kind = 'steering' \
               AND m.workarea_id = ANY($2::uuid[]) \
             ORDER BY array_position($2::uuid[], m.workarea_id) ASC, \
                      m.promoted_at ASC, m.id ASC"
        );
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(nodes)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(memory_from_row).collect()
    }

    async fn steering_as_of(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        as_of: &EventTime,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The steering tips at the instant on the named nodes, promoted
        // today, never a retirement version, in steering_in_force's order
        // (ADR-0059 d15, d18).
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m \
             WHERE m.tenant_id = $1 AND {} \
               AND NOT m.retirement AND m.kind = 'steering' \
               AND m.workarea_id = ANY($2::uuid[]) \
             ORDER BY array_position($2::uuid[], m.workarea_id) ASC, \
                      m.promoted_at ASC, m.id ASC",
            tip_at_sql("($3)::timestamptz")
        );
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(nodes)
            .bind(as_of.as_str())
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(memory_from_row).collect()
    }

    async fn tips_as_of(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
        as_of: &EventTime,
    ) -> Result<Vec<Uuid>, StoreError> {
        if memory_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let ids: Vec<Uuid> = sqlx::query_scalar(&format!(
            "SELECT m.id FROM memories m WHERE m.tenant_id = $1 AND m.id = ANY($2::uuid[]) AND {}",
            tip_at_sql("($3)::timestamptz")
        ))
        .bind(tenant_id)
        .bind(memory_ids)
        .bind(as_of.as_str())
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(ids)
    }

    async fn guardrail_rows(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        author_key_id: Uuid,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The promoted served tips of kind guardrail on the named nodes,
        // never a retirement version, and the held versions the calling key
        // authored there: the state and author predicates are this
        // statement's own (proposed ADR-0037 d3, d4).
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m \
             WHERE m.tenant_id = $1 AND m.kind = 'guardrail' \
               AND m.workarea_id = ANY($2::uuid[]) \
               AND ((m.status = 'promoted' AND m.is_tip AND NOT m.retirement) \
                    OR (m.status = 'held' AND m.author_key_id = $3))"
        );
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(nodes)
            .bind(author_key_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(memory_from_row).collect()
    }

    async fn list_memories(
        &self,
        tenant_id: Uuid,
        filter: &ListingFilter,
        after: Option<&ListingPosition>,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut sql = QueryBuilder::<Postgres>::new(format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = "
        ));
        sql.push_bind(tenant_id);
        // The scope is the store's own predicate: the own key set off the
        // masked nodes (memories_author serves it), or promoted tips on the
        // node set alone — an empty array matches nothing (R3; ADR-0025 d1,
        // d13).
        match &filter.scope {
            ListingScope::Own { key_ids, masked } => {
                sql.push(" AND m.author_key_id = ANY(")
                    .push_bind(key_ids.clone())
                    .push("::uuid[]) AND NOT (m.workarea_id = ANY(")
                    .push_bind(masked.clone())
                    .push("::uuid[]))");
            }
            ListingScope::Tips {
                nodes,
                held,
                current_scanner,
            } => {
                // Inside an in-progress scope the held versions of every
                // author on the nodes join the tips (ADR-0030 d3).
                // At an instant, the tips then and the held rows recorded
                // by it (ADR-0059 d15, d19).
                match &filter.as_of {
                    None => sql.push(" AND ((m.status = 'promoted' AND m.is_tip)"),
                    Some(at) => sql.push(format!(" AND (({})", tip_at_sql(&instant_literal(at)))),
                };
                if !held.is_empty() {
                    // A version the scan held stays its author's (ADR-0065
                    // d5, amended 2026-11-10).
                    sql.push(" OR (");
                    if let Some(at) = &filter.as_of {
                        sql.push("m.created_at <= (")
                            .push_bind(at.as_str().to_owned())
                            .push(")::timestamptz AND ");
                    }
                    sql.push(
                        "m.status = 'held' AND NOT m.retirement \
                         AND m.scan_finding IS NULL \
                         AND m.kind IN ('memory', 'derived_fact') AND m.workarea_id = ANY(",
                    )
                    .push_bind(held.clone())
                    .push("::uuid[]) AND m.scanned_under IS NOT DISTINCT FROM ")
                    .push_bind(current_scanner.clone())
                    .push("::text)");
                }
                sql.push(") AND m.workarea_id = ANY(")
                    .push_bind(nodes.clone())
                    .push("::uuid[])");
            }
        }
        if let Some(subtree) = &filter.subtree {
            sql.push(" AND m.workarea_id = ANY(")
                .push_bind(subtree.clone())
                .push("::uuid[])");
        }
        if let Some(state) = filter.state {
            sql.push(" AND m.status = ").push_bind(state.as_str());
        }
        if let Some(kind) = filter.kind {
            sql.push(" AND m.kind = ").push_bind(kind.as_str());
        }
        if let Some(claim_key) = &filter.claim_key {
            sql.push(" AND m.claim_key = ").push_bind(claim_key.clone());
        }
        // The origin filters, read by `memories_anchor` (ADR-0057 d4).
        if let Some(repo) = &filter.anchor.repo {
            sql.push(" AND m.anchor_repo = ").push_bind(repo.clone());
        }
        // The prefix as a range on the leakproof `text_pattern_ops`
        // operators, so the index serves it under row security.
        if let Some(prefix) = &filter.anchor.path_prefix {
            sql.push(" AND m.anchor_path ~>=~ ")
                .push_bind(prefix.clone());
            if let Some(upper) = prefix_upper(prefix) {
                sql.push(" AND m.anchor_path ~<~ ").push_bind(upper);
            }
        }
        // The kind filter (ADR-0063 d3): a row without a kind never passes.
        if !filter.anchor.origin_kinds.is_empty() {
            sql.push(" AND m.origin_kind = ANY(")
                .push_bind(origin_kind_strings(&filter.anchor.origin_kinds))
                .push("::text[])");
        }
        if let Some(author) = filter.author {
            sql.push(" AND m.author_key_id = ").push_bind(author);
        }
        if let Some(tip) = filter.tip {
            sql.push(" AND m.is_tip = ").push_bind(tip);
        }
        if let Some(since) = &filter.since {
            sql.push(" AND m.created_at >= (")
                .push_bind(since.as_str().to_owned())
                .push(")::timestamptz");
        }
        if let Some(until) = &filter.until {
            sql.push(" AND m.created_at < (")
                .push_bind(until.as_str().to_owned())
                .push(")::timestamptz");
        }
        // Keyset: strictly after the position in the listing's order.
        if let Some(after) = after {
            sql.push(" AND (m.created_at, m.id) < ((")
                .push_bind(after.created_at.as_str().to_owned())
                .push(")::timestamptz, ")
                .push_bind(after.artifact_id)
                .push(")");
        }
        sql.push(" ORDER BY m.created_at DESC, m.id DESC LIMIT ")
            .push_bind(limit);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sql.build().fetch_all(&mut *tx).await.map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(memory_from_row).collect()
    }

    async fn claim_key_state(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        claim_key: &str,
    ) -> Result<ClaimKeyState, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(
            "SELECT max(version) AS greatest, \
                    coalesce(bool_or(status IN ('held', 'promoted', 'demoted')), false) AS live \
             FROM memories WHERE tenant_id = $1 AND workarea_id = $2 AND claim_key = $3",
        )
        .bind(tenant_id)
        .bind(workarea_id)
        .bind(claim_key)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let greatest: Option<i32> = row.try_get("greatest").map_err(backend)?;
        let version = |v: i32| {
            u32::try_from(v)
                .map_err(|_| StoreError::Backend(format!("stored version {v} is negative")))
        };
        Ok(ClaimKeyState {
            greatest_version: greatest.map(version).transpose()?,
            live: row.try_get("live").map_err(backend)?,
        })
    }

    async fn scan_backlog(
        &self,
        tenant_id: Uuid,
        current: Option<&str>,
    ) -> Result<ScanBacklog, StoreError> {
        // One pass over the tenant's partition; doctor reads it once per
        // run (ADR-0074 d23, d28). The unlisted count probes the findings
        // list by its primary key for the held versions that carry a
        // finding.
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let (held, all, unlisted): (i64, i64, i64) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE status = 'held' \
                                       AND scanned_under IS DISTINCT FROM $2::text), \
                    count(*) FILTER (WHERE scanned_under IS DISTINCT FROM $2::text), \
                    count(*) FILTER (WHERE status = 'held' AND scan_finding IS NOT NULL \
                                       AND NOT EXISTS (SELECT 1 FROM scan_findings f \
                                                        WHERE f.tenant_id = m.tenant_id \
                                                          AND f.memory_id = m.id)) \
             FROM memories m \
             WHERE tenant_id = $1 AND status <> 'erased'",
        )
        .bind(tenant_id)
        .bind(current)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(ScanBacklog {
            held_unrescanned: u64::try_from(held).map_err(backend)?,
            rescan_due: u64::try_from(all).map_err(backend)?,
            findings_unlisted: u64::try_from(unlisted).map_err(backend)?,
        })
    }

    async fn scan_findings(
        &self,
        tenant_id: Uuid,
        memory_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, VersionFinding)>, StoreError> {
        if memory_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(
            "SELECT memory_id, event_seq, class, rule, verdict, rule_set, field, line, via, \
                    span_start, span_end, found_by, migrated_from \
             FROM scan_findings WHERE tenant_id = $1 AND memory_id = ANY($2::uuid[]) \
             ORDER BY memory_id, event_seq, ord",
        )
        .bind(tenant_id)
        .bind(memory_ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter()
            .map(|r| {
                let get_u = |c: &str| -> Result<Option<u64>, StoreError> {
                    r.try_get::<Option<i64>, _>(c)
                        .map_err(backend)?
                        .map(u64::try_from)
                        .transpose()
                        .map_err(backend)
                };
                let verdict: String = r.try_get("verdict").map_err(backend)?;
                let found_by: String = r.try_get("found_by").map_err(backend)?;
                let span = match (get_u("span_start")?, get_u("span_end")?) {
                    (Some(a), Some(b)) => Some((a, b)),
                    _ => None,
                };
                Ok((
                    r.try_get("memory_id").map_err(backend)?,
                    VersionFinding {
                        class: r.try_get("class").map_err(backend)?,
                        rule: r.try_get("rule").map_err(backend)?,
                        verdict: oml_app::types::FindingVerdict::parse(&verdict)
                            .ok_or_else(|| backend(format!("verdict {verdict:?}")))?,
                        rule_set: r.try_get("rule_set").map_err(backend)?,
                        field: r.try_get("field").map_err(backend)?,
                        line: r
                            .try_get::<Option<i32>, _>("line")
                            .map_err(backend)?
                            .map(u32::try_from)
                            .transpose()
                            .map_err(backend)?,
                        via: r.try_get("via").map_err(backend)?,
                        span,
                        found_by: oml_app::types::FoundBy::parse(&found_by)
                            .ok_or_else(|| backend(format!("found_by {found_by:?}")))?,
                        event_seq: get_u("event_seq")?,
                        migrated_from: r.try_get("migrated_from").map_err(backend)?,
                    },
                ))
            })
            .collect()
    }

    async fn held_count(&self, tenant_id: Uuid, nodes: &[Uuid]) -> Result<u64, StoreError> {
        // One probe per node through memories_workarea (tenant_id,
        // workarea_id, state) (ADR-0030 d6).
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM memories \
             WHERE tenant_id = $1 AND workarea_id = ANY($2::uuid[]) AND status = 'held'",
        )
        .bind(tenant_id)
        .bind(nodes)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        u64::try_from(n).map_err(backend)
    }

    async fn stale_count(&self, tenant_id: Uuid, workarea_id: Uuid) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM memories \
             WHERE tenant_id = $1 AND workarea_id = $2 AND status = 'promoted' AND is_tip \
               AND source_check_result IN ('changed', 'removed')",
        )
        .bind(tenant_id)
        .bind(workarea_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        u64::try_from(n).map_err(backend)
    }

    async fn claim_latest_readable(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        claim_key: &str,
        own_key_ids: &[Uuid],
        reads_promoted: bool,
    ) -> Result<Option<u32>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let latest: Option<i32> = sqlx::query_scalar(
            "SELECT max(version) FROM memories \
             WHERE tenant_id = $1 AND workarea_id = $2 AND claim_key = $3 \
               AND ((status = 'promoted' AND ($5 OR author_key_id = ANY($4::uuid[]))) \
                 OR (status = 'held' AND author_key_id = ANY($4::uuid[])))",
        )
        .bind(tenant_id)
        .bind(workarea_id)
        .bind(claim_key)
        .bind(own_key_ids)
        .bind(reads_promoted)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        latest
            .map(|v| {
                u32::try_from(v)
                    .map_err(|_| StoreError::Backend(format!("stored version {v} is negative")))
            })
            .transpose()
    }

    async fn claim_tip(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
        claim_key: &str,
    ) -> Result<Option<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The memories_served_tip index answers this lookup.
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m \
             WHERE m.tenant_id = $1 AND m.workarea_id = $2 AND m.claim_key = $3 \
               AND m.is_tip AND m.status = 'promoted'"
        );
        let row = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(workarea_id)
            .bind(claim_key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(memory_from_row).transpose()
    }

    async fn held_successors(
        &self,
        tenant_id: Uuid,
        replaced: Uuid,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m \
             WHERE m.tenant_id = $1 AND m.replaces_artifact_id = $2 AND m.status = 'held' \
             ORDER BY m.created_at ASC, m.id ASC"
        );
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(replaced)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(memory_from_row).collect()
    }

    async fn review_claim_next(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
        workareas: &[Uuid],
    ) -> Result<Option<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // Serve the oldest entry this reviewer may judge: only rows within
        // its dreamer access (ADR-0025 d1), skipping rows another key
        // claimed, rows whose lineage closure contains the reviewer, and
        // held successors whose target has a held rival the reviewer
        // authored (admission.md review; roles-and-keys.md rival
        // successors, ADR-0025 d10; R2 proper re-runs at verdict time). A
        // re-claim by the same key matches its own held_by_key row again.
        // Every reference to memories names the tenant, so the plan keeps
        // its partition alone (ADR-0034 d11). The order walks
        // review_queue_order (migration 0024) and stops at the first row
        // this reviewer may judge; the rival check runs only for a
        // successor, so a first version never costs an anti-join over every
        // held row (ADR-0055 d9).
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM review_queue q \
             JOIN memories m ON m.tenant_id = q.tenant_id AND m.id = q.memory_id \
             WHERE q.tenant_id = $1 AND m.tenant_id = $1 \
               AND m.workarea_id = ANY($4::uuid[]) \
               AND (q.held_by_key IS NULL OR q.held_by_key = $2) \
               AND NOT (m.lineage_principals @> ARRAY[$3]::text[]) \
               AND NOT (m.lineage_key_ids @> ARRAY[$2]::uuid[]) \
               AND (m.replaces_artifact_id IS NULL OR NOT EXISTS ( \
                   SELECT 1 FROM memories r \
                   WHERE r.tenant_id = $1 AND r.id <> m.id AND r.status = 'held' \
                     AND r.replaces_artifact_id = m.replaces_artifact_id \
                     AND r.workarea_id = m.workarea_id AND r.claim_key = m.claim_key \
                     AND (r.author_key_id = $2 OR r.author_principal = $3))) \
             ORDER BY q.held_since ASC, q.memory_id ASC \
             LIMIT 1 \
             FOR NO KEY UPDATE OF q SKIP LOCKED"
        );
        let row = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(reviewer_key_id)
            .bind(reviewer_principal)
            .bind(workareas)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        let Some(row) = row else {
            tx.commit().await.map_err(backend)?;
            return Ok(None);
        };
        let memory = memory_from_row(&row)?;
        sqlx::query(
            "UPDATE review_queue SET held_by_key = $3 \
             WHERE tenant_id = $1 AND memory_id = $2",
        )
        .bind(tenant_id)
        .bind(memory.memory_id)
        .bind(reviewer_key_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(Some(memory))
    }

    async fn review_claimed(
        &self,
        tenant_id: Uuid,
        reviewer_key_id: Uuid,
    ) -> Result<Vec<Uuid>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The rows this key claimed, in the order review next serves them;
        // two tell one open item from several (ADR-0039 d10).
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT memory_id FROM review_queue \
             WHERE tenant_id = $1 AND held_by_key = $2 \
             ORDER BY held_since ASC, memory_id ASC \
             LIMIT 2",
        )
        .bind(tenant_id)
        .bind(reviewer_key_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(ids)
    }

    async fn review_held(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
        filter: &HeldFilter,
        after: Option<&ListingPosition>,
        limit: usize,
    ) -> Result<Vec<HeldRow>, StoreError> {
        if filter.nodes.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        // review_claim_next's predicate, unclaimed, a page at a time
        // (ADR-0055 d2): the same rows in the same order, so a listing
        // never shows what review would not serve this key. The order walks
        // review_queue_order (migration 0024) and stops at the page.
        let mut sql = QueryBuilder::<Postgres>::new(format!(
            "SELECT {MEMORY_COLUMNS}, \
             to_char(q.held_since AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
             AS held_since_signed \
             FROM review_queue q \
             JOIN memories m ON m.tenant_id = q.tenant_id AND m.id = q.memory_id \
             WHERE q.tenant_id = "
        ));
        sql.push_bind(tenant_id)
            .push(" AND m.tenant_id = ")
            .push_bind(tenant_id)
            .push(" AND m.workarea_id = ANY(")
            .push_bind(filter.nodes.clone())
            .push("::uuid[]) AND (q.held_by_key IS NULL OR q.held_by_key = ")
            .push_bind(reviewer_key_id)
            .push(") AND NOT (m.lineage_principals @> ARRAY[")
            .push_bind(reviewer_principal.to_owned())
            .push("]::text[]) AND NOT (m.lineage_key_ids @> ARRAY[")
            .push_bind(reviewer_key_id)
            .push(
                "]::uuid[]) AND (m.replaces_artifact_id IS NULL OR NOT EXISTS ( \
                 SELECT 1 FROM memories r WHERE r.tenant_id = ",
            )
            .push_bind(tenant_id)
            .push(
                " AND r.id <> m.id AND r.status = 'held' \
                 AND r.replaces_artifact_id = m.replaces_artifact_id \
                 AND r.workarea_id = m.workarea_id AND r.claim_key = m.claim_key \
                 AND (r.author_key_id = ",
            )
            .push_bind(reviewer_key_id)
            .push(" OR r.author_principal = ")
            .push_bind(reviewer_principal.to_owned())
            .push(")))");
        if let Some(author) = filter.author_key_id {
            sql.push(" AND m.author_key_id = ").push_bind(author);
        }
        // A literal prefix: no LIKE, so `_` and `%` match only themselves.
        if let Some(prefix) = &filter.claim_key_prefix {
            sql.push(" AND left(m.claim_key, char_length(")
                .push_bind(prefix.clone())
                .push(")) = ")
                .push_bind(prefix.clone());
        }
        if let Some(after) = after {
            sql.push(" AND (q.held_since, q.memory_id) > ((")
                .push_bind(after.created_at.as_str().to_owned())
                .push(")::timestamptz, ")
                .push_bind(after.artifact_id)
                .push(")");
        }
        sql.push(" ORDER BY q.held_since ASC, q.memory_id ASC LIMIT ")
            .push_bind(i64::try_from(limit).unwrap_or(i64::MAX));
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sql.build().fetch_all(&mut *tx).await.map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter()
            .map(|row| {
                Ok(HeldRow {
                    memory: memory_from_row(row)?,
                    held_since: event_time_column(row, "held_since_signed")?.ok_or_else(|| {
                        StoreError::Backend("review_queue.held_since is null".to_owned())
                    })?,
                })
            })
            .collect()
    }

    async fn key_by_hash(&self, key_hash: &str) -> Result<Option<KeyRecord>, StoreError> {
        let Some(hash) = key_hash_bytes(key_hash) else {
            return Ok(None); // cannot name any stored row — fail closed
        };
        let mut tx = self.pool.begin().await.map_err(backend)?;
        // Key resolution precedes the tenant: 0001's key_resolution policy
        // opens exactly the row whose hash equals oml.api_key_hash.
        sqlx::query("SELECT set_config('oml.api_key_hash', $1, true)")
            .bind(hex::encode(&hash))
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        let row = sqlx::query(&format!(
            "SELECT {KEY_COLUMNS} FROM api_keys WHERE hash = $1 AND revoked_at IS NULL"
        ))
        .bind(&hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(key_from_row).transpose()
    }

    async fn key_access(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Option<KeyAccess>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        // One snapshot for the three reads, so the epochs they return label
        // one journalled state (ADR-0025 d14). SET TRANSACTION must precede
        // every other statement of the transaction.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let key = sqlx::query("SELECT issued_seq FROM api_keys WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(key_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        let Some(key) = key else {
            tx.commit().await.map_err(backend)?;
            return Ok(None);
        };
        let grants = sqlx::query(
            "SELECT id, role, workarea_node_id, deny, epoch, covers FROM grants \
             WHERE tenant_id = $1 AND key_id = $2 ORDER BY epoch ASC, id ASC",
        )
        .bind(tenant_id)
        .bind(key_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        // A lifted denial masks nothing; its lift still moves the epoch.
        let denials = sqlx::query(
            "SELECT id, workarea_node_id, created_by, epoch FROM key_denials \
             WHERE tenant_id = $1 AND key_id = $2 AND lifted_epoch IS NULL \
             ORDER BY epoch ASC, id ASC",
        )
        .bind(tenant_id)
        .bind(key_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let lifts = sqlx::query(
            "SELECT lifted_epoch FROM key_denials \
             WHERE tenant_id = $1 AND key_id = $2 AND lifted_epoch IS NOT NULL \
             ORDER BY lifted_epoch ASC",
        )
        .bind(tenant_id)
        .bind(key_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;

        let grants = grants
            .iter()
            .map(|row| {
                let role: String = row.try_get("role").map_err(backend)?;
                let deny: Value = row.try_get("deny").map_err(backend)?;
                let covers: String = row.try_get("covers").map_err(backend)?;
                Ok(GrantRecord {
                    grant: Grant {
                        grant_id: row.try_get("id").map_err(backend)?,
                        node: row.try_get("workarea_node_id").map_err(backend)?,
                        role: role_from_str(&role)?,
                        deny: deny_from_json(&deny)?,
                        covers: Covers::parse(&covers).ok_or_else(|| {
                            StoreError::Backend(format!(
                                "grants.covers {covers:?} is not a coverage"
                            ))
                        })?,
                    },
                    epoch: seq_from_column(row, "epoch")?,
                })
            })
            .collect::<Result<_, StoreError>>()?;
        let denials = denials
            .iter()
            .map(|row| {
                Ok(DenialRecord {
                    denial: KeyDenial {
                        denial_id: row.try_get("id").map_err(backend)?,
                        node: row.try_get("workarea_node_id").map_err(backend)?,
                    },
                    created_by: row.try_get("created_by").map_err(backend)?,
                    epoch: seq_from_column(row, "epoch")?,
                })
            })
            .collect::<Result<_, StoreError>>()?;
        Ok(Some(KeyAccess {
            issued_seq: seq_from_column(&key, "issued_seq")?,
            grants,
            denials,
            lift_epochs: lifts
                .iter()
                .map(|row| seq_from_column(row, "lifted_epoch"))
                .collect::<Result<_, StoreError>>()?,
        }))
    }

    async fn resolve_name(
        &self,
        tenant_id: Uuid,
        kind: NamedKind,
        name: &Name,
    ) -> Result<Option<Uuid>, StoreError> {
        // One lookup on the kind's unique name index, in the tenant's
        // namespace; a revoked key's row stays, so its name resolves
        // (ADR-0026 d10). Tenant names are the deployment's.
        let sql = match kind {
            // The shared namespace alone (ADR-0044 d5).
            NamedKind::Workarea => {
                "SELECT id FROM workareas WHERE tenant_id = $1 AND name = $2 \
                 AND namespace_root_id IS NULL"
            }
            NamedKind::Crew => "SELECT id FROM crews WHERE tenant_id = $1 AND name = $2",
            NamedKind::Key => "SELECT id FROM api_keys WHERE tenant_id = $1 AND name = $2",
            NamedKind::Tenant => return Ok(None),
        };
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let id: Option<Uuid> = sqlx::query_scalar(sql)
            .bind(tenant_id)
            .bind(name.as_str())
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(id)
    }

    async fn crew(&self, tenant_id: Uuid, crew_id: Uuid) -> Result<Option<CrewRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(
            "SELECT id, tenant_id, name, charter_uri, external_manager FROM crews \
             WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant_id)
        .bind(crew_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(crew_from_row).transpose()
    }

    async fn crews(&self, tenant_id: Uuid) -> Result<Vec<CrewRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // COLLATE "C" compares the UTF-8 bytes, whatever the locale.
        let rows = sqlx::query(
            "SELECT id, tenant_id, name, charter_uri, external_manager FROM crews \
             WHERE tenant_id = $1 ORDER BY name COLLATE \"C\" ASC",
        )
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(crew_from_row).collect()
    }

    async fn key(&self, tenant_id: Uuid, key_id: Uuid) -> Result<Option<KeyRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // A revoked key's row stays as history and is read here (ADR-0026
        // d10).
        let row = sqlx::query(&format!(
            "SELECT {KEY_COLUMNS} FROM api_keys WHERE tenant_id = $1 AND id = $2"
        ))
        .bind(tenant_id)
        .bind(key_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(key_from_row).transpose()
    }

    async fn resolve_user_workarea(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        name: &Name,
    ) -> Result<Option<Uuid>, StoreError> {
        // One lookup on workareas_user_name (ADR-0044 d5).
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM workareas WHERE tenant_id = $1 AND namespace_root_id = $2 \
             AND name = $3",
        )
        .bind(tenant_id)
        .bind(user_root)
        .bind(name.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(id)
    }

    async fn key_references(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Option<KeyReferences>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // One query under the tenant's fence: the key row, its tenant, its
        // crew record, and its default workarea with the path the walk up
        // the parent_id links derives, root first, and whether that walk
        // reached a root (ADR-0039 d15).
        let row = sqlx::query(
            "WITH RECURSIVE k AS ( \
                 SELECT default_workarea_id, crew_id, user_root_id FROM api_keys \
                  WHERE tenant_id = $1 AND id = $2 \
             ), up AS ( \
                 SELECT w.id, w.parent_id, w.name, 0 AS depth \
                   FROM workareas w JOIN k ON w.id = k.default_workarea_id \
                  WHERE w.tenant_id = $1 \
                 UNION ALL \
                 SELECT w.id, w.parent_id, w.name, up.depth + 1 \
                   FROM workareas w JOIN up ON w.id = up.parent_id \
                  WHERE w.tenant_id = $1 \
             ) \
             SELECT t.name::text AS tenant_name, \
                    k.default_workarea_id, \
                    (SELECT up.name::text FROM up WHERE up.depth = 0) AS workarea_name, \
                    (SELECT string_agg(up.name::text, '.' ORDER BY up.depth DESC) FROM up) \
                        AS workarea_path, \
                    (SELECT bool_and(up.name IS NOT NULL) \
                            AND bool_or(up.parent_id IS NULL) FROM up) AS workarea_whole, \
                    k.crew_id, c.name::text AS crew_name, \
                    k.user_root_id, r.name::text AS user_name \
               FROM k \
               JOIN tenants t ON t.id = $1 \
               LEFT JOIN crews c ON c.tenant_id = $1 AND c.id = k.crew_id \
               LEFT JOIN workareas r ON r.tenant_id = $1 AND r.id = k.user_root_id",
        )
        .bind(tenant_id)
        .bind(key_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let tenant = NamedRef {
            id: tenant_id,
            name: stored_name(&row, "tenant_name")?
                .ok_or_else(|| StoreError::Backend(format!("tenant {tenant_id} has no name")))?,
        };
        let default_workarea = match row
            .try_get::<Option<Uuid>, _>("default_workarea_id")
            .map_err(backend)?
        {
            None => None,
            Some(workarea_id) => {
                // A default the tenant does not hold, or whose walk names a
                // row with no name or stops short of a root, fails closed,
                // as the ancestry read does.
                let whole: Option<bool> = row.try_get("workarea_whole").map_err(backend)?;
                let path: Option<String> = row.try_get("workarea_path").map_err(backend)?;
                match (whole, stored_name(&row, "workarea_name")?, path) {
                    (Some(true), Some(name), Some(path)) => Some(WorkareaRef {
                        id: workarea_id,
                        name,
                        path,
                    }),
                    _ => {
                        return Err(StoreError::Backend(format!(
                            "key {key_id}'s default workarea {workarea_id} has no named \
                             ancestry to a root in this tenant"
                        )));
                    }
                }
            }
        };
        let crew = match row.try_get::<Option<Uuid>, _>("crew_id").map_err(backend)? {
            None => None,
            Some(crew_id) => Some(NamedRef {
                id: crew_id,
                name: stored_name(&row, "crew_name")?.ok_or_else(|| {
                    StoreError::Backend(format!("key {key_id}'s crew {crew_id} names no crew"))
                })?,
            }),
        };
        let user = match row
            .try_get::<Option<Uuid>, _>("user_root_id")
            .map_err(backend)?
        {
            None => None,
            Some(root) => Some(NamedRef {
                id: root,
                name: stored_name(&row, "user_name")?.ok_or_else(|| {
                    StoreError::Backend(format!("key {key_id}'s user root {root} names no root"))
                })?,
            }),
        };
        Ok(Some(KeyReferences {
            tenant,
            default_workarea,
            crew,
            user,
        }))
    }

    async fn unnamed_rows(&self, tenant_id: Uuid) -> Result<UnnamedRows, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let tenant: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tenants WHERE id = $1 AND name IS NULL)",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        let workareas: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM workareas WHERE tenant_id = $1 AND name IS NULL ORDER BY id",
        )
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let keys: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM api_keys WHERE tenant_id = $1 AND name IS NULL ORDER BY id",
        )
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(UnnamedRows {
            tenant,
            workareas,
            keys,
        })
    }

    async fn workarea(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Option<WorkareaRecord>, StoreError> {
        let sql =
            format!("SELECT {WORKAREA_COLUMNS} FROM workareas WHERE tenant_id = $1 AND id = $2");
        Ok(self
            .workarea_rows(tenant_id, workarea_id, &sql)
            .await?
            .into_iter()
            .next())
    }

    async fn workarea_ancestry(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Option<Vec<WorkareaRecord>>, StoreError> {
        // Up the parent_id links from the node; the depth orders the walk.
        let sql = format!(
            "WITH RECURSIVE up AS ( \
                 SELECT {WORKAREA_COLUMNS}, 0 AS depth FROM workareas \
                  WHERE tenant_id = $1 AND id = $2 \
                 UNION ALL \
                 SELECT w.id, w.tenant_id, w.parent_id, w.name, w.status, w.crew, w.crew_id, \
                        w.user_root, w.namespace_root_id, w.mode, up.depth + 1 \
                   FROM workareas w JOIN up ON w.id = up.parent_id \
                  WHERE w.tenant_id = $1 \
             ) SELECT {WORKAREA_COLUMNS} FROM up ORDER BY depth ASC"
        );
        let ancestry = self.workarea_rows(tenant_id, workarea_id, &sql).await?;
        let Some(top) = ancestry.last() else {
            return Ok(None);
        };
        // A walk that stops short of a root left the tenant; it would drop
        // ancestors' grants and denials, so it is refused. Since 0008 the
        // key (tenant_id, parent_id) refuses such a parent at its insert,
        // having validated every row before it (ADR-0034 d12); this refusal
        // stays as the walk's own guard.
        if let Some(parent_id) = top.parent_id {
            return Err(StoreError::Backend(format!(
                "the ancestry of {workarea_id} names parent {parent_id}, \
                 which this tenant does not hold"
            )));
        }
        Ok(Some(ancestry))
    }

    async fn workarea_subtree(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Vec<WorkareaRecord>, StoreError> {
        let sql = format!(
            "WITH RECURSIVE down AS ( \
                 SELECT {WORKAREA_COLUMNS}, 0 AS depth FROM workareas \
                  WHERE tenant_id = $1 AND id = $2 \
                 UNION ALL \
                 SELECT w.id, w.tenant_id, w.parent_id, w.name, w.status, w.crew, w.crew_id, \
                        w.user_root, w.namespace_root_id, w.mode, down.depth + 1 \
                   FROM workareas w JOIN down ON w.parent_id = down.id \
                  WHERE w.tenant_id = $1 \
             ) SELECT {WORKAREA_COLUMNS} FROM down ORDER BY depth ASC, id ASC"
        );
        self.workarea_rows(tenant_id, workarea_id, &sql).await
    }

    async fn workarea_children(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Vec<WorkareaRecord>, StoreError> {
        // COLLATE "C" compares the UTF-8 bytes, whatever the database's
        // locale, and uuid compares its 16 bytes.
        let sql = format!(
            "SELECT {WORKAREA_COLUMNS} FROM workareas \
             WHERE tenant_id = $1 AND parent_id = $2 \
             ORDER BY name COLLATE \"C\" ASC, id ASC"
        );
        self.workarea_rows(tenant_id, workarea_id, &sql).await
    }

    async fn transition(
        &self,
        tenant_id: Uuid,
        transition_id: Uuid,
    ) -> Result<Option<TransitionRecord>, StoreError> {
        let sql = format!(
            "SELECT {TRANSITION_COLUMNS} FROM workarea_transitions \
             WHERE tenant_id = $1 AND id = $2"
        );
        Ok(self
            .transition_rows(tenant_id, transition_id, &sql)
            .await?
            .into_iter()
            .next())
    }

    async fn pending_transitions(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Vec<TransitionRecord>, StoreError> {
        // The index on (tenant_id, workarea_id, state) serves the filter.
        let sql = format!(
            "SELECT {TRANSITION_COLUMNS} FROM workarea_transitions \
             WHERE tenant_id = $1 AND workarea_id = $2 AND state = 'pending' \
             ORDER BY proposed_seq"
        );
        self.transition_rows(tenant_id, workarea_id, &sql).await
    }

    async fn idempotency_get(
        &self,
        scope: &IdempotencyScope,
    ) -> Result<Option<IdempotencyRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, scope.tenant_id).await?;
        let row = sqlx::query(
            "SELECT request_digest, response FROM idempotency \
             WHERE tenant_id = $1 AND key = $2 AND committed_at IS NOT NULL",
        )
        .bind(scope.tenant_id)
        .bind(scope_key(scope))
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let digest_bytes: Vec<u8> = row.try_get("request_digest").map_err(backend)?;
        let response: Option<Value> = row.try_get("response").map_err(backend)?;
        Ok(Some(IdempotencyRecord {
            request_digest: String::from_utf8(digest_bytes).map_err(backend)?,
            response: response.unwrap_or(Value::Null),
        }))
    }

    async fn idempotency_claim(
        &self,
        scope: &IdempotencyScope,
        request_digest: &str,
        claim_id: Uuid,
    ) -> Result<IdempotencyClaim, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, scope.tenant_id).await?;
        // A claim is a row without `committed_at` whose response names the
        // claim and when it was taken. A concurrent claim's insert waits on
        // the primary key until this one commits, then inserts nothing and
        // reads this row (ADR-0070 d5; #233).
        let claimed = sqlx::query(
            "INSERT INTO idempotency (tenant_id, key, request_digest, response, \
             committed_at) VALUES ($1, $2, $3, \
             jsonb_build_object('claim', $4::text, 'claimed_at', now()), NULL) \
             ON CONFLICT (tenant_id, key) DO NOTHING",
        )
        .bind(scope.tenant_id)
        .bind(scope_key(scope))
        .bind(request_digest.as_bytes())
        .bind(claim_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected()
            == 1;
        let held = if claimed {
            None
        } else {
            sqlx::query(
                "SELECT request_digest, response, committed_at IS NOT NULL AS recorded \
                 FROM idempotency WHERE tenant_id = $1 AND key = $2",
            )
            .bind(scope.tenant_id)
            .bind(scope_key(scope))
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?
        };
        tx.commit().await.map_err(backend)?;
        if claimed {
            return Ok(IdempotencyClaim::Claimed);
        }
        let Some(row) = held else {
            // Released between the insert and the read: the claimant was
            // refused, so this attempt waits and claims again.
            return Ok(IdempotencyClaim::InProgress {
                request_digest: request_digest.to_owned(),
            });
        };
        let digest = String::from_utf8(
            row.try_get::<Vec<u8>, _>("request_digest")
                .map_err(backend)?,
        )
        .map_err(backend)?;
        if row.try_get::<bool, _>("recorded").map_err(backend)? {
            let response: Option<Value> = row.try_get("response").map_err(backend)?;
            Ok(IdempotencyClaim::Recorded(IdempotencyRecord {
                request_digest: digest,
                response: response.unwrap_or(Value::Null),
            }))
        } else {
            Ok(IdempotencyClaim::InProgress {
                request_digest: digest,
            })
        }
    }

    async fn idempotency_settle(
        &self,
        put: IdempotencyPut,
        claim_id: Uuid,
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, put.scope.tenant_id).await?;
        let settled = sqlx::query(
            "UPDATE idempotency SET response = $4, committed_at = now() \
             WHERE tenant_id = $1 AND key = $2 AND request_digest = $3 \
             AND committed_at IS NULL AND response ->> 'claim' = $5",
        )
        .bind(put.scope.tenant_id)
        .bind(scope_key(&put.scope))
        .bind(put.record.request_digest.into_bytes())
        .bind(put.record.response)
        .bind(claim_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        tx.commit().await.map_err(backend)?;
        Ok(settled == 1)
    }

    async fn idempotency_release(
        &self,
        scope: &IdempotencyScope,
        claim_id: Uuid,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, scope.tenant_id).await?;
        sqlx::query(
            "DELETE FROM idempotency WHERE tenant_id = $1 AND key = $2 \
             AND committed_at IS NULL AND response ->> 'claim' = $3",
        )
        .bind(scope.tenant_id)
        .bind(scope_key(scope))
        .bind(claim_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)
    }

    async fn brief_edge(
        &self,
        tenant_id: Uuid,
        edge_id: Uuid,
    ) -> Result<Option<BriefEdgeRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {BRIEF_EDGE_COLUMNS} FROM brief_edges e \
             JOIN api_keys k ON k.tenant_id = e.tenant_id AND k.id = e.proposer_key_id \
             WHERE e.tenant_id = $1 AND e.id = $2"
        ))
        .bind(tenant_id)
        .bind(edge_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(brief_edge_from_row).transpose()
    }

    async fn brief_edges(
        &self,
        tenant_id: Uuid,
        workarea_id: Uuid,
    ) -> Result<Vec<BriefEdgeRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {BRIEF_EDGE_COLUMNS} FROM brief_edges e \
             JOIN api_keys k ON k.tenant_id = e.tenant_id AND k.id = e.proposer_key_id \
             WHERE e.tenant_id = $1 AND e.workarea_id = $2 \
             ORDER BY e.created_at DESC, e.id DESC"
        ))
        .bind(tenant_id)
        .bind(workarea_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(brief_edge_from_row).collect()
    }

    async fn claim_edge(
        &self,
        tenant_id: Uuid,
        edge_id: Uuid,
    ) -> Result<Option<ClaimEdgeRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {CLAIM_EDGE_COLUMNS} FROM claim_edges e \
             JOIN api_keys k ON k.tenant_id = e.tenant_id AND k.id = e.proposer_key_id \
             WHERE e.tenant_id = $1 AND e.id = $2"
        ))
        .bind(tenant_id)
        .bind(edge_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(claim_edge_from_row).transpose()
    }

    async fn claim_edges(
        &self,
        tenant_id: Uuid,
        selection: &ClaimEdgeSelection,
        after: Option<&ClaimEdgePosition>,
        limit: usize,
    ) -> Result<Vec<ClaimEdgeRecord>, StoreError> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        // Each endpoint's row, its scan naming its tenant as every statement
        // on memories does (ADR-0034 d13).
        let mut sql = QueryBuilder::<Postgres>::new(format!(
            "SELECT {CLAIM_EDGE_COLUMNS} FROM claim_edges e \
             JOIN api_keys k ON k.tenant_id = e.tenant_id AND k.id = e.proposer_key_id \
             JOIN memories s ON s.tenant_id = e.tenant_id AND s.id = e.src_artifact_id \
             JOIN memories d ON d.tenant_id = e.tenant_id AND d.id = e.dst_artifact_id \
             WHERE e.tenant_id = "
        ));
        sql.push_bind(tenant_id);
        sql.push(" AND s.tenant_id = ")
            .push_bind(tenant_id)
            .push(" AND d.tenant_id = ")
            .push_bind(tenant_id);
        // Read scope: both endpoints in a reader node, each promoted or the
        // key's own (roles-and-keys.md footnote 5).
        for row in ["s", "d"] {
            sql.push(format!(" AND {row}.workarea_id = ANY("))
                .push_bind(selection.readable_nodes.clone())
                .push(format!(
                    "::uuid[]) AND ({row}.status = 'promoted' OR {row}.author_key_id = "
                ))
                .push_bind(selection.reader_key_id)
                // Inside an in-progress scope, any author's held memory or
                // derived fact the scan did not hold (ADR-0030 d3; ADR-0065
                // d5, amended 2026-11-10).
                .push(format!(
                    " OR ({row}.status = 'held' AND NOT {row}.retirement \
                     AND {row}.scan_finding IS NULL \
                     AND {row}.kind IN ('memory', 'derived_fact') AND {row}.workarea_id = ANY("
                ))
                .push_bind(selection.held_nodes.clone())
                .push(format!(
                    "::uuid[]) AND {row}.scanned_under IS NOT DISTINCT FROM "
                ))
                .push_bind(selection.current_scanner.clone())
                .push("::text))");
        }
        if let Some(artifact_id) = selection.artifact_id {
            sql.push(" AND (e.src_artifact_id = ")
                .push_bind(artifact_id)
                .push(" OR e.dst_artifact_id = ")
                .push_bind(artifact_id)
                .push(")");
        }
        if let Some(within) = &selection.within {
            sql.push(" AND (s.workarea_id = ANY(")
                .push_bind(within.clone())
                .push("::uuid[]) OR d.workarea_id = ANY(")
                .push_bind(within.clone())
                .push("::uuid[]))");
        }
        // Keyset: strictly after the position in the listing's order.
        if let Some(after) = after {
            sql.push(" AND (e.created_at, e.id) < ((")
                .push_bind(after.created_at.as_str().to_owned())
                .push(")::timestamptz, ")
                .push_bind(after.edge_id)
                .push(")");
        }
        sql.push(" ORDER BY e.created_at DESC, e.id DESC LIMIT ")
            .push_bind(limit);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sql.build().fetch_all(&mut *tx).await.map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(claim_edge_from_row).collect()
    }

    async fn ratified_term_edges(
        &self,
        tenant_id: Uuid,
        label: &str,
    ) -> Result<Vec<(String, String)>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // Both endpoints are terms; each memories scan names its tenant
        // (ADR-0034 d13). Byte order on the pair, as the port asks.
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT s.claim_key, d.claim_key FROM claim_edges e \
             JOIN memories s ON s.tenant_id = $1 AND s.id = e.src_artifact_id \
                            AND s.kind = 'term' \
             JOIN memories d ON d.tenant_id = $1 AND d.id = e.dst_artifact_id \
                            AND d.kind = 'term' \
             WHERE e.tenant_id = $1 AND e.state = 'ratified' AND e.label = $2 \
             ORDER BY s.claim_key COLLATE \"C\", d.claim_key COLLATE \"C\"",
        )
        .bind(tenant_id)
        .bind(label)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(rows)
    }
}

/// A `timestamptz` column in the fixed-width signed form, so `EventTime`
/// round-trips exactly, as a key's `revoked_at` does.
macro_rules! utc {
    ($column:literal) => {
        concat!(
            "to_char(",
            $column,
            " AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')"
        )
    };
}

const CODE_COLUMNS: &str = concat!(
    "id, key_id, hash, issued_seq, voided_seq, ",
    utc!("expires_at"),
    " AS expires_at_signed, ",
    utc!("redeemed_at"),
    " AS redeemed_at_signed"
);

const CONNECTION_COLUMNS: &str = concat!(
    "id, key_id, code_id, client_id, client_kind, client_metadata_digest, redirect_uri, \
     redirect_uri_sent, resource, code_challenge, auth_code_hash, state, revoked_cause, \
     opened_seq, revoked_seq, ",
    utc!("auth_code_expires_at"),
    " AS auth_code_expires_at_signed, ",
    utc!("auth_code_redeemed_at"),
    " AS auth_code_redeemed_at_signed, ",
    utc!("created_at"),
    " AS created_at_signed"
);

fn event_time_column(row: &PgRow, column: &str) -> Result<Option<EventTime>, StoreError> {
    let at: Option<String> = row.try_get(column).map_err(backend)?;
    at.as_deref()
        .map(EventTime::parse)
        .transpose()
        .map_err(backend)
}

fn optional_seq(row: &PgRow, column: &str) -> Result<Option<u64>, StoreError> {
    let seq: Option<i64> = row.try_get(column).map_err(backend)?;
    seq.map(u64::try_from).transpose().map_err(backend)
}

fn code_from_row(row: &PgRow) -> Result<ConnectCodeRecord, StoreError> {
    let hash: Vec<u8> = row.try_get("hash").map_err(backend)?;
    Ok(ConnectCodeRecord {
        code_id: row.try_get("id").map_err(backend)?,
        key_id: row.try_get("key_id").map_err(backend)?,
        code_hash: hash32(&hash)?,
        expires_at: event_time_column(row, "expires_at_signed")?
            .ok_or_else(|| StoreError::Backend("connect_codes.expires_at is null".to_owned()))?,
        issued_seq: seq_from_column(row, "issued_seq")?,
        redeemed_at: event_time_column(row, "redeemed_at_signed")?,
        voided_seq: optional_seq(row, "voided_seq")?,
    })
}

/// A `client_kind` or `kind` column of `table` as its [`ClientKind`].
fn client_kind_column(row: &PgRow, table: &str, column: &str) -> Result<ClientKind, StoreError> {
    let kind: String = row.try_get(column).map_err(backend)?;
    match kind.as_str() {
        "cimd" => Ok(ClientKind::Cimd),
        "dcr" => Ok(ClientKind::Dcr),
        other => Err(StoreError::Backend(format!(
            "{table}.{column} {other:?} is not a client kind"
        ))),
    }
}

const AUTHORIZATION_COLUMNS: &str = concat!(
    "id, handle_hash, client_id, client_kind, metadata_digest, redirect_uri, redirect_uri_sent, \
     code_challenge, state, code_id, code_tenant_id, key_id, ",
    utc!("expires_at"),
    " AS expires_at_signed"
);

fn authorization_from_row(row: &PgRow) -> Result<AuthorizationRecord, StoreError> {
    let handle: Vec<u8> = row.try_get("handle_hash").map_err(backend)?;
    let digest: Vec<u8> = row.try_get("metadata_digest").map_err(backend)?;
    Ok(AuthorizationRecord {
        authorization_id: row.try_get("id").map_err(backend)?,
        handle_hash: hash32(&handle)?,
        client_id: row.try_get("client_id").map_err(backend)?,
        client_kind: client_kind_column(row, "oauth_authorizations", "client_kind")?,
        metadata_digest: hash32(&digest)?,
        redirect_uri: row.try_get("redirect_uri").map_err(backend)?,
        redirect_uri_sent: row.try_get("redirect_uri_sent").map_err(backend)?,
        code_challenge: row.try_get("code_challenge").map_err(backend)?,
        state: row.try_get("state").map_err(backend)?,
        expires_at: event_time_column(row, "expires_at_signed")?.ok_or_else(|| {
            StoreError::Backend("oauth_authorizations.expires_at is null".to_owned())
        })?,
        code_id: row.try_get("code_id").map_err(backend)?,
        code_tenant_id: row.try_get("code_tenant_id").map_err(backend)?,
        key_id: row.try_get("key_id").map_err(backend)?,
    })
}

/// The class of the transaction-scoped advisory locks that serialize the
/// client caps: the document cache's count and eviction, and the count of
/// unused registrations, so two racing inserts cannot both pass a cap at
/// its last free row (ADR-0031 d5). The two-key form never meets a tenant's
/// one-key chain lock.
const CLIENT_CAP_LOCK: i32 = 0x0031_0005;
/// The document cache's lock within [`CLIENT_CAP_LOCK`].
const CIMD_CAP_LOCK: i32 = 1;
/// The unused registrations' lock within [`CLIENT_CAP_LOCK`].
const DCR_CAP_LOCK: i32 = 2;

async fn lock_client_cap(tx: &mut Transaction<'_, Postgres>, which: i32) -> Result<(), StoreError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(CLIENT_CAP_LOCK)
        .bind(which)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

const CLIENT_COLUMNS: &str = concat!(
    "client_id, kind, client_name, redirect_uris, metadata_digest, max_age, ",
    utc!("fetched_at"),
    " AS fetched_at_signed, ",
    utc!("last_used_at"),
    " AS last_used_at_signed, ",
    utc!("connected_at"),
    " AS connected_at_signed, ",
    utc!("created_at"),
    " AS created_at_signed"
);

fn client_from_row(row: &PgRow) -> Result<OAuthClientRecord, StoreError> {
    let digest: Vec<u8> = row.try_get("metadata_digest").map_err(backend)?;
    Ok(OAuthClientRecord {
        client_id: row.try_get("client_id").map_err(backend)?,
        kind: client_kind_column(row, "oauth_clients", "kind")?,
        client_name: row.try_get("client_name").map_err(backend)?,
        redirect_uris: row.try_get("redirect_uris").map_err(backend)?,
        metadata_digest: hash32(&digest)?,
        fetched_at: event_time_column(row, "fetched_at_signed")?,
        max_age: row
            .try_get::<Option<i32>, _>("max_age")
            .map_err(backend)?
            .map(u32::try_from)
            .transpose()
            .map_err(backend)?,
        last_used_at: event_time_column(row, "last_used_at_signed")?,
        connected_at: event_time_column(row, "connected_at_signed")?,
        created_at: event_time_column(row, "created_at_signed")?
            .ok_or_else(|| StoreError::Backend("oauth_clients.created_at is null".to_owned()))?,
    })
}

fn connection_from_row(row: &PgRow) -> Result<ConnectionRecord, StoreError> {
    let client_kind = client_kind_column(row, "oauth_connections", "client_kind")?;
    let state: String = row.try_get("state").map_err(backend)?;
    let cause: Option<String> = row.try_get("revoked_cause").map_err(backend)?;
    let state = match (state.as_str(), cause.as_deref()) {
        ("authorized", None) => ConnectionState::Authorized,
        ("open", None) => ConnectionState::Open,
        ("revoked", Some("admin")) => ConnectionState::Revoked(RevokedCause::Admin),
        ("revoked", Some("refresh_reuse")) => ConnectionState::Revoked(RevokedCause::RefreshReuse),
        ("revoked", Some("code_reuse")) => ConnectionState::Revoked(RevokedCause::CodeReuse),
        ("revoked", Some("replaced")) => ConnectionState::Revoked(RevokedCause::Replaced),
        (state, cause) => {
            return Err(StoreError::Backend(format!(
                "oauth_connections state {state:?} with cause {cause:?} is not a connection state"
            )));
        }
    };
    let digest: Vec<u8> = row.try_get("client_metadata_digest").map_err(backend)?;
    let auth_code_hash: Option<Vec<u8>> = row.try_get("auth_code_hash").map_err(backend)?;
    Ok(ConnectionRecord {
        connection_id: row.try_get("id").map_err(backend)?,
        key_id: row.try_get("key_id").map_err(backend)?,
        code_id: row.try_get("code_id").map_err(backend)?,
        client_id: row.try_get("client_id").map_err(backend)?,
        client_kind,
        client_metadata_digest: hash32(&digest)?,
        redirect_uri: row.try_get("redirect_uri").map_err(backend)?,
        redirect_uri_sent: row.try_get("redirect_uri_sent").map_err(backend)?,
        resource: row.try_get("resource").map_err(backend)?,
        code_challenge: row.try_get("code_challenge").map_err(backend)?,
        auth_code_hash: auth_code_hash.as_deref().map(hash32).transpose()?,
        auth_code_expires_at: event_time_column(row, "auth_code_expires_at_signed")?,
        auth_code_redeemed_at: event_time_column(row, "auth_code_redeemed_at_signed")?,
        state,
        opened_seq: optional_seq(row, "opened_seq")?,
        revoked_seq: optional_seq(row, "revoked_seq")?,
        created_at: event_time_column(row, "created_at_signed")?.ok_or_else(|| {
            StoreError::Backend("oauth_connections.created_at is null".to_owned())
        })?,
    })
}

const TOKEN_COLUMNS: &str = concat!(
    "hash, connection_id, kind, ",
    utc!("expires_at"),
    " AS expires_at_signed, ",
    utc!("rotated_at"),
    " AS rotated_at_signed, ",
    utc!("created_at"),
    " AS created_at_signed"
);

fn token_from_row(row: &PgRow) -> Result<TokenRecord, StoreError> {
    let hash: Vec<u8> = row.try_get("hash").map_err(backend)?;
    let kind: String = row.try_get("kind").map_err(backend)?;
    let kind = match kind.as_str() {
        "access" => TokenKind::Access,
        "refresh" => TokenKind::Refresh,
        other => {
            return Err(StoreError::Backend(format!(
                "oauth_tokens.kind {other:?} is not a token kind"
            )));
        }
    };
    Ok(TokenRecord {
        token_hash: hash32(&hash)?,
        connection_id: row.try_get("connection_id").map_err(backend)?,
        kind,
        expires_at: event_time_column(row, "expires_at_signed")?
            .ok_or_else(|| StoreError::Backend("oauth_tokens.expires_at is null".to_owned()))?,
        rotated_at: event_time_column(row, "rotated_at_signed")?,
        created_at: event_time_column(row, "created_at_signed")?
            .ok_or_else(|| StoreError::Backend("oauth_tokens.created_at is null".to_owned()))?,
    })
}

fn token_kind_str(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::Access => "access",
        TokenKind::Refresh => "refresh",
    }
}

/// Opens the pre-tenant lookup in `tx`: the `key_resolution` policies of
/// 0012 show exactly the rows whose digest equals `oml.api_key_hash`, with
/// no `oml.tenant_id` set, as `key_by_hash` finds `api_keys.hash`
/// (postgres-schema.md "connections (ADR-0031)").
async fn set_lookup_digest(
    tx: &mut Transaction<'_, Postgres>,
    digest: &Sha256Hash,
) -> Result<(), StoreError> {
    sqlx::query("SELECT set_config('oml.api_key_hash', $1, true)")
        .bind(hex::encode(digest.0))
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

/// Whether the entry `hash` names is the tenant's only admin entry, read
/// with every admin entry locked in this transaction, so two commits that
/// would each leave the other the last admin are serialized and the second
/// sees the first's change (ADR-0045 d10; ADR-0070 d5).
async fn last_admin_entry(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    hash: &[u8],
) -> Result<bool, StoreError> {
    let admins: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT email_hash FROM web_emails WHERE tenant_id = $1 AND admin \
         ORDER BY email_hash FOR UPDATE",
    )
    .bind(tenant_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(admins.iter().any(|h| h == hash) && admins.iter().all(|h| h == hash))
}

/// Ends every session of one allowlist entry, in the commit that changes
/// or removes it (ADR-0045 d5).
async fn end_entry_sessions(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    email_hash: &[u8],
) -> Result<(), StoreError> {
    sqlx::query("DELETE FROM web_sessions WHERE tenant_id = $1 AND email_hash = $2")
        .bind(tenant_id)
        .bind(email_hash)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

const WEB_ENTRY_COLUMNS: &str = concat!(
    "tenant_id, email, user_name, admin, google_subject, github_subject, added_by, ",
    "terms_version, ",
    utc!("disabled_at"),
    " AS disabled_at_signed"
);

fn web_entry_from_row(row: &PgRow) -> Result<WebEntry, StoreError> {
    let email: String = row.try_get("email").map_err(backend)?;
    let user: String = row.try_get("user_name").map_err(backend)?;
    Ok(WebEntry {
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        email: Email::parse(&email).map_err(backend)?,
        user: Name::parse_reference(&user).map_err(backend)?,
        admin: row.try_get("admin").map_err(backend)?,
        google_subject: row.try_get("google_subject").map_err(backend)?,
        github_subject: row.try_get("github_subject").map_err(backend)?,
        added_by: row.try_get("added_by").map_err(backend)?,
        terms_version: row.try_get("terms_version").map_err(backend)?,
        disabled_at: event_time_column(row, "disabled_at_signed")?,
    })
}

const WEB_SESSION_COLUMNS: &str = concat!(
    "token_hash, tenant_id, email_hash, user_name, admin, ",
    utc!("created_at"),
    " AS created_at_signed, ",
    utc!("expires_at"),
    " AS expires_at_signed"
);

fn web_session_from_row(row: &PgRow) -> Result<WebSessionRecord, StoreError> {
    let token_hash: Vec<u8> = row.try_get("token_hash").map_err(backend)?;
    let email_hash: Vec<u8> = row.try_get("email_hash").map_err(backend)?;
    let user: String = row.try_get("user_name").map_err(backend)?;
    Ok(WebSessionRecord {
        token_hash: hash32(&token_hash)?,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        email_hash: hash32(&email_hash)?,
        user: Name::parse_reference(&user).map_err(backend)?,
        admin: row.try_get("admin").map_err(backend)?,
        created_at: event_time_column(row, "created_at_signed")?
            .ok_or_else(|| StoreError::Backend("web_sessions.created_at is null".to_owned()))?,
        expires_at: event_time_column(row, "expires_at_signed")?
            .ok_or_else(|| StoreError::Backend("web_sessions.expires_at is null".to_owned()))?,
    })
}

impl PgStore {
    /// The tenant's keys a `predicate` on `$2` names, in name order.
    async fn keys_where<B>(
        &self,
        tenant_id: Uuid,
        predicate: &str,
        bind: B,
    ) -> Result<Vec<KeyRecord>, StoreError>
    where
        B: for<'q> FnOnce(
            sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>,
        ) -> sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>,
    {
        let sql = format!(
            "SELECT {KEY_COLUMNS} FROM api_keys WHERE tenant_id = $1 AND {predicate} ORDER BY name"
        );
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = bind(sqlx::query(&sql).bind(tenant_id))
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(key_from_row).collect()
    }
}

/// The web rows (ADR-0045 d2, d4, d5): the allowlist's reads and its
/// pre-tenant lookup by the email's digest, the pin, and the session rows,
/// each in a transaction of its own under `oml.tenant_id`, the digest
/// lookups under 0015's `key_resolution` policies.
impl WebStore for PgStore {
    async fn web_entry_by_email(
        &self,
        email_hash: &Sha256Hash,
    ) -> Result<Option<WebEntry>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_lookup_digest(&mut tx, email_hash).await?;
        let row = sqlx::query(&format!(
            "SELECT {WEB_ENTRY_COLUMNS} FROM web_emails WHERE email_hash = $1"
        ))
        .bind(email_hash.0.to_vec())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(web_entry_from_row).transpose()
    }

    async fn web_entries(
        &self,
        tenant_id: Uuid,
        after: Option<&Sha256Hash>,
        limit: usize,
    ) -> Result<Vec<WebEntry>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {WEB_ENTRY_COLUMNS} FROM web_emails \
             WHERE tenant_id = $1 AND ($2::bytea IS NULL OR email_hash > $2) \
             ORDER BY email_hash LIMIT $3"
        ))
        .bind(tenant_id)
        .bind(after.map(|a| a.0.to_vec()))
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(web_entry_from_row).collect()
    }

    async fn web_pin(
        &self,
        tenant_id: Uuid,
        email_hash: &Sha256Hash,
        provider: Provider,
        subject: &str,
    ) -> Result<PinOutcome, StoreError> {
        let column = match provider {
            Provider::Google => "google_subject",
            Provider::GitHub => "github_subject",
        };
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // One conditional update pins a first subject; the row read back
        // says whether this call pinned it, it matched, or another is held.
        let pinned = sqlx::query(&format!(
            "UPDATE web_emails SET {column} = $3 \
             WHERE tenant_id = $1 AND email_hash = $2 AND {column} IS NULL"
        ))
        .bind(tenant_id)
        .bind(email_hash.0.to_vec())
        .bind(subject)
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        let held: Option<Option<String>> = sqlx::query_scalar(&format!(
            "SELECT {column} FROM web_emails WHERE tenant_id = $1 AND email_hash = $2"
        ))
        .bind(tenant_id)
        .bind(email_hash.0.to_vec())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        match held {
            None => Err(StoreError::Backend("no entry to pin".to_owned())),
            Some(_) if pinned == 1 => Ok(PinOutcome::Pinned),
            Some(Some(held)) if held == subject => Ok(PinOutcome::Matched),
            Some(_) => Ok(PinOutcome::Differs),
        }
    }

    async fn web_session_open(
        &self,
        session: WebSessionRecord,
        replaces: Option<Sha256Hash>,
        credentials: Vec<(&'static str, Sha256Hash)>,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, session.tenant_id).await?;
        if let Some(old) = replaces {
            // The old row may be another tenant's after an entry moved;
            // under this tenant's fence only its own row is ended, and a
            // row elsewhere lapses at its expiry.
            sqlx::query("DELETE FROM web_sessions WHERE tenant_id = $1 AND token_hash = $2")
                .bind(session.tenant_id)
                .bind(old.0.to_vec())
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
        }
        sqlx::query(
            "INSERT INTO web_sessions (token_hash, tenant_id, email_hash, user_name, admin, \
             created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5, ($6)::timestamptz, ($7)::timestamptz)",
        )
        .bind(session.token_hash.0.to_vec())
        .bind(session.tenant_id)
        .bind(session.email_hash.0.to_vec())
        .bind(session.user.as_str())
        .bind(session.admin)
        .bind(session.created_at.as_str())
        .bind(session.expires_at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        // The capacity credentials' digests, in the commit that opens the
        // session; the foreign key ends them with it (ADR-0070 d2).
        for (capacity, digest) in credentials {
            sqlx::query(
                "INSERT INTO web_session_credentials (credential_hash, tenant_id, token_hash, \
                 capacity) VALUES ($1, $2, $3, $4)",
            )
            .bind(digest.0.to_vec())
            .bind(session.tenant_id)
            .bind(session.token_hash.0.to_vec())
            .bind(capacity)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn web_session_credential(
        &self,
        credential_hash: &Sha256Hash,
    ) -> Result<Option<(WebSessionRecord, String)>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_lookup_digest(&mut tx, credential_hash).await?;
        let found = sqlx::query(
            "SELECT tenant_id, token_hash, capacity FROM web_session_credentials \
             WHERE credential_hash = $1",
        )
        .bind(credential_hash.0.to_vec())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(found) = found else {
            tx.commit().await.map_err(backend)?;
            return Ok(None);
        };
        let tenant_id: Uuid = found.try_get("tenant_id").map_err(backend)?;
        let token_hash: Vec<u8> = found.try_get("token_hash").map_err(backend)?;
        let capacity: String = found.try_get("capacity").map_err(backend)?;
        // The matched row's tenant fences the session's read.
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {WEB_SESSION_COLUMNS} FROM web_sessions WHERE tenant_id = $1 AND token_hash = $2"
        ))
        .bind(tenant_id)
        .bind(token_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref()
            .map(web_session_from_row)
            .transpose()
            .map(|s| s.map(|s| (s, capacity)))
    }

    async fn web_session(
        &self,
        token_hash: &Sha256Hash,
    ) -> Result<Option<WebSessionRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_lookup_digest(&mut tx, token_hash).await?;
        let row = sqlx::query(&format!(
            "SELECT {WEB_SESSION_COLUMNS} FROM web_sessions WHERE token_hash = $1"
        ))
        .bind(token_hash.0.to_vec())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(web_session_from_row).transpose()
    }

    async fn web_session_end(
        &self,
        tenant_id: Uuid,
        token_hash: &Sha256Hash,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        sqlx::query("DELETE FROM web_sessions WHERE tenant_id = $1 AND token_hash = $2")
            .bind(tenant_id)
            .bind(token_hash.0.to_vec())
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn web_user_keys(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> Result<Vec<KeyRecord>, StoreError> {
        self.keys_where(tenant_id, "user_root_id = $2", |q| q.bind(user_root))
            .await
    }

    async fn web_session_keys(
        &self,
        tenant_id: Uuid,
        session_user: &Name,
    ) -> Result<Vec<KeyRecord>, StoreError> {
        let user = session_user.as_str().to_owned();
        self.keys_where(tenant_id, "session_for = $2", move |q| q.bind(user))
            .await
    }

    async fn web_queue(&self, tenant_id: Uuid) -> Result<Vec<QueueDepth>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(concat!(
            "SELECT workarea_id, count(*) AS held, ",
            utc!("min(created_at)"),
            " AS oldest_signed FROM memories WHERE tenant_id = $1 AND status = 'held' \
             GROUP BY workarea_id ORDER BY workarea_id"
        ))
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter()
            .map(|row| {
                let held: i64 = row.try_get("held").map_err(backend)?;
                Ok(QueueDepth {
                    workarea_id: row.try_get("workarea_id").map_err(backend)?,
                    held: u64::try_from(held).map_err(backend)?,
                    oldest: event_time_column(row, "oldest_signed")?.ok_or_else(|| {
                        StoreError::Backend("a held group has no oldest row".to_owned())
                    })?,
                })
            })
            .collect()
    }

    async fn web_admin_search(
        &self,
        tenant_id: Uuid,
        search: &AdminSearch,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let state = match search.state {
            None => "TRUE",
            Some(AdminState::Held) => "m.status = 'held'",
            Some(AdminState::Promoted) => "m.status = 'promoted' AND m.is_tip",
            Some(AdminState::Superseded) => "m.status = 'promoted' AND NOT m.is_tip",
            Some(AdminState::Rejected) => "m.status = 'rejected'",
            Some(AdminState::Demoted) => "m.status IN ('demoted', 'erased')",
        };
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = $1 \
             AND ($2::text IS NULL OR to_tsvector('simple', COALESCE(m.search_text, m.claim)) @@ plainto_tsquery('simple', $2)) \
             AND ($3::uuid[] IS NULL OR m.workarea_id = ANY($3)) \
             AND {state} \
             AND ($4::timestamptz IS NULL OR (m.created_at, m.id) < ($4::timestamptz, $5::uuid)) \
             AND (cardinality($7::text[]) = 0 OR m.origin_kind = ANY($7)) \
             AND ($8::uuid IS NULL OR m.author_key_id = $8) \
             ORDER BY m.created_at DESC, m.id DESC LIMIT $6"
        );
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(search.query.as_deref())
            .bind(search.nodes.clone())
            .bind(
                search
                    .after
                    .as_ref()
                    .map(|a| a.created_at.as_str().to_owned()),
            )
            .bind(search.after.as_ref().map(|a| a.artifact_id))
            .bind(i64::try_from(search.limit).unwrap_or(i64::MAX))
            .bind(origin_kind_strings(&search.origin_kinds))
            .bind(search.author)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(memory_from_row).collect()
    }

    async fn web_deletion_requests(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<oml_app::types::DeletionRequest>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(concat!(
            "SELECT user_name, ",
            utc!("requested_at"),
            " AS requested_at_signed FROM web_deletion_requests \
             WHERE tenant_id = $1 AND closed_at IS NULL ORDER BY requested_at"
        ))
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter()
            .map(|row| {
                let user: String = row.try_get("user_name").map_err(backend)?;
                Ok(oml_app::types::DeletionRequest {
                    user: Name::parse_reference(&user).map_err(backend)?,
                    requested_at: event_time_column(row, "requested_at_signed")?
                        .ok_or_else(|| StoreError::Backend("requested_at is null".to_owned()))?,
                })
            })
            .collect()
    }

    async fn web_sessions_sweep(&self, tenant_id: Uuid, at: &EventTime) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let removed = sqlx::query(
            "DELETE FROM web_sessions WHERE tenant_id = $1 AND expires_at <= ($2)::timestamptz",
        )
        .bind(tenant_id)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        tx.commit().await.map_err(backend)?;
        Ok(removed)
    }
}

/// The connection rows under the tenant's RLS, and the provider tenant's
/// client and authorization-request rows under its own (postgres-schema.md
/// "connections (ADR-0031)"). The journalled acts are folds in
/// `apply_fold`; these are the operational writes and the reads, each in a
/// transaction of its own under `oml.tenant_id`, Connect's one transaction
/// switching it from the provider tenant to the code's (ADR-0031 d5, d8,
/// d9, d12).
impl ConnectionStore for PgStore {
    async fn code_by_hash(
        &self,
        code_hash: &Sha256Hash,
    ) -> Result<Option<TenantRow<ConnectCodeRecord>>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_lookup_digest(&mut tx, code_hash).await?;
        let row = sqlx::query(&format!(
            "SELECT tenant_id, {CODE_COLUMNS} FROM connect_codes WHERE hash = $1"
        ))
        .bind(code_hash.0.to_vec())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref()
            .map(|row| {
                Ok(TenantRow {
                    tenant_id: row.try_get("tenant_id").map_err(backend)?,
                    row: code_from_row(row)?,
                })
            })
            .transpose()
    }

    async fn take_authorization_by_handle(
        &self,
        handle_hash: &Sha256Hash,
    ) -> Result<Option<AuthorizationRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        let row = sqlx::query(&format!(
            "DELETE FROM oauth_authorizations WHERE tenant_id = $1 AND handle_hash = $2 \
             RETURNING {AUTHORIZATION_COLUMNS}"
        ))
        .bind(PROVIDER_TENANT)
        .bind(handle_hash.0.to_vec())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let request = row.as_ref().map(authorization_from_row).transpose()?;
        tx.commit().await.map_err(backend)?;
        Ok(request)
    }

    async fn connection_by_auth_code(
        &self,
        auth_code_hash: &Sha256Hash,
    ) -> Result<Option<TenantRow<ConnectionRecord>>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_lookup_digest(&mut tx, auth_code_hash).await?;
        let row = sqlx::query(&format!(
            "SELECT tenant_id, {CONNECTION_COLUMNS} FROM oauth_connections \
             WHERE auth_code_hash = $1"
        ))
        .bind(auth_code_hash.0.to_vec())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref()
            .map(|row| {
                Ok(TenantRow {
                    tenant_id: row.try_get("tenant_id").map_err(backend)?,
                    row: connection_from_row(row)?,
                })
            })
            .transpose()
    }

    async fn token_by_hash(
        &self,
        token_hash: &Sha256Hash,
        kind: TokenKind,
    ) -> Result<Option<TenantRow<TokenRecord>>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_lookup_digest(&mut tx, token_hash).await?;
        // At /mcp only an access row authenticates, at a refresh only a
        // refresh row (roles-and-keys.md connections; ADR-0031 d11).
        let row = sqlx::query(&format!(
            "SELECT tenant_id, {TOKEN_COLUMNS} FROM oauth_tokens WHERE hash = $1 AND kind = $2"
        ))
        .bind(token_hash.0.to_vec())
        .bind(token_kind_str(kind))
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref()
            .map(|row| {
                Ok(TenantRow {
                    tenant_id: row.try_get("tenant_id").map_err(backend)?,
                    row: token_from_row(row)?,
                })
            })
            .transpose()
    }

    async fn insert_token_pair(
        &self,
        tenant_id: Uuid,
        connection_id: Uuid,
        tokens: TokenPair,
        at: &EventTime,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The connection's row lock holds it open until the rows commit.
        let open: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM oauth_connections \
             WHERE tenant_id = $1 AND id = $2 AND state = 'open' FOR UPDATE",
        )
        .bind(tenant_id)
        .bind(connection_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if open.is_none() {
            return Err(StoreError::Backend(format!(
                "connection {connection_id} is not an open connection of this tenant"
            )));
        }
        insert_tokens(&mut tx, tenant_id, connection_id, tokens, at).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn rotate_refresh<F>(
        &self,
        tenant_id: Uuid,
        token_hash: &Sha256Hash,
        next: TokenPair,
        at: &EventTime,
        decide: F,
    ) -> Result<RefreshOutcome, StoreError>
    where
        F: FnOnce(Option<&RefreshFacts>) -> RefreshDecision + Send,
    {
        // One transaction without the chain lock (postgres-schema.md,
        // redemption and rotation are conditional updates).
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let hash = token_hash.0.to_vec();
        let connection_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT connection_id FROM oauth_tokens \
             WHERE tenant_id = $1 AND hash = $2 AND kind = 'refresh'",
        )
        .bind(tenant_id)
        .bind(&hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(connection_id) = connection_id else {
            tx.commit().await.map_err(backend)?;
            return Ok(RefreshOutcome {
                decision: decide(None),
                facts: None,
            });
        };
        let connection = sqlx::query(&format!(
            "SELECT {CONNECTION_COLUMNS} FROM oauth_connections \
             WHERE tenant_id = $1 AND id = $2 FOR UPDATE"
        ))
        .bind(tenant_id)
        .bind(connection_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        let connection = connection_from_row(&connection)?;
        // Read after the lock, so a racing refresh's rotation is seen.
        let (rotated, expired): (bool, bool) = sqlx::query_as(
            "SELECT rotated_at IS NOT NULL, expires_at <= ($3)::timestamptz FROM oauth_tokens \
             WHERE tenant_id = $1 AND hash = $2 AND kind = 'refresh'",
        )
        .bind(tenant_id)
        .bind(&hash)
        .bind(at.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        let live: bool = sqlx::query_scalar(
            "SELECT revoked_at IS NULL FROM api_keys WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant_id)
        .bind(connection.key_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        .unwrap_or(false);
        let facts = RefreshFacts {
            connection_id,
            connection: connection.state,
            client_id: connection.client_id,
            resource: connection.resource,
            key: if live {
                KeyState::Active
            } else {
                KeyState::Revoked
            },
            rotated,
            expired,
        };
        let decision = decide(Some(&facts));
        if decision == RefreshDecision::Rotate {
            let matched: Option<Uuid> = sqlx::query_scalar(
                "UPDATE oauth_tokens SET rotated_at = ($3)::timestamptz \
                 WHERE tenant_id = $1 AND hash = $2 AND kind = 'refresh' \
                 AND rotated_at IS NULL AND expires_at > ($3)::timestamptz \
                 RETURNING connection_id",
            )
            .bind(tenant_id)
            .bind(&hash)
            .bind(at.as_str())
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
            if matched.is_none() {
                return Err(StoreError::Backend(
                    "the refresh token moved under its connection's lock".to_owned(),
                ));
            }
            insert_tokens(&mut tx, tenant_id, connection_id, next, at).await?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(RefreshOutcome {
            decision,
            facts: Some(facts),
        })
    }

    async fn sweep_expired(
        &self,
        tenant_id: Uuid,
        at: &EventTime,
    ) -> Result<SweepReport, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let connections = sqlx::query(
            "DELETE FROM oauth_connections WHERE tenant_id = $1 AND state = 'authorized' \
             AND auth_code_expires_at <= ($2)::timestamptz",
        )
        .bind(tenant_id)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let tokens = sqlx::query(
            "DELETE FROM oauth_tokens t WHERE t.tenant_id = $1 \
             AND (t.expires_at <= ($2)::timestamptz \
                  OR EXISTS (SELECT 1 FROM oauth_connections c \
                              WHERE c.tenant_id = t.tenant_id AND c.id = t.connection_id \
                                AND c.state = 'revoked'))",
        )
        .bind(tenant_id)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let codes = sqlx::query(
            "UPDATE oauth_connections SET auth_code_hash = NULL \
             WHERE tenant_id = $1 AND state = 'revoked' AND auth_code_hash IS NOT NULL",
        )
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        let authorizations = sqlx::query(
            "DELETE FROM oauth_authorizations WHERE tenant_id = $1 \
             AND expires_at + interval '24 hours' <= ($2)::timestamptz",
        )
        .bind(PROVIDER_TENANT)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(SweepReport {
            connections_removed: connections.rows_affected(),
            tokens_removed: tokens.rows_affected(),
            codes_cleared: codes.rows_affected(),
            authorizations_removed: authorizations.rows_affected(),
        })
    }

    async fn tenant_name(&self, tenant_id: Uuid) -> Result<Option<Name>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query("SELECT name FROM tenants WHERE id = $1")
            .bind(tenant_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(match row {
            Some(row) => stored_name(&row, "name")?,
            None => None,
        })
    }

    async fn spend_code(
        &self,
        tenant_id: Uuid,
        code_id: Uuid,
        at: &EventTime,
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The one conditional update two racing entries cannot both pass.
        let spent = sqlx::query(
            "UPDATE connect_codes SET redeemed_at = ($3)::timestamptz \
             WHERE tenant_id = $1 AND id = $2 AND redeemed_at IS NULL AND voided_seq IS NULL \
             AND expires_at > ($3)::timestamptz",
        )
        .bind(tenant_id)
        .bind(code_id)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(spent.rows_affected() == 1)
    }

    async fn connection_connect(
        &self,
        handle_hash: &Sha256Hash,
        grant: ConnectGrant,
    ) -> Result<ConnectOutcome, StoreError> {
        // One transaction, rolled back as a whole on any refusal: dropping
        // `tx` without a commit rolls it back (postgres-schema.md
        // "connections (ADR-0031)").
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        let row = sqlx::query(&format!(
            "DELETE FROM oauth_authorizations WHERE tenant_id = $1 AND handle_hash = $2 \
             RETURNING {AUTHORIZATION_COLUMNS}"
        ))
        .bind(PROVIDER_TENANT)
        .bind(handle_hash.0.to_vec())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(row) = row else {
            return Ok(ConnectOutcome::NoRequest);
        };
        let request = authorization_from_row(&row)?;
        // The stamp commits only with the connection row (ADR-0031 d5).
        sqlx::query(
            "UPDATE oauth_clients SET connected_at = ($3)::timestamptz \
             WHERE tenant_id = $1 AND client_id = $2 AND connected_at IS NULL",
        )
        .bind(PROVIDER_TENANT)
        .bind(&request.client_id)
        .bind(grant.at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let tenant_id = request.code_tenant_id;
        set_tenant(&mut tx, tenant_id).await?;
        let voided: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT voided_seq FROM connect_codes WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant_id)
        .bind(request.code_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        match voided {
            None => {
                return Err(StoreError::Backend(format!(
                    "connection {} names a code its tenant does not hold",
                    grant.connection_id
                )));
            }
            Some(Some(_)) => return Ok(ConnectOutcome::CodeVoided),
            Some(None) => {}
        }
        let connection = ConnectionRecord {
            connection_id: grant.connection_id,
            key_id: request.key_id,
            code_id: request.code_id,
            client_id: request.client_id,
            client_kind: request.client_kind,
            client_metadata_digest: request.metadata_digest,
            redirect_uri: request.redirect_uri,
            redirect_uri_sent: request.redirect_uri_sent,
            resource: grant.resource,
            code_challenge: request.code_challenge,
            auth_code_hash: Some(grant.auth_code_hash),
            auth_code_expires_at: Some(grant.auth_code_expires_at),
            auth_code_redeemed_at: None,
            state: ConnectionState::Authorized,
            opened_seq: None,
            revoked_seq: None,
            created_at: grant.at,
        };
        // A key or code the tenant does not hold meets the keys (tenant_id,
        // key_id) and (tenant_id, code_id), and a second connection of one
        // code meets oauth_connections_tenant_id_code_id (ADR-0034 d12).
        sqlx::query(
            "INSERT INTO oauth_connections (id, tenant_id, key_id, code_id, client_id, \
             client_kind, client_metadata_digest, redirect_uri, redirect_uri_sent, resource, \
             code_challenge, auth_code_hash, auth_code_expires_at, state, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, \
             ($13)::timestamptz, 'authorized', ($14)::timestamptz)",
        )
        .bind(connection.connection_id)
        .bind(tenant_id)
        .bind(connection.key_id)
        .bind(connection.code_id)
        .bind(&connection.client_id)
        .bind(connection.client_kind.as_str())
        .bind(connection.client_metadata_digest.0.to_vec())
        .bind(&connection.redirect_uri)
        .bind(connection.redirect_uri_sent)
        .bind(&connection.resource)
        .bind(&connection.code_challenge)
        .bind(connection.auth_code_hash.map(|h| h.0.to_vec()))
        .bind(
            connection
                .auth_code_expires_at
                .as_ref()
                .map(EventTime::as_str),
        )
        .bind(connection.created_at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(ConnectOutcome::Authorized {
            tenant_id,
            connection: Box::new(connection),
        })
    }

    async fn authorization_insert(&self, record: AuthorizationRecord) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        sqlx::query(
            "INSERT INTO oauth_authorizations (id, tenant_id, handle_hash, client_id, \
             client_kind, metadata_digest, redirect_uri, redirect_uri_sent, code_challenge, \
             state, expires_at, code_id, code_tenant_id, key_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, ($11)::timestamptz, $12, $13, $14)",
        )
        .bind(record.authorization_id)
        .bind(PROVIDER_TENANT)
        .bind(record.handle_hash.0.to_vec())
        .bind(&record.client_id)
        .bind(record.client_kind.as_str())
        .bind(record.metadata_digest.0.to_vec())
        .bind(&record.redirect_uri)
        .bind(record.redirect_uri_sent)
        .bind(&record.code_challenge)
        .bind(&record.state)
        .bind(record.expires_at.as_str())
        .bind(record.code_id)
        .bind(record.code_tenant_id)
        .bind(record.key_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn authorization(
        &self,
        handle_hash: &Sha256Hash,
    ) -> Result<Option<AuthorizationRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        let row = sqlx::query(&format!(
            "SELECT {AUTHORIZATION_COLUMNS} FROM oauth_authorizations \
             WHERE tenant_id = $1 AND handle_hash = $2"
        ))
        .bind(PROVIDER_TENANT)
        .bind(handle_hash.0.to_vec())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(authorization_from_row).transpose()
    }

    async fn oauth_client_insert(&self, record: OAuthClientRecord) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        sqlx::query(
            "INSERT INTO oauth_clients (client_id, tenant_id, kind, client_name, redirect_uris, \
             metadata_digest, fetched_at, max_age, last_used_at, connected_at, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, ($7)::timestamptz, $11, ($8)::timestamptz, \
             ($9)::timestamptz, ($10)::timestamptz)",
        )
        .bind(&record.client_id)
        .bind(PROVIDER_TENANT)
        .bind(record.kind.as_str())
        .bind(&record.client_name)
        .bind(&record.redirect_uris)
        .bind(record.metadata_digest.0.to_vec())
        .bind(record.fetched_at.as_ref().map(EventTime::as_str))
        .bind(record.last_used_at.as_ref().map(EventTime::as_str))
        .bind(record.connected_at.as_ref().map(EventTime::as_str))
        .bind(record.created_at.as_str())
        .bind(
            record
                .max_age
                .map(i32::try_from)
                .transpose()
                .map_err(backend)?,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn oauth_client(&self, client_id: &str) -> Result<Option<OAuthClientRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        let row = sqlx::query(&format!(
            "SELECT {CLIENT_COLUMNS} FROM oauth_clients WHERE tenant_id = $1 AND client_id = $2"
        ))
        .bind(PROVIDER_TENANT)
        .bind(client_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(client_from_row).transpose()
    }

    async fn upsert_cimd(
        &self,
        document: ClientDescription,
        fetched_at: &EventTime,
        max_age: u64,
    ) -> Result<DocumentCaching, StoreError> {
        let lifetime = i32::try_from(cache_lifetime(max_age)).map_err(backend)?;
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        lock_client_cap(&mut tx, CIMD_CAP_LOCK).await?;
        // A refetch refreshes the row in place, keeping connected_at and
        // created_at (postgres-schema.md `oauth_clients`).
        let refreshed = sqlx::query(
            "UPDATE oauth_clients SET client_name = $3, redirect_uris = $4, \
             metadata_digest = $5, fetched_at = ($6)::timestamptz, max_age = $7, \
             last_used_at = ($6)::timestamptz \
             WHERE tenant_id = $1 AND client_id = $2 AND kind = 'cimd'",
        )
        .bind(PROVIDER_TENANT)
        .bind(&document.client_id)
        .bind(&document.client_name)
        .bind(&document.redirect_uris)
        .bind(document.metadata_digest.0.to_vec())
        .bind(fetched_at.as_str())
        .bind(lifetime)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if refreshed.rows_affected() == 1 {
            tx.commit().await.map_err(backend)?;
            return Ok(DocumentCaching::Cached { evicted: None });
        }
        let cached: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM oauth_clients WHERE tenant_id = $1 AND kind = 'cimd'",
        )
        .bind(PROVIDER_TENANT)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        let mut evicted = None;
        if cached >= i64::from(CACHED_DOCUMENTS_MAX) {
            // The least recently used document whose client has not
            // connected; FOR UPDATE rechecks connected_at against a Connect
            // that stamped the row meanwhile, and finding none writes
            // nothing (ADR-0031 d5).
            let victim: Option<String> = sqlx::query_scalar(
                "SELECT client_id FROM oauth_clients \
                 WHERE tenant_id = $1 AND kind = 'cimd' AND connected_at IS NULL \
                 ORDER BY last_used_at NULLS FIRST, client_id COLLATE \"C\" \
                 LIMIT 1 FOR UPDATE",
            )
            .bind(PROVIDER_TENANT)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
            let Some(victim) = victim else {
                tx.rollback().await.map_err(backend)?;
                return Ok(DocumentCaching::NotCached);
            };
            sqlx::query("DELETE FROM oauth_clients WHERE tenant_id = $1 AND client_id = $2")
                .bind(PROVIDER_TENANT)
                .bind(&victim)
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
            evicted = Some(victim);
        }
        sqlx::query(
            "INSERT INTO oauth_clients (client_id, tenant_id, kind, client_name, redirect_uris, \
             metadata_digest, fetched_at, max_age, last_used_at, created_at) \
             VALUES ($1, $2, 'cimd', $3, $4, $5, ($6)::timestamptz, $7, ($6)::timestamptz, \
             ($6)::timestamptz)",
        )
        .bind(&document.client_id)
        .bind(PROVIDER_TENANT)
        .bind(&document.client_name)
        .bind(&document.redirect_uris)
        .bind(document.metadata_digest.0.to_vec())
        .bind(fetched_at.as_str())
        .bind(lifetime)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(DocumentCaching::Cached { evicted })
    }

    async fn touch_cimd(&self, client_id: &str, at: &EventTime) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        let touched = sqlx::query(
            "UPDATE oauth_clients SET last_used_at = ($3)::timestamptz \
             WHERE tenant_id = $1 AND client_id = $2 AND kind = 'cimd'",
        )
        .bind(PROVIDER_TENANT)
        .bind(client_id)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(touched.rows_affected() == 1)
    }

    async fn insert_dcr(
        &self,
        registration: ClientDescription,
        at: &EventTime,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        lock_client_cap(&mut tx, DCR_CAP_LOCK).await?;
        let unused: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM oauth_clients \
             WHERE tenant_id = $1 AND kind = 'dcr' AND connected_at IS NULL \
             AND created_at > ($2)::timestamptz - make_interval(secs => $3)",
        )
        .bind(PROVIDER_TENANT)
        .bind(at.as_str())
        .bind(f64::from(UNUSED_REGISTRATION_SECONDS))
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if unused >= i64::from(UNUSED_REGISTRATIONS_MAX) {
            tx.rollback().await.map_err(backend)?;
            return Err(StoreError::CapReached);
        }
        sqlx::query(
            "INSERT INTO oauth_clients (client_id, tenant_id, kind, client_name, redirect_uris, \
             metadata_digest, created_at) \
             VALUES ($1, $2, 'dcr', $3, $4, $5, ($6)::timestamptz)",
        )
        .bind(&registration.client_id)
        .bind(PROVIDER_TENANT)
        .bind(&registration.client_name)
        .bind(&registration.redirect_uris)
        .bind(registration.metadata_digest.0.to_vec())
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn remove_unused_dcr(&self, registered_by: &EventTime) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, PROVIDER_TENANT).await?;
        let removed = sqlx::query(
            "DELETE FROM oauth_clients \
             WHERE tenant_id = $1 AND kind = 'dcr' AND connected_at IS NULL \
             AND created_at <= ($2)::timestamptz",
        )
        .bind(PROVIDER_TENANT)
        .bind(registered_by.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(removed.rows_affected())
    }

    async fn connect_codes(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Vec<ConnectCodeRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {CODE_COLUMNS} FROM connect_codes \
             WHERE tenant_id = $1 AND key_id = $2 ORDER BY issued_seq"
        ))
        .bind(tenant_id)
        .bind(key_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(code_from_row).collect()
    }

    async fn connections(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Vec<ConnectionRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {CONNECTION_COLUMNS} FROM oauth_connections \
             WHERE tenant_id = $1 AND key_id = $2 ORDER BY created_at, id"
        ))
        .bind(tenant_id)
        .bind(key_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(connection_from_row).collect()
    }

    async fn connection(
        &self,
        tenant_id: Uuid,
        connection_id: Uuid,
    ) -> Result<Option<ConnectionRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {CONNECTION_COLUMNS} FROM oauth_connections \
             WHERE tenant_id = $1 AND id = $2"
        ))
        .bind(tenant_id)
        .bind(connection_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(connection_from_row).transpose()
    }
}
