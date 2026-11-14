---
status: Stable
verified: 2026-11-14
answers: "when two recalls return the same answer: the inputs that define a recall's result under the default pipeline, what changes it between calls, what happens when a memory is approved between two calls, which parts of an answer are outside the guarantee, and how it is tested"
---

# When a recall repeats

Under the default pipeline, `bm25-postings`, the same recall returns the same answer. The recall is the one per-question read of the [serving view](views.md#logical-one-agent-key-and-everything-it-is-served). The steering, the hits, their order and their scores are identical byte for byte, call after call, on one server process or on two that share a database. `recall_id` always differs, because it names the call. Two other members can differ where the server serves them: `timings`, which is served only when the request asks, and `search.widen`, a hint on a tenant with federation on that reads a per-process signal up to 300 seconds old and is never carried when the request sends `now`. Neither changes the hits.

The test behind this page ran on a small tenant, 14 approved memories and two steering rows, whose queries PostgreSQL planned serially. A score is a sum whose order of addition PostgreSQL chooses, and a parallel plan's worker order can vary between runs of the same plan. So the last digit of a score, and with it the order of two hits whose scores differ by one `float4` step, is not proven identical across query plans. That is not observed and not excluded; [issue #310](https://github.com/e8kor/open-memory-lab/issues/310) tracks proving it at 100,000 rows.

This page lists what "the same recall" means, what changes the answer, and what the guarantee leaves out. [Retrieval](retrieval.md) explains how the answer is ranked.

## What defines the answer

A recall's answer, apart from the members listed under [what is outside the guarantee](#what-is-outside-the-guarantee), is a function of these inputs and nothing else.

| Input | What it is |
|---|---|
| The request | The query after Unicode normalization (NFC), and every other field sent: `workarea`, `k`, `kinds`, `match`, `include_text`, `max_tokens`, `include_unreviewed`, `include_expired`, the anchor and origin filters, `sources`, `as_of` and `search`. |
| The key | Its grants and denials, and its default workarea when the request names none. They decide which workareas the recall reads. |
| The approved set | The approved memories in those workareas: their text, kind, valid time, source checks, and which version of each claim is current. It includes the steering in force on the workarea and its ancestors. |
| `now` | The instant the recall is read at. It decides which memories have ended, and what a relative date in the query means. |
| The settings | The pipeline that runs: the tenant's default, or the server's `search.default`. On a tenant with federation on, its federation settings too. |
| The server version | The pipeline's code. The stored word forms of a memory are computed once, when it is written. |

The default pipeline reads no model, no clock other than `now`, no random number and no record of earlier recalls. Scores use word statistics taken from the memories the key can read, inside one database snapshot. Hits are ordered by score, highest first, and equal scores by artifact id. Steering is ordered by depth from `universe`, then approval time, then artifact id.

## What changes the answer

Each row below changes one of the inputs. Until one of them happens, the answer stays the same.

| Change | Effect on the next recall |
|---|---|
| A memory in a readable workarea is approved, superseded, retired or demoted | The approved set changed. See [the approved set changes](#when-the-approved-set-changes-between-two-calls). |
| A source check is recorded on a hit | The hit's `stale` and `source_checked` change. The order does not. |
| A grant or denial on the key changes, or a workarea is put in progress or committed | The recall reads a different set of workareas. |
| The clock passes a memory's `valid_until`, and the request sent no `now` | The memory stops being a hit. |
| The query names a relative date, such as "last week", and the request sent no `now` | `search.window` and the hits follow the clock. |
| An admin changes the tenant's default pipeline, or an operator changes `search.default` or `search.enabled` | Another pipeline ranks the hits. `search.pipeline` names it. |
| The server is upgraded to a version that changes the pipeline | Scores or order can change. The release notes say so. |

Two requests that differ in any field are two recalls. A different `k` or `max_tokens` cuts the same ranked list at a different place.

### Send `now` to pin the clock

Without `now`, the server reads its own clock once per recall. Two server processes read two clocks. A memory that ends between the two readings is a hit in one answer and absent from the other.

A request that sends `now` is read at that instant on every process. Send the same `now` to compare two answers, or to repeat a recall later. `now` does not reach into the past of the approved set: for that, send `as_of`.

## When the approved set changes between two calls

A held memory changes nothing. While a write waits for review, every other key's recall returns the same bytes as before the write. The hits, their scores and their order do not move.

An approval changes the answer once. The next recall reads the new approved set, and every recall after it repeats that new answer until the set changes again. The change can reach more than one hit:

- The approved memory can become a hit and push another hit below the `k` cut or the token budget.
- Scores of other hits can change, because the word statistics count the new memory. Scores compare within one answer only.
- A supersession replaces the old version with the new one. A retirement removes the claim's hits.

The ranking reads one snapshot of the database, and each ranked row is checked again when it is served. An approval that commits while a recall runs can be missing from that answer. The next recall has it.

Two cases serve unapproved memories, so a write changes the answer at once:

- With `include_unreviewed`, the key's own held memories join the hits.
- Inside an [in-progress workarea](workspaces.md#in-progress-workareas), every author's held memories and derived facts that the scan did not hold join, labeled `unreviewed`.

## What is outside the guarantee

| Part of the answer | Why it differs |
|---|---|
| `recall_id` | It names the call. Feedback on a recall quotes it. |
| `timings` | Each stage's duration, served only when the request asks and the server sets `search.timings`. |
| `search.widen` | A hint on a tenant with federation on. It is read from counts the server process built in the background, at most 300 seconds old. One call can carry it and the next not. A recall that sends `now` never carries it. The hits do not depend on it. |

Other pipelines read more than the inputs above:

| Pipeline | What else it reads |
|---|---|
| `sparse`, `bm25-sparse`, `bm25-sparse-rrf` | Weights a local model computes after each write. Until the encoder reaches a new memory, it ranks without them. A different model gives different weights. |
| `bm25-rerank` | A local cross-encoder, under a time limit. When it is busy or late, the answer keeps the first stage's order and says `search.rerank: "skipped"`. |
| `bm25-postings-time` | The memories' valid time, against the date window the query names, relative to `now`. |
| `bm25-trigram`, `bm25`, `keyword-any`, `keyword-all` | They read no model. The test below does not cover them. |

A recall with `as_of` reads the scope at that instant. The swap is made per list: the `bm25` scan reads each `bm25-postings` list and each `sparse` list, because their counts and weights are today's. So `bm25-postings` and `sparse` are swapped, and so are the pipelines that fuse those lists: `bm25-sparse`, `bm25-sparse-rrf` and `bm25-postings-time`. `keyword-all` is answered by `keyword-any` with every word required. `bm25`, `bm25-rerank`, `bm25-trigram` and `keyword-any` read their own lists as they are. The test below does not cover a recall with `as_of`.

A recall that names `sources: "wide"` or a list reads more workareas, chosen without a clock, a random number or a model. The same inputs choose the same workareas ([ADR-0072](../adr/ADR-0072-recall-searches-the-trusted-workspaces-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md) d9).

## How it is tested

`crates/bins/oml-server-lead/tests/repeatable_recall.rs` starts the server binary over a throwaway database and compares whole response bodies. It replaces the value of `recall_id` and nothing else. It does not sort, round or re-serialize an answer before comparing.

- Eight recalls, each sent 20 times to one process: a plain query, a deeper `k`, `match: all`, a token budget, `include_text: false`, a child workarea, a misspelt query and a sent `now`. The memories include rows whose scores tie.
- The same recalls sent to a second process over the same database while the first still serves.
- The same recalls sent to a process started after the first stopped.
- A held write, which leaves another key's answer unchanged, then its approval, which changes it once.
- A memory with a `valid_until`, recalled at a `now` before and after it, from both processes.

The test runs on 14 approved memories and two steering rows at two depths. The author ran it on PostgreSQL 18.6; CI runs PostgreSQL 17. The PostgreSQL version and the platform's maths library are inputs this page does not hold fixed. It was not run on a large tenant, where PostgreSQL can choose another query plan for the same statement. A score is a sum of one number per matched query word. With three or more matched words, the plan chooses the order of that sum, and a parallel plan's worker order can vary between runs of the same plan. So the last digit of a score, and the order of two hits whose scores differ by one `float4` step, is not proven identical across plans or PostgreSQL versions. It is not observed and not excluded; [issue #310](https://github.com/e8kor/open-memory-lab/issues/310) tracks proving it.

The decisions behind this page are [ADR-0049](../adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md) d3 and d12, [ADR-0052](../adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) d6 and [ADR-0072](../adr/ADR-0072-recall-searches-the-trusted-workspaces-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md) d9.
