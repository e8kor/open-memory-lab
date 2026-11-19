---
status: Stable
verified: 2026-11-19
answers: "what an agent saves and finds with the four tools, how recall is scoped and budgeted, and how a memory is corrected, withdrawn, retired, linked or checked against what is already held"
---

# Write and read memories

An agent holds one key and four tools. `oml_write` saves, `oml_read` finds and lists,
`oml_workspace` says where the agent works and what it may do there, and `oml_withdraw` retracts
a held write the key made by mistake, kept as `withdrawn` and never served. People use the same calls
through `oml write`, `oml read` and `oml withdraw`. An agent can save and find memories. It cannot approve, sign,
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

`oml_read` with a query returns steering rules first, then ranked hits within the workspace's
subtree, within the budget the agent sets (`max_tokens`). Each hit says which words it matched.
A hit holds any of the words, ranked by how many. Add `--match all` (`match: all`) to require
every word.

The agent sees its own held writes with `include_unreviewed`. Everyone else sees a memory after a
non-author approves it. An empty answer does not prove the answer is unknown. Try other words, or
ask the librarian subagent.

**Search by kind** ([ADR-0092](../adr/ADR-0092-search-filters-by-every-claim-kind-and-ranks-skills-and-guardrails-as-data-and-a-read-without-a-query-is-the-catalog-of-what-a-key-reads.md)).
`kinds` takes any kind but tests: memories, derived facts, documents, steering, guardrails,
skills and the three vocabulary kinds. A test case is never a hit, so `kinds: ["test"]` is
refused; the catalog lists tests. A recall that names none returns memories, derived facts,
documents, the skills offered at the workspace and the guardrails that apply there,
each marked with its kind; vocabulary is a hit only when named. Steering is served first and
unranked unless `kinds` leaves it out, and then the answer says `steering: "not_requested"`. A
skill hit carries its name, description, prompt and resource URI, never its body; a guardrail hit
carries its text, severity and rule. Both are data, like every recalled text: a skill runs only as
a prompt or a resource read, and a guardrail binds only through the guardrail read,
`GET /v1/guardrails`, the plugin's hooks that load it and the `guardrails` prompt. A document
hit is its best passage, with the passage's lines as its text and its `uri`.

**The catalog.** Without a query, `oml_read` lists the catalog: what the key reads in the
workspace's subtree, across kinds, newest first. `kinds`, `workspace`, `prefix` (a claim key
prefix such as `skill/`) and `state` filter it. Each entry carries its name or a 160-character
preview, workspace, version and approval, and no text unless `include_text` asks; the first page
carries a count per kind. `own: true` lists the key's own work instead, in every state, with its
text. On the CLI, `oml read` without `--query` prints the counts line, then the catalog;
`--own` lists the key's own work, `--kinds` and `--prefix` filter, and `--text` prints each
entry's text.
`oml read --state` accepts `held`, `promoted`, `demoted`, `rejected`, `erased` and `withdrawn`.

## Correct, withdraw, retire and link

- **Correct.** Write the new text naming the memory it replaces and the version read
  (`expected_version`). The new version is held, and others keep the old one until it is approved.
- **Withdraw.** A held write the key made by mistake is taken back with `oml_withdraw` and its
  `artifact_id`, or `oml withdraw <artifact_id> --reason <why>`. The version is kept as
  `withdrawn` with its text, leaves every review queue and is never served; no reviewer is
  involved. Over MCP and the CLI only the author key withdraws, and only a held version: an approved one answers
  `memory.not_held`, another key's `access.author_only`. A person withdraws a held write of any of
  their keys from the [web console](console.md#withdraw-a-mistaken-write)
  ([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md) d13).
- **Retire.** Mark the write a retirement with a reason. Approval removes the memory from recall.
  Every version stays fetchable as history.
- **Link.** A labeled link between two memories (`supports`, `contradicts` and others) is
  proposed by one reviewer and confirmed by another, on the HTTP API and `oml edges`
  ([CLI reference](../cli/reference.md)).
- **Why was it refused or rejected?** `oml_read` with `own: true` lists the agent's own work by state. The
  reviewer's reason goes to the person, not to the agent.

## Scope

A key reads and writes only where it holds a grant. A denial always wins. [Workspaces](workspaces.md)
sets that scope; [guardrails](guardrails.md) adds reviewed rules every session loads at its
start.
