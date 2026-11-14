---
status: Beta
verified: 2026-11-14
answers: "how a workarea moves from development to production, where loose rules turn strict, which key binding reads which stage, where an admin fix enters, how held work is shared vertically and promoted work horizontally, and which parts are built today"
---

# Workarea lifecycle

Two views extend the [four architecture views](views.md) from one memory to one workarea's life. They follow one principle: **frictionless in development, controlled toward production.** Development scopes are loose, so a team sees its own work at once. Production scopes are strict, so a different person always decides what agents receive.

Much of this is designed and not built. Each figure draws a built element solid and an unbuilt one dashed, tagged `designed`. The records are accepted on their pull requests and are not on `main` yet: [#375](https://github.com/e8kor/open-memory-lab/pull/375) (ADR-0080 to ADR-0083) and [#377](https://github.com/e8kor/open-memory-lab/pull/377) (ADR-0084 to ADR-0089).

## Lifecycle: development to release

<figure markdown="span">
  [![Lifecycle view: a development workarea in progress, tests and an impact report (designed), review per item, then across the boundary a committed or validated workarea and a release cut by one person and published by another (designed). tip keys read the promoted tip and held work in progress; latest and pinned keys read releases (designed). An admin fix overlays releases at once and is confirmed afterwards (designed).](img/lifecycle-light.svg#only-light)](img/lifecycle.svg)
  [![Lifecycle view: a development workarea in progress, tests and an impact report (designed), review per item, then across the boundary a committed or validated workarea and a release cut by one person and published by another (designed). tip keys read the promoted tip and held work in progress; latest and pinned keys read releases (designed). An admin fix overlays releases at once and is confirmed afterwards (designed).](img/lifecycle-dark.svg#only-dark)](img/lifecycle.svg)
  <figcaption>Built today: development and in-progress workareas, per-item review with own-root approval marked <code>same_user</code>, and the committed and validated rungs, each moved by two keys. Designed: tests and the impact report (ADR-0086, ADR-0087), releases and the <code>latest</code> and pinned bindings (ADR-0085), admin fixes and their confirmation (ADR-0089), and withdrawal by an author (ADR-0083 d13). Every key reads as a <code>tip</code> key today.</figcaption>
</figure>

| Scope | Rule | Built |
|---|---|---|
| A `development` workarea, an in-progress subtree | A team reads its held work, marked unreviewed. In a person's own root, that person's review key approves their agents' work, marked `same_user`; the root is capped at `development` and never becomes a production scope (decided 2026-11-14; today the code still lets it climb to `committed`). An admin fix there is confirmed by the admin's own person, marked `same_user`. | In-progress sharing inside the subtree and own-root approval: yes. Fixes: no (ADR-0089 d8). |
| A shared `committed` or `validated` workarea, a published release, every read of a `latest` or pinned key | Another person approves, publishes and confirms. Nothing unreviewed is served, except an admin fix's marked overlay. | Committed and validated: yes. Releases and bindings: no (ADR-0085). |
| Every scope | An agent never promotes, signs, demotes or deletes. | Yes: no such verb exists on any agent surface ([roles and keys](roles-and-keys.md)). |

**Bindings.** A key held by a person, and any key scoped in a user root, defaults to `tip`; other agent and service keys default to `latest`; an agent key may be issued at `tip` inside an in-progress root or at an ancestor of one (the operator, 2026-11-14, written into ADR-0085 d7). An admin may still ratify a ladder move (ADR-0009 d3, kept).

**What a stage costs.** Under a `latest` binding, an approved memory reaches production agents only with the next release (ADR-0085 d5). Teams that want approvals to flow at once keep their agents on `tip` and give up the staging step.

**Rollback.** A person withdraws a release, and `latest` keys read the previous one on their next request (ADR-0085 d9). `oml workareas revert --to <label>` drafts the inverse change as held versions for review (ADR-0085 d11). When that cannot wait, an admin fixes at once and a person confirms or reverts afterwards (ADR-0089). Nothing is deleted. The release commands are designed; [ADR-0085](https://github.com/e8kor/open-memory-lab/pull/377) spells their noun `oml workarea`.

## Sharing: vertical and horizontal

<figure markdown="span">
  [![Sharing view: payments.dev is an in-progress root with two children. tip keys inside it read held work, marked; tip keys at the ancestor payments read the subtree's held items too (designed). The sibling payments.ops reads nothing of the subtree until a commit or a release. Another branch, risk, reads promoted rows of committed or validated workareas through federation, and a published release of the subtree (designed). A latest key at payments reads the release, never a held item (designed).](img/sharing-light.svg#only-light)](img/sharing.svg)
  [![Sharing view: payments.dev is an in-progress root with two children. tip keys inside it read held work, marked; tip keys at the ancestor payments read the subtree's held items too (designed). The sibling payments.ops reads nothing of the subtree until a commit or a release. Another branch, risk, reads promoted rows of committed or validated workareas through federation, and a published release of the subtree (designed). A latest key at payments reads the release, never a held item (designed).](img/sharing-dark.svg#only-dark)](img/sharing.svg)
  <figcaption>Built today: held work shared inside the in-progress subtree, a sibling that reads nothing of it, and federation over committed and validated workareas. Designed: the ancestor chain as part of the audience (ADR-0080 d16), federation of a published release (ADR-0085 d15), and the <code>latest</code> binding (ADR-0085 d6).</figcaption>
</figure>

- **Vertical.** Held items of every kind move only inside an in-progress subtree and, once ADR-0080 d16 is built, up its ancestor chain to `tip` keys. Instructions are offered down as well: a `tip` key at an in-progress root, inside it or at one of its ancestors is offered the subtree's skills, guardrails and steering below its scope node; on a name clash the nearer item by tree distance wins, an ancestor winning a tie. Never a held retirement, never a version the scan held or has not scanned.
- **Horizontal.** Siblings, other branches, federation and wide sources read promoted rows of committed or validated workareas, or a published release. Held work never crosses.
- **Binding wins.** A key bound to `latest` or a pin reads the release wherever it sits, and never a held item.

The built mechanics are on [workareas](workareas.md#in-progress-workareas) and in the [serving view](views.md#logical-one-agent-key-and-everything-it-is-served).

## Between servers

A release leaves a server as a signed bundle and lands held on the target. The [import and export](../scenarios/import-export.md) page draws it and lists what moves.

## Where each audience meets it

- One person with a root of their own: [individual users](../scenarios/individual.md).
- A team building skills, guardrails and tests toward production: [engineering teams](../scenarios/engineering-teams.md).
- A team preparing documents and vocabulary for production agents: [business-analyst teams](../scenarios/analyst-teams.md).

Who may approve what, by class and scope, and two Euler views of one key's reach are on [producers and reviewers](../concepts/producers-and-reviewers.md).

The figures' sources are `design/diagrams/arch-lifecycle.svg`, `arch-sharing.svg` and `arch-exchange.svg`; `python scripts/gen_design_tokens.py` writes the files this page shows.
