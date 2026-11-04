<!--
id: engineering/sdks
tier: engineering
status: design draft for review · decided, not built
answers: which generators make the Python and TypeScript clients from openapi.yaml and at which pinned versions, the package names, what the hand-written wrappers add — the URL-and-key start, one Idempotency-Key per write, cursor iteration, one error type carrying the envelope, the seven verbs — and the rule that regenerates both clients whenever openapi.yaml changes
-->

# Client SDKs

**Status:** design draft for review · decided, not built

[FR-12](../product/roadmap.md) asks for "generated Python and TypeScript clients from
[openapi.yaml](../api/openapi.yaml) with hand-written wrappers for idempotency, uploads and
pagination". This page decides how those clients are made and kept current: the generators and
their pins, the package names, what the hand-written wrapper covers, and the regeneration rule.
FR-12's other half, `POST /v1/memories:batch`, changes the server and is not decided here; the
row stays open until the batch path and the upload wrapper are built. Since 2026-10-31 both
clients are in `sdks/` and their tests run against a real `oml-server-lead`. Neither offers an
upload: the server serves no upload sessions yet, and the raising stub that stood for one left
in `0.2.0` ([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d10).

## Two layers

Each client has two layers, and only one of them is written by hand:

- **The generated layer** is the output of a pinned generator run over `docs/api/openapi.yaml`.
  It is committed and never edited, and it carries every operation and schema openapi.yaml
  declares, which is exactly what the server routes; the designed operations of
  `openapi-designed.yaml` generate nothing
  ([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d9).
- **The wrapper** is a small hand-written module over the generated layer. It covers what a
  generator cannot know: where the URL and key come from, how a write's `Idempotency-Key`
  survives its retries, how a listing's cursor is followed, how a refusal becomes the one error
  type, and which route a verb's fields choose.

The contract stays declared once, in openapi.yaml and [errors.md](../api/errors.md)
([ADR-0018](../adr/ADR-0018-the-documentation-set.md) d5): the wrapper builds every request and
reads every response through the generated types, and the fields its methods take keep the
spec's own spellings, but for `write`'s `text`, `oml_write`'s name for the body a served claim
also answers as `text`.

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
version, `0.2.0` since the v0.4.0 rewrite renamed their verbs
([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d12), independent of the Cargo
workspace's version.

## What the wrapper covers

| Concern | Rule |
|---|---|
| The URL and key | One pair, as the CLI starts from ([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d15): the constructor's `url` and `api_key` (`apiKey` in TypeScript), else `OML_URL` and `OML_API_KEY`, taken from the first of those two sources that sets either value. A source setting one alone is refused with `ConfigError` naming the other, so a key is never sent to a URL from another source, and no source at all is refused the same way; either way nothing is sent and no server is guessed. The URL is the server's, without `/mcp`. The key appears in no `repr`, message or exception the wrapper builds. |
| Idempotency | Every state-creating `POST` the wrapper sends carries one `Idempotency-Key` per call: the caller's, or a fresh UUIDv4. Every retry of that call reuses it, so a retry after a lost response replays the stored response instead of writing twice ([idempotency](../api/http-api.md#idempotency)). A write returns its result with the key it used and whether the answer carried `Idempotency-Replay: true`, so a caller that retries later, past the wrapper's own attempts, can send the same key. |
| Retries | A call is retried after a transport failure and after a refusal whose `retryable` is `true`, waiting for `Retry-After` when the refusal sends one (at most 30 seconds), else 0.2 seconds doubling per attempt; two retries by default, each attempt bounded by a 10-second timeout. A refusal with `retryable: false`, `idempotency.digest_mismatch` among them, is raised at once. |
| Request bodies | The wrapper sends only the fields the caller set: a `write` without `claim_key` sends none, nor a verb or a kind, and the server derives each (ADR-0039 d3, d6). The generated Python models would otherwise send their schema defaults, and a server that does not serve a field refuses it by name ([recall](../api/http-api.md#recall-post-v1memoriesrecall)). |
| Pagination | `read`'s listing as one page with its `next_cursor`, and as an iterator (`pages` and `iter_memories`; `pages` and `memories` in TypeScript) that follows `next_cursor` until it is `null`, sending the same filters on every page. Cursors are positions, not snapshots ([pagination](../api/http-api.md#pagination)): an iteration can miss or repeat a row that changed between pages, and the wrapper does not hide that. |
| Errors | One error type, `ApiError`, for every refusal, carrying the envelope of [errors.md](../api/errors.md) — `code`, `message`, `rule`, `retryable`, `details`, `request_id` (`requestId` in TypeScript) — and the HTTP status ([ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d11). A `write` or `read` whose fields fall outside the mode they choose raises the same `request.validation` the server and `oml_write` or `oml_read` answer, `details.field` naming the field, before anything is sent, its `request_id` `null`. Each family of errors.md has its subclass of `ApiError`, chosen by the code's family prefix, so a code the client has never seen lands in its family's class, and a family it has never seen lands in the base class `ApiError`: never a crash, as the [versioning rule](../api/http-api.md#versioning-and-deprecation) asks of clients. The Python wrapper reads error bodies itself rather than through the generated `ErrorEnvelope`, whose `ErrorCode` enum refuses a code that the spec it was generated from does not list. |
| Uploads | None while the server serves no upload sessions. When it does, the wrapper gains the session flow of the [HTTP API](../api/http-api.md#attachments): create, put each part, complete, with the digest computed client-side and the session restarted on `upload.session_invalid`. |
| Verbs | Seven, those of [ADR-0039](../adr/ADR-0039-the-api-rewritten-for-v0-4-0.md) d10, which replaced `remember`, `learn`, `supersede`, `recall`, `get`, the listings and `verdict`. `write` takes `oml_write`'s fields, `text` sent as the HTTP `body`: without `replaces` one submit, with it one supersede, `retirement` included; it sends no `kind` unless the caller names one, so the server stores `memory` where 0.1.0's `learn` sent `derived_fact`. `read` takes `oml_read`'s: `artifact_id` fetches, `query` recalls, and neither lists, `own` defaulting to true, in `pages` and `iter_memories` (`memories`) too; `include_text` joins recall's fields. `workarea` reads one node, omitted the key's default as `GET /v2/whoami` names it; `events` tails the journal; `review_next` (`reviewNext`), `approve` and `reject` run the review ceremony for a dreamer key. A verdict sends the `serve_id` its `review next` returned, or none for the one item the key has claimed, and never builds one: identifiers are opaque ([conventions](../api/http-api.md#conventions)). Administration — bootstrap, workareas, keys — is the CLI's ([ADR-0015](../adr/ADR-0015-cli-administration.md)); the generated layer carries those operations, and the wrapper adds no second administration surface. |

The family subclasses are named after the family: `AuthError`, `AccessError`, `RequestError`,
`IdempotencyError`, `SubmissionError`, `WorkareaError`, `ReviewError`, `SimulationError`,
`VocabError`, `UploadError`, `LifecycleError`, `LimitError`, `ExportError`, `SettingsError`,
`OAuthError` for `oauth`, `NamingError` for `name` and `ServerError` for `system`, because
Python's builtins already own `NameError` and `SystemError`. Both languages use these names, all under `ApiError`, which is
under `OmlError` beside `ConfigError` and `TransportError` (a transport failure after the last
retry).

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
- **The generator's warnings, recorded.** The Python generator's report is committed as
  `sdks/python/generator-warnings.txt`, and `--check` fails when the report changes, so a new
  gap cannot land unnoticed and a closed one shows as a diff. The report is empty. Its one gap,
  openapi-python-client 0.29.1 naming the inline `status` enum of `Workarea` `WorkareaStatus`,
  the name of the status read's schema, and so dropping `Workarea`, `WorkareaPage` and
  `WorkareaView`, closed when ADR-0039 d9 removed that read and its schema.

## Tests

Both suites run against a real `oml-server-lead` on a throwaway database, never against a
double. Each suite:

1. mints a uniquely named owner/serving role pair and a database the owner owns, boots the
   server on it, migrating under the owner and serving under the serving role
   ([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d2, d4, d8),
   with a keyring in a scratch directory, and reads the enrollment token from its stdout;
   minting takes a superuser, and under a role that cannot mint the server tests skip with
   `did not run: <role> cannot create roles`, or fail under `OML_TEST_REQUIRE_SPLIT=1`, as CI
   sets;
2. claims the bootstrap and issues keys with the `oml` CLI: an agent key (writer, with writer
   and reader grants on two workareas) and a dreamer key on the second workarea only, so the
   review test's queue holds only that test's rows;
3. runs its tests, then stops the server and drops the database and the role pair.

| Test | Proves |
|---|---|
| The agent loop | A `write` is `held`; its author's recall with `include_unreviewed` returns it `held` and a plain recall does not return it; the dreamer key's `review_next` serves it and an `approve` without `serve_id` promotes it; a plain recall then returns it `promoted`. |
| Idempotent replay | A write whose response is lost in transit is retried under the same `Idempotency-Key`, answered with `Idempotency-Replay: true`, and leaves one version; a key reused with a different body is an `IdempotencyError` and is not retried; two calls send two keys. |
| Pagination | Iterating the own-work listing with a page limit of 2 follows `next_cursor` across at least three pages and yields each written row once. |
| Error mapping | A bad key is an `AuthError` (`auth.invalid`), a writer key asking for review is an `AccessError` (`access.verb_ceiling`), and a second claim under a live `claim_key` is a `SubmissionError` (`submission.claim_key_conflict`), each with its `rule`, `message`, `retryable` and `request_id`. A refusal raises `ApiError` carrying the envelope's `code` and `details`. Every family errors.md declares has its own class, read from errors.md itself. |
| The URL and key | `OML_URL` and `OML_API_KEY` supply the pair when neither is passed and the arguments win over them; an environment or arguments setting one alone, and no source at all, refuse before any request; the key is in no `repr`. |
| The verbs | A `write` without `claim_key` sends none; `replaces` supersedes and sends `reason`; a `read` with `artifact_id` and `query`, and any field outside the mode, raise `request.validation` naming it before sending; `read` with neither lists the key's own rows; `approve` without `serve_id` sends none; `workarea` without a name reads the key's default; `events` resumes from `after_seq`; the listing iterators send `own=true` unless the caller passes `own=False`; a `write` with `derives_from` sends no `kind` unless the caller names one; superseding a held version is `submission.tip_conflict`, and once a reviewer promotes it the supersede is held. |

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
