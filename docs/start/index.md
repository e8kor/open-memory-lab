---
status: Stable
verified: 2026-11-06
answers: "which page to open first: connect an agent, run a server or administer one"
---

# Get started

Every client needs two settings: the server's address and a key. The server derives everything else from the key.

| You are | You have | Open |
|---|---|---|
| Trying OML | PostgreSQL or Docker | [Quickstart](quickstart.md) |
| Deciding whether to adopt OML | nothing | [Evaluate OML](../product/evaluate.md) |
| An agent user | an agent key ([get one](../guides/get-an-agent-key.md)) | [Install the CLI](install.md), then [Claude Code](../guides/claude-code.md), [claude.ai and your phone](../guides/claude-ai-and-mobile.md) or [another tool](../guides/other-harnesses.md) |
| Running a server | a machine with Docker, or a PostgreSQL host | [Run your own server](run-your-own.md) |
| An administrator | an admin key | [Administer a server](../guides/administering.md) |

Something failed? Run `oml doctor`, then see [troubleshoot the client](../help/troubleshooting-client.md) or the [FAQ](../faq.md).

## Integrations at a glance

| Tool | How it connects | You get | Page |
|---|---|---|---|
| Claude Code | plugin over MCP, installed by `oml setup claude-code` | the three tools, a session-start hook, a librarian agent | [Connect Claude Code](../guides/claude-code.md) |
| claude.ai, Claude Desktop connectors, Claude phone apps | custom connector with a one-time connect code | the three tools | [claude.ai and your phone](../guides/claude-ai-and-mobile.md) |
| Codex CLI, Cursor, Gemini CLI, VS Code, Claude Desktop | files from the agent kit | the three tools over MCP | [Other agent tools](../guides/other-harnesses.md) |
| Your own code | Python and TypeScript SDKs | write, read and review calls | [Client SDKs](../engineering/sdks.md) |
| A reviewer crew | HTTP review loop | a crew that approves or rejects | [Reviewer crews](../api/reviewer-crews.md) |
