<!--
id: engineering/sdks
tier: engineering
status: design draft for review · decided, not built
answers: which generators make the Python and TypeScript clients from openapi.yaml and at which pinned versions, the package names, what the hand-written wrappers add — the key, one Idempotency-Key per write, cursor iteration, one exception class per error family, the upload stub — and the rule that regenerates both clients whenever openapi.yaml changes
-->

# Client SDKs

**Status:** design draft for review · decided, not built

[FR-12](../product/roadmap.md) asks for "generated Python and TypeScript clients from
[openapi.yaml](../api/openapi.yaml) with hand-written wrappers for idempotency, uploads and
pagination". This page decides how those clients are made and kept current: the generators and
their pins, the package names, what the hand-written wrapper covers, and the regeneration rule.
FR-12's other half, `POST /v1/memories:batch`, changes the server and is not decided here; the
row stays open until the batch path and the upload wrapper are built. Since 2026-10-31 both
clients are in `sdks/` and their tests run against a real `oml-server-lead`; the upload wrapper
is a stub that refuses, because the server serves no upload sessions yet.

## Two layers

Each client has two layers, and only one of them is written by hand:

- **The generated layer** is the output of a pinned generator run over `docs/api/openapi.yaml`.
  It is committed and never edited, and it carries every operation and schema of both rings,
  less the one Python generator gap recorded under [the regeneration rule](#the-regeneration-rule).
- **The wrapper** is a small hand-written module over the generated layer. It covers what a
  generator cannot know: where the key comes from, how a write's `Idempotency-Key` survives its
  retries, how a listing's cursor is followed, and how a refusal becomes a typed exception.

The contract stays declared once, in openapi.yaml and [errors.md](../api/errors.md)
([ADR-0018](../adr/ADR-0018-the-documentation-set.md) d5): the wrapper builds every request and
reads every response through the generated types, and the fields its methods take keep the
spec's own spellings.

## Generators and pins

| | Python | TypeScript |
|---|---|---|
| Generator | `openapi-python-client` **0.29.1** | `openapi-typescript` **7.13.0**, which emits types only |
| Runtime client | the generated `httpx` client | `openapi-fetch` **0.17.0**, typed by the generated `paths` |
| Pinned in | `sdks/python/pyproject.toml`, dependency group `generate`; the generator's whole dependency tree, `ruff` included, locked in `sdks/python/uv.lock` | `sdks/typescript/package.json`, exact versions, with `typescript` **5.9.3**; the tree locked in `sdks/typescript/package-lock.json` |
| Generator settings | `sdks/python/openapi-python-client.yaml` | `--default-non-nullable false`, passed by `scripts/generate_sdks.py`, so a field with a schema default stays optional in a request body |
| Output | `sdks/python/src/oml_sdk/generated/` | `sdks/typescript/src/generated/schema.ts` |
| Runtime dependencies | `httpx`, `attrs` | `openapi-fetch` |
| Tested on | Python 3.11, the floor: the generated models import `typing.Self` | Node 24, which runs the TypeScript sources by type stripping |

Each generator is written in the language it generates for and runs under the package manager
that language's users already have (`uv`, `npm`), so regenerating needs no JVM and no third
runtime. The Python generator's formatting post-hooks run the locked `ruff` with `--isolated`,
so its output does not depend on any ruff configuration found around the output directory.

## Package names

- Python: the distribution `oml-sdk`, imported as `oml_sdk`; the generated layer is
  `oml_sdk.generated`.
- TypeScript: the npm package `oml-sdk`, ES modules only; `npm run build` emits `dist/`.

Both names were unclaimed on PyPI and npm on 2026-10-31. Neither package is released to a
registry: both install from a checkout, as each package's README shows, and the release
pipeline ([CI and toolchain](ci-and-toolchain.md)) ships no SDK artifact. Both carry their own
version, `0.1.0`, independent of the Cargo workspace's version.

## What the wrapper covers

| Concern | Rule |
|---|---|
| The key | An explicit key, else `OML_API_KEY`; the server URL, explicit, else `OML_URL`. With either missing, the client refuses at construction with `ConfigError` and sends nothing: it never guesses a server. The key appears in no `repr`, message or exception the wrapper builds. |
| Idempotency | Every state-creating `POST` the wrapper sends carries one `Idempotency-Key` per call: the caller's, or a fresh UUIDv4. Every retry of that call reuses it, so a retry after a lost response replays the stored response instead of writing twice ([idempotency](../api/http-api.md#idempotency)). A write returns its result with the key it used and whether the answer carried `Idempotency-Replay: true`, so a caller that retries later, past the wrapper's own attempts, can send the same key. |
| Retries | A call is retried after a transport failure and after a refusal whose `retryable` is `true`, waiting for `Retry-After` when the refusal sends one (at most 30 seconds), else 0.2 seconds doubling per attempt; two retries by default, each attempt bounded by a 10-second timeout. A refusal with `retryable: false`, `idempotency.digest_mismatch` among them, is raised at once. |
| Request bodies | The wrapper sends only the fields the caller set. The generated Python models would otherwise send their schema defaults, `expand_terms: none` among them, and a server that does not serve a field refuses it by name ([recall](../api/http-api.md#recall-post-v1memoriesrecall)). |
| Pagination | `GET /v1/memories` as one page with its `next_cursor`, and as an iterator that follows `next_cursor` until it is `null`, sending the same filters on every page. Cursors are positions, not snapshots ([pagination](../api/http-api.md#pagination)): an iteration can miss or repeat a row that changed between pages, and the wrapper does not hide that. |
| Errors | One exception class per family of [errors.md](../api/errors.md), each carrying `code`, `rule`, `message`, `retryable`, `details`, `request_id` (`requestId` in TypeScript) and the HTTP status. The class is chosen by the code's family prefix, so a code the client has never seen lands in its family's class, and a family it has never seen lands in the base class `ApiError`: never a crash, as the [versioning rule](../api/http-api.md#versioning-and-deprecation) asks of clients. The Python wrapper reads error bodies itself rather than through the generated `ErrorEnvelope`, whose `ErrorCode` enum refuses a code that the spec it was generated from does not list. |
| Uploads | A stub that raises `NotBuiltError` before sending anything, because the server serves no upload sessions yet. When it does, the stub becomes the session flow of the [HTTP API](../api/http-api.md#attachments): create, put each part, complete, with the digest computed client-side and the session restarted on `upload.session_invalid`. |
| Verbs | Submit (`remember` and `learn`), recall, get, the listing, supersede (retirement included), and the review ceremony's next and verdict for a dreamer key. A verdict sends the `serve_id` its `review next` returned and never builds one: identifiers are opaque ([conventions](../api/http-api.md#conventions)). Administration — bootstrap, workareas, keys — is the CLI's ([ADR-0015](../adr/ADR-0015-cli-administration.md)); the generated layer carries those operations, and the wrapper adds no second administration surface. |

The exception classes are named after the family: `AuthError`, `AccessError`, `RequestError`,
`IdempotencyError`, `SubmissionError`, `WorkareaError`, `ReviewError`, `SimulationError`,
`VocabError`, `UploadError`, `LifecycleError`, `LimitError`, `ExportError`, `SettingsError`,
`NamingError` for `name` and `ServerError` for `system`, because Python's builtins already own
`NameError` and `SystemError`. Both languages use these names, all under `ApiError`, which is
under `OmlError` beside `ConfigError`, `NotBuiltError` and `TransportError` (a transport failure
after the last retry).

## The regeneration rule

- `python3 scripts/generate_sdks.py` regenerates both generated layers from
  `docs/api/openapi.yaml` in place. With `--check` it regenerates into a temporary directory and
  fails on any difference: a file added, removed or changed.
- **A change to openapi.yaml regenerates both clients in the same change.** CI's
  `sdk-generated` job runs `--check`, so a pull request that changes the spec without
  regenerating fails. A change to a generator pin regenerates the same way.
- A regeneration that breaks a wrapper is fixed in the same change: the TypeScript wrapper is
  type-checked against the regenerated `schema.ts` (`npm run check`), and both wrappers' tests
  run against a real server.
- **The one generator gap, recorded.** openapi-python-client 0.29.1 names the inline `status`
  enum of the `Workarea` schema `WorkareaStatus`, which is already the name of the
  `WorkareaStatus` component schema. It therefore drops `Workarea`, `WorkareaPage` and
  `WorkareaView` from the generated Python models, and five `/v2` workarea operations parse no
  typed response. The TypeScript layer is unaffected, and no wrapper verb reads those schemas.
  The generator's report of the gap is committed as `sdks/python/generator-warnings.txt`, and
  `--check` fails when the report changes, so a new gap cannot land unnoticed and a closed one
  shows as a diff. The gap closes when openapi.yaml declares the status ladder's enum once, as a
  component schema that `Workarea`, `WorkareaView` and `WorkareaStatus` reference: a spec change
  left to the next change to those schemas.

## Tests

Both suites run against a real `oml-server-lead` on a throwaway database, never against a
double. Each suite:

1. creates a database with a unique name, boots the server on it with a keyring in a scratch
   directory, and reads the enrollment token from its stdout;
2. claims the bootstrap and issues keys with the `oml` CLI: an agent key (writer, with writer
   and reader grants on two workareas) and a dreamer key on the second workarea only, so the
   review test's queue holds only that test's rows;
3. runs its tests, then stops the server and drops the database.

| Test | Proves |
|---|---|
| The agent loop | A `remember` is `held`; its author's recall with `include_own_held` returns it `held` and a plain recall does not return it; the dreamer key's `review next` serves it and its `approve` verdict promotes it; a plain recall then returns it `promoted`. |
| Idempotent replay | A write whose response is lost in transit is retried under the same `Idempotency-Key`, answered with `Idempotency-Replay: true`, and leaves one version; a key reused with a different body is an `IdempotencyError` and is not retried; two calls send two keys. |
| Pagination | Iterating the own-work listing with a page limit of 2 follows `next_cursor` across at least three pages and yields each written row once. |
| Error mapping | A bad key is an `AuthError` (`auth.invalid`), a writer key asking for review is an `AccessError` (`access.verb_ceiling`), and a `learn` without lineage is a `SubmissionError` (`submission.lineage_required`), each with its `rule`, `message`, `retryable` and `request_id`. Every family errors.md declares has its own class, read from errors.md itself. |
| The key | `OML_API_KEY` supplies the key when none is passed, a client with no key refuses before any request, and the key is in no `repr`. |
| Uploads | The stub raises `NotBuiltError` and sends no request. |

The harness hands the server and the CLI an environment built from a short allow-list, so a key
in the caller's environment never reaches either, and the tests remove `OML_API_KEY` and
`OML_URL` from their own environment before each test.

CI runs three jobs in `.github/workflows/ci.yml`: `sdk-python` and `sdk-typescript` each build
`oml-server-lead` and `oml` from source as the `e2e` job does and run their suite against them,
and `sdk-generated` runs the regeneration check.

## Rejected alternatives

- **Hand-written clients.** Two hand-kept copies of every request and response shape drift from
  openapi.yaml with nothing to catch the drift: the paraphrase ADR-0018 d5 forbids, in code.
  Generated layers keep the spec the one declaration site, and a stale client becomes a CI
  failure instead of a bug report.
- **openapi-generator.** It runs on the JVM — its README asks for a "Java 11 runtime at a
  minimum" ([README](https://github.com/OpenAPITools/openapi-generator), fetched 2026-10-31) — so
  every machine that regenerates, CI included, would need Java for a Rust project whose clients
  are Python and TypeScript.
- **Generating at install or build time instead of committing the output.** The client a user
  installs would depend on a generator being present and on the version it resolves. Committed
  output shows what a spec change does to each client in the diff of that change, and the check
  is a plain comparison.
- **Generating models only and writing the transport by hand.** The paths, parameters and
  headers, the half that drifts, would stay hand-written.
- **A fresh `Idempotency-Key` per attempt.** It turns a lost response into a second write,
  the duplicate the idempotency record exists to prevent.
- **Retrying every refusal.** A refusal the server marks `retryable: false` answers the same way
  every time; retrying it only delays the caller.
- **Wrapping the administration verbs.** It would make the SDK a second administration surface
  beside the CLI, which ADR-0015 makes the only one.
