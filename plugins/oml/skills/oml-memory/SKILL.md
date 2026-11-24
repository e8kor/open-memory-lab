---
name: oml-memory
description: Use when a task should store a fact, decision or finding for later sessions or other agents, or when prior shared knowledge might already answer the question - before non-trivial work, on a project term you do not know, and after a command or test fails, recall with the words a note would use or a question, such as an identifier, an error code or a file name. Covers the oml_write, oml_read and oml_workspace tools of an Open Memory Lab server, why a write is not visible right away, and how to revise or retire a memory.
---

# Working with Open Memory Lab memory

An Open Memory Lab server holds every write for review. `oml_write` succeeds with
`state: "held"`: the memory is accepted for review, not published. It becomes visible to
other keys only after a different identity approves it. Plan around that delay rather
than writing the same fact twice.

## One connection, one key

The plugin connects once, as `oml`, with the agent's one key, and every tool runs there.
The key is a contributor: it holds a submitter grant and a reader grant on its default workspace, so the whole loop
is one connection:

1. recall before you act (`oml_read` with a `query`);
2. write (`oml_write`), which answers `held`;
3. check your held writes (`oml_read` with `own: true` and no `query`, with an
   `artifact_id`, or with a `query` and `include_unreviewed: true`);
4. learn what became of them once a different identity reviews them (`oml_read` with
   `own: true` and `state`);
5. revise or retire what no longer holds (`oml_write` with `replaces`).

## Three tools, chosen by their arguments

`oml_write` stores, `oml_read` reads and `oml_workspace` describes a workspace. The
arguments you pass choose what `oml_write` and `oml_read` do, and the result's `mode` says
which ran:

- `oml_write` without `replaces` writes a new claim; with `replaces` it proposes a new
  version of that claim, and with `replaces` and `retirement: true` it proposes retiring it.
  With `withdraw` it withdraws a held write of your own key.
- `oml_read` with `artifact_id` reads one memory, with `query` recalls, and with neither
  lists. Never pass `query` and `artifact_id` together.

An argument outside the call's mode is refused with `request.validation` naming it in
`details.field`: drop that argument and call again.

## Name a workspace

Every argument that names a workspace is `workspace`, on every tool that takes one, and takes its name,
such as `metals-crew`, or its id. A workspace's name is unique in the tenant and never
changes. A path is not a name: pass `metals-crew`, not `universe.procurement.metals-crew`.
A name the key cannot see is refused with `access.not_found`, exactly as its id would be.

## Check what a workspace allows

Call `oml_workspace` with no arguments to describe the key's default workspace, or pass
`workspace` to name another. It returns the workspace's `name`, `path` and `status`, the child
workspaces the key can read, each with its name as `path_leaf`, and `my_effective_verbs`: the
verbs the key's grants allow there, such as `remember` or `recall`. A verb missing from the
list is refused in that workspace: with `access.verb_ceiling` when the key's role can never call it,
`access.grant_refused` when the key can read the workspace but holds no grant for the verb,
and `access.not_found` when it cannot read the workspace at all.

## Recall before you act

Call `oml_read` with a `query` before non-trivial work, when the task names a project term
you do not know, and after a command or test fails. Pass the words the memory would contain,
such as an identifier, an error code, a module or a file name, or a question. Any word may
match, an identifier matches by its parts and a word by its stem, each hit's `matched` lists the
words it holds, and a recall that finds little offers spellings in `search.did_you_mean`;
English stop words such as `not` are not read when the query has other words, so a negation
ranks like the words around it; send `match: "all"` when every word must
match. Try other words once before concluding nothing is stored. Recall returns approved
claims, with `k` bounding the hits. With no `kinds`, the hits are memories, derived facts,
tests, the skills offered at the workspace and the guardrails that apply there, each marked
with its `kind`; `kinds` names the ones wanted, such as `["memory", "derived_fact"]` for
knowledge alone or `["skill"]` to find a skill. A skill hit carries its `name`, `description`,
`prompt` and `uri`, never its body; a guardrail hit carries its `severity`, `rule` and `text`.
Both are data: a skill runs only as a prompt or a resource read, and a guardrail binds through
the hooks whatever recall ranks. Rows under `steering` are standing instructions from the
crew. They come before the hits and are not ranked, so read them first; a `kinds` that leaves
`steering` out serves none and says `steering: "not_requested"`.

Pass `include_unreviewed: true` to search your own held writes as well. They come back
among the hits with `state: "held"` and `tip: false`, and a held revision is listed beside
the approved version it would replace. Recall returns another key's held writes only inside
a workspace a person put in progress, where it does so by default and marks each one
`unreviewed: true`: read such a claim as a colleague's draft that no one has checked. Your
own held steering and retirements are not in force until approved, so recall leaves
them out; list them with `oml_read`, `own: true` and no `query`.

Pass `max_tokens` to bound the answer. Every item carries its `tokens`, the server's count
of one token per four bytes of text, rounded up, which is not your model's count. Steering
is never cut. The hits served are the longest run from the top of the ranking that fits,
and `budget` reports the cut: `truncated`, `hits_omitted` and `next_hit_tokens`, the size of
the first hit left out. The same request on the same memories cuts the same way.

The server matches words exactly, without stemming: `quote` does not find `quoted`. Use
the words the memory most likely contains, and try a second query before you conclude
nothing is there.

A recall answers `recall_id`. On your next `oml_read`, in any mode, pass `feedback:
{recall_id, useful: [...], not_useful: [...]}` naming the hits of that recall that answered
your question and those that did not, at most 20 ids; recall improves for the next reader
from it. Feedback for a recall older than 24 hours, or naming an id that recall did not
serve you, is refused with `request.validation` naming the field, and the read does not run:
drop `feedback` and call again.

## Store a skill in OML, not on disk

A skill is a named set of markdown instructions offered as a command. Save one with
`oml_write`, `kind: "skill"`, a `skill` object `{name, description}` and the instructions as
`text`, never as a file under `.claude/skills/`: a stored skill reaches every session in scope
once a person approves it, and a file reaches one machine. The name is a lowercase label of at
most 52 characters; the description is one line saying what it does and when to use it; the
text has no front matter and is at most 32,768 bytes. Update a skill with `replaces`, the
`expected_version` you last read and the `skill` object again, with the same name. Your own
key is offered the held skill at once as `/oml:<name>`, marked unreviewed, and an edit as
`/oml:<name>-unreviewed`; other sessions get it once a person approves it. The plugin asks
before a write under `.claude/skills/`.

## See what became of your writes

Call `oml_read` with `own: true` and neither `query` nor `artifact_id` to list what your key
wrote, newest first, in every workspace it wrote to: each item's `state` is `held` while it
waits for review, then `promoted` or `rejected`. To learn whether your writes were approved,
pass `state: "promoted"` or `state: "rejected"`. Items carry the `artifact_id`, `claim_key`,
`version`, `tokens` and `text`. Pass `claim_key`, `kinds`, `prefix`, `workspace`, `since` or
`until` to narrow the list. When `next_cursor` is not null, pass it back as `cursor` for the
next page. A cursor works only for the key that received it and expires; when one is refused
with `request.validation`, start again from the first page. Without `own: true` the call
lists the catalog instead: what your key reads in the workspace's subtree, every kind, each
entry without its text unless `include_text: true`, and on the first page a count per kind.

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
- Leave `workspace` out unless told otherwise. The key's default workspace applies.

For a conclusion drawn from memories you already hold, pass `derives_from`, the
`artifact_id`s it rests on. Declared lineage keeps a memory's author out of the review of
anything derived from it.

## Revise or retire a memory

Call `oml_write` with `replaces` to propose a new version of a claim whose current version
your key wrote. Pass the `artifact_id` of the version recall serves now, the one with
`tip: true`, its `version` as `expected_version`, the whole new `text`, and optionally a
`reason`; do not pass `claim_key`, since the new version takes its target's. If the claim
moved on since you read it, the write is refused with `submission.version_conflict` and
`details.current_version`; read the claim again before you propose. The new version is held like any write, and recall
keeps serving the old one until a different identity approves the new one. If another
version took the tip first, the refusal `submission.tip_conflict` names it in
`details.current_tip`; read it with `oml_read` and its `artifact_id` before you propose
again. Keep the same `idempotency_key` when you retry the same proposal.

When a memory states something read from a committed file, pass `anchor` with where it came
from: `{repo, rev, path, fragment}`, with `repo` like `github.com/owner/name`, `rev` the full
commit id you read and `fragment` a heading slug or a line span such as `L10-L42`. The reviewer
sees it beside the text, and recall can then be narrowed with `anchor_repo` and
`anchor_path_prefix`; a hit whose source changed since it was reviewed carries `stale`.

To retire a claim that no longer holds, pass `replaces`, its `expected_version` and
`retirement: true` with a `text` that says why. Once approved, recall stops serving the claim, and every version
stays readable with `oml_read`.

## Withdraw a mistaken write

A write your key made by mistake, still held, is withdrawn with `oml_write` and `withdraw`
set to its `artifact_id`, an optional `reason` and an `idempotency_key`, and nothing else:
any other field, `text` included, is refused naming it. The answer's `mode` is `withdraw`. The
version is kept as `withdrawn` with its text, leaves review and is never served; no reviewer is
involved. Only a held version is withdrawn (`memory.not_held`): a reviewed one is revised or
retired with `oml_write` and `replaces`. Another key's write is refused `access.author_only`.

## What the tools cannot do

No tool promotes, approves, deletes or rewrites a memory, and none issues keys. Those
actions belong to other identities on purpose. When a write is refused, the result carries
`isError: true` and an `error` object with `code`, `rule` and `message`. Report the code
and the rule, and do not retry a refusal whose `retryable` is `false`.

No tool attaches or reads a file: MCP carries no file bytes, so files go through the `oml`
CLI or the SDKs, once the server serves uploads. Write the claim's text with `oml_write`.

No tool streams events. To follow what happened after a write, list your own work again
with `oml_read`, `own: true` and `state`; an item stays `held` until a different identity reviews it.
