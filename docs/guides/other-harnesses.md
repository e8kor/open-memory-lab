<!--
id: guides/other-harnesses
tier: engineering
status: draft for review · the kit since v0.3.1; the one pair since v0.4.0; the default server since v0.4.1
answers: how OpenAI Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop reach an OML server from the agent kit with OML_API_KEY, the maintainer's public server by default; the one URL edit for another server; Claude Code without the plugin; what each tool gets and what it does not; and why there is no Cursor install link yet
-->

# Guide: Codex, Cursor, Gemini CLI, VS Code and Claude Desktop

**Status:** draft for review · the kit since v0.3.1; the one pair since v0.4.0; the default server since v0.4.1

**Release:** the agent kit since v0.3.1; `OML_URL` and `OML_API_KEY` as its only settings since
v0.4.0; every configuration naming the maintainer's public server, so the key is the only
setting, planned for v0.4.1 ([releases](../product/releases.md))

Every tool here reaches the same `/mcp` endpoint with the same three tools. The configurations
come from the agent kit, `oml-agent-kit-X.Y.Z.tar.gz` (or `.zip`), attached to each release and
signed with cosign. The kit's [README](https://github.com/e8kor/open-memory-lab/blob/main/kit/README.md)
is the declaration site: it names each file, where it goes and the tool's documentation with its
fetch date, and marks what that documentation does not settle as unverified. This page is the
order of the steps.

## 1. The key

Export the agent's key in the shell that starts the tool:

```sh
export OML_API_KEY="$(cat "$HOME/.config/oml/keys/crew-agent-7")"
```

The key file is the one `oml keys issue-agent --out` wrote; without one, export the key you were
given. None of the kit's files holds a key.

## 2. The tool's file

| Tool | Copy |
|---|---|
| Cursor | `cursor/.cursor/mcp.json` to `.cursor/mcp.json` |
| Gemini CLI | merge `gemini/.gemini/settings.json` into `.gemini/settings.json`; also wires the session-start hook |
| OpenAI Codex CLI | add `codex/config.toml`'s `[mcp_servers.oml]` to `~/.codex/config.toml` |
| VS Code | `vscode/.vscode/mcp.json` to `.vscode/mcp.json`; the header's `${env:OML_API_KEY}` is unverified |
| Claude Desktop | `claude-desktop/claude_desktop_config.json` to the app's config directory; runs the `mcp-remote` bridge, unverified on Windows |

Every file names `https://memory.doublegate.io/mcp`, the maintainer's public server
([ADR-0043](../adr/ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md)).
With a key from that server, there is nothing to edit. For your own server, replace
`https://memory.doublegate.io` with its address and keep `/mcp`, the same edit in every file. A
key sent to the wrong server is refused there with `401 auth.invalid`, and that server's operator
has seen it.

Gemini CLI's session-start hook reads `OML_URL` beside the key and takes no default, so for it
also export the address of the server the file names:

```sh
export OML_URL=https://memory.doublegate.io      # without /mcp; your own server's address otherwise
```

Paste the kit's `AGENTS.md` block into the project's `AGENTS.md`: it carries the plugin skill's
guidance on when to search and when to save.

## 3. Check it

Ask the agent what OML holds about the project. It calls `oml_read`; a refusal names its reason,
such as `auth.invalid`. A save answers `held`: the agent reads it back at once, and others
find it after a reviewer approves it.

## What these tools get

The three tools and the `AGENTS.md` guidance. Gemini CLI also gets the session-start context,
through `oml hook session-start`, when `oml` is on `PATH`. Cursor documents a session-start hook
with a different output field, so the kit wires none. No tool here gets the librarian or failure
recall, which are Claude Code plugin parts.

## Claude Code without the plugin

```sh
claude mcp add --transport http oml https://memory.doublegate.io/mcp --header "Authorization: Bearer $OML_API_KEY"
```

For your own server, put its address in place of `https://memory.doublegate.io`.

This gives the three tools and nothing else: no skill, no hooks, no librarian. The shell expands
the key into the command line, and Claude Code stores the header in its own configuration file,
not the keychain. The plugin, through `oml setup claude-code`, keeps the key in the keychain
([Claude Code guide](claude-code.md)).

## Cursor install link

Cursor can install a server from a link. The kit offers none: a link carries its configuration in
the URL, and until a connection can open without a key, through
[ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md)'s connect code from Cursor too,
that configuration would carry the key. Whether Cursor completes ADR-0031's flow is untested.
