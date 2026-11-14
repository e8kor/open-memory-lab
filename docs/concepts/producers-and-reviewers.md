---
status: Beta
verified: 2026-11-14
answers: "which producer and reviewer pairs count, in which scopes, for each class of claim: an agent's write approved by a person, by another agent's key or by the same person; how R2 is checked in code; how each approval is marked; and, as Euler views, which held and promoted items one key sees across nested workareas and how a binding narrows that"
---

# Producers and reviewers

Every write has a producer and needs a reviewer who is not that producer. Three pairs occur: an agent writes and a person approves; an agent writes and another agent's key approves; a person writes and another person approves. Which of them counts depends on the claim's class and on the scope: loose in development, strict toward production ([workarea lifecycle](../architecture/lifecycle.md)).

## How the check works today

- **A verdict needs a `dreamer` key.** Every other role is refused. A key `held_by: service` is refused every review verb, whatever its grants. An admin key never gives a verdict.
- **R2 is checked per key and per principal.** A verdict is refused `review.author_in_lineage` when the reviewer's key id or principal is anywhere in the version's lineage closure, its author included (`crates/oml-domain/src/model.rs`, `check_reviewer`). The recorded policy is one verdict, role `dreamer`, independence axis `principal`, no person required.
- **Two keys of one person with different principals count as two reviewers.** The person is not part of the check, although the records decide that in a production scope no key of the author's person counts, not only no key of its principal ([ADR-0001](../adr/ADR-0001-the-constitution.md) d5 and [ADR-0082](../adr/ADR-0082-a-person-builds-and-edits-a-workareas-skills-guardrails-and-documents-in-the-console-each-edit-a-held-version-under-the-persons-own-keys.md) d7). It decides only the mark: `same_user` is `true` when the reviewing and authoring keys act for the same person, read from the key's user root or its console session.
- **The console counts as a person.** `<user>-review`, created at first sign-in, is a `dreamer` key `held_by: human` with dreamer and reader on the person's root. Console writes and console-issued agent keys each have their key name as principal, so the person's review key approves them, marked `same_user`.
- **No verdict travels over MCP.** An agent with a reviewer key judges over HTTP, `/v2` ([reviewer crews](../api/reviewer-crews.md)). An agent's own key never promotes or signs (R1); a reviewer crew's verdict is a review act by a different key.

## Who may approve what

| Class | Producer | Reviewer | Counts in development and in-progress scopes | Counts in committed, validated and published scopes | Marking | Built |
|---|---|---|---|---|---|---|
| Knowledge: `memory`, `derived_fact` | agent key A | another person's dreamer key, `held_by: human` (the built check: another key and principal) | yes | yes | `approver.held_by: human` | built |
| Knowledge | agent key A of person P | P's review key, inside P's own root | yes | not applicable: the root stays `development` ([ADR-0085](../adr/ADR-0085-a-workarea-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d17) | `same_user: true` | built; today a user root can still climb to `committed`, which the design removes |
| Knowledge | agent key A of person P | a key of P, outside P's root | no (ADR-0082 d7, refused `review.author_in_lineage` with `details.reason: same_person`) | no | none | designed; today it counts when the principals differ |
| Knowledge | agent key A | agent key B: a reviewer crew's dreamer key, `held_by: agent`, over HTTP, neither the author nor a key of the author's person | yes | yes: knowledge may be approved by a crew (ADR-0080 d16) | `approver.held_by: agent` | built |
| Knowledge | person A | person B | yes | yes | `held_by: human` | built |
| Knowledge: `document`, `test` | as `memory` | as `memory` | as `memory` | as `memory` | as `memory` | designed kinds ([ADR-0081](../adr/ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md), [ADR-0086](../adr/ADR-0086-a-workareas-tests-are-reviewed-claims-the-server-runs-under-the-callers-key-and-a-release-is-published-only-through-ordered-gates.md)) |
| Instruction: `steering`, `guardrail` | agent key A or a person | another person (the built check: another key and principal) | yes | yes | `held_by: human` | built |
| Instruction | agent key A of person P | P's review key, inside P's own root | yes | not applicable: the root stays `development` | `same_user: true` | built for guardrail and steering; skill designed |
| Instruction | agent key A | agent key B (`held_by: agent`) | no: `review.person_required` ([ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d16) | no | none | designed; today an agent's verdict counts on a guardrail or steering row |
| Instruction: `skill` | agent key A or a person | a person, as above | as above | as above | as above | designed kind (ADR-0080) |
| Vocabulary: `term`, `term_mapping`, `vocab_release` | a writer key, over HTTP only | as knowledge, an agent's key included | yes | yes | as knowledge | built |
| Any class | any key | the author's key, or any key sharing a principal with the lineage | no | no | none | built: refused `review.author_in_lineage` |
| Any class, in progress | any key | an agent `curator` profile | never approves; rejects, proposes retirements, flags contradictions | not applicable | none | built |

Two more acts follow the same split, both designed: a release is cut in a shared workarea, by one person, and published by another person, since a published release is a production scope; no release is cut in a user root, which stays in development (ADR-0085 d4, d17); an admin fix is confirmed by the admin's own person in a development scope, marked `same_user`, and only by another person in a production scope or on a release's overlay ([ADR-0089](../adr/ADR-0089-an-admin-fixes-a-production-workarea-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md) d8).

## Who sees what where

The matrix says who may approve. The two Euler views below say who sees what. Each workarea is a region nested in its parent; a key's reach is the outlined region over them. A hollow circle is a held item, served marked unreviewed; a solid circle is promoted or published; a dotted, struck circle is held and not served to that key.

<figure markdown="span">
  [![Reach view: key A, an agent key bound to tip and scoped in payments.dev, reaches the committed team root payments. Inside the in-progress subtree it reads held memories and, designed, held skills, guardrails and steering, marked; in the validated sibling payments.ops it reads promoted items only; the person's own root universe.dana lies outside its reach.](../architecture/img/reach-light.svg#only-light)](../architecture/img/reach.svg)
  [![Reach view: key A, an agent key bound to tip and scoped in payments.dev, reaches the committed team root payments. Inside the in-progress subtree it reads held memories and, designed, held skills, guardrails and steering, marked; in the validated sibling payments.ops it reads promoted items only; the person's own root universe.dana lies outside its reach.](../architecture/img/reach-dark.svg#only-dark)](../architecture/img/reach.svg)
  <figcaption>Built: held memories shared inside the in-progress subtree, promoted rows elsewhere in reach, and a user root no other person's key reads. Designed: held skills, guardrails and steering served the same way (ADR-0080 d10, d16), and keys at an ancestor joining the audience (ADR-0080 d16).</figcaption>
</figure>

<figure markdown="span">
  [![Handoff view in two panels. Panel 1: key A writes memory v2, held in payments.dev; key B, bound to tip, reaches the same workarea and reads it there, marked; a key outside A's lineage, a crew or a person, approves it, and two persons publish it in release 1.4.0 (designed). Panel 2: key B bound to latest reaches the release alone and reads memory v2 as published, never the held v3 or a memory promoted after the cut (designed).](../architecture/img/handoff-light.svg#only-light)](../architecture/img/handoff.svg)
  [![Handoff view in two panels. Panel 1: key A writes memory v2, held in payments.dev; key B, bound to tip, reaches the same workarea and reads it there, marked; a key outside A's lineage, a crew or a person, approves it, and two persons publish it in release 1.4.0 (designed). Panel 2: key B bound to latest reaches the release alone and reads memory v2 as published, never the held v3 or a memory promoted after the cut (designed).](../architecture/img/handoff-dark.svg#only-dark)](../architecture/img/handoff.svg)
  <figcaption>Built: panel 1 up to the promotion. Designed: releases and the <code>latest</code> binding (ADR-0085). Binding wins over the in-progress mark: a <code>latest</code> or pinned key never reads a held item, wherever its scope sits (ADR-0085 d6).</figcaption>
</figure>

The scenarios walk these pairs end to end: [individual users](../scenarios/individual.md) (an agent writes, the same person approves in their own root), [engineering teams](../scenarios/engineering-teams.md) (agents write, persons approve instructions, a crew may approve knowledge) and [business-analyst teams](../scenarios/analyst-teams.md) (persons write and approve, staging to production).
