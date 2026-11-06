<!--
id: changelog
tier: client
status: current
answers: what each OML release changed for users and operators, newest first
-->

# Release notes

What each release changed, newest first. A release is a signed tag on the
[releases page](https://github.com/e8kor/open-memory-lab/releases); this page reads what it holds
from the tagged source. Open work is tracked as
[issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap), and the decision
behind each feature is a [design record](adr/index.md).


## 0.6.0

Released 2026-11-06.

In-progress workareas: inside a development workarea that opts in, readers recall memories still waiting for review, marked as such, and nothing leaves the workarea until it is committed with nothing waiting; a curator key profile narrows inside one. Chat memory: every memory says what kind of source it came from (`origin.kind`), and a person's chat workarea holds the facts an agent writes from a conversation, held for review. Valid time: a memory may state when it is true, and recall hides what has ended. A write names the memories that already hold its text or a near copy, and `on_similar: refuse` turns it away. The secret scanner is the real `oml-scan/1` rule set, and `oml admin rescan` reports what it finds in memories already stored. A separate `oml-lakewriter` binary writes the Delta lake. A production policy with SLOs and an error budget for a public server. Migrations 0026 to 0031 run at upgrade.

Design records: [ADR-0030](adr/ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md), [ADR-0063](adr/ADR-0063-a-chat-turn-becomes-memory-only-when-the-agent-in-it-writes-the-fact-and-every-memory-says-what-kind-of-source-it-came-from.md), [ADR-0059](adr/ADR-0059-a-memory-states-when-it-is-true-recall-hides-what-has-ended-and-searches-the-dates-a-query-names-without-a-model.md), [ADR-0050](adr/ADR-0050-remembering-checks-what-is-already-held-replaces-instead-of-repeating-and-says-how-the-memory-will-be-found.md), [ADR-0065](adr/ADR-0065-the-scan-is-a-versioned-compiled-in-rule-set-that-vetoes-leaked-keys-and-holds-what-a-reviewer-should-judge.md), [ADR-0060](adr/ADR-0060-the-lake-writer-drains-the-outbox-it-has-seals-what-it-materializes-and-answers-only-the-operator.md), [ADR-0058](adr/ADR-0058-a-public-server-runs-to-stated-slos-with-an-error-budget.md).

## 0.5.8

Released 2026-11-05.

Every memory may say where it came from: a repository, the commit read, the file and the part of it, and a fingerprint of that part's text, shown to the reviewer and signed with the memory. Recall and the listing find memories by repository and folder, inside the index reads. A hit says when its source changed or went away since it was reviewed. `oml sync` reads a repository's markdown documents at a commit, one memory per decision of a decision record or per section of any other document, writes what is new, replaces what changed and proposes retiring what was removed, in batches one person reviews at once, shows its plan with `--dry-run`, lists what a commit made stale with `--check`, leaves paths out with `--exclude`, loads a draft pull request into a workarea of its own, and proposes the links between decision records for two people to confirm; `oml anchor` prints the anchor of one part of a file. Breaking: replacing a memory names the version the caller read (`expected_version`); a stale one is refused, so restart agent sessions after the upgrade. Migration 0025 runs at upgrade.

Design records: [ADR-0057](adr/ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md), [ADR-0049](adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md), [ADR-0047](adr/ADR-0047-project-knowledge-and-a-code-skeleton-follow-the-development-lifecycle.md).

## 0.5.7

Released 2026-11-05.

Two optional searches that also find memories by related words they do not say: `bm25-sparse` adds a small local model's word weights to the default search and ranks best on every measured set (for example 0.730 against 0.685 nDCG@10 on SciFact, 0.550 against 0.503 on LoCoMo), and `sparse` reads the model's weights alone; each hit names the words that matched and which ones the model added. Both are off by default because at about 60,000 readable memories `bm25-sparse`'s slowest searches take about 270 ms; an operator places the model's files and enables them, and the server refuses to start when a configured sparse search lacks its model. Recall quality, measured: each recall is recorded for 30 days, its query text only where an organisation opts in; a read can say which hits helped; an admin reads the events and replays them against other searches with `oml usage`; quality figures and a Grafana dashboard on the metrics listener. `/readyz` carries no counts any more; they are on `/metrics`. Bulk review: a person lists held memories on the admin page or with `oml review list`, and approves or rejects a chosen set in one act, each memory its own signed verdict, marked `same_user` where reviewer and author are one person. Migrations 0022 to 0024 run at upgrade.

Design records: [ADR-0052](adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md), [ADR-0054](adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md), [ADR-0055](adr/ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md).

## 0.5.6

Released 2026-11-05.

Recall reads an index, so it stays fast as an organisation's memories grow: about 25 ms at the 95th percentile with about 100,000 memories readable, where v0.5.5 took 485 ms, ranking as before. It is the default search, `bm25-postings`; the answer says when it left out a very common word. An optional local reranker, `bm25-rerank`, reorders the best candidates with a small model on the server's own CPU, more accurate and several times slower; it is off unless an operator places the model's files and enables it. A relevance floor can withhold reranked hits below a set relevance; it is off by default. `oml doctor` checks the new index and the reranker. Upgrading builds the index once at the new server's first start, holding writes while it fills, about 0.4 s per 1,000 memories.

Design records: [ADR-0052](adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md), [ADR-0053](adr/ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md).

## 0.5.5

Released 2026-11-05.

Recall finds a memory by any of its words, by the parts of a code name and by another form of a word, ranking rarer words higher, and each hit says which words it matched; this search, `bm25`, is the default, chosen because it scored best on OML's recall test and on the public benchmarks, whose results are now published with the documentation. Each memory's search words are stored when it is written, so recall is many times faster: on a 5,000-memory benchmark, about 40 times. A misspelling is matched by a second search, `bm25-trigram`, which an organisation's administrator can make the default without a restart, or a recall can name. Suggestions for a word that matched nothing come from each workarea's own words, kept as memories are written. A recall that sent many words and relied on every one being required now gets more results. Upgrading rewrites every stored memory once, at the new server's first start. Recall still slows as the memories a key can read grow: about 0.6 s at 50,000.

Design records: [ADR-0049](adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md).

## 0.5.4

Released 2026-11-04.

Fixes the upgrade from v0.5.1 and earlier; v0.5.3 must not be installed over an existing v0.5.1 database. v0.5.3 refused to start on a database whose top workarea a v0.1.0 server created, because that server stored the top workarea's status as development while every later one showed it as committed; v0.5.4 stores the status every earlier server showed before the step that failed, and changes nothing anyone reads.

v0.5.2's records; [upgrading from v0.5.1 or earlier](operations/upgrades.md#upgrading-from-v051-or-earlier-the-upgrade-to-v054)

## 0.5.3

Released 2026-11-04.

v0.5.2's content, published, with the macOS builds of the command line paused while GitHub's macOS runners are unreliable; on a Mac, build `oml` from source.

v0.5.2's records; [paused targets](https://github.com/e8kor/open-memory-lab/blob/main/docs/contributing/ci-and-toolchain.md)

## 0.5.2

Released 2026-11-04.

Tagged, not published: its release waited on a macOS runner GitHub never assigned; the same content shipped as v0.5.3. The workarea lifecycle: a workarea moves from development to committed to validated, each step proposed by one identity and confirmed by another, and a validated workarea stays readable and takes no new writes; a person's own root climbs to committed at most. Labelled links between two memories, such as "supports" or "contradicts", each proposed by one reviewer and confirmed by another, on the HTTP API and the command line. A brief for each workarea: a few reviewed memories its reviewers agreed an agent entering it reads first, within a size limit the server sets, shown at every session start and by the agent's workarea tool. `oml doctor` names an empty brief and the steps waiting for a second identity.

Design records: [ADR-0046](adr/ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md), [ADR-0009](adr/ADR-0009-status-ladder.md), [ADR-0008](adr/ADR-0008-workarea-algebra.md), [ADR-0021](adr/ADR-0021-ai-human-ontologies.md).

## 0.5.1

Released 2026-11-04.

Guardrails: reviewed rules every agent session loads at its start, from its workarea and, as an administrator sets on its key, the workareas above it, each shown with where it was set and who approved it, and a check before each matching tool call that refuses or asks, never allows; an export and a prompt for tools without the plugin's hooks; the rules in force on the user page. The eight v0.3.0 agent tool names and the old command-line verbs and flags are removed.

Design records: [ADR-0037](adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md), [ADR-0039](adr/ADR-0039-the-api-rewritten-for-v0-4-0.md).

## 0.5.0

Released 2026-11-04.

A workarea of one's own for each person, created when their first key is issued, where their keys write, read and make workareas, so two people can each have a `project`; the organisation's shared area above them, read by everyone and written only by curators an administrator chooses, each copy reviewed by someone else and shown without its author or its source. Workareas and keys made before then stay where they are. Two web pages on the server's own address, for people and never for agents: sign in with Google or GitHub, if an administrator has put the address on the organisation's list; on the user page, see and make your own agent keys and connect codes, read the setup steps for each agent tool, browse your memories and the shared area's, make workareas of your own, and review what your own agents wrote; on the admin page, keep the list, choose who reviews and who curates, see how many memories wait and for how long, read any memory with who wrote, derived and reviewed it, each read recorded in the history, and review, curate and replace memories, each act of review or curation done by the person's own reviewer or curator identity, never the administrator's. The eight v0.3.0 agent tool names and the old command-line verbs and flags still answer, unlisted; their removal moves to v0.5.1. Not in it: demoting a memory from the admin page, planned for v0.6.0; no sign-in has yet reached Google or GitHub.

Design records: [ADR-0044](adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md), [ADR-0045](adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md).

## 0.4.1

Released 2026-11-04.

Seeing the traffic. The server writes one log line for every request and every agent tool call: who sent it, from which key, organisation, workarea and client, the address it came from, how it ended and how long it took. A request carries an id the caller may choose, and every answer returns it. Logs are kept at most 30 days by whatever stores them, because they now hold addresses and names. Request counts and timings are served for Prometheus on a separate address, off unless set, so they never reach the public internet. Traces can be sent to an OpenTelemetry collector, off unless an address is set. The Claude Code plugin and the agent kit connect to the maintainer's public server, `https://memory.doublegate.io`, unless told otherwise, so an agent needs only its key; every other command still needs a server address. `oml doctor` prints the server's version.

Design records: [ADR-0042](adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md), [ADR-0043](adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md).

## 0.4.0

Released 2026-11-04.

The agent interface rewritten while OML is pre-release: three task-shaped agent tools in place of the eight, with the eight old names and the old command-line verbs still answering, unlisted, through the 0.4 releases, and one server address and one key as the only settings any client needs. The Claude Code plugin set up through Claude Code's own plugin settings, with the key kept in the system keychain. The agent searches for itself during a session, a failed command searches only when the user opts in, and a read-only librarian subagent searches for you. Claude's web and mobile apps reach OML over OAuth, through a one-time connect code bound to one agent key, on the server's own address; no live claude.ai connection has been made yet. A quick start in one command per role: an installer that checks what it downloads, `oml login`, `oml setup claude-code` and `oml quickstart`. The documentation upgrade: a Get started page and an install page, guides for Claude Code, claude.ai and mobile, other agent tools, self-hosting and upgrading from v0.3, eight short use-case scenarios beside the full use-case catalogue, refreshed for v0.4.0, and the README, architecture overview and plugin and kit READMEs rewritten. Not in it: the one-line install that also runs the quick start, and the short install address.

Design records: [ADR-0039](adr/ADR-0039-the-api-rewritten-for-v0-4-0.md), [ADR-0040](adr/ADR-0040-plugin-setup-in-claude-code.md), [ADR-0038](adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md), [ADR-0031](adr/ADR-0031-oauth-connections-by-connect-code.md), [ADR-0041](adr/ADR-0041-a-quick-start-in-one-command-per-role.md).

## 0.3.1

Released 2026-11-03.

The server runs its database changes as one account and serves as a second account that owns nothing, and refuses to start when both are the same. Each organisation's search index is its own. The `oml` command line ships on its own for Linux, macOS and Windows, and the server for Intel and Arm Linux. Each release also ships the Claude Code plugins as an offline archive and as a marketplace pinned to the release, and a kit that connects other agent tools: OpenAI Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop.

Design records: [ADR-0034](adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md).

## 0.3.0

Released 2026-10-31.

Names instead of ids. Workareas, crews, keys and organisations carry one name each, unique and never changed or reused, and every surface accepts a name wherever it accepts an id. Crews become records of their own. An administrator's key reads the shape of the workarea tree but never a memory in it. On its first start over older data the server names every unnamed row and records that in the history. A Helm chart runs the server on Kubernetes as one instance. Python and TypeScript clients sit in the source tree, not yet published to a package registry.

Design records: [ADR-0026](adr/ADR-0026-names-are-keys.md), [ADR-0033](adr/ADR-0033-interim-helm-chart.md).

## 0.2.2

Released 2026-10-31.

Every Claude Code session starts from what OML holds for the agent's key: its recent work and the memories that match the project, within a budget, and nothing at all if the server cannot be reached.

Design records: [ADR-0029](adr/ADR-0029-session-start-hook.md).

## 0.2.1

Released 2026-10-31.

One command issues an agent's key with search access beside write access, and can write the key straight to a private file. The `oml-admin` plugin lets Claude Code issue a key without the key ever appearing in the conversation.

Design records: [ADR-0028](adr/ADR-0028-agent-key-issuance.md).

## 0.2.0

Released 2026-10-31.

OML as an agent's only memory, with one key. One key stores and searches through the workareas it is granted, with denials that always win. An agent reads back its own writes while they wait for review, searches within a budget, gets standing instructions ahead of every result, fetches a memory by id, replaces one or proposes retiring one, lists its own work and reads its workarea. A new version replaces the old one for others only once someone else approves it. The event feed shows each key only what it may see. Eight agent tools; the plugin connects with one key. The health check names the build that answered. The image runs on both Intel and Arm Linux.

Design records: [ADR-0025](adr/ADR-0025-the-agent-only-memory-surface.md).

## 0.1.0

Released 2026-10-30.

The first working slice. One server on one host, or in Docker with its database. One organisation, claimed once with an enrollment token. Every write is held, checked by a deterministic scan and made visible only when someone other than its author approves it. Approved memories are found by keyword search, and a memory can be replaced by a new version. Every change is recorded in a signed, tamper-evident history that a separate command-line verifier checks. The server checks its history at startup and refuses to run on a forked one. Four agent tools: remember, learn, recall and the event feed. The Claude Code plugin, which connected with two keys, one to write and one to search. The `oml` command line for the claim, keys, workareas, review and a health check. A signed release image for 64-bit Intel and AMD Linux, a Linux archive and a software bill of materials.

Design records: [ADR-0001](adr/ADR-0001-the-constitution.md), [ADR-0002](adr/ADR-0002-postgresql-system-of-record.md), [ADR-0006](adr/ADR-0006-pure-rust.md), [ADR-0007](adr/ADR-0007-tenancy-first-class.md), [ADR-0008](adr/ADR-0008-workarea-algebra.md), [ADR-0010](adr/ADR-0010-roles-and-keys.md), [ADR-0012](adr/ADR-0012-admission-and-review.md), [ADR-0013](adr/ADR-0013-reads-served-from-postgres.md), [ADR-0014](adr/ADR-0014-http-api-and-mcp-facade.md), [ADR-0015](adr/ADR-0015-cli-administration.md), [ADR-0019](adr/ADR-0019-hexagonal-architecture-and-plugins.md), [ADR-0022](adr/ADR-0022-two-ring-api.md), [ADR-0023](adr/ADR-0023-install-profiles-and-cluster-mode.md).
