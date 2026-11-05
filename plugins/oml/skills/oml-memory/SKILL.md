---
name: oml-memory
description: Use when a task should store a fact, decision or finding for later sessions or other agents, or when prior shared knowledge might already answer the question - before non-trivial work, on a project term you do not know, and after a command or test fails, recall with the words a note would use or a question, such as an identifier, an error code or a file name. Covers the oml_write, oml_read and oml_workarea tools of an Open Memory Lab server, why a write is not visible right away, and how to revise or retire a memory.
---

# Working with Open Memory Lab memory

An Open Memory Lab server holds every write for review. `oml_write` succeeds with
`state: "held"`: the memory is accepted for review, not published. It becomes visible to
other keys only after a different identity approves it. Plan around that delay rather
than writing the same fact twice.

## One connection, one key

The plugin connects once, as `oml`, with the agent's one key, and every tool runs there.
The key holds a writer grant and a reader grant on its default workarea, so the whole loop
is one connection:

1. recall before you act (`oml_read` with a `query`);
2. write (`oml_write`), which answers `held`;
3. check your held writes (`oml_read` with no `query`, with an `artifact_id`, or with a
   `query` and `include_unreviewed: true`);
4. learn what became of them once a different identity reviews them (`oml_read` with
   `state`);
5. revise or retire what no longer holds (`oml_write` with `replaces`).

## Three tools, chosen by their arguments

`oml_write` stores, `oml_read` reads and `oml_workarea` describes a workarea. The
arguments you pass choose what `oml_write` and `oml_read` do, and the result's `mode` says
which ran:

- `oml_write` without `replaces` writes a new claim; with `replaces` it proposes a new
  version of that claim, and with `replaces` and `retirement: true` it proposes retiring it.
- `oml_read` with `artifact_id` reads one memory, with `query` recalls, and with neither
  lists. Never pass `query` and `artifact_id` together.

An argument outside the call's mode is refused with `request.validation` naming it in
`details.field`: drop that argument and call again.

## Name a workarea

Every argument that names a workarea is `workarea`, on all three tools, and takes its name,
such as `metals-crew`, or its id. A workarea's name is unique in the tenant and never
changes. A path is not a name: pass `metals-crew`, not `universe.procurement.metals-crew`.
A name the key cannot see is refused with `access.not_found`, exactly as its id would be.

## Check what a workarea allows

Call `oml_workarea` with no arguments to describe the key's default workarea, or pass
`workarea` to name another. It returns the workarea's `name`, `path` and `status`, the child
workareas the key can read, each with its name as `path_leaf`, and `my_effective_verbs`: the
verbs the key's grants allow there, such as `remember` or `recall`. A verb missing from the
list is refused in that workarea: with `access.verb_ceiling` when the key's role can never call it,
`access.grant_refused` when the key can read the workarea but holds no grant for the verb,
and `access.not_found` when it cannot read the workarea at all.

## Recall before you act

Call `oml_read` with a `query` before non-trivial work, when the task names a project term
you do not know, and after a command or test fails. Pass the words the memory would contain,
such as an identifier, an error code, a module or a file name, or a question. Any word may
match, an identifier matches by its parts and a word by its stem, each hit's `matched` lists the
words it holds, and a recall that finds little offers spellings in `search.did_you_mean`;
English stop words such as `not` are not read when the query has other words, so a negation
ranks like the words around it; send `match: "all"` when every word must
match. Try other words once before concluding nothing is stored. Recall returns approved memories, with `k` bounding the hits and
`kinds` narrowing them to `memory` or `derived_fact`. Rows under `steering` are standing
instructions from the crew. They come before the hits and are not ranked, so read them first.

Pass `include_unreviewed: true` to search your own held writes as well. They come back
among the hits with `state: "held"` and `tip: false`, and a held revision is listed beside
the approved version it would replace. Recall never returns another key's held writes, and
your own held steering and retirements are not in force until approved, so recall leaves
them out; list them with `oml_read` and no `query`.

Pass `max_tokens` to bound the answer. Every item carries its `tokens`, the server's count
of one token per four bytes of text, rounded up, which is not your model's count. Steering
is never cut. The hits served are the longest run from the top of the ranking that fits,
and `budget` reports the cut: `truncated`, `hits_omitted` and `next_hit_tokens`, the size of
the first hit left out. The same request on the same memories cuts the same way.

The server matches words exactly, without stemming: `quote` does not find `quoted`. Use
the words the memory most likely contains, and try a second query before you conclude
nothing is there.

## See what became of your writes

Call `oml_read` with neither `query` nor `artifact_id` to list what your key wrote, newest
first, in every workarea it wrote to: each item's `state` is `held` while it waits for
review, then `promoted` or `rejected`. To learn whether your writes were approved, pass
`state: "promoted"` or `state: "rejected"`. Items carry the `artifact_id`, `claim_key`,
`version`, `tokens` and `text`. Pass `claim_key`, `kind`, `workarea`, `since` or `until` to
narrow the list. When `next_cursor` is not null, pass it back as `cursor` for the next page.
A cursor works only for the key that received it and expires; when one is refused with
`request.validation`, start again from the first page. With `own: false` the call lists the
approved claims of a workarea instead of your own work.

## Read one memory back

Call `oml_read` with an `artifact_id` to read one version with its `text`. Your own writes
are answered in any state, so you can check a held memory before anyone approves it;
another key's memory is answered only once approved, and anything else is refused with
`access.not_found`. `tip: true` marks the version recall serves for its claim. An older
version stays readable with `tip: false`, and `replaces` names the version a revision
replaced.

## Remember one claim at a time

- `text` is the claim itself, written so a reader with no context can check it. Include
  the source and the date it holds for.
- `idempotency_key` must stay the same when the same write is retried, for example
  `<task>/<run>/<step>`. A retry then returns the stored receipt with
  `idempotent_replay: true` and writes nothing twice.
- Leave `claim_key` out. The server derives one from your key and the text and returns it
  in the result; never invent one. A key you pass that starts with `auto-` is refused.
- Leave `workarea` out unless told otherwise. The key's default workarea applies.

For a conclusion drawn from memories you already hold, pass `derives_from`, the
`artifact_id`s it rests on. Declared lineage keeps a memory's author out of the review of
anything derived from it.

## Revise or retire a memory

Call `oml_write` with `replaces` to propose a new version of a claim whose current version
your key wrote. Pass the `artifact_id` of the version recall serves now, the one with
`tip: true`, the whole new `text`, and optionally a `reason`; do not pass `claim_key`, since
the new version takes its target's. The new version is held like any write, and recall
keeps serving the old one until a different identity approves the new one. If another
version took the tip first, the refusal `submission.tip_conflict` names it in
`details.current_tip`; read it with `oml_read` and its `artifact_id` before you propose
again. Keep the same `idempotency_key` when you retry the same proposal.

To retire a claim that no longer holds, pass `replaces` and `retirement: true` with a
`text` that says why. Once approved, recall stops serving the claim, and every version
stays readable with `oml_read`.

## What the tools cannot do

No tool promotes, approves, deletes or rewrites a memory, and none issues keys. Those
actions belong to other identities on purpose. When a write is refused, the result carries
`isError: true` and an `error` object with `code`, `rule` and `message`. Report the code
and the rule, and do not retry a refusal whose `retryable` is `false`.

No tool attaches or reads a file: MCP carries no file bytes, so files go through the `oml`
CLI or the SDKs, once the server serves uploads. Write the claim's text with `oml_write`.

No tool streams events. To follow what happened after a write, list your own work again
with `oml_read` and `state`; an item stays `held` until a different identity reviews it.
