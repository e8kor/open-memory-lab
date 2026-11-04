# The `oml` agent plugin

This plugin connects Claude Code to an Open Memory Lab server over MCP. It adds eight
tools, `oml_remember`, `oml_learn`, `oml_recall`, `oml_get`, `oml_supersede`,
`oml_my_work`, `oml_workarea_info` and `oml_events_tail`, plus a skill that explains when
to use them. Every write is held until a different identity approves it.
That is the point of the server, and it means a fact you store is not shared the moment
you store it.

## Install

*Available since v0.1.0; the pinned marketplace and offline archive since v0.3.1 · [releases](https://e8kor.github.io/open-memory-lab/product/releases/)*

```sh
export OML_URL=http://127.0.0.1:8080        # the server, without /mcp
export OML_API_KEY=oml_...                  # this agent's one key
claude plugin marketplace add e8kor/open-memory-lab
claude plugin install oml@open-memory-lab
```

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
[release artifacts](../../docs/engineering/ci-and-toolchain.md#release-artifacts-sbom-signing-provenance).

The plugin registers one MCP connection, `oml`, and every tool runs on the one key behind
it. Issue that key with the writer role, a writer grant and a reader grant on the agent's
workarea, and that workarea as its default. A key without the reader grant still writes,
but `oml_recall` refuses it with `access.not_found`.

Every tool argument that takes a workarea, `workarea` or `scope`, takes its name, such as
`metals-crew`, or its id. A workarea's name is unique in the tenant and never changes. A name
the key cannot see is refused exactly as its id would be, with `access.not_found`, and a path
such as `universe.procurement.metals-crew` is not a name and is refused.

## The loop

On that one key the agent:

1. checks what it may do in its workarea with `oml_workarea_info`;
2. recalls before it acts with `oml_recall`, reading the `steering` rows first;
3. writes with `oml_remember`, or `oml_learn` for a conclusion drawn from memories it
   holds, and gets `state: "held"` back;
4. sees its held writes in `oml_my_work`, reads one back with `oml_get`, and searches them
   beside the approved memories by passing `include_own_held: true` to `oml_recall`; no
   other key reads or recalls them before approval;
5. waits for a different identity to approve them, following `oml_events_tail`, after
   which every key that reads the workarea recalls them;
6. revises or retires a memory with `oml_supersede`, which is held and approved like any
   write while recall keeps serving the version it replaces.

`oml_recall` takes `max_tokens` to bound its answer: the steering rows are never cut, the
hits are the longest run from the top of the ranking that fits, and `budget` reports the cut.

Give each agent its own key. Attribution, and the rule that a reviewer never reviews
their own work, both depend on it. Keep the key in the environment, not in a file you
commit. To issue keys and run a server, see "Quickstart" in the
[repository README](../../README.md#quickstart).

`oml keys issue-agent <key-name> --principal <principal> --workarea <workarea>` issues an
agent's key in one call: a writer key named `<key-name>`, with a writer and a reader grant on the
workarea, given by its name or id, which is also its default. It prints the `export OML_URL=…`
and `export OML_API_KEY=…` lines to run before you start Claude Code. With `--out <path>` the
value goes to a new owner-only file and the second line reads it from there; the `oml-admin`
plugin in this marketplace runs it that way from inside Claude Code, so the value never enters
the conversation. See the
[CLI reference](../../docs/cli/reference.md#oml-keys-issuance-revocation-rotation).

## The `oml` CLI

*Available since v0.1.0; per-platform archives since v0.3.1 · [releases](https://e8kor.github.io/open-memory-lab/product/releases/)*

The session-start hook below and key issuance need the `oml` CLI on your `PATH`. From the first
release that carries them, each release attaches it as `oml-cli-X.Y.Z-<target>.tar.gz`, or `.zip`
on Windows, with a signature bundle beside each archive. Verify the archive, then extract it:

Linux, on x86_64 (on arm64, `t=aarch64-unknown-linux-musl`); the binary is static, so it runs on
any distribution:

```sh
v=X.Y.Z t=x86_64-unknown-linux-musl          # the release and your target
base=https://github.com/e8kor/open-memory-lab/releases/download/v$v
curl -fsSLO "$base/oml-cli-$v-$t.tar.gz" -O "$base/oml-cli-$v-$t.tar.gz.sigstore.json"
cosign verify-blob \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' \
  --bundle "oml-cli-$v-$t.tar.gz.sigstore.json" "oml-cli-$v-$t.tar.gz" \
  && tar -xzf "oml-cli-$v-$t.tar.gz" \
  && install -D -m 0755 "oml-cli-$v-$t/oml" ~/.local/bin/oml
```

macOS: the same lines with `t=aarch64-apple-darwin` (Apple silicon) or `t=x86_64-apple-darwin`
(Intel), with the last line `install -m 0755 "oml-cli-$v-$t/oml" /usr/local/bin/oml`, or any
directory on your `PATH`.

Windows, in PowerShell:

```powershell
$v = "X.Y.Z"; $t = "x86_64-pc-windows-msvc"; $n = "oml-cli-$v-$t"
$base = "https://github.com/e8kor/open-memory-lab/releases/download/v$v"
Invoke-WebRequest "$base/$n.zip" -OutFile "$n.zip"
Invoke-WebRequest "$base/$n.zip.sigstore.json" -OutFile "$n.zip.sigstore.json"
cosign verify-blob `
  --certificate-oidc-issuer https://token.actions.githubusercontent.com `
  --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' `
  --bundle "$n.zip.sigstore.json" "$n.zip"
if ($LASTEXITCODE -eq 0) { Expand-Archive "$n.zip" -DestinationPath . }
```

Then put `$n\oml.exe` in a directory on your `PATH`. Extract only after `cosign verify-blob`
exits 0: an archive that does not verify is not a release.

On Windows the hook runs only when Git for Windows, which provides Git Bash, is installed. See
the [CLI reference](../../docs/cli/reference.md#installing-oml-and-what-differs-on-windows).

## Every session starts from memory

*Available since v0.2.2 · [releases](https://e8kor.github.io/open-memory-lab/product/releases/)*

When the `oml` CLI is on your `PATH`, the plugin's SessionStart hook runs `oml hook
session-start` each time a session starts, resumes, is cleared, is compacted or is forked. It reads the
workarea's steering, your key's ten newest submissions and the memories that match the name of
the directory you work in, at most 800 budget tokens of matches, and adds them to the session as
stored data to check. After a compaction it also asks Claude to record the summary's durable facts
with `oml_remember`. It reads `OML_URL` and `OML_API_KEY`, never the CLI's own credentials, and
prints nothing when the server does not answer. Set `OML_SESSION_CONTEXT=off` to turn it off, or
`OML_SESSION_CONTEXT_MAX_TOKENS` to change the budget. Without the CLI the hook does nothing. See
the [CLI reference](../../docs/cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin).

A plugin cannot allow its own tools for you, so Claude Code asks you before each call to one
of them until you allow it. To stop the question on reads and keep it on every write, add the
read tools to `permissions.allow` in your settings:

```json
{
  "permissions": {
    "allow": [
      "mcp__plugin_oml_oml__oml_recall",
      "mcp__plugin_oml_oml__oml_get",
      "mcp__plugin_oml_oml__oml_my_work",
      "mcp__plugin_oml_oml__oml_workarea_info",
      "mcp__plugin_oml_oml__oml_events_tail"
    ]
  }
}
```

The broader rule, `mcp__plugin_oml_oml`, allows every tool of the plugin, the writes too, and
costs you the sight of them: Claude then calls `oml_remember`, `oml_learn` and `oml_supersede`,
which can propose retiring a claim, without showing you the call, so whatever the session
chooses to record leaves for the server unseen. What it chooses is shaped by the stored text the
hook adds at session start, which other writers in the workarea can influence. Every write is
still held until a different identity reviews it, so nothing it records is shared before then.

## Other MCP clients

*Available since v0.1.0; the agent kit since v0.3.1 · [releases](https://e8kor.github.io/open-memory-lab/product/releases/)*

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

## What is served today

*Eight tools since v0.2.0; three task-shaped tools planned for v0.4.0 · [releases](https://e8kor.github.io/open-memory-lab/product/releases/)*

The catalog in [docs/api/mcp-tools.json](../../docs/api/mcp-tools.json) declares twelve tools.
This release serves the eight above. The other four are not served yet: `oml_attach`,
`oml_relate_propose`, `oml_graph` and `oml_vocab` answer with an error saying they are not
built yet. They arrive with the milestones in the
[critical path](../../docs/product/critical-path.md).
