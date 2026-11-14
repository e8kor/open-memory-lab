---
status: Stable
verified: 2026-11-14
answers: "why a write is held until a different identity approves it, how it becomes readable, what the rule costs and what was rejected"
---

# Why is a write held until someone else approves it?

**Short answer.** One agent's wrong conclusion should not become every agent's fact. OML holds each write out of other keys' recall, scans it, and serves it to them only after an identity that did not write it approves it. The cost is a delay and reviewer effort.

## How a write becomes readable

1. **Write.** An agent calls `oml_write` or a person runs `oml write`. The answer is `held`.
2. **Scan.** The server scans the text in the same call. A leaked key is refused. Text that looks like an instruction aimed at the reader is held for a closer look.
3. **Hold.** The memory is stored. Its author can find it by asking for unreviewed memories. Recall serves it to no other key, except inside an in-progress workarea (below). A tenant admin can read it through the journaled [admin read](../architecture/roles-and-keys.md#the-admin-read).
4. **Review.** A reviewer key (role `dreamer`) takes the memory, reads it and approves or rejects it. The server never serves a reviewer their own writes or memories built on them.
5. **Sign.** The verdict is signed into the journal, a hash-chained record of events.
6. **Read.** An approved memory has the state `promoted`. Keys with read access to its workarea now find it.

## The rule: the reviewer is never the author

Two of the four rules declared in [the constitution](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md) carry this page. A key that wrote a memory, or wrote something it derives from, cannot approve it (R2). And an agent's tools reach no verb that approves, signs, demotes or deletes (R1). No setting or role turns either off.

Approval changes visibility, not ranking. A reviewed memory is not scored higher than an unreviewed one would be.

## In-progress workareas, and a designed exception

In an in-progress workarea, other agents can read held memories labeled `unreviewed`; nothing leaves that workarea until a person commits it with no held memory left ([workareas](../use/workspaces.md)).

A development workarea where a person's own approval of their own memory counts once, marked `self_signed` and never called a review, is designed ([ADR-0001](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md) d5) and not built.

## What was rejected

- **The author reviews their own write.** It is fast and proves nothing: the error that wrote the memory also approves it.
- **Moderation after the fact.** A wrong memory is read by other agents before anyone looks. Withdrawing it does not undo what they did with it.
- **Trust by ranking.** Showing unreviewed memories lower in results still lets them steer an agent, and a reader cannot tell which hits were checked.

## What it costs

- A write is not recallable by others until a reviewer acts. How fast review goes depends on the reviewers; OML has no measured figure.
- Someone has to review. A person can approve a listed set in one command ([review held memories](../guides/reviewing.md)); an external crew can review over HTTP ([reviewer crews](../api/reviewer-crews.md)).
- The reviewers set the quality. OML records who approved what; it cannot make a reviewer careful.

## Related

[What OML guarantees](../product/commitments.md), [how a write is admitted](../architecture/admission.md), [roles and keys](../architecture/roles-and-keys.md), [quickstart](../start/quickstart.md).
