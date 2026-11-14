---
status: Stable
verified: 2026-11-14
answers: "which verbs each of the five roles may perform; how a key is issued, scoped and denied; how the reviewer is kept from being the author; what a connector key, a connect code and a connection are, and why a connection is never more than its key"
---

# Roles and keys

An agent holds a key with one role. Grants give that key access to workarea subtrees. OML works out what the key may do from its effective access at call time. The [serving view](views.md#logical-one-agent-key-and-everything-it-is-served) draws one key's reach, and [producers and reviewers](../concepts/producers-and-reviewers.md) lists which reviewer pairs count.

The four rules, R1 to R4, are declared once, in [the constitution](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md). This page holds the role matrix that R1's verb ceiling refers to and the independence check that enforces R2.

## The five roles

| Role | What it does |
|---|---|
| `reader` | Recalls, reads, searches and reads the graph within its scope. Each recall is recorded in recall telemetry for 30 days. |
| `writer` | Writes memories, held on arrival, and proposes a new version of its own. Never gives a verdict. |
| `dreamer` | The reviewer role. Proposes edits and connections to other keys' memories, gives verdicts, ratifies connection proposals. |
| `admin` | Governs a tenant: keys, workareas, crews, grants and denials. Its verdicts on content are refused. |
| `operator` | Designed and not built: would run the provider level, create tenants and issue each tenant's first admin key, and never write, read or review content. No path issues an operator key. |

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
| Propose a new version of the key's own memory | — | yes | — | — | — |
| Edit another key's memory (as a proposal) | — | — | yes | — | — |
| Propose a connection edge | — | — | yes | — | — |
| Verdict on a held memory | — | — | yes | refused | — |
| Ratify a connection proposal | — | — | yes | — | — |
| Propose or ratify a workarea status change | — | — | yes | yes | — |
| Issue keys, set grants and denials | — | `<user>-web` only, see below | — | yes | designed, not built |
| Mint a connect code, revoke a connection | — | `<user>-web` only, see below | — | yes | — |
| Create workareas | — | writer in its own user root only | — | yes | — |
| Create crews | — | — | — | yes | — |
| Create a tenant, issue its first admin key | — | — | — | — | designed, not built |
| Read any memory's text and attribution ([admin read](#the-admin-read)) | — | — | — | yes, journalled | — |
| Read, write or review content | — | — | — | — | refused |

The `<user>-web` [session key](#session-keys), a writer key, also issues reader and writer keys in its person's root, mints their connect codes and cuts them off with a denial.

Approve, sign and delete appear in no column: approval is an effect of review, `oml-server` signs, and deletion does not exist.

- **A memory written by a key is that key's own.** A bearer key reads its own memories in any state. The signed-in user page also reads every key belonging to that user (ADR-0045 d16). Other keys read `promoted` memories. A reviewer key is served the held memory it judges, and a person's reviewer key lists the held memories it may judge, with their text (`GET /v2/review/held`). A reader inside an [in-progress workarea](workareas.md#in-progress-workareas) recalls held memories and derived facts there, marked unreviewed, except those the scan held or last scanned under an older rule set. Also, an admin of the tenant reads any memory through the journaled [admin read](#the-admin-read) ([ADR-0045](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d21).
- **An admin key reads workarea structure without grants**: names, ids, parents, statuses, crews, children and edges in its tenant. It reads a memory's content only through the [admin read](#the-admin-read).
- **A global denial on an admin key is refused, `request.validation` with `details.field: key`.**
- **Recall, search and the read by id need a reader grant, and no admin key can carry one.** An admin key that calls recall gets `access.verb_ceiling`.

A development workarea letting a writer approve its own memories, marked `self_signed`, is designed ([ADR-0001](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md) d5) and not built: the server marks no approval `self_signed`.

## Issuing keys

A key is issued with a role no higher than its issuer's. Admin keys issue, and the `<user>-web` session key issues reader and writer keys in its person's root.

- A tenant admin issues reader, writer, dreamer and admin keys inside its own tenant. No tenant key can mint an operator key.
- The operator tier is designed and not built: no route creates a tenant and no path issues an operator key.

| Key role | Grant roles it may carry |
|---|---|
| `reader` | `reader` |
| `writer` | `reader`, `writer` |
| `dreamer` | `reader`, `dreamer`; an agent-held `curator` profile may also carry `writer` inside an in-progress scope (ADR-0030 d8) |
| `admin`, `operator` | none |

A grant outside that set is refused with `access.issuer_ceiling`. An admin key with a dreamer grant is refused with `access.role_exclusive`: the two-key rule. A person who administers and reviews holds two keys, so the journal shows which capacity acted.

After the request's names resolve and its limits are checked, checks run in this order, first failure answers: role exclusivity, issuer ceiling, grant admissibility, node and crew existence, deny containment, coverage of the default workarea, connection eligibility (with `--connect-code`), name free (`name.taken`).

`oml keys issue` and `oml keys issue-agent` print the key value once. OML stores a SHA-256 hash only, so there is no "show me the key again". `oml keys show` displays grants, denials, crew and connections, never the value.

Not available: revoking or rotating a key. To cut a key off today, add a global denial with `oml deny set`.

### Expiry and rate

A reader, writer or dreamer key can carry two limits, each set when it is issued (`--expires`, `--requests-per-minute`) and neither required:

- **An expiry**, an instant. From it on the key authenticates nothing: its request answers `auth.invalid` with `details.reason: expired`, where an unknown or revoked key answers `unknown`. Only the holder of the key's own credential is told. The key's row, grants and name stay, and `oml keys show` says it has expired. A connector key's expiry is its connection's: the access token is refused the same way, a refresh answers `invalid_grant`, and no connect code is minted for it.
- **A rate**, in requests per UTC minute. The request past it answers `429 limit.rate` with `Retry-After`. A key of an account under a quota is held to both rates. Each server process counts alone, so n processes admit up to n times the rate ([settings](../operations/configuration.md#limits-counted-per-process)).

`oml keys limit <key>` changes or clears either, journalled as `key.limits_set`. It takes an admin key that is not the key itself: no agent key reaches the route, and an admin key naming itself is refused, so no key extends its own date or raises its own rate. Moving an expired key's date later makes it work again. A session key takes neither limit, and a revoked key none. A person issuing their own key on the web console's Keys page may give it an expiry, a day that ends in UTC, and no rate; changing that date afterwards takes an admin key like any other.

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
| `writer` | `remember`, `learn`, `attach`, `supersede` |
| `dreamer` | `propose`, `review`, `supersede` |

A `service` key omits `review`.

## Review independence

OML limits which rows a dreamer may review, and checks it when approving.

- **Lineage exclusion.** If the reviewing key or its principal appears in the memory's lineage closure (every memory it derives from, through declared `derives_from` and connection edges, pending or ratified), the verdict is refused with `review.author_in_lineage`. A reviewer cannot propose an edit and then approve it.
- **Rival successors.** While several successors of one memory are held, each author is excluded from reviewing the others.
- **Different principals.** A requirement that the reviewer's crew or external manager differ from the author's is designed and not built: every workarea takes one dreamer verdict from outside the memory's lineage.

OML cannot see undeclared derivation or prove two principals do not collude.

## The admin read

An admin key reads any memory of its tenant with its text and attribution: every state, held included, in every workarea, user root and `universe`. The routes are `POST /v2/admin/memories/search` and `GET /v2/admin/memories/{artifact_id}`; `oml admin read` and the admin page's Memories and Atlas views call them. The single read shows every verdict with its reviewing key and principal. Each read is journaled as `memory.admin_read`, with the ids and a digest of the query, never the text ([event catalog](../data/event-catalog.md)). No other role reaches the routes, the operator included. So a tenant admin can read every person's memories before review; [security](security.md) lists that as a residual risk ([ADR-0045](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d21).

## Admin limits

An admin key gives no verdict and writes no memory. An admin who needs to write uses a separate writer key; the write is held and reviewed like any other.

Demotion ([ADR-0010](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0010-roles-and-keys.md) d6) and erasure by crypto-shredding ([ADR-0005](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0005-erasure-by-crypto-shredding.md)) are designed and not built: no key can demote or erase a memory in this build. A memory leaves recall only by an approved [retirement](audit-and-lineage.md#which-version-is-current), which keeps every version. Bans and reassignment between workareas are not built either.

## The operator and the first admin key

The operator tier is designed and not built. A single-use enrollment token starts a new server: `oml bootstrap claim` exchanges it for the first admin key of the server's tenant and stores the URL and key in an owner-only file (mode 0600). No route creates a further tenant.

## User keys

A key with the field `user` (the person an admin attests it acts for) is a **user key**; one without is a **tenant key**.

- **Who may carry it:** reader, writer and dreamer keys, connector keys included. Admin, operator and `service` keys cannot. OML never derives it from principal, name or label.
- The first key naming a user creates that user's root, `universe.<user>`.
- **Grants stay inside the user's root.** Any other grant is refused with `access.issuer_ceiling`. Issuance adds one reader grant on the `universe` node only.
- A user key's bare workarea names resolve in its root; tenant keys use `<user>.<name>`.
- **Writes into `universe` are refused** (`submission.universe_refused`). Only a curator writes there: a tenant writer key granted writer on `universe`, reviewed by a dreamer outside the memory's lineage.
- **A memory is started in a user's root by that user's keys only.** Any other key's new memory in `universe.<user>` or below it is refused with `submission.user_root_refused`, whatever grant it holds there. So is its successor of a ratified retirement, which would put the claim back in service. A person's keys are their user keys and the [session keys](#session-keys) that serve them: `<user>-review` and `<user>-curate` carry no `user` field and are read as that person's by their session, in that person's root and in no one else's. A reviewer granted there still proposes a successor or a retirement of a served memory, and its reads, recall and verdicts are unchanged. `my_effective_verbs` still lists what its grants confer. A key that cannot read the workarea gets `access.not_found`.

## Session keys

A **session key** has the credential `session`: no value, no connection. It acts only from one person's web session, which every request re-checks. It cannot be sent as a bearer key and never gets a connect code. A person holds up to four:

| Key | Role and grants |
|---|---|
| `<user>-web` | User key: writer and reader on `universe.<user>`, node read on `universe` |
| `<user>-review` | Dreamer key: dreamer and reader on `universe.<user>` |
| `<user>-curate` | Tenant writer key: writer on the `universe` node alone, and reader on each workarea it is designated to curate from |
| `<user>-admin` | Tenant admin key, no grants |

`<user>-web` and `<user>-review` are created at first sign-in, and `<user>-admin` too for an admin entry; `<user>-curate` is created when an admin designates the person a curator. Each session key's principal is its own name. `<user>-admin` reads memory text through the journaled [admin read](#the-admin-read) and gives no verdicts.

A session key that carries a denial, on any workarea, runs none of its key acts: a denied `<user>-web` issues no key, mints no connect code and cuts no key off. Each act answers `access.verb_ceiling` with `details.reason: session_key_denied` and journals nothing. The key it would issue holds the person's whole root, which a key denied inside that root or on `universe` no longer does; a denial on any other workarea masks nothing the key reads and refuses the three acts all the same. The refusal is permanent: no route lifts a denial, and every sign-in finds the same `<user>-web`, also after the person's entry is removed and added back. The session still opens the pages, lists the person's keys, downloads their export, files their deletion request and ends by sign-out; the Keys and Start pages draw none of the three forms and say so, and `GET /v2/whoami` tells `<user>-web`, and no other key, whether it carries a denial (`denied`, a boolean).

## Connections

A **connection** is one OAuth client's standing authority to act as one connector key at `/mcp`, so Claude's hosted apps use the [agent plugin](../api/mcp-plugin.md) without a pasted key. Connections need `oauth.issuer` set on the server.

- **Connector keys.** `oml keys issue-agent … --connect-code` issues one. It has no value and authenticates only through a connection, which reaches only `/mcp`. It has at most one open connection; opening a new one revokes the earlier.
- **Connect codes.** Twelve Crockford base32 symbols (60 bits), shown once as `7KQ2-M9XD-4TPA`, never in a log, URL or journal payload. Lives 10 minutes, or the `--ttl` an admin asks for, up to 30 days, for a connector key confined to one workarea ([ADR-0067](../adr/ADR-0067-a-directory-reviewer-reaches-a-demo-workarea-through-a-connect-code-that-lives-longer-and-the-tools-and-instructions-describe-instead-of-command.md) d3); the first correct entry spends it. A newer code, a revoked connection or a revoked key voids it.
- **Tokens.** Access token `oml_at_` plus 43 characters, 1 hour. Refresh token `oml_rt_` plus 43 characters, 30 days unused. Only digests are stored. Each refresh rotates the pair; a rotated refresh token presented again revokes the connection and all its tokens. Tokens travel only in the `Authorization` header and work only at `/mcp`.
- **A token is no more than its key.** Verb ceiling, grants, denials, own-memory rules and the permission epoch apply on every request.

A revoked connection never reopens. `oml keys disconnect <key> <connection-id>` ends one; `oml keys show` lists them.

Design record: [ADR-0010](../adr/ADR-0010-roles-and-keys.md), [ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md), [ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md), [ADR-0044](../adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md), [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md).
