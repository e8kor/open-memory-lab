<!--
id: adr/ADR-0040-plugin-setup-in-claude-code
tier: engineering
status: proposed
answers: how a user points the oml plugin at a server and gives it a key inside Claude Code; the two userConfig options, why they are the same URL-and-key pair as every other client, and why the key is sensitive; why .mcp.json reads the options alone while the hooks fall back to OML_URL and OML_API_KEY, and the precedence between them; how a key file reaches the keychain without entering argv or a transcript; the minimum Claude Code version; what it amends in ADR-0028 and ADR-0029; which directory-submission blockers it clears and which remain
-->

# ADR-0040 — The plugin takes its server URL and its key through Claude Code's plugin configuration, keeps the key in the keychain, and falls back to `OML_URL` and `OML_API_KEY` only in its hooks

**Status:** proposed · 2026-11-03

**Release:** planned for v0.4.0 ([releases](../product/releases.md#decisions))

**Amends:** ADR-0028 (d1), ADR-0029 (d4)

## Context

Today the `oml` plugin is configured only through the environment: its MCP entry reads
`"url": "${OML_URL:-http://127.0.0.1:8080}/mcp"` and `"Bearer ${OML_API_KEY}"`
(`plugins/oml/.mcp.json`), and `oml hook session-start` reads the same two (`hook.rs:66`,
`107-108`; [ADR-0029](ADR-0029-session-start-hook.md) d4). The key sits in a shell profile, and a
session not started from that shell has no tools.

Claude Code has a place for these values. A plugin's `userConfig` declares options that Claude
Code prompts for when the plugin is enabled; an option with `sensitive: true` "masks input and
stores the value in secure storage instead of `settings.json`"; `${user_config.KEY}` is
substituted in MCP server config, and every hook process receives `CLAUDE_PLUGIN_OPTION_<KEY>`
([plugin manifest reference](https://code.claude.com/docs/en/plugins-reference), "User
configuration", "Reference a saved value", fetched 2026-11-03).

The directory submission depends on the same change. Anthropic's pre-submission checklist blocks
a remote MCP `url` that is not an absolute `https://` or `wss://` URL, a `${user_config.KEY}`
reference, or `""`, and holds for a reviewer a plugin that reads a credential already set in the
user's environment and sends it to a server, README examples included
([pre-submission checklist](https://claude.com/docs/plugins/pre-submission-checklist), "Review
what the plugin runs and connects to", fetched 2026-11-03). Today's entry meets both findings.

On 2026-11-03 the maintainer chose to configure the plugin in Claude Code, placed it in v0.4.0
beside [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md), and decided where the environment
fallback stops: **the MCP entry reads the plugin's settings only, and the hooks and the CLI keep
the `OML_URL` and `OML_API_KEY` fallback**. The same day the maintainer set the rule that every
client starts from a URL and a key and nothing else (ADR-0039 d15). Every decision below is the
maintainer's. Decided, not built.

## Decision

- **d1 — `plugins/oml/.claude-plugin/plugin.json` declares two `userConfig` options, `url` and a
  sensitive `api_key`: the same pair as every other client.**

    ```json
    "userConfig": {
      "url": {
        "type": "string",
        "title": "OML server URL",
        "description": "Your Open Memory Lab server, without /mcp, for example https://oml.example.com",
        "required": true
      },
      "api_key": {
        "type": "string",
        "title": "Agent key",
        "description": "This agent's own OML key, never an admin key. Stored in the system keychain.",
        "sensitive": true,
        "required": true
      }
    }
    ```

    The names are `OML_URL` and `OML_API_KEY` lower-cased, the pair ADR-0039 d15 gives every
    client; everything else the server derives from the key. Neither has a default, which would
    send the key to whatever listens there for a user who never named a server (ADR-0029).
    `sensitive: true` keeps the key out of `settings.json`, which users commit or copy.

- **d2 — `.mcp.json` reads the two options and nothing from the environment. Confirmed by the
  maintainer on 2026-11-03.**

    ```json
    {
      "mcpServers": {
        "oml": {
          "type": "http",
          "url": "${user_config.url}/mcp",
          "headers": { "Authorization": "Bearer ${user_config.api_key}" }
        }
      }
    }
    ```

    Claude Code documents no environment fallback inside a `${user_config.*}` reference, and a
    second source would let the tools and the hook read different keys. A v0.3.0 user who only
    exported the variables gets no tools until they configure the plugin once (d4).
    `claude plugin validate` errors on a reference to an undeclared option and on a remote `url`
    that is not a valid absolute URL ([plugin manifest reference](https://code.claude.com/docs/en/plugins-reference),
    "Validate the manifest", fetched 2026-11-03), so a typo in either name fails CI.

- **d3 — The hooks read the plugin's options first and `OML_URL` and `OML_API_KEY` second, as a
  pair, never one from each.** `oml hook session-start`, and every later `oml hook` verb, resolves
  its server and key this way:

    1. If `CLAUDE_PLUGIN_OPTION_URL` or `CLAUDE_PLUGIN_OPTION_API_KEY` is set and non-empty, the
       hook uses those two and ignores the environment pair. If only one is set, the hook treats
       the plugin as not configured and fails open (ADR-0029 d5).
    2. Otherwise it uses `OML_URL` and `OML_API_KEY`, as ADR-0029 d4 decides.

    An empty variable counts as unset. One source per pair keeps a key from going to a URL from
    elsewhere, as in the CLI (ADR-0039 d15). The options come first because the tools read them
    (d2), so the hook reads what the tools read (ADR-0029 d4); the environment stays for callers
    that are not the plugin, such as a user running the hook by hand or another harness. The
    printable-ASCII check and the `[OML_API_KEY]` redaction of ADR-0029 d4 hold for either pair.

    The hook never reads the CLI's credentials file, which `oml login` now fills and which often
    holds an operator's admin key (ADR-0029 d4). An admin key exported as `OML_API_KEY` for the
    CLI can still reach the fallback; it recalls no claim content
    ([roles and keys](../architecture/roles-and-keys.md)), so the hook's recall is refused and it
    fails open. `hooks.json` keeps its shell-form command (ADR-0029 d7), which rejects
    `${user_config.*}` and does not need it, because the options arrive as environment variables
    ([plugin manifest reference](https://code.claude.com/docs/en/plugins-reference), "Fields
    that run through a shell", fetched 2026-11-03).

- **d4 — The key reaches the keychain through Claude Code, never through argv or a
  transcript.** There are two paths:

    - **In a session.** `/plugin install` opens the configuration dialog, and `/plugin configure
      oml@open-memory-lab` opens it again later ([plugin commands
      reference](https://code.claude.com/docs/en/plugins/cli-reference), "/plugin in a session",
      fetched 2026-11-03). The user pastes the key into a masked field.
    - **From a shell.** `claude plugin install` does not open the dialog. The key file that `oml
      keys issue-agent --out` writes ([ADR-0028](ADR-0028-agent-key-issuance.md) d1) is piped
      into `claude plugin configure --values-stdin`, which "reads option values from stdin as a
      JSON object of single-line strings and save[s] them" (same page, "plugin configure", fetched
      2026-11-03):

        ```sh
        jq -n --arg u 'https://oml.example.com' --rawfile k "$HOME/.config/oml/keys/crew-agent-7" \
          '{url: $u, api_key: ($k | rtrimstr("\n"))}' \
          | claude plugin configure oml@open-memory-lab --values-stdin
        ```

    `issue-agent --out` prints this line beside its two export lines, the value nowhere, amending
    ADR-0028 d1's output; the model never runs the printed lines (ADR-0028 d3).

- **d5 — The minimum Claude Code version is 2.1.285, for the shell path.** `claude plugin
  configure` "Requires Claude Code v2.1.285 or later" (plugin commands reference, fetched
  2026-11-03); the dialog path names no version. A manifest has no minimum-version field (plugin
  manifest reference, "Fields"), so the README, the plugin's CHANGELOG and `oml doctor`'s
  documentation state it. CI's `claude plugin validate --strict` needs 2.1.281, the first version
  that checks MCP entries and accepts the directory listing fields (same page).

- **d6 — `scripts/check_plugin.py` holds the shape.** It checks that plugin.json declares exactly
  d1's two options with `api_key` sensitive, that `.mcp.json`'s URL is `${user_config.url}/mcp`
  and its header `Bearer ${user_config.api_key}`, that no `OML_` variable appears in `.mcp.json`,
  and that the README shows no `export OML_API_KEY` line on the Claude Code path. The check today
  asserts the reverse, a bearer key from the environment (`scripts/check_plugin.py:93`), and is
  rewritten in the same change.

## Consequences

- **ADR-0029 d4 is amended**: the hook's source of truth becomes the plugin's options, with the
  environment second (d3); ADR-0029 d7's command does not change. At acceptance ADR-0028 and
  ADR-0029 gain `· amended by ADR-0040`, and the D14 row of the [D-register](index.md) names this
  record.
- **Pages that change at build:** [mcp-plugin.md](../api/mcp-plugin.md), [the CLI
  reference](../cli/reference.md) for the hook and `issue-agent`, the plugin README and the
  `oml-admin` skill.
- **Migration for v0.3.0 users**, `oml.doublegate.io` among them: `claude plugin update
  oml@open-memory-lab` (auto-update is off by default for a third-party marketplace), then one of
  d4's two paths. The exported variables can then leave the shell profile.
- **Directory blockers this clears.** The MCP `url` is no longer `${OML_URL:-http://…}`, and the
  MCP entry and the README no longer send a credential from the user's environment. Whether the
  portal reads `${user_config.url}/mcp` as "a `${user_config.KEY}` reference" is unverified; if
  its Validate blocks it, the option holds the full URL and the hook strips a trailing `/mcp`, a
  small amendment decided on the portal's answer.
- **What remains before a submission.**
    - **The hooks keep the `OML_API_KEY` fallback.** The checklist holds a plugin that reads an
      environment credential and sends it to a server; it blocks this only for an HTTP hook, and
      this is a command hook. The maintainer kept the fallback for the callers d3 names, so a
      hold, which is not a rejection, is possible.
    - **The hook command's `command -v oml` guard.** For a plugin in a subfolder of its
      repository, as `plugins/oml` is, the checklist blocks a hook command using a path not
      written in full from `${CLAUDE_PLUGIN_ROOT}`, another variable or a command substitution.
      Whether `command -v oml` counts is unverified; exec form is the candidate if it does.
    - **One plugin per submission.** `oml` is submitted alone; `oml-admin`, which drives an admin
      CLI, stays marketplace-only.
    - **Claude.ai chat** ignores hooks and an MCP server whose URL holds a `${user_config.*}`
      reference, and Cowork ignores such a server when a referenced option has no default
      ([platform support](https://claude.com/docs/plugins/platform-support), fetched 2026-11-03),
      so a listing gives chat users the skill and no tools; they need the proposed ADR-0031's
      connector.
    - **README disclosure** of the hook's two requests, the directory name it sends as the query,
      and the configured server. **Unverified:** whether `privacyPolicyUrl` or
      `termsOfServiceUrl` is required under the Anthropic Software Directory Policy.
- **Opened:** FR-67 on the [roadmap](../product/roadmap.md), marked proposed.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Keep environment-only configuration** | The key sits in a shell profile, a session not started from that shell has no tools, and the MCP entry fails the directory's URL rule and raises its environment-credential hold. |
| **A `file`-type option naming the key file** | It substitutes the path, not the text, so it cannot fill the `Authorization` header, and `headersHelper` receives no option values ([plugin manifest reference](https://code.claude.com/docs/en/plugins-reference), fetched 2026-11-03). The file is how the key travels (d4); the keychain is where it is kept. |
| **Environment fallback in `.mcp.json` too** (the maintainer's first framing, replaced on 2026-11-03) | No documented form combines a `${user_config.*}` reference with an environment default; two sources would let the tools and the hook send different keys, and keep the directory's environment-credential finding on the MCP entry. |
| **Drop the hooks' environment fallback as well** | It would clear the possible directory hold, at the cost of callers that are not the plugin; the maintainer kept it for the hooks and the CLI. |
| **Prefer `OML_URL` and `OML_API_KEY` over the options in the hook** | A stale export would override what the user configured in Claude Code, and the hook would read a different key from the tools. |
| **Take each value from whichever source has it** | A configured URL could be paired with an exported key meant for another server. |
| **An option named `server_url`**, as the first draft had | A second spelling of the setting every other client calls `OML_URL` (ADR-0039 d15). |
| **A default `url` of `http://127.0.0.1:8080`** | The hook would send the agent's key to whatever listens there, which ADR-0029 already rejected for `OML_URL`. |
| **`claude plugin install --config api_key=<value>` in the README** | The key enters argv and shell history; `--values-stdin` exists for this. |
