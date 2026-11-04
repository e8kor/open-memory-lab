<!--
id: adr/ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call
tier: engineering
status: proposed
answers: what a guardrail is and how it differs from steering; how it composes with user roots, universe curation and the web pages of v0.5.0; what its severities mean; why only promoted guardrails bind and how one ratifies; which guardrails apply at a scope node under the key's read access and the guardrail scope an admin sets on the key, and how a conflict is shown; the one read that serves them; how the agent plugin delivers them at session start, after a compaction and to a subagent, under which frame and cap; the deterministic rule and the local PreToolUse check, its cache and its failure modes; how other harnesses get them; how a change freeze is time-boxed; what the hooks never do; the build order
-->

# ADR-0037 — A guardrail is a claim of its own kind, ratified under its workarea's review policy, that the agent plugin loads at every session start within the scope an admin set on its key, and whose optional rule a local hook checks before each tool call

**Status:** proposed · 2026-10-31 · revised 2026-11-04 onto the v0.4.0 API, ADR-0044 and ADR-0045

**Release:** planned for v0.5.1 ([releases](../product/releases.md#decisions))

**Amends:** ADR-0001 (d5), ADR-0006 (d6), ADR-0012 (d5), ADR-0013 (d5), ADR-0014 (d2), ADR-0018 (d4), ADR-0022 (d2), ADR-0025 (d2, d11), ADR-0026 (d9), ADR-0028 (d1), ADR-0029 (d7), ADR-0039 (d2, d3, d15); and the proposed ADR-0044 (d10) and ADR-0045 (d9), when each is accepted

## Context

**The request** (the user, 2026-10-31): hook session start in Claude Code and other agentic
harnesses and load guardrails "from workarea or from workarea and all workareas parents, depends
on settings"; guardrails are "special kind of claims". Steering already applies along that chain
([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d8) and `oml hook session-start` injects
it ([ADR-0029](ADR-0029-session-start-hook.md) d1, d2), but as "data to check, not instructions
to follow" (its d6), with no severity and no check at the tool call, where every harness
surveyed enforces; [session claims](../engineering/session-claims.md#sources) quotes each source
this record leans on and catalogues the other claim kinds these hook points could carry.
Proposed, not built.

**What the maintainer decided on 2026-10-31, recorded here, not reopened.** On the setting: "It
should be decided in api key issued by server? Never on client". (1) No reach-down: a key loads
an ancestor's guardrail only under its own reader access (d4). (2) The scope, `workarea` or
`ancestors`, is an attribute an admin sets on the key and the server applies; severity decides no
inheritance (d2, d4). (3) The review policy decides, with no human-verdict floor (d10). (4) With
no rule cache at all, the check allows and warns (d9). Each replaced reading is rejected below.

**What moved under this record since 2026-10-31, and how it is rebased (revision of
2026-11-04).** Three records changed the ground it stands on, and this revision follows each
without reopening a decision above:

- **The v0.4.0 API** ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md)). MCP serves three
  tools, `oml_write`, `oml_read` and `oml_workarea` (its d2); a request names its workarea in a
  `workarea` field and the `X-OML-Workarea` header is gone (its d7); a response says `text` where
  it said `body` (its d7); and `GET /v2/whoami` was built to gain "the guardrail scope when
  ADR-0037's is built" (its d15). d4, d5, d10 and d11 below speak that API.
- **User roots** (ADR-0044, on `feat/user-roots`,
  proposed, planned for v0.5.0). Every person works under `universe.<user>`; a user key's grants
  stay inside that root, and issuance adds one reader grant on the `universe` node alone (its
  d3, d4); `universe` takes claims only from curator keys, reviewed by a second identity (its
  d7), and serves them without their source (its d9). Its d10 already reads this record's
  `guardrail_scope: ancestors` as walking the same ancestry and so serving `universe`'s
  guardrails to every user key. d4 and d16 below state that composition.
- **The web pages** (ADR-0045, on `feat/web-console`,
  proposed, planned for v0.5.0). Every page act is a session key's, which has no value and never
  reaches `/v1` or `/mcp` (its d6), and its d20 (g) moves this record to v0.5.1. d17 below says
  what the pages may show.

The maintainer's decisions above stand as they were taken: guardrail is its own claim kind; no
reach-down; the scope is set on the key at issuance, `workarea` or `ancestors`, never by the
client; the review policy decides with no human floor; no cache means allow and warn. Where
this revision met a question those decisions did not settle, d18 listed it; the maintainer
answered all ten on 2026-11-04, and d18 records the answers.

## Decision

- **d1 — `guardrail` is a claim kind of its own, with steering's applicability, and differs from
  steering in six ways.** A guardrail is a promoted rule for every agent at its workarea or
  below; like steering it is never a hit, never ranked by the query and never cut. The cost: one
  `kind` value and one read, against ADR-0013 d5's "no separate delivery channel".

    | | steering (ADR-0025 d7, d8) | guardrail |
    |---|---|---|
    | content | a body | a body, a `severity` (d2), an optional `rule` (d8) |
    | delivery | recall's first segment, every recall | its own read (d5) and hook (d6), never in recall |
    | frame | under ADR-0029 d6's data line | its own section, instructions with provenance (d6) |
    | ratification | the workarea's review policy | the same policy, never self-signed (d10) |
    | budget | served whole past any budget | never cut, a hard cap, no partial set (d7) |
    | tool calls | none | a rule checked locally before a matching call (d9) |

- **d2 — Severity is `never`, `must` or `should`: how binding a line is, which effect its rule
  may take (d8), and the first key of the order (d4).** After RFC 2119, `must` is "an absolute
  requirement", `never` its MUST NOT, `should` the reviewed default
  ([RFC 2119](https://www.rfc-editor.org/rfc/rfc2119), fetched 2026-10-31). Severity decides no
  inheritance (d4); it is a clear, signed field, never inherited by a successor (ADR-0025 d11).

- **d3 — Only a promoted guardrail binds.** No held guardrail is served as a rule, inside an
  in-progress workarea of proposed ADR-0030 included (its d8). The read (d5) lists the caller's
  own held guardrails in its chain by `claim_key` and version, under "held, not in force", at
  most five lines, never by body.

- **d4 — Applicable guardrails at scope node N are ADR-0025 d8's applicable set for kind
  `guardrail`, under the key's own read access and the guardrail scope an admin set on the key.**
    - **Read access, unchanged (decided by the maintainer).** A guardrail on N or an ancestor of
      N applies only where the key's reader effective access covers that node
      ([ADR-0008](ADR-0008-workarea-algebra.md) d4). The cost, plainly: an agent key sees its
      organisation's guardrails only if it is granted read on those ancestors, which also reads
      their promoted memories; a `--write-only` key loads none.
    - **Guardrail scope, on the key (decided by the maintainer).** `guardrail_scope: workarea`
      serves the guardrails on N alone; `ancestors` serves N's and those of every ancestor of N
      the key reads. The server resolves it from the journalled issuance, as it does `held_by`
      ([ADR-0022](ADR-0022-two-ring-api.md) d5). An admin sets it with the flag
      `--guardrail-scope <workarea|ancestors>` on `oml keys issue` and `oml keys issue-agent`,
      sent in `POST /v2/keys` and journalled in `key.issued` (amends ADR-0025 d2,
      [ADR-0028](ADR-0028-agent-key-issuance.md) d1); the default is `ancestors` (decided
      by the maintainer, d18 Q1). Issuance is a journalled act
      ([ADR-0010](ADR-0010-roles-and-keys.md) d3) and "There is no in-place re-key"
      ([key lifecycle](../architecture/roles-and-keys.md#key-lifecycle)), so only a rotation
      changes it (decided by the maintainer, d18 Q2): `oml keys rotate --guardrail-scope` sets
      the successor's, which otherwise keeps its predecessor's, journalled in `key.rotated`, the
      successor's stand-in for `key.issued` (amends ADR-0026 d9). Rotation is decided, not built;
      until then, revoke and reissue. A `key.issued` or `key.rotated` without `guardrail_scope`,
      every key issued before the build included, folds as `ancestors`, the issuance default, and
      `api_keys` holds that value for it; nothing in the journal is rewritten (decided by the
      maintainer, d18 Q3).
    - **Never on the client.** `GET /v1/guardrails` takes no scope parameter, the hooks read no
      scope setting, and the `workarea` query field (ADR-0039 d7) picks N within the key's
      grants, never the scope. No
      `off` value exists: an admin who wants none issues the key with `workarea` where none is.
    - **Under user roots** (ADR-0044, on `feat/user-roots`),
      `ancestors` walks from the scope node through the user's root to `universe` under this
      same reader test, and reaches nothing else (d16).
    - **Order and conflicts.** Severity (`never`, `must`, `should`), then depth from the root,
      `promoted_at` and `artifact_id`, never the query (R3). All applicable guardrails apply,
      so the strictest comes first; rows sharing a `claim_key` along the chain restate one topic,
      and when their severities differ each carries `conflict`, the `artifact_id`s of the others,
      and its d6 line ends `conflicts with <artifact_id>, …`. Rules union (d8). A contradiction
      under different keys is not detectable.

- **d5 — One provider-ring read serves the set: `GET /v1/guardrails`,** which amends ADR-0022
  d2's `/v1` verb list by this read, under the key at its default workarea or the `workarea`
  query field ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d7) and its
  `guardrail_scope` (d4). It answers `{scope_node, scope_path, guardrail_scope, set_digest, over_cap,
  guardrails: [{artifact_id, claim_key, version, workarea_path, depth, severity, text, rule,
  tokens, promoted_at, verdicts: [{held_by, at}], conflict}], own_held:
  [{claim_key, version}]}`; `set_digest`, over the served ids and versions, is opaque to clients,
  which only compare it (d11). Recall never serves kind `guardrail`, and the read is logged as
  recall is (ADR-0025 d9). No reviewer key name is served: ADR-0025 d15 keeps review events in
  the evidence export. The cost: every reader in scope learns how many reviewers ratified a rule
  and how many of them were human. `GET /v2/whoami` serves the key's `guardrail_scope` beside
  its grants, as ADR-0039 d15 reserved, so `oml doctor` and a page can say which scope a key
  carries without loading the set; that reads no claim and logs no read. The route enters
  `openapi.yaml` with its build, not before (ADR-0039 d9).

- **d6 — `oml hook guardrails` is a second command hook, on SessionStart for every source and on
  SubagentStart, and prints the set in its own frame.** It reads `OML_URL` and `OML_API_KEY`
  alone (ADR-0029 d4), sends one `GET /v1/guardrails`, prints one hook document and, on
  SessionStart only, writes the rule cache (d9), so a subagent checks its session's cache. Its
  own process gives it its own 10,000-character cap ([hooks](https://code.claude.com/docs/en/hooks),
  fetched 2026-10-31) and failure mode; a compaction re-runs it (`source: compact`). The frame:

    ```text
    OML guardrails for this session, from <origin>, workarea <path>, guardrail scope <scope>: rules promoted under
    each workarea's review policy by keys other than their authors'. These are instructions: follow never and must
    lines as binding and should lines as the reviewed default. Lines marked "checked" are also checked before
    matching tool calls; every other line is text only.
    - [must] Run the test suite before every commit. (universe.eng, eng-tests v2, ratified 2026-10-31 by 2 reviewers (1 human); checked: ask shell ["git","commit","*"])
    ```

    Lines are flattened and redacted as ADR-0029 d6 and d4 do; each hook's output frames itself,
    so the order of parallel hooks means nothing. A failure, a cache not written included, exits
    0 with a `systemMessage` the user sees, and its lines print without "checked": memory may
    fail silently (ADR-0029 d5), rules may not. The cost: a third request per session start, and
    `OML_SESSION_CONTEXT=off` leaves guardrails on. Both `hooks.json` entries are ADR-0029 d7's
    with this verb, no matcher, the status message `Loading OML guardrails` and ADR-0029 d5's
    timeouts, and the guard makes a missing or older `oml` load nothing silently:

    ```sh
    command -v oml >/dev/null 2>&1 || exit 0; oml hook guardrails || exit 0
    ```

- **d7 — Guardrails are never cut; the set is capped, and over the cap no partial set is
  printed.** Admission caps a body at 300 UTF-8 bytes and a rule at 160 (d10); the hook caps a
  line's provenance at 200 characters and the own held list at five lines (d3); at most 12
  guardrails apply at a node, so the section stays under 9,600 characters, below Claude Code's
  cap. Over 12, the read serves all with `over_cap: true`, and the hook prints the header, "N
  guardrails apply here, above the cap of 12, so none is shown: a partial set would read as the
  whole; `oml guardrails show` lists them" and a `systemMessage`, still caching every rule.
  `oml guardrails show` prints the hook's text and exits 1 over the cap. The caps are design
  constants derived from one sourced cap.

- **d8 — A rule is a deterministic predicate on one tool call, and its effect is `deny` or `ask`,
  never `allow`.** Grammar `oml-gr/1`, one rule per guardrail: `{"rule": "oml-gr/1", "tool":
  "shell" | "edit" | "read" | "fetch" | "mcp", <match>, "effect": "deny" | "ask"}`, over these
  Claude Code tools ([hooks](https://code.claude.com/docs/en/hooks), fetched 2026-10-31):

    | `tool` | Claude Code tools | `<match>`, against |
    |---|---|---|
    | `shell` | `Bash` | `command`, a list of word globs, against each segment of `command` (below) |
    | `edit` | `Write`, and every tool whose name ends in `Edit` | `path`, a glob, against `file_path`; for `NotebookEdit` the field is *unverified* |
    | `read` | `Read`, `Grep`, `Glob` | `path`, a glob, against `file_path` for `Read`, and `path` for `Grep` and `Glob`, the input's `cwd` when absent |
    | `fetch` | `WebFetch` | `host`, exact or `*.` suffix, against the [WHATWG URL](https://url.spec.whatwg.org/) host of `url`, lower-cased, without a trailing dot |
    | `mcp` | every tool named `mcp__*` | `name`, a glob, against the tool name |

    A `shell` command is tokenised by POSIX quoting without expansion, then split into segments
    at unquoted `;`, `&`, `&&`, `||`, `|` and newlines; the list matches a segment's words in
    order from its first, a final `*` word matching zero or more (`["git","push","*"]` matches
    `git push`). A path and a relative glob are made absolute against the input's `cwd` and
    normalised lexically (`.`, `..`, repeated `/`), symlinks unresolved; `*` and `?` stop at `/`,
    `**` does not, and each matches a leading `.`. `*` matches any run and `?` one character, in
    O(pattern × input) time with no backtracking. Whenever a rule of its class is cached, `ask`
    answers a `PowerShell` call (class `shell`; POSIX quoting does not tokenise it), a mapped
    call lacking its argument, and a command that does not tokenise or holds command or process
    substitution, a subshell, a brace group or a here-document; no other tool, `WebSearch` and
    `Agent` included, is matched. `should` takes `ask` only. Rules union and deny wins over ask,
    so a poisoned rule at worst stops work ("An allow rule can't carve an exception out of a
    deny rule", [permissions](https://code.claude.com/docs/en/permissions)). Named bypasses, as
    for Claude Code's deny rules: `sh -c`, aliases, another path to the program, a leading
    assignment, `env`, `sudo`, an option before a subcommand (`git -C . push`), a symlink, a
    search over a parent directory.

- **d9 — `oml hook pre-tool-use` checks the cached rules before each matching tool call, with no
  network call; a known rule it cannot check fails closed.** At SessionStart `oml hook
  guardrails` writes the served rules, each with its guardrail's `artifact_id` and body, as one
  JSON line and a line holding its SHA-256 in hex, to `$XDG_STATE_HOME/oml/guardrails/<session_id>.json`
  (default `~/.local/state`; directory 0700, file 0600) by write-and-rename, refusing a
  `session_id` outside `[A-Za-z0-9-]`, and removes files there older than 30 days, a design
  constant. The cache is a client copy class outside erasure (FR-54); no tool argument leaves
  the machine; a built-in rule asks before any `edit` under the cache directory and any `shell`
  word containing `oml/guardrails`.

    | At PreToolUse | The check |
    |---|---|
    | a cached rule matches | prints `permissionDecision` `"deny"` or `"ask"` with a `permissionDecisionReason` naming the guardrail's `artifact_id` and body; deny wins over ask |
    | no rule matches | prints nothing; it never prints `allow` |
    | the cache does not parse, its digest line does not match, or it names an unknown grammar | denies every call, naming the cause and the fix, a restarted session: a known rule that cannot be checked fails closed |
    | no cache file (decided by the maintainer: allow and warn) | decides nothing and prints one `systemMessage`, "OML guardrails are not loaded for this session, so tool calls are not checked; restart the session to load them"; it keeps no state, so the line repeats on each matching call |
    | `oml` is missing or older than the verb | checks nothing (the probe below) |
    | the verb fails or stalls past 5 seconds | blocks the call |

    The entry's matcher, `^(Bash|PowerShell|Write|.*Edit|Read|Grep|Glob|WebFetch|mcp__.*)$`, is
    anchored because Claude Code tests it as an unanchored JavaScript regular expression; its
    timeout is 5 seconds with `onFailure: "block"` (Claude Code 2.1.295 and later); its status
    message is `Checking OML guardrails`; its command's probe keeps an older `oml`'s exit 2 on an
    unknown subcommand from blocking every call (ADR-0029 d7), and the verb has no `|| exit 0`:

    ```sh
    command -v oml >/dev/null 2>&1 || exit 0; oml hook pre-tool-use --help >/dev/null 2>&1 || exit 0; oml hook pre-tool-use
    ```

    **What this is, plainly.** A text guardrail is an instruction to a model, not a guarantee,
    and fails open as ADR-0029 d5 does. A checked rule is "harness-enforced where installed":
    without managed settings or a Team or Enterprise login a Claude Code mod can let through a
    call a PreToolUse hook blocked, and a process running as the user can rewrite the cache,
    digest included. The costs: a second process per matching call; a change mid-session reaches
    the cache only at the next start or compaction; and a session idle past 30 days loses its
    cache to another session's start, leaving it unchecked and warned.

- **d10 — Writing a guardrail is `POST /v1/memories` with `kind: "guardrail"`; it ratifies under its
  workarea's review policy like any claim, and never by self-signature.** `severity` is required
  and `rule` optional on kind `guardrail`, both refused elsewhere and on a retirement
  (`request.validation`); a supersession re-sends both. As ADR-0025 d12 does for the vocabulary
  kinds, a guardrail is written and superseded on the HTTP API only: `oml_write`'s `kind`
  admits no `guardrail`, refused with `request.validation` and `details.field: kind`, and an
  `oml_write` whose `replaces` names a guardrail is refused with `details.field: replaces`, as
  it refuses a vocabulary target (amends [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md)
  d3).
  A `guardrail` target admits only kind `guardrail`, and no other target admits it (amends ADR-0025 d11). A simulation write of kind
  `guardrail` is refused: a rule binds beyond any audience. Any writer on the node may propose
  one ([role table](../architecture/roles-and-keys.md), steering row). The scan adds two veto
  classes under a new `ruleset_vN`: `guardrail.rule_unparsable` (outside `oml-gr/1`, an `allow`,
  a `should` that denies) and `guardrail.too_long` (d7's caps).
    - **The review policy decides (decided by the maintainer).** A version, a retirement
      included, promotes when its workarea's policy is met
      ([ADR-0012](ADR-0012-admission-and-review.md) d3, unchanged); R2 holds, so no verdict of
      its author or its lineage closure counts. A workarea that wants a person on every rule
      names kind `guardrail` in `human_required_classes`. The cost: where the policy names no
      person, agents' verdicts alone make a rule bind.
    - **Never self-signed (decided by the maintainer, d18 Q4),** whatever the tenant's policy:
      a rule can stop every agent below it, so it needs one identity other than its author's. In
      a `development` workarea it waits for a non-author verdict, as ADR-0025 d17 makes a
      reviewed tip's successor wait (amends ADR-0001 d5). The serve of a held guardrail adds
      "every key that reads this workarea will load this rule, here and below it unless its
      guardrail scope is workarea" (amends ADR-0012 d5).

- **d11 — Harnesses without these hooks get an export and an MCP prompt, each text only.**
  `oml guardrails export --format agents-md | claude-md --out <path> [--check]` reads
  `GET /v1/guardrails` under the key it is given and that key's scope (an admin key reads no
  content and is refused, [ADR-0026](ADR-0026-names-are-keys.md)), refuses a scope node inside an
  in-progress subtree (proposed ADR-0030 d4), and writes promoted guardrails only: `claude-md` a
  whole `.claude/rules/` file with no `paths` frontmatter, which Claude Code loads
  "unconditionally", or `agents-md` the text between the one pair of `<!-- oml:guardrails -->`
  markers in the named file, refusing any other count. Its header names the origin, scope node,
  guardrail scope, count and `set_digest`, and "text, not enforcement; do not edit"; `--check`
  exits 1 when the live digest differs. The costs: a copy no erasure or demotion reaches
  (FR-54), read by the repository's readers, stale until re-exported. An MCP prompt,
  `guardrails`, renders d6's text for the key's default workarea; prompts are "designed to be
  user-controlled" ([prompts](https://modelcontextprotocol.io/specification/2025-06-18/server/prompts),
  fetched 2026-10-31), so it reaches Claude's apps only when a person invokes it (amends ADR-0014
  d2 with the prompts capability, and ADR-0039 d2 by one prompt beside its three tools, which
  its d8 admits: a prompt is not a tool). No
  adapter for the [other harnesses](../engineering/session-claims.md#hook-points-by-harness) is
  built here (FR-53).

- **d12 — A change freeze is a guardrail with ADR-0036's validity interval, once that record is
  accepted.** Until then a freeze ends by a reviewed retirement. On acceptance of both, kind
  `guardrail` takes `valid_from` and `valid_to`, reviewed with the body, and applicability adds
  "now ∈ [valid_from, valid_to)", a structural test that never ranks, the one place valid time
  schedules (amends proposed ADR-0036 d2 and d4); the cache carries each interval, so the check
  stops denying at `valid_to` with no request.

- **d13 — The hooks never execute guardrail text and never widen a key's access.** Bodies and
  rules are data, never passed to a shell, an interpreter or the `hooks.json` command line; both
  hooks read under the agent's own key, access and scope (d4), and the check answers deny, ask
  or nothing. A shared guardrail store is the poisoned-repository case of
  [OWASP LLM01](https://genai.owasp.org/llmrisk/llm01-prompt-injection/) (fetched 2026-10-31),
  mitigated by the review policy and R2 (d10), provenance (d6), access and scope (d4), the caps
  (d7) and deny-or-ask only (d8); the frame is a marking, not a guarantee. Under Meta's
  [Agents Rule of Two](https://ai.meta.com/blog/practical-ai-agent-security/) (fetched
  2026-10-31), checked rules fence sensitive access and state change; text fences nothing.

- **d14 — The wire and the vocabulary, held here until built.** `kind` gains `guardrail`;
  `POST /v1/memories` and its supersede gain `severity`, clear on the submission events, and `rule`, sealed
  under the version's DEK with its digest in the envelope ([ADR-0004](ADR-0004-bodies-and-attachments.md)
  d1); issuance, rotation, `key.issued`, `key.rotated`, the key reads and `api_keys` gain
  `guardrail_scope`, and `GET /v2/whoami` serves it (d5); d5's read, d11's prompt and d10's two
  finding classes are added, and no error code. The contract pages change with the build, not before. ADR-0018 d4 gains:

    | Term | Means | Never |
    |---|---|---|
    | guardrail | a claim of kind `guardrail`: a promoted rule with a severity, ratified under its workarea's review policy and never self-signed, served by its own read under its own frame | "policy" (review policy objects own it); "enforced" without "harness-enforced where installed" |
    | guardrail rule | a guardrail's optional `oml-gr/1` predicate on one tool call, effect `deny` or `ask`, checked by the local hook | "allow rule" |
    | severity | `never`, `must` or `should` on a guardrail; the first key of the order, and it decides no inheritance | "priority" |
    | guardrail scope | a key's issuance attribute, `workarea` or `ancestors`, set by an admin, that bounds which applicable guardrails the key loads | "reach" |

- **d15 — The build is planned for v0.5.1, after v0.5.0, in six steps, each closed by its tests.**
  S1, S2 and the pure core of S3 and S4 rest on the v0.4.0 API alone and may land before
  ADR-0044 and ADR-0045 are built; S6 waits for both, and S5's d12 for ADR-0036 S1 and S3.

    | Step | Lands | Check, tests named as sentences |
    |---|---|---|
    | S1 | d2, d10, d14 and d4's key attribute: kind, severity, rule, scan classes, `guardrail_scope` on issuance and on `whoami`, the serve | `a_guardrail_without_a_severity_is_refused`, `severity_and_rule_are_refused_on_any_other_kind_and_on_a_retirement`, `a_guardrail_target_admits_only_kind_guardrail`, `a_rule_outside_oml_gr_1_or_with_allow_is_vetoed_as_guardrail_rule_unparsable`, `a_body_over_300_bytes_is_vetoed_as_guardrail_too_long`, `a_guardrail_promotes_under_its_workarea_policy_and_no_author_verdict_counts`, `a_guardrail_is_never_self_signed_whatever_the_tenant_policy`, `the_serve_of_a_held_guardrail_carries_the_load_warning`, `oml_write_refuses_kind_guardrail_and_a_guardrail_target`, `a_key_issued_without_guardrail_scope_folds_as_ancestors`, `a_key_is_issued_with_guardrail_scope_ancestors_unless_the_admin_names_workarea`, `key_issued_journals_the_guardrail_scope`, `whoami_serves_the_guardrail_scope`; with simulations, `a_simulation_write_of_kind_guardrail_is_refused`; with rotation, `a_rotation_keeps_the_guardrail_scope_unless_it_names_one` |
    | S2 | d3, d4, d5: the read | `a_held_guardrail_is_never_served_as_a_rule`, `recall_never_serves_kind_guardrail`, `an_ancestor_guardrail_is_served_only_to_a_key_whose_reader_access_covers_that_ancestor`, `guardrail_scope_workarea_serves_the_scope_node_alone`, `no_parameter_or_field_changes_the_guardrail_scope`, `the_set_orders_by_severity_then_depth_then_promoted_at_then_artifact_id`, `rows_of_one_claim_key_with_different_severities_each_name_the_others_in_conflict`, `a_set_over_twelve_is_served_whole_with_over_cap_true`, `the_read_logs_the_served_ids`; with ADR-0030, `…_even_inside_an_in_progress_workarea` |
    | S3 | d6, d7: `oml hook guardrails` and its SessionStart and SubagentStart entries | `every_line_names_its_workarea_claim_key_version_and_reviewer_count`, `a_conflict_line_names_the_other_artifact_ids`, `over_the_cap_no_guardrail_line_is_printed_and_the_user_is_told`, `a_compact_source_prints_the_whole_set_again`, `the_header_names_the_guardrail_scope_the_server_applied`, `the_key_value_is_redacted_in_every_line`; `check_plugin.py` holds both entries as ADR-0029 d7 holds its own |
    | S4 | d8, d9: the rule cache, `oml hook pre-tool-use` and its PreToolUse entry | `a_matching_deny_rule_answers_deny_with_the_guardrail_id_and_body`, `deny_wins_over_ask`, `no_input_ever_answers_allow`, `the_check_sends_no_request`, `an_unreadable_cache_denies_every_call`, `a_cache_whose_digest_line_does_not_match_denies_every_call`, `a_session_with_no_cache_decides_nothing_and_warns_in_one_line`, `a_cache_not_written_prints_no_line_as_checked_and_names_the_cause`, `a_subagent_start_writes_no_cache`, `a_cache_file_older_than_thirty_days_is_removed_at_start`, `an_edit_of_the_cache_directory_or_a_shell_word_naming_it_asks`, `each_mapped_tool_is_matched_on_its_argument_and_no_other_tool_by_any_rule`, `a_shell_segment_after_a_control_operator_is_matched`, `a_quoted_operator_splits_no_segment`, `a_command_that_does_not_tokenise_or_holds_a_substitution_asks`, `a_final_star_word_matches_zero_or_more_words_from_the_first_word_on`, `a_path_and_its_glob_are_normalised_against_the_input_cwd`, `a_star_stops_at_a_slash_and_a_double_star_does_not`, `a_fetch_host_is_the_whatwg_host_lower_cased_without_a_trailing_dot`, `a_glob_of_many_stars_matches_a_long_command_in_linear_time`; `check_plugin.py` holds the PreToolUse matcher equal to d8's tools, the timeout, `onFailure` and the status message, and runs the command under `/bin/sh` with no `oml`, with a stand-in older `oml` and with one whose verb fails: the first two exit 0 with no output, the third exits non-zero |
    | S5 | d11: export and the MCP prompt; d12 | `export_writes_promoted_guardrails_only`, `export_refuses_a_scope_inside_an_in_progress_subtree`, `the_header_carries_origin_scope_node_guardrail_scope_and_set_digest`, `check_exits_one_when_the_set_changed`, `agents_md_refuses_a_file_without_exactly_one_marker_pair`, `the_prompt_renders_the_hook_text`, `a_freeze_applies_only_inside_its_interval` |
    | S6 | d16, d17: user roots and the pages, once ADR-0044 and ADR-0045 W3 are built | `a_user_key_with_scope_ancestors_loads_universes_guardrails_through_its_node_grant`, `a_user_key_with_scope_workarea_loads_no_universe_guardrail`, `no_guardrail_of_another_users_root_is_ever_served`, `a_user_keys_guardrail_write_into_universe_is_refused_as_universe_curated`, `a_universe_guardrail_is_served_with_origin_universe_and_no_source`, `the_users_reviewer_key_ratifies_a_guardrail_its_agents_propose_in_its_root`, `the_user_page_shows_the_guardrails_in_force_at_the_users_root_and_writes_none` |

- **d16 — Under user roots, `ancestors` walks from the scope node through the user's root to
  `universe`, under the same reader test, and `universe`'s guardrails are the curators'.** This
  composes d4 with ADR-0044, on `feat/user-roots`
  and changes neither:
    - **The walk.** For a scope node N inside `universe.<user>`, `ancestors` serves the
      guardrails on N, on every ancestor of N up to and including `universe.<user>`, and on
      `universe`, each only where the key's reader effective access covers that node (d4).
      ADR-0044 d3 puts every grant of a user key inside its root and adds one reader grant on
      `universe` with `covers: node`, so the walk meets the user's own nodes and `universe`, and
      no shared workarea or other user's root is an ancestor of N: ADR-0044 d1 never moves a
      node across a root's boundary. This is the reading ADR-0044 d10 already states.
      `workarea` serves N's alone, so a user key issued with it loads none of `universe`'s; that
      is the issuing admin's choice, as for every key (d4).
    - **Who writes them.** A user key writes only inside its root (ADR-0044 d3), so a person's
      agents propose guardrails there alone, ratified under that workarea's review policy; the
      person's `<user>-review` key may approve them, never its own (R2 per key and principal,
      ADR-0044 d8; ADR-0045 d6). A guardrail on `universe` is a claim on `universe`, and only a
      curator key writes there, held and reviewed by a second identity (ADR-0044 d7): so
      `universe`'s guardrails are admin-curated, the admin choosing who curates by issuing the
      curator keys. A user key's guardrail write into `universe` is ADR-0044 d14's
      `submission.universe_refused`, as every user write there is. A shared workarea's
      guardrails, crews' included (ADR-0044 d13), are its tenant writers', as before.
    - **What is served.** A guardrail on `universe` is served to every key outside its authoring
      key set under ADR-0044 d9: `workarea_path` `universe`, nothing below it, and d5 serves no
      author in any case; the hook's line names its origin `universe`. Its `verdicts`, attestation
      and time, are served as on every guardrail, since they name no key (decided by the
      maintainer, d18 Q6).
    - **Tenant keys.** A key issued without `user`, every key before v0.5.0 included, is a
      tenant key (ADR-0044 d12) and walks d4's ancestry exactly as before.

- **d17 — The pages show guardrails and write none.** Under
  ADR-0045, on `feat/web-console`:
    - **Session keys load nothing.** A session key has no value, so it never reaches `/v1` or
      `/mcp` (ADR-0045 d6), and no hook ever runs under one; its `guardrail_scope` matters only
      to a page that reads the set through it.
    - **The user page may show the set in force at the person's root**, read-only: d5's use case
      under `<user>-web` at `universe.<user>`, rendered as d6's lines with their provenance, and
      the person's own held guardrails as d3 lists them. It reads no rule cache and checks no
      call.
    - **Review shows the load warning.** A held guardrail served on either page's review view
      carries d10's sentence, since the page serves the same ceremony (ADR-0045 d11).
    - **No page writes, supersedes, retires or exports a guardrail** in v0.5.1: a guardrail is
      written on the HTTP API and the CLI (d10), and ADR-0045's acts are its own list. Built as
      S6, after ADR-0045's W3.

- **d18 — The maintainer's answers of 2026-11-04 to this revision's ten questions.** Each is
  folded into the decision it governs; this list is where each answer is found.
    - **Q1 — The issuance default is `ancestors`** (d4).
    - **Q2 — Only a rotation changes a key's scope** (d4); until rotation is built, an admin
      revokes and reissues.
    - **Q3 — A key issued before the build folds as `ancestors`** (d4), its row holding that
      value and its journal unchanged.
    - **Q4 — A guardrail is never self-signed**, in a `development` workarea included (d10).
    - **Q5 — A key a person issues on the user page takes the issuance default**, and the page
      offers no choice (d17): only an admin issuing a key sets `workarea`.
    - **Q6 — A `universe` guardrail is served with its verdicts' attestations and times** (d5,
      d16), which name no key, curator or source.
    - **Q7 — The listings serve kind `guardrail` like any kind, and recall refuses
      `kinds: ["guardrail"]`** with `request.validation`, `details.field: /kinds` (d5): recall
      never serves a guardrail, and a filter that names one is refused, never answered empty.
    - **Q8 — `oml write` takes `--severity` and `--rule`** (d10): the CLI is an HTTP client, and
      d10 keeps guardrails off MCP alone.
    - **Q9 — `info.version` moves once, at the release that carries this record,** one step past
      the revision v0.5.0 sets; the build does not move it.
    - **Q10 — The rule grammar, the matcher, the check and the rule cache's types live in a leaf
      crate of their own, `oml-guardrail`,** which `oml-domain` and `oml-cli` both link and
      which links no workspace crate. `oml-domain`'s `guardrail` module re-exports it and keeps
      what only the server needs: the guardrail scope, admission and its caps' canonical
      measure, the set's order, conflicts and digest. `oml-cli` links `oml-guardrail` and no
      other inward crate, so the hooks and the server cannot disagree on what a rule means.
      This amends [ADR-0006](ADR-0006-pure-rust.md) d6's "no inward edge" for `oml-cli` by this
      one leaf, and the [workspace plan](../engineering/workspace-plan.md) states it.

## Consequences

- **Pages on acceptance**: retrieval, admission, roles and keys, the CLI reference, audit and
  lineage (the rule cache and the export as client copy classes), the agent plugin and its
  README, the event catalog, the Postgres schema, the test strategy, and the D11 to D14 cells of
  the [register](index.md). Each cost is stated in its decision (d1, d4, d5, d6, d9, d10, d11).
- **Roadmap rows, each proposed**: [FR-50](../product/roadmap.md) to FR-54 for this record, and
  FR-55 to FR-59 for the kinds catalogued on [session claims](../engineering/session-claims.md).
- **Process.** With the proposal: this file, a register row, a nav entry, `llms.txt`, one
  [changelog](../changelog.md) entry and the roadmap rows. The contract pages, `openapi.yaml`
  and the designed annex change with the build step that serves each field (ADR-0039 d9), and
  `info.version` with them (d18, Q9).
  On acceptance, and not before: the record and its row become `accepted design`, dated; each
  record on the Amends line gains "· amended by ADR-0037"; ADR-0013 d5 and ADR-0029's
  Consequences gain dated notes; ADR-0018 d4 gains d14's rows; proposed ADR-0036 (d2, d4) and
  ADR-0030 (d8) take d12 and d3 when accepted; proposed ADR-0044 (d10) and ADR-0045 (d9) take
  d16 and d17 when each is accepted.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — A `facet: guardrail` on kind steering** | Every steering rule (cap, frame, review, delivery, valid time) would branch on a second field, and `human_required_classes` names classes by kind. |
| **d1 — A guardrail as an ADR-0027 `oml:about` classification** | A classifier's edge, added later by another key, would turn a memory into a binding rule without its author's reviewed content. |
| **d1 — Rules only in review policy objects** | Those bind OML's own acts at admission, an admin's journalled object; they say nothing to an agent's session. |
| **d2 — Severity, or a separate `inherit: required` mark, decides which ancestors' rules load** (the earlier reading: `never` and `must` at every descendant) | Replaced by the maintainer's decision of 2026-10-31: the key's guardrail scope alone decides, so no row field overrides what an admin set. |
| **d4 — Downward addressing: an ancestor's guardrail served to every reader of a descendant, amending ADR-0008 d4 for one kind** (the earlier reading) | Rejected by the maintainer on 2026-10-31: access stays ADR-0008 d4's for every kind, so no read reaches past a key's grants. |
| **d4 — A client setting for the scope: `OML_GUARDRAILS`, or any variable, header or parameter** (the earlier reading) | Rejected by the maintainer on 2026-10-31, "Never on client": what binds an agent is the issuing admin's to bound, on the key (ADR-0022 d5). |
| **d4 — An `off` value, on the client or on the key** | On the client a committed settings file would drop every rule for everyone who opens the repository; on the key it is not needed, since an admin who wants none issues the key where none applies. |
| **d4 — Closest wins, as AGENTS.md does** | A descendant could loosen an ancestor's rule by restating it. |
| **d5 — A guardrail segment in every recall, or an `oml_guardrails` MCP tool** | The first repeats up to 9,600 characters per recall under a cap and provenance unlike steering's; the second lets the model choose when to read its own rules. |
| **d6 — A first section of `oml hook session-start`** | One 10,000-character cap shared with never-cut steering, over which Claude gets a 2,000-character preview; and one fail-silent mode for rules a user must know did not load. |
| **d6 — A per-turn reminder at UserPromptSubmit** | The set does not change within a session; its effect is unmeasured (FR-52). |
| **d8 — Regular expressions, or an `allow` effect** | The first can run away and differs across engines; the second lets a stored claim widen what an agent may do. |
| **d9 — Fetch the set on every tool call** | A request per tool call, the tool's arguments sent to the server, and a down server stops all work. |
| **d9 — An `exit 0` fallback after the check, as ADR-0029 d7 has** | A crash of the verb would allow the call, so `onFailure: "block"` would cover a timeout alone. |
| **d9 — With no cache, the newest cache of another session** | The check makes no request, so it cannot know the key's scope node, and another session's file may come from another key. |
| **d9 — Fail closed with no cache at all** | Rejected by the maintainer on 2026-10-31: with no cache the hook knows no rule, and denying everything would stop every user whose server was down at session start. |
| **d10 — A human verdict on every guardrail version that no policy removes, amending ADR-0012 d3** (the earlier reading) | Rejected by the maintainer on 2026-10-31: the workarea's review policy decides, and a workarea that wants a person names kind `guardrail` in `human_required_classes`. |
| **d10 — A guardrail crew or a curator profile owning guardrails** | A new profile in the grant algebra for a kind the review policy already governs, and a crew that writes and reviews its own rules works against R2. |
| **d18 Q5 — The user page offers the scope** | A person could issue their agents keys that skip `universe`'s admin-curated guardrails; the scope is the issuing admin's to set. |
| **d18 Q7 — Recall answers `kinds: ["guardrail"]` with nothing** | An empty answer reads as "no guardrail applies"; a refusal names where the set is served. |
| **d18 Q10 — `oml-cli` links `oml-domain` for the guardrail module alone** | The CLI would then link the whole pure core, chain signing types included, and "no inward edge" would no longer say what the CLI cannot do. |
| **d18 Q10 — A second copy of the check in the CLI, held equal by shared vectors** | Two implementations of a deny rule drift between test vectors, and a drift is a bypass or a block of every call. |
| **d11 — Render rules into Claude Code `permissions.deny` or managed settings** | Writing harness settings is an admin act on the user's machine, not the agent key's (R1); left to the catalogue, unbuilt. |
