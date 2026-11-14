<!--
id: adr/ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards
tier: engineering
status: proposed
answers: which acts an admin key may take on a workarea's content, on which targets, and with which key; why each takes effect at once and what makes it break-glass; how an admin edit is scanned; how a fix reaches keys bound to a release; how a fix that loosens a guardrail is alerted; what readers see; who confirms or reverts a fix, who authors a revert and what it may not revert; what an admin may read and on which surfaces; what happens in a frozen workarea and in universe; exactly which constitutional sentences this changes and which earlier record already changed one; the person-side read, doctor lines and metrics; and the questions left for the operator
-->

# ADR-0089 — An admin fixes a production workarea at once by a marked break-glass act that a person confirms or reverts afterwards

**Status:** proposed · 2026-11-14

**Release:** not scheduled; with ADR-0085, whose releases a fix overlays

**Amends:**
- ADR-0001: d5 gains its third bounded exception (d14), edited into ADR-0001's text in the same
  change and accepted together with this record.
- ADR-0010: d6's admin verbs and its sentence "Edit or create by hand is this path and no other".
- ADR-0026 (d15) and ADR-0045 (d21): admin reads carry a reason.
- ADR-0077:
  - d2: `approver` gains `act` and `confirmed`;
  - d5: the read-path law admits a marked fix.
- ADR-0009 (d5): a frozen workarea refuses an admin edit or retirement.
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
([ADR-0085](ADR-0085-a-workarea-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md)
d9, d11). That path is too slow in two cases:

- the fix needs another person's review (R2), and nobody is there;
- the version is served to `tip` keys, or is in no release, so there is nothing to withdraw.

What exists today:

- Demotion is designed (ADR-0009 d5, ADR-0010 d6) and not built: no transition reaches `Demoted`
  (`crates/oml-domain/src/model.rs:139`).
- An admin key cannot hold dreamer grants (`crates/oml-domain/src/access.rs:131`).
- An admin key already reads every memory of its tenant, held rows included, on two routes, each
  read journalled as `memory.admin_read`
  ([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d21; `crates/oml-app/src/app/pages.rs:1081`).

Decided, not built.

## Decision

- **d1 — A fix is defined by its target, and is taken by an admin key held by a person.**
  - The target is any version, release or import of the admin key's tenant, whatever its
    workarea's status or mode, except as d12 and d13 restrict.
  - The admin key must be `held_by: human` (*operator to confirm*). A service-held admin key is
    refused `admin.fix_requires_person`.
  - No agent key, service key or operator key takes a fix, on any transport.

- **d2 — Six fix acts.** A ladder move is not one of them: it keeps ADR-0009 d3's two
  identities, so no admin moves a workarea alone.

  | Act | Effect |
  |---|---|
  | **demote** | A version stops being served. The claim's tip becomes its previous promoted version if there is one, and the bytes are kept (ADR-0009 d5). |
  | **edit** | A supersession written by the admin key becomes the tip at once, under d4. |
  | **retire** | A retirement by the admin key takes effect at once. |
  | **roll back a release** | Withdraw the release, or re-point a workarea's `latest` to an earlier published release until a person publishes again (ADR-0085 d13). |
  | **roll back an import** | For one `workarea.imported` event ([ADR-0084](ADR-0084-a-workarea-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d11), each promoted version it brought in is demoted, restoring the tip it replaced, and each claim key it created is retired. Its held versions are marked `import_rolled_back`. |
  | **erase** | A crypto-shred, as a legal act ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md) d2). |

- **d3 — Every fix is break-glass.**
  - It takes effect in its own transaction, never waiting for review.
  - It is journalled as `admin.fix {act, targets, reason, loosens_guardrail}`. The reason is
    required, stored in clear and scanned (ADR-0074).
  - In the same transaction it opens a confirmation item for the persons who hold dreamer on the
    target's workarea (d15).

- **d4 — An admin edit's body passes the scan its kind requires.**
  - The body is scanned like any write of its kind (ADR-0074, including d24's instruction kinds).
  - A veto refuses the edit with `submission.scan_veto`.
  - A finding that would hold the write also refuses the edit as a fix
    (`admin.fix_held_by_scan`), because a fix serves at once and a held finding needs a person
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
  retirement, edit or rollback can remove a guardrail, lower its severity, or withdraw a skill. It
  still takes effect at once. It also:
  - sets `loosens_guardrail: true` on the event;
  - increments `oml_admin_fixes_total{act, loosens_guardrail="true"}`;
  - holds a doctor alert open until a person confirms or reverts the fix.

  This amends ADR-0037 d10 and ADR-0080 d3, which made every change to what binds a ratified act.

- **d7 — Readers see the mark.**
  - ADR-0077 d2's `approver` gains `act`: `verdict` for a version promoted by review, `admin_fix`
    for a version served because of a fix (an edit or retirement, or a version a demotion or
    rollback made the tip again).
  - It also gains `confirmed`: `false` until confirmed, `null` when `act` is `verdict`.
  - For a fix, `held_by` is `human` and `same_user` is `false`. No reader learns which admin.
  - A release re-pointed by an admin reads `latest_by: "admin_fix"`.
  - Agents see the mark on every surface, MCP included.

- **d8 — A person of another principal confirms or reverts the fix. A revert is authored by the
  server and may not revert a superseded fix.**
  - **Who.** A person's dreamer key (`held_by: human`) with dreamer on the workarea, whose
    principal differs from the admin key's.
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
  - **A one-person development tenant.** In a tenant in `development` mode with one person
    (ADR-0001 d5's home), that person may confirm their own fix, marked `self_signed`
    (*operator to confirm*).
  - Every row a fix touched stays (R4).

- **d9 — ADR-0077 d5's read-path law gains the fix.** Every served hit must satisfy one of:
  - it is served unreviewed;
  - its approving verdict is by another key and principal than its author's;
  - it carries `approver.act: "admin_fix"`, and an `admin.fix` event names it.

  A hit with `confirmed: true` also has an `admin.fix_confirmed` by a principal other than the
  admin's.

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
    - MCP: `oml_read`, `oml_write`, `oml_workarea`, prompts and resources;
    - `/v1` recall, listing and get;
    - the session-start and guardrail hooks.
  - Why:
    - an agent surface puts text into a model's context, where no reason is asked;
    - recall moves usage telemetry (ADR-0054);
    - separate surfaces keep "the gate is not callable by the thing being gated" true for every
      agent key.

- **d12 — In a frozen workarea an admin demotes and erases, and never edits or retires.**
  - A `validated` workarea admits demotion, erasure and crew metadata (ADR-0009 d5).
  - An admin edit or retirement there is refused `workarea.frozen` (*operator to confirm*).

- **d13 — In `universe` an admin may edit and retire, marked.**
  - A `universe` row served because of a fix carries `approver.act: "admin_fix"` and nothing else.
    This is an exception to ADR-0077 d4, which serves `universe` rows without an approver
    (*operator to confirm*).

- **d14 — The constitution changes in R2 and R3, for admin keys, as ADR-0001 d5's third bounded
  exception; R1 does not change.**
  - **R1 does not change.** R1 is a ceiling on the surface an agent calls (ADR-0001 d1). Admin
    keys already demote and erase under ADR-0010 d6, and no agent key gains anything here.
  - **R2 changes in its order:**
    - an admin edit is served before any identity other than its author has reviewed it;
    - it is reviewed afterwards, by a person of another principal (d8).
  - **R3 changes in its first sentence:** an admin's edit is readable by others before it is
    ratified.
  - **ADR-0045 d21 already relaxed R3** for admin reads of held rows, without recording it in
    ADR-0001. This exception records it.
  - **ADR-0001 d5's "R2 and R3 hold unconditionally in `committed` and `validated` workareas"**
    is overridden for exactly this exception.
  - **Conditions.** The exception holds only when all of these are true:
    - the admin key is `held_by: human`;
    - a reason is given;
    - the act is one of d2's;
    - readers see the mark (d7);
    - the act is queued for a person of another principal (d8);
    - while unconfirmed, the version is excluded from every cut (ADR-0085 d1).
  - **Numbering.** It is the third exception. ADR-0088, if accepted, would be the fourth.
  - **No other record** widens it.
  - **The text** is edited into ADR-0001 d5, marked proposed.
  - **ADR-0010 d6's sentence** "Edit or create by hand is this path and no other" gains: "except
    an admin's marked fix (ADR-0089)".

- **d15 — Surfaces, the person-side read, doctor lines and metrics.**
  - **Admin HTTP:**
    - `POST /v2/admin/fixes {act, target, reason}`;
    - `GET /v2/admin/fixes?state=unconfirmed`;
    - `POST /v2/admin/investigations {reason}`, which opens d10's session.
  - **Person-side read:**
    - `GET /v2/review/fixes?count=true` takes a person's dreamer key: the `<user>-review` key the
      Personal Review page reads with (ADR-0083 d8 places "Fixes to confirm" there), or a tenant
      dreamer key.
    - It serves the unconfirmed fixes on workareas where that key holds dreamer, and their count.
    - Each entry carries the act, the target version, the replaced version, the reason, the time
      and `loosens_guardrail`. It never names the admin key, principal or user.
    - A fix taken by the reader's own principal is listed without confirm or revert.
    - Confirm and revert are `POST /v2/review/fixes/{id}/confirm|revert`.
  - **CLI:**
    - `oml admin fix demote|edit|retire|release-rollback|import-rollback --reason`;
    - `oml admin investigate --reason`;
    - `oml review fixes`.
  - **Doctor:**
    - unconfirmed fixes, with the oldest's age;
    - fixes that loosen a guardrail;
    - fixes on workareas with no person holding dreamer;
    - fixes visible only to the admin's own principal;
    - reverted fixes.
  - **Metrics:**
    - `oml_admin_fixes_total{act, loosens_guardrail}`;
    - `oml_admin_fixes_unconfirmed`;
    - `oml_admin_fix_oldest_unconfirmed_seconds`;
    - `oml_admin_reads_total`.

## Consequences

- Demotion is built with this record. `crates/oml-domain/src/model.rs:139` gains `Demoted`.
- ADR-0084 records every version an import wrote, so its rollback can find them.
- ADR-0077's contract change is additive: two members.
- In a tenant whose only person is the admin, fixes stay unconfirmed outside development mode
  (d8), and the doctor names them.
- Questions for the operator, recommendation first:
  1. Should an admin key that fixes be `held_by: human` (d1)?
  2. Should a frozen workarea refuse an admin edit or retirement (d12)?
  3. Should a `universe` fix carry the mark (d13)?
  4. Should self-confirmation be allowed in a one-person development tenant (d8)?
  5. Should an unconfirmed fix ever expire? The recommendation is no.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Erasure only** | It leaves no fast lever between "served" and "shredded". |
| **Fixes held for review** | That is the developer path (ADR-0085 d9, d11), too slow where a fix is needed. |
| **Unmarked fixes** | Readers would take an unreviewed version for a reviewed one. |
| **A ladder move as a fix** | It would let one admin move a workarea's status alone; ADR-0009 d3's two identities stay. |
| **Admin edits that skip the scan** | A fix would be the one write path where a leaked key or an injection reaches readers at once. |
| **A revert authored by the confirming person** | That person would author text another person approved before; R2 would have to be bent twice. |
| **Fixes do not reach release-bound keys** | A production agent on `latest` would keep the bad version until a person cuts again, which defeats the fix. |
| **Admin reads on the agent surfaces** | That puts text into a model's context with no reason asked. |
| **Confirmation by any key** | An agent confirming an admin's fix is the gate called by the thing gated (R1). |
