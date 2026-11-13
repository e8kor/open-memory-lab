---
status: Stable
verified: 2026-11-06
answers: "what a team lead needs to decide whether to adopt OML: why writes are held, what it costs, its limits, benchmarks and security, each with the page that owns the detail"
---

# Evaluate OML

OML is a memory server for AI agents in which no write is readable by others until a different identity approves it. This page is the short path through the facts a team lead weighs. Each section links to the page that owns the detail.

## Why writes are held

An agent's wrong conclusion should not become every agent's fact. A write is stored where only its author reads it, scanned, and made readable after a reviewer who did not write it approves it. The cost is a delay and reviewer effort. [Why a write is held](../concepts/why-a-write-is-held.md) has the reasoning and the rejected alternatives. [See it in the quickstart](../start/quickstart.md).

## What it costs

- **Reviewer time.** The reviewers decide how fast a write becomes readable. No measured figure for review speed exists.
- **Running it.** A self-hosted server needs PostgreSQL 17 or later and backups, and one process serves the API. A server someone else runs removes that work and asks for trust in its operator.
- **No model fees from OML.** OML runs no language model. The scan and the search are deterministic code.

More in [Is OML a fit?](is-oml-for-you.md).

## Limits

One server, one lead: an upgrade or restart stops the API until the new instance is ready. The largest measured recall load is 99,721 readable memories at 25.5 ms p95 at the database. 1,000,000 memories, review throughput and write ceilings are unmeasured. [Limits](../reference/limits.md) lists every bound, the gaps and what is not available in this build.

## Benchmarks

Recall quality is keyword-search grade: nDCG@10 of 0.32 to 0.87 depending on the corpus, against 0.665 and 0.325 for the published BM25 baseline on SciFact and NFCorpus. These are retrieval metrics from the maintainers' runs on one host. They are not the answer-accuracy scores other memory products publish and cannot be compared with them. [Benchmarks](../benchmarks/index.md) has the method and every table.

## Security and guarantees

The tenant comes from the key, never from the request. Row-level security covers every tenant table, and every write, verdict and key issue enters a signed, hash-chained journal. An agent has no approve, sign, demote or delete verb. Nobody has penetration-tested these controls. [Security](../architecture/security.md) and [what OML guarantees](commitments.md) list what is enforced, what is bounded and what is not claimed.

## When not to use it

Every write must be visible to every agent at once, or nobody can review. See [when not to use OML](is-oml-for-you.md#when-not-to-use-oml).
