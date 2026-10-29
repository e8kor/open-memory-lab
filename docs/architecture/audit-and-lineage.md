# Audit and lineage

**Status:** design draft for review · decided, not built

This page owns the evidence story and decision D4: what the journal records and what it never
records, what an evidence export contains, how lineage is represented and queried, and what
erasure actually does — including the full inventory of every copy class a memory's content can
reach and what happens to each one when that memory is erased. The journal's commit protocol,
per-tenant hash chain and signing are mechanics owned by [storage](storage.md); the read log's
retention and anchoring are owned by [retrieval](retrieval.md); the simulation fence whose rows
every export must separate is owned by [simulations](simulations.md); the CLI verb that produces
the export, `oml audit export`, is specified in the [CLI reference](../cli/reference.md). The
decision record for erasure is ADR-0005 in the [decision register](../adr/index.md).

Nothing below exists as code. Every mechanism is a design commitment whose exit criterion is
tracked in the [roadmap](../product/roadmap.md). The claim this page makes for the system is the
sanctioned one and no stronger: it **produces the evidence an auditor asks for**. It does not
claim compliance with any regulation, and the one place a regulation is named —
[GDPR Article 17, below](#gdpr-article-17-a-named-tension) — names it as a tension with a
designed answer and an open edge, not as a box ticked.

## What the journal records

The journal is the system of record: a per-tenant, signed, hash-chained, append-only sequence of
events in PostgreSQL ([storage](storage.md) owns the chain-head serialization, the synchronous
durability domain, the append-only enforcement and the fork check). Every act that changes what
the system holds or who may see it is a journal event: capture, scan verdict, review verdict,
ratification (with the review-policy snapshot it was decided under), demote, erase, reassign,
supersession, key issuance and revocation, grant and deny changes, workarea creation, re-parent
and status transitions, bans and lifted bans, simulation start, audience change and end,
dead-letter and re-enqueue, materialize-skipped. Each event carries the acting principal and `api_key_id`, a
real chain timestamp, and — where it concerns content — references to that content.

Two deliberate attestation limits travel with every claim this page makes about the journal:

- **The signature attests the deployment, not a unique process.** Every `oml-server` replica
  loads the same signing key; a compromised replica is a compromised key. The threat model is
  [security](security.md)'s to state; the evidence export inherits the limit.
- **Undeclared derivation is invisible.** A writer that omits `derives_from` produces a row the
  service cannot connect to its sources. Lineage exclusion — the reviewer-independence check,
  simulation taint — is exactly as good as the declared graph, and no page of this
  documentation claims otherwise.

## What the journal never records

**Body plaintext never enters the journal.** A journal event — and the outbox row written in the
same transaction — references content only as a plaintext digest or as a reference to
per-artifact-DEK ciphertext. The ed25519 signature covers that digest/ciphertext envelope, not
the body, which is what makes the design's central evidence property possible: **chain
verification survives crypto-shredding.** After an erase, every signature still verifies and
every hash still chains, because nothing the signatures cover has changed; the bytes the digests
point at have simply stopped being decryptable.

Two further exclusions, each with its reason:

- **Reads are not chain events.** Read events (key, role, workarea, memory ids or query digest,
  timestamp) live outside the hash chain in a retention-bounded log whose per-batch digests are
  anchored into the journal. The anchoring keeps the chain from growing at read rate and keeps
  read records erasable, and the chain proves the log's integrity from each batch's anchor
  onward — the not-yet-anchored tail is protected only by ordinary table permissions until its
  batch digest lands, and the anchoring cadence is a stated deployment parameter. Retention is
  tenant policy; the mechanics are [retrieval](retrieval.md)'s.
- **Identity metadata in the chain is minimized.** The chain carries writer identity and
  principal because attribution is a design requirement of this system — who wrote what, when, where.
  It carries no more than that, and the design is honest that what it does carry is not erased
  by crypto-shredding: see [the boundary](#the-honest-boundary), and the attribution-metadata
  erasure FR in the [roadmap](../product/roadmap.md).

## The evidence export

`oml audit export` produces a signed bundle ([CLI reference](../cli/reference.md) owns the verb
and its flags). Each section of the bundle answers a question an auditor actually asks, and each
carries its own stated limit:

| Section | The question it answers | Built from | Stated limit |
|---|---|---|---|
| Per-memory decision trail | "Show me everything that happened to this claim, in order, and who decided each step." | The journal events for the artifact: capture, scan verdict, every review verdict with the served digests it was recorded against ([admission](admission.md) owns serve binding), ratification with its policy snapshot, and any later demote, reassign, supersession or erase. | For an erased memory the trail shows that each decision was made over a digest that can no longer be opened — decisions survive, content does not. |
| Per-tenant chain verification | "Prove this record was not edited after the fact." | Recomputing the tenant's hash chain and verifying every signature against the deployment key; the result names the verified head (tenant, seq, hash). | Attests the deployment, not a unique process ([security](security.md)); proves integrity of the record, not truth of what agents wrote. |
| Grants history | "Who could do what, where, and when — and who gave it to them?" | Journalled key issuance (with the issuer ceiling check), revocations, grants, per-grant denies, global key denials and permission epochs. Every effective-access computation is reproducible from these records alone ([workareas](workareas.md), [roles and keys](roles-and-keys.md)). | History of authority, not of intent; a key's holder is whoever the tenant gave it to. |
| Read log, retention-scoped | "Who read this memory, and when?" | The read-event log, exported only within the tenant's retention window, with the journal-anchored batch digests that let the window's contents be verified. | Answers nothing outside the window, and says so in the bundle rather than exporting an unverifiable tail ([retrieval](retrieval.md)). |
| Reciprocal-approval statistics | "Could the writer and the reviewer be the same interest?" | Per key-pair approval counts across the export window, surfacing pairs that predominantly ratify each other. | Makes collusion **visible, not impossible** — the independence axis ([roles and keys](roles-and-keys.md)) is checked at promotion; this section is the standing check on what slipped past it. |
| Simulations, separated | "Is any of this synthetic?" | Simulation rows, excluded from the main sections by default and listed separately per `simulation_id` when included; the tainted closure is listed in the simulations section of **every** export, even one that excludes simulation content ([simulations](simulations.md)). The simulations section appears in every export, even one that includes nothing, so absence is asserted rather than assumed. | Fencing is as good as declared derivation (§ above). |

The bundle is itself signed, so a recipient can verify it was produced by the deployment it
claims. What the bundle is **not**: it is not a compliance certificate, it is not legal advice,
and no sentence in it or about it reads "audit-ready". It is the evidence; the verdict is the
auditor's.

## Lineage

Lineage answers two different questions with two different structures: *what did this claim come
from* (the derivation graph) and *which version of this claim is current* (the supersession
chain). Both are journalled; both are materialized into `gold.lineage` for analytics
([Delta tables](../data/delta-tables.md)), but every authoritative read is served from Postgres.

### The derivation graph

Every submission may declare `derives_from` edges to the claims it was built from. A dreamer's
edit-as-proposal is a new attributed submission whose `derives_from` names the version it edits;
a connection proposal is an explicit claim↔claim edge, pending until ratified by an identity
that proposed neither endpoint. The **lineage closure** of a row — the transitive closure over
`derives_from` and claim edges, **pending and ratified alike** — is computed at promotion time
and is load-bearing twice:

- **Reviewer independence.** A verdict is refused when the reviewing key or principal appears
  anywhere in the closure, not merely as the row's direct author. Pending edges count, so a
  proposer cannot launder authorship through an unratified connection
  ([admission](admission.md)).
- **Simulation taint.** A closure that touches a simulation row makes the claim sim-derived,
  with everything that follows ([simulations](simulations.md)).

Closure over pending edges is deliberately the conservative choice: it refuses some reviews a
ratified-only reading would allow, and the design accepts that cost because the alternative —
an author approving their own work through an edge nobody ratified yet — is the exact failure
the constitution exists to prevent.

### Supersession chains, claim_key and the tip

A `claim_key` names which submissions are one claim in several versions; the served member of
that chain is the **tip**.
The rules, enforced as constraints in the [Postgres schema](../data/postgres-schema.md):

- One served tip per `(tenant, workarea_id, claim_key)` — a partial unique index over served
  rows; the [Postgres schema](../data/postgres-schema.md) records why the index is spelled with
  the workarea: `claim_key` is namespaced per workarea.
- `claim_key` is unique per `(tenant, workarea_id)` over non-erased rows.
- **Promotion of a supersession CAS-checks its target is still the current tip.** Two
  supersessions racing for the same tip cannot both win; the loser is refused with a typed
  error and re-proposes against the new tip. The tip never changes except by a ratified
  promotion, so readers never see an unreviewed version ahead of a reviewed one.
- **Reassignment that would collide on `claim_key` in the destination is refused**, unless the
  journalled move event records a re-key. A silent re-key would break the chain's identity;
  a recorded one is lineage.
- **The default tip is never a simulation-tainted version.** Tip resolution is audience-scoped
  ([simulations](simulations.md)): taint never captures the default tip. Tip computation for keys
  outside a simulation's audience ignores `provenance: simulation` rows and sim-derived claims,
  a promoted tainted supersession is served only as that audience's overlay tip, and any act that
  would make a tainted version the default served tip is refused (`simulation.tip_refused`). The
  default tip of a real claim is therefore bit-for-bit indifferent to every simulation.

A superseded version is not deleted — it remains readable as history behind the tip, carrying
its own decision trail, until demoted or erased by its own journalled act.

## Erasure: crypto-shredding

Bodies and attachments are encrypted under per-artifact data-encryption keys (DEKs) wrapped by a
key-encryption key in KMS or a Secret ([security](security.md) owns key custody; the
`artifact_keys` table is in the [Postgres schema](../data/postgres-schema.md)). Erasure is two
acts in one journalled operation:

```mermaid
flowchart LR
    A["erase request<br/><i>admin key, journalled</i>"] --> B["erase event appended<br/><i>signed, in the tenant chain</i>"]
    B --> C["DEK destroyed<br/><i>artifact_keys.destroyed_at set,<br/>live wrapped key destroyed#59;<br/>backup copies decay with KEK retirement</i>"]
    C --> D["live derived copies removed<br/><i>tsvector, pgvector, caches</i>"]
    D --> E["every ciphertext copy<br/>everywhere is now noise"]
    B -.-> F["chain still verifies:<br/>signatures cover digests,<br/>never bodies"]
```

The journalled erase event records who erased, when, under what request provenance, and the key
identifier destroyed — never the key. Demotion is a different act entirely: demote removes a
claim from consumer surfaces and **keeps the bytes**; erase destroys the ability to read them.
Both exist on every workarea status, including `validated` — freeze stops new writes, not
governance or legal reality ([workareas](workareas.md)).

**The guarantee is scoped to body and attachment bytes.** That sentence is the boundary of the
promise, and the inventory below is the promise spelled out per copy.

### The erasure inventory

Every copy class a memory's content — or the key material that can unlock it — can reach, and its
disposition at erase. This table is the normative statement; any page that mentions erasure links
here rather than restating it.

| Copy class | What it holds | Disposition at erase |
|---|---|---|
| `journal.payload` / `signed_bytes` | Digests and ciphertext references only, by design — never plaintext | Never needs touching; the chain verifies unchanged |
| `outbox.payload` | Same digest/ciphertext envelope the journal carries | Never needs touching |
| `bronze.events`, including all Delta versions and checkpoints | Digest-only mirror of journal events | Never needs touching |
| `silver`/`gold` body columns | Per-artifact-DEK ciphertext | Unreadable the moment the DEK is destroyed; a retract MERGE updates row state. Parquet files of **earlier Delta versions and checkpoints** keep the ciphertext until `VACUUM` and checkpoint cleanup remove them — that retention bound is part of the erasure SLO, stated, not hidden |
| Object-store versions (`oml-bodies`, quarantine) | Ciphertext of bodies and attachments, including prior object versions | Unreadable at DEK destruction; versions age out under bucket retention |
| `pgvector` rows and `tsvector` entries | **Plaintext-derived** index content ([retrieval](retrieval.md)) | Removed **in the same transaction as the erase event**; **persist in Postgres base backups and WAL until the named backup retention window expires** — that is the honest statement, and the window is a stated operational bound, not an apology |
| Postgres temporary and spill files; standby replicas' own disks | Transient plaintext-derived content (sorts, hash spills over index rows) on the primary's and every standby's volume | Bounded by volume encryption at rest plus the operating system's temp-file lifetime; a standby's disk holds the same WAL and table copies as the primary and inherits every bound above. Named here because the inventory's rule is completeness, not comfort |
| OS/block-level volume snapshots of the Postgres volume | Everything the volume held at snapshot time — plaintext-derived index content **and** wrapped DEKs — outside the base-backup/WAL accounting above | Whether the deployment takes volume snapshots is an operational choice; if it does, their retention window joins the erasure SLO composition exactly as base backups do, and the [deployment](../operations/deployment.md) backup table must list them |
| `artifact_keys.wrapped_dek` in Postgres base backups and WAL | The **wrapped DEK itself** — a usable key copy, not content | The live row is nulled at erase; copies in base backups and WAL survive until the backup retention window expires, and **while the wrapping KEK lives, a restored backup plus KMS unwrap could recover the DEK and decrypt every surviving ciphertext copy**. The designed bound: scheduled KEK rotation re-wraps **live** DEKs only, and a retired KEK is destroyed once the backup horizon has passed — destroying it invalidates every wrapped-DEK copy in older backups. Erasure therefore completes against live stores immediately, and against backups no later than **backup retention window + KEK rotation period** ([security](security.md) owns key custody; the fenced-restore ceremony is [deployment](../operations/deployment.md)'s) |
| `read_events` | Memory ids and query digests, not bodies | Retention-bounded by tenant policy ([retrieval](retrieval.md)); in this inventory because a query digest can correlate with erased content until its window closes |
| Serving-path caches | Decrypted bodies in flight | Invalidated at erase; any cache added later enters this inventory as a named copy class before it ships |
| Attribution metadata (writer identity, principal, timestamps, workarea path) | Plaintext, in journal, metadata rows and lake rows | **Not erased by crypto-shredding.** Its erasure or pseudonymization is a tracked FR with an exit criterion in the [roadmap](../product/roadmap.md) — open, not solved |

### The honest boundary

What an erase promises: after the journalled erase event, no component of the system can decrypt
the body or attachments from any live store, every live plaintext-derived copy is removed, and
every remaining ciphertext copy decays to noise on its stated retention schedule (Delta `VACUUM`,
object versioning, backup expiry). What it does not promise: instantaneous disappearance of
plaintext-derived index entries from backups taken before the erase; instantaneous invalidation of
the wrapped-DEK copies in those same backups — they remain usable key copies until the retired KEK
that wrapped them is destroyed, no later than backup retention window + KEK rotation period; and
erasure of attribution metadata. All three limits are stated here, bounded by named windows or tracked FRs, and repeated
wherever erasure is mentioned — because the alternative, a one-word "erased" that a reader takes
as absolute, is an overclaim waiting for a retraction. The sanctioned formulation, everywhere:
**a journalled erase event whose effect on each copy class is listed here.**

## GDPR Article 17: a named tension

An append-only journal and a right to erasure pull in opposite directions, and this design does
not pretend otherwise. The resolution rests on one foundational move and one deliberate scoping:

- The foundational move: *decisions are immutable; content is erasable.* A "content stays
  addressable forever" invariant is untenable for personal data — agent memory is free text
  about people as the normal case, not an edge case — and tombstoning (suppress from queries,
  keep the bytes readable) is rejected as the legally weakest option. Crypto-shredding is chosen
  because it is the only option that preserves a verifiable record: destroy the key, keep the
  ciphertext and the chain.
- The scoping: the claim is never unconditional. Statements like "never personal data", "no
  missed replicas", "no race with caches" do not survive scrutiny — a retained usable key backup
  or a surviving plaintext-derived copy is a counterexample to naive "delete the key, done"
  erasure. Deleting the live key alone is insufficient; erasure requires an inventory of every
  usable key copy and every plaintext or derived copy, plus fenced restore. That scoping is why
  [the inventory above](#the-erasure-inventory) exists as a table rather than a sentence, why
  backups and Delta version history appear in it with named windows, why the wrapped-DEK copies in
  Postgres backups appear in it as a usable-key-copy class bounded by KEK retirement, and why attribution
  metadata is listed as not-erased rather than assumed non-personal because adjacent content is
  hashed — signed identifying metadata must be minimized or individually erasable, and OML's
  answer is minimization now and an erasure FR for the rest.

The design's answer to Article 17, then: bodies and attachments are erasable by crypto-shredding
with a journalled, auditable erase event; the journal holds no plaintext, so the record survives;
plaintext-derived copies are removed live and bounded in backups by a named retention window;
wrapped-DEK copies in backups are bounded by scheduled KEK rotation and retired-KEK destruction;
attribution metadata is the open edge, tracked as an FR with an exit criterion. The open edges
are stated because a second-line risk function is paid to find them, and finding them stated is
the difference between a design review and a retraction. None of this is legal advice, and no
sentence here or elsewhere in this documentation claims the design is GDPR-compliant; the claim
is that an erase **produces the evidence an auditor asks for** — the erase event, the inventory,
and a chain that still verifies.

## Decision records

- ADR-0005 — erasure by crypto-shredding with a named inventory and honest boundary
  ([decision register](../adr/index.md)).
- The journal's own decisions (system of record, per-tenant chains) are ADR-0002, owned by
  [storage](storage.md); the read log's are ADR-0013, owned by [retrieval](retrieval.md).
