<!--
id: guides/administering
tier: engineering
status: draft for review · describes administration as v0.4.0 builds it; key revocation, rotation, listing, grant changes and guardrails not built
answers: how an administrator runs an OML server with the oml CLI: the first hour on a new server; claiming it with the enrollment token and --token-stdin; where the admin credentials live; tenants; workareas, their tree and names; crews; issuing agent, reviewer and admin keys, --out and --connect-code; grants and denials; ending a connection; what stands in for revoking, rotating and listing keys until they are built; reading the journal; and the rules no admin key bypasses
-->

# Guide: administering a server

**Status:** draft for review · describes administration as v0.4.0 builds it; key revocation,
rotation, listing, grant changes and guardrails not built

**Release:** the claim since v0.1.0, with `--token-stdin` since v0.4.0; workarea creation and key
issuance since v0.1.0, `oml keys issue-agent` since v0.2.1, crews, names and `oml keys show` since
v0.3.0; connector keys, `oml keys connect-code` and `oml keys disconnect` since v0.4.0
([releases](../product/releases.md))

Administration is the `oml` CLI; there is no web console
([ADR-0015](../adr/ADR-0015-cli-administration.md)). Every verb below is one request to the
server's [HTTP API](../api/http-api.md), journalled by the server, and every refusal names its
rule. The full contract of each verb is the [CLI reference](../cli/reference.md); this page is the
order and the tasks. The verbs here run in the CLI's own test suites against a real server; this
page's walkthrough has not been run end to end against a live deployment.

## The first hour on a new server

You need the server running with nothing claimed, the `oml` CLI ([install](../start/install.md)),
and the server reachable only by you until step 2 is done.

1. **Read the enrollment token.** While no tenant exists, the server prints a single-use token,
   `enr_` and 48 hex digits, on stdout at every start. A restart prints a new one and voids the
   old. On the interim chart:
   `TOKEN="$(kubectl --namespace oml logs deployment/oml-server-lead | grep -o 'enr_[0-9a-f]\{48\}' | tail -1)"`.
2. **Claim the server** before any proxy or Ingress exposes it: whoever presents the token first
   receives the first admin key and the tenant
   ([the bootstrap race](../operations/deployment.md#claiming-the-first-admin-key-the-bootstrap-race)).

   ```sh
   printf '%s\n' "$TOKEN" | oml --url https://oml.example.com bootstrap claim ops-admin \
       --token-stdin --tenant-name acme --principal platform/you
   ```

3. **Check it.** `oml doctor` prints each health signal with a verdict and exits `1` on any fail.
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

   Run the last line it prints, `oml setup claude-code --url … --key-file …`, where Claude Code
   runs ([Claude Code](claude-code.md)), then delete the file.
6. **Issue a reviewer's key** under another principal:

   ```sh
   oml keys issue maya --role dreamer --held-by human --principal crew/maya \
       --default-workarea metals --grant metals:dreamer
   ```

7. **Look at what you built:** `oml workareas tree` and `oml keys show claude-laptop`.

The agent's writes now wait as `held` until `maya` approves them with `oml review next` and
`oml review approve` ([review](../cli/reference.md#oml-review-the-review-ceremony)).

## Claiming a server

`oml bootstrap claim <key-name> --token-stdin [--tenant-name <name>] [--principal <text>]` sends
`POST /v2/bootstrap/claim` with no key and takes the URL alone. `--token-stdin` is required: the
token is read without echo from a terminal, or as the first line of piped stdin, so it never
reaches argv or shell history. v0.3.0's `--token <secret>` is gone and refused as an unknown flag
([CLI reference](../cli/reference.md#oml-bootstrap-claiming-the-root-operator-key)).

On success the token is consumed, a second claim is refused, and the CLI prints the first admin
key once, with its name, key id, tenant id and root workarea id. The server keeps only the key's
hash. `oml quickstart` makes the same claim for a laptop and keeps its pair in its own directory
([self-hosting](self-hosting.md#1-a-laptop-oml-quickstart)).

## The admin credentials

The claim writes `url` and `api_key`, nothing else, to the credentials file, owner-only (mode
0600 on Unix): `~/.config/oml/credentials`, or the file `OML_CLI_CREDENTIALS` names, so a second
server is a second file. `oml quickstart` writes `oml/credentials` in its directory instead.

The CLI takes its pair from the first source that sets either value: `--url` and `--api-key`, then
`OML_URL` and `OML_API_KEY`, then the file; a source that sets one alone is refused
([start settings](../cli/reference.md#start-settings)). A shell that exports an agent's pair
therefore wins over the stored admin pair: run admin verbs with both variables unset. `oml login
<url>` replaces the stored pair after `GET /v2/whoami` accepts the key.

An admin key never goes into an agent: `oml setup claude-code` refuses an admin or operator key
as `setup.admin_key`. To write or recall yourself, issue yourself a writer key.

## Tenants

The claim creates the one tenant a deployment hosts in v0.4.0, with its root workarea `universe`.
`oml tenants create` exists and refuses locally: the operator tier that creates further tenants
is not built ([CLI reference](../cli/reference.md#oml-tenants-the-operators-verbs)).

## Workareas and names

A tenant's workareas form one tree under `universe`. A grant on a workarea covers its subtree.

| Task | Command |
|---|---|
| Create a child | `oml workareas create <parent> <name> [--crew <crew>]` |
| See the tree | `oml workareas tree [--root <workarea>]` |
| See one node | `oml workareas show <workarea>` |
| Point a workarea at another crew | `oml workareas crew set <workarea> --crew <crew>` |

A new workarea starts in `development`; the ladder to `committed` and `validated` is planned for
v0.5.0. An admin key reads the structure of every workarea in its tenant and never a memory in it.

Workareas, crews, keys and tenants each carry a **name**: unique in the tenant, given at creation,
never changed and never released ([names](../concepts.md#names)). Every verb accepts a name where
it accepts an id, and the server binds grants and events to the id. No verb renames anything; a
row that needs another name is a new row. Choose names you can live with, because a retired key's
name stays taken.

## Crews

A crew is a record, name, charter URI and external manager, that OML stores and does not operate.
A workarea names at most one; a key names one at issuance, fixed for its life.

| Task | Command |
|---|---|
| Create | `oml crews create <name> [--charter-uri <uri>] [--external-manager <text>]` |
| Change the charter or manager | `oml crews update <crew> --charter-uri <uri>` and/or `--external-manager <text>` |
| List | `oml crews list` |
| Show one | `oml crews show <crew>` |

## Issuing keys by role

A key's role fixes which grants it may carry: `writer` takes `reader` and `writer` grants,
`dreamer` takes `reader` and `dreamer`, `reader` takes `reader`, and `admin` takes none
([admissibility](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)). An
admin issues keys up to its own role, other admin keys included
([issuer ceiling](../architecture/roles-and-keys.md#issuer-ceiling)). Every value is shown once;
a lost value is not recoverable.

**An agent.** `oml keys issue-agent <key-name> --principal <principal> --workarea <workarea>
[--held-by agent|human] [--crew <crew>] [--write-only] [--out <path>] [--connect-code]` issues a
writer key with a writer grant on the workarea, its reader grant beside it unless `--write-only`,
and the workarea as its default. Give each agent its own principal: review independence keys on
it. Without `--out` it prints `export OML_URL=…` and `export OML_API_KEY=…`. With `--out` the CLI
creates the file owner-only before sending anything, refuses a path that exists, writes the value
there and prints four lines that read the file, none carrying the value
([output](../cli/reference.md#oml-keys-issuance-revocation-rotation)). The
[`oml-admin` plugin](../cli/reference.md#the-oml-admin-plugin) runs the same verb from Claude Code
with `--out`, so the value never enters the conversation.

**A claude.ai or phone connection.** Add `--connect-code`: the key gets no value, and the verb
prints the connector URL and a single-use code instead. It needs `oauth.issuer` set; without it
the issuance is refused as `oauth.connect_refused`. `oml keys connect-code <key>` mints a fresh
code and voids the earlier ones. The whole flow is
[claude.ai and mobile](claude-ai-and-mobile.md); no live claude.ai connection has been made yet.

**A reviewer.** `oml keys issue <key-name> --role dreamer --grant <workarea>:dreamer
[--held-by human] [--principal <text>] [--default-workarea <workarea>] [--crew <crew>]`. Use a
principal no writer in that workarea uses. Add `--grant <workarea>:reader` too, or the reviewer is
served a held memory without its sources; the reviewer's side is
[reviewing memories](reviewing.md).

**Another admin.** `oml keys issue <key-name> --role admin --held-by human --principal <text>`,
with no `--grant`. An admin key with a dreamer grant is refused as `access.role_exclusive`.

**A reader or service.** `oml keys issue <key-name> --role reader --held-by service --grant
<workarea>:reader`.

`oml keys issue` also takes `--label <text>` and `--write-only`, and names on stderr each reader
grant it adds beside a writer grant. Key expiry is not built: an issued key lives until you deny
it.

## Grants and denials

Grants are fixed at issuance in v0.4.0: `oml grants add`, `remove`, `list` and `effective` are
designed, not built. What you can change on a live key is a **denial**:

```sh
oml deny set <key> <workarea> [--reason <text>]
```

It journals `key.denied` and wins over every grant the key holds on that subtree, at the key's
next request. A denial on an admin key is refused: an admin key holds no grant to attenuate.
Lifting a denial is not built, so a denial is permanent in v0.4.0; to restore access, issue a new
key under a new name.

## Cutting a key off, replacing it, and finding it

| Task | In v0.4.0 |
|---|---|
| Cut a key off | `oml deny set <key> universe` keeps it from every workarea. `oml keys revoke` is designed, not built. |
| Rotate a key | Issue a successor under a new name, move the holder to it, then deny the old key `universe`. `oml keys rotate` is designed, not built. |
| List keys | Not built (`oml keys list`). `oml keys show <key>` reads one, by id or name, a revoked key's included: grants, denials, crew, epoch, and its revocation time or `no (live)`. |
| End one connection | `oml keys show <key>` lists the connector key's connections; `oml keys disconnect <key> <connection-id> [--reason <text>]` ends one, journalled as `connection.revoked`. |
| End every connection of a key | `oml deny set <key> universe`: a connection acts as exactly its key ([ending a connection](claude-ai-and-mobile.md#ending-a-connection)). |

No admin key can be cut off in v0.4.0: a denial on it is refused and revocation is not built.
Keep the admin credentials file on one machine, and issue a second admin key only to a person who
needs one.

If an issuance answers nothing usable after reaching the server, the CLI cannot tell whether the
key exists: `oml keys show <key-name>` answers, and a key whose value was lost is denied
`universe` and replaced under a new name.

## Reading the journal

`oml events [--after-seq <seq>] [--workarea <workarea>] [--limit <n>]` prints one page of
`GET /v2/events`: digest-only events within the key's reader access, with the `next_after_seq` to
pass back. It needs reader access on the workarea, so run it with a reader key, not the admin key.
Key, grant, crew, connection and review events appear on no key's tail; they belong to the
evidence export ([events tail](../api/http-api.md#events-tail)), which is not built. `oml audit
verify-chain` exists and is refused while the server serves no chain export.

## Rules no admin key bypasses

- **The reviewer is never the author.** An admin verdict is refused, an admin key never carries a
  dreamer grant, and a verdict from any key in a claim's lineage is refused as
  `review.author_in_lineage`. A person who administers and reviews holds two keys.
- **No promotion by fiat.** There is no `oml promote`; a claim promotes only by a non-author
  verdict on a serve.
- **Nothing is deleted silently.** There is no `oml delete`; a rejected or retired claim keeps its
  bytes, and erasure is designed as a journalled act, not built
  ([verbs that do not exist](../cli/reference.md#verbs-that-do-not-exist)).
- **No name changes or returns.** Every name is permanent and never reused.
- **No other tenant.** No verb reads another tenant, and the tenant is read off the key, never
  the request.

Guardrails, the reviewed rules every session loads and each tool call is checked against, are
planned for v0.5.0 ([ADR-0037](../product/releases.md#planned-releases), proposed); no guardrail
setting exists in v0.4.0.
