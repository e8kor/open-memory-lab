# Open Memory Lab

**Status:** design draft for review · decided, not built

**Read the published design at <https://e8kor.github.io/open-memory-lab/>.**

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

This repository contains a design under review, plus a first working slice of it. **The design
is the authority; the code implements its M1 slice** (the signed journal chain, the one-transaction
commit point, submit → review → promote → recall over the standalone profile) and the part of M2
that lets an agent keep its whole memory in OML on one key, through eight of the agent plugin's
twelve tools. You can run it today: see the [quickstart](#quickstart). Every
capability beyond that slice is a design commitment with a tracked exit criterion, not a property
of running software. If you are here to evaluate the design, start at the
[review guide](docs/review-guide.md).

## Quickstart

Docker with the compose plugin, `curl` and `jq` are enough. From a checkout:

```sh
docker compose up -d          # Postgres and oml-server on 127.0.0.1:8080
bash scripts/quickstart.sh    # claim the server, issue keys, check the agent plugin
```

The compose file uses the published image `ghcr.io/e8kor/open-memory-lab`. Use
`docker compose up -d --build` to build it from the checkout instead. The quickstart claims the
server's one-time enrollment token from its log and creates a workarea. It then issues two keys:
one key for your agent, which both stores memories and searches them, and a dreamer key for the
person who reviews. With the agent's key it writes one memory through the plugin's endpoint,
finds it waiting for review in the agent's own list and in the agent's own search, has the
dreamer key approve it, and searches for it again. Then it prints the lines to connect Claude
Code:

```sh
export OML_URL=http://127.0.0.1:8080
export OML_API_KEY=<agent key>
claude plugin marketplace add e8kor/open-memory-lab
claude plugin install oml@open-memory-lab
```

Each key is shown once, so store them when the quickstart prints them. The agent sees its own
writes while they wait, but nothing it writes reaches anyone else until someone approves it
with the dreamer key:

```sh
docker compose exec oml-server oml --server http://127.0.0.1:8080 --key <dreamer key> review next
docker compose exec oml-server oml --server http://127.0.0.1:8080 --key <dreamer key> \
    review verdict --memory <artifact_id> --approve
```

The compose credentials are for a laptop, and the server listens on the loopback interface only.
The plugin and its tools are described in [plugins/oml](plugins/oml/README.md). Releases, their
signatures and SBOMs are on the [releases page](https://github.com/e8kor/open-memory-lab/releases).

## Documentation

The full design lives in [`docs/`](docs/) and is published via GitHub Pages at
<https://e8kor.github.io/open-memory-lab/>.

| Section | What it covers |
| --- | --- |
| [Live Atlas](https://e8kor.github.io/open-memory-lab/atlas/) | See it move: an interactive 3D tour of crews working memory through OML — grant-scoped recall, admission, review, the signed chain — over mocked data |
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
uvx --with mkdocs-material mkdocs serve
```

Then open http://127.0.0.1:8000.

## Reviewing this design

Start at the [review guide](docs/review-guide.md), which lays out the questions a reviewer should
answer and the order in which the design asks them. Disagreements belong at the decision level:
each page links the decision record it rests on.

## Contributing

Review findings and proposals are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for how
feedback and decision changes work, and [SECURITY.md](SECURITY.md) for reporting
security-relevant findings.

## License

The repository and any future code are licensed under [Apache-2.0](LICENSE). The documentation
under [`docs/`](docs/) is additionally licensed under [CC-BY-4.0](LICENSE-docs).

## Running the PoC

The repository now carries a proof-of-concept implementation of the design's M1 slice and of the
part of M2 that makes OML an agent's only memory: the standalone lead server (`oml-server-lead`) and the thin administration CLI (`oml`), composed
per [docs/engineering/workspace-plan.md](docs/engineering/workspace-plan.md).

**Prerequisites**

- Rust (stable, via rustup) — `cargo build --release` builds both binaries.
- A local PostgreSQL with a role that can create databases, e.g.
  `postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev` (or start one with
  `docker compose up -d postgres`). Migrations are embedded and apply on server boot;
  there is no separate migrate step.
- `jq` and `psql` for the demo script.

**Two commands**

```sh
# 1. Build, then boot the lead (migrates on start, mints the keyring, prints the
#    one-time enrollment token on stdout and the signing public key in the boot log):
cargo build --release
OML_POSTGRES__URL=postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev ./target/release/oml-server-lead

# 2. Or let the demo script drive the whole story against a throwaway database —
#    bootstrap, keys, submit, review, the R2 refusal, supersede, audit, shutdown:
bash scripts/poc-demo.sh
```

CLI verbs, exit codes and transcripts are specified in [docs/cli/reference.md](docs/cli/reference.md).

**Honest status.** The PoC is the M1 slice of the
[critical path](docs/product/critical-path.md) and the part of M2 that makes OML an agent's
only memory: one standalone lead, one tenant (bootstrap-claimed), held-by-default admission with
a deterministic marker scanner, the dreamer review ceremony with R2 enforced against the lineage
closure, full-text recall, the signed per-tenant hash chain, and eight of the agent plugin's
twelve tools (`oml_remember`, `oml_learn`, `oml_recall`, `oml_get`, `oml_supersede`,
`oml_my_work`, `oml_workarea_info`, `oml_events_tail`). One key both stores and searches through
the workareas it is granted. With it an agent reads back its own writes while they wait for
review, searches within a budget with standing instructions served first, replaces a memory with
a new version and proposes retiring one; a new version replaces the old one for other readers
only once someone other than its author approves it, and the event feed shows each key only the
events it may see. **Not implemented yet:** the other four plugin tools, attachments and the
object lake, erasure ceremonies, checking the signed chain against a live server (the
command-line verifier reads an export of the whole chain that the server does not serve yet, so
`oml audit verify-chain` stops with an error that says so), follower servers and read scaling,
simulations, embedding-based retrieval, and the operator tier; on keys, adding or revoking a
grant after issuance, lifting a denial, retrying an issuance safely, rotation, and expiry (a key
issued with an expiry is refused); held writes that expire when nobody reviews them; the record
of what each search served; and links between workareas. One working piece differs from the
design: an agent's search of its own waiting writes reads them through a store query restricted
to their author, not through the separate author-only search index the design specifies. The
[critical path](docs/product/critical-path.md) orders what lands next.
