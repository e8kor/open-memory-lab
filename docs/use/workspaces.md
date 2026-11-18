---
status: Stable
verified: 2026-11-14
answers: "how memory is organized in workspaces, what a person's own workspace and the shared universe are, how a workspace matures from development to frozen, how an in-progress workspace serves unreviewed memories inside it, and what links and briefs do"
---

# Organize memory in workspaces

A workspace is a node in a tree, not an account or environment; the tenant is the boundary. In Databricks and Microsoft Fabric a workspace is the environment boundary itself; in OML, workspaces nest under the tenant's root, `universe`.

A workspace is a node in one tree per organization, rooted at `universe`. Keys read and write
through grants on nodes. A grant covers the node and its subtree, and a denial on a node always
wins. `universe` holds only what curators place there, each memory reviewed by a second identity.
No user key writes to it directly.

## The user's own workspace

When a person's first key is issued, OML creates a workspace of their own under `universe`. Their
keys write, read and make workspaces there, so two people can each have a `project`. Under alice's
key, `oml workspaces create alice project` creates `universe.alice.project`.

A search inside a person's workspace also searches the shared area, so a curated memory reaches
everyone below without copying. A memory a curator copies into the shared area is shown without its
author or its source. Workspaces and keys made before user roots existed belong to no person.

## Move a workspace from development to frozen

A new workspace is in `development`. It moves to `protected` and then to `frozen`. Each step
needs two identities: one proposes it, and a different one ratifies it.

```sh
oml workspaces status propose project --to protected
oml workspaces status ratify project <transition-id>
```

A frozen workspace stays readable and takes no new writes. The changes still allowed (removing a
memory, updating the crew's recorded details) each leave a record. A person's own root and every
workspace under it stay `development`: a proposal to move any of them is refused
`workspace.transition_refused` with reason `user_root`, so production work happens in shared workspaces
([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d17). There are no backward transitions.

## In-progress workspaces

Review costs a delay. While a team builds something together, that delay hides each agent's work
from the others. A workspace whose drafts are visible to everyone working here, an **in-progress**
workspace, trades the delay for a marked risk, inside the workspace only.

1. **Make drafts visible to everyone working here.** A person's reviewer key (role `reviewer`, used by a person) runs
   `oml workspaces mode <workspace> in_progress`. The decision is journaled as
   `workspace.mode_set`. An agent's key cannot make it.
2. **Read with the label.** Inside the workspace, and from every workspace above it, readers
   recall and list other agents' claims that still wait for review, of every kind, each marked
   `unreviewed`. A read at a sibling workspace or on another branch gets nothing from it until the
   commit.
3. **Make drafts visible to the writing key again, with none waiting.** `oml workspaces mode
   <workspace> standard` does it. It
   is refused with `workspace.held_remaining` while anything in it is held. Review first with
   `oml review list --workspace <workspace>`, then `oml review approve --ids -`.
4. **Narrow with a curator profile.** A key issued with `--profile curator` can reject, propose a retirement or a summary and flag a contradiction inside the in-progress workspace. It never approves.

The cost: an agent can act on a memory nobody has judged, and so can an agent working in a parent
workspace. Use this mode for development workspaces,
where that trade is the point. The reviewer is still never the author. Self-signing was retired on 2026-11-14
([ADR-0001](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md) d5). In a person's own root, their review key approves their
agents' work, marked `same_user`.

## Import a bundle

A bundle exported from another server lands in a workspace in progress, and every version in it arrives held, written by the person who imports it ([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d6). A second person reviews each one before anyone else is served it.

```sh
oml workspaces import payments.oml-bundle --into payments-dev \
  --identity ~/.config/age/key.txt --expect-key <source key fingerprint> --dry-run
```

The CLI decrypts the bundle on the importing machine, checks it, and uploads it; nothing decrypted is written to disk. `--dry-run` prints the plan: which claim keys are new, changed, unchanged or in conflict, and the upload it sent, which `--upload <id>` then imports without sending the bundle again. Without it the import starts on the server and the CLI reads its state every 2 seconds until it is done or failed, then prints its counts and the versions the target refused; `GET /v2/imports/{id}` reads the same. A failed run abandons the upload it opened. A tenant runs one import at a time; a second is refused `import.busy` until the first ends. The first import from a source needs `--expect-key`, the source key's fingerprint obtained out of band, and the target pins it: a later bundle signed by another key is refused. Importing the same bundle again writes nothing, and an older one is refused unless `--allow-older`. A name that does not exist yet is created in progress. The key is a writer key used by a person; an agent's key never imports.

## Links and the brief

Labeled links (`supports`, `contradicts`, `amends`) connect two memories. A **brief** is the few
reviewed memories an agent most needs on entering a workspace: purpose, key decisions, conventions
and where things are. A memory joins the brief when one identity marks it and a second identity
ratifies it. The second identity is neither the first nor the author. The session-start hook loads
the brief within `brief.max_tokens` (400 budget tokens unless set).

How the tree grants access is explained in [Workspaces, roles and keys](../architecture/workspaces.md).
