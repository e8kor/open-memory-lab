# Contributing

**Status:** design draft for review · decided, not built

This repository is a design specification under review. No code exists, so contributing today
means reviewing decisions, finding flaws, and improving the pages that carry them. That is not a
lesser form of contribution — a decision challenged now is far cheaper than one discovered wrong
in production. Start at the [review guide](docs/review-guide.md), which lists the decisions in
the order in which an error would be most expensive.

## Where feedback goes

- **GitHub Issues** — for concrete findings: a design flaw, a contradiction between pages, a
  tier-boundary or vocabulary violation, a broken link. Use the issue templates; they ask for
  the affected page and the decision handle, because a finding that names its decision lands on
  the page that owns it.
- **GitHub Discussions** — for open-ended questions, alternatives worth weighing, and anything
  that is not yet a specific finding.
- Security-relevant design flaws are welcome as public issues during the design phase; see
  [SECURITY.md](SECURITY.md) for the boundary where private reporting takes over.

## How decisions change: the ADR lifecycle

Every decision that shapes OML is a numbered record in [docs/adr/](docs/adr/index.md), and each
record numbers its decisions `d1`…`dn` so later records can amend by handle. The lifecycle:

1. **Proposed** — a new record enters as a proposal, in the register's fixed format (Context,
   Decision, Consequences, Rejected alternatives).
2. **Accepted design** — the maintainer accepts it; the record is binding on every page and on
   the eventual build. Nothing in the register is built, and every record's Status line says so.
3. **Amended-by / superseded-by** — an accepted record is never edited into a different
   decision. A later record amends it by naming the d-handle it changes
   (e.g. "amends ADR-0014 d3"), or supersedes it whole; the old record gains a pointer to the
   record that changed it.

### New ADR, amendment, or page edit?

- A **new ADR** is needed when a change introduces a decision no existing record owns — a new
  mechanism, a new invariant, a reversal of a rejected alternative.
- An **amendment** (a new record that names the d-handles it changes) is needed when a change
  alters what an accepted record decided, even slightly: the decision register must stay an
  honest history, so the old decision stays visible under its pointer.
- A **page edit** suffices when the decision is unchanged and only its carrying prose is wrong:
  a broken link, a vocabulary violation, a missing marker, a clearer explanation of the same
  mechanics.

## Who approves

The maintainer is the repository owner, **e8kor** — for now. Every change lands through a pull
request the maintainer reviews; [CODEOWNERS](.github/CODEOWNERS) enforces that, with
`docs/adr/` called out explicitly. This governance is sized for the project's current reality
(one maintainer, a design under review) and widens as contributors arrive: sustained
contributors will be added to CODEOWNERS for the areas they maintain, and the approval rule on
ADRs will grow from one maintainer to the then-current set.

## The review checklist

[ADR-0018](docs/adr/ADR-0018-the-documentation-set.md) is the doc-set constitution; its rules
are the checklist every review holds a change against:

- **Tier boundary (d2).** `README.md`, `docs/index.md` and `docs/product/` carry no table
  names, no D-numbers, no engineering jargon. Everything else is full-precision.
- **Honesty discipline (d3).** Every factual claim sourced or marked unverified; every unbuilt
  or unmeasured claim carries its marker ("decided, not built" / "unmeasured — FR-n"); no
  present-tense capability claims for software that does not exist; limitations are roadmap
  rows with exit criteria, never apologies in prose; compliance vocabulary only in the
  sanctioned form ("produces the evidence an auditor asks for"); the market claim only in its
  scoped, re-verified form.
- **Fixed vocabulary (d4).** One word per concept; a spelling in the "never" column of the
  table — ADR-0018 d4 and the records that amend it
  ([ADR-0025](docs/adr/ADR-0025-the-agent-only-memory-surface.md) d6, d11) — is a defect wherever
  it appears.
- **One declaration site (d5).** Each rule, table or identifier family is declared on exactly
  one page and linked from everywhere else; a paraphrase on a second page is drift.
- **Machine readability (d6).** Pages carry the frontmatter comment block; `docs/llms.txt`
  stays current with new pages.

## Building the docs locally

The site is MkDocs Material, built strict. With [uv](https://docs.astral.sh/uv/) installed:

```sh
uvx --with mkdocs-material mkdocs serve
```

Then open http://127.0.0.1:8000. Before submitting, verify a clean strict build:

```sh
uvx --with mkdocs-material mkdocs build --strict
```

A warning under `--strict` — a broken link, a page missing from the nav — fails the build and
will fail the pull request.

## Spec versioning and tags

The specification carries its own version, currently **0.2.0-design** (matching
`info.version` in [docs/api/openapi.yaml](docs/api/openapi.yaml)). The conventions:

- **`spec-v0.1`** is tagged when implementation starts, freezing the design the build is held
  against.
- A **tag accompanies every ADR-amending change** after that point, so any implementation state
  can name the exact spec revision it implements.
- Every design-changing pull request adds a **changelog entry** describing what changed and
  which decision handles it touched.

## Pull requests

The [pull request template](.github/PULL_REQUEST_TEMPLATE.md) asks three things: which ADR the
change rests on, which pages own the mechanics it touches, and the honesty checklist above. A
pull request that cannot name its ADR is usually a signal that it needs one.
