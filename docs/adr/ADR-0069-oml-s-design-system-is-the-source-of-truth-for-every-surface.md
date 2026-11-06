<!--
id: adr/ADR-0069-oml-s-design-system-is-the-source-of-truth-for-every-surface
tier: engineering
status: proposed
answers: where OML's colours, type, spacing, glyphs and brand marks are declared; what is generated from them and how drift fails CI; which surfaces must use them; the content rule for UI copy; the brand mark rules; the contrast check; the rejected roads
-->

# ADR-0069 — OML's design system is the source of truth for every surface

**Status:** proposed · 2026-11-06

**Amends:** ADR-0045 (d8: the pages' one hashed stylesheet is generated tokens followed by a hand-written sheet, and its digest is still computed when the crate compiles)

## Context

Open Memory Lab shows itself on four surfaces: the server's `/web` and `/web/admin` pages, the
documentation site, the repository README, and the public landing page. Each carried its own copy
of the brand colours: a literal stylesheet in `page.rs`
([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
d8), Material's `deep purple` palette in `mkdocs.yml`, shields colours in the README, and a hand
kept page on the server. They had already drifted: the web pages drew every state other than
`held` and `promoted` the same grey, and the docs did not use the brand palette at all.

The OML design system ([the artifact](https://claude.ai/artifact/FNfuqHMqkNAheL2aKkc8es)) now
declares the tokens (colour per theme, type scale, spacing, radius, size, shadow), the state
glyphs, the brand marks, the voice and a contrast table computed from the tokens. It changes after
review rounds; a copy kept by hand would drift again. Decided, and built for the web pages, the
docs, the README and a landing page source by the change that carries this record; the landing
page is not deployed by it.

## Decision

- **d1 — `design/tokens.json` is the one declaration of every colour, type, spacing, radius, size
  and shadow value, copied unchanged from the design system.** A value changes in the design
  system first and is copied in whole; no surface writes a literal colour or size the tokens
  declare. The state glyphs are `design/glyphs/*.svg` and the marks `design/assets/*`, copied the
  same way. The cost: a token change is a two-step change (the artifact, then the copy), which is
  what keeps the review in one place.

- **d2 — `scripts/gen_design_tokens.py` writes every derived file, and `--check` fails CI when one
  differs.** It writes the web pages' custom properties and inline glyphs
  (`crates/transports/oml-transport-http/src/web/tokens.css`, `brand.rs`), the docs site's
  `docs/stylesheets/oml.css` with Material's palette mapped onto the tokens, the docs' copy of the
  marks, and the review-gate diagram as two theme-resolved SVGs for the README. Generated files
  carry a header naming the generator and are never edited by hand. The Practices workflow runs
  `--check`, so a hand edit or a token change without regeneration fails the pull request.

- **d3 — The web pages keep one inline stylesheet whose SHA-256 the CSP names, now the generated
  tokens followed by the hand-written `web.css`.** `page.rs` concatenates the two files at compile
  time and computes the digest as before, so `style-src` still names one hash and the pages still
  carry no script, no image source and no font source. Glyphs and the wordmark are inline SVG,
  coloured by the stylesheet, because `default-src 'none'` blocks the design system's data-URI
  masks. The pages name the design system's families first and fall back to the system's fonts,
  since no font is served; that is the cost of keeping `font-src` closed.

- **d4 — Every surface uses the tokens: `/web` and `/web/admin`, the docs site, the README, the
  landing page, and any page the server adds.** Night (`dark`) is the primary theme; paper
  (`light`) follows `prefers-color-scheme`. A state is shown as its word, its glyph and its colour
  together (the design system's `StateChip`), never colour alone.

- **d5 — UI copy is terse.** A section is a noun heading and its control. An explanation is one
  muted line, written only when it changes a decision: held until someone else reviews it, shown
  once, expires, irreversible. Buttons are verbs and use the one verb the CLI, API and docs use.
  Longer help is a `?` disclosure or a docs link.

- **d6 — The mark is copied, never redrawn, and appears only in headers, the hero, social cards
  and the app icon.** Below 24px the favicon is used, never below 16px; clear space is a quarter
  of the mark's width; the star is never recoloured. Diagrams, empty states, chips and buttons
  use the neutral gate glyph instead.

- **d7 — `scripts/check_contrast.py` checks every documented pair from `design/tokens.json` in
  CI.** It is the design system's contrast script: WCAG 2 relative luminance, `-soft` tints
  composited over their ground, 4.5:1 for text and 3:1 for borders, marks and focus. A token
  change that drops a pair below its floor fails the Practices workflow.

## Consequences

- `design/README.md` is the short brand book (voice, content rule, states and glyphs, marks) and
  links the artifact for the full system. `BRAND.md` keeps the brand story and points to it.
- A branch that adds web styles adds them to `web.css` with `var(--…)` values, not to a literal in
  `page.rs`.
- The docs site drops Material's `deep purple` palette and Google Fonts for the generated sheet
  and fonts served from the site.
- The landing page source lives beside the deployment scripts and uses the same tokens; deploying
  it is an operator step outside this record.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A Tailwind dependency** | It adds a Node build to a Rust and Python repository and a class vocabulary nobody else reads. The pages' CSP needs one hashed stylesheet whose content is known at compile time; a utility build would generate it from templates and change its hash with every template edit. |
| **Per-surface styles** | That is what drifted: four copies of the palette, each a little different. The contrast check can only guard values it can read in one place. |
| **An external design tool as the source** | The repository could not check a value it does not hold, and CI could not regenerate from it offline. The artifact is the review surface; the repository holds the copy CI checks. |
