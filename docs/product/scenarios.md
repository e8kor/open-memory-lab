<!--
id: product/scenarios
tier: client
status: draft for review · describes v0.4.0
answers: what people and agents do with Open Memory Lab, as eight short scenarios of five steps or fewer, one call per step, on the three agent tools; who each is for, what each step shows, and whether it works since v0.3.1 or since v0.4.0
-->

# Quick scenarios

**Status:** draft for review · describes v0.4.0

Eight scenarios cover what people and agents do with Open Memory Lab (OML). Each has five steps
or fewer, and each step is one call: one tool call by an agent, or one command by a person.
Agents use three tools: `oml_write` saves, `oml_read` finds, and `oml_workarea` says where the
agent works and what it may do there. People use the `oml` command. This page is the quick read;
the [use cases](use-cases.md) catalogue gives every use case one at a time, with its goal, its
story and its status, and the [use-case call map](../engineering/use-case-call-map.md) is the
engineering companion.

## How to read the status

| Status | Means |
|---|---|
| **Available now** | Works since v0.3.1 or earlier, under its older tool names. |
| **In v0.4.0** | Introduced in v0.4.0, the current release. |

Nothing on this page has been measured. Where a step depends on how fast or how well something
works, such as how good search results are, the [roadmap](roadmap.md) holds the open question.

## One rule behind every scenario

Every save is **held**. Its author can read it at once; everyone else finds it only after a
reviewer who did not write it approves it. That is why a fact an agent just saved does not show up
for a teammate yet, and why every scenario that saves has a reviewer somewhere in it.

## 1. Remember a decision and find it next session

**In v0.4.0** (available now under the older tool names) · an agent, with the person it works for

1. The agent saves the decision with `oml_write`. OML answers **held**.
2. Later in the session, the agent searches with `oml_read`, asking to include its own unreviewed
   memories, and finds it, marked as unreviewed.
3. A reviewer approves it with `oml review approve`.
4. Next session, any agent in the workarea finds it with `oml_read`.

## 2. Check before acting

**In v0.4.0** · an agent

1. Before non-trivial work, the agent searches with `oml_read` and a few words about the task.
   The workarea's standing instructions come first, then the matches, within the budget it set.
2. When nothing is found, that is not proof the answer is unknown: search matches words exactly,
   so the agent tries other words, or asks the librarian, which runs up to eight searches for it.
   Then it proceeds, and saves what it learns.

## 3. Correct or retire a memory

**In v0.4.0** (available now under the older tool names) · an agent

1. The agent finds the memory with `oml_read`.
2. It saves the corrected text with `oml_write`, naming the memory it replaces. To retire it
   instead, it says why and marks the write as a retirement. Either way OML answers **held**.
3. A reviewer approves it. Until then everyone else keeps finding the old version; after, the new
   one, or nothing for a retired memory. The old version stays in the history.

## 4. Learn whether a note was approved

**In v0.4.0** · an agent

1. The agent lists its own work with `oml_read`, by state: held, approved or rejected.

OML does not show the agent a reviewer's reason for a rejection; the reviewer tells the person.

## 5. Two agents in one crew: one writes, one reviews

**In v0.4.0** (available now with the older review command) · an administrator, two agents

1. The administrator issues an agent key with `oml keys issue-agent`, and a reviewer key for the
   second agent.
2. The first agent saves with `oml_write`.
3. The reviewer takes the next held memory with `oml review next`. It is never served its own
   writes.
4. It approves with `oml review approve`, or rejects with `oml review reject` and a reason.

## 6. Give a teammate's agent its key

**In v0.4.0** · an administrator, a teammate

1. The administrator runs `oml keys issue-agent … --out <file>`. The key goes into a private file
   and never onto the screen, and the command prints the line for the teammate.
2. The teammate runs that line, `oml setup claude-code --key-file <file>`, which installs the OML
   plugin and gives it the address and the key.
3. The teammate deletes the file.

## 7. Try OML on a laptop

**In v0.4.0** · anyone with Docker

1. `oml quickstart` starts a server, creates a workarea, issues an agent key and a reviewer key,
   and checks one save end to end.
2. The printed `oml setup claude-code` line connects Claude Code on the same machine.
3. Scenario 1 works from there.

## 8. Use OML from claude.ai or a phone

**In v0.4.0** · a person with a claude.ai account, and an administrator

1. The administrator issues a key with a one-time connect code.
2. The person adds the server's address as a custom connector on claude.ai.
3. They enter the code once on OML's page, which shows what the connection may do.
4. Every Claude app signed in to that account, the phone apps included, then has the three tools.

The server needs a public HTTPS address for this. No connection from claude.ai itself has been
made yet ([claude.ai and mobile](../guides/claude-ai-and-mobile.md)).

## Planned next

Not built, each with its release on [releases](releases.md): team rules checked before each tool
call (v0.5.1), and a workarea that moves from development to committed to validated (v0.5.2); teammates
seeing each other's unreviewed work inside a workarea that opts in (v0.6.0); files attached to a
memory, erasure, and one export of who wrote and approved a memory (not scheduled).
