---
status: Stable
verified: 2026-11-06
answers: "which verbs each of the five roles may perform; how a key is issued, scoped and denied; how the reviewer is kept from being the author; what a connector key, a connect code and a connection are, and why a connection is never more than its key"
---

# Roles and keys

An agent holds a key with one role. Grants give that key access to workarea subtrees. OML works out what the key may do from its effective access at call time.

Two rules shape everything below.

1. An agent never approves, signs, demotes or deletes.
2. The reviewer is never the author. OML checks this when a memory is approved, against the author and the memory's derivation lineage.

## The five roles

| Role | What it does |
|---|---|
| `reader` | Recalls, reads, searches and reads the graph within its scope. Every read is logged. |
| `writer` | Writes memories, held on arrival, and proposes a new version of its own. Never gives a verdict. |
| `dreamer` | The reviewer role. Proposes edits and connections to other keys' memories, gives verdicts, ratifies connection proposals. |
| `admin` | Governs a tenant: keys, workareas, crews, grants and denials. Its verdicts on content are refused. |
| `operator` | Runs the provider level: creates tenants and issues each tenant's first admin key. Never writes, reads or reviews content. |

Roles do not imply one another: a crew member who writes and recalls holds a writer key with a reader grant beside the writer grant. The grant decides, not the role name.

`held_by` records who holds a key: `agent`, `human` or `service`. The issuing admin attests it; the server cannot check. A `service` key is for infrastructure and is refused every review verb.

## Permission matrix

A dash means OML refuses the verb with a typed error. For content verbs, the column names the grant role effective access must confer on the workarea. For key-level verbs, it names the key's own role.

| Verb | reader | writer | dreamer | admin | operator |
|---|---|---|---|---|---|
| Recall, search, read, graph read, events tail | yes | — | — | — | — |
| Workarea info, tree and edges | yes | — | — | yes (see below) | — |
| Own-work status | yes | yes | yes | yes | — |
| Remember, learn | — | yes | — | — | — |
| Propose a new version of your own memory | — | yes | — | — | — |
| Edit another key's memory (as a proposal) | — | — | yes | — | — |
| Propose a connection edge | — | — | yes | — | — |
| Verdict on a held memory | — | — | yes | refused | — |
| Ratify a connection proposal | — | — | yes | — | — |
| Propose or ratify a workarea status change | — | — | yes | yes | — |
| Issue keys, set grants and denials | — | — | — | yes | yes (operator and first admin keys only) |
| Mint a connect code, revoke a connection | — | — | — | yes | — |
| Create workareas and crews | — | writer in its own user root only | — | yes | — |
| Create a tenant, issue its first admin key | — | — | — | — | yes |
| Read, write or review content | — | — | — | — | refused |

Approve, sign and delete appear in no column: approval is an effect of review, `oml-server` signs, and deletion does not exist.

- **A memory you wrote is your own.** A bearer key reads its own memories in any state. The signed-in user page also reads every key belonging to that user (ADR-0045 d16). Everyone else sees `promoted` memories only.
- **An admin key reads workarea structure without grants**: names, ids, parents, statuses, crews, children and edges in its tenant. Never a memory's content.
- **A global denial on an admin key is refused, `request.validation` with `details.field: key`.**
- **Only a reader grant reads content, and no admin key can carry one.** An admin key that calls recall gets `access.verb_ceiling`.

A development workarea can let a writer approve its own memories, marked `self_signed` ([workareas](workareas.md)).

## Issuing keys

A key is issued with a role no higher than its issuer's. Only admin and operator keys issue.

- A tenant admin issues reader, writer, dreamer and admin keys inside its tenant. No tenant key can mint an operator key.
- An operator key issues operator keys and each tenant's first admin key, only for a tenant that has never held a key.

| Key role | Grant roles it may carry |
|---|---|
| `reader` | `reader` |
| `writer` | `reader`, `writer` |
| `dreamer` | `reader`, `dreamer`; an agent-held `curator` profile may also carry `writer` inside an in-progress scope (ADR-0030 d8) |
| `admin`, `operator` | none |

A grant outside that set is refused with `access.issuer_ceiling`. An admin key with a dreamer grant is refused with `access.role_exclusive`: the two-key rule. A person who administers and reviews holds two keys, so the journal shows which capacity acted.

Checks run in this order, first failure answers: role exclusivity, issuer ceiling, grant admissibility, node existence, name free (`name.taken`).

`oml keys issue` and `oml keys issue-agent` print the key value once. OML stores a SHA-256 hash only, so there is no "show me the key again". `oml keys show` displays grants, denials, crew and connections, never the value.

Not available: revoking or rotating a key. To cut a key off today, add a global denial with `oml deny set`.

### Expiry and rate

A reader, writer or dreamer key can carry two limits, each set when it is issued (`--expires`, `--requests-per-minute`) and neither required:

- **An expiry**, an instant. From it on the key authenticates nothing: its request answers `auth.invalid` with `details.reason: expired`, where an unknown or revoked key answers `unknown`. Only the holder of the key's own credential is told. The key's row, grants and name stay, and `oml keys show` says it has expired. A connector key's expiry is its connection's: the access token is refused the same way, a refresh answers `invalid_grant`, and no connect code is minted for it.
- **A rate**, in requests per UTC minute. The request past it answers `429 limit.rate` with `Retry-After`. A key of an account under a quota is held to both rates. Each server process counts alone, so n processes admit up to n times the rate ([settings](../operations/configuration.md#limits-counted-per-process)).

`oml keys limit <key>` changes or clears either, journalled as `key.limits_set`. It takes an admin key that is not the key itself: no agent key reaches the route, and an admin key naming itself is refused, so no key extends its own date or raises its own rate. Moving an expired key's date later makes it work again. A session key takes neither limit, and a revoked key none.

An admin key carries neither limit: issuing one with an expiry or a rate is refused, and so is `oml keys limit` naming one, each as `request.validation` with `details.reason: admin_key`. The admin key role is the one that changes limits, so no key that can change a limit carries one: a limited key cannot lift its own through a second admin key, and no admin key ends another by a date. The cost is that no admin key is temporary ([ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md)).

### Names, principals and labels

A key has a `name` (unique in its namespace; admin verbs address it wherever an id works), a `principal` (who acts; in every journal envelope; the independence check compares it; two keys of one person share it) and a free-text `label`. None changes after issuance. A key can name a [crew](workareas.md) at issuance, fixed for its life and read by the independence check.

### Key format

An API key is `oml_` plus 43 base62 characters (256 random bits). The value encodes no role or tenant; a row looked up by its hash holds everything. The fixed prefix lets secret scanners find keys.

Send it only in the `Authorization: Bearer` header. `oml keys issue-agent --out` writes the value to a new owner-only file (mode 0600, never overwritten).

## Refusal order

For a verb that needs a grant role on a workarea:

1. The key's role cannot carry that grant role: `access.verb_ceiling`. This depends only on the role and runs before the workarea is resolved.
2. The needed role is in the key's effective access: allowed.
3. The key can read the workarea but lacks the needed role: `access.grant_refused`, naming the missing role.
4. Otherwise `access.not_found`, the same answer as for a workarea that does not exist.

OML works this out again on every request and caches nothing. The permission epoch labels the grant state each answer used.

A writer supersedes only its own memory; another key's readable memory answers `access.author_only`. A dreamer edit also needs reader on the node.

| Effective grant role | Verbs (`my_effective_verbs`) |
|---|---|
| `reader` | `recall`, `read`, `graph_read`, `events_tail` |
| `writer` | `remember`, `learn`, `supersede` |
| `dreamer` | `propose`, `review`, `supersede` |

A `service` key omits `review`.

## Review independence

OML limits which rows a dreamer may review, and checks it when approving.

- **Lineage exclusion.** If the reviewing key or its principal appears in the memory's lineage closure (every memory it derives from, through declared `derives_from` and connection edges, pending or ratified), the verdict is refused with `review.author_in_lineage`. A reviewer cannot propose an edit and then approve it.
- **Rival successors.** While several successors of one memory are held, each author is excluded from reviewing the others.
- **Different principals.** A `committed` workarea can require more: a reviewer crew or external manager that differs from the author's crew. OML uses the metadata journaled at approval. A verdict that cannot establish the requirement does not count, and the memory stays held.

OML cannot see undeclared derivation or prove two principals do not collude.

## Admin limits

An admin who needs to write uses a separate writer key; the write is held and reviewed like any other. Not available: bans, reassignment between workareas, demotion and erasure verbs.

## The operator and the first admin key

The operator creates tenants, issues operator keys and each tenant's first admin key, and holds no grant inside any tenant. A single-use enrollment token starts a new server. `oml bootstrap claim` exchanges it for the first key and stores the URL and key in an owner-only file (mode 0600).

## User keys

A key with the field `user` (the person an admin attests it acts for) is a **user key**; one without is a **tenant key**.

- **Who may carry it:** reader, writer and dreamer keys, connector keys included. Admin, operator and `service` keys cannot. OML never derives it from principal, name or label.
- The first key naming a user creates that user's root, `universe.<user>`.
- **Grants stay inside the user's root.** Any other grant is refused with `access.issuer_ceiling`. Issuance adds one reader grant on the `universe` node only.
- A user key's bare workarea names resolve in its root; tenant keys use `<user>.<name>`.
- **Writes into `universe` are refused** (`submission.universe_refused`). Only a curator writes there: a tenant writer key granted writer on `universe`, reviewed by a dreamer outside the memory's lineage.
- **A user's root takes memories from that user's keys only.** A tenant key's new memory, successor or retirement in `universe.<user>` or below it is refused with `submission.user_root_refused`, whatever writer or dreamer grant it holds there. Its reads, recall and verdicts there are unchanged, and `my_effective_verbs` still lists what its grants confer. A key that cannot read the workarea gets `access.not_found`.

## Session keys

A **session key** has the credential `session`: no value, no connection. It acts only from one person's web session, which every request re-checks. It cannot be sent as a bearer key and never gets a connect code. A person holds up to four:

| Key | Role and grants |
|---|---|
| `<user>-web` | User key: writer and reader on `universe.<user>`, node read on `universe` |
| `<user>-review` | Dreamer key: dreamer and reader on `universe.<user>` |
| `<user>-curate` | Tenant writer key: writer on the `universe` node |
| `<user>-admin` | Tenant admin key, no grants |

`<user>-web` and `<user>-review` are created at first sign-in. Each session key's principal is its own name. `<user>-admin` reads memory text through the journaled admin read and gives no verdicts.

A session key that carries a denial, on any workarea, runs none of its key acts: a denied `<user>-web` issues no key, mints no connect code and cuts no key off. Each act answers `access.verb_ceiling` with `details.reason: session_key_denied` and journals nothing. The key it would issue holds the person's whole root, which a key denied inside that root or on `universe` no longer does; a denial on any other workarea masks nothing the key reads and refuses the three acts all the same. The refusal is permanent: no route lifts a denial, and every sign-in finds the same `<user>-web`, also after the person's entry is removed and added back. The session still opens the pages, lists the person's keys and ends by sign-out.

## Connections

A **connection** is one OAuth client's standing authority to act as one connector key at `/mcp`, so Claude's hosted apps use the [agent plugin](../api/mcp-plugin.md) without a pasted key. Connections need `oauth.issuer` set on the server.

- **Connector keys.** `oml keys issue-agent … --connect-code` issues one. It has no value and authenticates only through a connection, which reaches only `/mcp`. It has at most one open connection; opening a new one revokes the earlier.
- **Connect codes.** Twelve Crockford base32 symbols (60 bits), shown once as `7KQ2-M9XD-4TPA`, never in a log, URL or journal payload. Lives 10 minutes, or the `--ttl` an admin asks for, up to 30 days, for a connector key confined to one workarea ([ADR-0067](../adr/ADR-0067-a-directory-reviewer-reaches-a-demo-workarea-through-a-connect-code-that-lives-longer-and-the-tools-and-instructions-describe-instead-of-command.md) d3); the first correct entry spends it. A newer code, a revoked connection or a revoked key voids it.
- **Tokens.** Access token `oml_at_` plus 43 characters, 1 hour. Refresh token `oml_rt_` plus 43 characters, 30 days unused. Only digests are stored. Each refresh rotates the pair; a rotated refresh token presented again revokes the connection and all its tokens. Tokens travel only in the `Authorization` header and work only at `/mcp`.
- **A token is no more than its key.** Verb ceiling, grants, denials, own-memory rules and the permission epoch apply on every request.

A revoked connection never reopens. `oml keys disconnect <key> <connection-id>` ends one; `oml keys show` lists them.

Design record: [ADR-0010](../adr/ADR-0010-roles-and-keys.md), [ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md), [ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md), [ADR-0044](../adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md), [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md).
