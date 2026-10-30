<!--
id: adr/ADR-0028-agent-key-issuance
tier: engineering
status: proposed
answers: how the CLI issues the one key the agent plugin needs in one call and prints the lines the plugin reads, how --out keeps the key value off the terminal, why the issuing verbs add a reader grant beside a writer grant unless --write-only, and how the oml-admin plugin runs the issuance without the value entering a transcript
-->

# ADR-0028 — The CLI issues an agent's key in one call with its reader grant by default, and an admin skill runs it without the key value entering a transcript

**Status:** proposed · 2026-10-30

**Amends:** ADR-0015 (d1)

## Context

**The plugin's key takes three steps and one easy mistake.** The `oml` agent plugin reads two
environment variables, `OML_URL` and `OML_API_KEY` ([agent plugin](../api/mcp-plugin.md)). One
writer-role key that holds a writer grant and a reader grant on one workarea both remembers and
recalls ([grant admissibility](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order),
[ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d1). Today an operator issues it with
`oml keys issue`, which sends exactly the grants it is told, then copies the value out of a
printed block into two `export` lines by hand. An issuer who names only the writer grant gets a
key that fails at its first recall, with `access.not_found`, and nothing earlier says why.

**An operator working in Claude Code has no safe way to run it.** A skill that runs `oml keys
issue` puts the printed value into the session transcript and the model's context, places the
show-once ceremony ([ADR-0015](ADR-0015-cli-administration.md) d1) and the
[key format rule](../architecture/roles-and-keys.md#key-format) forbid.

**Drafted with names, shipped before them.** These decisions were drafted as d12–d14 of the
proposed ADR-0026 (names are keys), where `issue-agent` takes the key's name as its positional
argument. The maintainer decided to ship the key tool in v0.2.1, before names exist. This record
carries those decisions as d1–d3, adapted to the server as it is now: a workarea is addressed by
its node id, a key by its id and its principal, and the CLI's context is the server and key it
resolves from `--server`, `OML_CLI_SERVER` or the stored credentials
([CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation)). ADR-0026 d1 adds the
positional key name to `issue-agent` when it lands; nothing below depends on it.

**What this record holds itself to.** It adds no HTTP surface: `POST /v2/keys` takes exactly the
grants it is sent (ADR-0025 d2), and every refusal stays the issuance's own. It puts no admin verb
on MCP: the facade serves the agent-safe subset and nothing above it
([ADR-0014](ADR-0014-http-api-and-mcp-facade.md) d2, R1). The verb, the default, the plugin and
their tests land in the same change as this record; the record stays proposed until the
maintainer accepts it.

## Decision

- **d1 — `oml keys issue-agent --principal <principal> --workarea <workarea-id>` issues the
  plugin's one key in one `POST /v2/keys` and prints the two export lines the `oml` plugin
  reads.** The verb sends a writer-role issuance with the given principal, `held_by` from
  `--held-by agent|human` (default `agent`), a writer grant on the workarea and its reader grant
  (d2), and `default_workarea_id` set to that workarea, so the key's default never rests on the
  order of its grants. It sends no other request, before or after. On success it prints exactly
  two lines on stdout:

    ```console
    export OML_URL='https://oml.example.com'
    export OML_API_KEY='oml_…'
    ```

    `OML_URL` is the server the CLI resolved, without `/mcp`; `OML_API_KEY` is the value, printed
    this once (ADR-0015 d1). Both are single-quoted for a POSIX shell. Every other line goes to
    stderr, and `--json` prints the `201` body instead, as every verb does.

    **`--out <path>` writes the value to a file instead of printing it.** Before it sends anything
    the CLI creates the path exclusively, with mode 0600; a path that already exists is refused
    and nothing is sent, so no file is ever overwritten. After the `201` it writes the value and
    one newline to that file and prints, with the path made absolute:

    ```console
    export OML_URL='https://oml.example.com'
    export OML_API_KEY="$(cat '/home/op/.config/oml/keys/crew-agent-7')"
    ```

    No key value reaches stdout, stderr or the terminal. A refused issuance removes the empty file
    the CLI created. If the value cannot be written once the key exists, the value is lost: the
    CLI removes the partial file, names the key's id on stderr, says to revoke it, and exits `2`.
    A path containing a quote or a newline is refused before anything is sent, so the printed
    line is always safe to run, and `--out` with `--json` is refused, because the `201` body
    carries the value. This amends ADR-0015 d1: an issued value is shown exactly once, or written
    exactly once to the file the operator named, and the
    [key format rule](../architecture/roles-and-keys.md#key-format) names that file as the one
    place a value may rest outside the show-once response.

    The verb composes an existing call, so every rule of the issuance order applies unchanged, and
    its acceptance test is that the printed lines, exported, let one key remember and recall on
    that workarea. The principal is required: it is the one readable thing a key carries until
    names exist, and review independence and the reciprocal-approval statistics key on it, so
    every plugin key should have its own. The positional key name of ADR-0026 d12 arrives with
    ADR-0026 d1. The cost is a verb that must stay in step with `POST /v2/keys`, and with
    `--out`, a second place the value rests: owner-only, and the operator's to delete. The rule
    is declared on the [CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation).

- **d2 — `oml keys issue` and `oml keys issue-agent` add a reader grant on the same workarea
  beside every writer grant unless `--write-only` is passed.** For each writer grant the request
  asks for, the CLI sends a reader grant on the same workarea, unless the request already names
  that reader grant or `--write-only` is given. The default is the CLI's alone: HTTP issuance is
  unchanged, the server takes exactly the grants it is sent (ADR-0025 d2), and the added grant is
  an ordinary grant in the request body, journalled as its own `grant.issued` and echoed in the
  `201` like any other. A write-only key still reads its own work, in every state, with no grant
  at all ([own](../architecture/roles-and-keys.md#own), ADR-0025 d3).

    The common key, one agent that remembers and recalls, needs both grants, and the mistake the
    default prevents stays silent until the agent's first recall. Keeping it a CLI default keeps
    the authority explicit: the grant exists because a request named it, the journal shows it,
    and `--write-only` is the visible choice for a crew whose writers must not read each other's
    promoted work. The cost is that a writer key issued by the CLI without the flag now reads its
    workarea's promoted memory, which every existing caller of `oml keys issue` gets on its next
    run, the repository's quickstart and demo scripts included. The flag and the default are
    declared on the [CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation).

- **d3 — An `oml-admin` Claude Code plugin holds one skill that runs `issue-agent` with `--out`,
  serves no MCP tool, and never puts a key value in the model's context.** The plugin is listed
  beside `oml` in the repository's marketplace and holds one skill, `issue-agent-key`, and no MCP
  server entry. The skill runs `oml keys issue-agent --principal <principal> --workarea
  <workarea-id> --out <path>` itself, always with `--out`, under
  `~/.config/oml/keys/<principal>` with each `/` of the principal written as `-`, or a path the
  operator names outside any repository, and never with `--json`. It hands the operator only the
  two printed lines, which carry a path and no value, to run in the shell that will start the
  agent; it never reads the key file, never runs those lines itself and never runs another verb
  whose output carries a key value; and it tells the operator to delete the file once the key is
  in the environment that needs it.

    A value in a command's output, or in the skill's own shell, enters the session transcript or
    the model's context, which ADR-0015 d1 and the key format rule forbid; `--out` is what lets a
    skill run the ceremony at all. The agent-facing `oml` plugin still cannot issue a key: its
    tools are the agent-safe subset (ADR-0014 d2). The cost is that the file is readable by the
    operator's own user, the user the skill runs as, so "never reads the key file" is the skill's
    instruction and the repository's check on it, not something the operating system enforces.
    The plugin is declared on the [CLI reference](../cli/reference.md#the-oml-admin-plugin).

## Consequences

- The CLI gains `oml keys issue-agent` and `--write-only` on both issuing verbs; the CLI reference
  declares them (D14), and [roles and keys](../architecture/roles-and-keys.md#key-format) names
  the `--out` file in the key format rule. No HTTP operation, error code, event type or claim
  state is added, and `docs/api/openapi.yaml` is unchanged.
- The repository's marketplace lists a second plugin, `oml-admin`, versioned with the Cargo
  workspace like `oml`; `scripts/check_plugin.py` fails if the marketplace stops listing it with a
  manifest, if it gains an `.mcp.json`, or if its skill runs `issue-agent` without `--out` or with
  `--json`.
- The skill checks no context role before it runs: the CLI has no `oml context show` yet, and a
  key that may not issue is refused by the issuance itself. Key revocation is not built, so the
  write-failure message also names `oml deny set <key-id> <root workarea id>`, a journalled global
  denial that keeps the key from every workarea, as the stop until it is.
- What arrives later, with ADR-0026: the positional key name, `--crew`, and the skill's checks by
  `oml context show` and `oml keys show`. `--expires` and `--label` pass-through wait on key
  expiry and on the CLI verbs that would show a label; neither is decided here.
- Process: the record registers at its three points (this file, a [register](index.md) row and a
  `mkdocs.yml` nav entry); `llms.txt` names it; the D14 cell of the D-register names ADR-0028;
  one [changelog](../changelog.md) entry records the change. On acceptance, and not before: this
  record's Status line and frontmatter `status` become `accepted design` with the acceptance date,
  its register row follows, and ADR-0015's Status line gains "· amended by ADR-0028". ADR-0026's
  d12–d14 point here.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — Wait for names and ship the verb with ADR-0026** | The plugin key is the first key every operator issues, and the maintainer chose to ship it in v0.2.1; the positional name ADR-0026 adds is additive. |
| **d1 — The principal as the positional argument** | The positional slot belongs to the key name ADR-0026 d1 adds; an argument whose meaning changed when names land would break every script that used it. |
| **d1 — Leave `default_workarea_id` out and let the server take the first grant's node** | The default would hold only while the writer grant comes first; naming it states what the verb means. |
| **d1 — A new HTTP operation for agent keys** | `POST /v2/keys` already carries every field the verb needs; a second issuance path would need its own validation order and refusals. |
| **d1 — Print the value alone and let the operator write the exports** | The two lines are what the plugin reads; printing them removes a step where the value gets pasted somewhere else. |
| **d1 — Let `--out` overwrite an existing file** | A mistyped path would destroy an unrelated file, and a second issuance to one path would destroy the only copy of an earlier key's value. |
| **d1 — Create the `--out` file after the issuance succeeds** | A path that cannot be created would surface only after the key exists, with its value already lost. |
| **d2 — The server implies read from writer** | It contradicts roles-and-keys.md ("not a writer key with implied read") and ADR-0025's rejected "writer implying read", and the authority would never appear in the journal. |
| **d2 — The HTTP issuance adds the reader grant** | It would amend ADR-0025 d2, under which the server takes exactly the grants it is sent. |
| **d2 — A tenant-wide default read** | It voids per-workarea access. |
| **d2 — No default** | Every issuer of the common key must remember the second grant, and the key that forgets it fails only at its first recall. |
| **d3 — An admin MCP server or an `oml_issue_key` tool** | An admin verb on MCP breaks ADR-0014 d2 and R1: the facade serves the agent ceiling and nothing above it. |
| **d3 — The skill runs `issue-agent` and relays the export lines** | The key value would sit in the transcript and the model's context, which ADR-0015 d1 and the key format rule forbid. |
| **d3 — Hand the command to the operator to run in a terminal of their own** | It defeats the skill's purpose, running the ceremony for the operator, and moves the one error-prone step back to the operator. |
| **d3 — Put the skill in the `oml` plugin** | The agent plugin would carry an admin ceremony into every agent that installs it. |
