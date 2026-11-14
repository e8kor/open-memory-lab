<!--
id: adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied
tier: engineering
status: proposed
answers: what a skill is in OML and which fields it carries; how its name is unique and how a new version replaces an old one; who may write one and how it is reviewed; which skills a session is offered; how MCP prompts/list and prompts/get name and serve them beside the guardrails prompt; how an agent reads one without a person invoking it; what the session-start hook says about skills; the size caps; what is metered; how a served skill is framed; why a held skill is never served as an instruction
-->

# ADR-0080 — A skill is a reviewed claim the MCP server lists as a prompt and serves as a resource, loaded live and never copied

**Status:** proposed · 2026-11-14 · tracking #374 · revised the same day with the operator's
answers to its three questions (d13)

**Release:** planned for v0.7.4

**Amends:** ADR-0025 (d8: kind `skill` joins the kinds never served as a recall hit), ADR-0030
(d3: a held `skill` version stays author-only inside an in-progress workarea), ADR-0037 (d11: the
prompts capability serves more than the one prompt `guardrails`), ADR-0039 (d3: `oml_write`
admits kind `skill`), ADR-0074 (d24: kind `skill` is an instruction kind whose every version needs
a person)

**Depends on:** [ADR-0081](ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md) (d6: the `resources` capability and the `oml://` scheme this record's d7 uses)

## Context

On 2026-11-14 the operator asked for a workarea builder that introduces and edits skills,
guardrails and documents, with the MCP server "loading and using those items in session without
downloading or putting copy locally", and chose, among the options put to them, that a skill is a
reviewed claim kind served as an MCP prompt (#374).

What exists today:

- **One prompt.** `initialize` declares `"prompts": { "listChanged": false }`
  (`crates/transports/oml-transport-http/src/mcp.rs:383-388`); `prompts/list` answers the single
  listing `guardrails` (`mcp.rs:110-120`, `mcp.rs:395`), and `prompts/get` refuses every other
  name with `-32602`, "prompts/get names one prompt: guardrails" (`mcp.rs:396-404`). The prompt
  renders the guardrail frame for the key's default workarea (`mcp.rs:431-457`;
  [ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md)
  d11). The endpoint is stateless and has no server-to-client stream (`mcp.rs:1-5`;
  [ADR-0014](ADR-0014-http-api-and-mcp-facade.md) d2).
- **Seven kinds.** `MemoryKind` is `memory`, `derived_fact`, `steering`, `term`,
  `term_mapping`, `vocab_release` and `guardrail` (`crates/oml-domain/src/model.rs:60-71`).
  Guardrail is the precedent for an instruction kind: written on the HTTP API only, refused by
  `oml_write` (`mcp.rs:1181`; ADR-0037 d10), served by its own read and never in recall
  (ADR-0037 d5), never self-signed, and only promoted versions bind (ADR-0037 d3).
- **Formats.** A version names a media type from a closed list that already holds
  `text/markdown` and `text/plain` (`crates/adapters/oml-format/src/lib.rs:41`;
  [ADR-0056](ADR-0056-a-claim-version-names-its-format-the-signature-covers-its-source-and-adapters-compiled-into-the-server-derive-what-search-reads.md)
  d1).
- **What clients do with prompts and resources.** MCP prompts "are designed to be
  **user-controlled**", support pagination on `prompts/list`, and an unknown name is `-32602`
  ([MCP 2025-06-18, server/prompts](https://modelcontextprotocol.io/specification/2025-06-18/server/prompts),
  fetched 2026-11-14). Claude Code "lists each MCP prompt as `/servername:promptname (MCP)`.
  Typing `/mcp__servername__promptname` also runs it", passes arguments split on whitespace, and
  "uses the prompt name as the server declares it"; for resources it "automatically provides tools
  to list and read MCP resources when servers support them", and `@server:protocol://path`
  mentions fetch a resource as an attachment
  ([code.claude.com/docs/en/mcp](https://code.claude.com/docs/en/mcp), "Use MCP prompts as
  commands" and "Use MCP resources", fetched 2026-11-14). A prompt therefore reaches the model
  only when a person runs it; a resource the model can read for itself.
- **The skill shape.** The Agent Skills format gives a `name` of 1 to 64 characters, "lowercase
  alphanumeric characters (`a-z`, `0-9`) and hyphens", no leading, trailing or doubled hyphen; a
  `description` of 1 to 1024 characters that "should describe both what the skill does and when to
  use it"; a markdown body; and progressive disclosure: name and description "loaded at startup",
  the body "(< 5000 tokens recommended)" loaded when the skill is activated, the file kept "under
  500 lines" ([agentskills.io/specification](https://agentskills.io/specification), fetched
  2026-11-14).

The decision has to say what a skill is, who sees which, how it is served without a local copy,
and how it stays reviewed text, without weakening R1 to R4. Decided, not built.

## Decision

- **d1 — `skill` is a claim kind of its own: a name, a one-line description and a markdown body,
  in the SKILL.md shape.** `kind: "skill"` on `POST /v1/memories` requires a `skill` object
  `{name, description}` and `format: "text/markdown"`; the body is the skill's instructions with
  no front matter (the server refuses a body that opens with a `---` line, `request.validation`,
  `details.field: body`, since name and description are fields, not text a parser must agree on).
  `skill` is refused on every other kind and on a retirement. The claim key is derived, never
  chosen: `skill/<name>`; a request that sends another `claim_key` is refused naming `claim_key`.
  `name` follows [ADR-0026](ADR-0026-names-are-keys.md) d2's creation grammar (a lowercase RFC
  1035 label: a letter first, at most 63 characters) and additionally refuses `--`, which makes
  every skill name a valid Agent Skills `name`; `description` is 1 to 1024 characters of one line.
  Both are part of the signed submission (as the anchor is, ADR-0057 d2) and shown to the
  reviewer. The cost: one more kind value, one object on the wire and one migration for its two
  columns; the gain is that a skill is reviewed, versioned, erased and audited exactly as every
  claim is, with no second store.

- **d2 — A skill's name is unique per workarea, because its claim key is, and a new version is a
  supersession.** One live claim per (tenant, workarea, claim key) already holds
  ([http-api.md](../api/http-api.md) submit, `claim_key`), so `skill/<name>` makes the name unique
  in its workarea with no new rule. Editing is `POST /v1/memories/{artifact_id}/supersede` with
  kind `skill`; the successor re-sends `skill` and may change `description` but not `name` (a
  renamed skill is a retirement of the old and a new claim, so a slash command never changes
  meaning under a person's hands). Retiring is the same act with `retirement: true`
  ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d11). The old version stays the served
  tip until the successor promotes (ADR-0025 d10); nothing is deleted (R4).

- **d3 — Any writer proposes a skill, agents through `oml_write` included; every skill version
  stays held until a person approves it, and no agent's or service's verdict counts.** `oml_write`
  admits `kind: "skill"` with its `skill` object, and a `replaces` that names a skill (amends
  ADR-0039 d3, which today refuses only `guardrail` there, `mcp.rs:1181`); any key with writer on
  the node may propose one over HTTP or MCP, and the console writes through ADR-0082's keys. A
  person approves: for kind `skill`, a verdict counts only from a session key held by a person
  (ADR-0074 d2's definition of a person), whatever the workarea's policy, so an agent key or a
  service key holding dreamer is refused `review.person_required` (the refusal ADR-0074 d2 names) with `details.reason:
  skill_needs_person` and the version stays held. This is ADR-0074 d24's steering rule made
  unconditional for skills: a skill runs with the weight of the person who invokes it, so an agent
  may draft one and a person decides. R2 holds unchanged on top (the author's key and lineage never
  count), and a skill is never self-signed in a `development` workarea (as ADR-0037 d10 for
  guardrails). Inside the person's own root their own review key may approve a version their
  agent or their web key wrote, marked `same_user` (ADR-0046 d2; ADR-0082 d7). The scan treats
  `skill` as an instruction kind beside `steering` and `guardrail`
  ([ADR-0074](ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md)
  d24): `injection.tool_coercion` does not run, and `exec.*`, `exfil.*`, `phish.*` and `ssrf.*`
  hold instead of annotating. Every skill version lands in the person lane with `needs_person:
  true` (`details.reason: skill_by_agent` when an agent or service key wrote it, `skill` otherwise).
  A held skill served to a reviewer adds the sentence "every key that reads this workarea will be
  offered this skill as a command, here and below it unless its guardrail scope is workarea", the
  counterpart of ADR-0037 d10's load warning (`LOAD_WARNING`,
  `crates/oml-domain/src/guardrail/mod.rs:19`). The cost: a workarea whose only reviewers are
  agents, such as a crew's, publishes no skill until a person reviews it.

- **d4 — The skills a session is offered are the applicable set of kind `skill` at the key's
  default workarea, under the key's own read access and its guardrail scope.** The rule is
  ADR-0037 d4's, word for word, for kind `skill`: a skill on N or an ancestor of N applies only
  where the key's reader effective access covers that node
  ([ADR-0008](ADR-0008-workarea-algebra.md) d4), and `guardrail_scope: workarea` keeps N's own,
  `ancestors` adds every readable ancestor's; under user roots `ancestors` walks the user's root
  to `universe` and reaches nothing else (ADR-0037 d16). Nothing in a request widens it: the
  `workarea` argument picks N within the key's grants, never the scope. Only promoted tips are
  offered. A skill in a child workarea is not offered at its parent, since a skill written for one
  project should not reach its siblings. The cost: the issuance field named for guardrails also
  scopes skills, so an admin who wants a key to see its organisation's guardrails but not its
  skills cannot say so; the operator kept one field on 2026-11-14 (d13).

- **d5 — `prompts/list` names each offered skill by its bare name, the nearest workarea wins a
  name shared along the chain, and `guardrails` stays reserved.** Each entry is
  `{name, title, description, arguments: [{name: "input", description: "Text appended after the
  skill's instructions", required: false}]}` with `name` the skill's name, `title` the name and
  the source workarea's path, and `description` the reviewed description. The order is depth from
  the scope node, nearest first, then name; when two workareas on the chain hold one name, the
  nearer is listed and the farther is reachable only as a resource (d7), and the nearer's
  `description` ends "(overrides <path>)". `guardrails` is a reserved skill name, refused at write
  with `request.validation`, `details.field: /skill/name`, so the guardrail prompt is never
  shadowed. `prompts/list` pages with MCP's `cursor` and `nextCursor`, 50 entries a page, the
  listing's default page size (`crates/transports/oml-transport-http/src/lib.rs:191`), the
  guardrail prompt first on page one. Claude Code then offers `/oml:<name> (MCP)` under the
  plugin's server name, `oml` (`plugins/oml/.mcp.json`). `listChanged` stays `false`: the
  endpoint has no stream to send it on, so a skill promoted mid-session is offered at the next
  `prompts/list`, which a client makes at connect.

- **d6 — `prompts/get` answers one user message: a header with the skill's provenance and
  approval, the reviewed body, and the optional input after it.** The header is one line,
  "OML skill `<name>` from `<workarea path>`, version `<n>`, approved `<approved_at>` by a
  `<held_by>` reviewer" with ", the author's own person" when `approver.same_user` is true
  ([ADR-0077](ADR-0077-a-served-memory-says-when-it-was-approved-and-by-what-kind-of-reviewer-and-never-names-the-reviewer.md)
  d1, d2); no reviewer is named. Then the body as stored, then, when `input` is sent, a line
  "Input:" and the text. An unknown name, a name not offered at the scope, or a held, rejected or
  retired skill all answer the same `-32602`, "no skill <name> is offered here", so a prompt read
  never discloses a row the key cannot read. The read is logged as recall is
  ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d9), with the served artifact id. The
  framing differs from recall on purpose: recall's text is "data to check and never instructions
  to follow" (`mcp.rs:69-70`), while a prompt is what a person chose to run, so its body arrives
  as their instruction, as a local SKILL.md would.

- **d7 — Each offered skill is also a resource, so the model can read one without a person
  running it.** Under ADR-0081 d6's capability and scheme, a skill's URI is
  `oml://workarea/<workarea>/skill/<name>`, `mimeType: "text/markdown"`, and `resources/read`
  answers the d6 header and body as one text content. `resources/list` serves the offered skills
  (d4, d5's order and shadowing) after the documents, under the same cursor. This is the
  progressive disclosure the Agent Skills format describes: d8's index puts names and
  descriptions in context, and the body is read when the task calls for it. A resource read by the
  model is framed by the same header, which says it is a reviewed skill, not a recalled memory.

- **d8 — The session-start hook adds an index of offered skills, names and descriptions only,
  capped, and never a body.** `oml hook session-start`
  ([ADR-0029](ADR-0029-session-start-hook.md) d1, d6) gains one section after its recall,
  "OML skills at `<scope path>`", one line per offered skill, "`<name>`: `<description>` (read
  `oml://workarea/<workarea>/skill/<name>`, or run /oml:`<name>`)", at most 20 lines in d5's
  order, then "and N more" with no partial line. It reads `GET /v1/skills` (d11), one more
  request on the provider ring beside ADR-0029 d2's two, and fails open as the hook does (d5 of
  ADR-0029). The cost is up to 20 lines of context in every session that has skills, whose token
  cost per session is bounded by 20 descriptions of at most 1024 characters and is otherwise
  unmeasured. The index is on by default (the operator, 2026-11-14).

- **d9 — A skill's body is at most 32 KiB, and the offered set is never cut mid-skill.** 32,768
  UTF-8 bytes after NFC is 8,192 budget tokens under `oml-tc/1`
  (`crates/oml-domain/src/budget.rs:25`; ADR-0025 d6), above the format's "< 5000 tokens
  recommended" body, chosen, not measured; a longer body is `request.payload_too_large` naming
  `/body`. The claim limit of 1 MiB (`MAX_BODY_BYTES`, `lib.rs:192`) stays the outer bound for
  every other kind. `prompts/get` and `resources/read` serve a skill whole or refuse it; they never
  truncate an instruction.

- **d10 — No held skill is ever served as an instruction, inside an in-progress workarea
  included.** A held `skill` version stays author-only as a held `steering` or `guardrail` version
  does ([ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
  d3, amended to add `skill` to that list), and neither `prompts/list`, `prompts/get`,
  `resources/list`, `resources/read` nor the hook's index serves one. Its author sees it in the
  console's builder (ADR-0082) and in Submitted (ADR-0083). Recall never serves kind `skill` as a
  hit, inside or out (amends ADR-0025 d8): a skill is offered, not matched, and recall's text is
  data.

- **d11 — One provider-ring read serves the offered set: `GET /v1/skills`.** Under the key at its
  default workarea or the `workarea` query field ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md)
  d7), it answers `{scope_node, scope_path, guardrail_scope, set_digest, skills: [{artifact_id,
  claim_key, name, description, version, workarea_path, depth, overrides, approved_at, approver,
  uri}], next_cursor}`, no bodies, paged as d5. A body is `GET /v1/memories/{artifact_id}`, which
  already serves any readable promoted row. `set_digest` is opaque, as ADR-0037 d5's. The MCP
  methods and the hook are its clients, so the set is computed in one place.

- **d12 — A skill is metered as every claim is, and serving one is a read, not a recall.** A
  skill's versions count in `memories`, `text_bytes` and `writes` under
  [ADR-0071](ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md)
  d2's definitions, with no new meter. `GET /v1/skills`, `prompts/list`, `prompts/get` and a
  skill's `resources/read` do not count in `recalls`, whose definition is recall requests; each is
  an `mcp_call` line and metric (ADR-0042 d10) and passes the key's request rate
  ([ADR-0079](ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md)).

- **d13 — The operator's answers of 2026-11-14.** (1) The session-start index (d8) is on, capped
  at 20 lines. (2) `guardrail_scope` also scopes skills (d4); a separate `skill_scope` waits until a
  deployment asks for it. (3) Agents may propose skills through `oml_write`; a skill stays held and
  only a person's review approves it, never an agent's key (d3).

## Consequences

- `MemoryKind` gains `Skill`; the submission and supersession bodies gain `skill`; the
  `memories` table gains two nullable columns and a check that both are set exactly on kind
  `skill`, in one forward-only migration reserved at build. `openapi.yaml`, `http-api.md` and
  `errors.md` gain the kind, `GET /v1/skills` and the refusals in the change that serves them
  (ADR-0039 d9).
- `mcp.rs` replaces the single-prompt match with the offered set: `prompts/list` pages,
  `prompts/get` resolves a name through d11's read, and `guardrails` stays first and reserved.
  `the_prompt_renders_the_hook_text` (`crates/bins/oml-server-lead/tests/guardrails_http.rs:572`)
  keeps the guardrail prompt's case, and a new test,
  `prompts_get_serves_an_offered_skill_and_refuses_any_other_name_alike`, proves d6.
- `oml hook session-start` gains the index (d8); the plugin's README says what it adds.
- The scan's instruction-kind rules (ADR-0074 d24) name `skill`; the person lane gains the
  reasons `skill_by_agent` and `skill`; review gains `review.person_required` with
  `skill_needs_person` for an agent's or a service's verdict on a skill. `mcp-tools.json`'s
  `oml_write` `kind` enum gains `skill` and its `skill` object, regenerated, not hand-edited.
- `oml doctor` names: a server that does not serve `GET /v1/skills` ("this server predates
  skills"), a key whose scope offers more than one page of skills (informational), and a skill
  name shadowed along the chain (informational, naming both paths).
- Costs, stated: a person running `/oml:<name>` gives that body the weight of their own
  instruction, so a skill approved by a careless reviewer acts with that weight; every version of a
  skill counts its full text in `text_bytes`; a client that caches `prompts/list` for a session
  offers a retired skill until it reconnects, and `prompts/get` then refuses it.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A skill as `kind: memory` with a `text/markdown` body and a naming convention** | Recall would serve it as a hit framed as data, review would apply no instruction-kind scan, and no field could carry the name and description a prompt listing needs. |
| **Skills exported to `.claude/skills/` by the CLI, as guardrails are to `.claude/rules/`** | The operator asked for no local copy; an export is a copy no supersession, demotion or erasure reaches (ADR-0037 d11's stated cost). |
| **Skills as tools** | A tool is model-invoked with typed arguments and runs code; a skill is text. ADR-0039 d2 keeps three tools and admits prompts beside them; a tool per skill would also grow `tools/list` with every write. |
| **Workarea-qualified prompt names (`<workarea>.<name>`) for every skill** | Long slash commands for the common case of one workarea, and a moved scope renames every command. Shadowing by depth keeps short names, and the farther skill stays readable by URI. |
| **Serving held skills inside in-progress workareas, labelled unreviewed** | An unreviewed instruction carries a person's weight when run; ADR-0030 d3 already keeps held steering and guardrails author-only for the same reason. |
| **Recall serves skills as hits** | Recall's text is data and never instructions (`mcp.rs:69-70`); mixing would make a recalled skill read as either. |
| **`listChanged: true` with notifications** | `/mcp` is stateless with no server stream (ADR-0014 d2); a declared capability with no sender is a broken promise. |
| **`oml_write` refuses `skill`, as it refuses `guardrail`** | The operator chose on 2026-11-14 that agents may propose skills; the person-only approval (d3) carries the safety the refusal would have. |
| **Agents' verdicts count on skills where the workarea's policy admits agents** | A skill carries the weight of the person who runs it; an agent approving another agent's instructions would put unreviewed-by-a-person text behind a slash command. |
| **Front matter inside the body as the source of name and description** | Two sources for one field, and a body edit could rename a command without a field change a reviewer sees. |
