<!--
id: product/releases
tier: client
status: release record · shipped releases from the tags, planned releases as decided on 2026-11-03 and revised on 2026-11-04
answers: which release introduced each feature and decision, which release each planned one is for, and which are parked or not scheduled
-->

# Releases

**Status:** release record · shipped releases from the tags, planned releases as decided on 2026-11-03 and revised on 2026-11-04

This page is the one place that says when a feature arrived or when it is planned to arrive.
Every other page links here instead of repeating it. A shipped release is a signed tag on the
[releases page](https://github.com/e8kor/open-memory-lab/releases); what it holds is read from
the tagged source, not from intentions. A planned release is a plan the maintainer made on
2026-11-03 and narrowed on 2026-11-04, when v0.5.0 became a person's own workarea and the two web
pages alone: nothing in it is built until its tag exists, and its contents can still move.

Four words carry the status of everything below:

- **Introduced in vX** — the tagged release vX holds it.
- **Planned for vY** — decided or proposed, not built, and assigned to the release vY.
- **Parked** — designed or proposed, deliberately set aside, with no release.
- **Not scheduled** — decided, not built, and no release is assigned yet.

## Shipped releases

| Version | Date | What shipped | Decisions and features |
|---|---|---|---|
| **v0.1.0** | 2026-10-30 | The first working slice. One server on one host, or in Docker with its database. One organisation, claimed once with an enrollment token. Every write is held, checked by a deterministic scan and made visible only when someone other than its author approves it. Approved memories are found by keyword search, and a memory can be replaced by a new version. Every change is recorded in a signed, tamper-evident history that a separate command-line verifier checks. The server checks its history at startup and refuses to run on a forked one. Four agent tools: remember, learn, recall and the event feed. The Claude Code plugin, which connected with two keys, one to write and one to search. The `oml` command line for the claim, keys, workareas, review and a health check. A signed release image for 64-bit Intel and AMD Linux, a Linux archive and a software bill of materials. | [ADR-0001](../adr/ADR-0001-the-constitution.md), [ADR-0002](../adr/ADR-0002-postgresql-system-of-record.md), [ADR-0006](../adr/ADR-0006-pure-rust.md), [ADR-0007](../adr/ADR-0007-tenancy-first-class.md), [ADR-0008](../adr/ADR-0008-workarea-algebra.md), [ADR-0010](../adr/ADR-0010-roles-and-keys.md), [ADR-0012](../adr/ADR-0012-admission-and-review.md), [ADR-0013](../adr/ADR-0013-reads-served-from-postgres.md), [ADR-0014](../adr/ADR-0014-http-api-and-mcp-facade.md), [ADR-0015](../adr/ADR-0015-cli-administration.md), [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md), [ADR-0022](../adr/ADR-0022-two-ring-api.md), [ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md), each in part |
| **v0.2.0** | 2026-10-31 | OML as an agent's only memory, with one key. One key stores and searches through the workareas it is granted, with denials that always win. An agent reads back its own writes while they wait for review, searches within a budget, gets standing instructions ahead of every result, fetches a memory by id, replaces one or proposes retiring one, lists its own work and reads its workarea. A new version replaces the old one for others only once someone else approves it. The event feed shows each key only what it may see. Eight agent tools; the plugin connects with one key. The health check names the build that answered. The image runs on both Intel and Arm Linux. | [ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md), in part |
| **v0.2.1** | 2026-10-31 | One command issues an agent's key with search access beside write access, and can write the key straight to a private file. The `oml-admin` plugin lets Claude Code issue a key without the key ever appearing in the conversation. | [ADR-0028](../adr/ADR-0028-agent-key-issuance.md) |
| **v0.2.2** | 2026-10-31 | Every Claude Code session starts from what OML holds for the agent's key: its recent work and the memories that match the project, within a budget, and nothing at all if the server cannot be reached. | [ADR-0029](../adr/ADR-0029-session-start-hook.md) |
| **v0.3.0** | 2026-10-31 | Names instead of ids. Workareas, crews, keys and organisations carry one name each, unique and never changed or reused, and every surface accepts a name wherever it accepts an id. Crews become records of their own. An administrator's key reads the shape of the workarea tree but never a memory in it. On its first start over older data the server names every unnamed row and records that in the history. A Helm chart runs the server on Kubernetes as one instance. Python and TypeScript clients sit in the source tree, not yet published to a package registry. | [ADR-0026](../adr/ADR-0026-names-are-keys.md), in part; [ADR-0033](../adr/ADR-0033-interim-helm-chart.md) (the record is still proposed) |
| **v0.3.1** | 2026-11-03 | The server runs its database changes as one account and serves as a second account that owns nothing, and refuses to start when both are the same. Each organisation's search index is its own. The `oml` command line ships on its own for Linux, macOS and Windows, and the server for Intel and Arm Linux. Each release also ships the Claude Code plugins as an offline archive and as a marketplace pinned to the release, and a kit that connects other agent tools: OpenAI Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop. | [ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md), in part |
| **v0.4.0** | 2026-11-04 | The agent interface rewritten while OML is pre-release: three task-shaped agent tools in place of the eight, with the eight old names and the old command-line verbs still answering, unlisted, through the 0.4 releases, and one server address and one key as the only settings any client needs. The Claude Code plugin set up through Claude Code's own plugin settings, with the key kept in the system keychain. The agent searches for itself during a session, a failed command searches only when the user opts in, and a read-only librarian subagent searches for you. Claude's web and mobile apps reach OML over OAuth, through a one-time connect code bound to one agent key, on the server's own address; no live claude.ai connection has been made yet. A quick start in one command per role: an installer that checks what it downloads, `oml login`, `oml setup claude-code` and `oml quickstart`. The documentation upgrade: a Get started page and an install page, guides for Claude Code, claude.ai and mobile, other agent tools, self-hosting and upgrading from v0.3, eight short use-case scenarios beside the full use-case catalogue, refreshed for v0.4.0, and the README, architecture overview and plugin and kit READMEs rewritten. Not in it: the one-line install that also runs the quick start, and the short install address. | [ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md), [ADR-0040](../adr/ADR-0040-plugin-setup-in-claude-code.md), [ADR-0038](../adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md), [ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md), [ADR-0041](../adr/ADR-0041-a-quick-start-in-one-command-per-role.md); the librarian is ADR-0038's d10 |
| **v0.4.1** | 2026-11-04 | Seeing the traffic. The server writes one log line for every request and every agent tool call: who sent it, from which key, organisation, workarea and client, the address it came from, how it ended and how long it took. A request carries an id the caller may choose, and every answer returns it. Logs are kept at most 30 days by whatever stores them, because they now hold addresses and names. Request counts and timings are served for Prometheus on a separate address, off unless set, so they never reach the public internet. Traces can be sent to an OpenTelemetry collector, off unless an address is set. The Claude Code plugin and the agent kit connect to the maintainer's public server, `https://memory.doublegate.io`, unless told otherwise, so an agent needs only its key; every other command still needs a server address. `oml doctor` prints the server's version. | [ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md), [ADR-0043](../adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md) |
| **v0.5.0** | 2026-11-04 | A workarea of one's own for each person, created when their first key is issued, where their keys write, read and make workareas, so two people can each have a `project`; the organisation's shared area above them, read by everyone and written only by curators an administrator chooses, each copy reviewed by someone else and shown without its author or its source. Workareas and keys made before then stay where they are. Two web pages on the server's own address, for people and never for agents: sign in with Google or GitHub, if an administrator has put the address on the organisation's list; on the user page, see and make your own agent keys and connect codes, read the setup steps for each agent tool, browse your memories and the shared area's, make workareas of your own, and review what your own agents wrote; on the admin page, keep the list, choose who reviews and who curates, see how many memories wait and for how long, read any memory with who wrote, derived and reviewed it, each read recorded in the history, and review, curate and replace memories, each act of review or curation done by the person's own reviewer or curator identity, never the administrator's. The eight v0.3.0 agent tool names and the old command-line verbs and flags still answer, unlisted; their removal moves to v0.5.1. Not in it: demoting a memory from the admin page, planned for v0.6.0; no sign-in has yet reached Google or GitHub. | [ADR-0044](../adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md), [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) but demotion |
| **v0.5.1** | 2026-11-04 | Guardrails: reviewed rules every agent session loads at its start, from its workarea and, as an administrator sets on its key, the workareas above it, each shown with where it was set and who approved it, and a check before each matching tool call that refuses or asks, never allows; an export and a prompt for tools without the plugin's hooks; the rules in force on the user page. The eight v0.3.0 agent tool names and the old command-line verbs and flags are removed. | [ADR-0037](../adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md); [ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d12 |
| **v0.5.2** | 2026-11-04 | Tagged, not published: its release waited on a macOS runner GitHub never assigned; the same content shipped as v0.5.3. The workarea lifecycle: a workarea moves from development to committed to validated, each step proposed by one identity and confirmed by another, and a validated workarea stays readable and takes no new writes; a person's own root climbs to committed at most. Labelled links between two memories, such as "supports" or "contradicts", each proposed by one reviewer and confirmed by another, on the HTTP API and the command line. A brief for each workarea: a few reviewed memories its reviewers agreed an agent entering it reads first, within a size limit the server sets, shown at every session start and by the agent's workarea tool. `oml doctor` names an empty brief and the steps waiting for a second identity. | [ADR-0046](../adr/ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md); [ADR-0009](../adr/ADR-0009-status-ladder.md), and the memory links of [ADR-0008](../adr/ADR-0008-workarea-algebra.md) and [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md), in part |
| **v0.5.3** | 2026-11-04 | v0.5.2's content, published, with the macOS builds of the command line paused while GitHub's macOS runners are unreliable; on a Mac, build `oml` from source. | v0.5.2's records; [paused targets](../engineering/ci-and-toolchain.md#paused-targets) |
| **v0.5.4** | 2026-11-04 | Fixes the upgrade from v0.5.1 and earlier; v0.5.3 must not be installed over an existing v0.5.1 database. v0.5.3 refused to start on a database whose top workarea a v0.1.0 server created, because that server stored the top workarea's status as development while every later one showed it as committed; v0.5.4 stores the status every earlier server showed before the step that failed, and changes nothing anyone reads. | v0.5.2's records; [upgrading from v0.5.1 or earlier](../operations/upgrades.md#upgrading-from-v051-or-earlier-the-upgrade-to-v054) |
| **v0.5.5** | 2026-11-05 | Recall finds a memory by any of its words, by the parts of a code name and by another form of a word, ranking rarer words higher, and each hit says which words it matched; this search, `bm25`, is the default, chosen because it scored best on OML's recall test and on the public benchmarks, whose results are now published with the documentation. Each memory's search words are stored when it is written, so recall is many times faster: on a 5,000-memory benchmark, about 40 times. A misspelling is matched by a second search, `bm25-trigram`, which an organisation's administrator can make the default without a restart, or a recall can name. Suggestions for a word that matched nothing come from each workarea's own words, kept as memories are written. A recall that sent many words and relied on every one being required now gets more results. Upgrading rewrites every stored memory once, at the new server's first start. Recall still slows as the memories a key can read grow: about 0.6 s at 50,000. | [ADR-0049](../adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md), increment 1 (the record is still proposed); [benchmarks](../api/benchmarks/index.md) |
| **v0.5.6** | 2026-11-05 | Recall reads an index, so it stays fast as an organisation's memories grow: about 25 ms at the 95th percentile with about 100,000 memories readable, where v0.5.5 took 485 ms, ranking as before. It is the default search, `bm25-postings`; the answer says when it left out a very common word. An optional local reranker, `bm25-rerank`, reorders the best candidates with a small model on the server's own CPU, more accurate and several times slower; it is off unless an operator places the model's files and enables it. A relevance floor can withhold reranked hits below a set relevance; it is off by default. `oml doctor` checks the new index and the reranker. Upgrading builds the index once at the new server's first start, holding writes while it fills, about 0.4 s per 1,000 memories. | [ADR-0052](../adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md), [ADR-0053](../adr/ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md); [benchmarks](../api/benchmarks/index.md) |
| **v0.5.7** | 2026-11-05 | Two optional searches that also find memories by related words they do not say: `bm25-sparse` adds a small local model's word weights to the default search and ranks best on every measured set (for example 0.730 against 0.685 nDCG@10 on SciFact, 0.550 against 0.503 on LoCoMo), and `sparse` reads the model's weights alone; each hit names the words that matched and which ones the model added. Both are off by default because at about 60,000 readable memories `bm25-sparse`'s slowest searches take about 270 ms; an operator places the model's files and enables them, and the server refuses to start when a configured sparse search lacks its model. Recall quality, measured: each recall is recorded for 30 days, its query text only where an organisation opts in; a read can say which hits helped; an admin reads the events and replays them against other searches with `oml usage`; quality figures and a Grafana dashboard on the metrics listener. `/readyz` carries no counts any more; they are on `/metrics`. Bulk review: a person lists held memories on the admin page or with `oml review list`, and approves or rejects a chosen set in one act, each memory its own signed verdict, marked `same_user` where reviewer and author are one person. Migrations 0022 to 0024 run at upgrade. | [ADR-0052](../adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) d13–d17, [ADR-0054](../adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) (measurement half), [ADR-0055](../adr/ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md) |
| **v0.5.8** | 2026-11-05 | Every memory may say where it came from: a repository, the commit read, the file and the part of it, and a fingerprint of that part's text, shown to the reviewer and signed with the memory. Recall and the listing find memories by repository and folder, inside the index reads. A hit says when its source changed or went away since it was reviewed. `oml sync` reads a repository's markdown documents at a commit, one memory per decision of a decision record or per section of any other document, writes what is new, replaces what changed and proposes retiring what was removed, in batches one person reviews at once, shows its plan with `--dry-run`, lists what a commit made stale with `--check`, leaves paths out with `--exclude`, loads a draft pull request into a workarea of its own, and proposes the links between decision records for two people to confirm; `oml anchor` prints the anchor of one part of a file. Breaking: replacing a memory names the version the caller read (`expected_version`); a stale one is refused, so restart agent sessions after the upgrade. Migration 0025 runs at upgrade. | [ADR-0057](../adr/ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md) (accepted); [ADR-0049](../adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md) d13; [ADR-0047](../adr/ADR-0047-project-knowledge-and-a-code-skeleton-follow-the-development-lifecycle.md)'s answers, in part |

## Planned releases

Nothing in this section is released. Each row is the plan of 2026-11-03 as revised on 2026-11-04.

| Version | Status | What is planned | Decisions |
|---|---|---|---|
| **v0.6.0** | planned | In-progress workareas: inside a development workarea that opts in, readers recall memories still waiting for review, marked as such, and nothing leaves the workarea until it is committed with nothing waiting. On the admin page, demoting a memory, which keeps it. | [ADR-0030](../adr/ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md); [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) in part |
| **Parked** | parked | Ontology crews that classify memories and curate shared vocabularies, and the vocabularies themselves. Deliverables: the files a memory chain ends in, beyond the storage component already written on a branch. Time: stating when a memory is true and reading what OML knew at a past moment. A digest that summarises a whole workarea for an agent entering it. | ADR-0027, ADR-0035, ADR-0036 (proposed); [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md) (accepted, not built) |

Proposed records live on branches and draft pull requests until they are accepted:
ADR-0027 ([#10](https://github.com/e8kor/open-memory-lab/pull/10)), ADR-0030
([#12](https://github.com/e8kor/open-memory-lab/pull/12)), ADR-0035
([#17](https://github.com/e8kor/open-memory-lab/pull/17)), ADR-0036
([#19](https://github.com/e8kor/open-memory-lab/pull/19)). The number ADR-0032 is not used by any record.

## Not scheduled

These are decided and not built, and no release is assigned to them yet: files attached to a
memory and encryption of each memory under its own key; erasure that destroys that key;
simulations; dataset imports; the analytics lake and its lineage; checking that the derived
stores agree with the history; a record of what each search served; semantic search;
read followers and cluster mode; organisations beyond the first and the operator who creates
them; revoking, rotating and expiring keys, and changing a key's grants after issue; review
policies per workarea and held writes that expire unreviewed; moving a workarea in the tree;
the evidence export and checking the history against a running server; settings managed
through the command line; plugins for checks, embedding models and export formats; and writing
many memories in one request. A web console beyond the two pages of ADR-0045 is declined, not deferred.

## Feature index

| Feature | Release |
|---|---|
| Held writes, the deterministic scan and review by someone other than the author | introduced in v0.1.0 |
| The signed, tamper-evident history and its separate verifier | introduced in v0.1.0; checking it against a running server is not scheduled |
| Refusing to start on a forked history | introduced in v0.1.0 |
| Keyword search over approved memories | introduced in v0.1.0 |
| Replacing a memory with a new version | introduced in v0.1.0; the old version stays the one others see until the new one is approved since v0.2.0 |
| Agent tools: remember, learn, recall, event feed | introduced in v0.1.0 |
| Agent tools: get, supersede, my work, workarea info | introduced in v0.2.0 |
| Three task-shaped agent tools in place of the eight | introduced in v0.4.0 |
| The Claude Code plugin | introduced in v0.1.0 (two keys); one key since v0.2.0 |
| Plugin setup through Claude Code's plugin settings, key in the keychain | introduced in v0.4.0 |
| The plugins and the agent kit connect to the maintainer's public server unless told otherwise, so an agent needs only its key | introduced in v0.4.1 |
| Docker quickstart | introduced in v0.1.0 |
| Signed releases with a software bill of materials | introduced in v0.1.0 |
| Health and readiness checks | introduced in v0.1.0; the build is named since v0.2.0 |
| One key that stores and searches through grants, with denials | introduced in v0.2.0 |
| Reading back your own writes while they wait for review | introduced in v0.2.0 |
| Search within a token budget | introduced in v0.2.0 |
| Standing instructions ahead of every result | introduced in v0.2.0 |
| An event feed filtered to what each key may see | introduced in v0.2.0 (the unfiltered feed in v0.1.0) |
| Release image for both Intel and Arm Linux | introduced in v0.2.0 |
| Issuing an agent's key in one command, written to a private file | introduced in v0.2.1 |
| The `oml-admin` plugin | introduced in v0.2.1 |
| A session that starts from what OML holds for its key | introduced in v0.2.2 |
| Recall during the session, and after a failed command when opted in | introduced in v0.4.0 |
| Names for workareas, crews, keys and organisations | introduced in v0.3.0 |
| Crew records and the workarea tree read | introduced in v0.3.0 |
| Helm chart for Kubernetes, one instance | introduced in v0.3.0 |
| Python and TypeScript clients | in the source tree since v0.3.0; not published to a registry |
| Migrating as one database account and serving as another that owns nothing | introduced in v0.3.1 |
| A search index of its own for each organisation | introduced in v0.3.1 |
| The command line for Linux, macOS and Windows; the server for Intel and Arm Linux | introduced in v0.3.1 |
| The plugins as an offline archive and a pinned marketplace | introduced in v0.3.1 |
| The kit for Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop | introduced in v0.3.1 |
| Claude's web and mobile apps over OAuth by connect code | introduced in v0.4.0 |
| The librarian subagent | introduced in v0.4.0 |
| Quick start in one command per role | introduced in v0.4.0 |
| Documentation upgrade | introduced in v0.4.0 |
| One log line per request and per agent tool call, under an id the caller may choose | introduced in v0.4.1 |
| Request metrics for Prometheus on a separate address | introduced in v0.4.1 |
| Trace export to an OpenTelemetry collector, off by default | introduced in v0.4.1 |
| A workarea of one's own for each person, and a curated shared area above them | introduced in v0.5.0 |
| Sign-in with Google or GitHub to a user page and an admin page | introduced in v0.5.0 |
| Reviewing your own agents' work on the user page | introduced in v0.5.0 |
| Review, curation and replacing memories from the admin page | introduced in v0.5.0 |
| Reading any memory, with who wrote, derived and reviewed it, on the admin page | introduced in v0.5.0 |
| Guardrails | introduced in v0.5.1 |
| Workarea lifecycle: development, committed, validated | introduced in v0.5.2 (workareas carry the development status since v0.1.0) |
| Labelled links between memories | introduced in v0.5.2 |
| A brief for each workarea | introduced in v0.5.2 |
| Recall by any word, code-name parts, word forms and misspellings, chosen per organisation | introduced in v0.5.5 |
| Recall from an index that stays fast as readable memories grow | introduced in v0.5.6 |
| An optional local reranker and a relevance floor | introduced in v0.5.6 |
| Optional searches by related words, from a local learned sparse model | introduced in v0.5.7 |
| Recall quality measured: recall events, feedback on a read, the replay and the dashboard | introduced in v0.5.7 |
| Bulk review on the admin page and in the CLI | introduced in v0.5.7 |
| Demotion on the admin page | planned for v0.6.0 |
| A memory's source anchor, recall and listing by origin, and the stale mark | introduced in v0.5.8 |
| `oml sync` for markdown documents, and links between decision records | introduced in v0.5.8 |
| In-progress workareas | planned for v0.6.0 |
| Ontology crews, shared vocabularies | parked |
| Deliverables and files on any memory | parked |
| Time: when a memory is true, and what OML knew at a past moment | parked |
| A digest of a whole workarea | parked |
| Everything listed under [Not scheduled](#not-scheduled) | not scheduled |

## Decision records and their releases {#decisions}

A decision record can ship in parts. Where it did, the row says which part arrived when and
which part is still decided, not built.

| Record | Status of the record | Release |
|---|---|---|
| [ADR-0001](../adr/ADR-0001-the-constitution.md) the constitution | accepted | Partly in v0.1.0: the limit on what each role may do, review by someone other than the author and no silent deletion hold on every verb served. The one bounded development exception is not built; ADR-0030, which revises it, is planned for v0.6.0. |
| [ADR-0002](../adr/ADR-0002-postgresql-system-of-record.md) the database is the system of record | accepted | Partly in v0.1.0: one transaction commits each change, one signed history per organisation, an append-only history holding digests and never text, the startup fork check. Synchronous replication of the history is not scheduled. |
| [ADR-0003](../adr/ADR-0003-delta-lake-derived-lakehouse.md) the derived lake | accepted | Not scheduled. The delivery rows it would read are written since v0.1.0; nothing reads them. |
| [ADR-0004](../adr/ADR-0004-bodies-and-attachments.md) bodies and attachments | accepted | Not scheduled. Bodies are stored with the memory, unencrypted. |
| [ADR-0005](../adr/ADR-0005-erasure-by-crypto-shredding.md) erasure | accepted | Not scheduled. |
| [ADR-0006](../adr/ADR-0006-pure-rust.md) pure Rust | accepted | Partly in v0.1.0: every binary is Rust. Lake maintenance is not scheduled. |
| [ADR-0007](../adr/ADR-0007-tenancy-first-class.md) tenancy | accepted | Partly in v0.1.0: the organisation is read from the key and every table is fenced by organisation. v0.3.1 added the account that owns nothing and the per-organisation search index. More than one organisation is not scheduled. |
| [ADR-0008](../adr/ADR-0008-workarea-algebra.md) workareas | accepted | Partly in v0.1.0: one tree per organisation and access bound to a workarea's id. v0.2.0 added grants combined with denials that win; v0.3.0 added crew records. Links between memories and the brief arrived in v0.5.2; workarea edges and moving a workarea are not scheduled. |
| [ADR-0009](../adr/ADR-0009-status-ladder.md) the workarea lifecycle | accepted | Introduced in v0.5.2: the ladder, its two-identity transitions, the freeze, and the user root's ceiling. Workareas are created in development since v0.1.0; the simulation fence waits for simulations. |
| [ADR-0010](../adr/ADR-0010-roles-and-keys.md) roles and keys | accepted | Partly in v0.1.0: reader, writer, reviewer and admin keys, the enrollment claim, and admins kept out of review. Key names since v0.3.0. The operator, bans, revocation and rotation are not scheduled. |
| [ADR-0011](../adr/ADR-0011-simulations.md) simulations | accepted | Not scheduled. |
| [ADR-0012](../adr/ADR-0012-admission-and-review.md) admission and review | accepted | Partly in v0.1.0: capture, scan, hold, review bound to what was served, approval. Review policies, held writes that expire and comparing crews for independence are not scheduled. |
| [ADR-0013](../adr/ADR-0013-reads-served-from-postgres.md) reads served from the database | accepted | Partly in v0.1.0: every read is served from the database. Standing instructions first since v0.2.0. The read record and semantic search are not scheduled. |
| [ADR-0014](../adr/ADR-0014-http-api-and-mcp-facade.md) one HTTP API with MCP over it | accepted | Partly in v0.1.0: the HTTP API, the MCP tools over it, safe retries, typed errors, health checks. The answer for a retry that arrives while its first attempt is still running is not built. The rewrite of the tools arrived in v0.4.0 (ADR-0039), and connections on `/mcp` with it (ADR-0031). |
| [ADR-0015](../adr/ADR-0015-cli-administration.md) the command line administers | accepted | Partly in v0.1.0: keys shown once, the review ceremony, the health check. Dry runs, the evidence export, settings and status are not scheduled. |
| [ADR-0016](../adr/ADR-0016-kubernetes-workloads.md) Kubernetes workloads | accepted | Partly in v0.3.0: a chart with the server alone, under ADR-0033. The full workload set and its two profiles are not scheduled. |
| [ADR-0017](../adr/ADR-0017-reconciliation.md) reconciliation | accepted | Not scheduled. |
| [ADR-0018](../adr/ADR-0018-the-documentation-set.md) the documentation set | accepted | Governs the documentation since the first design revision of 2026-10-29; no software ships it. The documentation upgrade arrived with v0.4.0. |
| [ADR-0019](../adr/ADR-0019-hexagonal-architecture-and-plugins.md) a pure core with ports | accepted | Partly in v0.1.0: the pure core, ports and thin binaries. Two of the five binaries exist; the plugin points are not scheduled. |
| [ADR-0020](../adr/ADR-0020-dataset-onboarding.md) dataset imports | accepted | Not scheduled. |
| [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md) shared vocabularies | accepted | Partly in v0.5.2: claim edges between memories. Vocabularies are parked. |
| [ADR-0022](../adr/ADR-0022-two-ring-api.md) the two-ring API | accepted | Introduced in v0.1.0: one listener, a stable agent ring and an evolving lab ring, MCP over both. The compatibility promise is not yet in force. Amended by the v0.4.0 rewrite (ADR-0039). |
| [ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) install profiles and cluster mode | accepted | Partly in v0.1.0: one host and Docker, with a file keyring. The Kubernetes chart in v0.3.0. Read followers and cluster mode are not scheduled. |
| [ADR-0024](../adr/ADR-0024-performance-foundation.md) the performance foundation | accepted | Not scheduled as a release item. No part of it is measured; batch writes and streamed attachments are not built. |
| [ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) the agent-only memory surface | accepted | Partly in v0.2.0: one key, own held writes in recall, budgets, ordering, versions, the own-work listing, the workarea read and the filtered feed. The separate author-only search index, the read record and changing grants after issue are not scheduled. |
| [ADR-0026](../adr/ADR-0026-names-are-keys.md) names are keys | accepted | Partly in v0.3.0: names on every verb that is served. Names on unbuilt verbs (key revocation, rotation, the key listing, the lookup by path) and the operator's names are not scheduled. |
| ADR-0027 ontology crews | proposed | Parked. |
| [ADR-0028](../adr/ADR-0028-agent-key-issuance.md) agent key issuance | accepted | Introduced in v0.2.1; the key's name as its first argument since v0.3.0. |
| [ADR-0029](../adr/ADR-0029-session-start-hook.md) the session-start hook | accepted | Introduced in v0.2.2. |
| [ADR-0030](../adr/ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md) in-progress workareas | accepted | Planned for v0.6.0: the workarea mode, unreviewed reads inside, the commit and the curator profile. |
| [ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) OAuth by connect code | accepted | Introduced in v0.4.0. |
| [ADR-0033](../adr/ADR-0033-interim-helm-chart.md) the interim Helm chart | proposed | Introduced in v0.3.0, while the record is still proposed. |
| [ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) the server serves as an account that owns nothing | accepted | Partly in v0.3.1: the two accounts, the startup refusal, the per-organisation search index and the keys that carry the organisation. Its spike groundwork shipped in v0.3.0. The separate bootstrap workload and the health check for the split are not scheduled; one further hardening step waits on search measurements. |
| ADR-0035 deliverables | proposed | Parked. |
| ADR-0036 time | proposed | Parked. |
| [ADR-0037](../adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md) guardrails | accepted | Introduced in v0.5.1; the export's refusal inside an in-progress workarea and the time-boxed freeze wait for ADR-0030 and ADR-0036. |
| [ADR-0038](../adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md) turn context | accepted | Introduced in v0.4.0. |
| [ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) the API rewrite | accepted | Introduced in v0.4.0; the hidden v0.3.0 names it kept answered through v0.5.0 and were removed in v0.5.1 (d12). |
| [ADR-0040](../adr/ADR-0040-plugin-setup-in-claude-code.md) plugin setup | accepted | Introduced in v0.4.0. |
| [ADR-0041](../adr/ADR-0041-a-quick-start-in-one-command-per-role.md) quick start per role | accepted | Partly in v0.4.0: the installers, `oml login`, `oml doctor`, `oml setup claude-code`, `oml quickstart` and the install page. `install.sh --quickstart` and the short install address are not in it. |
| [ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) request logs, metrics and traces | accepted | Introduced in v0.4.1. |
| [ADR-0043](../adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md) the default server | accepted | Introduced in v0.4.1. |
| [ADR-0044](../adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) a workarea of one's own for each person, and a curated shared area | accepted | Introduced in v0.5.0. |
| [ADR-0047](../adr/ADR-0047-project-knowledge-and-a-code-skeleton-follow-the-development-lifecycle.md) project knowledge and the code skeleton | proposed | Planned, not assigned; gated on its evaluation harness and on ADR-0030. Its first slice is ADR-0057, introduced in v0.5.8. |
| [ADR-0048](../adr/ADR-0048-search-improves-in-stages-any-word-first-meaning-last-each-gated-on-the-recall-eval.md) search in stages | proposed | Partly built while the record is still proposed: S1–S3 introduced in v0.5.5 as ADR-0049's first pipelines, S6 in v0.5.6 as ADR-0053's `bm25-rerank`; S4 (filters) and S5 (author keywords) not built; S7 is ADR-0052's learned sparse kind, introduced in v0.5.7 as the opt-in `sparse` and `bm25-sparse`. |
| [ADR-0049](../adr/ADR-0049-search-is-a-pipeline-of-retrievers-behind-a-port-chosen-by-name-and-gated-by-the-recall-eval.md) search pipelines | proposed | Introduced in v0.5.5 (increment 1, with ADR-0048's first three stages), while the record is still proposed; d13, each retriever's list capped before fusion, introduced in v0.5.8; later increments not assigned. |
| [ADR-0052](../adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md) search from an index under row security | accepted | Introduced in v0.5.6: `bm25-postings`, the default; the learned sparse kind, `sparse` and `bm25-sparse` (d13–d17), introduced in v0.5.7, opt-in; since v0.5.8 each list `bm25-sparse` fuses is capped at its depth before fusion (ADR-0049 d13). |
| [ADR-0053](../adr/ADR-0053-reranking-is-a-local-cross-encoder-stage-after-fusion-off-by-default-until-it-fits-the-latency-budget.md) reranking and the relevance floor | accepted | Introduced in v0.5.6: `bm25-rerank`, opt-in, and the relevance floor, off by default. |
| [ADR-0057](../adr/ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md) source anchors and `oml sync` | accepted | Introduced in v0.5.8: the anchor on every memory, the origin filters, `stale` and source checks, `expected_version` on every supersession, `oml sync` and `oml anchor`. |
| [ADR-0054](../adr/ADR-0054-recall-is-measured-by-what-readers-do-next-and-only-other-readers-use-moves-a-rank-within-a-bound.md) recall and remember quality, and usage as a ranking signal | accepted in part | The measurement half (d1–d6, d11–d13) introduced in v0.5.7: the quality metrics and dashboard, `recall_events` and `recall_outcomes`, the opt-in query text, the inferred labels and the `feedback` member, and the replay; the ranking half (d7–d10) proposed, not assigned, pending its open question 6. |
| [ADR-0055](../adr/ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md) bulk review | accepted | Introduced in v0.5.7: bulk review on the admin page and in the CLI, and `same_user` on a claim's approval. |
| [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) sign-in to a user page and an admin page | accepted | Introduced in v0.5.0: sign-in, sessions, the email list, the user page with its keys, setup, memories, workareas and review of one's own agents' work, and the admin page's list, capacities, queue, review, curation, replacing memories and reading any memory with its attribution. Demotion on the admin page is planned for v0.6.0, since it waits for a verb that is not built. |
| [ADR-0046](../adr/ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md) the lifecycle, claim edges and the brief in v0.5.2 | accepted | Introduced in v0.5.2; the `oml_write` `links` field and a view on the web pages are planned for a later record. |
