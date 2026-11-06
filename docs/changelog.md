---
status: Stable
verified: 2026-11-06
answers: "what each OML release changed for users and operators, newest first"
---

# Release notes

Newest first. Each release is a signed tag on the [releases page](https://github.com/e8kor/open-memory-lab/releases).

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
