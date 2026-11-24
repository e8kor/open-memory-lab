---
status: Stable
verified: 2026-11-14
answers: "which verbs each of the six roles may perform; how a key is issued, scoped and denied; how the reviewer is kept from being the author; what a connector key, a connect code and a connection are, and why a connection is never more than its key"
---

# Roles and keys

An agent holds a key with one role. Grants give that key access to workspace subtrees. OML works out what the key may do from its effective access at call time. The [serving view](views.md#logical-one-agent-key-and-everything-it-is-served) draws one key's reach, and [producers and reviewers](../concepts/producers-and-reviewers.md) lists which reviewer pairs count.

The four rules, R1 to R4, are declared once, in [the constitution](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md). This page holds the role matrix that R1's verb ceiling refers to and the independence check that enforces R2.

## The six roles

| Role | What it does |
|---|---|
| `reader` | Recalls, reads, searches and reads the graph within its scope. Each recall is recorded in recall telemetry for 30 days. |
| `contributor` | Recalls and writes: carries at least one `reader` and one `submitter` grant. Writes memories, held on arrival, and proposes a new version of its own. Never gives a verdict. Called `writer` before v0.8.4. |
| `submitter` | Writes without recall: carries `submitter` grants only. Reads its own memories in every state and recalls nothing else. Never gives a verdict. |
| `reviewer` | Proposes edits and connections and reviews other identities' writes, never on a lineage it proposed in: gives verdicts and ratifies connection proposals. Called `dreamer` before v0.8.3. |
| `admin` | Governs a tenant: keys, workspaces, crews, grants and denials. Its verdicts on content are refused. |
| `operator` | Designed and not built: would run the provider level, create tenants and issue each tenant's first admin key, and never write, read or review content. No path issues an operator key. |

Roles do not imply one another: a crew member who writes and recalls holds a contributor key, whose recall is a `reader` grant beside its `submitter` grant in the journal, never implied by the role. The grant roles are `reader`, `submitter` and `reviewer`; `contributor` is a key role only. A contributor key holding a submitter grant on a node that no reader grant of it reaches, such as a key issued before v0.8.4 as a write-only writer, is a contributor without recall: it works as before, `oml doctor` names it (`key_contributor_without_recall`) and the console marks it "no recall"; its holder replaces it with a submitter key ([ADR-0093](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0093-contributor-and-submitter-replace-writer.md) d3).

`used_by` records who holds a key: `agent`, `person` or `service`. The issuing admin attests it; the server cannot check. A `service` key is for infrastructure and is refused every review verb.

## Permission matrix

A dash means OML refuses the verb with a typed error. For content verbs, the column names the grant role effective access must confer on the workspace. For key-level verbs, it names the key's own role.

| Verb | reader | submitter | reviewer | admin | operator |
|---|---|---|---|---|---|
| Recall, search, read, graph read, events tail | yes | — | — | — | — |
| Workspace info, tree and edges | yes | — | — | yes (see below) | — |
| Own-work status | yes | yes | yes | yes | — |
| Remember, learn | — | yes | — | — | — |
| Propose a new version of the key's own memory | — | yes | — | — | — |
| Edit another key's memory (as a proposal) | — | — | yes | — | — |
| Propose a connection edge | — | — | yes | — | — |
| Verdict on a held memory | — | — | yes | refused | — |
| Ratify a connection proposal | — | — | yes | — | — |
| Propose or ratify a workspace status change | — | — | yes | yes | — |
| Issue keys, set grants and denials | — | `<user>-web` only, see below | — | yes | designed, not built |
| Mint a connect code, revoke a connection | — | `<user>-web` only, see below | — | yes | — |
| Create workspaces | — | a contributor key in its own user root only | — | yes | — |
| Create crews | — | — | — | yes | — |
| Create a tenant, issue its first admin key | — | — | — | — | designed, not built |
| Read any memory's text and attribution ([admin read](#the-admin-read)) | — | — | — | yes, journalled | — |
| Read, write or review content | — | — | — | — | refused |

The `<user>-web` [session key](#session-keys), a contributor key, also issues contributor and reader keys in its person's root, mints their connect codes and cuts them off with a denial.

Approve, sign and delete appear in no column: approval is an effect of review, `oml-server` signs, and deletion does not exist.

- **A memory written by a key is that key's own.** A bearer key reads its own memories in any state. The signed-in user page also reads every key belonging to that user (ADR-0045 d16). Other keys read `promoted` memories. A reviewer key is served the held memory it judges, and a person's reviewer key lists the held memories it may judge, with their text (`GET /v2/review/held`). A reader inside an [in-progress workspace](workspaces.md#in-progress-workspaces) recalls held memories and derived facts there, marked unreviewed, except those the scan held or last scanned under an older rule set. Also, an admin of the tenant reads any memory through the journaled [admin read](#the-admin-read) ([ADR-0045](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d21).
- **An admin key reads workspace structure without grants**: names, ids, parents, statuses, crews, children and edges in its tenant. It reads a memory's content only through the [admin read](#the-admin-read).
- **A global denial on an admin key is refused, `request.validation` with `details.field: key`.**
- **Recall, search and the read by id need a reader grant, and no admin key can carry one.** An admin key that calls recall gets `access.verb_ceiling`.

Self-signing was retired on 2026-11-14 ([ADR-0001](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md) d5). In a person's own root, their review key approves their agents' work, marked `same_user`.

## Issuing keys

A key is issued with a role no higher than its issuer's. Admin keys issue, and the `<user>-web` session key issues contributor and reader keys in its person's root.

- A tenant admin issues reader, contributor, submitter, reviewer and admin keys inside its own tenant. No tenant key can mint an operator key.
- The operator tier is designed and not built: no route creates a tenant and no path issues an operator key.

| Key role | Grant roles it may carry |
|---|---|
| `reader` | `reader` |
| `contributor` | `reader`, `submitter`: at least one of each |
| `submitter` | `submitter` |
| `reviewer` | `reader`, `reviewer`; an agent-held `curator` profile may also carry `submitter` inside an in-progress scope ([ADR-0030](../adr/ADR-0030-an-in-progress-workspace-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md) d8) |
| `admin`, `operator` | none |

A grant outside that set is refused with `access.issuer_ceiling`. A contributor key without a reader or without a submitter grant is refused with `request.validation`, `details.field: /grants`, `details.reason: contributor_grants`, and nothing is journalled: the server takes exactly the grants it is sent and never adds the missing one. Grants added to an issued key are held to the same set. An admin key with a reviewer grant is refused with `access.role_exclusive`: the two-key rule. A person who administers and reviews holds two keys, so the journal shows which capacity acted.

After the request's names resolve and its limits are checked, checks run in this order, first failure answers: role exclusivity, issuer ceiling, grant admissibility, node and crew existence, deny containment, coverage of the default workspace, a contributor's reader and submitter grants, connection eligibility (with `--connect-code`), name free (`name.taken`).

`oml keys issue` and `oml keys issue-agent` print the key value once. OML stores a SHA-256 hash only, so there is no "show me the key again". `oml keys show` displays grants, denials, crew and connections, never the value.

Not available: revoking or rotating a key. To cut a key off today, add a global denial with `oml deny set`.

### Expiry and rate

A reader, contributor, submitter or reviewer key can carry two limits, each set when it is issued (`--expires`, `--requests-per-minute`) and neither required:

- **An expiry**, an instant. From it on the key authenticates nothing: its request answers `auth.invalid` with `details.reason: expired`, where an unknown or revoked key answers `unknown`. Only the holder of the key's own credential is told. The key's row, grants and name stay, and `oml keys show` says it has expired. A connector key's expiry is its connection's: the access token is refused the same way, a refresh answers `invalid_grant`, and no connect code is minted for it.
- **A rate**, in requests per UTC minute. The request past it answers `429 limit.rate` with `Retry-After`. A key of an account under a quota is held to both rates. Each server process counts alone, so n processes admit up to n times the rate ([settings](../operations/configuration.md#limits-counted-per-process)).

`oml keys limit <key>` changes or clears either, journalled as `key.limits_set`. It takes an admin key that is not the key itself: no agent key reaches the route, and an admin key naming itself is refused, so no key extends its own date or raises its own rate. Moving an expired key's date later makes it work again. A session key takes neither limit, and a revoked key none. A person issuing their own key on the web console's Keys page may give it an expiry, a day that ends in UTC, and no rate; changing that date afterwards takes an admin key like any other.

An admin key carries neither limit: issuing one with an expiry or a rate is refused, and so is `oml keys limit` naming one, each as `request.validation` with `details.reason: admin_key`. The admin key role is the one that changes limits, so no key that can change a limit carries one: a limited key cannot lift its own through a second admin key, and no admin key ends another by a date. The cost is that no admin key is temporary ([ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md)).

### Names, principals and labels

A key has a `name` (unique in its namespace; admin verbs address it wherever an id works), a `principal` (who acts; in every journal envelope; the independence check compares it; two keys of one person share it) and a free-text `label`. None changes after issuance. A key can name a [crew](workspaces.md) at issuance, fixed for its life and read by the independence check.

### Key format

An API key is `oml_` plus 43 base62 characters (256 random bits). The value encodes no role or tenant; a row looked up by its hash holds everything. The fixed prefix lets secret scanners find keys.

Send it only in the `Authorization: Bearer` header. `oml keys issue-agent --out` writes the value to a new owner-only file (mode 0600, never overwritten).

## Refusal order

For a verb that needs a grant role on a workspace:

1. The key's role cannot carry that grant role: `access.verb_ceiling`. This depends only on the role and runs before the workspace is resolved.
2. The needed role is in the key's effective access: allowed.
3. The key can read the workspace but lacks the needed role: `access.grant_refused`, naming the missing role.
4. Otherwise `access.not_found`, the same answer as for a workspace that does not exist.

OML works this out again on every request and caches nothing. The permission epoch labels the grant state each answer used.

A contributor or submitter key supersedes only its own memory; a person's session web key also supersedes a memory any key of its person wrote, read the one way `same_user` reads it ([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md) d2). Another key's readable memory answers `access.author_only`. A reviewer edit also needs reader on the node.

| Effective grant role | Verbs (`my_effective_verbs`) |
|---|---|
| `reader` | `recall`, `read`, `graph_read`, `events_tail` |
| `submitter` | `remember`, `learn`, `attach`, `supersede` |
| `reviewer` | `propose`, `review`, `supersede`; a person's session reviewer key with reader beside it also `remember`, outside its person's own root |

A `service` key omits `review`.

A person's session key whose role is reviewer also starts a new claim, held as every write is, where it holds reviewer and reader outside that person's own root (inside it the person's web key writes, and the review key is refused `access.verb_ceiling`, reason `review_key_in_own_root`), and lists `remember` in `my_effective_verbs` there, so the verbs it lists are the verbs it may use; its lineage excludes it from judging that claim. No other reviewer key starts one or lists `remember` ([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md) d2, d10).

## Review independence

OML limits which rows a reviewer may review, and checks it when approving.

- **Lineage exclusion.** If the reviewing key or its principal appears in the memory's lineage closure (every memory it derives from, through declared `derives_from` and connection edges, pending or ratified), the verdict is refused with `review.author_in_lineage`. A reviewer cannot propose an edit and then approve it.
- **Rival successors.** While several successors of one memory are held, each author is excluded from reviewing the others.
- **Different principals.** A requirement that the reviewer's crew or external manager differ from the author's is designed and not built: every workspace takes one reviewer verdict from outside the memory's lineage.

OML cannot see undeclared derivation or prove two principals do not collude.

## The admin read

An admin key reads any memory of its tenant with its text and attribution: every state, held included, in every workspace, user root and `universe`. The routes are `POST /v2/admin/memories/search` and `GET /v2/admin/memories/{artifact_id}`; `oml admin read` and the admin page's Memories and Atlas views call them. The single read shows every verdict with its reviewing key and principal. Each read is made under an investigation the admin key opened with a reason: `POST /v2/admin/investigations {reason}`, `oml admin investigate --reason`, or the reason the console asks for before its admin pages show text. The reason is stored in clear and scanned, the investigation stays open until its key closes it, and `oml doctor` names one open for more than seven days. A read that names no open investigation of its key is refused `admin.investigation_required`. Each read is journaled as `memory.admin_read`, with the ids, the investigation's id and a digest of the query, never the text ([event catalog](../data/event-catalog.md); [ADR-0089](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) d10, d17). No other role reaches the routes, the operator included. So a tenant admin can read every person's memories before review; [security](security.md) lists that as a residual risk ([ADR-0045](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d21).

## Admin limits

An admin key gives no verdict and writes no memory. An admin who needs to write uses a separate contributor key; the write is held and reviewed like any other.

Demotion ([ADR-0010](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0010-roles-and-keys.md) d6) and erasure by crypto-shredding ([ADR-0005](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0005-erasure-by-crypto-shredding.md)) are designed and not built: no key can demote or erase a memory in this build. A memory leaves recall only by an approved [retirement](audit-and-lineage.md#which-version-is-current), which keeps every version. Bans and reassignment between workspaces are not built either.

## The operator and the first admin key

The operator tier is designed and not built. A single-use enrollment token starts a new server: `oml bootstrap claim` exchanges it for the first admin key of the server's tenant and stores the URL and key in an owner-only file (mode 0600). With `--person <name> --email <address>` the claim also creates that person's user root, links the admin key to it, as an admin fix requires ([ADR-0089](../adr/ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md), the operator's decision of 2026-11-19), and records the address as that person's accepted admin sign-in, the one way the person gets a first sign-in ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md), the operator's decision of 2026-11-20); without them a console admin links the key later. Where authority is weighed, an admin fix's person, a fix's settlement and a write's recorded author, a user key counts as the person its issuer counts as, so a key an admin issues in someone's root counts as the admin's person (ADR-0089, the decision of 2026-11-20). No route creates a further tenant.

## User keys

A key with the field `user` (the person an admin attests it acts for) is a **user key**; one without is a **tenant key**.

- **Who may carry it:** reader, contributor, submitter and reviewer keys, connector keys included. Admin, operator and `service` keys cannot. OML never derives it from principal, name or label.
- The first key naming a user creates that user's root, `universe.<user>`.
- **Grants stay inside the user's root.** Any other grant is refused with `access.issuer_ceiling`. Issuance adds one reader grant on the `universe` node only.
- A user key's bare workspace names resolve in its root; tenant keys use `<user>.<name>`.
- **Writes into `universe` are refused** (`submission.universe_refused`). Only a curator writes there: a tenant contributor key granted submitter on the `universe` node, reviewed by a reviewer outside the memory's lineage and, for a person's `<user>-curate`, outside that person: no key of the same person judges it in `universe` ([ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workspaces-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md) d7).
- **A memory is started in a user's root by that user's keys only.** Any other key's new memory in `universe.<user>` or below it is refused with `submission.user_root_refused`, whatever grant it holds there. So is its successor of a ratified retirement, which would put the claim back in service. A person's keys are their user keys and the [session keys](#session-keys) that serve them: `<user>-review` and `<user>-curate` carry no `user` field and are read as that person's by their session, in that person's root and in no one else's. A reviewer granted there still proposes a successor or a retirement of a served memory, and its reads, recall and verdicts are unchanged. `my_effective_verbs` still lists what its grants confer. A key that cannot read the workspace gets `access.not_found`.

### Linked tenant keys

A person works in a shared workspace, such as `payments.dev`, with a tenant key: their user keys and session keys hold grants only inside their own root. An admin links such a key to the person on the console's admin Keys page (`POST /v2/keys/{key}/link`), journalled as `key.linked_person`, and ends the link there (`POST /v2/keys/{key}/unlink`, `key.unlinked_person`). Only a signed-in console admin, `<user>-admin`, links or unlinks: a bearer admin key names no person, so its link is refused `session_admin_only`, and no CLI command links. Only a live tenant bearer key `used_by: person`, an admin key's included, that holds no link and none pending, is linked, to a user of the tenant who signs in and is not disabled, and never by an admin to that admin's own person; every other request is `request.validation` with `details.reason` naming why (`self_link`, `already_linked` among them). The link then waits on that person: until they accept it from their own console session, with **Accept link** on their Keys page (`POST /v2/keys/{key}/accept_link`, journalled `key.link_accepted`), the key acts for no one, so it writes nothing into their root, and an admin key with a pending link takes no fix. Anyone else's acceptance is refused (`not_named_person`, `session_only`). The bootstrap's link, made by the person who runs `oml bootstrap claim --person`, is accepted at once. A key holds one link or one pending link at a time: an unlink ends either. A revoked key can still be unlinked. `key.linked_person` names who linked it and `key.link_accepted` who accepted. An accepted link makes the key its person's key for writing into that person's root, wherever its grants allow (ADR-0085 d16, the operator's decision of 2026-11-19).

The console then acts with the key: `oml-web` sends `OML-Acting-Key: <key id>` beside the person's capacity credential, and the request runs as that key, with its grants, verb ceilings and request rate. The server honours the header once per request, only beside a capacity credential `oml-web` presents with its client identity, only on the routes Review acts through (`GET /v2/whoami`, `GET /v2/review/held`, `GET /v1/memories/{artifact_id}`, `POST /v2/review/verdicts/bulk`), and only for a key of the session's tenant, linked to the session's own user, not revoked, not past its expiry and `used_by: person`. Anything else answers `auth.invalid` with `details.reason: acting_key_not_linked` and runs nothing; the request log names the key it asked for (`acting_key_refused`). An accepted request's log names the key and the person (`acting_for`). The link is read once per request, so an unlink or a revocation takes effect from the next one. A linked key's approval of its person's own agent write counts as the same person, as a user key's does ([ADR-0085](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d16). The cost is one key read and one link check on each such request.

## Session keys

A **session key** has the credential `session`: no value, no connection. It acts only from one person's web session, which every request re-checks. It cannot be sent as a bearer key and never gets a connect code. A person holds up to four:

| Key | Role and grants |
|---|---|
| `<user>-web` | User contributor key: submitter and reader on `universe.<user>`, node read on `universe` |
| `<user>-review` | Reviewer key: reviewer and reader on `universe.<user>` |
| `<user>-curate` | Tenant contributor key: submitter on the `universe` node alone, and reader on each workspace it is designated to curate from |
| `<user>-admin` | Tenant admin key, no grants |

`<user>-web` and `<user>-review` are created at first sign-in, and `<user>-admin` too for an admin entry; `<user>-curate` is created when an admin designates the person a curator. Each session key's principal is its own name. `<user>-admin` reads memory text through the journaled [admin read](#the-admin-read) and gives no verdicts.

A session key that carries a denial, on any workspace, runs none of its key acts: a denied `<user>-web` issues no key, mints no connect code and cuts no key off. Each act answers `access.verb_ceiling` with `details.reason: session_key_denied` and journals nothing. The key it would issue holds the person's whole root, which a key denied inside that root or on `universe` no longer does; a denial on any other workspace masks nothing the key reads and refuses the three acts all the same. The refusal is permanent: no route lifts a denial, and every sign-in finds the same `<user>-web`, also after the person's entry is removed and added back. The session still opens the pages, lists the person's keys, downloads their export, files their deletion request and ends by sign-out; the Keys and Start pages draw none of the three forms and say so, and `GET /v2/whoami` tells `<user>-web`, and no other key, whether it carries a denial (`denied`, a boolean).

## Connections

A **connection** is one OAuth client's standing authority to act as one connector key at `/mcp`, so Claude's hosted apps use the [agent plugin](../api/mcp-plugin.md) without a pasted key. Connections need `oauth.issuer` set on the server.

- **Connector keys.** `oml keys issue-agent … --connect-code` issues one. It has no value and authenticates only through a connection, which reaches only `/mcp`. It has at most one open connection; opening a new one revokes the earlier.
- **Connect codes.** Twelve Crockford base32 symbols (60 bits), shown once as `7KQ2-M9XD-4TPA`, never in a log, URL or journal payload. Lives 10 minutes, or the `--ttl` an admin asks for, up to 30 days, for a connector key confined to one workspace ([ADR-0067](../adr/ADR-0067-a-directory-reviewer-reaches-a-demo-workspace-through-a-connect-code-that-lives-longer-and-the-tools-and-instructions-describe-instead-of-command.md) d3); the first correct entry spends it. A newer code, a revoked connection or a revoked key voids it.
- **Tokens.** Access token `oml_at_` plus 43 characters, 1 hour. Refresh token `oml_rt_` plus 43 characters, 30 days unused. Only digests are stored. Each refresh rotates the pair; a rotated refresh token presented again revokes the connection and all its tokens. Tokens travel only in the `Authorization` header and work only at `/mcp`.
- **A token is no more than its key.** Verb ceiling, grants, denials, own-memory rules and the permission epoch apply on every request.

A revoked connection never reopens. `oml keys disconnect <key> <connection-id>` ends one; `oml keys show` lists them.

Design record: [ADR-0010](../adr/ADR-0010-roles-and-keys.md), [ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md), [ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md), [ADR-0044](../adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md), [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md).
