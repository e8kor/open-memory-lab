<!--
id: adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server
tier: engineering
status: accepted design · 2026-11-04
answers: which server the plugins and harness configurations OML releases connect to when the user names none; why the plugin's url option has a default and is no longer required; how the hooks read a plugin configured with its key alone; why oml setup claude-code takes the default and no other CLI verb does; why every kit configuration names the default URL literally; what a user sees when a key meant for another server is sent to the default one; what it amends in ADR-0039, ADR-0040 and ADR-0041
-->

# ADR-0043 — What OML releases defaults to the maintainer's public server, so an agent needs only its key; only `oml setup claude-code` among the CLI's verbs takes that default

**Status:** accepted design · 2026-11-04

**Release:** introduced in v0.4.1 ([releases](../product/releases.md#decisions))

**Amends:** ADR-0039 (d15), ADR-0040 (d1, d3), ADR-0041 (d7)

## Context

Since v0.4.0 every client starts from a URL and a key ([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md)
d15), and the plugin's `url` option is required and has no default
([ADR-0040](ADR-0040-plugin-setup-in-claude-code.md) d1). A user of the maintainer's own server
still has to find and type its address before the first memory.

On 2026-11-04 the maintainer's deployment moved from `oml.doublegate.io` to
`https://memory.doublegate.io`, with no redirect from the old name. The same day the maintainer
decided that **the plugins and harness integrations OML releases default to that server**: a user
of the Claude Code plugin, or of any harness configuration a release ships, needs only an API
key, and the URL stays overridable.

ADR-0040 d1 rejected a default because it "would send the key to whatever listens there for a
user who never named a server", and ADR-0040's rejected alternatives turned down a loopback
default for the same reason. A public HTTPS server that the maintainer runs and names is a
different case from whatever listens on a port, but the cost is the same in kind: a key can go to
a server its holder did not mean. This record decides where the default applies and where it
must not, and says what the user sees when it is the wrong one.

Claude Code's plugin manifest documents a `default` on a `userConfig` option, "Value used when
the user provides nothing", and `required` as "the configuration dialog doesn't accept an empty
value" ([plugin manifest reference](https://code.claude.com/docs/en/plugins-reference), "User
configuration", fetched 2026-11-04). The same page says `CLAUDE_PLUGIN_OPTION_<KEY>` is
"exported to hook processes for every option". Read from the bundled code of Claude Code
2.1.312 on 2026-11-04: the MCP configuration is filled from the saved values merged over each
option's default, while a hook's `CLAUDE_PLUGIN_OPTION_*` variables come from the saved values
alone, so an option the user never saved reaches the tools as its default and the hook as unset.
Every decision below is the maintainer's or follows from that one. Decided, not built.

## Decision

- **d1 — The default server is `https://memory.doublegate.io`, the maintainer's public server,
  and every integration a release ships names it.** It is one constant per language: the
  manifest's `default`, `DEFAULT_URL` in `crates/bins/oml-cli/src/setup.rs`, and
  `DEFAULT_URL` in `scripts/package_agent_artifacts.py`, which `scripts/check_plugin.py` and
  `scripts/check_agent_kit.py` hold equal to the Rust constant. The URL stays overridable on
  every surface. The cost, carried openly: a release names one operator's server, so a
  self-hoster must override it on every surface; and the default changes only with a release,
  so an install keeps sending to the name its release shipped, as one shipped with
  `oml.doublegate.io` would now send to a name that no longer answers.

- **d2 — The plugin's `url` option takes `https://memory.doublegate.io` as its default and is no
  longer required (amends ADR-0040 d1).**

    ```json
    "url": {
      "type": "string",
      "title": "OML server URL",
      "description": "Your Open Memory Lab server, without /mcp. Leave the default to use the maintainer's public server.",
      "default": "https://memory.doublegate.io"
    }
    ```

    `api_key` is unchanged: sensitive, required, no default. A user of the public server pastes
    the key in the configuration dialog and leaves the URL as shown. `.mcp.json` is unchanged
    (ADR-0040 d2): `${user_config.url}` is filled with the default when no value was saved.

- **d3 — A plugin configured with its key alone is the default server and that key, in every
  hook (amends ADR-0040 d3 step 1).** When `CLAUDE_PLUGIN_OPTION_API_KEY` is set and
  `CLAUDE_PLUGIN_OPTION_URL` is unset or empty, `oml hook` takes `https://memory.doublegate.io`
  as the URL, because that is where the plugin's tools send the same key (d2 and the Context);
  the hook still reads what the tools read. The rest of ADR-0040 d3 stands:
    - `CLAUDE_PLUGIN_OPTION_URL` alone is a plugin not configured, and the hook fails open
      naming `CLAUDE_PLUGIN_OPTION_API_KEY`;
    - the environment pair takes no default: `OML_API_KEY` without `OML_URL` fails open naming
      `OML_URL`. That fallback serves callers that are not the plugin, whose tools carry no
      default, so a default there could send the hook's key somewhere the tools do not.

- **d4 — `oml setup claude-code` takes the default when it is given a key and no URL, says so
  before it sends anything, and names `--url` when the default server refuses the key (amends
  ADR-0041 d7 step 1 and, for this verb alone, ADR-0039 d15).** A key named without a URL —
  `--key-file` or `--api-key` without `--url`, `OML_API_KEY` without `OML_URL`, or a
  credentials file holding `api_key` without `url` — is paired with
  `https://memory.doublegate.io`. Before `whoami` the verb prints to stderr that no URL was given
  beside the key, that it goes to the maintainer's public server, and that `--url` names
  another. A URL named without a key is still refused. When the defaulted `whoami` fails, the
  verb prints that the key went to the default server and that a key from another server is
  unknown there, then the refusal, and installs nothing. The configure line it runs or prints
  carries the URL it used, so the plugin's saved `url` and the checked one are the same.

- **d5 — Every other CLI verb keeps requiring a URL.** ADR-0039 d15 is unchanged for them: the
  flags, the environment and the credentials file each give both values or are refused, and
  `oml login <url>` keeps its positional. An administrator's command never reaches the public
  server because nothing named one, and an administrator's key never goes there by default.
  `oml setup` is the exception because its key is an agent's (it refuses an admin or operator
  key, ADR-0041 d7 step 2) and its job is the plugin's, which carries the same default.

- **d6 — Every agent kit configuration names `https://memory.doublegate.io/mcp` and reads the key
  from the environment as before (amends ADR-0039 d15's kit clause).** OpenAI Codex CLI, Cursor,
  Gemini CLI, VS Code and Claude Desktop each name the default URL literally; the override is
  one edit in each, the same edit for every tool. Cursor and Gemini CLI no longer read
  `${OML_URL}`, which an unset variable leaves as `/mcp`, a URL that names no server. The
  Gemini CLI session-start hook runs the plugin's own command and so reads the environment pair
  (d3), which takes no default: it loads context only when `OML_URL` is exported beside the key,
  and a self-hoster who exports `OML_URL` must also edit the configuration's URL, or the tools
  and the hook reach different servers. `kit/README.md` says both.

- **d7 — A key meant for another server, sent to the default one, is refused there, and the
  user sees where it went.** The default server does not know the key and answers `401
  auth.invalid`:
    - **The plugin's tools.** `/mcp` answers `401` with `WWW-Authenticate` and, where that
      server has `oauth.issuer` set, the OAuth challenge with `error="invalid_token"`
      ([ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) d2). Claude Code serves no
      `oml` tools, and its `/mcp` panel lists the server as failed or as needing
      authentication. **Unverified:** which of the two Claude Code 2.1.312 shows for that
      answer; no run has recorded it.
    - **The hooks** print one stderr line with the refusal and load no context (ADR-0029 d5).
    - **`oml setup claude-code`** prints d4's lines and installs nothing.

    The cost: the key value has been presented to the maintainer's server, whose operator could
    read it and use it against the server that issued it. A user who sent a key to the wrong
    server treats it as exposed; revocation is not built, and until it is, `oml deny set <key>
    universe` on the issuing server keeps it from every workarea
    ([CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation)).

- **d8 — The checks hold the default.** `scripts/check_plugin.py` checks that `url` declares the
  default and no `required`, that `api_key` declares neither a default nor anything but d2's
  fields, and that the default equals `DEFAULT_URL` in `setup.rs`; `scripts/check_agent_kit.py`
  checks that every configuration's URL is `DEFAULT_URL` with `/mcp` and that the kit README
  names the one edit for every tool. The CLI's tests prove d3's pair, d4's choice and d5's
  refusal.

## Consequences

- **ADR-0040 d1 and d3, ADR-0041 d7 step 1 and ADR-0039 d15 are amended** for the default. Each
  of the three Status lines gains `· amended by ADR-0043 (proposed)`, which drops `(proposed)` at
  acceptance, and the D14 row of the [D-register](index.md) names this record.
- **Pages that change with the build:** the plugin README and the kit README, the Claude Code,
  other-tools, claude.ai and mobile and troubleshooting guides, the get-started page, the
  repository README's quick start, and the CLI reference for `oml setup claude-code` and the
  hook's pair.
- **A move of the maintainer's server is a release.** The default names a host, not a role; the
  next move needs a release before it, or a redirect from the old name for as long as releases
  naming it are in use.
- **Claude Code's Cowork** ignores a plugin MCP server when a referenced option has no default
  ([platform support](https://claude.com/docs/plugins/platform-support), fetched 2026-11-03);
  `url` now has one and `api_key` still has none, so this record does not change what Cowork
  loads.
- **No roadmap row is opened.** The cost of d1 and d7 is a property of the default, not an
  unmeasured quantity.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **No default** (ADR-0040 d1 as accepted) | Every user of the maintainer's server has to find and type its address before the first memory, against the maintainer's rule that an agent needs only its key. |
| **A default only in the agent kit** | The Claude Code plugin is the integration most users install; it would stay the one that needs a URL. |
| **A default for every CLI verb** | An administrator's command, `oml keys issue-agent` among them, would reach the public server whenever its URL was forgotten, sending an admin key there. The CLI's pair rule (ADR-0039 d15) exists so a key is sent only to a URL named beside it. |
| **A default in the hooks' environment fallback** | That fallback serves callers that are not the plugin, whose tools carry no default; the hook could send the key to the public server while the tools send it elsewhere. |
| **Keep `${OML_URL}` in the Cursor and Gemini CLI configurations** | With only the key set, the URL becomes `/mcp`, which names no server, so those two tools would still need a URL. |
| **A default only in the dialog's description text, with `url` still required** | The user still types the address; the description is help text, not a value. |
| **`oml.doublegate.io`, the address in the v0.4.0 documents** | It no longer answers since the move of 2026-11-04, and there is no redirect. |
