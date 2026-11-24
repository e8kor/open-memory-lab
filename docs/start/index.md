---
status: Stable
verified: 2026-11-06
answers: "which page to open first: connect an agent, run a server or administer one"
---

# Get started

Every client needs two settings: the server's address and a key. The server derives everything else from the key.

| Role | Needs | Open |
|---|---|---|
| Trying OML | PostgreSQL or Docker | [Quickstart](quickstart.md) |
| Deciding whether to adopt OML | nothing | [Evaluate OML](../product/evaluate.md) |
| An agent user | an agent key ([get one](../guides/get-an-agent-key.md)) | [Install the CLI](install.md), then [Claude Code](../guides/claude-code.md), [claude.ai and the phone](../guides/claude-ai-and-mobile.md) or [another tool](../guides/other-harnesses.md) |
| Running a server | a machine with Docker, or a PostgreSQL host | [Run a server](run-your-own.md) |
| An administrator | an admin key | [Administer a server](../guides/administering.md) |

Something failed? Run `oml doctor`, then see [troubleshoot the client](../help/troubleshooting-client.md) or the [FAQ](../faq.md).

## Sign up on a server: no key yet

Where a server's operator opens sign-up ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md), proposed), only a Google or GitHub account and the server's address are needed. Sign in at `<server>/web`, accept the terms, and the page lands on `/web/start`:

1. Choose the client: claude.ai, Claude Desktop or the phone app; Claude Code; or another MCP tool.
2. One button issues the right credential, shown once: a connect code and the connector URL for claude.ai, a key and the plugin's install commands for Claude Code, or the URL and a key for the rest.
3. The page waits for the agent's first connection, then for its first memory, which is reviewed on the same site.

The account gets a private space nobody else can read, up to 1,000 memories, 5 keys and 5 connections; the operator can raise a limit. The agents' writes wait for the review. The operator of the server can read what is stored. Export everything as JSON, or ask for the account to be deleted, from `/web/start`.

## Integrations at a glance

| Tool | How it connects | Gives | Page |
|---|---|---|---|
| Claude Code | plugin over MCP, installed by `oml setup claude-code` | the three tools, a session-start hook, a librarian agent | [Connect Claude Code](../guides/claude-code.md) |
| claude.ai, Claude Desktop connectors, Claude phone apps | custom connector with a one-time connect code | the three tools | [claude.ai and the phone](../guides/claude-ai-and-mobile.md) |
| Codex CLI, Cursor, Gemini CLI, VS Code, Claude Desktop | files from the agent kit | the three tools over MCP | [Other agent tools](../guides/other-harnesses.md) |
| The user's own code | Python and TypeScript SDKs | write, read and review calls | [Client SDKs](../engineering/sdks.md) |
| A reviewer crew | HTTP review loop | a crew that approves or rejects | [Reviewer crews](../api/reviewer-crews.md) |
