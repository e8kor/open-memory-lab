---
name: oml-memory
description: Use when a task should store a fact, decision or finding for later sessions or other agents, or when prior shared knowledge might already answer the question. Covers the oml_remember, oml_learn, oml_recall, oml_workarea_info and oml_events_tail tools of an Open Memory Lab server, and why a write is not visible right away.
---

# Working with Open Memory Lab memory

An Open Memory Lab server holds every write for review. `oml_remember` succeeds with
`state: "held"`: the memory is accepted for review, not published. It becomes visible to
other keys only after a different identity approves it. Plan around that delay rather
than writing the same fact twice.

## Two connections, one server

The plugin connects twice. The `oml` connection uses the agent's writer key, so call
`oml_remember` and `oml_learn` there. The `oml-recall` connection uses a reader key, so
call `oml_recall`, `oml_workarea_info` and `oml_events_tail` there. Each connection lists
all five tools, and a tool called on the wrong connection is refused: `oml_remember` and
`oml_learn` with `access.verb_ceiling` on the reader key, and the three reads with
`access.not_found` on a writer key that holds no reader grant.

## Check what a workarea allows

Call `oml_workarea_info` with no arguments to describe the key's default workarea, or pass
`workarea` to name another. It returns the workarea's `path` and `status`, the child
workareas the key can read, and `my_effective_verbs`: the verbs the key's grants allow
there, such as `remember` or `recall`. The answer is about the key behind the connection
you call, and a verb missing from the list is refused in that workarea.

## Recall before you act

Call `oml_recall` with a short query when earlier work might already answer the question.
Recall returns only approved memories, never held ones, including your own. Rows under
`steering` are standing instructions from the crew. They come before the hits and are not
ranked, so read them first.

The server matches words exactly, without stemming: `quote` does not find `quoted`. Use
the words the memory most likely contains, and try a second query before you conclude
nothing is there.

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

## What the tools cannot do

No tool promotes, approves, deletes or rewrites a memory, and none issues keys. Those
actions belong to other identities on purpose. When a write is refused, the result carries
`isError: true` and an `error` object with `code`, `rule` and `message`. Report the code
and the rule, and do not retry a refusal whose `retryable` is `false`.

To follow what happened after a write, poll `oml_events_tail` with the last `seq` you saw
as `after_seq`.
