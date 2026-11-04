# The `oml-admin` plugin

This plugin is for the person who administers an Open Memory Lab server from Claude Code. It
adds one skill, `issue-agent-key`, and no tool: it issues an agent's key through the `oml` CLI on
your machine, under the admin key the CLI already uses, and the key value never enters the
conversation. Install it beside, not instead of, the agent plugin `oml`; an agent that installs
`oml` gains no admin instruction.

## Install

*Available since v0.2.1 · [releases](https://e8kor.github.io/open-memory-lab/product/releases/)*

```text
/plugin marketplace add e8kor/open-memory-lab
/plugin install oml-admin@open-memory-lab
```

It needs the `oml` CLI on your `PATH`, logged in with an admin key (`oml login <url>`), or the
admin pair `oml quickstart` or `oml bootstrap claim` stored.

## What it does

Ask Claude to issue a key for an agent. The skill asks for the key's name, the principal and the
workarea, then runs:

```sh
oml keys issue-agent agent-7-laptop --principal crew/agent-7 --workarea metals-crew --out "$HOME/.config/oml/keys/crew-agent-7"
```

The value goes to a new owner-only file. The CLI prints four lines that carry the file's path and
never the value, and the skill hands you the one to run on the agent's machine:

```sh
oml setup claude-code --url 'https://oml.example.com' --key-file '/home/op/.config/oml/keys/crew-agent-7'
```

which installs the `oml` plugin at the CLI's release and configures it, refusing an admin key.
Where that machine has no `oml` CLI, the skill hands you the printed `jq | claude plugin configure
--values-stdin` line instead. Delete the key file once the line has run: it is the only copy of the
value, and the server keeps only its hash.

The skill never runs `issue-agent` without `--out` or with `--json`, never reads the key file, and
never issues connector keys for claude.ai; `oml keys issue-agent --connect-code` is run in a
terminal. See the [CLI reference](../../docs/cli/reference.md#the-oml-admin-plugin).
