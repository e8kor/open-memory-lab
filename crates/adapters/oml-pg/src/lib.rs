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

use oml_app::domain::{
    ChainHead, Envelope, EventTime, HeldBy, KeySet, MemoryStatus, Role, Sha256Hash, SignedEnvelope,
    VerifyError, verify_chain,
};
use oml_app::ed25519_dalek::Signature;
use oml_app::ports::{
    CommitReceipt, CommitRequest, IdempotencyRecord, IdempotencyScope, JournalStore, MetadataFold,
    StoreError,
};
use oml_app::types::{KeyRecord, MemoryRecord};
use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use sqlx::{Postgres, Row, Transaction};
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
}

// ---------------------------------------------------------------------------
// Small conversions between the port's types and the schema's columns
// ---------------------------------------------------------------------------

fn backend<E: std::fmt::Display>(e: E) -> StoreError {
    StoreError::Backend(e.to_string())
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

fn status_from_str(s: &str) -> Result<MemoryStatus, StoreError> {
    match s {
        "held" => Ok(MemoryStatus::Held),
        "promoted" => Ok(MemoryStatus::Promoted),
        "rejected" => Ok(MemoryStatus::Rejected),
        "superseded" => Ok(MemoryStatus::Superseded),
        other => Err(StoreError::Backend(format!(
            "unknown memory status {other:?}"
        ))),
    }
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

/// The m-qualified memory column list every memory read shares. `created_at`
/// comes back through `to_char` in the fixed-width signed form so
/// `EventTime` round-trips exactly (timestamptz(6) holds the microseconds).
const MEMORY_COLUMNS: &str = "m.id, m.tenant_id, m.workarea_id, m.claim_key, m.claim, \
     m.claim_digest, m.status, m.author_principal, m.author_key_id, m.derives_from, \
     m.lineage_principals, m.lineage_key_ids, m.superseded_by, m.scan_finding, \
     to_char(m.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
     AS created_at_signed";

fn memory_from_row(row: &PgRow) -> Result<MemoryRecord, StoreError> {
    let status: String = row.try_get("status").map_err(backend)?;
    let claim_digest: Vec<u8> = row.try_get("claim_digest").map_err(backend)?;
    let created: String = row.try_get("created_at_signed").map_err(backend)?;
    Ok(MemoryRecord {
        memory_id: row.try_get("id").map_err(backend)?,
        tenant_id: row.try_get("tenant_id").map_err(backend)?,
        workarea_id: row.try_get("workarea_id").map_err(backend)?,
        claim_key: row.try_get("claim_key").map_err(backend)?,
        body: row.try_get("claim").map_err(backend)?,
        body_digest: hash32(&claim_digest)?.to_prefixed(),
        status: status_from_str(&status)?,
        author_principal: row.try_get("author_principal").map_err(backend)?,
        author_key_id: row.try_get("author_key_id").map_err(backend)?,
        derives_from: row.try_get("derives_from").map_err(backend)?,
        lineage_principals: row.try_get("lineage_principals").map_err(backend)?,
        lineage_key_ids: row.try_get("lineage_key_ids").map_err(backend)?,
        superseded_by: row.try_get("superseded_by").map_err(backend)?,
        scan_finding: row.try_get("scan_finding").map_err(backend)?,
        created_at: EventTime::parse(&created).map_err(backend)?,
    })
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
    sqlx::query(
        "INSERT INTO memories (id, tenant_id, workarea_id, claim, claim_digest, status, \
         created_event_id, superseded_by, created_at, claim_key, author_principal, \
         author_key_id, derives_from, lineage_principals, lineage_key_ids, scan_finding) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, ($9)::timestamptz, $10, $11, $12, $13, $14, \
         $15, $16)",
    )
    .bind(m.memory_id)
    .bind(m.tenant_id)
    .bind(m.workarea_id)
    .bind(&m.body)
    .bind(digest.0.to_vec())
    .bind(m.status.as_str())
    .bind(created_event_id)
    .bind(m.superseded_by)
    .bind(m.created_at.as_str())
    .bind(m.claim_key.as_deref())
    .bind(&m.author_principal)
    .bind(m.author_key_id)
    .bind(m.derives_from.clone())
    .bind(m.lineage_principals.clone())
    .bind(m.lineage_key_ids.clone())
    .bind(m.scan_finding.as_deref())
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

/// Applies the metadata fold rows of one commit (ADR-0002 d1: same
/// transaction as the journal insert; `memories.created_event_id` references
/// the journal row, so this runs after it).
async fn apply_fold(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    event_id: Uuid,
    fold: MetadataFold,
) -> Result<(), StoreError> {
    match fold {
        // The tenants row is upserted before the journal insert (FK order);
        // a verdict short of the policy's k is journalled, metadata unchanged.
        MetadataFold::TenantGenesis | MetadataFold::VerdictRecorded { .. } => {}
        MetadataFold::WorkareaCreated(w) => {
            // 0001's ladder floor: the PoC WorkareaRecord carries no status,
            // so every node lands 'development' (workareas.md ladder start).
            sqlx::query(
                "INSERT INTO workareas (id, tenant_id, parent_id, name, status) \
                 VALUES ($1, $2, $3, $4, 'development')",
            )
            .bind(w.workarea_id)
            .bind(w.tenant_id)
            .bind(w.parent_id)
            .bind(&w.name)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
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
                 default_workarea_id) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(k.key_id)
            .bind(k.tenant_id)
            .bind(hash)
            .bind(k.role.as_str())
            .bind(&k.principal)
            .bind(k.held_by.as_str())
            .bind(k.default_workarea)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        MetadataFold::MemoryHeld { memory, enqueue } => {
            insert_memory(tx, event_id, &memory).await?;
            if enqueue {
                enqueue_review(tx, tenant_id, memory.memory_id).await?;
            }
        }
        MetadataFold::MemoryPromoted { memory_id } => {
            set_memory_status(tx, tenant_id, memory_id, MemoryStatus::Promoted).await?;
        }
        MetadataFold::MemoryRejected { memory_id } => {
            set_memory_status(tx, tenant_id, memory_id, MemoryStatus::Rejected).await?;
        }
        MetadataFold::MemorySuperseded {
            superseded,
            successor,
        } => {
            // Successor first: the target's superseded_by FK points at it.
            let successor_id = successor.memory_id;
            insert_memory(tx, event_id, &successor).await?;
            enqueue_review(tx, tenant_id, successor_id).await?;
            sqlx::query(
                "UPDATE memories SET status = 'superseded', superseded_by = $3 \
                 WHERE tenant_id = $1 AND id = $2",
            )
            .bind(tenant_id)
            .bind(superseded)
            .bind(successor_id)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The port implementation
// ---------------------------------------------------------------------------

impl JournalStore for PgStore {
    async fn commit(&self, req: CommitRequest) -> Result<CommitReceipt, StoreError> {
        // Everything networked happened before BEGIN (ADR-0024): the event
        // arrives signed; inside the transaction nothing waits on anything
        // but Postgres.
        let env = &req.event.envelope;
        let tenant_id = env.tenant_id;
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
        let (expected_seq, expected_prev) = match &head {
            None => (1_u64, Sha256Hash::ZERO),
            Some(row) => {
                let seq: i64 = row.try_get("seq").map_err(backend)?;
                let head_hash: Vec<u8> = row.try_get("head_hash").map_err(backend)?;
                let seq = u64::try_from(seq).map_err(backend)?;
                (seq + 1, hash32(&head_hash)?)
            }
        };
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
        // it (idempotently — the rig also reuses this fold as a no-op).
        if matches!(req.fold, MetadataFold::TenantGenesis) {
            let name = env
                .payload
                .get("tenant_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            sqlx::query(
                "INSERT INTO tenants (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING",
            )
            .bind(tenant_id)
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
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
        .execute(&mut *tx)
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
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        apply_fold(&mut tx, tenant_id, env.event_id, req.fold).await?;

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
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        if let Some(put) = req.idempotency {
            // request_digest stores the digest string's bytes verbatim —
            // the PoC digest is an opaque comparand (http-api.md idempotency).
            sqlx::query(
                "INSERT INTO idempotency (tenant_id, key, request_digest, response, \
                 committed_at) VALUES ($1, $2, $3, $4, now())",
            )
            .bind(put.scope.tenant_id)
            .bind(scope_key(&put.scope))
            .bind(put.record.request_digest.into_bytes())
            .bind(put.record.response)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }

        tx.commit().await.map_err(backend)?;
        Ok(CommitReceipt {
            seq: env.seq,
            hash: req.event.hash,
        })
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

    async fn recall_fts(
        &self,
        tenant_id: Uuid,
        workarea_id: Option<Uuid>,
        query: &str,
        k: usize,
    ) -> Result<Vec<MemoryRecord>, StoreError> {
        let k = i64::try_from(k).unwrap_or(i64::MAX);
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // Promoted only (retrieval.md R3 at the store); the 'simple'
        // configuration matches 0001's GIN expression index.
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM memories m \
             WHERE m.tenant_id = $1 AND m.status = 'promoted' \
               AND ($2::uuid IS NULL OR m.workarea_id = $2) \
               AND to_tsvector('simple', m.claim) @@ plainto_tsquery('simple', $3) \
             ORDER BY ts_rank(to_tsvector('simple', m.claim), \
                              plainto_tsquery('simple', $3)) DESC, m.id ASC \
             LIMIT $4"
        );
        let rows = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(workarea_id)
            .bind(query)
            .bind(k)
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
    ) -> Result<Option<MemoryRecord>, StoreError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        set_tenant(&mut tx, tenant_id).await?;
        // Serve the oldest entry this reviewer may judge: skip rows another
        // key claimed, and rows whose lineage closure contains the reviewer
        // (admission.md review; R2 proper re-runs at verdict time). A
        // re-claim by the same key matches its own held_by_key row again.
        let sql = format!(
            "SELECT {MEMORY_COLUMNS} FROM review_queue q \
             JOIN memories m ON m.id = q.memory_id \
             WHERE q.tenant_id = $1 \
               AND (q.held_by_key IS NULL OR q.held_by_key = $2) \
               AND NOT (m.lineage_principals @> ARRAY[$3]::text[]) \
               AND NOT (m.lineage_key_ids @> ARRAY[$2]::uuid[]) \
             ORDER BY q.held_since ASC, q.memory_id ASC \
             LIMIT 1 \
             FOR NO KEY UPDATE OF q SKIP LOCKED"
        );
        let row = sqlx::query(&sql)
            .bind(tenant_id)
            .bind(reviewer_key_id)
            .bind(reviewer_principal)
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
        let row = sqlx::query(
            "SELECT id, tenant_id, role, principal, held_by, default_workarea_id, hash \
             FROM api_keys WHERE hash = $1",
        )
        .bind(&hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let tenant_id: Option<Uuid> = row.try_get("tenant_id").map_err(backend)?;
        let tenant_id = tenant_id.ok_or_else(|| {
            StoreError::Backend(
                "provider-level key rows (tenant_id NULL) are outside the PoC port shape"
                    .to_owned(),
            )
        })?;
        let role: String = row.try_get("role").map_err(backend)?;
        let held_by: String = row.try_get("held_by").map_err(backend)?;
        let stored_hash: Vec<u8> = row.try_get("hash").map_err(backend)?;
        Ok(Some(KeyRecord {
            key_id: row.try_get("id").map_err(backend)?,
            tenant_id,
            role: role_from_str(&role)?,
            principal: row.try_get("principal").map_err(backend)?,
            held_by: held_by_from_str(&held_by)?,
            default_workarea: row.try_get("default_workarea_id").map_err(backend)?,
            key_hash: format!("sha256:{}", hex::encode(stored_hash)),
        }))
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
