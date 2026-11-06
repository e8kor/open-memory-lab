---
status: Stable
verified: 2026-11-06
answers: "how to connect Claude Code to an OML server from nothing: install the CLI, oml login, oml setup claude-code, reload, the first write and read and why the write is held, the session-start hook and the permission rule it needs, the librarian and failure recall, the manual path without the CLI, and removal"
---

# Connect Claude Code to OML

This page takes you from nothing to an agent that saves and finds shared memory in Claude Code.

## Before you start

- Claude Code 2.1.285 or later.
- An agent key from your administrator.
- The server's address, unless you use [the public server](../start/install.md). With a key from the public server, the key is the only setting.

The commands are in the [CLI reference](../cli/reference.md). The plugin's behavior is in its
[README](https://github.com/e8kor/open-memory-lab/blob/main/plugins/oml/README.md).

## 1. Install the CLI

```sh
curl -fsSL --proto '=https' --tlsv1.2 https://raw.githubusercontent.com/e8kor/open-memory-lab/v0.6.0/scripts/install/install.sh | sh
```

It writes one file, `~/.local/bin/oml`, and prints what it checked. For Windows and the
inspect-first form, see [install](../start/install.md).

## 2. Log in, for your own server

With a key file for the public server, skip to step 3. For your own server:

```sh
oml login https://oml.example.com
oml doctor
```

`oml login` reads the key without echo and calls `GET /v2/whoami`. It stores `url` and `api_key`
in the owner-only credentials file only when the server accepts the key. `oml doctor` prints
where each value came from, never the key, and the key's role, tenant and default workarea
([start settings](../cli/reference.md)).

## 3. Set up the plugin

```sh
oml setup claude-code --key-file "$HOME/.config/oml/keys/crew-agent-7"   # the public server
oml setup claude-code                                                    # after step 2
```

Given a key and no URL, the command pairs the key with the public server and says so on stderr
before it sends anything. If that server refuses the key, the command names `--url` and installs
nothing. It refuses an `admin` or `operator` key (`setup.admin_key`) and checks `claude --version`.
It adds the marketplace at the CLI's own tag, installs `oml@open-memory-lab` and passes the pair to
`claude plugin configure --values-stdin`. The key reaches neither a command line nor a transcript.

If your administrator sent a key file from `oml keys issue-agent --out`, skip step 2 and run the
line they sent: `oml setup claude-code --url <url> --key-file <file>`. With `--print`, the
command runs nothing and prints the commands.

Then run `/reload-plugins` in Claude Code, or restart it. This setup has not been run against a
real Claude Code yet.

## 4. Check the first write and read

Ask the agent to remember a decision. It calls `oml_write`, and the answer carries
`state: "held"`. The agent reads it back at once with `oml_read` and `include_unreviewed: true`.
No other key reads it until a reviewer who did not write it approves it
([why a write is held](../concepts/why-a-write-is-held.md)). The agent lists its own work with
`oml_read` and `state`.

## 5. Start every session from memory

With `oml` on `PATH`, the plugin's SessionStart hook runs `oml hook session-start` at each start,
resume, clear, compaction and fork. It adds the workarea's steering, the key's ten newest writes
and the memories matching the project directory's name, within 800 budget tokens
([CLI reference](../cli/reference.md)).
It sends and prints nothing when the server cannot be reached. `OML_SESSION_CONTEXT=off` turns it
off.

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

`mcp__plugin_oml_oml` also allows the write, which Claude then makes without showing you the
call. The write is still held for review.

## 6. Optional: the librarian and failure recall

- `@agent-oml:librarian` answers "what does OML hold about X" with up to eight searches on Haiku.
  `oml_read` is its only tool.
- `OML_FAILURE_RECALL=1` makes a failed Bash command search for up to three distinctive words of
  its error. The cost: those words go to your server
  ([CLI reference](../cli/reference.md)).

## Install without the CLI

Inside Claude Code:

```text
/plugin marketplace add e8kor/open-memory-lab
/plugin install oml@open-memory-lab
```

The install opens a dialog for `url` and `api_key`. The key field is masked, and Claude Code
keeps the key in the system keychain. Paste the key. Leave `url` empty for the public server, or
enter your own server's address.

If you leave `url` at the default with a key from another server, the public server refuses it
with `401 auth.invalid` and the plugin serves no tools. That server's operator has seen the key,
so ask your administrator to replace it. Without the CLI, the session-start hook does nothing.
For a connection with no plugin, see [other tools](other-harnesses.md#claude-code-without-the-plugin).

## Remove the plugin

```sh
claude plugin uninstall oml@open-memory-lab
claude plugin marketplace remove open-memory-lab
rm ~/.local/bin/oml ~/.config/oml/credentials
```

The memories stay on the server. Removing the plugin deletes only this machine's settings. OML has
no revoke verb, so ask your administrator to run `oml deny set <key> universe`, which keeps the key
out of every workarea ([CLI reference](../cli/reference.md)).

If something fails, see [client troubleshooting](../help/troubleshooting-client.md).
