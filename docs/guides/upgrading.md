<!--
id: guides/upgrading
tier: engineering
status: draft for review · describes the upgrade to v0.4.0
answers: how a user and an operator move from v0.3.x to v0.4.0: the old-to-new tool, verb and setting table, the alias window and its undecided reading, the break list, claude plugin update and the one plugin configuration, the credentials file, and the server upgrade
-->

# Guide: upgrading from v0.3.x to v0.4.0

**Status:** draft for review · describes the upgrade to v0.4.0

**Release:** since v0.4.0 ([releases](../product/releases.md))

v0.4.0 rewrites what agents and clients call and leaves the data alone: keys, grants, stored
memories and the journal do not change
([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d13). The schema only grows: the
server runs two migrations for connections at start
([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md)), with no step of yours. The server, the `oml` CLI and
the Claude Code plugin move together. The mechanics of the server's side are on
[upgrades](../operations/upgrades.md#server-cli-and-plugin-together-the-upgrade-to-v040); this page is
the order for each person.

## Upgrading from v0.3.x: what was renamed

The complete mapping is ADR-0039 d10, and the break list with each migration is d13, repeated in
the [changelog](../changelog.md). The names a user meets:

| v0.3.x | v0.4.0 |
|---|---|
| `oml_remember`, `oml_learn` | `oml_write`; `derives_from` makes it a conclusion |
| `oml_supersede` | `oml_write` with `replaces`, and `retirement: true` to propose retiring |
| `oml_recall`, `oml_get`, `oml_my_work` | `oml_read`: `query` searches, `artifact_id` fetches, neither lists your own work |
| `oml_workarea_info` | `oml_workarea` |
| `oml_events_tail` | `oml_read` with `state` for your own writes; `oml events` or `GET /v2/events` to audit |
| `include_own_held`, `scope`, `X-OML-Workarea` | `include_unreviewed`, `workarea` |
| `oml submit`, `oml recall`, `oml supersede` | `oml write`, `oml read --query`, `oml write --replaces` |
| `oml review verdict --approve`/`--reject` | `oml review approve`, `oml review reject --reason` |
| `--server`, `--key`; `OML_CLI_SERVER`, `OML_CLI_KEY` | `--url`, `--api-key`; `OML_URL`, `OML_API_KEY` |
| `oml bootstrap claim … --token <secret>` | `oml bootstrap claim … --token-stdin` |
| plugin read from exported `OML_URL`, `OML_API_KEY` | plugin settings `url` and `api_key`, the key in the keychain |

**The alias window, closed in v0.5.1.** Through v0.5.0 the eight old MCP tool names and the old
CLI verbs and flags still answered, hidden: each old MCP call logged `mcp_alias_called` naming the
calling key, and each old CLI verb printed one stderr line naming its successor. v0.5.1 removed
them. An old tool name now answers JSON-RPC -32602 `Unknown tool` like any name the server does
not serve, and an old verb or flag is the CLI's usage error, exit `2`, with nothing sent.
`OML_CLI_SERVER` and `OML_CLI_KEY` are not read at all; a missing setting names the variable to
rename. HTTP and the SDKs kept no aliases (ADR-0039 d12).

## Upgrading from v0.3.x as a user of Claude Code

1. Install the v0.5.2 CLI: `curl -fsSL --proto '=https' --tlsv1.2 https://raw.githubusercontent.com/e8kor/open-memory-lab/v0.5.2/scripts/install/install.sh | sh`.
2. `oml login <url>` once. It rewrites the credentials file with `url` and `api_key`; a v0.3 file's
   `server` and `key` were read through v0.5.0 and are not read since v0.5.1.
3. `oml setup claude-code`. It finds the `open-memory-lab` marketplace you added, pinned at a v0.3
   tag or at none, removes it, adds it at the CLI's own tag, `v0.5.2`, and installs again each plugin the removal
   uninstalled, then configures the plugin; a marketplace from another source, such as a local
   checkout, is refused as `setup.marketplace`, nothing changed
   ([CLI reference](../cli/reference.md#oml-setup-claude-code-and-oml-quickstart-the-quick-start)).
   Without the CLI, run `claude plugin update oml@open-memory-lab` and `/plugin configure
   oml@open-memory-lab`. Until both settings are set the plugin serves no tools.
4. Replace the `permissions.allow` rules that named the v0.3 read tools with
   `mcp__plugin_oml_oml__oml_read` and `mcp__plugin_oml_oml__oml_workarea`, or with
   `mcp__plugin_oml_oml` for all three.
5. Remove the exported `OML_URL` and `OML_API_KEY` from your shell profile, unless another client
   still reads them. Run `/reload-plugins`.

## Upgrading from v0.3.x as an operator

1. Back up and upgrade the server as [upgrades](../operations/upgrades.md#the-upgrade-order)
   orders it. A v0.3.0 install that still serves as one database role is split first
   ([splitting a single-role install](../operations/upgrades.md#splitting-a-single-role-install-the-upgrade-to-v031),
   [ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d7).
2. Install the v0.4.0 CLI on every host that runs `oml`, including each agent host the
   session-start hook runs on.
3. Move scripts to the new verbs, and enrollment to `--token-stdin`, which keeps the secret out of
   argv and shell history.
4. Since v0.5.1 no log line names the clients that still call an old tool, since `mcp_alias_called`
   was removed with the old names: such a call logs `mcp_call` with `tool` `unknown` and
   `error_code` `-32602`, under the calling key's `key_id`
   ([observability](../operations/observability.md)).
5. Check with `oml doctor`.

Rolling back means the v0.3 server, CLI and plugin together. The two migrations stay, and no
v0.3 binary has been run on the v0.4.0 schema yet.
