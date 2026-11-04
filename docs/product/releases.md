<!--
id: product/releases
tier: client
status: release record · shipped releases from the tags, planned releases as decided on 2026-11-03
answers: which release introduced each feature and decision, which release each planned one is for, and which are parked or not scheduled
-->

# Releases

**Status:** release record · shipped releases from the tags, planned releases as decided on 2026-11-03

This page is the one place that says when a feature arrived or when it is planned to arrive.
Every other page links here instead of repeating it. A shipped release is a signed tag on the
[releases page](https://github.com/e8kor/open-memory-lab/releases); what it holds is read from
the tagged source, not from intentions. A planned release is a plan the maintainer made on
2026-11-03: nothing in it is built until its tag exists, and its contents can still move.

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

## Planned releases

Nothing in this section is released. Each row is the plan of 2026-11-03.

| Version | Status | What is planned | Decisions |
|---|---|---|---|
| **v0.5.0** | planned | Guardrails: reviewed rules loaded at every session start and checked before each tool call. Workareas that move from development to committed to validated, each step proposed by one identity and confirmed by another. Labelled links between memories. A brief for each workarea: a few reviewed memories, confirmed as the area's brief, loaded when an agent enters it. A workarea of one's own for each person, created when their first key is issued, where their keys write, read and make workareas, so two people can each have a `project`; the organisation's shared area above them, read by everyone and written only by curators an administrator chooses, each copy reviewed by someone else and shown without its author or its source. Workareas and keys made before then stay where they are. Two web pages on the server's own address, for people and never for agents: sign in with Google or GitHub, if an administrator has put the address on the organisation's list; on the user page, see and make your own agent keys and connect codes, read the setup steps for each agent tool, browse your memories and the shared area's, and make workareas of your own; on the admin page, keep the list, choose who reviews and who curates, see how many memories wait and for how long, and review, curate and replace memories, each act done by the person's own reviewer or curator identity, never the administrator's. | ADR-0037 (proposed); [ADR-0044](../adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) (proposed); [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) in part (proposed); [ADR-0009](../adr/ADR-0009-status-ladder.md) and the memory links of [ADR-0008](../adr/ADR-0008-workarea-algebra.md) and [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md) (accepted, not built) |
| **v0.6.0** | planned | In-progress workareas: inside a development workarea that opts in, readers recall memories still waiting for review, marked as such, and nothing leaves the workarea until it is committed with nothing waiting. On the admin page, who wrote, derived and reviewed any one memory, without its text, and demoting a memory, which keeps it. | ADR-0030 (proposed); [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) in part (proposed) |
| **Parked** | parked | Ontology crews that classify memories and curate shared vocabularies, and the vocabularies themselves. Deliverables: the files a memory chain ends in, beyond the storage component already written on a branch. Time: stating when a memory is true and reading what OML knew at a past moment. A digest that summarises a whole workarea for an agent entering it. | ADR-0027, ADR-0035, ADR-0036 (proposed); [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md) (accepted, not built) |

Proposed records live on branches and draft pull requests until they are accepted:
ADR-0027 ([#10](https://github.com/e8kor/open-memory-lab/pull/10)), ADR-0030
([#12](https://github.com/e8kor/open-memory-lab/pull/12)), ADR-0035
([#17](https://github.com/e8kor/open-memory-lab/pull/17)), ADR-0036
([#19](https://github.com/e8kor/open-memory-lab/pull/19)), ADR-0037
([#20](https://github.com/e8kor/open-memory-lab/pull/20)); ADR-0044 is on the branch `feat/user-roots`, and ADR-0045 on `feat/web-console`. The number ADR-0032 is not used by any record.

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
| Guardrails | planned for v0.5.0 |
| Workarea lifecycle: development, committed, validated | planned for v0.5.0 (workareas carry the development status since v0.1.0) |
| Labelled links between memories | planned for v0.5.0 |
| A workarea of one's own for each person, and a curated shared area above them | planned for v0.5.0 |
| A brief for each workarea | planned for v0.5.0 |
| Sign-in with Google or GitHub to a user page and an admin page | planned for v0.5.0 |
| Review, curation and replacing memories from the admin page | planned for v0.5.0 |
| Who wrote, derived and reviewed a memory, and demotion, on the admin page | planned for v0.6.0 |
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
| [ADR-0008](../adr/ADR-0008-workarea-algebra.md) workareas | accepted | Partly in v0.1.0: one tree per organisation and access bound to a workarea's id. v0.2.0 added grants combined with denials that win; v0.3.0 added crew records. Links are planned for v0.5.0; moving a workarea is not scheduled. |
| [ADR-0009](../adr/ADR-0009-status-ladder.md) the workarea lifecycle | accepted | Decided, not built; planned for v0.5.0. Workareas are created in development since v0.1.0. |
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
| [ADR-0021](../adr/ADR-0021-ai-human-ontologies.md) shared vocabularies | accepted | Decided, not built. Links between memories are planned for v0.5.0; vocabularies are parked. |
| [ADR-0022](../adr/ADR-0022-two-ring-api.md) the two-ring API | accepted | Introduced in v0.1.0: one listener, a stable agent ring and an evolving lab ring, MCP over both. The compatibility promise is not yet in force. Amended by the v0.4.0 rewrite (ADR-0039). |
| [ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md) install profiles and cluster mode | accepted | Partly in v0.1.0: one host and Docker, with a file keyring. The Kubernetes chart in v0.3.0. Read followers and cluster mode are not scheduled. |
| [ADR-0024](../adr/ADR-0024-performance-foundation.md) the performance foundation | accepted | Not scheduled as a release item. No part of it is measured; batch writes and streamed attachments are not built. |
| [ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) the agent-only memory surface | accepted | Partly in v0.2.0: one key, own held writes in recall, budgets, ordering, versions, the own-work listing, the workarea read and the filtered feed. The separate author-only search index, the read record and changing grants after issue are not scheduled. |
| [ADR-0026](../adr/ADR-0026-names-are-keys.md) names are keys | accepted | Partly in v0.3.0: names on every verb that is served. Names on unbuilt verbs (key revocation, rotation, the key listing, the lookup by path) and the operator's names are not scheduled. |
| ADR-0027 ontology crews | proposed | Parked. |
| [ADR-0028](../adr/ADR-0028-agent-key-issuance.md) agent key issuance | accepted | Introduced in v0.2.1; the key's name as its first argument since v0.3.0. |
| [ADR-0029](../adr/ADR-0029-session-start-hook.md) the session-start hook | accepted | Introduced in v0.2.2. |
| ADR-0030 in-progress workareas | proposed | Planned for v0.6.0. |
| [ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) OAuth by connect code | accepted | Introduced in v0.4.0. |
| [ADR-0033](../adr/ADR-0033-interim-helm-chart.md) the interim Helm chart | proposed | Introduced in v0.3.0, while the record is still proposed. |
| [ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) the server serves as an account that owns nothing | accepted | Partly in v0.3.1: the two accounts, the startup refusal, the per-organisation search index and the keys that carry the organisation. Its spike groundwork shipped in v0.3.0. The separate bootstrap workload and the health check for the split are not scheduled; one further hardening step waits on search measurements. |
| ADR-0035 deliverables | proposed | Parked. |
| ADR-0036 time | proposed | Parked. |
| ADR-0037 guardrails | proposed | Planned for v0.5.0. |
| [ADR-0038](../adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md) turn context | accepted | Introduced in v0.4.0. |
| [ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) the API rewrite | accepted | Introduced in v0.4.0. |
| [ADR-0040](../adr/ADR-0040-plugin-setup-in-claude-code.md) plugin setup | accepted | Introduced in v0.4.0. |
| [ADR-0041](../adr/ADR-0041-a-quick-start-in-one-command-per-role.md) quick start per role | accepted | Partly in v0.4.0: the installers, `oml login`, `oml doctor`, `oml setup claude-code`, `oml quickstart` and the install page. `install.sh --quickstart` and the short install address are not in it. |
| [ADR-0042](../adr/ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) request logs, metrics and traces | accepted | Introduced in v0.4.1. |
| [ADR-0043](../adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md) the default server | accepted | Introduced in v0.4.1. |
| [ADR-0044](../adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) a workarea of one's own for each person, and a curated shared area | proposed | Planned for v0.5.0; decided, not built. |
| [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) sign-in to a user page and an admin page | proposed | Planned for v0.5.0 in part: sign-in, sessions, the email list, the user page, and review, curation and replacing memories on the admin page. The admin page's attribution view and demotion are planned for v0.6.0, since each waits for a verb that is not built. Decided, not built. |
