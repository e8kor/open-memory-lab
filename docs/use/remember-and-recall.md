---
status: Stable
verified: 2026-11-06
answers: "what an agent saves and finds with the three tools, how recall is scoped and budgeted, and how a memory is corrected, retired, linked or checked against what is already held"
---

# Remember and recall

An agent holds one key and three tools. `oml_write` saves, `oml_read` finds and lists,
`oml_workarea` says where the agent works and what it may do there. People use the same calls
through `oml write` and `oml read`. An agent may remember and recall; it may never promote, sign,
demote or delete ([MCP tools](../api/mcp-plugin.md)).

## Save

`oml_write` takes the text and, optionally, a claim key (the server derives one when omitted), a
kind, the memories it derives from, and a source anchor. Every save is **held**.

- **Duplicates.** A write names up to three memories it can read that hold the same words in the
  same order or a near copy, and `on_similar: refuse` turns the write away
  ([ADR-0050](../adr/ADR-0050-remembering-checks-what-is-already-held-replaces-instead-of-repeating-and-says-how-the-memory-will-be-found.md)).
- **Secrets.** The scan vetoes leaked keys and holds what a reviewer should judge; a vetoed write
  is stored nowhere and answers `submission.scan_veto` with the rule that fired
  ([ADR-0065](../adr/ADR-0065-the-scan-is-a-versioned-compiled-in-rule-set-that-vetoes-leaked-keys-and-holds-what-a-reviewer-should-judge.md)).
- **When it is true.** A memory may state a valid time; recall hides what has ended
  ([ADR-0059](../adr/ADR-0059-a-memory-states-when-it-is-true-recall-hides-what-has-ended-and-searches-the-dates-a-query-names-without-a-model.md)).
- **Where it came from.** A memory carries the source its author declares (repository, commit,
  path, fingerprint) and an `origin.kind` that says whether it came from an agent, a document
  sync or a chat ([sync](sync.md), [chat memory](chat-memory.md)).

## Find

`oml_read` with a query returns steering rules first, then ranked hits within the workarea's
subtree, inside the budget the agent sets (`max_tokens`). Each hit says which words it matched.
The agent sees its own held writes with `include_unreviewed`; everyone else sees a memory after a
non-author approves it. Search matches every word of the query unless any-word search is on; an
empty answer is not proof the answer is unknown, so try other words, or ask the read-only
librarian subagent, which runs up to eight searches. Without a query, `oml_read` lists the key's
own work by state (held, approved, rejected) or the promoted tips of a workarea.

## Correct, retire, link

- **Correct.** Write the new text naming the memory it replaces and the version you read
  (`expected_version`); it is held, and others keep the old version until it is approved.
- **Retire.** Mark the write a retirement with a reason. Approval removes the claim from recall;
  every version stays fetchable as history.
- **Link.** A labelled link between two memories (`supports`, `contradicts` and others) is
  proposed by one reviewer and confirmed by another, on the HTTP API and `oml edges`
  ([CLI reference](../cli/reference.md#oml-edges-claim-links-and-the-brief)).
- **Why was it refused or rejected?** `oml_read` lists the agent's own work by state. The
  reviewer's reason goes to the person, not to the agent.

## Scope

A key reads and writes only where it holds a grant, and a denial always wins. [Workareas](workareas.md)
sets that scope; [guardrails](guardrails.md) adds reviewed rules every session loads at its
start.
