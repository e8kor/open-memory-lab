<!--
id: adr/ADR-0063-a-chat-turn-becomes-memory-only-when-the-agent-in-it-writes-the-fact-and-every-memory-says-what-kind-of-source-it-came-from
tier: engineering
status: accepted design · 2026-11-06
answers: what origin.kind is, which values it takes, who declares or derives it, how it is signed, filtered and shown; the chat anchor's grammar and why its session id is pseudonymous; why a turn's text is not stored and when an excerpt is; how the plugin's hooks ask the running agent to extract facts, with which budgets and loop guards, and why the server calls no model; where a person's chat memory lives and who may create it; how the ADR-0030 curator keeps it clean; the opt-in per-turn recall hook, its budgets and labels; how chat memory reaches a team; the end-to-end QA benchmark and its retrieval-only proxy; privacy, telemetry, doctor lines; the rejected alternatives and the open questions
-->

# ADR-0063 — A chat turn becomes memory only when the agent in it writes the fact, and every memory says what kind of source it came from

**Status:** accepted design · 2026-11-06 (was proposed) · introduced in v0.6.0 · first recorded 2026-11-05

**Release:** introduced in v0.6.0 ([releases](../changelog.md))

**Amends:** ADR-0057 (d1, d3, d4) — the anchor's `uri` admits a `chat` scheme with its own
grammar, `hasher` gains `oml-turn/1`, and the origin filters gain `origin_kinds`. ADR-0038
(rejected alternative C) — per-prompt recall is built, opt-in, under d10's budgets.
ADR-0039 (d4, d8) — `oml_write` gains `origin` and `oml_read` gains `origin_kinds`; the
catalog stays three tools.

## Context

**What the operator decided on 2026-11-05.** v0.6.0 adds chat memory, the use case mem0 and
Letta sell: what a person says to an agent in one session is there in the next. It keeps OML's
gate. The decision fixed six things, recorded here as given: extraction is done by **the agent
itself**, the model already running in the harness, never by the OML server and never by a
separate model; every memory gains an `origin.kind` and chat memories a chat anchor; capture
runs through plugin hooks; curation is ADR-0030's curator role; recall on every turn is opt-in;
and an end-to-end QA benchmark measures the path.

**How the competition does it.** mem0 makes one model call per write with an extraction prompt
whose "sole operation is ADD" (mem0 `configs/prompts.py:468-946`, read 2026-11-05 in the
competitor study). Its Claude Code plugin exposes one read-only tool, `search_memories`, writes
through hooks, and leaves forgetting to the user. OML's plugin already has that shape (ADR-0038,
ADR-0039 d8): the agent reads with `oml_read` and writes held claims with `oml_write`, and it
can never promote. What OML lacks is the capture moment and a place for a person's chat facts.

**What the harness offers.** Claude Code's hooks reference (code.claude.com/docs/en/hooks,
fetched 2026-11-05):

- `Stop`: "`decision` | `"block"` prevents Claude from stopping"; "`reason` | Required when
  `decision` is `"block"`. Tells Claude why it should continue". Its input carries
  `stop_hook_active`, "`true` when Claude Code is already continuing as a result of a stop
  hook", and `last_assistant_message`; continuation runs under "the `stop_hook_active` input and
  the 8-consecutive-continuation cap".
- `PreCompact`: receives `trigger` and `custom_instructions`; it can block compaction, and
  "Claude Code discards a PreCompact hook's `systemMessage` and `continue` fields". It has no way
  to put text in front of the model, so it cannot ask the agent to capture.
- `UserPromptSubmit`: receives `prompt`; `additionalContext` is "String added to Claude's
  context alongside the submitted prompt"; the default command timeout is lowered "to 30 on
  `UserPromptSubmit`".
- `SessionEnd` cannot block and runs under a 1.5-second shared budget (ADR-0038, context); the
  session is ending, so it cannot ask the agent anything either.

So the agent can be asked to capture at `Stop` only. `PreCompact` can record that a compaction
happened; `SessionEnd` can only clean up.

**Why per-turn recall is reconsidered now.** ADR-0038 deferred recall on every prompt "while
FTS-only": AND matching of a whole prompt matched almost nothing and was unranked. Since then
recall ranks by bm25 from integer postings, 29.9 ms p95 between 10,000 and 165,000 memories
([ADR-0052](ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md),
`bench/scale/REPORT.md`), and `match: "any"` exists. Irrelevant injected text still costs
accuracy (Shi et al., arXiv 2302.00093, quoted in ADR-0038), and no labelled per-prompt set
exists. That keeps it opt-in.

**Held against.** R1–R4 ([ADR-0001](ADR-0001-the-constitution.md)): no agent key promotes; the
reviewer is never the author; nothing is readable by others until reviewed, but for ADR-0030's
in-progress relaxation; nothing is deleted. The Occam rule (ADR-0039 d8): every call traces to
a use case, and a capability is a parameter before it is a tool.

## Decision

- **d1 — Every memory version carries `origin.kind`, one of `repo`, `doc`, `code`, `lake`,
  `chat`, `web` or `manual`.** It says what kind of source the text was taken from, beside the
  anchor that says which one. `repo`: a repository's metadata (a commit, a pull request, an
  issue), not a file's text; `doc`: a document's prose; `code`: source code; `lake`: a lake
  table's rows or schema; `chat`: a conversation with a person; `web`: a page fetched over
  https; `manual`: written by a person or an agent from no source it names. The set is closed;
  any other value is refused with `request.validation` naming `/origin/kind`, and `origin` is a
  closed object, so an unknown member is refused naming it (ADR-0025 d16). The cost: a new kind
  is a new decision and a migration of the check constraint.

- **d2 — The kind is signed, immutable per version, and declared or derived, never inferred by
  the server.** It is journalled by value as `origin` in `submission.remember` and
  `submission.learn` beside `anchor`, so the chain signs it, and the version row stores it in
  `memories.origin_kind`. A supersession declares its own, inheriting nothing, as the anchor
  does (ADR-0057 d2). An agent declares it on `oml_write` (`origin: {kind}`); a write that sends
  none is recorded `manual`, and that default is journalled, so the signed event says what the
  row says. `oml sync` derives it from the adapter: `adr`, `headings` and `lines` give `doc`;
  the planned `rust` adapter (ADR-0061) gives `code` and the Delta mapping (ADR-0062) `lake`.
  The server checks two compositions with the anchor and refuses each with `request.validation`
  naming `/origin/kind`: a chat anchor (d4) requires `chat`, and `chat` requires a chat anchor,
  so every chat memory names its turn. Versions written before the migration carry no kind and
  are served `origin: null`; no row is backfilled, because the kind would not be in their signed
  event.

- **d3 — Recall and the listing filter by kind, and every claim item shows it.** `POST
  /v1/memories/recall`, `GET /v1/memories` and `oml_read` take `origin_kinds`, a list of one to
  seven kinds, a row passing when its kind is one of them; it combines by AND with the anchor
  filters and every other filter, and leaves out rows without a kind. It is one more of the
  origin filters of ADR-0057 d4, applied inside the candidate statement before ranking's cut, by
  the same predicate machinery, so a filtered recall never under-returns. Every claim item
  carries `origin: {kind}` or `null`. Ranking reads no kind (R3 as ADR-0001 d3 states it):
  the kind narrows, it never reorders.

- **d4 — A chat anchor is `{uri: "chat://<harness>/<session>", fragment: "turn=<n>", hash,
  hasher: "oml-turn/1"}`.** It is ADR-0057's `uri` form with a second scheme. `<harness>` is 1 to
  32 characters of `[a-z0-9-]`, starting with a letter (`claude-code`); `<session>` is exactly 32
  lower-case hex digits; nothing follows it, and no user, port, query or `rev` is admitted. The
  fragment is required with `chat://` and is `turn=<n>` or `turn=<n>-<m>`, `1 <= n <= m <=
  100000`, the 1-based number of the person's prompt in the session; it is refused elsewhere.
  `hash` and `hasher` are required with `chat://`: `oml-turn/1` is the HMAC-SHA-256 of the
  prompt's text as the harness passed it to `UserPromptSubmit`, UTF-8, after NFC, keyed by the
  person's chat salt (d5). Whoever holds the transcript and the salt can recognise the turn
  again; whoever holds only the signed anchor, the operator and every reviewer included, cannot
  confirm or search a guessed prompt against it, and erasure leaving the anchor in place (ADR-0057
  d2) leaves no unkeyed digest of what the person said. *Amended 2026-11-05 from the PR #78
  review (H2): an unkeyed SHA-256 of a short prompt is a guessable fingerprint that outlives
  erasure.* `oml-turn/1` is refused
  with any other anchor. The scan of ADR-0057 d2 runs over the `uri` and the fragment as over any
  anchor. The cost: a turn's hash proves nothing to the server, which never sees the turn, as no
  anchor is verified (ADR-0057 d2).

- **d5 — The session in a chat anchor is pseudonymous, and the transcript is not stored.**
  The plugin derives `<session>` as the first 32 hex digits of SHA-256(salt ‖ the harness's
  session id), where the salt is 32 random bytes the CLI writes once to
  `oml/chat-salt` under `XDG_CONFIG_HOME` (or `HOME/.config`), 0600 in a 0700 directory. The salt
  is the person's secret: no hook, request or anchor carries it, and it keys both the pseudonym
  and every turn hash (d4). The server cannot link a
  chat anchor to a harness transcript file, and two machines of one person produce different
  sessions. Because Claude Code's ids are UUIDs, the grammar's 32 hex digits without hyphens
  refuses a raw id sent by mistake. No turn text is sent by any hook (d6) and no transcript is
  stored: a chat memory's text is the fact the agent wrote. With `OML_CHAT_EXCERPT=1` the
  capture skill may add one quoted excerpt of at most 280 characters of what the person said
  under the fact, prefixed `> `. It is part of the body, so the server's deterministic scanner
  runs over it as over every body, and a finding refuses the write with `submission.scan_veto`;
  crypto-shredding erases it with the body (ADR-0005). What the scanner finds is its rule set's
  to say: `oml-scan/1` (ADR-0065) vetoes the secret shapes it declares. So nothing here promises that a secret is refused: the skill tells the agent
  never to quote one, and the excerpt test runs a case for each rule family of `oml-scan/1`,
  through a chat excerpt. Off by default because an excerpt is the person's words, kept.

- **d6 — Capture is asked of the running agent at `Stop`, under budgets, and the server calls no
  model.** Three hook verbs, each a no-op unless `OML_CHAT_CAPTURE=1` (or `OML_TURN_RECALL=1` for
  the first) and a chat workspace is named (`OML_CHAT_WORKSPACE`, or the plugin option
  `chat_workspace`):

    | Event | Verb | What it does |
    |---|---|---|
    | `UserPromptSubmit` | `oml hook user-prompt` | Counts the turn and stores its `oml-turn/1` hash in the session cache (ADR-0038 d9's file, ids and digests only); then d10 when that is on. Sends no prompt text unless d10 is on. |
    | `Stop` | `oml hook stop` | When a capture is due, prints `{"decision": "block", "reason": …}`: the instruction to run the `oml:oml-chat-capture` skill over turns `a..b`, naming each turn's anchor, the workspace, the idempotency key prefix and the facts left in the budget. Otherwise prints nothing. |
    | `PreCompact` | `oml hook pre-compact` | Records that the session compacted, so the next `Stop` asks at once; prints nothing and never blocks. |

    The three verbs are one dispatcher, `oml hook <event>`: it reads the event's JSON once, runs
    every handler registered for that event in order, and merges their answers (the first block
    decision wins; contexts are joined under one `additionalContext`). Any harness with command
    hooks calls the same verbs with the same JSON, and the plugin's `hooks.json` holds no logic.
    The chat handlers are the first rows; repository detection at session start, filling an
    anchor before `oml_write`, the paths an edit touched, and a stale-claim check at `Stop` are
    later rows of the same table, not new verbs (operator's direction, 2026-11-05, for v0.6.1).

    A capture is due when at least `OML_CHAT_CAPTURE_EVERY` (default 5) turns have ended since
    the last one, or a compaction was recorded and at least one turn is uncaptured. The loop
    guards: nothing when `stop_hook_active` is true, so a capture never triggers another; nothing
    when no turn is uncaptured; at most 4 capture requests per session; at most 6 facts per
    request and 24 per session, stated in the reason and counted in the hook's state, so no
    request asks past the session's budget (the agent is asked for at most that many; the
    server does not count chat facts); the cache records the last captured turn before the
    reason is printed, so a capture the agent skips is not asked again. A resumed session
    numbers on: `oml hook session-end` deletes the turn digests but keeps the session's turn
    count, one number in `<id>.chat-turns`, pruned after 30 idle days, so a turn's anchor and
    idempotency key never repeat when the harness resumes a session under the same id. Each fact is
    one `oml_write` with `idempotency_key` `chat-<session>-t<n>-<i>`, so a repeated capture
    replays instead of writing twice. The agent extracts with its own model, in its own context,
    which already holds the turns: no turn text leaves the machine except as the facts the agent
    writes, and the server's work per fact is one ordinary write. The cost: capture spends the
    session's own tokens, at most four short continuations per session, and a session that ends
    between captures loses its last turns, since neither `SessionEnd` nor `PreCompact` can ask
    the agent.

- **d7 — The `oml-chat-capture` skill says what a fact is.** One claim per fact, in the third
  person, about the person or the work: a preference, a decision, a stated constraint, a
  correction of something earlier, a fact about the person's project or environment that holds
  past this session. Not: a summary of the session, the agent's own reasoning, instructions to
  future agents, secrets, credentials, or facts about other people. `claim_key` is
  `chat/<topic-slug>`, so a changed preference supersedes the earlier one under the same key
  with `expected_version` (ADR-0057 d19). `origin: {kind: "chat"}` and the turn's anchor from the
  reason; `kind: memory`. Zero facts is a valid answer, and the skill says so first.

- **d8 — A person's chat memory lives in `chat` under their user root, created only by a key a
  person holds.** The chat workspace is the child named `chat` of a user root (ADR-0044), so
  its path is `universe.<user>.chat`. The name `chat` is reserved in every user root's namespace
  for a key `used_by: person`, at any depth: creating a workspace named `chat` anywhere under a user
  root with any other key, an agent's writer key in that root included, is refused with
  `access.verb_ceiling`, `details.reason: chat_scope_is_human_only`, before anything is
  journalled, so an agent cannot take the name first (the namespace is one per root, ADR-0044 d5).
  `oml chat init` creates it with the CLI's stored credentials; when the name is taken it adopts
  only `universe.<user>.chat` and otherwise refuses with `chat_scope_not_the_persons`; it prints the `OML_CHAT_WORKSPACE` value, and, once
  the server serves ADR-0030's mode, sets it `in_progress`, so the person's other agent keys
  read each other's chat facts labelled unreviewed (ADR-0030 d3, d4). Until then an agent reads
  its own held chat facts, as every key reads its own (ADR-0025 d4). No MCP tool or argument
  creates it. The cost, as for bulk review (ADR-0055 d1): `used_by` is the issuer's label, so an
  agent with a shell and the person's stored CLI credentials can run `oml chat init` itself.

- **d9 — Curation inside the chat workspace is ADR-0030's curator, narrow only.** A curator key
  on the chat workspace rejects the newer of two near-duplicates found by ADR-0050's MinHash
  check (lane `feat/remember-dedup`), flags two facts that disagree with a `contradicts` edge,
  and proposes the retirement of a preference a later fact replaced; it never promotes
  (ADR-0030 d8, d9). The reference curator peer stays outside the server with its own key,
  cursor and budget (ADR-0030 d10). This record builds none of it: it names the chat workspace as
  that peer's first scope.

- **d10 — Recall on every turn is opt-in: `OML_TURN_RECALL=1` injects at most six hits, within a
  token and a latency budget, each labelled.** `oml hook user-prompt` takes up to eight
  distinctive words of the prompt by ADR-0038 d5's filter, every secret shape removed first, and
  sends two recalls with `match: "any"` and `include_text`: the chat workspace with `k` 3 and
  `include_unreviewed: true`, and the key's default workspace with `k` 3, promoted only. Each asks
  `max_tokens` 150; the injected context is at most 1,600 bytes, each hit at most 400. The
  latency budget is `OML_TURN_RECALL_BUDGET_MS`, default 300 and at most 1,000: one deadline
  for both requests, and when it passes nothing is injected and the turn goes on. Three misses
  of the deadline in a session stop recall for that session; a miss is counted before a request is
  sent and taken back by an answer in time, so a refused connection or a hook ended by its
  deadline counts too. Outside the opted-in chat workspace, per-turn recall reads promoted rows
  only, a law its tests hold by the request bodies. A hit is a line naming its
  artifact id, its `origin.kind`, its workspace and its time, with `[unreviewed]` when its state
  is held, under ADR-0029 d6's framing line and the line `Recalled from OML for this prompt; it
  may be irrelevant — ignore it if so.`; an artifact already injected in the session is not
  repeated. Off by default because the prompt's words leave the machine on every turn, and
  because no labelled set yet shows that injected hits help (#111).

- **d11 — Chat memory reaches a team only by a person's review.** A chat fact is held when it is
  written (R1). It becomes readable outside the chat workspace only when a person approves it
  through bulk review (ADR-0055) and, in an in-progress chat workspace, commits it (ADR-0030 d6),
  or writes it again in a team workspace. No hook, skill, curator or agent promotes, and no
  setting turns that on.

- **d12 — An end-to-end benchmark runs LongMemEval (S) and LoCoMo through this path.** The
  harness is `eval/bench/chat.py`, beside `bench.py`, and writes `oml-bench/1` results. Each
  dataset's sessions are ingested as chat writes: one claim per turn, `origin.kind` `chat`, a
  chat anchor whose session is the pseudonym of the dataset's session id and whose turn is the
  turn's number, by an agent key into a workspace per question group; the rows stay held and are
  recalled by their author with `include_unreviewed: true`, as d8's chat workspace serves them. It
  then recalls with each question and measures two things:

    1. **Retrieval proxy, no model:** recall@k for k in 5, 10 and 50 of the evidence turns
       (LongMemEval's turns marked `has_answer`, LoCoMo's `evidence` dialogue ids), and the
       session-level recall@k against LongMemEval's `answer_session_ids`.
    2. **QA, with a model:** the top-k hits and the question go to one fixed answering model,
       and a judge model grades the answer against the gold with LongMemEval's published judge
       prompt; LoCoMo is graded the same way. Both model ids are required arguments, recorded
       in the result, never defaulted, so two runs name what they compared. It needs an API key;
       without one it stops after the proxy and says so.

    Turn-level ingestion is the floor the agent's own extraction must beat. Replaying the
    capture skill over a dataset needs a model run per session and is not built here; #111
    tracks the comparison. Results are written in `oml-bench/1` and enter the bench pages only
    when `scripts/gen_bench_pages.py` is taught their suites.

- **d13 — Privacy, telemetry and doctor lines.** Opt-in switches, each off by default:
  `OML_CHAT_CAPTURE`, `OML_CHAT_EXCERPT`, `OML_TURN_RECALL`. The session cache keeps turn numbers
  and turn hashes, never a prompt; `oml hook session-end` deletes it (ADR-0038 d9). Recall from
  the per-turn hook is recorded in `recall_events` like any recall, and its query text only
  when the tenant opted in (ADR-0054 d3). One metric family is added:
  `oml_recall_hits_by_origin_total{origin_kind}`, eight closed values (the seven kinds and
  `none`), no workspace or key label (ADR-0054 d1's series budget). `oml doctor` names: chat
  capture or turn recall switched on with no chat workspace named; a named chat workspace that does
  not resolve, or that is not `universe.<user>.chat`; a server that does not accept
  `origin_kinds` (before v0.6.0), by one listing filtered by it; a chat salt file that is not
  0600; a switch set to a value it does not take; and per-turn recall that missed its budget in
  an open session.

- **d14 — Each new call traces to a use case, and no MCP tool is added.**

    | Call | Use case |
    |---|---|
    | `origin` on `oml_write`, `POST /v1/memories`, `…/supersede`; `oml write --origin-kind` | UC-CH-01, say what kind of source a memory came from |
    | `origin_kinds` on recall, the listing and `oml_read`; `oml read --origin-kind` | UC-CH-02, find only what a person said, or only what documents say |
    | the chat anchor; `oml hook user-prompt`, `stop`, `pre-compact`; the capture skill | UC-CH-03, remember what a person said in a session |
    | `oml chat init` | UC-CH-04, open a person's chat memory |
    | `OML_TURN_RECALL` | UC-CH-05, be reminded on each prompt |

- **d15 — The build, each step closed by named tests, written red first.** `origin.kind`
  signed, immutable and filterable (`an_origin_kind_is_journalled_in_the_submission_and_served_on_the_item`,
  `a_supersession_inherits_no_origin_kind`, `recall_filtered_by_origin_kind_leaves_out_other_kinds_and_rows_without_one`);
  the chat anchor's grammar and the composition rules (`oml-domain` anchor tests); an excerpt
  carrying a secret refused by the scanner; the hooks idempotent and bounded, by golden outputs
  for each verb; per-turn recall within its budget and skipped on timeout; an agent key refused
  creating the chat workspace and a person's key admitted.

## Consequences

- `docs/api/openapi.yaml` (`info.version` `0.6.0-design`), `docs/api/mcp-tools.json`, the
  generated SDKs, the [HTTP API](../api/http-api.md), the [CLI reference](../cli/reference.md),
  the [event catalog](../data/event-catalog.md) (`submission.remember` and `submission.learn`
  carry `origin`) and the [Postgres schema](https://github.com/e8kor/open-memory-lab/tree/main/migrations) (`memories.origin_kind`,
  migrations 0029 and 0030, numbered after the other lanes' 0026 to 0028; 0030 validates and indexes outside any transaction) change in the same change as the code. The error enum is unchanged:
  `request.validation`, `submission.scan_veto` and `access.verb_ceiling` carry the new refusals.
- The plugin gains three hook entries and one skill; `scripts/check_plugin.py` holds them. The
  plugin README names the three switches.
- [open issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap): #111, whether the agent's extraction beats turn-level
  ingestion on QA accuracy; #139, the last turns of a session that ends between captures are
  lost; #111, a labelled set for per-turn recall's relevance, the condition for turning it on
  by default; #139, a person's chat facts on two machines are two pseudonymous sessions that
  cannot be joined; #111, the benchmark's QA numbers wait for an API key in the environment
  that runs it.
- Costs carried openly: capture spends the session's tokens; an opted-in per-turn recall sends
  prompt words to the server on every turn; a kind is the author's statement and is not
  verified; rows written before migration 0029 have no kind and drop out of every kind filter.

## Rejected alternatives

| Alternative | Why not | Revisit when |
|---|---|---|
| **Extraction by a model the server calls** (mem0's one call per write) | The operator ruled it out: the server would hold model credentials, read every turn, and spend for every tenant; the agent already holds the turns in context. | — |
| **Extraction by a separate model in the hook** | A second model reads the transcript off disk, outside the harness's permission model, and needs its own key. | — |
| **Storing the transcript by default** | It keeps everything a person said, secrets included, for facts that fit in a sentence; the scanner sees bodies, not files on disk. | — |
| **Automatic promotion of chat facts** | R1: an agent's write never releases itself; and chat facts are the least reviewed text OML would hold. | — |
| **Per-turn recall on by default** | Every prompt's words leave the machine, and no labelled set shows the hits help (#111). | #111's set shows at least half of injected hits relevant at a p95 under the budget. |
| **Capture at `SessionEnd` or `PreCompact`** | Neither can put text in front of the model (Context); only `Stop` can. | The harness lets either inject context. |
| **Capture on every `Stop`** | A continuation per turn doubles the turns a session spends; every fifth turn bounds it. | — |
| **`origin.kind` inferred by the server from the anchor** | The anchor says where, not what kind: a repository file may be a document or code; inference would be a guess signed as a statement. | — |
| **A chat workspace per project** | Preferences belong to the person across projects; project facts already go to the project's workspace. A per-project chat workspace is a child the person may create under `chat`. | People ask for it. |
| **The raw session id in the anchor** | It links a stored memory to a transcript file on the person's disk and to the harness's logs. | — |

## Open questions

1. Whether `turn=<n>` numbering survives `/clear` and resume in Claude Code; the session id
   may change, which starts a new pseudonymous session (#139).
2. Whether the curator should also merge a run of small facts into one, which ADR-0030 d9's
   `write` act allows but no signal proposes yet.
3. Which answering and judge models the published QA numbers use; d12 records whatever the run
   names, and the first run with a key decides the pair the release reports.
