<!--
id: adr/ADR-0085-a-workarea-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one
tier: engineering
status: proposed
answers: what a workarea release is and what it may contain; how it is named, proposed, published and withdrawn, and by whom; what a key binds to and how a binding resolves per workarea; how a release-bound key and an in-progress workarea meet; whether a release can be cut while the workarea is in progress; who may change a binding; who sees which keys a release reaches; what the release notes say and which changes they flag as breaking; how this answers ADR-0047 d10's frozen release child; which admin acts end; and the questions left for the operator
-->

# ADR-0085 — A workarea publishes immutable releases that one person cuts and another ratifies, and each key binds to the tip, the latest release or a pinned one

**Status:** proposed · 2026-11-14

**Release:** not scheduled; the first record of the workarea lifecycle
([review](https://github.com/e8kor/open-memory-lab/blob/main/design/workarea-lifecycle.md)), before
ADR-0086, ADR-0087 and ADR-0084

**Amends:** ADR-0009 (d3: the ratifier of a status move is a person's dreamer key, never an admin's),
ADR-0010 (d6: an admin key neither demotes nor ratifies a status move; erasure stays),
ADR-0026 (d15: admins read release status), ADR-0030 (d2, d3: a release-bound read is served the
manifest whatever the marks; d7: a release cut inside an in-progress workarea carries its promoted
versions out), ADR-0037 (d5: the guardrail set is read from the bound release), ADR-0059 (d14: a
release read beside `as_of`), ADR-0072 (d5: a federated source is read at its latest release), and,
on their branches, ADR-0047 (d10, proposed) and ADR-0080 (d10, proposed)

## Context

A promotion is a publication today: once a version is promoted, every key with reader access on its
scope recalls it on the next request
([ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
d6 makes the commit the only boundary, and only for an in-progress workarea). There is no staging
step, no name for "what production agents receive", and no rollback faster than a superseding
version that another person must review. `validated` cannot be that name: it freezes the workarea
against supersession ([ADR-0009](ADR-0009-status-ladder.md) d5), which ADR-0047 d10 already
rejected for living knowledge. `as_of` reads the past at an instant
([ADR-0059](ADR-0059-a-memory-states-when-it-is-true-recall-hides-what-has-ended-and-searches-the-dates-a-query-names-without-a-model.md)
d14) but names nothing, and guardrails and skills have no as-of read.

On 2026-11-14 the operator decided four points this record takes as given:

1. Team work is developed in a shared workarea such as `payments.dev`, written by persons' tenant
   keys, and the team binds to its releases; no copy verb is added and personal roots stay
   personal ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) d3 unchanged).
2. Binding wins over in progress: a `latest` or pinned key is served the release manifest by
   artifact id whatever the workarea's marks, and never sees held items. Only a `tip` key is served
   as ADR-0030 d3 and ADR-0080 d10 serve.
3. A release may be cut from approved versions while the workarea stays in progress; held work by
   others is not in it.
4. Admin acts keep erasure only, as a legal act; admin demotion and admin ratification of status
   moves end; admins read release status without claim keys.

Held against R1 (a release changes what agents receive, so it is a person's act), R2 (proposer and
ratifier differ), R3 (a release never carries a held row) and R4 (a withdrawal deletes nothing)
([ADR-0001](ADR-0001-the-constitution.md)). Decided, not built.

## Decision

- **d1 — A release is an immutable manifest of promoted versions.** It lists, for each claim key in
  its subtree (d3), the promoted tip at the cut: artifact id, version, kind and plaintext digest.
  The manifest's digest is signed in the release events (d12). A manifest never holds a held,
  rejected, demoted or erased version, a `self_signed` row (ADR-0001 d5 keeps those inside their
  development workarea), or a simulation row (ADR-0011 d4). A version demoted or erased after the
  cut is no longer served from any release that lists it, as `as_of` already behaves (ADR-0059 d15):
  a demotion has to reach every reader. The release then reads `degraded`, and its listing names
  the member that dropped out.

- **d2 — Vocabulary: a version is promoted, a release is published.** A release is named
  `<workarea path>@<label>`, the label a semantic version, unique per workarea, immutable and never
  released for reuse ([ADR-0026](ADR-0026-names-are-keys.md) d1, d10). "Publish" means ratifying a
  release proposal and nothing else; "promote" keeps its meaning, a verdict on a version.

- **d3 — A release covers a workarea and its subtree, and its cut is refused across an in-progress
  boundary.** The subtree is one unit, as ADR-0009 d4 already treats it for `validated`. A cut is
  refused `workarea.release_refused` with reason `crosses_in_progress` when any node of the subtree
  has an in-progress root different from the release node's. Inside one in-progress root, a cut is
  allowed: the manifest takes only promoted versions, so held work, by any author, is not in it.
  This amends ADR-0030 d7: a published release carries its promoted versions out of the in-progress
  boundary before the commit.

- **d4 — One person's dreamer key proposes, a different person's dreamer key publishes; no agent,
  admin or service key does either.** The proposer and the ratifier each hold dreamer on the release
  node and are held by a human (`held_by: human`). They are different keys and different principals.
  Inside a user root, the same user's second dreamer key may ratify, and the release is marked
  `same_user` (as ADR-0046 d2 marks a status move); outside one, they are two users. A CI service key
  runs tests and attaches their runs (ADR-0086) and never proposes or ratifies: a proposal by CI
  would let an automated actor choose what a person is asked to publish.

- **d5 — Every key has a binding: `tip`, `latest` or a pin, resolved per workarea.**
  - `tip` is today's behaviour, including ADR-0030 d3's serving of held memories and ADR-0080 d10's
    serving of held skills and guardrails inside an in-progress workarea.
  - `latest` reads, for each workarea in the read, the newest published and not withdrawn release
    whose subtree holds it.
  - A pin is `{<workarea>: <label>}`, one or more entries; a workarea without an entry resolves as
    `latest`.
  - Resolution is per workarea: a read spanning `payments.dev` and `payments.docs` serves each from
    its own release.
  - A `latest` key on a workarea with no release is served the promoted tip, never a held item, and
    every answer carries `binding_fallback: tip`.
  - A pin to a withdrawn release keeps being served, marked `withdrawn: true`, because the pin was
    chosen; a harmful member is handled by demotion (d1), which reaches every release.
  - Defaults: `latest` for a key held by an agent, `tip` for a key held by a person. Keys issued
    before this record keep `tip`, and the doctor names every agent key on `tip` whose scope has a
    published release.

- **d6 — A release-bound read is served the manifest by artifact id, whatever the workarea's
  marks.** For a `latest` or pinned key, ADR-0030 d2's boundary does not hide a release member,
  and ADR-0030 d3's and ADR-0080 d10's serving of held items does not apply: such a key never sees
  a held item, inside or outside an in-progress root. Recall, the listing, get, the skill and
  guardrail reads and the MCP prompts and resources all read the manifest's versions. The guardrail
  set is the bound release's (amends ADR-0037 d5). This amends ADR-0030 d2 and d3 and ADR-0080 d10.

- **d7 — A binding is set at issuance and changed only by a person, toward newer content only with
  dreamer access; an admin key may only narrow it; a key never changes its own.** Toward newer
  content means `latest` or a pin to `tip`, or a pin to a newer label. That change is a person's
  dreamer key with dreamer on every workarea it affects. Narrowing means `tip` to `latest`, `latest`
  to a pin at the current label, or a pin to an older label. An admin key may narrow, and may issue
  a key at `latest` or a pin but not at `tip` for a key held by an agent. Every change is journalled
  as `key.binding_set {key, from, to}`, as ADR-0079 treats a key's date.

- **d8 — Who a release reaches is a consumers read: counts for the ratifier, key names for admins.**
  `GET /v2/workareas/{workarea}/consumers` answers the number of keys bound `tip`, `latest` and
  pinned whose scope reaches the node, and the receiving scopes. A person with dreamer on the node
  reads counts and scopes; an admin key also reads key names, since admins manage keys
  (ADR-0026 d15). The ratifier is shown the counts before publishing.

- **d9 — Withdrawing returns readers to the previous release and deletes nothing.** A withdrawal is
  journalled as `workarea.release_withdrawn {release, reason}`, and the release stays listed as
  withdrawn. `latest` keys read the previous published release on their next request, and their
  guardrail and skill sets at the next session start. One person's dreamer key may withdraw, unless
  the withdrawal removes or loosens a guardrail that the withdrawn release served. In that case a
  second person ratifies, because one person must not be able to switch off a reviewed rule
  (*operator to confirm*). The tip is fixed afterwards by a revert (d11).

- **d10 — Release notes are generated, and breaking changes are flagged.** The notes list, by kind,
  every claim key added, changed, retired or dropped against the previous release. For each item
  they give the version, the reviewer's kind and `same_user`, never the reviewer's name
  ([ADR-0077](ADR-0077-a-served-memory-says-when-it-was-approved-and-by-what-kind-of-reviewer-and-never-names-the-reviewer.md) d2),
  then the attached test runs and impact report (ADR-0086, ADR-0087). These are **breaking**:
  - a skill retired, dropped or renamed (a name no longer offered);
  - a guardrail retired, dropped or of lower severity;
  - a document key removed.

  The proposer must confirm each breaking item before the proposal is accepted, and the label must
  raise its major number or the proposal is refused with `release_label_not_major`. The notes are
  stored as the release's own document claim (ADR-0081, proposed), reviewed with the release.

- **d11 — The change set is computed review UI, and a revert drafts the inverse.** A change set is
  the subtree's versions since the latest release, shown with a line diff per claim key against
  the release. It is computed on read, never stored, and has no event and no note. Review stays
  per item ([ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)
  d4). `oml workarea revert --to <label>` writes held supersessions back to that release's text,
  and held retirements of keys it did not have, as the calling person's writes; they are reviewed
  and published like any change.

- **d12 — Surfaces, events and doctor lines.**
  - HTTP: `POST /v2/workareas/{w}/releases` (propose), `.../releases/{label}/ratify` (publish),
    `.../releases/{label}/withdraw` (with `.../ratify` under d9), `GET .../releases`,
    `GET .../releases/{label}` (manifest, notes, gate records), `GET .../consumers`, and
    `POST /v2/keys/{key}/binding`.
  - CLI: `oml workarea release propose|publish|withdraw|list|show`, `oml workarea revert`,
    `oml keys bind`.
  - Console: a Releases section on a workarea's personal page. No MCP tool (R1).
  - Events: `workarea.release_proposed`, `workarea.release_ratified`, `workarea.release_withdrawn`
    and `key.binding_set`, each with the manifest digest. `recall_events` gains the release a read
    was served from (ADR-0054).
  - Doctor: agent keys on `tip` beside a release; `latest` keys served `binding_fallback`; pins to
    withdrawn releases; degraded releases.

- **d13 — Admin acts on content end, except erasure, and admins read release status.** This amends
  ADR-0010 d6 by removing demote and status-move ratification from admin's verbs, and ADR-0009 d3:
  a status move is ratified by a person's dreamer key that did not propose it; an admin may still
  propose one, since a proposal promotes nothing. Erasure stays an admin act, as a legal act
  ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md)). Demotion is left to the record that builds
  it, as a person's act. ADR-0026 d15 gains a release status read for admins: labels, state,
  proposer and ratifier kind, `same_user`, member counts by kind and digests, and no claim keys.

- **d14 — This answers ADR-0047 d10 without a frozen child.** ADR-0047 d10 creates a
  `<repo>-r-<version>` child that freezes as evidence, because freezing the living workarea would
  make its facts uncorrectable. A manifest gives the same evidence, immutable and signed, without
  copying content into a child or freezing anything; the living workarea stays correctable, and the
  release's notes document is the record of known issues ADR-0047 d10 wanted. When ADR-0047 is
  built, its release step publishes a release of the repository workarea under this record.

## Consequences

- A migration adds releases, members and withdrawals; `api_keys` gains `binding`.
- Every read path resolves the binding before its store statement, so a release-bound read filters
  by the manifest before any `LIMIT`, as ADR-0030 d2 requires of its boundary. The cost is a join
  against the members table per read, which is unmeasured; it is a bottleneck row to open with the
  build.
- `as_of` and release reads are different questions and both stay: `as_of` answers "what was the
  tip at an instant", a release "what did people publish".
- ADR-0084's `release` mode exports a manifest; ADR-0086's gates and ADR-0087's report attach to a
  proposal.
- The roles page loses demote and ratify from the admin row; the ladder's code
  (`crates/oml-app/src/app.rs:4660`, `ratify_admitted`) refuses an admin key.
- Questions for the operator, recommendation first:
  1. Default binding: `latest` for agent keys and `tip` for person keys, or `tip` everywhere?
  2. Withdrawal: one person, two when it removes or loosens a guardrail (d9), or always two?
  3. A pin to a withdrawn release: served and marked (d5), or refused?
  4. Should a breaking change force a major label (d10), or only be flagged?

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **`validated` as the release** | Freezes the workarea against supersession (ADR-0009 d5), so a fact found wrong after release stays uncorrectable; ADR-0047 d10 rejected it for the same reason. |
| **A frozen release child holding copies (ADR-0047 d10 as written)** | Copies every member, doubles review and storage, and the frozen copies cannot be demoted without a second path; a manifest by artifact id gives the same evidence. |
| **A release as an `as_of` instant** | An instant names no set; members promoted within one instant of the cut are ambiguous, and an instant cannot exclude `self_signed` or simulation rows. |
| **CI proposes releases** | An automated actor would choose what a person is asked to publish; the person's act would shrink to approving a list a machine drew up. |
| **Release-bound keys still see held items inside in progress** | Would make a "production" key read unreviewed text whenever its workarea is in progress, which the operator ruled out. |
| **The key chooses its binding per request** | The gated thing would choose what gating applies to it (R1). |
| **Admins read the manifest's claim keys** | Claim keys can carry content (`doc/<path>`, `skill/<name>`); ADR-0026 d15 keeps admins to structure. |
