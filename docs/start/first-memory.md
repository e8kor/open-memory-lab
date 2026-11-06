<!--
id: start/first-memory
tier: client
status: current
answers: how to save a first memory, why it is held, how to find it, how a reviewer approves it, and when others see it
-->

# Your first memory

You need a connected agent ([Claude Code](../guides/claude-code.md) or [another
tool](../guides/other-harnesses.md)) or the `oml` CLI with a key, and a reviewer other than the
author. About two minutes joining a server that has a workarea for you.

1. **Save.** Ask the agent to remember a decision, or run
   `oml write "We deploy on Tuesdays; Friday deploys need two approvals."`. The answer is
   **held**: scanned, journalled and readable by its author alone. An agent uses the `oml_write`
   tool, which answers the same.
2. **Find it as the author.** `oml read --query "deploy Friday" --include-unreviewed` returns the
   held row, marked unreviewed. Without `--include-unreviewed` the author sees what everyone
   sees, which is nothing yet.
3. **Review it as someone else.** A reviewer key (`dreamer`) takes the next held memory with
   `oml review next`, reads it and its sources, and runs `oml review approve` or
   `oml review reject --reason …`. The server never serves a reviewer their own writes or
   anything derived from them (`review.author_in_lineage`).
4. **Find it as anyone with access.** After approval `oml read --query "deploy Friday"` returns
   it from the workarea and its subtree. The approval changed who can read it, not how it ranks.
5. **Correct it.** `oml write "…" --replaces <artifact_id> --expected-version <n>` writes a new
   version. It is held too: everyone else keeps finding the old version until a reviewer
   approves the new one. Add `--retirement --reason …` to propose retiring a memory instead.

Recall matches memories that contain every word of the query unless any-word search is on
(`--match any`); if nothing is found, try fewer words. Reviewing a long queue is a person's act,
and a person can review a listed set in one command ([reviewing](../guides/reviewing.md)).
