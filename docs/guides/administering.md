---
status: Stable
verified: 2026-11-06
answers: "how an administrator runs an OML server with the oml CLI: the first hour (claim the server, create a workarea, issue an agent key and a reviewer key), where admin credentials live, workareas, crews, keys by role, denials, ending a connection, reading the journal, and the rules no admin key bypasses"
---

# Administer an OML server

Administration is the `oml` CLI: each verb is one [HTTP API](../api/http-api.md) request, journaled, and each refusal names its rule. The [CLI reference](../cli/reference.md) holds the contracts; this page gives the order.

## The first hour

You need an unclaimed server only you can reach, and the `oml` CLI ([install](../start/install.md)).

1. **Read the enrollment token**, `enr_` plus 48 hex digits, which the server prints at every start while no tenant exists; a restart voids the old one. On Kubernetes, read the pod log ([Kubernetes](kubernetes.md)).
2. **Claim the server** before any proxy exposes it; the first to present the token gets the first admin key ([deploy](../operations/deployment.md#claim-the-first-admin-key)).

   ```sh
   printf '%s\n' "$TOKEN" | oml --url https://oml.example.com bootstrap claim ops-admin \
       --token-stdin --tenant-name acme --principal platform/you
   ```

3. **Check it** with `oml doctor`, which exits `1` on any failure.
4. **Create a crew and a workarea for it.**

   ```sh
   oml crews create metals-crew --charter-uri https://wiki.example.com/metals
   oml workareas create universe metals --crew metals-crew
   ```

5. **Issue the agent's key into a file:**

   ```sh
   mkdir -p -m 700 ~/.config/oml/keys
   oml keys issue-agent claude-laptop --principal crew/agent-7 --workarea metals \
       --crew metals-crew --out ~/.config/oml/keys/claude-laptop
   ```

   Run the last line it prints, `oml setup claude-code --url ... --key-file ...`, where Claude Code runs ([Claude Code](claude-code.md)); then delete the file.
6. **Issue a reviewer key (role `dreamer`)** under a different principal:

   ```sh
   oml keys issue maya --role dreamer --held-by human --principal crew/maya \
       --default-workarea metals --grant metals:dreamer --grant metals:reader
   ```

Its writes wait as `held` until `maya` approves them with `oml review next` and `oml review approve` ([reviewing](reviewing.md)). Check the result with `oml workareas tree` and `oml keys show claude-laptop`.

## Where the admin credentials live

The claim stores `url` and `api_key` in `~/.config/oml/credentials` (0600). `OML_CLI_CREDENTIALS` names another file.

The CLI takes its pair from the first source that sets either value: `--url` and `--api-key`, then `OML_URL` and `OML_API_KEY`, then the file. An exported agent pair therefore wins over the stored admin pair, so run admin verbs with both unset. `oml login <url>` replaces the stored pair once `GET /v2/whoami` accepts the key.

Never put an admin key into an agent; issue yourself a writer key.

## Workareas and names

A tenant's workareas form one tree under `universe`; a grant covers its subtree.

Verbs: `oml workareas create <parent> <name> [--crew <crew>]`, `tree`, `show`, `crew set`.

A new workarea starts in `development` and climbs to `committed` and `validated` with `oml workareas status`; one key proposes each step and another ratifies it. An admin key reads every workarea's structure, never a memory, except through the journaled [admin read](../api/http-api.md) routes.

Workareas, crews, keys and tenants carry a **name**: unique in the tenant, never changed or released ([names](../concepts.md)). Every verb accepts a name where it accepts an id.

## Crews

A crew is a record (name, charter URI, external manager) OML stores and does not operate. A workarea names at most one; a key's is fixed at issuance. Verbs: `oml crews create | update | list | show`. `oml tenants create` refuses: a deployment hosts one tenant.

## Issue keys by role

A key's role fixes its grants: `writer` takes `reader` and `writer`, `dreamer` takes `reader` and `dreamer`, `reader` takes `reader`, `admin` takes none ([roles and keys](../architecture/roles-and-keys.md)). The server shows each key value once.

**An agent.** `oml keys issue-agent` issues a writer key with a writer grant on the workarea, a reader grant unless `--write-only`, and the workarea as default. Give each agent its own principal. With `--out` the CLI creates the file owner-only first, refuses a path that exists, and prints lines that read the file, none carrying the value; without it, the verb prints `export` lines. The `oml-admin` plugin runs this from Claude Code with `--out`.

**A claude.ai or phone connection.** Add `--connect-code`: the key gets no value; the verb prints the connector URL and a single-use code. It needs `oauth.issuer`, else `oauth.connect_refused`. `oml keys connect-code <key>` mints a fresh code, voiding earlier ones ([claude.ai and mobile](claude-ai-and-mobile.md)).

**A reviewer.** `oml keys issue --role dreamer --grant <workarea>:dreamer` with a principal no writer there uses, plus `--grant <workarea>:reader`, or the reviewer sees a held memory without its sources ([reviewing](reviewing.md)).

**Another admin.** `--role admin --held-by human`, no `--grant`; a dreamer grant is refused as `access.role_exclusive`. **A reader or service:** `--role reader --held-by service --grant <workarea>:reader`.

## Deny, cut off, replace, disconnect

Grants cannot change after issue, and there is no revoke, rotate or list verb for keys: issue a new key under a new name. A live key can take a permanent **denial**, which wins over every grant on that subtree from the key's next request:

```sh
oml deny set <key> <workarea> [--reason <text>]
```

It is journaled as `key.denied`; on an admin key it is refused.

- **Cut off or end every connection:** `oml deny set <key> universe`.
- **Rotate:** issue a successor under a new name, move the holder, deny the old key `universe`.
- **Read one key:** `oml keys show <key>` shows grants, denials, crew, epoch and connections, never the value.
- **End one connection:** `oml keys disconnect <key> <connection-id> [--reason <text>]`.

An admin key cannot be cut off today: keep the admin credentials file on one machine. Deny a key whose value you lost and replace it.

## Read the journal

`oml events [--after-seq <seq>] [--workarea <workarea>] [--limit <n>]` prints one digest-only page of `GET /v2/events` within a reader key's access. `oml audit verify-chain` is refused while the server serves no chain export.

## Rules no admin key bypasses

An admin verdict is refused, and a verdict from any key in a memory's lineage is refused as `review.author_in_lineage` ([roles and keys](../architecture/roles-and-keys.md#review-independence)). A person who administers and reviews holds two keys. There is no `oml promote` and no `oml delete`: only a different reviewer's verdict approves, and rejected and retired memories keep their bytes. Names are permanent, and the server reads the tenant off the key. `--guardrail-scope workarea|ancestors` sets a key's guardrail scope at issuance ([guardrails](../use/guardrails.md)).

Design record: [ADR-0015](../adr/ADR-0015-cli-administration.md), [ADR-0010](../adr/ADR-0010-roles-and-keys.md), [ADR-0026](../adr/ADR-0026-names-are-keys.md).
