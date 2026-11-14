---
status: Stable
verified: 2026-11-06
answers: "how to install and use the Python and TypeScript clients: package names, what the hand-written wrapper does (URL and key, idempotency, retries, pagination, errors), the seven verbs, and an example"
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
| `read` | With `artifact_id`, fetches. With `query`, recalls. With neither, lists the key's own rows. |
| `workspace` | Reads one workspace; omitted, the key's default. |
| `events` | Tails the journal from `after_seq`. |
| `review_next` (`reviewNext`) | Serves one held memory to a reviewer key. |
| `approve`, `reject` | Record a verdict. Pass the `serve_id`, or none for the one item the key has taken. |

`write` takes the same fields as `oml_write`, and `read` the same as `oml_read`. Administration
(bootstrap, workspaces, keys) stays in the `oml` CLI. The SDK adds no second administration surface.

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
- **Pagination.** `read` returns one page with its `next_cursor`. The iterators (`iter_memories`
  and `pages` in Python, `memories` and `pages` in TypeScript) follow `next_cursor` to the end.
  Cursors are positions, not snapshots: a row that changes between pages can be missed or repeated
  ([pagination](../api/http-api.md)).
- **Errors.** Every refusal raises `ApiError` with the envelope of [errors](../api/errors.md):
  `code`, `message`, `rule`, `retryable`, `details`, `request_id` (`requestId` in TypeScript) and
  the HTTP status. Each error family has a subclass, such as `AuthError` or `AccessError`. An
  unknown code lands in its family's class, and an unknown family in `ApiError`.
- **Validation before sending.** Fields outside the chosen mode raise `request.validation`,
  with `details.field` naming the field, before anything reaches the network.

Uploads are not available, because the server serves no upload sessions.
