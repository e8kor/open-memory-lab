# Recall eval

A fixed set of memories and queries that says, with numbers, how well OML's
recall finds what an agent asks for, and where it fails. The findings are in
[REPORT.md](REPORT.md); this page is the data format and how to run it.

## Files

| file | what it is |
|---|---|
| `corpus.json` | 200 memories a crew running a fictional invoicing service ("Tallyline") would store, one checkable claim each |
| `queries.json` | 121 queries, 11 per UX category, each with graded relevance labels, and since data version 2, 22 `unanswerable` queries no memory answers |
| `metrics.py` | the metrics and the table printer; standard library |
| `run.py` | loads the corpus into a live server and runs every query through `oml_read`; standard library |
| `baselines.py` | the offline and direct-PostgreSQL comparison methods; needs a scratch venv |
| `tables.py` | prints `results/tables.md` from `results/*.json` |
| `check_data.py` | CI check: the data parses and every label resolves (job `recall-eval-data`) |
| `served.sh` | boots a lead against a throwaway database, loads the set and runs `run.py` once per served search pipeline, then `gate.py` (job `recall-eval`) |
| `gate.py` | each pipeline's gate (ADR-0049 d9; `bm25-rerank`'s from ADR-0053 d9); fails when the default misses its gate or a category falls by more than one query, or when `keyword-all` does not reproduce v0.5.0 |
| `federated.sh`, `federated.py` | the federated recall eval (ADR-0072 d11): one committed workarea per topic, `sources` scope, wide and the union on a scoped and a cross split, the widen hint, and a skew split padded with SciFact abstracts |
| `fed2.py`, `descriptors.json` | the phase 2 gate (ADR-0072 d4.5, d11; #206), run as `EVAL_PHASE2=1 federated.sh`: the union's ranking per query from the served lead, then `content`, `described`, oracle and ReDDE selection replayed offline on `bm25-postings` and `bm25-sparse`; `descriptors.json` holds one author-written descriptor per topic |
| `results/` | one JSON per method (summary per category, and per query the ranked ids and latency), and `tables.md` |

### `corpus.json`

`{"version": 1, "workarea": "eval-crew", "memories": [{id, type, topic, text}]}`.
`type` is one of finding, decision, convention, gotcha, fact, steering;
`topic` is one of deploy, database, api, auth, frontend, ci, data, billing,
observability, incident, process. Both are labels for reading the results; the
server receives only the text, as kind `memory`, under claim_key
`recall-eval-<id>`. The steering-type rows are written as `memory` too: a row
of kind `steering` is never ranked (it comes back with every recall), so it
would not test search.

The text names no real company, person or secret. Identifiers, paths and
versions are made up but shaped like real ones (snake_case, CamelCase, dotted
paths, URLs, version numbers), because how a tokenizer splits them is one of
the things measured.

### `queries.json`

`{"version": 2, "queries": [{id, category, query, relevant: {memory_id: grade}}]}`.
Grade 2: the memory answers the query. Grade 1: related, useful beside an
answer. Every query has at least one grade-2 memory. Unlabelled memories count
as grade 0.

| category | what the query does |
|---|---|
| `exact` | uses words that appear verbatim in the answer |
| `paraphrase` | says the same thing in other words |
| `inflection` | uses another form of the answer's words (plural, tense, -ing) |
| `partial` | a prefix, fragment or abbreviation (`autoscal`, `otel`, `2fa`) |
| `typo` | one or two misspelled words |
| `identifier` | a code identifier, environment variable, path or version |
| `question` | a natural-language question with stop words |
| `long` | a long question that mixes several concepts |
| `negation` | asks what not to do |
| `temporal` | asks for the latest, current, before or after |
| `multihop` | needs two memories from different topics |
| `unanswerable` | (version 2) asks something no memory says, often in the corpus's own words ("which Redis version runs the rate limiter"); labels nothing |

Unanswerable queries are scored apart: `summary.unanswerable.empty` is the share answered with
no hit, and they never enter `overall`, so the ranking numbers stay comparable with version 1.
They measure a relevance floor (ADR-0053 d6); `eval/bench/rerank_floor.py` sweeps one over a run
made with none.

## Metrics

Over the top `k = 10` hits: **success@1** and **success@5** (a grade-2 memory
in the top 1 or 5), **recall@10** (share of all labelled memories in the top
10), **MRR** (reciprocal rank of the first grade-2 memory), **nDCG@10** (gain
`2^grade - 1`), **zero-result rate** (share of queries with no hits), and
**median latency** per query. Every number is reported per category and
overall.

## Running it

1. A throwaway PostgreSQL with a superuser, and a lead built from this tree
   against it, booted and claimed the way `scripts/poc-demo.sh` does: an
   owner/serving role pair, `OML_POSTGRES__URL`, `OML_POSTGRES__OWNER_URL`,
   `OML_SERVER__LISTEN`, `OML_KMS__KEYRING_PATH`, then `oml bootstrap claim`.
2. A workarea and two keys of different principals, values written to
   owner-only files:

   ```sh
   oml workareas create universe eval-crew
   oml --json keys issue eval-writer --role writer --held-by agent --principal crew/writer \
       --default-workarea eval-crew --grant eval-crew:writer --grant eval-crew:reader | jq -r .value > writer.key
   oml --json keys issue eval-reviewer --role dreamer --held-by human --principal crew/reviewer \
       --default-workarea eval-crew --grant eval-crew:dreamer | jq -r .value > reviewer.key
   ```

3. The served recall, then the same queries minus stop words:

   ```sh
   python3 eval/recall/run.py --url http://127.0.0.1:8437 \
       --writer-key-file writer.key --reviewer-key-file reviewer.key --out eval/recall/results
   python3 eval/recall/run.py ... --keywords
   ```

   The first run writes every memory with the writer key, approves each with
   the reviewer key (a different principal, so the reviewer is never the
   author), and keeps the claim_key-to-artifact map in `results/idmap.json`
   (ignored by git), so later runs skip the load. Queries go through
   `oml_read` over `POST /mcp`, the tool the `oml` plugin calls, with `k = 10`
   and `max_tokens = 2000`.

4. The baselines, from a scratch venv, never the repository's or the system's:

   ```sh
   uv venv /srv/build/eval-venv
   uv pip install --python /srv/build/eval-venv/bin/python \
       rank_bm25 snowballstemmer fastembed "psycopg[binary]"
   /srv/build/eval-venv/bin/python eval/recall/baselines.py --out eval/recall/results \
       --pg-dsn postgresql://postgres@127.0.0.1:55437/oml_eval
   python3 eval/recall/tables.py eval/recall/results > eval/recall/results/tables.md
   ```

   The `pg_*` methods read the loaded server's `memories` table directly as a
   superuser, so they rank exactly the 200 promoted rows the server holds.
   `pg_simple_and` is the served statement and must reproduce `oml`'s scores;
   if it does not, the harness is wrong.

5. Every served search pipeline, gated (ADR-0049), the way CI's `recall-eval` job runs it:

   ```sh
   cargo build --release --bin oml-server-lead --bin oml
   PGHOST=127.0.0.1 PGPORT=5432 PGUSER=oml PGPASSWORD=... eval/recall/served.sh .tmp/recall-eval
   ```

   It writes `served_<pipeline>.json` per pipeline (`run.py --search <pipeline>`) and
   `hook_<pipeline>.json`, the failure hook's own shape (the query minus stop words, `k` 3,
   200 tokens), for the default and for `keyword-all`. `EVAL_PIPELINES` and `EVAL_DEFAULT`
   narrow the run. Copy the JSON into `results/` when a change is meant to move the numbers;
   the gate compares each category with the committed file.

6. The phase 2 gate, with the SciFact archive and the sparse encoder's files:

   ```sh
   scripts/fetch_sparse_model.sh .tmp/oml-models/doc-v3-distill
   EVAL_PHASE2=1 OML_SEARCH__SPARSE__MODEL_DIR=.tmp/oml-models/doc-v3-distill \
       PGHOST=127.0.0.1 PGPORT=... PGUSER=oml PGPASSWORD=... \
       eval/recall/federated.sh .tmp/fed2 path/to/scifact/source.zip
   python3 eval/recall/fed2.py analyze --out .tmp/fed2   # replays from fed2_raw.json alone
   ```

   It writes `fed2_raw.json` (each query's union ranking at depth 100 on `bm25-postings` and
   `sparse`, E(s) per candidate, the served `wide` answer, and the descriptor scores) and
   `fed2.json`, the gate. The readings it takes are ADR-0072's "Phase 2 gate as measured":

   - **The split** is d11's as phase 1 built it, cross and skew together (242 queries) for the
     floor and the scoped split for "scope-only queries".
   - **The descriptors** were written once, from `corpus.json` alone, before `queries.json` was
     read, by the lane that ran the gate. Each is a summary of at most 600 characters and eight
     questions. `about` is empty, since no ontology terms are on main.
   - **The descriptor score** is BM25 over the candidates' descriptors alone. It comes from a
     served `bm25` recall whose `sources` lists one workarea per candidate descriptor, because
     memory-kind-3 postings are not built.
   - **The replay.** A searched set's answer is the union's ranking restricted to it (d7.2).
     `analyze` checks this against the served `wide` answer on every query. `bm25-sparse` is
     refused under `wide`, so its legs are fused over R and restricted after fusion.
   - **α** is the largest value in 0.0 to 1.0, in steps of 0.1, with the best selection recall
     on a seeded random half of the skew split, and it is reported on the other half.
     `descriptor_slots` is its default of 2.
   - **The interval** is a paired percentile bootstrap, 10,000 resamples, seed 20261111.
   - **The adversarial descriptor** holds every word of every query, unbounded by the schema. It
     replaces each topic's descriptor in turn, and the worst topic is reported.

## Changing the set

Edit the JSON, run `python3 eval/recall/check_data.py`, rerun everything, and
regenerate `results/`. Results from different versions of the data are not
comparable, so bump `version` in both files when a label or a memory changes.
