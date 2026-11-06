---
status: Stable
verified: 2026-11-06
answers: "how oml sync keeps a workarea in step with a repository's documents and code, reviewed in bulk"
---

# Sync documents to a workarea

`oml sync` keeps a workarea in step with committed markdown. It reads git's object store at one
commit and extracts memories through an adapter. It diffs them against what the workarea holds and
writes the difference. Each memory is anchored to the repository, commit, path, fragment and
fingerprint of the span it came from.

1. **Pick the source and the adapter.** `adr` yields one memory per decision bullet, plus a summary
   and a rejected-alternatives memory. `headings` yields one per heading of level 2 to 4. `lines`
   yields blocks of at most 40 lines.
2. **Create the workarea and a writer key.** Keep using the same sync key. A writer supersedes only
   its own tips.
3. **Preview.** `oml sync docs/adr --workarea design --adapter adr --dry-run` lists each item as
   `new`, `changed`, `removed`, `moved` or `unchanged` and writes nothing.
4. **Run.** Drop `--dry-run`. A run writes up to 200 items (`--max-writes`), so repeat for larger
   sources. The public server is refused unless you pass `--allow-public-server`, because its
   operators read every memory and an anchor and text are never erased.
5. **Review in bulk.** A person reads the batch with `oml review list --author-key <sync key>` and
   approves it with `oml review approve --ids -`.
6. **Check for drift.** `oml sync … --check` writes nothing and exits `1` on `stale`, `removed` or
   `untracked`, so CI can run it after every merge. Add `--record` to record the source checks.
7. **Link memories.** `--edges` proposes the `amends` and `supersedes` links the records state.
   `--edges --ratify` under a second person's key ratifies them.

## Options

| Option | What it does |
|---|---|
| `--rev <REV>` | The commit read; default `HEAD`. Never the working tree. |
| `--repo <REPO>` | The anchor's repository name; omitted, derived from `origin`. |
| `--exclude <GLOB>` | Paths left out of the source, repeatable (`docs/adr/**`). The set is recorded for the source on this clone. |
| `--pr <N>` | Load the records pull request `<n>` adds or changes between `--base` and `--rev` into the draft workarea `<workarea>-pr-<n>`. |
| `--base <BASE>` | With `--pr`, the branch the pull request targets; default `origin/main`. |
| `--retire-all` | Propose retiring every memory this key wrote in the workarea under the source, as when a draft's pull request closes. |

## Refusals

- A source that rewound needs `--allow-rollback`.
- Items already present without the sync's anchor need `--adopt`.
- A changed adapter or exclude set needs `--change-adapter` or `--change-exclude`.
- A file whose name matches a secret pattern (`.env*`, `*.pem`, `*id_*`, `*credentials*`) is
  reported `unreadable` and skipped.

We have not measured whether a reviewer catches a wrong extraction in a batch of 200. All flags are
in the [CLI reference](../cli/reference.md).
