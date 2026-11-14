---
status: Stable
verified: 2026-11-14
answers: "who OML is for, the three audiences and the experience it aims for, what it gives and costs, when not to use it, and five short scenarios that use only built verbs"
---

# Is OML a fit?

OML is for teams that run AI agents on any framework and must show who wrote a memory and who checked it. If nobody in the organization is asked that question, a plain memory store is cheaper and faster.

For a decision, start with [Evaluate OML](evaluate.md): why writes are held, what it costs, [limits](../reference/limits.md), [benchmarks](../benchmarks/index.md) and [security](../architecture/security.md), one line each.

## Who it serves

- **Individual users**: one person whose agents remember across sessions, Claude Code, claude.ai and a phone, with personal knowledge kept apart ([scenario](../scenarios/individual.md)).
- **Engineering teams**: a crew building skills, guardrails and runbooks in a shared development workspace, toward production agents ([scenario](../scenarios/engineering-teams.md)).
- **Business-analyst teams**: documents and vocabulary prepared on staging and handed to production ([scenario](../scenarios/analyst-teams.md)).

All three get the same experience: frictionless in development, controlled toward production.

## The problem OML answers

A crew's memory is the one output nobody reviews. Code goes through review. A deployment goes through a gate. But what an agent writes down becomes the input of every later agent at once.

The store cannot tell a verified finding from a hallucination or a planted instruction. When an agent acts on a wrong memory, someone asks who wrote it, who checked it and why it was served.

## What it gives

OML holds every memory until a second identity approves it. Every write is scanned, held, reviewed by someone who did not write it, signed, and only then readable by others. Read [why a write is held](../concepts/why-a-write-is-held.md) for the reasoning.

Memory lives in workspaces that mirror the crews. Every write, verdict and key issue is an entry in the journal, a signed, hash-chained record. The [chain format](../data/chain-format.md) procedure checks that record; `oml audit verify-chain` refuses until the server serves a chain export, which is not built.

OML works as a memory provider on its own. Agents write, recall, fetch and replace memories through three tools. See [MCP tools](../api/mcp-plugin.md).

## What it costs

- **Review effort.** A memory stays held until a reviewer acts. The organization supplies that capacity, usually a reviewing crew with a person as the fallback.
- **A write that waits.** Other agents do not see a new memory right away. Its author reads it at once, marked unreviewed. Inside a workspace whose drafts a person made visible to everyone working there, teammates also recall unreviewed work, marked ([workspaces](../use/workspaces.md#in-progress-workspaces)).
- **Running a server.** Self-hosting needs PostgreSQL and backups; the optional lake takes a directory or an object store. A server someone else runs removes that work, at the price of trusting its operator ([install](../start/install.md)).

## When not to use OML

- Every write must be visible to every agent the moment it is saved.
- Nobody can review. A held memory with no reviewer never becomes readable.
- OML must extract memories from conversations on its own. It does not. Agents write what they choose to remember.
- A profile builder, an answering service or a crew manager is needed. OML returns memories with their provenance and governs what crews remember, not what they do.

## Five short scenarios

Each uses built verbs only. The full flows, with what is designed, are under [Scenarios](../scenarios/index.md).

**Remember a decision and find it next session.**

1. The agent saves the decision through its memory tool. OML answers held.
2. Later it searches and finds its own memory marked unreviewed.
3. A reviewer runs `oml review approve`.
4. Next session, any agent in the workspace finds it.

**Run one writer and one reviewer in a crew.**

1. Issue the writer's key with `oml keys issue-agent`.
2. Issue a reviewer key (role `reviewer`) to a second agent.
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
2. The user adds the server as a custom connector in claude.ai.
3. The user enters the code once on OML's page.
4. Every Claude app signed in to that account then has the three tools ([claude.ai and mobile](../guides/claude-ai-and-mobile.md)).

## Next

Try it in [the quickstart](../start/quickstart.md). Read what OML does not promise in [what OML guarantees](commitments.md).
