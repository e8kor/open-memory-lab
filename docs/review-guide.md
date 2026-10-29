# Review guide

**Status:** design draft for review · decided, not built

This documentation set is a set of drafts for review. Nothing is built; every decision below is
open to challenge, and challenging one here is far cheaper than discovering it wrong in
production. This page lists the decisions a reviewer should examine, in the order in which an
error would be most expensive, with the page that carries each one. The full decision register is
at [adr/index.md](adr/index.md); the open questions each decision leaves are tracked as FR/RS
rows in the [roadmap](product/roadmap.md).

## 1. The constitution and its one exception (ADR-0001)

Four rules bound everything else: the agent-callable surface never promotes, signs, demotes or
deletes; the reviewer is never the author (checked at promotion time against the full lineage
closure, not just at key issuance); the gate changes visibility, never ranking; and there is no
silent deletion. The one sanctioned exception — a `development` workarea may, by journalled
tenant policy, admit marked `self_signed` rows that never leave that workarea — is declared up
front rather than discovered later. **Check:** is the exception's boundary actually tight
(readable only inside the workarea, never promotable beyond it, never entering consumer-readable
lake zones)? Is the honesty rule about undeclared derivation — the service cannot detect a writer
who omits `derives_from`, so lineage exclusion is only as good as the declared graph — acceptable
as a stated limitation, or does it gut R2? Carried by
[architecture/admission.md](architecture/admission.md) (the gate and review policy) and
[architecture/workareas.md](architecture/workareas.md) (where the exception lives).

## 2. The Postgres-authority / Delta-derived split (D1, D2, D12)

PostgreSQL is the single transactional commit point and system of record: one transaction commits
the signed, per-tenant hash-chained journal event, the metadata rows, and the outbox row. Delta
Lake is strictly derived — a single lake-writer drains the outbox, exactly-once delivery rests on
Delta application-transaction ids, and **no client read is ever served from the lake**; all reads
come from Postgres. **Check:** the failure seams. Does the per-tenant chain-head lock serialize
correctly under failover (the startup fork check)? Is the exactly-once protocol sound given that
the Delta txn version, not the Postgres frontier, is the recovery authority — and note the design
itself flags that delta-rs support for this must be verified against its documentation at writing
time? Is "reads never touch the lake" sustainable, or does it smuggle a future second read path?
Carried by [architecture/storage.md](architecture/storage.md), with the read side in
[architecture/retrieval.md](architecture/retrieval.md).

## 3. The erasure boundary (D4)

Erasure is crypto-shredding: bodies and attachments are encrypted under per-artifact keys;
erasing is a journalled event plus key destruction. The guarantee is deliberately scoped to body
and attachment bytes, and the design enumerates every copy class (journal, outbox, lake versions
and checkpoints, object-store versions, search indexes, backups, read logs, caches) with its
disposition — including the honest ones: plaintext-derived index entries persist in backups until
the retention window expires, and attribution metadata (who wrote it, when, where) is **not**
erased by shredding; its erasure is an open roadmap item. **Check:** is this boundary defensible
for the deployments you expect? Is the copy-class inventory complete? Does any page anywhere
drift from the sanctioned phrasing ("produces the evidence an auditor asks for") into a
compliance claim? Carried by
[architecture/audit-and-lineage.md](architecture/audit-and-lineage.md).

## 4. The dreamer split and simulations (D9, D10)

One role name, `dreamer`, carries two capability classes separated at promotion time: propose
(edits-as-proposals, connection proposals) and review (verdicts, ratification) — and review is
refused whenever the dreamer appears anywhere in the row's lineage closure. Simulations are a
fenced provenance channel: every simulation write carries an indelible marker, is served only to
the simulation's audience, stays out of shared indexes, and taint propagates — anything derived
from a simulation row inherits the marker, cannot enter validated workareas, and is demoted or
re-reviewed when the simulation ends. **Check:** is promotion-time lineage-closure enforcement
computationally and operationally realistic? Is the taint rule airtight given that derivation
declaration is voluntary (see decision 1)? Does the audience fence hold across every read path —
search, vector, fetch, graph? Carried by
[architecture/roles-and-keys.md](architecture/roles-and-keys.md) and
[architecture/simulations.md](architecture/simulations.md).

## 5. Tenancy and privileged services (D6)

Tenancy is first-class: tenant identity derives server-side from the API key, Postgres row-level
security applies to every table, and the privileged services are designed rather than waved
through — the lake-writer runs RLS-scoped per tenant with an encrypt-only key grant, the
reconciler runs read-only and emits repairs through the front door, and only the API server holds
the signing key. Rate limiting is honestly approximate in v1 (per-replica token buckets at
limit/N). **Check:** the STRIDE model redone for hostile co-tenants — are the privileged-service
exemptions (no BYPASSRLS, split KMS grants, key custody boundaries) sufficient, and is the
per-replica rate-limit approximation an acceptable v1 posture? Carried by
[architecture/security.md](architecture/security.md).

## 6. CLI-only administration (D14, ADR-0015)

There is no web UI. Administration, key ceremonies, the review flow, mass operations
(dry-run by default), evidence export, and diagnostics all live in one CLI speaking the same HTTP
API as the agents. The review ceremony is the load-bearing part: a reviewer is served an item
with all its attachments under a server-side serve binding that expires, is consumed by its first
verdict, and is invalidated by any change to the served content — a verdict without a valid serve
is refused. **Check:** does declining a web console entirely hold up — is a CLI review ceremony
usable enough that external crews and the occasional
human reviewer will actually run it, or does the absence of a UI quietly starve the review
pipeline the whole design depends on? Carried by [cli/reference.md](cli/reference.md) and
[cli/sessions.md](cli/sessions.md) (worked transcripts).

## After these six

The remaining pages — workarea algebra and the status ladder, the admission pipeline's capacity
assumptions, reconciliation's finding vocabulary, the API and MCP facade, deployment — each carry
their own decisions and their own open items. Read them with the same posture: every unmeasured
claim must carry its marker, and anything that reads as a present-tense property of running
software is a documentation bug worth reporting.
