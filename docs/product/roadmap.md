# Roadmap

**Status:** design draft for review · decided, not built

This page is the honesty ledger. Nothing in Open Memory Lab is built, so there is no
"shipped" column to flatter: every row below is open, and the register exists so that every
limitation, open question and unmeasured claim in the documentation resolves to exactly one
row here with a verifiable exit criterion. The rules:

- A limitation is never deleted and never apologised for; it becomes a row, and it leaves the
  register when its exit criterion is met and verifiable — not when someone feels good about
  it. A row's number is never reused.
- An **RS** row is an open question the design has not answered. An **FR** row is a
  commitment: the design has decided what the answer must look like, and the row says what
  proves it.
- No measurement of OML exists. No page states a number for an unmeasured quantity; each such
  quantity resolves to a row here whose exit criterion will produce the number.

## Open questions

| ID | Statement | Exit criterion | Status |
|---|---|---|---|
| RS-1 | Which embedding model the [search index](../architecture/retrieval.md) uses, and where it runs — inside the deployment or as an external endpoint — is undecided. The choice sets recall quality, cost, and what leaves the deployment's trust boundary. | A recorded decision naming the model, where it runs, what crosses the boundary to reach it, and how its output is erased when a memory is erased; FR-4's measurement then runs against that choice. | open |
| RS-2 | Agents authenticate to the [agent plugin](../api/mcp-plugin.md) with bearer API keys. The MCP specification defines an OAuth 2.1 authorization model; whether and when OML adopts it is open. Per the sourcing rules, this row is finalized only in the form: *spec revision X (fetched DATE) defines OAuth 2.1; client support as of DATE is [verified finding]; OML ships bearer keys; the OAuth trigger is [named condition]* — the bracketed parts are to be verified with fetch dates before publication. | The row restated in that form with real fetch dates, and the trigger condition recorded as a decision. | open — to be verified with fetch dates before publication |
| RS-3 | Federation — mapping claims across tenants into one "whole world" view, so that what separate organizations' crews learned can be related — is the stated end goal and is explicitly future. Nothing in the current design reaches across a tenant boundary; see [workareas](../architecture/workareas.md). | A design that states what a claim crossing a tenant boundary means for review, attribution and erasure, recorded as a decision; until then, every page that mentions the whole-world goal marks it future. | open — future |

## Commitments

| ID | Statement | Exit criterion | Status |
|---|---|---|---|
| FR-1 | Erasure destroys body and attachment bytes by destroying their keys; attribution metadata — who wrote it, when, into which workarea — survives erasure. The boundary is stated honestly on the [audit and lineage](../architecture/audit-and-lineage.md) page; closing it is this row. | A journalled erasure or pseudonymization act for attribution metadata, demonstrated end to end: after the act, the named fields are unrecoverable from every copy class in the erasure inventory, and the evidence export reflects it. | open |
| FR-2 | Review throughput is unmeasured. The design's gate is only as useful as the review capacity behind it, and no number says what a deployment's reviewing crews can clear per unit time. A fast write path proves nothing about how fast review drains: if arrival outruns the drain rate, the held queue grows without bound and items dead-letter on expiry — which is why this row exists. | A measured drain rate — held memories cleared per second under a stated review policy, reviewer configuration and claim mix — published with date and setup, and compared against a stated arrival rate. | open — unmeasured |
| FR-3 | The lake mirror claims exactly-once application of every event across crashes and retries, resting on the Delta transaction-id mechanism as the Rust library (delta-rs) exposes it; see [storage](../architecture/storage.md). Whether the library behaves as the design assumes is unverified. | A conformance test that kills and restarts the lake writer mid-batch, under retries and failover, and verifies no event applied twice or lost — the claim is then written as "exactly-once, verified by conformance test T-n"; the library capabilities it rests on are cited from delta-rs documentation with a fetch date. | open — unverified |
| FR-4 | Search quality over the serving store is unmeasured: recall, latency, and the exact-versus-approximate trade for vector search have no numbers; see [retrieval](../architecture/retrieval.md). | Measured recall and latency at stated corpus sizes and tenant counts, published with date, hardware and index settings, and a recorded exact-vs-approximate decision justified by those numbers. | open — unmeasured |
| FR-5 | Lake tables hold encrypted bodies only. A decrypted export scoped to a named audience — the journalled act that would let an analytics consumer read promoted content — is designed as future and does not exist in the current design. | A journalled per-audience export: the act names its audience, decrypts only for it, appears in the evidence export, and leaves no decrypted copy outside the export it produced. | open — future feature |
| FR-6 | Rotating a crew's keys is key-by-key. A crew of many agents whose keys must rotate together — on compromise, on schedule — has no bulk act; see [roles and keys](../architecture/roles-and-keys.md). | A bulk re-issue act: one journalled act re-issues a named set of keys with grants carried over reproducibly, old keys expiring on a stated schedule, demonstrated on a crew-sized key set. | open |
| FR-7 | The record store's scale ceiling is unmeasured — how many events, memories and tenants one deployment's PostgreSQL carries before it degrades — and the partition strategy past that ceiling is undecided. | A measured ceiling with date, setup and the load shape that produced it, and a recorded partitioning decision naming the threshold that triggers it. | open — unmeasured |
| FR-8 | Rate limiting is an approximation: each server replica enforces a tenant's limit divided by the replica count, so a tenant can briefly exceed its quota and the enforcement error grows with replica churn. The approximation is documented, not hidden; see [security](../architecture/security.md). | Either shared enforcement measured accurate within a stated bound, or a recorded decision keeping the approximation with its error bound stated where quotas are documented. | open |
| FR-9 | The scoped market claim on the [positioning page](positioning.md) — no provider ships structural, multi-party, evidence-bearing admission control — rests on a September 2026 market check, which also produced the finding that killed the claim's categorical form. It is stale the day it is read. | A live search re-run with a fetch date at publication time; the claim re-published, re-scoped or withdrawn against what the search finds, and this row re-opened at every subsequent publication. | open — blocks the positioning claim |
