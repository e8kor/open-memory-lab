---
status: Stable
verified: 2026-11-06
answers: "what an agent saves and finds with the three tools, how recall is scoped and budgeted, and how a memory is corrected, retired, linked or checked against what is already held"
---

# Save and find memories

An agent holds one key and three tools. `oml_write` saves, `oml_read` finds and lists,
`oml_workarea` says where the agent works and what it may do there. People use the same calls
through `oml write` and `oml read`. An agent can save and find memories. It cannot approve, sign,
demote or delete them ([MCP tools](../api/mcp-plugin.md)).

## Save

`oml_write` takes the text and, optionally, a claim key (the server derives one when omitted), a
kind, the memories it derives from, and a source anchor. Every save is **held**.

- **Duplicates.** A write returns up to three readable memories that already say the same thing.
  Set `on_similar: refuse` to reject the write instead.
- **Secrets.** The scan vetoes leaked keys and holds what a reviewer should judge. A vetoed write
  is stored nowhere and answers `submission.scan_veto` with the rule that fired.
- **When it is true.** A memory can state a valid time. Recall hides what has ended.
- **Where it came from.** A memory carries the source its author declares (repository, commit,
  path, fingerprint) and an `origin.kind` that names its source: an agent, a document
  sync or a chat ([sync](sync.md), [chat memory](chat-memory.md)).

## Find

`oml_read` with a query returns steering rules first, then ranked hits within the workarea's
subtree, within the budget the agent sets (`max_tokens`). Each hit says which words it matched.
A hit holds any of your words, ranked by how many. Add `--match all` (`match: all`) to require
every word.

The agent sees its own held writes with `include_unreviewed`. Everyone else sees a memory after a
non-author approves it. An empty answer does not prove the answer is unknown. Try other words, or
ask the librarian subagent.

Without a query, `oml_read` lists the key's own work by state or the promoted tips of a workarea.
`oml read --state` accepts `held`, `promoted`, `demoted`, `rejected` and `erased`.

## Correct, retire and link

- **Correct.** Write the new text naming the memory it replaces and the version you read
  (`expected_version`). The new version is held, and others keep the old one until it is approved.
- **Retire.** Mark the write a retirement with a reason. Approval removes the memory from recall.
  Every version stays fetchable as history.
- **Link.** A labeled link between two memories (`supports`, `contradicts` and others) is
  proposed by one reviewer and confirmed by another, on the HTTP API and `oml edges`
  ([CLI reference](../cli/reference.md)).
- **Why was it refused or rejected?** `oml_read` lists the agent's own work by state. The
  reviewer's reason goes to the person, not to the agent.

## Scope

A key reads and writes only where it holds a grant. A denial always wins. [Workareas](workareas.md)
sets that scope; [guardrails](guardrails.md) adds reviewed rules every session loads at its
start.
