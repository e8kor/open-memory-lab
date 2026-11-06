<!--
id: guides/claude-code
tier: engineering
status: draft for review · describes v0.4.0
answers: how to connect Claude Code to an OML server from nothing: install the CLI, oml login, oml setup claude-code, reload, the first write and read and why the write is held, the session-start hook and the permission rule it needs, the librarian and failure recall, the manual path without the CLI, and removal
-->

# Guide: Claude Code

**Status:** draft for review · describes v0.4.0

**Release:** the plugin since v0.1.0; setup through Claude Code's plugin settings and `oml setup
claude-code` since v0.4.0; the maintainer's public server as the default since v0.4.1
([releases](../product/releases.md))

From nothing to an agent that saves and finds shared memory in Claude Code. You need Claude Code
2.1.285 or later and an agent key from your administrator. The plugin connects to the maintainer's
public server, `https://memory.doublegate.io`, unless you name another, so with a key from that
server the key is the only setting; for any other server you also need its address
([ADR-0043](../adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md)). The commands
and their flags are declared in the [CLI reference](../cli/reference.md) and the plugin's
behaviour in its [README](https://github.com/e8kor/open-memory-lab/blob/main/plugins/oml/README.md);
this page links them rather than restating them.

## 1. Install the CLI

```sh
curl -fsSL --proto '=https' --tlsv1.2 https://raw.githubusercontent.com/e8kor/open-memory-lab/v0.6.0/scripts/install/install.sh | sh
```

It writes one file, `~/.local/bin/oml`, and prints what it checked. On Windows and for the
inspect-first form, see [install](../start/install.md).

## 2. Log in, for your own server

With a key file for the public server, skip to step 3. For your own server:

```sh
oml login https://oml.example.com
oml doctor
```

`oml login` reads the key without echo, calls `GET /v2/whoami` and stores `url` and `api_key` in
the owner-only credentials file only when the server accepts the key. `oml doctor` prints where
each value came from, never the key, and the key's role, tenant and default workarea
([start settings](../cli/reference.md#start-settings)).

## 3. Set up the plugin

```sh
oml setup claude-code --key-file "$HOME/.config/oml/keys/crew-agent-7"   # the public server
oml setup claude-code                                                    # after step 2
```

Given a key and no URL, it pairs the key with `https://memory.doublegate.io` and says so on
stderr before it sends anything; when that server refuses the key, it says the key went there and
names `--url`, and installs nothing. It refuses an `admin` or `operator` key (`setup.admin_key`), checks `claude --version`, adds the
marketplace at the CLI's own tag, installs `oml@open-memory-lab` and hands the pair to `claude
plugin configure --values-stdin`, so the key reaches neither a command line nor a transcript
([ADR-0041](../adr/ADR-0041-a-quick-start-in-one-command-per-role.md) d7). With a key file from
`oml keys issue-agent --out`, skip step 2 and run the line the administrator sent you, `oml setup
claude-code --url <url> --key-file <file>`. With `--print` it runs nothing and prints the
commands.

Then run `/reload-plugins` in Claude Code, or restart it. Unverified: the verb has been run
against a fake `claude` in its test suite and its `claude` commands by hand on Claude Code
2.1.312, but not end to end against a real Claude Code (ADR-0041, "Where each item stands").

## 4. The first write and read

Ask the agent to remember a decision. It calls `oml_write` and the answer carries `state:
"held"`: the agent reads it back at once with `oml_read` and `include_unreviewed: true`, and no
other key reads or recalls it until a reviewer who did not write it approves it. The agent
learns that outcome by listing its own work with `oml_read` and `state`. About two minutes from
step 1 to this held write is a target, unmeasured ([FR-68](../product/roadmap.md)).

## 5. Every session starts from memory

With `oml` on `PATH`, the plugin's SessionStart hook runs `oml hook session-start` at each start,
resume, clear, compaction and fork, and adds the workarea's steering, the key's ten newest writes
and the memories matching the project directory's name, within 800 budget tokens
([ADR-0029](../adr/ADR-0029-session-start-hook.md); [CLI
reference](../cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin)). It
reads the plugin's two settings, and sends nothing and prints nothing when the server cannot be
reached. `OML_SESSION_CONTEXT=off` turns it off.

Claude Code asks before every tool call until you allow it. To allow reads and keep the question
on writes, add to `permissions.allow` in your settings:

```json
{
  "permissions": {
    "allow": [
      "mcp__plugin_oml_oml__oml_read",
      "mcp__plugin_oml_oml__oml_workarea"
    ]
  }
}
```

`mcp__plugin_oml_oml` allows the write too, which Claude then makes without showing you the call;
the write is still held for review.

## 6. Optional: the librarian and failure recall

- `@agent-oml:librarian` answers "what does OML hold about X" with up to eight searches on Haiku,
  with `oml_read` as its only tool ([ADR-0038](../adr/ADR-0038-the-agent-recalls-for-itself-and-a-failed-command-recalls-only-when-opted-in.md) d10).
- `OML_FAILURE_RECALL=1` makes a failed Bash command search for up to three distinctive words of
  its error; the cost is that those words go to your server
  ([CLI reference](../cli/reference.md#oml-hook-tool-failure-recall-after-a-failed-command)).

## Without the CLI

Inside Claude Code:

```text
/plugin marketplace add e8kor/open-memory-lab
/plugin install oml@open-memory-lab
```

The install opens a dialog for `url` and `api_key`; the key field is masked and Claude Code keeps
it in the system keychain ([ADR-0040](../adr/ADR-0040-plugin-setup-in-claude-code.md)). Paste the
key; leave `url` empty or at its default, `https://memory.doublegate.io`, for the public server,
or enter your own server's address. Left at the default, a key from another server is refused
there with `401 auth.invalid`: the plugin serves no tools, and that server's operator has seen the
key, so ask your administrator to replace it. Without
the CLI the session-start hook does nothing. A connection with no plugin, and so with neither the
skill nor the hooks, is under [other tools](other-harnesses.md#claude-code-without-the-plugin).

## Removing it

```sh
claude plugin uninstall oml@open-memory-lab
claude plugin marketplace remove open-memory-lab
rm ~/.local/bin/oml ~/.config/oml/credentials
```

The memories stay on the server; removing the plugin deletes only this machine's settings. Ask
your administrator to retire the key: revocation is not built yet, and until it is, `oml deny set
<key> universe` keeps a key from every workarea ([CLI
reference](../cli/reference.md#oml-keys-issuance-revocation-rotation)).
