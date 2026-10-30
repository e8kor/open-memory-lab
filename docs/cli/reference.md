<!--
id: cli/reference
tier: engineering
status: design draft for review · decided, not built
answers: every administrative CLI verb, its flags and refusals, and how each rides the lab ring of the same HTTP API with no authority of its own; the reader grant the issuing verbs add beside a writer grant; the oml-admin plugin; how an ontology crew is created and its members' keys issued
-->

# CLI reference

**Status:** design draft for review · decided, not built

`oml` is the administration surface of Open Memory Lab. There is no web UI: every administrative
ceremony — key issuance, review, mass operations, settings — is a CLI verb, a decision recorded in
ADR-0015. The CLI is a lab client: it speaks the same [HTTP API](../api/http-api.md) as every
agent — every ceremony rides the lab ring (`/v2`,
[ADR-0022](../adr/ADR-0022-two-ring-api.md)); the CLI holds no verb the API lacks — authenticated
by operator, admin or dreamer keys, and holds no authority of its own. Every
act it performs is a journalled act performed by the server; every refusal is a
[typed error](../api/errors.md) naming its rule. The role model the verbs below assume is declared
in [roles and keys](../architecture/roles-and-keys.md); nothing on this page widens it.

This page owns the verb set of decision D14. [sessions.md](sessions.md) shows the same verbs in
mockup transcripts.

## Conventions

Every verb below is designed, not built; no transcript or flag on this page has run.

- **`--json` everywhere.** Every verb accepts `--json` and then emits exactly one JSON document on
  stdout — the server's typed response, unreshaped — so scripting never parses the human layout.
  The human layout may change between versions; the JSON layout is versioned with the API.
- **Exit codes.** `0` success; `1` the server refused with a typed error (the error's `rule` field
  is printed either way); `2` local failure (no context, unreachable server, bad arguments). A
  `--dry-run` that predicts refusals still exits `0`: the preview succeeded.
- **Mass operations default to `--dry-run`** and require `--apply` to act — the
  preview-every-mass-act rule: an act whose reach the operator has not seen is an act the
  operator has not decided. On this page that covers `bans add`, `keys revoke --matching`,
  `workareas re-parent`, and `simulations end`; each such synopsis lists `[--apply]`. The same
  discipline extends to **irreversible single acts**: `workareas create --prefix` states that a
  prefix is permanent and asks for confirmation before registering it.
  Single-target verbs whose synopsis lists `[--dry-run]` (`claims reassign`, `erase`) act by
  default; there `--dry-run` is an explicit opt-in preview, not the default.
- **Names or ids.** Every argument that names a workarea, crew, key or tenant takes its id or its
  [name](../api/http-api.md#names-and-ids), and the server resolves it; the CLI resolves nothing
  itself. Every grant, act and journal event binds to the **id**
  ([workareas](../architecture/workareas.md)); the CLI prints name and id, and the JSON output
  carries both. A derived path is not an argument: `oml workareas tree` shows paths, and every
  verb takes the node's name. No verb renames anything — names never change
  ([ADR-0026](../adr/ADR-0026-names-are-keys.md)).
- **Nothing is deleted silently.** There is no `oml ... delete` for content. Erasure is
  `oml erase`, a journalled crypto-shredding act whose honest boundary is declared in
  [audit and lineage](../architecture/audit-and-lineage.md).

## `oml context` — auth and endpoint

The CLI keeps named contexts (endpoint URL + a reference to a key) in a local file with
owner-only permissions. The key value itself can be supplied per call (`--key-stdin`, or the
`OML_API_KEY` environment variable); storing it in the context file is a convenience the operator
may forbid by policy. The context never changes what a key can do — tenant and role are derived
server-side from the key on every call, never from the context
([security](../architecture/security.md)).

| Verb | Does |
|---|---|
| `oml context add <name> --url <https://…> [--key-stdin]` | Record a context. With `--key-stdin` the key is read once from stdin, never from argv (argv is visible to other processes). |
| `oml context use <name>` | Select the active context. |
| `oml context show` | Active context: endpoint, key id (never the value), role, tenant, key expiry — resolved by one authenticated call. |
| `oml context remove <name>` | Forget a context locally. Removing a context revokes nothing; revocation is `oml keys revoke`. |

## `oml bootstrap` — claiming the root operator key

The first-start Helm job never prints key material: it mints a single-use, expiring **enrollment
secret**, stored as a Kubernetes Secret, and its logs carry progress only
([deployment](../operations/deployment.md)). `oml bootstrap claim` exchanges that secret over TLS
for the root operator key. This is the one verb that takes `--url` directly: it runs before any
context or key exists, so there is nothing for a context to resolve.

| Verb | Does |
|---|---|
| `oml bootstrap claim --url <https://…> --name <key-name> [--token-stdin]` | Exchange the enrollment secret for the root operator key, named `<key-name>` in the provider's namespace (fronts `POST /v2/bootstrap/claim` — bootstrap is a lab ceremony, [ADR-0022](../adr/ADR-0022-two-ring-api.md)). With `--token-stdin` the secret is read once from stdin, never from argv. The secret is consumed on first use and expires unclaimed; a second claim is refused. The key value is printed exactly once (show-once) and the issuance is journalled as issued-without-value in the provider chain — the server keeps only the hash. If no operator key survives, re-running the bootstrap ceremony mints a fresh enrollment secret — a deliberate ceremony, not an everyday verb ([roles and keys](../architecture/roles-and-keys.md)). |

## `oml tenants` — the operator's verbs

The operator role exists to bootstrap tenants and nothing else: it never writes, reads or reviews
content ([roles and keys](../architecture/roles-and-keys.md)). The root operator key is claimed
show-once through `oml bootstrap claim` above, against the enrollment secret the first-start Helm
job mints ([deployment](../operations/deployment.md)); the
[bootstrap transcript](sessions.md#1-bootstrap-the-operator-key-and-the-first-tenant)
shows the whole ceremony.

| Verb | Does |
|---|---|
| `oml tenants create <name>` | Create a tenant named `<name>`, unique in the deployment (`name.taken`), and its permanent root workarea, named `universe`. |
| `oml tenants list` | Tenants and their chain-head seqs. No content crosses this surface. |
| `oml tenants issue-admin <tenant> <key-name> [--label <text>]` | Issue the tenant's **first** admin key, named `<key-name>`, show-once; `<tenant>` is the tenant's id or name. Refused once the tenant has an admin: further keys come from that admin under the issuer ceiling. |

## `oml keys` — issuance, revocation, rotation

Key lifecycle rules — issuer ceiling, expiry, the bootstrap ceremony — are declared in
[roles and keys](../architecture/roles-and-keys.md). The CLI adds the **show-once ceremony**: a key
value is printed exactly once, at issuance — or, with `oml keys issue-agent --out`, written exactly
once to a new owner-only file and printed nowhere — and the journal records the issuance
*without the value*. The server stores only the key hash; a lost value is unrecoverable by design, and the remedy is
revoke-and-reissue.

**The reader grant beside a writer grant.** This paragraph is the single declaration site of the
CLI's issuance default ([ADR-0026](../adr/ADR-0026-names-are-keys.md) d13). `oml keys issue` and
`oml keys issue-agent` send, beside every writer grant they are asked for, a reader grant on the
same workarea with the same deny masks, unless the request already carries that reader grant or
`--write-only` is passed. The default is the CLI's alone: `POST /v2/keys` takes exactly the grants
it is sent ([issuance order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)),
so the added grant is an ordinary grant in the request body, journalled as its own `grant.issued`
and listed by `oml keys show` like any other. `--write-only` sends the writer grants alone, for a
crew whose writers must not read one another's promoted work; such a key still reads its own work,
in every state, with no grant at all ([own](../architecture/roles-and-keys.md#own)). `oml grants
add` is not an issuing verb and sends exactly the grant it names.

| Verb | Does |
|---|---|
| `oml keys issue <key-name> --role <reader\|writer\|dreamer\|admin> --grant <workarea>:<role> [--grant …] [--default-workarea <workarea>] [--crew <crew>] [--write-only] [--principal <text>] [--expires <duration>] [--label <text>]` | Issue a key named `<key-name>` — its [name](../api/http-api.md#names-and-ids), unique in the tenant and never released (`name.taken`), distinct from its principal and label ([key names](../architecture/roles-and-keys.md#key-names-principals-and-labels)). Refused when any requested role exceeds the issuer's (issuer ceiling), and when the key would combine the admin role with a dreamer-class grant (`access.role_exclusive` — the two-key rule: admin ≠ reviewer). `--default-workarea` records a journalled issuance attribute: the workarea that provider-ring (`/v1`) requests from this key target when they name none ([ADR-0022](../adr/ADR-0022-two-ring-api.md), d4); its default when omitted is fixed by the [issuance order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order). `--crew` names the key's [crew](#oml-crews-crew-records), fixed for the key's life. Each `--grant` role must be [admissible](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order) under `--role`, and a writer grant brings its reader grant unless `--write-only` (above), so one writer key issued with `--grant <wa>:writer` both remembers and recalls. Prints the value once, and only after every grant is journalled; then only the key's name and id ever again. Sends no `profile`, so a key naming an ontology crew is refused (`access.profile_mismatch`); [`oml crews add-member`](#oml-crews-crew-records) issues those. |
| `oml keys issue-agent <key-name> --workarea <workarea> [--out <path>] [--write-only] [--crew <crew>] [--principal <text>] [--expires <duration>] [--label <text>]` | The one key the [agent plugin](../api/mcp-plugin.md#wiring-it-into-your-agent) needs, in one call: a `POST /v2/keys` with role `writer`, name `<key-name>`, principal `<key-name>` unless `--principal` names another, `held_by: agent`, a writer grant on `<workarea>` and its reader grant unless `--write-only` (above), and no default workarea, so the default is `<workarea>`, the first grant's node. No other call and no HTTP surface of its own: every refusal is the issuance's. Prints the two export lines below, and nothing else on stdout; with `--out` the value goes to a file and neither line carries it ([ADR-0026](../adr/ADR-0026-names-are-keys.md) d12). |
| `oml keys revoke <key>\|--matching <selector> [--reason <text>] [--apply]` | Journalled revocation, effective at the key's next request served by the lead, and after replication lag on a follower ([workareas](../architecture/workareas.md)). `--matching <selector>` revokes a set and therefore defaults to `--dry-run`, requiring `--apply` to act. |
| `oml keys rotate <key> <new-key-name> [--grace <duration>]` | Mint a successor key named `<new-key-name>` — new key id and name, since both keys are live through the grace window, linked by `rotated_from`, grants and crew reproduced from the journalled records — and print the new value once. One journalled act (`key.rotated`) carries both key ids and the grace deadline; the old key stays usable through the bounded grace window and is revoked at the deadline (journalled). Rotation is per key for now; bulk crew rotation is open — [FR-6](../product/roadmap.md). |
| `oml keys list [--role …] [--expiring <duration>]` | Key names, ids, roles, principals, labels, expiries. Never values. |
| `oml keys show <key>` | One key, by id or name — a revoked or expired key's name still resolves: grants with their `edge_labels`, denials, crew, member profile, epoch, issuance and revocation events. |

**`oml keys issue-agent` output.** On success the verb prints exactly two lines on stdout, the ones
the `oml` plugin reads, and every other line on stderr:

```console
export OML_URL='https://oml.example.com'
export OML_API_KEY='oml_…'
```

`OML_URL` is the active context's endpoint, without `/mcp`; `OML_API_KEY` is the key value,
printed this once and never again (the show-once ceremony). `eval "$(oml keys issue-agent
claude-laptop --workarea metals-crew)"` puts both into the shell that will start the agent without
the value entering shell history. With `--json` the verb prints the `201` body instead, as every
verb does, and no export lines.

**`--out <path>`** writes the value to a file instead of printing it. Before it sends anything the
CLI creates `<path>` exclusively, with mode 0600; a path that already exists is refused (exit `2`)
and nothing is issued, so no file is ever overwritten. After the `201` the CLI writes the value
and one newline to the file and prints only:

```console
export OML_URL='https://oml.example.com'
export OML_API_KEY="$(cat '/home/op/.config/oml/keys/claude-laptop')"
```

No key value reaches stdout, stderr or the terminal; the second line reads it from the file when
the operator's shell runs it. A refused issuance removes the empty file the CLI created. If the
value cannot be written once the key is issued, the value is lost: the CLI prints the key's name
and id on stderr and exits `2`, and the key is to be revoked (`oml keys revoke`). A path
containing a quote or a newline is refused, so the printed line is always safe to run, and
`--out` with `--json` is refused, because the `201` body carries the value. The file is the one
place the [key format rule](../architecture/roles-and-keys.md#key-format) lets a value rest; the
operator deletes it once the key is in the environment that needs it.

## `oml grants`, `oml deny`, `oml bans` — scope and refusal

Grant and deny semantics (node-id binding, per-grant deny masks, the global per-key deny object,
deny-wins, the permission epoch) are declared in [workareas](../architecture/workareas.md); bans
are declared in [roles and keys](../architecture/roles-and-keys.md) and checked at admission. All
three verb
groups share `--dry-run`, which previews **reach**: the server computes and returns what the act
would change — which keys gain or lose which verbs over which subtrees, which pending submissions
a ban would refuse — without journalling anything.

| Verb | Does |
|---|---|
| `oml grants add <key> <workarea>:<role> [--deny <subtree> …] [--edge-label <label> …]` | Add a grant, optionally carrying deny entries that mask subtrees within this grant only. `--edge-label`, on a dreamer grant only, [narrows](../architecture/roles-and-keys.md#narrowed-dreamer-grants-edge_labels) it to claim edges with the listed `oml:` labels. A dreamer-class grant on an admin key is refused (`access.role_exclusive` — the two-key rule), and a grant a key's [member profile](../architecture/roles-and-keys.md#member-profiles) does not admit is refused (`access.profile_mismatch`). Journalled; bumps the key's permission epoch. |
| `oml grants remove <key> <grant-id>` | Remove one grant. Journalled; epoch bump. |
| `oml grants list [--key <key>] [--workarea <workarea>]` | Grants as journalled, with the derived effective access per key. |
| `oml grants effective <key> [--at <workarea>]` | The resolved answer: what this key can do, where, and which deny produced each hole — reproducible from the journal by construction. |
| `oml deny set <key> <subtree> [--reason <text>]` | Create or extend the key's **global** deny object (deny-wins across all grants). Distinct from a per-grant `--deny`. |
| `oml deny lift <key> <deny-id>` | Journalled lift. |
| `oml bans add --principal <id>\|--source <selector> [--reason <text>] [--apply]` | Ban an author or source. Default `--dry-run` prints the reach preview: keys affected, held submissions that would be refused, workareas touched. `--apply` journals it (`ban.imposed`). |
| `oml bans lift <ban-id>` | Journalled lift; admission resumes at the next check. |
| `oml bans list` | Active and lifted bans with who imposed and who lifted each. |

## `oml workareas` — tree, edges, status

The algebra — one tree per tenant under `universe`, labelled `related` edges conferring zero
access, forward-only status ladder — is declared in [workareas](../architecture/workareas.md).
The reads here — `tree`, `show` — run under an admin context, which reads the structure of every
workarea in its tenant without grants and never claim content
([admin structure reads](../architecture/roles-and-keys.md#permission-matrix)), or under a key with
reader effective access. Two of these verbs are two-party ceremonies: a status transition and a
workarea edge each need a **proposer and a distinct ratifier** (R2 extended to edges and transitions), so the CLI splits
them into `propose` and `ratify` run under different keys.

| Verb | Does |
|---|---|
| `oml workareas create <parent> <name> [--crew <crew>] [--prefix <PREFIX>]` | Create a child workarea named `<name>` — unique in the tenant (`name.taken`; `universe` is the root's) — in `development`. Refused under a `validated` parent. `--crew` names an existing [crew record](#oml-crews-crew-records). `--prefix` registers a vocabulary workarea ([ADR-0021](../adr/ADR-0021-ai-human-ontologies.md)): the prefix is tenant-unique and **permanent** — the command states this and confirms before acting — and requires `--crew` naming a crew of kind `ontology` (a vocabulary needs a named curator crew, [ADR-0027](../adr/ADR-0027-ontology-crews.md) d2). |
| `oml workareas tree [--root <workarea>] [--status]` | The tenant tree from `universe` down, with status and crew per node. |
| `oml workareas show <workarea>` | One node, by id or name: name, id, path (derived), status, crew record, edges, policy snapshot in force. |
| `oml workareas crew set <workarea> --crew <crew>` | Journalled crew-metadata update: point the workarea at another crew record. Allowed on `validated` areas — freeze covers claims and promotions, not crew metadata. |
| `oml workareas relate propose <a> <b> --label <relates_to\|continues\|derived_from\|mirrors>` | Propose an edge. Requires dreamer(propose) on ≥1 endpoint and reader on both. |
| `oml workareas relate ratify <edge-id>` | Ratify under a different key with dreamer(review) on **both** endpoints; ratifier ≠ proposer, refused otherwise. |
| `oml workareas re-parent <workarea> --new-parent <target> [--apply]` | Journalled admin act; the server performs the single-transaction descendant path rewrite. Mass-reach act: defaults to `--dry-run` showing the paths that would be rewritten and the grants whose derived paths change; `--apply` performs the rewrite. |
| `oml workareas status propose <workarea> --to <committed\|validated>` | Propose the next forward rung; the proposal is itself journalled (`workarea.status_proposed`). The ladder has no skips: `--to validated` is proposable only from `committed`, and a rung-skipping or out-of-ladder proposal is refused as `workarea.transition_refused` — the same code that refuses proposing `validated` while any child is not `validated`. Backward transitions do not exist; there is no flag for them. |
| `oml workareas status ratify <transition-id>` | Second, non-proposing dreamer or admin ratifies; the journal records the transition with its policy snapshot. The [freeze transcript](sessions.md#4-freezing-a-workarea) shows the full ceremony. |
| `oml workareas status pending` | Transitions awaiting a ratifier. |

## `oml crews` — crew records

A crew is a named record — name, kind, charter URI, external manager — recorded, not operated; a
workarea names at most one, and a key may name one at issuance
([workareas](../architecture/workareas.md#crews-recorded-not-operated)). Admin key role
throughout; no agent role reaches these verbs.

| Verb | Does |
|---|---|
| `oml crews create <name> [--kind general\|ontology] [--charter-uri <uri>] [--external-manager <text>]` | Create a crew record (`crew.created`). The name is unique in the tenant (`name.taken`) and never changes; so does the kind, `general` unless `--kind ontology` makes it an [ontology crew](../api/ontology-crews.md). |
| `oml crews update <crew> [--charter-uri <uri>] [--external-manager <text>]` | Journalled crew-record update (`crew.updated`), at least one flag; the name and the kind are not updatable. |
| `oml crews list` | Crew records: name, id, kind, charter URI, external manager. |
| `oml crews show <crew>` | One crew record, by id or name. |
| `oml crews add-member <crew> <key-name> --as <classifier\|term-creator\|curator> --vocabulary <workarea> [--vocabulary …] [--workarea <workarea> …] --out <path> [--held-by <agent\|human>] [--write-only] [--principal <text>] [--expires <duration>] [--label <text>]` | Issue one member key of an ontology crew in one `POST /v2/keys`, with the role, grants and default workarea of its [member profile](../architecture/roles-and-keys.md#member-profiles) (below). No other call and no HTTP surface of its own: every server refusal is the issuance's. Writes the value only to `--out`, which is required, and prints the two export lines below ([ADR-0027](../adr/ADR-0027-ontology-crews.md) d14). |

**`oml crews add-member`.** This paragraph is the single declaration site of the verb's request.
The body carries `name` `<key-name>`, `crew` `<crew>`, `profile` (`term-creator` is the CLI's
spelling of `term_creator`), `held_by` from `--held-by` (default `agent`; a curator who must
satisfy `human_required_classes` is `human`), `principal` `<key-name>` unless `--principal` names
another, and, per profile:

| `--as` | Role | Grants, in this order | Default workarea |
|---|---|---|---|
| `classifier` | `dreamer` | reader on each `--workarea`; then, on each `--vocabulary`, dreamer with `edge_labels: ["oml:about"]` and reader | the first `--workarea` |
| `term-creator` | `writer` | on each `--vocabulary`, writer and reader — the reader dropped with `--write-only`, as for every issuing verb ([above](#oml-keys-issuance-revocation-rotation)) | the first `--vocabulary` |
| `curator` | `dreamer` | on each `--vocabulary`, dreamer and reader; then, on each `--workarea`, dreamer with `edge_labels: ["oml:about"]` and reader | the first `--vocabulary` |

A classifier needs at least one `--workarea`, every profile at least one `--vocabulary`, a term
creator takes no `--workarea`, and `--write-only` is a term creator's alone; an argument the
profile cannot take exits `2` before anything is sent. **`--out <path>` is required** and follows
`issue-agent`'s discipline exactly ([above](#oml-keys-issuance-revocation-rotation)): the file is
created exclusively with mode 0600 before anything is sent, an existing path or one containing a
quote or a newline is refused, a refused issuance removes the empty file, and a value that cannot
be written once the key is issued is lost, the CLI printing the key's name and id on stderr and
exiting `2`. The verb prints only:

```console
export OML_URL='https://oml.example.com'
export OML_API_KEY="$(cat '/home/op/.config/oml/keys/metals-classifier-1')"
```

`--json` is refused, because the `201` body carries the value, so no key value reaches stdout,
stderr or a transcript. What each member does with its key is worked through on the
[ontology crews](../api/ontology-crews.md) page.

## `oml review` — the review ceremony

The CLI is the *exception* path for review — the designed reviewers are external crews holding
dreamer keys over the HTTP API ([admission](../architecture/admission.md)) — but the ceremony is
identical on both surfaces, and the CLI is where a human exercises it.

**`serve_id` mechanics.** `oml review next` serves exactly one held item and returns a `serve_id`
that the server binds to (reviewer key, artifact version, attachment digest set). A verdict is
accepted only against a valid `serve_id`; the serve expires, is consumed by its first verdict, and
is invalidated by any change to the served content. The journalled verdict records the digests that
were served. The point is evidentiary: no verdict can exist that wasn't preceded by the server
handing *this reviewer* *this exact content* — a reviewer is served every attachment of a claim
before a verdict counts ([storage](../architecture/storage.md), D3), and a supersession is served
with the version it replaces — a reviewer reads a change against what it replaces, never in
isolation.

| Verb | Does |
|---|---|
| `oml review next [--workarea <subtree>]` | Serve one held item: body, all attachments (downloaded to a working directory, verified against their served digests), the superseded version when applicable, and the `serve_id`. Refused with `review.author_in_lineage` when the reviewer's key or principal appears in the item's lineage closure — the server never serves what this key may not judge. |
| `oml review approve <serve_id> [--note <text>]` | Verdict: promote. Refused without a valid, unexpired, unconsumed serve. |
| `oml review reject <serve_id> --reason <text>` | Verdict: reject. Same serve rules. |
| `oml review reopen <artifact>@<version>` | Non-author reopen of a rejection — a journalled verb in its own right, not a resubmission. Refused for the original author. |
| `oml review queue [--workarea <subtree>]` | Held-item counts and ages per workarea; the same numbers `oml status` reports as queue depth. |

## `oml simulations` — the fenced channel

Semantics — indelible `provenance: simulation`, audience-scoped visibility, taint propagation,
fail-closed promotion — are declared in [simulations](../architecture/simulations.md). The CLI
operates the fence; it cannot weaken it.

| Verb | Does |
|---|---|
| `oml simulations open --audience <key>[,…] [--label <text>]` | Journalled act creating the simulation and its audience list. A simulation whose audience contains no dreamer key can promote nothing — the CLI warns, the server fails closed. |
| `oml simulations list [--ended]` | Simulations with state, audience size, row and taint counts. |
| `oml simulations show <sim-id>` | One simulation: audience, rows written, sim-derived (tainted) claims outside it. |
| `oml simulations end <sim-id> [--apply]` | Journalled end: demotes the simulation's rows and triggers re-review of tainted claims. Mass-reach act: defaults to `--dry-run` listing every row and taint the end would touch; `--apply` journals `simulation.ended` and the per-row demotions. |

## `oml claims` and `oml erase` — claim administration

Admin verbs over existing claims. None of them is a verdict: admin ≠ reviewer.

| Verb | Does |
|---|---|
| `oml claims reassign <artifact> --to <workarea> [--re-key <claim_key>] [--dry-run]` | Journalled move recording both policy snapshots; a claim moved to a stricter-policy destination re-enters `held`. Refused into `validated` areas. A `claim_key` collision at the destination is refused unless the move records a re-key. |
| `oml claims demote <artifact>@<version> --reason <text>` | Journalled demotion; bytes are kept (R4). |
| `oml claims tip-override <workarea>:<claim_key> --to-version <n> --reason <text>` | Designate an earlier promoted version as the served tip without demoting the later one — explicit and journalled (`lifecycle.tip_override`, [event catalog](../data/event-catalog.md)), never a silent re-rank. Admin verb; not a verdict. |
| `oml erase <artifact> [--dry-run]` | Journalled crypto-shredding: an erase event plus DEK destruction, scoped to body and attachment bytes. `--dry-run` prints the erasure inventory for this artifact — every copy class and its disposition, including the retention windows plaintext-derived data persists under ([audit and lineage](../architecture/audit-and-lineage.md)). Attribution metadata survives erasure; closing that is [FR-1](../product/roadmap.md). |

## `oml audit` — evidence export and chain verification

What the export contains and what the chain proves are declared in
[audit and lineage](../architecture/audit-and-lineage.md). The sanctioned claim, verbatim: the
export **produces the evidence an auditor asks for** — never "compliant", never "regulator-grade".

| Verb | Does |
|---|---|
| `oml audit export --out <dir> [--from <seq>] [--include-simulations]` | Signed evidence bundle: per-memory decision trail, per-tenant chain verification, grants history, read log scoped to its retention window, reciprocal-approval statistics per key pair, and — only with the flag — simulation rows, listed separately by `simulation_id`. The export act is itself journalled. |
| `oml audit verify-chain [--from <seq>] [--genesis]` | Recompute and verify the per-tenant hash chain and signatures from the local export or live from the server. Digest-only journal payloads mean verification survives crypto-shredding (D1). Prints the signing key set in use — each key's fingerprint, the lowercase-hex SHA-256 of the 32 raw public-key bytes, and whether it is current or superseded per `system.signing_key_rotated` — the value a recipient confirms over an independent channel ([chain-format](../data/chain-format.md)). `--genesis` runs the full-history verification that the reconciler otherwise runs as an occasional named job ([reconciliation](../architecture/reconciliation.md)). |

## `oml settings` — configured vs effective

Settings are journalled records on the server; the CLI never edits a live value in place.

| Verb | Does |
|---|---|
| `oml settings show [--effective]` | Without the flag: the configured document as last applied. With it: what the running deployment is actually using, with every divergence (defaulted value, not-yet-picked-up change, per-replica variance) marked per key. |
| `oml settings validate <file>` | Server-side validation without applying. A document that names a **retired settings key is refused by that key's name**, with the setting that replaced it — silently ignoring unknown keys is how configuration rots. |
| `oml settings apply <file>` | Validate, then journal and apply. Refused on any validation error; there is no `--force`. |

## `oml status` and `oml doctor` — the named signals

`status` is a snapshot; `doctor` evaluates each signal against its configured threshold and says
what to do next. Both emit the same four named signals — the contract that
[observability](../operations/observability.md) owns:

| Signal | Meaning | Unhealthy looks like |
|---|---|---|
| **Queue depth per step** | Items waiting at each admission step: quarantine, scan, review, ratification ([admission](../architecture/admission.md)). | Review depth growing without bound — arrival rate exceeds review capacity, which is exactly the unmeasured quantity [FR-2](../product/roadmap.md) tracks. |
| **Review availability** | Whether each workarea's review policy can currently be satisfied — at least one eligible non-author dreamer reachable. Two vocabulary refinements of the same signal ([ADR-0021](../adr/ADR-0021-ai-human-ontologies.md)): a vocabulary workarea with no active unnarrowed dreamer grant is flagged orphaned ("propose freeze" is the named next action) — a classifier's grant, [narrowed](../architecture/roles-and-keys.md#narrowed-dreamer-grants-edge_labels) to classifications, is not curation and does not count — and a workarea whose policy names `human_required_classes` with no attested-human dreamer key in scope is a review-availability warning. | A workarea failing closed: writes held indefinitely because no eligible reviewer exists. |
| **Outbox frontier lag** | Journal head seq minus the lake writer's applied frontier, per tenant ([storage](../architecture/storage.md)). | A rising frontier lag with a healthy journal — the lake writer is down, split-brained-and-fenced, or stuck. |
| **Dead-letter count** | Rows the lake writer abandoned after its retry budget. Each is already a journalled `materialize-skipped` event and a named reconciler finding ([reconciliation](../architecture/reconciliation.md)). | Any nonzero count that is not shrinking; repair is a journalled re-enqueue, not a manual table edit. |

| Verb | Does |
|---|---|
| `oml status` | The four signals, one screen, plus chain-head seq and last reconciler run per tenant. |
| `oml doctor` | Each signal against its threshold with a verdict (`ok`/`warn`/`fail`) and the next action — the named command or the runbook anchor. Exit `1` on any `fail`, so probes and CI can consume it. |

## The `oml-admin` plugin

A Claude Code plugin that wraps this CLI for an operator working inside Claude Code. This section
is its single declaration site ([ADR-0026](../adr/ADR-0026-names-are-keys.md) d14); it is decided,
not built. It is listed beside the agent-facing `oml` plugin in the repository's marketplace and is
installed separately: an agent that installs `oml` gains no admin instruction, and the `oml` plugin
cannot issue keys, since its tools are the agent-safe subset
([agent plugin](../api/mcp-plugin.md); ADR-0014 d2).

- **Contents.** One skill, `issue-agent-key`, and no MCP server entry, so the plugin adds no tool
  to any tool list. Every act goes through the `oml` CLI on the operator's `PATH`, under the
  operator's active context.
- **What the skill does**, in order: runs `oml context show` and stops unless the active context's
  key role is `admin`; checks the key name and the workarea reference against the
  [name grammar](../api/http-api.md#names-and-ids) and says which rule fails, sending nothing;
  picks the key file — `~/.config/oml/keys/<key-name>` unless the operator names another path
  outside any repository — and creates its directory owner-only if it is missing; runs `oml keys
  issue-agent <key-name> --workarea <workarea> --out <path>` itself, with `--write-only`, `--crew`,
  `--expires` or `--label` when the operator asks for them, after stating the reader grant it adds
  ([above](#oml-keys-issuance-revocation-rotation)); hands the operator the two printed lines,
  which carry a path and no value, to run in the shell that will start Claude Code; runs `oml keys
  show <key-name>` and reports the key's id, grants and default workarea; and tells the operator to
  start Claude Code from that shell, so the `oml` plugin reads `OML_URL` and `OML_API_KEY`, and to
  delete the key file afterwards.
- **What it never does.** It runs `oml keys issue-agent` only with `--out`, and never with
  `--json`. It never runs any other verb whose output carries a key value — `keys issue`,
  `keys rotate`, `tenants issue-admin`, `bootstrap claim` — never reads the key file, and never
  runs the printed lines in its own shell, because a value in a command's output or in its own
  environment would enter the session transcript or the model's context, places the show-once
  ceremony ([ADR-0015](../adr/ADR-0015-cli-administration.md) d1) and the
  [key format rule](../architecture/roles-and-keys.md#key-format) forbid.

## Verbs that do not exist

Absences are design, not gaps. No `oml promote` — promotion is only an effect of ratification
inside the review ceremony, and the gate cannot be bypassed from the admin seat (admin ≠ reviewer,
R1/R2). Demotion exists only as the journalled admin act `oml claims demote` above — it is
governance, never a verdict. No
`oml delete` — erasure is journalled crypto-shredding, demotion keeps bytes (R4). No backward
status transition. No verb reads another tenant. No verb renames a workarea, crew, key or tenant:
a name never changes, and a row that needs another name is a new row
([ADR-0026](../adr/ADR-0026-names-are-keys.md)). Where a workflow seems to need one of these, the
answer is a policy or roadmap question — see the [roadmap](../product/roadmap.md), not a flag.
