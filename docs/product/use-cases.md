<!--
id: product/use-cases
tier: client
status: current
answers: what people and agents do with Open Memory Lab, as short scenarios of five steps or fewer, who each is for, and which page holds the detail
-->

# Use cases

Each scenario has five steps or fewer. An agent step is one tool call; a person's step is one
command. Agents use three tools: `oml_write` saves, `oml_read` finds, `oml_workarea` says where the
agent works and what it may do there. Every save is **held**: its author reads it at once, and
everyone else finds it only after a reviewer who did not write it approves it. Every scenario
that saves has a reviewer somewhere in it.

## For agents and the people they work for

**Remember a decision and find it next session.**

1. The agent saves the decision with `oml_write`; OML answers **held**.
2. Later in the session it searches with `oml_read`, including its own unreviewed memories, and
   finds it marked unreviewed.
3. A reviewer approves it with `oml review approve`.
4. Next session, any agent in the workarea finds it with `oml_read`.

**Check before acting.** Before non-trivial work the agent searches with a few words about the
task. The workarea's standing instructions come first, then the matches, within the budget the
agent set. An empty answer is not proof the answer is unknown: it tries other words or asks the
librarian subagent, then proceeds and saves what it learns.

**Correct or retire a memory.** The agent finds the memory, saves the corrected text naming the
memory it replaces (or marks the write a retirement with a reason), and a reviewer approves it.
Until then everyone else keeps finding the old version, which stays in the history afterwards.

**Learn whether a note was approved.** The agent lists its own work with `oml_read` by state:
held, approved or rejected. The reviewer's reason goes to the person, not the agent.

## For administrators

**Two agents in one crew: one writes, one reviews.** Issue the writer's key with
`oml keys issue-agent` and a reviewer key to the second agent. The reviewer takes the next held
memory with `oml review next`, which never serves its own writes, and approves or rejects with a
reason ([first workarea and keys](../start/first-workarea.md)).

**Give a teammate's agent its key.** Run `oml keys issue-agent … --out <file>`; the key goes into a
private file and never onto the screen. The teammate runs the printed
`oml setup claude-code --key-file <file>` line and deletes the file.

**Try OML on a laptop.** With Docker, `oml quickstart` starts a server, a workarea and both keys,
and checks one save end to end ([run your own server](../start/run-your-own.md)).

**Use OML from claude.ai or a phone.** The administrator issues a connector key with a one-time
connect code, the person adds the server as a custom connector, enters the code once on OML's
page, and every Claude app signed in to that account then has the three tools. The server needs a
public HTTPS address ([claude.ai and mobile](../guides/claude-ai-and-mobile.md)).

## For teams

- **Share work before review.** An in-progress development workarea lets teammates recall each
  other's unreviewed work, marked, and holds it inside until a person commits it
  ([in-progress workareas](../use/in-progress.md)).
- **Give agents standing rules.** Reviewed guardrails load at every session start and are checked
  before matching tool calls ([rules for agents](../use/guardrails.md)).
- **Keep memory in step with documents.** `oml sync` loads a repository's decision records or
  docs as anchored memories, reviewed in bulk ([sync](../use/sync.md)).
- **Remember what was said in chat.** The facts an agent writes from a conversation go to a
  person's own chat workarea ([chat memory](../use/chat-memory.md)).
- **Answer an auditor.** `oml audit` exports, per memory, who wrote it, who reviewed it, who
  approved it and under what policy, with a chain a separate command verifies
  ([audit and lineage](../architecture/audit-and-lineage.md), [compliance](compliance.md)).

Features still to come are open [issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap).
