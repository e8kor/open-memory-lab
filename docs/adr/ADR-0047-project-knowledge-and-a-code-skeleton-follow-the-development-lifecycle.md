<!--
id: adr/ADR-0047-project-knowledge-and-a-code-skeleton-follow-the-development-lifecycle
tier: engineering
status: proposed
answers: what OML stores about a client project's code (a small structural skeleton and the knowledge the code cannot tell) and what it does not; how each entry is anchored to code and found stale; how branches, pull requests, merges and releases map onto workareas, review and the status ladder, and who drives each step with R1 and R2 intact; how churn stays inside a branch; how repository files are imported and generated; when an automated reviewer may approve; which parsers it reuses; the preconditions and the evaluation that gate the build; which downsides are arguments and which are opportunities; what core function must not degrade; and the questions left for the maintainer
-->

# ADR-0047 — Project knowledge and a code skeleton follow the development lifecycle

**Status:** proposed · 2026-11-04

**Release:** planned, not assigned; gated on the evaluation harness (d17) and ADR-0030 ([releases](../product/releases.md#decisions))

**Amends:** ADR-0025 (d10, d12), ADR-0029 (d2, d7), ADR-0039 (d3); and, on their branches,
ADR-0030 (d1), ADR-0044 (d6), ADR-0045 (d9)

**Depends on:** [ADR-0048](ADR-0048-search-improves-in-stages-any-word-first-meaning-last-each-gated-on-the-recall-eval.md)
for the anchor and kind filters on reads.

The design mockups for this record and ADR-0048 are on one page:
[the mockups](../design/code-map/mockups.html) (the lifecycle flow, a pull request's knowledge
review, the CI gate, a Claude Code session, the hybrid search result, the user page's Project
knowledge view and stored entries).

## Context

**The operator's question of 2026-11-04.** Does OML gain from a code mapping feature, with review
tied to the project's development lifecycle rather than immediate, and are the downsides strong
arguments or chances to improve the product without degrading its core? This record answers for
the code side; [ADR-0048](ADR-0048-search-improves-in-stages-any-word-first-meaning-last-each-gated-on-the-recall-eval.md)
answers for search. A first draft of this record stored a full structural map of every file; the
operator narrowed it the same day, and that draft is the first rejected alternative.

**What other tools do.** A survey of nineteen products and papers, every page fetched on
2026-11-04:

- *Structure is derived locally, on demand.* Aider's map "contains a list of the files in the
  repo, along with the key symbols which are defined in each file", built with tree-sitter and
  recomputed as the chat changes ([aider.chat](https://aider.chat/docs/repomap.html),
  [aider.chat, 2023-10-22](https://aider.chat/2023/10/22/repomap.html)); Serena answers from
  language servers, "operating at the symbol level" ([Serena](https://github.com/oraios/serena));
  GitNexus "indexes code locally without an LLM"
  ([GitNexus](https://github.com/abhigyanpatwari/GitNexus)). An agent without a checkout
  (claude.ai, a phone) has none of this; that is the case for storing a small skeleton.
- *Generated prose fails and goes stale.* Of DeepWiki: "it frequently produces hallucinations and
  factual errors", and it "does not get regenerated if the repo is updated"
  ([OpenWrt forum, 2026-04-26](https://forum.openwrt.org/t/important-consideration-deepwiki-ai-generated-docs-may-contain-inaccuracies/249471)).
- *Facts cite code and are checked when used.* Copilot Memory keeps "citations pointing to the
  code that supports the fact" and "checks the fact's citations against the current branch"; it
  warns that a fact from one branch may be "modified, superseded, or never merged at all"
  ([GitHub docs](https://docs.github.com/en/copilot/concepts/agents/copilot-memory)).
- *Injection is budgeted.* Claude Code advises "target under 200 lines per CLAUDE.md file. Longer
  files consume more context and reduce adherence"
  ([Claude Code memory](https://code.claude.com/docs/en/memory)).

**The competition is the repository.** `CLAUDE.md`, `AGENTS.md`, `.cursor/rules` and `docs/adr`
are versioned, reviewed in the pull request, free and read by every harness. For knowledge that
belongs to the code, a separate gate duplicates code review. OML is worth its cost for knowledge
that does not belong in the repository (operational, customer-specific, learned in a chat or on
a phone, spanning repositories); for more facts than can be injected whole; for capture without
a pull request of its own; and where a wrong shared fact is costly. This record turns the
competition into an input (d12): repository files are imported, and generated from what OML holds.

**Benefit is unproven.** Independent, peer-reviewed: context files "do not generally improve
task success rates", cost "over 20%" more, and "repository overviews ... are not helpful"
([arXiv 2602.11988 v3, ICLR 2026](https://arxiv.org/abs/2602.11988)); a line-level definition
graph gave "average relative improvement of 32.8%" on SWE-bench Lite
([arXiv 2410.14684](https://arxiv.org/html/2410.14684)). Vendors on their own products: GitHub,
pull-request merge rate 83% to 90% with Copilot Memory
([GitHub blog, 2026-01-15](https://github.blog/ai-and-ml/github-copilot/building-an-agentic-memory-system-for-github-copilot/));
Augment, "improved agent performance by 70%+"
([Augment, 2026-02-06](https://www.augmentcode.com/blog/context-engine-mcp-now-live)). Nothing is
measured on OML; d17 makes that the gate.

**What OML brings to the lifecycle.** The status ladder moves a workarea `development` →
`committed` → `validated`, each step proposed and ratified by different identities
([ADR-0009](ADR-0009-status-ladder.md) d3); an in-progress workarea lets every reader inside see
held writes at once and lets nothing out until its commit, which is refused while anything is held
(ADR-0030 d1, d4, d12, on `feat/in-progress-workareas`). A branch, its pull request and its merge
have that shape. Decided, not built.

## Decision

- **d1 — Two things are stored: a small, stable code skeleton and the knowledge the code cannot
  tell; per-file and per-symbol structure is not.** The **skeleton** is one entry per component
  (a top-level package, crate, module or service): its path, its purpose, its public entry points
  (binaries, served routes, exported top-level symbols) and its dependencies on other components.
  A repository has tens of these, not thousands, and they change when the architecture does, not
  on every commit. It exists for readers with no checkout. **Knowledge** is a decision and its
  reason, a convention, a gotcha, an abandoned approach and why, an operational fact, or a fact
  spanning repositories. Per-file maps, per-symbol entries, signatures and imports are not stored:
  they are derivable and are computed locally when needed.

- **d2 — Both are a new claim kind, `code`, so they never crowd ordinary recall.** Recall serves
  kind `code` only when `kinds` names it (ADR-0048 d5), the way vocabulary kinds are kept out of
  the shared indexes ([retrieval](../architecture/retrieval.md)). A `code` entry is written with
  `oml_write` like any memory (amends ADR-0039 d3 by one kind value), and its text is prose a
  person can review. The cost is one more kind in every enum and one more filter for an agent to
  remember, which the skill and the hooks carry.

- **d3 — Every `code` entry carries an anchor `{repo, branch, path?, symbol?, commit, hash?,
  hasher?}`, completed on the client.** `repo` is the name `oml code init` gives the repository;
  `branch` is the branch it was written on; `path` and `symbol` name what it describes, or are
  absent for a fact about the whole repository; `commit` is `HEAD` at the write; `hash` is d4's
  hash of the symbol, or of the file when no symbol is named. The agent names `path` and `symbol`;
  `oml hook code-anchor`, on `PreToolUse` for `oml_write`, fills the rest and refuses a path git
  ignores, a vendored or generated file, or a path shaped like a secret. Harnesses without that
  hook run `oml anchor <path>[#symbol]`, which prints the anchor for the agent to pass. The server
  checks the anchor's grammar and computes nothing from it.

- **d4 — The symbol hash is computed on the client from a symbol's span, with no grammar of
  OML's own.** The client takes the symbol's line range from universal-ctags' JSON output
  (`--fields=+ne`) or, where a language server runs, from `textDocument/documentSymbol`; takes
  the span's text; drops lines that are only a comment, by a table of line-comment markers per
  file extension; collapses whitespace; and hashes the result with SHA-256: hasher `oml-span/1`.
  Reformatting and moving the symbol leave the hash unchanged; a change to its name, signature or
  body changes it; a comment inside a code line still changes it. With no ctags and no language
  server, the hash is of the whole file after the same normalisation, and every edit to the file
  marks its facts stale. Nothing structural is stored to compute it: the hash is recomputed from
  the working tree whenever it is compared.

- **d5 — A branch is an in-progress development workarea under the repository's workarea.** The
  repository's workarea, *R*, is `<user>.<repo>` for a person working alone (ADR-0044) or a shared
  workarea an admin creates once for a team, granting the team's agent keys `writer`, each
  reviewer's key `dreamer` and the CI key `dreamer` (d8). *R* is marked a **branch parent**.
  The first `code` write on a branch creates its workarea, a child of *R* named
  `<repo>-b-<slug>-<first 7 hex of the branch's first commit>` so that a reused branch name never
  reuses a workarea name ([ADR-0026](ADR-0026-names-are-keys.md)), in `development` and marked in
  progress (ADR-0030 d1): the team's agents on that branch read each other's held entries at once,
  marked unreviewed, and nothing leaves until the commit. A new entry goes to the branch workarea;
  an update or retirement of an entry that already lives elsewhere is written where the entry
  lives (a supersession stays in its target's workarea), held, and carries the branch in its
  anchor so d6 collects it.

- **d6 — A pull request is a review batch, reviewed by the pull request's reviewer in the same
  sitting as the code, and capped.** The batch is every held version in the branch workarea and
  every held version elsewhere whose anchor names the branch. A CI step on the pull request
  (`oml code pr`) posts a check with the batch's counts by added, updated, retired and stale, and
  a link to the user page's Review view filtered to the batch (amends ADR-0045 d9); fact text is
  posted to the forge only with `--post-text`, since a pull request may be more widely read than
  the workarea. The reviewer approves or rejects each entry under their own `<user>-review` key
  (R2 per key and principal, ADR-0044 d8). A batch over `pr.max_entries`, 20 unless set, chosen
  and unmeasured (FR-79), fails the check: past that, review stops meaning anything.

- **d7 — A merge is the branch workarea's commit; an abandoned branch leaves nothing promoted.**
  The commit is ADR-0030 d12's `development` → `committed` transition, refused while any version
  inside is held, so every entry of the merged branch has been approved or rejected. Two
  identities: the reviewer proposes it from the batch view once the batch is empty, and the CI
  key ratifies it in the merge job (ADR-0009 d3, R2 for transitions). From the commit, the branch
  workarea has no in-progress root, and its promoted entries reach every read at *R*, since a read
  at *R* covers its subtree. A pull request closed unmerged makes the CI key reject the branch's
  held entries with the reason `branch_abandoned`: a rejection promotes nothing, so no automated
  identity ever decides that something is true. No agent key proposes, ratifies or approves
  anything (R1).

- **d8 — The CI key is a dreamer held by an agent, never a service key, and it approves only what
  it can re-derive.** It ratifies merge commits, rejects abandoned batches, and approves the
  mechanical parts of skeleton entries (d9). A `held_by: service` key is refused every review
  verb ([roles and keys](../architecture/roles-and-keys.md)), so it is issued `held_by: agent`. It
  lives in the forge's secret store, out of every agent's reach, and is never configured in the
  plugin.

- **d9 — An automated reviewer approves only skeleton facts it re-derives from the code at the
  anchored commit; it never approves model-written knowledge.** A skeleton entry has two parts:
  the mechanical part (path, entry points, dependencies between components), rendered by
  `oml code skeleton` from build manifests (`Cargo.toml`, `package.json`, `pyproject.toml`,
  `go.mod`, `pom.xml`) and ctags output, and the purpose line, which a person or the agent writes.
  The two are written as two entries. The CI key fetches the anchored commit, re-renders the
  mechanical entry, and approves only on byte equality, rejecting with `mismatch` otherwise. A
  purpose line, a decision or a gotcha goes to a person: a reviewer from the writer's model
  family shares the writer's errors, so its approval adds little that R2 is meant to add.

- **d10 — A release is `validated` for a release record, not for living knowledge.** A frozen
  area admits no supersession ([ADR-0009](ADR-0009-status-ladder.md) d5), so freezing the merged
  branch workareas would make their facts uncorrectable for good. At a release tag, CI creates a
  child of *R*, `<repo>-r-<version>`, holding the release's own record (its known issues and
  operational facts, written and reviewed like any batch), which climbs to `validated` and freezes
  as evidence of what shipped. Living knowledge stays correctable in its committed workareas.
  Question 3 holds the alternative the operator named.

- **d11 — Staleness is a workflow: an edit marks facts stale locally, the agent answers at Stop,
  and the merge gate checks the rest.** At session start in an enabled clone the session-start
  hook lists the `code` entries anchored in this repository at *R* and the current branch
  workarea (ADR-0048 d5's filters) into a per-session cache that SessionEnd deletes (amends
  ADR-0029 d2). `oml hook code-mark`, on `PostToolUse` for `Write|Edit|MultiEdit`, recomputes the
  hashes of anchors in the edited file and records the entries whose symbol changed or vanished,
  sending nothing. At `Stop`, `oml hook code-stop` blocks once, honouring `stop_hook_active`,
  naming those entries and asking for each: **confirm** (a supersession with the same text and a
  fresh anchor), **update** (corrected text) or **retire**. In CI, `oml code check --base <merge
  base>` fails the pull request while an entry anchored to a symbol the diff changed has no
  successor, confirmation or retirement in the batch. The cost: a correct fact on a changed symbol
  costs one review to confirm. `plugins/oml/hooks/hooks.json` gains `PreToolUse` `mcp__plugin_oml_oml__oml_write` →
  `code-anchor`, `PostToolUse` `Write|Edit|MultiEdit` → `code-mark` and `Stop` → `code-stop`
  (amends ADR-0029 d7); each prints nothing where the clone is not enabled, reads only the
  plugin's own key pair and never the CLI's credentials file (ADR-0029 d4), fails open, and
  writes nothing, so ADR-0029 d1's "performs no act" still holds.

- **d12 — Repository files are an input and an output.** `oml code import CLAUDE.md` (and
  `AGENTS.md`, `.cursor/rules/*`, `docs/adr/*`) splits a file by heading and list item into held
  `code` entries in the branch workarea, anchored to the file, so the first pull request reviews
  them like any batch. `oml code export --to AGENTS.md` writes the promoted entries of *R*
  between generated markers, in a fixed order, so a team that wants its knowledge under code
  review keeps it there and every harness reads it natively; OML is then the managed source
  behind the file. Text outside the markers is never touched. Export never reads a held entry.

- **d13 — Capture is the agent's, at low volume.** The `oml-memory` skill tells the agent to
  write a `code` entry when it learned something by failing, took a decision whose reason the
  code will not show, was told a convention, or abandoned an approach; not what the code or a
  repository file says, nor task status. Before a write, `code-anchor` shows the entries already on
  that anchor and refuses the write once, so the agent supersedes instead of duplicating, unless
  it marks the write `distinct`. A session writes at most `capture.max_per_session` entries, 5
  unless set (FR-79). No hook writes, and no hook asks for capture: a prompt at every Stop is how
  noise gets in, and noise is what empties review of meaning.

- **d14 — Churn stays in the branch, and a merge promotes one version.** An agent revising its
  own held entry supersedes it (d15 c), so a branch may hold many superseded drafts and a merge
  promotes the last. Every draft stays, since nothing deletes ([ADR-0001](ADR-0001-the-constitution.md)
  R4). A **recorded compaction** would erase the bodies of never-promoted, superseded versions of
  a committed or abandoned branch workarea as one journalled erasure per workarea, keeping every
  version's metadata and digest. R4 names erasure by crypto-shredding as the journalled form of
  removal, which is not built ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md), not
  scheduled); whether a policy-driven erasure with no person's act is "silent" is a reading of the
  constitution this record does not take. It is question 6.

- **d15 — The backend adds four things, and no tool.**
    - **a. Kind `code` with its anchor** (d2, d3): accepted by `POST /v1/memories`, its
      supersession and `oml_write`; the anchor required on kind `code` and refused on every other
      kind (`request.validation`), as ADR-0037 d10 does for `severity` and `rule`; served in the
      claim item (amends ADR-0025 d12).
    - **b. A writer creates branch workareas under a branch parent.** An admin marks a workarea
      a branch parent; a writer key with effective `writer` on it may create a child that is
      `development` and in progress at creation, and nothing else. Amends ADR-0044 d6, which
      admits creation by a user key in its own root only, and ADR-0030 d1, under which only an
      admin marks a workarea in progress.
    - **c. The author may supersede or withdraw its own held version (closes
      [RS-17](../product/roadmap.md)).** The held predecessor moves to `superseded`, leaves the
      review queue and never promotes; an open serve of it answers `review.serve_invalid`; the
      supersession event carries `while_held: true`. A retirement of one's own never-promoted
      version withdraws it without review, since no one outside an in-progress area has read it.
      Amends ADR-0025 d10.
    - **d. The anchor filters** on reads, declared in ADR-0048 d5 (its stage S4).

    Not added: the batch route (a branch writes tens of entries, not thousands), a hook that
    writes, server-side staleness, per-file entries, and a fourth MCP tool
    ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d8).

- **d16 — Preconditions, stated as scope.** Client code is mapped only on a server the client
  runs, or in an organisation of its own on a shared one, which waits on organisations beyond
  the first, not scheduled ([releases](../product/releases.md#not-scheduled)); `oml code init`
  refuses `https://memory.doublegate.io`
  ([ADR-0043](ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md)) unless
  `--allow-public-server` is given, after printing that the server's admins read every entry
  (ADR-0045 d21) and that nothing written can be erased. Entries carry no source body, diff or
  string literal; the client's secret filter (the patterns of the server scan and of
  [ADR-0038](ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md)
  d5) runs before the server's scan ([admission](../architecture/admission.md)). Storing more
  than paths, symbol names and prose about them, such as code excerpts, waits on erasure
  (ADR-0005). The committed `.oml/code.toml` names no server, key or workarea, and only a local
  enablement under `.git/oml/` turns the hooks on, so cloning a repository maps nothing.

- **d17 — An evaluation harness gates everything beyond the skill change.** `testing/eval/` runs a
  fixed task set in three arms: no memory; OML (the brief, the anchored reads and recall as built);
  and the same promoted entries exported to `AGENTS.md` (d12), the real competitor. Tasks are
  closed issues with a merged fix and a test, from this repository and two the maintainer names,
  each run at the fix's parent commit with the merged test as the oracle; entries are those
  captured while solving earlier tasks of the same repository. It records task success, tokens and
  wall time per task, and the share of served entries that were stale. Sample size and runs per
  task are unmeasured (FR-78). The build continues past step 1 only when the maintainer accepts the
  result under question 8's criterion.

- **d18 — The build, each step closed by named tests.**
    0. **Skill text** (allowed before the gate): `check_plugin.py` holds d13's thresholds.
    1. **Harness:** `the_harness_runs_each_task_in_three_arms_at_the_parent_commit`,
       `the_oracle_is_the_merged_test`, `tokens_and_wall_time_are_recorded_per_task`.
    2. **Server:** `an_anchor_is_required_on_kind_code_and_refused_elsewhere`,
       `recall_serves_kind_code_only_when_kinds_names_it`,
       `a_writer_creates_an_in_progress_child_only_under_a_branch_parent`,
       `an_author_supersedes_its_own_held_version_and_the_predecessor_never_promotes`,
       `an_author_withdraws_its_own_never_promoted_version_without_review`.
    3. **Client:** `the_span_hash_ignores_whitespace_and_comment_lines`,
       `the_span_hash_changes_with_the_signature_or_body`,
       `code_anchor_refuses_a_secret_shaped_path`,
       `code_anchor_refuses_a_duplicate_anchor_once`,
       `code_mark_sends_nothing`, `code_stop_blocks_once_and_not_when_stop_hook_active`,
       `no_code_hook_reads_the_cli_credentials_file`,
       `init_refuses_the_public_server_without_allow_public_server`.
    4. **CI:** `the_merge_job_ratifies_the_commit_the_reviewer_proposed`,
       `a_closed_pull_request_rejects_its_held_batch_as_branch_abandoned`,
       `code_check_fails_while_a_changed_symbol_has_an_unanswered_entry`,
       `a_batch_over_pr_max_entries_fails_the_check`,
       `the_ci_key_approves_a_byte_equal_skeleton_and_never_a_purpose_line`.
    5. **Files and page:** `import_splits_a_file_into_held_entries_by_heading_and_item`,
       `export_writes_only_promoted_entries_between_the_markers`,
       `the_project_knowledge_view_writes_nothing`.

- **d19 — Every new failure has a doctor line.** `oml doctor` in a clone names: a committed
  `.oml/code.toml` with no local enablement; the public default server without its recorded
  confirmation; no ctags and no language server (whole-file hashes in use); stale entries by
  count; a branch workarea with held entries and no open pull request older than a week; a pull
  request batch over its cap; a CI key that is `held_by: service`; and a branch parent with no
  dreamer able to ratify.

## Downsides: argument or opportunity

| Downside | Verdict | Reasoning, and what it implies |
|---|---|---|
| Repository files already do this, free and reviewed | opportunity | Import and export (d12) make OML the managed source behind them; OML keeps what does not belong in the repository and what exceeds a file's injectable size. Implies: d12 ships with the first build step after the gate. |
| A stored code map churns and duplicates git | solved | Only a skeleton of tens of entries is stored (d1); structure is computed locally (d4). |
| Review becomes overhead if it is immediate and per write | solved | Review is per pull request, by the reviewer already reading the code (d6), capped (d6). |
| Automatic capture is noise; a same-family automated reviewer has correlated errors | argument, bounded | No capture prompt, a session cap and a duplicate check (d13); automation approves only what it re-derives (d9). Implies a scope limit: volume stays low by design, and a team wanting bulk capture is not served. |
| Facts go stale as code changes | opportunity | The symbol hash and the merge gate (d4, d11) make staleness a step of the workflow; a repository file has no such check. |
| Hosted trust: one person's server, whose admins read everything | argument | Implies a precondition: self-hosting or an organisation per client (d16). |
| Permanence: nothing written can be erased | argument | Implies a precondition: no code content beyond names and prose until ADR-0005 is built (d16); compaction waits on it too (d14). |
| Benefit is unproven | argument | Implies the gate (d17). |
| Parser maintenance | argument, reduced | No grammar of OML's own (d4); ctags or a language server is a dependency the user installs, and its absence degrades to whole-file hashes. Implies a doctor line, not a crate. |
| Keyword-only recall misses paraphrase | opportunity | ADR-0048; anchors already find entries by location, not words. |
| Workareas accumulate, one per merged branch | argument | Nothing deletes a workarea; the count is unmeasured (FR-80). Implies question 2's alternative of one workarea per repository if the count proves costly. |

**What must not degrade, and how it is protected.** R1: no agent key proposes, ratifies,
approves or rejects; the CI key is not in the plugin (d7, d8). R2: every verdict and every
ratification is a second key and principal; automation approves only re-derivable text (d9). R3:
held entries are read beyond their author only inside an in-progress branch workarea, and
nothing leaves it before its commit (d5, d7). R4: nothing is deleted; compaction is an open
question, not a decision (d14). The three MCP tools stay three (d15). Ordinary recall's quality
and latency are untouched, since kind `code` is outside it unless asked (d2). Review keeps its
meaning because it is capped, human for prose, and done where the code is read (d6, d9).

## Consequences

- **Roadmap rows.** RS-17 is answered by d15 c. FR-78 opens (the evaluation's sample size and the
  benefit), FR-79 (the capture cap and the batch cap, chosen not measured), FR-80 (the count of
  branch workareas a repository accumulates and its cost to tree reads).
- **Dependencies.** ADR-0030 (in-progress), ADR-0044 and ADR-0045 (user roots and the Review
  view), ADR-0046 (the brief, for the skeleton at session start), ADR-0048 (the filters).
- **Pages to change at acceptance.** The HTTP API and openapi.yaml (kind `code`, `anchor`, the
  branch-parent create, `while_held`); the agent plugin and mcp-tools.json; the CLI reference
  (`oml code init|skeleton|pr|check|import|export`, `oml anchor`, the hooks); workareas (the
  branch parent); admission (RS-17); and each amended record's Status line.

## Open questions for the maintainer

| # | Question | Options | Recommended |
|---|---|---|---|
| 1 | How is a correct fact on a changed symbol confirmed? | a. a supersession with the same text and a fresh anchor, reviewed; b. a re-anchor act without review; c. left stale until wrong | a: no unreviewed change reaches a promoted entry |
| 2 | One workarea per branch, or per repository? | a. per branch, merge = commit; b. one in-progress workarea per repository, merge = approving the batch, no commit | a (d5, d7), revisited if FR-80 shows the count costly |
| 3 | What does a release validate? | a. a release-record child per release; b. the merged branch workareas themselves; c. nothing | a (d10): b freezes living knowledge for good |
| 4 | Who proposes and who ratifies the merge commit? | a. the reviewer proposes, the CI key ratifies at merge; b. the CI key proposes, the reviewer ratifies; c. two reviewers | a (d7): one act by the person, in the sitting |
| 5 | May a writer create branch workareas? | a. under an admin-marked branch parent only; b. the CI key creates them on first push; c. an admin creates each | a (d15 b): the agent writes before any push |
| 6 | Is compaction allowed, and how? | a. a journalled erasure per workarea of never-promoted superseded versions, by a person's act, after ADR-0005; b. the same, by policy, with no person's act; c. never | a (d14): b needs a reading of R4 the constitution does not give |
| 7 | Is fact text posted to the forge? | a. counts and a link only, text on `--post-text`; b. text always; c. nothing | a (d6) |
| 8 | What does the gate require? | a. OML beats no memory and is no worse than the `AGENTS.md` export, at no more tokens; b. beats no memory; c. the maintainer's judgement | a (d17) |
| 9 | Where does the skeleton's purpose line come from? | a. the agent writes it, a person approves; b. a person writes it; c. none | a (d9) |
| 10 | What is the batch cap? | a. 20 entries per pull request; b. 10; c. none | a (d6), revisited with FR-79 |
| 11 | Should the public server be allowed at all for client code? | a. only with `--allow-public-server`; b. never | a (d16) |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A stored two-layer code map**: one reviewed claim per file with symbols, signatures and imports, a verifier re-deriving each, plus model-written notes | Structure is derivable and better computed locally on demand, as Aider, Serena and the code-graph servers do; storing it duplicates git; reviewing a deterministic derivation is pure overhead; and every version of every changed file stays in a journal that never deletes. It needed a batch route, upsert and writing hooks to carry churn with no value once stored. |
| **Immediate review of every write** | It puts a person in front of every capture, which is overhead at volume and noise at low attention; the pull request is when someone already reads the code. |
| **Own tree-sitter grammars** | A grammar per language to build and update, and a larger binary; ctags and language servers already answer "where is this symbol". |
| **An automated reviewer for knowledge** | Its errors correlate with the writer's; R2's value is an independent judgement. |
| **Kind `memory` with an optional anchor** | Code entries would crowd ordinary recall, and every reader would pay for them. |
| **Validate the merged branch workareas at release** | A frozen area admits no supersession; a fact found wrong after a release could never be corrected. |
| **A service key in CI** | A service key is refused every review verb; it could not ratify or reject. |
| **A fourth MCP tool** | ADR-0039 d8: a kind, an anchor field, read filters and hooks carry all of it. |
