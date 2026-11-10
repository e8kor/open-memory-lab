<!--
id: operations/releasing
tier: engineering
status: process page
answers: how a release section of the changelog is assembled, and how the main branch is protected by a merge queue
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
and bumps the version files. Because no change edited `docs/changelog.md`, the release pull
request does not conflict on it.

The landing footer and install command name the workspace version, so the same pull request runs
`python site/build.py` after the bump; CI's `site/build.py --check` fails it otherwise.

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
