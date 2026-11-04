<!--
id: changelog
tier: engineering
status: design draft for review · decided, not built
answers: how the design spec is versioned, what 0.1.0-design, 0.2.0-design, 0.3.0-design and 0.4.0-design cover, what each revision changed, and the breaks of 0.4.0 with their migrations
-->

# Spec changelog

**Status:** design draft for review · decided, not built

This page is the revision record of the design specification itself. The current spec version
is **0.4.0-design**, matching `info.version` in the [OpenAPI document](api/openapi.yaml). Until
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

## 0.4.0-design

Opened on 2026-11-03 for the rewrite [ADR-0039](adr/ADR-0039-the-api-rewritten-for-v0-4-0.md)
records. `info.version` moved to `0.4.0-design` with the contract split below; entries landed
here as the build did. ADR-0031's own move to `0.4.0-design` is the same bump, so it lands in
this section rather than a second one. Release v0.4.0 of 2026-11-04 carries this revision.

### 2026-11-03 — The status ladder (implementation)

An implementation milestone; no decision changes. The code now builds the accepted
[ADR-0009](adr/ADR-0009-status-ladder.md)'s ladder in the domain, the use cases and the Postgres
store. `info.version` stays `0.3.0-design`.

- **The ladder** (d1, d3, d4). A workarea's status is one stored column; an admin, or a dreamer
  with dreamer access on the node, proposes one rung forward as a pending `workarea_transitions`
  row and one `workarea.status_proposed`; a second identity ratifies it as one
  `workarea.status_transition` carrying the policy snapshot, and the node takes its `to` in that
  commit. The ratifying transaction re-checks the proposal and the children under the chain lock,
  so a child created concurrently with a ratification to `validated` leaves either the child or
  the validated node, never both. Refusals are `workarea.transition_refused` with a reason and
  `review.ratifier_is_proposer` ([errors](api/errors.md)); the readings are on
  [workareas](architecture/workareas.md#readings-the-build-takes).
- **The freeze** (d5). A `validated` workarea refuses a submit, a supersede, an approval and a
  child create as `workarea.frozen`, while its crew record stays correctable.
- **Not built:** reassignment, re-parent, demote, erasure and the simulation fence; no HTTP route
  or MCP tool serves the transitions, since the proposed ADR-0039 rewrites both. The Postgres
  end-to-end laws run only where a superuser mints the suites' role pairs, CI's
  `cargo test (Postgres)`.
- **Pages touched:** [workareas](architecture/workareas.md),
  [postgres-schema](data/postgres-schema.md) and this page.

### 2026-11-04 — ADR-0043 proposed: the maintainer's public server as the default, planned for v0.4.1

[ADR-0043](adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md), proposed:
the plugins and agent kit OML releases default to `https://memory.doublegate.io`, the maintainer's
server since its move from `oml.doublegate.io` that day, so an agent needs only its key. The
plugin's `url` option gains that default and is no longer required, a plugin key set alone reaches
it in every hook, `oml setup claude-code` pairs a key named without a URL with it, and every kit
configuration names it; every other CLI verb still requires a URL. No `/v1` contract changes, so
`info.version` stays `0.4.0-design`.

- **Amended records:** ADR-0039 d15, ADR-0040 d1 and d3 and ADR-0041 d7 gain `amended by
  ADR-0043 (proposed)`; the D14 row names it.
- **Pages:** the plugin and kit READMEs, the README quick start, [get started](start/index.md),
  the [Claude Code](guides/claude-code.md), [other tools](guides/other-harnesses.md) and
  [troubleshooting](guides/troubleshooting.md) guides, [mcp-plugin.md](api/mcp-plugin.md), the
  [CLI reference](cli/reference.md) and [releases](product/releases.md).

### 2026-11-04 — Release v0.4.0: ADR-0031, ADR-0038, ADR-0039, ADR-0040 and ADR-0041 accepted

The maintainer accepted [ADR-0031](adr/ADR-0031-oauth-connections-by-connect-code.md),
[ADR-0038](adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md),
[ADR-0039](adr/ADR-0039-the-api-rewritten-for-v0-4-0.md),
[ADR-0040](adr/ADR-0040-plugin-setup-in-claude-code.md) and
[ADR-0041](adr/ADR-0041-a-quick-start-in-one-command-per-role.md) on 2026-11-04 with release
v0.4.0: three agent tools, one URL and one key to start, claude.ai and mobile by connect code,
the plugin set up in Claude Code, and a one-line install. Each record's Status becomes accepted
design, and its Release line names v0.4.0.

- **Amended records:** ADR-0010, ADR-0014, ADR-0015, ADR-0018, ADR-0022, ADR-0025, ADR-0028 and
  ADR-0029 gain their "amended by" pointers to the accepted records that amend them.
- **Rewordings ADR-0031 owes:** a claim↔claim or workarea↔workarea link is a "connection edge"
  in ADR-0010 d5, ADR-0012's context and ADR-0025's d1 rejected alternative; ADR-0025's agent-facing
  results say the plugin "needs one key"; and the "token" that named a key in ADR-0022 and
  ADR-0025 d13, d14 and d13's rejected alternative is now "key". RS-2 leaves the
  [roadmap](product/roadmap.md), its exit criterion met, and [commitments](product/commitments.md)
  lists FR-34, FR-35 and FR-36 in its place.
- **Not done here:** the `spec-v0.4` tag of ADR-0031 d15 is cut with the release tag, not by this
  change.
- **Corrected:** [upgrades](operations/upgrades.md) and the [upgrading guide](guides/upgrading.md)
  said v0.4.0 adds no migration; it adds `0012_connections` and `0013_oauth_clients`, which the
  lead runs at start under the owner URL, and the upgrade gains the optional step that turns
  connections on.
- **Pages touched:** the five records, the amended records, the [register](adr/index.md),
  [releases](product/releases.md), the roadmap, commitments, `llms.txt`, the pages citing RS-2,
  every page whose release marker said planned or built for v0.4.0, and this page.
- **No surface change:** `info.version` stays `0.4.0-design`.

### 2026-11-04 — The v0.4.0 documentation: Get started, install, five guides, eight quick scenarios beside the use-case catalogue, and every front page rewritten for the three tools and the one pair (ADR-0018, ADR-0041)

A documentation change carrying out the [v0.4.0 documentation plan](engineering/docs-upgrade-v0.4.0.md)
steps 1 to 5. The README marks each unbuilt capability as planned with its release, or not
scheduled, and its Quickstart is the installer at `v0.4.0`, `oml login` and `oml setup
claude-code`. New: [Get started](start/index.md), [install](start/install.md), and the guides for
[Claude Code](guides/claude-code.md), [claude.ai and mobile](guides/claude-ai-and-mobile.md),
[other agent tools](guides/other-harnesses.md), [self-hosting](guides/self-hosting.md) and
[upgrading from v0.3](guides/upgrading.md). Eight [quick scenarios](product/scenarios.md) on
the three tools sit beside the [use cases](product/use-cases.md) catalogue from PR #26, whose
statuses read v0.3.1 as released and v0.4.0 as built, and the
[use-case call map](engineering/use-case-call-map.md) names the calls v0.4.0 serves for each.

- **Checks:** `check_release.py` fails an installer URL on a reader page at a tag other than the
  workspace version's or `vX.Y.Z`, and a README Quickstart other than those three lines;
  `check_contract_sync.py` reads `docs/start/`, `docs/guides/`, the use cases and the quick
  scenarios as start pages.
- **No surface change:** `info.version` stays `0.4.0-design`.
- **Unmeasured:** every time named is ADR-0041 d10's target, FR-68.

### 2026-11-04 — `oml keys issue-agent --out` prints the `oml setup claude-code --key-file` line after the configure line (ADR-0041 d7, implementation)

With `--out` the verb prints a fourth stdout line,
`oml setup claude-code --url '<server>' --key-file '<file>'`, which installs the plugin at the
CLI's release and configures it from the same file, refusing an admin key. The `jq | claude
plugin configure` line stays for a machine without the `oml` CLI. The `oml-admin` skill hands the
operator the new line first.

- **Pages touched:** [CLI reference](cli/reference.md#oml-keys-issuance-revocation-rotation)
  (the output, the `oml-admin` plugin), the `oml-admin` skill and this page.
- **Tests:** `the_setup_line_runs_setup_claude_code_with_the_url_and_the_key_file`, and
  `issue_agent_out_prints_the_plugin_configure_line_and_the_setup_line_naming_the_key_file_and_never_the_key`.

### 2026-11-04 — `oml setup claude-code` runs again over a marketplace already added, and each quickstart is a compose project of its own (ADR-0041 d7, d9, implementation)

`oml setup claude-code` now reads `claude plugin marketplace list --json` before it adds the
marketplace. Claude Code 2.1.312 refuses an add whose source differs from the one it holds, so a
v0.3.x user whose marketplace is pinned at `#v0.3.1` got a failure partway, or the 0.3 plugin.
At the CLI's tag it adds nothing. From the same repository at another tag it removes the
marketplace, adds it at the CLI's tag and installs again each plugin the remove uninstalled.
From another source it refuses as `setup.marketplace`. `oml quickstart` names its compose
project from the directory's absolute path, so two stacks in two `oml` directories no longer
share containers and volumes. ADR-0041's d9 row no longer reads as if the compose file had run:
it has not run under Docker yet.

- **Pages touched:** [ADR-0041](adr/ADR-0041-a-quick-start-in-one-command-per-role.md) (d7 step
  4, d9 step 2, and both status rows),
  [CLI reference](cli/reference.md#oml-setup-claude-code-and-oml-quickstart-the-quick-start)
  and this page.

### 2026-11-04 — The workspace, the chart, the plugins and the installers' default name 0.4.0 (ADR-0041 d3, implementation)

The installers install the release they are cut with (d3), and `check_release.py`,
`check_plugin.py` and `check_chart.py` hold that release equal to the workspace version, so the
workspace, `Chart.yaml`'s `version` and `appVersion`, both `plugin.json` files and the two
installers' defaults move from 0.3.1 to 0.4.0 together. `oml setup claude-code` installs the
plugin from the marketplace at the CLI's own tag, so a 0.4.0 CLI installs the 0.4.0 plugin, the
first with the `url` and `api_key` options. No tag is cut and nothing is released: until v0.4.0
is tagged, an install without `--version` fails naming the 404, and `install-smoke` keeps
installing 0.3.1, the newest published release.

- **Pages touched:** [ADR-0041](adr/ADR-0041-a-quick-start-in-one-command-per-role.md) (d3's
  status) and this page.

### 2026-11-04 — The quick start's two verbs: `oml setup claude-code` and `oml quickstart` (ADR-0041 d7, d9, implementation)

The quick start ([ADR-0041](adr/ADR-0041-a-quick-start-in-one-command-per-role.md), proposed)
is merged into this branch, and two of its decided verbs are built. No decision changes, no HTTP
surface moves, and the record stays proposed; its Consequences now give each item's status.

- **`oml setup claude-code`** (d7) refuses an `admin` or `operator` key as `setup.admin_key` after
  `GET /v2/whoami`, and a Claude Code older than 2.1.285 as `setup.claude_code_version`. It then
  installs the plugin from the marketplace at the CLI's own tag and writes the URL and key to
  `claude plugin configure --values-stdin`, never to argv. `--key-file` reads the file
  `issue-agent --out` wrote. `--print`, or no `claude` on `PATH`, prints the commands instead.
- **`oml quickstart`** (d9) writes a compose file embedded in the binary, its image pinned to the
  CLI's version, and an owner-only `.env` of generated passwords for the superuser and the
  owner and serving roles. It runs `docker compose up --detach --wait` on `127.0.0.1`, claims the
  token the lead logged, and issues an agent key and a reviewer key into owner-only files. It
  checks a write end to end, then prints the agent's key file, the `oml setup claude-code` line
  and the `oml login` line. A directory a quickstart wrote into is refused as `quickstart.exists`.
- **Not built:** `install.sh --quickstart`. `scripts/quickstart.sh` stays for the `e2e` and
  `chart` jobs.
- **The checks.** `oml-cli`'s `setup_claude_code` suite runs the verb against a fake `claude`, and
  its `quickstart` suite against a fake `docker` and the real lead. CI's `compose` job runs `oml
  quickstart` against the image built from the checkout, then a second run that must be refused.
- **Pages touched:** [ADR-0041](adr/ADR-0041-a-quick-start-in-one-command-per-role.md) (the
  status of each item), [CLI reference](cli/reference.md#oml-setup-claude-code-and-oml-quickstart-the-quick-start),
  [CI and toolchain](engineering/ci-and-toolchain.md), the
  [v0.4.0 documentation plan](engineering/docs-upgrade-v0.4.0.md) and this page.

### 2026-11-04 — Connections on the rewritten surface: `/mcp` takes an access token as its key, and `/v2` mints and revokes (ADR-0031, ADR-0039 d9, implementation)

No decision changes, and ADR-0031 stays proposed.

- **`/mcp`** accepts a connection's access token beside a key value. The facade resolves the token
  once per request to its live connection's key and runs every tool under that key, so a token
  is never more than its key and never reaches a ring route as a credential (d10, d11). A `401`
  carries `WWW-Authenticate: Bearer resource_metadata="<issuer>/.well-known/oauth-protected-resource/mcp", scope="mcp"`,
  with `error="invalid_token"` when a credential failed, and `Bearer realm="oml"` without
  `oauth.issuer` (d2).
- **Served** in [openapi.yaml](api/openapi.yaml), moved from the annex, which now holds 45
  operations: `POST /v2/keys/{key}/connect-codes` and
  `POST /v2/keys/{key}/connections/{connection_id}/revoke`; `connect_code` on `POST /v2/keys`, its
  `201` carrying `connect_code` in the value's place; `credential` and `connections` on
  `GET /v2/keys/{key}`. A connection's liveness, client name and times stay designed.
- **The CLI:** `oml keys issue-agent --connect-code` now reaches the route; `oml keys connect-code`
  and `oml keys disconnect` are added, and `oml keys show` lists a key's connections. The key value
  still goes only to `--out`.
- **Doctor** gains `oauth_discovery`: `off`, `ok`, or `failing` with the URL, from both discovery
  documents fetched at the issuer the lead names on `/readyz`, which gains `oauth_issuer`, so a
  proxy that answers `404` for the discovery paths reads as `failing`, never `off`.
- **The check.** `oml-cli`'s `connect_flow` suite runs the flow over HTTP against the lead on a TCP
  port: the claim, the CLI's issuance, a registration, the page, the exchange, `/mcp` under the
  access token, the refresh token refused there, the token refused on `/v1`, the disconnect and a
  fresh code. It runs where a superuser mints the role pair, as CI does.
- **Docs:** [deployment](operations/deployment.md#connections-behind-a-reverse-proxy) states
  `oauth.issuer=https://<public origin>` and what the proxy passes through on the same origin.
- **SDKs** regenerate from openapi.yaml.
- **Pages touched:** [HTTP API](api/http-api.md), [errors](api/errors.md),
  [agent plugin](api/mcp-plugin.md), [roles and keys](architecture/roles-and-keys.md),
  [CLI reference](cli/reference.md), [configuration](operations/configuration.md),
  [deployment](operations/deployment.md), [observability](operations/observability.md), the
  [roadmap](product/roadmap.md) (FR-34, FR-35), `README.md` and this page.

### 2026-11-04 — Connections merged onto the rewrite: the OAuth routes are served and the connect-code routes designed (ADR-0031, ADR-0039 d9, implementation)

Merging the connections build into this branch. No decision changes, and ADR-0031 stays
proposed. The build was made on v0.3.1 with nothing wired into `/mcp`, `/v1` or `/v2`, so the
merge sorts its operations by what the lead now routes
([ADR-0039](adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d9).

- **Served** in [openapi.yaml](api/openapi.yaml): the two discovery documents, `GET` and
  `POST /oauth/authorize`, `POST /oauth/token` and `POST /oauth/register`, six operations routed
  where `oauth.issuer` is set. They join the 23 routed operations, from a second public table,
  `OAUTH_ROUTES`, beside `ROUTES`, and the server test that holds the table equal to the document
  now compares both tables with its 29 operations. Their schemas and the `oauth` codes of the
  `ErrorCode` enum come with them.
- **Designed** in [openapi-designed.yaml](api/openapi-designed.yaml): `POST /v2/keys/{key}/connect-codes`
  and `POST /v2/keys/{key}/connections/{connection_id}/revoke`, which the use cases serve and no
  route calls, so the annex holds 47 operations; `connect_code` on key issuance, request and
  response, and `credential` and `connections` on the key read, under `x-oml-designed-fields`;
  the connector-key refusal of key rotation. `POST /v2/keys` still refuses `connect_code` as an
  unknown field.
- **Migrations:** `0012_connections.sql` and `0013_oauth_clients.sql` keep their numbers after
  this branch's `0009`; `0010` and `0011` stay unused.
- **SDKs:** both generated clients gain the six served operations, and their tests hold the 29
  equal to openapi.yaml as before; both wrappers gain `OAuthError`, the class of the `oauth`
  family [errors](api/errors.md) now declares.
- **Not wired, as before the merge:** `/mcp` accepting an access token, the `/v2` connect-code and
  connection routes, and `POST /v2/keys` reading `connect_code`, so
  `oml keys issue-agent --connect-code` is refused by the server until that step lands.
- **Pages touched:** the two OpenAPI documents, [HTTP API](api/http-api.md),
  [the MCP plugin](api/mcp-plugin.md), the [CLI reference](cli/reference.md),
  [SDKs](engineering/sdks.md), ADR-0031's Release line, the [register](adr/index.md),
  [releases](product/releases.md), [roadmap](product/roadmap.md), `llms.txt` and this page.

### 2026-11-04 — Connections by connect code: the tables, the use cases and the `/oauth` routes (implementation)

An implementation milestone; no decision changes. The code now builds the internals and the
unversioned OAuth surface of the proposed
[ADR-0031](adr/ADR-0031-oauth-connections-by-connect-code.md). The record stays proposed: no
Status line moves, `info.version` stays `0.4.0-design`, no `spec-v0.4` tag is cut, and no package
version moves until the release.

- **The tables** (d5, d7–d10). Migration `0012_connections.sql` adds `api_keys.credential`,
  `connect_codes`, `oauth_connections` and `oauth_tokens`, and `0013_oauth_clients.sql` adds
  `oauth_clients` and `oauth_authorizations`, the provider tenant's deployment-level state
  ([postgres-schema](data/postgres-schema.md#connections-adr-0031)). `0010` and `0011` are left to
  other work.
- **The use cases** (d7, d8, d10–d12). A connector key is issued with a connect code in place of a
  value; an admin mints a fresh code, lists a key's connections and revokes one; the first
  exchange opens a connection in one `connection.opened`, each refresh rotates by one conditional
  update and journals nothing, and a reused refresh token or redeemed code ends the connection in
  a `connection.revoked`. An access token resolves to exactly its key's caller.
- **The routes** (d1, d3–d6, d9, d13, d14). With `oauth.issuer` set, the lead serves the two
  discovery documents, `GET` and `POST /oauth/authorize` with the authorization page,
  `POST /oauth/token` and `POST /oauth/register`, outside both rings; unset, each answers `404`.
  The page fetches client ID metadata documents through one capped fetcher, 16 slots, one per host
  and per address, behind the address allow-list
  ([HTTP API](api/http-api.md#connections-oauth-for-hosted-mcp-clients)).
- **The CLI** (d7). `oml keys issue-agent --connect-code` sends `connect_code: true`, refuses
  `--out`, and prints the connector URL and the code; `POST /v2/keys` does not yet read the member.
- **The check.** `oml-server-lead`'s `oauth_flow` suite runs the whole flow through the real lead:
  a registration, the page, the code and Connect, the exchange, a refresh and the replay of the
  rotated refresh token, which ends the connection, with exactly `connection.code_issued`,
  `connection.opened` and `connection.revoked` journalled. It runs only where a superuser mints
  the role pair, CI's `cargo test (Postgres)`, and prints `did not run` elsewhere.
- **Not built:** what waits for the proposed ADR-0039, which rewrites `/mcp` and the `/v2` routes:
  `/mcp` accepting an access token and its `401` challenge (d2); the `/v2` connect-code,
  connections and revoke routes and the CLI's wiring to them, `oml keys connect-code` and
  `oml keys disconnect` included; the doctor checks `oauth_discovery`, `connect_codes` and
  `connections`; and the issuer recorded in the cluster lease (d14). FR-34 stays open: no live
  claude.ai connection has run.
- **No surface change** beyond the routes the 2026-10-31 entry below declared: no error code or
  event type is added.
- **Pages touched:** [HTTP API](api/http-api.md), [roles and keys](architecture/roles-and-keys.md),
  [Postgres schema](data/postgres-schema.md) and this page.

### 2026-11-04 — The release map covers the v0.4.0 records and pages (merge of main)

A documentation change with no decision or surface change. Merging main brings
[releases](product/releases.md) to this branch; the proposed
[ADR-0038](adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md),
[ADR-0039](adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) and
[ADR-0040](adr/ADR-0040-plugin-setup-in-claude-code.md) each gain a **Release:** line, planned for
v0.4.0, and a Release cell in the [register](adr/index.md), and the releases page links them.

- **Pages touched:** the three records, the register, releases, `README.md`,
  `plugins/oml/README.md`, [http-api](api/http-api.md), [the MCP plugin](api/mcp-plugin.md),
  [retrieval](architecture/retrieval.md), [roles and keys](architecture/roles-and-keys.md), the
  [CLI reference](cli/reference.md), [upgrades](operations/upgrades.md) and this page. Each
  section this branch added carries the release line main gave its neighbours: the three tools,
  `GET /v2/whoami`, the start settings, `oml login`, `oml write`, `oml read`, `oml events`,
  `oml hook tool-failure`, the librarian and the upgrade to v0.4.0 are planned for v0.4.0. Each
  section it rewrote keeps main's release line and adds the parts that are new, each planned for
  v0.4.0: on http-api, for instance, submit's `workarea` in the body and derived `claim_key`, get's
  `text` and the events tail's `workarea` parameter, and on the CLI reference and the plugin
  README the session-start hook's plugin settings and compaction prompt naming `oml_write`. The
  releases page records the librarian as ADR-0038's d10.
- **No surface change:** `info.version` stays `0.4.0-design`; no record's decision moves.

### 2026-11-04 — The plugin's librarian subagent answers what OML holds about a subject, reading with `oml_read` alone (ADR-0038 d10)

The proposed [ADR-0038](adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md)
gains d10, the librarian v0.4.0 carries (ADR-0039, context).

- **Decided and built:** `plugins/oml/agents/librarian.md`, the subagent `oml:librarian`, on
  `model: haiku`, whose only tool is `mcp__plugin_oml_oml__oml_read`; it recalls with the
  subject's exact words and then one variant at a time, at most eight queries, because recall
  stems nothing, and returns each finding's `artifact_id`, `claim_key`, `workarea_path` and
  `created_at` under the hooks' framing line, with the queries it ran. The plugin README
  describes it, and `scripts/check_plugin.py` fails on a second agent, another model, any tool
  beside `oml_read`, an omitted `tools` field, a field a plugin subagent ignores, or a prompt
  without hook.rs's framing line.
- No HTTP operation, MCP tool, error code or package version changes.
- **Pages touched:** ADR-0038 and its [register](adr/index.md) row, the plugin README,
  `scripts/check_plugin.py` and this page.

### 2026-11-04 — Turn context on the rewritten surface: the agent recalls with `oml_read`, and a failed command recalls when opted in (ADR-0038, rebased onto ADR-0039 d2 and ADR-0040 d3)

[ADR-0038](adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md),
proposed on 2026-11-03 and accepted by the maintainer on 2026-11-04 with release v0.4.0,
was built on v0.3.0's tools in PR #24; this change merges it into v0.4.0 and fits it to the
rewrite, as ADR-0039's consequences owe. It amends [ADR-0029](adr/ADR-0029-session-start-hook.md)
d6 and d7; the pointer on ADR-0029 was added on acceptance, on 2026-11-04.

- **Decided and built:** `initialize` answers with server instructions under 4,096 characters
  that ask for `oml_read` with a query within their first 1,024 and name `oml_write` for a record,
  with no `claim_key` and no v0.3.0 tool name (d1); `oml_read`'s description in
  `docs/api/mcp-tools.json` and the `oml-memory` skill open with when to recall and with which
  words (d2); one recall line in the session-start context naming `oml_read` (d3);
  `oml hook tool-failure`, off unless `OML_FAILURE_RECALL=1`, which reads its server and key as
  the session-start hook does, the plugin's `url` and `api_key` options before `OML_URL` and
  `OML_API_KEY` as one pair (ADR-0040 d3), recalls with at most three distinctive words of a
  failed Bash command's error, secrets removed, skips `grep`, `test`, `[` and `diff`, an error the
  session already recalled and any session whose earlier request failed, and sends one `POST
  /v1/memories/recall` of `k` 3 under 200 budget tokens with `include_text`, never
  `include_unreviewed`, a 1-second timeout and a 2-second deadline (d4–d7); its context prints
  each hit's `text` under the data frame and a may-be-irrelevant line, at most 1,600 bytes,
  nothing on a miss (d8); the per-session cache of ids and digests, 0600, seeded by session start
  and deleted by `oml hook session-end`, and the two guarded hook entries (d9).
- **The catalog an agent reads** grows from 8,130 to 8,488 characters with `oml_read`'s opening
  sentence, against 18,893 for v0.3.0's twelve entries, measured as before.
- **Checks:** `crates/bins/oml-cli/tests/cli_e2e.rs` runs both verbs against the real lead behind
  the request recorder, one with the plugin's options beside an environment pair that names
  another server, and against stubs for a failing and a silent server; `hook.rs`'s unit tests hold
  the request body, the hit's `text`, the recall line and the pair refusals;
  `crates/bins/oml-server-lead/tests/mcp_facade.rs` holds the instructions; and
  `scripts/check_plugin.py` holds all three hook entries and that no line of `hook.rs` names a
  tool `tools/list` does not offer.
- **Not verified here:** that Claude Code runs the two new hooks and adds the failure context;
  whether `session_id` survives `/clear`. The [roadmap](product/roadmap.md) moves FR-57 and opens
  FR-63 and FR-64.
- **No HTTP surface change:** no operation, field, error code or event type is added, and
  `info.version` stays `0.4.0-design`.
- **Pages touched:** [ADR-0038](adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md), the [decision register](adr/index.md) (row and the D14
  cell), `mkdocs.yml`, [llms.txt](llms.txt), [CLI reference](cli/reference.md),
  [agent plugin](api/mcp-plugin.md), [mcp-tools.json](api/mcp-tools.json), the
  [roadmap](product/roadmap.md) (FR-57, FR-63, FR-64) and this page; outside `docs/`,
  `plugins/oml/README.md`, `plugins/oml/skills/oml-memory/SKILL.md`,
  `plugins/oml/hooks/hooks.json` and `scripts/check_plugin.py`.

### 2026-11-04 — The 0.4.0 breaks, each with its migration (ADR-0039 d13, d14 step 5, implementation)

This entry is the break list a v0.3 user reads before upgrading; the entries below record each
change as it was built. Keys, grants, stored memories, the journal and the schema do not change,
and v0.4.0 adds no migration.

| Break | Who it reaches | Migration |
|---|---|---|
| Three tools, `oml_write`, `oml_read` and `oml_workarea`, replace the eight of v0.3.0 | `permissions.allow` rules naming the v0.3.0 read tools, which now allow nothing | allow `mcp__plugin_oml_oml__oml_read` and `mcp__plugin_oml_oml__oml_workarea`, or the server-wide `mcp__plugin_oml_oml`; the plugin README and the root README carry both |
| The events tail leaves MCP | callers of `oml_events_tail` | `oml_read` with `state` for an agent's own writes; `GET /v2/events`, the SDKs' `events` or `oml events` to audit |
| The plugin takes `url` and `api_key` as its own settings, and its cached skill and hook name the old tools | plugin 0.3 installs, which serve no tools after the update until configured | `claude plugin update oml@open-memory-lab`, then `/plugin configure oml@open-memory-lab` or `claude plugin configure --values-stdin`; install the v0.4.0 CLI for the hook |
| One `workarea` field replaces the `X-OML-Workarea` header and the `workarea_id` filter; `include_unreviewed` replaces `include_own_held`; recall takes `include_text` for `include_bodies`; a sent `auto-` claim key is refused | HTTP callers, the CLI, the hook | upgrade the CLI with the server; a renamed spelling is refused with `request.validation` naming the new one in `details.renamed_to`, and an `auto-` key naming `/claim_key` |
| The admin routes' request fields are named after the kind they reference: `POST /v2/keys` takes `default_workarea` for `default_workarea_id`, `tenant` for `tenant_id`, each grant's `workarea` for `workarea_node_id` and `deny[].workarea` for `deny[].subtree_node_id`; `POST /v2/keys/{key}/denials` takes `workarea` for `workarea_node_id`; `POST /v2/workareas` takes `parent` for `parent_id`; and the `scope` of v0.3.0's `oml_recall` and `oml_events_tail` is `workarea` on `oml_read` and `GET /v2/events` | admin scripts, HTTP callers, MCP clients, the CLI | send the new spelling: each admin route field above is refused with `request.validation` naming its successor in `details.renamed_to`, the CLI sends the new spellings, and the two v0.3.0 tools still take `scope` through the alias window |
| A served claim's `body` is `text` | HTTP readers, the SDKs | read `text` |
| A submission with `derives_from` and no `verb` is journalled `submission.learn` | journal readers | none; an explicit `verb: "remember"` keeps the old event |
| The SDKs' verbs are `write`, `read`, `workarea`, `events`, `review_next`, `approve` and `reject`, at `0.2.0`; openapi.yaml declares only the 23 routed operations, so the unrouted ones leave the generated modules for the designed annex; three operationIds are renamed, `searchMemories` to `listMemories`, `proposeSupersession` to `supersedeMemory` and `updateCrewMetadata` to `setWorkareaCrew`, and the generated calls with them | SDK users | the wrapper's `write` and `read`; call a renamed generated operation by its new name; the removed generated calls never reached a route |
| CLI verbs, flags and variables renamed: `oml write`, `oml read`, `oml events`, `oml review approve` and `reject`, `--url` and `--api-key`, `OML_URL` and `OML_API_KEY`; the bootstrap secret's argv flag removed | scripts, shell profiles | the new names; `oml login <url>` once; `oml bootstrap claim … --token-stdin` |
| `oml.doublegate.io` on a v0.3 release | the maintainer's deployment | upgrade the server, the CLI and the plugin together, configure the plugin, replace the permission rules, and record the upgrade with its date in [upgrades](operations/upgrades.md#server-cli-and-plugin-together-the-upgrade-to-v040); not run yet |

**The alias window.** The eight v0.3.0 tool names answer through `tools/call`, never in
`tools/list`, with their v0.3.0 inputs, outputs and refusal codes, and each call logs
`mcp_alias_called`; `oml submit`, `oml recall`, `oml supersede`, `oml review verdict`,
`--server` and `--key` run their successors, naming each on stderr; and a v0.3.0 credentials
file's `server` and `key` are read as `url` and `api_key`. All of these answer through v0.4.x
and leave in v0.5.0, as ADR-0039 d12 reads the window since the rewrite moved to v0.4.0
(2026-11-03, below). **The maintainer has not decided that reading**; the other is no aliases
at all. HTTP and the SDKs have none. `OML_CLI_SERVER` and `OML_CLI_KEY` are not read, and a
missing start setting names the variable to rename.

**The catalog an agent reads.** The three served entries of
[mcp-tools.json](api/mcp-tools.json) come to 8,488 characters, against 18,893 for v0.3.0's
twelve, each counted as the length of the entry's JSON from Python's `json.dumps` with its
default separators, the measure that gives the 18,893 ADR-0039 records.

`scripts/check_contract_sync.py` now fails a start page, the READMEs, the plugin skills, the
agent kit's README, the docs index, `llms.txt`, [SDKs](engineering/sdks.md), the operations
pages and the scripts, that names a v0.3.0 spelling of ADR-0039 d10 outside a section headed
"Upgrading from …": any of the eight tool names, read from the facade's alias table,
`X-OML-Workarea`, `include_own_held`, `OML_CLI_SERVER`, `OML_CLI_KEY`, `--token`, `--server`,
`--key`, `oml submit`, `oml recall`, `oml supersede`, `oml review verdict`, or the release
0.3.2 written with or without its `v`. The pattern is first run against one probe line per
spelling and against the v0.4.0 spellings it must let through. The quickstart's own CLI URL
variable is `OML_CLI_URL`.

- **Pages touched:** the root README's quickstart (the server, `oml login`, the plugin, `oml
  write` and `oml read`, the permission rules), the `oml-memory` skill's mode rules and its file
  rule, [upgrades](operations/upgrades.md), the [overview](index.md), `llms.txt`, the
  [HTTP API](api/http-api.md), [reviewer crews](api/reviewer-crews.md),
  [roles and keys](architecture/roles-and-keys.md), the
  [PostgreSQL schema](data/postgres-schema.md), the [critical path](product/critical-path.md),
  [CI and toolchain](engineering/ci-and-toolchain.md) and this page.

### 2026-11-04 — The agent kit reads the OML_URL and OML_API_KEY pair (ADR-0039 d15, implementation)

The kit's Cursor configuration reaches `${env:OML_URL}/mcp` and its Gemini CLI configuration
`${OML_URL}/mcp`, each tool's documented form for a variable inside a URL, and every
configuration still takes its bearer from `OML_API_KEY`. Codex, VS Code and Claude Desktop's
`mcp-remote` bridge document no variable in a URL, so theirs keep `http://127.0.0.1:8080/mcp`,
and the kit README's "Before you start" exports both variables and names that one edit for those
three alone. Every page the README cites for a URL or key form was fetched again on 2026-11-04.
`scripts/check_agent_kit.py` holds each configuration's URL to the form its tool documents and
the README's edit to the tools that cannot expand it.

- **Pages:** the kit README and [CI and toolchain](engineering/ci-and-toolchain.md).

### 2026-11-04 — The session-start hook reads the plugin's options before the environment pair, and issue-agent --out prints the plugin configure line (ADR-0040 d3, d4, implementation)

`oml hook session-start` takes its server and key from `CLAUDE_PLUGIN_OPTION_URL` and
`CLAUDE_PLUGIN_OPTION_API_KEY` when either is set and non-empty, and otherwise from `OML_URL` and
`OML_API_KEY`; one option set alone sends nothing and names the unset one, so a key never goes
to a URL from another source. The printable-ASCII refusal names whichever key variable it read,
and the redaction covers either pair. `oml keys issue-agent --out` prints a third stdout line, the
`jq … | claude plugin configure oml@open-memory-lab --values-stdin` pipeline that reads the key
file, amending ADR-0028 d1's two-line output; the value still enters neither stream. With the
previous entry, an install that follows the plugin README gets its tools and its session context
from the two settings alone; no release carries that entry without this one.

- **Pages:** the [CLI reference](cli/reference.md) for the hook's variables, the issue-agent
  output and the `oml-admin` plugin; [mcp-plugin.md](api/mcp-plugin.md); [the CLI
  sessions](cli/sessions.md); the plugin README; the `oml-admin` skill, which now hands over the
  configure line and has the key file deleted only after it succeeds.

### 2026-11-04 — The plugin's userConfig and .mcp.json read the url and api_key options alone (ADR-0040 d1, d2, d5, d6, implementation)

`plugins/oml/.claude-plugin/plugin.json` declares two `userConfig` options, `url` and a
`sensitive` `api_key`, neither with a default, and `.mcp.json` reaches `${user_config.url}/mcp`
with `Bearer ${user_config.api_key}` and names no `OML_` variable. A v0.3.x install that only
exported `OML_URL` and `OML_API_KEY` serves no tools until the plugin is configured once, in the
`/plugin install` dialog, with `/plugin configure oml@open-memory-lab`, or from a shell by piping
the key file through `jq` into `claude plugin configure --values-stdin`, which needs Claude Code
2.1.285. `scripts/check_plugin.py` checks that the plugin declares exactly url and a sensitive
api_key, that the mcp entry reads no environment variable, and that the plugin README shows no
`export OML_API_KEY` line; CI's plugin job also runs `claude plugin validate --strict`. The
plugin README discloses the session-start hook's two requests and the directory name it sends.
The hook still reads `OML_URL` and `OML_API_KEY` (ADR-0040 d3 is not built), and the plugin's
version stays the workspace's until the release bump.

- **Pages:** [mcp-plugin.md](api/mcp-plugin.md), the plugin README, the root README's Claude
  Code block and the quickstart's closing instructions.

### 2026-11-04 — Both SDKs move to 0.2.0 with the seven wrapper verbs and the URL-and-key start (ADR-0039 d10, d11, d15, implementation)

The Python and TypeScript wrappers take `write`, `read`, `workarea`, `events`, `review_next`
(`reviewNext`), `approve` and `reject`. `write` takes `oml_write`'s fields and chooses submit or,
with `replaces`, supersede; `read` takes `oml_read`'s and chooses fetch, recall or the listing,
`own` defaulting to true; a field outside the mode, `query` beside `artifact_id` among them, is
`request.validation` naming it, raised before anything is sent. A `write` without `claim_key`,
and an `approve` or `reject` without `serve_id`, send none. The constructor takes `url` and
`api_key`, else `OML_URL` and `OML_API_KEY`, under the CLI's one-source rule, and every refusal
is `ApiError` carrying the envelope. The generated layers were already current with
openapi.yaml.

- **Breaks:** `remember`, `learn`, `supersede`, `recall`, `get`, `list_memories`
  (`listMemories`) and `verdict` are gone, with no alias (d12); `base_url` (`baseUrl`) is `url`;
  a URL argument with the key left to `OML_API_KEY`, or the reverse, is refused; the raising
  `upload_attachment` stub and `NotBuiltError` are removed.
- **Breaks, same names:** `pages` and `iter_memories` (`pages` and `memories`) now send
  `own=true` when `own` is omitted, as `read`'s listing does, so they yield the key's own rows in
  every state where 0.1.0 sent no `own` and got the promoted tips of the default workarea; pass
  `own=False` (`own: false`) to keep that. TypeScript's `pages(query, options)` and
  `memories(query, options)` take one argument: move `options.workarea` into the query.
- **Breaks, stored kind:** `learn` defaulted `kind` to `derived_fact`; `write` with
  `derives_from` sends no kind, so the server stores `memory` and a recall filtered on `kinds`
  finds the row under that kind. Pass `kind="derived_fact"` (`kind: "derived_fact"`) where a
  caller relied on `learn`'s default.
- **Pages touched:** [SDKs](engineering/sdks.md), the root README and both SDK READMEs.

### 2026-11-04 — `oml login <url>` stores the pair once whoami accepts the key (ADR-0039 d15, implementation)

`oml login <url>` reads the key without echo from the terminal, or the first line of stdin when
stdin is not a terminal, calls `GET /v2/whoami` under it, and only on success writes `url` and
`api_key`, nothing else, to the owner-only credentials file. A refusal writes nothing, leaving a
stored pair as it was, and exits `1` with the envelope's code. The designed `oml context` verbs
leave the reference: `oml login` replaces `context add`, `oml doctor` replaces `context show`,
and a second server is a second file through `OML_CLI_CREDENTIALS`. The no-echo read is
`rpassword` 7.5, the CLI's one new dependency, since std reads no terminal without echo. The
[CLI sessions](cli/sessions.md) mockups follow: the bootstrap claim stores the operator's pair,
each later identity runs `oml login` once into its own file, and `export OML_CLI_CREDENTIALS=…`
switches between them. `check_contract_sync.py` fails a reader page that still runs a retired
`oml context` verb.

- **Pages touched:** [cli/reference.md](cli/reference.md#oml-login-and-oml-doctor-storing-and-checking-the-pair), [cli/sessions.md](cli/sessions.md).

### 2026-11-04 — `oml doctor` reports each start setting's source and probes with a URL alone (ADR-0039 d15, implementation)

`oml doctor` no longer takes the pair through the refusal every other verb applies. It names the
source of the URL and of the key, never the key; probes `/healthz` and `/readyz` keyless at the
first source's URL; and calls `GET /v2/whoami` and the events tail only when that source also
holds the key. A URL alone is a `warn` (exit `0`) naming the missing key, as `oml --server X
doctor` answered without a key in v0.3.0; no URL in the first source sends nothing and is a
`fail` (exit `1`). `--json` gains `url_source`, `key_source`, `whoami` and `next`, and its
`verdict` can be `warn`.

- **Pages touched:** [cli/reference.md](cli/reference.md#start-settings).

### 2026-11-04 — The CLI starts from one URL-and-key pair: flag, then environment, then file (ADR-0039 d15, implementation)

The CLI's start settings are `--url` and `--api-key`, `OML_URL` and `OML_API_KEY`, and the
credentials file's `url` and `api_key`, taken as one pair from the first of those sources that
sets either value. A source that sets one alone is refused (exit `2`) naming the other, and
nothing is sent, so `--url` is never paired with a key from the environment or the file.
`oml bootstrap claim` takes the URL alone and stores the pair and nothing else; a v0.3.0 file's
`server` and `key` are read through v0.4.x and its other fields ignored, and `oml audit
verify-chain` without `--tenant` reads the key's tenant through `GET /v2/whoami`. The hidden
`--server` and `--key` print one stderr line naming their successors; `OML_CLI_SERVER` and
`OML_CLI_KEY` are not read, and a missing start setting names whichever is set for renaming.

- **Breaks:** `--key` with the URL left to the credentials file is refused, and an `OML_URL`
  and `OML_API_KEY` exported for the plugin win over the stored admin pair; scripts pass both
  flags or unset both variables, as the quickstart, the PoC demo and the SDK harnesses now do.
- **Pages touched:** [cli/reference.md](cli/reference.md#start-settings),
  [deployment.md](operations/deployment.md), the root README and the `issue-agent-key` skill.

### 2026-11-04 — A verdict without `serve_id` applies to the reviewer key's one claimed item (ADR-0039 d10, implementation)

`POST /v2/review/verdicts` takes `serve_id` as optional. Omitted, the verdict applies to the one
item the key has claimed through `POST /v2/review/next`, so a reviewer holding one item need not
copy what `review next` printed; while the key holds none or several it is `request.validation`
with `details.field` `/serve_id`, and nothing is journalled. The default rests on the claimed
queue row, as the explicit `serve_id` does, and the serve binding stays FR-66. A request that
sends `serve_id` is unchanged.

The route honours its `Idempotency-Key`, scoped to the key and a nil workarea, and looks the
record up before the default resolves: a retry of a committed verdict replays its stored
response with `Idempotency-Replay: true` whatever the key has claimed since, so a retried
default never applies to an item its caller was not shown, and the same key with another body is
`idempotency.digest_mismatch`.

- **Pages touched:** [http-api.md](api/http-api.md#review), [errors.md](api/errors.md)
  (`request.validation`), [openapi.yaml](api/openapi.yaml) and the SDKs' generated layers.

### 2026-11-04 — The eight v0.3.0 tool names answer as hidden aliases with their v0.3.0 shapes until v0.5.0 (ADR-0039 d12, implementation)

`oml_remember`, `oml_learn`, `oml_recall`, `oml_get`, `oml_supersede`, `oml_my_work`,
`oml_workarea_info` and `oml_events_tail` answer only through `tools/call`, never in
`tools/list`. Each takes its v0.3.0 input, `scope` as the ring's `workarea`, `include_own_held`
as its `include_unreviewed` and `claim_key` still required, and answers its v0.3.0 output
shape and refusal codes; `oml_events_tail` rides `GET /v2/events`. Every call logs the event
`mcp_alias_called` naming the tool, its successor, the calling key's `key_id` and `tenant_id`
and the call's `request_id`, so an operator sees which keys still call an old name. The window is v0.4.x, and v0.5.0 removes
the eight, as d12 reads it; the maintainer has not decided that reading.
[mcp-plugin.md](api/mcp-plugin.md) states the window and the removal release, and the facade
tests hold each name to a fixture of its v0.3.0 shape.

- **Pages touched:** [mcp-plugin.md](api/mcp-plugin.md) and this page.

### 2026-11-04 — `tools/list` names three tools, `oml_write`, `oml_read` and `oml_workarea`, and every surface that names a tool moves with it (ADR-0039 d2, d5, d8, d13, implementation)

`mcp-tools.json` declares exactly the three served tools with their annotations, and the facade
serves it whole. The new `mcp-tools.designed.json`, never served, holds `oml_relate_propose`,
`oml_vocab`, `oml_read`'s `lineage` and `depth` and `oml_write`'s `attachments`; a call to a
designed tool is JSON-RPC -32602 naming that file, and `oml_attach` and `oml_graph` answer -32602
naming their successors. The eight v0.3.0 tool names leave `tools/list` and still answer, as d12
reads its window. [mcp-plugin.md](api/mcp-plugin.md) describes the three tools, their modes and
hints, the events tail on HTTP and the CLI only, and d8's rule, and records the catalog at 8,130
characters against 18,893. The skill and the agent kit's `AGENTS.md` block teach the three tools,
with no invented `claim_key` and `oml_read {state}` for a write's outcome; the session-start
hook's lines name `oml_write`, `oml_read` and `oml_workarea`; the plugin README's permission rules
are `mcp__plugin_oml_oml__oml_read` and `__oml_workarea`, or `mcp__plugin_oml_oml` (amends
ADR-0029 d8); the quickstart drives `oml_write` and `oml_read`. `check_plugin.py` fails on a tool
the skill or the hook names that is not served, and `check_contract_sync.py` on a catalog other
than the served `tools/list` or a designed tool in both files.

### 2026-11-04 — `oml_read` chooses get, recall or listing from the fields present, and `oml_workarea` reads one node (ADR-0039 d2, d4, implementation)

The facade serves `oml_read` and `oml_workarea` beside `oml_write` and the eight v0.3.0 tools,
declared in `mcp-tools.json` with `readOnlyHint` and `idempotentHint` true and `openWorldHint`
false. `artifact_id` is one `GET /v1/memories/{artifact_id}`; `query` is one
`POST /v1/memories/recall` with `workarea`, `k`, `kinds`, `include_unreviewed` and
`max_tokens`; neither is one `GET /v1/memories` with `own` (default `true`), `workarea`,
`claim_key`, `state`, `kind`, `since`, `until`, `limit` and `cursor`, and `own: false`, the
scoped listing, is new on MCP. An argument outside the mode, and `query` beside `artifact_id`,
is `request.validation` naming it in `details.field`; the lineage mode's `lineage` and `depth`
are designed and refused the same way. The output is `{mode, items}` for get, `{mode, steering,
hits, budget?}` for recall and `{mode, items, next_cursor}` for the listing, `mode` being `get`,
`recall` or `listing`, and every item is the claim item with its `text`, the listing's included,
so `oml_read {state}` tells an agent whether its write was approved without the events tail.
`oml_workarea` is one `GET /v2/workareas/{workarea}` answered as `oml_workarea_info` answers.

### 2026-11-04 — `oml_write` takes one flat argument set, and the fields present choose the endpoint (ADR-0039 d3, implementation)

The facade serves `oml_write` beside the eight v0.3.0 tools, declared in `mcp-tools.json` with
its annotations. `text` and `idempotency_key` are required. Without `replaces` it is one
`POST /v1/memories`, journalled `submission.learn` when `derives_from` is non-empty, its
`claim_key` and `kind` left to the server's defaults; with `replaces` it is one
`…/{replaces}/supersede`, and `retirement: true` proposes retiring the target. A field outside
the mode is `request.validation` naming it in `details.field`: `retirement` and `reason` without
`replaces`; `claim_key`, a `kind` other than `memory`, `derived_fact` or `steering`, a `workarea`
not the target's and a vocabulary-kind target with it. The output is `{mode, artifact_id,
claim_key, version, state, workarea, retirement, replaces, idempotent_replay}`, `mode` being
`submit` or `supersede`. The vocabulary-target refusal now carries `details.field: /replaces` on
`oml_supersede` too. The v0.3.0 tools leave `tools/list` in a later step.

### 2026-11-04 — The memory routes and the events tail take one `workarea` field, `include_unreviewed` and `include_text`, and serve `text` (ADR-0039 d4, d7, implementation)

`POST /v1/memories`, `…/supersede` and `…/recall` take the body field `workarea`, and
`GET /v1/memories` and `GET /v2/events` the query parameter `workarea`, each an id or a name.
Omitted, recall and the scoped listing read the key's default workarea and the own-work listing
every workarea the key wrote in. Recall takes `include_unreviewed`, reaching the key's own held
rows and defaulting to `false`, and `include_text`; a served claim carries `text` where v0.3.0
said `body`, on get, recall, the listings and the review serve. The request field `body` and the
serve's `served_digests.body` keep their names.

- **Breaks, each with its migration:** the `X-OML-Workarea` header is `request.validation` with
  `details.renamed_to` `/workarea` on a body route and `workarea` on a query route, and on get,
  which names no workarea, `details.field` alone: send the field. The listing's and the events
  tail's `workarea_id` is refused naming `workarea`; on the scoped listing it was a filter inside
  the scope, and the one field now is the scope. Recall's `include_own_held` and
  `include_bodies` are refused naming `include_unreviewed` and `include_text`. Read `text`.
- The recall `query_digest` keeps its members' v0.3.0 names, so read-log digests compare across
  the rename ([retrieval](architecture/retrieval.md#the-read-log)).
- The CLI's `submit --workarea` and `recall --workarea` send the field, and `oml hook
  session-start` sends `include_text` and `include_unreviewed` and reads `text`, amending
  ADR-0029 d2. The v0.3.0 MCP tools keep their arguments until `oml_read` and `oml_write` replace
  them; the facade sends their `workarea`, `scope` and `include_own_held` as the ring's new
  fields. The SDKs are regenerated.

### 2026-11-03 — The key and workarea routes take v0.4.0's request spellings and refuse v0.3.0's by name (ADR-0039 d7, admin half, implementation)

`POST /v2/keys` takes `default_workarea`, `tenant`, each grant's `workarea` and each deny entry's
`workarea`; `POST /v2/keys/{key}/denials` takes `workarea`; `POST /v2/workareas` takes `parent`.
The path parameters are `{workarea}`, `{crew}` and `{key}`, in openapi-designed.yaml too. Each
takes an id or a name. A v0.3.0 spelling is `request.validation` with `details.field` naming it
and `details.renamed_to` its successor, as JSON Pointers, and the denial and workarea-create
bodies are now closed, so no field is dropped. Responses keep their v0.3.0 names. The CLI's
`keys issue`, `keys issue-agent`, `deny set` and `workareas create` send the new spellings, and
the SDKs are regenerated. The provider ring's renames and the MCP arguments are not built yet.

### 2026-11-03 — openapi.yaml declares only what the server routes; the rest is a designed annex (ADR-0039 d9, implementation)

[openapi.yaml](api/openapi.yaml) now holds exactly the 23 routed operations, the 22 of v0.3.0 and
`GET /v2/whoami`, each with only the fields the build accepts, and the complete `ErrorCode` enum;
`info.version` is `0.4.0-design`. The new [openapi-designed.yaml](api/openapi-designed.yaml), from
which nothing is generated, holds the 45 designed operations and, under `x-oml-designed-fields`,
the designed fields of served ones: the attachment, simulation and vocabulary fields of submit,
supersede and recall, a workarea's `prefix`, the `expiry` and `tenant_id` of key issuance, the
review serve's `workarea` filter, and the workarea read's review policy and pending transitions. A designed field sent to a served operation answers its v0.3.0 code.

- **Removed:** `GET /v2/workareas/{id}/status`, a second read of the node the workarea read
  serves.
- **Merged:** the claim-edge and workarea-edge families become one designed `/v2/edges` family of
  three, keyed by `kind` `claim` | `workarea`, each kind keeping its authority, refusal codes and
  journal events.
- **Renamed operationIds:** `searchMemories` → `listMemories`, `proposeSupersession` →
  `supersedeMemory`, `updateCrewMetadata` → `setWorkareaCrew`; the generated SDK modules follow,
  and the wrappers call them under their existing verbs.
- **Checks:** the router builds its routes from the transport's public `ROUTES` table, which a
  server test compares with openapi.yaml; `check_contract_sync.py` compares
  [http-api.md](api/http-api.md), which now marks each endpoint served or designed, with the
  union of both documents' paths, and fails on a path in both or on a designed field the served
  operation also declares; each SDK's tests hold its generated operations equal to openapi.yaml's.
  The Python generator's recorded warnings are now empty, since the status read's schema, whose
  name clashed with `Workarea`'s status enum, is gone.

### 2026-11-03 — The API rewrite and the plugin setup retargeted to v0.4.0 (ADR-0039, ADR-0040, proposed)

The maintainer moved the rewrite from v0.3.2 to v0.4.0 the same day it was proposed. Both
proposed records now name v0.4.0, and the spec version the build moves to is `0.4.0-design`.
ADR-0039 is renamed to `ADR-0039-the-api-rewritten-for-v0-4-0.md`, with its `id:` header and
every link, while nothing outside the branch links to it. ADR-0039 d12's alias window becomes
hidden through v0.4.x and removed in v0.5.0, and a v0.3.0 credentials file's `server` and `key`
are read through v0.4.x. **The maintainer has not decided that window**; the other reading is no
aliases at all, which drops d14's step 3. Turn context (ADR-0038), the mobile OAuth connector
(ADR-0031) and the librarian subagent also land in v0.4.0 from their own branches and rebase onto
ADR-0039's build.

- **Pages touched:** the two records, their [register](adr/index.md) rows, `mkdocs.yml`,
  `llms.txt`, the roadmap's FR-65 and FR-67 rows and this page.
- **Not built.**

### 2026-10-31 — Connections: Claude's hosted apps over OAuth, bound by a one-time connect code (ADR-0031)

[ADR-0031](adr/ADR-0031-oauth-connections-by-connect-code.md), proposed on 2026-10-31 and accepted by the
maintainer on 2026-11-04 with release v0.4.0, designs how claude.ai,
Claude Desktop and the Claude iOS and Android apps reach the [agent plugin](api/mcp-plugin.md)
through a claude.ai custom connector, on the maintainer's three fixed decisions: OML as an OAuth 2.1
protected resource with its own authorization server, beside unchanged bearer keys; the binding by a
one-time connect code; the build after iteration 3. It amends ADR-0010 (d3, d7), ADR-0014 (d1, d2,
d3, d4), ADR-0015 (d1), ADR-0018 (d4), ADR-0025 (d2, d15) and ADR-0028 (d1); their Status lines gain
the back-pointer on acceptance, and did on 2026-11-04. Sources: the MCP authorization specification, revision
2025-11-25, Anthropic's connector documentation, the client ID metadata document draft (-02), RFC
9700, RFC 9207, the Fetch Standard, the IANA IPv4 and IPv6 Special-Purpose Address Registries and
Caddy's `log` directive documentation, all fetched 2026-10-31.

- **Decided, not built:** the `/mcp` `401` keeps its typed body and gains the RFC 9728 challenge
  (d2); one protected-resource document at `/.well-known/oauth-protected-resource/mcp` and one RFC
  8414 document (d3); `/oauth/authorize`, `/oauth/token` and `/oauth/register`, unversioned, in the
  RFCs' shapes, S256 PKCE only, public clients only, one scope `mcp` (d4); client ID metadata
  documents preferred, dynamic registration as the fallback, no pre-registered client (d5), with the
  document URL's form, an allow-list of global unicast addresses, TLS, proxy and concurrency bounds,
  a document cache that evicts and never refuses, and a token endpoint that identifies the client by
  the connection; a fixed redirect allowlist of exactly `https://claude.ai/api/mcp/auth_callback`,
  matched byte for byte after one form decoding (d6); the connector key, which has no value and at
  most one open connection, `oml keys issue-agent --connect-code`, and connect-code responses
  stored in the idempotency record without the code, amending ADR-0014 d3 (d7); the connect code —
  twelve Crockford base32 symbols, shown once, stored as a digest,
  journalled by id, ten minutes by default, with a new code or a revoked connection ending every
  page in progress for the key (d8); the two-step authorization page with a sealed handle and no row
  until a code verifies, Connect and Cancel each using up the request, no cookie, fixed headers,
  `Referrer-Policy: same-origin`, `iss` on every
  redirect, a warning against forwarded links, and a confirmation naming the tenant and built from
  the key's role (d9); opaque tokens bound to their connection's resource, rotation on every refresh
  by one conditional update, reuse ending the connection, and a redeemed code ending it only in the
  hands of its verifier's holder (d10); a token as exactly its key, only an access token at `/mcp`,
  the key's state read at code entry, exchange and refresh, when a connection is live, `oml_attach`
  refused under a connection, and `oml keys disconnect` (d11); `connection.code_issued`,
  `connection.opened` and `connection.revoked`, and no event for a refresh (d12); the `oauth` error
  family (d13); the configuration keys, doctor fields, proxy routing and access logs, and cluster
  mode with the follower serving the lead's issuer (d14); the build order (d16).
- **On names** ([ADR-0026](adr/ADR-0026-names-are-keys.md), accepted with v0.3.0 after this
  record was proposed): the record follows them where they apply. `oml keys issue-agent
  <key-name> … --connect-code` sends the key's name, and `oml keys connect-code` and `oml keys
  disconnect` take the key by id or name, while a connection, which has no name, is named by its
  id. The connect-code check is the issuance order's seventh step, ahead of the name check, which
  ADR-0026 d5 keeps after every other check and which becomes the eighth. The confirmation names
  the key and every granted workarea by name, and the journal events keep binding to ids.
- **The vocabulary** gains three rows, *connection*, *connect code*, and *access token, refresh
  token*, because "grant" belongs to workarea grants and "token" alone could mean a key (d7). The
  claim↔claim and workarea↔workarea sense is always "connection edge" or "connection proposal",
  reworded where it stood bare.
- **`info.version` 0.3.0-design → 0.4.0-design** (d15): the error taxonomy, which the rule on this
  page names, gains eleven `oauth` codes, and the OpenAPI document gains `/mcp`, the two discovery
  documents, four OAuth operations and two `/v2` key operations. No `/v1` operation changes; on
  `/v2` the connect-code operations' idempotency record stores no code. `spec-v0.3` was cut when
  [ADR-0026](adr/ADR-0026-names-are-keys.md) was accepted, so this record opens the next
  revision, and `spec-v0.4` is cut at its acceptance.
- **Roadmap:** RS-2 restated in ADR-0018 d3's fixed form; the record meets its exit criterion on
  acceptance, when the row leaves the register. FR-34 opened: the flow is designed, not built, and
  no live connection has run. FR-35 opened: the `/v1` and `/v2` `401` answers carry no
  `WWW-Authenticate` challenge, which RFC 9110 §15.5.2 requires. FR-36 opened: a bearer key's
  issuance or rotation keeps its value in the idempotency record's stored response.
- **Pages touched:** [ADR-0031](adr/ADR-0031-oauth-connections-by-connect-code.md), the
  [decision register](adr/index.md) (row; the D9, D13 and D14 cells; the ADR-0022 row says "key"
  where it said "token"), `mkdocs.yml`,
  [llms.txt](llms.txt), [HTTP API](api/http-api.md), [openapi.yaml](api/openapi.yaml),
  [mcp-tools.json](api/mcp-tools.json) (the revision only), [errors](api/errors.md),
  [the agent plugin](api/mcp-plugin.md), [roles and keys](architecture/roles-and-keys.md),
  [security](architecture/security.md), [event catalog](data/event-catalog.md),
  [Postgres schema](data/postgres-schema.md), [configuration](operations/configuration.md),
  [observability](operations/observability.md), [CLI reference](cli/reference.md),
  [roadmap](product/roadmap.md), [positioning](product/positioning.md) ("key" where it said
  "token"), [admission](architecture/admission.md) (the token formats in
  `secret.api_token`), [audit and lineage](architecture/audit-and-lineage.md) and
  [concepts](concepts.md) ("connection edge" or "connection proposal" where "connection" stood
  bare for an edge), [the overview](index.md),
  [CI and toolchain](engineering/ci-and-toolchain.md) and [upgrades](operations/upgrades.md)
  (the revision), and this page; outside `docs/`, `README.md` (using OML from claude.ai and a
  phone).
- **Not built.** No code changes with this entry; the implementation follows iteration 3, and the
  pure modules d16 names may land before it.

## 0.3.0-design

### 2026-11-03 — Use cases and the use-case call map (documentation)

A documentation change; no decision changes. The maintainer asked on 2026-11-03 for
comprehensive use-case documentation and for a client API designed by Occam's razor. Two pages
answer it. [Use cases](product/use-cases.md), on the client tier, gives every use case by journey
and user, with its goal, its story, what its user sees and its status, and a day of an agent and
a reviewer. The [use-case call map](engineering/use-case-call-map.md), on the engineering tier,
declares the `UC-` ids, maps each use case to the calls it needs today, derives the minimal
client call set, lists the cut, merge and default candidates, and states which decisions of the
proposed ADR-0039, the v0.3.2 API rewrite, it supports or contradicts.

- **Roadmap:** no row opens or closes.
- **No surface change:** `info.version` stays `0.3.0-design`.
- **Pages touched:** the two pages, `mkdocs.yml`, `llms.txt` and this page.
- **Not built:** the candidates are inputs to ADR-0039 and to the proposed records each names.

### 2026-11-03 — Every record and feature page names the release that built it or is planned to

A documentation change with no decision or surface change. [Releases](product/releases.md), a new
client page, is the one declaration site for when each feature and decision record shipped, read
from the tags v0.1.0 to v0.3.1, and for the plan the maintainer made on 2026-11-03: v0.4.0 (the API
rewrite, plugin setup, turn context, OAuth by connect code, the librarian, the quick start, a
documentation upgrade), v0.5.0 (guardrails, workarea transitions, claim edges, the workarea brief),
v0.6.0 (in-progress workareas) and what is parked. Every record carries a **Release:** line under
its Status line, and the [register](adr/index.md) a Release column; neither restates the page.

- **Pages touched:** the new page, `mkdocs.yml`, [llms.txt](llms.txt), every record, the register,
  the architecture, API, CLI reference and operations pages, `README.md`, `plugins/oml/README.md`
  and this page. The CLI reference and the deployment page no longer say that no tag has run the
  per-platform archive jobs; v0.3.1 ran them.
- **No surface change:** `info.version` stays `0.3.0-design`; no record's decision moves.

### 2026-11-03 — A quick start in one command per role (proposed ADR-0041), the installers, and the v0.4.0 documentation plan

[ADR-0041](adr/ADR-0041-a-quick-start-in-one-command-per-role.md), proposed on 2026-11-03 and accepted by the
maintainer on 2026-11-04 with release v0.4.0, decides how a person
goes from nothing to a first memory in one command per role. Its d1–d3 and d8 are built;
d4–d7, d9 and d10 are decided, not built. No `info.version` change: no HTTP surface moves.

- **Built.** `scripts/install/install.sh` and `install.ps1` install the CLI for the user without
  sudo. They check the archive against the release's `SHA256SUMS`, and `SHA256SUMS` with cosign
  when it is on `PATH`, and say which happened. They install the release they were cut with unless
  `--version` names another or `latest`, and they edit no profile.
  `ci.yml` job `install-smoke` runs them against v0.3.1 on Linux, macOS and Windows.
  `scripts/check_release.py` now fails a pull request if a script names a release file the
  release does not sign, misses a target, drops cosign's identity or drifts from the workspace
  version. The [CLI reference](cli/reference.md#installing-oml-and-what-differs-on-windows) shows
  the one line.
- **Decided.** `oml login` and `oml doctor` (from the proposed ADR-0039) as the onboarding step;
  `oml setup claude-code`, which refuses an admin key; `oml quickstart` for the docker profile;
  a redirect from `oml.doublegate.io`; the time targets.
- **Opened:** FR-68 and FR-69 on the [roadmap](product/roadmap.md).
- **Planned:** the [v0.4.0 documentation plan](engineering/docs-upgrade-v0.4.0.md), page by page.

### 2026-11-03 — Releases ship the Claude Code plugins and an agent kit (implementation)

An implementation milestone with no decision or surface change: each tag also ships the oml and
oml-admin plugins as an offline marketplace archive and as a marketplace pinned to the tag, and an
agent kit of MCP configurations for other agent tools, signed and summed with the binaries
([release artifacts](engineering/ci-and-toolchain.md#release-artifacts-sbom-signing-provenance)).

### 2026-11-03 — The API rewritten for v0.3.2, and the plugin configured in Claude Code (ADR-0039, ADR-0040, proposed)

[ADR-0039](adr/ADR-0039-the-api-rewritten-for-v0-4-0.md), proposed on 2026-11-03 and accepted by the maintainer
on 2026-11-04 with release v0.4.0, records the
maintainer's choice that day to rewrite the API while OML is pre-release instead of layering on it,
to ship the rewrite as v0.3.2, and the maintainer's answers to its open questions and to six
findings of the use-case call map. It decides the three MCP tools `oml_write`, `oml_read` and
`oml_workarea` in place of the eight served ones, with their field rules and annotations; the
events tail off MCP and kept on HTTP, the SDKs and a new `oml events`; no `oml_attach` tool, and
`oml_graph` as a lineage mode of `oml_read`; a `claim_key` the server derives from the authoring
key and the text when it is omitted, beside a required `idempotency_key`; `kind` defaulting to
`memory`; one `workarea` field in place of the `X-OML-Workarea` header, and one
`include_unreviewed` flag in place of `include_own_held`, which ADR-0030 adopts; a verdict's
`serve_id` defaulting to the reviewer's one open item; openapi.yaml holding only the routed
operations, with the 45 designed ones in an annex; the mapping from every served tool, route,
CLI verb and SDK method to its successor; one error shape; the old MCP tool names and CLI verbs
callable but hidden through v0.3.x and removed in v0.4.0, with no HTTP or SDK aliases; every
client started from `OML_URL` and `OML_API_KEY` alone, the CLI's `oml login <url>` storing that
pair owner-only, flag before environment before file, and a new `GET /v2/whoami` deriving the
tenant, role, key name and default workarea; `oml bootstrap claim --token-stdin` in place of the
built `--token`; the breaking changes and their migration; and the build plan. It amends ADR-0014
(d2), ADR-0022 (d2, d4, d6), ADR-0025 (d4, d12, d13), ADR-0028 (d1) and ADR-0029 (d2, d6, d8).

[ADR-0040](adr/ADR-0040-plugin-setup-in-claude-code.md), proposed the same day and accepted with release
v0.4.0 on 2026-11-04, records the
maintainer's choice to configure the plugin in Claude Code: a `url` option and a sensitive
`api_key` option kept in the keychain, the same pair as every other client; `.mcp.json` reading
`${user_config.*}` and nothing from the environment; the hooks reading `CLAUDE_PLUGIN_OPTION_*`
before `OML_URL` and `OML_API_KEY`; the key file piped to `claude plugin configure
--values-stdin`; and Claude Code 2.1.285 as the minimum version. It amends ADR-0028 (d1) and
ADR-0029 (d4). The records bind nothing before acceptance, and the amended records' Status lines
gain their back-pointers then.

- **Opened:** FR-65, FR-66 and FR-67 on the [roadmap](product/roadmap.md), marked proposed;
  FR-46 to FR-64 are used on other branches.
- **No surface change yet:** openapi.yaml, mcp-tools.json, http-api.md and errors.md keep the
  v0.3.0 contract until ADR-0039's build changes them together; `info.version` stays
  `0.3.0-design`, and the build moves it to `0.3.2-design`.
- **Pages touched:** the two records, their [register](adr/index.md) rows, `mkdocs.yml`,
  `llms.txt`, the roadmap and this page.
- **Not built.**

### 2026-11-03 — The lead serves as a role that owns nothing (implementation)

An implementation milestone; no decision changes. The code now builds d10's steps 2 to 4 of the
proposed [ADR-0034](adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md), the work
v0.3.1 carries. The record stays proposed: no Status line moves, `info.version` stays
`0.3.0-design`, no `spec-v0.3` tag is cut, and no package version moves until the release.

- **The split** (d2, d3, d6). Migration `0007_role_split.sql` grants the serving role only the
  DML each table's adapter statements use, named through `oml.serving_role`, and revokes every
  privilege on `_sqlx_migrations` and `CREATE` on `public` from `PUBLIC`
  ([postgres-schema](data/postgres-schema.md#privileges)).
- **The boot** (d4, d5). `oml-server-lead` requires `postgres.owner_url`
  (`OML_POSTGRES__OWNER_URL`), runs d5's checks, migrates on the owner pool, runs the checks
  again and closes that pool before its keyring and listener. A refusal exits non-zero before any
  listener binds, logged with `event` `role_split_refused`, `code` `postgres.role_split` and `reason`
  one of d5's nine names; a catalog read that failed refuses the same way with its `error` in
  place of a reason. The boot that serves logs `event` `role_split_booted` with `owner_role` and
  `serving_role`. The lines are plain text, not yet the JSON of
  [observability](operations/observability.md#structured-logging). No `oml doctor` field is added:
  a refused lead binds no listener for doctor to read.
- **Keys carrying `tenant_id`** (d12). Migration `0008_tenant_keys.sql` rewrites the five keys
  into tenant tables as `(tenant_id, col)` and drops the keys of `journal.api_key_id` and
  `api_keys.issued_by` ([postgres-schema](data/postgres-schema.md#keys-into-tenant-tables)).
- **Per-tenant FTS partitions** (d11, d13). Migration `0009_memories_partitions.sql`
  list-partitions `memories` by `tenant_id`, and the owner creates each new tenant's fenced
  partition before its genesis commits; a DDL that loses a lock contest answers
  `system.unavailable` with `details.reason` `partition_lock_timeout`, a reason
  [errors](api/errors.md) names at the record's acceptance. Every adapter statement on `memories`
  binds its tenant. No function is marked `LEAKPROOF`, so an FTS read scans its tenant's
  partition, and its latency per tenant size is unmeasured (FR-25)
  ([postgres-schema](data/postgres-schema.md#the-built-fts-index-per-tenant-partitions)).
- **The payloads** (d2, d7, d8). `docker-compose.yml` and the chart's kind job create `oml_owner`
  and `oml_server`, the chart's Secret carries both URLs, and `scripts/poc-demo.sh`, every Postgres
  suite and both SDK harnesses mint a uniquely named pair, which takes a superuser. Under a role
  that cannot create roles a suite prints `did not run: <role> cannot create roles`; CI sets
  `OML_TEST_REQUIRE_SPLIT=1`, which fails on it. `rls-spike` migrates as the lead does and records
  no failure, its two `estimate` checks passing over the partitions or failing the run (d9).
- **Not built or not run:** the live host's split (d7), not run; the bootstrap workload, which
  would take the migrations and the partition DDL from the lead (d4); the `oml doctor` role-split
  check [observability](operations/observability.md#per-profile-checks) designs for the
  standalone and docker profiles. CI's pinned PostgreSQL 17 has not run over the partitions.
- **No surface change:** no HTTP operation, error code or event type.
- **Pages touched:** [postgres-schema](data/postgres-schema.md), the
  [spike 3 findings](engineering/spikes/spike-3-rls.md) and the [M0 spikes](engineering/spikes/index.md)
  page, [SDKs](engineering/sdks.md), README, CONTRIBUTING, the SDK and chart READMEs,
  `release.yml`'s install notes, the Dockerfile's comment and this page.

### 2026-11-03 — v0.3.1 ships per-tenant FTS partitions with no `LEAKPROOF` wrapper (ADR-0034, proposed)

[ADR-0034](adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md), still proposed, joins the
v0.3.1 work and records as d13 what the maintainer decided on 2026-11-03: v0.3.1 ships d11's
per-tenant partitions with no superuser `LEAKPROOF` wrapper; search latency per tenant size is
measured first (FR-25), and the `LEAKPROOF` step is decided later on those numbers. The wrapper,
until now a rejected alternative, is deferred with that reason. d10's step 1 is recorded as done
by #8 and #9, released as v0.3.0, and its step 5 names v0.3.1 as the release that carries steps 2
to 4. The record's Context links the spike 3 findings, now in this tree, and d9 names the harness
by its path.

A confirming review narrowed the record: d11 states the DDL that creates a partition (`LIKE`, then
`ATTACH PARTITION` under a lock timeout, named by tenant id), that it runs with no serving
transaction open, the codes a failed DDL answers and the partition a failed genesis leaves; d2's owner
owns what the migrations create, extension members excepted, and d7's superuser path moves
`oml_name` and leaves `ltree` with the superuser; d8 names the local not-run and CI's
`OML_TEST_REQUIRE_SPLIT`; d10's step 2 adds the SDK harnesses, their jobs and the release notes, and
the demo mints its role pair instead of using d2's fixed names.

- **Roadmap:** no row opens or closes. At the record's acceptance FR-25's exit criterion names
  search latency at stated tenant sizes.
- **No surface change:** `info.version` stays `0.3.0-design`.
- **Pages touched:** the record, its [register](adr/index.md) row and this page.
- **Not built.**

### 2026-11-03 — The CLI ships apart from the server (implementation)

An implementation milestone; no decision changes, and no tag has run the new jobs. The maintainer
decided on 2026-10-31 that the `oml` CLI ships as release artifacts of its own.

- **Archives.** `.github/workflows/release.yml` replaces `oml-X.Y.Z-x86_64-linux.tar.gz` with
  `oml-cli-X.Y.Z-<target>` archives for two static musl Linux targets, two macOS targets and
  Windows, each built on a native runner, and `oml-server-lead-X.Y.Z-<target>.tar.gz` for x86_64
  and aarch64 Linux; one job writes a CycloneDX SBOM per archive and one `SHA256SUMS`, and signs
  every archive, SBOM and the sums with cosign under the existing identity
  ([CI and toolchain](engineering/ci-and-toolchain.md#release-artifacts-sbom-signing-provenance)).
  The image and chart jobs are unchanged.
- **Before a tag.** `ci.yml` job `cli-matrix` builds `oml`, runs `--version` and runs the CLI's
  unit tests on every CLI target's runner; `scripts/check_release.py` holds both workflows to the
  target list.
- **Windows.** The credentials file and the `--out` key file get a protected one-entry DACL for
  the current user on Windows, set on a handle opened with no sharing, and mode `0600` on Unix; any
  other platform refuses. The credentials default to `%USERPROFILE%` when `HOME` is unset. The
  plugin's POSIX-shell hook runs on Windows only under Git Bash; a portable command would change
  [ADR-0029](adr/ADR-0029-session-start-hook.md) d7 and is not made
  ([CLI reference](cli/reference.md#installing-oml-and-what-differs-on-windows)).

### 2026-10-31 — The lead serves as a role that owns nothing (ADR-0034, proposed)

[ADR-0034](adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md), proposed on 2026-10-31 and accepted
by the maintainer on 2026-11-03 with release v0.3.1, records what the maintainer decided that day on M0 spike 3's findings: `FORCE ROW LEVEL SECURITY`
fences table reads only, an owner/serving role split keeps the `pg_stats` view and DDL from every
workload, and `_sqlx_migrations` is ADR-0007 d2's one named exemption, reachable by the owner
alone. After the spike's first pinned PostgreSQL 17.11 run, the maintainer added that the FTS
index becomes per tenant and that foreign keys into tenant tables carry `tenant_id`, as
implementation fixes of what ADR-0007 d4 already decides. It decides the two roles and their
privileges, the lead's boot under `postgres.owner_url` and `postgres.url`, the startup refusal
`postgres.role_split` when one URL serves both, the one-time split of a single-role install with
the live host's entry marked not run, `memories` list-partitioned by `tenant_id` with a GIN index
per partition that the owner creates at tenant creation (d11), the composite keys (d12), the
checks, the `rls-spike` change and the build order after iteration 3. What the per-tenant index
leaves open, EXPLAIN's estimate and per-tenant catalog counts and ids, is FR-45. Numbers 0027 and
0030 to 0032 are taken by records on other branches. It amends ADR-0007 (d2), whose Status line gains the
back-pointer at acceptance; it binds nothing before then.

- **Opened:** FR-44 and FR-45 on the [roadmap](product/roadmap.md), marked proposed; FR-34 to
  FR-36 and FR-40 to FR-43 are used on other branches.
- **No surface change:** no HTTP operation, error code or event type; `info.version` stays
  `0.3.0-design`.
- **Pages touched:** the record, its [register](adr/index.md) row, `mkdocs.yml`, `llms.txt`,
  the roadmap and this page.
  security.md, configuration.md, deployment.md, upgrades.md, postgres-schema.md, retrieval.md,
  the critical path and ADR-0007 change at the record's acceptance, and did on 2026-11-03 with
  the register's D6 cell, FR-25's exit criterion and errors.md's `system.unavailable` row.
- **Not built.**

### 2026-10-31 — Names as keys (implementation)

An implementation milestone; no decision changes. The code now serves what the proposed
[ADR-0026](adr/ADR-0026-names-are-keys.md) decides on every verb this build serves, except what is
listed as unbuilt below, which also names the verbs ADR-0026 reaches that this build does not
serve. The record stays proposed: no Status line moves, `info.version` stays `0.3.0-design`, no
`spec-v0.3` tag is cut, and no package version moves until the release.

- **Classification at parse** (d2, d3). Every workarea, crew, key and tenant reference is an id in
  the RFC 9562 hex-and-dash form whose version digit is not `8`, or a name under the reference
  grammar, the derived form included; anything else is `request.validation` with `details.field`
  naming the field, so a path and an uppercase name are refused, never lowercased. A creation
  refuses the derived form, and a name that names no row is replaced by its version-8 unresolved
  value.
- **Storage** (d1, d5, d6, d10). Migration `0006_names.sql` adds the `crews` table and the `name`
  columns with their unique indexes, keeps the PoC's `tenants.name` and `workareas.name` as
  `prior_name`, ties `universe` to the root with a CHECK, and names nothing. The unique index is
  `name.taken`'s authority, so a creation that loses a race journals nothing, and a revoked key
  keeps its name, which still resolves; the one revocation this build journals is the server's
  own, of an issuance that failed after its `key.issued`.
- **Resolution and crews** (d4, d5, d8, d9, d15). Every use case resolves the workareas, crews and
  keys it is given after the verb ceiling and before any read, so an unknown or unreadable name
  answers `access.not_found` in the same step, with the same code and details, as its id would.
  Admin keys create, read and update crew records, journalled as `crew.created` and `crew.updated`;
  `workarea.create` and `workarea.crew_updated` journal `crew_id`, and `key.issued` journals the
  key's `name` and `crew_id`. The bootstrap claim and `POST /v2/keys` refuse a body with no name,
  or one the creation grammar refuses, as `request.validation` with `details.field` `/name`, the
  claim only after its enrollment token. An admin key reads workarea structure across its tenant by
  key role, the workarea read listing every child with `my_effective_verbs` empty, and still
  answers `access.verb_ceiling` on every claim read; a global denial on an admin key is refused.
- **The lead's start** (d11, d15). Before it serves, the lead names every row that predates names,
  in one `system.names_assigned` per chain: the root `universe`, a usable creation-time name kept,
  every other row in the derived form, and each distinct crew object one crew record. In the same
  commit it lifts each global denial on an admin key, one `key.denial_lifted` apiece with
  `reason_digest` over `adr-0026-d15`, folded into `key_denials.lifted_epoch`. A second start
  journals nothing; a lead that leaves a row unnamed reports `names_assigned` not ready on
  `/readyz` and answers every other route `system.unavailable`.
- **HTTP and MCP** (d4, d5, d8, d9, d15). Every header, path parameter, query parameter and body
  field the transport takes for a workarea, crew or key takes its id or its name, among them
  `X-OML-Workarea`, `{workarea_id}`, `{key_id}`, the listing's `workarea_id`, `parent_id`, a
  grant's `workarea_node_id`, a denial's `subtree_node_id`, `default_workarea_id` and `crew`.
  `name.taken` answers 409; `GET` and `POST /v2/crews`, `GET /v2/crews/{crew_id}`,
  `POST /v2/crews/{crew_id}/update`, `POST /v2/workareas/{workarea_id}/crew` and
  `GET /v2/workareas/{workarea_id}/tree` are served, the tree a page at a time under a cursor
  bound to the key, an admin key reading every node by its key role and a reader key every node
  within its read access; workarea responses carry `name` and `crew`, the crew record or null, and
  key responses `name`; and `GET /v2/keys/{key_id}` answers a key by its id or its name. A
  `workarea` on `POST /v2/review/next`, by id or by name, is refused as `request.validation` with
  `details.field` `/workarea`, never ignored, because the queue filter is not built and the queue
  is served whole. The served entries of [mcp-tools.json](api/mcp-tools.json) take a workarea's
  name and their `_meta.pending_design` notes for names are gone, and `oml_workarea_info` answers
  `crew` null for a workarea that names none.
- **CLI** (d2, d3, d4, d8, d12, d15). Every argument that names a workarea, crew, key or tenant
  takes its id or its name, sent as typed for the server to resolve, as one percent-encoded segment
  where the route takes it in its path; one a path carries that is empty, `.` or `..` is refused
  before anything is sent, exit `2`, since no path carries it as one segment. Among them are the
  `--grant <workarea>:<role>`, `--default-workarea`, `--workarea` and `--crew` of
  `oml keys issue <key-name>` and `oml keys issue-agent <key-name>`, `oml keys show <key>`,
  `oml deny set <key> <subtree>`, `oml submit --workarea`, `oml recall --workarea` and
  `oml audit verify-chain --tenant`. `oml keys issue-agent` keeps
  [ADR-0028](adr/ADR-0028-agent-key-issuance.md) d1's `--out`, d2's reader grant and `--write-only`;
  `oml crews create`, `update`, `list` and `show` administer crew records;
  `oml workareas create <parent> <name>` takes `--crew`, and `oml workareas crew set <workarea>
  --crew <crew>` points a workarea at another crew record; `oml workareas show` and `tree` run
  under an admin context, `tree` reading every page of the tree read; and the human layout of
  `oml keys show` prints a `revoked` line, the key's revocation time or `no (live)`.
- **Plugins** (d14). The `oml-admin` skill asks for the key's name and the workarea's name and
  passes them to `issue-agent`, with `--crew` when the operator names a crew, still only with
  `--out` and never with `--json`. The `oml` plugin's README and skill say that a workarea
  argument takes the workarea's name or its id, and the [agent plugin](api/mcp-plugin.md) page's
  `issue-agent` synopsis carries the key name. `scripts/check_plugin.py` now also fails if the
  skill's `issue-agent` run does not name its key right after the verb with a name the creation
  grammar admits, or if a command on a plugin page passes `--workarea` anything but a workarea
  name or `<workarea>`.
- **SDKs and the chart** (d1, d10, d12). Both generated layers of the [SDKs](engineering/sdks.md)
  are regenerated from this `openapi.yaml`, as their regeneration rule asks; both wrappers raise
  `NamingError` for the `name` family, named so beside Python's `NameError`; and both test harnesses
  name the keys they claim and issue. The Helm chart's install notes and the
  [deployment](operations/deployment.md) page's claim through a port-forward run
  `oml bootstrap claim <key-name>`.
- **Checks:** `crates/oml-domain/tests/names.rs`, the port contracts of
  `testing/oml-port-contracts`, which both stores run, `crates/adapters/oml-pg/tests/pg_store.rs`,
  `crates/oml-app/tests/use_cases.rs`, `crates/bins/oml-server-lead/tests/http_api.rs` and
  `mcp_facade.rs`, and `crates/bins/oml-cli/tests/cli_e2e.rs` run in `cargo test --workspace`.
- **Still unbuilt.** The verbs ADR-0026 reaches that this build does not serve, so no name reaches
  them either: key revocation (`POST /v2/keys/{key_id}/revoke`, `oml keys revoke`) and rotation, and
  so `new_name` on `key.rotated` and `oml keys rotate <key> <new-key-name>`; the key listing
  (`GET /v2/keys`) and `oml keys list`, so d10's revoked key is read as history only through
  `GET /v2/keys/{key_id}` and `oml keys show`; of the other verbs d9 addresses by the key's name,
  adding or revoking a grant after issuance, lifting a denial and the effective-access read; d15's
  lookup by path (`GET /v2/workareas?path=`) and its `edges` and `status` reads under
  `GET /v2/workareas/{workarea_id}`; the re-parent's `new_parent_id`, a workarea edge's
  `to_workarea_id` and the reassign's `workarea_id`; the queue filter, `workarea` on
  `POST /v2/review/next`, refused by name above; and a simulation audience's `key_id`. The
  operator tier is unbuilt, and so are tenant names beyond the one tenant the bootstrap claim
  creates, operator keys and the provider's key namespace, `tenant_id` on issuance and
  `POST /v2/tenants`. So are the independence axis's crew and manager comparisons at promotion,
  whose domain functions exist and which no promotion calls yet. Two pieces differ from the design
  because the operator tier is unbuilt: the PoC has no provider chain, so each tenant's name is
  journalled in its own chain's `system.names_assigned`; and the bootstrap claim mints the tenant's
  first admin key, so its `name` is checked in that tenant's namespace, not the provider's. Two
  listings are ordered otherwise than the [pagination](api/http-api.md#pagination) convention's
  `created_at` descending: the tree is served in path order, a node before its children and
  siblings by name in byte order, its cursor carrying the last path served, where newest first
  would list a node's children before it; and the crew listing is served in name order.
- **Pages touched:** this page and the [agent plugin](api/mcp-plugin.md) (the `issue-agent`
  synopsis); [mcp-tools.json](api/mcp-tools.json) and the [CLI reference](cli/reference.md)
  change with the code that serves them; the [runbooks](operations/runbooks.md) spell every key
  argument `<key>`, as the CLI reference does; outside `docs/`, `README.md` (the honest status),
  `plugins/oml/README.md`, `plugins/oml/skills/oml-memory/SKILL.md`,
  `plugins/oml-admin/skills/issue-agent-key/SKILL.md` and `scripts/check_plugin.py`; and, with
  the SDKs and the chart, [SDKs](engineering/sdks.md), [deployment](operations/deployment.md),
  `sdks/` and `deploy/helm/oml/templates/NOTES.txt`.

### 2026-10-30 — names as keys (ADR-0026)

[ADR-0026](adr/ADR-0026-names-are-keys.md), accepted by the maintainer on 2026-10-31 with release v0.3.0, gives workareas, crews, keys and tenants a
name that is a unique, immutable key beside the id. It proposes amending ADR-0008 (d3, d7), ADR-0010
(d6, d7), ADR-0012 (d4), ADR-0018 (d4), ADR-0022 (d4), ADR-0025 (d1, d2, d14) and ADR-0028 (d1, d3);
each of those records' Status lines gains the back-pointer when ADR-0026 is accepted.

- **Decided:** one flat namespace per kind — the tenant for workareas, crews and keys (operator keys
  share the provider's), the deployment for tenants — with names never changed and never released,
  and the reserved provider tenant carrying none; a lowercase RFC 1035 label as the grammar,
  refusing every spelling that could read as an id, with no case mapping, and the derived form of
  rows that predate names refused at creation and accepted in a reference; the ids of the four kinds
  fixed as RFC 9562 hex-and-dash UUID text, minted as version 4 and never version 8, so a value in
  that form is always an id; names accepted wherever an id is — `/v1` and `/v2` path parameters,
  query parameters and body fields, `X-OML-Workarea`, MCP tool arguments and CLI arguments — and
  resolved on the server by substituting the id after the verb ceiling, in a namespace fixed by the
  caller and the kind, so an unknown or unreadable name answers exactly what its id would, with no
  refusal naming an id a name resolved to beyond what the naming rule already discloses, and an
  unknown name takes a deterministic version-8 unresolved value of its own that shares no
  idempotency scope; the request digest over paths and bodies as sent, with `X-OML-Workarea` outside
  it and resolved into the provider ring's record scope; `name.taken` (409) for a taken name at
  creation, which only admin and operator keys, and the holder of a valid enrollment token at the
  bootstrap claim, can receive; an operator key's operator keys issued without `tenant_id` and named
  in the provider's namespace, and its first admin key for a tenant refused once that tenant has
  held any key; the root named `universe` in every tenant; paths derived from names, served and
  looked up but never accepted as a reference; crews as named records that workareas and keys
  reference, created and updated by admins only, with the independence axis reading a key's crew and
  failing closed where none is named; a key's name distinct from its principal and label, and a
  rotation's successor named afresh; the naming of rows that predate names by a deterministic rule
  journalled as `system.names_assigned`, which lists kept names too, read before the creation events
  it names by a re-fold, with the PoC's name columns kept as `prior_name`, every pre-names denial on
  an admin key lifted in the same commit, and followers not ready until the event is folded; the
  key's name as the positional argument of `oml keys issue-agent` and its crew as `--crew`, and
  every creating CLI verb's name positional, `oml bootstrap claim` included (d12), while the verb,
  its `--out` key file, reader grant default and `oml-admin` plugin moved to
  [ADR-0028](adr/ADR-0028-agent-key-issuance.md) so they ship before names (d13 and d14 point
  there); and admin keys reading the structure of every workarea in their tenant without grants —
  the workarea read, the lookup by path, the tree, edges and status — and never claim content, with
  no global denial on an admin key, declared in the roles-and-keys permission matrix (d15).
- **Surface:** the `name` error family with `name.taken` (errors.md, the OpenAPI `ErrorCode` enum
  and http-api.md's tables together); `GET /v2/crews`, `POST /v2/crews`, `GET /v2/crews/{crew_id}`
  and `POST /v2/crews/{crew_id}/update`; the `crew.created`, `crew.updated` and
  `system.names_assigned` events; `crew_id` in place of the crew object on `workarea.create` and
  `workarea.crew_updated`; `name` and `crew_id` on `key.issued`, `new_name` on `key.rotated`,
  `reason_digest` on `key.denial_lifted`; the `oml_name` domain, the `tenants` and `crews` tables
  and the `name` columns with their unique indexes, and the PoC's name columns kept as `prior_name`;
  the OpenAPI `Id` refusing version 8; the OpenAPI `NameRef` schema for references beside `Name` for
  creations; `name` and `crew_id` on the lake's `gold.areas` in place of its three crew columns.
  Example ids of the four kinds are rewritten to UUID text on the OpenAPI document, the agent
  plugin, the reviewer-crews pages and the CLI sessions; the other example ids are untouched.
- **`info.version` 0.2.0-design → 0.3.0-design**, because `/v1` changes on the request side:
  `X-OML-Workarea` and the listing's `workarea_id` accept names, and a value that is neither an id
  nor a name, which 0.2.0-design answered `access.not_found`, now answers `request.validation`. That
  narrowing is permitted only because `/v1` is not frozen (FR-21). The 0.2.0-design revision was
  tagged `spec-v0.2` when ADR-0025 was accepted, so this record cannot share its bump; `spec-v0.3`
  is cut when ADR-0026 is accepted.
- **Pages touched:** [ADR-0026](adr/ADR-0026-names-are-keys.md), the
  [decision register](adr/index.md) (row and the D7, D9, D13 and D14 cells),
  [workareas](architecture/workareas.md), [roles and keys](architecture/roles-and-keys.md),
  [concepts](concepts.md), [HTTP API](api/http-api.md), [OpenAPI](api/openapi.yaml),
  [errors](api/errors.md), [mcp-tools.json](api/mcp-tools.json) (unserved tools' descriptions and
  `_meta.pending_design`; the tool count is unchanged), [agent plugin](api/mcp-plugin.md),
  [reviewer crews](api/reviewer-crews.md) (example ids), [PostgreSQL schema](data/postgres-schema.md),
  [event catalog](data/event-catalog.md), [Delta tables](data/delta-tables.md) (`gold.areas`),
  [CLI reference](cli/reference.md),
  [CLI sessions](cli/sessions.md), [deployment](operations/deployment.md) and
  [runbooks](operations/runbooks.md) (the CLI invocations whose arguments changed),
  [observability](operations/observability.md) (the `names_assigned` doctor line),
  [llms.txt](llms.txt), this changelog, `mkdocs.yml` and `CONTRIBUTING.md` (the vocabulary row);
  the spec version also on the [home page](index.md),
  [CI and toolchain](engineering/ci-and-toolchain.md) and [upgrades](operations/upgrades.md),
  which also states that the upgrade adding names stops every earlier binary before it migrates.
- **Roadmap:** no row opens or closes. Name resolution's request-path cost is unmeasured and is
  tracked against FR-25.

## 0.2.0-design

### 2026-10-31 — M0 spikes: the Rust tests' copies of the vectors are compared with the page, S3.2 is two findings, and EXPLAIN's estimate is a recorded cross-tenant disclosure

A second review fix to the M0 spike findings and harnesses. No decision changes, and no record is
amended or proposed.

- **Chain vectors:** the M0 spikes page said the repository held no second copy of the vectors.
  It holds three: the Rust tests in `crates/oml-domain` and `crates/bins/oml-cli` keep the
  vectors as literals, and each Rust test checks the code against its own copy, so until now the
  copies agreed with the page only because they had been copied by hand.
  `scripts/check_chain_vectors.py` now finds every Rust file that copies the vectors and compares
  each copy with the page. It reads `crates/` and changes nothing there. Run from a scratch tree,
  the check as first committed passed with a drifted copy. As it stands, it fails on six drifted
  copies and passes on the repository's three files (6 `signed_bytes` literals, 34 hex values).
  Having the Rust tests read the page is a follow-up for `feat/iteration-3`
  ([finding](engineering/spikes/chain-vectors.md#the-rust-tests-copies)).
- **S3.2:** S3.2 is now two findings. The first is PostgreSQL's mechanism: a non-owner role
  without `BYPASSRLS` stays inside the policy, and migrations succeed under the owner. It can
  pass in CI. Its controls are the refused DDL taking effect for the owner, rolled back, and the
  service role's rows under tenant B. The `BYPASSRLS` control role is gone. The second is the
  claim about OML's own roles. It is **inconclusive, not testable** on every host, because the
  harness's roles are its own and no page specifies a deployment's roles and grants, so a CI run
  never prints `pass` and `none` for it. The harness's role attributes are now preconditions,
  not checks ([findings](engineering/spikes/spike-3-rls.md#s32)).
- **S3.1 and S3.4:** the page claimed that the role split confines the statistics leak to the
  owner. Its own S3.4 observation contradicted that. EXPLAIN's row estimate under tenant A follows
  tenant B's words, and by PostgreSQL's documentation and the `REL_17_STABLE` source it does so for
  every role the policy binds. The estimate is now the check `S3.4/<role>/estimate`. The owner's
  failure is recorded. The service role's is not, so CI fails if it leaks. Both findings now also
  cite ADR-0007 d4 and security.md's STRIDE information-disclosure mitigation ("no cross-tenant
  index to leak through"), which the shared index `memories_claim_fts` breaks. Their action is
  ADR amendment needed, or an implementation fix that makes the FTS index per tenant, awaiting
  the maintainer. The designed `memories_fts` and `held_fts` are not partitioned per tenant
  either.
- **Harness fix found on the way:** the owner's `UPDATE journal` observation ran with no tenant
  set, matched no row, and could not show the guard trigger. It now runs under tenant A.
- **Pages touched:** [M0 spikes](engineering/spikes/index.md),
  [chain vectors](engineering/spikes/chain-vectors.md), [spike 3](engineering/spikes/spike-3-rls.md),
  [critical path](product/critical-path.md) (M0 progress),
  [CI and toolchain](engineering/ci-and-toolchain.md) (the gate row), `docs/llms.txt` and this
  page. Outside `docs/`, the two harnesses.
- **Not verified here:** any pg17 or service-role result, S3.2's mechanism among them. CI jobs
  `chain-vectors` and `rls-spike` have not run. The S3.2 code path was run only with the owner
  standing in for the service role, which the preconditions reported as not as minted.

### 2026-10-31 — M0 spike 3: a failed scenario says ADR amendment needed, the lead run is recorded, and the harness follows new migrations

A review fix to the spike 3 findings and harness of the entry below. No decision changes, and no
record is amended or proposed.

- **Findings:** S3.1 and S3.4 now say **ADR amendment needed** against ADR-0007 d2 and
  security.md's `FORCE` commitment, awaiting the maintainer's decision. The earlier wording said
  no amendment was needed and that the maintainer had confirmed this, and the maintainer had not.
  `_sqlx_migrations`, which has no RLS, no `FORCE` and no policy, is recorded as ADR amendment
  needed (ADR-0007 d2) or an implementation fix. The run of `oml-server-lead` 0.2.0 behind both
  claims is now on the page: the command, the configuration shape and the catalog output, which
  show the serving role owning all 12 tables
  ([findings](engineering/spikes/spike-3-rls.md)).
- **Harness:** `testing/spikes/rls/spike_rls.py` prints the same action for a failure. It records
  which tables its seeders write. A table that a later migration adds and no seeder writes is
  reported as unprobed for row visibility, and it does not fail the run. The no-tenant write and
  the probe function no longer depend on the 0001–0005 column shapes. Run on PostgreSQL 18.6 with
  `feat/iteration-3`'s `0006_names.sql` added from scratch, the harness as first committed failed
  S3.1 (`crews` empty) and S3.3 (`23514`). As fixed, it gives the 0001–0005 verdicts and reports
  `crews` as unprobed. It still gives the 0001–0005 verdicts on this branch's migrations.
- **Pages touched:** [M0 spikes](engineering/spikes/index.md) (how the harness follows the
  migrations, and two more rejected alternatives), [spike 3](engineering/spikes/spike-3-rls.md),
  [critical path](product/critical-path.md) (M0 progress) and this page. Outside `docs/`, the
  harness.
- **Not verified here:** any pg17 or service-role result. CI job `rls-spike` has not run.

### 2026-10-31 — M0: the chain-format vectors reproduce outside Rust, and spike 3 runs on PostgreSQL 18.6

An M0 change: two of the critical path's de-risking experiments are built and run, and their
findings are recorded. No decision changes, no record is amended or proposed, and the harness
decisions live on an engineering page, [M0 spikes](engineering/spikes/index.md).

- **Built:** `scripts/check_chain_vectors.py`, which rebuilds the three
  [chain format](data/chain-format.md) vectors from their inputs with two RFC 8785
  implementations (`rfc8785`, `jcs`), two Ed25519 implementations (`cryptography`, PyNaCl) and
  two SHA-256 implementations (`hashlib`, libsodium). It runs the verification procedure and its
  mutation claims, and fails on any difference. Also built: `testing/spikes/rls/spike_rls.py`,
  which applies `migrations/*.sql` to a throwaway database and asserts S3.1–S3.4. Both pin their
  libraries through uv, inline and in a committed lock file. `.github/workflows/m0-spikes.yml`
  adds the jobs `chain-vectors` and `rls-spike`, the latter on `pgvector/pgvector:pg17`.
- **Found:** the vectors reproduce byte for byte
  ([finding](engineering/spikes/chain-vectors.md)). On PostgreSQL 18.6, as the table owner, every
  read shape of the fold tables stayed inside its tenant. A read with no tenant set returns zero
  rows, which is now the recorded behaviour. `pg_stats` shows the owner other tenants' lexemes
  through the expression index `memories_claim_fts`, so S3.1 and S3.4 fail for the owner, and the
  harness records both as expected failures. Under RLS that GIN index serves no FTS plan, since
  `@@` is not leakproof. S3.2 did not run, for lack of a role that can create roles
  ([findings](engineering/spikes/spike-3-rls.md)).
- **Page edit:** chain format now says that omitting the last event fails step 9, against the
  stated head, and not step 1 or 5. The check's first run failed on the old sentence.
- **Not verified here:** any result on the pinned pg17, any result for a non-owner role, and
  S3.2. The two CI jobs have not run. Spikes 1, 2 and 4 are not run.
- **No surface change:** no HTTP operation, error code, event type or claim state changes, and
  `info.version` stays `0.2.0-design`.
- **Pages touched:** [M0 spikes](engineering/spikes/index.md) and its two finding pages (new),
  [chain format](data/chain-format.md), [critical path](product/critical-path.md) (M0
  progress), [CI and toolchain](engineering/ci-and-toolchain.md) (the M0 spikes gate),
  `mkdocs.yml`, [llms.txt](llms.txt) and this page. Outside `docs/`, the two scripts, their lock
  files and the workflow.

### 2026-10-31 — A Helm chart for the current build's one lead (ADR-0033)

[ADR-0033](adr/ADR-0033-interim-helm-chart.md), proposed on 2026-10-31, decides what a Helm
chart holds while the build has one server binary, and this change builds it. Numbers 0030 to
0032 are taken by records on other branches. It amends ADR-0016 (d4) and ADR-0023 (d16) for this
chart only; their Status lines gain the back-pointer when the record is accepted.

- **Decided and built:** `deploy/helm/oml`, the first revision of the one chart ADR-0016 d2
  names, holding `oml-server-lead` alone (d1): one replica under `Recreate` in place of the lead
  lease, with the chain lock as the fence (d2); the keyring on a ReadWriteOnce claim mounted at
  `/var/lib/oml/keyring`, kept by `helm uninstall` and reattachable through
  `keyring.existingClaim` (d3); uid and gid 999, the image's `oml` user, with `fsGroup: 999` under
  `fsGroupChangePolicy: OnRootMismatch`, because the default policy turns the 0600 keyring into
  0660 and the lead refuses it (d4); `runAsNonRoot`, a read-only root filesystem, every capability
  dropped, seccomp `RuntimeDefault`, no service account token, resources as comments, and an
  optional NetworkPolicy (d5); `OML_POSTGRES__URL` from an existing Secret's key, never a value
  (d6); startup and liveness probes on `/healthz` and readiness on `/readyz` (d7); the Ingress
  off by default, and install notes that give the claim of the enrollment token through
  `kubectl port-forward` before any Ingress, and warn when an install enables one (d8);
  `version` = `appVersion` = the workspace version, the image
  `ghcr.io/e8kor/open-memory-lab:<appVersion>`, pinned by `image.digest` when set (d9); and the
  release and CI jobs (d10).
- **Checks:** `scripts/check_chart.py` runs `helm lint --strict` and `helm template` on the
  default values, `ci/kind-values.yaml` and the optional features, and fails on a version
  mismatch, a duplicated mapping key, or the loss of any property d2–d8 decide, NOTES.txt's
  included; with `--image-user` it also requires the pod's ids to be the built image's.
  `scripts/check_release.py` now also fails if `release.yml` stops checking the chart's version
  against the tag, packaging it, signing it with `cosign sign-blob` or uploading the archive and
  its bundle to the release. `ci.yml` gains job `chart`: kind, `pgvector/pgvector:pg17` in the
  cluster, the image built from the checkout and loaded, `helm install`, the claim through
  port-forward, `scripts/quickstart.sh`, then a restart that must keep the public key, answer
  `/readyz` 200 and print no token, and an uninstall that must leave the keyring claim and a
  reinstall that must load the same key. `release.yml` gains jobs `chart` and `chart-release`; its
  other jobs are unchanged.
- **Not run here:** the kind job and the release jobs. This change ran `helm lint`,
  `helm template`, a strict Kubernetes schema validation of the rendered objects, both checkers
  and `actionlint` locally, and the quickstart against the installed lead the way the kind job
  drives it; the pull request's CI run is the kind job's first, and the first tag after it is the
  release jobs' first. d4's fsGroup behaviour is read from the kubelet's source and the
  Kubernetes documentation, not observed on a cluster.
- **No surface change:** no HTTP operation, error code, event type or claim state; `info.version`
  stays `0.2.0-design`. The chart is at the workspace version, 0.2.2, and moves with it.
- **Pages touched:** [ADR-0033](adr/ADR-0033-interim-helm-chart.md), the
  [decision register](adr/index.md) (row and the D15 cell), `mkdocs.yml`, [llms.txt](llms.txt),
  [deployment](operations/deployment.md) (the interim chart's section and the keyring backup
  row), [CI and toolchain](engineering/ci-and-toolchain.md) (the chart gate and the release
  artifact) and this page; outside `docs/`, `deploy/helm/oml/`, `deploy/k8s/README.md`,
  `scripts/check_chart.py`, `scripts/check_release.py`, `.github/workflows/ci.yml` and
  `.github/workflows/release.yml`.

### 2026-10-31 — Python and TypeScript clients generated from openapi.yaml (implementation, FR-12)

[Client SDKs](engineering/sdks.md), a new engineering page, decides how the client half of
[FR-12](product/roadmap.md) is made and kept current, and this change builds it. FR-12's batch
half, `POST /v1/memories:batch`, changes the server and is not part of it. No ADR records the
decision: FR-12 already decides that the clients are generated, and the page decides the
generators, the package names, the wrapper and the regeneration rule, with the rejected
alternatives. It amends no record.

- **Decided and built:** `sdks/python`, the distribution `oml-sdk` imported as `oml_sdk`,
  generated by openapi-python-client 0.29.1, and `sdks/typescript`, the npm package `oml-sdk`,
  typed by openapi-typescript 7.13.0 over openapi-fetch 0.17.0, with every generator pinned and
  locked in `uv.lock` and `package-lock.json`; neither is released to a registry. Over each, a
  hand-written wrapper that takes the key from the caller or `OML_API_KEY`, sends one
  `Idempotency-Key` per write and reuses it on every retry of that call, follows `next_cursor`
  through the listing, raises one exception class per [errors.md](api/errors.md) family carrying
  `code`, `rule`, `message` and `retryable`, and refuses uploads with `NotBuiltError`, because the
  server serves no upload sessions yet.
- **The regeneration rule:** `scripts/generate_sdks.py` regenerates both generated layers from
  openapi.yaml, and with `--check` fails on stale generated code or on a change to the Python
  generator's recorded warnings, so a change to openapi.yaml regenerates both clients in the
  same change.
- **Checks:** `sdks/python/tests` (pytest) and `sdks/typescript/test` (`node --test`) run against
  a real `oml-server-lead` on a throwaway database: remember, held, the author's own recall, a
  dreamer key's approve verdict, recall; a write whose response was lost, retried and replayed
  under the same key; a key reused with another body, refused and not retried; the listing
  iterated across pages; the error mapping. `.github/workflows/ci.yml` gains three jobs,
  `sdk-python`, `sdk-typescript` and `sdk-generated`, and changes none.
- **Not verified here:** the three CI jobs, which have not run yet. Locally both suites passed
  against the installed `oml-server-lead` 0.2.0 and `oml` 0.2.2, the Python suite on Python 3.11
  and 3.13 and the TypeScript suite on Node 24.
- **Not built:** the batch write path and the upload wrapper, which keep FR-12 open; and the
  Python generated layer types no `Workarea`, `WorkareaPage` or `WorkareaView`, the generator gap
  the SDK page records.
- **No surface change:** no HTTP operation, error code, event type or claim state; `info.version`
  stays `0.2.0-design`.
- **Pages touched:** [client SDKs](engineering/sdks.md), new; [HTTP API](api/http-api.md) and
  [errors](api/errors.md), which link it; the FR-12 status on the [roadmap](product/roadmap.md),
  the M2 progress on the [critical path](product/critical-path.md),
  [CI and toolchain](engineering/ci-and-toolchain.md), `mkdocs.yml`, [llms.txt](llms.txt) and this
  page; outside `docs/`, `README.md`, `sdks/`, `scripts/generate_sdks.py` and
  `.github/workflows/ci.yml`.

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
