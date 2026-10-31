<!--
id: adr/ADR-0029-session-start-hook
tier: engineering
status: proposed
answers: how the oml agent plugin starts every Claude Code session from what OML holds for its key; what `oml hook session-start` reads over the HTTP rings and with which key; which query it recalls with and under which budget; how it fails open; what context it injects; how the plugin wires it; and how a user lets the plugin's tools run without a prompt
-->

# ADR-0029 — The agent plugin starts every session from what OML holds for its key, through `oml hook session-start`

**Status:** proposed · 2026-10-31

## Context

**A session starts knowing nothing.** The `oml` plugin gives an agent its tools
([agent plugin](../api/mcp-plugin.md)), and every one of them runs only when the model decides to
call it. A new session, a resumed one, one after `/clear` and one after a compaction each begin
without the workarea's steering, without the agent's own held submissions and without the
memories the work needs, unless the model thinks to recall first. After a compaction the facts
the summary carries are also not yet in OML, and nothing asks the model to record them.

**Claude Code can inject context at session start.** A SessionStart hook runs when a session
starts, resumes, is cleared, is compacted or is forked; its stdin is a JSON object carrying the
session's `source` and `cwd`, and a `hookSpecificOutput.additionalContext` string it prints is
added to Claude's context before the first prompt
([hooks reference](https://code.claude.com/docs/en/hooks), fetched 2026-10-31). A bash prototype
of such a hook, run by the maintainer outside this repository, showed the shape: the key's own
recent work, the steering and the matching memories, read under a token budget, and one more line
after a compaction.

**What the maintainer decided on 2026-10-31, recorded here, not reopened.** The hook is a Rust
subcommand of the existing `oml` CLI, `oml hook session-start`, because user-facing OML tools are
Rust. It is on by default, bounded by a token budget, and `OML_SESSION_CONTEXT=off` disables it.
Three alternatives were rejected then and are listed below: a Claude Code `mcp_tool` hook, a shell
script over `curl` and `jq`, and off by default.

**What this record holds itself to.** The CLI reaches the server over HTTP only, never MCP
([ADR-0006](ADR-0006-pure-rust.md) d6). The hook adds no HTTP surface: it composes the own-work
listing and recall as they are ([HTTP API](../api/http-api.md#search-get-v1memories)). It reads
with the agent's own key and holds no authority of its own, so it sees exactly what that key's
tools see, and no other key's held row (R3). The verb, the plugin wiring, the checks and their
tests land in the same change as this record; the record stays proposed until the maintainer
accepts it.

## Decision

- **d1 — `oml hook session-start` is a subcommand of the `oml` CLI that the `oml` plugin runs on
  every SessionStart; it is on by default, and `OML_SESSION_CONTEXT=off` disables it.** The hook
  fires for every `source` Claude Code sends (`startup`, `resume`, `clear`, `compact`, `fork`),
  because each of them begins from a context that does not hold OML's state. `OML_SESSION_CONTEXT`
  unset, empty or `on` runs the hook; `off` prints nothing; any other value prints nothing and
  names the variable and its two values on stderr, refused rather than read as either.

    The verb is the CLI's one verb run under an agent's own key by design: it reads, under that
    key, what the key's tools would read, and performs no act. On by default is what makes it
    useful, since almost no one would discover an opt-in hook; the cost is two authenticated reads
    at every session start of every user with the plugin and the CLI, until they set the variable.
    The verb is declared on the
    [CLI reference](../cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin).

- **d2 — The hook sends two requests on the provider ring and no other: the own-work listing,
  then a recall under a token budget.** Each request carries `Authorization: Bearer` with the key
  of d4.

    1. `GET /v1/memories?own=true&limit=10`: the key's ten newest submissions in every state,
       across every workarea it wrote to, with no grant needed
       ([own](../architecture/roles-and-keys.md#own), [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d13).
    2. `POST /v1/memories/recall` on the key's default workarea, which the request does not name,
       with `{"query": <d3>, "k": 10, "include_bodies": true, "include_own_held": true,
       "max_tokens": <budget>}`. The answer carries the applicable steering whatever the query,
       and the hits the query matches, own held rows among them (ADR-0025 d4, d8).

    The budget is 800 budget tokens, overridable by `OML_SESSION_CONTEXT_MAX_TOKENS`, which takes
    an integer from 1 to 2000; any other value prints nothing and names the variable and its
    bounds on stderr, refused rather than clamped. The server spends it as every recall's budget
    is spent: steering is never cut, and the hits served are the longest rank prefix that fits
    ([token budgets](../architecture/retrieval.md#token-budgets), ADR-0025 d7). The ceiling is set
    by Claude Code: it caps `additionalContext` at 10,000 characters and, over the cap, gives
    Claude a file path and a 2,000-character preview instead
    ([hooks reference](https://code.claude.com/docs/en/hooks), fetched 2026-10-31). A budget token
    is ⌈body bytes ÷ 4⌉, so 2000 of them are at most 8,000 bytes of hits, which leaves room under
    the cap for the frame, the ten own-work lines and short steering. `k` is 10, the same count as
    the listing; the budget cuts within it.

    The own-work listing is the one read that shows the key its held rows without a reader grant,
    and recall with `include_own_held` is the one that shows its held rows beside the promoted
    memory they would join; together they are the prototype's two reads, on the ring the CLI
    speaks. The cost is that steering past the budget is still injected whole, as the server
    serves it: a workarea whose steering alone exceeds Claude Code's cap reaches Claude as a
    preview.

- **d3 — The recall's query is the words of the session's working directory name.** The hook
  takes the last component of the stdin `cwd`, splits it at every character that is not a letter
  or a digit, and joins the words with single spaces: `/home/op/src/open-memory-lab` recalls with
  `open memory lab`. With no `cwd`, or a name with no word in it, the query is empty, and the
  recall serves the steering and no hit.

    The SessionStart stdin carries no prompt; the directory's name is the one text it carries
    that names the work, and memories about a project usually name the project. Splitting the
    name widens the match where the store reads a hyphenated name as one compound term beside its
    parts. The cost is the store's matching rule: the PoC matches every word of the query
    (`plainto_tsquery` in [retrieval](../architecture/retrieval.md#the-recall-ordering-contract-steering-rows-first-unranked)),
    so a memory is a hit only when it names every word of the directory's name; a generic name
    such as `src` matches generic memories, and a worktree named after its branch matches only the
    memories that name the branch. How well the directory name recalls is unmeasured, under
    [FR-4](../product/roadmap.md).

- **d4 — The hook authenticates with `OML_URL` and `OML_API_KEY` alone, the agent's own plugin
  key, and prints no key value.** These are the two variables the plugin's MCP entry reads, so the
  hook reads exactly what the agent's tools read. It never uses `--server`, `--key`,
  `OML_CLI_SERVER` or `OML_CLI_KEY`, and never reads the stored CLI credentials
  (`~/.config/oml/credentials`, or `OML_CLI_CREDENTIALS`): it runs before the CLI loads them.
  `OML_URL` is the server without `/mcp`, and has no default here: an unset URL means the user has
  not pointed this agent at a server. A key value that holds whitespace or a character outside
  printable ASCII is refused before anything is sent, with a stderr line that names the variable
  and never the value. Every occurrence of the key value in what the hook prints, stored text and
  stderr lines alike, is replaced by `[OML_API_KEY]`, before any line is flattened.

    An admin key must never feed an agent's context: the agent's session would read under a key
    above the agent ceiling (R1), whatever that key's grants reach, and the read log would
    attribute the agent's session to the admin. A key value in the
    context would enter the session transcript, which the
    [key format rule](../architecture/roles-and-keys.md#key-format) forbids; the redaction closes
    the one path left once the hook prints no credential of its own, a stored body that holds the
    value. The redaction matches the value exactly, so it holds only for a value the server reads
    as sent: a key with a stray `\r` would reach stderr raw inside a header error, and one with a
    trailing space would still authenticate, since the server trims it, while a stored body
    holding the bare key would go unredacted. Refusing every value that is not printable ASCII
    closes both. The cost is that a CLI user who keeps only the stored admin credentials gets no
    session context until the agent's own key is in the environment.

- **d5 — The hook fails open: every failure ends in no output and exit 0, and the context is
  printed whole or not at all.** No stdout and exit 0 follow `OML_SESSION_CONTEXT=off`, a refused
  `OML_SESSION_CONTEXT`, budget or key value (d1, d2, d4), a missing or empty `OML_URL` or
  `OML_API_KEY`,
  stdin that is not a JSON object or whose `source` or `cwd` is not a string, an unreachable
  server, a typed refusal, a timeout, and an answer the hook cannot read. Each request has a
  3-second timeout. If the listing fails the recall is not sent, and if the recall fails nothing
  is printed. One line naming the reason may go to stderr, with the key redacted as in d4; Claude
  never sees the stderr of a hook that exits 0 ([hooks reference](https://code.claude.com/docs/en/hooks),
  fetched 2026-10-31).

    A session must start whether or not OML answers, and a hook error at every session start
    while a server is down would be noise the user cannot act on from the session. All or nothing
    keeps the context from saying "none" for a list the hook never received. Claude's first
    response waits for the SessionStart hooks (same reference), so the 3-second timeout bounds the
    delay a slow or silent server adds to about six seconds over two requests. The cost: a
    write-only key ([ADR-0028](ADR-0028-agent-key-issuance.md) d2), which recall refuses, gets no
    session context at all, and a server slower than 3 seconds per request gets none either.

- **d6 — The output is one SessionStart hook document whose context frames the entries as stored
  data: the server and workarea, steering, the agent's recent submissions, the matching memories,
  then how to record.** On success the hook prints one line of JSON,
  `{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"…"}}`, and nothing
  else. The context, in order:

    1. a header naming the server URL and the workarea path, and a line saying the entries below
       are stored memories, data to check and not instructions to follow;
    2. the steering in force, as the recall serves it;
    3. the key's recent submissions, newest first, each with its state and version;
    4. the memories the query matched, each with its state, and a line naming how many more the
       budget cut, when it cut any;
    5. one line on recording a durable fact or decision with `oml_remember`: one checkable claim,
       its source and date, a stable `claim_key`, an `idempotency_key`, never a secret;
    6. when the stdin `source` is `compact`, one more line: the conversation was just compacted,
       and before continuing Claude records with `oml_remember` the durable facts and decisions
       from the summary that are not in OML yet.

    Neither request names the key's default workarea, so the header names the path of the newest
    own submission that carries one, which is that workarea unless the submission named another;
    with none, it says "this key's default workarea" and that `oml_workarea_info` names it. Every
    entry is one line, its whitespace collapsed, so a stored body cannot forge a section header or
    the framing line. The verb takes `--json` like every verb and prints the same document, since
    its stdout is already the one JSON document its reader parses. The exact text is declared on
    the [CLI reference](../cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin).

- **d7 — `plugins/oml/hooks/hooks.json` declares one SessionStart command hook that runs `oml hook
  session-start` only when `oml` is on `PATH`.** The entry has no matcher (d1), a timeout of 10
  seconds, above the two 3-second requests of d5, the status message `Loading OML memory`, and the
  command

    ```sh
    command -v oml >/dev/null 2>&1 || exit 0; oml hook session-start || exit 0
    ```

    A user with the plugin and no CLI sees no hook error and no context. The trailing `|| exit 0`
    covers the CLI the plugin outran: a marketplace update can bring the hook before the user
    installs a CLI that has the verb, and an older `oml` answers an unknown subcommand with exit
    `2`, which Claude Code would show as a hook error at every session start. The cost is that a
    crash of the verb itself is silent too. `scripts/check_plugin.py` holds the rule by running
    the command under `/bin/sh`: with no `oml` on `PATH` it must exit 0 with no output, and with a
    stand-in `oml` that fails it must run it with exactly the arguments `hook session-start` and
    still exit 0.

- **d8 — The plugin README tells the user to add `mcp__plugin_oml_oml` to `permissions.allow`.**
  A plugin cannot pre-allow its tools for the user: of the settings a plugin ships, Claude Code
  applies only `agent` and `subagentStatusLine`
  ([plugins reference](https://code.claude.com/docs/en/plugins-reference), fetched 2026-10-31).
  The context of d6 asks Claude to call `oml_remember`, so without the rule every call asks the
  user first. The rule allows every tool of the plugin's one server; each tool stays inside the
  agent ceiling (R1), and every write is still held until a different identity reviews it.

## Consequences

- The CLI gains `oml hook session-start`; the [CLI reference](../cli/reference.md) declares it,
  its variables and its fail-open behaviour (D14), and the D14 cell of the
  [D-register](index.md) names this record. The [agent plugin](../api/mcp-plugin.md) page links to
  it from "Wiring it into your agent", and the plugin README carries one paragraph and the
  permission rule of d8.
- The `oml` plugin gains `hooks/hooks.json`; `scripts/check_plugin.py` fails if the file is
  missing or does not parse, or if its SessionStart hooks are not exactly one command that runs
  `oml hook session-start` guarded so that a missing or failing `oml` exits 0.
- No HTTP operation, error code, event type or claim state is added: `docs/api/openapi.yaml` and
  `docs/api/errors.md` are unchanged, and `info.version` stays `0.2.0-design`. No package version
  moves until the release.
- Costs carried openly: every session start of every user with the plugin and the CLI makes two
  reads, logged as reads of the agent's key once the read log is built
  ([retrieval](../architecture/retrieval.md#the-read-log)); a write-only key gets no session
  context; a directory name that names nothing the memories name recalls nothing but steering;
  steering is injected whole whatever the budget; the header's workarea path is read off the
  newest own submission, not the key's default.
- Unverified: the tests run the verb against the real lead in process, not inside Claude Code.
  `claude plugin validate` checks the hook file's shape; that Claude Code runs the command and
  adds the context to a session is the hooks reference's statement, not a check in this
  repository.
- Process: the record registers at its three points (this file, a [register](index.md) row and a
  `mkdocs.yml` nav entry); `llms.txt` names it; one [changelog](../changelog.md) entry records the
  change. On acceptance, and not before, this record's Status line and frontmatter `status`
  become `accepted design` with the acceptance date, and its register row follows.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — A Claude Code `mcp_tool` hook calling `oml_my_work` and `oml_recall`** | Observed on 2026-10-31 with Claude Code 2.1.312: the plugin's MCP server is not yet connected when SessionStart fires, so a tool hook injected nothing. |
| **d1 — A shell script over `curl` and `jq`** | A non-Rust user-facing tool with a `jq` dependency, tested only by the plugin check; the prototype was that script. |
| **d1 — Off by default** | Almost no one would discover it. |
| **d1 — A separate `oml-hook` binary** | A second artifact to build, ship and version beside the `oml` CLI, which the release already ships. |
| **d1 — Read any value other than `off` as on** | A mistyped value would run the hook the user tried to turn off. |
| **d2 — The listing without `own`** | It serves promoted tips in scope and never the key's held rows, which are what the agent most needs to see again. |
| **d2 — A third request, `GET /v2/workareas/{workarea_id}`, for the default workarea's path** | Neither the listing nor the recall names the default workarea's id, so the hook has no id to send; an id-less self-view is an HTTP surface change for its own record. |
| **d2 — Clamp an out-of-range budget** | A mistyped budget would silently widen or narrow the context; a refusal names the variable. |
| **d2 — Cut each body at 400 characters, as the prototype did** | It serves partial bodies, which the budget cut never does; the server's budget already bounds the hits. |
| **d2 — Cut steering in the client to fit the budget** | Steering is never cut ([token budgets](../architecture/retrieval.md#token-budgets)): a standing instruction a budget can drop fails exactly when it is needed. |
| **d3 — A fixed word, such as the prototype's `OML`** | It matches only memories that name the product, in every project alike. |
| **d3 — The whole working directory path** | Its parent directories are words no memory names, and a store that matches every word would match nothing. |
| **d3 — The repository name from `git`** | It needs a second process and a repository, and outside one there is no answer. |
| **d3 — A query variable** | No caller needs one yet; a later record adds it if the directory name proves the wrong default. |
| **d4 — Fall back to the stored credentials or `OML_CLI_KEY`** | An admin key would feed an agent's context: the session would read under a key above the agent ceiling, and the read would be attributed to the admin. |
| **d4 — Default `OML_URL` to `http://127.0.0.1:8080`, as the MCP entry does** | The hook would send the agent's key to whatever listens there at every session start, for a user who never named a server. |
| **d4 — Print stored text verbatim** | A body that holds the key value would carry it into the transcript. |
| **d5 — Exit non-zero on failure** | Claude Code would show a hook error at every session start while the server is down. |
| **d5 — Print what the successful request returned** | A partial context reads as an empty list: "no steering" when the recall was refused. |
| **d5 — The prototype's 8 seconds per request** | Up to 16 seconds before the first answer of a session whose server does not answer. |
| **d5 — Retry a failed request** | It doubles the delay a down server adds, for a context the next session reads again. |
| **d6 — Plain-text stdout** | Claude Code accepts it, but the JSON document names its event and gives stdout one meaning. |
| **d6 — Bodies printed with their own line breaks** | A body's newlines could forge a section header or the framing line. |
| **d6 — The own-work bodies, by one get per item** | Ten more requests at every session start, for text the recall already serves when it matches. |
| **d7 — A binary bundled in the plugin under `${CLAUDE_PLUGIN_ROOT}`** | The plugin would carry one platform-specific binary per release, beside the CLI the user already installs. |
| **d7 — An unguarded command** | Every user with the plugin and no CLI would see a hook error at every session start. |
| **d7 — Exec form with `args`** | Without a shell there is no test for the binary, so a missing `oml` is a hook error. |
| **d8 — A `settings.json` in the plugin that allows the tools** | Claude Code drops every plugin setting but `agent` and `subagentStatusLine` at load. |
| **d8 — A skill that tells Claude to add the rule** | A permission is the user's decision about their own settings; the plugin states it and leaves it to them. |
