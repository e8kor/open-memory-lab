<!--
id: adr/ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards
tier: engineering
status: accepted design · 2026-11-14
answers: which acts an admin key may take on a workspace's content, on which targets, and with which key; why each takes effect at once and what makes it break-glass; how an admin edit is scanned; how a fix reaches keys bound to a release; how a fix that loosens a guardrail is alerted; what readers see; who confirms or reverts a fix, who authors a revert and what it may not revert; what an admin may read and on which surfaces; what happens in a frozen workspace and in universe; exactly which constitutional sentences this changes and which earlier record already changed one; the person-side read, doctor lines and metrics; and the questions left for the operator
-->

# ADR-0089 — An admin fixes a production workspace at once by a marked break-glass act that a person confirms or reverts afterwards

**Status:** accepted design · 2026-11-14, by the operator, before it is built, with its five questions decided at their
recommendations and every other default as drafted, and its amendment of ADR-0001 d5 accepted with
it · proposed the same day · amended 2026-11-17 by the operator (d16: demote's targets, the
fix route as the one surface for demote and erase, erase's claim-wide scope, a fix's events; as-of reads skip a demoted version) · readings of 2026-11-18 recorded for the operator's batch (d17) · amendments of d8 of 2026-11-18, from the operator's batch (strict confirmation wherever the fixed version is served to production and for any target outside the admin's own root; a cut refuses a `same_user`-only member; #429), accepted 2026-11-19 by the operator · amended 2026-11-19 by the operator (d16, O8: a claim whose approved versions are all demoted is written again, continuing its numbers; #474) · amended 2026-11-19 by the operator after the review of #498 (an admin key is linked to its person; the settlement fails closed; a revert checks R2 by person; proposed for the record, see the decision at the end) · amended 2026-11-19 by the operator after the second review of #498 (a link of an admin or reviewer key takes effect only once its person accepts it; each fix records its admin's person, which the settlement compares; see the second decision at the end) · reading of 2026-11-19 in d17 (a fix whose mark a later fix takes over is settled `superseded`)

**Release:** built in v0.8.4, except the release overlays (d5), which wait for ADR-0085

**Twin parts** (ADR-0090 d18, PR #388): an `admin.fix` and its reason, its confirmation or
revert, and an investigation's reason are journal or part, never excluded. A release's overlay
state (`degraded`, `admin_fix`, then `fixed`, d5) is derived: the replayed fix, confirm and revert
events recompute it. An investigation session's binding of each read to its reason (d10) is
journal, carried by every `memory.admin_read`'s session id; the live session row itself is
excluded, as web sessions are (ADR-0090 d5). The build names each column's class under ADR-0090
d4.

**Amends:**
- ADR-0001: d5 gains its third bounded exception (d14), edited into ADR-0001's text and accepted
  with this record on 2026-11-14.
- ADR-0010: d6's admin verbs and its sentence "Edit or create by hand is this path and no other".
- ADR-0026 (d15) and ADR-0045 (d21): admin reads carry a reason.
- ADR-0077:
  - d2: `approver` gains `act` and `confirmed`;
  - d5: the read-path law admits a marked fix.
- ADR-0009 (d5): a frozen workspace refuses an admin edit or retirement.
- ADR-0025 (d11): an admin's demotion can be reverted.
- ADR-0011 (d3): outside a simulation, a marked admin fix changes served content before review.
- ADR-0044 (d7): an admin may fix `universe`, marked.
- ADR-0037 (d10) and, on its branch, ADR-0080 (d3): a fix may loosen a guardrail or withdraw a
  skill alone, with an alert.

## Context

On 2026-11-14 the operator said: "let's make focus on developers scenarios and admin should be the
person who can fix workareas on prod, so, all types of action, demote, edit, rollback imports for
example". The operator then decided:

- an admin's fix takes effect at once and is marked;
- admins read claim text, each read journalled;
- a fix reaches release-bound keys as a marked overlay;
- an admin may loosen or remove a guardrail alone, with an alert.

The developer path to fixing a bad publish is to withdraw the release and fix forward
([ADR-0085](ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md)
d9, d11). That path is too slow in two cases:

- the fix needs another person's review (R2), and nobody is there;
- the version is served to `tip` keys, or is in no release, so there is nothing to withdraw.

What exists today:

- Demotion is designed (ADR-0009 d5, ADR-0010 d6) and not built: no transition reaches `Demoted`
  (`crates/oml-domain/src/model.rs:139`).
- An admin key cannot hold reviewer grants (`crates/oml-domain/src/access.rs:131`).
- An admin key already reads every memory of its tenant, held rows included, on two routes, each
  read journalled as `memory.admin_read`
  ([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d21; `crates/oml-app/src/app/pages.rs:1081`).

Decided, not built.

## Decision

- **d1 — A fix is defined by its target, and is taken by an admin key used by a person.**
  - The target is any version, release or import of the admin key's tenant, whatever its
    workspace's status or mode, except as d12 and d13 restrict.
  - The admin key must be `used_by: person` (the operator, 2026-11-14). A service-held admin key is
    refused `admin.fix_requires_person`.
  - No agent key, service key or operator key takes a fix, on any transport.

- **d2 — Six fix acts.** A ladder move is not one of them: it keeps ADR-0009 d3's two
  identities, so no admin moves a workspace alone.

  | Act | Effect |
  |---|---|
  | **demote** | A version stops being served. The claim's tip becomes its previous promoted version if there is one, and the bytes are kept (ADR-0009 d5). |
  | **edit** | A supersession written by the admin key becomes the tip at once, under d4. |
  | **retire** | A retirement by the admin key takes effect at once. |
  | **roll back a release** | Withdraw the release, or re-point a workspace's `latest` to an earlier published release until a person publishes again (ADR-0085 d13). |
  | **roll back an import** | For one `workspace.imported` event ([ADR-0084](ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d11), each promoted version it brought in is demoted, restoring the tip it replaced, and each claim key it created is retired. Its held versions are marked `import_rolled_back`. |
  | **erase** | A crypto-shred, as a legal act ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d2). |

- **d3 — Every fix is break-glass.**
  - It takes effect in its own transaction, never waiting for review.
  - It is journalled as `admin.fix {act, targets, reason, loosens_guardrail}`. The reason is
    required, stored in clear and scanned (ADR-0074).
  - In the same transaction it opens a confirmation item for the persons who hold reviewer on the
    target's workspace (d15).

- **d4 — An admin edit's body passes the scan its kind requires.**
  - The body is scanned like any write of its kind (ADR-0074, including d24's instruction kinds).
  - A veto refuses the edit with `submission.scan_veto`.
  - A finding that would hold the write also refuses the edit as a fix
    (`admin.fix_held_by_scan`; d4 named it `admin.fix_used_by_scan`, a slip corrected 2026-11-18,
    d17), because a fix serves at once and a held finding needs a person
    first. The admin can demote instead, which serves already-reviewed text.

- **d5 — A fix reaches release-bound keys as a marked overlay.** For every release that lists the
  claim, pinned ones included (ADR-0085 d6):
  - **demote:** the release serves the previous promoted version;
  - **edit:** it serves the admin's version;
  - **retire:** it serves nothing for that member;
  - **import rollback:** each member is handled as above.

  The release reads `degraded` and `admin_fix` until a person confirms the fix, then `fixed`. A
  revert removes the overlay.

- **d6 — A fix that loosens a guardrail is one person's act, with an alert.** A demotion,
  retirement, edit or rollback can remove or weaken an instruction of any kind of the class
  (ADR-0080 d16): withdraw a steering row, a guardrail or a skill, or lower a guardrail's severity.
  It still takes effect at once. It also:
  - sets `loosens_guardrail: true` on the event;
  - increments `oml_admin_fixes_total{act, loosens_guardrail="true"}`;
  - holds a doctor alert open until a person confirms or reverts the fix.

  This amends ADR-0037 d10 and ADR-0080 d3, which made every change to what binds a ratified act.

- **d7 — Readers see the mark.**
  - ADR-0077 d2's `approver` gains `act`: `verdict` for a version promoted by review, `admin_fix`
    for a version served because of a fix (an edit or retirement, or a version a demotion or
    rollback made the tip again).
  - It also gains `confirmed`: `false` until confirmed, `null` when `act` is `verdict`.
  - For a fix, `used_by` is `person`. `approver.same_user` is `false` until the fix is confirmed,
    and `true` once a development-scope confirmation is by the admin's own person (d8). No reader
    learns which admin.
  - A release re-pointed by an admin reads `latest_by: "admin_fix"`.
  - Agents see the mark on every surface, MCP included.

- **d8 — In a production scope, a person other than the admin's own confirms or reverts the fix;
  in a development scope the admin's own person may. A revert is authored by the server and may
  not revert a superseded fix.**
  - **Who, in a production scope.** A person's reviewer key (`used_by: person`) with reviewer on the
    workspace, of a person other than the admin's own (never `same_user`). In a development scope, the rule below lets the admin's own person confirm,
    marked `same_user`. *(Superseded outside the admin's own root 2026-11-18, accepted 2026-11-19 by the operator: see the amendments at the end of this record.)*
  - **Confirm** journals `admin.fix_confirmed`.
  - **Revert** journals `admin.fix_reverted` and writes the inverse as new state:
    - a demoted version returns as a new version carrying its text;
    - an edit is superseded by a new version carrying the previous text;
    - a retirement is followed by a new version carrying the retired text;
    - a release rollback is undone by re-pointing;
    - an import rollback is undone member by member.
  - **Authorship of a revert.** Each restored version is authored by the server under the fix id,
    with `derives_from` naming the original. It is promoted by the reverting person's verdict. R2
    excludes from that verdict the original version's lineage and the admin's principal.
  - **Superseded fixes.** A fix whose target has since been superseded by a promoted version is
    refused revert, `admin.fix_superseded`: the claim has moved on, and a person fixes it forward.
  - **Erasure** is confirmed and never reverted.
  - **Loose in development, strict in production** (the operator, 2026-11-14: "decisions are loose
    for in-development\in-progress areas, but strict once we are publishing and preparing for
    production"). On a target in a `development` workspace or an in-progress subtree, a reviewer
    key of the admin's own person may confirm or revert the fix, the confirmation marked
    `same_user`. On a target in a `protected` or `frozen` workspace, and for the overlay a fix
    places on a published release (d5), only a person other than the admin's own (never `same_user`) confirms or reverts.
    The cost: a one-person tenant confirms its own fixes in development, but a fixed item stays
    unconfirmed in a production scope, excluded from every cut (ADR-0085 d1), until a second
    person confirms it. *(Superseded outside the admin's own root 2026-11-18, accepted 2026-11-19
    by the operator: see the amendments at the end of this record.)*
  - Every row a fix touched stays (R4).

- **d9 — ADR-0077 d5's read-path law gains the fix.** Every served hit must satisfy one of:
  - it is served unreviewed;
  - its approving verdict is by another key and principal than its author's;
  - it carries `approver.act: "admin_fix"`, and an `admin.fix` event names it.

  A hit with `confirmed: true` also has an `admin.fix_confirmed`: in a production scope by a person
  other than the admin's own; in a development scope it may be by the admin's own person, and the
  hit's `approver.same_user` is then `true`. *(Superseded outside the admin's own root 2026-11-18, accepted 2026-11-19 by the operator: see the amendments at the end of this record.)*

- **d10 — An admin reads claim text tenant-wide, as ADR-0045 d21 already serves it, with one
  reason per investigation.**
  - The reach is ADR-0045 d21's, already built: every memory of the tenant, every state.
  - A reason is given once per investigation session. The session is a server-side id bound to
    every read made under it. The reason is stored in clear and scanned.
  - Each read writes `memory.admin_read` with the ids served and the session id.
  - `oml_admin_reads_total` counts reads.
  - ADR-0026 d15's "never claim content" now reads "never without a reason, and never on an agent
    surface".

- **d11 — Admin keys stay off the agent surfaces.**
  - These remain refused to an admin key with `access.verb_ceiling`:
    - MCP: `oml_read`, `oml_write`, `oml_workspace`, prompts and resources;
    - `/v1` recall, listing and get;
    - the session-start and guardrail hooks.
  - Why:
    - an agent surface puts text into a model's context, where no reason is asked;
    - recall moves usage telemetry (ADR-0054);
    - separate surfaces keep "the gate is not callable by the thing being gated" true for every
      agent key.

- **d12 — In a frozen workspace an admin demotes and erases, and never edits or retires.**
  - A `frozen` workspace admits demotion, erasure and crew metadata (ADR-0009 d5).
  - An admin edit or retirement there is refused `workspace.frozen` (the operator, 2026-11-14).

- **d13 — In `universe` an admin may edit and retire, marked.**
  - A `universe` row served because of a fix carries `approver.act: "admin_fix"` and nothing else.
    This is an exception to ADR-0077 d4, which serves `universe` rows without an approver (the
    operator, 2026-11-14).

- **d14 — The constitution changes in R2 and R3, for admin keys, as ADR-0001 d5's third bounded
  exception; R1 does not change.**
  - **R1 does not change.** R1 is a ceiling on the surface an agent calls (ADR-0001 d1). Admin
    keys already demote and erase under ADR-0010 d6, and no agent key gains anything here.
  - **R2 changes in its order:**
    - an admin edit is served before any identity other than its author has reviewed it;
    - it is reviewed afterwards: by a person other than the admin's own (never `same_user`) in a production scope, and in a
      development scope by any person holding reviewer there, its own person's key marked
      `same_user` (d8). *(Superseded outside the admin's own root 2026-11-18, accepted 2026-11-19 by the operator: see the amendments at the end of this record.)*
  - **R3 changes in its first sentence:** an admin's edit is readable by others before it is
    ratified.
  - **ADR-0045 d21 already relaxed R3** for admin reads of held rows, without recording it in
    ADR-0001. This exception records it.
  - **ADR-0001 d5's "Outside the three exceptions R2 and R3 hold without exception"** names this exception
    among the three, so it holds in `protected` and `frozen` workspaces too, for exactly this.
  - **Conditions.** The exception holds only when all of these are true:
    - the admin key is `used_by: person`;
    - a reason is given;
    - the act is one of d2's, and in a `frozen` workspace not an edit or a retirement (d12);
    - every reader sees the fix's mark (d7), a release rollback's `latest_by` and an erasure's
      record included;
    - the act is queued for confirmation, by a person other than the admin's own (never `same_user`) in a production scope (d8);
    - while unconfirmed, the version is excluded from every cut (ADR-0085 d1).
  - **Numbering.** It is the third exception. ADR-0088, if accepted, would be the fourth.
  - **The admin read is separate from these conditions.** Any admin key may read claim text with
    a reason, on the admin routes alone, never on an agent surface, each read journalled and bound
    to an investigation's reason (d10) (the operator, 2026-11-14: no `used_by: person` requirement).
  - **No other record** widens it.
  - **The text** is edited into ADR-0001 d5, accepted with this record on 2026-11-14.
  - **ADR-0010 d6's sentence** "Edit or create by hand is this path and no other" gains: "except
    an admin's marked fix (ADR-0089)".

- **d15 — Surfaces, the person-side read, doctor lines and metrics.**
  - **Admin HTTP:**
    - `POST /v2/admin/fixes {act, target, reason}`;
    - `GET /v2/admin/fixes?state=unconfirmed`;
    - `POST /v2/admin/investigations {reason}`, which opens d10's session.
    - `GET /v2/admin/investigations?state=open`, the admin key's open investigations with their
      reason, id, read count and age, for ADR-0083 d11's admin To do page.
  - **Person-side read:**
    - `GET /v2/review/fixes` takes a person's reviewer key: the `<user>-review` key the
      Personal To do page reads with (ADR-0083 d8 makes "Admin fixes to confirm" a To do group,
      one page per fix at `/web/todo/fix`), or a tenant reviewer key.
    - It serves the unconfirmed fixes on workspaces where that key holds reviewer. Their count is
      the `fixes` member of ADR-0083 d11's counts read, `GET /v2/todo/counts`.
    - Each entry carries the act, the target version, the replaced version, the reason, the time
      and `loosens_guardrail`. It never names the admin key, principal or user.
    - A fix taken by the reader's own person is listed with confirm and revert in a development
      scope (marked `same_user` when confirmed) and without either in a production scope (d8). *(Superseded outside the admin's own root 2026-11-18, accepted 2026-11-19 by the operator: see the amendments at the end of this record.)*
    - Confirm and revert are `POST /v2/review/fixes/{id}/confirm|revert`.
  - **CLI:**
    - `oml admin fix demote|edit|retire|release-rollback|import-rollback --reason`;
    - `oml admin investigate --reason`;
    - `oml review fixes`.
  - **Doctor:**
    - unconfirmed fixes, with the oldest's age;
    - fixes that loosen a guardrail;
    - fixes on workspaces with no person holding reviewer;
    - fixes visible only to the admin's own principal;
    - reverted fixes.
  - **Metrics:**
    - `oml_admin_fixes_total{act, loosens_guardrail}`;
    - `oml_admin_fixes_unconfirmed`;
    - `oml_admin_fix_oldest_unconfirmed_seconds`;
    - `oml_admin_reads_total`.

- **d16 — Readings the build takes, decided 2026-11-17.** The operator decided c3 and c4 of the
  v0.8.3 plan and accepted c1 and c12 as recommended, before the code.
  - **Demote targets any promoted version (the operator, 2026-11-17: "Any approved version").**
    - An admin may demote the served tip, or an earlier promoted version that a later version
      superseded and that is no longer served.
    - Demoting the served tip serves again the latest earlier version of the claim that is
      promoted and not demoted; with none, the claim is served by no version.
    - Demoting a version that is not served moves no tip. It changes that version's state to
      `demoted`, its history (the `lifecycle.demote` event and the state every read reports), and
      the as-of reads.
    - **As-of reads skip a demoted version (the operator, 2026-11-17).** A demoted version is
      served by no read at any instant, and for the instants it was the tip an as-of read serves
      the latest earlier promoted, non-demoted version instead; the journal still records what
      was actually served. The tip-at probe counts only promoted, non-demoted versions, so
      today's tip is still the greatest of them and the probe's short cut stays sound
      (ADR-0059 d22, which states the probe's cost).
    - A held version cannot be demoted (`admin.fix_target_not_promoted`): the author withdraws
      it (ADR-0083 d13) or a reviewer rejects it. A rejected, withdrawn or erased version cannot
      be demoted either.
    - The cost: an as-of read no longer reports what was served at that instant when a version
      served then was later demoted; the journal still does, and the version keeps its bytes
      (R4). A demotion of a superseded version is an act on history a reader of today's tip never
      sees, and the doctor's fix lines count it like any other fix.
  - **One surface (accepted 2026-11-17, recommended in the plan).** Demote and erase are reached
    only as fix acts through `POST /v2/admin/fixes` (d15), never as routes of their own. The
    designed `POST /v2/memories/{id}/demote` and `/erase` leave `openapi-designed.yaml`. An unmarked,
    unqueued demotion or erasure would be the "unmarked fixes" alternative this record rejects.
  - **Erase takes a claim (the operator, 2026-11-17: "All its versions").** An erase names one
    version and erases every version of its `claim_key` in its workspace, held, rejected,
    withdrawn and demoted ones included, since a supersession's text overlaps its predecessor's.
    The mechanism, while PostgreSQL holds bodies in clear, is ADR-0005's amendment of 2026-11-17.
  - **A fix's events (accepted 2026-11-17).** Each fix journals `admin.fix` and, in the same
    transaction, the lifecycle event it causes: `lifecycle.demote` (naming the version, and the
    version it served again as `restored_artifact_id`, or null), `lifecycle.erase` (naming every
    erased version), or an edit's or retirement's supersession and promotion. `admin.fix` names
    those events' ids, so the PostgreSQL fold and the lake writer read the lifecycle events they
    already read.
  - **Notes of 2026-11-18, the coordinator's readings for the operator's batch (#454, #455).**
    Each is built as stated unless it says otherwise; the operator confirms, widens or reverses it.
    - **A replaced version gets back the end it had before the supersession.** A ratified
      supersession whose successor declares a start ends the version it replaces there
      (ADR-0059 d4). Demoting that successor gives the replaced version, while it is still
      approved, the end it had before the ratification, an open end staying open, whether the
      successor was the tip or a version below it: recall does not hide a restored tip as
      expired, and an as-of read in the demoted version's tenure serves the version below it.
      `lifecycle.demote` names that version and its end (`reopened_artifact_id`,
      `restored_valid_until`), so a rebuild reads them. Both stores compare-and-set the pair the
      demotion read, as they do its restored tip, and refuse a stale one as a moved tip, so the
      signed event never names an end the store did not write (#474).
      Each store keeps that earlier end when a ratification replaces it: PostgreSQL in
      `memories.valid_until_ratified` and `valid_until_before` (migration 0058). A version a
      ratification ended before 0058 keeps the end it has when restored. The law
      `a_demotion_restores_the_end_the_version_had_before_its_supersession` and
      `demoting_a_version_below_the_tip_restores_the_end_its_ratification_gave_the_version_it_replaced`
      hold it over both stores, today and at instants before the demotion.
    - **A retirement and a vocabulary term are not demoted.** A demotion of a retirement version
      or of a `term`, `term_mapping` or `vocab_release` version is refused, naming
      `lifecycle.demote_kind_unsupported`, and journals nothing: returning a retired claim to
      service and taking a term out of a published vocabulary each need a reading of their own.
      The operator may widen this later.
    - **The author's read of a demoted version.** Its author keeps reading the version by id and
      in their own listing, marked `demoted`; no other read serves it (admission.md).
    - **Superseded by O8 below: a claim key whose every approved version is demoted.** The reading is that
      `demoted` does not count as live, as `erased` does not, so the key can be written again.
      It is not built in this change: `memories_claim_version` keeps a demoted version's ordinal,
      so a fresh claim cannot start at version 1, and the usage meter (ADR-0071 d2,
      `oml_chain_live` and `oml_chain_account`, migration 0045) counts a chain with a demoted
      version as live and finds its account by its version 1. Freeing the key needs the
      operator to choose where the fresh claim's ordinals start and how the meter counts it.
    - **O8 — A claim whose approved versions are all demoted is written again, continuing its
      numbers (the operator, 2026-11-19; #474).** `demoted` no longer holds a claim key: a new
      write to a key whose every version is demoted, rejected or erased is admitted, and when a
      demoted version is among them the new version continues the claim's ordinals (v4 after
      v1 to v3, the demoted ones keeping theirs in `memories_claim_version`) rather than starting
      at 1. The usage meter keeps counting the chain as one live memory: its account is still its
      earliest version 1's, it is live again once the new version is held, and the write charges
      no new memory. No migration: both stores' claim-key read stops counting `demoted` as live,
      and the meter's definition (migration 0045) already counts the chain. With it the fix route
      demotes a claim's last approved version too, and d17's `admin.fix_would_lock_claim_key`
      guard is not built. The law
      `a_claim_whose_approved_versions_are_all_demoted_takes_a_new_version_continuing_its_numbers`
      holds it over both stores, the meter included.
    - **Held successors of a demoted tip.** A version held as the successor of a tip that is
      later demoted can never be approved, since its target is no longer the served tip. `oml
      doctor` counts held versions whose target is no longer their claim's promoted tip
      (`memory.held_successor_stranded`), so an admin sees them; the author withdraws them or a
      reviewer rejects them. A held first version is not counted: it replaces nothing and stays
      approvable. The doctor also counts erased versions whose data key is still live
      (`lake.erased_key_live`).

- **d17 — Readings of v0.8.3 bulk 2, recorded 2026-11-18 before the code, for the operator's
  batch (#422, #423, #474).** The operator decided the first three on 2026-11-17; the coordinator
  confirmed the plan's c1, c2, c5-c8 and c10-c12 as recommended; the rest are the build's own,
  each with its rejected alternative.
  - **Investigations close only by an explicit close (the operator, 2026-11-17).** No idle expiry.
    `POST /v2/admin/investigations/{id}/close` is one route beyond d15; only the key that opened
    the investigation closes it, and a close of an investigation already closed answers it as it
    is and journals nothing. `oml doctor` warns `admin.investigation_open_long` for each one open
    for more than seven days. Rejected: an 8-hour idle expiry (plan c9), which the operator
    overrode; closing by any admin key of the tenant, which would let one admin end another's
    stated reason.
  - **How a read names its investigation (c9, confirmed).** The search body's `investigation`
    field and the one-memory read's `?investigation=` query. A read naming none, or one that is
    unknown, closed or another key's, is refused `admin.investigation_required` (403), naming the
    field (`/investigation` or `investigation`), and journals nothing; one code for all four, so
    the refusal says nothing of another key's investigations. Every admin route that serves claim
    text requires it: today the two of ADR-0045 d21. Counts, structure and recall telemetry
    (ADR-0054 d2) serve no claim text and do not. `memory.admin_read` names `investigation_id`,
    and its commit compare-and-sets that the investigation is still open and the key's.
  - **The reason.** 1 to 1,024 bytes after trimming, scanned by the veto scan as a verdict's
    reason is (a finding that would hold a write does not apply to a reason), stored in clear in
    `admin.investigation_opened` and `admin_investigations` (migration 0063).
    `GET /v2/admin/investigations?state=open|closed` lists the key's own, `open` by default.
  - **The console keeps the id in the browser session** (c9's "the web session holds the open
    id"), as the cookie `__Host-oml_investigation`, not in a new session column: an admin page
    that reads claim text without one, or under one closed since, asks for the reason and opens
    one through `POST /web/admin/investigation`, then returns to the page; the admin pages carry
    a **Close investigation** control. Rejected: a column on `web_sessions`, which needs a lead
    route for the console to set it; reading under the key's newest open investigation, which
    would bind one person's browsers to one reason they did not all give.
  - **The fix route is the only surface for demote and erase (c1, confirmed; d16).**
  - **A reserved per-tenant server key authors a revert (c5, confirmed).** Role none,
    `used_by: service`, revoked at issuance, principal `oml:server`, issued in the revert's own
    transaction the first time a tenant needs it. Its restored version's lineage carries the
    original version's lineage and the admin's principal, so R2 excludes both from the
    reverter's verdict.
  - **The held-by-scan code is `admin.fix_held_by_scan` (c6, confirmed).** d4's
    `admin.fix_used_by_scan` was a slip; d4 reads `admin.fix_held_by_scan`.
  - **Any admin edit of an instruction-class claim sets `loosens_guardrail` (c7, confirmed),**
    since whether a rewritten rule binds less is undecidable; so does a demotion or retirement
    that takes a served instruction-class tip out of service.
  - **The scope is judged at confirm or revert time (c8, confirmed):** the target's place then,
    the strictest current reading. Who may confirm or revert is the 2026-11-18 amendment's rule
    below, which this reading defers to (#490): a reviewer key of the admin's own person confirms
    or reverts, marked `same_user`, only a fix whose target lies inside that person's own user
    root and is a member of no release; anywhere else, open shared workspaces included, another
    person does. Until releases exist (ADR-0085, R3) "a member of no release" holds for every
    target; the check is one named function that R3 fills. Rejected: d8's development-scope
    reading, superseded the same day.
  - **CLI and HTTP only (c10, confirmed).** No console Fix form until the Releases view exists
    (ADR-0083 d9).
  - **An erasure is confirmed, never reverted, and has no dispute comment yet (c11, confirmed);**
    the dispute comment is a follow-up issue.
  - **A fix journals `admin.fix` and the lifecycle event it causes (c12, confirmed; d16),**
    `admin.fix` naming that event's id.
  - **Demoting the last approved version of a claim (superseded the next day by d16's O8).** The
    coordinator's reading of 2026-11-18 refused it as `admin.fix_would_lock_claim_key`; the
    operator decided on 2026-11-19 that the key is released and written again continuing its
    numbers (d16, O8), so the guard was never shipped.
  - **The build's readings of #422 (recorded 2026-11-19 with the code, for the operator's batch).**
    Each names its rejected alternative.
    - **An edit sends `body` and an optional `title`; a retirement's body is the reason.** The
      kind, format, guardrail fields and kind fields stay the target's. Rejected: a full
      supersession body, which would let a fix change a claim's kind.
    - **The reserved server key is role `reader` with no grant**, since no key record carries
      "role none"; revoked at issuance, it authenticates nothing either way. Its id is derived
      from the tenant's, its name the derived form `key-<hex>`, which no created name collides
      with. Rejected: a fixed name such as `oml-server`, which a tenant's key may already hold.
    - **A revert of a demotion of a version below the tip is `admin.fix_superseded`:** the claim
      had moved on before the fix, and serving the demoted text again would replace today's tip.
      A person confirms such a fix instead. A revert in a frozen workspace is `workspace.frozen`,
      since it promotes.
    - **Two codes beyond d4's and d8's:** `admin.fix_not_revertible` for a revert of an erasure,
      and `admin.fix_settled` for a confirm or revert of a fix already settled, the store
      compare-and-setting `unconfirmed`. A same-person settlement outside the strict rule's
      allowance answers `review.author_in_lineage` with `details.reason: same_person`, as R2 by
      person does elsewhere.
    - **The person-side read marks each fix with `acts`** (`[confirm, revert]`, `[confirm]` for an
      erasure, `[]` where this key may take neither), rather than leaving it out, so the
      admin's own person sees a fix waits on someone else.
    - **The doctor lines** are `admin.fix_unconfirmed` (with the oldest's age),
      `admin.fix_loosens_guardrail`, `admin.fix_without_person_reviewer`,
      `admin.fix_admin_person_only` (d15's "visible only to the admin's own principal", read as
      no other person holds reviewer there) and `admin.fix_reverted`, from the admin rescan
      report. The two gauges are set by the lead's upload sweep, as the import gauges are.
    - **Not built here:** d13's `universe` mark and d5's release overlays, which wait for
      releases (R3).
  - **A fix whose mark a later fix takes over is settled `superseded` (the build's reading of
    2026-11-19, the second review of #498; built in v0.8.4).** A version carries one fix mark. When a later fix F2
    serves a version an earlier, unconfirmed fix F1 serves (F1 demotes v2 and serves v1; v3 then
    replaces v1 by review; F2 demotes v3 and serves v1 again), F2's mark replaces F1's, and F1
    could no longer be confirmed or reverted. In F2's own commit the store settles F1 as state
    `superseded`, `superseded_by` naming F2, at F2's seq and time, with no settling key
    (`admin_fixes.superseded_by`, migration 0064). F1 leaves `GET /v2/review/fixes` and the
    `admin.fix_unconfirmed` count; a confirm or revert of it answers `admin.fix_settled` with
    `state: superseded`; the admin's `GET /v2/admin/fixes?state=all` lists it. A confirmed or
    reverted F1 keeps its state. The fold derives it from `admin.fix` alone, so a rebuild that
    replays the fix events reproduces it. Rejected: settling F1 as confirmed by F2, which would
    record a confirmation no person made; leaving F1 unconfirmed, which keeps an act no one can
    take on the person's list and in the doctor count.
  - **A demotion's reopened end is compare-and-set (#474).** `MetadataFold::MemoryDemoted`
    carries the reopened version and its earlier end, and both stores refuse a stale pair as a
    moved tip (d16's note of 2026-11-18).

## Consequences

- Demotion is built with this record. `crates/oml-domain/src/model.rs:139` gains `Demoted`.
- ADR-0084 records every version an import wrote, so its rollback can find them.
- ADR-0077's contract change is additive: two members.
- In a tenant whose only person is the admin, fixes in production scopes stay unconfirmed and
  out of every cut until a second person confirms them (d8), and the doctor names them.
- The operator's answers of 2026-11-14, each the recommendation: an admin key that fixes is
  `used_by: person` (d1); a frozen workspace refuses an admin edit or retirement (d12); a `universe`
  fix carries the mark (d13); a fix in a development scope may be confirmed by the admin's own
  person, marked `same_user`, and one in a production scope only by a person other than the admin's own (never `same_user`) (d8, which
  replaced the one-person `self_signed` confirmation the same day); an unconfirmed fix never expires, and stays served and marked until a
  person confirms or reverts it. *(Amended 2026-11-14: the development self-signing exception is retired (ADR-0001 d5); the build never sets `self_signed`, so this text describes nothing that is served.)*

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Erasure only** | It leaves no fast lever between "served" and "shredded". |
| **Fixes held for review** | That is the developer path (ADR-0085 d9, d11), too slow where a fix is needed. |
| **Unmarked fixes** | Readers would take an unreviewed version for a reviewed one. |
| **A ladder move as a fix** | It would let one admin move a workspace's status alone; ADR-0009 d3's two identities stay. |
| **Admin edits that skip the scan** | A fix would be the one write path where a leaked key or an injection reaches readers at once. |
| **A revert authored by the confirming person** | That person would author text another person approved before; R2 would have to be bent twice. |
| **Fixes do not reach release-bound keys** | A production agent on `latest` would keep the bad version until a person cuts again, which defeats the fix. |
| **Admin reads on the agent surfaces** | That puts text into a model's context with no reason asked. |
| **Confirmation by any key** | An agent confirming an admin's fix is the gate called by the thing gated (R1). |

## Amendments of 2026-11-18 (the lifecycle review; the operator's batch, accepted 2026-11-19, #429)

The lifecycle review's model proposed these with the operator's answers D1 to D4 (ADR-0085 and
ADR-0084, amendments of 2026-11-18), and the operator approved the reviewed proposal. Each was
proposed and marked **[operator batch]**, to stand until the operator confirmed or reversed it
with the other decisions of the batch; the operator accepted every one on 2026-11-19 ("Accept
all"). Sentences above that contradict them stay as written,
marked superseded.

- **Confirmation is strict wherever the fixed version is served to production, and for any target
  outside the admin's own root (amends d8, d14, d15; ADR-0001 d5, exception 3) [operator batch].** Only a person other than the admin's own
  (never `same_user`) confirms or reverts a fix whose target:
  - sits in a `protected` or `frozen` workspace;
  - is a member of any release, published or withdrawn, since a pin to a withdrawn release is
    still served (ADR-0085 d6); or
  - lies outside the admin's own person's user root.

  A reviewer key of the admin's own person confirms or reverts, marked `same_user`, only a fix on
  a target inside that person's own root that is a member of no release. The rule covers two
  sets: the places served to production (a `protected` or `frozen` workspace and every release
  member), and any target outside the admin's own root, open shared workspaces included, which are
  not production. Why the second set: under ADR-0085 d6 as first written, a `latest` key on a
  workspace with no release reads the promoted tip, so an open shared workspace with no release
  would serve the admin's own-confirmed edit to every default agent. D2 refuses that read only in a
  `protected` or `frozen` workspace, so in an open shared workspace the confirmation has to be
  another person's whatever the binding. The cost: in a one-person tenant, a fix
  in a shared workspace stays unconfirmed until a second person joins; it is served marked (d7)
  and excluded from every cut meanwhile. **Superseded:** d8's "On a target in a `development`
  workspace or an in-progress subtree, a reviewer key of the admin's own person may confirm or
  revert the fix", outside the admin's own root; the same reading in d14's R2 bullet, in d15's
  "listed with confirm and revert in a development scope" and in ADR-0001 d5's exception 3.

- **A cut refuses a member whose only verdict is `same_user` (amends d8; ADR-0085 d1)
  [operator batch].** A fix confirmed under development rules is marked `same_user`. d1 of
  ADR-0085 excludes only an unconfirmed fix from a cut, so without this the next cut would carry
  the confirmed fix into production with no other person's verdict on it; a publication approves
  the set, never each item. The refusal, `workspace.release_refused` with reason
  `same_user_only`, is ADR-0085's amendment of the same day. With the strict confirmation above, a
  `same_user` confirmation happens only inside a user root, where no cut is made, so the refusal is
  the backstop should any `same_user` verdict reach a subtree a cut covers.

## Operator decision of 2026-11-19: an admin key is linked to its person (proposed for the record; #498 review)

The operator decided on 2026-11-19, after the review of #498, that an admin key that fixes acts for
a known person, so the rule that keeps the admin's own person off the confirmation reads a person
and never an absence of one. It amends d1, d8 and d17 (c5, c8) as follows.

- **An admin fix needs an admin key linked to its person (amends d1).** The admin key must be
  `used_by: person` and must name its person: a bearer admin key linked under ADR-0085 d16 (now
  admitted for admin keys), or a console admin session key, whose person is its session user. An
  admin key that names no person is refused `admin.fix_requires_person` with
  `details.reason: unlinked`; one used by a service keeps `details.reason: not_used_by_person`.
  Nothing is journalled.
- **The settlement rule fails closed (amends d8 as amended 2026-11-18).** Outside the own-root
  allowance a confirm or revert is admitted only when the admin key's person and the settling
  key's person are both known and differ; an unknown person on either side answers
  `review.author_in_lineage` with `details.reason: same_person`. Rejected: reading an unknown
  person as "another person", which let an unlinked admin's own reviewer key confirm its fix.
- **A revert checks R2 by person against the original version's authors (amends d8).** Beside the
  key-and-principal R2 over the restored version's lineage, the reverter's person is compared with
  the person recorded at the write (`author_person`, ADR-0085 d16, #448) of the original version
  and of every version in its lineage the store holds: the same person outside the reverter's own
  root is refused `same_person`; inside it the revert is admitted marked `same_user`. A confirm of
  an edit or retirement applies the same two checks over the served version's lineage, so a person
  never confirms a fix of a claim they wrote outside their own root.
- **The bootstrap links its admin key (amends ADR-0085 d16's "Console only" for the ceremony).**
  `POST /v2/bootstrap/claim` takes an optional `person`; with it the ceremony creates that
  person's user root and journals `key.linked_person` for the first admin key, actor `operator`,
  in the ceremony's chain. `oml bootstrap claim --person` sends it, and the quickstart and the
  stack jobs pass one. Without it the tenant's first admin key is unlinked and takes no fix until
  a console admin links it. The console link stays console-only and still refuses a self-link.
- **A reverted fix's served version reads `reverted` (d17, the build's reading of D6).** The
  version a fix served keeps its mark when the fix is reverted, and reads `approver.act:
  admin_fix`, `confirmed: false`, `reverted: true`; the restored version reads `act: verdict`.
  Rejected: clearing the mark, which would show an edit no one reviewed as `act: verdict`.
- **A non-admin reader never learns which admin fixed a version (d7, the build's reading of A5).**
  On a version written by a fix (an edit or retirement), every non-admin read serves `principal`
  and `author_key_id` as null and `provenance: admin_fix`; an admin read serves them as stored.

## Operator decision of 2026-11-19, second: a link waits on its person, and a fix records its admin's person (#498 second review)

Built in v0.8.4, with the rest of this record's build.

The operator decided on 2026-11-19, after the second review of #498, how a key comes to act for a
person, since one console admin could otherwise link any key to any other person alone. Three
attacks followed: (A) an admin links their bearer admin key to another person, fixes, and confirms
with their own reviewer key as "another person"; (B) after a fair fix the admin relinks the admin
key to another person and confirms; (C) an admin links their own reviewer key to another person and
confirms with it. It amends d1, d8 and the first decision of that day.

- **A link of an admin key or a reviewer key takes effect only once the named person accepts it
  from their own console session (ADR-0085 d16, amended the same day).** Until then the link is
  pending and the key counts as unlinked. An admin key with no link or a pending one is refused a
  fix, `admin.fix_requires_person` with `details.reason: unlinked`, and nothing is journalled. A
  reviewer key whose link is pending acts for no person, so the settlement rule finds its person
  unknown and fails closed: its confirm or revert answers `review.author_in_lineage` with
  `details.reason: same_person`, and the person-side read marks the fix `acts: []` for it. The
  build applies acceptance to every link, a writer key's included (ADR-0085 d16's reading).
- **Each fix records its admin's person at fix time (amends d3 and d8).** `admin_fixes.admin_person`
  and the `admin.fix` payload's `admin_person` hold the person the admin key acted for when it
  fixed, its accepted link's or its console session's. The settlement compares that recorded
  person with the settling key's person as it stands, never the admin key's link as it stands at
  the settlement, so a relink after the fix (B) changes nothing. The doctor's
  `admin.fix_admin_person_only` reads the same recorded person. The admin's own view of a fix
  serves `admin_person`; a person's reviewer view never does (d15).
- **The bootstrap's link is accepted at creation.** The person `oml bootstrap claim --person`
  names runs the ceremony, so its link is theirs and takes effect at once; the quickstart, the PoC
  demo and the CI stack jobs keep working unchanged.
- **What the decision admits, and why it is correct.** Once the named person accepts an admin key's
  link, that key's fixes are theirs, recorded as theirs, and any other person may confirm them,
  the linking admin's own reviewer key included: the person consented to act through that key.
- A fix that a later fix's mark supersedes is d17's reading of the same day.
