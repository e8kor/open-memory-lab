# Client SDK generators, regeneration and tests

Contributor reference for the Python and TypeScript clients in `sdks/`. The user guide is [Use the client SDKs](../engineering/sdks.md).

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
  openapi-python-client 0.29.1 naming the inline `status` enum of `Workspace` `WorkspaceStatus`,
  the name of the status read's schema, and so dropping `Workspace`, `WorkspacePage` and
  `WorkspaceView`, closed when removed that read and its schema.

## Tests

Both suites run against a real `oml-server-lead` on a throwaway database, never against a
double. Each suite:

1. mints a uniquely named owner/serving role pair and a database the owner owns, boots the
   server on it, migrating under the owner and serving under the serving role,
   with a keyring in a scratch directory, and reads the enrollment token from its stdout;
   minting takes a superuser, and under a role that cannot mint the server tests skip with
   `did not run: <role> cannot create roles`, or fail under `OML_TEST_REQUIRE_SPLIT=1`, as CI
   sets;
2. claims the bootstrap and issues keys with the `oml` CLI: an agent key (writer, with writer
   and reader grants on two workspaces) and a reviewer key on the second workspace only, so the
   review test's queue holds only that test's rows;
3. runs its tests, then stops the server and drops the database and the role pair.

| Test | Proves |
|---|---|
| The agent loop | A `write` is `held`; its author's recall with `include_unreviewed` returns it `held` and a plain recall does not return it; the reviewer key's `review_next` serves it and an `approve` without `serve_id` promotes it; a plain recall then returns it `promoted`. |
| Idempotent replay | A write whose response is lost in transit is retried under the same `Idempotency-Key`, answered with `Idempotency-Replay: true`, and leaves one version; a key reused with a different body is an `IdempotencyError` and is not retried; two calls send two keys. |
| Pagination | Iterating the own-work listing with a page limit of 2 follows `next_cursor` across at least three pages and yields each written row once. |
| Error mapping | A bad key is an `AuthError` (`auth.invalid`), a writer key asking for review is an `AccessError` (`access.verb_ceiling`), and a second claim under a live `claim_key` is a `SubmissionError` (`submission.claim_key_conflict`), each with its `rule`, `message`, `retryable` and `request_id`. A refusal raises `ApiError` carrying the envelope's `code` and `details`. Every family errors.md declares has its own class, read from errors.md itself. |
| The URL and key | `OML_URL` and `OML_API_KEY` supply the pair when neither is passed and the arguments win over them; an environment or arguments setting one alone, and no source at all, refuse before any request; the key is in no `repr`. |
| The verbs | A `write` without `claim_key` sends none; `replaces` supersedes and sends `reason`; a `read` with `artifact_id` and `query`, and any field outside the mode, raise `request.validation` naming it before sending; `read` with neither lists the key's own rows; `approve` without `serve_id` sends none; `workspace` without a name reads the key's default; `events` resumes from `after_seq`; the listing iterators send `own=true` unless the caller passes `own=False`; a `write` with `derives_from` sends no `kind` unless the caller names one; superseding a held version is `submission.tip_conflict`, and once a reviewer promotes it the supersede is held. |

The harness hands the server and the CLI an environment built from a short allow-list, so a key
in the caller's environment never reaches either, and the tests remove `OML_API_KEY` and
`OML_URL` from their own environment before each test.

CI runs three jobs in `.github/workflows/ci.yml`: `sdk-python` and `sdk-typescript` each build
`oml-server-lead` and `oml` from source as the `e2e` job does and run their suite against them,
and `sdk-generated` runs the regeneration check.

## Rejected alternatives

- **Hand-written clients.** Two hand-kept copies of every request and response shape drift from
  openapi.yaml with nothing to catch the drift: the paraphrase forbids, in code.
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
  beside the CLI, which makes the only one.
