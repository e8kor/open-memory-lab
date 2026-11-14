<!--
id: adr/ADR-0064-a-repositorys-knowledge-is-captured-implicitly-by-git-hooks-and-agent-hooks-each-opted-into-per-clone-and-a-person-promotes-it-in-one-bulk-act
tier: engineering
status: accepted design · 2026-11-05 · amended 2026-11-12, accepted by the operator the same day with its thirteen questions decided
answers: how source-code knowledge is captured with no new step for the agent or the person; the two capture modes (git hooks and agent-session hooks), what each does alone and what changes when both are on; the one-time setup and who marks the parent that agents may create children under; per-repository or per-branch workareas in v0.6.1; what the session-start hook resolves and recalls; how the pre-tool hook fills an anchor and an origin on oml_write and which file it chooses; how touched paths are checked at Stop and confirmed only by reviewed supersession; when and how a background sync runs, its queue, its lock and its guards; how the git-hook layer and the agent layer de-duplicate, through the sync identity and idempotency keys; how a chat fact about code carries both its turn and its code anchor; how a person promotes; how other harnesses map onto the shared dispatcher; the latency and friction budgets; what is never read or stored; the evaluation that gates any default; the rejected alternatives, open questions, tests, doctor lines and the phased build
-->

# ADR-0064 — A repository's knowledge is captured implicitly by git hooks and agent hooks, each opted into per clone, and a person promotes it in one bulk act

**Status:** accepted design · 2026-11-05 · planned for v0.6.1 · its nine questions decided by the operator on 2026-11-05 at their recommendations ([the answers](#the-operators-answers-of-2026-11-05)) · amended 2026-11-12 by [the amendment](#the-amendment-of-2026-11-12), which the operator accepted the same day with its thirteen questions decided ([the answers](#the-operators-answers-of-2026-11-12)): the minimal phase is planned for v0.8.0 (#344), and d18's re-mark waits for an amendment of ADR-0030 d2 that this record does not make (#350)

**Release:** planned for v0.8.0 (the minimal phase, d19; the amendment of 2026-11-12, point a and question 1; #344); the record planned it for v0.6.1, which shipped without it; later phases not assigned

**Amends:** ADR-0047 (d2: no claim kind `code` in v0.6.1; d5: one workarea per repository first,
branches later; d11: the staleness workflow's hooks become verbs of the shared dispatcher, and
the Stop check reads the working tree; d13: the duplicate check reads the session cache; d15 b:
the branch parent becomes the code parent, marked by a person's reviewer key; d17: v0.6.1 builds
opt-in beside the harness, and only a default waits for the gate; question 2's answer is applied
per repository first), ADR-0057 (d1: a hook-filled anchor's hash may describe the working tree
over `rev`; d6: `--ff-only`, `--paths` and `--background` on `oml sync`; d8: a source has one
sync identity), ADR-0029 (d1, d4, d7: the plugin's hooks gain three verbs, one reads the sync
key file for a detached run), ADR-0038 (d9: the session cache gains anchor rows and touched
paths, never a body), ADR-0044 (d6: a writer key creates an in-progress child under a code
parent); and, on their branches, ADR-0030 (d1: a child created under a code parent is in progress
at creation, under the person's mark of the parent), ADR-0063 (d2: `chat` admits a code anchor
when `origin.turn` names the turn; d6: one `stop` verb answers for chat capture and for stale
anchors)

**Depends on:** [ADR-0057](ADR-0057-a-memory-carries-the-source-anchor-its-author-declares-and-oml-sync-keeps-a-workarea-in-step-with-a-repositorys-documents.md)
(anchors, `oml sync`, `oml anchor`, `expected_version`, introduced in v0.5.8); ADR-0030 revision 2
(in-progress workareas and the curator, [#79](https://github.com/e8kor/open-memory-lab/pull/79),
planned for v0.6.0); ADR-0061 (the `rust` adapter,
[#70](https://github.com/e8kor/open-memory-lab/pull/70), planned for v0.5.9); ADR-0063 (chat memory
and `origin.kind`, [#78](https://github.com/e8kor/open-memory-lab/pull/78), planned for v0.6.0);
[ADR-0055](ADR-0055-a-person-reviews-a-listed-set-of-held-items-in-one-act-each-item-judged-and-signed-as-one-verdict.md)
(bulk review).

**Numbering.** ADR-0058 to ADR-0063 are taken or left to lanes in flight on 2026-11-05; this
record takes 0064, and its roadmap rows take FR-170 to FR-176, past the chat lane's FR-164. Either
is renumbered at merge if a gap is not taken.

## Context

**The operator's request of 2026-11-05.** Source-code knowledge is mapped implicitly, from the
agent's chat and from other agent integrations, through the same API the native integration uses,
with no friction for the agent or the person. The same day the operator added two directions:
capture runs in two modes, chosen per repository, **git** (hooks that git runs, for every
commit, whoever makes it) and **agent** (hooks the harness runs, for what the agent learns),
either or both; and CI is not a mode in v0.6.1, so a person's bulk approval stays the promotion
path.

**What exists on `main` (v0.5.8).** An anchor `{repo, rev, path, fragment?, hash?, hasher?}` on
any memory, signed and immutable per version, which the server checks and never computes
(ADR-0057 d1–d3); the origin filters `anchor_repo` and `anchor_path_prefix` (d4); `stale` from
source checks recorded by the client (d5); `oml sync` over committed bytes, with its diff by claim
key, its idempotent capped runs, its `moved` and `rollback` outcomes and the refusal of a sync
from an older checkout (d6–d11, d18–d20, built in `45658ba`); `oml anchor <path>[#fragment]`;
`expected_version` on every supersession (d19). The plugin runs `oml hook session-start`,
`guardrails`, `pre-tool-use` (guardrails, on every tool call including `mcp__.*`),
`tool-failure` and `session-end` ([hooks.json](https://github.com/e8kor/open-memory-lab/blob/main/plugins/oml/hooks/hooks.json)).

**What is decided on branches.** ADR-0047 (proposed) stores a small skeleton and the knowledge
the code cannot tell, marks facts stale at edit, asks at Stop, and gates everything past its skill
text on an evaluation harness (d17); its eleven questions are answered (Q1 a: a confirmation is a
reviewed supersession; Q2 a: a workarea per branch; Q5 a: writers create children only under a
parent a person marked; Q11 a: the public server only with `--allow-public-server`). ADR-0030
revision 2 (accepted design, v0.6.0) serves held memories inside an in-progress workarea to every
reader inside, lets a curator agent only narrow, and lets nothing leave before a person commits
with nothing held. ADR-0061 (proposed, v0.5.9) reads a Rust workspace as one claim per crate and
one per module doc. ADR-0063 (proposed, v0.6.0) adds `origin.kind`, a chat anchor
`chat://<harness>/<session>#turn=<n>`, and capture asked of the running agent at Stop, building
its hooks as one dispatcher, `oml hook <event>`.

**What implicit means here.** Today an agent that wants a code fact anchored must run `oml
anchor`, paste the JSON into `oml_write`, choose a workarea and remember to recheck the fact when
it edits the file. Each of those is a step an agent skips. Implicit capture moves every step that
does not need judgement into hooks: the agent writes one sentence, and the hooks say where it is
from, where it goes, and when it may no longer hold. The judgement steps stay where R1 and R2 put
them: the agent decides what is worth writing, and a person decides what is true.

**What it is held against.** The constitution ([ADR-0001](ADR-0001-the-constitution.md)): R1, no
agent key promotes, proposes or ratifies; R2, the reviewer is never the author, per key and
principal; R3, a held row is read by its author and its reviewer, relaxed only inside an
in-progress workarea (ADR-0030 d3); R4, nothing is deleted. The Occam rule
([ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md) d8): a capability is a parameter, a resource
or a hook before it is a tool. ADR-0047's "no stored code map": paths, fragments, hashes and prose,
never a code body.

Decided, not built: this lane writes no code and ran no build.

## Decision

### The two modes and the setup

- **d1 — Implicit capture has two modes, `git` and `agent`, chosen per clone, either or both, and
  recorded in the clone's git config.** `oml setup repo [--mode agent|git|both|off]`, run inside a
  clone, writes `oml.mode` to the clone's local git config (`.git/config`, never a committed file),
  with `oml.workarea` (d4) and `oml.syncKeyFile` (d10). Cloning a repository maps nothing: a clone
  without `oml.mode` is off, whatever the person's other clones say, as ADR-0047 d16 requires.
  `off` removes the git hooks (d7) and the `oml.*` keys, and leaves every memory and the workarea
  where they are (R4).

    | | `git` alone | `agent` alone | `both` |
    |---|---|---|---|
    | Who triggers it | git, on every commit, checkout, merge, rewrite and push, by a person or an agent | the harness, on session start, `oml_write`, file edits and Stop | both |
    | Mechanical sync (crate, module and document claims) | post-commit, post-merge and post-rewrite, in the background (d8) | at Stop, in the background, when `HEAD` moved since the last sync (d14) | git's hooks own it; the agent's Stop sync runs only when the git hooks are found not firing (d10) |
    | Stale marking | source checks for the sync identity's claims at each synced commit; a warning at pre-push for every anchored claim under the pushed paths (d9) | the touched-path check at Stop, from the working tree (d13) | both, each on its own claims |
    | Branches | post-checkout records the branch; only the sync branch is synced (d5) | anchors name `HEAD` whatever the branch | git's record is read by the agent hooks |
    | Offline | a queue in `.git/oml/`, flushed by the next hook (d8) | fails open; the Stop sync is queued the same way | one queue |
    | Knowledge capture (anchor fill, the confirm prompt, chat facts) | none | d11–d13, d15 | the agent's alone |
    | Prompts the agent sees | none | the Stop block (d13), at most once a turn | the agent's alone |

    **The recommended default is `both`**, and `oml setup repo` with no `--mode` takes it after
    printing why: the git hooks see every commit, including a person's commits made outside any
    agent, and run with no harness and no network; the agent hooks are the only place an anchor
    can be filled for what an agent learned; and the overlap costs nothing, since one identity
    owns the sync (d10). `agent` is the choice for a person who will not install git hooks in a
    clone (a shared hook manager, d7); `git` for a clone no agent works in. `both` falls back to
    `agent` with a doctor line when the git hooks cannot be installed.

- **d2 — A person sets up once per person and once per clone; the first `oml setup repo` does
  both.** The per-person part, done the first time and skipped after:
    1. creates `<user>.code` in the person's root if absent ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) d6);
    2. marks it a **code parent** (d3) under the person's reviewer key, a dreamer key `held_by:
       human`, which the command names and asks for when the CLI's stored key is not one;
    3. issues the person's **sync key**, `<user>-code-sync`, writer and reader on `<user>.code`
       and its subtree, `held_by: agent`, to an owner-only file (`--out`, as
       [ADR-0028](ADR-0028-agent-key-issuance.md) issues agent keys) under
       `$XDG_CONFIG_HOME/oml/keys/` (d10).

    The per-clone part derives the repository (d4), refuses the maintainer's public server unless
    `--allow-public-server` (d17), resolves or creates the clone's workarea, records the mode, and
    installs the git hooks when the mode includes `git` (d7). The agent's harness needs nothing
    more than the plugin it already has: the hooks are verbs of the CLI it already runs.

- **d3 — A person marks a parent as a code parent; agent writer keys then create in-progress
  children under it, and nowhere else.** *Amends ADR-0047 d15 b and, on its branch, ADR-0030 d1.*
  `POST /v2/workareas/{workarea}/mode` gains a second mark, `{children: "agent"}`, journalled
  `workarea.mode_set` like the first and admitted under exactly ADR-0030 d1's predicate: a dreamer
  key `held_by: human` with dreamer effective access on the node; every other key is refused
  `access.verb_ceiling`, `details.reason: mode_is_human_only`. A code parent is itself `standard`,
  so its children may each be in progress (ADR-0030 d1 refuses nesting). Under a code parent, and
  only there, `POST /v2/workareas` admits `mode: in_progress` in the create body from a writer key
  with effective `writer` on the parent: the child is `development` and in progress from its
  first instant, journalled as one `workarea.create` carrying the mode and the parent's mark event.
  Anywhere else `mode` in a create body is refused `request.validation` naming `/mode`, and
  creation stays ADR-0044 d6's. This is ADR-0047 question 5, answer a, with the marker moved from
  an admin to the person's reviewer key, since in a user root the person is the one who reviews.
  The person's attestation is the mark of the parent; each child created under it is in progress
  by that attestation, and the person's own commit (d16) is still the only way out. A code parent
  may be unmarked by the same predicate; children already created keep their mode. No MCP tool,
  argument or resource creates a workarea or sets a mark (ADR-0039 d8).

- **d4 — v0.6.1 keeps one workarea per repository, named from the repository, resolved by name
  and cached in the clone.** *Amends ADR-0047 d5, and applies its question 2 per repository
  first.* The repository is ADR-0057 d3's `repo`, derived from the `origin` remote with the scheme,
  user, password and trailing `.git` dropped, as `oml sync` does. Its workarea is
  `<user>.code.<last segment>-<the first 6 hex digits of SHA-256(repo)>`, for example
  `e8kor.code.open-memory-lab-3fa2c1` (digits illustrative): a pure function of the repository, so
  two clones of one repository on two machines find one workarea with no lookup table, and two
  repositories with one last segment never collide ([ADR-0026](ADR-0026-names-are-keys.md)). The
  workarea's description is the full `repo`. The name is cached in `oml.workarea`; the hooks
  resolve it from the cache, and only `oml setup repo` and a cache miss reach the server, one
  `GET /v2/workareas/{name}` and, when absent, one create (d3). A clone without an `origin`
  remote takes `--repo`, else stays off with a doctor line.

- **d5 — Per-branch workareas are not built in v0.6.1; only the sync branch is synced, and
  post-checkout records the branch.** The git hooks make a branch workarea cheap to create (one
  request at post-checkout) but not cheap to own: each is a name kept for good (FR-80), each
  merge needs a person's commit (ADR-0030 d6), every mechanical fact a branch changes is reviewed
  twice (on the branch and again on the sync branch, as ADR-0057 d14 item 3 counts for drafts),
  and a branch switch makes the repository workarea's next sync an older checkout, which the
  stale-revision guard refuses (ADR-0057 d20). The **sync branch** is `origin/HEAD`'s target, or
  `oml.syncBranch` when set. Post-checkout writes the branch name and whether it is the sync
  branch to `.git/oml/state.json` and nothing else. A commit on another branch is not synced; its
  changed paths are checked (d9) so the pre-push warning has an answer. Agent knowledge written on
  any branch goes to the repository workarea, anchored at the `HEAD` it read; a fact about code
  that never merges then reads `removed` or `stale` at the next check on the sync branch, and the
  person rejects it at promotion (d16). ADR-0047 d5's branch workareas are phase 3 (d19), with
  post-checkout as their trigger.

### Layer 1: the git hooks

- **d6 — The git hooks are `oml hook git <event>`, each finishing in the foreground under 100 ms,
  and none ever fails a git operation.** Every hook script is one line,
  `oml hook git <event> "$@" </dev/null >/dev/null 2>&1 || true`, so a missing `oml`, a crash, a
  timeout or a refusal returns 0 to git. In the foreground a hook reads the local git config and
  `.git/oml/state.json`, appends to the queue (d8), and starts a detached job (`setsid`, its own
  process group, output to `.git/oml/log`, rotated at 1 MiB); it opens no network connection.

    | Git hook | Foreground | Background job |
    |---|---|---|
    | `post-commit` | queue the new `HEAD` for each source whose path prefix the commit changed | on the sync branch, d8's sync; on any other branch, d9's check of the changed paths |
    | `post-checkout` (branch switch only, its third argument `1`) | record the branch (d5) | none |
    | `post-merge` | queue `HEAD` for every source the merge changed | d8's sync with `--ff-only` |
    | `post-rewrite` (`rebase`, `amend`) | queue `HEAD`, mark the queue `rewritten` | d8's sync with `--ff-only`; never `--allow-rollback` |
    | `pre-push` | print the cached check of the pushed range (d9), warn-only | a fresh check of the range for the next push |

    The 100 ms is a budget the build measures (FR-172), on a cold page cache, with the queue file
    and the state file under `.git/oml/`.

- **d7 — `oml setup repo` installs the hooks without overwriting any, chains into the hook
  managers, and uninstalls cleanly.** Each installed file or line carries the marker
  `# oml-managed`, and the installer records what it did in `.git/oml/hooks.json`.
    - **Plain hooks** (no `core.hooksPath`, or a local one pointing at an untracked directory):
      an absent hook is written as an oml-only script; a present one gains one marked line after
      its shebang, after a copy to `.git/oml/hooks-backup/`. Nothing else in the file changes.
    - **lefthook:** the commands go into `lefthook-local.yml`, the file lefthook reads beside the
      committed `lefthook.yml` for one clone (lefthook's documentation; not verified by this
      record), so no teammate is enrolled.
    - **A committed hook directory or config** (husky's `.husky/`, the pre-commit framework's
      `.pre-commit-config.yaml`, a `core.hooksPath` under the work tree): the installer writes
      nothing, prints the line for each hook and the reason (a committed hook enrols every
      clone, which d1 refuses), and records the mode as `git` pending; the first hook that fires
      clears the mark. A person who adds the lines to the committed config enrols their team
      knowingly.
    - **Uninstall** (`--mode agent` or `off`): removes each oml-only file the record names whose
      content is unchanged, removes each marked line, and leaves a file that was edited after
      install with a printed note. The queue, the log and the state stay until `off`.

- **d8 — A background sync is the sync identity's `oml sync` over the sources the commit touched,
  serialised by a lock, coalesced by a queue, guarded against rewrites, and bounded.**
    - **Sources.** Recorded per clone in `oml.source` (multi-valued) by `oml setup repo`, each
      `<path>:<adapter>`. The defaults: `rust` (ADR-0061) on the directory holding the workspace
      `Cargo.toml` when there is one; `adr` on `docs/adr` when it holds `ADR-NNNN-*.md`;
      `headings` on `docs` with `--exclude 'docs/adr/**'`, and on the root's `*.md`. Each source
      is its own namespace in the one workarea (ADR-0057 d14's rule, one adapter per source,
      kept). The default excludes, on every source: `target/**`, `vendor/**`, `third_party/**`,
      `node_modules/**`, `dist/**`, `build/**`, `**/*.generated.*`, `**/*.pb.*`, and every path
      the client's secret patterns match (ADR-0057 d6); the adapters' own skips stand (ADR-0061
      d1's `@generated`).
    - **Queue.** `.git/oml/queue.json` holds, per source, the newest commit to sync and the
      reason; a newer commit replaces an older one, since a sync is a diff of state at one commit
      and the newest subsumes the rest. The queue is flushed by the next background job, the next
      hook of either mode, or `oml sync --flush`. Offline, the job leaves the entry and exits; a
      later flush runs it.
    - **Lock.** One `flock` on `.git/oml/sync.lock`; a job that cannot take it at once exits and
      leaves the queue to the holder, which re-reads the queue before releasing.
    - **Guard.** `oml sync --ff-only` (amends ADR-0057 d6) refuses, writing nothing, unless every
      `rev` the workarea's claims of the source were read at, and every `rev` their latest source
      checks name, is an ancestor of the commit to sync. This extends the older-checkout refusal
      (ADR-0057 d20, built in `45658ba`), which catches a commit older than the synced one, to a
      commit beside it, which a rebase of the sync branch or a forced pull produces. The refusal
      is a doctor line (d18), and a person resolves it with an explicit `oml sync`
      (`--allow-rollback` for a deliberate revert). A hook never passes `--allow-rollback`.
    - **Bound.** `--max-writes` 200 per run (ADR-0057 d9); `--paths` (amends ADR-0057 d6)
      narrows the listing and the diff to the changed paths' claims, so a one-file commit lists
      one file's claims; a wall-clock limit of 120 s, after which the job stops between requests
      and leaves the queue entry. `--background` names the run's detached form: no output but
      the log, exit codes recorded in `state.json`.

- **d9 — The pre-push warning reads a check the background already ran.** After each synced or
  checked commit the job runs `oml sync --check --paths <changed paths>` for each source and
  lists, for the same paths, every anchored claim in the workarea, of any author, whose anchor
  hash differs from the span at the commit (the agent's own knowledge included), writing the
  result to `.git/oml/check/<commit>.json`. `pre-push` prints, for the pushed range, the counts
  and the first five claims (`stale`, `removed`, `untracked`), or `not checked yet` for a commit
  with no file; it never blocks. Source checks are recorded on the server (`--record`) only for
  the sync identity's own claims, since the source-check route admits only the author or a
  dreamer (ADR-0057 d5), and the git hooks hold neither for an agent's claims.

### The sync identity and de-duplication

- **d10 — Each source has one sync identity, the key that wrote its first claim, and every other
  key's run of it is a check.** *Amends ADR-0057 d8.* `oml sync` already refuses to supersede a
  tip another key wrote (`foreign`); this makes the refusal whole: when the listing shows the
  source's claims written by another key, the run writes nothing, says which key owns the source,
  and degrades to `--check`. In v0.6.1 the sync identity is the person's sync key (d2) for every
  repository under `<user>.code`, whichever mode starts the run, so the git hooks and the agent's
  Stop sync are one writer. Three properties make a commit synced twice write once:
    1. one writer: the same key, and so the same claim-key namespace and the same `foreign` rule;
    2. one diff: a second run at the same commit finds every item `unchanged` and writes nothing
       (ADR-0057 d8);
    3. one idempotency key: `sync:<workarea>:<claim_key>:<outcome>:<hash16>` names no commit and
       no mode (ADR-0057 d9), so two runs racing past the lock (two clones of one person) replay
       each other's writes inside the server's window, and a supersession that loses the race is
       refused `submission.version_conflict` (ADR-0057 d19) and re-planned by the next run.

    **Ownership.** With `both`, git owns the sync and the agent owns capture: the agent's Stop
    sync (d14) runs only when `state.json` shows no git hook fired since the last commit the
    agent saw, so a broken hook installation degrades to the agent's sync rather than to none.
    With `agent` alone the agent's Stop sync is the only one. The detached job reads the sync key
    from `oml.syncKeyFile`; the hook process that starts it reads only the plugin's key pair and
    the path (amends ADR-0029 d4, whose reason, never reading the CLI's administrator
    credentials, holds: the sync key is a writer key on `<user>.code` alone). A later CI layer
    (d19 phase 3) takes the sync identity by holding the same key in the forge's secret store,
    after which the person's clones run `--mode agent`; no second key ever writes a source.

    | Writes | Owner | Key |
    |---|---|---|
    | crate, module and document claims, their supersessions and retirements | the sync (git hooks, else the agent's Stop sync) | the person's sync key |
    | source checks of those claims | the sync | the person's sync key |
    | knowledge an agent writes, its confirmations, updates and retirements | the agent | the agent's own key |
    | chat facts (ADR-0063) | the agent | the agent's own key |
    | workarea creation under the code parent | whichever first needs it | the sync key or the agent's key |
    | the code-parent mark, verdicts, the commit | the person | the person's reviewer key |

### Layer 2: the agent-session hooks

- **d11 — Every agent hook is a verb of the one dispatcher, `oml hook <event>`, and each prints
  nothing in a clone whose mode excludes `agent`.** *Amends ADR-0029 d7 and, on its branch,
  ADR-0063 d6.* The verbs this record adds or extends:

    | Event | Verb | Adds |
    |---|---|---|
    | `SessionStart` | `session-start` | d12 |
    | `PreToolUse` on `mcp__plugin_oml_oml__oml_write` | `pre-tool-use` (the guardrails verb) | d15 |
    | `PostToolUse` on `Write\|Edit\|MultiEdit\|NotebookEdit` and `Read` | `post-tool-use` (new) | d13's touched and read paths |
    | `UserPromptSubmit` | `user-prompt` (ADR-0063) | the turn count and hash, also when only code capture is on |
    | `Stop` | `stop` (ADR-0063) | d13's check and d14's sync, in one reason with chat capture's |
    | `SessionEnd` | `session-end` | deletes the cache, as ADR-0038 d9 |

    One verb per event, so one process per event and one answer: two PreToolUse hooks that each
    rewrite `oml_write`'s input would race, and two Stop hooks that each block would ask twice.
    The anchor fill joins the guardrails check that already runs on `mcp__.*`; `stop` composes
    chat capture's reason (ADR-0063 d6) and the stale list (d13) into one block. Each verb fails
    open (ADR-0029 d5), reads the plugin's key pair only (ADR-0029 d4), and sends no file content.

- **d12 — Session start resolves the repository and its workarea from the clone's cache and
  recalls the repository's knowledge within a budget.** *Amends ADR-0029 d2, d3.* In a clone
  whose mode includes `agent`: `git rev-parse --show-toplevel`, the cached `oml.workarea`, and, in
  parallel with ADR-0029's two requests, two more: a recall scoped to that workarea with
  `anchor_repo`, the session's directory words as ADR-0029 d3 has them, `k` 6 and `max_tokens`
  600 (chosen, unmeasured, FR-173), injected under the data frame after the own-work lines; and
  the listing of the workarea's anchored claims (ids, versions, paths, fragments, hashes, kinds;
  no text) into the session cache, which d13 and d15 read. A listing over 2,000 claims is cut at
  2,000 and the cut is named (FR-173). The same flush of the queue as any hook (d8) starts in the
  background. Nothing is created at session start: a clone set up for `agent` already has its
  workarea (d2), and a clone without `oml.mode` prints what it printed before.

- **d13 — Touched paths are recorded locally, checked at Stop against the working tree, and a
  stale claim is confirmed, updated or retired only by a reviewed supersession.** *Amends ADR-0047
  d11.* `post-tool-use` appends the repository-relative path of each successful edit to the
  session cache's touched set, and of each read to its read set, with no network request; a path
  outside the clone, ignored by git, or secret-shaped is not recorded. At `Stop`, unless
  `stop_hook_active` is true, `stop` recomputes, from the working tree, the hash of each cached
  claim anchored under a touched path (the span named by its fragment, under its hasher), and
  for each claim the key may supersede (its own, per the `foreign` rule) whose hash no longer
  matches, it blocks once with one reason naming, per claim, its artifact id, its
  `expected_version`, its path and fragment, and the three answers:
    - **confirm:** `oml_write` with `replaces`, `expected_version`, and the same text; the
      pre-tool hook fills the new anchor (d15), and a person reviews the move (ADR-0047 Q1 a);
    - **update:** the same with corrected text;
    - **retire:** `oml_write` with `replaces`, `expected_version` and `retirement: true`.

    No hook re-anchors anything (ADR-0047 question 1): every answer is a held version a
    non-author judges. A span found with the same hash at another place in the same file (lines
    inserted above it) is `moved`: not listed, no review, as ADR-0057 d18 has it for a sync.
    Claims of other keys under touched paths are counted in the reason, not listed, since this
    key cannot supersede them; the next sync's check marks them. At most 10 claims are listed,
    the rest counted. The check sends no request: the anchors are in the cache, and the spans are
    on disk. Budget: under 2 s for 50 touched files (FR-172). A clean check prints nothing and
    does not block.

- **d14 — In agent mode, Stop also starts a background sync when `HEAD` moved.** `stop` compares
  `HEAD` with `state.json`'s last seen commit; when it moved on the sync branch, it queues every
  source and starts the job of d8, under d10's ownership rule, and never waits for it. When the
  moved commits touched paths of the key's own anchored claims, the job also records source
  checks for those claims under the agent's key (ADR-0057 d5 admits the author), so the agent's
  facts read `stale` to every reader without a prompt. *Amends ADR-0029 d1:* a source check is an
  act a hook-started job performs; it changes no text, no anchor and no state of review.

- **d15 — The pre-tool hook fills the anchor, the origin and the workarea of an `oml_write`; the
  agent writes only the sentence.** In a clone whose mode includes `agent`, for a call that sends
  no `anchor`, `pre-tool-use` chooses one file and, when it finds one, answers with the call's
  input completed (`updatedInput` in Claude Code). The choice, in order, first match wins:
    1. **Named paths.** Every token of the text that resolves to a regular file in the clone (a
       repository-relative path, or a basename that matches exactly one path in the session's
       touched or read set). One such file is chosen; two or more anchor nothing.
    2. **Touched files.** Else, the files edited since the session's previous `oml_write`: one is
       chosen; two or more anchor nothing.
    3. **Read files.** Else, the files read since the previous `oml_write`: one is chosen; more
       anchor nothing.
    4. **Nothing.** Else the call goes through unchanged: ambiguity is resolved by not anchoring.

    A file is never chosen, nor opened, when it is outside the clone, a symlink leading outside
    it, ignored by git, under a default exclude (d8), or secret-shaped; the reason goes into the
    hook's message to the agent, not into the write. **The span.** When the text names an
    identifier that appears on exactly one definition line of the file (a table of definition
    keywords per extension: `fn`, `struct`, `enum`, `trait`, `impl`, `mod`, `type`, `const` for
    `.rs`; `def`, `class` for `.py`; `function`, `class`, `interface`, `type`, `const` for
    `.ts` and `.js`; `func`, `type` for `.go`), the fragment is the line span `L<n>-L<m>` of that
    line's indentation block (ADR-0057 d21), hashed under `oml-md/1`, whose normalisation is
    format-agnostic; otherwise the anchor names the file with no fragment and **no hash**, a
    citation that is never stale and is listed `unhashed` by `--check`, so an edit anywhere in a
    file does not prompt (ADR-0047 d4's whole-file hash, rejected below). **The revision.** `rev`
    is `HEAD`; when the file differs from `HEAD`, the hash is of the working tree's span. *Amends
    ADR-0057 d1's reading of `rev`:* a hook-filled anchor states what the author read, which is
    `rev` plus the session's uncommitted edit, and it reads `fresh` at the commit that lands the
    edit; a file the agent created and never committed is anchored the same way. **The rest.**
    `origin.kind` is `code` for a source file and `doc` for markdown (ADR-0063 d1), unless the
    agent sent `origin`; `workarea` is the clone's workarea when the call names none and an
    anchor was filled; an anchor the agent sent with `path` only is completed (`repo`, `rev`,
    `hash`, `hasher`) and never replaced. **Refusals,** each once per call, as a deny with the
    reason, which the agent answers by resending: a text that holds three or more consecutive
    lines of the chosen file, or a fenced code block (d17); an existing claim in the cache on the
    same path and fragment (ADR-0047 d13's duplicate check, now local), unless the resend names
    `replaces` or is the same call again; and the session's sixth anchored write (ADR-0047 d13's
    `capture.max_per_session`, 5). Budget: under 50 ms with the cache warm (FR-172), since it
    sends no request.

- **d16 — A chat fact about code is one claim with one anchor: the code anchor, with the turn
  in `origin`.** *Amends, on its branch, ADR-0063 d2.* When the agent writes a fact a person
  stated, it sends `origin: {kind: "chat"}`; `pre-tool-use` fills the code anchor by d15 and adds
  `origin.turn: {uri, fragment, hash, hasher}`, the chat anchor of the current turn from the
  session cache (ADR-0063 d4, d5). ADR-0063 d2's composition becomes: `chat` requires a chat
  anchor, or a repository anchor with `origin.turn`; `origin.turn` requires `chat`. The code
  anchor is primary because it is the one a read filters by and a check can test: a turn's hash
  can be recognised only by whoever holds the transcript. Both are in the signed submission and
  immutable per version (ADR-0057 d2), and a supersession inherits neither. `origin.turn` is
  stored and served, not filtered by, in v0.6.1 (FR-174). A fact the agent learned by itself
  carries `code` or `doc` and no turn.

### Promotion, privacy and the gate

- **d17 — No code body is stored or sent, client code needs the operator's own server, and a
  secret file is never read.** ADR-0047's "no stored code map" and its d16 hold: a memory holds
  the agent's prose and an anchor of repository, commit, path, line span and hash. The pre-tool
  hook refuses, once per call, a text carrying three consecutive normalised lines of the
  anchored file or a fenced code block (d15), and the server's scan runs over the text and the
  anchor as it does today (ADR-0057 d2). `oml setup repo` refuses `https://memory.doublegate.io`
  ([ADR-0043](ADR-0043-what-oml-releases-defaults-to-the-maintainers-public-server.md)) unless
  `--allow-public-server` is given, after printing that the server's admins read every memory and
  that nothing written can be erased (ADR-0047 question 11, a); the answer is recorded in
  `oml.allowPublicServer`, and every hook of either mode is silent in a clone whose server is the
  public one without it. A file is never opened by any hook when its path matches the client's
  secret patterns, is ignored by git, is outside the clone or is excluded (d8), and the sync reads
  committed bytes only (ADR-0057 d6). The remote is recorded without credentials (ADR-0057 d3),
  and a remote that carries them is a doctor line.

- **d18 — A person promotes in one bulk act per merge or release; curators narrow; no agent and
  no hook promotes.** *Amends ADR-0047 d7 for per-repository workareas.* The repository workarea
  is in progress from creation (d3), so every reader inside (each of the person's agent keys, in
  any harness, and the sync key) reads held writes at once, marked `unreviewed` (ADR-0030 d3,
  d4), and nothing leaves it. A curator key (ADR-0030 d8, d9) may reject duplicates and propose
  retirements inside; it never approves. When the person merges to the sync branch or releases,
  `oml review commit <workarea>` does, under the person's reviewer key and in one sitting: list
  the held set (ADR-0055 d2), write it to a file the person edits to `approve`, `reject <reason>`
  or leave, submit the bulk verdicts (ADR-0055 d3), each signed as one verdict with R2 per item
  and `same_user` marked; then, when nothing is held, commit the workarea (ADR-0030 d6) and mark
  it in progress again, so the next iteration's writes stay inside. A held item left is left: the
  commit is refused while anything is held, and the command says so and stops before the commit.
  The git hooks never prompt; session start shows the held count (ADR-0029's own-work line). The
  cost: approved facts reach reads outside the repository workarea (the user root, a phone) only
  at the commit, not at approval (ADR-0030 d2); the re-mark is one more journalled act per
  iteration.

- **d19 — The build is phased, and v0.6.1 is the minimal phase.**
    1. **v0.6.1, minimal:** d1–d4 (the modes, the setup, the code parent, the repository
       workarea), d5 (per repository only), d6–d10 (the git hooks, the background sync, its
       queue, lock and guards, the pre-push warning, the sync identity), d11–d15 (the dispatcher,
       session start, touched paths at Stop, the Stop-time sync, the anchor fill), d17, d18's
       `oml review commit`, the evaluation harness of d20 in its first form, and the Claude Code
       mapping of d21. Waits for ADR-0030 (v0.6.0) and ADR-0061 (v0.5.9); d16 waits for ADR-0063.
    2. **Next:** the other harnesses of d21 that have hooks (Codex CLI, Gemini CLI, Cursor), and
       the MCP-instructions path for the rest.
    3. **Later:** branch workareas through post-checkout (ADR-0047 d5–d7); a CI layer as an
       optional promotion helper (ADR-0047 d8, d9: the CI key re-derives mechanical claims and
       approves only those, the merge job proposes nothing a person did not); `oml-span/1`
       symbol hashes from ctags (ADR-0047 d4); export to `AGENTS.md` (ADR-0047 d12); claim kind
       `code` if the gate shows ordinary recall crowded (question 2).

- **d20 — The evaluation gate measures implicit capture before anything is on by default.**
  *Amends ADR-0047 d17:* v0.6.1 builds the capture opt-in, beside the harness, and the default
  (any clone captured without `oml setup repo`, or `both` chosen without the prompt) waits for the
  result under question 8's criterion (ADR-0047 question 8, a). The harness of ADR-0047 d17
  (`testing/eval/`, closed issues with a merged fix and a test, run at the fix's parent commit,
  the merged test as the oracle) runs five arms: no memory; `AGENTS.md` exported from the same
  promoted claims; `git` alone; `agent` alone; `both`. Knowledge is captured while solving the
  repository's earlier tasks and promoted by a person before the measured task, as it would be at
  a merge. Per task it records success, input and output tokens (the hooks' injected context and
  the Stop continuations counted), wall time, hook latency per event, the share of served claims
  that were stale, anchored writes per session, the share of anchors a reviewer rejected as
  wrongly placed, and review minutes per merged pull request. Sample size and runs per task are
  unmeasured (FR-78). The anchor fill and the Stop prompt are each switchable off, so each arm
  can isolate them.

### Other harnesses

- **d21 — Every harness reaches the same dispatcher; one without hooks gets the server's
  instructions and the skill.** The harness differences live in one adapter per harness inside
  `oml hook`, which reads that harness's event document and writes its answer; the decisions
  above do not change. Read on 2026-11-05 from each harness's documentation:

    | Harness | Events documented | Input rewrite before a tool | Mapping | Source |
    |---|---|---|---|---|
    | Claude Code | `SessionStart`, `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `Stop` (`decision: block`, `stop_hook_active`), `SessionEnd` | yes: "`updatedInput` directly under `hookSpecificOutput` replaces a tool's arguments before it runs" | d11 as written; MCP tools match `mcp__<server>__<tool>` | [hooks](https://code.claude.com/docs/en/hooks) |
    | Codex CLI | `SessionStart`, `SessionEnd`, `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `Stop`, `PreCompact` and others, in `hooks.json` or `[hooks]` in `config.toml` | yes, and only with `permissionDecision: "allow"` | as Claude Code; because a rewrite there also allows the call, the adapter rewrites only when the person's configuration already allows `oml_write` without asking, and otherwise denies once with the filled anchor in the reason for the agent to resend | [hooks](https://learn.chatgpt.com/docs/hooks) |
    | Gemini CLI | `SessionStart`, `SessionEnd`, `BeforeTool`, `AfterTool`, `AfterAgent`, `PreCompress` and others | yes: `hookSpecificOutput.tool_input` "merges with and overrides the model's arguments" | `BeforeTool` → `pre-tool-use`, `AfterTool` → `post-tool-use`, `AfterAgent` with `decision: deny` → `stop`; MCP tools match `mcp_<server>_<tool>` | [hooks](https://geminicli.com/docs/hooks/), [reference](https://geminicli.com/docs/hooks/reference/) |
    | Cursor | `sessionStart`, `preToolUse`, `beforeMCPExecution`, `afterFileEdit`, `beforeSubmitPrompt`, `stop` | `preToolUse`: yes (`updated_input`), ignored for HTTP MCP tools in cloud agents; `beforeMCPExecution`: allow, deny or ask only | `preToolUse` → `pre-tool-use`; `afterFileEdit` → `post-tool-use`; `stop` → `stop`, whose ability to continue the agent is not verified here | [hooks](https://cursor.com/docs/agent/hooks) |
    | Cline | the SDK's plugin hooks (`beforeTool` may return `input`); the extension's file hooks (`PreToolUse`, `PostToolUse`, `TaskComplete`) are documented only on pages now replaced, and are **unverified** | SDK: yes; extension: unverified | the SDK plugin calls `oml hook` per event; the extension waits for a verified page | [SDK plugins](https://docs.cline.bot/sdk/plugins.md) |
    | Roo Code | no hooks page found; a draft pull request proposes advisory hooks (**unverified**, not shipped) | none documented | MCP instructions and the skill | — |
    | Continue | no hooks page found; its CLI docs suggest calling `cn` from git hooks | none documented | MCP instructions and the skill; the `git` mode covers its commits | [headless mode](https://docs.continue.dev/cli/headless-mode) |

    **Without hooks** the agent sees the server's instructions (ADR-0038 d1), which Claude Code,
    Codex CLI and Gemini CLI document reading (Cursor's MCP page does not mention them,
    unverified), and the `oml-memory` skill where the harness loads skills: both gain two
    sentences, to run `oml anchor <path>[#fragment]` and pass its output when a fact concerns a
    file, and to answer a `stale` hit by `replaces` with `expected_version`. With no hooks the
    `git` mode still syncs and checks every commit, whichever tool made it, which is why `git`
    exists as a mode of its own.

### Budgets

- **d22 — The friction budget: one command per clone for the person, nothing new for the agent;
  and a latency budget per hook.** The person runs `oml setup repo` once per clone (the first
  time, it also does the per-person part of d2 and asks once for the reviewer key), and `oml
  review commit` when they merge or release. The agent sees no new tool, argument or instruction
  it must follow: its `oml_write` gains fields it did not send, its session start gains the
  repository's recall, and its Stop may be blocked once with a list it answers by ordinary
  writes. The budgets, each chosen and to be measured (FR-172):

    | Hook | Budget | Network |
    |---|---|---|
    | any git hook, foreground | under 100 ms | none |
    | `session-start`, cache warm | under 300 ms | the existing two requests and two more, in parallel, 1 s timeout each |
    | `pre-tool-use` on `oml_write` | under 50 ms | none |
    | `post-tool-use` | under 20 ms | none |
    | `stop` check | under 2 s for 50 touched files | none |
    | background sync | never awaited; 120 s limit per job | the sync's requests |

    A hook over its budget finishes and the overrun is logged locally for `oml doctor`; it never
    fails the tool call or the git operation.

- **d23 — Every new failure has a doctor line.** `oml doctor`, in a clone, names: a clone whose
  `oml.mode` includes `git` and whose hooks have not fired since install (a hook manager reset
  `core.hooksPath`, or the committed config was never edited); a hook file the installer marked
  that was edited after install; a queue entry older than a day (offline, or the job failing),
  with the log's last line; a `--ff-only` refusal, with the commits and the command that resolves
  it; a source whose claims another key owns (d10); a clone on the public server without its
  recorded confirmation; an `origin` remote carrying credentials; a sync key file missing or not
  owner-only; a repository workarea that is not in progress, or whose parent lost its code-parent
  mark; held items older than 14 days in the repository workarea (a commit not done); hooks over
  budget, by event, from the local log; and the harness's hook support missing (for example a
  Claude Code version without `updatedInput`), so the anchor fill is off.

## The use cases

Each new call traces to a use case (ADR-0039 d8), declared on the
[use cases](../product/use-cases.md) page and the
[call map](../engineering/use-case-call-map.md) at acceptance.

| Call | Use case |
|---|---|
| `{children: "agent"}` on `POST /v2/workareas/{workarea}/mode`; `mode` on `POST /v2/workareas` under a code parent | UC-CC-01, a person's agents keep each repository's knowledge in a workarea of its own |
| `oml setup repo [--mode]`, `oml hook git <event>`, `oml sync --ff-only --paths --background --flush` | UC-CC-02, keep a repository's documents and crates in step on every commit, whoever commits |
| `oml hook pre-tool-use` filling `anchor`, `origin` and `workarea`; `origin.turn` | UC-CC-03, say where an agent's fact about code came from without the agent writing it |
| `oml hook post-tool-use`, `oml hook stop`'s stale list | UC-CC-04, find out that a fact a person wrote no longer matches the code they just changed |
| `oml review commit` | UC-CC-05, approve a repository's held knowledge in one sitting at a merge or release |

No MCP tool is added, and no argument the agent must learn: `anchor`, `origin` and `workarea`
already exist on `oml_write` (ADR-0057, ADR-0063).

## Test plan

Each step is closed by the tests named, each written red first.

1. **Server:** `only_a_human_reviewer_key_marks_a_code_parent`,
   `a_writer_creates_an_in_progress_child_only_under_a_code_parent`,
   `mode_in_a_create_body_elsewhere_is_request_validation`,
   `a_code_parent_is_standard_and_its_children_may_each_be_in_progress`,
   `origin_turn_requires_chat_and_chat_admits_a_repository_anchor_with_a_turn`.
2. **Sync:** `ff_only_refuses_a_commit_beside_the_synced_one`,
   `ff_only_accepts_a_fast_forward`, `paths_narrows_the_listing_and_the_diff`,
   `a_run_under_a_key_that_does_not_own_the_source_writes_nothing_and_checks`,
   `two_runs_at_one_commit_write_once`, `racing_runs_replay_by_idempotency_key`,
   `a_queue_entry_is_replaced_by_a_newer_commit`, `a_job_that_cannot_lock_leaves_the_queue`.
3. **Git hooks:** `every_git_hook_exits_zero_when_oml_is_missing_or_fails`,
   `a_git_hook_opens_no_connection_in_the_foreground`,
   `install_never_overwrites_an_existing_hook`, `uninstall_removes_only_unchanged_marked_content`,
   `a_committed_hook_config_is_never_written`, `post_commit_off_the_sync_branch_writes_nothing`,
   `post_rewrite_never_passes_allow_rollback`, `pre_push_reads_the_cached_check_and_never_blocks`.
4. **Agent hooks:** `pre_tool_use_anchors_the_one_named_path`,
   `two_named_paths_anchor_nothing`, `one_touched_file_is_anchored_when_none_is_named`,
   `a_secret_shaped_or_ignored_file_is_never_opened`,
   `a_unique_definition_gives_a_line_span_and_a_hash`,
   `no_unique_definition_gives_an_unhashed_file_anchor`,
   `an_anchor_the_agent_sent_is_completed_never_replaced`,
   `three_copied_lines_are_refused_once`, `a_duplicate_anchor_is_refused_once`,
   `the_sixth_anchored_write_is_refused`, `post_tool_use_sends_nothing`,
   `stop_lists_only_own_claims_whose_span_changed_and_blocks_once`,
   `stop_does_not_block_when_stop_hook_active`, `a_span_moved_within_the_file_is_not_listed`,
   `stop_starts_a_sync_only_when_head_moved_and_no_git_hook_fired`,
   `one_stop_reason_carries_chat_capture_and_stale_claims`,
   `no_agent_hook_reads_the_cli_credentials_file`.
5. **Setup and privacy:** `a_clone_without_oml_mode_maps_nothing`,
   `setup_refuses_the_public_server_without_allow_public_server`,
   `setup_records_the_remote_without_credentials`,
   `the_workarea_name_is_a_function_of_the_repository`.
6. **Promotion:** `review_commit_stops_before_the_commit_while_anything_is_held`,
   `review_commit_re_marks_the_workarea_in_progress`, `no_hook_sends_a_verdict`.
7. **Harness:** ADR-0047 d18 step 1's three tests, and
   `each_arm_records_tokens_including_hook_context_and_stop_continuations`.

## Consequences

- **Costs.** One mark and one create field on the server, one journalled act per commit
  iteration (the re-mark), and no new route. Per clone: a queue, a lock, a log and a state file
  under `.git/oml/`, and up to five hook files. Per commit on the sync branch, one background sync
  of the sources it touched. Per session: two more requests at start, and at most one Stop block
  per turn. Review: every anchored fact and every mechanical claim is a held item a person judges;
  a confirmation costs one review (ADR-0047 Q1); an edit in a file whose facts name no unique
  definition costs nothing and checks nothing (d15).
- **Roadmap rows.** FR-170 (the share of anchors the pre-tool hook places on the wrong file, and
  of writes it leaves unanchored), FR-171 (the definition-line table and the indentation block
  as a span, per language, against ctags), FR-172 (every latency budget of d22), FR-173 (session
  start's `k`, token budget and listing cap), FR-174 (filtering by `origin.turn`), FR-175 (the
  repository-workarea count per person and its cost to tree reads, beside FR-80), FR-176 (hook
  managers that reset `core.hooksPath` and the share of clones whose git hooks stop firing).
  FR-78 and FR-79 continue.
- **Pages to change at acceptance.** The HTTP API and openapi-designed.yaml (the mark, the create
  field, `origin.turn`); the CLI reference (`oml setup repo`, `oml hook git`, `oml hook
  post-tool-use`, `oml review commit`, the new `oml sync` flags); the plugin's `hooks.json` and
  README, the `oml-memory` skill, the server instructions; workareas (the code parent); the use
  cases and the call map; releases (v0.6.1); and the Status lines of ADR-0029, ADR-0030, ADR-0038,
  ADR-0044, ADR-0047, ADR-0057 and ADR-0063.

## The operator's answers of 2026-11-05

The operator accepted this record on 2026-11-05 and decided each question at its recommendation; the decisions above already state those answers.

| # | Question | Options | Answer |
|---|---|---|---|
| 1 | May v0.6.1 build capture before the evaluation result? | a. build opt-in beside the harness, default waits (d20); b. harness first, capture after (ADR-0047 d17 as written) | a: the operator scheduled the build for v0.6.1, and opt-in changes no session that did not ask; decided by the operator, 2026-11-05 |
| 2 | Is claim kind `code` (ADR-0047 d2) still needed? | a. not in v0.6.1: `origin.kind` and the filters do its work, revisited if the gate shows recall crowded; b. build it now | a; decided by the operator, 2026-11-05 |
| 3 | What does an anchor say when the file differs from `HEAD`? | a. `rev` is `HEAD`, the hash the working tree's span (d15); b. no hash until the edit is committed; c. defer the write | a: the claim reads `fresh` at the commit that lands the edit; b checks nothing; c loses the fact; decided by the operator, 2026-11-05 |
| 4 | What anchors a fact that names no unique definition? | a. the file with no hash, never stale (d15); b. the file's whole-text hash, stale on every edit (ADR-0047 d4's fallback) | a: b prompts at every edit of a touched file; decided by the operator, 2026-11-05 |
| 5 | Who marks the code parent? | a. the person's reviewer key (d3); b. an admin (ADR-0047 d15 b as written) | a: in a user root the person is the reviewer; decided by the operator, 2026-11-05 |
| 6 | When does the person promote? | a. `oml review commit` at a merge or release, committing and re-marking (d18); b. approve only, never commit, so nothing leaves the repository workarea | a; decided by the operator, 2026-11-05 |
| 7 | What is `oml setup repo`'s default mode? | a. `both`, with the reasons printed (d1); b. `agent`; c. no default, ask | a; decided by the operator, 2026-11-05 |
| 8 | Who holds the sync identity? | a. one sync key per person for every repository (d10); b. one per repository | a: one key file to keep; b when CI takes a repository over; decided by the operator, 2026-11-05 |
| 9 | How does a chat fact about code carry both origins? | a. one claim, the code anchor primary, the turn in `origin.turn` (d16); b. two anchors; c. `derives_from` a chat claim | a: one anchor stays the one origin that filters and staleness read; decided by the operator, 2026-11-05 |

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Per-branch workareas in v0.6.1**, created at post-checkout | Cheap to create, costly to own: a name kept for good per branch, a person's commit per merge, every mechanical fact reviewed twice, and a branch switch that the stale-revision guard refuses on the repository's sync. Phase 3, triggered by the same hook (d5, d19). |
| **CI as a third mode in v0.6.1** | The operator moved it to later as an optional promotion helper; a person's bulk approval is the path, and CI needs a dreamer key held by an agent in the forge (ADR-0047 d8) that a solo user does not have. |
| **Mechanical sync from every branch into the repository workarea** | Each branch switch would sync an older or a sibling commit: the guard refuses it, or, without the guard, the sync retires what the other branch added and reverts what it changed. |
| **The agent passes the anchor itself**, after `oml anchor` | Three steps an agent skips, and a path an agent may misspell; the hook knows which files the agent touched. It stays the path for harnesses without hooks (d21). |
| **Anchor the most recently touched file when several qualify** | A wrong anchor is a signed false statement of origin kept for good (ADR-0057 d2); an unanchored fact is merely less findable. Ambiguity anchors nothing. |
| **A whole-file hash when no definition is unique** | Every edit of the file marks the fact stale and costs the agent a Stop prompt and the person a review, for a fact that likely still holds (question 4). |
| **Two anchors per claim**, a chat anchor and a code anchor | The anchor is one structured origin that filters, staleness and the moved rule read (ADR-0057 d1); an array doubles each and makes "is it stale" ambiguous. The turn rides in `origin` (d16). |
| **The chat provenance as `derives_from` a chat claim** | Two writes and two reviews per fact; `derives_from` widens the reviewer-exclusion closure (R2); and a target in the chat workarea, inside another in-progress subtree, answers `access.not_found` from the repository workarea (ADR-0030 d7). |
| **A capture prompt at every Stop** | ADR-0047 d13: noise is what empties review of meaning. Stop asks only about the agent's own claims whose source it just changed. |
| **A hook that writes knowledge**, extracting facts from the diff or the transcript | It would author memories no agent decided to write, under the agent's key; the agent writes, the hooks only complete. |
| **Silent re-anchoring at Stop** when the text still holds | ADR-0047 question 1 and ADR-0057 d11: an unreviewed change of a promoted memory's origin. |
| **A global `core.hooksPath` or `init.templateDir`** | Every clone would be enrolled, including client repositories never opted into (d1); per-clone opt-in is the privacy rule. |
| **Overwriting existing hooks, or writing a committed hook config** | The first breaks a person's tooling; the second enrols a whole team through one person's setup. |
| **A blocking pre-push check** | A memory server must never stop a push; the warning reads a cached check (d9). |
| **Two sync keys, one per mode** | Each key could supersede only its own tips, so the second would meet `foreign` on every claim the first wrote, and the source would split between two owners (d10). |
| **The agent's key as the sync key** | Mechanical claims would read as the agent's knowledge, and a repository's sync would change owner whenever the person changed harness or key. |
| **A fourth MCP tool, `oml_anchor`** | ADR-0039 d8: the hook carries it, and `oml_write` already takes `anchor`. |
| **A server-side crawler or a server-computed stale mark** | ADR-0057's rejected alternatives: credentials on the server, memories authored by no key, R1 and R2 with no author to check. |

## The amendment of 2026-11-12

**Status of this amendment:** accepted design · 2026-11-12, by the operator, with its thirteen
questions decided ([the answers](#the-operators-answers-of-2026-11-12)) · proposed the same day.
d1 to d23 above stand as accepted on 2026-11-05 and read as points a to h amend them. Nothing in
this section is built. Question 9's answer needs an amendment of ADR-0030 d2 that this record does
not make: it is tracked as [#350](https://github.com/e8kor/open-memory-lab/issues/350), to be
proposed for the operator's acceptance before it is built.

The record was accepted against v0.5.8 and was carried to `main` unchanged on 2026-11-12 from
`feat/implicit-capture` ([#97](https://github.com/e8kor/open-memory-lab/pull/97)). That branch's
build of part of the minimal phase is re-ported, not merged
([#344](https://github.com/e8kor/open-memory-lab/issues/344)). Between the two dates `main` went
from v0.5.8 to v0.7.2. Each point names an accepted sentence that `main` no longer lets a build
keep as written, what `main` holds instead, read at `d5f85c34`, and the replacement. The
intention of every decision is kept, and none gains a d-number. What the points add inside
existing decisions: `oml.server` in the clone's git config, the sync key file kept per origin, and
the `whoami` answer recorded in `.git/oml/state.json` and refreshed by each sync run and each
session start (b, f); setup's print of each source's claim count (question 8); and four doctor
lines (a hook whose pair names another origin, a sync key file of another origin, `capture_key`,
and question 8's refused sync). The back-pointers `· amended by ADR-0064` on the Status lines of
ADR-0029, ADR-0030, ADR-0038, ADR-0044, ADR-0047, ADR-0057 and ADR-0063, which "Pages to change
at acceptance" names, wait for the build; this change adds none.

### a — The release

The Status and Release lines plan the minimal phase (d19, phase 1) for v0.6.1. v0.6.1 was
released on 2026-11-06 without it (`docs/changelog.md`), and `main` is at v0.7.2 with none of d1
to d23 built: the CLI has no `oml setup repo`, `oml hook git`, `oml hook post-tool-use` or `oml
review commit` (`crates/bins/oml-cli/src/main.rs`), and no route takes the code-parent mark.
Every "v0.6.1" in d4, d5, d10, d16, d19 and d20 reads "the release that builds the minimal
phase". The build is tracked as #344, in the v0.8.0 milestone. That release is v0.8.0
(question 1).

### b — d17 asks consent of a server that declares itself shared, and names no server

**What the record says.** d2, step 3, "done the first time and skipped after": "issues the
person's **sync key**, `<user>-code-sync`, […] to an owner-only file […] under
`$XDG_CONFIG_HOME/oml/keys/` (d10)". d2: the per-clone part "refuses the maintainer's public
server unless `--allow-public-server` (d17)". d17: "`oml setup repo` refuses" the URL ADR-0043
made the default "unless `--allow-public-server` is given, after printing that the server's admins
read every memory and that nothing written can be erased (ADR-0047 question 11, a); the answer is
recorded in `oml.allowPublicServer`, and every hook of either mode is silent in a clone whose
server is the public one without it." d23: "a clone on the public server without its recorded
confirmation".

**What `main` holds.**
[ADR-0076](ADR-0076-releases-ship-no-default-server-and-setup-asks-for-the-server-or-reads-it-from-the-key-or-the-environment.md)
(accepted 2026-11-11, built in v0.7.2) d1: no surface a release ships names a server it was not
given, and `scripts/check_no_default_server.py` fails a change that brings the hosted service's
name, a `PUBLIC_SERVER` constant or the phrase "the public server" into the code or the pages. A build of
d17 as written cannot pass it. ADR-0076 d9 replaced the host test of `oml sync` with the server's
own statement: `GET /v2/whoami` answers `shared`, `true` when `server.shared` is set or sign-up is
open; a `whoami` without it, from a server older than v0.7.2, counts as not shared, by the
operator's choice of 2026-11-11; consent is one origin (scheme, lowercased host, port) per value
of the multi-valued `oml.allowPublicServer`; and the value `true`, which names no server, is not
consent (`authorize_shared_server`, `record_shared_server_consent` and `origin_of` in
`crates/bins/oml-cli/src/sync/source.rs`; `authorize_public_sync` in
`crates/bins/oml-cli/src/main.rs`; the doctor lines in `crates/bins/oml-cli/src/sync/doctor.rs`).
The build on #97 wrote `oml.allowPublicServer=true`, the value `main` no longer reads. ADR-0076 d1
also keeps the pair rule whole: a key goes only to a URL its holder named beside it. d10's
detached job reads the sync key from `oml.syncKeyFile`, and the record names no URL for it. Nor
does the key file say which server issued it: d2 issues it once per person, and the build on #97
wrote `oml/keys/<user>-code-sync.key` and skipped the issuance when the file existed
(`crates/bins/oml-cli/src/capture/setup.rs` on `feat/implicit-capture`). A second clone set up
against another server would record that server beside a key the first one issued, and the
detached job would send the key there.

**The replacement.** These sentences replace the four quoted above; the rest of d2, d17 and d23
stands.

- **d2, step 3:** "issues the person's **sync key** for this server, `<user>-code-sync`, writer and
  reader on `<user>.code` and its subtree, `held_by: agent`, to the owner-only file
  `$XDG_CONFIG_HOME/oml/keys/<origin>/<user>-code-sync.key` (d10), where `<origin>` is the
  server's origin as consent records it (ADR-0076 d9: scheme, lowercased host, port) with every
  byte outside `[a-z0-9.-]` percent-encoded, for example
  `oml/keys/https%3A%2F%2Foml.example.org/e8kor-code-sync.key`. This step is done the first time
  for an origin and skipped after. A key file is paired only with the origin whose directory
  holds it: setup refuses, sending nothing, to record an `oml.syncKeyFile` that lies anywhere
  else, the accepted layout's `oml/keys/<user>-code-sync.key` included, and the detached job
  sends nothing under such a file."
- **d2, the per-clone part:** "derives the repository (d4), records the server (d17), asks consent
  of a server that declares itself shared unless the clone already holds it for that origin (d17),
  resolves or creates the clone's workarea, records the mode, and installs the git hooks when the
  mode includes `git` (d7)."
- **d17:** "`oml setup repo` records the server it was run against as `oml.server` in the clone's
  git config: the URL of the pair the person named (ADR-0039 d15, ADR-0076 d1), which is the URL
  the detached job pairs with the sync key. It asks that server `GET /v2/whoami`. When the answer
  says `shared: true`, setup refuses, creating and installing nothing, unless
  `--allow-public-server` is given or the clone's `oml.allowPublicServer` already holds the
  server's origin. With the flag it prints what `oml sync` prints, that the server's admins can
  read submitted memories and that superseding or retiring a memory does not erase submitted text
  or anchors, and records the origin through the function `oml sync` uses. One consent per clone
  and origin covers setup, every hook and every sync. A server that answers `shared: false` is
  asked nothing. Every hook of either mode is silent in a clone unless the server it would reach
  has the origin of `oml.server`, and that server either did not declare itself shared when it
  was last asked or has its origin among the values of `oml.allowPublicServer`. The answer is the
  latest one recorded in `.git/oml/state.json`: setup records it, each sync run asks `whoami`
  before it writes (ADR-0076 d9) and records it, and session start records the `shared` of the
  `whoami` it already reads (point f), at no further request. So no git hook opens a connection
  in the foreground to decide (d6), and the fill sends no request (d15)."
- **d23:** "a clone whose server declares itself shared without consent recorded for its origin
  (ADR-0076 d9's `warn sync_shared_server`, its remedy naming `oml setup repo
  --allow-public-server` as well); a clone holding `oml.allowPublicServer=true` (`warn
  sync_consent_unbound`); a hook whose pair names another origin than `oml.server`; an
  `oml.syncKeyFile` outside the directory of `oml.server`'s origin (`warn
  capture_sync_key_origin: <file> is not the sync key file of <origin>, so the background sync
  sends nothing; run oml setup repo against <origin>`)".

`oml.allowPublicServer` keeps its name and takes ADR-0076 d9's meaning: one origin per value,
shared with `oml sync`, `true` never written and never read as consent. The flag keeps its name.
Test plan step 5's `setup_refuses_the_public_server_without_allow_public_server` becomes
`setup_refuses_a_server_that_declares_itself_shared_without_consent_for_its_origin`, beside
`consent_for_one_origin_covers_no_other_in_setup_or_in_a_hook`,
`a_recorded_true_is_not_consent_for_setup`,
`a_hook_is_silent_when_its_pair_names_another_origin_than_the_clones_server`,
`setup_issues_one_sync_key_per_origin_and_pairs_no_key_file_with_another_origin` and
`session_start_records_the_servers_shared_answer_without_another_request`.

**What it keeps and what it costs.** Client code still reaches a server other people use only
after the person said so for that server, and every hook is still silent without it. The accepted
refusal held against one named host whatever that host answered. The amended one rests on the
server's own `shared`, which ADR-0076 d9 calls a disclosure and not a security control: a shared
server whose operator leaves `server.shared` unset is asked nothing. ADR-0076 d9's other case, a
`whoami` that omits `shared`, does not reach setup, since a server that serves d3's mark is newer
than v0.7.2. The recorded answer can be stale: a server that becomes shared after setup keeps
receiving hook-filled anchors until the next `whoami` is recorded. The session-start refresh
leaves one window, a session already running when the server changed, until its next sync run or
its end; and a session start whose `whoami` goes unanswered refreshes nothing.

### c — d4's workarea is referenced as `<user>.<name>`

d4 names the repository workarea `<user>.code.<last segment>-<6 hex>`, for example
`e8kor.code.open-memory-lab-3fa2c1`. That has three labels.
[ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
d5 accepts a qualified name of "exactly two labels joined by one `.`" and refuses a value with
more as a path, `request.validation`
([ADR-0026](ADR-0026-names-are-keys.md) d7). The build on #97 met the refusal
(`crates/bins/oml-cli/src/capture/mod.rs` on `feat/implicit-capture`). d4 as amended:

- The workarea is created with `parent: <user>.code` and `name: <label>-<6 hex>`. Its reference,
  cached in `oml.workarea` and read with `GET /v2/workareas/{name}`, is the qualified name
  `<user>.<label>-<6 hex>`, for example `e8kor.open-memory-lab-3fa2c1` (digits illustrative). Its
  path, `universe.<user>.code.<label>-<6 hex>`, is displayed and never accepted.
- `<label>` is the repository's last segment made a name under ADR-0026 d2
  (`crates/oml-domain/src/name.rs`: a lowercase label of at most 63 characters, a letter first,
  that does not read as an id). Every clone must compute the same name, so the rule is exactly
  the one the build on #97 has (`workarea_name`, `crates/bins/oml-cli/src/capture/mod.rs` on
  `feat/implicit-capture`), in this order: (1) the text after the last `/` of `repo`, lowercased;
  (2) every character outside `[a-z0-9]` becomes `-`; (3) leading and trailing `-` are cut; (4)
  `repo-` goes first when the result does not start with a letter, an empty result included; (5)
  `repo-` goes first when the result holds only hex digits and `-`; (6) the result is cut to its
  first 56 characters; (7) trailing `-` are cut again, so a cut that ends on `-` gives
  `x-3fa2c1` and not `x--3fa2c1`. The name is that label, `-`, and the first six hex digits of
  SHA-256(`repo`), so it stays a pure function of the repository.
- A user namespace holds every name beneath that user's root (ADR-0044 d5), so the name is unique
  in the person's whole root, not under `<user>.code` alone. `name.taken` at creation means
  another workarea of the person holds it: setup names that workarea and stops.

### d — What the record waited for is on `main`

| The record depends on | The record says | On `main` | What changes here |
|---|---|---|---|
| ADR-0030 revision 2: in-progress workareas, the curator, the commit | accepted design, planned for v0.6.0, #79 | introduced in v0.6.0 with migration 0028: `workareas.mode`, `POST /v2/workareas/{workarea}/mode`, the boundary rule (d2), the curator profile (d8), the commit (d6) | d19's wait is over. d3 adds its mark to the route as served. A held version the scan held, or one not yet scanned under the server's rule set, stays author-only inside (ADR-0030 d3, amended 2026-11-10), so d18's "reads held writes at once" has that exception. d18's re-mark meets ADR-0030 d2: point h, first item. |
| ADR-0061: the `rust` adapter | proposed, planned for v0.5.9, #70 | accepted 2026-11-06, introduced in v0.6.1: `oml sync --adapter rust`, never inferred, one claim per crate and per module doc; its writes carry `origin.kind` `doc`, as every adapter's do (`write_body`, `crates/bins/oml-cli/src/sync/plan.rs`) | d19's wait is over and d8's default `rust` source can be recorded. ADR-0063 d2 gives the `rust` adapter `code` and `plan.rs` sends `doc`: the two disagree, and a follow-up issue asks which stands. The adapter is a function of every file under the source (ADR-0061 d4), and a crate claim states its dependents (d6), so an edit of one crate's manifest can change another crate's claim. d8's `--paths` therefore cannot narrow a `rust` source to "the changed paths' claims": it decides only whether the source is queued (question 4). |
| ADR-0063: chat memory, `origin.kind`, the chat anchor, capture at `Stop` | proposed, planned for v0.6.0, #78 | accepted, introduced in v0.6.0 with migrations 0029 and 0030; `user-prompt`, `stop` and `pre-compact` run through one dispatcher (`crates/bins/oml-cli/src/events.rs`) | d16 no longer waits. `origin` is a closed object (ADR-0063 d1), so `origin.turn` is a new member in the contract and a change to d2's composition check, both unbuilt. The dispatcher differs from d11's reading in two places: point e. |
| ADR-0055: bulk review | a dependency, by link | introduced in v0.5.7: `oml review list --workarea`, `oml review approve` and `reject --ids -`, at most 200 items in one bulk verdict | d18's `oml review commit` submits one bulk verdict per 200 held items. |
| ADR-0057: anchors and `oml sync` | introduced in v0.5.8 | as the record has it; d6's consent is ADR-0076 d9's (point b) | none beyond b. |
| ADR-0047: project knowledge | proposed | still proposed and not built | none. |

### e — d11 against the dispatcher `main` runs

`plugins/oml/hooks/hooks.json` and the CLI's `oml hook` verbs (`crates/bins/oml-cli/src/main.rs`,
`hook.rs`, `guardrails.rs`, `chat.rs`, `events.rs`) on `main`:

| Event | Verb | What it does on `main` |
|---|---|---|
| `SessionStart` | `session-start` | under the plugin's pair, the own-work listing and one budgeted recall, then `GET /v2/whoami` and the default workarea's read for the brief; one after another, 3 s a request, 8 s in all |
| `SessionStart`, `SubagentStart` | `guardrails` | loads the key's reviewed rules as context and, at session start, writes the session's rule cache |
| `PreToolUse` on `^(Bash\|PowerShell\|Write\|.*Edit\|Read\|Grep\|Glob\|WebFetch\|mcp__.*)$` | `pre-tool-use` | checks the call against the rule cache with no request; answers deny, ask or nothing, never allow; a failure blocks the call (`onFailure: block`) |
| `PostToolUseFailure` on `Bash` | `tool-failure` | with `OML_FAILURE_RECALL=1`, one recall from the error's words |
| `UserPromptSubmit` | `user-prompt` | through the dispatcher: counts and hashes the turn when chat capture or per-turn recall is on; per-turn recall |
| `Stop` | `stop` | through the dispatcher: chat capture's block when a capture is due; 1 s in the CLI, 2 s in `hooks.json` |
| `PreCompact` | `pre-compact` | through the dispatcher: records the compaction |
| `SessionEnd` | `session-end` | deletes the session cache and keeps the chat turn count |

No `PostToolUse` hook exists. d11's table, restated against that; "one verb per event" holds as
written, since every behaviour joins the verb its event already runs and only `PostToolUse`
gains one:

| Event | Verb | ADR-0064 adds |
|---|---|---|
| `SessionStart` | `session-start` | d12: the repository recall, the anchor listing and the queue flush. `guardrails` stays the event's second command; it adds context and decides nothing, so d11's race does not arise |
| `PreToolUse` | `pre-tool-use` | d15, as a second handler after the guardrail check, on a call to the plugin's `oml_write` (below) |
| `PostToolUse` on `^(Write\|.*Edit\|Read)$` | `post-tool-use`, new, with a new `hooks.json` entry | d13's touched and read paths |
| `UserPromptSubmit` | `user-prompt` | the turn count and hash also when the clone's mode includes `agent`, for d16; today the handler returns unless chat capture or per-turn recall is on |
| `Stop` | `stop` | d13's check and d14's sync, as a second handler row of `events.rs` |
| `SessionEnd` | `session-end` | deletes the anchor rows and the touched and read sets with the cache |
| `PostToolUseFailure`, `PreCompact`, `SubagentStart` | as on `main` | nothing |

Four sentences of d11, d13 and d22 change to fit:

- **The tool name.** d11 matches `mcp__plugin_oml_oml__oml_write`, the plugin's name for the tool,
  which stands. The same tool reached through a claude.ai connector is, in Claude Code,
  `mcp__claude_ai_<the connector's name>__oml_write`: `mcp__claude_ai_Open_Memory_Lab__oml_write`
  in a session of 2026-11-12 whose connector is named "Open Memory Lab". The middle part is the
  name the person typed, and Claude Code's hooks page does not state the form (observed, not
  documented). The hook cannot tell which server such a tool reaches, and point b lets a hook act
  only for the clone's own server. The fill does not match it: it fills the plugin's tool only
  (question 5).
- **The fill inside the guardrails verb.** `pre-tool-use` never answers `allow` and fails closed
  ([ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md));
  d11's "each verb fails open" holds for the fill's handler, not for the verb. The fill answers
  `updatedInput` with no `permissionDecision`, a guardrail's deny or ask wins and
  drops the fill, and a fill that fails prints nothing and leaves the guardrail's answer. Claude
  Code's hooks page, read on 2026-11-12, says of `updatedInput`: "Replaces the entire input
  object, so include unchanged fields alongside modified ones", so the fill sends every field of
  the call; and: "With no `permissionDecision`, auto mode can deny the changed call", after which
  "Claude can issue the changed call again so that the server reviews it". In auto mode the hook
  can therefore cause the agent's own `oml_write` to be denied, which sets ADR-0037 d8 and d9's
  never-`allow` against d22's "nothing new for the agent". How often it happens is unmeasured,
  and d20's arms record it. The rewrite stays without `permissionDecision` (question 12).
- **One block at `Stop`.** d11 has `stop` compose chat capture's reason and the stale list "into
  one block". The dispatcher as built keeps the first block and drops the rest
  (`events.rs`, "the first block decision wins"). It joins the reasons in handler order instead,
  which the accepted test `one_stop_reason_carries_chat_capture_and_stale_claims` already asks.
- **The Stop budget.** d13 and d22 give the check "under 2 s for 50 touched files". The verb's
  deadline is 1 s (`SESSION_END_DEADLINE`, `hook.rs`) and the plugin's `Stop` timeout is 2 s, so
  a check at its budget is cut off. The `stop` verb's deadline becomes 3 s and the plugin's `Stop`
  timeout 5 s (question 6).

### f — ADR-0044 d16: only the person's own keys start memories in their root

*This point rests on ADR-0044 d16, which the operator accepted on 2026-11-12 and which is on `main`
since [#339](https://github.com/e8kor/open-memory-lab/pull/339) merged (`548dc46e`), for v0.7.3;
read there, in `docs/adr/ADR-0044-*.md`.*

d16: "A memory is started in a user root, or in a workarea beneath one, by that user's keys only;
any other key is refused `submission.user_root_refused`. A successor or a retirement of a served
memory there stays under the grants." Its test reads the key's person: a session key's
`session_user`, else the name of the root a user key's `user` names; a key with neither acts for
no one. Every repository workarea lies beneath `universe.<user>`, so:

- **d2, step 3.** The sync key is issued naming its person, `user: <user>` (ADR-0044 d2, d11), as
  the build on #97 issued it. A sync key that names no user is refused its first claim.
- **d10, the rows "knowledge an agent writes" and "chat facts".** The agent's own key must be one
  of that person's keys: one an admin issued naming them, or one the person issued on their
  account page. A tenant key, which names no user, or another person's key starts no memory in a
  clone set up under `<user>.code`: it is answered `submission.user_root_refused` where it reads
  the workarea and `access.not_found` where it does not. d13's three answers are successors and
  stay under the grants, but such a key wrote nothing there to answer for. A workarea there is
  already created by a user's key or an admin's, never by a tenant writer key (ADR-0044 d6).
- **The put-back.** d16's guard gives a successor of a ratified retirement to the person's keys
  alone. One capture path proposes one: the sync answers a claim that was retired and that the
  source states again with a successor of the retirement (`plan.rs`, "the claim was retired; the
  source states it again"), under the sync key, which is one of the person's. d13's three answers
  replace a version still in service, and d18 proposes nothing.
- **d12 and d15.** Session start learns the key's `user` from the `GET /v2/whoami` it already
  sends for the brief (`default_brief`, `crates/bins/oml-cli/src/hook.rs`). When it is not the
  first label of `oml.workarea`, the fill, the Stop check and the Stop sync stay off for the
  session, so no write is sent to be refused, and session start says once: `oml capture: this key
  is not one of <user>'s keys, so nothing is captured into <workarea>; give the agent one of
  <user>'s keys`. The recall of d12 runs wherever the key reads. The client's test on `whoami`'s
  `user` is enough, though d16 also reads `session_user`: capture's keys are bearer keys, and an
  `omls_` session credential is refused on `/mcp` always and on `/v1` and `/v2` outside `oml-web`
  (ADR-0070 d2), so no hook holds one.
- **d22, the `session-start` row.** It counts "the existing two requests and two more, in
  parallel, 1 s timeout each". `main`'s session start sends four, one after another, at 3 s a
  request and 8 s in all (point e). With d12's two it sends six, and this point adds none, since
  its test and point b's `shared` read the `whoami` already sent. Whether the six run in parallel
  under the row's 300 ms is for the build to measure (FR-172).
- **`oml setup repo`** issues the sync key with `user`, and ends with the line that the agents
  working in this clone need keys of `<user>`'s.
- **d23 gains** `warn capture_key: this agent key names no user (or names <other>), and
  <workarea> lies in <user>'s root, where only <user>'s keys start memories
  (submission.user_root_refused); give the agent one of <user>'s keys`, beside d16's own
  `user root` line.

A repository a crew captures under one shared tenant key has no place beneath one person's
`<user>.code`. The record is per person (d2), and d16 makes that a server rule.

### g — The sync's claims are metered on the person's account

[ADR-0071](ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md)
(accepted 2026-11-12, on `main` since #324) charges a write to the account of the key that wrote
it, the account being the user root the key names; a tenant key belongs to no account (d1). The
sync key names its person with or without point f: it creates the repository workarea when
it is the first to need it (d10's table), and in a user root only a user key or an admin creates
one (ADR-0044 d6), which is why the build on #97 issued it with `user`. The sync identity is that key (d10), so every mechanical claim
is one live chain on that person's `memories`, its text on their `text_bytes`, and each version a
write on their month's `writes` (d2); the commit refuses past a limit with `429 limit.quota` (d3).
The sync key is one more key on their `keys`.

**How many.** Counted on this repository at `d5f85c34` by the adapters themselves: a scratch
program outside the tree linked the `oml-cli` library and called `sync::run::load` and
`sync::run::extract` once per default source of d8, under d8's default excludes. An item is a
claim a first sync writes.

| Source (d8's defaults) | Files read | Claims | Text, bytes |
|---|---|---|---|
| `rust` on the workspace root | 328 | 192: 22 crates, 170 module docs | 111,682 |
| `adr` on `docs/adr` | 71 records | 947: 71 summaries, 807 decisions, 69 rejected-alternatives items | 1,604,426 |
| `headings` on `docs`, excluding `docs/adr/**` | 89 | 691 | 663,928 |
| `headings` on the root's `*.md` | 6 | 41 | 21,920 |
| **Total** | | **1,871** | **2,401,956** |

The `adr` adapter writes a claim per decision, not per record. Of the 691 heading claims, 205
come from two generated reference pages and the release notes (`docs/api/http-api.md` 96,
`docs/cli/reference.md` 85, `docs/changelog.md` 24), which d8's default excludes do not name.
This record adds 25 claims to the `adr` source.

**The limits today.** The `default` profile limits `memories` at 1,000, `keys` and `connections`
at 5 and requests at 120 a minute per key; `text_bytes` and `writes_per_month` are unlimited
until set. An account of the sign-up tenant is on `quota.signup_profile`; an account outside it
is under no limit until an admin assigns one, and is metered either way (ADR-0071 d5;
`docs/operations/configuration.md`). The free profile's numbers are the operator's open decision
([#180](https://github.com/e8kor/open-memory-lab/issues/180)). On the `default` profile a first
sync of this repository is refused at its 1,001st claim, before an agent has written one fact. At
d8's 200 writes a run it is ten runs and ten bulk verdicts (ADR-0055 d3), and at 120 requests a
minute it takes about sixteen minutes of the sync key's rate. They are counted like any memory
(question 8).

### h — Other sentences that no longer build as written

- **d18's re-mark closes the workarea again, which is the effect the operator declined.** d18
  commits and then marks the workarea in progress again, and states the cost as "approved facts reach reads outside the repository workarea (the
  user root, a phone) only at the commit". ADR-0030 d2 as built serves a row on a node inside an
  in-progress subtree to no read scoped outside it, "promoted rows included", and a re-mark
  "writes no row" (`crates/oml-domain/src/in_progress.rs`, `crosses_into`). `oml review commit`
  re-marks in the same sitting, so once it returns a read scoped outside finds none of the
  repository's facts, approved ones included, and the next commit changes that only until its
  own re-mark: the facts are read inside the workarea, or by id. That is what option b of
  question 6 of 2026-11-05 offered, "approve only, never commit, so nothing leaves the repository
  workarea", and the operator chose a over it. Decided on 2026-11-12 (question 9, c): the re-mark
  stays, and a read scoped outside is to be served the promoted rows a commit released, by an
  amendment of ADR-0030 d2 that is not made here
  ([#350](https://github.com/e8kor/open-memory-lab/issues/350)) and that the re-mark does not ship
  before.
- **A code snippet is a claim format since v0.6.1.**
  [ADR-0056](ADR-0056-a-claim-version-names-its-format-the-signature-covers-its-source-and-adapters-compiled-into-the-server-derive-what-search-reads.md)
  d1 and d7 admit `format: text/vnd.oml.code; lang=<tag>`. d15 and d17 refuse a fenced block and
  three copied lines, and name no format, so a write that declares the code format passes them
  with a code body. The pre-tool hook refuses the code format on a write it anchors, once per
  call, as for a fenced block (question 10).
- **The per-person part needs keys a signed-up person does not hold.** d2 marks the code parent
  and d18 commits "under the person's reviewer key" in the CLI, and d2 issues the sync key, which
  the build on #97 did under an admin key. A bearer reviewer key is issued by an admin
  (`docs/guides/reviewing.md`). A person who signed up on a server with open sign-up reviews as
  their `<user>-review` session key on the web console, which the CLI cannot present
  ([ADR-0070](ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md)
  d2), and the console issues reader and writer keys only (`crates/clients/oml-web/src/keys.rs`).
  The minimal phase serves a person who holds a bearer reviewer key and a sync key an admin
  issued; elsewhere `oml setup repo` and `oml review commit` name the missing key and stop
  (question 11).
- **Roadmap rows, the use-case pages and the release page.**
  [ADR-0066](ADR-0066-open-work-is-tracked-as-github-issues-and-the-roadmap-fr-numbers-and-design-draft-status-lines-are-retired.md)
  (accepted 2026-11-06) retired FR numbers and the roadmap, and the restructure deleted the
  use-cases page, the call map and the releases page. FR-170 to FR-176 resolve to #344 through
  `docs/contributing/fr-to-issues.md` until each is an issue of its own (question 13); FR-78 to FR-80 are
  [#142](https://github.com/e8kor/open-memory-lab/issues/142). UC-CC-01 to UC-CC-05 are declared
  in this record's own table, whose two page links no longer resolve. Of "Pages to change at
  acceptance", the use cases, the call map and releases no longer exist; the build changes
  `openapi-designed.yaml` or `openapi.yaml`, the generated references, the plugin's files and the
  guides under `docs/use/` and `docs/guides/`.

### The operator's answers of 2026-11-12

The operator accepted this amendment on 2026-11-12 and decided its thirteen questions in one
sitting, each at the option this record recommended: questions 9, 8 and 1 one by one, and
questions 2 to 7 and 10 to 13 together, all at their recommendations. The points above already
state those answers. Each row keeps its question and options as they were put, and the reason the
recommendation gave.

| # | Question | Options | Answer |
|---|---|---|---|
| 1 | Which release builds the minimal phase (a)? | a. v0.8.0, where #344 sits; b. ahead of it, in a v0.7.x after v0.7.3; c. v0.8.3, with the other agent integrations | a: v0.7.3 is the public opening and its bulk is already in review; the re-port still owes its PostgreSQL suites, an independent review and d20's harness. It ships opt-in either way (question 1 of 2026-11-05). The cost: the capture waits for v0.7.3 and competes with v0.8.0's access hardening; decided by the operator, 2026-11-12, at the recommendation |
| 2 | Does d17 become the shared-server consent of point b? | a. yes: the server's own `shared`, consent per origin in `oml.allowPublicServer`, hooks silent without it; b. ask consent of every server at setup, shared or not; c. decline, leaving d17 unbuildable under ADR-0076 d1 | a: one rule for sync and capture, and no release names a server. b makes every self-hoster confirm their own server per clone, which ADR-0076 rejected for sync. The cost of a is in point b: a shared server that does not say so is asked nothing, and a server that becomes shared after setup keeps receiving hook-filled anchors from a session already running, until that session's next sync run or its end. The session-start `whoami` of point f refreshes `shared` for every later session at no extra request; decided by the operator, 2026-11-12, at the recommendation |
| 3 | Is the repository workarea referenced as `<user>.<label>-<6 hex>`, with the label rule of point c? | a. yes; b. keep three labels and amend ADR-0044 d5 to accept a path | a: ADR-0026 d7 refuses every path as a reference, and a qualified name survives a re-parenting. The cost: the name must be unique in the person's whole root; decided by the operator, 2026-11-12, at the recommendation |
| 4 | How does `--paths` treat a `rust` source (d)? | a. it decides only whether the source is queued, and the run reads and diffs the whole source; b. the adapter reports which claims a changed path feeds, and the run narrows to those | a: a crate claim reads several files and other crates' manifests, and the whole source of this repository is 192 claims, one listing page. b when a workspace's listing exceeds a few pages; decided by the operator, 2026-11-12, at the recommendation |
| 5 | Which `oml_write` does the pre-tool hook fill (e)? | a. the plugin's tool only; b. any `mcp__…__oml_write`, a connector's included | a: the hook knows the plugin's server and can hold it to `oml.server` (b); it cannot tell which server a connector's tool reaches, and a filled anchor names the repository, the commit and a path. The cost: a fact written through a connector in Claude Code is not anchored; decided by the operator, 2026-11-12, at the recommendation |
| 6 | What gives the Stop check its 2 s (e)? | a. the `stop` verb's deadline becomes 3 s and the plugin's `Stop` timeout 5 s; b. the deadlines stay and d13's budget becomes under 1 s | a: the budget was accepted and is still unmeasured (FR-172); chat capture's handler does no file work and is not slowed. The cost: a hung `stop` holds every plugin user's turn for 5 s, where it held it for 2; decided by the operator, 2026-11-12, at the recommendation |
| 7 | With ADR-0044 d16 accepted, does capture run only under the person's own keys, with the lines of point f? | a. yes; b. only the sync key names the user, and agents' facts go to a workarea outside the person's root | a: it is what d16 enforces, and one workarea per repository keeps d10's single owner. The cost: a crew's shared tenant key captures nothing until shared capture is designed; decided by the operator, 2026-11-12, at the recommendation |
| 8 | How are the sync's claims counted (g)? | a. like any memory: the profile's limits must fit the repositories a person syncs; b. not counted when the sync key wrote them; c. a second limit for chains under a code parent | a for the minimal phase: a mechanical claim is stored, reviewed and recalled like any other, and an unmetered key is a way round every quota. `oml setup repo` prints each source's claim count beside the account's free `memories` before the first sync, and a refused sync keeps its queue entry and gets a doctor line. The cost: on a `memories` limit of 1,000 this repository does not fit, so #180's free limit decides who can use capture, and the 205 generated-page claims count too. c after d20's evaluation shows which sources recall uses; it adds a seventh meter and a tree read in the commit. b would rest on the key, and a key says nothing of what it writes: the sync key is a writer key on `<user>.code` kept in a file on the person's machine, so any text sent under it, by `oml sync` or by hand, would be stored uncounted, and this repository's 2.4 MB of text costs the server the same whichever key wrote it; decided by the operator, 2026-11-12, at the recommendation |
| 9 | What does `oml review commit` do after the commit (h)? On 2026-11-05 the operator chose "committing and re-marking" over "approve only, never commit, so nothing leaves the repository workarea" (question 6 of 2026-11-05), and d18 states the cost as facts reaching reads outside "only at the commit". ADR-0030 d2 as built leaves a re-marked subtree out of every read scoped outside it, so the re-mark as accepted has the effect of the option declined | a. mark in progress again, as accepted: once the command returns no read scoped outside finds the repository's facts, approved ones included; they are read inside the workarea or by id. This is the effect of question 6's option b, which the operator declined. Nothing to build beyond d18; b. leave the workarea `standard` after the commit: approved facts are served wherever the key reads, from each approval on, and after the first commit `oml review commit` is the bulk verdicts alone. A new write is held and author-only until reviewed (R3), so the person's agents and the sync key stop reading each other's unreviewed facts, which d18's first sentence rests on, and d3's in-progress creation serves the first iteration only. Nothing to build, and d23's line for a repository workarea that is not in progress goes; c. keep the re-mark, and serve to reads scoped outside the promoted rows of an in-progress subtree that a commit released: a change to ADR-0030 d2, proposed as its own amendment. It would have to guarantee at least that a read outside is served only versions promoted before the subtree's latest commit, never a held one and never one approved since the re-mark, and that a demotion or an erasure inside takes effect outside at once. Its cost: a recorded commit point per root, where d2 stamps nothing on a row; a second predicate in every retriever's statement, before its `LIMIT` and its statistics (ADR-0049 d2); `P-R3-BOUNDARY` restated; and a rule for what a read outside is served of a chain superseded since the commit | c: it keeps both things the operator accepted on 2026-11-05, the reading of unreviewed facts between the person's agents inside (d18, ADR-0030 d3) and approved facts leaving at the commit (question 6, a). The cost: an amendment of ADR-0030 d2 accepted and built on the server before the minimal phase ships, the largest build of the three. That amendment is not made by this answer or in this record: it is tracked as [#350](https://github.com/e8kor/open-memory-lab/issues/350), to be proposed for the operator's acceptance before it is built. d18's re-mark does not ship before it: until ADR-0030 d2 is amended and the amendment is built, a re-marked workarea lets no fact leave (point h, first item), so the re-port ([#344](https://github.com/e8kor/open-memory-lab/issues/344)) takes #350 first, and the minimal phase waits for it. b, the fallback offered for the minimal phase, was not chosen; decided by the operator, 2026-11-12, at the recommendation, on a question that said the re-mark as accepted has the effect the operator declined on 2026-11-05 |
| 10 | Does the pre-tool hook refuse the code format on a write it anchors (h)? | a. yes, once per call, as for a fenced block; b. no | a: d17's "no code body is stored or sent" stays whole. The cost: an agent in a capturing clone cannot store a snippet about a file there; decided by the operator, 2026-11-12, at the recommendation |
| 11 | Who does the per-person part where the person holds no admin and no bearer reviewer key (h)? | a. the minimal phase serves a person who holds a bearer reviewer key and a sync key an admin issued, on a server they administer or on a team's server whose admin issues both; elsewhere `oml setup repo` and `oml review commit` name the missing key and stop; b. the web console gains the code-parent mark and the commit, and setup takes a sync key file issued there | a: capture is opt-in and d17 already asks consent before a shared server takes code knowledge. The cost: a person on a hosted server with open sign-up cannot set up capture until b is built; decided by the operator, 2026-11-12, at the recommendation |
| 12 | How does the fill reach `oml_write` in Claude Code, where a rewrite with no `permissionDecision` can be denied in auto mode (e)? | a. rewrite with no `permissionDecision`, as point e has it: the verb never prints `allow`, a guardrail's deny or ask drops the fill, and d11's "fails open" holds for the fill's handler only; b. rewrite with `allow` only when the person's configuration already allows `oml_write` without asking, as d21 does for Codex CLI, and otherwise as c; c. never rewrite: deny once with the filled anchor in the reason, for the agent to resend | a: ADR-0037 d8 and d9 stay whole, and outside auto mode the agent sees nothing. The cost: in auto mode a filled write can be denied once, and the agent resends the changed call, which is what c costs every filled write in every mode; how often is unmeasured, and d20's arms record it. b amends ADR-0037 d9 (`no_input_ever_answers_allow`) for this one tool, has the hook read and interpret the harness's permission settings inside its 50 ms, and approves a call whose input the hook changed (the hooks page: "Combine with `"allow"` to auto-approve"). c gives up d22's "nothing new for the agent": every anchored write is two calls. b or c if d20 shows the denial is common; decided by the operator, 2026-11-12, at the recommendation |
| 13 | Do FR-170 to FR-176 become issues of their own now (h)? | a. they stay in #344, which holds their text, until the re-port opens each as an issue; b. seven issues now, labelled `roadmap` and `measure` (FR-174 `feature`), in the milestone of question 1 | a: six of the seven measure a capture that is not built, and the answers above may change what they measure (question 6 for FR-172's Stop budget, questions 5 and 12 for the writes FR-170 counts as unanchored). The cost: seven numbers resolve to one tracking issue until then, where ADR-0066 makes each limitation an issue; decided by the operator, 2026-11-12, at the recommendation |
