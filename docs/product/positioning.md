# Positioning

**Status:** design draft for review · decided, not built

## Who this is for

Open Memory Lab is for teams that run external AI crews — agents built and managed elsewhere,
on whatever framework the team chose — and that must answer for what those agents learned.
The person this page addresses signs for the crews' output: when an agent acts on a remembered
"fact" and the action is wrong, that person is asked who wrote the fact, who checked it, and
why it was ever served as true. If nobody in your organization is ever asked that question,
you do not need this product; a plain memory store is cheaper and faster.

## The problem

A crew's memory is the one part of its output nobody reviews. Code goes through review; a
deployment goes through a gate; but the conclusions an agent writes down become the input of
every later agent the moment they are written, unreviewed, unattributed beyond a log line, and
removable only by deleting them silently. The store does not distinguish a verified finding
from a hallucination or a planted instruction, and when something must be removed — a leaked
secret, a personal record, a poisoned entry — the removal itself leaves no account.

## What you get

OML is an external memory provider that treats every memory as a claim under test. A new
memory — text, files, images, PDFs — arrives held: scanned, then reviewed by an identity that
did not write it, and only then visible to anyone else. Every write, verdict, approval,
removal and key issuance is a signed entry in a tamper-evident record, and an export produces
the per-memory decision trail on demand. Memory is organized into workareas that mirror your
crews and mature from open development to a frozen, validated state. The
[front page](../index.md) describes each of these in full.

The cost is stated in the same breath: review is a real gate, so a memory is not visible to
others until a reviewer has acted, and your organization must supply that review capacity —
normally a reviewing crew of its own, with human review as the exception path. A team that
wants writes visible instantly is asking for the thing this design exists to refuse.

## Evidence

Nothing here is built, so the honest inventory of evidence is short:

- **About OML itself: none yet.** Every capability on these pages is a design commitment with
  a tracked exit criterion in the [roadmap](roadmap.md), not a property of running software.
  No throughput, latency or recall number exists for OML; each will exist only when the
  roadmap row committed to measuring it produces one.
- **About the mechanism's value: the record it is designed to produce.** A deployment's
  journal and its export are built to produce the evidence an auditor asks for — who wrote
  this, who reviewed it, who approved it, under what policy, and who read it within the
  retention window. That is a claim about our output, never about a regulator's verdict: no
  page of this documentation calls the system compliant, certified or audit-ready.

### The market claim, in its only permitted form

As of the last market check (searches of 2026-09-08, re-read 2026-10-06), **no provider ships
structural, multi-party, evidence-bearing admission control for agent memory** — a gate that
is enforced by the system rather than by convention, that requires a reviewing identity
distinct from the writer, and that leaves a signed decision record. *This claim is to be
re-verified by live search, with a fetch date, before any publication asserts it* (roadmap row
FR-9); until that re-verification, it describes the market of late 2026, not the market of the
day you read this.

The broader claim — that no provider gates memory writes at all — is dead, and it is worth
saying why. The September 2026 market check found that the Mnemosyne memory provider had
shipped a write-approval switch (v3.14.0, July 2026): writes stage to a pending directory, and
a tool replays approved records. That falsified the categorical claim. What it did not close
is the scoped gap: in that mechanism the approver is whoever runs the tool — the writing agent
included — no second identity is required, and no signed decision record is produced. A
staging flag is a weekend of work; the distance between a staging flag and an evidence-bearing
gate is this whole product — and that distance is also what a competitor would have to build,
which is why the claim carries a standing re-verification duty rather than a publication date.

## Deliberate boundaries

What OML does not do, each by decision rather than omission, each with its alternative named:

- **No web UI.** Administration — key issuance, workarea management, the review ceremony,
  audit export — is a command-line tool, and every command has machine-readable output for
  teams that want their own dashboards on top. The [review guide](../review-guide.md)
  examines the choice, and the cost is acknowledged plainly: an operator who expects to click
  will not find anything to click.
- **No question-answering service.** OML returns memories with their provenance; it does not
  synthesize answers from them. Its clients are agents that recall and reason for themselves,
  so a grounded Q&A layer belongs to the crew, not the provider. Teams wanting an
  ask-the-knowledge-base product should put one in front of OML, not expect one inside it.
- **No crew management.** Workareas mirror crews that are managed outside OML — the crew's
  name, charter and external manager are recorded, not operated. OML governs what the crews
  remember, never what they do.

## Where to go next

The [front page](../index.md) carries the full problem-to-evidence walk; the
[review guide](../review-guide.md) lists what a reviewer of this design should decide, in
order; the [roadmap](roadmap.md) is the register of everything undecided, unbuilt or
unmeasured, including the re-verification duty this page's market claim carries.
