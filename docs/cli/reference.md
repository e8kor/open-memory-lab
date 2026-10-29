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
- **Names resolve to ids.** Workareas may be named by path for convenience, but every grant, act
  and journal event binds to the workarea **node id** ([workareas](../architecture/workareas.md));
  the CLI prints both and the JSON output always carries the id.
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
| `oml bootstrap claim --url <https://…> [--token-stdin]` | Exchange the enrollment secret for the root operator key (fronts `POST /v2/bootstrap/claim` — bootstrap is a lab ceremony, [ADR-0022](../adr/ADR-0022-two-ring-api.md)). With `--token-stdin` the secret is read once from stdin, never from argv. The secret is consumed on first use and expires unclaimed; a second claim is refused. The key value is printed exactly once (show-once) and the issuance is journalled as issued-without-value in the provider chain — the server keeps only the hash. If no operator key survives, re-running the bootstrap ceremony mints a fresh enrollment secret — a deliberate ceremony, not an everyday verb ([roles and keys](../architecture/roles-and-keys.md)). |

## `oml tenants` — the operator's verbs

The operator role exists to bootstrap tenants and nothing else: it never writes, reads or reviews
content ([roles and keys](../architecture/roles-and-keys.md)). The root operator key is claimed
show-once through `oml bootstrap claim` above, against the enrollment secret the first-start Helm
job mints ([deployment](../operations/deployment.md)); the
[bootstrap transcript](sessions.md#1-bootstrap-the-operator-key-and-the-first-tenant)
shows the whole ceremony.

| Verb | Does |
|---|---|
| `oml tenants create <name>` | Create a tenant and its permanent `universe` root workarea. |
| `oml tenants list` | Tenants and their chain-head seqs. No content crosses this surface. |
| `oml tenants issue-admin <tenant> [--label <text>]` | Issue the tenant's **first** admin key, show-once. Refused once the tenant has an admin: further keys come from that admin under the issuer ceiling. |

## `oml keys` — issuance, revocation, rotation

Key lifecycle rules — issuer ceiling, expiry, the bootstrap ceremony — are declared in
[roles and keys](../architecture/roles-and-keys.md). The CLI adds the **show-once ceremony**: a key
value is printed exactly once, at issuance, and the journal records the issuance
*without the value*. The server stores only the key hash; a lost value is unrecoverable by design, and the remedy is
revoke-and-reissue.

| Verb | Does |
|---|---|
| `oml keys issue --role <reader\|writer\|dreamer\|admin> --grant <workarea>:<role> [--grant …] [--default-workarea <node>] [--expires <duration>] [--label <text>]` | Issue a key. Refused when any requested role exceeds the issuer's (issuer ceiling), and when the key would combine the admin role with a dreamer-class grant (`access.role_exclusive` — the two-key rule: admin ≠ reviewer). `--default-workarea` records a journalled issuance attribute: the workarea that provider-ring (`/v1`) requests from this key target when they name none ([ADR-0022](../adr/ADR-0022-two-ring-api.md), d4). Prints the value once, then only the key id ever again. |
| `oml keys revoke <key-id>\|--matching <selector> [--reason <text>] [--apply]` | Journalled revocation, effective at the next permission-epoch check. `--matching <selector>` revokes a set and therefore defaults to `--dry-run`, requiring `--apply` to act. |
| `oml keys rotate <key-id> [--grace <duration>]` | Mint a successor key — new key id, linked by `rotated_from`, grants reproduced from the journalled grant records — and print the new value once. One journalled act (`key.rotated`) carries both key ids and the grace deadline; the old key stays usable through the bounded grace window and is revoked at the deadline (journalled). Rotation is per key for now; bulk crew rotation is open — [FR-6](../product/roadmap.md). |
| `oml keys list [--role …] [--expiring <duration>]` | Key ids, roles, labels, expiries. Never values. |
| `oml keys show <key-id>` | One key: grants, denials, epoch, issuance and revocation events. |

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
| `oml grants add <key-id> <workarea>:<role> [--deny <subtree> …]` | Add a grant, optionally carrying deny entries that mask subtrees within this grant only. A dreamer-class grant on an admin key is refused (`access.role_exclusive` — the two-key rule). Journalled; bumps the key's permission epoch. |
| `oml grants remove <key-id> <grant-id>` | Remove one grant. Journalled; epoch bump. |
| `oml grants list [--key <key-id>] [--workarea <path>]` | Grants as journalled, with the derived effective access per key. |
| `oml grants effective <key-id> [--at <workarea>]` | The resolved answer: what this key can do, where, and which deny produced each hole — reproducible from the journal by construction. |
| `oml deny set <key-id> <subtree> [--reason <text>]` | Create or extend the key's **global** deny object (deny-wins across all grants). Distinct from a per-grant `--deny`. |
| `oml deny lift <key-id> <deny-id>` | Journalled lift. |
| `oml bans add --principal <id>\|--source <selector> [--reason <text>] [--apply]` | Ban an author or source. Default `--dry-run` prints the reach preview: keys affected, held submissions that would be refused, workareas touched. `--apply` journals it (`ban.imposed`). |
| `oml bans lift <ban-id>` | Journalled lift; admission resumes at the next check. |
| `oml bans list` | Active and lifted bans with who imposed and who lifted each. |

## `oml workareas` — tree, edges, status

The algebra — one tree per tenant under `universe`, labelled `related` edges conferring zero
access, forward-only status ladder — is declared in [workareas](../architecture/workareas.md).
Two of these verbs are two-party ceremonies: a status transition and a workarea edge each need a
**proposer and a distinct ratifier** (R2 extended to edges and transitions), so the CLI splits
them into `propose` and `ratify` run under different keys.

| Verb | Does |
|---|---|
| `oml workareas create <parent> <name> [--crew-name …] [--charter-uri …] [--external-manager …] [--prefix <PREFIX>]` | Create a child workarea in `development`. Refused under a `validated` parent. `--prefix` registers a vocabulary workarea ([ADR-0021](../adr/ADR-0021-ai-human-ontologies.md)): the prefix is tenant-unique and **permanent** — the command states this and confirms before acting — and requires the crew flags (a vocabulary needs a named curator crew). |
| `oml workareas tree [--root <workarea>] [--status]` | The tenant tree from `universe` down, with status and crew per node. |
| `oml workareas show <workarea>` | One node: id, path (derived), status, crew metadata, edges, policy snapshot in force. |
| `oml workareas crew set <workarea> --crew-name … [--charter-uri …]` | Journalled crew-metadata update. Allowed on `validated` areas — freeze covers claims and promotions, not crew metadata. |
| `oml workareas relate propose <a> <b> --label <relates_to\|continues\|derived_from\|mirrors>` | Propose an edge. Requires dreamer(propose) on ≥1 endpoint and reader on both. |
| `oml workareas relate ratify <edge-id>` | Ratify under a different key with dreamer(review) on **both** endpoints; ratifier ≠ proposer, refused otherwise. |
| `oml workareas re-parent <workarea> --new-parent <target> [--apply]` | Journalled admin act; the server performs the single-transaction descendant path rewrite. Mass-reach act: defaults to `--dry-run` showing the paths that would be rewritten and the grants whose derived paths change; `--apply` performs the rewrite. |
| `oml workareas status propose <workarea> --to <committed\|validated>` | Propose the next forward rung; the proposal is itself journalled (`workarea.status_proposed`). The ladder has no skips: `--to validated` is proposable only from `committed`, and a rung-skipping or out-of-ladder proposal is refused as `workarea.transition_refused` — the same code that refuses proposing `validated` while any child is not `validated`. Backward transitions do not exist; there is no flag for them. |
| `oml workareas status ratify <transition-id>` | Second, non-proposing dreamer or admin ratifies; the journal records the transition with its policy snapshot. The [freeze transcript](sessions.md#4-freezing-a-workarea) shows the full ceremony. |
| `oml workareas status pending` | Transitions awaiting a ratifier. |

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
| `oml simulations open --audience <key-id>[,…] [--label <text>]` | Journalled act creating the simulation and its audience list. A simulation whose audience contains no dreamer key can promote nothing — the CLI warns, the server fails closed. |
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
| `oml audit verify-chain [--from <seq>] [--genesis]` | Recompute and verify the per-tenant hash chain and signatures from the local export or live from the server. Digest-only journal payloads mean verification survives crypto-shredding (D1). `--genesis` runs the full-history verification that the reconciler otherwise runs as an occasional named job ([reconciliation](../architecture/reconciliation.md)). |

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
| **Review availability** | Whether each workarea's review policy can currently be satisfied — at least one eligible non-author dreamer reachable. Two vocabulary refinements of the same signal ([ADR-0021](../adr/ADR-0021-ai-human-ontologies.md)): a vocabulary workarea with no active dreamer grant is flagged orphaned ("propose freeze" is the named next action), and a workarea whose policy names `human_required_classes` with no attested-human dreamer key in scope is a review-availability warning. | A workarea failing closed: writes held indefinitely because no eligible reviewer exists. |
| **Outbox frontier lag** | Journal head seq minus the lake writer's applied frontier, per tenant ([storage](../architecture/storage.md)). | A rising frontier lag with a healthy journal — the lake writer is down, split-brained-and-fenced, or stuck. |
| **Dead-letter count** | Rows the lake writer abandoned after its retry budget. Each is already a journalled `materialize-skipped` event and a named reconciler finding ([reconciliation](../architecture/reconciliation.md)). | Any nonzero count that is not shrinking; repair is a journalled re-enqueue, not a manual table edit. |

| Verb | Does |
|---|---|
| `oml status` | The four signals, one screen, plus chain-head seq and last reconciler run per tenant. |
| `oml doctor` | Each signal against its threshold with a verdict (`ok`/`warn`/`fail`) and the next action — the named command or the runbook anchor. Exit `1` on any `fail`, so probes and CI can consume it. |

## Verbs that do not exist

Absences are design, not gaps. No `oml promote` — promotion is only an effect of ratification
inside the review ceremony, and the gate cannot be bypassed from the admin seat (admin ≠ reviewer,
R1/R2). Demotion exists only as the journalled admin act `oml claims demote` above — it is
governance, never a verdict. No
`oml delete` — erasure is journalled crypto-shredding, demotion keeps bytes (R4). No backward
status transition. No verb reads another tenant. Where a workflow seems to need one of these, the
answer is a policy or roadmap question — see the [roadmap](../product/roadmap.md), not a flag.
