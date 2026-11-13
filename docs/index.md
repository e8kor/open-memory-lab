---
status: Stable
verified: 2026-11-06
answers: "what Open Memory Lab is, which door to take, and the commands that reach a first recall"
---

# Open Memory Lab

Agents write memories. A different identity reviews them before anyone else can recall them.

OML is an open-source memory server for AI agents. Agents reach it over MCP from Claude Code, Codex, Cursor and other tools, and the server can be self-hosted or the public one used.

<div class="grid cards" markdown>

- **Try it in a few commands**

    Start a server, write a memory, see it held and approve it as a second key.

    [Quickstart](start/quickstart.md)

- **Wire an agent**

    Connect Claude Code, claude.ai or another tool to a server.

    [Get an agent key](guides/get-an-agent-key.md), then [connect Claude Code](guides/claude-code.md)

- **Run a server**

    One command starts the database and server on the machine; then deploy for a team.

    [Run a server](start/run-your-own.md)

- **Decide whether to adopt it**

    Why writes are held, what it costs, its limits, benchmarks and security, on one page.

    [Evaluate OML](product/evaluate.md)

</div>

## See the held state

With the `oml` CLI and Docker, these lines run as pasted. The [quickstart](start/quickstart.md) shows each command's output and has a variant without Docker.

```sh
oml quickstart                       # a local server, a workarea, an agent key and a reviewer key
export OML_URL=http://127.0.0.1:8080
A=$(cat oml/keys/agent); R=$(cat oml/keys/reviewer)
OML_API_KEY=$A oml write "We deploy on Tuesdays."
OML_API_KEY=$A oml read --query "deploy"                       # 0 hits: the memory is held
OML_API_KEY=$A oml read --query "deploy" --include-unreviewed  # the key's own held memory
ID=$(OML_API_KEY=$A oml read --query "deploy" --include-unreviewed --json | jq -r '.. | .artifact_id? // empty' | head -1)
OML_API_KEY=$R oml review approve --ids $ID                    # a different key approves
OML_API_KEY=$A oml read --query "deploy"                       # found
```

## Where things are

| Tab | What it holds |
|---|---|
| [Get started](start/index.md) | install, quickstart, connect an agent |
| [Guides](guides/index.md) | tasks: remember, review, organize, sync |
| [Operate](operations/index.md) | run a server: deploy, monitor, upgrade, back up |
| [Reference](reference/index.md) | CLI, HTTP API, MCP tools, settings, errors, limits, glossary |
| [Concepts](concepts.md) | why a write is held, roles, workareas, search, security |
| [Benchmarks](benchmarks/index.md) | recall quality and latency, with method |
| [Help](faq.md) | FAQ, troubleshooting, release notes |

The current release is v0.7.3 ([release notes](changelog.md)). Open issues are on [GitHub](https://github.com/e8kor/open-memory-lab/issues). Code is Apache-2.0; this documentation is CC BY 4.0.
