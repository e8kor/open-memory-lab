# Open Memory Lab design

<p>
  <img src="assets/oml-wordmark-light.svg#gh-light-mode-only" height="32" alt="Open Memory Lab">
  <img src="assets/oml-wordmark-dark.svg#gh-dark-mode-only" height="32" alt="Open Memory Lab">
</p>

Every Open Memory Lab surface in this repository (the `/web` pages, the docs site and this
repository's README) draws from the files here ([ADR-0069](../docs/adr/ADR-0069-oml-s-design-system-is-the-source-of-truth-for-every-surface.md)).
The full system, with every component, screen and the contrast table, is the
[design system artifact](https://claude.ai/artifact/FNfuqHMqkNAheL2aKkc8es). This page is the
short form.

## What is here

| Path | What | Edit |
|---|---|---|
| `tokens.json` | colour per theme, type, spacing, radius, size, shadow | copy from the artifact, whole |
| `glyphs/*.svg` | the state, marker and actor glyphs, in `currentColor` | copy from the artifact |
| `assets/` | the marks, lockups, README banner, social card; `scripts/brand/gen.py` draws them | copy, never redraw |
| `diagrams/` | the review gate as `var(--token)` SVG | copy from the artifact |
| `fonts/` | Newsreader, Inter, JetBrains Mono (OFL 1.1, licences beside them) | copy |

After a copy, run `python scripts/gen_design_tokens.py`. It writes the web pages'
`tokens.css` and `brand.rs`, the docs site's `docs/stylesheets/oml.css` and its copies of the
marks and fonts, and `assets/review-gate-{dark,light}.svg`. CI runs it with `--check` and runs
`python scripts/check_contrast.py`; either fails the pull request when an output is stale or a
pair drops below its floor. The landing page is not in this repository (ADR-0070 d12, as amended
on 2026-11-14); it takes the design system from here at a tagged revision.

## Voice

Technical, plain, present tense, active voice, neutral. UI copy is impersonal: no first or second
person ("you", "your", "my", "me", "we") in a heading, label, hint, button, notice, refusal, empty
state, caption, chip, header or footer. A sentence names the thing, not the reader: "Waiting for
review · 6", "Empty: every readable workarea", "Held writes by this account's keys". Where an actor
must be named it is "the user" on the personal side and "the admin" on the administration side, or
the person or key as the data shows it (`alice`, `alice-laptop`); never "client", which is an MCP
client or the CLI. The scope words are **Personal** and **Administration**. Neutral tone: no
exclamations, no reassurance or apology, no "please", no rhetorical questions; a refusal states its
code and the fact. Product codes, verbs and states are kept exactly. Sentence-case titles, no
emoji, no exclamation marks. Banned: seamless, powerful, simply, just, magic, revolutionary; "you",
"your", "my" and "me" in UI copy; "client" for a person. "Open Memory Lab" at first mention, "OML"
after.

*Voice changed on 2026-11-13 at the operator's request: the reader was "you" until then.* The
rule is the console's. The documentation and the landing page keep their wording until the
rewrite of [#357](https://github.com/e8kor/open-memory-lab/issues/357) changes it.

## The content rule

A section is a noun heading and its control. An explanation is one muted line, and only when
it changes a decision: held until someone else reviews it, shown once, expires, irreversible.
Longer help sits behind a `?` or a docs link. Buttons are verbs, and the same verb as the CLI,
API and docs: Remember, Recall, Approve, Reject, Supersede, Propose retirement, Commit.

| Wordy | Terse |
|---|---|
| "Here you can manage all of the API keys that belong to your agents. Click the button below to issue a new key." | **Keys** · [Issue key] |
| "A connect code lets you connect claude.ai to one of your agent keys. Please note that the code will only be shown to you one time." | **Connect code** · [New code] · "Shown once · works once · expires 12:10Z" |

Proposals say so: a supersede or a retirement waits for review by someone else and is never
called done before that. Errors name the code and the fact: "`review.author_in_lineage`:
this reviewer key wrote this version."

## States and glyphs

A state is shown as its word, its glyph and its colour together, never colour alone. Amber
means "needs a second look"; red means refusal or failure; grey recedes but stays visible.

| State | Glyph | Means |
|---|---|---|
| `held` | open ring with a gap | written; waiting for an identity that did not write it |
| `promoted` | solid disc | approved by someone other than its author; the tip |
| `superseded` | half disc | on record, no longer the tip |
| `retired` | slashed ring, dashed border | the claim's tip is an approved retirement |
| `rejected` | square, the only square | a reviewer refused it; the text is kept |
| `unreviewed` | held ring with a centre dot | a held memory served inside an in-progress workarea |
| `stale` | triangle | the latest source check found it changed or removed |
| `self-signed` | loop arrow | approved by its own author where policy permits; never a review |

## The mark

Headers, the hero, social cards and the app icon only. Never in a diagram, an empty state, a
chip or a button: those use the gate glyph. Below 24px use `favicon.svg`, never below 16px.
Clear space is a quarter of the mark's width. Never recolour the star, rotate the mark, put it
on a state colour or add a glow.
