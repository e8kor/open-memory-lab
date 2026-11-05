# Change fragments

Each change adds one file here, `changes/<pr-number-or-branch-name>.md`, holding the changelog
paragraph for that change: the ADRs and pages it touched, newest decisions first, in the voice of
`docs/changelog.md`. A release runs `python scripts/assemble_changelog.py <version>`, which moves
the fragments into a new section of `docs/changelog.md` and deletes them, so two changes never
edit the same lines. CI fails a pull request that touches `crates/` or `docs/adr/` without a
fragment; the `no-changelog` label skips the check.
