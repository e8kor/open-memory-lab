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
use oml_app::domain::claim_kinds::TestShape;
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
    ChainHead, Envelope, EventTime, KeySet, MemoryKind, MemoryStatus, Role, Sha256Hash,
    SignedEnvelope, UsedBy, VerifyError, WorkspaceStatus, ratifier_is_proposer, verify_chain,
};
use oml_app::ed25519_dalek::Signature;
use oml_app::ports::{
    Ceiling, CommitReceipt, CommitRequest, ConnectionStore, IdempotencyClaim, IdempotencyPut,
    IdempotencyRecord, IdempotencyScope, JournalStore, MetadataFold, Retriever, ScanBacklog,
    SessionKeyName, StoreError, SubjectRow, TailEvent, TailScan, WebStore, Written, claim_subject,
};
use oml_app::search::{
    Access, Candidate, CandidateScope, MatchMode, MatchedToken, OwnHeldScope, PostingsHealth,
    RetrievalQuery, Retrieved, RetrieverKind, SparsePending, SparsePosting, SparseTenant,
    SparseUnit,
};
use oml_app::types::{
    AdminSearch, AdminState, Approval, AuthorizationRecord, BriefEdgeRecord, ClaimEdgePosition,
    ClaimEdgeRecord, ClaimEdgeSelection, ClaimKeyState, ClientDescription, ClientKind, ConnectCode,
    ConnectCodeRecord, ConnectGrant, ConnectOutcome, ConnectionRecord, Credential, CrewRecord,
    DenialRecord, DocumentCaching, FormatFields, GrantRecord, GuardrailFields, HIT_KINDS,
    HeldFilter, HeldRow, INSTRUCTION_HIT_KINDS, KeyAccess, KeyRecord, KeyReferences, KeySuspension,
    KindFields, ListingFilter, ListingPosition, ListingScope, MemoryRecord, NamedRef,
    OAuthClientRecord, PROVIDER_TENANT, PersonReviewer, PinOutcome, QueueDepth, RefreshFacts,
    RefreshOutcome, ReviewLane, SourceCheck, SuspensionCause, SweepReport, TenantRow, TokenPair,
    TokenRecord, TransitionRecord, TransitionState, UnnamedRows, VersionFinding, WebCredentialKey,
    WebCredentialRows, WebEntry, WebSessionRecord, WorkspaceRecord, WorkspaceRef,
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
///
/// It runs before migration 0047 renames the schema, so it names the table
/// and the status as that database holds them (ADR-0091 d6, d11).
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
        derived_index_names_ddl(&partition),
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

/// The statement that renames the indexes `CREATE TABLE … (LIKE … INCLUDING
/// ALL)` derived for `partition` from its parent's index columns. An index
/// keeps the column names it was built with, so a partition created after
/// migration 0047 would derive names that hold the former word; this gives
/// them the current one, as 0047 renamed every partition it found, cutting
/// the byte before a numbered suffix where the longer word passes the 63-byte
/// limit, and keeping two names that meet apart by a number, as PostgreSQL
/// keeps the names it derives apart (ADR-0091 d6).
pub(crate) fn derived_index_names_ddl(partition: &str) -> String {
    format!(
        "DO $rename$ DECLARE r record; renamed text; suffix text; base text; n int; BEGIN \
         FOR r IN SELECT c.relname FROM pg_index i JOIN pg_class c ON c.oid = i.indexrelid \
                   WHERE i.indrelid = '{partition}'::regclass \
                     AND strpos(c.relname, 'workarea') > 0 LOOP \
           renamed := replace(r.relname, 'workarea', 'workspace'); \
           suffix := coalesce(substring(renamed from '(_[a-z]+[0-9]*)$'), ''); \
           IF octet_length(renamed) > 63 THEN \
             renamed := left(renamed, 63 - octet_length(suffix)) || suffix; \
           END IF; \
           base := renamed; n := 0; \
           WHILE to_regclass(quote_ident(renamed)) IS NOT NULL LOOP \
             n := n + 1; \
             renamed := left(base, 63 - octet_length(n::text)) || n::text; \
           END LOOP; \
           EXECUTE format('ALTER INDEX %I RENAME TO %I', r.relname, renamed); \
         END LOOP; END $rename$"
    )
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
        derived_index_names_ddl(&postings),
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
        derived_index_names_ddl(&similar),
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
#[must_use]
pub fn recall_fts_sql() -> String {
    keyword_all_sql(
        "m.tenant_id = $1 AND m.status = 'promoted' AND m.is_tip \
         AND NOT m.retirement AND m.kind = ANY($4::text[]) \
         AND m.workspace_id = ANY($2::uuid[]) /*anchor*/",
        "$3",
        "$5",
    )
}

/// `keyword-all`'s statement over the rows `scope` admits (a predicate on
/// `m`), the query bound at `query` and `k` at `k`: [`WHOLE_UNITS`] read as
/// v0.5.0 read every row, and [`PASSAGE_UNITS`] beside them, one candidate
/// per version ([`one_per_version`]).
fn keyword_all_sql(scope: &str, query: &str, k: &str) -> String {
    let branch = |u: &Units| {
        format!(
            "SELECT m.id, {ordinal} AS ordinal, {bytes} AS body_bytes, \
                    ts_rank(to_tsvector('simple', {text}), plainto_tsquery('simple', {query}), 0) AS score, \
                    '{{}}'::text[] AS matched \
             FROM {from} \
             WHERE {scope} AND {only} \
               AND to_tsvector('simple', {text}) @@ plainto_tsquery('simple', {query})",
            ordinal = u.ordinal,
            bytes = u.bytes,
            text = u.text,
            from = u.from,
            only = u.only,
        )
    };
    format!(
        "{} ORDER BY score DESC, id ASC, ordinal ASC LIMIT {k}",
        one_per_version(&whole_units(&branch), &branch(&PASSAGE_UNITS))
    )
}

/// The own held recall statement `recall_own_held` runs, binding `$1` the
/// tenant, `$2` the author keys (the own key set, and the mark of the
/// server's rule set on each node inside an in-progress subtree,
/// `held_authors`), `$3` the readable nodes, `$4`
/// the query, `$5` the kinds and `$6` `k`; pruned to the tenant's partition
/// as [`recall_fts_sql`] is (ADR-0034 d11).
#[must_use]
pub fn recall_own_held_sql() -> String {
    keyword_all_sql(
        "m.tenant_id = $1 AND m.status = 'held' \
         AND (m.author_key_id = ANY($2::uuid[]) \
           OR (m.scan_finding IS NULL AND encode(substring(sha256(convert_to(COALESCE(m.scanned_under, '') || '/' || m.workspace_id::text, 'UTF8')) FROM 1 FOR 16), 'hex')::uuid = ANY($2::uuid[]))) \
         AND NOT m.retirement AND m.kind = ANY($5::text[]) \
         AND m.workspace_id = ANY($3::uuid[]) /*anchor*/",
        "$4",
        "$6",
    )
}

/// The offered instructions' part of the v0.5.0 recall statement
/// (ADR-0092 d3): `$2` the candidates by id, `$3` the query, `$4` `k`,
/// the anchor's binds from `$5`; the rows of the instructions the core
/// admitted, in any state, every query word held.
#[must_use]
pub fn recall_instructions_sql() -> String {
    keyword_all_sql(
        "m.tenant_id = $1 AND m.id = ANY($2::uuid[]) AND NOT m.retirement /*anchor*/",
        "$3",
        "$4",
    )
}

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
       AND ((m.kind = ANY($3::text[]) \
       AND ((m.status = 'promoted' AND m.is_tip AND m.workspace_id = ANY($2::uuid[])) \
         OR (m.status = 'held' AND m.workspace_id = ANY($5::uuid[]) \
             AND (m.author_key_id = ANY($4::uuid[]) \
               OR (m.scan_finding IS NULL \
                   AND encode(substring(sha256(convert_to(COALESCE(m.scanned_under, '') || '/' || m.workspace_id::text, 'UTF8')) FROM 1 FOR 16), 'hex')::uuid = ANY($4::uuid[])))))) \
         OR m.id = ANY($4::uuid[]))";

/// The author keys a held search statement binds: the own key set, and,
/// for each node inside an in-progress subtree (`any_author_nodes`), the
/// mark of the server's rule set on that node ([`scanner_mark`]), which
/// admits every author's held row there the scan did not hold and last
/// read under that rule set (ADR-0030 d3; ADR-0065 d15; ADR-0080 d16). The
/// statements compare a row's own mark, computed from its `scanned_under`
/// and its node, only beside `m.scan_finding IS NULL`, so a row the scan
/// held, one an older rule set passed, or one on a node outside every
/// subtree, stays its author's: one node array serves both sets, and no
/// statement binds a second.
fn held_authors(own: &OwnHeldScope) -> Vec<Uuid> {
    let mut keys = own.author_key_ids.clone();
    keys.extend(
        own.any_author_nodes
            .iter()
            .map(|node| scanner_mark(own.current_scanner.as_deref(), *node)),
    );
    keys
}

/// A rule-set version on a node as an id the held statements' author array
/// can carry: the first 16 bytes of SHA-256 over the version, the empty
/// string for none, then `/` and the node's id in its hyphenated lower-case
/// text, as the statements compute it from `m.scanned_under` and
/// `m.workspace_id::text`. A key id equal to one is a 2^-122 chance at most
/// (ids are minted v4 or v7).
fn scanner_mark(version: Option<&str>, node: Uuid) -> Uuid {
    let marked = format!("{}/{}", version.unwrap_or(""), node.hyphenated());
    let digest = Sha256Hash::digest(marked.as_bytes()).0;
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
const TIPS_TODAY: &str =
    "(m.status = 'promoted' AND m.is_tip AND m.workspace_id = ANY($2::uuid[]))";

/// The held part of [`SEARCH_SCOPE`], up to its author predicate.
const HELD_TODAY: &str = "OR (m.status = 'held' AND m.workspace_id = ANY($5::uuid[])";

/// Whether `m` was its claim's tip at the instant `at`, an SQL
/// `timestamptz` expression (ADR-0059 d15, d16, d20, d22): promoted at or
/// before it, and the greatest version of its `workspace_id` and
/// `claim_key`, in any state today but `demoted`, promoted at or before it.
/// Today's tip promoted by then was the tip then, since it is the greatest
/// non-demoted version promoted by any instant since its own promotion
/// (d22), so the probe runs only for a replaced version: correlated, once
/// per such row, it reads `memories_tip_moves` by an index condition on the
/// claim, backwards, reading each entry's state from the heap and stopping
/// at the first that is not demoted, never a join the planner could
/// unparameterize. Exported so the suite plans the probe the adapter runs.
#[must_use]
pub fn tip_at_sql(at: &str) -> String {
    // `<>` on text is `textne`, leakproof, so row security still lets the
    // index serve the probe (ADR-0059 d22). The in-memory store's `tip_at`
    // makes the same reading.
    format!(
        "m.status = 'promoted' AND m.promoted_at <= {at} \
         AND (m.is_tip OR m.version = (SELECT max(s.version) FROM memories s \
           WHERE s.tenant_id = m.tenant_id AND s.workspace_id = m.workspace_id \
             AND s.claim_key = m.claim_key AND s.promoted_at <= {at} \
             AND s.status <> 'demoted'))"
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
            "({} AND m.workspace_id = ANY($2::uuid[]))",
            tip_at_sql(&instant_literal(at))
        ),
    )
    .replace(
        HELD_TODAY,
        &format!(
            "OR (m.status = 'held' AND m.created_at <= {} AND m.workspace_id = ANY($5::uuid[])",
            instant_literal(at)
        ),
    )
}

/// One shape of a scanned version's search units (ADR-0081 d4; migration
/// 0059): every statement that scans `memories` reads its candidates in two
/// branches joined by `UNION ALL`, [`WHOLE_UNITS`] and [`PASSAGE_UNITS`], so
/// a row of any kind but `document` is read by the statement as it was
/// before passages, its plan included, and only document rows are joined to
/// their passages (the review's finding 3).
struct Units {
    /// The relation the branch reads, `m` the version.
    from: &'static str,
    /// The kinds the branch reads.
    only: &'static str,
    /// A unit's ordinal (-1 the whole version), served bytes, lexemes,
    /// length and text.
    ordinal: &'static str,
    bytes: &'static str,
    vector: &'static str,
    len: &'static str,
    text: &'static str,
}

/// Every row of a kind but `document`: one unit, its own columns, read by
/// the statement as before passages, its parallel plan included.
const WHOLE_UNITS: Units = Units {
    from: "memories m",
    only: "m.kind <> 'document'",
    ordinal: "-1",
    bytes: "m.body_bytes",
    vector: "m.search_vector",
    len: "m.search_len",
    text: "COALESCE(m.search_text, m.claim)",
};

/// The whole-version branch as one relation of `branch`'s columns: every
/// kind but `document`. A document version with no passage cut, written
/// before 0059 and not cut yet or a text that holds no content line (R1),
/// is no search unit, here as in the postings (migration 0059's
/// `oml_search_units`), so neither list reads it.
fn whole_units(branch: &impl Fn(&Units) -> String) -> String {
    branch(&WHOLE_UNITS)
}

/// A document version's passages, each its own unit, read from the
/// passages and joined to their version by its key, so a scope with no
/// document reads nothing here.
const PASSAGE_UNITS: Units = Units {
    from: "document_passages d JOIN memories m ON m.tenant_id = $1 AND m.id = d.memory_id",
    only: "d.tenant_id = $1 AND m.kind = 'document'",
    ordinal: "d.ordinal",
    bytes: "d.body_bytes",
    vector: "d.search_vector",
    len: "d.search_len",
    text: "d.search_text",
};

/// One candidate per version (ADR-0081 d4; the review's reading R3
/// amended), from the two branches `whole` and `passages`, each selecting
/// `id`, `ordinal`, `body_bytes`, `score` and `matched`: a whole version is
/// its own candidate, and of a document's passages the best by score, then
/// ordinal, carrying the count of the others as `more_passages`. The caller
/// orders and cuts the union, so its `k` counts versions.
fn one_per_version(whole: &str, passages: &str) -> String {
    format!(
        "SELECT id, ordinal, body_bytes, score, matched, 0::int8 AS more_passages \
           FROM ({whole}) w \
         UNION ALL \
         SELECT id, ordinal, body_bytes, score, matched, more_passages \
           FROM (SELECT p.*, row_number() OVER (PARTITION BY p.id ORDER BY p.score DESC, p.ordinal ASC) AS unit_rank, \
                        count(*) OVER (PARTITION BY p.id) - 1 AS more_passages \
                   FROM ({passages}) p) b \
          WHERE b.unit_rank = 1"
    )
}

/// The versions of the tenant `$1` whose valid time ended at or before the
/// request's now, `$NOW`, read by the `memories_valid_until` index's
/// condition; the postings statements leave them out (proposed ADR-0059 d3).
const EXPIRED_IDS: &str = "expired AS NOT MATERIALIZED (SELECT m.id FROM memories m \
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
            "SELECT m.id, m.body_bytes, m.kind, \
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
    let branch = |u: &Units| {
        format!(
            "SELECT m.id, {ordinal} AS ordinal, {bytes} AS body_bytes, \
                    ts_rank(to_tsvector('simple', {text}), q.tq, 0) AS score, \
                    NULL::text[] AS matched \
             FROM {from} CROSS JOIN q \
             WHERE {SEARCH_SCOPE}{anchor} AND {only} \
               AND to_tsvector('simple', {text}) @@ q.tq \
               AND (NOT $8 OR to_tsvector('simple', {text}) @@ plainto_tsquery('simple', $6))",
            ordinal = u.ordinal,
            bytes = u.bytes,
            text = u.text,
            from = u.from,
            only = u.only,
        )
    };
    // `q` is read in each branch and after the cut, and inlined in each,
    // since a materialized CTE would keep the scan from a parallel plan.
    // The terms each kept unit holds are read after the cut, by its key,
    // so no row outside the k is tokenized twice: the sort over the two
    // branches' union cannot postpone them as one statement's sort does.
    with_now(
        &format!(
            "WITH q AS NOT MATERIALIZED (SELECT NULLIF(replace(plainto_tsquery('simple', $6)::text, ' & ', ' | '), '')::tsquery AS tq, \
                    tsvector_to_array(to_tsvector('simple', $6)) AS terms), \
             top AS ({} ORDER BY score DESC, id ASC, ordinal ASC LIMIT $7) \
             SELECT top.id, top.ordinal, top.body_bytes, top.score, top.more_passages, \
                    ARRAY(SELECT t FROM unnest(q.terms) t \
                          WHERE t = ANY(tsvector_to_array(to_tsvector('simple', \
                                COALESCE(d.search_text, m.search_text, m.claim)))) ORDER BY t) AS matched \
             FROM top CROSS JOIN q \
             JOIN memories m ON m.tenant_id = $1 AND m.id = top.id \
             LEFT JOIN document_passages d \
               ON d.tenant_id = $1 AND d.memory_id = top.id AND d.ordinal = top.ordinal \
             ORDER BY top.score DESC, top.id ASC, top.ordinal ASC",
            one_per_version(&whole_units(&branch), &branch(&PASSAGE_UNITS))
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

/// The BM25 statements' unit statistics over the two unit branches
/// ([`Units`]), `scope` the predicate on `m` a branch reads: `n` counts
/// versions, since each version has exactly one unit of ordinal -1 or 0,
/// and `avgdl` is the units' mean length (the review's reading R6 amended).
fn bm25_stats(scope: &str) -> String {
    let branch = |u: &Units| {
        format!(
            "SELECT {ordinal} AS ordinal, {len} AS len FROM {from} WHERE {scope} AND {only}",
            ordinal = u.ordinal,
            len = u.len,
            from = u.from,
            only = u.only,
        )
    };
    format!(
        "stats AS (SELECT count(*) FILTER (WHERE x.ordinal < 1)::float8 AS n, \
                          GREATEST(COALESCE(avg(x.len), 1), 1)::float8 AS avgdl \
                   FROM ({} UNION ALL {}) x)",
        whole_units(&branch),
        branch(&PASSAGE_UNITS)
    )
}

/// Each scanned unit's query forms with their frequencies, over `scope`:
/// the whole versions' and the passages', as two relations.
fn bm25_unit_forms(scope: &str) -> (String, String) {
    let branch = |u: &Units| {
        format!(
            "SELECT m.id, {ordinal} AS ordinal, {bytes} AS body_bytes, \
                    {len}::float8 AS dl, u.lexeme, \
                    GREATEST(cardinality(u.positions), 1)::float8 AS tf \
             FROM {from} CROSS JOIN q \
             CROSS JOIN LATERAL unnest(ts_filter(setweight({vector}, 'A', q.fa), '{{a}}')) u \
             WHERE {scope} AND {only} AND {vector} @@ q.tq",
            ordinal = u.ordinal,
            bytes = u.bytes,
            len = u.len,
            vector = u.vector,
            from = u.from,
            only = u.only,
        )
    };
    (whole_units(&branch), branch(&PASSAGE_UNITS))
}

/// Each form's document frequency from `whole` and `passages`, relations
/// of the [`bm25_unit_forms`] columns: a whole version counts once, and a
/// document once however many of its passages hold the form (the review's
/// reading R6 amended), so the whole versions are counted in one pass as
/// before passages.
fn bm25_df(whole: &str, passages: &str) -> String {
    format!(
        "df AS (SELECT lexeme, sum(n)::float8 AS df FROM ( \
                  SELECT lexeme, count(*) AS n FROM {whole} GROUP BY lexeme \
                  UNION ALL \
                  SELECT lexeme, count(DISTINCT id) FROM {passages} GROUP BY lexeme) f \
                GROUP BY lexeme)"
    )
}

/// The end of a BM25 statement: `per`'s contributions summed per unit, the
/// units holding every kept word under `$8`, one per version, cut at `$7`.
fn bm25_tail() -> String {
    format!(
        "scored AS (SELECT id, ordinal, body_bytes, sum(c)::float4 AS score, \
                           array_agg(w ORDER BY w COLLATE \"C\") AS words \
                    FROM per GROUP BY id, ordinal, body_bytes), \
         matching AS (SELECT id, ordinal, body_bytes, score, words AS matched FROM scored \
                      WHERE (NOT $8 OR cardinality(words) = (SELECT count(*) FROM kept))) \
         {} \
         ORDER BY score DESC, id ASC, ordinal ASC \
         LIMIT $7",
        one_per_version(
            "SELECT * FROM matching WHERE ordinal < 0",
            "SELECT * FROM matching WHERE ordinal >= 0"
        )
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
         {stats}, \
         tf_w AS MATERIALIZED ({whole}), \
         tf_d AS MATERIALIZED ({passages}), \
         {df}, \
         per AS (SELECT tf.id, tf.ordinal, tf.body_bytes, forms.w, \
                        max(ln(1 + (s.n - df.df + 0.5) / (df.df + 0.5)) \
                            * tf.tf * 2.2 / (tf.tf + 1.2 * (0.25 + 0.75 * tf.dl / s.avgdl))) AS c \
                 FROM (SELECT * FROM tf_w UNION ALL SELECT * FROM tf_d) tf JOIN df USING (lexeme) JOIN forms ON forms.f = tf.lexeme CROSS JOIN stats s \
                 {within} \
                 GROUP BY tf.id, tf.ordinal, tf.body_bytes, forms.w), \
         {tail}",
            stats = bm25_stats(SEARCH_SCOPE),
            whole = bm25_unit_forms(SEARCH_SCOPE).0,
            passages = bm25_unit_forms(SEARCH_SCOPE).1,
            df = bm25_df("tf_w", "tf_d"),
            tail = bm25_tail(),
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
       AND m.workspace_id = ANY($10::uuid[])";

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
         {stats}, \
         reference_w AS ({reference_w}), \
         reference_d AS ({reference_d}), \
         {df}, \
         tf_w AS MATERIALIZED ({whole}), \
         tf_d AS MATERIALIZED ({passages}), \
         per AS (SELECT tf.id, tf.ordinal, tf.body_bytes, forms.w, \
                        max(ln(1 + (s.n - df.df + 0.5) / (df.df + 0.5)) \
                            * tf.tf * 2.2 / (tf.tf + 1.2 * (0.25 + 0.75 * tf.dl / s.avgdl))) AS c \
                 FROM (SELECT * FROM tf_w UNION ALL SELECT * FROM tf_d) tf JOIN df USING (lexeme) JOIN forms ON forms.f = tf.lexeme CROSS JOIN stats s \
                 {within} \
                 GROUP BY tf.id, tf.ordinal, tf.body_bytes, forms.w), \
         {tail}",
            stats = bm25_stats(REFERENCE_SCOPE),
            reference_w = bm25_unit_forms(REFERENCE_SCOPE).0,
            reference_d = bm25_unit_forms(REFERENCE_SCOPE).1,
            df = bm25_df("reference_w", "reference_d"),
            whole = bm25_unit_forms(SEARCH_SCOPE).0,
            passages = bm25_unit_forms(SEARCH_SCOPE).1,
            tail = bm25_tail(),
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
/// frequency among them. Both sum the per-workspace counts, so they are the
/// readable rows', never the tenant's. The first row also counts the served
/// documents among them, so a scope with none runs the statement that reads
/// no passage (ADR-0081 d4).
const POSTINGS_COUNTS: &str = "SELECT NULL::int8 AS token_id, COALESCE(sum(c.rows), 0)::int8 AS n, \
            COALESCE(sum(c.total_len), 0)::int8 AS len, COALESCE(sum(c.units), 0)::int8 AS units, \
            COALESCE(sum(c.rows) FILTER (WHERE c.memory_kind = 3), 0)::int8 AS documents \
     FROM search_workspace_rows c \
     WHERE c.tenant_id = $1 AND c.workspace_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) \
     UNION ALL \
     SELECT c.token_id, sum(c.rows)::int8, 0::int8, 0::int8, 0::int8 \
     FROM search_term_rows c \
     WHERE c.tenant_id = $1 AND c.workspace_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) AND c.token_id = ANY($4::int8[]) \
     GROUP BY c.token_id";

/// The reference set's counts per node (ADR-0072 d4.2, d12): for each node
/// of `$2`, a row with no token holding its served versions of the memory
/// kinds `$3`; then each token of `$4` with its document frequency there.
/// Read by the counts' primary keys, `workspace_id = ANY` and `token_id =
/// ANY` being leakproof equalities.
const REFERENCE_COUNTS: &str = "SELECT c.workspace_id, NULL::int8 AS token_id, \
            COALESCE(sum(c.rows), 0)::int8 AS n \
     FROM search_workspace_rows c \
     WHERE c.tenant_id = $1 AND c.workspace_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) \
     GROUP BY c.workspace_id \
     UNION ALL \
     SELECT c.workspace_id, c.token_id, sum(c.rows)::int8 \
     FROM search_term_rows c \
     WHERE c.tenant_id = $1 AND c.workspace_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) AND c.token_id = ANY($4::int8[]) \
     GROUP BY c.workspace_id, c.token_id";

/// The served versions on the reference nodes `$2` of the kinds `$3` whose
/// valid time ended at or before the request's now `$4`, with their node:
/// the rows the counts hold that the reference set does not (ADR-0072
/// d4.2). Read by the `memories_valid_until` index's condition.
const REFERENCE_EXPIRED: &str = "SELECT m.id, m.workspace_id FROM memories m \
     WHERE m.tenant_id = $1 AND m.valid_until_us <= $4::int8 AND m.status = 'promoted' \
       AND m.is_tip AND NOT m.retirement AND m.workspace_id = ANY($2::uuid[]) \
       AND m.kind = ANY($3::text[])";

/// Each node of `$2`'s served versions of the memory kinds `$3`: the widen
/// signal's rows (ADR-0072 d8, as built by #244).
const VOCABULARY_ROWS: &str = "SELECT c.workspace_id, COALESCE(sum(c.rows), 0)::int8 \
     FROM search_workspace_rows c \
     WHERE c.tenant_id = $1 AND c.workspace_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) \
     GROUP BY c.workspace_id";

/// Every token's count per node of `$2`, [`REFERENCE_COUNTS`]' second half
/// without its token filter, cut at `$4` rows so a vocabulary past the
/// signal's cap is never read whole: the widen signal's read, made off the
/// request path (ADR-0072 d8, as built by #244).
const VOCABULARY_TERMS: &str = "SELECT c.workspace_id, c.token_id, sum(c.rows)::int8 \
     FROM search_term_rows c \
     WHERE c.tenant_id = $1 AND c.workspace_id = ANY($2::uuid[]) AND c.kind = 1 \
       AND c.memory_kind = ANY($3::int2[]) \
     GROUP BY c.workspace_id, c.token_id \
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
/// `memories_author` index (ADR-0052 d5, open question 7); and whether
/// each is a document, whose passages are its units (ADR-0081 d4).
const POSTINGS_OWN_HELD: &str = "SELECT m.id, COALESCE(d.search_len, m.search_len), m.kind = 'document', m.kind \
     FROM memories m \
     LEFT JOIN document_passages d \
       ON m.kind = 'document' AND d.tenant_id = $1 AND d.memory_id = m.id \
     WHERE m.tenant_id = $1 AND m.status = 'held' \
        AND (m.kind <> 'document' OR d.memory_id IS NOT NULL) \
        AND (m.author_key_id = ANY($2::uuid[]) \
          OR (m.scan_finding IS NULL AND encode(substring(sha256(convert_to(COALESCE(m.scanned_under, '') || '/' || m.workspace_id::text, 'UTF8')) FROM 1 FOR 16), 'hex')::uuid = ANY($2::uuid[]))) \
        AND m.workspace_id = ANY($3::uuid[]) AND m.kind = ANY($4::text[]) AND NOT m.retirement \
        AND (m.valid_until_us IS NULL OR m.valid_until_us > $5::int8)";

/// The served versions on the readable nodes `$2` of the kinds `$3` whose
/// valid time ended at or before the request's now `$4`, with their
/// lengths: the rows the per-workspace counts hold that the scope does not,
/// taken out of the statistics so an expired version counts in none, as in
/// `bm25`'s scan (proposed ADR-0059 d3; review M5). Read by the
/// `memories_valid_until` index's condition.
const POSTINGS_EXPIRED: &str = "SELECT m.id, m.search_len FROM memories m \
     WHERE m.tenant_id = $1 AND m.valid_until_us <= $4::int8 AND m.status = 'promoted' \
       AND m.is_tip AND NOT m.retirement AND m.workspace_id = ANY($2::uuid[]) \
       AND m.kind = ANY($3::text[])";

/// The offered instructions `$2` (ADR-0092 d3, d4) with their lengths, in
/// any state the core admitted them in: each counts in BM25's statistics
/// as an own held row does, read by the primary key.
const POSTINGS_INSTRUCTIONS: &str = "SELECT m.id, m.search_len FROM memories m \
     WHERE m.tenant_id = $1 AND m.id = ANY($2::uuid[]) AND NOT m.retirement";

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
/// the serving role under row security. It reads the whole versions
/// (ordinal -1) alone, one row per version, and is the statement a scope
/// that reads no document runs: [`bm25_postings_units_sql`] is the one
/// that reads passages (ADR-0081 d4). Exported so the suite plans the
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
                 AND p.workspace_id = ANY($2::uuid[]) AND p.state = 1 \
                 AND p.memory_kind = ANY($3::int2[]) AND p.ordinal < 0{anchor} \
               UNION ALL \
               SELECT p.memory_id, p.token_id, p.weight, p.doc_len FROM search_postings p \
               WHERE p.tenant_id = $1 AND p.memory_id = ANY($4::uuid[]) AND p.kind = 1 \
                 AND p.token_id = ANY($10::int8[]) AND p.ordinal < 0{anchor}), \
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
         SELECT r.memory_id AS id, m.body_bytes, m.kind, r.score::float4 AS score, r.words AS matched \
         FROM ranked r JOIN memories m ON m.tenant_id = $1 AND m.id = r.memory_id \
         WHERE NOT $12 OR cardinality(r.words) = $13 \
         ORDER BY score DESC, id ASC \
         LIMIT $14"
        ),
        15,
    )
}

/// [`bm25_postings_sql`] over a scope that reads a document (ADR-0081 d4):
/// the whole versions are generated, scored and cut as that statement
/// does; a document's passages are generated apart and only they are
/// collapsed to one unit per version, its best by score then ordinal,
/// counting its other passages; the two cut lists are merged and cut
/// again at `$11`, and each kept unit is rescored by its own postings.
/// The binds are [`bm25_postings_sql`]'s. Exported so the suite plans the
/// statement the adapter runs.
#[must_use]
pub fn bm25_postings_units_sql() -> String {
    bm25_postings_units_sql_within("")
}

/// [`bm25_postings_units_sql`] with `anchor`, as
/// [`bm25_postings_sql_within`] takes it.
fn bm25_postings_units_sql_within(anchor: &str) -> String {
    with_now(
        &format!(
            "WITH q AS (SELECT q.w, q.token_id, ln(1 + ($8 - q.df + 0.5) / (q.df + 0.5)) AS idf \
                    FROM unnest($5::text[], $6::int8[], $7::float8[]) AS q(w, token_id, df)), \
         p AS (SELECT p.memory_id, p.token_id, p.weight, p.doc_len FROM search_postings p \
               WHERE p.tenant_id = $1 AND p.kind = 1 AND p.token_id = ANY($10::int8[]) \
                 AND p.workspace_id = ANY($2::uuid[]) AND p.state = 1 \
                 AND p.memory_kind = ANY($3::int2[]) AND p.ordinal < 0{anchor} \
               UNION ALL \
               SELECT p.memory_id, p.token_id, p.weight, p.doc_len FROM search_postings p \
               WHERE p.tenant_id = $1 AND p.memory_id = ANY($4::uuid[]) AND p.kind = 1 \
                 AND p.token_id = ANY($10::int8[]) AND p.ordinal < 0{anchor}), \
         gen AS (SELECT p.memory_id, q.w, max({POSTING_CONTRIBUTION}) AS c \
                 FROM p JOIN q ON q.token_id = p.token_id GROUP BY p.memory_id, q.w), \
         {EXPIRED_IDS}, \
         top_w AS (SELECT memory_id, sum(c)::float4 AS s FROM gen \
                   WHERE memory_id NOT IN (SELECT id FROM expired) GROUP BY memory_id \
                   ORDER BY sum(c)::float4 DESC, memory_id ASC LIMIT $11), \
         pd AS (SELECT p.memory_id, p.ordinal, p.token_id, p.weight, p.doc_len FROM search_postings p \
                WHERE p.tenant_id = $1 AND p.kind = 1 AND p.token_id = ANY($10::int8[]) \
                  AND p.workspace_id = ANY($2::uuid[]) AND p.state = 1 \
                  AND p.memory_kind = ANY($3::int2[]) AND p.ordinal >= 0{anchor} \
                UNION ALL \
                SELECT p.memory_id, p.ordinal, p.token_id, p.weight, p.doc_len FROM search_postings p \
                WHERE p.tenant_id = $1 AND p.memory_id = ANY($4::uuid[]) AND p.kind = 1 \
                  AND p.token_id = ANY($10::int8[]) AND p.ordinal >= 0{anchor}), \
         gen_d AS (SELECT p.memory_id, p.ordinal, q.w, max({POSTING_CONTRIBUTION}) AS c \
                   FROM pd p JOIN q ON q.token_id = p.token_id GROUP BY p.memory_id, p.ordinal, q.w), \
         units_d AS (SELECT memory_id, ordinal, sum(c)::float4 AS s FROM gen_d \
                     WHERE memory_id NOT IN (SELECT id FROM expired) GROUP BY memory_id, ordinal), \
         top AS (SELECT memory_id, ordinal, more_passages FROM ( \
                   SELECT memory_id, (-1)::int4 AS ordinal, s, 0::int8 AS more_passages FROM top_w \
                   UNION ALL \
                   SELECT memory_id, ordinal, s, more_passages FROM ( \
                     SELECT u.*, row_number() OVER (PARTITION BY u.memory_id ORDER BY u.s DESC, u.ordinal ASC) AS unit_rank, \
                            count(*) OVER (PARTITION BY u.memory_id) - 1 AS more_passages \
                     FROM units_d u) b WHERE b.unit_rank = 1) t \
                 ORDER BY s DESC, memory_id ASC, ordinal ASC LIMIT $11), \
         scored AS (SELECT p.memory_id, p.ordinal, q.w, max({POSTING_CONTRIBUTION}) AS c \
                    FROM top JOIN search_postings p \
                      ON p.memory_id = top.memory_id AND p.ordinal = top.ordinal \
                    JOIN q ON q.token_id = p.token_id \
                    WHERE p.tenant_id = $1 AND p.kind = 1 AND p.token_id = ANY($6::int8[]) \
                    GROUP BY p.memory_id, p.ordinal, q.w), \
         ranked AS (SELECT memory_id, ordinal, sum(c) AS score, array_agg(w ORDER BY w COLLATE \"C\") AS words \
                    FROM scored GROUP BY memory_id, ordinal) \
         SELECT r.memory_id AS id, r.ordinal, COALESCE(d.body_bytes, m.body_bytes) AS body_bytes, \
                r.score::float4 AS score, r.words AS matched, top.more_passages \
         FROM ranked r JOIN top ON top.memory_id = r.memory_id AND top.ordinal = r.ordinal \
         JOIN memories m ON m.tenant_id = $1 AND m.id = r.memory_id \
         LEFT JOIN document_passages d \
           ON d.tenant_id = $1 AND d.memory_id = r.memory_id AND d.ordinal = r.ordinal \
         WHERE NOT $12 OR cardinality(r.words) = $13 \
         ORDER BY score DESC, id ASC, ordinal ASC \
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
     p AS (SELECT p.memory_id, p.ordinal, p.token_id, p.weight, p.doc_len FROM search_postings p \
           WHERE p.tenant_id = $1 AND p.kind = 2 AND p.token_id = ANY($5::int8[]) \
             AND p.workspace_id = ANY($2::uuid[]) AND p.state = 1 \
             AND p.memory_kind = ANY($3::int2[]) /*anchor*/ \
           UNION ALL \
           SELECT p.memory_id, p.ordinal, p.token_id, p.weight, p.doc_len FROM search_postings p \
           WHERE p.tenant_id = $1 AND p.memory_id = ANY($4::uuid[]) AND p.kind = 2 \
             AND p.token_id = ANY($5::int8[]) /*anchor*/), \
     hit AS (SELECT p.memory_id, p.ordinal, q.piece, q.w * p.weight::float8 AS c, p.doc_len = 1 AS in_text \
             FROM p JOIN q ON q.token_id = p.token_id), \
     {EXPIRED_IDS}, \
     units AS (SELECT memory_id, ordinal, sum(c) AS score FROM hit \
               WHERE memory_id NOT IN (SELECT id FROM expired) GROUP BY memory_id, ordinal \
               HAVING NOT $8 OR count(*) = $9), \
     top AS (SELECT memory_id, ordinal, score, more_passages FROM ( \
               SELECT memory_id, ordinal, score, 0::int8 AS more_passages FROM units WHERE ordinal < 0 \
               UNION ALL \
               SELECT memory_id, ordinal, score, more_passages FROM ( \
                 SELECT u.*, row_number() OVER (PARTITION BY u.memory_id ORDER BY u.score::float4 DESC, u.ordinal ASC) AS unit_rank, \
                        count(*) OVER (PARTITION BY u.memory_id) - 1 AS more_passages \
                 FROM units u WHERE u.ordinal >= 0) b WHERE b.unit_rank = 1) x \
             ORDER BY score::float4 DESC, memory_id ASC, ordinal ASC LIMIT $10) \
     SELECT t.memory_id AS id, t.ordinal, COALESCE(d.body_bytes, m.body_bytes) AS body_bytes, \
            t.score::float4 AS score, t.more_passages, \
            array_agg(h.piece ORDER BY h.c DESC, h.piece COLLATE \"C\") AS pieces, \
            array_agg(h.c::float4 ORDER BY h.c DESC, h.piece COLLATE \"C\") AS contributions, \
            array_agg(h.in_text ORDER BY h.c DESC, h.piece COLLATE \"C\") AS in_text \
     FROM top t JOIN hit h ON h.memory_id = t.memory_id AND h.ordinal = t.ordinal \
     JOIN memories m ON m.tenant_id = $1 AND m.id = t.memory_id \
     LEFT JOIN document_passages d \
       ON d.tenant_id = $1 AND d.memory_id = t.memory_id AND d.ordinal = t.ordinal \
     GROUP BY t.memory_id, t.ordinal, d.body_bytes, m.body_bytes, t.score, t.more_passages \
     ORDER BY score DESC, id ASC, t.ordinal ASC"), 11)
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
/// them until then; each with its version's search units: its claim, or a
/// document's passages' search texts in ordinal order (ADR-0052 d15;
/// ADR-0081 d4).
const SPARSE_PENDING: &str = "WITH due AS ( \
         SELECT q.memory_id FROM search_sparse_queue q \
          WHERE q.tenant_id = $1 AND q.parked_at IS NULL AND q.next_at <= clock_timestamp() \
          ORDER BY q.enqueued_at, q.memory_id LIMIT $2 FOR UPDATE SKIP LOCKED), \
     leased AS (UPDATE search_sparse_queue q \
                   SET next_at = clock_timestamp() + make_interval(secs => $3) \
                  FROM due WHERE q.tenant_id = $1 AND q.memory_id = due.memory_id \
                RETURNING q.memory_id, q.enqueued_at) \
     SELECT l.memory_id, COALESCE(d.ordinal, -1), COALESCE(d.search_text, m.search_text, m.claim), \
            (extract(epoch FROM l.enqueued_at) * 1000000)::int8 \
       FROM leased l JOIN memories m ON m.tenant_id = $1 AND m.id = l.memory_id \
       LEFT JOIN document_passages d ON m.kind = 'document' AND d.tenant_id = $1 AND d.memory_id = m.id \
      ORDER BY l.enqueued_at, l.memory_id, 2";

/// The postings health statement (ADR-0052 d12), as the serving role under
/// `$1`'s row security, `$2` its partition's name: whether the partition
/// exists; the versions that should have postings (a hit kind, no
/// retirement, not erased, some lexeme) and have none, the fill
/// incomplete; the served versions with no served posting, behind; the
/// per-workspace counts that disagree with the served versions; and, of
/// 200 count rows and as many served (workspace, kind, token)
/// triples, the ones whose count disagrees with the served postings or is
/// missing. The 200 start at a random token each read and wrap round, so
/// repeated reads cover every count; each side's read is bounded by its
/// index (review F7). The version checks read every version of the tenant
/// once with an index probe each: doctor's cost, not a recall's.
const POSTINGS_HEALTH: &str = "WITH v AS ( \
         SELECT m.id, m.workspace_id, oml_search_memory_kind(m.kind) AS mk, m.search_len, \
                m.kind, m.search_vector, \
                oml_search_state(m.status, m.is_tip, m.retirement, m.kind) AS state \
         FROM memories m WHERE m.tenant_id = $1), \
     want AS (SELECT * FROM v WHERE state IS NOT NULL AND search_len > 0 \
                AND (v.kind IS DISTINCT FROM 'document' OR EXISTS (SELECT 1 FROM document_passages d \
                     WHERE d.tenant_id = $1 AND d.memory_id = v.id))), \
     served AS (SELECT v.workspace_id, v.mk, count(DISTINCT v.id)::int8 AS rows, \
                       count(*)::int8 AS units, sum(u.search_len)::int8 AS len \
                FROM v CROSS JOIN LATERAL oml_search_units($1, v.id, v.kind, v.search_vector, v.search_len) u \
                WHERE v.state = 1 GROUP BY v.workspace_id, v.mk), \
     kept AS (SELECT workspace_id, memory_kind AS mk, rows::int8 AS rows, units, total_len AS len \
              FROM search_workspace_rows WHERE tenant_id = $1 AND kind = 1), \
     start AS MATERIALIZED (SELECT ((random() - 0.5) * 1.8e19)::int8 AS s), \
     sampled AS (SELECT * FROM ( \
         (SELECT 0 AS lap, c.workspace_id, c.memory_kind, c.token_id, c.rows \
            FROM search_term_rows c, start WHERE c.tenant_id = $1 AND c.token_id >= start.s \
           ORDER BY c.token_id LIMIT 200) \
         UNION ALL \
         (SELECT 1, c.workspace_id, c.memory_kind, c.token_id, c.rows \
            FROM search_term_rows c, start WHERE c.tenant_id = $1 AND c.token_id < start.s \
           ORDER BY c.token_id LIMIT 200)) x \
       WHERE x.token_id IS NOT NULL ORDER BY lap, token_id LIMIT 200), \
     posted AS (SELECT DISTINCT y.workspace_id, y.memory_kind, y.token_id FROM ( \
         (SELECT 0 AS lap, p.workspace_id, p.memory_kind, p.token_id FROM search_postings p, start \
           WHERE p.tenant_id = $1 AND p.kind = 1 AND p.state = 1 AND p.token_id >= start.s \
           ORDER BY p.token_id LIMIT 200) \
         UNION ALL \
         (SELECT 1, p.workspace_id, p.memory_kind, p.token_id FROM search_postings p, start \
           WHERE p.tenant_id = $1 AND p.kind = 1 AND p.state = 1 AND p.token_id < start.s \
           ORDER BY p.token_id LIMIT 200)) y) \
     SELECT to_regclass($2) IS NOT NULL AS partition, \
       (SELECT count(*) FROM want w WHERE NOT EXISTS ( \
            SELECT 1 FROM search_postings p WHERE p.tenant_id = $1 AND p.memory_id = w.id))::int8 \
         AS unfilled, \
       (SELECT count(*) FROM want w WHERE w.state = 1 AND NOT EXISTS ( \
            SELECT 1 FROM search_postings p WHERE p.tenant_id = $1 AND p.memory_id = w.id \
                                          AND p.state = 1))::int8 AS served_behind, \
       (SELECT count(*) FROM served s FULL JOIN kept k USING (workspace_id, mk) \
         WHERE s.rows IS DISTINCT FROM k.rows OR s.len IS DISTINCT FROM k.len \
            OR s.units IS DISTINCT FROM k.units)::int8 \
         AS workspace_drift, \
       ((SELECT count(*) FROM sampled c WHERE c.rows <> ( \
            SELECT count(DISTINCT p.memory_id) FROM search_postings p \
             WHERE p.tenant_id = $1 AND p.kind = 1 AND p.token_id = c.token_id \
               AND p.workspace_id = c.workspace_id AND p.state = 1 \
               AND p.memory_kind = c.memory_kind)) \
        + (SELECT count(*) FROM posted t WHERE NOT EXISTS ( \
            SELECT 1 FROM search_term_rows c WHERE c.tenant_id = $1 AND c.kind = 1 \
               AND c.workspace_id = t.workspace_id AND c.memory_kind = t.memory_kind \
               AND c.token_id = t.token_id)))::int8 AS term_drift, \
       ((SELECT count(*) FROM sampled) + (SELECT count(*) FROM posted))::int8 AS terms_checked, \
       (SELECT count(*) FROM v WHERE v.kind = 'document' AND v.state IS NOT NULL \
           AND NOT EXISTS (SELECT 1 FROM document_passages d \
                            WHERE d.tenant_id = $1 AND d.memory_id = v.id))::int8 AS documents_uncut";

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
    let branch = |u: &Units| {
        format!(
            "SELECT m.id, {ordinal} AS ordinal, {bytes} AS body_bytes, \
                    word_similarity($6, oml_search_text({text})) AS score, '{{}}'::text[] AS matched \
             FROM {from} \
             WHERE {SEARCH_SCOPE}{anchor} AND {only} AND $6 <% oml_search_text({text})",
            ordinal = u.ordinal,
            bytes = u.bytes,
            text = u.text,
            from = u.from,
            only = u.only,
        )
    };
    with_now(
        &format!(
            "{} ORDER BY score DESC, id ASC, ordinal ASC LIMIT $7",
            one_per_version(&whole_units(&branch), &branch(&PASSAGE_UNITS))
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
                        WHERE v.tenant_id = $1 AND v.workspace_id = ANY($2::uuid[]) \
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
       AND ((s.state = 1 AND s.workspace_id = ANY($3::uuid[])) \
         OR (s.state = 0 AND s.author_key_id = ANY($4::uuid[]) \
             AND s.workspace_id = ANY($5::uuid[]))) \
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
    /// workspace's status as the one v0.5.1 and earlier served, so 0017's
    /// `workspaces_universe_committed` holds over a database whose root a
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
        // A document version written before 0059 has no passages, and is
        // one whole-version unit until they are cut (ADR-0081 d4).
        ensure_document_passages(owner).await.map_err(|e| {
            sqlx::migrate::MigrateError::Execute(sqlx::Error::Protocol(e.to_string()))
        })?;
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
        self.verify_stored_chain_reading(tenant_id, keys, |_| {})
            .await
    }

    /// [`Self::verify_stored_chain`], showing `read` each stored event as it
    /// pages them, so a start that also scans the journal reads it once
    /// (ADR-0091 d4: the former-names check rides on the fork check).
    pub async fn verify_stored_chain_reading(
        &self,
        tenant_id: Uuid,
        keys: &KeySet,
        mut read: impl FnMut(&SignedEnvelope),
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
            page.iter().for_each(&mut read);
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
    /// `workspaces` and `api_keys` rows whose `name` is NULL. 0006 adds the
    /// column NULL on every row it finds and every creation since names its
    /// row, an issuance's key included, so a NULL here is a row the lead
    /// names before it serves (ADR-0026 d9, d11; http-api.md "Rows created
    /// before names").
    pub async fn rows_without_names(
        &self,
        tenants: &[Uuid],
    ) -> Result<Vec<(&'static str, u64)>, StoreError> {
        let (mut tenant_rows, mut workspace_rows, mut key_rows) = (0_i64, 0_i64, 0_i64);
        for tenant_id in tenants {
            let mut tx = self.pool.begin().await.map_err(backend)?;
            set_tenant(&mut tx, *tenant_id).await?;
            let row = sqlx::query(
                "SELECT (SELECT count(*) FROM tenants WHERE id = $1 AND name IS NULL) \
                        AS tenants, \
                        (SELECT count(*) FROM workspaces WHERE tenant_id = $1 AND name IS NULL) \
                        AS workspaces, \
                        (SELECT count(*) FROM api_keys WHERE tenant_id = $1 AND name IS NULL) \
                        AS api_keys",
            )
            .bind(tenant_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
            tenant_rows += row.try_get::<i64, _>("tenants").map_err(backend)?;
            workspace_rows += row.try_get::<i64, _>("workspaces").map_err(backend)?;
            key_rows += row.try_get::<i64, _>("api_keys").map_err(backend)?;
        }
        [
            ("tenants", tenant_rows),
            ("workspaces", workspace_rows),
            ("api_keys", key_rows),
        ]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(table, n)| Ok((table, u64::try_from(n).map_err(backend)?)))
        .collect()
    }

    /// One workspace read under the tenant's RLS context: `sql` binds the
    /// tenant as `$1` and a node id as `$2` and selects [`WORKSPACE_COLUMNS`].
    async fn workspace_rows(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        sql: &str,
    ) -> Result<Vec<WorkspaceRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(sql)
            .bind(tenant_id)
            .bind(workspace_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(workspace_from_row).collect()
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

/// The `workspace_transitions` columns a transition read selects, and the
/// instant of its `workspace.status_proposed`, read off the journal row its
/// `proposed_seq` names in the same statement (ADR-0095, amendment of
/// 2026-11-24).
const TRANSITION_COLUMNS: &str = "id, tenant_id, workspace_id, from_status, to_status, state, \
     proposer_key_id, proposer_principal, ratifier_key_id, \
     (SELECT to_char(j.at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
        FROM journal j \
       WHERE j.tenant_id = workspace_transitions.tenant_id \
         AND j.seq = workspace_transitions.proposed_seq) AS proposed_at";

/// A `workspace_transitions` row as the port's record; a status or state
/// word the record does not spell fails closed.
fn transition_from_row(row: &PgRow) -> Result<TransitionRecord, StoreError> {
    let transition_id: Uuid = row.try_get("id").map_err(backend)?;
    let status = |column: &str| -> Result<WorkspaceStatus, StoreError> {
        let word: String = row.try_get(column).map_err(backend)?;
        WorkspaceStatus::parse(&word).ok_or_else(|| {
            StoreError::Backend(format!("transition {transition_id} has {column} {word:?}"))
        })
    };
    let state: String = row.try_get("state").map_err(backend)?;
    Ok(TransitionRecord {
        transition_id,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        workspace_id: row.try_get("workspace_id").map_err(backend)?,
        from: status("from_status")?,
        to: status("to_status")?,
        state: TransitionState::parse(&state).ok_or_else(|| {
            StoreError::Backend(format!("transition {transition_id} has state {state:?}"))
        })?,
        proposer_key_id: row.try_get("proposer_key_id").map_err(backend)?,
        proposer_principal: row.try_get("proposer_principal").map_err(backend)?,
        ratifier_key_id: row.try_get("ratifier_key_id").map_err(backend)?,
        proposed_at: {
            let at: String = row.try_get("proposed_at").map_err(backend)?;
            EventTime::parse(&at).map_err(backend)?
        },
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

/// One snapshot for the reads of this transaction, so the epochs they
/// return label one journalled state (ADR-0025 d14). SET TRANSACTION must
/// precede every other statement of the transaction.
async fn one_snapshot(tx: &mut Transaction<'_, Postgres>) -> Result<(), StoreError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

/// The reads of [`JournalStore::key_access`], in a transaction that holds
/// [`one_snapshot`] and the tenant's fence.
async fn key_access_in(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    key_id: Uuid,
) -> Result<Option<KeyAccess>, StoreError> {
    let key = sqlx::query("SELECT issued_seq FROM api_keys WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id)
        .bind(key_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(backend)?;
    let Some(key) = key else {
        return Ok(None);
    };
    let grants = sqlx::query(
        "SELECT id, role, workspace_node_id, deny, epoch, covers, may_create FROM grants \
         WHERE tenant_id = $1 AND key_id = $2 AND revoked_epoch IS NULL \
         ORDER BY epoch ASC, id ASC",
    )
    .bind(tenant_id)
    .bind(key_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(backend)?;
    // A lifted denial masks nothing; its lift still moves the epoch.
    let denials = sqlx::query(
        "SELECT id, workspace_node_id, created_by, epoch FROM key_denials \
         WHERE tenant_id = $1 AND key_id = $2 AND lifted_epoch IS NULL \
         ORDER BY epoch ASC, id ASC",
    )
    .bind(tenant_id)
    .bind(key_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(backend)?;
    let lifts = sqlx::query(
        "SELECT lifted_epoch FROM key_denials \
         WHERE tenant_id = $1 AND key_id = $2 AND lifted_epoch IS NOT NULL \
         UNION ALL \
         SELECT revoked_epoch FROM grants \
         WHERE tenant_id = $1 AND key_id = $2 AND revoked_epoch IS NOT NULL \
         ORDER BY 1 ASC",
    )
    .bind(tenant_id)
    .bind(key_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(backend)?;
    // The tenant's public nodes, from the partial index on them: the one
    // read the public reach adds (ADR-0098 d1, the bottleneck register).
    let public_nodes = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM workspaces WHERE tenant_id = $1 AND public ORDER BY id",
    )
    .bind(tenant_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(backend)?;

    let grants = grants
        .iter()
        .map(|row| {
            let role: String = row.try_get("role").map_err(backend)?;
            let deny: Value = row.try_get("deny").map_err(backend)?;
            let covers: String = row.try_get("covers").map_err(backend)?;
            Ok(GrantRecord {
                grant: Grant {
                    grant_id: row.try_get("id").map_err(backend)?,
                    node: row.try_get("workspace_node_id").map_err(backend)?,
                    role: role_from_str(&role)?,
                    deny: deny_from_json(&deny)?,
                    covers: Covers::parse(&covers).ok_or_else(|| {
                        StoreError::Backend(format!("grants.covers {covers:?} is not a coverage"))
                    })?,
                    create: row.try_get("may_create").map_err(backend)?,
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
                    node: row.try_get("workspace_node_id").map_err(backend)?,
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
        public_nodes,
    }))
}

/// The 0001 idempotency PK is `(tenant_id, key)`; the port scope's
/// `(key_id, workspace_id, idem_key)` packs into the key column. Uuids
/// contain no `:`, so the packing is injective.
fn scope_key(scope: &IdempotencyScope) -> String {
    format!("{}:{}:{}", scope.key_id, scope.workspace_id, scope.idem_key)
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

/// The states `memories_status_check` admits (postgres-schema.md
/// `memories`); `lifecycle.demote` writes 'demoted' and `lifecycle.erase`
/// 'erased' (ADR-0089 d16).
fn status_from_str(s: &str) -> Result<MemoryStatus, StoreError> {
    match s {
        "held" => Ok(MemoryStatus::Held),
        "promoted" => Ok(MemoryStatus::Promoted),
        "demoted" => Ok(MemoryStatus::Demoted),
        "rejected" => Ok(MemoryStatus::Rejected),
        "erased" => Ok(MemoryStatus::Erased),
        "withdrawn" => Ok(MemoryStatus::Withdrawn),
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
        "contributor" => Ok(Role::Contributor),
        "submitter" => Ok(Role::Submitter),
        "reviewer" => Ok(Role::Reviewer),
        "admin" => Ok(Role::Admin),
        "operator" => Ok(Role::Operator),
        other => Err(StoreError::Backend(format!("unknown role {other:?}"))),
    }
}

fn used_by_from_str(s: &str) -> Result<UsedBy, StoreError> {
    match s {
        "agent" => Ok(UsedBy::Agent),
        "person" => Ok(UsedBy::Person),
        "service" => Ok(UsedBy::Service),
        other => Err(StoreError::Backend(format!("unknown used_by {other:?}"))),
    }
}

/// The m-qualified memory column list every memory read shares.
/// `created_at` and `promoted_at` come back through `to_char` in the
/// fixed-width signed form so `EventTime` round-trips exactly
/// (timestamptz(6) holds the microseconds).
const MEMORY_COLUMNS: &str = "m.id, m.tenant_id, m.workspace_id, m.claim_key, m.version, \
     m.kind, m.claim, m.claim_digest, m.body_bytes, m.status, m.is_tip, m.retirement, \
     m.replaces_artifact_id, m.author_principal, m.author_key_id, m.author_person, \
     m.derives_from, \
     m.lineage_principals, m.lineage_key_ids, m.scan_finding, m.scanned_under, \
     m.severity, m.rule, \
     m.skill_name, m.skill_description, m.document_title, m.test_shape, \
     m.valid_from, m.valid_until, m.valid_from_us, m.valid_until_us, \
     m.anchor_repo, m.anchor_uri, m.anchor_rev, m.anchor_path, m.anchor_fragment, \
     m.anchor_hash, m.anchor_hasher, m.source_check_rev, m.source_check_result, \
     m.source_check_hash, m.source_check_path, m.source_check_fragment, m.source_check_at, \
     m.format, m.title, m.search_text, m.search_text_by, m.origin_kind, \
     to_char(m.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS created_at_signed, \
     to_char(m.promoted_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS promoted_at_signed, \
     m.approver_used_by, m.approver_same_user, m.admin_fix_id, m.admin_fix_confirmed, m.admin_fix_same_user, \
     m.admin_fix_reverted, \
     CASE WHEN m.admin_fix_id IS NOT NULL THEN (SELECT f.act FROM admin_fixes f \
          WHERE f.tenant_id = m.tenant_id AND f.id = m.admin_fix_id) END AS admin_fix_act, \
     CASE WHEN m.admin_fix_id IS NOT NULL AND m.approved_at IS NULL THEN \
          (SELECT to_char(f.fixed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
             FROM admin_fixes f WHERE f.tenant_id = m.tenant_id AND f.id = m.admin_fix_id) \
     END AS admin_fix_at_signed, \
     to_char(m.approved_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS approved_at_signed";

/// R2 by person in SQL, over `m` and the reviewer key `{key}`: true for a
/// row whose author's person, recorded at its write (`m.author_person`,
/// ADR-0085 d16 as amended 2026-11-18), is the person the reviewer key acts
/// for (a key's user root, its linked root, or the user root its session key
/// names), in a workspace outside that person's root; NULL or false
/// otherwise, so a reviewer acting for no person is never narrowed
/// (ADR-0001 d5; ADR-0082 d7). The tenant is read off `m`, so the serve
/// queue and the held listing, which bind their parameters differently,
/// share one text; `key` is the reviewer key's placeholder, split at
/// [`REVIEWER_KEY`] where the caller binds it.
const SAME_PERSON_OUTSIDE_ROOT: &str = "EXISTS (SELECT 1 FROM api_keys rk \
    CROSS JOIN LATERAL (SELECT COALESCE(rk.user_root_id, rk.linked_user_id, \
        (SELECT u.id FROM workspaces u \
         WHERE u.tenant_id = m.tenant_id AND u.user_root AND u.name = rk.session_for)) AS root) r \
    CROSS JOIN LATERAL (SELECT (SELECT u.id FROM workspaces u \
         WHERE u.tenant_id = m.tenant_id AND u.user_root AND u.name = m.author_person) AS root) a \
    WHERE rk.tenant_id = m.tenant_id AND rk.id = {key} AND r.root = a.root \
      AND NOT EXISTS (SELECT 1 FROM workspaces w \
          WHERE w.tenant_id = m.tenant_id AND w.id = m.workspace_id \
            AND r.root IN (w.id, w.namespace_root_id)))";

/// The reviewer key's placeholder in [`SAME_PERSON_OUTSIDE_ROOT`].
const REVIEWER_KEY: &str = "{key}";

/// The held listing's predicate after `WHERE q.tenant_id = `:
/// review_claim_next's, unclaimed by another key, narrowed by the filter's
/// nodes, author, claim-key prefix and lane (ADR-0055 d2; ADR-0074 d19).
fn push_held_predicates(
    sql: &mut QueryBuilder<'_, Postgres>,
    tenant_id: Uuid,
    reviewer_principal: &str,
    reviewer_key_id: Uuid,
    filter: &HeldFilter,
) {
    sql.push_bind(tenant_id)
        .push(" AND m.tenant_id = ")
        .push_bind(tenant_id)
        .push(" AND m.workspace_id = ANY(")
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
         AND r.workspace_id = m.workspace_id AND r.claim_key = m.claim_key \
         AND (r.author_key_id = ",
        )
        .push_bind(reviewer_key_id)
        .push(" OR r.author_principal = ")
        .push_bind(reviewer_principal.to_owned())
        .push(")))");
    // R2 by person, as review_claim_next serves it (ADR-0001 d5; ADR-0082
    // d7): the listing and its counts hold no row the reviewer may not judge.
    let (before, after) = SAME_PERSON_OUTSIDE_ROOT
        .split_once(REVIEWER_KEY)
        .expect("the predicate names the reviewer key once");
    sql.push(" AND NOT ")
        .push(before)
        .push_bind(reviewer_key_id)
        .push(after);
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
    if let Some(lane) = lane_sql(filter.lane, "m") {
        sql.push(" AND ").push(lane);
    }
}

/// The person lane's predicate on the memories row aliased `m`: an instruction-class version, or a
/// successor that replaces one (ADR-0080 d16), the kinds spelled from the
/// domain's class table. `None` for the lane that admits every row.
fn lane_sql(lane: ReviewLane, m: &str) -> Option<String> {
    let kinds = MemoryKind::ALL
        .iter()
        .filter(|k| k.class().needs_person())
        .map(|k| format!("'{}'", k.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let needs = format!(
        "({m}.kind IN ({kinds}) OR ({m}.replaces_artifact_id IS NOT NULL AND EXISTS ( \
         SELECT 1 FROM memories t WHERE t.tenant_id = {m}.tenant_id \
         AND t.id = {m}.replaces_artifact_id AND t.kind IN ({kinds}))))"
    );
    match lane {
        ReviewLane::Any => None,
        ReviewLane::Person => Some(needs),
        ReviewLane::Reviewer => Some(format!("NOT {needs}")),
    }
}

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
    let kind = MemoryKind::parse(&kind)
        .ok_or_else(|| StoreError::Backend(format!("unknown memory kind {kind:?}")))?;
    let kind_fields = kind_fields_from_row(row, kind)?.map(Box::new);
    Ok(MemoryRecord {
        memory_id: row.try_get("id").map_err(backend)?,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        workspace_id: row.try_get("workspace_id").map_err(backend)?,
        claim_key: row.try_get("claim_key").map_err(backend)?,
        version: unsigned(row, "version")?,
        kind,
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
        author_person: stored_name(row, "author_person")?,
        derives_from: row.try_get("derives_from").map_err(backend)?,
        lineage_principals: row.try_get("lineage_principals").map_err(backend)?,
        lineage_key_ids: row.try_get("lineage_key_ids").map_err(backend)?,
        scan_finding: row.try_get("scan_finding").map_err(backend)?,
        scanned_under: row.try_get("scanned_under").map_err(backend)?,
        created_at: EventTime::parse(&created).map_err(backend)?,
        guardrail,
        kind_fields,
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

/// A skill's, a document's or a test case's fields from their columns,
/// `None` on every other kind and on a retirement, which store none
/// (migration 0048; ADR-0080 d1; ADR-0081 d1; ADR-0086 d1).
fn kind_fields_from_row(row: &PgRow, kind: MemoryKind) -> Result<Option<KindFields>, StoreError> {
    Ok(match kind {
        MemoryKind::Skill => {
            let name: Option<String> = row.try_get("skill_name").map_err(backend)?;
            let description: Option<String> = row.try_get("skill_description").map_err(backend)?;
            name.zip(description)
                .map(|(name, description)| KindFields::Skill { name, description })
        }
        MemoryKind::Document => {
            let title: Option<String> = row.try_get("document_title").map_err(backend)?;
            title.map(|title| KindFields::Document { title: Some(title) })
        }
        MemoryKind::Test => {
            let shape: Option<String> = row.try_get("test_shape").map_err(backend)?;
            shape
                .map(|word| {
                    TestShape::parse(&word)
                        .map(|shape| KindFields::Test { shape })
                        .ok_or_else(|| StoreError::Backend(format!("unknown test shape {word:?}")))
                })
                .transpose()?
        }
        _ => None,
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
    let approved_at: Option<String> = row.try_get("approved_at_signed").map_err(backend)?;
    // A version served because of an admin fix reads as the fix marks it:
    // used by a person, unconfirmed until a person confirms it, never
    // naming which admin, approved at the fix's time where the row holds
    // no approval of its own, as the double reads it (ADR-0089 d7; 0064).
    // Read before the row's own approval, so a legacy row with none is
    // still marked.
    let fix_id: Option<Uuid> = row.try_get("admin_fix_id").map_err(backend)?;
    if let Some(fix_id) = fix_id {
        let confirmed: Option<bool> = row.try_get("admin_fix_confirmed").map_err(backend)?;
        let same_user: Option<bool> = row.try_get("admin_fix_same_user").map_err(backend)?;
        let reverted: Option<bool> = row.try_get("admin_fix_reverted").map_err(backend)?;
        let act: Option<String> = row.try_get("admin_fix_act").map_err(backend)?;
        let fixed_at: Option<String> = row.try_get("admin_fix_at_signed").map_err(backend)?;
        let at = approved_at.or(fixed_at).ok_or_else(|| {
            StoreError::Backend(format!("fix {fix_id} marks a version with no time"))
        })?;
        return Ok(Some(Approval {
            used_by: UsedBy::Person,
            same_user: Some(same_user.unwrap_or(false)),
            at: EventTime::parse(&at).map_err(backend)?,
            fix: Some(oml_app::types::FixMark {
                fix_id,
                confirmed: confirmed.unwrap_or(false),
                reverted: reverted.unwrap_or(false),
                written: matches!(act.as_deref(), Some("edit" | "retire")),
            }),
        }));
    }
    let Some(at) = approved_at else {
        return Ok(None);
    };
    let used_by: Option<String> = row.try_get("approver_used_by").map_err(backend)?;
    let used_by = used_by
        .ok_or_else(|| StoreError::Backend("an approval without its attestation".to_owned()))?;
    Ok(Some(Approval {
        used_by: used_by_from_str(&used_by)?,
        same_user: row.try_get("approver_same_user").map_err(backend)?,
        at: EventTime::parse(&at).map_err(backend)?,
        fix: None,
    }))
}

/// A row's `kind` column as the kind it names; a kind this build does not
/// know fails the read by name rather than being guessed.
/// A statement that selects no `kind` leaves it to [`PgStore::fill_kinds`],
/// which reads every candidate's kind before the list is returned.
fn kind_of_row(row: &PgRow) -> Result<MemoryKind, StoreError> {
    let Ok(kind) = row.try_get::<String, _>("kind") else {
        return Ok(MemoryKind::Memory);
    };
    MemoryKind::parse(&kind)
        .ok_or_else(|| StoreError::Backend(format!("a stored row names an unknown kind {kind:?}")))
}

/// A search unit's ordinal from a row's `ordinal`: -1, the whole version,
/// when the statement selects none (migration 0059).
fn unit_ordinal(row: &PgRow) -> i32 {
    row.try_get::<i32, _>("ordinal").unwrap_or(-1)
}

/// A search unit's count of its version's other matching passages from a
/// row's `more_passages`: 0 when the statement selects none.
fn unit_more_passages(row: &PgRow) -> u32 {
    row.try_get::<i64, _>("more_passages")
        .ok()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0)
}

/// A recall candidate from a row of `id`, `body_bytes` and `score`, and
/// `matched`, `ordinal` and `more_passages` when the statement selects them.
fn candidate_from_row(row: &PgRow) -> Result<Candidate, StoreError> {
    let matched: Option<Vec<String>> = row.try_get("matched").ok();
    Ok(Candidate {
        memory_id: row.try_get("id").map_err(backend)?,
        kind: kind_of_row(row)?,
        ordinal: unit_ordinal(row),
        body_bytes: unsigned(row, "body_bytes")?,
        score: row.try_get("score").map_err(backend)?,
        matched: matched.unwrap_or_default(),
        tokens: Vec::new(),
        more_passages: unit_more_passages(row),
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
        kind: kind_of_row(row)?,
        ordinal: unit_ordinal(row),
        body_bytes: unsigned(row, "body_bytes")?,
        score: row.try_get("score").map_err(backend)?,
        matched,
        tokens,
        more_passages: unit_more_passages(row),
    })
}

/// The listing's predicate on `m` after `m.tenant_id = <tenant>`: the
/// scope, then every filter; `every_kind` leaves the kinds filter out, as
/// the catalog's counts do (ADR-0092 d6).
fn push_listing_predicate(
    sql: &mut QueryBuilder<'_, Postgres>,
    filter: &ListingFilter,
    every_kind: bool,
) {
    // The scope is the store's own predicate: the own key set off the
    // masked nodes (memories_author serves it), or promoted tips on the
    // node set alone — an empty array matches nothing (R3; ADR-0025 d1,
    // d13).
    match &filter.scope {
        ListingScope::Own { key_ids, masked } => {
            sql.push(" AND m.author_key_id = ANY(")
                .push_bind(key_ids.clone())
                .push("::uuid[]) AND NOT (m.workspace_id = ANY(")
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
                // d5, amended 2026-11-10); any kind else (ADR-0080 d16).
                sql.push(" OR (");
                if let Some(at) = &filter.as_of {
                    sql.push("m.created_at <= (")
                        .push_bind(at.as_str().to_owned())
                        .push(")::timestamptz AND ");
                }
                sql.push(
                    "m.status = 'held' AND NOT m.retirement \
                     AND m.scan_finding IS NULL \
                     AND m.workspace_id = ANY(",
                )
                .push_bind(held.clone())
                .push("::uuid[]) AND m.scanned_under IS NOT DISTINCT FROM ")
                .push_bind(current_scanner.clone())
                .push("::text)");
            }
            sql.push(") AND m.workspace_id = ANY(")
                .push_bind(nodes.clone())
                .push("::uuid[])");
        }
    }
    if let Some(subtree) = &filter.subtree {
        sql.push(" AND m.workspace_id = ANY(")
            .push_bind(subtree.clone())
            .push("::uuid[])");
    }
    // Any one state clause admits a row (ADR-0083 d3).
    if !filter.states.is_empty() {
        sql.push(" AND (");
        for (i, clause) in filter.states.iter().enumerate() {
            if i > 0 {
                sql.push(" OR ");
            }
            sql.push("(m.status = ").push_bind(clause.status.as_str());
            if let Some(tip) = clause.tip {
                sql.push(" AND m.is_tip = ").push_bind(tip);
            }
            if let Some(since) = &clause.promoted_since {
                sql.push(" AND m.promoted_at >= (")
                    .push_bind(since.as_str().to_owned())
                    .push(")::timestamptz");
            }
            sql.push(")");
        }
        sql.push(")");
    }
    if !filter.kinds.is_empty() && !every_kind {
        sql.push(" AND m.kind = ANY(")
            .push_bind(filter.kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>())
            .push("::text[])");
    }
    // A claim key prefix as a byte range, never LIKE, so row security
    // never evaluates a pattern before its policy (ADR-0092 d9).
    if let Some(prefix) = &filter.prefix {
        sql.push(" AND m.claim_key COLLATE \"C\" >= ")
            .push_bind(prefix.clone());
        if let Some(upper) = prefix_upper(prefix) {
            sql.push(" AND m.claim_key COLLATE \"C\" < ")
                .push_bind(upper);
        }
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
}

/// The kinds of `kinds` a recall serves as hits by their node: steering
/// never is, whatever the filter names, and the instruction kinds are
/// candidates by id alone (ADR-0092 d2, d3).
fn hit_kinds(kinds: &[MemoryKind]) -> Vec<&'static str> {
    kinds
        .iter()
        .filter(|kind| HIT_KINDS.contains(kind) && !INSTRUCTION_HIT_KINDS.contains(kind))
        .map(|kind| kind.as_str())
        .collect()
}

/// A bm25 statement's candidates narrowed to the kinds `scope` asks, a
/// predicate on `m` naming them as literals from [`MemoryKind::as_str`]'s
/// closed set (ADR-0092 d4): empty when they hold every kind the statement
/// reads, which a recall naming no `kinds` always does, so its statement is
/// the one it always was.
fn kind_narrowing(scope: &CandidateScope) -> String {
    let read = scope
        .counted_node_kinds()
        .into_iter()
        .chain(scope.instructions.iter().map(|(_, k)| *k));
    let mut asked = read.filter(|k| !scope.kinds.contains(k)).peekable();
    if asked.peek().is_none() {
        return String::new();
    }
    let names: Vec<String> = scope
        .kinds
        .iter()
        .filter(|k| HIT_KINDS.contains(k))
        .map(|k| format!("'{}'", k.as_str()))
        .collect();
    if names.is_empty() {
        " AND false".to_owned()
    } else {
        format!(" AND m.kind IN ({})", names.join(", "))
    }
}

/// A kind's code in the postings and their counts, as
/// `oml_search_memory_kind` stores it (migrations 0021, 0062; ADR-0092 d4);
/// a test has none, since it is never a hit (ADR-0086 d1).
fn memory_kind_code(kind: &str) -> Option<i16> {
    match kind {
        "memory" => Some(1),
        "derived_fact" => Some(2),
        "document" => Some(3),
        "skill" => Some(5),
        "guardrail" => Some(6),
        "term" => Some(7),
        "term_mapping" => Some(8),
        "vocab_release" => Some(9),
        _ => None,
    }
}

/// The workspace columns every workspace read selects (unqualified: the
/// recursive walks select them from their CTE).
const WORKSPACE_COLUMNS: &str = "id, tenant_id, parent_id, name, status, crew, crew_id, user_root, namespace_root_id, mode, \
     public, public_act_id";

/// A workspace row as the port's record. A row with no name yet predates
/// names and is served to no one: the lead names every such row before it
/// serves (ADR-0026 d11), and until that naming is built a lead whose store
/// holds one serves no request ([`PgStore::rows_without_names`]), so one
/// found here fails closed.
fn workspace_from_row(row: &PgRow) -> Result<WorkspaceRecord, StoreError> {
    let workspace_id: Uuid = row.try_get("id").map_err(backend)?;
    let crew: Option<Value> = row.try_get("crew").map_err(backend)?;
    Ok(WorkspaceRecord {
        workspace_id,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        parent_id: row.try_get("parent_id").map_err(backend)?,
        name: stored_name(row, "name")?.ok_or_else(|| {
            StoreError::Backend(format!(
                "workspace {workspace_id} has no name: the lead names every row that predates \
                 names before it serves (ADR-0026 d11)"
            ))
        })?,
        status: {
            let status: String = row.try_get("status").map_err(backend)?;
            WorkspaceStatus::parse(&status).ok_or_else(|| {
                StoreError::Backend(format!("workspace {workspace_id} has status {status:?}"))
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
        public: row.try_get("public").map_err(backend)?,
        public_act_id: row.try_get("public_act_id").map_err(backend)?,
    })
}

/// The `api_keys` columns a key read selects. `revoked_at` and
/// `expires_at` come back through `to_char` in the fixed-width signed form,
/// as a memory's `promoted_at` does, so `EventTime` round-trips exactly.
const KEY_COLUMNS: &str = "id, tenant_id, role, principal, used_by, default_workspace_id, \
     issued_by, hash, credential, name, crew_id, user_root_id, session_for, guardrail_scope, \
     profile, requests_per_minute, linked_user_id, link_pending_user_id, pending_act_id, \
     suspended_cause, suspended_act_id, \
     to_char(suspended_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS suspended_at_signed, \
     to_char(revoked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS revoked_at_signed, \
     to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS expires_at_signed";

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

/// The columns an import's record reads, its position counted from the
/// versions recorded (ADR-0084 d15).
const IMPORT_COLUMNS: &str = "i.tenant_id, i.id, i.target_workspace_id, i.importer_key_id, \
     i.upload_id, i.source_server_id, i.source_tenant_id, i.manifest_digest, \
     i.signer_fingerprint, i.authenticated, i.allow_older, i.state, i.counts, i.refusals, \
     to_char(i.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
         AS created_at_signed, \
     to_char(i.ended_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
         AS ended_at_signed, \
     (SELECT count(*) FROM import_versions v \
       WHERE v.tenant_id = i.tenant_id AND v.import_id = i.id) AS position";

/// An import row as the port's record.
fn import_from_row(row: &PgRow) -> Result<oml_app::types::ImportRecord, StoreError> {
    let state: String = row.try_get("state").map_err(backend)?;
    let counts: Value = row.try_get("counts").map_err(backend)?;
    let refusals: Value = row.try_get("refusals").map_err(backend)?;
    let position: i64 = row.try_get("position").map_err(backend)?;
    Ok(oml_app::types::ImportRecord {
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        import_id: row.try_get("id").map_err(backend)?,
        target_workspace_id: row.try_get("target_workspace_id").map_err(backend)?,
        importer_key_id: row.try_get("importer_key_id").map_err(backend)?,
        upload_id: row.try_get("upload_id").map_err(backend)?,
        source_server_id: row.try_get("source_server_id").map_err(backend)?,
        source_tenant_id: row.try_get("source_tenant_id").map_err(backend)?,
        manifest_digest: row.try_get("manifest_digest").map_err(backend)?,
        signer_fingerprint: row.try_get("signer_fingerprint").map_err(backend)?,
        authenticated: row.try_get("authenticated").map_err(backend)?,
        allow_older: row.try_get("allow_older").map_err(backend)?,
        state: oml_app::domain::import::ImportState::parse(&state)
            .ok_or_else(|| StoreError::Backend(format!("import state {state:?}")))?,
        position: u64::try_from(position).map_err(backend)?,
        counts: serde_json::from_value(counts).map_err(backend)?,
        refusals: serde_json::from_value(refusals).map_err(backend)?,
        created_at: event_time_column(row, "created_at_signed")?
            .ok_or_else(|| StoreError::Backend("imports.created_at is null".to_owned()))?,
        ended_at: event_time_column(row, "ended_at_signed")?,
    })
}

const BRIEF_EDGE_COLUMNS: &str = "e.id, e.tenant_id, e.artifact_id, e.workspace_id, e.state, \
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
        workspace_id: row.try_get("workspace_id").map_err(backend)?,
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
/// A key row's suspension (ADR-0097 d8, d21; 0073): `None` when no cause
/// is stored.
fn suspension_from_row(row: &PgRow) -> Result<Option<KeySuspension>, StoreError> {
    let cause: Option<String> = row.try_get("suspended_cause").map_err(backend)?;
    let Some(cause) = cause else {
        return Ok(None);
    };
    let at: Option<String> = row.try_get("suspended_at_signed").map_err(backend)?;
    Ok(Some(KeySuspension {
        cause: SuspensionCause::parse(&cause).ok_or_else(|| {
            StoreError::Backend(format!("api_keys.suspended_cause {cause:?} is not a cause"))
        })?,
        act_id: row.try_get("suspended_act_id").map_err(backend)?,
        at: at
            .as_deref()
            .map(EventTime::parse)
            .transpose()
            .map_err(|e| StoreError::Backend(e.to_string()))?
            .ok_or_else(|| StoreError::Backend("api_keys.suspended_at is null".to_owned()))?,
    }))
}

fn key_from_row(row: &PgRow) -> Result<KeyRecord, StoreError> {
    let tenant_id: Option<Uuid> = row.try_get("tenant_id").map_err(backend)?;
    let role: String = row.try_get("role").map_err(backend)?;
    let used_by: String = row.try_get("used_by").map_err(backend)?;
    let stored_hash: Option<Vec<u8>> = row.try_get("hash").map_err(backend)?;
    let credential: String = row.try_get("credential").map_err(backend)?;
    let revoked: Option<String> = row.try_get("revoked_at_signed").map_err(backend)?;
    let expires: Option<String> = row.try_get("expires_at_signed").map_err(backend)?;
    let rate: Option<i32> = row.try_get("requests_per_minute").map_err(backend)?;
    let scope: String = row.try_get("guardrail_scope").map_err(backend)?;
    Ok(KeyRecord {
        key_id: row.try_get("id").map_err(backend)?,
        tenant_id: tenant_id.unwrap_or(PROVIDER_TENANT),
        role: role_from_str(&role)?,
        principal: row.try_get("principal").map_err(backend)?,
        used_by: used_by_from_str(&used_by)?,
        default_workspace: row.try_get("default_workspace_id").map_err(backend)?,
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
        expiry: expires
            .as_deref()
            .map(EventTime::parse)
            .transpose()
            .map_err(backend)?,
        requests_per_minute: rate.map(u32::try_from).transpose().map_err(backend)?,
        linked_user: row.try_get("linked_user_id").map_err(backend)?,
        link_pending: row.try_get("link_pending_user_id").map_err(backend)?,
        pending_act: row.try_get("pending_act_id").map_err(backend)?,
        suspended: suspension_from_row(row)?,
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

/// An import's write's `import_versions` row and the version's
/// `import_provenance`, in the write's own transaction (ADR-0084 I14).
async fn record_imported_version(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    memory_id: Uuid,
    imported: &oml_app::types::ImportedWrite,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO import_versions (tenant_id, import_id, memory_id, source_artifact_id, \
                source_version, outcome) VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT DO NOTHING",
    )
    .bind(tenant_id)
    .bind(imported.import_id)
    .bind(memory_id)
    .bind(&imported.source_artifact_id)
    .bind(i64::try_from(imported.source_version).map_err(backend)?)
    .bind(&imported.outcome)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    sqlx::query("UPDATE memories SET import_provenance = $3 WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id)
        .bind(memory_id)
        .bind(&imported.provenance)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    Ok(())
}

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

/// Writes a document version's passages (ADR-0081 d3; migration 0059),
/// derived from its body by the one rule, so a replayed fold cuts the same
/// passages; nothing for any other version. Returns how many it wrote.
async fn insert_passages(
    tx: &mut Transaction<'_, Postgres>,
    m: &MemoryRecord,
) -> Result<usize, StoreError> {
    let units = m.passage_units();
    if units.is_empty() {
        return Ok(0);
    }
    let mut ordinals = Vec::with_capacity(units.len());
    let mut starts = Vec::with_capacity(units.len());
    let mut ends = Vec::with_capacity(units.len());
    let mut paths = Vec::with_capacity(units.len());
    let mut fragments = Vec::with_capacity(units.len());
    let mut bytes = Vec::with_capacity(units.len());
    let mut texts = Vec::with_capacity(units.len());
    for u in &units {
        ordinals.push(i32::try_from(u.passage.ordinal).map_err(backend)?);
        starts.push(i32::try_from(u.passage.start_line).map_err(backend)?);
        ends.push(i32::try_from(u.passage.end_line).map_err(backend)?);
        // A text[] per passage travels as one JSON array, unnested below,
        // since sqlx binds no array of arrays.
        paths.push(serde_json::to_string(&u.passage.heading_path).map_err(backend)?);
        fragments.push(u.passage.fragment.as_str());
        bytes.push(i32::try_from(u.body_bytes).map_err(backend)?);
        texts.push(u.search_text.as_str());
    }
    sqlx::query(
        "INSERT INTO document_passages (tenant_id, memory_id, ordinal, start_line, end_line, \
                                        heading_path, fragment, body_bytes, search_text) \
         SELECT $1, $2, o, s, e, ARRAY(SELECT jsonb_array_elements_text(h::jsonb)), f, b, t \
           FROM unnest($3::int4[], $4::int4[], $5::int4[], $6::text[], $7::text[], \
                       $8::int4[], $9::text[]) AS x(o, s, e, h, f, b, t)",
    )
    .bind(m.tenant_id)
    .bind(m.memory_id)
    .bind(&ordinals)
    .bind(&starts)
    .bind(&ends)
    .bind(&paths)
    .bind(&fragments)
    .bind(&bytes)
    .bind(&texts)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(units.len())
}

/// Cuts the passages of every document version written before migration
/// 0059, which a migration cannot cut (the rule is this crate's, not
/// SQL's), and refills the postings and counts of each tenant that had
/// one, so those versions are searched by passage as a new one is
/// (ADR-0081 d3, d4). Each tenant in its own transaction under its row
/// security; the refill takes its chain lock. Returns the versions cut.
async fn ensure_document_passages(owner: &PgPool) -> Result<u64, StoreError> {
    let mut tx = owner.begin().await.map_err(backend)?;
    sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM tenants ORDER BY id")
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
    tx.commit().await.map_err(backend)?;
    let mut cut = 0;
    for tenant_id in tenants {
        let mut tx = owner.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The tenant's chain lock, which the refill takes too: a second lead
        // starting at once waits here, then finds the passages cut.
        sqlx::query(
            "SELECT pg_advisory_xact_lock( \
                 ('x' || encode(substr(sha256(uuid_send($1)), 1, 8), 'hex'))::bit(64)::bigint)",
        )
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let rows = sqlx::query(&format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = $1 \
               AND m.kind = 'document' AND NOT m.retirement AND m.status <> 'erased' \
               AND NOT EXISTS (SELECT 1 FROM document_passages d \
                                WHERE d.tenant_id = $1 AND d.memory_id = m.id)"
        ))
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let mut written = 0;
        for row in &rows {
            let m = memory_from_row(row)?;
            if insert_passages(&mut tx, &m).await? > 0 {
                written += 1;
            }
        }
        if written > 0 {
            sqlx::query("SELECT oml_search_postings_refill($1)")
                .bind(tenant_id)
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
        }
        tx.commit().await.map_err(backend)?;
        cut += written;
    }
    Ok(cut)
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
    // The author's person, recorded once at the write (ADR-0085 d16,
    // amended 2026-11-18; migration 0061), named last and only when the key
    // acts for one.
    let person_n = scanner_n + usize::from(m.scanned_under.is_some());
    let (person_column, person_value) = if m.author_person.is_some() {
        (", author_person".to_owned(), format!(", ${person_n}"))
    } else {
        (String::new(), String::new())
    };
    // A document version's passages before its row, so the row's insert
    // fold finds them and writes its postings and counts one unit per
    // passage (ADR-0081 d3, d4; migration 0059).
    insert_passages(tx, m).await?;
    let sql = format!(
        "INSERT INTO memories (id, tenant_id, workspace_id, claim, claim_digest, status, \
         created_event_id, created_at, claim_key, author_principal, \
         author_key_id, derives_from, lineage_principals, lineage_key_ids, scan_finding, \
         version, kind, body_bytes, is_tip, promoted_at, retirement, replaces_artifact_id, \
         severity, rule{columns}{origin_column}{scanner_column}{person_column}) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, ($8)::timestamptz, $9, $10, $11, $12, $13, $14, \
         $15, $16, $17, $18, $19, ($20)::timestamptz, $21, $22, $23, $24{values}{origin_value}\
         {scanner_value}{person_value})"
    );
    let mut insert = sqlx::query(&sql)
        .bind(m.memory_id)
        .bind(m.tenant_id)
        .bind(m.workspace_id)
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
    if let Some(person) = &m.author_person {
        insert = insert.bind(person.as_str());
    }
    insert.execute(&mut **tx).await.map_err(backend)?;
    // A skill's, a document's or a test case's fields in the same
    // transaction, only when the version carries any (migration 0048).
    if let Some(fields) = m.kind_fields.as_deref() {
        let (name, description, title, shape) = match fields {
            KindFields::Skill { name, description } => {
                (Some(name.as_str()), Some(description.as_str()), None, None)
            }
            KindFields::Document { title } => (None, None, title.as_deref(), None),
            KindFields::Test { shape } => (None, None, None, Some(shape.as_str())),
        };
        sqlx::query(
            "UPDATE memories SET skill_name = $3, skill_description = $4, document_title = $5, \
             test_shape = $6 WHERE tenant_id = $1 AND id = $2",
        )
        .bind(m.tenant_id)
        .bind(m.memory_id)
        .bind(name)
        .bind(description)
        .bind(title)
        .bind(shape)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
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
            "UPDATE memories SET approver_used_by = $3, approver_same_user = $4, \
             approved_at = ($5)::timestamptz WHERE tenant_id = $1 AND id = $2",
        )
        .bind(m.tenant_id)
        .bind(m.memory_id)
        .bind(approval.used_by.as_str())
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
             promoted_at = ($3)::timestamptz, approver_used_by = $4, \
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
            .bind(approval.used_by.as_str())
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
/// own workspace and `claim_key` and the successor replaces it, reading the
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
           AND t.workspace_id = s.workspace_id AND t.claim_key = s.claim_key \
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
        // The end it had is kept for a demotion that restores it (0058;
        // ADR-0089 d16, note of 2026-11-18).
        sqlx::query(
            "UPDATE memories SET valid_until_ratified = true, \
                    valid_until_before = valid_until, valid_until_before_us = valid_until_us, \
                    valid_until = $3, valid_until_us = $4 \
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

/// The fold of `lifecycle.demote` (ADR-0089 d16): the compare-and-set
/// demotes `memory_id` only while it is promoted and, with `restored`, the
/// tip whose greatest promoted version below is `restored`, not the tip;
/// without, while it is not the tip or no promoted version lies below it.
/// A zero row count refuses the commit, which rolls back. `reopened` is
/// compare-and-set the same way (#474): the version `memory_id` replaces,
/// still promoted and ended by a ratification, locked, must be the one named
/// with the end named, or none must exist when none is named. The postings
/// and the similarity keys follow the state through their triggers; nothing
/// is deleted (R4).
async fn demote_memory(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    memory_id: Uuid,
    restored: Option<Uuid>,
    reopened: Option<&(Uuid, Option<ValidEnd>)>,
) -> Result<(), StoreError> {
    let moved = || StoreError::TipMoved {
        replaced: memory_id,
    };
    // The promoted version directly below, read in the statement that
    // demotes, so the check and the write see one row set.
    let demoted = sqlx::query(
        "WITH d AS (SELECT * FROM memories WHERE tenant_id = $1 AND id = $2 \
                     AND status = 'promoted' FOR UPDATE), \
              b AS (SELECT s.id, s.is_tip FROM memories s, d \
                     WHERE s.tenant_id = $1 AND s.workspace_id = d.workspace_id \
                       AND s.claim_key = d.claim_key AND s.version < d.version \
                       AND s.status = 'promoted' \
                     ORDER BY s.version DESC LIMIT 1) \
         UPDATE memories m SET status = 'demoted', is_tip = false FROM d \
          WHERE m.tenant_id = $1 AND m.id = d.id \
            AND CASE WHEN $3::uuid IS NULL \
                     THEN NOT d.is_tip OR NOT EXISTS (SELECT 1 FROM b) \
                     ELSE d.is_tip AND EXISTS (SELECT 1 FROM b WHERE b.id = $3 AND NOT b.is_tip) \
                END",
    )
    .bind(tenant_id)
    .bind(memory_id)
    .bind(restored)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    if demoted.rows_affected() != 1 {
        return Err(moved());
    }
    // The version it replaced, still promoted, gets back the end it had
    // before this version's ratification, whichever version is the tip
    // (0058; ADR-0089 d16, note of 2026-11-18). Locked and compared with
    // what the demotion read and signed (#474).
    let reopenable: Option<(Uuid, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT r.id, r.valid_until_before, r.valid_until_before_us \
           FROM memories r, memories d \
          WHERE d.tenant_id = $1 AND d.id = $2 \
            AND r.tenant_id = $1 AND r.id = d.replaces_artifact_id \
            AND r.status = 'promoted' AND r.valid_until_ratified \
          FOR UPDATE OF r",
    )
    .bind(tenant_id)
    .bind(memory_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    let reopenable = reopenable.map(|(r, text, micros)| {
        (
            r,
            text.zip(micros).map(|(t, m)| ValidEnd::from_stored(t, m)),
        )
    });
    if reopenable.as_ref() != reopened {
        return Err(moved());
    }
    if let Some((r, _)) = reopened {
        sqlx::query(
            "UPDATE memories SET valid_until = valid_until_before, \
                    valid_until_us = valid_until_before_us, valid_until_ratified = false, \
                    valid_until_before = NULL, valid_until_before_us = NULL \
              WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant_id)
        .bind(r)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
    if let Some(restored) = restored {
        // `memories_served_tip` admits it: the demoted version left the tip
        // in the statement above.
        let back = sqlx::query(
            "UPDATE memories SET is_tip = true \
              WHERE tenant_id = $1 AND id = $2 AND status = 'promoted' AND NOT is_tip",
        )
        .bind(tenant_id)
        .bind(restored)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
        if back.rows_affected() != 1 {
            return Err(moved());
        }
    }
    Ok(())
}

/// The fold of `lifecycle.erase` (ADR-0089 d16; ADR-0005 d2, amended
/// 2026-11-17): the compare-and-set takes the ids only while they are every
/// version of one claim; then each row is `erased`, off the tip, and its
/// body and every column derived from it emptied, so the triggers drop its
/// postings and similarity keys; its passages go, its recall telemetry is
/// erased, and its data key, where the lake writer keyed it, is destroyed.
/// The digest, the title, the attribution, the lineage and `body_bytes`
/// stay (ADR-0005 d3, d5).
async fn erase_claim(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    memory_ids: &[Uuid],
) -> Result<(), StoreError> {
    let first = memory_ids.first().copied().unwrap_or_default();
    let moved = || StoreError::TipMoved { replaced: first };
    // The claim's versions, locked, compared with the ids named.
    let claim: Vec<Uuid> = sqlx::query_scalar(
        "SELECT m.id FROM memories m, memories n \
          WHERE n.tenant_id = $1 AND n.id = $2 \
            AND m.tenant_id = $1 AND m.workspace_id = n.workspace_id \
            AND m.claim_key = n.claim_key \
          ORDER BY m.id FOR UPDATE OF m",
    )
    .bind(tenant_id)
    .bind(first)
    .fetch_all(&mut **tx)
    .await
    .map_err(backend)?;
    let mut named = memory_ids.to_vec();
    named.sort_unstable();
    named.dedup();
    if claim.is_empty() || claim != named {
        return Err(moved());
    }
    // `claim` and `search_vector` are NOT NULL: the body becomes empty, and
    // `memories_search_vector` sets the empty lexeme set from it.
    sqlx::query(
        "UPDATE memories SET status = 'erased', is_tip = false, claim = '', terms = '{}', \
                rule = NULL, search_text = NULL, search_text_by = NULL \
          WHERE tenant_id = $1 AND id = ANY($2::uuid[])",
    )
    .bind(tenant_id)
    .bind(&named)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    for sql in [
        "DELETE FROM review_queue WHERE tenant_id = $1 AND memory_id = ANY($2::uuid[])",
        "DELETE FROM document_passages WHERE tenant_id = $1 AND memory_id = ANY($2::uuid[])",
        // Migration 0056's grant: the update that destroys a key, never a
        // read of the wrapped key itself.
        "UPDATE artifact_keys SET wrapped_dek = NULL, destroyed_at = now() \
          WHERE tenant_id = $1 AND artifact_id = ANY($2::uuid[]) AND destroyed_at IS NULL",
        "SELECT oml_telemetry_erase($1, $2::uuid[])",
    ] {
        sqlx::query(sql)
            .bind(tenant_id)
            .bind(&named)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
    }
    Ok(())
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
/// event's `at`; the keys refuse a claim, workspace or proposer of another
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
        "INSERT INTO brief_edges (id, tenant_id, artifact_id, workspace_id, state, \
         proposer_key_id, ratifier_key_id, ratified_seq, created_at) \
         VALUES ($1, $2, $3, $4, 'pending', $5, NULL, NULL, ($6)::timestamptz)",
    )
    .bind(edge.edge_id)
    .bind(tenant_id)
    .bind(edge.artifact_id)
    .bind(edge.workspace_id)
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
/// crew record inserted, each listed row — the tenant, its workspaces, its
/// keys — named where its `name` is still NULL, each listed workspace
/// pointed at its crew record, and every workspace path that is still NULL
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
            NamedKind::Workspace => (
                "UPDATE workspaces SET name = $3 WHERE tenant_id = $1 AND id = $2 \
                 AND name IS NULL",
                if assignment.name.as_str() == UNIVERSE {
                    "workspaces_universe_is_root"
                } else {
                    // Every row that predates names is shared (ADR-0044 d12).
                    "workspaces_shared_name"
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
            sqlx::query("UPDATE workspaces SET crew_id = $3 WHERE tenant_id = $1 AND id = ANY($2)")
                .bind(tenant_id)
                .bind(&crew.workspace_ids)
                .bind(crew.crew_id)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
        if pointed.rows_affected() != u64::try_from(crew.workspace_ids.len()).map_err(backend)? {
            return Err(StoreError::Backend(format!(
                "crew {} names a workspace this tenant does not hold",
                crew.crew_id
            )));
        }
    }
    // Each path is its parent's with the node's name appended, the root's
    // its name alone (ADR-0026 d7).
    sqlx::query(
        "WITH RECURSIVE tree (id, path) AS ( \
             SELECT id, text2ltree(name) FROM workspaces \
              WHERE tenant_id = $1 AND parent_id IS NULL AND name IS NOT NULL \
             UNION ALL \
             SELECT w.id, tree.path || text2ltree(w.name) FROM workspaces w \
               JOIN tree ON w.parent_id = tree.id \
              WHERE w.tenant_id = $1 AND w.name IS NOT NULL \
         ) UPDATE workspaces w SET path = tree.path FROM tree \
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
        MetadataFold::AdminMemoryRead {
            investigation_id,
            key_id,
            persons,
        } => {
            let counted = sqlx::query(
                "UPDATE admin_investigations SET reads = reads + 1 \
                  WHERE tenant_id = $1 AND id = $2 AND key_id = $3 AND closed_at IS NULL",
            )
            .bind(tenant_id)
            .bind(investigation_id)
            .bind(key_id)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if counted.rows_affected() != 1 {
                return Err(StoreError::InvestigationNotOpen { investigation_id });
            }
            // One count per person whose own root the page read, for the
            // month of the read (ADR-0097 d16; 0074).
            let month = at.as_str().get(..7).unwrap_or_default().to_owned();
            for person in &persons {
                sqlx::query(
                    "INSERT INTO admin_read_counts (tenant_id, person, month, reads) \
                     VALUES ($1, $2, $3, 1) \
                     ON CONFLICT (tenant_id, person, month) \
                     DO UPDATE SET reads = admin_read_counts.reads + 1",
                )
                .bind(tenant_id)
                .bind(person.as_str())
                .bind(&month)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            }
        }
        MetadataFold::NoticeWritten {
            notice_id,
            kind,
            act_id,
            recipients,
            detail,
        } => {
            // One row per recipient (ADR-0097 d20; 0074).
            for recipient in &recipients {
                sqlx::query(
                    "INSERT INTO notices (tenant_id, id, recipient, kind, act_id, detail, \
                            created_at, seq) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7::timestamptz, $8)",
                )
                .bind(tenant_id)
                .bind(notice_id)
                .bind(recipient.as_str())
                .bind(kind.as_str())
                .bind(act_id)
                .bind(&detail)
                .bind(at.as_str())
                .bind(seq)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            }
        }
        MetadataFold::NoticeDismissed {
            notice_id,
            recipient,
        } => {
            let dismissed = sqlx::query(
                "UPDATE notices SET dismissed_at = $4::timestamptz \
                  WHERE tenant_id = $1 AND id = $2 AND recipient = $3 AND dismissed_at IS NULL",
            )
            .bind(tenant_id)
            .bind(notice_id)
            .bind(recipient.as_str())
            .bind(at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if dismissed.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "notice {notice_id} is not an undismissed notice of {}",
                    recipient.as_str()
                )));
            }
        }
        MetadataFold::AdminAct(act) => {
            fold_admin_act(tx, tenant_id, &act, seq, at).await?;
        }
        MetadataFold::AdminFixRecorded(fix) => {
            // The fix's row, unconfirmed, and its mark on the version it
            // made the served tip: used by a person, unconfirmed, never
            // naming which admin (ADR-0089 d3, d7; 0064).
            sqlx::query(
                "INSERT INTO admin_fixes (tenant_id, id, act, target_id, workspace_id, \
                        claim_key, replaced_id, served_id, reason, loosens_guardrail, \
                        admin_key_id, lifecycle_event_id, fixed_at, fixed_seq, admin_person) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, \
                        ($13)::timestamptz, $14, $15)",
            )
            .bind(tenant_id)
            .bind(fix.fix_id)
            .bind(fix.act.as_str())
            .bind(fix.target_id)
            .bind(fix.workspace_id)
            .bind(&fix.claim_key)
            .bind(fix.replaced_id)
            .bind(fix.served_id)
            .bind(&fix.reason)
            .bind(fix.loosens_guardrail)
            .bind(fix.admin_key_id)
            .bind(fix.lifecycle_event_id)
            .bind(at.as_str())
            .bind(seq)
            .bind(fix.admin_person.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if let Some(served) = fix.served_id {
                // An unconfirmed fix whose mark this one takes over leaves
                // the queue: settled `superseded` by this fix, at its seq,
                // since no person could settle it any more (ADR-0089 d17,
                // the reading of 2026-11-19).
                sqlx::query(
                    "UPDATE admin_fixes f SET state = 'superseded', superseded_by = $3, \
                            settled_at = ($4)::timestamptz, settled_seq = $5 \
                       FROM memories m \
                      WHERE m.tenant_id = $1 AND m.id = $2 AND m.admin_fix_id IS NOT NULL \
                        AND m.admin_fix_id <> $3 \
                        AND f.tenant_id = $1 AND f.id = m.admin_fix_id \
                        AND f.state = 'unconfirmed'",
                )
                .bind(tenant_id)
                .bind(served)
                .bind(fix.fix_id)
                .bind(at.as_str())
                .bind(seq)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
                let marked = sqlx::query(
                    "UPDATE memories SET admin_fix_id = $3, admin_fix_confirmed = false, \
                            admin_fix_same_user = false, admin_fix_reverted = false \
                      WHERE tenant_id = $1 AND id = $2",
                )
                .bind(tenant_id)
                .bind(served)
                .bind(fix.fix_id)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
                // One row, or the commit aborts by name (ADR-0089 d7).
                if marked.rows_affected() != 1 {
                    return Err(StoreError::FixVersionMissing { fix_id: fix.fix_id });
                }
            }
        }
        MetadataFold::AdminFixSettled {
            fix_id,
            state,
            settled_by_key_id,
            same_user,
            restored_id,
            served_id,
        } => {
            // Compare-and-set: the fix moves once, from unconfirmed
            // (ADR-0089 d8).
            let settled = sqlx::query(
                "UPDATE admin_fixes SET state = $3, settled_by_key_id = $4, \
                        settled_same_user = $5, settled_at = ($6)::timestamptz, \
                        settled_seq = $7, restored_id = $8 \
                  WHERE tenant_id = $1 AND id = $2 AND state = 'unconfirmed'",
            )
            .bind(tenant_id)
            .bind(fix_id)
            .bind(state.as_str())
            .bind(settled_by_key_id)
            .bind(same_user)
            .bind(at.as_str())
            .bind(seq)
            .bind(restored_id)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if settled.rows_affected() != 1 {
                return Err(StoreError::FixSettled { fix_id });
            }
            // The served version, by its primary key: confirmed and
            // same_user as judged, or marked reverted; one row, or the
            // commit aborts by name (ADR-0089 d7, d17).
            if let Some(served) = served_id {
                let marked = if state == oml_app::types::FixState::Confirmed {
                    sqlx::query(
                        "UPDATE memories SET admin_fix_confirmed = true, admin_fix_same_user = $4 \
                          WHERE tenant_id = $1 AND id = $2 AND admin_fix_id = $3",
                    )
                    .bind(tenant_id)
                    .bind(served)
                    .bind(fix_id)
                    .bind(same_user)
                } else {
                    sqlx::query(
                        "UPDATE memories SET admin_fix_reverted = true \
                          WHERE tenant_id = $1 AND id = $2 AND admin_fix_id = $3",
                    )
                    .bind(tenant_id)
                    .bind(served)
                    .bind(fix_id)
                }
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
                if marked.rows_affected() != 1 {
                    return Err(StoreError::FixVersionMissing { fix_id });
                }
            }
        }
        MetadataFold::InvestigationOpened {
            investigation_id,
            key_id,
            reason,
        } => {
            sqlx::query(
                "INSERT INTO admin_investigations (tenant_id, id, key_id, reason, opened_at, \
                        opened_seq) VALUES ($1, $2, $3, $4, ($5)::timestamptz, $6)",
            )
            .bind(tenant_id)
            .bind(investigation_id)
            .bind(key_id)
            .bind(reason)
            .bind(at.as_str())
            .bind(seq)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::InvestigationClosed {
            investigation_id,
            key_id,
        } => {
            let closed = sqlx::query(
                "UPDATE admin_investigations SET closed_at = ($4)::timestamptz, closed_seq = $5 \
                  WHERE tenant_id = $1 AND id = $2 AND key_id = $3 AND closed_at IS NULL",
            )
            .bind(tenant_id)
            .bind(investigation_id)
            .bind(key_id)
            .bind(at.as_str())
            .bind(seq)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if closed.rows_affected() != 1 {
                return Err(StoreError::InvestigationNotOpen { investigation_id });
            }
        }
        MetadataFold::WorkspaceImported(ended) => {
            // The row ends done in the event's transaction, and the ledger
            // moves to the bundle's; the pin its begin wrote never moves
            // (ADR-0084 d8, d11, R11, I12).
            let done = sqlx::query(
                "UPDATE imports SET state = 'done', ended_at = ($3)::timestamptz, counts = $4, \
                        refusals = $5 \
                  WHERE tenant_id = $1 AND id = $2 AND state = 'running'",
            )
            .bind(tenant_id)
            .bind(ended.import_id)
            .bind(at.as_str())
            .bind(json!(ended.counts))
            .bind(serde_json::to_value(&ended.refusals).map_err(backend)?)
            .execute(&mut **tx)
            .await
            .map_err(backend)?
            .rows_affected();
            if done != 1 {
                return Err(StoreError::Backend(format!(
                    "import {} is not running",
                    ended.import_id
                )));
            }
            sqlx::query(
                "INSERT INTO import_ledger (tenant_id, target_workspace_id, source_server_id, \
                        source_tenant_id, exported_at, release_label, release_manifest_digest, \
                        signer_fingerprint, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, ($9)::timestamptz) \
                 ON CONFLICT (tenant_id, target_workspace_id, source_server_id, source_tenant_id) \
                 DO UPDATE SET exported_at = GREATEST(import_ledger.exported_at, EXCLUDED.exported_at), \
                    release_label = EXCLUDED.release_label, \
                    release_manifest_digest = EXCLUDED.release_manifest_digest, \
                    updated_at = EXCLUDED.updated_at",
            )
            .bind(tenant_id)
            .bind(ended.target_workspace_id)
            .bind(&ended.source_server_id)
            .bind(&ended.source_tenant_id)
            .bind(&ended.ledger.exported_at)
            .bind(&ended.ledger.release_label)
            .bind(&ended.ledger.release_manifest_digest)
            .bind(&ended.ledger.signer_fingerprint)
            .bind(at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::WorkspaceExported { user_root, bytes } => {
            // The export's bytes on the tenant's day, every export's, and on
            // its account's day, in the export's own transaction (ADR-0084
            // d10, I19; ADR-0071 d2, d3).
            sqlx::query(
                "INSERT INTO tenant_activity (tenant_id, day, exports) \
                 VALUES ($1, ($2)::date, $3) ON CONFLICT (tenant_id, day) \
                 DO UPDATE SET exports = tenant_activity.exports + EXCLUDED.exports",
            )
            .bind(tenant_id)
            .bind(oml_app::domain::usage::day_of(at))
            .bind(i64::try_from(bytes).map_err(backend)?)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if let Some(root) = user_root {
                sqlx::query(
                    "INSERT INTO account_meters (tenant_id, user_root) VALUES ($1, $2) \
                     ON CONFLICT (tenant_id, user_root) DO NOTHING",
                )
                .bind(tenant_id)
                .bind(root)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
                sqlx::query(
                    "INSERT INTO account_activity (tenant_id, user_root, day, exports) \
                     VALUES ($1, $2, ($3)::date, $4) ON CONFLICT (tenant_id, user_root, day) \
                     DO UPDATE SET exports = account_activity.exports + EXCLUDED.exports",
                )
                .bind(tenant_id)
                .bind(root)
                .bind(oml_app::domain::usage::day_of(at))
                .bind(i64::try_from(bytes).map_err(backend)?)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            }
        }
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
        MetadataFold::WorkspaceFederationSet {
            workspace_id,
            accept_federated,
        } => {
            let updated = sqlx::query(
                "UPDATE workspaces SET accept_federated = $3 WHERE tenant_id = $1 AND id = $2",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(accept_federated)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "workspace {workspace_id} is not held by this tenant"
                )));
            }
        }
        MetadataFold::WorkspacePublicSet {
            workspace_id,
            public,
            act_id,
        } => {
            let updated = sqlx::query(
                "UPDATE workspaces SET public = $3, public_act_id = $4 \
                 WHERE tenant_id = $1 AND id = $2",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(public)
            .bind(act_id)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "workspace {workspace_id} is not held by this tenant"
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
        MetadataFold::WorkspaceCreated(w) => {
            // The status stored is the record's, the one column every
            // workspace read serves (ADR-0009 d1). The path is the parent's
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
                "INSERT INTO workspaces (id, tenant_id, parent_id, name, path, status, crew, \
                 crew_id, user_root, namespace_root_id) VALUES ($1, $2, $3, $4, \
                 CASE WHEN $3::uuid IS NULL THEN text2ltree($4) \
                      ELSE (SELECT p.path FROM workspaces p \
                             WHERE p.tenant_id = $2 AND p.id = $3) || text2ltree($4) END, \
                 $5, $6, $7, $8, $9)",
            )
            .bind(w.workspace_id)
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
                // (postgres-schema.md `workspaces`; ADR-0026 d5, d6). A name
                // is unique in its namespace, the shared one or its user
                // root's (ADR-0044 d5).
                let constraint = if w.parent_id.is_some() && w.name.as_str() == UNIVERSE {
                    "workspaces_universe_is_root"
                } else if w.namespace.is_some() {
                    "workspaces_user_name"
                } else {
                    "workspaces_shared_name"
                };
                name_taken_or_backend(e, constraint, NamedKind::Workspace, &w.name)
            })?;
        }
        MetadataFold::WebEmailAllowed(entry) => {
            // A pending entry records when it began to wait on its person
            // (ADR-0045 d2's note of 2026-11-19).
            sqlx::query(
                "INSERT INTO web_emails (tenant_id, email_hash, email, user_name, admin, \
                 created_at, added_by, terms_version, pending_since, admin_since) \
                 VALUES ($1, $2, $3, $4, $5, ($6)::timestamptz, $7, $8, \
                         CASE WHEN $9 THEN ($6)::timestamptz END, \
                         CASE WHEN $5 AND NOT $9 THEN ($6)::timestamptz END)",
            )
            .bind(tenant_id)
            .bind(entry.email.digest().0.to_vec())
            .bind(entry.email.as_str())
            .bind(entry.user.as_str())
            .bind(entry.admin)
            .bind(at.as_str())
            .bind(entry.added_by)
            .bind(entry.terms_version.as_deref())
            .bind(entry.pending)
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
            fold_sole_admin(tx, tenant_id).await?;
        }
        MetadataFold::WebEmailChanged {
            entry,
            keep_an_admin,
        } => {
            let hash = entry.email.digest().0.to_vec();
            // An entry the change leaves pending signs in to nothing, so
            // making the last admin entry pending is un-flagging it too
            // (ADR-0045 d10; d2's note of 2026-11-19).
            if keep_an_admin
                && (!entry.admin || entry.pending)
                && last_admin_entry(tx, tenant_id, &hash).await?
            {
                return Err(StoreError::LastAdminEntry);
            }
            end_entry_sessions(tx, tenant_id, &hash).await?;
            // A change that leaves the entry pending keeps the time it began
            // to wait; one that makes it pending starts it now.
            let changed = sqlx::query(
                "UPDATE web_emails SET user_name = $3, admin = $4, added_by = $7, \
                 pending_since = CASE WHEN $5 THEN coalesce(pending_since, ($6)::timestamptz) END, \
                 admin_since = CASE WHEN $4 AND NOT $5 THEN \
                     CASE WHEN admin AND pending_since IS NULL AND user_name = $3 \
                          THEN coalesce(admin_since, ($6)::timestamptz) \
                          ELSE ($6)::timestamptz END END \
                 WHERE tenant_id = $1 AND email_hash = $2",
            )
            .bind(tenant_id)
            .bind(&hash)
            .bind(entry.user.as_str())
            .bind(entry.admin)
            .bind(entry.pending)
            .bind(at.as_str())
            .bind(entry.added_by)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if changed.rows_affected() != 1 {
                return Err(StoreError::Backend(
                    "web.email_changed names an entry the tenant does not hold".to_owned(),
                ));
            }
            fold_sole_admin(tx, tenant_id).await?;
        }
        MetadataFold::WebEmailAccepted {
            email_hash,
            user,
            admin,
        } => {
            // The pending entry naming this user, with the admin flag the
            // person reviewed, signs in from this commit on; anything else,
            // a flag flipped since included, aborts it (ADR-0045 d2's note,
            // the operator's decisions of 2026-11-19 and 2026-11-20).
            let accepted = sqlx::query(
                "UPDATE web_emails SET pending_since = NULL, \
                 admin_since = CASE WHEN admin THEN ($5)::timestamptz END \
                 WHERE tenant_id = $1 AND email_hash = $2 AND user_name = $3 \
                 AND admin = $4 AND pending_since IS NOT NULL",
            )
            .bind(tenant_id)
            .bind(email_hash.0.to_vec())
            .bind(user.as_str())
            .bind(admin)
            .bind(at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if accepted.rows_affected() != 1 {
                return Err(StoreError::Backend(
                    "web.email_accepted names no pending entry of this user with that admin flag"
                        .to_owned(),
                ));
            }
            fold_sole_admin(tx, tenant_id).await?;
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
            // The last live admin person is never disabled, on any surface
            // (ADR-0097 d25): the tenant's live admin entries, locked.
            let live: Vec<String> = sqlx::query_scalar(
                "SELECT user_name::text FROM web_emails WHERE tenant_id = $1 AND admin \
                 AND pending_since IS NULL AND disabled_at IS NULL ORDER BY email_hash FOR UPDATE",
            )
            .bind(tenant_id)
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?;
            if live.iter().any(|u| u == user.as_str()) && live.iter().all(|u| u == user.as_str()) {
                return Err(StoreError::LastAdminEntry);
            }
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
        MetadataFold::UserEnabled { user } => {
            sqlx::query(
                "UPDATE web_emails SET disabled_at = NULL WHERE tenant_id = $1 AND user_name = $2",
            )
            .bind(tenant_id)
            .bind(user.as_str())
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
        // crew (0006_names.sql `workspaces_crew`).
        MetadataFold::WorkspaceCrewUpdated {
            workspace_id,
            crew_id,
        } => {
            let updated =
                sqlx::query("UPDATE workspaces SET crew_id = $3 WHERE tenant_id = $1 AND id = $2")
                    .bind(tenant_id)
                    .bind(workspace_id)
                    .bind(crew_id)
                    .execute(&mut **tx)
                    .await
                    .map_err(backend)?;
            if updated.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "workspace {workspace_id} is not held by this tenant"
                )));
            }
        }
        MetadataFold::WorkspaceModeSet {
            workspace_id,
            in_progress,
            held_free,
        } => {
            // The commit's zero-held test, in the committing transaction
            // under the chain lock (ADR-0030 d6).
            if !held_free.is_empty() {
                let held: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM memories WHERE tenant_id = $1 \
                     AND workspace_id = ANY($2::uuid[]) AND status = 'held')",
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
                sqlx::query("UPDATE workspaces SET mode = $3 WHERE tenant_id = $1 AND id = $2")
                    .bind(tenant_id)
                    .bind(workspace_id)
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
                    "workspace {workspace_id} is not held by this tenant"
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
            // A connector key has no hash, which
            // api_keys_connector_has_no_hash holds (0012; ADR-0031 d7). A
            // key with no limit names neither limit column, so a schema
            // before 0046 still takes it, and an admin key with one is
            // refused by api_keys_admin_key_has_no_limits (0046; ADR-0079
            // d1, d4).
            let limited = k.expiry.is_some() || k.requests_per_minute.is_some();
            let insert = if limited {
                "INSERT INTO api_keys (id, tenant_id, hash, credential, role, principal, \
                 used_by, default_workspace_id, issued_by, issued_seq, name, crew_id, \
                 user_root_id, session_for, guardrail_scope, profile, expires_at, \
                 requests_per_minute) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
                 ($17)::timestamptz, $18)"
            } else {
                "INSERT INTO api_keys (id, tenant_id, hash, credential, role, principal, \
                 used_by, default_workspace_id, issued_by, issued_seq, name, crew_id, \
                 user_root_id, session_for, guardrail_scope, profile) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)"
            };
            let mut insert = sqlx::query(insert)
                .bind(k.key_id)
                .bind(k.tenant_id)
                .bind(hash)
                .bind(k.credential.as_str())
                .bind(k.role.as_str())
                .bind(&k.principal)
                .bind(k.used_by.as_str())
                .bind(k.default_workspace)
                .bind(k.issued_by)
                .bind(seq)
                .bind(k.name.as_ref().map(Name::as_str))
                .bind(k.crew_id)
                .bind(k.user)
                .bind(k.session_user.as_ref().map(Name::as_str))
                .bind(k.guardrail_scope.as_str())
                .bind(k.profile.map(KeyProfile::as_str));
            if limited {
                insert = insert
                    .bind(k.expiry.as_ref().map(EventTime::as_str))
                    .bind(k.requests_per_minute.map(i64::from));
            }
            insert.execute(&mut **tx).await.map_err(|e| match &k.name {
                Some(name) => {
                    name_taken_or_backend(e, "api_keys_tenant_id_name", NamedKind::Key, name)
                }
                None => backend(e),
            })?;
            // A key that waits on a widening act names it, so it
            // authenticates nothing until a second admin person confirms
            // it (ADR-0097 d23; 0070). Its own statement, so a schema
            // before 0070 still takes every other key.
            if let Some(act_id) = k.pending_act {
                sqlx::query(
                    "UPDATE api_keys SET pending_act_id = $3 WHERE tenant_id = $1 AND id = $2",
                )
                .bind(k.tenant_id)
                .bind(k.key_id)
                .bind(act_id)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            }
        }
        // The schema's role CHECK and its foreign keys refuse a grant or
        // denial on a key or node the tenant does not hold; the error aborts
        // the commit. The keys name the tenant with the id, (tenant_id,
        // key_id) and (tenant_id, workspace_node_id), because PostgreSQL runs
        // a foreign-key check without row-level security: a reference on id
        // alone would accept another tenant's key or node (0004_grants.sql).
        MetadataFold::GrantIssued { key_id, grant } => {
            sqlx::query(
                "INSERT INTO grants (id, tenant_id, key_id, role, workspace_node_id, deny, epoch, \
                 covers, may_create) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(grant.grant_id)
            .bind(tenant_id)
            .bind(key_id)
            .bind(grant.role.as_str())
            .bind(grant.node)
            .bind(deny_to_json(&grant.deny))
            .bind(seq)
            .bind(grant.covers.as_str())
            .bind(grant.create)
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
                "INSERT INTO key_denials (id, tenant_id, key_id, workspace_node_id, epoch, \
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
        MetadataFold::GrantRevoked { key_id, grant_id } => {
            // The row stays as history, its revoked_epoch this commit's seq;
            // the guard matches only a grant in force on this key of this
            // tenant (0073_admin_act_undo.sql `grants.revoked_epoch`).
            let revoked = sqlx::query(
                "UPDATE grants SET revoked_epoch = $4 \
                 WHERE tenant_id = $1 AND id = $2 AND key_id = $3 AND revoked_epoch IS NULL",
            )
            .bind(tenant_id)
            .bind(grant_id)
            .bind(key_id)
            .bind(seq)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if revoked.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "grant {grant_id} is not a grant in force on key {key_id} of this tenant"
                )));
            }
        }
        MetadataFold::ConnectCodeVoided { key_id, code_id } => {
            let voided = sqlx::query(
                "UPDATE connect_codes SET voided_seq = $4 \
                 WHERE tenant_id = $1 AND id = $2 AND key_id = $3 \
                   AND redeemed_at IS NULL AND voided_seq IS NULL",
            )
            .bind(tenant_id)
            .bind(code_id)
            .bind(key_id)
            .bind(seq)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if voided.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "code {code_id} is not an unredeemed live code of key {key_id}"
                )));
            }
        }
        MetadataFold::KeySuspended { key_id, suspension } => {
            // Only a live key that is not suspended already is suspended
            // (0073_admin_act_undo.sql); anything else aborts the commit.
            let suspended = sqlx::query(
                "UPDATE api_keys SET suspended_cause = $3, suspended_act_id = $4, \
                        suspended_at = ($5)::timestamptz \
                  WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL \
                    AND suspended_cause IS NULL",
            )
            .bind(tenant_id)
            .bind(key_id)
            .bind(suspension.cause.as_str())
            .bind(suspension.act_id)
            .bind(suspension.at.as_str())
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if suspended.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "key {key_id} is not a live key of this tenant that is not suspended"
                )));
            }
        }
        MetadataFold::KeySuspensionLifted { key_id } => {
            let lifted = sqlx::query(
                "UPDATE api_keys SET suspended_cause = NULL, suspended_act_id = NULL, \
                        suspended_at = NULL \
                  WHERE tenant_id = $1 AND id = $2 AND suspended_cause IS NOT NULL",
            )
            .bind(tenant_id)
            .bind(key_id)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if lifted.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "key {key_id} is not a suspended key of this tenant"
                )));
            }
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
        MetadataFold::KeyRotated {
            key_id,
            key_hash,
            previous,
        } => {
            // The new hash on the live bearer key's row; the guard aborts
            // the commit for anything else. The previous hash, with its
            // grace's end, replaces the key's earlier one, or a rotation
            // without grace deletes it (ADR-0096 d13; 0069).
            let hash = key_hash_bytes(&key_hash).ok_or_else(|| {
                StoreError::Backend("a rotated key's hash is not sha256".to_owned())
            })?;
            let set = sqlx::query(
                "UPDATE api_keys SET hash = $3 \
                 WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL \
                 AND credential = 'bearer'",
            )
            .bind(tenant_id)
            .bind(key_id)
            .bind(&hash)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if set.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "key {key_id} is not a live bearer key of this tenant"
                )));
            }
            sqlx::query("DELETE FROM key_previous_hashes WHERE tenant_id = $1 AND key_id = $2")
                .bind(tenant_id)
                .bind(key_id)
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            if let Some((old, until)) = previous {
                let old = key_hash_bytes(&old).ok_or_else(|| {
                    StoreError::Backend("a rotated key's previous hash is not sha256".to_owned())
                })?;
                sqlx::query(
                    "INSERT INTO key_previous_hashes (tenant_id, key_id, hash, valid_until) \
                     VALUES ($1, $2, $3, ($4)::timestamptz)",
                )
                .bind(tenant_id)
                .bind(key_id)
                .bind(&old)
                .bind(until.as_str())
                .execute(&mut **tx)
                .await
                .map_err(backend)?;
            }
        }
        MetadataFold::KeyLimitsSet {
            key_id,
            expiry,
            requests_per_minute,
        } => {
            // Both limits take the event's values. The guard matches only a
            // live bearer or connector key of this tenant that is not an
            // admin key, so a change of anything else aborts the commit
            // rather than journalling a change of nothing (ADR-0079 d5).
            let set = sqlx::query(
                "UPDATE api_keys SET expires_at = ($3)::timestamptz, requests_per_minute = $4 \
                 WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL \
                 AND credential <> 'session' AND role <> 'admin'",
            )
            .bind(tenant_id)
            .bind(key_id)
            .bind(expiry.as_ref().map(EventTime::as_str))
            .bind(requests_per_minute.map(i64::from))
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if set.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "key {key_id} is not a live bearer or connector key of this tenant, or is an admin key"
                )));
            }
        }
        MetadataFold::KeyDefaultSet {
            key_id,
            default_workspace,
        } => {
            // The guard matches only a live key of this tenant and a node
            // it holds, so a default on anything else aborts the commit
            // (ADR-0097 d30, the operator's decision of 2026-11-27).
            let set = sqlx::query(
                "UPDATE api_keys SET default_workspace_id = $3 \
                 WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL \
                 AND EXISTS (SELECT 1 FROM workspaces WHERE tenant_id = $1 AND id = $3)",
            )
            .bind(tenant_id)
            .bind(key_id)
            .bind(default_workspace)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if set.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "key {key_id} is not a live key of this tenant, or {default_workspace} is not its node"
                )));
            }
        }
        MetadataFold::KeyLinked {
            key_id,
            user_root,
            accepted,
        } => {
            // A link's guard matches only a live tenant bearer key used by
            // a person, an admin key's included (0064), that holds no link
            // and none pending, and a root that is a user root of this
            // tenant; an accepted link (the bootstrap's) sets the link, a
            // console admin's sets it pending (0064). An unlink's matches
            // any key of the tenant with a link or a pending one, a revoked
            // one included. Anything else aborts the commit (ADR-0085 d16,
            // the operator's decision of 2026-11-19).
            let sql = if accepted {
                "UPDATE api_keys SET linked_user_id = $3 \
                 WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL \
                 AND credential = 'bearer' AND used_by = 'person' \
                 AND user_root_id IS NULL AND linked_user_id IS NULL \
                 AND link_pending_user_id IS NULL \
                 AND EXISTS (SELECT 1 FROM workspaces w \
                      WHERE w.tenant_id = $1 AND w.id = $3 AND w.user_root)"
            } else {
                "UPDATE api_keys SET link_pending_user_id = $3 \
                 WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL \
                 AND credential = 'bearer' AND used_by = 'person' \
                 AND user_root_id IS NULL AND linked_user_id IS NULL \
                 AND link_pending_user_id IS NULL \
                 AND EXISTS (SELECT 1 FROM workspaces w \
                      WHERE w.tenant_id = $1 AND w.id = $3 AND w.user_root)"
            };
            let set = match user_root {
                Some(root) => sqlx::query(sql).bind(tenant_id).bind(key_id).bind(root),
                None => sqlx::query(
                    "UPDATE api_keys SET linked_user_id = NULL, link_pending_user_id = NULL \
                     WHERE tenant_id = $1 AND id = $2 \
                     AND (linked_user_id IS NOT NULL OR link_pending_user_id IS NOT NULL)",
                )
                .bind(tenant_id)
                .bind(key_id),
            }
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if set.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "key {key_id} cannot take this link: not a live unlinked tenant key used by a person, the root is not a user root, or there is no link to end"
                )));
            }
        }
        MetadataFold::KeyLinkAccepted { key_id, user_root } => {
            // The pending link to this root becomes the link, on a live key
            // only; anything else aborts the commit (ADR-0085 d16, the
            // operator's decision of 2026-11-19).
            let set = sqlx::query(
                "UPDATE api_keys SET linked_user_id = link_pending_user_id, \
                        link_pending_user_id = NULL \
                 WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL \
                 AND link_pending_user_id = $3 AND linked_user_id IS NULL",
            )
            .bind(tenant_id)
            .bind(key_id)
            .bind(user_root)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if set.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "key {key_id} has no live link pending to this root"
                )));
            }
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
            imported,
        } => {
            insert_memory(tx, event_id, &memory).await?;
            if let Some(imported) = imported {
                // An import's write carries its provenance in its own
                // commit (ADR-0084 I14).
                record_imported_version(tx, tenant_id, memory.memory_id, &imported).await?;
            }
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
        MetadataFold::MemoryWithdrawn { memory_id } => {
            set_memory_status(tx, tenant_id, memory_id, MemoryStatus::Withdrawn).await?;
        }
        MetadataFold::MemoryDemoted {
            memory_id,
            restored,
            reopened,
        } => {
            demote_memory(tx, tenant_id, memory_id, restored, reopened.as_ref()).await?;
        }
        MetadataFold::ClaimErased { memory_ids } => {
            erase_claim(tx, tenant_id, &memory_ids).await?;
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
                "INSERT INTO workspace_transitions (id, tenant_id, workspace_id, from_status, \
                 to_status, state, proposer_key_id, proposer_principal, ratifier_key_id, \
                 proposed_seq) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(p.transition_id)
            .bind(tenant_id)
            .bind(p.workspace_id)
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
            workspace_id,
            to,
            ratifier_key_id,
        } => {
            ratify_transition(
                tx,
                tenant_id,
                seq,
                transition_id,
                workspace_id,
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

/// `workspace.status_transition`'s fold, under the chain lock the commit
/// already holds. The node row and the proposal row are locked and read
/// first, so nothing moves either between the re-checks and the writes
/// (workspaces.md, ratification re-checks). A row that is not a proposal on
/// this node with this `to`, and a ratifier key the tenant does not hold,
/// are defects; a row already ratified or whose `from` is no longer the
/// node's status is [`StoreError::TransitionStale`]; a ratifier that is the
/// proposing key or holds the proposer's principal, read from its key row
/// here, is [`StoreError::RatifierIsProposer`]; and a step to `frozen`
/// that finds a direct child not `frozen` is
/// [`StoreError::ChildrenNotFrozen`]. Any refusal refuses the commit,
/// which writes nothing. The schema still refuses a ratifier that is the
/// proposing key and a universe root moved off `protected` (ADR-0009 d1,
/// d3, d4; ADR-0010 R2).
async fn ratify_transition(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    seq: i64,
    transition_id: Uuid,
    workspace_id: Uuid,
    to: WorkspaceStatus,
    ratifier_key_id: Uuid,
) -> Result<(), StoreError> {
    let defect = || {
        StoreError::Backend(format!(
            "transition {transition_id} is not a proposal on workspace {workspace_id} to {}",
            to.as_str()
        ))
    };
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM workspaces WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    let Some(current) = status else {
        return Err(defect());
    };
    let row: Option<(Uuid, String, String, String, Uuid, String)> = sqlx::query_as(
        "SELECT workspace_id, from_status, to_status, state, proposer_key_id, \
         proposer_principal FROM workspace_transitions \
         WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(transition_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    let Some((row_workspace, from, row_to, state, proposer_key_id, proposer_principal)) = row
    else {
        return Err(defect());
    };
    if row_workspace != workspace_id || row_to != to.as_str() {
        return Err(defect());
    }
    if state != TransitionState::Pending.as_str() || from != current {
        let from = WorkspaceStatus::parse(&from).ok_or_else(|| {
            StoreError::Backend(format!("transition {transition_id} has from {from:?}"))
        })?;
        return Err(StoreError::TransitionStale {
            transition_id,
            workspace_id,
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
    if to == WorkspaceStatus::Frozen {
        // Under the chain lock every child create and reparent waits for
        // this commit, so the read sees the subtree as it is (ADR-0009 d4).
        let short: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM workspaces WHERE tenant_id = $1 AND parent_id = $2 \
             AND status <> 'frozen' LIMIT 1",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(backend)?;
        if short.is_some() {
            return Err(StoreError::ChildrenNotFrozen {
                transition_id,
                workspace_id,
            });
        }
    }
    sqlx::query(
        "UPDATE workspace_transitions SET state = 'ratified', ratifier_key_id = $3, \
         ratified_seq = $4 WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(transition_id)
    .bind(ratifier_key_id)
    .bind(seq)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    sqlx::query("UPDATE workspaces SET status = $3 WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id)
        .bind(workspace_id)
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
/// seven counts.
type CounterRow = (Uuid, Option<String>, i64, i64, i64, i64, i64, i64, i64);
/// A ledger row as read: root, day, profile and seven counts.
type LedgerTuple = (
    Uuid,
    String,
    Option<String>,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
);
/// A drift row: root, then each compared meter's counter and source value.
type DriftRow = (
    Uuid,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
    i64,
);
/// Seven counts in [`Meter::ALL`]'s order.
type Counts = (i64, i64, i64, i64, i64, i64, i64);

/// Seven counts in [`Meter::ALL`]'s order as a usage; a negative count,
/// which only a drifted counter holds, reads as zero.
fn usage_of(
    (memories, text_bytes, writes, recalls, keys, connections, workspaces): Counts,
) -> oml_app::types::UserUsage {
    let n = |v: i64| u64::try_from(v).unwrap_or(0);
    oml_app::types::UserUsage {
        memories: n(memories),
        text_bytes: n(text_bytes),
        writes: n(writes),
        recalls: n(recalls),
        keys: n(keys),
        connections: n(connections),
        workspaces: n(workspaces),
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
    workspace_id: Uuid,
    claim_key: String,
    /// The chain's account; `None` for a chain whose first version the
    /// fold itself may store, so its account is read once the fold has,
    /// and only when its liveness changed.
    account: Option<Uuid>,
    live_before: bool,
}

/// The account a key belongs to, if any: its user root, or for a person's
/// `<user>-review` session key its person's; `<user>-curate` and
/// `<user>-admin` belong to none (ADR-0071 d1, the operator's decision of
/// 2026-11-19; migration 0065).
async fn author_account(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    key_id: Uuid,
) -> Result<Option<Uuid>, StoreError> {
    // `oml_key_account` written out, so a store reads it on a schema
    // before 0065 too.
    let root: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT CASE \
                  WHEN k.user_root_id IS NULL AND k.credential = 'session' \
                       AND k.name = k.session_for || '-review' \
                  THEN (SELECT w.id FROM workspaces w \
                         WHERE w.tenant_id = k.tenant_id AND w.user_root \
                           AND w.name = k.session_for) \
                  ELSE k.user_root_id \
                END \
           FROM api_keys k WHERE k.tenant_id = $1 AND k.id = $2",
    )
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
    workspace_id: Uuid,
    claim_key: &str,
) -> Result<bool, StoreError> {
    sqlx::query_scalar("SELECT oml_chain_live($1, $2, $3)")
        .bind(tenant_id)
        .bind(workspace_id)
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
    workspace_id: Uuid,
    claim_key: &str,
) -> Result<Option<TrackedChain>, StoreError> {
    let (account, live): (Option<Uuid>, bool) =
        sqlx::query_as("SELECT oml_chain_account($1, $2, $3), oml_chain_live($1, $2, $3)")
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(claim_key)
            .fetch_one(&mut **tx)
            .await
            .map_err(backend)?;
    Ok(account.map(|account| TrackedChain {
        workspace_id,
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
        "SELECT m.workspace_id, m.claim_key, \
                oml_chain_account(m.tenant_id, m.workspace_id, m.claim_key), \
                oml_chain_live(m.tenant_id, m.workspace_id, m.claim_key) \
           FROM memories m WHERE m.tenant_id = $1 AND m.id = $2",
    )
    .bind(tenant_id)
    .bind(memory_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(row.and_then(|(workspace_id, claim_key, account, live)| {
        account.map(|account| TrackedChain {
            workspace_id,
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
                        tracked_chain(tx, tenant_id, memory.workspace_id, &memory.claim_key).await?;
                }
                // Nothing was read for the store: the chain may be this
                // version's to start, so its account is read after the
                // fold, when its liveness changed.
                None => {
                    plan.chain = Some(TrackedChain {
                        workspace_id: memory.workspace_id,
                        claim_key: memory.claim_key.clone(),
                        account: None,
                        live_before: chain_live(
                            tx,
                            tenant_id,
                            memory.workspace_id,
                            &memory.claim_key,
                        )
                        .await?,
                    });
                }
            }
        }
        MetadataFold::MemoryPromoted { memory_id, .. }
        | MetadataFold::MemoryRejected { memory_id }
        | MetadataFold::MemoryWithdrawn { memory_id }
        | MetadataFold::MemoryDemoted { memory_id, .. } => {
            plan.chain = tracked_chain_of(tx, tenant_id, *memory_id).await?;
        }
        // An erasure takes the whole chain: it leaves the memories meter
        // when no version of it is live any more (ADR-0071 d2).
        MetadataFold::ClaimErased { memory_ids } => {
            if let Some(first) = memory_ids.first() {
                plan.chain = tracked_chain_of(tx, tenant_id, *first).await?;
            }
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
        // A node below a user root counts on that root's `workspaces`
        // meter, whoever created it; the root itself names no namespace,
        // so it counts on none (ADR-0095 d4).
        MetadataFold::WorkspaceCreated(w) => {
            if let Some(root) = w.namespace {
                plan.charges.push((root, Meter::Workspaces, 1));
            }
        }
        // No meter counts these. The match names each, so a fold added
        // later, a demotion or an erasure among them, does not compile
        // until it says what it moves (ADR-0071 d2).
        MetadataFold::TenantGenesis { .. }
        | MetadataFold::CrewCreated(_)
        | MetadataFold::CrewUpdated(_)
        | MetadataFold::WorkspaceCrewUpdated { .. }
        | MetadataFold::WorkspaceModeSet { .. }
        | MetadataFold::NamesAssigned(_)
        | MetadataFold::GrantIssued { .. }
        | MetadataFold::GrantRevoked { .. }
        | MetadataFold::KeySuspended { .. }
        | MetadataFold::KeySuspensionLifted { .. }
        | MetadataFold::KeyDenied { .. }
        | MetadataFold::KeyDenialLifted { .. }
        // An expired key stays on its account's keys meter: its date can
        // be moved later (ADR-0079 d3).
        | MetadataFold::KeyLimitsSet { .. }
        | MetadataFold::KeyDefaultSet { .. }
        | MetadataFold::KeyRotated { .. }
        | MetadataFold::KeyLinked { .. }
        | MetadataFold::KeyLinkAccepted { .. }
        | MetadataFold::ConnectCodeIssued { .. }
        | MetadataFold::ConnectCodeVoided { .. }
        | MetadataFold::VerdictRecorded { .. }
        | MetadataFold::SourceChecked { .. }
        | MetadataFold::FindingsRecorded { .. }
        | MetadataFold::WebEmailAllowed(_)
        | MetadataFold::WebEmailChanged { .. }
        | MetadataFold::WebEmailAccepted { .. }
        | MetadataFold::WebEmailRemoved { .. }
        | MetadataFold::AdminRead
        | MetadataFold::AdminMemoryRead { .. }
        | MetadataFold::NoticeWritten { .. }
        | MetadataFold::NoticeDismissed { .. }
        | MetadataFold::InvestigationOpened { .. }
        | MetadataFold::InvestigationClosed { .. }
        | MetadataFold::AdminFixRecorded(_)
        | MetadataFold::AdminFixSettled { .. }
        | MetadataFold::AdminAct(_)
        // An import's end moves no meter: each version it wrote was
        // metered by its own commit (ADR-0084 d15).
        | MetadataFold::WorkspaceImported(_)
        | MetadataFold::QuotaSet { .. }
        | MetadataFold::AccountProfileSet { .. }
        | MetadataFold::UserDisabled { .. }
        | MetadataFold::UserEnabled { .. }
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
        | MetadataFold::WorkspaceFederationSet { .. }
        | MetadataFold::WorkspacePublicSet { .. } => {}
        // The `exports` meter is a flow the fold counts itself, held under
        // no quota (ADR-0084 d10, d15).
        MetadataFold::WorkspaceExported { .. } => {}
    }
    Ok(plan)
}

/// One account's deltas, in the order the ceiling is held:
/// memories, text bytes, writes, keys, connections, workspaces.
type Deltas = [i64; 6];

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
        let after = chain_live(tx, tenant_id, chain.workspace_id, &chain.claim_key).await?;
        if after != chain.live_before {
            let account = match chain.account {
                Some(account) => Some(account),
                None => sqlx::query_scalar("SELECT oml_chain_account($1, $2, $3)")
                    .bind(tenant_id)
                    .bind(chain.workspace_id)
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
            Meter::Workspaces => 5,
            Meter::Recalls => unreachable!("no fold counts a recall"),
        };
        accounts.entry(root).or_default()[slot] += delta;
    }
    for (root, deltas) in accounts {
        let [memories, text_bytes, writes, keys, connections, workspaces] = deltas;
        let mut used: Deltas = [0; 6];
        if [memories, text_bytes, keys, connections, workspaces] != [0; 5] {
            let (m, b, k, c, w): (i64, i64, i64, i64, i64) = sqlx::query_as(
                "INSERT INTO account_meters \
                        (tenant_id, user_root, memories, text_bytes, keys, connections, \
                         workspaces) \
                 VALUES ($1, $2, greatest($3::bigint, 0), greatest($4::bigint, 0), \
                         greatest($5::bigint, 0), greatest($6::bigint, 0), \
                         greatest($7::bigint, 0)) \
                 ON CONFLICT (tenant_id, user_root) DO UPDATE SET \
                     memories = greatest(account_meters.memories + $3::bigint, 0), \
                     text_bytes = greatest(account_meters.text_bytes + $4::bigint, 0), \
                     keys = greatest(account_meters.keys + $5::bigint, 0), \
                     connections = greatest(account_meters.connections + $6::bigint, 0), \
                     workspaces = greatest(account_meters.workspaces + $7::bigint, 0) \
                 RETURNING memories, text_bytes, keys, connections, workspaces",
            )
            .bind(tenant_id)
            .bind(root)
            .bind(memories)
            .bind(text_bytes)
            .bind(keys)
            .bind(connections)
            .bind(workspaces)
            .fetch_one(&mut **tx)
            .await
            .map_err(backend)?;
            (used[0], used[1], used[3], used[4], used[5]) = (m, b, k, c, w);
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
                (Quota::Workspaces, workspaces, used[5]),
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
                                      writes, recalls, keys, connections, exports, workspaces, \
                                      recorded_at) \
             SELECT m.tenant_id, m.user_root, d.day::date, coalesce(p.profile, $5), \
                    m.memories, m.text_bytes, \
                    coalesce(a.writes, 0), coalesce(a.recalls, 0), m.keys, m.connections, \
                    coalesce(a.exports, 0), m.workspaces, ($3)::timestamptz \
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
        for table in ["account_activity", "tenant_activity"] {
            sqlx::query(&format!(
                "DELETE FROM {table} WHERE tenant_id = $1 AND day < ($2)::date - $3::int"
            ))
            .bind(tenant_id)
            .bind(today)
            .bind(i32::try_from(oml_app::domain::usage::ACTIVITY_RETENTION_DAYS).map_err(backend)?)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(appended)
    }

    /// `keyword-all`: the v0.5.0 statements, [`recall_fts_sql`] and
    /// [`recall_own_held_sql`], one candidate per version, their lists merged under the total
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
        let fts = recall_fts_sql().replace("/*anchor*/", &anchor.row("m", 6));
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
            let held = recall_own_held_sql().replace("/*anchor*/", &anchor.row("m", 7));
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
        let instructions = scope.instruction_candidates();
        if !instructions.is_empty() {
            let offered = recall_instructions_sql().replace("/*anchor*/", &anchor.row("m", 5));
            rows.extend(
                anchor
                    .bind(
                        sqlx::query(&offered)
                            .bind(tenant_id)
                            .bind(&instructions)
                            .bind(query.query)
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
        let mut candidates = oml_app::search::one_unit_per_version(candidates);
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
    /// The kinds a row is a candidate of by its node ([`hit_kinds`]).
    kinds: &'a [&'static str],
    /// The kinds BM25's statistics count by node, fixed before `kinds`
    /// (ADR-0092 d4).
    counted: &'a [&'static str],
    /// The held rows' author keys and every offered instruction: the `$4`
    /// of a bm25 statement, whose statistics count them all (ADR-0092 d4).
    counted_ids: &'a [Uuid],
    /// Every offered instruction, and those that are candidates
    /// (ADR-0092 d3).
    instructions: &'a [Uuid],
    candidate_instructions: &'a [Uuid],
    /// A bm25 statement's candidates narrowed to the asked kinds, a
    /// predicate on `m` of literal kind names; empty when it narrows
    /// nothing (ADR-0092 d4).
    narrowing: &'a str,
    /// The held rows' author keys ([`held_authors`]): on each node inside
    /// an in-progress subtree it names the server's rule-set mark there
    /// too, every author's unflagged held rows scanned under it on that
    /// node (ADR-0030 d3; ADR-0065 d15; ADR-0080 d16); empty, no held row.
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

    /// Every candidate's kind, read by the primary key in the list's own
    /// snapshot, which `search.matched_by_kind` counts (ADR-0092 d4).
    async fn fill_kinds(
        tx: &mut Transaction<'_, Postgres>,
        tenant_id: Uuid,
        mut candidates: Vec<Candidate>,
    ) -> Result<Vec<Candidate>, StoreError> {
        if candidates.is_empty() {
            return Ok(candidates);
        }
        let ids: Vec<Uuid> = candidates.iter().map(|c| c.memory_id).collect();
        let kinds: BTreeMap<Uuid, String> = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, kind FROM memories WHERE tenant_id = $1 AND id = ANY($2::uuid[])",
        )
        .bind(tenant_id)
        .bind(&ids)
        .fetch_all(&mut **tx)
        .await
        .map_err(backend)?
        .into_iter()
        .collect();
        // The list and this read share the transaction's snapshot, so a
        // candidate with no row is a store fault, refused by name rather
        // than counted as a memory.
        for c in &mut candidates {
            let kind = kinds.get(&c.memory_id).ok_or_else(|| {
                StoreError::Backend(format!(
                    "candidate {} has no row in its own list's snapshot",
                    c.memory_id
                ))
            })?;
            c.kind = MemoryKind::parse(kind).ok_or_else(|| {
                StoreError::Backend(format!("a stored row names an unknown kind {kind:?}"))
            })?;
        }
        Ok(candidates)
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
            None => bm25_sql_within(&format!("{}{}", scope.anchor.row("m", 10), scope.narrowing)),
            Some(_) => bm25_reference_sql_within(&format!(
                "{}{}",
                scope.anchor.row("m", 11),
                scope.narrowing
            )),
        };
        let mut statement = sqlx::query(&sql)
            .bind(scope.tenant_id)
            .bind(scope.nodes)
            .bind(scope.counted)
            .bind(scope.counted_ids)
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
            .filter_map(|k| memory_kind_code(k))
            .collect();
        let counted_kinds: Vec<i16> = scope
            .counted
            .iter()
            .filter_map(|k| memory_kind_code(k))
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
        let counted: Vec<(Option<i64>, i64, i64, i64, i64)> = sqlx::query_as(POSTINGS_COUNTS)
            .bind(scope.tenant_id)
            .bind(counted_nodes)
            .bind(&counted_kinds)
            .bind(&tokens)
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?;
        // N and every df count versions; the mean length is the units'
        // (ADR-0081 d4; the review's reading R6 amended).
        let mut rows = 0_u64;
        let mut units = 0_u64;
        let mut total_len = 0_u64;
        let mut documents = 0_i64;
        let mut df: BTreeMap<i64, u64> = tokens.iter().map(|t| (*t, 0)).collect();
        for (token, n, len, unit_count, document_count) in counted {
            let n = u64::try_from(n).map_err(backend)?;
            match token {
                None => {
                    rows = n;
                    total_len = u64::try_from(len).map_err(backend)?;
                    units = u64::try_from(unit_count).map_err(backend)?;
                    documents = document_count;
                }
                Some(token) => *df.entry(token).or_default() += n,
            }
        }
        // An expired version counts in no statistic, as in bm25's scan: the
        // per-workspace counts hold every served version, so the expired ones
        // on the readable nodes are taken out by id (proposed ADR-0059 d3).
        let expired: Vec<(Uuid, i32)> = sqlx::query_as(POSTINGS_EXPIRED)
            .bind(scope.tenant_id)
            .bind(counted_nodes)
            .bind(scope.counted)
            .bind(scope.expired_at)
            .fetch_all(&mut **tx)
            .await
            .map_err(backend)?;
        // A document states no valid time, so an expired version is one
        // unit (ADR-0081 d1).
        if !expired.is_empty() {
            rows = rows.saturating_sub(expired.len() as u64);
            units = units.saturating_sub(expired.len() as u64);
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
        // The own held rows of the counted kinds count; those of the asked
        // kinds are candidates (ADR-0092 d4).
        let mut held: Vec<(Uuid, i32, bool, String)> =
            if scope.authors.is_some_and(<[Uuid]>::is_empty) || scope.held_nodes.is_empty() {
                Vec::new()
            } else {
                sqlx::query_as(POSTINGS_OWN_HELD)
                    .bind(scope.tenant_id)
                    .bind(scope.authors)
                    .bind(scope.held_nodes)
                    .bind(scope.counted)
                    .bind(scope.expired_at)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(backend)?
            };
        // The offered instructions count as the own held rows do, and are
        // candidates when their kind is asked (ADR-0092 d3, d4).
        if !scope.instructions.is_empty() {
            let offered: Vec<(Uuid, i32)> = sqlx::query_as(POSTINGS_INSTRUCTIONS)
                .bind(scope.tenant_id)
                .bind(scope.instructions)
                .fetch_all(&mut **tx)
                .await
                .map_err(backend)?;
            held.extend(
                offered
                    .into_iter()
                    .map(|(id, len)| (id, len, false, String::new())),
            );
        }
        // Under a reference set the own held rows are scored and never
        // counted: R holds served rows alone (ADR-0072 d7).
        let mut all_held: Vec<Uuid> = held.iter().map(|(id, ..)| *id).collect();
        all_held.sort_unstable();
        all_held.dedup();
        if scope.reference.is_none() {
            rows += all_held.len() as u64;
            units += held.len() as u64;
            total_len += held
                .iter()
                .map(|(_, len, ..)| u64::try_from(*len).unwrap_or(0))
                .sum::<u64>();
        }
        let mut held_ids: Vec<Uuid> = held
            .iter()
            .filter(|(.., kind)| scope.kinds.contains(&kind.as_str()))
            .map(|(id, ..)| *id)
            .chain(scope.candidate_instructions.iter().copied())
            .collect();
        held_ids.sort_unstable();
        held_ids.dedup();
        if rows == 0 {
            return Ok(empty(terms));
        }
        if !all_held.is_empty() && scope.reference.is_none() {
            let held_counted: Vec<(i64, i64)> = sqlx::query_as(POSTINGS_OWN_HELD_DF)
                .bind(scope.tenant_id)
                .bind(&all_held)
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
        let (n, avgdl) = (
            rows as f64,
            (total_len as f64 / units.max(1) as f64).max(1.0),
        );
        // A scope that reads no document runs the statement that reads no
        // passage, which plans as the one before documents did: the
        // passages' branch costs its planning on every recall (ADR-0081
        // d4; the review's benchmark of 2026-11-18). The counts are the
        // scanned nodes' only without a reference set.
        // 3: the document code ([`memory_kind_code`]).
        let reads_documents = memory_kinds.contains(&3)
            && (scope.reference.is_some()
                || documents > 0
                || held.iter().any(|(_, _, document, _)| *document));
        let anchor = scope.anchor.posting("p.memory_id", 16);
        let sql = if reads_documents {
            bm25_postings_units_sql_within(&anchor)
        } else {
            bm25_postings_sql_within(&anchor)
        };
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
        let counted = hit_kinds(&scope.counted_kinds);
        let (authors, held_nodes): (Option<Vec<Uuid>>, Vec<Uuid>) = scope
            .own_held
            .as_ref()
            .map_or((Some(Vec::new()), Vec::new()), |own| {
                (Some(held_authors(own)), own.nodes.clone())
            });
        // The offered instructions join the scope by id (ADR-0092 d3): the
        // statements' `$4` holds them beside the held rows' author keys, a
        // version id never being a key id or a rule-set mark. A candidate
        // statement names the candidates; bm25's statistics every offered
        // one, its candidates narrowed by kind (d4).
        let instructions = scope.instruction_ids();
        let candidate_instructions = scope.instruction_candidates();
        let scoped_ids: Vec<Uuid> = authors
            .iter()
            .flatten()
            .chain(&candidate_instructions)
            .copied()
            .collect();
        let counted_ids: Vec<Uuid> = authors
            .iter()
            .flatten()
            .chain(&instructions)
            .copied()
            .collect();
        let narrowing = kind_narrowing(scope);
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
                Some(&scoped_ids),
                &held_nodes,
            )
            .await?;
        let scoped = ScopeBinds {
            tenant_id,
            nodes: &scope.promoted_nodes,
            kinds: &kinds,
            counted: &counted,
            authors: authors.as_deref(),
            counted_ids: &counted_ids,
            instructions: &instructions,
            candidate_instructions: &candidate_instructions,
            narrowing: &narrowing,
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
                    .bind(&scoped_ids)
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
                        RetrieverKind::Bm25 => {
                            bm25_sql_within(&format!("{}{narrowing}", anchor.row("m", 10)))
                        }
                        _ => trigram_sql(&anchor.row("m", 9)),
                    },
                    as_of,
                );
                // bm25 counts every kind the statistics count and narrows
                // its candidates by kind; the other lists read the
                // candidates alone (ADR-0092 d4).
                let (bound_kinds, bound_ids) = if kind == RetrieverKind::Bm25 {
                    (&counted, &counted_ids)
                } else {
                    (&kinds, &scoped_ids)
                };
                // A statement at an instant is planned once: its literal
                // instant would fill the statement cache (ADR-0059 d20).
                let mut statement = sqlx::query(&sql)
                    .persistent(as_of.is_none())
                    .bind(tenant_id)
                    .bind(&scope.promoted_nodes)
                    .bind(bound_kinds)
                    .bind(bound_ids)
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
        let candidates = Self::fill_kinds(&mut tx, tenant_id, candidates).await?;
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
            workspace_drift: count("workspace_drift")?,
            term_drift: count("term_drift")?,
            terms_checked: count("terms_checked")?,
            sparse_backlog: u64::try_from(queued.0).map_err(backend)?,
            sparse_failing: u64::try_from(queued.1).map_err(backend)?,
            sparse_parked: u64::try_from(queued.2).map_err(backend)?,
            sparse_enabled: queued.3,
            similar_partition: similar.0,
            similar_unfilled: u64::try_from(similar.1).map_err(backend)?,
            similar_drift: u64::try_from(similar.2).map_err(backend)?,
            documents_uncut: count("documents_uncut")?,
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
        let rows: Vec<(Uuid, i32, String, i64)> = sqlx::query_as(SPARSE_PENDING)
            .bind(tenant_id)
            .bind(i64::try_from(limit).unwrap_or(i64::MAX))
            .bind(SPARSE_LEASE_SECS as f64)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        // One entry per version, its units in ordinal order.
        let mut pending: Vec<SparsePending> = Vec::new();
        for (memory_id, ordinal, text, enqueued_at_us) in rows {
            let unit = SparseUnit { ordinal, text };
            match pending.last_mut() {
                Some(p) if p.memory_id == memory_id => p.units.push(unit),
                _ => pending.push(SparsePending {
                    memory_id,
                    units: vec![unit],
                    enqueued_at_us,
                }),
            }
        }
        Ok(pending)
    }

    async fn sparse_write(
        &self,
        tenant_id: Uuid,
        pending: &SparsePending,
        postings: &[Vec<SparsePosting>],
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
        let Some((workspace_id, Some(memory_kind), Some(state))) = version else {
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
        // Each unit's postings carry its ordinal (ADR-0081 d4).
        let units = pending.units.iter().zip(postings);
        let ordinals: Vec<i32> = units
            .clone()
            .flat_map(|(u, list)| list.iter().map(move |_| u.ordinal))
            .collect();
        let postings: Vec<&SparsePosting> = units.flat_map(|(_, list)| list.iter()).collect();
        let tokens: Vec<i64> = postings.iter().map(|p| i64::from(p.token_id)).collect();
        let weights: Vec<f32> = postings.iter().map(|p| p.weight).collect();
        let in_text: Vec<i32> = postings.iter().map(|p| i32::from(p.in_text)).collect();
        sqlx::query(
            "INSERT INTO search_postings (tenant_id, memory_id, workspace_id, token_id, weight, \
                                          doc_len, kind, memory_kind, state, ordinal) \
             SELECT $1, $2, $3, t, w, d, 2, $4, $5, o \
               FROM unnest($6::int8[], $7::float4[], $8::int4[], $9::int4[]) AS x(t, w, d, o)",
        )
        .bind(tenant_id)
        .bind(pending.memory_id)
        .bind(workspace_id)
        .bind(memory_kind)
        .bind(state)
        .bind(&tokens)
        .bind(&weights)
        .bind(&in_text)
        .bind(&ordinals)
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
            .filter_map(|k| memory_kind_code(k))
            .collect();
        let mut held: Vec<Uuid> =
            if scope.authors.is_some_and(<[Uuid]>::is_empty) || scope.held_nodes.is_empty() {
                Vec::new()
            } else {
                sqlx::query_as::<_, (Uuid, i32, bool, String)>(POSTINGS_OWN_HELD)
                    .bind(scope.tenant_id)
                    .bind(scope.authors)
                    .bind(scope.held_nodes)
                    .bind(scope.kinds)
                    .bind(scope.expired_at)
                    .fetch_all(&mut **tx)
                    .await
                    .map_err(backend)?
                    .into_iter()
                    .map(|(id, ..)| id)
                    .collect()
            };
        held.extend(scope.candidate_instructions.iter().copied());
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
pub const SPARSE_HOLD_VERSION: &str = "SELECT m.workspace_id, oml_search_memory_kind(m.kind), \
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
    async fn web_person_reviewers(
        &self,
        tenant_id: Uuid,
    ) -> Result<Vec<PersonReviewer>, StoreError> {
        // The keys, then their grants and the denials in force, in one
        // snapshot: three statements over `api_keys`, `grants` and
        // `key_denials`, never `memories` (#435).
        let mut tx = self.pool.begin().await.map_err(backend)?;
        one_snapshot(&mut tx).await?;
        set_tenant(&mut tx, tenant_id).await?;
        let keys = sqlx::query(&format!(
            "SELECT {KEY_COLUMNS} FROM api_keys WHERE tenant_id = $1 AND role = 'reviewer' \
             AND used_by = 'person' AND crew_id IS NULL AND profile IS NULL \
             AND revoked_at IS NULL ORDER BY name"
        ))
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?
        .iter()
        .map(key_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        let ids: Vec<Uuid> = keys.iter().map(|k| k.key_id).collect();
        let grants = sqlx::query(
            "SELECT key_id, id, role, workspace_node_id, deny, covers, may_create FROM grants \
             WHERE tenant_id = $1 AND key_id = ANY($2) AND revoked_epoch IS NULL \
             ORDER BY epoch ASC, id ASC",
        )
        .bind(tenant_id)
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let denials = sqlx::query(
            "SELECT key_id, id, workspace_node_id FROM key_denials \
             WHERE tenant_id = $1 AND key_id = ANY($2) AND lifted_epoch IS NULL \
             ORDER BY epoch ASC, id ASC",
        )
        .bind(tenant_id)
        .bind(&ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let mut out: Vec<PersonReviewer> = keys
            .into_iter()
            .map(|key| PersonReviewer {
                key,
                grants: Vec::new(),
                denials: Vec::new(),
            })
            .collect();
        for row in &grants {
            let role: String = row.try_get("role").map_err(backend)?;
            let deny: Value = row.try_get("deny").map_err(backend)?;
            let covers: String = row.try_get("covers").map_err(backend)?;
            let grant = Grant {
                grant_id: row.try_get("id").map_err(backend)?,
                node: row.try_get("workspace_node_id").map_err(backend)?,
                role: role_from_str(&role)?,
                deny: deny_from_json(&deny)?,
                covers: Covers::parse(&covers).ok_or_else(|| {
                    StoreError::Backend(format!("grants.covers {covers:?} is not a coverage"))
                })?,
                create: row.try_get("may_create").map_err(backend)?,
            };
            let key_id: Uuid = row.try_get("key_id").map_err(backend)?;
            if let Some(r) = out.iter_mut().find(|r| r.key.key_id == key_id) {
                r.grants.push(grant);
            }
        }
        for row in &denials {
            let denial = KeyDenial {
                denial_id: row.try_get("id").map_err(backend)?,
                node: row.try_get("workspace_node_id").map_err(backend)?,
            };
            let key_id: Uuid = row.try_get("key_id").map_err(backend)?;
            if let Some(r) = out.iter_mut().find(|r| r.key.key_id == key_id) {
                r.denials.push(denial);
            }
        }
        Ok(out)
    }

    async fn upload_open(
        &self,
        session: &oml_app::types::UploadSession,
        per_key: u64,
        tenant_max_bytes: u64,
    ) -> Result<Result<(), oml_app::domain::upload::UploadRefusal>, StoreError> {
        use oml_app::domain::upload::UploadRefusal;
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, session.tenant_id).await?;
        // One opening at a time per tenant, so two concurrent ones cannot
        // both pass a limit the other fills (ADR-0004 d3).
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('upload_sessions:' || $1::text, 0))",
        )
        .bind(session.tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let row = sqlx::query(
            "SELECT count(*) FILTER (WHERE key_id = $2) AS by_key, \
                    COALESCE(sum(size), 0)::bigint AS bytes \
               FROM upload_sessions \
              WHERE tenant_id = $1 AND state IN ('open', 'completed')",
        )
        .bind(session.tenant_id)
        .bind(session.key_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        let by_key: i64 = row.try_get("by_key").map_err(backend)?;
        let bytes: i64 = row.try_get("bytes").map_err(backend)?;
        if u64::try_from(by_key).unwrap_or(u64::MAX) >= per_key {
            return Ok(Err(UploadRefusal::SessionLimit { limit: per_key }));
        }
        if u64::try_from(bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(session.size)
            > tenant_max_bytes
        {
            return Ok(Err(UploadRefusal::TenantLimit {
                max_bytes: tenant_max_bytes,
            }));
        }
        sqlx::query(
            "INSERT INTO upload_sessions (tenant_id, id, key_id, purpose, size, sha256, state, \
                 part_key, created_at, touched_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5, $6, 'open', $7, ($8)::timestamptz, ($8)::timestamptz, \
                 ($9)::timestamptz)",
        )
        .bind(session.tenant_id)
        .bind(session.upload_id)
        .bind(session.key_id)
        .bind(session.purpose.as_str())
        .bind(i64::try_from(session.size).map_err(backend)?)
        .bind(&session.sha256)
        .bind(session.part_key.map(|k| k.to_vec()))
        .bind(session.created_at.as_str())
        .bind(session.expires_at.as_ref().map(EventTime::as_str))
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(Ok(()))
    }

    async fn upload_session(
        &self,
        tenant_id: Uuid,
        upload_id: Uuid,
    ) -> Result<Option<oml_app::types::UploadSession>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {UPLOAD_COLUMNS} FROM upload_sessions WHERE tenant_id = $1 AND id = $2"
        ))
        .bind(tenant_id)
        .bind(upload_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(upload_from_row).transpose()
    }

    async fn upload_part_received(
        &self,
        tenant_id: Uuid,
        upload_id: Uuid,
        n: u64,
        at: &EventTime,
        expires_at: &EventTime,
    ) -> Result<Option<oml_app::types::UploadSession>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(&format!(
            "UPDATE upload_sessions \
                SET received = CASE WHEN $3 = ANY (received) THEN received \
                                    ELSE array_append(received, $3) END, \
                    touched_at = ($4)::timestamptz, expires_at = ($5)::timestamptz \
              WHERE tenant_id = $1 AND id = $2 AND state = 'open' \
          RETURNING {UPLOAD_COLUMNS}"
        ))
        .bind(tenant_id)
        .bind(upload_id)
        .bind(i64::try_from(n).map_err(backend)?)
        .bind(at.as_str())
        .bind(expires_at.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(upload_from_row).transpose()
    }

    async fn upload_parts_lost(
        &self,
        tenant_id: Uuid,
        upload_id: Uuid,
        parts: &[u64],
    ) -> Result<(), StoreError> {
        let parts = parts
            .iter()
            .map(|&n| i64::try_from(n))
            .collect::<Result<Vec<i64>, _>>()
            .map_err(backend)?;
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        sqlx::query(
            "UPDATE upload_sessions \
                SET received = ARRAY(SELECT n FROM unnest(received) AS n WHERE n <> ALL ($3)) \
              WHERE tenant_id = $1 AND id = $2 AND state = 'open'",
        )
        .bind(tenant_id)
        .bind(upload_id)
        .bind(&parts)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn upload_transition(
        &self,
        tenant_id: Uuid,
        upload_id: Uuid,
        from: &[oml_app::domain::upload::UploadState],
        to: oml_app::domain::upload::UploadState,
        at: &EventTime,
        expires_at: Option<&EventTime>,
        shred: bool,
    ) -> Result<Option<oml_app::types::UploadSession>, StoreError> {
        let from: Vec<&str> = from.iter().map(|s| s.as_str()).collect();
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(&format!(
            "UPDATE upload_sessions \
                SET state = $4, expires_at = ($6)::timestamptz, \
                    completed_at = CASE WHEN $4 = 'completed' THEN ($5)::timestamptz \
                                        ELSE completed_at END, \
                    bound_at = CASE WHEN $4 = 'bound' THEN COALESCE(bound_at, ($5)::timestamptz) \
                                    ELSE bound_at END, \
                    part_key = CASE WHEN $7 THEN NULL ELSE part_key END \
              WHERE tenant_id = $1 AND id = $2 AND state = ANY ($3) \
          RETURNING {UPLOAD_COLUMNS}"
        ))
        .bind(tenant_id)
        .bind(upload_id)
        .bind(&from)
        .bind(to.as_str())
        .bind(at.as_str())
        .bind(expires_at.map(EventTime::as_str))
        .bind(shred)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(upload_from_row).transpose()
    }

    async fn uploads_expire(&self, tenant_id: Uuid, at: &EventTime) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let expired = sqlx::query(
            "UPDATE upload_sessions SET state = 'expired', part_key = NULL \
              WHERE tenant_id = $1 AND state IN ('open', 'completed') \
                AND expires_at <= ($2)::timestamptz",
        )
        .bind(tenant_id)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        tx.commit().await.map_err(backend)?;
        Ok(expired)
    }

    async fn uploads_unswept(&self, tenant_id: Uuid, limit: u32) -> Result<Vec<Uuid>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM upload_sessions \
              WHERE tenant_id = $1 AND part_key IS NULL AND objects_removed_at IS NULL \
              ORDER BY id LIMIT $2",
        )
        .bind(tenant_id)
        .bind(i64::from(limit))
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(ids)
    }

    async fn upload_objects_removed(
        &self,
        tenant_id: Uuid,
        upload_id: Uuid,
        at: &EventTime,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        sqlx::query(
            "UPDATE upload_sessions SET objects_removed_at = ($3)::timestamptz \
              WHERE tenant_id = $1 AND id = $2 AND part_key IS NULL \
                AND objects_removed_at IS NULL",
        )
        .bind(tenant_id)
        .bind(upload_id)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn upload_stats(
        &self,
        tenant_id: Uuid,
        stale_before: &EventTime,
    ) -> Result<oml_app::types::UploadStats, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(
            "SELECT state, count(*) AS sessions, \
                    COALESCE(sum(size) FILTER (WHERE state IN ('open', 'completed')), 0)::bigint \
                        AS unbound_bytes, \
                    count(*) FILTER (WHERE part_key IS NULL AND objects_removed_at IS NULL) \
                        AS unswept, \
                    count(*) FILTER (WHERE state = 'bound' AND part_key IS NOT NULL \
                                      AND bound_at < ($2)::timestamptz) AS staging_stale \
               FROM upload_sessions WHERE tenant_id = $1 GROUP BY state",
        )
        .bind(tenant_id)
        .bind(stale_before.as_str())
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let mut stats = oml_app::types::UploadStats::default();
        for row in &rows {
            let state: String = row.try_get("state").map_err(backend)?;
            let sessions: i64 = row.try_get("sessions").map_err(backend)?;
            let bytes: i64 = row.try_get("unbound_bytes").map_err(backend)?;
            let unswept: i64 = row.try_get("unswept").map_err(backend)?;
            stats
                .by_state
                .insert(state, u64::try_from(sessions).map_err(backend)?);
            stats.unbound_bytes += u64::try_from(bytes).map_err(backend)?;
            stats.unswept += u64::try_from(unswept).map_err(backend)?;
            let stale: i64 = row.try_get("staging_stale").map_err(backend)?;
            stats.staging_stale += u64::try_from(stale).map_err(backend)?;
        }
        Ok(stats)
    }

    async fn import_target_view(
        &self,
        tenant_id: Uuid,
        target_workspace_id: Uuid,
        source_server_id: &str,
        source_tenant_id: &str,
    ) -> Result<oml_app::types::ImportTargetView, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // What imports from this source into this target wrote.
        let written = sqlx::query(
            "SELECT v.memory_id, v.source_artifact_id, v.source_version, m.claim_key \
               FROM import_versions v \
               JOIN imports i ON i.tenant_id = v.tenant_id AND i.id = v.import_id \
               JOIN memories m ON m.tenant_id = v.tenant_id AND m.id = v.memory_id \
              WHERE v.tenant_id = $1 AND i.target_workspace_id = $2 \
                AND i.source_server_id = $3 AND i.source_tenant_id = $4",
        )
        .bind(tenant_id)
        .bind(target_workspace_id)
        .bind(source_server_id)
        .bind(source_tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        // Each claim key's latest held or promoted version (R10).
        let tips = sqlx::query(
            "SELECT DISTINCT ON (claim_key) claim_key, id, version, claim_digest, retirement \
               FROM memories \
              WHERE tenant_id = $1 AND workspace_id = $2 AND status IN ('held', 'promoted') \
              ORDER BY claim_key, version DESC",
        )
        .bind(tenant_id)
        .bind(target_workspace_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        let ledger = sqlx::query(
            "SELECT exported_at, release_label, release_manifest_digest, signer_fingerprint \
               FROM import_ledger WHERE tenant_id = $1 AND target_workspace_id = $2 \
                AND source_server_id = $3 AND source_tenant_id = $4",
        )
        .bind(tenant_id)
        .bind(target_workspace_id)
        .bind(source_server_id)
        .bind(source_tenant_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let mut view = oml_app::types::ImportTargetView::default();
        let mut written_ids = BTreeSet::new();
        let mut imported_keys = BTreeSet::new();
        for row in &written {
            let memory_id: Uuid = row.try_get("memory_id").map_err(backend)?;
            let source: String = row.try_get("source_artifact_id").map_err(backend)?;
            let version: i64 = row.try_get("source_version").map_err(backend)?;
            let claim_key: String = row.try_get("claim_key").map_err(backend)?;
            written_ids.insert(memory_id);
            imported_keys.insert(claim_key);
            view.imported
                .push((source.clone(), u64::try_from(version).map_err(backend)?));
            view.carried.push((source, memory_id));
        }
        for row in &tips {
            let claim_key: String = row.try_get("claim_key").map_err(backend)?;
            let memory_id: Uuid = row.try_get("id").map_err(backend)?;
            let version: i32 = row.try_get("version").map_err(backend)?;
            view.tips.push(oml_app::types::ImportTip {
                written_by_import: written_ids.contains(&memory_id),
                imported_from_source: imported_keys.contains(&claim_key),
                claim_key,
                memory_id,
                version: u32::try_from(version).map_err(backend)?,
                body_digest: {
                    let digest: Vec<u8> = row.try_get("claim_digest").map_err(backend)?;
                    hash32(&digest)?.to_prefixed()
                },
                retirement: row.try_get("retirement").map_err(backend)?,
            });
        }
        view.ledger = ledger
            .map(|row| -> Result<_, StoreError> {
                Ok(oml_app::types::ImportLedgerEntry {
                    exported_at: row.try_get("exported_at").map_err(backend)?,
                    release_label: row.try_get("release_label").map_err(backend)?,
                    release_manifest_digest: row
                        .try_get("release_manifest_digest")
                        .map_err(backend)?,
                    signer_fingerprint: row.try_get("signer_fingerprint").map_err(backend)?,
                })
            })
            .transpose()?;
        Ok(view)
    }

    async fn import_begin(
        &self,
        import: &oml_app::types::ImportRecord,
        at: &EventTime,
    ) -> Result<oml_app::types::ImportBegin, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, import.tenant_id).await?;
        // The bind, the pin and the row are one transaction (ADR-0004 d3;
        // I4, I12): a session past its expiry at `at` binds nothing.
        let bound = sqlx::query(
            "UPDATE upload_sessions SET state = 'bound', expires_at = NULL, \
                    bound_at = COALESCE(bound_at, ($4)::timestamptz) \
              WHERE tenant_id = $1 AND id = $2 AND key_id = $3 AND state = 'completed' \
                AND expires_at > ($4)::timestamptz",
        )
        .bind(import.tenant_id)
        .bind(import.upload_id)
        .bind(import.importer_key_id)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        if bound != 1 {
            return Ok(oml_app::types::ImportBegin::NotBindable);
        }
        // R11, I12: the first import from a source pins its signer here;
        // an empty exported_at bounds no later bundle's age.
        sqlx::query(
            "INSERT INTO import_ledger (tenant_id, target_workspace_id, source_server_id, \
                    source_tenant_id, exported_at, signer_fingerprint, updated_at) \
             VALUES ($1, $2, $3, $4, '', $5, ($6)::timestamptz) \
             ON CONFLICT (tenant_id, target_workspace_id, source_server_id, source_tenant_id) \
             DO NOTHING",
        )
        .bind(import.tenant_id)
        .bind(import.target_workspace_id)
        .bind(&import.source_server_id)
        .bind(&import.source_tenant_id)
        .bind(&import.signer_fingerprint)
        .bind(at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let pinned: String = sqlx::query_scalar(
            "SELECT signer_fingerprint FROM import_ledger \
              WHERE tenant_id = $1 AND target_workspace_id = $2 AND source_server_id = $3 \
                AND source_tenant_id = $4",
        )
        .bind(import.tenant_id)
        .bind(import.target_workspace_id)
        .bind(&import.source_server_id)
        .bind(&import.source_tenant_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if pinned != import.signer_fingerprint {
            // Dropping the transaction rolls the bind back.
            return Ok(oml_app::types::ImportBegin::KeyPinned { pinned });
        }
        sqlx::query(
            "INSERT INTO imports (tenant_id, id, target_workspace_id, importer_key_id, upload_id, \
                    source_server_id, source_tenant_id, manifest_digest, signer_fingerprint, \
                    authenticated, allow_older, state, counts, refusals, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
                    ($15)::timestamptz)",
        )
        .bind(import.tenant_id)
        .bind(import.import_id)
        .bind(import.target_workspace_id)
        .bind(import.importer_key_id)
        .bind(import.upload_id)
        .bind(&import.source_server_id)
        .bind(&import.source_tenant_id)
        .bind(&import.manifest_digest)
        .bind(&import.signer_fingerprint)
        .bind(import.authenticated)
        .bind(import.allow_older)
        .bind(import.state.as_str())
        .bind(json!(import.counts))
        .bind(serde_json::to_value(&import.refusals).map_err(backend)?)
        .bind(import.created_at.as_str())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(oml_app::types::ImportBegin::Begun)
    }

    async fn imports_end_stopped(
        &self,
        tenant_id: Uuid,
        started_before: &EventTime,
        at: &EventTime,
        live: &[Uuid],
    ) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // I11: the import ends failed and its staging key goes in one
        // transaction; the upload sweep removes the objects.
        let uploads: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE imports SET state = 'failed', ended_at = ($3)::timestamptz \
              WHERE tenant_id = $1 AND state = 'running' AND created_at < ($2)::timestamptz \
                AND NOT (id = ANY($4)) \
             RETURNING upload_id",
        )
        .bind(tenant_id)
        .bind(started_before.as_str())
        .bind(at.as_str())
        .bind(live)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        if !uploads.is_empty() {
            sqlx::query(
                "UPDATE upload_sessions SET part_key = NULL \
                  WHERE tenant_id = $1 AND id = ANY($2) AND state = 'bound'",
            )
            .bind(tenant_id)
            .bind(&uploads)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        // A bound upload whose import ended and whose key its end could not
        // shred loses the key now.
        sqlx::query(
            "UPDATE upload_sessions u SET part_key = NULL FROM imports i \
              WHERE u.tenant_id = $1 AND u.state = 'bound' AND u.part_key IS NOT NULL \
                AND i.tenant_id = u.tenant_id AND i.upload_id = u.id AND i.state <> 'running'",
        )
        .bind(tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(uploads.len() as u64)
    }

    async fn flagged_creates(&self, tenant_id: Uuid) -> Result<Vec<(Uuid, u64)>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // A scan of the tenant's `workspace.create` events, read by the
        // admin rescan window alone (the bottleneck register's "Flagged
        // creates" row; ADR-0097 d31, the operator's decision of
        // 2026-11-27).
        let rows: Vec<(Uuid, i64)> = sqlx::query_as(
            "SELECT j.api_key_id, count(*) FROM journal j \
              WHERE j.tenant_id = $1 AND j.event_type = 'workspace.create' \
                AND j.payload ? 'create_grant_id' AND j.api_key_id IS NOT NULL \
              GROUP BY j.api_key_id ORDER BY j.api_key_id",
        )
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.into_iter()
            .map(|(key, n)| Ok((key, u64::try_from(n).map_err(backend)?)))
            .collect()
    }

    async fn workspace_creator_if_empty(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<Uuid>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The creating event is read only for an empty workspace, the
        // import's retry (ADR-0030 d1, amended 2026-11-18).
        let creator: Option<Option<Uuid>> = sqlx::query_scalar(
            "SELECT j.api_key_id FROM journal j \
              WHERE j.tenant_id = $1 AND j.event_type = 'workspace.create' \
                AND j.payload->>'workspace_id' = $2::text \
                AND NOT EXISTS (SELECT 1 FROM memories m \
                                 WHERE m.tenant_id = $1 AND m.workspace_id = $2) \
                AND NOT EXISTS (SELECT 1 FROM workspaces w \
                                 WHERE w.tenant_id = $1 AND w.parent_id = $2) \
              ORDER BY j.seq LIMIT 1",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(creator.flatten())
    }

    async fn import_version_recorded(
        &self,
        tenant_id: Uuid,
        import_id: Uuid,
        version: &oml_app::types::ImportedVersion,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        sqlx::query(
            "INSERT INTO import_versions (tenant_id, import_id, memory_id, source_artifact_id, \
                    source_version, outcome) VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT DO NOTHING",
        )
        .bind(tenant_id)
        .bind(import_id)
        .bind(version.memory_id)
        .bind(&version.source_artifact_id)
        .bind(i64::try_from(version.source_version).map_err(backend)?)
        .bind(&version.outcome)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query("UPDATE memories SET import_provenance = $3 WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id)
            .bind(version.memory_id)
            .bind(&version.provenance)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn import_provenance(
        &self,
        tenant_id: Uuid,
        memory_id: Uuid,
    ) -> Result<Option<Value>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let provenance: Option<Option<Value>> = sqlx::query_scalar(
            "SELECT import_provenance FROM memories WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant_id)
        .bind(memory_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(provenance.flatten())
    }

    async fn import_failed(
        &self,
        tenant_id: Uuid,
        import_id: Uuid,
        at: &EventTime,
        counts: &BTreeMap<String, u64>,
        refusals: &[oml_app::types::ImportRefusal],
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        sqlx::query(
            "UPDATE imports SET state = 'failed', ended_at = ($3)::timestamptz, counts = $4, \
                    refusals = $5 \
              WHERE tenant_id = $1 AND id = $2 AND state = 'running'",
        )
        .bind(tenant_id)
        .bind(import_id)
        .bind(at.as_str())
        .bind(json!(counts))
        .bind(serde_json::to_value(refusals).map_err(backend)?)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn import_record(
        &self,
        tenant_id: Uuid,
        import_id: Uuid,
    ) -> Result<Option<oml_app::types::ImportRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {IMPORT_COLUMNS} FROM imports i WHERE i.tenant_id = $1 AND i.id = $2"
        ))
        .bind(tenant_id)
        .bind(import_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref().map(import_from_row).transpose()
    }

    async fn imports_of_keys(
        &self,
        tenant_id: Uuid,
        key_ids: &[Uuid],
        state: Option<oml_app::domain::import::ImportState>,
        before: Option<(&EventTime, Uuid)>,
        limit: usize,
    ) -> Result<Vec<oml_app::types::ImportRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // Served by imports_importer_created (0068; ADR-0096 d6).
        let rows = sqlx::query(&format!(
            "SELECT {IMPORT_COLUMNS} FROM imports i \
              WHERE i.tenant_id = $1 AND i.importer_key_id = ANY($2) \
                AND ($3::text IS NULL OR i.state = $3) \
                AND ($4::text IS NULL OR (i.created_at, i.id) < (($4)::timestamptz, $5)) \
              ORDER BY i.created_at DESC, i.id DESC LIMIT $6"
        ))
        .bind(tenant_id)
        .bind(key_ids)
        .bind(state.map(|s| s.as_str()))
        .bind(before.map(|(at, _)| at.as_str().to_owned()))
        .bind(before.map(|(_, id)| id))
        .bind(i64::try_from(limit).map_err(backend)?)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(import_from_row).collect()
    }

    async fn import_stats(
        &self,
        tenant_id: Uuid,
        stopped_before: &EventTime,
    ) -> Result<oml_app::types::ImportStats, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(
            "SELECT state, count(*) AS imports, count(*) FILTER (WHERE allow_older) AS older, \
                    count(*) FILTER (WHERE state = 'running' \
                                      AND created_at < ($2)::timestamptz) AS stopped \
               FROM imports WHERE tenant_id = $1 GROUP BY state",
        )
        .bind(tenant_id)
        .bind(stopped_before.as_str())
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        // I14: a version an import's idempotency key names that carries no
        // provenance.
        let unprovenanced: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency i \
               JOIN memories m ON m.tenant_id = i.tenant_id \
                AND m.id::text = i.response->>'memory_id' \
              WHERE i.tenant_id = $1 AND split_part(i.key, ':', 3) = 'import' \
                AND m.import_provenance IS NULL",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let mut stats = oml_app::types::ImportStats::default();
        for row in &rows {
            let state: String = row.try_get("state").map_err(backend)?;
            let imports: i64 = row.try_get("imports").map_err(backend)?;
            let older: i64 = row.try_get("older").map_err(backend)?;
            stats
                .by_state
                .insert(state, u64::try_from(imports).map_err(backend)?);
            stats.allow_older += u64::try_from(older).map_err(backend)?;
            let stopped: i64 = row.try_get("stopped").map_err(backend)?;
            stats.stopped += u64::try_from(stopped).map_err(backend)?;
        }
        stats.unprovenanced = u64::try_from(unprovenanced).map_err(backend)?;
        Ok(stats)
    }

    async fn export_policy(
        &self,
        tenant_id: Uuid,
    ) -> Result<oml_app::domain::export::ExportPolicy, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let policy: Option<String> =
            sqlx::query_scalar("SELECT export_policy FROM tenants WHERE id = $1")
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        match policy {
            None => Ok(oml_app::domain::export::ExportPolicy::default()),
            Some(p) => oml_app::domain::export::ExportPolicy::parse(&p)
                .ok_or_else(|| backend(format!("export_policy {p:?} is not a policy"))),
        }
    }

    async fn export_allowed(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        at: &EventTime,
    ) -> Result<bool, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let allowed: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM export_allowances \
                             WHERE tenant_id = $1 AND workspace_id = $2 \
                               AND (until IS NULL OR until > ($3)::timestamptz))",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(at.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(allowed)
    }

    async fn export_versions(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        own_keys: &[Uuid],
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = $1 \
             AND m.workspace_id = ANY($2) \
             AND (m.status = 'promoted' \
                  OR (m.status = 'erased' AND m.promoted_at IS NOT NULL) \
                  OR m.author_key_id = ANY($3)) \
             ORDER BY m.created_at, m.id"
        );
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(nodes)
            .bind(own_keys)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(memory_from_row).collect()
    }

    async fn export_body_bytes(
        &self,
        tenant_id: Uuid,
        nodes: &[Uuid],
        own_keys: &[Uuid],
    ) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let bytes: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(m.body_bytes), 0)::bigint FROM memories m \
              WHERE m.tenant_id = $1 AND m.workspace_id = ANY($2) AND m.status <> 'erased' \
                AND (m.status = 'promoted' OR m.author_key_id = ANY($3))",
        )
        .bind(tenant_id)
        .bind(nodes)
        .bind(own_keys)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        u64::try_from(bytes).map_err(backend)
    }

    async fn exported_bytes(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        at: &EventTime,
    ) -> Result<u64, StoreError> {
        let month = Month::of(at);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let bytes: i64 = sqlx::query_scalar(
            "SELECT coalesce(sum(exports), 0)::bigint FROM account_activity \
              WHERE tenant_id = $1 AND user_root = $2 \
                AND day >= ($3)::date AND day < ($4)::date",
        )
        .bind(tenant_id)
        .bind(user_root)
        .bind(month.first_day())
        .bind(month.next().first_day())
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        u64::try_from(bytes).map_err(backend)
    }

    async fn tenant_exported_bytes(
        &self,
        tenant_id: Uuid,
        at: &EventTime,
    ) -> Result<u64, StoreError> {
        let month = Month::of(at);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let bytes: i64 = sqlx::query_scalar(
            "SELECT coalesce(sum(exports), 0)::bigint FROM tenant_activity \
              WHERE tenant_id = $1 AND day >= ($2)::date AND day < ($3)::date",
        )
        .bind(tenant_id)
        .bind(month.first_day())
        .bind(month.next().first_day())
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        u64::try_from(bytes).map_err(backend)
    }

    async fn user_usage(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        at: &EventTime,
    ) -> Result<oml_app::types::UserUsage, StoreError> {
        let month = Month::of(at);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row: Counts = sqlx::query_as(
            "SELECT coalesce(m.memories, 0), coalesce(m.text_bytes, 0), \
                    coalesce(a.writes, 0), coalesce(a.recalls, 0), \
                    coalesce(m.keys, 0), coalesce(m.connections, 0), \
                    coalesce(m.workspaces, 0) \
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
                    coalesce(a.writes, 0), coalesce(a.recalls, 0), m.keys, m.connections, \
                    m.workspaces \
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
                |(user_root, profile, a, b, c, d, e, f, g)| oml_app::types::AccountCounters {
                    user_root,
                    profile,
                    usage: usage_of((a, b, c, d, e, f, g)),
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
                        keys, connections, workspaces \
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
                |(user_root, day, profile, a, b, c, d, e, f, g)| oml_app::types::LedgerRow {
                    user_root,
                    day,
                    profile,
                    usage: usage_of((a, b, c, d, e, f, g)),
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
                  held AS (SELECT a.user_root, count(*)::bigint AS writes \
                             FROM memories m CROSS JOIN LATERAL \
                                  (SELECT oml_key_account(m.tenant_id, m.author_key_id) \
                                          AS user_root) a \
                            WHERE m.tenant_id = $1 AND a.user_root IS NOT NULL \
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
                    coalesce(c.connections, 0), coalesce(src.connections, 0), \
                    coalesce(c.workspaces, 0), coalesce(src.workspaces, 0) \
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
        for (root, cm, sm, cb, sb, cw, sw, ck, sk, cc, sc, cs, ss) in rows {
            for (meter, counter, source) in [
                (Meter::Memories, cm, sm),
                (Meter::TextBytes, cb, sb),
                (Meter::Writes, cw, sw),
                (Meter::Keys, ck, sk),
                (Meter::Connections, cc, sc),
                (Meter::Workspaces, cs, ss),
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
            "SELECT id, workspace_id, author_key_id FROM memories \
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
                    workspace_id: row.try_get("workspace_id").map_err(backend)?,
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
            "SELECT id FROM workspaces WHERE tenant_id = $1 AND id = ANY($2::uuid[]) \
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
        let memory_kinds: Vec<i16> = kinds.iter().filter_map(|k| memory_kind_code(k)).collect();
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
        let memory_kinds: Vec<i16> = kinds.iter().filter_map(|k| memory_kind_code(k)).collect();
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
               AND m.workspace_id = ANY($2::uuid[]) \
             ORDER BY array_position($2::uuid[], m.workspace_id) ASC, \
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
               AND m.workspace_id = ANY($2::uuid[]) \
             ORDER BY array_position($2::uuid[], m.workspace_id) ASC, \
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

    async fn instruction_rows(
        &self,
        tenant_id: Uuid,
        kind: MemoryKind,
        nodes: &[Uuid],
        author_key_id: Uuid,
        held_nodes: &[Uuid],
        bodies: bool,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // A listing that serves no body selects none (ADR-0080 d11), nor
        // the search text, which carries a titled version's body.
        let columns = if bodies {
            MEMORY_COLUMNS.to_owned()
        } else {
            MEMORY_COLUMNS
                .replacen("m.claim, ", "''::text AS claim, ", 1)
                .replacen("m.search_text, ", "NULL::text AS search_text, ", 1)
        };
        // The promoted served tips of the kind on the named nodes,
        // never a retirement version, the held versions the calling key
        // authored there, and every held version on the nodes inside an
        // in-progress subtree: the state and author predicates are this
        // statement's own (proposed ADR-0037 d3, d4; ADR-0080 d10, d16).
        let sql = format!(
            "SELECT {columns} FROM memories m \
             WHERE m.tenant_id = $1 AND m.kind = $5 \
               AND m.workspace_id = ANY($2::uuid[]) \
               AND ((m.status = 'promoted' AND m.is_tip AND NOT m.retirement) \
                    OR (m.status = 'held' AND (m.author_key_id = $3 \
                        OR m.workspace_id = ANY($4::uuid[]))))"
        );
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(nodes)
            .bind(author_key_id)
            .bind(held_nodes)
            .bind(kind.as_str())
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
        push_listing_predicate(&mut sql, filter, false);
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

    async fn count_memories(
        &self,
        tenant_id: Uuid,
        filter: &ListingFilter,
        kinds: &[MemoryKind],
        cap: u64,
    ) -> Result<Vec<(MemoryKind, u64)>, StoreError> {
        // Per kind, at most `cap + 1` admitted rows read by the catalog
        // index, so a count costs no more than its cap (ADR-0092 d6, d9).
        let mut sql = QueryBuilder::<Postgres>::new("SELECT k.kind, c.n FROM unnest(");
        sql.push_bind(kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>())
            .push(
                "::text[]) AS k(kind) CROSS JOIN LATERAL (SELECT count(*)::int8 AS n FROM \
                 (SELECT 1 FROM memories m WHERE m.tenant_id = ",
            )
            .push_bind(tenant_id);
        push_listing_predicate(&mut sql, filter, true);
        sql.push(" AND m.kind = k.kind LIMIT ")
            .push_bind(i64::try_from(cap.saturating_add(1)).unwrap_or(i64::MAX))
            .push(") x) c");
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows: Vec<(String, i64)> = sql
            .build_query_as()
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.into_iter()
            .map(|(kind, n)| {
                Ok((
                    MemoryKind::parse(&kind).ok_or_else(|| {
                        StoreError::Backend(format!("an unknown kind {kind:?} was counted"))
                    })?,
                    u64::try_from(n).map_err(backend)?,
                ))
            })
            .collect()
    }

    async fn claim_key_state(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        claim_key: &str,
    ) -> Result<ClaimKeyState, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(
            "SELECT max(version) AS greatest, \
                    coalesce(bool_or(status IN ('held', 'promoted')), false) AS live, \
                    coalesce(bool_or(status = 'demoted'), false) AS demoted \
             FROM memories WHERE tenant_id = $1 AND workspace_id = $2 AND claim_key = $3",
        )
        .bind(tenant_id)
        .bind(workspace_id)
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
            demoted: row.try_get("demoted").map_err(backend)?,
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
        // finding. The unapproved count is of served tips: a version a
        // ratified successor took the tip from is not counted, so
        // superseding one is a remedy that ends, and a retirement tip is
        // not counted, since recall serves none (ADR-0077 d6; #311, #332).
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The instruction class is read off the domain, as the queue read
        // reads it; `approver_used_by` is the approving key's attestation,
        // and a row with none is counted (ADR-0080 d16; #436).
        let instructions: Vec<&str> = MemoryKind::ALL
            .iter()
            .filter(|k| k.class().needs_person())
            .map(|k| k.as_str())
            .collect();
        let (held, all, unlisted, unapproved, withdrawn, unpersoned, stranded, keyed): (
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
            i64,
        ) = sqlx::query_as(
            "SELECT count(*) FILTER (WHERE status = 'held' \
                                       AND scanned_under IS DISTINCT FROM $2::text), \
                    count(*) FILTER (WHERE scanned_under IS DISTINCT FROM $2::text), \
                    count(*) FILTER (WHERE status = 'held' AND scan_finding IS NOT NULL \
                                       AND NOT EXISTS (SELECT 1 FROM scan_findings f \
                                                        WHERE f.tenant_id = m.tenant_id \
                                                          AND f.memory_id = m.id)), \
                    count(*) FILTER (WHERE status = 'promoted' AND is_tip \
                                       AND NOT retirement AND approved_at IS NULL), \
                    count(*) FILTER (WHERE status = 'withdrawn' \
                                       AND EXISTS (SELECT 1 FROM review_queue q \
                                                    WHERE q.tenant_id = m.tenant_id \
                                                      AND q.memory_id = m.id)), \
                    count(*) FILTER (WHERE status = 'promoted' AND is_tip \
                                       AND NOT retirement AND kind = ANY($3::text[]) \
                                       AND approver_used_by IS DISTINCT FROM 'person'), \
                    count(*) FILTER (WHERE status = 'held' AND replaces_artifact_id IS NOT NULL \
                                       AND NOT EXISTS (SELECT 1 FROM memories t \
                                                        WHERE t.tenant_id = m.tenant_id \
                                                          AND t.id = m.replaces_artifact_id \
                                                          AND t.status = 'promoted' AND t.is_tip)), \
                    (SELECT count(*) FROM memories e \
                      WHERE e.tenant_id = $1 AND e.status = 'erased' \
                        AND EXISTS (SELECT 1 FROM artifact_keys k \
                                     WHERE k.tenant_id = e.tenant_id \
                                       AND k.artifact_id = e.id AND k.destroyed_at IS NULL)) \
             FROM memories m \
             WHERE tenant_id = $1 AND status <> 'erased'",
        )
        .bind(tenant_id)
        .bind(current)
        .bind(&instructions)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(ScanBacklog {
            held_unrescanned: u64::try_from(held).map_err(backend)?,
            rescan_due: u64::try_from(all).map_err(backend)?,
            findings_unlisted: u64::try_from(unlisted).map_err(backend)?,
            promoted_unapproved: u64::try_from(unapproved).map_err(backend)?,
            withdrawn_in_queue: u64::try_from(withdrawn).map_err(backend)?,
            instructions_without_person_approval: u64::try_from(unpersoned).map_err(backend)?,
            held_successors_stranded: u64::try_from(stranded).map_err(backend)?,
            erased_keys_live: u64::try_from(keyed).map_err(backend)?,
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
        // One probe per node through memories_workspace (tenant_id,
        // workspace_id, state) (ADR-0030 d6).
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM memories \
             WHERE tenant_id = $1 AND workspace_id = ANY($2::uuid[]) AND status = 'held'",
        )
        .bind(tenant_id)
        .bind(nodes)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        u64::try_from(n).map_err(backend)
    }

    async fn stale_count(&self, tenant_id: Uuid, workspace_id: Uuid) -> Result<u64, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM memories \
             WHERE tenant_id = $1 AND workspace_id = $2 AND status = 'promoted' AND is_tip \
               AND source_check_result IN ('changed', 'removed')",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        u64::try_from(n).map_err(backend)
    }

    async fn approved_by_kind(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Vec<(MemoryKind, u64)>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // A claim has one served tip (ADR-0025 d10), so counting distinct
        // claim keys counts the tips once each (ADR-0082 d10).
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT kind, count(DISTINCT claim_key) FROM memories \
             WHERE tenant_id = $1 AND workspace_id = $2 AND status = 'promoted' AND is_tip \
               AND NOT retirement \
             GROUP BY kind ORDER BY kind",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.into_iter()
            .map(|(kind, n)| {
                let kind = MemoryKind::parse(&kind)
                    .ok_or_else(|| StoreError::Backend(format!("unknown stored kind {kind:?}")))?;
                Ok((kind, u64::try_from(n).map_err(backend)?))
            })
            .collect()
    }

    async fn claim_latest_readable(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        claim_key: &str,
        own_key_ids: &[Uuid],
        reads_promoted: bool,
    ) -> Result<Option<u32>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let latest: Option<i32> = sqlx::query_scalar(
            "SELECT max(version) FROM memories \
             WHERE tenant_id = $1 AND workspace_id = $2 AND claim_key = $3 \
               AND ((status = 'promoted' AND ($5 OR author_key_id = ANY($4::uuid[]))) \
                 OR (status = 'held' AND author_key_id = ANY($4::uuid[])))",
        )
        .bind(tenant_id)
        .bind(workspace_id)
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

    async fn fix(
        &self,
        tenant_id: Uuid,
        fix_id: Uuid,
    ) -> Result<Option<oml_app::types::FixRecord>, StoreError> {
        Ok(self.read_fixes(tenant_id, Some(fix_id), false).await?.pop())
    }

    async fn fixes(
        &self,
        tenant_id: Uuid,
        unconfirmed_only: bool,
    ) -> Result<Vec<oml_app::types::FixRecord>, StoreError> {
        self.read_fixes(tenant_id, None, unconfirmed_only).await
    }

    async fn admin_act(
        &self,
        tenant_id: Uuid,
        act_id: Uuid,
    ) -> Result<Option<oml_app::types::AdminActRecord>, StoreError> {
        Ok(self
            .read_admin_acts(tenant_id, Some(act_id), None)
            .await?
            .pop())
    }

    async fn admin_acts(
        &self,
        tenant_id: Uuid,
        state: Option<oml_app::domain::admin_act::AdminActState>,
    ) -> Result<Vec<oml_app::types::AdminActRecord>, StoreError> {
        self.read_admin_acts(tenant_id, None, state).await
    }

    async fn admin_standing(
        &self,
        tenant_id: Uuid,
    ) -> Result<oml_app::types::AdminStanding, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let sole_admin: Option<bool> =
            sqlx::query_scalar("SELECT sole_admin FROM tenants WHERE id = $1")
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        let rows = sqlx::query(
            "SELECT user_name::text AS user_name, disabled_at IS NOT NULL AS disabled, \
                    to_char(admin_since AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
                    AS admin_since_s \
               FROM web_emails \
              WHERE tenant_id = $1 AND admin AND pending_since IS NULL \
                AND admin_since IS NOT NULL \
              ORDER BY user_name",
        )
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let persons = rows
            .iter()
            .map(|row| {
                let user: String = row.try_get("user_name").map_err(backend)?;
                let since: String = row.try_get("admin_since_s").map_err(backend)?;
                Ok(oml_app::types::AdminPerson {
                    user: Name::parse_reference(&user).map_err(backend)?,
                    admin_since: EventTime::parse(&since).map_err(backend)?,
                    disabled: row.try_get("disabled").map_err(backend)?,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        Ok(oml_app::types::AdminStanding {
            sole_admin: sole_admin.unwrap_or(true),
            persons,
        })
    }

    /// No release table exists until R3 (ADR-0085), so no version is a
    /// member; R3 reads its membership here.
    async fn member_of_no_release(
        &self,
        _tenant_id: Uuid,
        _memory_id: Uuid,
    ) -> Result<bool, StoreError> {
        Ok(true)
    }

    async fn notices(
        &self,
        tenant_id: Uuid,
        recipient: &Name,
        dismissed: bool,
    ) -> Result<Vec<oml_app::types::NoticeRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows: Vec<(Uuid, String, Option<Uuid>, Value, String, Option<String>)> =
            sqlx::query_as(
                "SELECT id, kind, act_id, detail, \
                        to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
                        to_char(dismissed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
                   FROM notices \
                  WHERE tenant_id = $1 AND recipient = $2 AND ($3 OR dismissed_at IS NULL) \
                  ORDER BY seq, id",
            )
            .bind(tenant_id)
            .bind(recipient.as_str())
            .bind(dismissed)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.into_iter()
            .map(|(id, kind, act_id, detail, at, dismissed_at)| {
                Ok(oml_app::types::NoticeRecord {
                    notice_id: id,
                    recipient: recipient.clone(),
                    kind: oml_app::domain::notice::NoticeKind::parse(&kind)
                        .ok_or_else(|| StoreError::Backend(format!("notice kind {kind}")))?,
                    act_id,
                    detail,
                    at: EventTime::parse(&at).map_err(backend)?,
                    dismissed_at: dismissed_at
                        .as_deref()
                        .map(EventTime::parse)
                        .transpose()
                        .map_err(backend)?,
                })
            })
            .collect()
    }

    async fn admin_read_counts(
        &self,
        tenant_id: Uuid,
        person: &Name,
    ) -> Result<Vec<oml_app::types::AdminReadCount>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT month, reads FROM admin_read_counts \
              WHERE tenant_id = $1 AND person = $2 ORDER BY month",
        )
        .bind(tenant_id)
        .bind(person.as_str())
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(|(month, reads)| oml_app::types::AdminReadCount {
                month,
                reads: u64::try_from(reads).unwrap_or(0),
            })
            .collect())
    }

    async fn investigation(
        &self,
        tenant_id: Uuid,
        investigation_id: Uuid,
    ) -> Result<Option<oml_app::types::Investigation>, StoreError> {
        Ok(self
            .read_investigations(tenant_id, Some(investigation_id), None, false)
            .await?
            .pop())
    }

    async fn investigations(
        &self,
        tenant_id: Uuid,
        key_id: Option<Uuid>,
        open_only: bool,
    ) -> Result<Vec<oml_app::types::Investigation>, StoreError> {
        self.read_investigations(tenant_id, None, key_id, open_only)
            .await
    }

    async fn valid_until_before(
        &self,
        tenant_id: Uuid,
        memory_id: Uuid,
    ) -> Result<Option<Option<ValidEnd>>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row: Option<(Option<String>, Option<i64>)> = sqlx::query_as(
            "SELECT valid_until_before, valid_until_before_us FROM memories \
              WHERE tenant_id = $1 AND id = $2 AND valid_until_ratified",
        )
        .bind(tenant_id)
        .bind(memory_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(row.map(|(text, micros)| text.zip(micros).map(|(t, m)| ValidEnd::from_stored(t, m))))
    }

    async fn claim_versions(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        claim_key: &str,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // `memories_claim_version` answers this lookup.
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m \
             WHERE m.tenant_id = $1 AND m.workspace_id = $2 AND m.claim_key = $3 \
             ORDER BY m.version"
        );
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(claim_key)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(memory_from_row).collect()
    }

    async fn claim_tip(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
        claim_key: &str,
    ) -> Result<Option<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The memories_served_tip index answers this lookup.
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m \
             WHERE m.tenant_id = $1 AND m.workspace_id = $2 AND m.claim_key = $3 \
               AND m.is_tip AND m.status = 'promoted'"
        );
        let row = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(workspace_id)
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
        workspaces: &[Uuid],
        lane: ReviewLane,
    ) -> Result<Option<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // Serve the oldest entry this reviewer may judge: only rows within
        // its reviewer access (ADR-0025 d1), skipping rows another key
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
        // The person lane narrows by who may approve, and serves its rows
        // round-robin across author keys: a row's turn is its place among
        // its author's lane rows in age order, claimed or not, the lowest
        // turn first and the older row first within a turn (ADR-0074 d19;
        // the operator, 2026-11-17). The turn is taken in a subquery: a
        // locking clause and a window function never share a query level.
        let lane_filter = lane_sql(lane, "m").map_or_else(String::new, |p| format!(" AND {p}"));
        let lane_turns = lane_sql(lane, "m2").map_or_else(String::new, |p| format!(" AND {p}"));
        let (turns, order) = if lane == ReviewLane::Person {
            (
                format!(
                    "JOIN (SELECT q2.memory_id, row_number() OVER ( \
                         PARTITION BY m2.author_key_id ORDER BY q2.held_since, q2.memory_id) AS turn \
                       FROM review_queue q2 \
                       JOIN memories m2 ON m2.tenant_id = q2.tenant_id AND m2.id = q2.memory_id \
                       WHERE q2.tenant_id = $1 AND m2.tenant_id = $1 \
                         AND m2.workspace_id = ANY($4::uuid[]){lane_turns}) c \
                     ON c.memory_id = q.memory_id "
                ),
                "c.turn ASC, q.held_since ASC, q.memory_id ASC",
            )
        } else {
            (String::new(), "q.held_since ASC, q.memory_id ASC")
        };
        let same_person = SAME_PERSON_OUTSIDE_ROOT.replace(REVIEWER_KEY, "$2");
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM review_queue q \
             JOIN memories m ON m.tenant_id = q.tenant_id AND m.id = q.memory_id \
             {turns}WHERE q.tenant_id = $1 AND m.tenant_id = $1 \
               AND m.workspace_id = ANY($4::uuid[]) \
               AND (q.held_by_key IS NULL OR q.held_by_key = $2) \
               AND NOT (m.lineage_principals @> ARRAY[$3]::text[]) \
               AND NOT (m.lineage_key_ids @> ARRAY[$2]::uuid[]) \
               AND (m.replaces_artifact_id IS NULL OR NOT EXISTS ( \
                   SELECT 1 FROM memories r \
                   WHERE r.tenant_id = $1 AND r.id <> m.id AND r.status = 'held' \
                     AND r.replaces_artifact_id = m.replaces_artifact_id \
                     AND r.workspace_id = m.workspace_id AND r.claim_key = m.claim_key \
                     AND (r.author_key_id = $2 OR r.author_principal = $3))) \
               AND NOT {same_person} \
             {lane_filter} ORDER BY {order} \
             LIMIT 1 \
             FOR NO KEY UPDATE OF q SKIP LOCKED"
        );
        let row = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(reviewer_key_id)
            .bind(reviewer_principal)
            .bind(workspaces)
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
        lane: ReviewLane,
    ) -> Result<Vec<Uuid>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The rows this key claimed in its lane, in the order review next
        // serves them; two tell one open item from several (ADR-0039 d10;
        // ADR-0074 d19).
        let lane_filter = lane_sql(lane, "m").map_or_else(String::new, |p| format!(" AND {p}"));
        let ids: Vec<Uuid> = sqlx::query_scalar(&format!(
            "SELECT q.memory_id FROM review_queue q \
             JOIN memories m ON m.tenant_id = q.tenant_id AND m.id = q.memory_id \
             WHERE q.tenant_id = $1 AND m.tenant_id = $1 AND q.held_by_key = $2{lane_filter} \
             ORDER BY q.held_since ASC, q.memory_id ASC \
             LIMIT 2",
        ))
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
        push_held_predicates(
            &mut sql,
            tenant_id,
            reviewer_principal,
            reviewer_key_id,
            filter,
        );
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

    async fn review_held_authors(
        &self,
        tenant_id: Uuid,
        reviewer_principal: &str,
        reviewer_key_id: Uuid,
        filter: &HeldFilter,
    ) -> Result<Vec<(Uuid, u64)>, StoreError> {
        if filter.nodes.is_empty() {
            return Ok(Vec::new());
        }
        // The held listing's rows, counted per author key: the person
        // lane's counts beside its page (ADR-0074 d19).
        let mut sql = QueryBuilder::<Postgres>::new(
            "SELECT m.author_key_id, count(*) AS held FROM review_queue q \
             JOIN memories m ON m.tenant_id = q.tenant_id AND m.id = q.memory_id \
             WHERE q.tenant_id = ",
        );
        push_held_predicates(
            &mut sql,
            tenant_id,
            reviewer_principal,
            reviewer_key_id,
            filter,
        );
        sql.push(" GROUP BY m.author_key_id ORDER BY m.author_key_id");
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sql.build().fetch_all(&mut *tx).await.map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter()
            .map(|row| {
                let author: Uuid = row.try_get("author_key_id").map_err(backend)?;
                let held: i64 = row.try_get("held").map_err(backend)?;
                Ok((author, u64::try_from(held).map_err(backend)?))
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

    async fn key_by_previous_hash(
        &self,
        key_hash: &str,
    ) -> Result<Option<(KeyRecord, EventTime)>, StoreError> {
        let Some(hash) = key_hash_bytes(key_hash) else {
            return Ok(None);
        };
        let mut tx = self.pool.begin().await.map_err(backend)?;
        // 0069's key_resolution policy opens exactly the previous hash's
        // row; the key's own row is then read under its tenant, since its
        // current hash is another (ADR-0096 d13).
        sqlx::query("SELECT set_config('oml.api_key_hash', $1, true)")
            .bind(hex::encode(&hash))
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        let Some(previous) = sqlx::query(
            "SELECT tenant_id, key_id, \
             to_char(valid_until AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
             AS valid_until FROM key_previous_hashes WHERE hash = $1",
        )
        .bind(&hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        else {
            tx.commit().await.map_err(backend)?;
            return Ok(None);
        };
        let tenant_id: Uuid = previous.try_get("tenant_id").map_err(backend)?;
        let key_id: Uuid = previous.try_get("key_id").map_err(backend)?;
        let until: String = previous.try_get("valid_until").map_err(backend)?;
        let until = EventTime::parse(&until).map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let row = sqlx::query(&format!(
            "SELECT {KEY_COLUMNS} FROM api_keys \
             WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL"
        ))
        .bind(tenant_id)
        .bind(key_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        row.as_ref()
            .map(|row| key_from_row(row).map(|key| (key, until.clone())))
            .transpose()
    }

    async fn key_previous_valid_until(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Option<EventTime>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let until: Option<String> = sqlx::query_scalar(
            "SELECT to_char(valid_until AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
             FROM key_previous_hashes WHERE tenant_id = $1 AND key_id = $2",
        )
        .bind(tenant_id)
        .bind(key_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        until
            .map(|u| EventTime::parse(&u).map_err(backend))
            .transpose()
    }

    async fn key_access(
        &self,
        tenant_id: Uuid,
        key_id: Uuid,
    ) -> Result<Option<KeyAccess>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        one_snapshot(&mut tx).await?;
        set_tenant(&mut tx, tenant_id).await?;
        let access = key_access_in(&mut tx, tenant_id, key_id).await?;
        tx.commit().await.map_err(backend)?;
        Ok(access)
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
            NamedKind::Workspace => {
                "SELECT id FROM workspaces WHERE tenant_id = $1 AND name = $2 \
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

    async fn resolve_user_workspace(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
        name: &Name,
    ) -> Result<Option<Uuid>, StoreError> {
        // One lookup on workspaces_user_name (ADR-0044 d5).
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM workspaces WHERE tenant_id = $1 AND namespace_root_id = $2 \
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
        // crew record, and its default workspace with the path the walk up
        // the parent_id links derives, root first, and whether that walk
        // reached a root (ADR-0039 d15).
        let row = sqlx::query(
            "WITH RECURSIVE k AS ( \
                 SELECT default_workspace_id, crew_id, user_root_id FROM api_keys \
                  WHERE tenant_id = $1 AND id = $2 \
             ), up AS ( \
                 SELECT w.id, w.parent_id, w.name, 0 AS depth \
                   FROM workspaces w JOIN k ON w.id = k.default_workspace_id \
                  WHERE w.tenant_id = $1 \
                 UNION ALL \
                 SELECT w.id, w.parent_id, w.name, up.depth + 1 \
                   FROM workspaces w JOIN up ON w.id = up.parent_id \
                  WHERE w.tenant_id = $1 \
             ) \
             SELECT t.name::text AS tenant_name, \
                    k.default_workspace_id, \
                    (SELECT up.name::text FROM up WHERE up.depth = 0) AS workspace_name, \
                    (SELECT string_agg(up.name::text, '.' ORDER BY up.depth DESC) FROM up) \
                        AS workspace_path, \
                    (SELECT bool_and(up.name IS NOT NULL) \
                            AND bool_or(up.parent_id IS NULL) FROM up) AS workspace_whole, \
                    k.crew_id, c.name::text AS crew_name, \
                    k.user_root_id, r.name::text AS user_name \
               FROM k \
               JOIN tenants t ON t.id = $1 \
               LEFT JOIN crews c ON c.tenant_id = $1 AND c.id = k.crew_id \
               LEFT JOIN workspaces r ON r.tenant_id = $1 AND r.id = k.user_root_id",
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
        let default_workspace = match row
            .try_get::<Option<Uuid>, _>("default_workspace_id")
            .map_err(backend)?
        {
            None => None,
            Some(workspace_id) => {
                // A default the tenant does not hold, or whose walk names a
                // row with no name or stops short of a root, fails closed,
                // as the ancestry read does.
                let whole: Option<bool> = row.try_get("workspace_whole").map_err(backend)?;
                let path: Option<String> = row.try_get("workspace_path").map_err(backend)?;
                match (whole, stored_name(&row, "workspace_name")?, path) {
                    (Some(true), Some(name), Some(path)) => Some(WorkspaceRef {
                        id: workspace_id,
                        name,
                        path,
                    }),
                    _ => {
                        return Err(StoreError::Backend(format!(
                            "key {key_id}'s default workspace {workspace_id} has no named \
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
            default_workspace,
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
        let workspaces: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM workspaces WHERE tenant_id = $1 AND name IS NULL ORDER BY id",
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
            workspaces,
            keys,
        })
    }

    async fn workspace(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<WorkspaceRecord>, StoreError> {
        let sql =
            format!("SELECT {WORKSPACE_COLUMNS} FROM workspaces WHERE tenant_id = $1 AND id = $2");
        Ok(self
            .workspace_rows(tenant_id, workspace_id, &sql)
            .await?
            .into_iter()
            .next())
    }

    async fn workspace_ancestry(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Option<Vec<WorkspaceRecord>>, StoreError> {
        // Up the parent_id links from the node; the depth orders the walk.
        let sql = format!(
            "WITH RECURSIVE up AS ( \
                 SELECT {WORKSPACE_COLUMNS}, 0 AS depth FROM workspaces \
                  WHERE tenant_id = $1 AND id = $2 \
                 UNION ALL \
                 SELECT w.id, w.tenant_id, w.parent_id, w.name, w.status, w.crew, w.crew_id, \
                        w.user_root, w.namespace_root_id, w.mode, w.public, \
                        w.public_act_id, up.depth + 1 \
                   FROM workspaces w JOIN up ON w.id = up.parent_id \
                  WHERE w.tenant_id = $1 \
             ) SELECT {WORKSPACE_COLUMNS} FROM up ORDER BY depth ASC"
        );
        let ancestry = self.workspace_rows(tenant_id, workspace_id, &sql).await?;
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
                "the ancestry of {workspace_id} names parent {parent_id}, \
                 which this tenant does not hold"
            )));
        }
        Ok(Some(ancestry))
    }

    async fn workspace_subtree(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Vec<WorkspaceRecord>, StoreError> {
        let sql = format!(
            "WITH RECURSIVE down AS ( \
                 SELECT {WORKSPACE_COLUMNS}, 0 AS depth FROM workspaces \
                  WHERE tenant_id = $1 AND id = $2 \
                 UNION ALL \
                 SELECT w.id, w.tenant_id, w.parent_id, w.name, w.status, w.crew, w.crew_id, \
                        w.user_root, w.namespace_root_id, w.mode, w.public, \
                        w.public_act_id, down.depth + 1 \
                   FROM workspaces w JOIN down ON w.parent_id = down.id \
                  WHERE w.tenant_id = $1 \
             ) SELECT {WORKSPACE_COLUMNS} FROM down ORDER BY depth ASC, id ASC"
        );
        self.workspace_rows(tenant_id, workspace_id, &sql).await
    }

    async fn public_workspaces(&self, tenant_id: Uuid) -> Result<Vec<WorkspaceRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {WORKSPACE_COLUMNS} FROM workspaces WHERE tenant_id = $1 AND public \
             ORDER BY name COLLATE \"C\" ASC, id ASC"
        ))
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(workspace_from_row).collect()
    }

    async fn workspace_children(
        &self,
        tenant_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<Vec<WorkspaceRecord>, StoreError> {
        // COLLATE "C" compares the UTF-8 bytes, whatever the database's
        // locale, and uuid compares its 16 bytes.
        let sql = format!(
            "SELECT {WORKSPACE_COLUMNS} FROM workspaces \
             WHERE tenant_id = $1 AND parent_id = $2 \
             ORDER BY name COLLATE \"C\" ASC, id ASC"
        );
        self.workspace_rows(tenant_id, workspace_id, &sql).await
    }

    async fn transition(
        &self,
        tenant_id: Uuid,
        transition_id: Uuid,
    ) -> Result<Option<TransitionRecord>, StoreError> {
        let sql = format!(
            "SELECT {TRANSITION_COLUMNS} FROM workspace_transitions \
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
        workspace_id: Uuid,
    ) -> Result<Vec<TransitionRecord>, StoreError> {
        // The index on (tenant_id, workspace_id, state) serves the filter.
        let sql = format!(
            "SELECT {TRANSITION_COLUMNS} FROM workspace_transitions \
             WHERE tenant_id = $1 AND workspace_id = $2 AND state = 'pending' \
             ORDER BY proposed_seq"
        );
        self.transition_rows(tenant_id, workspace_id, &sql).await
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
        workspace_id: Uuid,
    ) -> Result<Vec<BriefEdgeRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {BRIEF_EDGE_COLUMNS} FROM brief_edges e \
             JOIN api_keys k ON k.tenant_id = e.tenant_id AND k.id = e.proposer_key_id \
             WHERE e.tenant_id = $1 AND e.workspace_id = $2 \
             ORDER BY e.created_at DESC, e.id DESC"
        ))
        .bind(tenant_id)
        .bind(workspace_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(brief_edge_from_row).collect()
    }

    async fn pending_brief_edges(
        &self,
        tenant_id: Uuid,
        workspace_ids: &[Uuid],
        ratifier: (Uuid, &str),
        after: Option<(&EventTime, Uuid)>,
        limit: usize,
    ) -> Result<Vec<BriefEdgeRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {BRIEF_EDGE_COLUMNS} FROM brief_edges e \
             JOIN api_keys k ON k.tenant_id = e.tenant_id AND k.id = e.proposer_key_id \
             WHERE e.tenant_id = $1 AND e.workspace_id = ANY($2) AND e.state = 'pending' \
               AND e.proposer_key_id <> $3 AND k.principal <> $4 \
               AND ($5::text IS NULL OR (e.created_at, e.id) > (($5)::timestamptz, $6)) \
             ORDER BY e.created_at, e.id LIMIT $7"
        ))
        .bind(tenant_id)
        .bind(workspace_ids)
        .bind(ratifier.0)
        .bind(ratifier.1)
        .bind(after.map(|(at, _)| at.as_str().to_owned()))
        .bind(after.map(|(_, id)| id))
        .bind(i64::try_from(limit).map_err(backend)?)
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
            sql.push(format!(" AND {row}.workspace_id = ANY("))
                .push_bind(selection.readable_nodes.clone())
                .push(format!(
                    "::uuid[]) AND ({row}.status = 'promoted' OR {row}.author_key_id = "
                ))
                .push_bind(selection.reader_key_id)
                // Inside an in-progress scope, any author's held version
                // the scan did not hold (ADR-0030 d3; ADR-0065 d5, amended
                // 2026-11-10; ADR-0080 d16).
                .push(format!(
                    " OR ({row}.status = 'held' AND NOT {row}.retirement \
                     AND {row}.scan_finding IS NULL \
                     AND {row}.workspace_id = ANY("
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
            sql.push(" AND (s.workspace_id = ANY(")
                .push_bind(within.clone())
                .push("::uuid[]) OR d.workspace_id = ANY(")
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

/// An upload session's columns, its times in the signed fixed-width form
/// so `EventTime` round-trips exactly (ADR-0004 d3).
const UPLOAD_COLUMNS: &str = "tenant_id, id, key_id, purpose, size, sha256, received, state, \
     part_key, \
     to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at_signed, \
     to_char(touched_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS touched_at_signed, \
     to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS expires_at_signed, \
     to_char(completed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS completed_at_signed, \
     to_char(bound_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS bound_at_signed";

fn upload_from_row(row: &PgRow) -> Result<oml_app::types::UploadSession, StoreError> {
    let purpose: String = row.try_get("purpose").map_err(backend)?;
    let state: String = row.try_get("state").map_err(backend)?;
    let size: i64 = row.try_get("size").map_err(backend)?;
    let received: Vec<i64> = row.try_get("received").map_err(backend)?;
    let part_key: Option<Vec<u8>> = row.try_get("part_key").map_err(backend)?;
    let required = |column: &str| {
        event_time_column(row, column)?
            .ok_or_else(|| StoreError::Backend(format!("upload_sessions.{column} is null")))
    };
    Ok(oml_app::types::UploadSession {
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        upload_id: row.try_get("id").map_err(backend)?,
        key_id: row.try_get("key_id").map_err(backend)?,
        purpose: oml_app::domain::upload::UploadPurpose::parse(&purpose)
            .ok_or_else(|| StoreError::Backend(format!("upload purpose {purpose:?}")))?,
        size: u64::try_from(size).map_err(backend)?,
        sha256: row.try_get("sha256").map_err(backend)?,
        received: received
            .into_iter()
            .map(u64::try_from)
            .collect::<Result<_, _>>()
            .map_err(backend)?,
        state: oml_app::domain::upload::UploadState::parse(&state)
            .ok_or_else(|| StoreError::Backend(format!("upload state {state:?}")))?,
        part_key: part_key
            .map(|k| <[u8; 32]>::try_from(k.as_slice()))
            .transpose()
            .map_err(backend)?,
        created_at: required("created_at_signed")?,
        touched_at: required("touched_at_signed")?,
        expires_at: event_time_column(row, "expires_at_signed")?,
        completed_at: event_time_column(row, "completed_at_signed")?,
        bound_at: event_time_column(row, "bound_at_signed")?,
    })
}

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
/// Takes the tenant out of `sole_admin` for good once it holds two live
/// admin persons, after a fold on its allowlist (ADR-0097 d10; 0070).
async fn fold_sole_admin(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE tenants SET sole_admin = false WHERE id = $1 AND sole_admin \
         AND (SELECT count(DISTINCT user_name) FROM web_emails \
               WHERE tenant_id = $1 AND admin AND pending_since IS NULL \
                 AND disabled_at IS NULL) >= 2",
    )
    .bind(tenant_id)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    Ok(())
}

/// `admin.act`'s fold (ADR-0097 d2; 0070): the first envelope inserts the
/// act's row, effective or pending; a later one moves it only from a state
/// that admits the move, a lost race aborting the commit. A confirmation
/// clears the `pending_act_id` of the key the act issued (d23).
async fn fold_admin_act(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    act: &oml_app::types::AdminActRecord,
    seq: i64,
    at: &EventTime,
) -> Result<(), StoreError> {
    use oml_app::domain::admin_act::AdminActState as St;
    let from: &[&str] = match act.state {
        St::Effective | St::PendingConfirmation => {
            sqlx::query(
                "INSERT INTO admin_acts (tenant_id, id, act, class, target, reason, \
                        actor_key_id, actor_person, state, sole_admin, cause, created_at, \
                        effect, seq) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, ($12)::timestamptz, \
                        $13, $14)",
            )
            .bind(tenant_id)
            .bind(act.act_id)
            .bind(act.act.as_str())
            .bind(act.class().as_str())
            .bind(&act.target)
            .bind(act.reason.as_deref())
            .bind(act.actor_key_id)
            .bind(act.actor_person.as_ref().map(Name::as_str))
            .bind(act.state.as_str())
            .bind(act.sole_admin)
            .bind(
                act.cause
                    .map(oml_app::domain::admin_act::AdminActCause::as_str),
            )
            .bind(at.as_str())
            .bind(&act.effect)
            .bind(seq)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            return Ok(());
        }
        St::Confirmed | St::Expired => &["pending_confirmation"],
        St::Declined => &["pending_confirmation", "effective"],
        St::Contested => &["effective"],
        St::Settled => &["effective", "contested"],
    };
    let from: Vec<String> = from.iter().map(|s| (*s).to_owned()).collect();
    let moved = sqlx::query(
        "UPDATE admin_acts SET state = $3, confirmed_by_person = $4, settled_by_key_id = $5, \
                settled_at = ($6)::timestamptz \
          WHERE tenant_id = $1 AND id = $2 AND state = ANY($7) \
            AND ($3 <> 'declined' OR state = 'pending_confirmation' OR sole_admin)",
    )
    .bind(tenant_id)
    .bind(act.act_id)
    .bind(act.state.as_str())
    .bind(act.confirmed_by_person.as_ref().map(Name::as_str))
    .bind(act.settled_by_key_id)
    .bind(at.as_str())
    .bind(&from)
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
    if moved.rows_affected() != 1 {
        return Err(StoreError::Backend(format!(
            "admin act {} does not move to {} from the state it holds",
            act.act_id,
            act.state.as_str()
        )));
    }
    if act.state == St::Confirmed {
        sqlx::query(
            "UPDATE api_keys SET pending_act_id = NULL \
              WHERE tenant_id = $1 AND pending_act_id = $2",
        )
        .bind(tenant_id)
        .bind(act.act_id)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
    Ok(())
}

async fn last_admin_entry(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    hash: &[u8],
) -> Result<bool, StoreError> {
    let admins: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT email_hash FROM web_emails WHERE tenant_id = $1 AND admin \
         AND pending_since IS NULL AND disabled_at IS NULL ORDER BY email_hash FOR UPDATE",
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
    "terms_version, pending_since IS NOT NULL AS pending, ",
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
        pending: row.try_get("pending").map_err(backend)?,
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

impl PgStore {
    /// The tenant's admin fixes, oldest first: one by id or every one, the
    /// unconfirmed ones alone when `unconfirmed_only` (ADR-0089 d15; 0064).
    async fn read_admin_acts(
        &self,
        tenant_id: Uuid,
        act_id: Option<Uuid>,
        state: Option<oml_app::domain::admin_act::AdminActState>,
    ) -> Result<Vec<oml_app::types::AdminActRecord>, StoreError> {
        use oml_app::domain::admin_act::{AdminActCause, AdminActKind, AdminActState};
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(
            "SELECT id, act, target, reason, actor_key_id, actor_person::text AS actor_person, \
                    state, confirmed_by_person::text AS confirmed_by_person, settled_by_key_id, \
                    sole_admin, cause, effect, \
                    to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
                    AS created_at_s, \
                    to_char(settled_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
                    AS settled_at_s \
               FROM admin_acts \
              WHERE tenant_id = $1 AND ($2::uuid IS NULL OR id = $2) \
                AND ($3::text IS NULL OR state = $3) \
              ORDER BY created_at, id",
        )
        .bind(tenant_id)
        .bind(act_id)
        .bind(state.map(AdminActState::as_str))
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let name = |v: Option<String>| -> Result<Option<Name>, StoreError> {
            v.as_deref()
                .map(Name::parse_reference)
                .transpose()
                .map_err(backend)
        };
        rows.iter()
            .map(|row| {
                let act: String = row.try_get("act").map_err(backend)?;
                let st: String = row.try_get("state").map_err(backend)?;
                let cause: Option<String> = row.try_get("cause").map_err(backend)?;
                let created: String = row.try_get("created_at_s").map_err(backend)?;
                let settled: Option<String> = row.try_get("settled_at_s").map_err(backend)?;
                Ok(oml_app::types::AdminActRecord {
                    act_id: row.try_get("id").map_err(backend)?,
                    act: AdminActKind::parse(&act)
                        .ok_or_else(|| StoreError::Backend(format!("unknown admin act {act:?}")))?,
                    target: row.try_get("target").map_err(backend)?,
                    reason: row.try_get("reason").map_err(backend)?,
                    actor_key_id: row.try_get("actor_key_id").map_err(backend)?,
                    actor_person: name(row.try_get("actor_person").map_err(backend)?)?,
                    state: AdminActState::parse(&st).ok_or_else(|| {
                        StoreError::Backend(format!("unknown admin act state {st:?}"))
                    })?,
                    confirmed_by_person: name(
                        row.try_get("confirmed_by_person").map_err(backend)?,
                    )?,
                    settled_by_key_id: row.try_get("settled_by_key_id").map_err(backend)?,
                    sole_admin: row.try_get("sole_admin").map_err(backend)?,
                    cause: cause
                        .as_deref()
                        .map(|c| {
                            AdminActCause::parse(c).ok_or_else(|| {
                                StoreError::Backend(format!("unknown admin act cause {c:?}"))
                            })
                        })
                        .transpose()?,
                    at: EventTime::parse(&created).map_err(backend)?,
                    settled_at: settled
                        .as_deref()
                        .map(EventTime::parse)
                        .transpose()
                        .map_err(backend)?,
                    effect: row.try_get("effect").map_err(backend)?,
                })
            })
            .collect()
    }

    async fn read_fixes(
        &self,
        tenant_id: Uuid,
        fix_id: Option<Uuid>,
        unconfirmed_only: bool,
    ) -> Result<Vec<oml_app::types::FixRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(
            "SELECT id, act, target_id, workspace_id, claim_key, replaced_id, served_id, \
                    reason, loosens_guardrail, admin_key_id, lifecycle_event_id, state, \
                    to_char(fixed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
                    AS fixed_at_s, settled_by_key_id, settled_same_user, \
                    to_char(settled_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
                    AS settled_at_s, restored_id, admin_person, superseded_by \
               FROM admin_fixes \
              WHERE tenant_id = $1 AND ($2::uuid IS NULL OR id = $2) \
                AND (NOT $3 OR state = 'unconfirmed') \
              ORDER BY fixed_at, id",
        )
        .bind(tenant_id)
        .bind(fix_id)
        .bind(unconfirmed_only)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter()
            .map(|row| {
                let act: String = row.try_get("act").map_err(backend)?;
                let state: String = row.try_get("state").map_err(backend)?;
                let fixed: String = row.try_get("fixed_at_s").map_err(backend)?;
                let settled: Option<String> = row.try_get("settled_at_s").map_err(backend)?;
                Ok(oml_app::types::FixRecord {
                    fix_id: row.try_get("id").map_err(backend)?,
                    act: oml_app::types::FixAct::parse(&act)
                        .ok_or_else(|| StoreError::Backend(format!("unknown fix act {act:?}")))?,
                    target_id: row.try_get("target_id").map_err(backend)?,
                    workspace_id: row.try_get("workspace_id").map_err(backend)?,
                    claim_key: row.try_get("claim_key").map_err(backend)?,
                    replaced_id: row.try_get("replaced_id").map_err(backend)?,
                    served_id: row.try_get("served_id").map_err(backend)?,
                    reason: row.try_get("reason").map_err(backend)?,
                    loosens_guardrail: row.try_get("loosens_guardrail").map_err(backend)?,
                    admin_key_id: row.try_get("admin_key_id").map_err(backend)?,
                    admin_person: {
                        let person: String = row.try_get("admin_person").map_err(backend)?;
                        Name::parse_reference(&person).map_err(backend)?
                    },
                    lifecycle_event_id: row.try_get("lifecycle_event_id").map_err(backend)?,
                    state: oml_app::types::FixState::parse(&state).ok_or_else(|| {
                        StoreError::Backend(format!("unknown fix state {state:?}"))
                    })?,
                    fixed_at: EventTime::parse(&fixed).map_err(backend)?,
                    settled_by_key_id: row.try_get("settled_by_key_id").map_err(backend)?,
                    settled_same_user: row.try_get("settled_same_user").map_err(backend)?,
                    settled_at: settled
                        .as_deref()
                        .map(EventTime::parse)
                        .transpose()
                        .map_err(backend)?,
                    restored_id: row.try_get("restored_id").map_err(backend)?,
                    superseded_by: row.try_get("superseded_by").map_err(backend)?,
                })
            })
            .collect()
    }

    /// The tenant's admin investigations, oldest first: one by id, or
    /// `key_id`'s, or every one, the open ones alone when `open_only`
    /// (ADR-0089 d10, d15; 0063).
    async fn read_investigations(
        &self,
        tenant_id: Uuid,
        investigation_id: Option<Uuid>,
        key_id: Option<Uuid>,
        open_only: bool,
    ) -> Result<Vec<oml_app::types::Investigation>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows: Vec<(Uuid, Uuid, String, String, i64, Option<String>)> = sqlx::query_as(
            "SELECT id, key_id, reason, \
                    to_char(opened_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
                    reads, \
                    to_char(closed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
               FROM admin_investigations \
              WHERE tenant_id = $1 AND ($2::uuid IS NULL OR id = $2) \
                AND ($3::uuid IS NULL OR key_id = $3) AND (NOT $4 OR closed_at IS NULL) \
              ORDER BY opened_at, id",
        )
        .bind(tenant_id)
        .bind(investigation_id)
        .bind(key_id)
        .bind(open_only)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.into_iter()
            .map(|(id, key_id, reason, opened, reads, closed)| {
                Ok(oml_app::types::Investigation {
                    investigation_id: id,
                    key_id,
                    reason,
                    opened_at: EventTime::parse(&opened).map_err(backend)?,
                    reads: u64::try_from(reads).unwrap_or(0),
                    closed_at: closed
                        .as_deref()
                        .map(EventTime::parse)
                        .transpose()
                        .map_err(backend)?,
                })
            })
            .collect()
    }
}

/// The web rows (ADR-0045 d2, d4, d5): the allowlist's reads and its
/// pre-tenant lookup by the email's digest, the pin, and the session rows,
/// each in a transaction of its own under `oml.tenant_id`, the digest
/// lookups under 0015's `key_resolution` policies.
impl WebStore for PgStore {
    async fn removed_admin_act(
        &self,
        email_hash: &Sha256Hash,
    ) -> Result<Option<(Uuid, oml_app::types::AdminActRecord)>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_lookup_digest(&mut tx, email_hash).await?;
        // Before any tenant is known, the contest lookup policy admits the
        // admin act rows whose target names this digest, SELECT only
        // (migration 0073; ADR-0097 d8, the operator's decision of
        // 2026-11-27).
        let found: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT tenant_id, id FROM admin_acts \
              WHERE act = 'remove_admin' AND state = 'effective' \
                AND target->>'email_digest' = $1 \
              ORDER BY created_at DESC LIMIT 1",
        )
        .bind(email_hash.to_prefixed())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let Some((tenant_id, act_id)) = found else {
            return Ok(None);
        };
        Ok(self
            .read_admin_acts(tenant_id, Some(act_id), None)
            .await?
            .pop()
            .map(|act| (tenant_id, act)))
    }

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
        // A session opens only on an entry that waits on no one: a pending
        // entry signs in to nothing (ADR-0045 d2's note of 2026-11-19).
        let opened = sqlx::query(
            "INSERT INTO web_sessions (token_hash, tenant_id, email_hash, user_name, admin, \
             created_at, expires_at) \
             SELECT $1, $2, $3, $4, $5, ($6)::timestamptz, ($7)::timestamptz \
             WHERE NOT EXISTS (SELECT 1 FROM web_emails e WHERE e.tenant_id = $2 \
                               AND e.email_hash = $3 AND e.pending_since IS NOT NULL)",
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
        if opened.rows_affected() != 1 {
            return Err(StoreError::Backend(
                "web_sessions: the entry waits on its person's acceptance".to_owned(),
            ));
        }
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

    /// The session gate's rows in one transaction under one snapshot
    /// (ADR-0070 d8, #297): each statement is the one its separate read
    /// runs, under the same fences, so the rows are those the separate
    /// reads return when nothing changes between them.
    async fn web_credential_rows(
        &self,
        credential_hash: &Sha256Hash,
        key_name: SessionKeyName,
    ) -> Result<Option<WebCredentialRows>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        one_snapshot(&mut tx).await?;
        // As `web_session_credential`: the digest's row, then the session
        // under the matched row's tenant.
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
        set_tenant(&mut tx, tenant_id).await?;
        let session = sqlx::query(&format!(
            "SELECT {WEB_SESSION_COLUMNS} FROM web_sessions WHERE tenant_id = $1 AND token_hash = $2"
        ))
        .bind(tenant_id)
        .bind(token_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(session) = session.as_ref().map(web_session_from_row).transpose()? else {
            tx.commit().await.map_err(backend)?;
            return Ok(None);
        };
        // As `resolve_name` then `key`: the row holding the name, a revoked
        // key's included, on the unique name index.
        let key = match key_name(&session.user, &capacity) {
            Some(name) => sqlx::query(&format!(
                "SELECT {KEY_COLUMNS} FROM api_keys WHERE tenant_id = $1 AND name = $2"
            ))
            .bind(tenant_id)
            .bind(name.as_str())
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?
            .as_ref()
            .map(key_from_row)
            .transpose()?,
            None => None,
        };
        let key = match key {
            Some(key) => {
                let access = key_access_in(&mut tx, tenant_id, key.key_id).await?;
                let user_root = match key.user {
                    // As `workspace`.
                    Some(root) => sqlx::query(&format!(
                        "SELECT {WORKSPACE_COLUMNS} FROM workspaces \
                         WHERE tenant_id = $1 AND id = $2"
                    ))
                    .bind(tenant_id)
                    .bind(root)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(backend)?
                    .as_ref()
                    .map(workspace_from_row)
                    .transpose()?,
                    None => None,
                };
                // As `person_keys_read_apart`: the person's session keys,
                // as `web_session_keys`, then the keys naming the root,
                // as `web_user_keys`.
                let session_keys = match &key.session_user {
                    Some(user) => sqlx::query(&format!(
                        "SELECT {KEY_COLUMNS} FROM api_keys \
                         WHERE tenant_id = $1 AND session_for = $2 ORDER BY name"
                    ))
                    .bind(tenant_id)
                    .bind(user.as_str())
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(backend)?
                    .iter()
                    .map(key_from_row)
                    .collect::<Result<Vec<_>, StoreError>>()?,
                    None => Vec::new(),
                };
                let root = match &key.session_user {
                    Some(_) => key
                        .user
                        .or_else(|| session_keys.iter().find_map(|k| k.user)),
                    None => key.user,
                };
                let user_keys = match root {
                    Some(root) => sqlx::query(&format!(
                        "SELECT {KEY_COLUMNS} FROM api_keys \
                         WHERE tenant_id = $1 AND user_root_id = $2 ORDER BY name"
                    ))
                    .bind(tenant_id)
                    .bind(root)
                    .fetch_all(&mut *tx)
                    .await
                    .map_err(backend)?
                    .iter()
                    .map(key_from_row)
                    .collect::<Result<Vec<_>, StoreError>>()?,
                    None => Vec::new(),
                };
                let own_keys = oml_app::ports::person_key_set(user_keys, session_keys);
                Some(WebCredentialKey {
                    key,
                    access,
                    user_root,
                    own_keys,
                })
            }
            None => None,
        };
        tx.commit().await.map_err(backend)?;
        Ok(Some(WebCredentialRows {
            session,
            capacity,
            key,
        }))
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

    async fn web_linked_keys(
        &self,
        tenant_id: Uuid,
        user_root: Uuid,
    ) -> Result<Vec<KeyRecord>, StoreError> {
        self.keys_where(
            tenant_id,
            "(linked_user_id = $2 OR link_pending_user_id = $2)",
            |q| q.bind(user_root),
        )
        .await
    }

    async fn web_unlinked_admin_keys(&self, tenant_id: Uuid) -> Result<Vec<KeyRecord>, StoreError> {
        self.keys_where(
            tenant_id,
            "revoked_at IS NULL AND role = 'admin' AND credential = 'bearer' \
             AND linked_user_id IS NULL",
            |q| q,
        )
        .await
    }

    async fn web_pending_links(&self, tenant_id: Uuid) -> Result<Vec<KeyRecord>, StoreError> {
        self.keys_where(
            tenant_id,
            "revoked_at IS NULL AND link_pending_user_id IS NOT NULL",
            |q| q,
        )
        .await
    }

    async fn web_pending_emails(&self, tenant_id: Uuid) -> Result<Vec<WebEntry>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        let rows = sqlx::query(&format!(
            "SELECT {WEB_ENTRY_COLUMNS} FROM web_emails \
             WHERE tenant_id = $1 AND pending_since IS NOT NULL ORDER BY email"
        ))
        .bind(tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(web_entry_from_row).collect()
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
        // The instruction class is read off the domain, so a kind added to it
        // is counted without touching this read (ADR-0080 d16; #435).
        let instructions: Vec<&str> = MemoryKind::ALL
            .iter()
            .filter(|k| k.class().needs_person())
            .map(|k| k.as_str())
            .collect();
        let rows = sqlx::query(concat!(
            "SELECT workspace_id, count(*) AS held, \
             count(*) FILTER (WHERE kind = ANY($2::text[])) AS held_instructions, ",
            utc!("min(created_at)"),
            " AS oldest_signed FROM memories WHERE tenant_id = $1 AND status = 'held' \
             GROUP BY workspace_id ORDER BY workspace_id"
        ))
        .bind(tenant_id)
        .bind(&instructions)
        .fetch_all(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter()
            .map(|row| {
                let held: i64 = row.try_get("held").map_err(backend)?;
                let held_instructions: i64 = row.try_get("held_instructions").map_err(backend)?;
                Ok(QueueDepth {
                    workspace_id: row.try_get("workspace_id").map_err(backend)?,
                    held: u64::try_from(held).map_err(backend)?,
                    held_instructions: u64::try_from(held_instructions).map_err(backend)?,
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
            Some(AdminState::Withdrawn) => "m.status = 'withdrawn'",
        };
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m WHERE m.tenant_id = $1 \
             AND ($2::text IS NULL OR to_tsvector('simple', COALESCE(m.search_text, m.claim)) @@ plainto_tsquery('simple', $2)) \
             AND ($3::uuid[] IS NULL OR m.workspace_id = ANY($3)) \
             AND {state} \
             AND ($4::timestamptz IS NULL OR (m.created_at, m.id) < ($4::timestamptz, $5::uuid)) \
             AND (cardinality($7::text[]) = 0 OR m.origin_kind = ANY($7)) \
             AND ($8::uuid IS NULL OR m.author_key_id = $8) \
             AND (cardinality($9::text[]) = 0 OR m.kind = ANY($9)) \
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
            .bind(
                search
                    .kinds
                    .iter()
                    .map(|k| k.as_str().to_owned())
                    .collect::<Vec<_>>(),
            )
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
        // The key's state at `at`: revoked, else past its expiry, else
        // active; a key the tenant does not hold reads as revoked
        // (ADR-0031 d11; ADR-0079 d2).
        let key: Option<(bool, bool)> = sqlx::query_as(
            "SELECT revoked_at IS NOT NULL, \
                    expires_at IS NOT NULL AND expires_at <= ($3)::timestamptz \
               FROM api_keys WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant_id)
        .bind(connection.key_id)
        .bind(at.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let facts = RefreshFacts {
            connection_id,
            connection: connection.state,
            client_id: connection.client_id,
            resource: connection.resource,
            key: match key {
                Some((false, false)) => KeyState::Active,
                Some((false, true)) => KeyState::Expired,
                Some((true, _)) | None => KeyState::Revoked,
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

#[cfg(test)]
mod prefix_upper_tests {
    use super::prefix_upper;

    /// The bound is the prefix with its last character stepped to the next
    /// scalar value, across multi-byte characters, over the surrogate gap
    /// after U+D7FF, and past a last character of U+10FFFF, which has none
    /// and is dropped first; every key that starts with the prefix sorts
    /// below it.
    #[test]
    fn prefix_upper_steps_the_last_scalar_value_over_the_surrogates_and_past_the_last_one() {
        for (prefix, upper) in [
            ("skill/", Some("skill0")),
            ("doc/r\u{e9}", Some("doc/r\u{ea}")),
            ("\u{65e5}\u{672c}", Some("\u{65e5}\u{672d}")),
            ("a\u{d7ff}", Some("a\u{e000}")),
            ("a\u{10ffff}", Some("b")),
            ("a\u{10ffff}\u{10ffff}", Some("b")),
            ("\u{10ffff}", None),
        ] {
            assert_eq!(prefix_upper(prefix).as_deref(), upper, "{prefix:?}");
        }
        for (prefix, key) in [
            ("a\u{d7ff}", "a\u{d7ff}\u{10ffff}"),
            ("a\u{10ffff}", "a\u{10ffff}\u{10ffff}z"),
            ("doc/r\u{e9}", "doc/r\u{e9}sum\u{e9}"),
        ] {
            let upper = prefix_upper(prefix).expect("a bound");
            assert!(
                key.starts_with(prefix) && key < upper.as_str(),
                "{key:?} < {upper:?}"
            );
        }
    }
}
