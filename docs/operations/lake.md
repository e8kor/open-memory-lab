---
status: Stable
verified: 2026-11-06
answers: "how to run the lake writer, rotate its key, read the lake with the named queries or DuckDB, check its health, and what happens on erase"
---

# Run the lake writer

The lake writer, `oml-lakewriter`, copies what PostgreSQL commits into Delta tables for analysis. No client
reads the lake. Recall, search and get are served from PostgreSQL. The lake answers the questions about how
the system is used. The tables are on [Delta tables](../data/delta-tables.md).

The lake writer is a separate process from the server. The Helm chart does not deploy it, and `oml quickstart`
does not start it. Run it on the server's host, or as a workload of its own on a cluster, with a role of its own.

## Start it

A running server, a directory or object-store path for the lake, and a PostgreSQL role for the writer are required.

```sh
oml-lakewriter keygen --out <kek-file>          # once; owner-only, back it up with the database
export OML_LAKE_URL=<lake-dir>
export OML_LAKE_DATABASE_URL=postgres://oml_lake:…@127.0.0.1/oml   # its own role
export OML_LAKE_OWNER_URL=postgres://oml_owner:…@127.0.0.1/oml     # grants that role at start
export OML_LAKE_KEK_FILE=<kek-file>
oml-lakewriter run --metrics-addr 127.0.0.1:9466
```

The role in `OML_LAKE_DATABASE_URL` must be neither the owner nor the serving role. It must not be a
superuser or bypass row security. The writer refuses one that is. One writer runs at a time, and a second
waits for the first's lease.

Losing the key-encryption key (KEK) file makes every sealed body in the lake unreadable, as erase does. The
file must be owner-only (mode 600). `run` and `drain-once` refuse to start on one that group or others can
read. Rotate it with the writer stopped:

```sh
oml-lakewriter keygen --rotate --kek-file <kek-file>   # takes the lease, re-wraps, retires the old KEK
```

If it stops part way, the file keeps both versions. Run it again. Back up the new file with the database
before the writer restarts.

## Where the lake lives

Each tenant has its own prefix, `<lake>/<tenant id>/`. The provider prefix, `<lake>/_provider/`, holds
`lake/batches` (the frontier) and `bench/results`. Load benchmark numbers with
`oml-lakewriter load-bench eval/bench/results`.

## Run the named queries

`oml-lakewriter query --list` lists them. `oml-lakewriter query <name>` runs one.

| Name | Answers |
|---|---|
| `writes-per-kind-per-day` | claims written each day, by kind |
| `review-latency` | seconds from write to promotion, per promotion day |
| `recall-volume-per-pipeline` | recalls and empty recalls per pipeline per day |
| `recall-outcomes` | what readers did after a recall, per pipeline |
| `stale-share` | share of promoted versions no longer the served tip |
| `top-workareas` | workareas by journalled activity, with names |
| `bench-latest` | each suite's methods on their overall metrics |

DuckDB, with its `delta` extension, answers the same questions against one tenant:

```sql
INSTALL delta; LOAD delta;
SET VARIABLE lake = '<lake-dir>/<tenant id>';
-- writes per kind per day
SELECT event_date, coalesce(kind, 'memory') AS kind, count(*) AS writes
FROM delta_scan(getvariable('lake') || '/bronze/events')
WHERE event_type IN ('submission.remember', 'submission.learn')
GROUP BY ALL ORDER BY ALL;
-- review latency, seconds
SELECT promoted_at::date AS day, count(*) AS promoted,
       median(epoch(promoted_at) - epoch(created_at)) AS median_s
FROM delta_scan(getvariable('lake') || '/silver/claims') GROUP BY ALL ORDER BY ALL;
-- recall volume per pipeline
SELECT day, pipeline, sum(recalls) AS recalls, sum(empty_recalls) AS empty
FROM delta_scan(getvariable('lake') || '/telemetry/recall_daily') GROUP BY ALL ORDER BY ALL;
-- stale share
SELECT kind, avg(CASE WHEN state = 'promoted' THEN 0 ELSE 1 END) AS stale_share
FROM delta_scan(getvariable('lake') || '/silver/claims') GROUP BY ALL;
```

Spark reads the same paths with `spark.read.format("delta").load(path)`.

## Check its health

`oml-lakewriter doctor` prints one line per check, `ok`, `warn` or `fail`, and exits 1 when one fails. The
checks are `lake.writer` (no writer holds the lease), `lake.lag`, `lake.lag_seconds`, `lake.heartbeat`,
`lake.kek`, `lake.commits`, `lake.schema`, `lake.dead_letter` and `lake.maintenance`. Pass `--kek-file` for
`lake.kek`, which checks permissions and warns past 90 days. The metrics listener serves the `oml_lake_*`
families ([metrics](../reference/metrics.md)).

## What happens on erase

On `lifecycle.erase`, the writer destroys the version's key in `artifact_keys` and rewrites its
`silver.claims` row as `erased` with no body. Every older table version that still holds the sealed body
then opens under no key. The daily VACUUM removes those files once they are older than the erasure bound,
7 days by default (`--retention-hours 168`). Bronze holds digests only and needs no change.
