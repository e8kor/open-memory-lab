<!--
id: adr/ADR-0040-plugin-setup-in-claude-code
tier: engineering
status: proposed
answers: how a user points the oml plugin at a server and gives it a key inside Claude Code; the two userConfig options and why the key is sensitive; what .mcp.json and the hooks read, and the precedence between the plugin's options and OML_URL and OML_API_KEY; how a key file reaches the keychain without entering argv or a transcript; the minimum Claude Code version; what it amends in ADR-0028 and ADR-0029; which directory-submission blockers it clears and which remain
-->

# ADR-0040 — The plugin takes its server URL and its key through Claude Code's plugin configuration, keeps the key in the keychain, and falls back to `OML_URL` and `OML_API_KEY` only in its hooks

**Status:** proposed · 2026-11-03

**Amends:** ADR-0028 (d1), ADR-0029 (d4)

## Context

Today the `oml` plugin is configured only through the environment. Its MCP entry reads
`"url": "${OML_URL:-http://127.0.0.1:8080}/mcp"` and `"Authorization": "Bearer ${OML_API_KEY}"`
(`plugins/oml/.mcp.json`), and `oml hook session-start` reads `OML_URL` and `OML_API_KEY` alone
(`crates/bins/oml-cli/src/hook.rs:66`, `107-108`; [ADR-0029](ADR-0029-session-start-hook.md) d4).
The README asks the user to export both before starting Claude Code, so the key sits in a shell
profile or a terminal's history, and a session started any other way has no tools.

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

The maintainer chose on 2026-11-03 to configure the plugin in Claude Code with the environment as
a fallback, and placed it in v0.3.2 beside [ADR-0039](ADR-0039-the-api-rewritten-for-v0-3-2.md).
Decided, not built.

## Decision

- **d1 — `plugins/oml/.claude-plugin/plugin.json` declares two `userConfig` options:
  `server_url` and a sensitive `api_key`.**

    ```json
    "userConfig": {
      "server_url": {
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

    Neither option has a default. ADR-0029's rejected alternative to default `OML_URL` holds here
    too: a default would send the agent's key to whatever listens at that address for a user who
    never named a server. `sensitive: true` keeps the key out of `settings.json`, which users
    commit or copy between machines, and puts it in the platform's secure credential store. The
    two option names are the CLI's and the SDKs' own names for the same values, lower-cased, so
    `CLAUDE_PLUGIN_OPTION_SERVER_URL` and `CLAUDE_PLUGIN_OPTION_API_KEY` sit beside `OML_URL` and
    `OML_API_KEY` without a translation table.

- **d2 — `.mcp.json` reads the two options and nothing from the environment.**

    ```json
    {
      "mcpServers": {
        "oml": {
          "type": "http",
          "url": "${user_config.server_url}/mcp",
          "headers": { "Authorization": "Bearer ${user_config.api_key}" }
        }
      }
    }
    ```

    The entry has no environment fallback. Claude Code documents none inside a `${user_config.*}`
    reference, and a second source in the MCP entry would let the tools and the hook read
    different keys. So a v0.3.0 user who only exported the variables gets no tools from plugin
    0.3.2 until they configure it once (d4). That one-time step is the cost; the benefit is that
    the key never has to sit in a shell profile again. `claude plugin validate` errors on a
    `${user_config.KEY}` reference to an undeclared option and on a remote `url` that is not a
    valid absolute URL ([plugin manifest reference](https://code.claude.com/docs/en/plugins-reference),
    "Validate the manifest", fetched 2026-11-03), so a typo in either name fails CI.

- **d3 — The hooks read the plugin's options first and `OML_URL` and `OML_API_KEY` second, as a
  pair, never one from each.** `oml hook session-start`, and every later `oml hook` verb, resolves
  its server and key this way:

    1. If `CLAUDE_PLUGIN_OPTION_SERVER_URL` or `CLAUDE_PLUGIN_OPTION_API_KEY` is set and non-empty,
       the hook uses those two and ignores `OML_URL` and `OML_API_KEY`. If only one of the two
       options is set, the hook treats the plugin as not configured and fails open (ADR-0029 d5).
    2. Otherwise it uses `OML_URL` and `OML_API_KEY`, exactly as ADR-0029 d4 decides.

    An empty variable counts as unset. Taking the pair from one source keeps a key configured for
    one server from being sent to a URL that came from somewhere else. The options come first
    because they are what the MCP entry reads (d2), and ADR-0029 d4's reason holds unchanged: the
    hook reads what the agent's tools read. The environment stays for callers that are not the
    plugin, such as a user running `oml hook session-start` by hand or another harness. The rest of
    ADR-0029 d4 holds for whichever pair is chosen: the printable-ASCII check names the variable it
    read, never the value, and every occurrence of the value is redacted as `[OML_API_KEY]`.

    `hooks.json` keeps its shell-form command (ADR-0029 d7). A shell-form hook command rejects
    `${user_config.*}`, and the hook does not need it, because the options arrive as environment
    variables ([plugin manifest reference](https://code.claude.com/docs/en/plugins-reference),
    "Fields that run through a shell", fetched 2026-11-03).

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
          '{server_url: $u, api_key: ($k | rtrimstr("\n"))}' \
          | claude plugin configure oml@open-memory-lab --values-stdin
        ```

    `issue-agent --out` prints this line as a third line beside its two export lines, with the
    URL and the file's path filled in and the value nowhere. This amends ADR-0028 d1's output,
    which prints the two lines and nothing else. The `oml-admin` skill keeps its rule that the
    model never runs the printed lines (ADR-0028 d3): the operator runs this one too.
    `claude plugin install --config api_key=…` is never shown, because it puts the key in argv and
    in the shell's history.

- **d5 — The minimum Claude Code version is 2.1.285, for the shell path.** `claude plugin
  configure` "Requires Claude Code v2.1.285 or later" (plugin commands reference, fetched
  2026-11-03). The dialog path names no version on the pages fetched, so the README states 2.1.285
  as the minimum for the documented setup and does not claim an earlier version works. A plugin
  manifest has no field that declares a minimum Claude Code version (plugin manifest reference,
  "Fields", fetched 2026-11-03), so the README, the plugin's CHANGELOG entry and `oml doctor`'s
  documentation say it instead. CI's `claude plugin validate --strict` needs 2.1.281 or later, the
  first version whose validate checks MCP entries and accepts the directory listing fields (same
  page, "Validate the manifest" and "Directory listing fields").

- **d6 — `scripts/check_plugin.py` holds the shape.** It checks that plugin.json declares exactly
  d1's two options with `api_key` sensitive, that `.mcp.json`'s URL is
  `${user_config.server_url}/mcp` and its header `Bearer ${user_config.api_key}`, that no
  `OML_` variable appears in `.mcp.json`, and that the README shows no `export OML_API_KEY` line
  on the Claude Code path. The check today asserts the reverse, a bearer key from the environment
  (`scripts/check_plugin.py:93`), and is rewritten in the same change.

## Consequences

- **ADR-0029 d4 is amended**: the hook's source of truth becomes the plugin's options, with the
  environment second (d3). ADR-0029 d7's hook command does not change. At acceptance ADR-0028 and
  ADR-0029 gain `· amended by ADR-0040`, and the D14 row of the [D-register](index.md) names this
  record.
- **Pages that change at build:** [mcp-plugin.md](../api/mcp-plugin.md) "Wiring it into your
  agent", [the CLI reference](../cli/reference.md) for `oml hook session-start` and `oml keys
  issue-agent`, the plugin README (install, configure, update, the minimum version), and the
  `oml-admin` skill.
- **Migration for v0.3.0 users**, `oml.doublegate.io` among them: `claude plugin update
  oml@open-memory-lab` (auto-update is off by default for a third-party marketplace), then one of
  d4's two paths. The exported variables can then be removed from the shell profile; the hook no
  longer needs them.
- **Directory blockers this clears.**
    - The MCP `url` is no longer `${OML_URL:-http://…}`. Whether the portal reads
      `${user_config.server_url}/mcp` as "a `${user_config.KEY}` reference", or wants the whole URL
      to be one reference, is unverified. If the portal's Validate blocks it, the option becomes
      `mcp_url`, holding the full URL, and the hook strips a trailing `/mcp`: a small amendment
      decided on the portal's answer, not before.
    - The MCP entry and the README no longer send a credential from the user's environment.
- **What remains before a submission.**
    - **The hook's fallback still reads `OML_API_KEY`.** The checklist holds for a reviewer a plugin
      that reads an environment credential and sends it to a server; it blocks this only for an
      HTTP hook, and this is a command hook. So the fallback may raise a hold, which is not a
      rejection. Dropping the fallback would clear it, at the cost of the non-plugin callers d3
      keeps; that waits on the portal's answer.
    - **The hook command's `command -v oml` guard.** For a plugin in a subfolder of its
      repository, as `plugins/oml` is, the checklist blocks a hook command that uses any path not
      written in full from `${CLAUDE_PLUGIN_ROOT}`, any other variable or any command
      substitution. Whether `command -v oml` counts is unverified until the portal's Validate runs;
      exec form is the candidate if it does.
    - **One plugin per submission.** The marketplace holds `oml` and `oml-admin`; `oml` is
      submitted alone, and `oml-admin` stays marketplace-only, since it drives an admin CLI.
    - **Claude.ai chat.** Chat ignores hooks and ignores an MCP server whose URL contains a
      `${user_config.*}` reference, and Cowork ignores such a server when a referenced option has
      no default ([platform support](https://claude.com/docs/plugins/platform-support), fetched
      2026-11-03). A listing therefore gives chat users the skill and no tools; they need the
      connector of the proposed ADR-0031.
    - **README disclosure.** The scan expects the README to describe everything the plugin sends:
      the session hook's two requests, the working directory's name as the query, and the server
      the user configured.
    - **Unverified:** whether the directory requires `privacyPolicyUrl` or `termsOfServiceUrl`
      under the Anthropic Software Directory Policy.
- **Opened:** FR-67 on the [roadmap](../product/roadmap.md), marked proposed: configuring the
  plugin in Claude Code is not built or verified in a session, and the portal's answers above are
  not yet recorded.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Keep environment-only configuration** | The key sits in a shell profile, a session not started from that shell has no tools, and the MCP entry fails the directory's URL rule and raises its environment-credential hold. |
| **A `file`-type option naming the key file** | A `file` option substitutes the path, not the file's text, so it cannot fill the `Authorization` header, and `headersHelper`, which could read the file, receives no option values ([plugin manifest reference](https://code.claude.com/docs/en/plugins-reference), fetched 2026-11-03). The file stays the way the key travels (d4), and the keychain is where it is kept. |
| **Environment fallback in `.mcp.json` too, as `${OML_API_KEY}` beside the option** | No documented form combines a `${user_config.*}` reference with an environment default, and two sources in the MCP entry would let the tools and the hook send different keys. It would also keep the directory's environment-credential finding on the MCP entry. |
| **Prefer `OML_URL` and `OML_API_KEY` over the options in the hook** | A stale export in a shell profile would then override what the user configured in Claude Code, and the hook would read a different key from the tools. |
| **Take each value from whichever source has it** | A configured URL could be paired with an exported key meant for another server, sending that key to the wrong host. d3 takes both from one source. |
| **A default `server_url` of `http://127.0.0.1:8080`** | The hook would send the agent's key to whatever listens there for a user who never named a server, which ADR-0029 already rejected for `OML_URL`. |
| **`claude plugin install --config api_key=<value>` in the README** | The key enters argv and shell history. `--values-stdin` exists for this. |
