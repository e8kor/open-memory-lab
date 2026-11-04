<!--
id: start/index
tier: client
status: draft for review · describes v0.4.0
answers: the three ways to start with Open Memory Lab, each led by one command: use OML with a server and a key, run OML on a machine with Docker, and add an agent as an administrator; what each needs, what it prints, its target time, and when a first memory becomes visible to others
-->

# Get started

**Status:** draft for review · describes v0.4.0

There are three ways in, one for each role. Each starts with one command. Every client needs
exactly two settings, the server's address and a key, and the server works out everything else
from the key.

The commands on this page are for v0.4.0 ([releases](../product/releases.md)).

## Use OML: you have a server address and a key

You need Claude Code on your machine, and two things from your administrator: the server's
address, such as `https://oml.example.com`, and an agent key.

```sh
curl -fsSL --proto '=https' --tlsv1.2 https://raw.githubusercontent.com/e8kor/open-memory-lab/v0.4.0/scripts/install/install.sh | sh
oml login https://oml.example.com
oml setup claude-code
```

1. The installer puts the `oml` command in your home directory without administrator rights, and
   says what it checked ([install](install.md)).
2. `oml login` asks for the key without showing it, checks it with the server, and only then
   stores the address and the key in a file only you can read. `oml doctor` shows what it
   stored, never the key.
3. `oml setup claude-code` installs the OML plugin in Claude Code at the same release and hands it
   the same two settings. It refuses an administrator's key. Run `/reload-plugins` in Claude
   Code, or restart it.

Ask your agent to remember something. The answer is **held**: the agent can read it back at once,
and everyone else in the workarea finds it only after a reviewer approves it. The target is about
two minutes from the first line to that held save; no run has timed it yet
([roadmap](../product/roadmap.md)).

The [Claude Code guide](../guides/claude-code.md) covers the same path step by step. For claude.ai
or a phone, see [claude.ai and mobile](../guides/claude-ai-and-mobile.md); for Codex, Cursor,
Gemini CLI, VS Code or Claude Desktop, see [other tools](../guides/other-harnesses.md).

## Run OML: you have a machine with Docker

You need Docker with compose v2. Install `oml` as above, then:

```sh
oml quickstart
```

It writes a compose file and private passwords into `./oml`, starts the database and the server
on `127.0.0.1:8080` only, claims the new server, creates the workarea `crew`, and issues an agent
key and a reviewer key into private files under `./oml/keys`. It then saves one memory as the
agent, approves it as the reviewer and finds it as the agent, and stops naming the step that
failed if one does. Last, it prints the line that connects Claude Code on this machine and the
`oml login` line for another one.

**Unverified.** The verb's own suite runs against a fake `docker`; its compose file has no
recorded green run under Docker yet ([self-hosting](../guides/self-hosting.md)).

The target is about five minutes on a machine with Docker, most of it the image download; no run
has timed it yet. A second run in the same directory is refused, so nothing is overwritten. The
laptop setup is one trust domain: anyone with root on the machine can read every file of it. The
[self-hosting guide](../guides/self-hosting.md) goes from here to a server for a team.

## Add an agent: you administer a server

With the administrator's key stored by `oml login`:

```sh
oml keys issue-agent agent-7 --principal crew/agent-7 --workarea crew --out "$HOME/.config/oml/keys/crew-agent-7"
```

The quick start keeps its administrator pair in `./oml/credentials`, not in the file `oml login`
writes, so against that stack name the file, as the quick start prints it:

```sh
OML_CLI_CREDENTIALS=./oml/credentials oml keys issue-agent agent-7 --principal crew/agent-7 --workarea crew --out "$HOME/.config/oml/keys/crew-agent-7"
```

It issues one key for one agent: it may save in the workarea and read it, and the workarea is its
default. The key goes into a new file only you can read, and never onto the screen. The command
prints, among its lines, the one to run where the agent works:

```sh
oml setup claude-code --url 'https://oml.example.com' --key-file '/home/you/.config/oml/keys/crew-agent-7'
```

Send that person the line and the file over a channel you trust, and delete the file once the line
has run. The target is under a minute per key; no run has timed it yet. Give each agent its own
key: attribution, and the rule that nobody reviews their own work, both depend on it. Inside
Claude Code, the `oml-admin` plugin runs the same command without the key entering the
conversation ([CLI reference](../cli/reference.md#the-oml-admin-plugin)).

## What every path has in common

- **Two settings.** The address and the key. The command line reads them from its flags, then
  from `OML_URL` and `OML_API_KEY`, then from the file `oml login` wrote.
- **Held first.** Nothing an agent saves reaches anyone else until a different identity approves
  it. That is the point of OML, and it is why a fact your agent just stored does not show up for
  a teammate yet.
- **Targets, not measurements.** Every time on this page is a target that the
  [roadmap](../product/roadmap.md) tracks until a timed run replaces it.
