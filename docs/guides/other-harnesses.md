---
status: Stable
verified: 2026-11-06
answers: "how OpenAI Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop reach an OML server from the agent kit with OML_API_KEY, the public server by default; the one URL edit for another server; Claude Code without the plugin; what each tool gets and what it does not; and why there is no Cursor install link"
---

# Connect Codex, Cursor, Gemini CLI, VS Code and Claude Desktop

Every tool here reaches the same `/mcp` endpoint with the same three tools. The configuration
for each comes from the agent kit, `oml-agent-kit-X.Y.Z.tar.gz` (or `.zip`), attached to each
release and signed with cosign. The kit's
[README](https://github.com/e8kor/open-memory-lab/blob/main/kit/README.md) names each file, where
it goes and the tool's documentation with its fetch date. It marks what that documentation does
not settle as unverified.

You need an agent key. OML provides configuration for these tools. None has been run against a real
client yet.


## 1. Export the key

Export the agent's key in the shell that starts the tool:

```sh
export OML_API_KEY="$(cat "$HOME/.config/oml/keys/crew-agent-7")"
```

The key file is the one `oml keys issue-agent --out` wrote. Without one, export the key you were
given. None of the kit's files holds a key.

## 2. Copy the tool's file

| Tool | Copy |
|---|---|
| Cursor | `cursor/.cursor/mcp.json` to `.cursor/mcp.json` |
| Gemini CLI | merge `gemini/.gemini/settings.json` into `.gemini/settings.json`; also wires the session-start hook |
| OpenAI Codex CLI | add `codex/config.toml`'s `[mcp_servers.oml]` to `~/.codex/config.toml` |
| VS Code | `vscode/.vscode/mcp.json` to `.vscode/mcp.json`; the header's `${env:OML_API_KEY}` is unverified |
| Claude Desktop | `claude-desktop/claude_desktop_config.json` to the app's config directory; runs the `mcp-remote` bridge, unverified on Windows |

Every file names the public server's `/mcp` address ([install](../start/install.md)). With a key
from that server, there is nothing to edit. For your own server, replace the host with its
address and keep `/mcp`. The edit is the same in every file. A key sent to the wrong server is
refused there with `401 auth.invalid`, and that server's operator has seen it.

Gemini CLI's session-start hook reads `OML_URL` beside the key and takes no default, so for it
also export the address of the server the file names:

```sh
export OML_URL=<server address>      # without /mcp; the address of the server the file names
```

Paste the kit's `AGENTS.md` block into the project's `AGENTS.md`. It says when to search and when
to save.

## 3. Check the connection

Ask the agent what OML holds about the project. It calls `oml_read`; a refusal names its reason,
such as `auth.invalid`. A save answers `held`. The agent reads it back at once, and others
find it after a reviewer approves it. If it fails, see [client troubleshooting](../help/troubleshooting-client.md).

## What these tools get

The three tools and the `AGENTS.md` guidance. Gemini CLI also gets the session-start context
through `oml hook session-start`, when `oml` is on `PATH`. Cursor's session-start hook uses a
different output field, so the kit wires none. No tool here gets the librarian or failure recall.

## Claude Code without the plugin

```sh
claude mcp add --transport http oml <server address>/mcp --header "Authorization: Bearer $OML_API_KEY"
```

For the public server, use its address from [install](../start/install.md).

This gives the three tools and nothing else: no skill, no hooks, no librarian. The shell expands
the key into the command line, and Claude Code stores the header in its own configuration file,
not the keychain. The plugin, through `oml setup claude-code`, keeps the key in the keychain
([Claude Code guide](claude-code.md)).

## Why there is no Cursor install link

Cursor can install a server from a link, and a link carries its configuration in the URL. The kit
offers none, because that configuration would carry the key. A link becomes possible when a
connection can open with the connect code instead of a key. Whether Cursor completes that flow is
untested.
