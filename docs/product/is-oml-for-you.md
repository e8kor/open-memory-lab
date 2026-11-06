---
status: Stable
verified: 2026-11-06
answers: "who OML is for, what it gives and costs, when not to use it, and five scenarios that use only built verbs"
---

# Is OML for you?

OML is for teams that run AI agents on any framework and must show who wrote a memory and who checked it. If nobody in your organization is asked that question, a plain memory store is cheaper and faster.

## The problem OML answers

A crew's memory is the one output nobody reviews. Code goes through review. A deployment goes through a gate. But what an agent writes down becomes the input of every later agent at once.

The store cannot tell a verified finding from a hallucination or a planted instruction. When an agent acts on a wrong memory, someone asks who wrote it, who checked it and why it was served.

## What you get

OML holds every memory until a second identity approves it. Every write is scanned, held, reviewed by someone who did not write it, signed, and only then readable by others. Read [why a write is held](../concepts/why-a-write-is-held.md) for the reasoning.

Memory lives in workareas that mirror your crews. Every write, verdict and key issue is an entry in the journal, a signed, hash-chained record. `oml audit verify-chain` checks that record.

OML works as a memory provider on its own. Agents write, recall, fetch and replace memories through three tools. See [MCP tools](../api/mcp-plugin.md).

## What it costs

- **Review effort.** A memory stays held until a reviewer acts. Your organization supplies that capacity, usually a reviewing crew with a person as the fallback.
- **A write that waits.** Other agents do not see a new memory right away. Its author reads it at once, marked unreviewed. Inside a development workarea, teammates can also recall unreviewed work ([workareas](../use/workareas.md)).
- **Running a server.** Self-hosting needs PostgreSQL, object storage and backups. The public server removes that work, at the price of trusting its operator ([install](../start/install.md)).

## When not to use OML

- You want every write visible to every agent the moment it is saved.
- Nobody can review. A held memory with no reviewer never becomes readable.
- You need OML to extract memories from conversations on its own. It does not. Agents write what they choose to remember.
- You need a profile builder, an answering service or a crew manager. OML returns memories with their provenance and governs what crews remember, not what they do.

## Five scenarios

**Remember a decision and find it next session.**

1. The agent saves the decision with `oml_write`. OML answers held.
2. Later it searches with `oml_read` and finds its own memory marked unreviewed.
3. A reviewer runs `oml review approve`.
4. Next session, any agent in the workarea finds it.

**Run one writer and one reviewer in a crew.**

1. Issue the writer's key with `oml keys issue-agent`.
2. Issue a reviewer key (role `dreamer`) to a second agent.
3. The reviewer runs `oml review next`, which never serves its own writes.
4. It approves or rejects with a reason ([administer a server](../guides/administering.md)).

**Correct or retire a memory.**

1. The agent finds the memory.
2. It writes the corrected text and names the memory it replaces, or marks the write a retirement with a reason.
3. A reviewer approves it.
4. Until then everyone else keeps finding the old version. The history keeps it afterward.

**Load a repository's decision records.**

1. Run `oml sync --dry-run` to see the plan.
2. Run `oml sync` to write what is new and propose what changed.
3. One person reviews the batch with `oml review list` and bulk approve ([sync](../use/sync.md)).

**Use OML from claude.ai or a phone.**

1. An administrator mints a connect code with `oml keys connect-code`.
2. You add the server as a custom connector in claude.ai.
3. You enter the code once on OML's page.
4. Every Claude app signed in to that account then has the three tools ([claude.ai and mobile](../guides/claude-ai-and-mobile.md)).

## Next

Try it in [the quickstart](../start/quickstart.md). Read what OML does not promise in [what OML guarantees](commitments.md).
