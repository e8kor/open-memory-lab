---
status: Stable
verified: 2026-11-06
answers: "how an administrator runs an OML server with the oml CLI: the first hour (claim the server, create a workarea, issue an agent key and a reviewer key), where admin credentials live, workareas, crews, keys by role, denials, ending a connection, reading the journal, and the rules no admin key bypasses"
---

# Administer an OML server

Administration is the `oml` CLI. Every verb is one request to the server's [HTTP API](../api/http-api.md),
the server journals it, and every refusal names its rule. The [CLI reference](../cli/reference.md) holds
each verb's full contract. This page gives the order and the tasks.

## The first hour

You need a server running with nothing claimed, the `oml` CLI ([install](../start/install.md)), and a
server that only you can reach until step 2 is done. `oml quickstart` already did this once for a workarea
named `crew`.

1. **Read the enrollment token.** While no tenant exists, the server prints a single-use token on
   stdout at every start. It is `enr_` and 48 hex digits. A restart prints a new one and voids the old one.
   On Kubernetes, read it from the running pod's log ([Install OML on Kubernetes](kubernetes.md)).
2. **Claim the server** before any proxy or Ingress exposes it. Whoever presents the token first
   receives the first admin key and the tenant ([deploy](../operations/deployment.md#claim-the-first-admin-key)).

   ```sh
   printf '%s\n' "$TOKEN" | oml --url https://oml.example.com bootstrap claim ops-admin \
       --token-stdin --tenant-name acme --principal platform/you
   ```

3. **Check it.** `oml doctor` prints each health signal with a verdict. It exits `1` on any failure.
4. **Create a crew and a workarea for it.**

   ```sh
   oml crews create metals-crew --charter-uri https://wiki.example.com/metals
   oml workareas create universe metals --crew metals-crew
   ```

5. **Issue the agent's key into a file**, never onto the screen:

   ```sh
   mkdir -p -m 700 ~/.config/oml/keys
   oml keys issue-agent claude-laptop --principal crew/agent-7 --workarea metals \
       --crew metals-crew --out ~/.config/oml/keys/claude-laptop
   ```

   Run the last line it prints, `oml setup claude-code --url ... --key-file ...`, where Claude Code
   runs ([Claude Code](claude-code.md)). Then delete the file.
6. **Issue a reviewer key (role `dreamer`)** under a different principal, because the reviewer is never
   the author:

   ```sh
   oml keys issue maya --role dreamer --held-by human --principal crew/maya \
       --default-workarea metals --grant metals:dreamer --grant metals:reader
   ```

7. **Look at what you built.** Run `oml workareas tree` and `oml keys show claude-laptop`.

The agent's writes now wait as `held` until `maya` approves them with `oml review next` and
`oml review approve` ([reviewing](reviewing.md)).

## Where the admin credentials live

The claim writes `url` and `api_key`, and nothing else, to `~/.config/oml/credentials`. The file is
owner-only (mode 0600 on Unix). The file `OML_CLI_CREDENTIALS` names replaces it, so a second server
is a second file. `oml quickstart` writes `oml/credentials` in its own directory instead.

The CLI takes its pair from the first source that sets either value: `--url` and `--api-key`, then
`OML_URL` and `OML_API_KEY`, then the file. A source that sets one alone is refused
([start settings](../cli/reference.md)).

A shell that exports an agent's pair therefore wins over the stored admin pair. Run admin verbs with both
variables unset. `oml login <url>` replaces the stored pair after `GET /v2/whoami` accepts the key.

Never put an admin key into an agent. `oml setup claude-code` refuses an admin or operator key. To write
or recall yourself, issue yourself a writer key.

## Tenants

The claim creates the one tenant a deployment hosts today, with its root workarea `universe`.
`oml tenants create` exists and refuses: creating further tenants is not available
([CLI reference](../cli/reference.md)).

## Workareas and names

A tenant's workareas form one tree under `universe`. A grant on a workarea covers its subtree.

| Task | Command |
|---|---|
| Create a child | `oml workareas create <parent> <name> [--crew <crew>]` |
| See the tree | `oml workareas tree [--root <workarea>]` |
| See one node | `oml workareas show <workarea>` |
| Point a workarea at another crew | `oml workareas crew set <workarea> --crew <crew>` |

A new workarea starts in `development`. It climbs to `committed` and `validated` with `oml workareas status`.
One key proposes each step and a different key ratifies it.

An admin key reads the structure of every workarea in its tenant, and never a memory in it. The
[admin read](../api/http-api.md) adds two routes on which it reads every memory's text and attribution.
The server journals each of those reads.

Workareas, crews, keys and tenants each carry a **name**. A name is unique in the tenant, given at
creation, never changed and never released ([names](../concepts.md)). Every verb accepts a name where it
accepts an id. No verb renames anything, and a retired key's name stays taken. Choose names you can
live with.

## Crews

A crew is a record that OML stores and does not operate: a name, a charter URI and an external manager.
A workarea names at most one crew. A key names one at issuance, fixed for its life.

| Task | Command |
|---|---|
| Create | `oml crews create <name> [--charter-uri <uri>] [--external-manager <text>]` |
| Change the charter or manager | `oml crews update <crew> --charter-uri <uri>` or `--external-manager <text>` |
| List | `oml crews list` |
| Show one | `oml crews show <crew>` |

## Issue keys by role

A key's role fixes which grants it may carry. A `writer` takes `reader` and `writer` grants. A `dreamer`
takes `reader` and `dreamer`. A `reader` takes `reader`. An `admin` takes none
([roles and keys](../architecture/roles-and-keys.md)). An admin issues keys up to its own role, other
admin keys included. The server shows each value once, and a lost value is not recoverable.

**An agent.** `oml keys issue-agent <key-name> --principal <principal> --workarea <workarea>
[--held-by agent|human] [--crew <crew>] [--write-only] [--out <path>] [--connect-code]` issues a
writer key. It carries a writer grant on the workarea, a reader grant beside it unless you pass
`--write-only`, and the workarea as its default.

Give each agent its own principal, because review independence keys on it. Without `--out`, the verb
prints `export OML_URL=...` and `export OML_API_KEY=...`. With `--out`, the CLI creates the file
owner-only before sending anything, refuses a path that exists, and writes the value there. It prints
four lines that read the file, none carrying the value.

The [`oml-admin` plugin](../cli/reference.md) runs the same verb from Claude Code with `--out`, so the
value never enters the conversation.

**A claude.ai or phone connection.** Add `--connect-code`. The key gets no value, and the verb prints
the connector URL and a single-use code. It needs `oauth.issuer`. Without it, the issuance is refused as
`oauth.connect_refused`. `oml keys connect-code <key>` mints a fresh code and voids the earlier ones. The
whole flow is [claude.ai and mobile](claude-ai-and-mobile.md).

**A reviewer.** `oml keys issue <key-name> --role dreamer --grant <workarea>:dreamer
[--held-by human] [--principal <text>] [--default-workarea <workarea>] [--crew <crew>]`. Use a principal
that no writer in that workarea uses. Add `--grant <workarea>:reader` too, or the reviewer sees a held
memory without its sources ([reviewing memories](reviewing.md)).

**Another admin.** `oml keys issue <key-name> --role admin --held-by human --principal <text>`, with no
`--grant`. An admin key with a dreamer grant is refused as `access.role_exclusive`.

**A reader or service.** `oml keys issue <key-name> --role reader --held-by service --grant <workarea>:reader`.

`oml keys issue` also takes `--label <text>`. Keys do not expire. An issued key lives until you deny it.

## Deny a key

A key's grants cannot change after issue. There is no verb to add, remove or list grants. To change
what a key reaches, issue a new key under a new name.

What you can change on a live key is a **denial**:

```sh
oml deny set <key> <workarea> [--reason <text>]
```

The server journals `key.denied`. The denial wins over every grant the key holds on that subtree,
at the key's next request. A denial on an admin key is refused, because an admin key holds no grant to
attenuate. A denial is permanent. To restore access, issue a new key under a new name.

## Cut a key off, replace it, or end a connection

OML has no revoke, rotate or list verb for keys. Deny the key with `oml deny set <key> universe` and issue a
new one.

| Task | Command |
|---|---|
| Cut a key off | `oml deny set <key> universe` keeps it from every workarea. |
| Rotate a key | Issue a successor under a new name, move the holder to it, then deny the old key `universe`. |
| Read one key | `oml keys show <key>` by id or name. It shows grants, denials, crew and epoch, never the value. |
| End one connection | `oml keys show <key>` lists a connector key's connections. `oml keys disconnect <key> <connection-id> [--reason <text>]` ends one. |
| End every connection of a key | `oml deny set <key> universe`. A connection acts as its key alone. |

You cannot cut off an admin key today. Keep the admin credentials file on one machine, and issue a
second admin key only to a person who needs one.

If an issuance returns nothing usable after reaching the server, the CLI cannot tell whether the key
exists. Run `oml keys show <key-name>` to find out. Deny a key whose value you lost and replace it
under a new name.

## Read the journal

`oml events [--after-seq <seq>] [--workarea <workarea>] [--limit <n>]` prints one page of
`GET /v2/events`. The events are digest-only and within the key's reader access. The page carries
`next_after_seq` to pass back. Run it with a reader key, because it needs reader access on the workarea.

Key, grant, crew, connection and review events appear on no key's tail. `oml audit verify-chain` exists
and is refused while the server serves no chain export.

## Rules no admin key bypasses

- **The reviewer is never the author.** An admin verdict is refused. An admin key never carries a
  dreamer grant. A verdict from any key in a memory's lineage is refused as `review.author_in_lineage`.
  A person who administers and reviews holds two keys.
- **An admin cannot promote a memory.** There is no `oml promote`. Only a verdict from a different
  reviewer approves one.
- **OML does not delete memories.** There is no `oml delete`. Rejected and retired memories keep their
  bytes and stay readable as history.
- **Names are permanent.** No name changes, and no name is reused.
- **Tenants are separate.** No verb reads another tenant, and the server reads the tenant off the key,
  never off the request.

An admin sets a key's guardrail scope at issuance with `--guardrail-scope workarea|ancestors`. It
defaults to `ancestors`. A key's scope changes only by reissuing the key ([guardrails](../use/guardrails.md)).
