<!--
id: operations/releasing
tier: engineering
status: process page
answers: how a release section of the changelog is assembled, how the SDKs are versioned and published to PyPI and npm, and how the main branch is protected by a merge queue
-->

# Releasing

**Status:** process page. The release pipeline itself (one tag building the binaries, the
image and the chart) is in [CI and toolchain](ci-and-toolchain.md).

## The changelog

Each change adds `changes/<pr-number-or-branch>.md` with its changelog paragraph (see
`changes/README.md`). CI fails a pull request that touches `crates/` or `docs/adr/` without one;
the `no-changelog` label skips the check.

The release pull request runs

```sh
python scripts/assemble_changelog.py <version>-design
```

which writes the section above the newest one in `docs/changelog.md` and deletes the fragments.
The release pull request then edits the section's lead paragraph by hand (the ADRs it accepts)
and bumps the version files, the SDKs' among them ([below](#publishing-the-sdks)). Because no change edited `docs/changelog.md`, the release pull
request does not conflict on it.

The landing footer and install command name the workspace version, so the same pull request runs
`python site/build.py` after the bump; CI's `site/build.py --check` fails it otherwise.

## Publishing the SDKs

The Python and TypeScript SDKs (`sdks/python`, `sdks/typescript`, both named `oml-sdk`) carry the
server's version, and the release builds them from the tag. Publishing them to PyPI and npm is an
outward act, so it is the operator's, behind one switch: the repository variable `PUBLISH_SDKS`.
Without it, a release builds, smoke-tests, signs and attaches the packages to the GitHub release
and uploads nothing.

**The version bump.** The release pull request that bumps the workspace version bumps these places
in the same change, and `scripts/check_sdk_versions.py` (run by `scripts/tests/` on every pull
request and by the release's `version` job with the tag) fails naming each one that disagrees:

- `Cargo.toml`, `[workspace.package] version`, the one number; `sdks/rust` follows it;
- `sdks/python/pyproject.toml` and the `oml-sdk` entry of `sdks/python/uv.lock`;
- `sdks/typescript/package.json`, both version lines of `package-lock.json`, and the `USER_AGENT`
  constant in `sdks/typescript/src/client.ts`, which a unit test already holds to `package.json`.

**What the release does.** `release.yml` job `sdks` runs `scripts/build_sdk_packages.py`, which
builds the wheel, the sdist and the npm tarball, checks their metadata and file lists, and installs
each into a fresh virtual environment or node project to import it. The three files are summed and
signed with the other archives. When `PUBLISH_SDKS` is `true`, job `publish-sdk-pypi` and job
`publish-sdk-npm` each run after the GitHub release exists, check the downloaded files against the
signed `SHA256SUMS` and upload them. A published version cannot be replaced or reused, so a rerun
after a partial publish fails at the file the registry already holds.

**The operator's steps, in order.** Nothing below has been done; the workflow cannot publish until
all of it is.

1. **PyPI: a pending trusted publisher.** Sign in at pypi.org, open Your account, Publishing,
   and add a GitHub pending publisher with these fields: PyPI project name `oml-sdk`; owner
   `e8kor`; repository name `open-memory-lab`; workflow name `release.yml`; environment name
   `pypi`. A pending publisher creates the project at the first upload and stores no token. It
   does not reserve the name before then, and a name another user registers first invalidates it
   ([PyPI: creating a project through OIDC](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/)).
2. **npm: the account and the first publish.** The package `oml-sdk` is free on npm as of
   2026-11-12. npm configures a trusted publisher in the settings of a package that exists, so
   the first publish needs a token:
   create a granular access token with read and write on packages, that expires in a week or less,
   for the account that will own `oml-sdk` (with the option to bypass two-factor authentication
   if the account requires it for writes, since a workflow cannot type a code), and store it as the repository secret `NPM_TOKEN`
   (Settings, Secrets and variables, Actions, Secrets). The job passes `--provenance`, so the
   first version carries provenance too. After the first publish, open the package's Settings on
   npmjs.com, add a Trusted Publisher of type GitHub Actions with organization or user `e8kor`,
   repository `open-memory-lab`, workflow filename `release.yml` and environment `npm`, then delete
   `NPM_TOKEN` and revoke the token; later releases publish over OIDC without one
   ([npm: trusted publishers](https://docs.npmjs.com/trusted-publishers)). A trusted-publisher
   configuration that is not used within two days expires.
3. **GitHub environments (recommended).** Settings, Environments: create `pypi` and `npm` and
   add yourself as a required reviewer, so each upload waits for a click. A job that names an
   environment that does not exist creates it without a reviewer.
4. **Set the switch.** Settings, Secrets and variables, Actions, Variables: create `PUBLISH_SDKS`
   with the value `true`. Delete it, or set it to anything else, to turn publishing off again.
5. **Cut a release.** Tag `vX.Y.Z` as usual. Watch jobs `publish-sdk-pypi` and `publish-sdk-npm`;
   approve each if step 3 put a reviewer on it.
6. **After the first publish.** In the next release pull request, edit
   `docs/engineering/sdks.md` (the "Neither package is on PyPI or npm" sentence and the install
   table's "today" and "after the first publish" rows) so the published lines are the primary install, and
   check `pip install oml-sdk==X.Y.Z` and `npm install oml-sdk@X.Y.Z` from a clean environment.

## Merge queue

**Status: not enabled.** `main` has no ruleset and no branch protection. GitHub merge queues
exist only in organization-owned repositories, and `e8kor/open-memory-lab` is owned by a
user account: creating a ruleset with a `merge_queue` rule returns
`422 Invalid rule 'merge_queue'` (tried 2026-11-05 with an admin token, nothing created).

To turn it on, the operator transfers the repository to an organization, then:

1. Confirm the workflows listen for `merge_group` (`ci.yml`, `docs-checks.yml` and
   `practices.yml` do), or the queue waits for checks that never start.
2. Create the ruleset (Settings, Rules, Rulesets) or with `gh api -X POST repos/<org>/open-memory-lab/rulesets --input ruleset.json`:

```json
{
  "name": "main merge queue",
  "target": "branch",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["refs/heads/main"], "exclude": [] } },
  "rules": [
    { "type": "merge_queue", "parameters": {
        "merge_method": "MERGE", "grouping_strategy": "ALLGREEN",
        "max_entries_to_build": 5, "max_entries_to_merge": 5,
        "min_entries_to_merge": 1, "min_entries_to_merge_wait_minutes": 5,
        "check_response_timeout_minutes": 60 } },
    { "type": "required_status_checks", "parameters": {
        "strict_required_status_checks_policy": false,
        "required_status_checks": [
          {"context": "fmt, clippy, doc"},
          {"context": "cargo test (Postgres)"},
          {"context": "demo and quickstart (release binaries)"},
          {"context": "docker profile (compose up, quickstart, oml quickstart)"},
          {"context": "release image (linux/amd64)"},
          {"context": "release image (linux/arm64)"},
          {"context": "oml CLI (x86_64-unknown-linux-musl)"},
          {"context": "oml CLI (aarch64-unknown-linux-musl)"},
          {"context": "oml CLI (x86_64-pc-windows-msvc)"},
          {"context": "installer (ubuntu-24.04)"},
          {"context": "installer (ubuntu-24.04, cosign)"},
          {"context": "cargo deny"},
          {"context": "agent plugin agrees with the server"},
          {"context": "recall eval per search pipeline (served)"},
          {"context": "recall eval data parses and its labels resolve"},
          {"context": "kubernetes chart (kind, helm install, quickstart)"},
          {"context": "Python SDK against the release server"},
          {"context": "TypeScript SDK against the release server"},
          {"context": "generated SDK code is current with openapi.yaml"},
          {"context": "mkdocs build --strict"},
          {"context": "benchmark pages match their results"},
          {"context": "contract surfaces agree"},
          {"context": "openapi.yaml validates"},
          {"context": "practice scripts pass their tests"},
          {"context": "migration, port 5432 and LIKE lints"}
        ] } }
  ]
}
```

The merge method is a merge commit, as the repository uses. The two pull-request-only checks
(`ADR and migration numbers are reserved` and `change adds a changelog fragment`) are left
out of the required list: they do not run for a queue entry, and a required check that never
starts holds the queue. Require them in a second ruleset without the queue, or leave them
advisory. `installer (windows-2025)` is left out because the run on main does not report it.
