<!--
id: index
tier: client
status: current
answers: what Open Memory Lab is, what it guarantees, and where to start
-->

# Open Memory Lab

Shared memory for AI agents, with a review gate. What an agent writes is held until someone who
did not write it approves it; only then do other agents and people find it.

<div class="grid cards" markdown>

- **Get started**

    Install the CLI, connect Claude Code, save a first memory. About two minutes with a key.

    [Get started](start/index.md)

- **Use with your agent**

    Remember, recall and review; arrange workareas; keep docs and code in sync.

    [Guides](guides/index.md)

- **Self-host**

    One command starts the database and server; then deploy, monitor and upgrade.

    [Run your own server](start/run-your-own.md)

</div>

Install problem? [Install troubleshooting](guides/troubleshooting.md#the-installer-refuses) and the [FAQ](faq.md).

## What OML is

An external memory provider. An agent has three tools, `oml_write`, `oml_read` and `oml_workarea`,
and may never promote, sign, demote or delete. Memory sits in a tree of workareas; a key reads
and writes only where it holds a grant, and a denial always wins. Recall reads an index: about
25 ms at the 95th percentile with about 100,000 memories readable ([benchmarks](benchmarks/scale.md)).
It works where your agents run: Claude Code, claude.ai and the Claude apps, Codex CLI, Cursor,
Gemini CLI, VS Code and Claude Desktop.

## Why the review gate

One agent's wrong conclusion should not become every agent's fact. A write is scanned and readable
by its author alone; a different identity approves it before anyone else reads it. Approval
changes visibility, never ranking. Every write, verdict and key issuance is chained and signed, so
`oml audit` answers "who approved this, and on what basis". The price is review effort and a
write that waits. [What OML guarantees](product/commitments.md) has the full list.

## Where things are

| Tab | What it holds |
|---|---|
| [Get started](start/index.md) | install, connect, first memory |
| [Guides](guides/index.md) | tasks: use, integrate, operate |
| [Reference](reference/index.md) | CLI, HTTP API, MCP tools, settings, errors |
| [Concepts](concepts.md) | the trust model, workareas, review, search |
| [Benchmarks](benchmarks/index.md) | recall quality and latency, with methodology |
| [FAQ and troubleshooting](help/index.md) | when something does not work |
| [Design records](adr/index.md) | every decision, numbered |

The current release is v0.6.0 ([release notes](changelog.md)). Open work is tracked as
[GitHub issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap). The code is
Apache-2.0 and this documentation CC BY 4.0.
