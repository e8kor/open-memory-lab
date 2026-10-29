<!--
id: adr/ADR-NNNN-short-slug
tier: engineering
status: proposed
answers: [one line naming the questions this record answers — fill in when copying this template]
-->

# ADR-NNNN — A title that names the decision, not the topic

**Status:** proposed · YYYY-MM-DD

> **This file is the skeleton for a new decision record — delete this block in the copy.**
> Copy it to `ADR-NNNN-short-slug.md` (next free number; numbers are never reused), replace
> every bracketed placeholder, and register the record at all three registration points: the
> file itself, a row in the [decision register](index.md) table, and a nav entry in
> `mkdocs.yml`. The shape below is fixed by the register's [Format](index.md#format) section
> and by [ADR-0018](ADR-0018-the-documentation-set.md); a record that drops a section is a
> defect. The Status line moves `proposed` → `accepted design` (with the acceptance date — a
> fact about the decision, never about code) and later, if a newer record changes it, gains
> `· amended by ADR-nnnn` or `· superseded by ADR-nnnn`. A record that changes an earlier one
> carries an `**Amends:**` or `**Supersedes:**` line directly under its Status line, naming
> the earlier record and the `dn` handles it touches; delete that line otherwise.

**Amends:** ADR-nnnn (dm) — [only when this record changes an earlier one; otherwise delete this line]

## Context

[Why this decision had to be taken, kept short: the forces, the constraint that makes "decide
nothing" untenable, and what the decision is held against. Cite owning pages by relative link
rather than restating their rules — one declaration site per rule. End with "Decided, not
built." if any sentence could be read as a claim about running software.]

## Decision

- **d1 — [One decision, stated as a commitment, in bold up to the em dash.]** [The reasoning:
  what it buys, and the cost carried openly in the same breath. One decision per handle, so a
  later record can amend by `dn`. Numbers for unmeasured quantities are never stated — write
  "unmeasured, tracked as FR-n" and open the row in the roadmap.]

- **d2 — [The next decision.]** [...]

## Consequences

- [What the decision commits downstream pages and the build to, including every FR/RS row it
  opens in the roadmap — the honesty ledger. Costs are consequences too; state them here, not
  in an apology elsewhere.]

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **[The road not taken]** | [The reason it lost, concrete enough that a reviewer could reopen it on new evidence. Present on every record that weighed alternatives.] |
