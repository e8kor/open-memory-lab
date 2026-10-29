<!--
id: adr/ADR-0018-the-documentation-set
tier: engineering
status: accepted design
answers: what the documentation set commits — the two tiers, the honesty discipline, the fixed vocabulary, one declaration site per rule, and the llms.txt and frontmatter contract
-->

# ADR-0018 — The documentation set: tiers, honesty discipline, llms.txt, frontmatter

**Status:** accepted design · 2026-10-29

## Context

OML's documentation set describes a system that is decided
and not built. A doc set for an unbuilt system earns trust only through discipline: a tier
boundary that keeps engineering vocabulary off client pages, a sourcing rule that marks every
unverified claim, and one declaration site per rule so pages cannot drift apart.
This record decides the set's shape, its tiers, its vocabulary, and the honesty rules that
bind every writer, and it is the page a reviewer holds any other page against.

## Decision

- **d1 — MkDocs Material on GitHub Pages.** One site, built from `docs/` and deployed on push
  by the pages workflow. Mermaid fenced blocks for diagrams. Every page is reachable from the
  mkdocs nav; links between pages are relative. Each page opens with one H1 and then a one-line
  **Status:** — for most pages `design draft for review · decided, not built`; ADRs carry
  `accepted design` with a date, a fact about the decision, never about the code.

- **d2 — Two tiers, and the boundary is enforceable.** Client tier: `README.md`, `docs/index.md`
  and `docs/product/` — no table names, no D-numbers, no jargon; `FR-n` handles may appear in
  the [roadmap](../product/roadmap.md) only, because a public commitment needs a public handle.
  Engineering tier: everything else — full precision, identifiers and filenames expected. A
  sentence that leaks engineering vocabulary onto a client page is a defect, not a style choice.

- **d3 — The honesty discipline.** These rules bind every page:
    - Every factual claim is sourced or marked unverified. Limitations are tracked FR rows with
      exit criteria in the [roadmap](../product/roadmap.md) — the honesty ledger — never
      apologies in prose, and a limitation leaves only when its exit criterion is met.
    - Every unbuilt or unmeasured claim carries its marker: "decided, not built" or
      "unmeasured — FR-n". No present-tense capability claims for software that does not exist.
    - No counts of our own output in prose.
    - Compliance vocabulary: the sanctioned form is "produces the evidence an auditor asks
      for" — never "compliant", "audit-ready", "regulator-grade", and no regulator's
      requirement is named without the sourced regulation and article.
    - The market claim appears only in its scoped form — *no provider ships structural,
      multi-party, evidence-bearing admission control* — and only after re-verification by live
      search with a fetch date, before positioning asserts it. The categorical form ("no
      provider gates writes") is never used: it is one competitor write-approval feature away
      from false, and only the scoped form survives re-verification.
    - A product descriptor is a name, not evidence: no descriptor excuses an unmeasured claim.
    - delta-rs capabilities are cited from the delta-rs documentation with a fetch date.
    - The MCP auth RS row takes the fixed form: spec revision X (fetched DATE) defines
      OAuth 2.1; client support as of DATE is [the verified finding]; OML ships bearer; the
      OAuth trigger is [the named condition].

- **d4 — The vocabulary is fixed.** One word per concept on every engineering surface; client
  pages say the same things in plain words. The table binds; a spelling in the "never" column
  is a defect wherever it appears.

    | Term | Means | Never |
    |---|---|---|
    | memory / claim | the stored unit; a memory asserts a claim, and `claim_key` groups the versions of one claim | — |
    | held | written and in the admission pipeline; invisible to every reader except its author and the reviewer it is served to | "pending", "unreviewed" as states |
    | promoted | ratified and visible to readers in scope; enqueued for silver materialization | "approved", "published" |
    | demoted | taken off every read surface by a journalled act; bytes kept | "deleted", "hidden" |
    | erased | crypto-shredded: a journalled erase event plus DEK destruction, scoped by the erasure inventory | "deleted" |
    | workarea | a governed node in a tenant's tree; grants bind to its node id | "workspace", "namespace", "folder" |
    | universe | a tenant's root workarea: permanently committed, refuses direct claim writes | — |
    | grant | a key's access to a workarea node, attenuated by its own deny entries | "permission", "scope" |
    | deny | a subtree mask inside one grant, or a global per-key deny object with deny-wins semantics | — |
    | serve | the server's act of putting one held item, with all its attachments, in front of one reviewer under a bound, expiring `serve_id` | — |
    | verdict | a reviewer's recorded decision on a served item | "review" for the decision itself |
    | ratification | the promotion act that applies the journalled review-policy snapshot and makes a claim visible | "approval" |
    | simulation | a fenced provenance channel whose rows carry indelible provenance and are served only to the audience | "sandbox", "test mode" |
    | taint | the sim-derived mark on any claim whose lineage closure touches a simulation row | — |
    | journal | the signed, hash-chained, append-only event log in Postgres that is the system of record | "ledger", "audit log" |
    | lake | the Delta lakehouse of derived bodies; serves no client read | "warehouse", "the database" |
    | outbox | the durable rows the committing transaction writes for the lake-writer to drain | "queue" |
    | frontier | the last outbox seq the lake has fully applied | — |
    | dead-letter | where an item leaves a queue by a journalled act, never a silent drop: a poison outbox row after its retry budget (lake side), or a held item at review expiry (admission side) | — |
    | evidence export | the signed bundle `oml audit export` produces | "regulator export" |
    | roles | `reader`, `writer`, `dreamer`, `admin`, `operator` — exactly these words | synonyms, extra roles |
    | steering | a claim of kind `steering`: a promoted standing instruction returned ahead of hits, unranked, in every recall of its scope | "pinned", "priority memory" |
    | overlay tip | the audience-scoped served tip a promoted tainted supersession becomes; the default tip never moves | — |
    | fold | the derivation of state from the journal by replaying events in sequence — the metadata tables and the reconciler's expected state are folds | "projection", "materialized view" as the concept's name |

- **d5 — One declaration site per rule or identifier.** Each rule, table, state machine or
  identifier family is declared on exactly one page — the [decision register](index.md)
  records which page owns which decision — and linked from everywhere else. A paraphrase in a second page is the drift
  this rule exists to prevent; the cost is an extra click, paid deliberately.

- **d6 — The set stays machine-readable: llms.txt and frontmatter.** `docs/llms.txt` is the
  curated entry point in the published llms.txt shape — an H1, a blockquote summary, file
  lists — deliberately short and opinionated about reading order. Every page carries an
  HTML-comment frontmatter block (`id`, `tier`, `status`, `answers`), invisible when rendered,
  with `status` matching the **Status:** line. These conventions exist because agents are at
  least half the readership. No enforcement script exists yet; until
  one does, the contract is held by review, and that gap is stated here rather than papered
  over.

## Consequences

- These rules bind every writer of every page; a violation of the tier boundary, the
  vocabulary or a missing marker is a defect to fix, not a style preference. The cost is
  accepted in the same breath as the benefit: pages carrying "decided, not built" and
  "unmeasured" read more hedged than competitor copy, and that hedging is the product's
  credibility, not a weakness to edit out.
- [review-guide.md](../review-guide.md) is the entry point for the drafts-for-review reading
  of the whole set.
- Open rows this record depends on, carried in the [roadmap](../product/roadmap.md): the
  market-claim re-verification with fetch date (an FR row — positioning may not assert the
  scoped claim until it is run), and the MCP OAuth RS row written in d3's fixed form.
- d6's enforcement gap is a known cost: frontmatter and llms.txt contracts held only by review
  will drift. Whether OML grows
  generated-index tooling is genuinely open; this record decides no tooling for v1 and
  does not invent it.
