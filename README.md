# Open Memory Lab

**Status:** design draft for review · decided, not built

Open Memory Lab (OML) is an external memory provider for AI agents and crews. Agents connect over
HTTP, authenticate with scoped API keys, and write memories — text, files, images, PDFs — into
shared workareas that mirror how their crews are organized. The premise in one line: **every memory
is a claim under test** — held on arrival, scanned, reviewed by an identity that did not write it,
and only then visible to anyone else. Nothing an agent writes becomes shared knowledge on its own
say-so.

That gate is structural, not advisory. The verbs an agent can call cannot approve, delete, or
rewrite history; review is performed by a different identity; erasure is an explicit, recorded act
with a stated boundary rather than a silent delete; and every decision leaves a signed, verifiable
trail that can be exported as evidence. The internals commit to one shape too: a single pure core
holds every one of these rules, and everything that touches the world — storage, transport,
plugins — sits behind an explicit boundary around it
([ADR-0019](docs/adr/ADR-0019-hexagonal-architecture-and-plugins.md)). The cost of this design is equally plain: writes are not
instantly visible, review capacity is a real operational constraint, and the documentation marks
every unmeasured or unbuilt property as exactly that.

## Status

This repository contains a design under review. **No code exists.** Every capability described in
the documentation is a design commitment with a tracked exit criterion, not a property of running
software. If you are here to evaluate the design, start at the
[review guide](docs/review-guide.md).

## Documentation

The full design lives in [`docs/`](docs/) and is published via GitHub Pages.

| Section | What it covers |
| --- | --- |
| [Overview](docs/index.md) | What OML is: the problem, what you get, the evidence it produces |
| [Concepts](docs/concepts.md) | The design's vocabulary: tenants, claims, workareas, roles, admission, simulations, the journal |
| [Review guide](docs/review-guide.md) | What a reviewer of this design decides, in order |
| [Product](docs/product/positioning.md) | Who it is for, and the [roadmap](docs/product/roadmap.md) of open questions with exit criteria |
| [Architecture](docs/architecture/overview.md) | System structure: admission, workareas, roles, storage, retrieval, security |
| [Data](docs/data/postgres-schema.md) | Schemas, lake tables, and the event catalog |
| [API](docs/api/http-api.md) | The HTTP contract, the agent plugin, and the error taxonomy |
| [CLI](docs/cli/reference.md) | Administration and review ceremonies, with session transcripts |
| [Operations](docs/operations/deployment.md) | Deployment profiles and observability |
| [Decisions](docs/adr/index.md) | The architecture decision records |

## Building the site locally

```sh
pip install mkdocs-material
mkdocs serve
```

Then open http://127.0.0.1:8000.

## Reviewing this design

Start at the [review guide](docs/review-guide.md), which lays out the questions a reviewer should
answer and the order in which the design asks them. Disagreements belong at the decision level:
each page links the decision record it rests on.
