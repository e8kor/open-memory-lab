<!-- This repository is a design spec built strict: every change is held against ADR-0018
     (docs/adr/ADR-0018-the-documentation-set.md), and a change that cannot name its decision
     usually needs one. CONTRIBUTING.md explains new ADR vs amendment vs page edit. -->

## What this changes

<!-- One paragraph. -->

## Which ADR this rests on

<!-- The decision record (and d-handle, if specific) that authorizes this change,
     e.g. "ADR-0022 d3". For an amendment or new ADR, say so and link the proposal issue. -->

## Pages that own the mechanics

<!-- Per ADR-0018 d5, each rule is declared on exactly one page. Name the owning page(s) this
     change edits, and confirm every other mention is a link, not a paraphrase. -->

## Honesty check (ADR-0018)

- [ ] Every factual claim added is sourced, or marked unverified.
- [ ] Every unbuilt or unmeasured claim carries its marker ("decided, not built" / "unmeasured — FR-n"); no present-tense capability claims for software that does not exist.
- [ ] No number is stated that has not been measured.
- [ ] New limitations are roadmap rows with exit criteria, not apologies in prose.
- [ ] Vocabulary follows the ADR-0018 d4 table; nothing from the "never" column appears.
- [ ] Tier boundary holds: no engineering vocabulary on README.md, docs/index.md, or docs/product/ pages.
- [ ] Compliance vocabulary appears only in the sanctioned form ("produces the evidence an auditor asks for").
- [ ] `uvx --with mkdocs-material mkdocs build --strict` passes; new pages are in the nav and carry the frontmatter block.
- [ ] If this changes an accepted decision after `spec-v0.1`: the change tags a spec revision and adds a changelog entry.
