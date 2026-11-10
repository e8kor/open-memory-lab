---
status: Stable
verified: 2026-11-06
answers: "how recall finds and orders memories: who can see a hit, what you can ask, which search pipelines exist and how one is chosen, how steering and your own held memories join the answer, how a token budget cuts it, and what was measured"
---

# How recall finds and orders memories

A recall returns promoted memories that your key can read, ranked by the search pipeline you chose. Standing instructions (steering) come first and are never ranked. PostgreSQL serves every read. The Delta lake serves none, because a derived copy must not become an authority.

## Who sees a hit

A recall returns promoted memories inside the key's effective access ([workareas](workareas.md)). It hides two kinds of row:

- **Held memories** are visible to their author only, through `oml read` and recall with `--include-unreviewed`.
- **Demoted and erased memories** are not served.

Superseded versions stay readable by id as history. They are never hits.

## What you can ask

Each flag below belongs to `oml read`. The same fields exist on the `oml_read` MCP tool and the [HTTP API](../api/http-api.md).

| You want | Flag | Default |
|---|---|---|
| Ranked hits for a query | `--query "<words>"` | none |
| More or fewer hits | `--k` | 8; range 1 to 100 |
| Only some kinds | `--kinds memory` or `--kinds derived_fact`, repeatable | all hits |
| A different subtree | `--workarea <id or name>` | the key's default workarea |
| A different search pipeline | `--search <name>` | the tenant's default |
| Every word to match | `--match all` | `any` |
| Your own held memories too | `--include-unreviewed` | off |
| Memories anchored in one repository | `--anchor-repo`, `--anchor-path-prefix` | no filter |
| Memories from one origin | `--origin-kind`, repeatable | no filter |
| Workareas beyond the scope | `--sources wide` or `--sources legal,product` | `scope`; needs federation on |

The API also takes `max_tokens` and `include_text` ([token budgets](#token-budgets)). Without `--query`, `oml read` lists instead of searching. Run `oml read --help` for the listing flags.

## How a recall runs

```mermaid
flowchart LR
    Q["Query + key + workarea"] --> S["Scope: readable nodes"]
    S --> ST["Steering in force<br/>first, unranked, never cut"]
    S --> P["Search pipeline<br/>ranked hits"]
    P --> C["k cut, then token budget cut"]
    ST --> A["Answer"]
    C --> A
```

The server decides what you may read before any ranking runs. A pipeline ranks the rows in that scope and nothing else. It never reads a verdict or a reviewer. Statistics such as document frequency come from the rows you can read. A memory you cannot read never moves the rank of one you can.

## Search pipelines

| Pipeline | What it ranks | Offered by default |
|---|---|---|
| `keyword-any` | A hit holds any of your words, exactly | yes |
| `keyword-all` | A hit holds every word, exactly | no |
| `bm25` | BM25 over each word's stored forms: the word, the parts of identifiers such as `useInvoiceFilters.ts`, and English stems | yes |
| `bm25-postings` | The same score as `bm25`, read from an index that works under row security | yes, and the default |
| `bm25-trigram` | `bm25` plus word similarity for misspellings and fragments | no |
| `sparse` | Weights from a local learned model | no |
| `bm25-sparse` | `bm25-postings` and `sparse`, combined | no |
| `bm25-rerank` | `bm25`, with the top hits reordered by a local cross-encoder | no |

The server lists the offered pipelines in `search.enabled` ([configuration](../operations/configuration.md)). A name the server does not offer is refused with `search.unavailable`.

### Choosing one

1. A request names a pipeline with `--search`.
2. Otherwise the tenant's default applies. An admin sets it with `oml search set-default`, and it takes effect on the next request without a restart.
3. Otherwise the server's `search.default` applies.

`oml search show` lists what the server offers and which one a recall will run.

### Any word or every word

The default is `--match any`: a hit holds any of your words. `--match all` keeps only memories that hold every word. When none does, the answer returns the any-word hits and marks `search.fallback: "any_term"`.

### What the answer says

The answer reports the pipeline that ran (`search.pipeline`) and the words it read (`search.terms`). Each hit carries `matched`, the words it holds, and `of`, how many words the query had.

When a recall finds fewer than three hits, `search.did_you_mean` suggests spellings. They come from words in memories you can read, so a suggestion never reveals a word that only an unreadable memory holds. Scores compare within one answer only.

Under `bm25-postings`, a very common word can be left out of the index read. `search.pruned` names every such word, and `pruned: []` means the list is the same as `bm25`'s.

## Order of the answer

A recall returns two segments in a fixed order.

1. **Steering.** Every promoted steering memory of the workarea and its ancestors, in order of depth from `universe`, then promotion time. The query does not rank steering, and the `k` cut and the token budget never remove it. A descendant's steering does not apply at an ancestor.
2. **Hits.** Promoted memories of kind `memory` or `derived_fact`, by score, highest first. Ties break by artifact id.

Steering comes first because a standing instruction that relevance could bury fails when an agent needs it most. Nothing a client sends changes that order.

A recall inside your user root also searches `universe`. Its memories rank with your own, and its steering applies as an ancestor's. See [workareas](workareas.md#your-own-workarea).

### Your own held memories

With `--include-unreviewed`, your key's own held memories join the hits. Each is marked `state: held`. They rank by the same order and count against `k` and the token budget. Held steering and held retirements never appear, because neither is in force yet.

Inside an in-progress workarea, every author's held memories join, labeled unreviewed. See [workareas](workareas.md#in-progress-workareas).

## Token budgets

Pass `max_tokens` to cap an answer. A memory costs its body size in bytes divided by 4, rounded up. The divisor is a convention, not a model's tokenizer count. Every served memory carries its `tokens`.

The cut works in four steps:

1. Steering is served whole, whatever it costs.
2. What remains of the budget is `max_tokens` minus the steering tokens, never below zero.
3. Hits are served in order while they fit.
4. The first hit that does not fit ends the list. The server skips ahead to no smaller hit and serves no partial body.

For example, take steering of 120 tokens, hits of 300, 250 and 90, and `max_tokens` of 500. The answer serves the first hit. It reports `used_tokens` 420, `truncated: true`, `hits_omitted: 2` and `next_hit_tokens: 250`. When steering alone exceeds the budget, the answer still serves it.

`max_tokens` with `include_text: false` is refused with `request.validation`, because it would budget text you did not ask to receive.

## What was measured

The benchmarks run every pipeline through the same served path as an agent. The table shows BEIR SciFact, 5,183 memories and 300 queries, run on 2026-11-05. The [benchmarks page](../benchmarks/index.md) holds every suite with the commit and host of each run.

| Pipeline | nDCG@10 | p95 latency |
|---|---:|---:|
| `keyword-any` | 0.515 | 1,075 ms |
| `bm25` | 0.685 | 116 ms |
| `bm25-postings` (default) | 0.685 | 65.6 ms |
| `bm25-sparse` | 0.730 | 83.7 ms |

`bm25-postings` ranks the same as `bm25` and answers faster because an index serves it under row security. `bm25-sparse` ranks higher but stays opt-in, because its latency at larger scope misses the 50 ms bar. See [benchmarks](../benchmarks/index.md) for the scale runs.

## Searching beyond the scope

`sources` says where a recall searches ([ADR-0072](../adr/ADR-0072-recall-searches-the-trusted-workareas-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md)). It is off until a tenant admin enables federation (`oml search federation --enabled on`); until then `wide` and a list are refused, never answered as `scope`.

| `sources` | Searches | Scores with the statistics of |
|---|---|---|
| `scope` (default) | the workarea's subtree, as before | the subtree |
| `wide` | the subtree, plus the `committed` and `validated` workareas the key reads elsewhere that accept federated reads | the subtree and every such workarea, searched or not |
| a list of names | exactly the listed workareas, answered whole or refused | the listed workareas |

Below the tenant's `max_sources` (8 by default), `wide` searches every such workarea. Above it, it searches those holding the largest share of the query's matching rows, each word weighted by its rarity, ties broken by name; `search.sources.cut` counts the rest. A large workarea ranks high only by holding most of the matches, so `universe` beside a crew of a few hundred memories is chosen on its share, not its size.

The statistics are fixed before any workarea is chosen, so no hit's score depends on which workareas were searched: with `bm25`, the answer for any searched set is the full search's answer restricted to that set. The same query, key, memories and settings give the same answer; selection reads no clock, no randomness and no model.

The answer says what it searched. Under `wide` or a list, `search.sources` lists each workarea searched with `via` (`scope` or `federated`), its estimate and the query words its rows hold, and each hit carries `via` beside its `workarea_path`. A `scope` answer on a federated tenant carries `search.widen` when a workarea the key reads elsewhere holds the query's words at least twice as often per memory as the scope, on at least two of its memories. It names no workarea; a retry with `sources: "wide"` searches them. Every name and number in an answer is of a workarea the key reads.

Each `scope` and `wide` recall of a federated tenant reads the per-workarea word counts of every such workarea, exact and never sampled; a list reads the listed ones. A tenant with federation off reads nothing more. [Workareas](workareas.md#federated-recall) covers the settings and how to make a workarea findable without granting it.

## What search does not do

- **It does not answer questions.** OML returns memories. The agent that recalls them reasons over them.
- **It does not match word forms in the keyword pipelines.** With `keyword-any`, a query for "deploy" does not find "deployments". `bm25` pipelines add English stems and identifier parts.
- **Latency is measured only in the scale benchmarks.** Without a text index under row security, every pipeline but `bm25-postings` costs more as the readable rows grow.
