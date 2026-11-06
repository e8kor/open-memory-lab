<!--
id: adr/ADR-0066-open-work-is-tracked-as-github-issues-and-the-roadmap-fr-numbers-and-design-draft-status-lines-are-retired
tier: engineering
status: accepted · 2026-11-06
answers: where open work is tracked; what happens to the roadmap, FR and RS numbers; how pages state what is built; how the documentation is grouped for readers
-->

# ADR-0066 — Open work is tracked as GitHub issues, and the roadmap, FR numbers and design-draft status lines are retired

**Status:** accepted · 2026-11-06

**Amends:** ADR-0018 (d1, d2, d3)

## Context

[ADR-0018](ADR-0018-the-documentation-set.md) d3 made a roadmap page the honesty ledger: every
limitation, open question and unmeasured claim resolved to one `FR-n` or `RS-n` row with an exit
criterion, and every page opened with a status line saying "decided, not built". When the code
was a first slice that was true. At v0.6.0 it is not: 50 of 76 pages still announced a design
draft, 25 roadmap rows described features already built, and a reader could not tell what works.
The roadmap held 121 open rows, none with an owner or a history, and about 330 mentions of them
sat in the decision records.

## Decision

- **d1 — Open work is a GitHub issue.** A limitation, an unmeasured claim, an unverified
  behaviour or an undecided question is an issue labelled `roadmap` and one of `measure`,
  `verify`, `feature` or `decision`. The issue states the problem, the proposed direction and
  the decision records it came from. An issue gives status, owner and history that a table row
  only imitated; the cost is that the ledger now lives outside the repository's files.
- **d2 — FR and RS numbers are retired.** The 121 former rows became 60 issues
  ([the mapping](https://github.com/e8kor/open-memory-lab/blob/main/docs/contributing/fr-to-issues.md)).
  Pages and code cite `#n`. Rows that described shipped behaviour closed without an issue. The
  number registry tracks ADR and migration numbers only.
- **d3 — A page states what the current release does.** The "design draft" and "decided, not
  built" status lines and the per-page release lines are removed. A decision record carries its
  own status (`accepted`, `proposed`, `superseded`, `declined`); the release notes say which
  release introduced a feature. A page names no plan, date or release it does not ship in.
- **d4 — The documentation is grouped by what a reader does.** Get started, Use, Integrate,
  Operate, FAQ and troubleshooting, Reference, Concepts, Release notes and Design records.
  Contributor material lives in `docs/contributing/` and `design/`, outside the published site.
  Design records are grouped in eight categories in the register.
- **d5 — Operator host details stay out of the repository.** Sizing, paths, ports, alert routing,
  backup status and incident notes of a named deployment are the operator's private notes. The
  published operating guide states policy and measured capacity of the software, and
  [ADR-0058](ADR-0058-a-public-server-runs-to-stated-slos-with-an-error-budget.md) is the
  general policy.

## Consequences

- `scripts/oml_numbers.py`, `reserve_number.py`, `check_numbers.py` and the number registry
  handle ADR and migration numbers.
- The roadmap page, the critical-path page and the spec changelog narrative are deleted; the
  release notes are the changelog.
- Records written before this one keep their wording about rows and roadmap; each `FR-n` in them
  became the issue that replaced it.
- ADR-0018 d2 and d3 are amended: `FR-n` handles no longer exist, and the honesty rule that a
  limitation is never apologised for in prose stands, with the issue as its ledger.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Keep `FR-n` as a handle and mirror each row to an issue** | Two numbering schemes and a check that keeps them equal, for no reader. |
| **Keep the roadmap page beside the issues** | It would drift from the issues the day it was written and repeat the status problem. |
| **One issue per row (121)** | Many rows share one exit measurement or one decision; 60 issues keep each deliverable whole. |
| **Rewrite the decision records without their roadmap sentences** | The records are history; the references were mapped, the text stays. |
