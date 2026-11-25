<!--
id: adr/ADR-0096-a-person-builds-a-key-in-the-console-with-grants-inside-the-own-root-and-a-rate-and-the-admin-reads-one-key-and-sets-its-limits
tier: engineering
status: accepted design · 2026-11-24
answers: what a person's web session may send when it issues a key from v0.8.5, and what stays refused; whether a person sets a request rate on their own key and within which bound; whether a person issues submitter keys in the console; what the admin console reads and changes on an existing key, and whether it issues keys; where the admin Fix form lives while releases are unbuilt; which imports a person's web session reads and how it lists them; which To do group ADR-0095's proposals get; what the key list and the usage card add; how the console enters an expiry without script; which import-review items wait; and for each, the routes, fields, callers, refusals, doctor lines, bottlenecks and store-call budget
-->

# ADR-0096 — A person builds a key in the console with grants inside the own root and a rate, and the admin reads one key and sets its limits

**Status:** accepted design · 2026-11-24, decided by the operator in the v0.8.5 console round
(D1 to D13 of the round's decisions, each recorded below with its handle); the details the
coordinator drafted beyond those answers, and the admin's deny on any workspace, were decided by
the operator on 2026-11-24 as written, each marked **decided by the operator, 2026-11-24**; the
mockups are approved for the build

**Release:** v0.8.5, built on branch `bulk085/console` except as its build notes below say

**Amends:**
- [ADR-0070](ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md)
  (d2 and d5: what `POST /v2/keys` takes from `<user>-web`, what `POST /v2/keys/{key}/limits` and
  `POST /v2/keys/{key}/denials` take from `<user>-admin`, and five changed or new operations; d8:
  the new pages' calls).
- [ADR-0079](ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md)
  (d1 and the operator's answer 3: a person's own issuance takes `requests_per_minute`, and the
  console sends an expiry as the end of the next UTC day).
- [ADR-0093](ADR-0093-contributor-and-submitter-replace-writer.md) (d6: the console's issue form
  offers `submitter` beside `contributor` and `reader`).
- [ADR-0083](ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md)
  (d9: the Fix form is a tab of the admin workspace page until the Releases view exists; d11: a
  To do group for brief proposals, and `briefs` in the counts read).
- [ADR-0084](ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md)
  (d15: a person's web session reads and lists the imports any key of the person ran).
- [ADR-0095](ADR-0095-an-agent-creates-its-own-child-workspaces-writes-guardrails-and-proposes-status-and-brief-changes-through-oml-write-and-every-verdict-and-setting-stays-with-people.md)
  (d5, d6: a live proposal's `proposed_at` and `proposed_by`, so the console's `409 proposal.pending`
  names who proposed and when; recorded in ADR-0095's amendment of 2026-11-24).

**Depends on:** [ADR-0001](ADR-0001-the-constitution.md) (R1, R2),
[ADR-0025](ADR-0025-the-agent-only-memory-surface.md) (d2: the server takes exactly the grants it
is sent), [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
(d3 to d5: a user key's grants, coverage and names),
[ADR-0089](ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md)
(d15: the fix acts), and ADR-0095 (PR #514: the `workspaces` meter of its d4 and the brief
proposals of its d6).

## Context

The console's Keys page issues two shapes of key: a contributor with submitter and reader on the
whole root, or a reader on the whole root. `POST /v2/keys` as `<user>-web` takes `name`, `role`,
`connect_code` and `expiry`, and refuses every other field by name
(`issue_key_as_session`, `crates/transports/oml-transport-http/src/console.rs`). The grants are the
use case's (`issue_own_key`, `crates/oml-app/src/app/pages.rs`), never the body's. A person who
wants a key on one child workspace, a key that reads one subtree and writes another, a denial, a
default workspace or a request rate asks an admin, who uses the CLI. The admin console reads a
person's keys and cuts them off on `universe`; it shows no key's full record and sets no limit.

The audit of 2026-11-24 (main at `b9a86bc3`) listed what the round's screens need from the API.
The operator decided its thirteen questions the same day: a personal key builder in v0.8.5 (D1),
a person's own request rate (D2), submitter keys in the console (D3), and the audit's
recommendation on the rest (D4 to D9, D11 to D13), with #131 deferred (D10). Three of those
answers reverse earlier operator answers: ADR-0079's answer 3 ("a rate on a person's own key is
still an admin key's to set"), ADR-0093 d6 ("the issue form offers `contributor` and `reader`, and
no `submitter`") and ADR-0083 d9 ("the fix acts sit on the admin workspace page's Releases view",
which does not exist while ADR-0085 is unbuilt). This record writes those reversals down with what
the build needs.

It is held against R1: no key raises its own limits or widens its own access, and no surface an
agent calls gains an act. Every widening below is an act of a person's own web session, inside
the person's own root, or of a person's admin session in its own tenant.

Decided; built in v0.8.5, with the frames the build notes list left out.

## Decision

- **d1 — A person's web session issues a key with grants of its choosing inside its own root
  (D1, D5).** `POST /v2/keys` as `<user>-web` takes, beside `name`, `role`, `connect_code` and
  `expiry`:

    | Field | Admitted from `<user>-web` | Rule |
    |---|---|---|
    | `role` | `contributor`, `reader`, `submitter` (d3) | any other role is `request.validation`, `details.field: /role`, as today |
    | `grants[]` | `{workspace, role, covers, deny[]}`, `role` one of `reader`, `submitter` | taken exactly as sent (ADR-0025 d2); each grant inside `universe.<user>` (ADR-0044 d3) |
    | `default_workspace` | a workspace a sent grant covers | as the issuance order says (`docs/architecture/roles-and-keys.md`) |
    | `guardrail_scope` | `workspace` or `ancestors` | fixed at issuance (ADR-0037 d17); `ancestors` when omitted, as today |
    | `requests_per_minute` | 1 to 1,000,000 | d2 |

    Without `grants`, the server issues today's shape, so the Start page's one button and the
    existing forms keep working: a contributor gets submitter and reader on the root, a reader
    reader on it, and a submitter submitter on it, each covering the subtree. With `grants`, the
    server takes them as sent and adds nothing but ADR-0044 d3's reader node grant on `universe`,
    which a contributor and a reader key get and a submitter key never does (ADR-0093, fix round
    of 2026-11-20). `contributor_grants` applies to the grants sent, as ADR-0093's first
    implementation note reads it.

    Still refused from `<user>-web`, each as today: `user`, `used_by`, `crew`, `label`,
    `principal`, `profile` and `tenant` (`request.validation` naming the field, so the server sets
    `user` to the session's person and `used_by` to `agent`); the role `admin`, `operator` or
    `reviewer` (`/role`). That role check in `issue_own_key` is the only guard that keeps a
    reviewer key out of the console: a person's issuance runs under `Issuance::PersonsOwn`, whose
    ceiling is the admin role, which admits reviewer keys, so without the check a person could
    issue an agent a key that approves (R1). The test
    `a_persons_web_session_issues_only_contributor_reader_or_submitter_keys_and_never_a_reviewer_key`
    pins it. Also refused: a grant outside the root (`403 access.issuer_ceiling`, `details.user`);
    a grant role the key role does not admit, a `reviewer` grant among them (`403
    access.issuer_ceiling`, `details.role`, `admissible`, `grant_index`); a name without the
    `<user>-` prefix (`/name`); exactly the four session names `<user>-web`, `<user>-review`,
    `<user>-curate` and `<user>-admin` (`/name`), and no other name, so `<user>-proj-review` is
    issued (`SESSION_KEY_SUFFIXES` compares the whole suffix); a deny on a node grant
    (`/grants/<i>/deny`); a default workspace no grant covers, or one that lies inside a grant's
    deny (`/default_workspace`, step 6 of the issuance order); a node the root does not hold (`404 access.not_found`); a session key
    that carries a denial (`403 access.verb_ceiling`, `session_key_denied`, ADR-0070 d2); the
    account's keys quota (`429 limit.quota`, ADR-0068 d5); a taken name (`409 name.taken`, checked
    last). Any session capacity but `web` stays `access.verb_ceiling`, `session_capacity`.

    **Decided by the operator, 2026-11-24:** a session issuance takes at most 16 grants and 16 denials per
    grant; more is `request.validation`, `details.field: /grants` or `/grants/<i>/deny`,
    `details.reason: too_many`. The numbers are chosen, not measured: a root a person browses in a
    no-script form has few nodes worth a grant row each, and the cap keeps one session request
    from journalling an unbounded number of `grant.issued` events. A bearer admin key keeps no cap.

    The builder is three no-script pages (D5): a starting shape (whole root, one workspace, read
    only, write only, connector), then the form with grant rows ("Add a workspace" posts the form
    back with one empty row), then a review `GET` that restates the effective access in words, as
    a read-only indented tree, and as the equivalent `oml keys issue` line, with **Issue key**
    posting once. The review step pre-checks every refusal above that the page can decide from
    the role table and the tree it already read; `name.taken` and `access.not_found` stay the
    server's answers (D12).

    The form adds a reader row beside each submitter row, on the same node with the same
    coverage, as the CLI does (ADR-0093 d2; ADR-0028 d2); the person may remove it. A contributor
    whose submitter grant then has no reader grant reaching it passes `contributor_grants` and is
    a contributor without recall (ADR-0093 d3, as read on 2026-11-20), so the review step warns
    "no recall" on that row before **Issue key**.

    *Doctor:* no new line. Such a key is served `without_recall: true` and named by
    `key_contributor_without_recall`. *Bottleneck:* an issuance runs the issuer ceiling, a node lookup and a
    deny-containment check per grant inside one transaction; the cap bounds it, and the build
    records the issuance time at 16 grants as a row in [limits](../reference/limits.md).
    *Budget:* the builder's form and review `GET`s each make `whoami` (2 store calls), then one
    tree page as `<user>-web` (8), two rounds; the `POST` is the issuance alone. The Keys page's
    calls do not change.

- **d2 — A person sets a request rate on a key at its issuance, and only an admin key changes it
  afterwards (D2).** This reverses the rate half of ADR-0079's answer 3. `requests_per_minute`
  from `<user>-web` follows ADR-0079 d4: a whole number from 1 to 1,000,000, else
  `request.validation`, `details.field: /requests_per_minute`. R1 holds as it does for the
  expiry: the session that sets the rate is not the key it limits, and the key itself never
  reaches a route that changes it. After issuance the rate moves or clears only through
  `POST /v2/keys/{key}/limits` by an admin key (ADR-0079 d5, unchanged); `<user>-web` stays
  `access.verb_ceiling` there, so a person cannot raise a rate they set.

    **The bound, decided by the operator, 2026-11-24:** no ceiling beyond ADR-0079 d4's range. A key of an
    account under a quota is held to both its own rate and the account's, and the lower refuses
    first (ADR-0079 d4), so a key rate above the account's limits nothing and harms nothing.
    Refusing it would tie issuance to the account's profile, and a profile an admin later lowers
    would leave keys "over" a bound nobody re-checks. The form shows the account's
    `requests_per_minute` beside the field (from `whoami.person.quotas`, already read) and says
    the lower of the two applies.

    *Doctor:* ADR-0079 d7's lines (the key's rate, and a warning on `limit.rate`) cover it.
    *Bottleneck:* the in-process counter holds only keys that carry a rate (ADR-0079 d6); more
    such keys grow it, bounded by the account's `keys` quota. *Budget:* none.

- **d3 — A person issues submitter keys in the console (D3).** This reverses the console half of
  ADR-0093 d6. The role select offers `contributor` (the default), `reader` and `submitter`,
  labelled by what each key does: "recall and write", "recall only", "write without recall". A
  submitter key carries submitter grants alone (ADR-0093 d2): a reader grant sent with it is
  `access.issuer_ceiling` at step 3, and it gets no `universe` grant. The rest of ADR-0093 d6
  stands: imports and the user-root create admit contributors alone, so a submitter key a person
  issues imports nothing and creates no workspace, over HTTP or over MCP (ADR-0095 d3's
  `create_is_own_root_only`); the builder's review step says so.

    **Decided by the operator, 2026-11-24:** a submitter connector key is admitted, as the API admits it; the
    form says that such a connection writes and recalls nothing.

    *Doctor:* none; `without_recall` is false for a submitter, whose role already says it.
    *Budget:* none.

- **d4 — The admin console reads one key, sets its limits and denies it on any workspace of the
  tenant, and still issues nothing (D1).** Three acts of `<user>-admin`, under capacity `admin`:
    - **The one-key read, an API change.** `/web/admin/keys?key=<key>` draws `GET /v2/keys/{key}`:
      the grants with coverage and denials, crew, principal, label, profile, guardrail scope,
      connections, expiry and rate. Today the route refuses the session: `get_key_inner` reads the
      raw `Authorization` value and calls the use case with it, not with the session gate's
      caller, so a `<user>-admin` capacity credential answers `401 auth.invalid`. The route
      changes to resolve the caller as `/limits` and `/denials` do, admitting the admin key role,
      `<user>-admin` included, and no other session capacity; the test
      `the_admin_session_reads_one_key_through_get_key` proves it, red first. The read, like
      `GET /v2/keys`, journals nothing, and the page says nothing of an audit trail.
    - **Limits.** `POST /v2/keys/{key}/limits` from `<user>-admin`, as ADR-0079 d5 has it: never
      the calling key (`own_key`), a session key (`session_key`), a revoked key (`revoked`) or an
      admin key (`admin_key`), each `request.validation`, `details.field: key`; a key of another
      tenant `access.not_found`. Called with an `omls_` credential, the route claims the
      `Idempotency-Key` and replays the recorded answer, as the session's other key acts do
      (ADR-0070 d3 layer 4), so the form's nonce journals one `key.limits_set`; a bearer admin key
      keeps the unreplayed behaviour (the replay decided by the operator, 2026-11-24). The form enters an expiry as d10 says and clears either
      limit with its own checkbox.
    - **Deny on a workspace (decided by the operator, 2026-11-24, beyond D1's words, which named the one-key read
      and the limits).** `POST /v2/keys/{key}/denials` from `<user>-admin` admits any
      workspace of the tenant, with the request's `reason`, where today a session key denies on
      `universe` alone. `<user>-web` keeps `universe` alone, cutting off one of the person's own
      keys. The refusals stand: never a session key, never an admin key (`request.validation`,
      `details.field: key`, with no `details.reason`), a workspace the tenant does not hold
      `access.not_found`.

    Admin issuance stays off the console: `POST /v2/keys` from `<user>-admin` remains
    `access.verb_ceiling`, `session_capacity` (ADR-0070 d2, "the admin capacity issues no plain
    key"). The admin builder is later work.

    *Doctor:* none new; ADR-0079 d7 and the denial lines stand. *Bottleneck:* the one-key read
    runs three use cases (ADR-0079 d4, #341), which costs nothing against a rate since an admin
    key carries none, and three times the store calls of one. *Budget:* the one-key page makes `whoami` (2), then `GET /v2/keys/{key}`,
    whose store calls the build measures and records in `web_console_reads.rs`; the limits and
    deny forms are `POST`s.

- **d5 — The admin Fix form is a tab of the admin workspace page until the Releases view exists
  (D6).** This amends ADR-0083 d9. `/web/admin/workspace?workspace=<w>&view=fix` carries the
  **Fix** form with the acts `POST /v2/admin/fixes` builds today, each with its target and the
  required reason, and a confirmation step (`…&view=fix&act=<act>&target=<target>`) that says the
  fix is served at once and waits for a person to confirm or revert it, and for `erase` that every
  version of the claim is crypto-shredded. It posts to `/web/admin/fixes`, which calls `POST
  /v2/admin/fixes` as `<user>-admin`. Release rollback and import rollback are absent until
  ADR-0085 is built; when its Releases view lands, the form moves there as d9 first said and the
  tab goes. No other admin card carries a fix act.
    - **The acts by status.** In a `development` or `protected` workspace the form offers
      `demote`, `edit`, `retire` and `erase`. In a `frozen` workspace it offers `demote` and
      `erase` alone (ADR-0089 d12); an edit or a retirement posted there anyway, from a stale form,
      answers `workspace.frozen`, and the page says to demote or erase instead.
    - **The target is read through an investigation (decided by the operator, 2026-11-24, amending ADR-0083 d9).** `target` is a version id, and an admin
      reads a claim only inside an investigation, each read journalled (ADR-0089 d10). The tab
      takes its target from an open investigation: a link on the investigation's memory page
      (`/web/admin/investigation`), which carries no act itself, opens the tab with that version as its target, and an edit's
      form shows the version's current text from that same read. With no open investigation the
      tab names none and says an investigation opens first.
    - **Who settles it.** The fix is confirmed or reverted by a person's reviewer key with
      reviewer on the workspace (ADR-0083 d8). Where the target lies outside the fixing admin's
      own person's root, in a `protected` or `frozen` workspace, or in a release, that person is
      someone other than the person the admin acted for: their own review key is served `acts: []`
      for it (ADR-0089, the amendment of 2026-11-18, accepted 2026-11-19). Inside that person's own
      root in development they may settle it themselves, marked `same_user`. An author of the
      target's lineage outside the own root is refused `review.author_in_lineage`, `same_person`,
      at the act. The confirmation step says "waits for a person other than <admin's person>"
      where the rule applies; the console reads the served `acts` and never computes the rule.

    Refusals shown by name: `admin.fix_held_by_scan`, `admin.fix_target_not_promoted`,
    `workspace.frozen`. A `<user>-admin` session's person is the session's user (ADR-0089, the
    operator's decision of 2026-11-19), so `admin.fix_requires_person` does not arise on this tab;
    the test `an_admin_session_fixes_as_the_person_its_session_names` proves it.

    *Doctor:* none new. *Budget:* the tab adds no call to the workspace page's `GET`; the target's
    read is the investigation's own, journalled as ADR-0089 d10 says.

- **d6 — A person's web session reads and lists the imports any key of the person ran (D7).**
  This amends ADR-0084 d15, where only the importing key reads an import.
    - `GET /v2/imports/{import_id}` admits, beside the importing key, `<user>-web` for an import
      run by any key whose user is the session's person. Any other caller, and any other import,
      stays `access.not_found`.
    - **New:** `GET /v2/imports?state=&limit=&cursor=`, newest first, `limit` up to 50. As
      `<user>-web` it lists the imports of every key of the person; as any other key, that key's
      own. Each item is the `Import` that `GET /v2/imports/{import_id}` serves, plus
      `key: {id, name}`, the importing key. `state` is `running`, `done` or `failed`; another value
      is `request.validation`, `details.field: state`.

    **The built import rule, which these pages state.** An import goes into a `development`
    workspace in progress that the key writes, or a new name it creates in progress; any other
    target is `409 import.target_not_in_progress`. ADR-0084's D4 amendment ("any open or protected
    workspace") is not built, so the console's import hand-off offers in-progress workspaces and a
    new name only. The first import from a source requires `--expect-key`, the source key's
    fingerprint of 64 hex digits (`request.validation`, `details.field: /expect_key`,
    `details.reason: first_import`), and pins it.

    **Decided by the operator, 2026-11-24:** "any key of the person" is every key that names the person as its
    user; a tenant key linked to the person (ADR-0085 d16) is not included, since the link lets
    the person act through the key, not read what an admin ran with it.

    *Doctor:* the `imports` line of ADR-0084 d15 stands. *Bottleneck:* the listing reads the
    imports table by tenant and key, newest first; the build adds the index that serves it and a
    row in [limits](../reference/limits.md). *Budget:* the import list page and the import page
    each make `whoami` (2), then one listing or read; the build records both counts.

- **d7 — Brief proposals get a To do group; status proposals do not (D8).** This amends ADR-0083
  d11. ADR-0095 d6 lets contributor keys, agents among them, propose brief members, so a person's
  reviewers get a queue agents can fill.
    - `GET /v2/todo/counts` gains `briefs`: the pending brief edges the person's `<user>-review` may
      ratify, at `workspace` and below or everywhere, counted under the same rules as the list
      and up to 1,000, sharing `truncated`.
    - **New:** `GET /v2/review/briefs?workspace=&cursor=` under `<user>-review` (capacity
      `review`): the pending brief edges on workspaces where the key holds reviewer effective
      access, oldest first, each with the edge id, workspace, the claim's id and title, the
      proposing key's name, `same_user` (ADR-0046 d2) and when it was proposed; an edge proposed
      by the key itself or under its principal is not listed, since ratification refuses those.
      Any other caller is `access.verb_ceiling`.
    - **Who ratifies.** A key and principal other than the proposer's, not another person
      (ADR-0046 d2, d8): a ratification by another key of the same person is admitted and marked
      `same_user`. The console proposes a brief member as `<user>-web`, so the person's
      `<user>-review` can ratify it; in a user root no other key of the person holds reviewer.
    - The group "Brief proposals to ratify" sits after "To review"; its act, **Ratify**, asks for
      confirmation and posts `POST /v2/edges/{id}/ratify` as `<user>-review`.

    Status proposals get no group: they come from reviewer keys alone (ADR-0095 d5) and already
    show on both Workspaces views.

    The admin To do's "Guardrail-loosening alerts" group (ADR-0083 d11) needs no new list: it is
    built from `loosens_guardrail` on the rows `GET /v2/admin/fixes` already serves. Only its
    count on the badge is an API change: `GET /v2/admin/todo/counts` gains `alerts`, counted from
    the same rows.

    *Doctor:* none. *Bottleneck:* the list and the count read pending brief edges under the key's
    reviewer grants; the build adds a row in [limits](../reference/limits.md). *Budget:* To do
    keeps its one counts call per page (ADR-0083 d14); the store calls inside that call grow by
    the brief count, which the build records in `web_console_reads.rs` with this reason.

- **d8 — The key list serves each grant's coverage (D9).** `GET /v2/keys` items gain
  `grants[].covers`, `subtree` or `node`, read from the grant row the listing already reads. It is
  required in the schema; a client reads an answer without it, from an older server, as
  `subtree`. Denials stay on the one-key read. The Keys pages mark a node grant "this workspace
  only". *Doctor:* none. *Budget:* none; the Keys page stays at 2, 2, 12, 4 and 8 store calls.

- **d9 — The usage card shows the profile, the month and recalls, and ADR-0095's `workspaces`
  meter (D11).** Fields on existing reads, no new call (ADR-0070 d8's remedy rule):
  `whoami.person` and each `GET /v2/web/users` row gain `profile` (the quota profile's name, null
  for none), `period` (the UTC month, `YYYY-MM`), `usage.recalls` (this month's), and, with
  ADR-0095 d4, `usage.workspaces` and `quotas.workspaces` (null is unlimited). Additive.
  *Doctor:* none. *Budget:* only a read that asks for `person` pays; the build measures `whoami`
  with `person` before and after and records any change in `web_console_reads.rs`.

- **d10 — The console sends an expiry date as the end of the next UTC day, with no script
  (D4).** A date input, `YYYY-MM-DD`, is sent as the next day's `23:59:59Z`, so the key works
  through the chosen date in every time zone and never ends earlier than meant; the cost is up to
  a day longer than meant east of UTC. This amends ADR-0079's answer 3, which sent the chosen
  day's own `23:59:59Z`. The form's line reads "The key works through this date in every time
  zone." The key lists show the stored instant as it is, "ends 2026-12-02 23:59 UTC", never a
  date the server did not store (decided by the operator, 2026-11-24). A date whose sent instant is not later than the page's clock is
  refused on the page, as the API would refuse it; a day its month does not hold rolls over as
  ADR-0079's answer 3 says. The admin limits form (d4) uses the same rule. No script, no CSP
  change (D13).

- **d11 — The import-review items wait.** ADR-0083 d11's "Imported versions to review" needs
  `import_provenance` on `GET /v2/review/held` rows, an `imports` member in `/v2/todo/counts`,
  and the `attests_not_source_author` attestation on the verdict (ADR-0084 d4), none of which the
  API serves. **Decided by the operator, 2026-11-24:** the audit lists them as gaps and the round's
  questions named none of them, so the round does not build the group or the Judge page with the
  attestation; an imported version stays in To review as the held row it is today. They wait
  for a record of their own, with the attestation.

- **d12 — Deferred.** Recording a key's last use (#131), which waits for a measured per-request
  cost and its own record (D10); the admin key builder (D1); an issuance dry run, whose two
  server-only answers stay on the review step (D12); hashed inline scripts on console pages
  (D13); the import-review items (d11). Adding or revoking a grant on an issued key, revoking and
  rotating keys stay unbuilt, and no page draws them.

## Consequences

- **OpenAPI** (`docs/api/openapi.yaml`): `issueKey`'s session paragraph lists the fields of d1
  and `too_many`; `getKey` resolves the session caller and admits `<user>-admin` (d4); `setKeyLimits` says what
  `<user>-admin` may do, and `addDenial` admits `<user>-admin` on any workspace (d4);
  `adminTodoCounts` gains `alerts` (d7);
  `getImport` admits `<user>-web` (d6); `listImports` (`GET /v2/imports`), `listBriefProposals`
  (`GET /v2/review/briefs`) are new; `todoCounts` gains `briefs`; `ListedKey.grants[]` gains
  `covers`; `WhoamiPerson` and `WebUser` gain `profile`, `period`, `usage.recalls`,
  `usage.workspaces` and `quotas.workspaces`. `errors.md` gains `too_many` under
  `request.validation`. The Rust and Python SDKs are regenerated, and the Rust SDK gains the
  operations `oml-web` calls.
- **Pages:** the key builder (`/web/keys/new`, three steps), the admin one-key page with its
  limits and deny forms, the admin workspace page's Fix tab, the import list and import page, To
  do's brief group, the Keys pages' coverage mark, and the usage card. `docs/operations/web-console.md`
  and `docs/architecture/roles-and-keys.md` say what each side issues and changes.
- **Tests,** each red first, named after the sentence they prove, among them:
  `a_persons_web_session_issues_a_key_with_grants_inside_its_root_taken_exactly_as_sent`,
  `a_persons_web_session_is_refused_a_grant_outside_its_root_with_issuer_ceiling`,
  `a_submitter_key_from_a_web_session_gets_no_universe_grant`,
  `a_web_session_issuance_past_sixteen_grants_is_refused_too_many`,
  `a_rate_set_at_a_persons_issuance_changes_only_by_an_admin_key`,
  `the_admin_session_reads_one_key_and_sets_its_limits_once_per_nonce`,
  `the_admin_session_denies_a_key_on_a_workspace_and_the_web_session_on_universe_only`,
  `the_admin_session_still_issues_no_key`,
  `a_persons_web_session_reads_and_lists_the_imports_of_every_key_of_the_person`,
  `the_brief_group_lists_what_the_review_key_may_ratify_and_never_its_own_proposal`,
  `the_key_list_serves_each_grants_coverage`,
  `the_expiry_date_is_sent_as_the_end_of_the_next_utc_day`.
- **Budget:** each new `GET` names its primary call and adds its counts to
  `web_console_reads.rs`; no page gains a third round.
- **Issues:** the build is tracked in milestone v0.8.5; d11's items and #131 stay open.
- **The cost:** a person can now issue a key of any shape their root admits, so a key that is
  narrower or odder than the two old shapes exists without an admin seeing it first. The keys
  quota, the root boundary and the review step bound it, and an admin still reads, limits and
  cuts off every such key.

## Amendment of 2026-11-25: the limits form waits on a raise, and keys are revoked (amends d4, d12; ADR-0097)

Decided by the operator on 2026-11-25 (the break-glass and key-access questions); recorded in [ADR-0097](ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md).

- **d4.** The deny takes effect at once and journals its reason (ADR-0097 d27, item 2, in v0.8.5).
  A raise, a cleared limit or a later expiry from the limits form waits on a second admin person
  and the form says so; a lower one takes effect at once (ADR-0097 d15). The one-key page gains one
  revoke button in v0.8.6.
- **d12.** Revoking a key is built as one plain verb for any key (ADR-0097 d14). A person's web
  session revokes the person's own keys with the same verb. Rotation and a cascade stay deferred.
  The admin key builder is no longer deferred: it ships in v0.8.6, reusing the personal builder's
  flow, and its keys wait on a second admin person (ADR-0097 d29, decided by the operator,
  2026-11-25).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Keep the two fixed shapes and send people to the CLI for the rest** | The operator chose a builder (D1). A person who is not an admin has no CLI path at all: `oml keys issue` needs an admin key. |
| **The admin builder in v0.8.5 as well** | It reverses ADR-0070 d2 ("the admin capacity issues no plain key") and needs the "whose key", crew, principal and profile fields; the operator staged it after the personal builder (D1). |
| **Let the web session send `user`, `used_by` or `profile`** | `user` is the session's person and nothing else; `used_by: person` would let a person mint keys the export and import person checks admit without an admin's attestation; `curator` is an admin's designation (ADR-0044 d7). |
| **No cap on grants from a session** | One request could journal an unbounded number of events; a bearer admin key is trusted with that, a browser form need not be. |
| **Keep ADR-0079's answer 3 (no rate from a person)** | The operator chose a rate (D2). The audit's case for keeping it, that a person's own rate guards nothing against that person, is weaker than the use: a person limits an agent key they hand to a tool. |
| **A key's rate at most the account's** | A second refusal that limits nothing the lower-refuses-first rule does not, and that goes stale when a profile changes (d2's reading). |
| **Let the person change the rate or the date afterwards** | ADR-0079 d5's one sentence, "only an admin key that is not the key itself", would grow a session exception; issuance is where the person decides. |
| **Keep submitter out of the console (ADR-0093 d6)** | The operator chose it (D3); a write-only key is the narrowest key an agent that only reports needs. |
| **Build a minimal Releases view now to host the Fix form** | A Releases view with no releases is a page for one form, and ADR-0085's real view would replace it (D6). |
| **The Fix form on the CLI only in v0.8.5** | Admin fixes are served in the console's To do (ADR-0083 d8); an admin who sees a fix to make would leave the console to make it. |
| **The console only hands off to the CLI for imports** | The person could not see whether an import they started finished, since a CLI import runs under a bearer key the session cannot read as (D7). |
| **`same_user` reads of imports for every key of the person** | Only the session is a person at a browser; an agent key reading its siblings' imports serves no screen. |
| **A To do group for status proposals** | They come from reviewer keys alone and show on both Workspaces views (D8). |
| **Fan the To do brief group out over `GET /v2/edges?kind=brief` per workspace** | One call per workspace breaks the two-round rule (ADR-0070 d8); a list read in the shape of ADR-0083 d11's other groups costs one. |
| **Denials on the key list too** | The listing is the Keys page's costliest call (12 store calls); the one-key read already serves them (D9). |
| **A zone select, or a hashed script that reads the browser's zone, for the expiry** | Each asks the person something or adds a CSP change (ADR-0069 d3) for at most a day's difference (D4, D13). |
| **Build the import-review group now** | Its attestation is a verdict change no decision of this round made; without it the group would show a check the server does not record (d11). |

## Operator decision of 2026-11-25: Grant read

Decided by the operator on 2026-11-25 and built in v0.8.5. It extends d4's admin capacities and
amends [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d10's capacity list, which named review and curation.

- **Grant read.** Administration, People, Review and curation gains a third form, **Grant read**
  (user, workspace). It rides `POST /v2/web/users/{user}/capacities` with `capacity: read`, and
  gives the person's `<user>-web` session key a `reader` grant, covering the subtree, on the
  workspace. Each grant is one `grant.issued` by `<user>-admin`, a grant already held is not
  added again, and the grant reaches the session that opens after it, as Grant review does.
  It confers no verb beyond reading: no verdict, no ratification, no write.
- **What the person sees.** The Personal Memories (Approved) and Catalog pages read as
  `<user>-web`, so they list the workspace's approved memories and documents with no page change.
  The workspace picker's hints list the person's root; a granted shared workspace is opened by
  its id, from the admin page's link below or from Browse.
- **Browse as the admin's person.** The admin workspace page shows **Browse as <user>** beside
  the investigation line where the admin's own `<user>-web` reads the workspace, leading to the
  Personal Memories view filtered to it, read without an investigation since it is the person's
  own read. Otherwise the page is as before. The check is one workspace read as `<user>-web`.
- **Under ADR-0097.** In v0.8.6 this grant becomes widening under ADR-0097's model; this note
  records the v0.8.5 shape only.

## Operator decision of 2026-11-25: d13, key rotation

Decided by the operator on 2026-11-25 and built in v0.8.5; [ADR-0097](ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person.md) d14 notes it.

- **d13 — A key is rotated: a new value, everything else kept.** `POST /v2/keys/{key}/rotate
  {grace_seconds?}` gives a bearer key a new value, answered once as `value` with `rotated_at` and
  `previous_valid_until`. The key keeps its id, name, grants, denials, limits, links, crew and
  history. The value is never journalled: `key.rotated` names the key, the rotating key and
  `previous_valid_until`, and the new hash rides the commit as an issuance's does. **Grace** is
  optional and none by default: with `grace_seconds` up to 86,400 the previous value keeps
  authenticating until then, held in `key_previous_hashes` (migration 0069) with its end, and
  without it the previous value stops at once. A later rotation of the key replaces or deletes the
  row; a row past its end resolves nothing. **Who:** an admin key, a `<user>-admin` session's
  included, for any key; a person's `<user>-web` session for that person's own keys; and the key
  itself, so an agent rolls its own secret; anyone else is `access.verb_ceiling`, and a person's
  session naming another person's key finds none. **Refused by name** (`request.validation`,
  `details.field: key`): `session_key`, `connector_key` (it takes a new connect code instead),
  `revoked`, `cut_off`, `expired`; a longer grace is `grace_too_long` on `/grace_seconds`. The
  `Idempotency-Key` is claimed before the act, so a replay answers without the value and rotates
  nothing again. **Surfaces:** `oml keys rotate <key> [--grace <dur>] [--out <file>]`, `--out`
  writing a new 0600 file and printing no value; **Rotate** beside **Cut off** on the Personal and
  admin Keys tabs, asked first with a grace of none, one hour or 24 hours, the result shown once
  with when the previous value stops; the Rust SDK's `rotate_key` and the generated layers.
  **Doctor:** `whoami` serves `rotation_grace_until` for a bearer key while its grace is open, and
  `oml doctor` prints `info key.rotation_grace_open`. **Cost:** the act 12 store calls, and a key
  value's `whoami` one read more (`web_console_reads.rs`). Under ADR-0097 (v0.8.6) rotation is
  neutral: it grants nothing new.

## Build notes (v0.8.5, 2026-11-25)

Built: d1 to d4, d6 to d10 on the server; on the console the key builder, the Keys list's coverage
mark and expiry line, the usage card's profile, month, recalls and `workspaces` meter (drawn only
where the API serves it), the admin one-key page with its limits and deny forms, the Fix tab (d5)
with its target from an investigation's memory page, the import hand-off, list and one import, the
To do brief group with **Ratify**, and the admin To do's fixes and alerts groups.
`POST /v2/edges/{id}/ratify` resolves a session caller, so `<user>-review` ratifies from To do.

Readings the build took, for the operator to confirm:

- The builder's rate field does not show the account's own rate beside it: the form's `GET` reads
  no person block, to stay at d1's budget (`whoami`, then one tree page). The hint says that
  under the account's quota the lower of the two holds.
- The Fix tab's edit form does not show the version's current text: that text is one more admin
  read, journalled, on the investigation's memory page, which the tab links back to.
- The admin one-key page shows the default workspace, the crew and each grant's and denial's
  workspace by id, not by name: naming them would add a tree read to the page.
- The admin To do draws the unconfirmed fixes and the alerts, not open investigations: the SDK has
  no investigations listing yet.

Not built in this lane, each waiting on another change: the brief section of the workspace page,
**Propose for brief** and the P9 status proposal on the workspace page (they call the proposal
routes ADR-0095 changes, which the `bulk085/mcp` lane builds, and `POST /v2/edges` still reads the
raw bearer, so a session cannot propose); the workspace export page (P10), which needs the SDK's
binary download (#465); the personal fix confirmation page `/web/todo/fix` (P14) and the imported
version's Judge (P15, deferred by d11).

