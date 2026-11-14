---
status: Stable
verified: 2026-11-14
answers: "who approved this memory, what the journal records and never records, how lineage and supersession chains work, and what the signature does and does not prove"
---

# Who approved this memory

Two records answer that question; the [gate view](views.md#logical-one-memory-through-the-review-gate) draws where each is written. The **journal** says what happened, in order, and who decided each step. **Lineage** says what a memory came from and which version of a claim is current.

## What the journal records

The journal is the system of record. It is a signed, hash-chained, append-only sequence of events for each tenant. Each event holds an actor, a timestamp and references to its content. The journal records:

- every write, with the scan rule set that ran;
- every approval and rejection, with the digest of the body served to the reviewer; an approval's `review.ratify` also carries the review policy in force, and a rejection carries none;
- every promotion and supersession;
- key issuance, grants, denials and workspace changes.

The [event catalog](../data/event-catalog.md) lists every event type. The byte-level format is in [chain format](../data/chain-format.md).

### What it never records

Body text never enters the journal. An event references content by digest only. The signature covers the digest, not the text.

Two limits travel with every claim about the journal:

- **The signature attests the deployment, not one process.** Only the lead loads the signing key. A compromised lead is a compromised key.
- **Undeclared derivation is invisible.** A writer that omits `derives_from` produces a memory the server cannot connect to its sources. Lineage checks are as good as the declared links.

The chain carries the actor's name and key id, because attribution is the point. It carries no more than that.

## Ask who approved a memory

| Goal | Run | Result |
|---|---|---|
| The memory's author, version, promotion time and sources | `oml read <artifact_id> --json` | `principal`, `version`, `promoted_at`, `derives_from`, `replaces` |
| When it was approved, and by what kind of reviewer | `oml read <artifact_id> --json` | `approved_at`, and `approver` with `used_by` and `same_user` |
| The approval event and its link to the verdict | `oml events --workspace <workspace>` | `lifecycle.promote` with `ratification_event_id` |
| Who reviewed it, as an admin | `oml admin read <artifact_id>` (admin key) | The memory with its attribution and each verdict with its reviewing key and principal; the read is journaled |
| Proof the record was not edited | `oml audit verify-chain` | Not available yet: the server serves no chain export |

A memory names its author and never its reviewer: who reviewed it is readable on the admin read, which is journaled as `memory.admin_read`. `approver.used_by` says whether the reviewing key is used by an `agent`, as a crew's reviewer is, or by a `person`. `approver.same_user` is `true` when a person approved their own agent's write. Both are `null` on a memory served before review, and on a `universe` memory served to anyone but its author. Recall hits and the `oml_read` tool carry the same two fields.

`oml events` is a feed, not a verification surface. A key sees events inside its read access. When the actor is not one of the reader's own keys, the event shows `principal` and `api_key_id` as `null`. So a reader learns that and when a memory was approved. Only the actor's own keys see the reviewer's identity in the feed. Each admin read of a memory's text is itself journaled as `memory.admin_read`.

An approval consists of two review events. `review.approve` records the verdict and the digest of the body the reviewer was served; a serve itself is not journaled. `review.ratify` records that the policy was met, with a snapshot of the policy and a digest of the author's lineage. `lifecycle.promote` then makes the memory readable.

## Lineage

Lineage answers two different questions with two structures.

### What a memory came from

A write may declare `derives_from` links to the memories it was built from. A proposed link between two memories stays pending until a key that proposed neither end ratifies it.

The **lineage closure** of a memory is every memory reachable through those links, pending and ratified alike. It is computed at approval and used once: a verdict is refused when the reviewing key or its person appears anywhere in the closure. Counting pending links stops an author from approving their own work through a link nobody ratified. The cost is that some reviews are refused that a ratified-only reading would allow.

### Which version is current

A `claim_key` names the versions of one claim. The server serves one **tip** per workspace and `claim_key`.

- Each version has its own `artifact_id` and a `version` ordinal. A successor's ordinal is one more than the highest in the claim, whatever the state of earlier versions.
- A supersession is a held successor that names the version it replaces. The replaced version stays the tip, and everyone keeps reading it, until a reviewer approves the successor. Only approval moves the tip.
- If two successors compete, the first approval wins. The other is refused with `submission.tip_conflict`, nothing is journaled, and that successor stays held until a reviewer rejects it. Its author can propose again against the new tip.
- A new claim whose `claim_key` collides with a held, promoted or demoted version in the workspace is refused with `submission.claim_key_conflict`. A rejected first version releases the key.
- A **retirement** is a supersession with `retirement: true`. Once approved, recall serves nothing of the claim. Every version stays readable by id as history, and nothing is demoted. A later approved supersession of the retirement returns the claim to service.

A superseded version is never deleted. It stays readable as history, with its own decision trail.

## What is not available

- **Erasure.** No verb erases a memory in this build. Erasure by crypto-shredding is designed ([ADR-0005](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0005-erasure-by-crypto-shredding.md)) and not built. Demotion, which keeps the bytes, is designed and not built either. A retirement takes a claim out of recall and keeps every version.
- **An evidence export.** `oml audit export` is designed ([ADR-0015](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0015-cli-administration.md) d4) and not built. A person's own export from the web console is built; it is a copy of the person's memories, not evidence.
- **Chain verification from the CLI.** `oml audit verify-chain` is refused until the server serves a chain export. The [chain format](../data/chain-format.md) procedure verifies the journal table directly.
