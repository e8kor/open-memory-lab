<!--
id: contributing/review-checklist
tier: engineering
status: process page
answers: which defects a change is checked for before a reviewer sees it, and which of those a script enforces
-->

# Pre-review checklist

**Status:** process page. Reviewers found the same classes of defect in change after change;
this list is what the author checks first. Paste it into the pull request body
with each box ticked or struck through with a reason.

```markdown
- [ ] No side-channel leaks on held or unreadable rows: counts, timings, 409 details and existence oracles answer the same for a row the caller may not read as for one that is absent.
- [ ] Every free-text field that reaches the journal passes the scan.
- [ ] Each migration is live-safe: `SET LOCAL lock_timeout`, constraints `NOT VALID` then `VALIDATE`, indexes `ON ONLY` the parent with a per-partition `ATTACH` (`scripts/lint_practices.py migrations`).
- [ ] Every new failure mode has a doctor line.
- [ ] The tests were committed red first, before the fix, as a separate commit.
- [ ] No test or script defaults to port 5432 (`scripts/lint_practices.py ports`).
- [ ] No `LIKE`, `ILIKE` or other non-leakproof operator in candidate SQL under row security (`scripts/lint_practices.py like`).
- [ ] No new MCP tool without a use case in `docs/product/use-cases.md` or `docs/engineering/use-case-call-map.md`.
- [ ] The SDKs are regenerated (`python scripts/generate_sdks.py`) and the contract surfaces agree (`python scripts/check_contract_sync.py`).
- [ ] Every ADR and migration number is reserved in `docs/NUMBERS.md` (`python scripts/check_numbers.py`).
- [ ] Reader-facing prose names each action once: headings use the command's verb (write, read), a memory is `held` or approved (state `promoted`), and "approve", "promote" and "ratify" are never swapped. Sentences stay under 25 words.
- [ ] A change fragment is in `changes/`, or the pull request carries the `no-changelog` label.
```

## What each item asks

- **Side channels.** Anything that touches held or unreadable rows answers a caller who may not
  read them. Compare the response, its status, its timing and its counts against the answer for
  a row that does not exist; they must match. A 409 body that names the competing row is an
  existence oracle.
- **Scan.** A free-text field written to the journal (title, note, reason, label, filename)
  goes through the same scan as a claim body. A field the scan does not see is where a secret
  ends up in an append-only record.
- **Live-safe migrations.** The migration runs against a server that is serving. A lock wait
  must time out rather than queue behind a long transaction, a constraint must not scan the
  table under a strong lock, and an index on a partitioned table is built per partition and
  attached.
- **Doctor lines.** A failure the operator can meet has a line in `oml doctor` that names it,
  added in the same change.
- **Red first.** The failing test is its own commit, ahead of the fix, so a reviewer sees it
  fail on the parent.
- **Port 5432.** Tests and scripts read their database from the environment and fail when it
  is unset. A default of 5432 reaches whatever PostgreSQL the machine runs, including a live
  one.
- **Operators under row security.** PostgreSQL may evaluate a non-leakproof operator on a row
  before the row-security policy has filtered it ([ADR-0052](../adr/ADR-0052-text-search-reads-integer-postings-that-row-security-lets-an-index-serve.md)).
  `LIKE` errors and timings can then reveal rows the caller cannot read. A literal prefix
  match that was reviewed carries `// lint:allow-like <why>`.
- **MCP tools.** A tool exists because a documented use case calls it (the Occam rule for the
  client API).
- **SDKs.** The generated clients change with `openapi.yaml`; CI fails on a stale one.
- **Numbers.** [Number registry](../NUMBERS.md): reserve before writing, commit the
  reservation first.

## Escapes

The lints take a reasoned comment on the line: `-- lint:allow <rule> <why>` in SQL,
`// lint:allow-like <why>` in Rust, `lint:allow-5432 <why>` anywhere. The allow-lists under
`scripts/lint_allow/` hold the violations that predate the lints; a new migration is never
added to one.
