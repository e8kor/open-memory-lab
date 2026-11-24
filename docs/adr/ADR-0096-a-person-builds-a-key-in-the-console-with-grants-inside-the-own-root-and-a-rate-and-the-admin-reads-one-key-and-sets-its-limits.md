<!--
id: adr/ADR-0096-a-person-builds-a-key-in-the-console-with-grants-inside-the-own-root-and-a-rate-and-the-admin-reads-one-key-and-sets-its-limits
tier: engineering
status: accepted design · 2026-11-24
answers: what a person's web session may send when it issues a key from v0.8.5, and what stays refused; whether a person sets a request rate on their own key and within which bound; whether a person issues submitter keys in the console; what the admin console reads and changes on an existing key, and whether it issues keys; where the admin Fix form lives while releases are unbuilt; which imports a person's web session reads and how it lists them; which To do group ADR-0095's proposals get; what the key list and the usage card add; how the console enters an expiry without script; which import-review items wait; and for each, the routes, fields, callers, refusals, doctor lines, bottlenecks and store-call budget
-->

# ADR-0096 — A person builds a key in the console with grants inside the own root and a rate, and the admin reads one key and sets its limits

**Status:** accepted design · 2026-11-24, decided by the operator in the v0.8.5 console round
(D1 to D13 of the round's decisions, each recorded below with its handle); the details the
operator did not decide are marked **coordinator's reading** and wait for the operator's
confirmation

**Release:** v0.8.5, not built

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

Decided, not built.

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
    `reviewer` (`/role`); a grant outside the root (`403 access.issuer_ceiling`, `details.user`);
    a grant role the key role does not admit, a `reviewer` grant among them (`403
    access.issuer_ceiling`, `details.role`, `admissible`, `grant_index`); a name without the
    `<user>-` prefix or with a reserved suffix `web`, `review`, `curate`, `admin` (`/name`); a deny
    on a node grant (`/grants/<i>/deny`); a default workspace no grant covers
    (`/default_workspace`); a node the root does not hold (`404 access.not_found`); a session key
    that carries a denial (`403 access.verb_ceiling`, `session_key_denied`, ADR-0070 d2); the
    account's keys quota (`429 limit.quota`, ADR-0068 d5); a taken name (`409 name.taken`, checked
    last). Any session capacity but `web` stays `access.verb_ceiling`, `session_capacity`.

    **Coordinator's reading:** a session issuance takes at most 16 grants and 16 denials per
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

    *Doctor:* no new line. A contributor built with a submitter grant that no reader grant
    reaches is a contributor without recall, served `without_recall: true` and named by
    `key_contributor_without_recall` (ADR-0093 d3, as read on 2026-11-20); the review step says
    so before issuance. *Bottleneck:* an issuance runs the issuer ceiling, a node lookup and a
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

    **Coordinator's reading, the bound:** no ceiling beyond ADR-0079 d4's range. A key of an
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

    **Coordinator's reading:** a submitter connector key is admitted, as the API admits it; the
    form says that such a connection writes and recalls nothing.

    *Doctor:* none; `without_recall` is false for a submitter, whose role already says it.
    *Budget:* none.

- **d4 — The admin console reads one key, sets its limits and denies it on any workspace of the
  tenant, and still issues nothing (D1).** Three acts of `<user>-admin`, under capacity `admin`:
    - **The one-key read.** `/web/admin/keys?key=<key>` draws `GET /v2/keys/{key}`: the grants with
      coverage and denials, crew, principal, label, profile, guardrail scope, connections,
      expiry and rate. The route takes the admin key role and `<user>-admin` is an admin key, so
      it is admitted; the build proves that with a test before the page is built.
    - **Limits.** `POST /v2/keys/{key}/limits` from `<user>-admin`, as ADR-0079 d5 has it: never
      the calling key (`own_key`), a session key (`session_key`), a revoked key (`revoked`) or an
      admin key (`admin_key`), each `request.validation`, `details.field: key`; a key of another
      tenant `access.not_found`. Called with an `omls_` credential, the route claims the
      `Idempotency-Key` and replays the recorded answer, as the session's other key acts do
      (ADR-0070 d3 layer 4), so the form's nonce journals one `key.limits_set`; a bearer admin key
      keeps the unreplayed behaviour. The form enters an expiry as d10 says and clears either
      limit with its own checkbox.
    - **Deny on a workspace.** `POST /v2/keys/{key}/denials` from `<user>-admin` admits any
      workspace of the tenant, with the request's `reason`, where today a session key denies on
      `universe` alone. `<user>-web` keeps `universe` alone, cutting off one of the person's own
      keys. The refusals stand: never a session key, never an admin key (`request.validation`,
      `details.field: key`), a workspace the tenant does not hold `access.not_found`.

    Admin issuance stays off the console: `POST /v2/keys` from `<user>-admin` remains
    `access.verb_ceiling`, `session_capacity` (ADR-0070 d2, "the admin capacity issues no plain
    key"). The admin builder is later work.

    *Doctor:* none new; ADR-0079 d7 and the denial lines stand. *Bottleneck:* the one-key read
    runs three use cases (ADR-0079 d4, #341), which costs nothing against a rate since an admin
    key carries none. *Budget:* the one-key page makes `whoami` (2), then `GET /v2/keys/{key}`,
    whose store calls the build measures and records in `web_console_reads.rs`; the limits and
    deny forms are `POST`s.

- **d5 — The admin Fix form is a tab of the admin workspace page until the Releases view exists
  (D6).** This amends ADR-0083 d9. `/web/admin/workspace?workspace=<w>&view=fix` carries the
  **Fix** form with the acts `POST /v2/admin/fixes` builds today, `demote`, `edit`, `retire` and
  `erase`, each with its target and the required reason, and a confirmation step
  (`…&view=fix&act=<act>&target=<target>`) that says the fix is served at once and waits for a
  person to confirm or revert it, and for `erase` what erasure removes. It posts to
  `/web/admin/fixes`, which calls `POST /v2/admin/fixes` as `<user>-admin`. Release rollback and
  import rollback are absent until ADR-0085 is built; when its Releases view lands, the form
  moves there as d9 first said and the tab goes. No other admin card carries a fix act.

    Refusals shown by name: `admin.fix_requires_person` (`not_used_by_person`, `unlinked`),
    `admin.fix_held_by_scan`, `admin.fix_target_not_promoted`, `workspace.frozen`.

    **Coordinator's readings:**
    - The confirmation names the target by its id and claim key as entered and shows an edit's
      new body as typed; it serves no stored text of the target, since an admin page shows memory
      text only inside an investigation (ADR-0089 d10).
    - The build proves with a test that a `<user>-admin` session passes ADR-0089's person check as
      the person its session row names. If it does not, the session's admin key counts as used by
      and linked to that person, since the session is that person's own sign-in (ADR-0070 d2), and
      the build makes it so; no other admin key gains anything.

    *Doctor:* none new. *Budget:* the tab adds no call to the workspace page's `GET`; the
    confirmation is a `GET` with no call beyond `whoami`.

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

    **Coordinator's reading:** "any key of the person" is every key that names the person as its
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
      proposing key's name, `same_user` (ADR-0046 d2) and when it was proposed; an edge the key
      proposed itself is not listed, since ratification refuses its proposer. Any other caller is
      `access.verb_ceiling`.
    - The group "Brief proposals to ratify" sits after "To review"; its act, **Ratify**, asks for
      confirmation and posts `POST /v2/edges/{id}/ratify` as `<user>-review`.

    Status proposals get no group: they come from reviewer keys alone (ADR-0095 d5) and already
    show on both Workspaces views.

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
  date the server did not store. A date whose sent instant is not later than the page's clock is
  refused on the page, as the API would refuse it; a day its month does not hold rolls over as
  ADR-0079's answer 3 says. The admin limits form (d4) uses the same rule. No script, no CSP
  change (D13).

- **d11 — The import-review items wait.** ADR-0083 d11's "Imported versions to review" needs
  `import_provenance` on `GET /v2/review/held` rows, an `imports` member in `/v2/todo/counts`,
  and the `attests_not_source_author` attestation on the verdict (ADR-0084 d4), none of which the
  API serves. **Coordinator's reading:** the audit lists them as gaps and the operator's
  decisions name none of them, so the round does not build the group or the Judge page with the
  attestation; an imported version stays in To review as the held row it is today. They wait
  for a record of their own, with the attestation.

- **d12 — Deferred.** Recording a key's last use (#131), which waits for a measured per-request
  cost and its own record (D10); the admin key builder (D1); an issuance dry run, whose two
  server-only answers stay on the review step (D12); hashed inline scripts on console pages
  (D13); the import-review items (d11). Adding or revoking a grant on an issued key, revoking and
  rotating keys stay unbuilt, and no page draws them.

## Consequences

- **OpenAPI** (`docs/api/openapi.yaml`): `issueKey`'s session paragraph lists the fields of d1
  and `too_many`; `setKeyLimits` and `addDenial` say what `<user>-admin` may do (d4);
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
