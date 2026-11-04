# Recall eval

A fixed set of memories and queries that says, with numbers, how well OML's
recall finds what an agent asks for, and where it fails. The findings are in
[REPORT.md](REPORT.md); this page is the data format and how to run it.

## Files

| file | what it is |
|---|---|
| `corpus.json` | 200 memories a crew running a fictional invoicing service ("Tallyline") would store, one checkable claim each |
| `queries.json` | 121 queries, 11 per UX category, each with graded relevance labels |
| `metrics.py` | the metrics and the table printer; standard library |
| `run.py` | loads the corpus into a live server and runs every query through `oml_read`; standard library |
| `baselines.py` | the offline and direct-PostgreSQL comparison methods; needs a scratch venv |
| `tables.py` | prints `results/tables.md` from `results/*.json` |
| `check_data.py` | CI check: the data parses and every label resolves (job `recall-eval-data`) |
| `served.sh` | boots a lead against a throwaway database, loads the set and runs `run.py` once per served search pipeline, then `gate.py` (job `recall-eval`) |
| `gate.py` | each pipeline's gate (ADR-0049 d9); fails when the default misses its gate or a category falls by more than one query, or when `keyword-all` does not reproduce v0.5.0 |
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

`{"version": 1, "queries": [{id, category, query, relevant: {memory_id: grade}}]}`.
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

## Changing the set

Edit the JSON, run `python3 eval/recall/check_data.py`, rerun everything, and
regenerate `results/`. Results from different versions of the data are not
comparable, so bump `version` in both files when a label or a memory changes.
