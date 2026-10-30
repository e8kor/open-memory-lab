---
name: oml-memory
description: Use when a task should store a fact, decision or finding for later sessions or other agents, or when prior shared knowledge might already answer the question. Covers the oml_remember, oml_learn, oml_recall, oml_get, oml_supersede, oml_my_work, oml_workarea_info and oml_events_tail tools of an Open Memory Lab server, why a write is not visible right away, and how to revise or retire a memory.
---

# Working with Open Memory Lab memory

An Open Memory Lab server holds every write for review. `oml_remember` succeeds with
`state: "held"`: the memory is accepted for review, not published. It becomes visible to
other keys only after a different identity approves it. Plan around that delay rather
than writing the same fact twice.

## One connection, one key

The plugin connects once, as `oml`, with the agent's one key, and every tool runs there.
The key holds a writer grant and a reader grant on its default workarea, so the whole loop
is one connection:

1. recall before you act (`oml_recall`);
2. remember (`oml_remember`, `oml_learn`), which answers `held`;
3. check your held writes (`oml_my_work`, `oml_get`, `oml_recall` with
   `include_own_held: true`);
4. follow what happens to them once a different identity reviews them (`oml_events_tail`);
5. revise or retire what no longer holds (`oml_supersede`).

## Check what a workarea allows

Call `oml_workarea_info` with no arguments to describe the key's default workarea, or pass
`workarea` to name another. It returns the workarea's `path` and `status`, the child
workareas the key can read, and `my_effective_verbs`: the verbs the key's grants allow
there, such as `remember` or `recall`. A verb missing from the list is refused in that
workarea: with `access.verb_ceiling` when the key's role can never call it,
`access.grant_refused` when the key can read the workarea but holds no grant for the verb,
and `access.not_found` when it cannot read the workarea at all.

## Recall before you act

Call `oml_recall` with a short query when earlier work might already answer the question.
Recall returns approved memories. Rows under `steering` are standing instructions from the
crew. They come before the hits and are not ranked, so read them first.

Pass `include_own_held: true` to search your own held writes as well. They come back among
the hits with `state: "held"` and `tip: false`, and a held revision is listed beside the
approved version it would replace. Recall never returns another key's held writes, and
your own held steering and retirements are not in force until approved, so recall leaves
them out; read them with `oml_my_work` and `oml_get`.

Pass `max_tokens` to bound the answer. Every item carries its `tokens`, the server's count
of one token per four bytes of text, rounded up, which is not your model's count. Steering
is never cut. The hits served are the longest run from the top of the ranking that fits,
and `budget` reports the cut: `truncated`, `hits_omitted` and `next_hit_tokens`, the size of
the first hit left out. The same request on the same memories cuts the same way.

The server matches words exactly, without stemming: `quote` does not find `quoted`. Use
the words the memory most likely contains, and try a second query before you conclude
nothing is there.

## See what became of your writes

Call `oml_my_work` to list what your key wrote, newest first, in every workarea it wrote
to: each item's `state` is `held` while it waits for review, then `promoted` or
`rejected`. Items carry the `artifact_id`, `claim_key`, `version` and `tokens` but not the
text. Pass `state`, `claim_key` or `workarea` to narrow the list. When `next_cursor` is not
null, pass it back as `cursor` for the next page. A cursor works only for the key that
received it and expires; when one is refused with `request.validation`, start again from
the first page.

## Read one memory back

Call `oml_get` with an `artifact_id` to read one version with its `text`. Your own writes
are answered in any state, so you can check a held memory before anyone approves it;
another key's memory is answered only once approved, and anything else is refused with
`access.not_found`. `tip: true` marks the version recall serves for its claim. An older
version stays readable with `tip: false`, and `replaces` names the version a revision
replaced.

## Remember one claim at a time

- `claim_key` names the claim inside its workarea, for example
  `supplier-quote-am-7741`. Reuse a key only for the same claim.
- `text` is the claim itself, written so a reader with no context can check it. Include
  the source and the date it holds for.
- `idempotency_key` must stay the same when the same write is retried, for example
  `<task>/<run>/<step>`. A retry then returns the stored receipt with
  `idempotent_replay: true` and writes nothing twice.
- Leave `workarea` out unless told otherwise. The key's default workarea applies.

Use `oml_learn` for a conclusion drawn from memories you already hold. It requires
`derives_from`, the `artifact_id`s it rests on. Declared lineage keeps a memory's author
out of the review of anything derived from it.

## Revise or retire a memory

Call `oml_supersede` to propose a new version of a claim whose current version your key
wrote. Pass the `artifact_id` of the version recall serves now, the one with `tip: true`,
the whole new `text`, and optionally a `reason`. The new version is held like any write,
and recall keeps serving the old one until a different identity approves the new one. If
another version took the tip first, the refusal `submission.tip_conflict` names it in
`details.current_tip`; read it with `oml_get` before you propose again. Keep the same
`idempotency_key` when you retry the same proposal.

To retire a claim that no longer holds, pass `retirement: true` with a `text` that says
why. Once approved, recall stops serving the claim, and every version stays readable with
`oml_get`.

## What the tools cannot do

No tool promotes, approves, deletes or rewrites a memory, and none issues keys. Those
actions belong to other identities on purpose. When a write is refused, the result carries
`isError: true` and an `error` object with `code`, `rule` and `message`. Report the code
and the rule, and do not retry a refusal whose `retryable` is `false`.

To follow what happened after a write, poll `oml_events_tail` and pass each call's
`next_after_seq` back as the next call's `after_seq`; `idle: true` means you have caught up.
Resume from `next_after_seq`, never from the last `seq` you saw: a page can hold no events
while `next_after_seq` still advances past events this key may not see.
