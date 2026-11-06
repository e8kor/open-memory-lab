<!--
id: index
tier: client
status: current
answers: what Open Memory Lab is, what it guarantees, and where to start
-->

# Open Memory Lab

Open Memory Lab (OML) is shared memory for AI agents. What an agent writes is held until someone
who did not write it approves it; only then do other agents and people find it. Every step is
signed into a history an auditor can verify, so you can answer "who approved this, and on what
basis" for any memory an agent acts on. The price is review effort and a write that waits;
the return is that one agent's wrong conclusion does not become every agent's fact.

The current release is v0.6.0 ([release notes](changelog.md)).

## What you get

- **Held, then reviewed, then visible.** A write is scanned and readable by its author alone. A
  different identity approves it before anyone else reads it. Approval changes visibility, never
  ranking.
- **Workareas that mirror your crews.** Memory sits in a tree of workareas; a key reads and
  writes only where it holds a grant, and a denial always wins.
- **A signed history.** Every write, verdict and key issuance is chained and signed. `oml audit`
  verifies the chain; a fork stops the server at startup.
- **Search that stays fast.** Recall reads an index: about 25 ms at the 95th percentile with
  about 100,000 memories readable ([benchmarks](api/benchmarks/scale.md)).
- **Three agent tools.** `oml_write`, `oml_read` and `oml_workarea`. An agent may never promote,
  sign, demote or delete.
- **Works where your agents run.** Claude Code, claude.ai and the Claude apps, Codex CLI,
  Cursor, Gemini CLI, VS Code and Claude Desktop.

## Start here

| You want to | Go to |
|---|---|
| Use a server you have a key for | [Get started](start/index.md), then [Claude Code](guides/claude-code.md) |
| Run your own server | [Run your own server](start/run-your-own.md) |
| Give an agent its key | [First workarea and keys](start/first-workarea.md) |
| Save and find a first memory | [Your first memory](start/first-memory.md) |
| Fix a problem | [FAQ](faq.md) and [troubleshooting](guides/troubleshooting.md) |
| Understand the guarantees | [Concepts](concepts.md) |

Every decision behind OML is a numbered [design record](adr/index.md). Open work is tracked as
[GitHub issues](https://github.com/e8kor/open-memory-lab/issues?q=label%3Aroadmap). The code is
Apache-2.0 and this documentation CC BY 4.0.
