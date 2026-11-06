---
status: Stable
verified: 2026-11-06
answers: "what Open Memory Lab is, which door to take, and the commands that reach a first recall"
---

# Open Memory Lab

Agents write memories. A different identity reviews them before anyone else can recall them.

<div class="grid cards" markdown>

- **Wire an agent**

    Connect Claude Code, claude.ai or another tool to a server.

    [Connect Claude Code](guides/claude-code.md)

- **Run your own server**

    One command starts the database and server on your machine; then deploy for a team.

    [Run your own server](start/run-your-own.md)

- **Decide whether to adopt it**

    Who OML is for, what it costs and when not to use it.

    [Is OML for you?](product/is-oml-for-you.md)

</div>

## See the held state in six commands

```sh
oml quickstart                       # a local server, a workarea, an agent key and a reviewer key
OML_API_KEY=$A oml write "We deploy on Tuesdays."
OML_API_KEY=$A oml read --query "deploy"                       # no hit: the memory is held
OML_API_KEY=$A oml read --query "deploy" --include-unreviewed  # your own held memory
OML_API_KEY=$R oml review approve --ids <artifact_id>          # a different key approves
OML_API_KEY=$A oml read --query "deploy"                       # found
```

The [quickstart](start/quickstart.md) sets `A` and `R` and explains each step.

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

The current release is v0.6.0 ([release notes](changelog.md)). Open issues are on [GitHub](https://github.com/e8kor/open-memory-lab/issues). Code is Apache-2.0; this documentation is CC BY 4.0.
