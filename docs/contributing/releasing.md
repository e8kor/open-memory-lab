<!--
id: operations/releasing
tier: engineering
status: process page
answers: how a release section of the changelog is assembled, how the SDKs are versioned and published to PyPI and npm, what a shared server needs before a release, and how the main branch is protected by a merge queue
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

## Markers in the release notes

A release's notes can carry a line written for a program to read, not a person: an HTML comment,
`<!-- oml-deployer: post-step <name> -->`, which GitHub does not display. It is a marker an installer
may read. One is defined: `admin-rescan-record-held`, present when the scanner's rule set (`RULESET` in
`crates/oml-scan/src/lib.rs`) differs from the previous stable tag's, since versions held under the old rule set
stay their authors' alone until `oml admin rescan --record-held` reads them
([upgrades](../operations/upgrades.md)). The release's publish job runs `scripts/release_markers.py append`
and then `check` on the notes before it creates the release, and `scripts/check_release.py` holds those steps. The version job runs `scripts/release_markers.py list` first, so a scanner source whose `RULESET` the script cannot read stops the release before anything is built; a unit test reads the real source in every pull request too.
When the check fails, it names the line to add to the notes' source, the notes step of `release.yml`.

## Publishing the SDKs

The Python and TypeScript SDKs (`sdks/python`, `sdks/typescript`) carry the server's version, and
the release builds them from the tag. Publishing them to PyPI and npm is an outward act, so it is the
operator's, behind two switches: the repository variable `PUBLISH_SDKS`, which is off until the operator sets
it, and `decided` in `sdks/REGISTRY-NAMES.toml`, which is `true`. Without both, a release builds, smoke-tests, signs and
attaches the packages to the GitHub release and uploads nothing.

**The version bump.** The release pull request that bumps the workspace version bumps these places
in the same change, and `scripts/check_sdk_versions.py` (run by `scripts/tests/` on every pull
request and by the release's `version` job with the tag) fails naming each one that disagrees:

- `Cargo.toml`, `[workspace.package] version`, the one number; `sdks/rust` follows it;
- `sdks/python/pyproject.toml` and the `open-memory-lab-sdk` entry of `sdks/python/uv.lock`;
- `sdks/typescript/package.json`, both version lines of `package-lock.json`, and the `USER_AGENT`
  constant in `sdks/typescript/src/client.ts`, which a unit test already holds to `package.json`.

**What the release does.** `release.yml` job `sdks` runs `scripts/build_sdk_packages.py`, which
builds the wheel, the sdist and the npm tarball, checks their metadata and file lists, and installs
each into a fresh virtual environment or node project to import it. The file names come from the
manifests (`scripts/sdk_names.py`). The three files are summed and signed with the other archives.
When `PUBLISH_SDKS` is `true` (GitHub compares case-insensitively, so `True` and `TRUE` count; any
other value, or no variable, is off), job `publish-sdk-pypi` and job `publish-sdk-npm` each run after the GitHub
release exists. Each stops at its first step unless `sdks/REGISTRY-NAMES.toml` says `decided = true`
with the names of the manifests, checks each file it uploads, by name, against the signed
`SHA256SUMS` (a file `SHA256SUMS` does not list fails), and uploads it. `scripts/check_release.py`
holds those steps, and refuses `continue-on-error`, `|| true` and a removed `environment` on them.
A published version cannot be replaced or reused, so a rerun after a partial publish fails at the
file the registry already holds. The release notes name the files and do not claim a registry
publication.

**The registry names.** The operator chose `open-memory-lab-sdk` for both PyPI and npm on
2026-11-12. The Python import name stays `oml_sdk`. `oml-sdk` could not be used: PyPI refuses a
name whose ultra-normalised form (`.-_` removed, `l` and `i` read as `1`, `o` as `0`) is an existing
project's, and `oml-sdk` becomes `0m1sdk`, the form of the existing `omi-sdk`; npm refuses a name
that differs from an existing one only by `.`, `-` or `_`, and `oml_sdk` exists. A 404 from
`pip index versions` or `npm view` shows that nothing has that exact name, not that a registry will
accept it, and neither registry reserves a name before the first upload.

**The operator's steps, in order.** Steps 1 and 2 are done. The workflow does not publish until the
rest is.

1. **Choose the names** (done: `open-memory-lab-sdk` on both), one for PyPI and one for npm (an npm name may be scoped, `@scope/name`,
   which needs the scope's account or organisation on npm first). Check each: on PyPI, that its
   ultra-normalised form is no existing project's (the simple index at `https://pypi.org/simple/`
   lists them); on npm, that no existing package differs from it only by `.-_`.
2. **Record the choice in one reviewed change** (done): set `[project] name` in `sdks/python/pyproject.toml`
   and `name` in `sdks/typescript/package.json`; change the name in `sdks/python/uv.lock` and
   `package-lock.json` (two entries) to match, and the one `importlib.metadata.version('<name>')`
   in `sdks/python/src/oml_sdk/client.py`; then write both names and `decided = true` in
   `sdks/REGISTRY-NAMES.toml`. The Python import name stays `oml_sdk`. Tests fail while any of
   these disagree.
3. **PyPI: a pending trusted publisher.** Sign in at pypi.org, open Your account, Publishing, and add
   a GitHub pending publisher with these fields: PyPI project name the Python name chosen in step 1;
   owner `e8kor`; repository name `open-memory-lab`; workflow name `release.yml`; environment name
   `pypi`. A pending publisher creates the project at the first upload and stores no token. It does
   not reserve the name before then, and a name another user registers first invalidates it
   ([PyPI: creating a project through OIDC](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/)).
4. **npm: the account and the first publish.** npm configures a trusted publisher in the settings
   of a package that exists, so the first publish needs a token: create a granular access token with
   read and write on packages, that expires in a week or less, for the account that will own the
   package (with the option to bypass two-factor authentication if the account requires it for
   writes, since a workflow cannot type a code), and store it as the repository secret `NPM_TOKEN`
   (Settings, Secrets and variables, Actions, Secrets). The job passes `--provenance`, so the first
   version carries provenance too. After the first publish, open the package's Settings on
   npmjs.com, add a Trusted Publisher of type GitHub Actions with organization or user `e8kor`,
   repository `open-memory-lab`, workflow filename `release.yml` and environment `npm`, then delete
   `NPM_TOKEN` and revoke the token; later releases publish over OIDC without one
   ([npm: trusted publishers](https://docs.npmjs.com/trusted-publishers)). A trusted-publisher
   configuration that is not used within two days expires.
5. **GitHub environments (recommended).** Settings, Environments: create `pypi` and `npm` and
   add yourself as a required reviewer, so each upload waits for a click. A job that names an
   environment that does not exist creates it without a reviewer.
6. **Set the switch.** Settings, Secrets and variables, Actions, Variables: create `PUBLISH_SDKS`
   with the value `true`. The jobs compare it with `==`, which GitHub evaluates without regard to
   case, so `true`, `True` and `TRUE` all turn publishing on. Any other value (`1` and `yes` too)
   leaves both jobs skipped, and so does deleting the variable; delete it to turn publishing off
   again.
7. **Cut a release.** Tag `vX.Y.Z` as usual. Watch jobs `publish-sdk-pypi` and `publish-sdk-npm`;
   approve each if step 5 put a reviewer on it.
8. **After the first publish.** In the next release pull request, add the registry install lines to
   `docs/engineering/sdks.md` and both SDK READMEs, replace the "Neither package is on PyPI or npm"
   sentence of `sdks.md` and the "It is not published there yet" sentence of each README, and check
   the install from a clean environment.

## What a shared server needs before a release that contains X

A server that people other than its operator use, an invitation-only one included, has three things to
do around an upgrade that a server only you use does not. Each is silent or slow when missed. The
release notes name the ones a release carries; this list says what each means and where it is
sourced. The upgrade steps themselves are in [Upgrade OML](../operations/upgrades.md).

| The release contains | Do | If it is missed |
|---|---|---|
| ADR-0076: no release names a server, and `oml sync` asks consent of a server that says it is shared ([ADR-0076](../adr/ADR-0076-releases-ship-no-default-server-and-setup-asks-for-the-server-or-reads-it-from-the-key-or-the-environment.md) d9) | Set `server.shared = true` (`OML_SERVER__SHARED`) before the upgrade, or in the same deploy. Open sign-up implies it; a server only you use leaves it `false`. Afterwards `GET /v2/whoami` answers `shared: true`. | Nothing fails and nothing warns. A server that omits `shared`, or answers `false`, gets no consent prompt from a new CLI, so its users' `oml sync` runs with no disclosure, and `oml doctor`'s `sync_shared_server` line cannot appear. |
| A new scanner rule set (the rule set's name, `oml-scan/<n>.<n>`, moves; the release notes say so, as for `oml-scan/1.2` in #226) | After the upgrade, run `oml admin rescan --record-held` (ADR-0074 d28). | Until it has run, held versions last scanned under the old rule set stay their authors' alone inside in-progress workareas, and `oml doctor` warns `scan_held_unrescanned` and `scan_rescan_due`. Nothing is demoted or deleted, and the findings the old rule set recorded stay on a held version until a reviewer judges it. |
| A migration that rewrites `memories` (migration 0044, #175, fills `approved_at` and the approver's kind on every promoted row from the journal) | Rehearse the upgrade on a restored copy ([Upgrade OML](../operations/upgrades.md#upgrade-in-order), step 3), read the time the migration took, and schedule the outage for it. Check free disk for the rewritten partition. | The server serves nothing until the migration ends: it runs at start, in one transaction, before the server answers. An older server refuses to start once it has run, so the way back is the dump taken before the upgrade. |

The one measurement of the 0044 fill is from the review of #307, on a throwaway PostgreSQL 18 on a
loaded shared host, as the non-superuser owner with row security forced: 0.84 to 1.5 seconds at
10,000 promoted rows and 13.4 to 21.6 seconds at 100,000, and the partition grew from 194 MB to
333 MB at 100,000 rows until vacuum ([#311](https://github.com/e8kor/open-memory-lab/issues/311)).
It is one host and not a benchmark, and its publication in the operations docs is still open on
that issue; a server with more promoted rows than 100,000, or slower disks, should expect more, and
the rehearsal is what measures yours.

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
