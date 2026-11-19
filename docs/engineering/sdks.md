---
status: Stable
verified: 2026-11-19
answers: "how to install and use the Python and TypeScript clients: package names, what the hand-written wrapper does (URL and key, idempotency, retries, pagination, errors), the seven verbs and withdraw, the skill set, skills and documents written in code, and an example"
---

# Use the client SDKs

Python and TypeScript clients wrap the [HTTP API](../api/http-api.md). Each is generated from
[openapi.yaml](../api/openapi.yaml) and has a small hand-written wrapper. Use them to build
an agent or reviewer crew in code. The generators, pins and regeneration rule are in the
repository's contributor docs.

## Before starting

A server address and an agent key are required (see [Run a server](../start/run-your-own.md),
or ask the server in use ([get an agent key](../guides/get-an-agent-key.md))). Neither package is on PyPI or npm. Both install from a checkout of the repository or from the files
a release attaches, at the server's version: the two SDKs and the server share one version number,
and a test fails if they diverge.

| | Python | TypeScript |
|---|---|---|
| Package | `open-memory-lab-sdk`, imported as `oml_sdk` | `open-memory-lab-sdk`, ES modules only |
| Floor | Python 3.11 | Node 24 |
| Install | `uv add ./sdks/python`, or `pip install` the wheel a release attaches | `cd sdks/typescript && npm ci && npm run build`, then `npm install /path/to/open-memory-lab/sdks/typescript`, or `npm install` the tarball a release attaches |

Each release builds both packages at its own version and attaches them to its GitHub release; the
file names carry the package names above, which are the names chosen for PyPI and npm
(`sdks/REGISTRY-NAMES.toml`). Neither is on a registry yet, so this page carries no registry install
line. Publishing is switched on by the repository variable `PUBLISH_SDKS`; the steps are in
[Releasing](https://github.com/e8kor/open-memory-lab/blob/main/docs/contributing/releasing.md#publishing-the-sdks).

## Write and read

```python
from oml_sdk import OmlClient

with OmlClient() as oml:   # url= and api_key=, else OML_URL and OML_API_KEY
    written = oml.write("The canary stage must drain before the next one starts.")
    print(written.value.state)          # held
    for hit in oml.read(query="canary stage", include_unreviewed=True).hits:
        print(hit.memory.state, hit.memory.text)
```

```ts
import { OmlClient } from "open-memory-lab-sdk";

const oml = new OmlClient();
const written = await oml.write({ text: "The canary stage must drain before the next one starts." });
const { hits } = await oml.read({ query: "canary stage", include_unreviewed: true });
```

The write is held. Other keys read it after a reviewer approves it. To correct a promoted memory,
pass `replaces` with its artifact id. Replacing a held version raises `submission.tip_conflict`.

## The seven verbs

| Verb | What it does |
|---|---|
| `write` | Saves a memory. With `replaces`, supersedes one; with `retirement`, retires it. |
| `read` | With `artifact_id`, fetches. With `query`, recalls. With neither, lists the catalog of what the key reads, or with `own` the key's own rows. |
| `workspace` | Reads one workspace; omitted, the key's default. |
| `events` | Tails the journal from `after_seq`. |
| `review_next` (`reviewNext`) | Serves one held memory to a reviewer key. |
| `approve`, `reject` | Record a verdict. Pass the `serve_id`, or none for the one item the key has taken. |

`write` takes the same fields as `oml_write`, and `read` the same as `oml_read`. Administration
(bootstrap, workspaces, keys) stays in the `oml` CLI. The SDK adds no second administration surface.

## Withdraw, skills and documents

| Helper | Python | TypeScript | What it does |
|---|---|---|---|
| Withdraw | `withdraw(artifact_id, reason=)` | `withdraw(artifact_id, { reason })` | Retracts a held version the key wrote. It is kept as `withdrawn`, leaves every review queue and is never served; no reviewer is involved. Another state is `memory.not_held`, another author's version `access.author_only` ([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md) d13). |
| The skill set, one page | `skills(workspace, include_own_held=, cursor=)` | `skills({ workspace, include_own_held, cursor })` | The skills offered at the workspace, else the key's default: name, description, resource URI and approval, 50 a page, without bodies. A body is `read` with the skill's `artifact_id`. `set_digest` is the same on every page of one set ([ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d11). |
| The skill set, every page | `iter_skills(workspace, include_own_held=)` | `iterSkills({ workspace, include_own_held })` | Follows `next_cursor` until it is null. |
| A skill or a document | `write(text, kind="skill", skill={"name", "description"})`, `write(text, kind="document", document={"title"})` | `write({ text, kind: "skill", skill })`, `write({ text, kind: "document", document })` | Writes a held skill or document; with `replaces`, its next version, which sends `skill` or `document` again. A skill needs a person's approval ([ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d1, d16). `oml_write` refuses both kinds; the SDKs write them over HTTP. |

A recall with `kinds=["document"]` answers each matching document once, by its best passage: the
hit's `passage` names its fragment, lines and headings, `more_passages` counts the other passages
that matched, and `uri` is the passage's MCP resource
([ADR-0081](../adr/ADR-0081-a-document-is-a-reviewed-markdown-or-text-claim-that-recall-searches-by-passage-and-the-mcp-server-serves-as-a-resource.md) d4).
The guides for [skills](../use/skills.md) and [documents](../use/documents.md) cover review and
offering.

```python
with OmlClient() as oml:
    oml.write(
        "Run the tests, tag the commit, then push the tag.",
        kind="skill",
        skill={"name": "release", "description": "Cut a release."},
    )
    for skill in oml.iter_skills():
        print(skill.name, skill.unreviewed, skill.uri)
```

The repository's Rust client, `oml-sdk`, which the web console uses, reads the same skill set
(`Client::skills`) and a recall's document passages (`RecallQuery::kinds`, `RecallHit::passage`).

Helpers for releases and bindings wait for them to be built
([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d12):
no server serves `/v2/workspaces/{workspace}/releases` or `/v2/keys/{key}/binding` yet.

## What the wrapper does

- **URL and key.** One pair, from the constructor or from `OML_URL` and `OML_API_KEY`. A source
  that sets one alone is refused with `ConfigError`, so a key never goes to a URL from another
  source. The key appears in no `repr`, message or exception.
- **Idempotency.** Each write sends one `Idempotency-Key`: the caller's, or a fresh UUID. Retries reuse
  it, so a lost response replays the stored answer instead of writing twice. The result carries
  the key and whether the answer was a replay ([idempotency](../api/http-api.md)).
- **Retries.** Transport failures and refusals marked `retryable: true` are retried twice by
  default. The wrapper waits for `Retry-After` (at most 30 seconds), else 0.2 seconds doubling
  per attempt. Each attempt times out at 10 seconds. A refusal with `retryable: false`, such as
  `idempotency.digest_mismatch`, is raised at once.
- **Only the fields set.** A `write` without `claim_key` sends none, and the server derives one.
- **Pagination.** `read` and `skills` return one page with its `next_cursor`. The iterators
  (`iter_memories`, `pages` and `iter_skills` in Python, `memories`, `pages` and `iterSkills` in
  TypeScript) follow `next_cursor` to the end.
  Cursors are positions, not snapshots: a row that changes between pages can be missed or repeated
  ([pagination](../api/http-api.md)).
- **Errors.** Every refusal raises `ApiError` with the envelope of [errors](../api/errors.md):
  `code`, `message`, `rule`, `retryable`, `details`, `request_id` (`requestId` in TypeScript) and
  the HTTP status. Each error family has a subclass, such as `AuthError` or `AccessError`. An
  unknown code lands in its family's class, and an unknown family in `ApiError`.
- **Validation before sending.** Fields outside the chosen mode raise `request.validation`,
  with `details.field` naming the field, before anything reaches the network.

The wrappers have no upload helper: the upload sessions of `/v2/uploads` are in the generated layer
alone ([HTTP API](../api/http-api.md)).
