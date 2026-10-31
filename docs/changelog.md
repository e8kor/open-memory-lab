<!--
id: changelog
tier: engineering
status: design draft for review · decided, not built
answers: how the design spec is versioned, what 0.1.0-design and 0.2.0-design cover, and what each revision changed
-->

# Spec changelog

**Status:** design draft for review · decided, not built

This page is the revision record of the design specification itself. The current spec version
is **0.2.0-design**, matching `info.version` in the [OpenAPI document](api/openapi.yaml). Until
2026-10-30 no code existed; since the PoC entry below, an entry may also record an
implementation milestone where it changes what the repository holds.

## How this changelog is kept

- **One entry per design PR.** A merged design change adds one dated entry naming the ADRs and
  pages it touched; a change with no entry is a defect.
- **`info.version` bumps on any /v1-surface change.** A change to the provider ring's
  contract — paths, request or response shapes, the error taxonomy, idempotency or pagination
  semantics — bumps the version in `docs/api/openapi.yaml`, and this page records the bump
  alongside the change that forced it.
- **The repo is tagged `spec-vX.Y` at each published revision whose records are accepted**,
  starting with `spec-v0.1` at implementation start, so an implementation can state exactly which
  revision of the design it was built against; a revision carrying a `proposed` record is
  tagged when that record is accepted.

## 0.2.0-design

### 2026-10-31 — A deliverable is the last claim in its chain (ADR-0035, proposed)

[ADR-0035](adr/ADR-0035-a-deliverable-is-the-last-claim-in-its-chain.md), proposed on
2026-10-31, designs how agents store finished work in workareas: a claim of kind `deliverable`
whose files ride `oml_remember` and `oml_supersede` as upload sessions or inline text, are named by
a file manifest, are served as descriptors, and are reviewed like every claim, and where every
file and lake table is stored: one object-store port whose backend one URL setting picks, the
local filesystem by default. Its number follows those that records on other branches hold. It
binds nothing before its acceptance, and no accepted record changes until then. The maintainer
confirmed d2's departure from the brief on 2026-10-31.

- **Proposed, not built:** kind `deliverable` (d1); the file manifest and
  `submission.file_manifest_invalid` (d2); download-only attachment fetch naming its version (d3);
  the two attachment forms and `oml attach put` (d4); descriptors, counter `oml-tc/2` and
  `include_text` (d5); the forms described per key and selected by the harness (d6); several
  deliverables in one workarea (d7); declared lineage and the descendants read (d8); one DEK per
  stored object, the `bound` session state, the object's frame format, its lifecycle and expiry,
  and their copy classes (d9); one `object_store` port for files and the lake, `storage.url` with
  the local filesystem by default, the conformance suite against the filesystem and MinIO, and
  `oml storage copy` (d10); two vocabulary rows (d11); the build order after iteration 3 (d12).
- **Pages touched:** the record, its register row and nav entry, `llms.txt`, and FR-40 to FR-43
  and FR-46 on the roadmap, each marked proposed (FR-44 and FR-45 are used on another branch). No
  contract page changes, so `info.version` stays `0.2.0-design`.
- **Amends, on acceptance:** ADR-0004 (d3, d4), ADR-0005 (d1, d4), ADR-0018 (d4), ADR-0022 (d6),
  ADR-0023 (d3, d8), ADR-0024 (d1) and ADR-0025 (d4, d6, d8, d11, d12, d13, d14); their Status
  lines gain the back-pointer then.

### 2026-10-31 — Every agent session starts from what OML holds for its key: `oml hook session-start` (ADR-0029)

[ADR-0029](adr/ADR-0029-session-start-hook.md), accepted by the maintainer on 2026-10-31 with release v0.2.2, records what the maintainer decided on
2026-10-31, and this change builds it: the `oml` plugin ships a SessionStart hook that runs a Rust
subcommand of the `oml` CLI, on by default and bounded by a token budget. Numbers 0026 and 0027
are taken by records on other branches. It amends no record.

- **Decided and built:** `oml hook session-start`, which sends the own-work listing
  (`own=true`, ten newest) and a recall with `include_own_held` under a budget of 800 budget
  tokens (`OML_SESSION_CONTEXT_MAX_TOKENS`, 1 to 2000), queried with the words of the working
  directory's name (d1–d3); `OML_URL` and `OML_API_KEY` alone, never the stored admin credentials
  or `OML_CLI_KEY`, a key value that is not all printable ASCII refused before anything is sent,
  and the key value redacted from everything it prints (d4); no output and exit
  0 on every failure, `OML_SESSION_CONTEXT=off` included, with a 3-second timeout per request, its
  connect included, an 8-second deadline on the whole verb, and a context printed whole or not at
  all (d5); one SessionStart hook document whose context puts the request to record, and after a
  compaction the request to record the summary's durable facts, right after the header and above
  the line that frames the entries below as stored data, and prints claim keys and the path in at
  most 120 characters, the key redacted before the cut (d6);
  `plugins/oml/hooks/hooks.json`, which runs the verb only when `oml` is on `PATH` (d7); and the
  two `permissions.allow` rules in the plugin README, the five read tools and every tool, with the
  cost of the second, writes the user never sees, stated beside it (d8).
- **Checks:** `crates/bins/oml-cli/tests/cli_e2e.rs` runs the verb against the real lead behind a
  request recorder; `scripts/check_plugin.py` now also fails if `plugins/oml/hooks/hooks.json` is
  missing or does not parse, or if its SessionStart hooks are not exactly one command, without a
  matcher, with a 10-second timeout and the status message `Loading OML memory`, that runs
  `oml hook session-start`, exits 0 with no output when `oml` is not on `PATH`, and exits 0 when
  that `oml` fails.
- **Not verified here:** that Claude Code runs the hook and adds its context to a session; the
  tests run the binary, not Claude Code. The [roadmap](product/roadmap.md) opens FR-33 for it,
  which closes on a recorded session on a named Claude Code version, a compaction run included.
- **No surface change:** no HTTP operation, error code, event type or claim state; `info.version`
  stays `0.2.0-design`, and no package version moves until the release.
- **Pages touched:** [ADR-0029](adr/ADR-0029-session-start-hook.md), the
  [decision register](adr/index.md) (row and the D14 cell), `mkdocs.yml`, [llms.txt](llms.txt),
  [CLI reference](cli/reference.md), [agent plugin](api/mcp-plugin.md), the
  [roadmap](product/roadmap.md) (FR-33) and this page; outside
  `docs/`, `plugins/oml/README.md`, `plugins/oml/hooks/hooks.json` and
  `scripts/check_plugin.py`.

### 2026-10-31 — The agent-only memory surface (implementation)

An implementation milestone; no decision changes. The code now serves what
[ADR-0025](adr/ADR-0025-the-agent-only-memory-surface.md), accepted on 2026-10-31 with this build,
decides for FR-10 and FR-11:

- **Eight of the twelve tools.** The [agent plugin](api/mcp-plugin.md)'s catalog holds twelve
  tools, and `tools/list` serves eight: `oml_remember`, `oml_learn`, `oml_recall`, `oml_get`,
  `oml_supersede`, `oml_my_work`, `oml_workarea_info` and `oml_events_tail`. `oml_attach`,
  `oml_relate_propose`, `oml_graph` and `oml_vocab` are named and answer not built (d12).
- **One key.** Grant admissibility per key role, the four-step refusal order and effective access
  computed on every verb, so a writer-role key with `wa:writer` and `wa:reader` grants remembers
  and recalls (d1); issuance that carries grants and denies and discloses the value only after
  the last `grant.issued` commits, and `POST /v2/keys/{key_id}/denials` (d2); "own" as the
  authoring key (d3).
- **Reading back, budgets and order.** `include_own_held` on recall (d4); the budget token under
  counter `oml-tc/1` and the `max_tokens` cut with its `budget` object (d6, d7); applicable
  steering ahead of the hits and a total hit order (d8); the ids served and the whole-request
  digest that the read log will record, computed on every recall (d9).
- **Versions.** Each claim version stores its ordinal, kind, NFC `body_bytes` and the version it
  replaces, and a never-promoted rejection releases its `claim_key` (d6, d18); the tip moves only
  at ratification, as one `lifecycle.supersede`, the author's retire-proposal is a supersession
  carrying `retirement: true`, a supersede replays from its stored receipt, and a dreamer's edit
  is a proposal (d10, d11). Migration `0005_claim_versions.sql` removes the PoC's `superseded`
  status.
- **Listings.** The own-work listing on `GET /v1/memories` (d13), the workarea read on
  `GET /v2/workareas/{workarea_id}` (d14), and the events tail's per-family visibility with a
  bounded scan answering `next_after_seq` (d15).
- **The one-key plugin.** `plugins/oml/.mcp.json` declares the `oml` server alone, and
  `scripts/check_plugin.py` fails if a second server appears; `OML_RECALL_KEY` is gone.
  `scripts/quickstart.sh` issues one agent key and a dreamer key; it remembers, lists its own work
  and recalls its own held row on the agent key, records an approving verdict with the dreamer
  key, and recalls the promoted row on the agent key.
- **The scenario.** `crates/bins/oml-server-lead/tests/agent_only_memory.rs` runs in
  `cargo test --workspace`. No CI run has carried it yet, so FR-10 and FR-11 stay open on the
  [roadmap](product/roadmap.md) until one passes; their rows then cite that run (ADR-0018 d3).
- **The multi-arch image** is recorded in the release-image entry below.
- **Bounds the ring already enforced, now declared.** Within 0.2.0-design,
  `RecallRequest.max_tokens` and the budget object's echo of it declare a maximum of 2⁵³ − 1
  ([retrieval](architecture/retrieval.md#token-budgets), d7, d9), and in
  [mcp-tools.json](api/mcp-tools.json) `oml_recall`'s `max_tokens` and `oml_my_work`'s `limit`
  declare the ring's maxima, 2⁵³ − 1 and 200. The served `oml_recall` entry now describes
  full-text search, and `_meta.pending_design` names the hybrid search mode and the read log that
  it does not serve yet (d12).
- **Still unbuilt**, each stated in the README or a code comment: the designed `held_fts` (own
  held recall reads through the author-predicated `recall_own_held` port method instead, d5); the
  read log (d9); held-item expiry (`held_expires_at` is null); workarea edges (`edges` is `[]`);
  grant add and revoke, and denial lift (d2); issuance idempotency; key rotation (`rotated_from`
  is null) and expiry enforcement (an expiry on issuance is refused with `request.validation`,
  d16); `api.cursor_ttl`, held as the code constant `CURSOR_TTL_SECS`, 3600 s, until settings
  are built; the operator tier; the `in_progress` idempotency claim
  ([http-api.md](api/http-api.md#idempotency)): the record is written in the commit's own
  transaction, so a concurrent identical request answers the first attempt's receipt as a replay,
  or, for a submit that read the `claim_key` after that attempt committed,
  `submission.claim_key_conflict`, and never `idempotency.in_progress` (`IdempotencyPut` in
  `crates/oml-app/src/ports.rs`); the chain export (`POST /v2/exports` with `kind: "chain"`,
  [http-api.md](api/http-api.md#evidence-export)), and with it live `oml audit verify-chain`,
  which refuses a live run by name since d15 made the events tail a filtered feed, not a
  verification surface — the evidence export of M4 on the
  [critical path](product/critical-path.md) owns it; and set-wise hydration of served rows: each
  served row's readable `derives_from` ancestors, its `replaces` target and its workarea path are
  read one row at a time, against ADR-0024 d4, at a cost that is unmeasured (FR-7, FR-25).

### 2026-10-30 — `/readyz` names the build that answered (implementation)

A page edit; no decision changes. [Health](api/http-api.md#health) now declares a `build` object
on every `/readyz` response, ready or not: version, commit, commit time, build time, compiler,
target and profile, each `unknown` when nothing could supply it. The `Ready` schema in
[openapi.yaml](api/openapi.yaml) gains the `Build` schema. The image build has no git metadata,
so the release workflow passes the commit and its time as build arguments, and
`scripts/check_release.py` refuses a release workflow that does not. `SOURCE_DATE_EPOCH`, when
set, fixes the reported build time so a pinned build stays reproducible. The stated cost: an
unauthenticated probe now discloses the exact build.

### 2026-10-30 — The key tool: `oml keys issue-agent`, the default reader grant, the `oml-admin` plugin (ADR-0028)

[ADR-0028](adr/ADR-0028-agent-key-issuance.md), accepted by the maintainer on 2026-10-31 with
release v0.2.1, decides, and this change builds, the
key tool the maintainer chose to ship in v0.2.1, before names. Its three decisions were drafted as
d12–d14 of the proposed ADR-0026 (names are keys), which with the proposed ADR-0027 sits on its
own branch; this record takes the next number after both. It amends ADR-0015 (d1), whose Status line
carries the back-pointer.

- **Decided and built:** `oml keys issue-agent --principal <principal> --workarea <workarea-id>`,
  one `POST /v2/keys` for a writer key with writer and reader grants on the workarea and that
  workarea as its default, printing exactly the `OML_URL` and `OML_API_KEY` export lines; with
  `--out <path>`, the value written once to a new 0600 file created before anything is sent, an
  existing path, a path with a quote or a newline, and `--out` with `--json` refused; an issuance
  that gets no usable answer once it may have reached the server, which the CLI reports as a key
  that may have been issued, never as a refusal, and without echoing the body; the reader
  grant `oml keys issue` and `issue-agent` add beside every writer grant unless `--write-only`,
  carrying the writer grant's deny masks (neither verb takes a mask yet, so none is sent),
  while HTTP issuance still takes exactly the grants it is sent (ADR-0025 d2); the `oml-admin`
  Claude Code plugin, one skill that runs `issue-agent` only with `--out` and never with `--json`,
  and no MCP server.
- **Checks:** `crates/bins/oml-cli/tests/cli_e2e.rs` drives the verbs against the real lead behind
  a request recorder, and against stubs that cut the `201` short or close the connection
  unanswered; `scripts/check_plugin.py` now also fails if the marketplace stops listing
  `oml-admin` with a manifest, if `oml-admin` gains an `.mcp.json`, or if its skill runs
  `issue-agent` without `--out` or with `--json`.
- **Not built:** the positional key name (ADR-0026 d1), key revocation, which a write failure
  after issuance names as the remedy, and `oml context show` and `oml keys show`, which the skill
  would otherwise check with.
- **No surface change:** no HTTP operation, error code, event type or claim state; `info.version`
  stays `0.2.0-design`, and no package version moves until the release.
- **Pages touched:** [ADR-0028](adr/ADR-0028-agent-key-issuance.md), the
  [decision register](adr/index.md) (row and the D14 cell), `mkdocs.yml`, [llms.txt](llms.txt),
  [CLI reference](cli/reference.md), [roles and keys](architecture/roles-and-keys.md) (the key
  format rule names the `--out` file) and this page; outside `docs/`,
  `.claude-plugin/marketplace.json`, `plugins/oml-admin/`, `plugins/oml/README.md` and
  `scripts/check_plugin.py`.

### 2026-10-30 — The release image for linux/amd64 and linux/arm64 (implementation)

An implementation milestone; no decision changes. `.github/workflows/release.yml` now builds the
image as [CI and toolchain](engineering/ci-and-toolchain.md) decides: one leg per platform on a
native runner (`ubuntu-24.04`, `ubuntu-24.04-arm`), each pushed by digest with its own SBOM and
provenance, then a merge job that assembles one index with `docker buildx imagetools create`,
asserts both platforms are in it, and signs the index digest with `cosign sign --recursive`. The
binary tarballs stay `x86_64-linux`, and the release notes say so. `ci.yml` gains an `image` job
that builds the image for both platforms on every pull request and runs its `oml --version` on
each, and `scripts/check_release.py` fails the pull request if `release.yml` stops building both
platforms, assembling the index or signing its digest. No tag has run the new pipeline yet; the
`v0.1.0` image is `linux/amd64` alone.

### 2026-10-30 — the agent-only memory surface (ADR-0025)

[ADR-0025](adr/ADR-0025-the-agent-only-memory-surface.md) decides the surface FR-10 and FR-11
need and the access model it rests on. It amends ADR-0001 (d5), ADR-0008 (d4, d6), ADR-0010
(d1, d3), ADR-0012 (d7), ADR-0013 (d1, d2, d5), ADR-0014 (d2), ADR-0018 (d4) and ADR-0022
(d4, d6), and each of those records' Status lines carries the back-pointer. Proposed on
2026-10-30; accepted by the maintainer on 2026-10-31, with the build that implements it, and
tagged `spec-v0.2`.

- **Decided:** grant admissibility per key role and one four-step refusal order, so one
  writer-role key with writer and reader grants remembers and recalls; issuance that carries
  grants and denies and discloses the value only after the last grant commits; "own" as the
  authoring key; `include_own_held` on recall, with held text matched only through an
  author-fenced index; the budget token (counter `oml-tc/1`) and the `max_tokens` cut with a
  `budget` object; applicable steering and a total hit order; a recall logs the ids served and a
  digest of the whole request; supersession moves the tip only at ratification, by a
  compare-and-set bound to the successor's workarea and `claim_key`, as one `lifecycle.supersede`,
  with the authors of rival held successors kept off each other's review; the author's
  retire-proposal as a supersession carrying `retirement: true`, open to the same keys as any
  supersession; distinct version ordinals, so competing held successors coexist and the tip
  compare-and-set decides between them; own read on the target version; a self-signed version
  never replacing a reviewed tip (d17); the `claim_key` release after a never-promoted rejection
  and the refusal of a reopen that would collide (d18); `simulation.write` journalling
  `body_bytes`, `replaces` and `retirement` like every submission event; own simulation rows read
  only inside the simulation's audience; `oml_get` and `oml_supersede` and one claim-item shape;
  the own-work listing across every workarea the key wrote to; the workarea read with the caller's
  children, effective verbs and permission epoch; per-family visibility on the events tail with a
  bounded scan and `next_after_seq`, where a `lifecycle.*` event reaches readers only while the
  version's latest move between held and promoted is a promotion, reaches its author in any case,
  and names no reviewer, admin or claim workarea the key holds nothing on; a request digest over
  method, route, path parameters and body, with supersede's idempotency record scoped to the key
  alone, looked up before the target's readability so a committed supersession always replays, and
  claimed only once the target is known readable, every target-dependent refusal ordered after
  that; a `held_by: service` key refused every review-class verb; `derives_from` and `replaces`
  served only as far as the caller can read; a successor's kind changing only within its class,
  and nothing but its kind inherited from the target; a row-count check on every `held_fts`
  removal; effective access computed on every request with no cache, so a revocation applies to
  the key's next request served by the lead, and on follower-served reads after replication lag
  (proposing to amend ADR-0008 d4 and d6); request bodies that refuse unknown fields.
- **`info.version` 0.1.0-design → 0.2.0-design**, because `/v1` request and response shapes
  change. No operation, error code, event type or claim state is added: the taxonomy and the
  operation set are unchanged. The MCP catalog goes from ten tools to twelve.
- **Roadmap:** FR-30 (the budget-token count against model tokenizers, unmeasured), FR-31 (the
  `claim_key` collision, the version ordinal, the tenant-wide journal positions and the lineage
  read's redacted ids as residual existence and count signals), FR-32 (a service key's grant
  confers every verb of its role, not only the one act its job needs) and RS-17 (revising one's
  own claim before its review) are opened. FR-10's exit criterion is reworded to the
  retire-proposal ADR-0025 d11 decides, a supersession of the claim rather than a verb of its
  own. FR-10 and FR-11 stay open until the agent-only-memory scenario passes in CI.
- **The release image** is decided as one index for `linux/amd64` and `linux/arm64`, signed on the
  index digest; a page edit, no decision change.
- **Pages touched:** the [decision register](adr/index.md) (register row; D4, D6, D7, D9, D11,
  D12, D13 and D14 cells), `mkdocs.yml`, [llms.txt](llms.txt), [openapi.yaml](api/openapi.yaml),
  [HTTP API](api/http-api.md), [errors](api/errors.md), [the agent plugin](api/mcp-plugin.md),
  [mcp-tools.json](api/mcp-tools.json) (the two new tools, `oml_my_work`, `oml_workarea_info` and
  `_meta`, with `_meta.pending_design` naming what the served `oml_recall` and `oml_events_tail`
  entries still lack; those entries gain only what the build already serves — the `k` maximum,
  the `after_seq` default, and no `scope` on the tail),
  [retrieval](architecture/retrieval.md), [roles and keys](architecture/roles-and-keys.md),
  [workareas](architecture/workareas.md), [admission](architecture/admission.md),
  [audit and lineage](architecture/audit-and-lineage.md),
  [event catalog](data/event-catalog.md), [Postgres schema](data/postgres-schema.md),
  [roadmap](product/roadmap.md), [CLI reference](cli/reference.md),
  [CI and toolchain](engineering/ci-and-toolchain.md), [upgrades](operations/upgrades.md),
  [security](architecture/security.md), [configuration](operations/configuration.md)
  (`auth.epoch_cache_bound` removed with the effective-access cache it bounded, which ADR-0025
  rejects),
  [commitments](product/commitments.md), [compliance](product/compliance.md),
  [concepts](concepts.md), [the overview](index.md) and this page; outside `docs/`, `CONTRIBUTING.md`, `README.md` and
  `plugins/oml/README.md` (version and tool counts).
- **Not built.** Nothing above is implemented by this entry; the PoC's divergences are named in
  ADR-0025's Context, and the code lands in later changes, each with its own entry. A
  `spec-v0.2` tag follows the maintainer's acceptance.

## 0.1.0-design

Reconstructed from the repository history; all dates 2026-10-29. The set was written and
revised in one sustained effort, so the entries below group commits by what they decided
rather than pretending a longer calendar.

### 2026-10-30 — The agent plugin's first tools, CI and the release pipeline (implementation)

An implementation milestone; no decision changes. The [agent plugin](api/mcp-plugin.md) is served
at `/mcp` for four of its ten tools — `oml_remember`, `oml_learn`, `oml_recall`,
`oml_events_tail` — as the stateless facade [ADR-0014](adr/ADR-0014-http-api-and-mcp-facade.md)
decides, each tool running its ring endpoint's own handler. The pinned MCP revision is
**2025-06-18**, checked against modelcontextprotocol.io (basic/transports, server/tools) on
2026-10-30. The remaining six tools answer as not built. A PoC key carries one role, so the Claude
Code plugin under `plugins/oml` connects twice, with a writer key and a reader key; the
single-key shape waits on grants of several roles per key. The merge gates and release artifacts
of [CI and toolchain](engineering/ci-and-toolchain.md) now run, except the MSRV build, the sqlx
offline check, the container tests against MinIO, the scheduled `cargo audit` and the bench job.
The docker profile's environment names were corrected to the ones
[configuration](operations/configuration.md) declares.

### 2026-10-30 — Scan-veto journalling decided; a transition precondition added

Two design points the Live Atlas review rounds forced into the open, now decided:

- **A scan veto journals nothing.** `submission.scan_veto` is a refusal like any other refusal:
  deterministic, reproducible from the journalled rule-set pin, and never a write into the
  signed chain (hostile input must not grow it). The wording is now uniform across
  [admission](architecture/admission.md), the
  [event catalog](data/event-catalog.md#what-is-deliberately-not-an-event),
  [errors](api/errors.md), [concepts](concepts.md), the [HTTP API](api/http-api.md),
  ADR-0012 (amended) and ADR-0020; whether refused submissions deserve a durable trace outside
  the chain is RS-16 on the [roadmap](product/roadmap.md).
- **`committed` → `validated` is refused while a simulation is fenced on the subtree** — the
  simulation must be ended (journalled) first, since taint can never enter a `validated` area
  ([workareas](architecture/workareas.md), transition matrix).

### 2026-10-30 — Live Atlas visualization

- The published site gains the [Live Atlas](atlas/index.html): an interactive 3D visualization
  of the crew-harness story — one crew per workarea, recall pulses pulled from production
  areas, remember pulses into the crew's own area, supersession with pulled evidence, ladder
  transitions, and the integration lattice between production areas — with drill-in modes for
  the admission reactor and the chain ledger. All data is mocked client-side in the shapes of
  the [event catalog](data/event-catalog.md); three.js is vendored (MIT), nothing loads from a
  CDN at runtime. Replaying a real PoC `/v2/events` tail is a planned data-source swap.

### 2026-10-30 — The PoC lands (M1 slice)

- The critical path's M1 slice is implemented in this repository (commit `54dadfd`): the Cargo
  workspace of [workspace-plan](engineering/workspace-plan.md), the signed chain reproducing
  [chain-format](data/chain-format.md)'s test vectors byte-for-byte, the one-transaction
  commit point, the standalone-profile server with startup fork check, and an independent CLI
  chain verifier — demonstrated end to end by `scripts/poc-demo.sh`. The spec remains the
  authority; where the PoC diverges it says so in code comments and the README.

### 2026-10-29 — Documentation finalization

A coordinated finalization pass over the whole set, encoding decisions taken for
implementation readiness:

- The [decision register](adr/index.md) gains the D1–D17 ownership table (which page declares
  which decision handle), the ADR status lifecycle, and an [ADR template](adr/TEMPLATE.md);
  this changelog and its going-forward rules are introduced.
- The closed error taxonomy grows 45 → 46 with `request.unsupported_media` (HTTP 415, family
  `request`, not retryable), carried on all three synced surfaces
  ([errors](api/errors.md), [openapi.yaml](api/openapi.yaml),
  [HTTP API](api/http-api.md)).
- The chain signing format is pinned: `signed_bytes` is RFC 8785 (JCS) canonical JSON of the
  enumerated envelope fields, `hash` is SHA-256 over them, `sig` is pure Ed25519 (RFC 8032)
  over `signed_bytes`; genesis is `prev_hash` = 32 zero bytes with `tenant.genesis` at seq 1
  ([chain format](data/chain-format.md)).
- The API key wire format is fixed (`oml_` prefix + 43 base62 characters, 256-bit random,
  stored as SHA-256 of the full key), and key-lifecycle journal events for signing-key and
  KEK rotation are added to the [event catalog](data/event-catalog.md).
- Operational commitments are pinned: forward-only `sqlx` migrations run by the bootstrap
  workload, a 24-hour committed floor on the idempotency replay window, positional pagination
  cursors with a named TTL setting, `simple` as the v1 FTS configuration with NFC
  normalization at admission, and one [runbooks page](operations/runbooks.md) with one
  anchored section per condition.
- Glossary entries for steering, the overlay tip and the fold join
  [Concepts](concepts.md).
- FR-26 (retention + legal hold), FR-27 (tenant offboarding ceremony verbs), FR-28 (operator
  verbs: dead-letter re-enqueue, erase replay, standalone migrate), FR-29 (attachment/bundle
  Range support) and RS-15 (per-tenant FTS configuration scope) added to the
  [roadmap](product/roadmap.md).

### 2026-10-29 — Install profiles, cluster mode, performance foundation

[ADR-0023](adr/ADR-0023-install-profiles-and-cluster-mode.md) (three install profiles, one
conformance bar, lead/follower cluster mode) and
[ADR-0024](adr/ADR-0024-performance-foundation.md) (hot-path discipline, the DEK serving
cache as a copy class, batch submit, latency targets as FR-25 rows).

### 2026-10-29 — The two-ring API

[ADR-0022](adr/ADR-0022-two-ring-api.md): one keyed listener, the /v1 provider ring under a
compatibility covenant and the /v2 lab ring, the token/header/body attribute-placement rule;
the [HTTP API](api/http-api.md) restructured around the rings.

### 2026-10-29 — AI+Human ontologies

[ADR-0021](adr/ADR-0021-ai-human-ontologies.md): vocabularies as governed claims — terms with
permanent CURIEs, releases as journalled pins, SSSOM mappings, the vocab scanner
([ontologies](architecture/ontologies.md)).

### 2026-10-29 — Critical review and dataset onboarding

The commitments register ([commitments](product/commitments.md)), the provider floor, dataset
onboarding ([ADR-0020](adr/ADR-0020-dataset-onboarding.md)), and the critical path
([critical path](product/critical-path.md)). Separately, the FR-9 market-claim
re-verification was refreshed with a 2026-10-29 fetch date.

### 2026-10-29 — Review-blocker resolution

All review blockers from the first read-through resolved; the [Concepts](concepts.md) page
and [ADR-0019](adr/ADR-0019-hexagonal-architecture-and-plugins.md) (hexagonal architecture
and plugins) added, the hexagonal-architecture presentation corrected, Mermaid sequence
diagrams fixed (semicolons escaped as `#59;`), and accidentally committed session telemetry
removed from the working tree and ignored (commit 19b4ee6); it remains in the repository's
earliest history pending a maintainer decision on a rewrite.

### 2026-10-29 — Initial design set

The founding commit: ADR-0001 through ADR-0018, the architecture, data, API, CLI and
operations pages, the [OpenAPI document](api/openapi.yaml), the [roadmap](product/roadmap.md)
as honesty ledger, and the GitHub Pages site built with `mkdocs --strict`.
