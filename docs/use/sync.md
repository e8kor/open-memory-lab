---
status: Stable
verified: 2026-11-06
answers: "how oml sync keeps a workarea in step with a repository's documents and code, reviewed in bulk"
---

# Sync docs and code

`oml sync` keeps a workarea in step with committed markdown. It reads git's object store at one
commit, extracts claims through an adapter, diffs them against what the workarea holds and
writes the difference, each memory anchored to the repository, commit, path, fragment and
fingerprint of the span it came from.

1. **Pick the source and the adapter.** `adr` yields one claim per decision bullet plus a summary
   and a rejected-alternatives claim; `headings` one claim per heading of level 2 to 4; `lines`
   blocks of at most 40 lines.
2. **Create the workarea and a writer key.** The sync key stays the sync key for good, because a
   writer supersedes only its own tips.
3. **Preview.** `oml sync docs/adr --workarea design --adapter adr --dry-run`. It lists each
   item as `new`, `changed`, `removed`, `moved` or `unchanged` and writes nothing.
4. **Run.** Drop `--dry-run`. A run writes up to 200 items (`--max-writes`); repeat for larger
   sources. The public server is refused unless you pass `--allow-public-server`, because its
   operators read every memory and an anchor and text are never erased.
5. **Review in bulk.** A person reads the batch with `oml review list --author-key <sync key>` and
   approves it with `oml review approve --ids -`.
6. **Re-run and check drift.** `oml sync … --check` writes nothing and exits `1` on `stale`,
   `removed` or `untracked`, so CI can run it after every merge.
7. **Edges.** `--edges` proposes the `amends` and `supersedes` links the records state;
   `--edges --ratify` under a second person's key ratifies them.

**Refusals to expect.** A source that rewound needs `--allow-rollback`; items already present
without the sync's anchor need `--adopt`; a changed adapter or exclude set needs
`--change-adapter` or `--change-exclude`. A file whose name matches a secret pattern (`.env*`,
`*.pem`, `*id_*`, `*credentials*`) is reported `unreadable` and skipped. Whether a reviewer
catches a wrong extraction in a batch of 200 is open work
([](https://github.com/e8kor/open-memory-lab/issues/138)). The verbs are in the
[CLI reference](../cli/reference.md#oml-sync-a-workarea-in-step-with-a-repositorys-documents).
