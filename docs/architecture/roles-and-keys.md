---
status: Stable
verified: 2026-11-06
answers: "which verbs each of the five roles may perform; how a key is issued, scoped and denied; how the reviewer is kept from being the author; what a connector key, a connect code and a connection are, and why a connection is never more than its key"
---

# Roles and keys

An agent holds a key with one role. Grants give that key access to workarea subtrees. OML works out what the key may do from its effective access at call time.

| I want to know | Section |
|---|---|
| What each role may do | [The five roles](#the-five-roles), [permission matrix](#permission-matrix) |
| Why a request was refused | [Refusal order](#refusal-order) |
| How a key is issued | [Issuing keys](#issuing-keys) |
| How a person's key differs | [User keys](#user-keys), [session keys](#session-keys) |
| How Claude's hosted apps connect | [Connections](#connections) |
| Why a reviewer cannot be the author | [Review independence](#review-independence) |

Related pages: [workareas](workareas.md) for grants and denials, [admission](admission.md) for the pipeline, and [audit and lineage](audit-and-lineage.md) for what the journal records.

Two rules shape everything below.

1. The surface an agent can call never approves, signs, demotes or deletes. No agent-facing role crosses that ceiling.
2. The reviewer is never the author. OML checks this when a memory is approved, against the author and against the memory's derivation lineage, not only when the key is issued.

## The five roles

| Role | What it does |
|---|---|
| `reader` | Recalls, reads, searches and reads the graph within its scope. OML logs every read. |
| `writer` | Writes memories, which are held on arrival, and proposes a new version of its own memories. Never gives a verdict. |
| `dreamer` | The reviewer key role. It proposes edits and connections to other keys' memories, gives verdicts, and ratifies connection proposals. |
| `admin` | Governs a tenant: keys, workareas, crews, grants and denials. Its verdicts on content are refused. |
| `operator` | Runs the provider level: creates tenants and issues each tenant's first admin key. Never writes, reads or reviews content. |

Roles do not imply one another. A key has one role, and the role names the grant roles the key may carry. A crew member who must both write and recall holds a writer key with a reader grant beside the writer grant. The role name alone never decides what a key may do. The grant on the workarea decides.

Humans hold the same roles through the same keys. No role is human-only or agent-only. The key attribute `held_by` records who holds a key: `agent`, `human` or `service`. The issuing admin attests it. The server records the attestation and cannot check whether a person is really a person. A `service` key is for infrastructure. OML refuses it every review verb, so it never counts as review capacity.

## Permission matrix

A dash means OML refuses the verb with a typed error. For content verbs, the column names the grant role that effective access must confer on the workarea. For key-level verbs, the column names the key's own role.

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

Approve, sign and delete appear in no column. Approval is an effect of review inside the admission pipeline. `oml-server` signs with the deployment key. Deletion does not exist.

Four rules qualify the matrix.

- **A memory you wrote is your own.** You can read your own memories in any state. Everyone else sees approved (state `promoted`) memories only.
- **An admin key reads workarea structure without grants.** It sees names, ids, parents, statuses, crews, children and edges for every workarea in its tenant. It never reads a memory's content.
- **An admin key carries no grant, so a denial has nothing to narrow.** A global denial on an admin key is refused, `request.validation` with `details.field: key`. OML therefore never accepts a denial and then ignores it.
- **Only a reader grant reads content, and no admin key can carry one.** An admin key that calls recall gets `access.verb_ceiling`.

One bounded exception exists. A development workarea can let a writer approve its own memories. OML marks them `self_signed`, serves them only inside that workarea, and never promotes them beyond it. [Workareas](workareas.md) describes it.

## Issuing keys

A key is issued with a role no higher than its issuer's. Only admin and operator keys can issue.

- A tenant admin issues reader, writer, dreamer and admin keys inside its tenant. No tenant key can mint an operator key.
- An operator key issues operator keys and each tenant's first admin key. It issues that admin key only for a tenant that has never held a key.

OML checks the ceiling at issuance and records it in the journal.

A key's role limits the grant roles it may carry:

| Key role | Grant roles it may carry |
|---|---|
| `reader` | `reader` |
| `writer` | `reader`, `writer` |
| `dreamer` | `reader`, `dreamer` |
| `admin`, `operator` | none |

A grant outside that set is refused with `access.issuer_ceiling`. An admin key that carries a dreamer grant is refused with `access.role_exclusive`. This is the two-key rule. A person who administers and also reviews holds two keys, so the journal shows which capacity acted.

Issuance runs its checks in a fixed order, and the first failure answers. Role exclusivity comes first. Then come the issuer ceiling, grant admissibility and node existence. Last, OML checks that the key's name is free (`name.taken`).

`oml keys issue` and `oml keys issue-agent` print the key value once. OML stores only a SHA-256 hash, and the journal records the issuance without the value. There is no "show me the key again". `oml keys show` displays a key's grants, denials, crew and connections, never its value.

Not available: revoking, rotating or expiring a key. To cut a key off today, add a global denial with `oml deny set`.

### Names, principals and labels

A key has three independent identifying fields.

| Field | What it is | Unique | Changes |
|---|---|---|---|
| `name` | What admin verbs address the key by, wherever an id works | yes, in its namespace | never |
| `principal` | Who acts. It appears in every journal envelope, and the independence check compares it. | no | never |
| `label` | A free-text note for the people who administer the key | no | never |

Two keys of one person share a principal. A key can also name a [crew](workareas.md) at issuance. The crew is fixed for the key's life, and the independence check reads it.

### Key format

An API key is the prefix `oml_` followed by 43 base62 characters, which encode 256 random bits. The value encodes no role and no tenant. Everything about a key is a row looked up by the hash of the value. The fixed prefix lets secret scanners find key material in submitted content, logs and repositories.

Clients send the key only in the `Authorization: Bearer` header, never in a URL or body. Log lines name keys by id, never by value. `oml keys issue-agent --out` writes the value to a new owner-only file (mode 0600, never overwritten). That file is the one place a key value rests outside the show-once response.

A plain SHA-256 hash is enough here. A password hash needs a salt because people pick guessable passwords. A 256-bit random key has no dictionary to attack. The plain hash also lets every request find its key with one index lookup.

## Refusal order

OML refuses a verb that needs a grant role on a workarea in this order:

1. The key's role cannot carry that grant role: `access.verb_ceiling`.
2. The needed role is in the key's effective access on the workarea: allowed.
3. The key can read the workarea but lacks the needed role: `access.grant_refused`, with the missing role named.
4. Otherwise: `access.not_found`.

Step 1 depends only on the key's role. It runs before OML resolves the workarea. Step 4 gives an unreadable workarea the same answer as one that does not exist. A key never learns the id of a node it holds nothing on.

OML works this out again on every request and caches nothing. The permission epoch labels the grant state each answer used. You can therefore tell later when a key could first or last do something.

Verbs map to roles as follows.

- **Writer:** write and supersede, on the node.
- **Reader:** recall, reading memories that are not your own, listing, the events tail and workarea info.
- **Dreamer:** review, edit proposals, connection and status-change proposals, and ratification. A dreamer edit also needs reader on the node.
- **Supersede:** a writer supersedes only its own memory. A readable memory by another key answers `access.author_only`. A dreamer may propose an edit to anyone's memory.

Effective access confers these verbs, as `my_effective_verbs` lists them:

| Effective grant role | Verbs |
|---|---|
| `reader` | `recall`, `read`, `graph_read`, `events_tail` |
| `writer` | `remember`, `learn`, `supersede` |
| `dreamer` | `propose`, `review`, `supersede` |

A `service` key's list omits `review`.

## Review independence

A key with the dreamer role can propose and review. OML does not split those at issuance. It limits which rows a reviewer may review, and it checks this when it approves.

**Lineage exclusion.** OML computes the memory's lineage closure. That is every memory it derives from, through declared `derives_from` edges and connection edges, whether the edge is pending or ratified. If the reviewing key or its principal appears anywhere in that closure, OML refuses the verdict with `review.author_in_lineage`. Editing an ancestor disqualifies a reviewer the same way writing the memory does. A reviewer cannot propose an edit and then approve it.

**Rival successors.** While several successors of one memory are held, OML also excludes each author from reviewing the others.

**The independence axis.** Reviewer and author must always have different principals; every workarea has the same review policy for a memory. Changing a workarea's status can require more: a `committed` workarea can require a reviewer crew or external manager that differs from the author's crew. OML uses the metadata as journaled at approval, so a later reshuffle does not change what was independent at the time. A verdict that cannot establish the requirement does not count, and the memory stays held.

**What this cannot do.** OML cannot see undeclared derivation. If a writer rebuilds someone's content without declaring `derives_from`, OML cannot know. Lineage exclusion is as good as the declared graph. OML also cannot prove that two distinct principals do not collude.

## Admin verbs

The admin governs and does not judge content. OML refuses admin verdicts on the server. Otherwise the authority that issued an author's key could act as reviewer by fiat.

- **Keys, grants and denials.** Issue keys, set global denials, mint connect codes and revoke connections.
- **Workareas.** Create workareas and propose or ratify status changes. An admin does not take part in edge proposals. Those need a dreamer key.
- **Crews.** Create crew records and attach them to workareas. OML records crews and does not operate them.

An admin who needs to write an ordinary memory uses a separate writer key. The write is held and reviewed like any other. An admin fixing a typo waits for a reviewer like everyone else. In return, the journal holds no unreviewed admin write.

Not available: bans, reassignment between workareas, and demotion and erasure verbs.

## The operator and the first admin key

The operator creates tenants, issues operator keys and issues each tenant's first admin key. It holds no grant inside any tenant.

A single-use enrollment token starts a new server. `oml bootstrap claim` exchanges it for the first key and stores the URL and the key in an owner-only file (mode 0600). The journal records the issuance without the key value.

From that first key, each link is a journaled event: tenant, then first admin key, then working keys. The provenance of any key therefore comes out of the journal.

## User keys

A key can carry a fourth field, `user`: the person an admin attests the key acts for. A key with `user` is a **user key**. A key without it is a **tenant key**.

- **Who may carry it.** Reader, writer and dreamer keys, including connector keys. Admin, operator and `service` keys cannot (`request.validation`, `details.field: user`). Admins set it explicitly. OML never derives it from the principal, name or label.
- **The first key naming a user creates that user's root,** `universe.<user>`.
- **Grants stay inside the user's root.** OML refuses any other grant with `access.issuer_ceiling`. Issuance adds one reader grant on `universe`, covering the node only. A user key therefore reads `universe`'s own memories and never another user's root.
- **Names resolve in the user's root.** A user key's bare workarea names resolve there. Tenant keys resolve the shared namespace and reach a user's workarea as `<user>.<name>`.
- **Writes into `universe` are refused** with `submission.universe_refused`. Only a curator writes there: a tenant writer key an admin granted writer on `universe`. A dreamer on `universe` outside the memory's lineage reviews each write.
- **Review exclusion stays per key and principal.** `user` is not part of it, so a person may review their own agents' writes through a separate dreamer key. The journal shows that both keys name the same user.

## Session keys

A **session key** has the credential `session`. It has no value and no connection. The server holds it for one person, and it acts only from that person's web session. Every request re-checks the session. You cannot send a session key as a bearer key, and it never gets a connect code.

A person holds up to four, one per capacity:

| Key | Role and grants |
|---|---|
| `<user>-web` | A user key: writer and reader on `universe.<user>`, plus the node read on `universe` |
| `<user>-review` | A dreamer key with dreamer and reader on `universe.<user>` |
| `<user>-curate` | A tenant writer key with writer on the `universe` node |
| `<user>-admin` | A tenant admin key with no grants |

OML creates `<user>-web` and `<user>-review` at the first sign-in. A person can then review their own agents' held writes on the user page without an admin step. Each session key's principal is its own name. The review check therefore lets `<user>-review` approve the person's agents' writes and never a version it proposed itself. `<user>-admin` reads every memory's text through the admin read, and the journal records each read. It still gives no verdicts.

## Connections

A **connection** is one OAuth client's standing authority to act as one connector key at `/mcp`. Claude's hosted apps cannot hold a key. A connection lets them use the [agent plugin](../api/mcp-plugin.md) without pasting a key value anywhere. The OAuth side is on the [HTTP API](../api/http-api.md) page. Connections need `oauth.issuer` set on the server.

**Connector keys.** `oml keys issue-agent … --connect-code` issues a connector key. The server mints no value for it, so it authenticates nothing by itself. Its one credential path is a connection, and a connection reaches only `/mcp`. A key is a bearer key or a connector key, never both. A connector key has at most one open connection. Opening a new one revokes the earlier one.

**Connect codes.** A connect code is twelve symbols from Crockford's base32 alphabet, 60 bits from a secure generator, shown as `7KQ2-M9XD-4TPA`. OML shows it once. It appears in no log, URL or journal payload, and the database holds only a digest. It lives 10 minutes. Its first correct entry on the authorization page spends it. A newer code, a revoked connection or a revoked key voids it. The chance that any wrong entry hits a live code is at most L·G / 2⁶⁰, where L is the number of live codes and G the number of wrong entries during their life.

**Tokens.** An access token (`oml_at_` plus 43 base62 characters) lives 1 hour. A refresh token (`oml_rt_` plus 43 characters) expires after 30 days unused. The database stores only digests. Each refresh issues a new pair and marks the old refresh token rotated. If a rotated refresh token appears again, OML revokes the connection and every token it holds. Tokens travel only in the `Authorization` header.

**A token is no more than its key.** At `/mcp`, an access token resolves to its connection, then to the connection's key. The key must be neither revoked nor expired. From there the request is the key's request. The verb ceiling, grants, denials, own-memory rules and the permission epoch all apply on every request. OML accepts a token nowhere but `/mcp`, so a connection can do at most what its key can.

A connection is live while it is open, its key is usable and its newest refresh token has not expired. A connection that stops being live stays `open` in the table without a journal event, because no actor ended it. A revoked connection never reopens. The key needs a new code.

**Ending a connection.** `oml keys disconnect <key> <connection-id>` ends one connection. The server refuses its tokens at the next request. `oml keys show` lists a key's connections.

## Alternatives considered

Letting an admin approve content was rejected, because the authority that issues keys would then also decide what is true. Deriving independence from a single principal check was rejected, because one person can hold many keys. A salted, slow hash for keys was rejected, because the keys are random and the hash sits on every request path.

## Costs

The two-key rule means a person who administers and reviews carries two keys. Writes wait for a reviewer, and an admin is no exception. Lineage exclusion only knows what writers declare. Nothing stops two distinct principals from approving each other's work. We have not measured how often that happens in practice.
