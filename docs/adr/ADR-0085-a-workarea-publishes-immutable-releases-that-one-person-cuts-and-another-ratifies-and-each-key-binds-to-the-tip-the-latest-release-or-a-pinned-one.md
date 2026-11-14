<!--
id: adr/ADR-0085-a-workarea-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one
tier: engineering
status: accepted design · 2026-11-14
answers: what a workarea release is, when its manifest is fixed and what it may contain; how it is named, cut, published and withdrawn, and by whom; which kinds a release governs and what that costs; what a key binds to, how a binding resolves per workarea, and how it meets an in-progress workarea, a key's own writes and an admin's fix; who may widen or narrow a binding; which keys persons and agents use in a shared development workarea and how the console acts with them; who sees which keys a release reaches; what the release notes say and against which baseline; how a released in-progress workarea is read by federation; how this answers ADR-0047 d10; what it measures; and the questions left for the operator
-->

# ADR-0085 — A workarea publishes immutable releases that one person cuts and another ratifies, and each key binds to the tip, the latest release or a pinned one

**Status:** accepted design · 2026-11-14, by the operator, before it is built, with its three questions decided at their
recommendations and every other default as drafted · proposed the same day

**Release:** not scheduled; the first record of the workarea lifecycle
([review](https://github.com/e8kor/open-memory-lab/blob/main/design/workarea-lifecycle.md)), before
ADR-0086, ADR-0087 and ADR-0084, with ADR-0089

**Twin parts** (ADR-0090 d18, PR #388): release manifests, their members, withdrawals and
acknowledgements, and each key's `binding` are journal or part, never excluded; an admin fix's
overlay on a release is derived from the replayed fix, confirm and revert events, as ADR-0089's
twin note and ADR-0090 d18 class it. The build names each column's class under ADR-0090 d4.

**Amends:**
- ADR-0018 (d4: "published" is the word for a ratified release, and still never for a version).
- ADR-0025 (d8: a claim's served tip is the bound release's member for a `latest` or pinned key).
- ADR-0028 (d1, d2: `issue-agent` and `issue` take `--binding`).
- ADR-0079 (a key's binding, like its date and rate, is set at issuance and changed only as d7
  says, never by the key itself).
- ADR-0026 (d15: admins read a release's manifest, notes and gate records).
- ADR-0030:
  - d2, d3: a release-bound read is served the manifest whatever the marks;
  - d7: a release cut inside an in-progress workarea carries its promoted versions out.
- ADR-0037:
  - d5: the guardrail set is read from the bound release;
  - d10: a withdrawal may stop a ratified guardrail binding, by one person with an alert, d9.
- ADR-0059 (d14: a release read beside `as_of`).
- ADR-0072:
  - d1: a released in-progress workarea is a federation source at its latest release, d15;
  - d5: a federated source is read at its latest release.
- On their branches:
  - ADR-0047 (d10, proposed);
  - ADR-0080 (d3: a withdrawal may stop a person-approved skill being offered, d9; d10:
    release-bound keys are not served held skills).

## Context

Today a promotion is a publication. Once a version is promoted, every key with reader access on its
scope recalls it on the next request
([ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
d6 makes the commit the only boundary, and only for an in-progress workarea). There is no staging
step, no name for "what production agents receive", and no rollback faster than a superseding
version that another person must review.

`validated` cannot be that name: it freezes the workarea against supersession
([ADR-0009](ADR-0009-status-ladder.md) d5), which ADR-0047 d10 already rejected for living
knowledge. `as_of` reads the past at an instant
([ADR-0059](ADR-0059-a-memory-states-when-it-is-true-recall-hides-what-has-ended-and-searches-the-dates-a-query-names-without-a-model.md)
d14) but names nothing.

The operator decided on 2026-11-14:

1. **Team delivery.** Team work is developed in a shared workarea such as `payments.dev`, written
   by persons' tenant keys, and the team binds to its releases. Personal roots stay personal
   ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) d3).
2. **Binding wins over in progress.** A `latest` or pinned key is served the release manifest,
   whatever the marks, and never another key's held item.
3. **A release may be cut while in progress.** It is cut from approved versions while the
   workarea stays in progress.
4. **The admin fixes production.**
   [ADR-0089](ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md)
   decides how; this record says how a fix reaches release-bound keys.
5. **Releases govern every kind, memories included.**
6. **An admin's fix reaches release-bound keys as a marked overlay.**
7. **Loosening a guardrail.** An admin's fix or a developer's withdrawal that loosens a guardrail
   is one person's act, with an alert.
8. **Widening a binding.** Only the key's issuer or its own person widens a binding; an admin only
   narrows it; every change is journalled.

This record is held against all four rules of [ADR-0001](ADR-0001-the-constitution.md):

- R2: the proposer and the ratifier differ.
- R3: a release never carries a held row.
- R4: a withdrawal deletes nothing.
- R1: no agent's key cuts, publishes, withdraws or rebinds.

Decided, not built.

## Decision

- **d1 — A release is a manifest of promoted versions, fixed when the cut is proposed.**
  - **What the manifest lists.** For each claim key in the release's subtree (d3), the promoted
    tip at the moment of the proposal: artifact id, version, kind and plaintext digest. Its digest
    is signed in `workarea.release_proposed`, and it never changes afterwards. A later promotion
    is in the next cut.
  - **What it never holds:**
    - a held, rejected, demoted or erased version;
    - a simulation row (ADR-0011 d4).
  - **Unconfirmed fixes block the cut.** A proposal is refused `workarea.release_refused`, reason
    `unconfirmed_fix`, while any member would be a version an unconfirmed admin fix serves (the
    operator, 2026-11-14).
  - **A member can drop out after the cut.** It drops out of every release that lists it when it
    is demoted, erased or retired. A demotion has to reach every reader, as `as_of` already
    behaves (ADR-0059 d15). The release then reads `degraded` and names the member.

- **d2 — Vocabulary: "cut" in prose, `propose` in the API, `published` once ratified.**
  - A release is named `<workarea path>@<label>`, with a semantic-version label. The name is
    unique per workarea, immutable and never released for reuse
    ([ADR-0026](ADR-0026-names-are-keys.md) d1, d10).
  - Its states are `proposed`, `published`, `withdrawn` and `refused`.
  - "Publish" means ratifying a release, and nothing else. A version is promoted, never published.
  - ADR-0018 d4 lists "published" as a word never used for `promoted`. That stays true for a
    version. This record adds one row to that table: **published**, a release a second person
    ratified; never said of a version.

- **d3 — A release covers a workarea and its subtree; a cut is refused across an in-progress
  boundary.**
  - The subtree is one unit, as ADR-0009 d4 treats it for `validated`.
  - A cut is refused, reason `crosses_in_progress`, when any node of the subtree has an
    in-progress root different from the release node's.
  - Inside one in-progress root, a cut is allowed. Held work, by any author, is not in it. This
    amends ADR-0030 d7: a published release carries its promoted versions out of the in-progress
    boundary before the commit.

- **d4 — One person cuts and another publishes, each with a person's tenant dreamer key.**
  - Both keys are tenant keys `held_by: human` with dreamer on the release node, such as
    `alice-dev-review` and `bob-dev-review`, and they belong to different principals.
  - A published release is a production scope (ADR-0001 d5), so the publisher is a person other
    than the proposer, inside a user root too: the same user's second dreamer key never publishes.
  - No agent key, admin key or service key cuts or publishes.
  - A CI service key learns of a proposal from `GET /v2/workareas/{w}/releases?state=proposed`
    or from the events tail (`workarea.release_proposed`). It runs tests and attaches the runs
    (ADR-0086), and that is all it does with a release.

- **d5 — A release governs every kind, memories included, for the keys bound to it.**
  - A key on `latest` or a pin reads every kind from the manifest: memories, steering, terms,
    guardrails, skills, documents and tests.
  - **The cost:** a memory approved in `payments.dev` on Monday reaches the team's production
    agents only when someone cuts and publishes the next release. A team that wants approvals to
    flow at once binds those agents to `tip` and gives up the staging step.
  - **Read-your-writes holds.** A release-bound key that writes is still served its own held rows,
    marked unreviewed, as ADR-0077 d5 serves a caller's own held rows. It is never served another
    key's.
  - **Writes check against the promoted tip.** `expected_version` and `replaces` check against the
    claim's promoted tip, not the release member, because a write supersedes the living claim.

- **d6 — Every key has a binding, `tip`, `latest` or a pin, resolved per workarea; a release-bound
  read is served the manifest whatever the marks.**
  - **`tip`** is today's behaviour, including ADR-0030 d3's and ADR-0080 d10's serving of held
    items inside an in-progress workarea, and ADR-0080 d16's vertical audience: a `tip` key scoped
    in an in-progress subtree or on its ancestor chain reads its held items, marked. Only a `tip`
    key gets that view; a `latest` or pinned key at an ancestor reads the release and never a held
    item. As in ADR-0001 d5, that view never includes a version the scan held or has not scanned,
    nor a held retirement.
  - **`latest`** reads, for each workarea in the read, the newest published, not withdrawn release
    whose subtree holds it.
  - **A pin** is `{<workarea>: <label>}`. A workarea without an entry resolves as `latest`.
  - **No release yet.** A `latest` key on a workarea with no release is served the promoted tip,
    never another key's held item, and every answer carries `binding_fallback: tip`.
  - **A pin to a withdrawn release** keeps being served, marked `withdrawn: true`.
  - **Marks do not apply.** For a `latest` or pinned key, ADR-0030 d2's boundary does not hide a
    member, and ADR-0030 d3's and ADR-0080 d10's serving of held items does not apply. This amends
    ADR-0030 d2, d3 and ADR-0080 d10.
  - **What reads the manifest.** Recall, the listing, get, the skill and guardrail reads, and the
    MCP prompts and resources (amends ADR-0037 d5).
  - **Served tip.** ADR-0025 d8's served tip is, for a `tip` key, the default or overlay tip as
    today. For a release-bound key it is the bound release's member, with an admin fix overlaid
    (next bullet). A claim with no member is not served to that key.
  - **An admin fix overlays every release that lists its claim, pinned ones included** (ADR-0089
    d5):

    | Fix | What release-bound keys are served instead of the member |
    |---|---|
    | demote | the claim's previous promoted version |
    | edit | the admin's version |
    | retire | nothing |
    | import rollback | each affected member as above |

    The served item is marked `approver.act: "admin_fix"`. The release reads `degraded` and
    `admin_fix` until a person confirms the fix, then `fixed`. A revert removes the overlay.

- **d7 — Binding defaults, issuance and change: widened by the key's issuer or its own person,
  narrowed by an admin, journalled always.**
  - **Defaults** (the operator, 2026-11-14):
    - `tip` for any key whose scope node lies in a user root, and for any key held by a person;
    - `latest` for every other key held by an agent or a service;
    - keys issued before this record keep `tip`.
  - **Development agent keys.** An admin, or the person issuing a key in their own root, may issue
    an agent key at `tip` outside a user root only when the key's scope node lies inside, or at an
    ancestor of, an in-progress root, the audience of ADR-0080 d16. The
    development agents of `payments.dev` are issued that way.
  - **Directions.** Widening means toward newer content: `latest` or a pin to `tip`, or a pin to a
    newer label. Narrowing means the reverse.
  - **Who may widen:**
    - the key that issued the key, when it is a person's key;
    - a key of the person the key belongs to (same `user`, or same principal, `held_by: human`)
      that holds dreamer on every workarea the change affects.
  - **Admins only narrow.** An admin key may narrow any key's binding and never widen one after
    issuance.
  - **The key itself** never changes its own binding.
  - Every change is journalled as `key.binding_set {key, from, to, by_kind}`.
  - The doctor names every key on `tip` whose scope node is outside any in-progress root and has a
    published release.

- **d8 — Who a release reaches is a consumers read: counts for the ratifier, key names for
  admins.**
  - `GET /v2/workareas/{workarea}/consumers` answers the number of keys bound `tip`, `latest` and
    pinned whose scope reaches the node, and the receiving scopes.
  - A person with dreamer on the node reads the counts and scopes. An admin key also reads key
    names.
  - The ratifier is shown the counts before publishing.

- **d9 — Withdrawing returns readers to the previous release, deletes nothing, and takes one
  person, with an alert when it loosens a guardrail.**
  - **Who.** One person's dreamer key with dreamer on the node.
  - **The record.** A withdrawal is journalled as `workarea.release_withdrawn {release, reason,
    loosens_guardrail}`. The release stays listed as withdrawn.
  - **Effect.** `latest` keys read the previous published release on their next request, and its
    guardrail and skill sets at their next session start.
  - **When a guardrail loosens.** The previous release may lack a guardrail, or hold it at lower
    severity, or lack a skill. The withdrawal still takes effect at once, and it:
    - raises `oml_release_withdrawals_total{loosens_guardrail="true"}`;
    - raises a doctor alert until a person of another principal acknowledges it
      (`workarea.release_withdrawal_acknowledged`).
  - This amends ADR-0037 d10 and ADR-0080 d3, which made every change to what binds a ratified
    act.
  - An admin may also withdraw, as a fix under the same rule (ADR-0089).

- **d10 — Release notes are generated against the latest published release, and breaking changes
  are flagged.**
  - **Baseline.** The latest published release, with a link to the last cut whatever its state.
  - **Contents.** By kind, every claim key added, changed, retired or dropped. Per item: the
    version, the reviewer's kind and `same_user`, never the reviewer's name
    ([ADR-0077](ADR-0077-a-served-memory-says-when-it-was-approved-and-by-what-kind-of-reviewer-and-never-names-the-reviewer.md) d2).
    Then the attached test runs and impact report (ADR-0086, ADR-0087).
  - **Breaking:**
    - a skill retired, dropped or renamed;
    - a guardrail retired, dropped or of lower severity;
    - a document key removed.
  - **Confirming breaking items.** The proposer must confirm each breaking item. The label must
    raise its major number, or the proposal is refused, reason `label_not_major`.
  - **Storage.** The notes are stored as the release's own document claim (ADR-0081, proposed).

- **d11 — The change set is computed review UI, and a revert drafts the inverse.**
  - A change set is the subtree's versions since the latest published release, shown with a line
    diff per claim key against it. It is computed on read and never stored.
  - Review stays per item
    ([ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md) d4).
  - `oml workareas revert --to <label>` writes, as the calling person's writes:
    - held supersessions back to that release's text;
    - held retirements of keys that release did not have.

    They are reviewed and cut like any change.

- **d12 — Surfaces, events, doctor lines and metrics.**
  - **HTTP:**
    - `POST /v2/workareas/{w}/releases` (propose);
    - `.../releases/{label}/ratify` (publish);
    - `.../releases/{label}/withdraw`;
    - `.../releases/{label}/acknowledge`;
    - `GET .../releases[?state=]`;
    - `GET .../releases/{label}` (manifest, notes, gate records, overlays);
    - `GET .../consumers`;
    - `POST /v2/keys/{key}/binding`.
  - **CLI:**
    - `oml workareas release propose|publish|withdraw|acknowledge|list|show`;
    - `oml workareas revert`;
    - `oml keys bind`.
  - **Console:** a Releases section on a workarea's personal page. No MCP tool (R1).
  - **Events:** `workarea.release_proposed`, `workarea.release_ratified`,
    `workarea.release_withdrawn`, `workarea.release_withdrawal_acknowledged`, `key.binding_set`.
    `recall_events` gains the release a read was served from (ADR-0054).
  - **Doctor:**
    - agent keys on `tip` outside an in-progress root;
    - `latest` keys served `binding_fallback`;
    - pins to withdrawn releases;
    - degraded releases;
    - unacknowledged loosening withdrawals.
  - **Metrics:**
    - `oml_key_bindings{binding}`;
    - `oml_binding_fallback_total`;
    - `oml_release_read_join_seconds` (a histogram of the manifest join per read);
    - `oml_release_withdrawals_total{loosens_guardrail}`.

- **d13 — Publishing stays with persons; the admin view reads every release and rolls one back as
  a fix.**
  - An admin key neither cuts nor publishes (d4).
  - The admin release view reads every release of its tenant: manifest, notes, gate records and
    overlays. Each read is journalled with a reason (ADR-0089; amends ADR-0026 d15).
  - It offers ADR-0089's release rollback: withdraw a release, or re-point a workarea's `latest`
    to an earlier published release.
    - Both are served at once, marked `latest_by: "admin_fix"`, and queued for a person to confirm
      or revert.
    - A person's next publish ends a re-point.

- **d14 — This answers ADR-0047 d10 without a frozen child.**
  - ADR-0047 d10 creates a `<repo>-r-<version>` child that freezes as evidence, because freezing
    the living workarea would make its facts uncorrectable.
  - A manifest gives the same evidence, immutable and signed, without copying content or freezing
    anything. The release's notes document is the record of known issues ADR-0047 d10 wanted.
  - When ADR-0047 is built, its release step cuts a release of the repository workarea under this
    record.

- **d15 — A released in-progress workarea is a federation source, read at its latest release.**
  - ADR-0072 d1 trusts only `committed` and `validated` nodes outside every in-progress subtree.
  - This record adds: an in-progress node with a published release is a trusted source, and a
    federated read of it serves its latest release's members only (the operator, 2026-11-14).

- **d16 — In a shared workarea, the console acts with a person's tenant key linked to their
  sign-in.**
  - The console's own keys (`<user>-web`, `<user>-review`) hold grants only inside the user's root
    (ADR-0044 d3).
  - An admin links a tenant key `held_by: human` to a signed-in person, journalled as
    `key.linked_person`. The console then offers that key for acts in the workareas it grants, one
    key per act, named on the page (the operator, 2026-11-14).

- **d17 — A user root never leaves development** (the operator, 2026-11-14: "Cap at
  development"). A user root and every workarea under it stay `development`: a proposal moving
  any of them to `committed` or `validated` is refused `workarea.transition_refused` with reason
  `user_root`. Production work happens in shared workareas, which publish releases. This amends
  ADR-0046 d1 (the ceiling, from `committed` to `development`) and d2 (same-user ratification of
  ladder moves, which has no move left to ratify inside a root). The cost: a person's own
  knowledge reaches a production scope only by a new write in a shared workarea, reviewed there,
  or a release cut there. The build today lets a user root reach `committed` (the `UserRoot`
  ceiling in `ladder.rs`); lowering it is a build item of this record, not built.

## Consequences

- A migration adds releases, members, overlays, withdrawals and acknowledgements. `api_keys` gains
  `binding`.
- **Read-path cost.** Every read path resolves the binding before its store statement, so a
  release-bound read filters by the manifest before any `LIMIT`, as ADR-0030 d2 requires. The join
  costs latency per read, which is unmeasured. It is a bottleneck row with the build and is
  measured by `oml_release_read_join_seconds`.
- `as_of` and release reads answer different questions, and both stay.
- ADR-0084's `release` mode exports a manifest. ADR-0086's gates and ADR-0087's report attach to a
  proposal.
- Key binding stays in this record rather than its own: a release with no bound reader decides
  nothing, and splitting would put the read rules of d5 and d6 in one record and the object they
  read in another.
- The operator's answers of 2026-11-14, each the recommendation: a cut over an unconfirmed fix is
  refused (d1); a released in-progress workarea federates at its latest release (d15); the console
  acts through linked tenant keys (d16). The To do page lists Releases to ratify and Withdrawal
  alerts to acknowledge for a person's dreamer keys (ADR-0083 d11).

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **`validated` as the release** | It freezes the workarea against supersession (ADR-0009 d5), so a fact found wrong after release stays uncorrectable. |
| **A frozen release child holding copies (ADR-0047 d10 as written)** | It copies every member and doubles review and storage, and the frozen copies cannot be demoted without a second path. |
| **A release as an `as_of` instant** | An instant names no set two people can check against each other; today's access and retirement change what it returns (ADR-0059 d15), and it cannot exclude `self_signed` or simulation rows. |
| **A manifest recomputed until publication** | The tests and impact report would certify a set that moves under them. |
| **Releases govern guardrails and skills only; memories flow live** | Two bindings per key, and a production agent would read a memory no release vetted beside instructions a release did; the operator chose one rule. |
| **CI cuts releases** | An automated actor would choose what a person is asked to publish. |
| **Release-bound keys still see others' held items inside in progress** | A production key would read unreviewed text whenever its workarea is in progress. |
| **The key chooses its binding per request** | The thing gated would choose its own gating (R1). |
| **Two persons to withdraw a release that loosens a guardrail** | The operator ruled it too slow for a bad publish; an alert and an acknowledgement keep it visible. |
| **Admins publish releases** | Publishing is the reviewed path; the admin's acts are fixes, marked and confirmed afterwards (ADR-0089). |
| **Key binding as its own record** | See Consequences: the read rules and the object they read belong together. |
