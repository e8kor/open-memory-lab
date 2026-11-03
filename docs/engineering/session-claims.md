<!--
id: engineering/session-claims
tier: engineering
status: design draft for review · proposed, not decided
answers: which claim kinds beyond guardrails could start, steer or close an agent's session or turn; which harness hook point each would use and what that hook can do; which are advisory and which can be enforced; how each fits R2 and R3; their risk and value; whether each is a new kind or a subtype of steering, memory or derived_fact; how they rank by value against cost; how a session handoff and the export bridge would work; what a workarea brief and a workarea digest are, how each is marked, scoped and loaded at session start, and how session-start recall would be scoped and queried; how guardrails, steering, the brief, the digest and the corpus layer; and the sources, quoted with fetch dates
-->

# Session claims: what claims could do at a session's start, turns and end

**Status:** design draft for review · proposed, not decided

[ADR-0037](../adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md)
proposes one kind, `guardrail`, and the two hooks that deliver and check it. This page catalogues
the other shapes a reviewed claim could take once a harness hook can put it in front of an agent
at the right moment. Nothing here is decided: every row is a proposal, and a row worth tracking
has a roadmap row (FR-55 to FR-62). The guardrail, the tool rule and the change freeze are
ADR-0037's; they appear here only so the ranking is complete.

## What a shape can be

- **A new kind** gets its own rules for review, delivery and budget, as `guardrail` does
  (ADR-0037 d1). The cost is a value in the `kind` enum and every page that lists kinds.
- **A subtype** would be one value of an optional, author-declared field on an existing kind,
  called `facet` here: inside the signed submission, reviewed with the body, and kept by a
  successor as `kind` is ([ADR-0025](../adr/ADR-0025-the-agent-only-memory-surface.md) d11). No
  record decides such a field. A subtype changes where and when a claim is shown; it changes no
  review rule and no rank, so R3 holds. It must not be an `oml:about` classification of proposed
  ADR-0027: a classifier adds that edge later under another key, which would let a classifier
  change what a claim does without its author's reviewed content.
- **Not a claim**: a delivery or a view over claims, such as the export bridge.

## Hook points by harness

What each harness offers at the moments a claim could be delivered or checked. Sources are in
[the table below](#sources); "—" means none was found in the pages fetched, and a cell marked
*unverified* has no quoted source, so no decision rests on it.

| Moment | Claude Code | Codex | Gemini CLI | Cursor | Copilot CLI |
|---|---|---|---|---|---|
| Session start, context string | SessionStart: `startup`, `resume`, `clear`, `compact`, `fork` | SessionStart, *unverified* | SessionStart, *unverified* | `sessionStart` `additional_context` | `sessionStart` `additionalContext` |
| Helper agent start | SubagentStart, context only | — | — | — | — |
| Each user turn, context string | UserPromptSubmit | UserPromptSubmit, *unverified* | BeforeAgent, this turn only | `beforeSubmitPrompt`, *unverified* | `userPromptSubmitted`, *unverified* |
| Before a tool call, deny | PreToolUse; deny wins | PreToolUse, *unverified* | BeforeTool, *unverified* | shell and MCP hooks, *unverified* | `preToolUse`, *unverified* |
| After a tool call, context string | PostToolUse | — | AfterTool, *unverified* | — | — |
| Turn end, can continue | Stop, SubagentStop | — | — | — | — |
| Compaction | PreCompact (can block), PostCompact (`compact_summary`) | — | PreCompress, *unverified* | — | — |
| Session end | SessionEnd: no decision, 1.5 s default | — | — | — | — |

Cline and Windsurf were found to read rule files only. Whether Claude's apps run any hook is
*unverified*; an MCP server reaches them through tools and user-invoked prompts. Instruction
files are context in every harness, and only a tool-call hook or the harness's own settings can
stop an action.

## The catalogue

Each row is **proposed, not decided**. *Advisory* means text a model reads; *checked* means a
deterministic test in a hook, harness-enforced where installed and bypassable by the user.

| Shape | Purpose | Hook point | Advisory or checked | R2 / R3 fit | Risk | Value | Row |
|---|---|---|---|---|---|---|---|
| **guardrail** (new kind) | standing rules for a subtree, with a severity | SessionStart, compact, SubagentStart | advisory text; its rule checked | ADR-0037 d3, d10 | highest: served first and never cut | high | FR-50 |
| **tool rule** (guardrail's `rule`) | deny or ask on a matching tool call | PreToolUse, from a local cache | checked | promoted only; deny or ask, never allow | a poisoned rule stops work | high | FR-50 |
| **change freeze** (guardrail with valid time) | rules for a stated period | SessionStart, PreToolUse | both | as guardrail | a wrong end date | high for operations | FR-50 |
| **[workarea brief](#the-workarea-brief)** (a ratified brief edge from an existing claim to the workarea) | the few reviewed claims to know on entering: purpose, key decisions, conventions, where things are | SessionStart, compact | advisory, under the data line | promoted claims only; the edge ratified by neither its proposer nor a key in the claim's closure; ordered structurally, never ranked | read first at every start, a poisoned entry too; capped and framed as data | high | FR-60 |
| **[workarea digest](#the-workarea-digest)** (derived_fact subtype `digest`) | one reviewed summary of the whole workarea, naming its sources | SessionStart when no brief entry applies; any recall | advisory, under the data line | reviewed by a key outside every source's closure (R2); loaded structurally | a summary can say what no source says; a small reviewer pool | medium to high, unmeasured | FR-61 |
| **[session-start recall](#session-start-recall)** (not a claim) | the hits the work needs, from the session's own workarea, queried by repository, branch and changed files beside the directory name | SessionStart | advisory | ranked hits, R3 unchanged | branch and file names leave the machine | medium to high, unmeasured | FR-62 |
| **session handoff** (memory subtype `handoff`) | what the last session did and left open | Stop asks; next SessionStart reads | advisory | held, read back by its author (R3) | carries injected tool output forward | high | FR-55 |
| **definition of done** (steering subtype `checklist`) | the checks a turn must meet before it ends | Stop, honouring `stop_hook_active` | advisory, with a nudge | promoted only | loops until Claude Code's 8-continuation cap | high | FR-56 |
| **pitfall / lesson** (memory `pitfall`; derived_fact `lesson`) | what went wrong before, with lineage | PostToolUse after a failure; UserPromptSubmit | advisory | ranked hits, R3 unchanged | error and prompt text leave the machine | medium to high | FR-57 |
| **path convention** (steering subtype, `applies_to` glob) | rules for some files only | PostToolUse after a matching Read or Edit | advisory | promoted only | moderate | medium to high | FR-58 |
| **playbook** (steering subtype, loaded on demand) | a procedure: title at start, body when used | SessionStart titles; `oml_get`; MCP prompt | advisory | promoted only | moderate | medium to high | FR-58 |
| **review rubric** (steering subtype, audience reviewer) | how verdicts are reached here | the review serve; reviewer sessions | advisory to reviewers | never shown to writers, so it cannot be gamed | a biased rubric biases every verdict | high for review | FR-59 |
| **role guide** (steering subtype, audience a member profile) | what one crew member does | SessionStart, SubagentStart | advisory | selection by the key's profile is visibility, not rank | low | medium | — |
| **charter** (steering subtype, one per workarea) | what the workarea is for and not for | SessionStart header | advisory | promoted only | low | medium | — |
| **decision record** (memory subtype `decision`) | a settled choice, linked to what it replaces | SessionStart hits | advisory | ranked hits | low | medium | — |
| **open question** (memory subtype) | work left open | SessionStart lists; Stop asks for new ones | advisory | closing one by retirement takes a non-author verdict (R2) | low | medium | — |
| **environment fact** (memory subtype `env_fact`) | "the tests need `CHROME_PATH`" | SessionStart hits | advisory | ranked hits | secrets and hostnames, caught only by the scan | medium | — |
| **glossary at the prompt** (existing kind `term`) | definitions for the terms a prompt names | UserPromptSubmit | advisory | reviewed vocabulary | the prompt leaves the machine | medium | FR-57 |
| **escalation contact** (memory subtype `contact`) | whom to ask when blocked | SessionStart, Stop when blocked | advisory | ranked hits | personal data: erasure applies | medium | — |
| **compaction summary** (held memory) | the summary Claude Code wrote | PostCompact `compact_summary` | advisory | held for its author | a hook that writes is no longer a read-only hook | low to medium | — |
| **since-last-session changes** (not a claim) | rules and memories promoted, superseded or retired since the key's last session | SessionStart, from the events tail | advisory | only what the key reads | low | medium | — |
| **export bridge** (not a claim) | promoted guardrails as files for harnesses without hooks | none: a CLI verb | advisory | promoted only | a copy outside erasure | high reach | FR-54 |
| **harness deny settings** (not a claim) | rules rendered into Claude Code `permissions.deny` or managed settings | none: an admin act on the machine | checked by the harness, not bypassable by a mod under managed settings | promoted only | writes settings an agent key must not control (R1) | high where managed settings exist | — |
| **server instructions at connect** (not a claim) | guardrails in the MCP `initialize` answer's `instructions` | MCP connect | advisory, if a client uses it | promoted only | unverified that any client puts it in context | medium for Claude's apps | FR-53 |

## Ranking by value against cost

1. **Guardrail, its rule and the freeze** (FR-50): the user's scenario. Cost: a kind, a read, two
   hooks and a scope attribute an admin sets on each key.
2. **Session-start recall** (FR-62): every session gains, at the cost of two recall fields and a
   `git` process in the hook, with no new claim; its quality is unmeasured under FR-4, and branch
   and file names leave the machine ([below](#session-start-recall)).
3. **Workarea brief** (FR-60): the reviewed orientation every session at a workarea needs, at the
   cost of an edge shape, a recall segment and a key attribute ([below](#the-workarea-brief)).
4. **Session handoff** (FR-55): high value at low cost, no new read; its one open question is
   review ([below](#session-handoff)).
5. **Definition of done** (FR-56): high value at low cost through one Stop hook; the risk is a
   loop, capped by the harness.
6. **Review rubric** (FR-59): verdicts that agree with each other, at the cost of an audience
   selector on the review serve.
7. **Path conventions and playbooks** (FR-58): keep the always-on set small; the cost is a
   subtype field and a PostToolUse hook.
8. **Pitfalls and the glossary at the prompt** (FR-57): the first per-turn recall, better than
   ADR-0029 d3's directory-name query; the cost is that prompt and error text leave the machine
   and enter the read log, so it is opt-in, and its quality is unmeasured under FR-4.
9. **Workarea digest** (FR-61): the brief's fallback, at the cost of a crew that writes it and a
   reviewer outside every source's closure; its value is unmeasured ([below](#the-workarea-digest)).
10. **Since-last-session changes**: low cost over the events tail; no row until 1 to 8 exist.
11. **Charter, role guide, decision record, open question, environment fact, contact**: medium
    value each; each needs only a subtype and the existing SessionStart hits, so they follow
    whichever subtype field FR-58 settles. A charter, decision record or environment fact that a
    brief edge joins reaches session start as a brief entry with no subtype.
12. **Compaction summary**: the summary is model text the session already holds; storing it
    from a hook would make a read-only hook a writer ([ADR-0029](../adr/ADR-0029-session-start-hook.md) d1).
    ADR-0029 d6 already asks the model to record the summary's durable facts.

## Session handoff

A handoff is a memory, subtype `handoff`, one per session, under `claim_key`
`handoff/<key name>/<date>-<n>`, written by the model with `oml_remember`, never by a hook:
SessionEnd runs for 1.5 seconds by default with no decision control and no model, and Claude Code
discards PreCompact's `systemMessage`. So the request goes where the model still runs:

- at Stop, when `stop_hook_active` is false, an `additionalContext` line asks the model to record
  what it did, what is open and what to check first, and Claude Code's loop protections bound it;
- at SessionStart with `source: compact`, beside ADR-0029 d6's line on recording the summary.

The next session reads it back as its author: held rows are visible to their author (R3), so no
review is needed for that. Two gaps remain. The own-work listing prints no body (ADR-0029 d6), so
the hook needs the newest handoff's body through a get or an own held recall. And every handoff
is a held row in the review queue, where it expires into the dead-letter under the workarea's
expiry if no one reviews it; whether handoffs are meant for promotion, are left to expire, or
need a class that never enters the queue is FR-55's decision. Inside an in-progress workarea of
proposed ADR-0030, teammates would read handoffs marked held. A handoff carries what the session
read from tool output into later sessions, a persistence path for injected text, so it stays
under the data frame of the memory hook and never under the guardrail frame.

## Workarea context at session start

The maintainer's design of 2026-11-02 for what an agent should know on entering a workarea,
recorded here as proposed, not decided: a [brief](#the-workarea-brief) curated from reviewed
claims, a [digest](#the-workarea-digest) that summarizes the workarea where it has no brief, and a
sharper [session-start recall](#session-start-recall), placed in the [layering](#the-layering)
below. Where the design left a choice, the reading taken is marked *this page's reading*. All
three reach the session through `oml hook session-start` under ADR-0029 d6's data line and keep
[what every hook here keeps](#what-every-hook-here-keeps); none is built.

### The workarea brief

The few reviewed claims an agent should know on entering a workarea: its purpose, its key
decisions, its conventions and where things are (FR-60).

- **Marked by an edge, not a kind.** A brief entry is an existing promoted claim joined to the
  workarea by a ratified edge from the claim to the workarea node, labelled `brief`. The design's
  word for it is one that ADR-0018 d4 keeps off every page as a name for steering, so this page
  calls it a brief edge. The edge is preferred to a kind `brief` because it points at a claim
  already reviewed: a decision recorded once enters the brief with no second copy to supersede
  beside it and drift from it, and the `kind` enum and every page that lists kinds stay as they
  are. A purpose no claim states yet is written as an ordinary memory and then joined. The costs:
  an edge takes a dreamer to propose it and another identity to ratify it, where a kind would take
  one writer, and an entry reads in its source claim's words, not in words written for the brief.
- **Who proposes and ratifies.** The [edge-authority rule](../architecture/workareas.md#edge-authority)
  with one claim endpoint, *this page's reading*: proposed under dreamer(propose) on the workarea,
  the endpoint whose sessions it changes, and reader on the claim's workarea; ratified under
  dreamer(review) on the workarea and reader on the claim's, never by the proposer's key or
  principal and never by a key or principal in the claim's lineage closure, as proposed ADR-0027
  d6 refuses a classification's ratifier. An edge not yet ratified confers nothing.
- **What it joins and binds.** A claim of kind `memory` or `derived_fact`; steering and
  guardrails already load whole, and an edge naming either is refused. The edge binds the claim,
  its `claim_key` in its own workarea, and serves that claim's served tip, so a correction stays
  in the brief and a ratified retirement takes it out, *this page's reading*. That departs from
  the version binding of every claim edge (proposed ADR-0027 d6): the ratifier judged one version
  fit for the brief, and a later version, reviewed under its own workarea's policy but never
  judged for the brief, takes its place. Each entry names the version served and, where they
  differ, the version the edge was ratified on, so the gap shows. Binding the version instead
  would drop an entry from the brief at its first correction until someone joins the new version.
- **What a key sees.** An entry is served only where a recall at the scope node would serve its
  claim: under the key's reader effective access on the claim's workarea, with no reach-down, as
  ADR-0037 d4 keeps for guardrails; served tips only; a `self_signed` claim only at its own
  workarea; a simulation row only to its audience
  ([retrieval](../architecture/retrieval.md#the-recall-ordering-contract-steering-rows-first-unranked)).
  The brief changes where a reviewed claim is shown, never who may read it and never its rank
  among hits (R3).
- **When and how it loads.** At every SessionStart source, compaction included (ADR-0029 d1), as
  a section of `oml hook session-start` between steering and the key's recent submissions, under
  ADR-0029 d6's data line and never under the guardrail frame. It rides the recall the hook
  already sends, as a segment that request asks for (`include_brief`, a sketch), so ADR-0029 d2's
  two requests stay two. Entries are ordered structurally, by the edge's ratification time and
  then `artifact_id`, never by the query.
- **A small budget.** Unlike steering, the brief is cut: a cap on its count and on its share of
  the hook's budget, both design constants of the deciding record, and the entries served are the
  longest prefix of the order that fits, as hits are
  ([token budgets](../architecture/retrieval.md#token-budgets)), with one line naming how many the
  cap left out and that `oml_get` reads them. A claim served in the brief is not served again
  among the hits, *this page's reading*, so it is paid for once.
- **Scoped to the session's own workarea by default, on a separate key attribute.** The brief at
  scope node N is the entries whose edges point at N; a parent's brief describes the parent's
  work, which is noise to a child. The scope does not ride ADR-0037 d4's `guardrail_scope`: that
  attribute defaults to `ancestors` because an ancestor's rule binds below it, and one attribute
  for both would make an admin who loads an organisation's rules load every ancestor's brief too,
  spending a small budget on noise. It is a separate attribute, `brief_scope`, `workarea` by
  default or `ancestors`, set by an admin at issuance, journalled in `key.issued`, changed only by
  rotation and never on the client, as `guardrail_scope` is, at the cost of one more issuance
  field, journal field and `api_keys` column in the same build step. If no key needs an
  ancestor's brief, the cheaper reading holds: no attribute, and the workarea alone.

### The workarea digest

One reviewed `derived_fact` that summarizes the whole workarea, loaded at session start where the
workarea has no brief and read by any agent that asks (FR-61).

- **Written by a crew, never by OML.** A dreamer crew's summarizing agent, under a key with writer
  and reader on the workarea, writes it with [`oml_learn`](../api/mcp-plugin.md#oml_learn), so its
  `derives_from` names the claims it summarizes. OML writes no summary: it extracts nothing,
  synthesizes no profile and answers no question
  ([deliberate boundaries](../product/positioning.md#deliberate-boundaries)), and consolidation is
  the dreamer crews' client-side work. The digest is a `derived_fact` of subtype `digest`, the
  author-declared field of [what a shape can be](#what-a-shape-can-be) that FR-58 settles, with
  one live digest claim per workarea and its versions made by supersession.
- **Its sources are its own workarea's promoted claims,** *this page's reading*: served tips of
  kind `memory` or `derived_fact` in the digest's workarea, not its descendants', and no held or
  simulation row. Every key that reads the digest then reads every source, so the summary carries
  no text past a deny, no held row past its review (R3) and no simulation row past its fence. A
  large workarea can be summarized in layers, as RAPTOR builds its tree: `derived_fact` claims for
  parts of the workarea, each deriving from its claims, and the digest deriving from them, so no
  one submission names every claim. `derives_from` has no bound in the contract
  ([openapi.yaml](../api/openapi.yaml), `SubmitRequest`), and the lineage closure runs over every
  layer.
- **Reviewed under R2, by a small pool.** A verdict is refused when the reviewing key or
  principal is anywhere in the lineage closure
  ([lineage](../architecture/audit-and-lineage.md#the-derivation-graph)), and a digest's closure
  holds every source's author, so only an identity that wrote none of what it summarizes may judge
  it: in a busy workarea, a person or a separate reviewing crew. The reviewer judges the summary
  against its sources, the one check against a summary that says what no source says.
- **Refreshed by supersession, on the crew's schedule.** The crew supersedes the digest after N
  promotions in the workarea, counted from the `lifecycle.promote` events the
  [events tail](../api/http-api.md#events-tail) serves since the digest's own, or on a schedule of
  its own. N and the schedule are the crew's, since OML schedules no crew
  ([crews](../architecture/workareas.md#crews-recorded-not-operated)). Until a refresh is
  promoted the previous digest stays the served tip, so a session reads a reviewed summary that
  may lag by the crew's cadence plus the review's latency, both unmeasured; its line names its
  version and its promotion date.
- **When it loads.** At session start, in the brief's section and within its budget, when no
  brief entry applies at the scope node; when one does, one line names the digest and that
  `oml_get` reads it. It is the session's own workarea's digest, never an ancestor's, whatever
  `brief_scope` says. As a promoted `derived_fact` it is also an ordinary hit of any recall
  ([the layering](#the-layering)).
- **Precedents.** Microsoft Research's GraphRAG answers "global questions directed at an entire
  text corpus" from "community summaries for all groups of closely related entities", generated
  before any question is asked ([Edge et al., arXiv:2404.16130](https://arxiv.org/abs/2404.16130));
  RAPTOR summarizes recursively, "constructing a tree with differing levels of summarization from
  the bottom up" ([Sarthi et al., arXiv:2401.18059](https://arxiv.org/abs/2401.18059)). Both
  build their summaries with a model and use them at query time, neither abstract describes a
  review step, and their gains are measured on their own benchmarks, not on OML. The digest is
  attributed, reviewed and loaded structurally, and its effect is unmeasured (FR-61).

### Session-start recall

ADR-0029's session-start recall, scoped and queried more sharply (FR-62); decided, it would amend
ADR-0029 d2 and d3.

- **The session's own workarea, not its subtree.** A recall's hits come from the scope node's
  subtree ([openapi.yaml](../api/openapi.yaml), `RecallRequest`); at session start they would come
  from the scope node alone, through a recall field the hook sends (`subtree: false`, a sketch),
  which joins the request digest's members (ADR-0025 d9) as every new recall field here does.
  Memories below N are the work of the workareas below it, as a descendant's steering belongs to
  another crew (ADR-0025 d8), and a session at a parent would spend its budget on them. Steering
  stays ancestor-or-self. The cost: a session at a parent whose work lives in its children recalls
  none of that work at start, and `oml_recall` still searches the subtree.
- **Stronger signals beside the directory name.** Beside ADR-0029 d3's words of the `cwd`, the
  query takes the words of the repository's top-level directory name, of the current branch, and
  of the repository-relative paths of recently changed files (those `git status` reports and those
  the last commit changed, at most a design-constant count), each split as d3 splits, the words
  deduplicated. This takes up d3's rejected alternative, the repository name from `git`, and
  answers both its costs: the `git` process runs within the hook's 8-second deadline (ADR-0029
  d5) under a short timeout of its own, and outside a repository, without `git` or past that
  timeout, the `git` words are dropped and d3's query stands alone.
- **A match on any word.** The PoC matches every word of the query (ADR-0029 d3), so more words
  would match fewer memories; the signals need a match on any word, ranked by the store's score
  (a recall field, `match: "any"`, a sketch). Generic words such as `main`, `feat` or `src` then
  match generic memories; how much better the query recalls is unmeasured, under FR-4.
- **What leaves the machine.** Branch and file names reach the server in the request; the read
  log keeps the request's digest, not its text ([read log](../architecture/retrieval.md#the-read-log),
  ADR-0025 d9). A file name can name a customer or an incident. The key value is redacted from
  every word as ADR-0029 d4 redacts it, and `OML_SESSION_CONTEXT=off` still turns the whole hook
  off.
- **The repository's own configuration.** `git` reads it, and `core.fsmonitor` can name a hook
  command that a command refreshing the index, `git status` among them, runs; on Git 2.35.1 and
  earlier the value `false` is itself read as a command's path ([sources](#sources)). The hook
  turns the monitor off for its own `git` calls; the spelling every supported Git reads as off,
  and whether another setting makes those calls run a program, are *unverified*, and the deciding
  record settles both before the build.

### The layering

What reaches a session, from the most binding to the least:

| Layer | What it holds | Frame | Chosen by | Loaded | Budget |
|---|---|---|---|---|---|
| guardrails ([ADR-0037](../adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md)) | rules with a severity | instructions, in their own section | structure: kind, applicability, guardrail scope | every session start, compaction and subagent | never cut; a cap and no partial set |
| steering (ADR-0025 d8) | standing instructions | instructions by definition (ADR-0018 d4), though ADR-0029 d6 prints them under its data line, which this page leaves as it is | structure: kind, the scope node and its ancestors | every recall, the session-start recall included | never cut |
| brief (FR-60) | existing claims a ratified brief edge joins | data | curation: the edge's proposer and ratifier | every session start | a small cap, cut as a prefix |
| digest (FR-61) | one summary naming its sources | data | summary: a crew writes it, a key outside its closure reviews it | session start where no brief entry applies; any recall | the brief's |
| corpus | every promoted `memory` and `derived_fact` the key reads | data, ranked | the query | on demand by `oml_recall`; at session start, ADR-0029's hits, sharpened by FR-62 | `k` and `max_tokens` |

Instructions are few and structural; data is curated, summarized or ranked, and only the corpus
is ranked, by the query alone. No layer moves a claim's rank, and each layer above the corpus
changes only where a promoted claim is shown, so R3 holds layer by layer. No "RAG claim" kind is
added: every promoted `memory` and `derived_fact` is already a recall hit (ADR-0025 d8), so such a
kind would mark nothing recall does not already serve, and kind would start to decide what is
retrievable, which state and access decide.

## The export bridge

Proposed in ADR-0037 d11: `oml guardrails export --format agents-md | claude-md` writes the
promoted guardrails its key loads under the guardrail scope an admin set on that key, with a
header naming the server, scope node, guardrail scope and set digest, and `--check` reports
drift. It reaches every harness that reads AGENTS.md or a `.claude/rules/`
file, which is all of those above but Gemini CLI. Its costs, stated there: a copy no erasure
or demotion reaches (FR-54), readers widened from OML's grants to the repository's, and
staleness until the next export. Conventions with an
`applies_to` glob (FR-58) would map to Claude Code `paths`; other harnesses' path-scoped rule
fields are *unverified*.

## What every hook here keeps

ADR-0029's discipline holds for each: a Rust subcommand of `oml`, a budget, the key value
redacted, the agent's own key alone, one stated failure mode, and no act performed by the hook.
Every hook that only adds context keeps ADR-0029 d7's whole guard, `command -v oml` before the
verb and `|| exit 0` after it. The tool-call check keeps the first half and replaces the second
with a probe for its verb (ADR-0037 d9), since a failure of the check must block. Only a
tool-call check may block, and it may only deny or ask.

## Sources

All fetched 2026-10-31 but the GraphRAG, RAPTOR and git-config rows, fetched 2026-11-03. Those
three and the Claude Code memory and permissions quotes were checked against saved full pages;
every other quote came through a fetch tool's extraction step and is to be rechecked against its
page before ADR-0037 is accepted. A row marked *unverified* states what was not
found, and no decision rests on it.

| Claim | Source | Quote |
|---|---|---|
| Instruction files are context, not enforcement | [Claude Code memory](https://code.claude.com/docs/en/memory) | "Claude treats them as context, not enforced configuration. To block an action regardless of what Claude decides, use a PreToolUse hook instead." |
| Files are concatenated root to working directory | [Claude Code memory](https://code.claude.com/docs/en/memory) | "All discovered files are concatenated into context rather than overriding each other." |
| Conflicts are left to the model | [Claude Code memory](https://code.claude.com/docs/en/memory) | "if two instructions contradict each other, Claude may pick one arbitrarily." |
| The managed file cannot be excluded | [Claude Code memory](https://code.claude.com/docs/en/memory) | "Developers can exclude other CLAUDE.md files with the `claudeMdExcludes` setting, but not this one." |
| Only the root file survives compaction | [Claude Code memory](https://code.claude.com/docs/en/memory) | "Project-root CLAUDE.md survives compaction: after `/compact`, Claude re-reads it from disk and re-injects it into the session." |
| Rules without paths load unconditionally | [Claude Code memory](https://code.claude.com/docs/en/memory) | "Rules without a paths field are loaded unconditionally and apply to all files." |
| Permission rules are the harness's | [Claude Code permissions](https://code.claude.com/docs/en/permissions) | "Permission rules are enforced by Claude Code, not by the model." |
| An allow cannot undo a deny | [Claude Code permissions](https://code.claude.com/docs/en/permissions) | "An allow rule can't carve an exception out of a deny rule." |
| A mod can bypass a hook's block | [Claude Code permissions](https://code.claude.com/docs/en/permissions) | "With neither managed settings nor a Team or Enterprise `/login` as the session's credential, the mod can approve a call that a deny rule refuses or a `PreToolUse` hook blocked." |
| A deny rule is a string match | [Claude Code permissions](https://code.claude.com/docs/en/permissions) | "A deny rule doesn't match the same program by path or inside `sh -c`" |
| SessionStart sources and context | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "\"startup\" for new sessions, \"resume\" for resumed sessions, \"clear\" after /clear, \"compact\" after compaction, or \"fork\"" |
| The 10,000-character cap | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "A hook's additionalContext, systemMessage, and initialUserMessage strings, and its plain stdout, are capped at 10,000 characters." |
| Over the cap, a preview | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "passes Claude the file path with a preview of up to the first 2,000 characters instead." |
| Hooks run in parallel, contexts all delivered | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "When an event fires, its matching hooks run in parallel." / "When several hooks return `additionalContext` for the same event, Claude receives all of the values." |
| Deny wins across hooks | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "When multiple PreToolUse hooks return different decisions, precedence is deny > defer > ask > allow." |
| A failing hook can block | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "To block the action instead, set `\"onFailure\": \"block\"` on a `command` or `http` hook. Requires Claude Code v2.1.295 or later." |
| systemMessage reaches the user | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "To surface a message to the user on any platform, return `systemMessage` in JSON output." |
| Every hook gets the session id | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "`session_id`: Current session identifier" |
| A matcher with other characters is a regular expression | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "Contains any other character \| JavaScript regular expression, unanchored" |
| Every hook gets the working directory | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "`cwd` \| Current working directory when the hook is invoked" |
| Exit 2 from PreToolUse blocks | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "a `PreToolUse` hook blocks the tool call" |
| PreToolUse tool inputs | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | the `tool_input` tables: `Bash` `command`; `Write`, `Edit` and `Read` `file_path`; `Glob` and `Grep` `path`; `WebFetch` `url`; and a `PowerShell` tool with `command` |
| `NotebookEdit` is an edit tool | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "`Edit.*` matches both `Edit` and `NotebookEdit`"; *unverified*: its `tool_input` fields, which the page does not list |
| PostToolUse adds context | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "String added to Claude's context alongside the tool result." (its `additionalContext` row) |
| PreCompact can block and cannot speak to the user; SessionEnd decides nothing | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "Blocks compaction" (the exit-code row naming PreCompact) / "Claude Code discards a PreCompact hook's `systemMessage` and `continue` fields." / "No decision control. Used for side effects like logging or cleanup." (the row naming SessionEnd) |
| SubagentStart injects context | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "SubagentStart hooks can't block subagent creation, but they can inject context into the subagent." |
| UserPromptSubmit adds, never rewrites | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "UserPromptSubmit: can't replace the prompt; it only injects additionalContext alongside it." |
| Stop can continue the turn, with a cap | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "the `stop_hook_active` input and the 8-consecutive-continuation cap" |
| SessionEnd cannot carry a handoff | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "SessionEnd hooks have a default timeout of 1.5 seconds." |
| PostCompact receives the summary | [Claude Code hooks](https://code.claude.com/docs/en/hooks) | "The compact_summary field contains the conversation summary generated by the compact operation." |
| Skills load bodies on demand | [Claude Code skills](https://code.claude.com/docs/en/skills) | "full skill content only loads when invoked." |
| MCP prompts are user-controlled | [MCP prompts, 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/server/prompts) | "Prompts are designed to be user-controlled" |
| A server may send instructions at connect | [MCP lifecycle, 2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle) | `"instructions": "Optional instructions for the client"` (the example initialize answer); *unverified*: whether any client puts it in the model's context |
| Codex walks root to working directory | [Codex AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md) (redirected from developers.openai.com) | "Files closer to your current directory override earlier guidance because they appear later in the combined prompt." |
| Codex loads hooks from every source | [Codex hooks](https://learn.chatgpt.com/docs/hooks) (redirected) | "If more than one hook source exists, Codex loads all matching hooks." |
| AGENTS.md: the nearest file wins | [agents.md](https://agents.md/) | "The closest AGENTS.md to the edited file wins; explicit user chat prompts override everything." |
| Cursor rules go first; team rules can be required | [Cursor rules](https://cursor.com/docs/context/rules) | "When applied, rule contents are included at the start of the model context." / "required for all team members and cannot be disabled in Customize" |
| Cursor sessionStart adds context | [Cursor hooks](https://cursor.com/docs/agent/hooks) | "additional_context: Additional context to add to the conversation's initial system context" |
| Copilot: nearest AGENTS.md | [Copilot repository instructions](https://docs.github.com/en/copilot/how-tos/configure-custom-instructions/add-repository-instructions) | "When Copilot is working, the nearest AGENTS.md file in the directory tree will take precedence." |
| Copilot CLI sessionStart adds context | [Copilot CLI hooks](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-hooks-reference) | "Optional — can inject additionalContext into the session." |
| Gemini CLI BeforeAgent is per turn | [Gemini CLI hooks](https://geminicli.com/docs/hooks/reference/) | "Text that is appended to the prompt for this turn only." |
| Cline reads AGENTS.md; workspace wins | [Cline rules](https://docs.cline.bot/features/cline-rules) | "Workspace rules take precedence when they conflict with global rules." |
| Windsurf system rules merge | [Windsurf memories (zh)](https://docs.windsurf.com/zh/windsurf/cascade/memories.md) | "系统级规则会与工作区规则和全局规则合并" (system-level rules merge with workspace and global rules) |
| RFC 2119 severity | [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119) | "mean that the definition is an absolute requirement of the specification." |
| Indirect injection through a repository | [OWASP LLM01:2025](https://genai.owasp.org/llmrisk/llm01-prompt-injection/) | "An attacker modifies a document in a repository used by a Retrieval-Augmented Generation (RAG) application." |
| Trusted sources only | [OWASP LLM08:2025](https://genai.owasp.org/llmrisk/llm082025-vector-and-embedding-weaknesses/) | "Accept data only from trusted and verified sources." |
| Models rank all text alike | [Wallace et al., arXiv:2404.13208](https://arxiv.org/abs/2404.13208) | "LLMs often consider system prompts (e.g., text from an application developer) to be the same priority as text from untrusted users and third parties." |
| Rule of Two | [Meta, practical AI agent security](https://ai.meta.com/blog/practical-ai-agent-security/) | "agents must satisfy no more than two of the following three properties within a session" |
| Notes outside the context window | [Anthropic, effective context engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) | "the agent regularly writes notes persisted to memory outside of the context window." |
| Global questions need summaries of the whole corpus | [Edge et al., arXiv:2404.16130](https://arxiv.org/abs/2404.16130) | "RAG fails on global questions directed at an entire text corpus, such as \"What are the main themes in the dataset?\"" / "Our approach uses an LLM to build a graph index in two stages" / "then to pregenerate community summaries for all groups of closely related entities." / "Given a question, each community summary is used to generate a partial response" |
| GraphRAG is Microsoft Research's | [GraphRAG documentation](https://microsoft.github.io/graphrag/) | "Microsoft Research’s new approach, GraphRAG, creates a knowledge graph based on an input corpus." |
| Recursive summaries form a tree | [Sarthi et al., arXiv:2401.18059](https://arxiv.org/abs/2401.18059) | "We introduce the novel approach of recursively embedding, clustering, and summarizing chunks of text, constructing a tree with differing levels of summarization from the bottom up." / "At inference time, our RAPTOR model retrieves from this tree" |
| A repository's configuration can name a command `git status` runs | [git-config](https://git-scm.com/docs/git-config) | "the built-in file system monitor can speed up Git commands that need to refresh the Git index (e.g. git status)" / "Otherwise, this variable contains the pathname of the \"fsmonitor\" hook command." (`core.fsmonitor`) |
| Older Git reads `false` as a path | [git-config](https://git-scm.com/docs/git-config) | "Git versions 2.35.1 and prior will not understand the boolean values and will consider the \"true\" or \"false\" values as hook pathnames to be invoked." |
| *Unverified*: Cline's rule placement | [Cline rules](https://docs.cline.bot/features/cline-rules) | the page does not say whether rules enter the system prompt |
| *Unverified*: Cursor nested rule directories and `.mdc` fields | [Cursor rules](https://cursor.com/docs/context/rules) | the page states nested AGENTS.md only |
| *Unverified*: Gemini CLI hook merging | [Gemini CLI configuration](https://geminicli.com/docs/reference/configuration/) | not stated in the portion fetched |
| *Unverified*: Copilot prompt-type sessionStart limits | Copilot CLI hooks reference, Korean mirror | a search summary only, not fetched |
| *Unverified*: managed settings forcing a plugin's hooks on | [Claude Code settings](https://code.claude.com/docs/en/settings) | not fetched |
