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

use oml_app::domain::access::{Grant, KeyDenial};
use oml_app::domain::name::{Name, NamedKind, Naming, UNIVERSE};
use oml_app::domain::{
    ChainHead, Envelope, EventTime, HeldBy, KeySet, MemoryKind, MemoryStatus, Role, Sha256Hash,
    SignedEnvelope, VerifyError, verify_chain,
};
use oml_app::ed25519_dalek::Signature;
use oml_app::ports::{
    CommitReceipt, CommitRequest, IdempotencyRecord, IdempotencyScope, JournalStore, MetadataFold,
    StoreError, SubjectRow, TailEvent, TailScan, claim_subject,
};
use oml_app::types::{
    ClaimKeyState, CrewRecord, DenialRecord, GrantRecord, HIT_KINDS, KeyAccess, KeyRecord,
    ListingFilter, ListingPosition, ListingScope, MemoryRecord, PROVIDER_TENANT, RecallCandidate,
    UnnamedRows, WorkareaRecord,
};
use serde_json::{Value, json};
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::{Postgres, QueryBuilder, Row, Transaction};
use uuid::Uuid;

/// The committed migrations, embedded at build time. The migrations are
/// normative in bytes (postgres-schema.md "Migrations"); the server/bootstrap
/// calls [`PgStore::migrate`] (or runs this migrator) before serving.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../../migrations");

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

/// The Postgres `JournalStore`.
#[derive(Debug, Clone)]
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    /// Connects a pool to `database_url` (no migration side effect; call
    /// [`Self::migrate`] where bootstrap wants it).
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    #[must_use]
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Applies any pending committed migrations (forward-only,
    /// postgres-schema.md "Migrations").
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        MIGRATOR.run(&self.pool).await
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
    /// `workareas` rows whose `name` is NULL. 0006 adds the column NULL on
    /// every row it finds and every creation since names its row, so a NULL
    /// here is a row the lead names before it serves (ADR-0026 d11;
    /// http-api.md "Rows created before names"). `api_keys` is not read:
    /// the HTTP issuance body carries no name yet (ADR-0026 d9), so a key
    /// with no name is every key, not a sign of a row that predates names,
    /// and every key that predates names belongs to a tenant this check
    /// counts.
    pub async fn rows_without_names(
        &self,
        tenants: &[Uuid],
    ) -> Result<Vec<(&'static str, u64)>, StoreError> {
        let (mut tenant_rows, mut workarea_rows) = (0_i64, 0_i64);
        for tenant_id in tenants {
            let mut tx = self.pool.begin().await.map_err(backend)?;
            set_tenant(&mut tx, *tenant_id).await?;
            let row = sqlx::query(
                "SELECT (SELECT count(*) FROM tenants WHERE id = $1 AND name IS NULL) \
                        AS tenants, \
                        (SELECT count(*) FROM workareas WHERE tenant_id = $1 AND name IS NULL) \
                        AS workareas",
            )
            .bind(tenant_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
            tenant_rows += row.try_get::<i64, _>("tenants").map_err(backend)?;
            workarea_rows += row.try_get::<i64, _>("workareas").map_err(backend)?;
        }
        [("tenants", tenant_rows), ("workareas", workarea_rows)]
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
}

// ---------------------------------------------------------------------------
// Small conversions between the port's types and the schema's columns
// ---------------------------------------------------------------------------

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
     m.lineage_principals, m.lineage_key_ids, m.scan_finding, \
     to_char(m.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS created_at_signed, \
     to_char(m.promoted_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS promoted_at_signed";

fn memory_from_row(row: &PgRow) -> Result<MemoryRecord, StoreError> {
    let status: String = row.try_get("status").map_err(backend)?;
    let kind: String = row.try_get("kind").map_err(backend)?;
    let claim_digest: Vec<u8> = row.try_get("claim_digest").map_err(backend)?;
    let created: String = row.try_get("created_at_signed").map_err(backend)?;
    let promoted: Option<String> = row.try_get("promoted_at_signed").map_err(backend)?;
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
        created_at: EventTime::parse(&created).map_err(backend)?,
    })
}

/// A recall candidate from a row of `id`, `body_bytes` and `score`.
fn candidate_from_row(row: &PgRow) -> Result<RecallCandidate, StoreError> {
    Ok(RecallCandidate {
        memory_id: row.try_get("id").map_err(backend)?,
        body_bytes: unsigned(row, "body_bytes")?,
        score: row.try_get("score").map_err(backend)?,
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
const WORKAREA_COLUMNS: &str = "id, tenant_id, parent_id, name, crew, crew_id";

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
        crew: crew
            .map(serde_json::from_value)
            .transpose()
            .map_err(backend)?,
        crew_id: row.try_get("crew_id").map_err(backend)?,
    })
}

/// The `api_keys` columns a key read selects.
const KEY_COLUMNS: &str =
    "id, tenant_id, role, principal, held_by, default_workarea_id, issued_by, hash, name, crew_id";

/// A key row as the port's record. A provider-level row names no tenant
/// (postgres-schema.md api_keys): it resolves under the provider tenant, so
/// its refusals are the use-cases' own, never a storage failure (ADR-0025
/// d13).
fn key_from_row(row: &PgRow) -> Result<KeyRecord, StoreError> {
    let tenant_id: Option<Uuid> = row.try_get("tenant_id").map_err(backend)?;
    let role: String = row.try_get("role").map_err(backend)?;
    let held_by: String = row.try_get("held_by").map_err(backend)?;
    let stored_hash: Vec<u8> = row.try_get("hash").map_err(backend)?;
    Ok(KeyRecord {
        key_id: row.try_get("id").map_err(backend)?,
        tenant_id: tenant_id.unwrap_or(PROVIDER_TENANT),
        role: role_from_str(&role)?,
        principal: row.try_get("principal").map_err(backend)?,
        held_by: held_by_from_str(&held_by)?,
        default_workarea: row.try_get("default_workarea_id").map_err(backend)?,
        issued_by: row.try_get("issued_by").map_err(backend)?,
        key_hash: format!("sha256:{}", hex::encode(stored_hash)),
        name: stored_name(row, "name")?,
        crew_id: row.try_get("crew_id").map_err(backend)?,
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
    sqlx::query(
        "INSERT INTO memories (id, tenant_id, workarea_id, claim, claim_digest, status, \
         created_event_id, created_at, claim_key, author_principal, \
         author_key_id, derives_from, lineage_principals, lineage_key_ids, scan_finding, \
         version, kind, body_bytes, is_tip, promoted_at, retirement, replaces_artifact_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, ($8)::timestamptz, $9, $10, $11, $12, $13, $14, \
         $15, $16, $17, $18, $19, ($20)::timestamptz, $21, $22)",
    )
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
    .execute(&mut **tx)
    .await
    .map_err(backend)?;
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

/// Ratification of a first version: it is promoted, made the tip and
/// stamped with the event's `at`; `memories_served_tip` refuses a second tip
/// of its claim (ADR-0025 d10).
async fn promote_memory(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    memory_id: Uuid,
    at: &EventTime,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE memories SET status = 'promoted', is_tip = true, \
         promoted_at = ($3)::timestamptz WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(memory_id)
    .bind(at.as_str())
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
    at: &EventTime,
) -> Result<(), StoreError> {
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
    promote_memory(tx, tenant_id, successor, at).await
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
                    "workareas_tenant_id_name"
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
/// event seq and time: a key's `issued_seq`, a grant's or denial's epoch and
/// a memory's `promoted_at`.
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
        MetadataFold::TenantGenesis { .. } | MetadataFold::VerdictRecorded { .. } => {}
        MetadataFold::WorkareaCreated(w) => {
            // No status transition is built, so the status stored is the one
            // the workarea read serves: the universe root's permanent
            // 'committed', every other node's 'development'. The path is the
            // parent's with this node's name appended, the root's its name
            // alone (ADR-0026 d7); a parent the lead has not named yet has
            // no path, and neither does its child until the naming fold.
            let crew = w
                .crew
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .map_err(backend)?;
            sqlx::query(
                "INSERT INTO workareas (id, tenant_id, parent_id, name, path, status, crew, \
                 crew_id) VALUES ($1, $2, $3, $4, \
                 CASE WHEN $3::uuid IS NULL THEN text2ltree($4) \
                      ELSE (SELECT p.path FROM workareas p \
                             WHERE p.tenant_id = $2 AND p.id = $3) || text2ltree($4) END, \
                 $5, $6, $7)",
            )
            .bind(w.workarea_id)
            .bind(w.tenant_id)
            .bind(w.parent_id)
            .bind(w.name.as_str())
            .bind(w.status())
            .bind(crew)
            .bind(w.crew_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                // A non-root `universe` meets the CHECK that ties the name
                // to the root before the unique index, and errors.md
                // answers it as `name.taken`; a root of another name lost
                // no name, so its refusal stays the backend's
                // (postgres-schema.md `workareas`; ADR-0026 d5, d6).
                let constraint = if w.parent_id.is_some() && w.name.as_str() == UNIVERSE {
                    "workareas_universe_is_root"
                } else {
                    "workareas_tenant_id_name"
                };
                name_taken_or_backend(e, constraint, NamedKind::Workarea, &w.name)
            })?;
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
        MetadataFold::NamesAssigned(naming) => {
            fold_names_assigned(tx, tenant_id, at, &naming).await?;
        }
        MetadataFold::KeyIssued(k) => {
            let hash = key_hash_bytes(&k.key_hash).ok_or_else(|| {
                StoreError::Backend(format!(
                    "key_hash {:?} is not a sha256:<hex> string",
                    k.key_hash
                ))
            })?;
            sqlx::query(
                "INSERT INTO api_keys (id, tenant_id, hash, role, principal, held_by, \
                 default_workarea_id, issued_by, issued_seq, name, crew_id) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            )
            .bind(k.key_id)
            .bind(k.tenant_id)
            .bind(hash)
            .bind(k.role.as_str())
            .bind(&k.principal)
            .bind(k.held_by.as_str())
            .bind(k.default_workarea)
            .bind(k.issued_by)
            .bind(seq)
            .bind(k.name.as_ref().map(Name::as_str))
            .bind(k.crew_id)
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
                "INSERT INTO grants (id, tenant_id, key_id, role, workarea_node_id, deny, epoch) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(grant.grant_id)
            .bind(tenant_id)
            .bind(key_id)
            .bind(grant.role.as_str())
            .bind(grant.node)
            .bind(deny_to_json(&grant.deny))
            .bind(seq)
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
            // The row stays as history; key_by_hash stops resolving it. A key
            // the tenant does not hold matches no row under tenant_isolation,
            // and that aborts the commit rather than journalling a
            // revocation of nothing.
            let revoked = sqlx::query(
                "UPDATE api_keys SET revoked_at = now() \
                 WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL",
            )
            .bind(tenant_id)
            .bind(key_id)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
            if revoked.rows_affected() != 1 {
                return Err(StoreError::Backend(format!(
                    "key {key_id} is not a live key of this tenant"
                )));
            }
        }
        MetadataFold::MemoryHeld { memory, enqueue } => {
            insert_memory(tx, event_id, &memory).await?;
            if enqueue {
                enqueue_review(tx, tenant_id, memory.memory_id).await?;
            }
        }
        MetadataFold::MemoryPromoted { memory_id } => {
            promote_memory(tx, tenant_id, memory_id, at).await?;
        }
        MetadataFold::MemoryRejected { memory_id } => {
            set_memory_status(tx, tenant_id, memory_id, MemoryStatus::Rejected).await?;
        }
        MetadataFold::TipSuperseded {
            successor,
            replaced,
        } => {
            supersede_tip(tx, tenant_id, successor, replaced, at).await?;
        }
    }
    Ok(())
}

/// Appends one event inside the commit's transaction if it extends
/// `expected`, the (seq, prev_hash) the head calls for: the journal row, the
/// chain-head upsert, the metadata fold, the outbox row and the idempotency
/// record (ADR-0002 d1).
async fn append_event(
    tx: &mut Transaction<'_, Postgres>,
    (expected_seq, expected_prev): (u64, Sha256Hash),
    req: CommitRequest,
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

    apply_fold(tx, tenant_id, env.event_id, seq, &env.at, req.fold).await?;

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

    Ok(CommitReceipt {
        seq: env.seq,
        hash: req.event.hash,
    })
}

// ---------------------------------------------------------------------------
// The port implementation
// ---------------------------------------------------------------------------

impl JournalStore for PgStore {
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
        let mut receipt = append_event(&mut tx, expected, first).await?;
        for req in reqs {
            if req.event.envelope.tenant_id != tenant_id {
                return Err(StoreError::Backend(
                    "a chain commit names one tenant".to_owned(),
                ));
            }
            receipt = append_event(&mut tx, (receipt.seq + 1, receipt.hash), req).await?;
        }

        tx.commit().await.map_err(backend)?;
        Ok(receipt)
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

    async fn recall_fts(
        &self,
        tenant_id: Uuid,
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> Result<Vec<RecallCandidate>, StoreError> {
        let k = i64::try_from(k).unwrap_or(i64::MAX);
        let kinds = hit_kinds(kinds);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // Served tips only, never a retirement version, so neither a
        // replaced version nor a retired claim is a hit (retrieval.md R3 at
        // the store; ADR-0025 d8), within the readable set alone — an empty
        // array matches nothing (ADR-0025 d1); the 'simple' configuration
        // matches 0001's GIN expression index. The score is ts_rank with
        // normalization 0 as float4, ties broken by id. A candidate carries
        // no body: the served rows hydrate after the cuts (ADR-0025 d7).
        let sql = "SELECT m.id, m.body_bytes, \
                    ts_rank(to_tsvector('simple', m.claim), \
                            plainto_tsquery('simple', $3), 0) AS score \
             FROM memories m \
             WHERE m.tenant_id = $1 AND m.status = 'promoted' AND m.is_tip \
               AND NOT m.retirement AND m.kind = ANY($4::text[]) \
               AND m.workarea_id = ANY($2::uuid[]) \
               AND to_tsvector('simple', m.claim) @@ plainto_tsquery('simple', $3) \
             ORDER BY score DESC, m.id ASC \
             LIMIT $5";
        let rows = sqlx::query(sql)
            .bind(tenant_id)
            .bind(readable)
            .bind(query)
            .bind(&kinds)
            .bind(k)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(candidate_from_row).collect()
    }

    async fn recall_own_held(
        &self,
        tenant_id: Uuid,
        author_key_ids: &[Uuid],
        readable: &[Uuid],
        query: &str,
        kinds: &[MemoryKind],
        k: usize,
    ) -> Result<Vec<RecallCandidate>, StoreError> {
        let k = i64::try_from(k).unwrap_or(i64::MAX);
        let kinds = hit_kinds(kinds);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // The PoC form of held text containment: the statement hard-codes
        // the held state and the author key set, so no other key's held text
        // is matched whatever the query (retrieval.md; ADR-0025 d5). The
        // designed form reads `held_fts`, rows that exist only while held,
        // behind an RLS author policy over `oml.own_key_ids` set from the
        // authenticated key and its rotation chain; that table is decided,
        // not built, because the PoC stores bodies inline. Never a steering
        // row or a retirement version, never off the readable nodes, and the
        // score is the same ts_rank as recall_fts, so the two lists merge
        // under one order (ADR-0025 d4).
        let sql = "SELECT m.id, m.body_bytes, \
                    ts_rank(to_tsvector('simple', m.claim), \
                            plainto_tsquery('simple', $4), 0) AS score \
             FROM memories m \
             WHERE m.tenant_id = $1 AND m.status = 'held' \
               AND m.author_key_id = ANY($2::uuid[]) \
               AND NOT m.retirement AND m.kind = ANY($5::text[]) \
               AND m.workarea_id = ANY($3::uuid[]) \
               AND to_tsvector('simple', m.claim) @@ plainto_tsquery('simple', $4) \
             ORDER BY score DESC, m.id ASC \
             LIMIT $6";
        let rows = sqlx::query(sql)
            .bind(tenant_id)
            .bind(author_key_ids)
            .bind(readable)
            .bind(query)
            .bind(&kinds)
            .bind(k)
            .fetch_all(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        rows.iter().map(candidate_from_row).collect()
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
            ListingScope::Tips { nodes } => {
                sql.push(" AND m.status = 'promoted' AND m.is_tip AND m.workarea_id = ANY(")
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
        Ok(ClaimKeyState {
            greatest_version: greatest
                .map(|v| {
                    u32::try_from(v)
                        .map_err(|_| StoreError::Backend(format!("stored version {v} is negative")))
                })
                .transpose()?,
            live: row.try_get("live").map_err(backend)?,
        })
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
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM review_queue q \
             JOIN memories m ON m.id = q.memory_id \
             WHERE q.tenant_id = $1 \
               AND m.workarea_id = ANY($4::uuid[]) \
               AND (q.held_by_key IS NULL OR q.held_by_key = $2) \
               AND NOT (m.lineage_principals @> ARRAY[$3]::text[]) \
               AND NOT (m.lineage_key_ids @> ARRAY[$2]::uuid[]) \
               AND NOT EXISTS ( \
                   SELECT 1 FROM memories r \
                   WHERE r.tenant_id = $1 AND r.id <> m.id AND r.status = 'held' \
                     AND r.replaces_artifact_id = m.replaces_artifact_id \
                     AND r.workarea_id = m.workarea_id AND r.claim_key = m.claim_key \
                     AND (r.author_key_id = $2 OR r.author_principal = $3)) \
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
            "SELECT id, role, workarea_node_id, deny, epoch FROM grants \
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
                Ok(GrantRecord {
                    grant: Grant {
                        grant_id: row.try_get("id").map_err(backend)?,
                        node: row.try_get("workarea_node_id").map_err(backend)?,
                        role: role_from_str(&role)?,
                        deny: deny_from_json(&deny)?,
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
            NamedKind::Workarea => "SELECT id FROM workareas WHERE tenant_id = $1 AND name = $2",
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
                 SELECT w.id, w.tenant_id, w.parent_id, w.name, w.crew, w.crew_id, \
                        up.depth + 1 \
                   FROM workareas w JOIN up ON w.id = up.parent_id \
                  WHERE w.tenant_id = $1 \
             ) SELECT {WORKAREA_COLUMNS} FROM up ORDER BY depth ASC"
        );
        let ancestry = self.workarea_rows(tenant_id, workarea_id, &sql).await?;
        let Some(top) = ancestry.last() else {
            return Ok(None);
        };
        // A walk that stops short of a root left the tenant (a parent_id the
        // foreign key admits across tenants); it would drop ancestors'
        // grants and denials, so it is refused.
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
                 SELECT w.id, w.tenant_id, w.parent_id, w.name, w.crew, w.crew_id, \
                        down.depth + 1 \
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

    async fn idempotency_get(
        &self,
        scope: &IdempotencyScope,
    ) -> Result<Option<IdempotencyRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, scope.tenant_id).await?;
        let row = sqlx::query(
            "SELECT request_digest, response FROM idempotency \
             WHERE tenant_id = $1 AND key = $2",
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
}
