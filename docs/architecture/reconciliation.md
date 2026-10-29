# Reconciliation: the journal is the fold source

**Status:** design draft for review · decided, not built

This page is the single declaration site for reconciliation (D16): how
`oml-reconciler` proves that the derived stores still agree with the journal,
what it may find, and how findings are repaired. The stores being compared —
the journal, the outbox protocol, the lake and its dead-letter handling — are
declared in [storage.md](storage.md); the lake tables and their transaction
protocol are specified in [data/delta-tables.md](../data/delta-tables.md); the
workload shape (CronJob, `concurrencyPolicy: Forbid`, deadline) is declared in
[operations/deployment.md](../operations/deployment.md); the operator-facing
surfaces that report its findings are declared in
[operations/observability.md](../operations/observability.md) and
[cli/reference.md](../cli/reference.md) (`oml status`, `oml doctor`).

## What reconciliation is for

PostgreSQL is the system of record; Delta Lake and the Postgres metadata
tables are derived from the journal ([storage.md](storage.md)). Derivation can
drift — a materialization bug, a dead-lettered row, a failover that replayed a
batch, a fold that missed an event. Reconciliation is the standing proof
obligation that derived state still folds out of the journal, run as a job
rather than assumed as a property. It never decrypts anything and never writes
derived state directly: the reconciler runs **read-only, key-less** (its
privilege posture is part of the tenancy design in
[security.md](security.md)), and every repair it wants travels through
journalled channels described below. A reconciler that could silently rewrite
the stores it audits would be worthless as evidence; the read-only posture is
what makes its findings citable in an evidence export
([audit-and-lineage.md](audit-and-lineage.md)).

## Incremental fold with a journalled cursor

The reconciler is incremental. It keeps a **persisted cursor** — the last
reconciled journal `seq` per tenant plus the running chain hash at that seq —
and each run:

1. loads the cursor and verifies the stored running hash against the journal
   at that seq (a cursor that no longer verifies is itself a finding, not a
   starting point);
2. folds journal events from `cursor.seq` forward into expected derived state,
   verifying the per-tenant hash chain as it walks;
3. compares expected state against the actual stores (next section);
4. persists the advanced cursor.

The cursor is **itself journalled**: advancing it is a journalled event, so
"what has been reconciled up to when, by which run" is answerable from the
journal alone, and a rolled-back or forged cursor is detectable the same way
any journal tampering is. The cost of incrementality is that a defect in
already-reconciled history is invisible to the incremental run; that gap is
closed by the full-genesis job below, not denied.

## The comparison: cardinality- and state-aware, at a named Delta version

Row-count equality is not reconciliation. The comparison key is
**(artifact_id, version, state, row count)**: the journal fold says, for every
artifact version, what state it should be in and that exactly one live row
should represent it; the stores are checked against that tuple, not against a
total. A lake holding the right number of rows with two copies of one artifact
and none of another passes a count check and fails this one — which is why
the tuple, not the count, is the contract.

The lake side of the comparison is read **at a named Delta version**: the
reconciler pins the table version it reads (Delta time travel) and records
that version in every finding. Without the pin, a finding could be an artifact
of racing the lake-writer mid-batch; with it, every finding names a consistent
snapshot that can be re-read verbatim when the finding is investigated. The
expected state for that snapshot is the journal fold **up to the outbox seq
that snapshot's transaction metadata records as applied**
([storage.md](storage.md) declares the per-table txn protocol) — the
reconciler compares like with like, never the live journal head against a lake
snapshot that has not seen it yet.

Digest comparison is over **ciphertext digests**. Lake rows and object-store
metadata carry the ciphertext digest precisely so a key-less process can
verify bytes ([storage.md](storage.md)); the reconciler checks that the
ciphertext digest in the store matches the one in the journal fold without
ever holding a DEK. Plaintext-digest verification is scoped to the serving
path, which holds keys ([retrieval.md](retrieval.md)).

## The finding vocabulary is closed

Every finding is one of seven classes. A condition that fits none of them is a
design gap to be raised, not an eighth ad-hoc label:

| Finding | Meaning |
|---|---|
| `missing` | the journal fold expects an artifact version in a store, and no row represents it |
| `orphan` | a store holds a row no journal event accounts for |
| `duplicate` | more than one live row represents one (artifact_id, version) |
| `state-mismatch` | the row exists, but its state disagrees with the fold (e.g. the journal says demoted, the lake row says promoted) |
| `digest-mismatch` | the row's ciphertext digest disagrees with the journal fold's — checked keylessly, as above |
| `stale-frontier` | a store's recorded frontier lags the outbox beyond the declared bound; the data may be internally consistent but is behind |
| `dead-lettered` | the lake-writer dead-lettered a row and journalled a materialize-skipped event ([storage.md](storage.md)); reconciliation surfaces it as a standing finding until it is repaired, so dead-lettering can never become silent loss |

The vocabulary is closed on purpose: alert routing, the `oml doctor` contract,
and the evidence export all enumerate these seven names, and a closed set is
what lets them claim completeness over finding kinds. What is **not** claimed:
that these seven cover every possible corruption. They cover every divergence
expressible against the comparison key above; a corruption invisible to that
key (and to chain verification) is outside this job's power, and the page says
so rather than implying omniscience.

## Repair channels

The reconciler repairs nothing in place. Two channels, by which store
diverged:

**Lake repairs go through the outbox.** The reconciler asks `oml-server` to
enqueue a repair operation **built from the current journal fold** — never
from the defective lake row — carrying the **fold seq** it was built at. The
lake-writer applies it through the same single ordered stream as every other
lake write, and **refuses any operation older than the artifact's last applied
fold seq**. That fence is what makes repair safe to retry and safe against
races: a repair built at fold seq 900 arriving after ordinary traffic has
already applied seq 950 for that artifact is refused, not applied backwards.
A refused repair is not an error; it means the stream already got there.

**Metadata repairs are a journalled re-fold.** When Postgres metadata (the
`memories` fold and its satellites) disagrees with the journal, the repair is
a journalled re-fold of that artifact from the journal — the journal replayed
over the derived rows, recorded as a journalled act naming the finding that
triggered it. The journal is never edited to match metadata; the fold
direction only ever runs one way.

In both channels the journal is the fold source and the only authority. No
repair path writes the journal's history, and no repair path copies a derived
row over another derived row.

## Attempt counters and the auto-repair halt

Every finding has an **identity** — the finding class plus the comparison key
that produced it — and a persisted **attempt counter**. A repair that works
makes the finding disappear on the next run; a finding that recurs under the
same identity increments its counter. When the counter reaches the configured
bound **N**, auto-repair **halts for that artifact** and an operator
condition is raised through the doctor contract
([operations/observability.md](../operations/observability.md)). Where N is
configured — tenant policy or deployment setting — is genuinely open; the
halt itself is not.

The halt is the point, not a failure mode: a finding that survives N honest
repairs means the fold, the writer, or the comparison itself is wrong, and a
loop that keeps re-emitting the same repair would be converting a software
defect into unbounded journalled churn. Halting turns it into a diagnosis
problem with a bounded blast radius of one artifact.

## The full-genesis verification job

Incremental runs trust their cursor. An **occasional full-genesis
verification** job trusts nothing: it verifies every per-tenant chain from the
first event, folds the entire journal, and runs the full comparison against
the stores at a named Delta version — the job that would catch a defect
introduced *behind* an already-advanced cursor, which the incremental run by
construction cannot see. It is the same code path with a cursor of zero, not a
second implementation, so it cannot drift from the incremental job's
semantics. How often it runs is an operational choice this design does not
fix; its runtime against realistic
journal sizes is **unmeasured — decided, not built**, and the Postgres scale
ceiling FR in [product/roadmap.md](../product/roadmap.md) is where that
measurement obligation lives.

## Decision record

The decision is ADR-0017 in the [decision register](../adr/index.md).
