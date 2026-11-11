---
status: Stable
verified: 2026-11-06
answers: "what each OML release changed for users and operators, newest first"
---

# Release notes

Newest first. Each release is a signed tag on the [releases page](https://github.com/e8kor/open-memory-lab/releases).

## 0.7.0

Released 2026-11-11.

**The web console misses its latency budget in this release.** ADR-0070 allows the web console 5 ms p95 over the API it calls. 16 of 21 pages miss it by 6.2 to 42.7 ms p95 (10.1 to 31.2 ms with 48 workareas), because every page opens with two `whoami` calls of 8 to 17 ms each ([Web console overhead](benchmarks/web.md)). v0.7.0 ships with the miss by the operator's decision of 2026-11-11, and v0.7.1 fixes it with fewer calls per page ([#286](https://github.com/e8kor/open-memory-lab/issues/286)).

- Changed the web console to run as its own process, `oml-web`, over the public HTTP API. Every page moved to a path of its own under `/web/` (Guardrails, Workareas, Memories, Atlas, Review, Keys, one memory, Curate, Start, setup, people and users), and the server answers the old `?view=` addresses with a redirect, which v0.7.1 removes. The server keeps only sign-in, the sign-up terms page and sign-out. Each release ships `oml-web-X.Y.Z-<target>.tar.gz`, the image carries `/usr/local/bin/oml-web`, and `docker-compose.yml`, `oml quickstart` and the Helm chart start it behind a Caddy router.
- Added the API operations the console calls: ten new ones, among them `GET /v2/keys`, `GET /v2/review/queue`, `GET /v2/workareas/{workarea}/offers` and the admin's user acts, and new fields on six more. A person's web session reaches the API only through `oml-web`, which proves itself with the web client secret. A console act claims its `Idempotency-Key` before it acts, so a form sent twice acts once and the resubmission replays the first answer.
- Added as-of reads: `as_of` on reads by id, listings, recall and `oml_read` returns memory as it was recorded at a past instant, under today's access.
- Added recall across the workareas a key already reads: `sources: "wide"` or a list of workareas, off until a tenant admin turns federation on (`oml search federation`). A `scope` recall carries `search.widen` when a wider search would find more.
- Added every scan finding on a held memory to its review card and to `POST /v2/review/next` and `GET /v2/review/held`, where a version stored only the first before. `oml admin rescan --rates` prints finding rates per class and origin, and `crates/oml-scan/tests/corpus/` holds an 802-sample red-team baseline.
- Changed a recall under a tenant default pipeline the server no longer serves to run the server's default and say so in `search.fallback: "tenant_default_unserved"`; `oml doctor` warns `search_tenant_default`. The retrieval page recommends `bm25-sparse` where its model is loaded (success@5 0.983 against 0.934 on the recall eval), at the cost of a 268 MB model, CPU at write time and a higher p95 past a few thousand memories. `bm25-postings` stays the default.
- Added `eval/recall/selfhost.sh`, which runs the recall eval on your own machine; CI runs the same script.
- Changed the landing pages to ship apart, built from `site/` as `oml-site-<version>.tar.gz`, and to say what the scan refuses and what it records as a finding.
- Fixed a web console disable or deletion close that met a concurrent write, which journalled its events twice.
- Added design records and measurements, none of them built: releases that name no server (ADR-0076, accepted); questions generated per memory (ADR-0075), whose offline phase 0 did not beat `bm25-sparse`, so the operator parked it; and phase 2 of federated recall, whose reviewed descriptors gained one query on the recall eval's cross and skew splits, below the three-point floor, and are not admitted.
- Changed internals only: the application core keeps no page use case, the crate walls are a CI check of their own, and one flaky hook test counts requests instead of timing them.

Upgrade notes:

- Migrations 0039 to 0043 run at upgrade. 0041 builds an index outside a transaction, one tenant at a time; reads never wait. Once 0043 has run, an older server refuses to start, and `oml doctor` counts held versions an older server wrote without their findings as `scan_findings_unlisted`.
- Start `oml-web` beside the server with the same `web.client_secret`, at least 32 characters, from the environment or an owner-only file, and route its pages to it in your proxy ([Run the web console](operations/web-console.md)). On Kubernetes, pass the secret as an environment variable; a volume-mounted Secret is refused.
- `POST /v2/keys` and `POST /v2/keys/{key}/connect-codes` now hold an `Idempotency-Key` to one request for every key. The same key with another request answers `409 idempotency.digest_mismatch`, and the same request again answers the first `201` without the key's value or the code. Send one key per act.
- `search.fallback` has a new value, `tenant_default_unserved`, which a client matching the field exhaustively must accept.

## 0.6.3

Released 2026-11-10.

- Fixed three ways content the secret scanner should hold reached other agents. The rule set is now `oml-scan/1.1`: it reads every memory through normalised views, so invisible and format characters, combining marks, fullwidth forms and the braille blank placed inside or between the words of an injection phrase no longer hide it, and a run of variation selectors is held. Each stored version now records the rule set it was scanned under (migration 0038 reads existing versions as `oml-scan/1`). Inside an in-progress workarea, another author's held version is served only when the current rule set scanned it and found nothing, in recall, listings, reads by id and claim edges.
- Added `oml admin rescan --record-held` (`POST /v2/admin/rescan`), which records the findings of every held version under the current rule set and journals each as `scan.findings_recorded`. **After you upgrade, run `oml admin rescan --record-held`.** Until it has run, sibling agents in an in-progress workarea do not see held versions written before the upgrade. Migration 0038 has no down step, and a server older than `oml-scan/1.1` ignores the new column.
- Added two `oml doctor` lines, `scan_held_unrescanned` and `scan_rescan_due`, which tell you when held versions still wait for that rescan.
- Fixed the weekly recall benchmark, which failed on its first call because it asked for pipelines that need a model while booting the five that do not. The web traffic dashboard gains the sign-in failure panel the observability page already described.
- Fixed `oml sync`: the first `--allow-public-server` explains the disclosure and records consent for the source clone, and later syncs of that clone reuse it. `oml workareas tree` accepts a positional root. The user page no longer opens held or rejected memories written by any of the person's agent keys, the workarea table keeps intermediate parents in each "Name to use" label, and the Guardrails page offers a workarea selector. Chat failures name a retry command and quickstart quotes the paths in the Docker commands it prints.
- Added design records, none implemented: a distributed provider with a standby lead and read nodes (ADR-0073), recall across the trusted workareas a key already reads (ADR-0072), threat classes for every scan finding (ADR-0074), and the web console as a client of the public API (ADR-0070).
- Changed internals only: one shared Postgres test fixture, and `ruff` now checks `scripts/` in CI.

## 0.6.2

Released 2026-11-06.

- Added memory cards to the web pages: `/web` and `/web/admin` list memories as paged cards you can filter by state, workarea, origin, author key and text, and draw the workareas you may read as an Atlas, a treemap the server draws with no script.
- Added a dropdown on each workarea in the web pages for its acts, each confirmed before it runs.
- Added open sign-up, off by default. With `signup.mode = open`, a person who signs in with Google or GitHub and holds no entry reaches a terms page, then gets a private root under quotas, a reviewer key of their own and a `/web/start` page that walks them through the first memory. A person can ask for their account to be deleted. Migration 0037 runs at upgrade. Nothing changes for you unless you set the mode.
- Changed the web pages, the documentation site and the README to one design system, with the same colours, type and state marks everywhere.
- Changed the landing page to lead with one memory across your AI tools, with a page for teams and one for individuals.
- Changed the documentation to be grouped by what a reader does, with open work tracked as GitHub issues.
- Changed continuous integration to run only the jobs a pull request's changed paths reach.

Upgrade notes: Migration 0037 runs at upgrade. Open sign-up stays off until you set `signup.mode = open`.

## 0.6.1

Released 2026-11-06.

- Added claim formats: a write may name a `format` (plain text, Markdown, Mermaid, JSON, YAML or code) and a one-line `title`; the format is signed with the version and changes only by replacement. Migrations 0032 to 0034 run at upgrade.
- Added the Rust adapter for `oml sync`, which reads a Cargo workspace as one claim per crate and one per module doc.
- Added titles to the MCP tools (Remember, Recall and Workarea) and rewrote the server instructions to describe what recall returns instead of telling the model to call it.
- Added `oml keys connect-code --ttl`, which mints a connect code that lives up to 30 days, only for a key confined to one workarea other than the universe.
- Added `oml demo seed <workarea>`, which fills a workarea with thirty demonstration memories.
- Changed the connect page to say where the code comes from, with a link to your account page.
- Changed a handler panic to answer a named `500 system.internal` with the request id; `oml doctor` warns when any occurred.

Upgrade notes: Migrations 0032 to 0034 run at upgrade.

## 0.6.0

Released 2026-11-06.

- Added in-progress workareas: in an opted-in development workarea, recall returns unreviewed memories, marked, and nothing leaves until a commit with none waiting.
- Added a curator key profile that narrows what a key may do inside one workarea.
- Added chat memory: `origin.kind` names each memory's source, and a person's chat workarea holds facts an agent writes from a conversation.
- Added valid time: a memory may state when it is true, and recall hides what has ended.
- Added `on_similar: refuse`, which turns away a write that repeats a readable memory.
- Changed the secret scanner to the `oml-scan/1` rule set and added `oml admin rescan` to report matches in stored memories.
- Added the `oml-lakewriter` binary, which writes the Delta lake.
- Added a production policy with service-level objectives and an error budget for a public server.

Upgrade notes: Migrations 0026 to 0031 run at upgrade.

## 0.5.8

Released 2026-11-05.

- Added memory sources: a repository, commit, file, part and a fingerprint of that part's text, shown to the reviewer and signed.
- Added recall and listing filters by repository and folder.
- Added a hit marker that says when a memory's source changed or went away.
- Added `oml sync` with `--dry-run`, `--check` and `--exclude` to load a repository's markdown documents in batches one person reviews.
- Added `oml anchor`, which prints the anchor of one part of a file.
- Changed replacement: a write names the version it read (`expected_version`), and a stale one is refused.

Upgrade notes: Restart agent sessions after the upgrade. Migration 0025 runs at upgrade.

## 0.5.7

Released 2026-11-05.

- Added the optional searches `bm25-sparse` and `sparse`, which also find memories by related words.
- Measured `bm25-sparse` at 0.730 nDCG@10 against 0.685 for the default (SciFact, 2026-11-05, 5,183 memories).
- Added recall telemetry kept 30 days, with `oml usage` to read events and replay them against other searches.
- Added bulk review: `oml review list` and approve or reject a chosen set, each memory its own signed verdict.

Upgrade notes: The server refuses to start when a configured sparse search lacks its model files. Migrations 0022 to 0024 run at upgrade.

## 0.5.6

Released 2026-11-05.

- Changed recall to read an index: p95 25 ms with about 100,000 readable memories, measured at the database, against 485 ms in 0.5.5.
- Added the default search `bm25-postings`, which says when it left out a very common word.
- Added the optional reranker `bm25-rerank`, off until an operator places the model files.
- Changed `oml doctor` to check the index and the reranker.

Upgrade notes: The first start builds the index and holds writes, at about 0.4 s per 1,000 memories.

## 0.5.5

Released 2026-11-05.

- Changed recall to match any of a memory's words, parts of a code name and word forms, with rarer words ranked higher.
- Changed the default search to `bm25`, and each hit names the words it matched.
- Changed search words to be stored at write time: about 40 times faster on a 5,000-memory benchmark.
- Added `bm25-trigram` for misspellings, which an administrator can make the default without a restart.
- Added suggestions for a word that matched nothing, drawn from each workarea's own words.

Upgrade notes: The first start rewrites every stored memory once.

## 0.5.4

Released 2026-11-04.

- Fixed the upgrade from 0.5.1 and earlier, which 0.5.3 broke on databases whose top workarea an early server created.

Upgrade notes: Do not install 0.5.3 over a 0.5.1 database. See [upgrades](operations/upgrades.md).

## 0.5.3

Released 2026-11-04.

- Published the content of 0.5.2.
- Paused the macOS command-line builds. On a Mac, build `oml` from source.

## 0.5.2

Released 2026-11-04.

- Added the workarea lifecycle: development, committed and validated, each step proposed by one identity and confirmed by another.
- Added labeled links between two memories, such as supports and contradicts, each proposed and confirmed by different reviewers.
- Added a brief for each workarea: reviewed memories an agent reads first at session start.
- Changed `oml doctor` to name an empty brief and steps waiting for a second identity.

Upgrade notes: This tag was not published. 0.5.3 carries the same content.

## 0.5.1

Released 2026-11-04.

- Added guardrails: reviewed rules each agent session loads at start, checked before matching tool calls.
- Removed the eight 0.3.0 agent tool names and the old command-line verbs. Use `oml_write`, `oml_read` and `oml_workarea`.

## 0.5.0

Released 2026-11-04.

- Added a workarea of one's own for each person, created at their first key.
- Added a shared area above user roots, written only by curators an administrator chooses.
- Added web pages for people: sign in with Google or GitHub, manage your keys and review your agents' memories.
- Added an admin page to manage the allowlist, reviewers and curators and to read any memory, each read recorded.

Upgrade notes: Existing workareas and keys stay where they are.

## 0.4.1

Released 2026-11-04.

- Added one log line for every request and agent tool call, kept at most 30 days by whatever stores them.
- Added Prometheus metrics on a separate address and OpenTelemetry traces, both off unless set.

## 0.4.0

Released 2026-11-04.

- Changed the agent interface to three task-shaped tools in place of eight.
- Added a one-server-address, one-key setup for every client.
- Added the Claude Code plugin with the key in the system keychain, and a read-only librarian subagent.
- Added OAuth for Claude's web and mobile apps through a one-time connect code.
- Added `oml login`, `oml setup claude-code` and `oml quickstart`.

## 0.3.1

Released 2026-11-03.

- Changed the server to run migrations as one database account and serve as another that owns nothing.
- Added the `oml` command line for Linux, macOS and Windows, and the server for Intel and Arm Linux.
- Added the Claude Code plugins as an offline archive and a kit for Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop.

Upgrade notes: The server refuses to start when the two database accounts are the same.

## 0.3.0

Released 2026-10-31.

- Added names for workareas, crews, keys and organizations, accepted wherever an id is.
- Added crews as records of their own.
- Added a Helm chart that runs the server on Kubernetes as one instance.
- Changed an admin key to read the shape of the workarea tree but never a memory.

## 0.2.2

Released 2026-10-31.

- Added session-start context: an agent's recent work and the memories that match the project, within a budget.

## 0.2.1

Released 2026-10-31.

- Added one command that issues an agent's key with search and write access and can write it to a private file.

## 0.2.0

Released 2026-10-31.

- Added one key that writes and searches through granted workareas, where a denial always wins.
- Added reads of your own held writes, budgeted search, standing instructions first, fetch by id, replace and retire.
- Added eight agent tools and an event feed that shows each key only what it may see.

## 0.1.0

Released 2026-10-30.

- Added the first server: every write held, scanned and visible only after someone other than its author approves.
- Added keyword search and replacement by a new version.
- Added a signed, hash-chained history with a command-line verifier and a startup fork check.
- Added four agent tools, the Claude Code plugin and the `oml` command line.
