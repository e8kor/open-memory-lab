<!--
id: index
tier: client
status: design draft for review · decided, not built
answers: what Open Memory Lab is, the problem it solves, what it guarantees in plain words, and where to read next
-->

# Open Memory Lab

Design revision **0.1.0-design** — see the [changelog](changelog.md).
**See it move:** the [Live Atlas](atlas/index.html) is an interactive 3D tour of how crews work
memory through OML — recall, remember, review, supersede, integrate — over mocked data.

**Status:** design draft for review · decided — and since 2026-10-30 the critical path's M1
slice runs as a PoC in this repository (see the README's *Running the PoC*)

Open Memory Lab (OML) is an external memory provider for AI agents and crews, built on one
premise: **what an agent learns is a claim, not a fact.** A memory written by an agent is held
until someone who did not write it has looked at it. Only then does it become visible to anyone
else.

## The problem

Agents and crews accumulate memory as they work — conclusions, summaries, learned facts, files
they produced or were given. Today that memory is written straight into whatever store the crew
uses, and nothing checks it. A wrong conclusion, a hallucinated fact, or a poisoned input becomes
part of the record the moment an agent writes it, and every agent that reads the store afterwards
treats it as true. Nobody reviewed it, nobody can say later who approved it, and when something
has to be removed, removal is usually silent and unverifiable.

## What you get

OML sits between your crews and their memory and makes the write path accountable:

- **Held, then reviewed, then visible.** Every new memory arrives in a held state, is scanned,
  and must be approved by an identity that did not write it before anyone else can read it. The
  reviewer sees everything the claim carries — including every attached file — before their
  verdict counts. Approval changes visibility, not ranking: nothing is quietly reordered, and
  nothing unreviewed is served to others.
- **Files, images and PDFs, not just text.** Attachments travel with the claim, are named by
  their content so they cannot be swapped after the fact, and are stored encrypted. Where a
  file's text can be extracted deterministically, that text is scanned and reviewed too; where it
  cannot, the record says honestly that what was reviewed is the file's origin, fingerprint and
  scan result.
- **Workareas that mirror your crews.** Memory is organized into workareas — one per crew,
  nested under a single root per organization. A workarea matures from open development, through
  a committed phase with full review, to a validated state that is frozen: no new writes, but
  everything in it stays readable and the few changes still allowed — removing a memory, and
  updating the crew's recorded details — each leave a record.
- **An evidence trail an auditor can ask for.** Every write, review verdict, approval, removal
  and key issuance is recorded in a tamper-evident journal. An export produces, per memory, the
  full decision trail: who wrote it, who reviewed it, who approved it, when, and under what
  policy. Reads are logged too, within a retention window your organization sets. The trade is
  stated plainly: this ceremony costs write latency and review effort; what it buys is a record
  you can hand to someone who asks "who approved this and on what basis."
- **No silent deletion.** Removing a memory is itself a recorded act. Content can be made
  permanently unreadable by destroying its encryption key, and the record of *that the removal
  happened* survives. The boundary is stated honestly: destruction takes effect immediately on
  the live system and, for copies held in backups, no later than a named retention-and-rotation
  window; and some traces (such as who-wrote-what metadata) are not destroyed yet — open work,
  tracked.

Agents connect over a standard HTTP plugin interface; humans administer the system entirely from
a command-line tool. There is no web console — that is a deliberate choice, examined in the
[review guide](review-guide.md).

## Honest status

This is a design under review with one proof-of-concept behind it. **The M1 slice is built
(the signed chain, the commit point, submit → review → promote → recall, standalone profile);
everything else described here is not, and nothing has been measured.** Every page in this
documentation is a design commitment with a tracked exit criterion, not a description of
running software; where the PoC diverges from a page, the page wins and the code says so. Where a claim depends on a measurement
(throughput, latency, recall quality), the page says so and points at the roadmap item that will
produce the number. No measurement of OML exists, and no number is quoted anywhere in this
documentation as a property of OML.

## Deployment

OML deploys to Kubernetes from a single Helm chart in two profiles: a production profile with a
managed PostgreSQL and an S3-compatible object store, and a local profile that runs the whole
system on one machine using a lightweight local cluster. Nothing in the design names or depends
on a particular machine. (Design commitment, not yet exercised.)

## Where to go next

- **New to the design?** Start with [Concepts](concepts.md) — the vocabulary, one concept at a
  time, each linked to the page that decides it.
- **Reviewing this design?** Start with the [review guide](review-guide.md) — the decisions that
  matter most, in the order to examine them.
- **Engineering?** Start with the [architecture overview](architecture/overview.md) — system
  context, containers, the module structure (one pure core holds every rule, and everything
  that touches the world sits behind an explicit boundary around it), and the main runtime
  scenarios.
