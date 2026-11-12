# The `oml` agent plugin

This plugin connects Claude Code to an Open Memory Lab server over MCP. It adds three
tools, `oml_write`, `oml_read` and `oml_workarea`, a skill that explains when to use
them, and a read-only subagent that searches for you. Every write is held until a
different identity approves it.
That is the point of the server, and it means a fact you store is not shared the moment
you store it.

## Install

*Available since v0.1.0; the pinned marketplace and offline archive since v0.3.1; setup through Claude Code's plugin settings since v0.4.0; the default server since v0.4.1 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

The plugin takes two settings: `url`, the server without `/mcp`, and `api_key`, this agent's own
key. Since v0.4.1 `url` defaults to the maintainer's public server, `https://memory.doublegate.io`,
so with a key from that server the key is the only thing to enter. Claude Code keeps the key in
the system keychain, never in `settings.json`.

In a Claude Code session, add the marketplace and install the plugin; the install opens a dialog
for the two settings, and the key field is masked:

```text
/plugin marketplace add e8kor/open-memory-lab
/plugin install oml@open-memory-lab
```

Paste the key and leave `url` empty or at its default for the public server; for your own
server, enter its address. Run `/plugin configure
oml@open-memory-lab` to change either setting later, then `/reload-plugins`.

The default costs you something if your key is from another server: left unchanged, `url` sends
that key to the maintainer's server, which refuses it with `401 auth.invalid`, so the plugin
serves no tools and the session-start hook loads nothing. The operator of that server has then
seen the key; ask your administrator to replace it.

With the `oml` CLI ([below](#the-oml-cli)), one command installs the plugin at the CLI's own
release and configures it, refusing an administrator's key:

```sh
oml setup claude-code --key-file "$HOME/.config/oml/keys/crew-agent-7"
```

Without `--url` it pairs the key with `https://memory.doublegate.io` and says so before it sends
anything. For your own server, run the line `oml keys issue-agent --out` printed, `oml setup
claude-code --url <url> --key-file <file>`, or log in first:

```sh
oml login https://oml.example.com      # once: checks the key, then stores the pair
oml setup claude-code
```

The [Claude Code guide](../../docs/guides/claude-code.md) walks the whole path.

From a shell without the CLI, `claude plugin install` opens no dialog, so pipe the settings to
`claude plugin configure --values-stdin`. This path needs Claude Code 2.1.285 or later. With the
key in the owner-only file that `oml keys issue-agent --out` writes, `jq` reads the file and the
key never enters a command line or your shell history; for the public server, `u` is
`https://memory.doublegate.io`:

```sh
claude plugin marketplace add e8kor/open-memory-lab
claude plugin install oml@open-memory-lab
jq -n --arg u 'https://oml.example.com' --rawfile k "$HOME/.config/oml/keys/crew-agent-7" \
  '{url: $u, api_key: ($k | rtrimstr("\n"))}' \
  | claude plugin configure oml@open-memory-lab --values-stdin
```

Until `api_key` is set the plugin serves no tools. The session-start hook below reads the same
two settings, and with `api_key` set and `url` left at its default it reaches the same default
server the tools do.

### One release, or offline

From the first release that carries them, each release attaches the plugins in two more forms,
each signed with cosign and listed in `SHA256SUMS`. To install exactly one release, pin the
marketplace to its tag:

```sh
claude plugin marketplace add e8kor/open-memory-lab#vX.Y.Z
```

or verify `oml-marketplace-X.Y.Z.json`, whose sources are this repository at that tag and its
commit, and add it from a directory:

```sh
v=X.Y.Z
base=https://github.com/e8kor/open-memory-lab/releases/download/v$v
curl -fsSLO "$base/oml-marketplace-$v.json" -O "$base/oml-marketplace-$v.json.sigstore.json"
cosign verify-blob \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' \
  --bundle "oml-marketplace-$v.json.sigstore.json" "oml-marketplace-$v.json" \
  && mkdir -p oml-marketplace/.claude-plugin \
  && cp "oml-marketplace-$v.json" oml-marketplace/.claude-plugin/marketplace.json \
  && claude plugin marketplace add ./oml-marketplace
```

With no network, take `oml-claude-plugins-X.Y.Z.tar.gz` (or `.zip`) instead: verify it the same
way, extract it, and run `claude plugin marketplace add ./oml-claude-plugins-X.Y.Z`. Claude Code
reads the plugins from that directory in place, so keep it where you extracted it. Every form
names the marketplace `open-memory-lab`, so `claude plugin install oml@open-memory-lab` installs
from whichever you added, and you hold one of them at a time. See
[release artifacts](../../docs/contributing/ci-and-toolchain.md).

The plugin registers one MCP connection, `oml`, and every tool runs on the one key behind
it. Issue that key with the writer role, a writer grant and a reader grant on the agent's
workarea, and that workarea as its default. A key without the reader grant still writes,
but recall through `oml_read` refuses it with `access.not_found`.

Every tool argument that takes a workarea is `workarea`, and takes its name, such as
`metals-crew`, or its id. A workarea's name is unique in the tenant and never changes. A name
the key cannot see is refused exactly as its id would be, with `access.not_found`, and a path
such as `universe.procurement.metals-crew` is not a name and is refused.

## Why can't I find what I just saved?

Every write is held until a reviewer who did not write it approves it. Until then only its author
reads it: the agent finds it with `oml_read` and `include_unreviewed: true`, and nobody else finds
it at all. Once it is approved, every key that reads the workarea finds it.

## The loop

On that one key the agent:

1. checks what it may do in its workarea with `oml_workarea`;
2. recalls before it acts with `oml_read` and a `query`, reading the `steering` rows first;
3. writes with `oml_write`, adding `derives_from` for a conclusion drawn from memories it
   holds, and gets `state: "held"` back; the server derives the `claim_key`;
4. lists its own writes with `oml_read` and no `query`, reads one back with its
   `artifact_id`, and searches them beside the approved memories by passing
   `include_unreviewed: true` with the `query`; no other key reads or recalls them before
   approval;
5. learns whether a different identity approved them by listing its own work with `state`,
   after which every key that reads the workarea recalls them;
6. revises or retires a memory with `oml_write`, `replaces` and the `expected_version` it read,
   which is held and approved like any write while recall keeps serving the version it
   replaces.

The arguments choose the mode, and an argument outside it is refused with
`request.validation` naming it. Recall takes `max_tokens` to bound its answer: the steering
rows are never cut, the hits are the longest run from the top of the ranking that fits, and
`budget` reports the cut.

Give each agent its own key. Attribution, and the rule that a reviewer never reviews
their own work, both depend on it. To run a server, see the
[self-hosting guide](../../docs/start/run-your-own.md).

`oml keys issue-agent <key-name> --principal <principal> --workarea <workarea>` issues an
agent's key in one call: a writer key named `<key-name>`, with a writer and a reader grant on the
workarea, given by its name or id, which is also its default. With `--out <path>` the value goes
to a new owner-only file, the file the shell path under "Install" reads, and the CLI prints
that path's `oml setup claude-code --key-file` line and its `jq | claude plugin configure`
line, filled in; the `oml-admin` plugin
in this marketplace runs it that way from inside Claude Code, so the value never enters the
conversation. See the
[CLI reference](../../docs/cli/reference.md#oml-keys-issuance-revocation-rotation).

## The `oml` CLI

*Available since v0.1.0; per-platform archives since v0.3.1; the installer since v0.4.0 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

`oml setup claude-code`, the session-start hook below and key issuance need the `oml` CLI on
your `PATH`. One line installs it for you alone and checks the download:

```sh
curl -fsSL --proto '=https' --tlsv1.2 https://raw.githubusercontent.com/e8kor/open-memory-lab/v0.7.0/scripts/install/install.sh | sh
```

v0.4.0 is the first release to carry the installer. Windows, the inspect-first form and the
manual `cosign` steps are on the [install page](../../docs/start/install.md). On Windows the hook
runs only when Git for Windows, which provides Git Bash, is installed; see the
[CLI reference](../../docs/cli/reference.md#installing-oml-and-what-differs-on-windows).

## Every session starts from memory

*Available since v0.2.2; the plugin settings first, the compaction prompt naming `oml_write` and the two read tools since v0.4.0 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

When the `oml` CLI is on your `PATH`, the plugin's SessionStart hook runs `oml hook
session-start` each time a session starts, resumes, is cleared, is compacted or is forked. It
sends two requests with the agent's key, both to that one server: `GET
/v1/memories?own=true&limit=10`, your key's ten newest submissions, and `POST
/v1/memories/recall`, whose query is the name of the directory you work in, split into words
(`open memory lab` for `~/src/open-memory-lab`, never the rest of the path), and which answers
the workarea's steering and at most 800 budget tokens of matching memories. It adds what comes
back to the session as stored data to check. After a compaction it also asks Claude to record the
summary's durable facts with `oml_write`. The hook reads its server and key from the plugin's two
settings, the ones the tools use, `url` at its default when only `api_key` is set; with neither
set, it reads `OML_URL` and `OML_API_KEY`, for a hook run outside the plugin, with no default
server, and never one value from each, nor the CLI's own credentials. With no whole pair it sends
nothing, and it prints nothing when the server does not answer. Set `OML_SESSION_CONTEXT=off` to turn it off, or
`OML_SESSION_CONTEXT_MAX_TOKENS` to change the budget. Without the CLI the hook does nothing. See
the [CLI reference](../../docs/cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin).

A plugin cannot allow its own tools for you, so Claude Code asks you before each call to one
of them until you allow it. To stop the question on reads and keep it on every write, add the
two read tools to `permissions.allow` in your settings:

```json
{
  "permissions": {
    "allow": [
      "mcp__plugin_oml_oml__oml_read",
      "mcp__plugin_oml_oml__oml_workarea"
    ]
  }
}
```

The broader rule, `mcp__plugin_oml_oml`, allows every tool of the plugin, the write too, and
costs you the sight of it: Claude then calls `oml_write`, which can propose retiring a claim,
without showing you the call, so whatever the session
chooses to record leaves for the server unseen. What it chooses is shaped by the stored text the
hook adds at session start, which other writers in the workarea can influence. Every write is
still held until a different identity reviews it, so nothing it records is shared before then.

## Recall after a failed command (opt-in)

*Available since v0.4.0 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

Set `OML_FAILURE_RECALL=1` and, when a Bash command fails, the plugin's PostToolUseFailure hook
runs `oml hook tool-failure`: it searches with up to three distinctive words of the error, such
as an error code or an identifier, with keys and tokens removed first, and adds at most three
matching memories, marked as possibly irrelevant. It skips `grep`, `test`, `[` and `diff`, an
error it already searched for in the session, and every search after one that failed; a miss adds
nothing. It reads its server and key as the session-start hook does, the plugin's two settings
first. The cost is that the error's words go to your server. A per-session file under
`~/.cache/oml/sessions` remembers which memories the session already has, never their text, and
the SessionEnd hook deletes it. See the
[CLI reference](../../docs/cli/reference.md#oml-hook-tool-failure-recall-after-a-failed-command).

## Chat memory (opt-in)

*Since v0.6.0 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

Run `oml chat init` once with your own key, not an agent's: it opens `chat` under your user
root, the one place your agents write what you tell them, and prints its id. Put that id in the
plugin's `chat_workarea` setting, or `OML_CHAT_WORKAREA`, and set `OML_CHAT_CAPTURE=1`. Every
fifth turn (`OML_CHAT_CAPTURE_EVERY`), and at once after a compaction, the Stop hook asks Claude
to use the `oml-chat-capture` skill: Claude itself picks out the preferences, decisions and
corrections those turns settled and writes each as one held memory, with the turn it came from.
No other model reads the conversation and no transcript leaves your machine. Each fact names its
turn by a fingerprint keyed with a secret salt kept owner-only in `~/.config/oml/chat-salt` and never
sent, so the fingerprint cannot be checked against a guessed prompt by anyone without that file. A session asks at most four
times and for at most 24 facts, and never while a Stop hook is already continuing. The cost is
the tokens Claude spends on those short continuations (Claude is asked for at most 24 facts, and
the server does not count them), and the last turns of a session that ends
between two captures are not kept. `OML_CHAT_EXCERPT=1` lets Claude quote up to 280 characters of
what you said under a fact.

Set `OML_TURN_RECALL=1` and the UserPromptSubmit hook searches with up to eight words of each
prompt, in your chat workarea and the key's default workarea, and adds at most six memories,
each marked `[unreviewed]` when no one has reviewed it yet. It waits at most
`OML_TURN_RECALL_BUDGET_MS` (300 ms by default, at most 1,000) and adds nothing when the server is
slower; after three slow answers it stops for the session. The cost is that your prompt's words
go to your server on every turn, and a short secret typed as plain words, such as a password, is
not recognised as one and goes with them. Nothing you or Claude writes leaves the chat workarea until you
review it.

## Guardrails

*Since v0.5.1 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

When the `oml` CLI is on your `PATH`, the plugin also runs `oml hook guardrails` at every session
start, after a compaction and when Claude starts a subagent. It loads the reviewed rules in force
for your key's workarea, and for the workareas above it when the key's guardrail scope is
`ancestors`, which an admin sets when issuing the key. Claude sees them as instructions, each with
where it was set and how many reviewers approved it. At session start it also writes the rules
that carry a check to a file only you can read, under `~/.local/state/oml/guardrails`, and the
PreToolUse hook, `oml hook pre-tool-use`, checks each matching tool call against that file before
it runs: a match denies the call or asks you, and nothing is ever allowed by a rule. The check
sends nothing to the server. Without the file, after a server outage at session start, the check
decides nothing and says so once per call; restart the session to load the rules. A text rule is
an instruction to Claude, not a guarantee, and a checked rule holds only where this plugin's hooks
run. See the
[CLI reference](../../docs/cli/reference.md#oml-hook-guardrails-and-oml-hook-pre-tool-use).

## The librarian

*Available since v0.4.0 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

The plugin adds one subagent, `oml:librarian`, for "what does OML hold about X". Claude delegates
to it, or you ask for it with `@agent-oml:librarian`. One query that finds nothing proves
little; the librarian runs up to eight, the subject's own words and then their variants
(synonyms, abbreviations, the most distinctive word alone), and returns each finding with its `artifact_id`, `claim_key`, workarea and time,
under the same data line the hooks use, with the queries it ran. Its only tool is `oml_read`: it
cannot call `oml_write`, edit a file or run a command. It runs on Haiku, so the searching costs
less than on the main conversation's model and its raw results stay out of that conversation's
context; the cost is that a smaller model chooses the variants and which hits matter. The
`oml_read` rule under `permissions.allow` above applies to the whole session, the librarian's
calls included; without it, each of its calls asks you in the main session.

## Other MCP clients

*Available since v0.1.0; the agent kit since v0.3.1 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

The server speaks stateless streamable HTTP at `/mcp`. Any client that accepts this shape
works:

```json
{
  "mcpServers": {
    "oml": {
      "type": "http",
      "url": "http://127.0.0.1:8080/mcp",
      "headers": { "Authorization": "Bearer ${OML_API_KEY}" }
    }
  }
}
```

Each release, from the first that carries it, also attaches `oml-agent-kit-X.Y.Z.tar.gz` (and
`.zip`), signed like the archives above: the configuration in each tool's own form for OpenAI
Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop, an `AGENTS.md` block with this
plugin's skill, and a Gemini CLI session-start hook running the same `oml hook session-start`.
Its README, [kit/README.md](../../kit/README.md), cites each tool's documentation and marks what
that documentation does not settle as unverified.

## Upgrading from 0.3

*Available since v0.4.0 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

Release 0.4.0 reads the server and the key from the plugin's two settings, not from `OML_URL` and
`OML_API_KEY`: a 0.3 install that only exported the variables gets no tools after the update until
you configure the plugin once. Run `claude plugin update oml@open-memory-lab` (auto-update is off
by default for a third-party marketplace), then set `url` to the old `OML_URL`, since from 0.4.1
an unset `url` is the maintainer's public server, and `api_key` to the old key, in the dialog or through the shell path under "Install". The session-start hook
reads the same two settings first, so the exported variables can then leave your shell profile,
unless the `oml` CLI or another client still reads them.

Release 0.4.0 also replaces the eight 0.3 tools with these three. The `permissions.allow` rules
you wrote for the 0.3 read tools, `oml_recall`, `oml_get`, `oml_my_work`, `oml_workarea_info` and
`oml_events_tail`, allow nothing any more: replace them with the two rules above, or with
`mcp__plugin_oml_oml`. Run `claude plugin update oml@open-memory-lab` and install the 0.4.0
CLI, so the skill and the session-start hook name the new tools. The events tail is no longer a
tool: an agent learns what became of its writes with `oml_read` and `state`, and an auditor reads
`GET /v2/events`.

## What is served today

*Eight tools since v0.2.0; three task-shaped tools since v0.4.0 · [releases](https://e8kor.github.io/open-memory-lab/changelog/)*

[docs/api/mcp-tools.json](../../docs/api/mcp-tools.json) declares exactly the three tools above,
each with its MCP annotations: `oml_read` and `oml_workarea` are read-only, and `oml_write` is
neither read-only nor destructive, since every write is held and a retirement is a proposal.
[docs/api/mcp-tools.designed.json](../../docs/api/mcp-tools.designed.json) holds what is designed
and not served: `oml_relate_propose`, `oml_vocab`, `oml_read`'s lineage mode and `oml_write`'s
`attachments`. A designed tool answers an error saying it is designed. No tool uploads files,
since MCP carries no file bytes; files will go through the CLI and the SDKs. These arrive with
the milestones in the critical path.
