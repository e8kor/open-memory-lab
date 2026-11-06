//! What the lake writer reads from Postgres, and the two tables it writes
//! there (ADR-0060 d4, d6, d13, d14). Every tenant read runs in a
//! transaction that sets `oml.tenant_id`, under the same row security the
//! lead runs under: the writer's role sees one tenant at a time
//! (delta-tables.md: "the lake-writer already iterates per tenant under an
//! RLS-scoped session role").

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection as _, Postgres, Row as _, Transaction};
use uuid::Uuid;

/// The session advisory lock that is the writer's lease (ADR-0060 d5):
/// "oml_lake" in ASCII.
pub const LEASE_KEY: i64 = 0x6f6d_6c5f_6c61_6b65;

/// How long the horizon waits for the transactions that may still hold an
/// outbox seq at or below the sequence's value before it gives up for this
/// tick (ADR-0060 d4).
const HORIZON_WAIT_POLLS: u32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("postgres: {0}")]
    Pg(#[from] sqlx::Error),
    /// The writer's login is a superuser or bypasses row security, so row
    /// security would not hold it to one tenant (ADR-0060 d13).
    #[error(
        "the lake writer's role {0} is a superuser or bypasses row security; give it a role of its own"
    )]
    RolePrivileged(String),
}

/// One journal event as the outbox delivered it.
#[derive(Debug, Clone)]
pub struct Event {
    pub outbox_seq: i64,
    pub journal_seq: i64,
    pub event_id: Uuid,
    pub event_type: String,
    pub at: DateTime<Utc>,
    pub prev_hash: Vec<u8>,
    pub hash: Vec<u8>,
    pub sig: Vec<u8>,
    pub signed_bytes: Vec<u8>,
    pub payload: serde_json::Value,
    pub principal: String,
    pub api_key_id: Option<Uuid>,
}

/// One claim version's row, read when an event names it.
#[derive(Debug, Clone)]
pub struct Memory {
    pub id: Uuid,
    pub version: i64,
    pub claim_key: String,
    pub kind: String,
    pub status: String,
    pub is_tip: bool,
    pub workarea_id: Uuid,
    pub author_key_id: Uuid,
    pub author_principal: String,
    pub retirement: bool,
    pub replaces: Option<Uuid>,
    pub body: String,
    pub digest: Vec<u8>,
    pub body_bytes: i64,
    pub created_at: DateTime<Utc>,
    pub promoted_at: Option<DateTime<Utc>>,
    pub derives_from: Vec<Uuid>,
}

/// One ratified edge.
#[derive(Debug, Clone)]
pub struct Edge {
    pub id: Uuid,
    pub graph: &'static str,
    pub label: String,
    pub src: Uuid,
    pub dst: Uuid,
    pub ratified: bool,
    pub proposer: Uuid,
    pub ratifier: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// One version's key row.
#[derive(Debug, Clone)]
pub struct KeyRow {
    pub wrapped: Option<Vec<u8>>,
    pub kek_id: String,
}

/// One day's recall aggregate for one pipeline and access path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecallDay {
    pub day: NaiveDate,
    pub pipeline: String,
    pub access: String,
    pub counts: [i64; 11],
}

/// The writer's Postgres, over its own role.
#[derive(Debug, Clone)]
pub struct Source {
    pool: PgPool,
}

impl Source {
    /// Connects and refuses a role row security would not bind.
    ///
    /// # Errors
    /// The connection fails or the role is privileged.
    pub async fn connect(url: &str) -> Result<Self, SourceError> {
        let pool = PgPoolOptions::new().max_connections(4).connect(url).await?;
        let (role, privileged): (String, bool) = sqlx::query_as(
            "SELECT current_user::text, rolsuper OR rolbypassrls FROM pg_roles \
             WHERE rolname = current_user",
        )
        .fetch_one(&pool)
        .await?;
        if privileged {
            return Err(SourceError::RolePrivileged(role));
        }
        Ok(Self { pool })
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Grants the writer's role what its statements use, through the owner
    /// URL, then closes it (ADR-0060 d13, after ADR-0034 d4): SELECT on the
    /// tables it reads and the outbox sequence, and SELECT, INSERT, UPDATE
    /// on the two tables it writes. GRANT is idempotent, so every start
    /// runs it.
    ///
    /// # Errors
    /// The owner cannot connect or grant.
    pub async fn grant(owner_url: &str, role: &str) -> Result<(), SourceError> {
        let mut owner = PgConnection::connect(owner_url).await?;
        let role = quote_ident(role);
        for stmt in [
            format!(
                "GRANT SELECT ON tenants, journal, outbox, memories, claim_edges, brief_edges, \
                 recall_events, recall_outcomes TO {role}"
            ),
            format!("GRANT SELECT, INSERT, UPDATE ON artifact_keys, lake_dead_letter TO {role}"),
            format!("GRANT SELECT ON SEQUENCE {} TO {role}", "outbox_seq_seq"),
        ] {
            sqlx::query(&stmt).execute(&mut owner).await?;
        }
        owner.close().await?;
        Ok(())
    }

    /// The role `url` logs in as, before any grant.
    ///
    /// # Errors
    /// The connection fails.
    pub async fn role_of(url: &str) -> Result<String, SourceError> {
        let mut conn = PgConnection::connect(url).await?;
        let role = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&mut conn)
            .await?;
        conn.close().await?;
        Ok(role)
    }

    /// The role this source logs in as.
    ///
    /// # Errors
    /// The query fails.
    pub async fn role(&self) -> Result<String, SourceError> {
        Ok(sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&self.pool)
            .await?)
    }

    /// Every tenant id, through 0003's startup enumeration policy.
    ///
    /// # Errors
    /// The query fails.
    pub async fn tenants(&self) -> Result<Vec<Uuid>, SourceError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('oml.startup_check', 'on', true)")
            .execute(&mut *tx)
            .await?;
        let ids = sqlx::query_scalar("SELECT id FROM tenants ORDER BY id")
            .fetch_all(&mut *tx)
            .await?;
        tx.rollback().await?;
        Ok(ids)
    }

    /// The outbox's stable prefix: the greatest seq at or below which no
    /// transaction can still commit a row (ADR-0003 d4, ADR-0060 d4). It
    /// reads the sequence's value, then the transactions holding the
    /// outbox's insert lock, which every transaction that drew a seq at or
    /// below that value took before drawing it; once those have ended, the
    /// value is stable. It takes no lock on the outbox and so never waits on
    /// or delays a commit. `None` when they have not ended within the wait.
    ///
    /// # Errors
    /// The queries fail.
    pub async fn horizon(&self) -> Result<Option<i64>, SourceError> {
        let mut conn = self.pool.acquire().await?;
        let value: i64 = sqlx::query_scalar(
            "SELECT CASE WHEN is_called THEN last_value ELSE 0 END FROM outbox_seq_seq",
        )
        .fetch_one(&mut *conn)
        .await?;
        let first = outbox_writers(&mut conn).await?;
        if first.is_empty() {
            return Ok(Some(value));
        }
        for _ in 0..HORIZON_WAIT_POLLS {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let now = outbox_writers(&mut conn).await?;
            if !now.iter().any(|v| first.contains(v)) {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    /// The tenant's events whose outbox seq is in (`after`, `upto`], in
    /// outbox order.
    ///
    /// # Errors
    /// The query fails.
    pub async fn events(
        &self,
        tenant: Uuid,
        after: i64,
        upto: i64,
    ) -> Result<Vec<Event>, SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        let rows = sqlx::query(
            "SELECT o.seq, j.seq AS journal_seq, j.event_id, j.event_type, j.at, j.prev_hash, \
                    j.signed_bytes_hash, j.sig, j.signed_bytes, j.payload, j.principal, j.api_key_id \
             FROM outbox o JOIN journal j ON j.tenant_id = o.tenant_id AND j.seq = o.tenant_seq \
             WHERE o.tenant_id = $1 AND o.seq > $2 AND o.seq <= $3 ORDER BY o.seq",
        )
        .bind(tenant)
        .bind(after)
        .bind(upto)
        .fetch_all(&mut *tx)
        .await?;
        tx.rollback().await?;
        rows.iter()
            .map(|r| {
                Ok(Event {
                    outbox_seq: r.try_get("seq")?,
                    journal_seq: r.try_get("journal_seq")?,
                    event_id: r.try_get("event_id")?,
                    event_type: r.try_get("event_type")?,
                    at: r.try_get("at")?,
                    prev_hash: r.try_get("prev_hash")?,
                    hash: r.try_get("signed_bytes_hash")?,
                    sig: r.try_get("sig")?,
                    signed_bytes: r
                        .try_get::<Option<Vec<u8>>, _>("signed_bytes")?
                        .unwrap_or_default(),
                    payload: r.try_get("payload")?,
                    principal: r.try_get("principal")?,
                    api_key_id: r.try_get("api_key_id")?,
                })
            })
            .collect::<Result<_, sqlx::Error>>()
            .map_err(Into::into)
    }

    /// The age of the tenant's oldest outbox row past `after`, for the lag.
    ///
    /// # Errors
    /// The query fails.
    pub async fn oldest_pending(
        &self,
        tenant: Uuid,
        after: i64,
    ) -> Result<Option<DateTime<Utc>>, SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        let at = sqlx::query_scalar(
            "SELECT min(created_at) FROM outbox WHERE tenant_id = $1 AND seq > $2",
        )
        .bind(tenant)
        .bind(after)
        .fetch_one(&mut *tx)
        .await?;
        tx.rollback().await?;
        Ok(at)
    }

    /// The claim version `id`.
    ///
    /// # Errors
    /// The query fails.
    pub async fn memory(&self, tenant: Uuid, id: Uuid) -> Result<Option<Memory>, SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        let row = sqlx::query(
            "SELECT id, version, claim_key, kind, status, is_tip, workarea_id, author_key_id, \
                    author_principal, retirement, replaces_artifact_id, claim, claim_digest, \
                    body_bytes, created_at, promoted_at, derives_from \
             FROM memories WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.rollback().await?;
        let Some(r) = row else { return Ok(None) };
        Ok(Some(Memory {
            id: r.try_get("id")?,
            version: i64::from(r.try_get::<i32, _>("version")?),
            claim_key: r.try_get("claim_key")?,
            kind: r.try_get("kind")?,
            status: r.try_get("status")?,
            is_tip: r.try_get("is_tip")?,
            workarea_id: r.try_get("workarea_id")?,
            author_key_id: r.try_get("author_key_id")?,
            author_principal: r.try_get("author_principal")?,
            retirement: r.try_get("retirement")?,
            replaces: r.try_get("replaces_artifact_id")?,
            body: r.try_get("claim")?,
            digest: r.try_get("claim_digest")?,
            body_bytes: i64::from(r.try_get::<i32, _>("body_bytes")?),
            created_at: r.try_get("created_at")?,
            promoted_at: r.try_get("promoted_at")?,
            derives_from: r.try_get("derives_from")?,
        }))
    }

    /// The claim or brief edge `id`.
    ///
    /// # Errors
    /// The query fails.
    pub async fn edge(&self, tenant: Uuid, id: Uuid) -> Result<Option<Edge>, SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        let claim = sqlx::query(
            "SELECT id, label, src_artifact_id, dst_artifact_id, state, proposer_key_id, \
                    ratifier_key_id, created_at FROM claim_edges WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let edge = if let Some(r) = claim {
            Some(Edge {
                id: r.try_get("id")?,
                graph: "claim",
                label: r.try_get("label")?,
                src: r.try_get("src_artifact_id")?,
                dst: r.try_get("dst_artifact_id")?,
                ratified: r.try_get::<String, _>("state")? == "ratified",
                proposer: r.try_get("proposer_key_id")?,
                ratifier: r.try_get("ratifier_key_id")?,
                created_at: r.try_get("created_at")?,
            })
        } else {
            let brief = sqlx::query(
                "SELECT id, artifact_id, workarea_id, state, proposer_key_id, ratifier_key_id, \
                        created_at FROM brief_edges WHERE tenant_id = $1 AND id = $2",
            )
            .bind(tenant)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
            match brief {
                None => None,
                Some(r) => Some(Edge {
                    id: r.try_get("id")?,
                    graph: "brief",
                    label: "brief".to_owned(),
                    src: r.try_get("artifact_id")?,
                    dst: r.try_get("workarea_id")?,
                    ratified: r.try_get::<String, _>("state")? == "ratified",
                    proposer: r.try_get("proposer_key_id")?,
                    ratifier: r.try_get("ratifier_key_id")?,
                    created_at: r.try_get("created_at")?,
                }),
            }
        };
        tx.rollback().await?;
        Ok(edge)
    }

    /// The version's key row.
    ///
    /// # Errors
    /// The query fails.
    pub async fn key(&self, tenant: Uuid, artifact: Uuid) -> Result<Option<KeyRow>, SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        let row = sqlx::query(
            "SELECT wrapped_dek, kek_id FROM artifact_keys WHERE tenant_id = $1 AND artifact_id = $2",
        )
        .bind(tenant)
        .bind(artifact)
        .fetch_optional(&mut *tx)
        .await?;
        tx.rollback().await?;
        Ok(match row {
            None => None,
            Some(r) => Some(KeyRow {
                wrapped: r.try_get("wrapped_dek")?,
                kek_id: r.try_get("kek_id")?,
            }),
        })
    }

    /// Live data keys of `tenant` wrapped by a KEK other than `kek_id`:
    /// (artifact, wrapped key, the id that wrapped it). A destroyed row has
    /// no key and is never listed.
    ///
    /// # Errors
    /// The query fails.
    pub async fn keys_not_under(
        &self,
        tenant: Uuid,
        kek_id: &str,
    ) -> Result<Vec<(Uuid, Vec<u8>, String)>, SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        let rows = sqlx::query(
            "SELECT artifact_id, wrapped_dek, kek_id FROM artifact_keys \
             WHERE tenant_id = $1 AND wrapped_dek IS NOT NULL AND kek_id <> $2 \
             ORDER BY artifact_id",
        )
        .bind(tenant)
        .bind(kek_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.rollback().await?;
        rows.iter()
            .map(|r| {
                Ok((
                    r.try_get("artifact_id")?,
                    r.try_get("wrapped_dek")?,
                    r.try_get("kek_id")?,
                ))
            })
            .collect()
    }

    /// Replaces a data key's wrapping if the row still holds `old_wrapped`:
    /// `false` when the key was destroyed or changed since it was read, so a
    /// re-wrap never resurrects an erased key.
    ///
    /// # Errors
    /// The statement fails.
    pub async fn rewrap_key(
        &self,
        tenant: Uuid,
        artifact: Uuid,
        old_wrapped: &[u8],
        new_wrapped: &[u8],
        new_kek_id: &str,
    ) -> Result<bool, SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        let n = sqlx::query(
            "UPDATE artifact_keys SET wrapped_dek = $4, kek_id = $5 \
             WHERE tenant_id = $1 AND artifact_id = $2 AND wrapped_dek = $3",
        )
        .bind(tenant)
        .bind(artifact)
        .bind(old_wrapped)
        .bind(new_wrapped)
        .bind(new_kek_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(n == 1)
    }

    /// Stores a new wrapped key unless the version already has a row, and
    /// returns the row that holds: a racing writer's key, or a destroyed
    /// row, wins over this one.
    ///
    /// # Errors
    /// The statements fail.
    pub async fn put_key(
        &self,
        tenant: Uuid,
        artifact: Uuid,
        wrapped: &[u8],
        kek_id: &str,
    ) -> Result<KeyRow, SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        sqlx::query(
            "INSERT INTO artifact_keys (tenant_id, artifact_id, wrapped_dek, kek_id) \
             VALUES ($1, $2, $3, $4) ON CONFLICT (tenant_id, artifact_id) DO NOTHING",
        )
        .bind(tenant)
        .bind(artifact)
        .bind(wrapped)
        .bind(kek_id)
        .execute(&mut *tx)
        .await?;
        let r = sqlx::query(
            "SELECT wrapped_dek, kek_id FROM artifact_keys WHERE tenant_id = $1 AND artifact_id = $2",
        )
        .bind(tenant)
        .bind(artifact)
        .fetch_one(&mut *tx)
        .await?;
        let row = KeyRow {
            wrapped: r.try_get("wrapped_dek")?,
            kek_id: r.try_get("kek_id")?,
        };
        tx.commit().await?;
        Ok(row)
    }

    /// Destroys the version's key: `wrapped_dek` NULL and `destroyed_at`
    /// set, whether or not a key existed, so no later materialization can
    /// key it again (ADR-0005 d2, ADR-0060 d6).
    ///
    /// # Errors
    /// The statement fails.
    pub async fn destroy_key(
        &self,
        tenant: Uuid,
        artifact: Uuid,
        kek_id: &str,
    ) -> Result<(), SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        sqlx::query(
            "INSERT INTO artifact_keys (tenant_id, artifact_id, wrapped_dek, kek_id, destroyed_at) \
             VALUES ($1, $2, NULL, $3, now()) \
             ON CONFLICT (tenant_id, artifact_id) DO UPDATE \
               SET wrapped_dek = NULL, destroyed_at = coalesce(artifact_keys.destroyed_at, now())",
        )
        .bind(tenant)
        .bind(artifact)
        .bind(kek_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Records that `table` skipped outbox row `seq` for `reason`.
    ///
    /// # Errors
    /// The statement fails.
    pub async fn dead_letter(
        &self,
        tenant: Uuid,
        seq: i64,
        table: &str,
        reason: &str,
    ) -> Result<(), SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        sqlx::query(
            "INSERT INTO lake_dead_letter (tenant_id, outbox_seq, lake_table, reason) \
             VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        )
        .bind(tenant)
        .bind(seq)
        .bind(table)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Unrepaired dead letters of every tenant.
    ///
    /// # Errors
    /// A query fails.
    pub async fn dead_letters(&self) -> Result<i64, SourceError> {
        let mut total = 0;
        for t in self.tenants().await? {
            let mut tx = self.tenant_tx(t).await?;
            let n: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM lake_dead_letter WHERE tenant_id = $1 AND repaired_at IS NULL",
            )
            .bind(t)
            .fetch_one(&mut *tx)
            .await?;
            tx.rollback().await?;
            total += n;
        }
        Ok(total)
    }

    /// The tenant's daily recall aggregates over the 30 days Postgres keeps
    /// (ADR-0054 d2): counts only, never a query's text, digest or the
    /// versions it served (ADR-0060 d7).
    ///
    /// # Errors
    /// The query fails.
    pub async fn recall_days(&self, tenant: Uuid) -> Result<Vec<RecallDay>, SourceError> {
        let mut tx = self.tenant_tx(tenant).await?;
        let rows = sqlx::query(
            "SELECT (e.created_at AT TIME ZONE 'UTC')::date AS day, e.pipeline, e.access, \
               count(*)::bigint AS recalls, \
               coalesce(sum(cardinality(e.hit_ids)), 0)::bigint AS hits, \
               (count(*) FILTER (WHERE cardinality(e.hit_ids) = 0))::bigint AS empty, \
               (count(*) FILTER (WHERE e.query_text IS NOT NULL))::bigint AS with_text, \
               coalesce(sum(o.got), 0)::bigint AS got, coalesce(sum(o.derived), 0)::bigint AS derived, \
               coalesce(sum(o.superseded), 0)::bigint AS superseded, \
               coalesce(sum(o.useful), 0)::bigint AS useful, \
               coalesce(sum(o.not_useful), 0)::bigint AS not_useful, \
               coalesce(sum(o.requery), 0)::bigint AS requery, \
               coalesce(sum(o.no_use), 0)::bigint AS no_use \
             FROM recall_events e LEFT JOIN ( \
               SELECT recall_id, created_at, \
                 count(*) FILTER (WHERE label = 'get') AS got, \
                 count(*) FILTER (WHERE label = 'derived') AS derived, \
                 count(*) FILTER (WHERE label = 'superseded') AS superseded, \
                 count(*) FILTER (WHERE label = 'feedback_useful') AS useful, \
                 count(*) FILTER (WHERE label = 'feedback_not_useful') AS not_useful, \
                 count(*) FILTER (WHERE label = 'requery') AS requery, \
                 count(*) FILTER (WHERE label = 'no_use') AS no_use \
               FROM recall_outcomes WHERE tenant_id = $1 GROUP BY recall_id, created_at) o \
               ON o.recall_id = e.recall_id AND o.created_at = e.created_at \
             WHERE e.tenant_id = $1 AND e.created_at >= now() - interval '30 days' \
             GROUP BY 1, 2, 3 ORDER BY 1, 2, 3",
        )
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;
        tx.rollback().await?;
        rows.iter()
            .map(|r| {
                let mut counts = [0_i64; 11];
                for (i, c) in [
                    "recalls",
                    "hits",
                    "empty",
                    "with_text",
                    "got",
                    "derived",
                    "superseded",
                    "useful",
                    "not_useful",
                    "requery",
                    "no_use",
                ]
                .iter()
                .enumerate()
                {
                    counts[i] = r.try_get(*c)?;
                }
                Ok(RecallDay {
                    day: r.try_get("day")?,
                    pipeline: r.try_get("pipeline")?,
                    access: r.try_get("access")?,
                    counts,
                })
            })
            .collect::<Result<_, sqlx::Error>>()
            .map_err(Into::into)
    }

    /// Whether any session holds the writer's lease (the doctor's "writer
    /// down" line).
    ///
    /// # Errors
    /// The query fails.
    pub async fn lease_held(&self) -> Result<bool, SourceError> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'advisory' \
               AND database = (SELECT oid FROM pg_database WHERE datname = current_database()) \
               AND classid = $1::bigint::oid AND objid = $2::bigint::oid AND objsubid = 1 AND granted)",
        )
        .bind(LEASE_KEY >> 32)
        .bind(LEASE_KEY & 0xffff_ffff)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn tenant_tx(&self, tenant: Uuid) -> Result<Transaction<'static, Postgres>, SourceError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('oml.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }
}

/// The single-writer lease: a session advisory lock held on a connection
/// of its own for as long as the writer runs (ADR-0060 d5). Losing the
/// connection loses the lease; the Delta transaction version stays the
/// fence that holds when two writers overlap (ADR-0016 d3).
pub struct Lease {
    conn: PgConnection,
}

impl Lease {
    /// The lease, or `None` when another session holds it.
    ///
    /// # Errors
    /// The connection fails.
    pub async fn try_acquire(url: &str) -> Result<Option<Self>, SourceError> {
        let mut conn = PgConnection::connect(url).await?;
        let got: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(LEASE_KEY)
            .fetch_one(&mut conn)
            .await?;
        Ok(got.then_some(Self { conn }))
    }

    /// Whether the lease's connection still answers.
    pub async fn alive(&mut self) -> bool {
        sqlx::query("SELECT 1")
            .execute(&mut self.conn)
            .await
            .is_ok()
    }

    /// Releases the lease.
    ///
    /// # Errors
    /// The connection fails.
    pub async fn release(mut self) -> Result<(), SourceError> {
        sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(LEASE_KEY)
            .execute(&mut self.conn)
            .await?;
        self.conn.close().await?;
        Ok(())
    }
}

/// The virtual transactions holding the outbox's insert lock, but this
/// session's.
async fn outbox_writers(conn: &mut PgConnection) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT virtualtransaction FROM pg_locks \
         WHERE locktype = 'relation' AND relation = 'outbox'::regclass \
           AND database = (SELECT oid FROM pg_database WHERE datname = current_database()) \
           AND mode = 'RowExclusiveLock' AND granted AND pid <> pg_backend_pid()",
    )
    .fetch_all(conn)
    .await
}

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
