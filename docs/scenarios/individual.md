---
status: Beta
verified: 2026-11-14
answers: "how one person uses OML end to end with a root of their own, Claude Code, claude.ai and a phone: connecting, capturing, recalling their own held work, approving their own agents' work, correcting, withdrawing, and keeping personal knowledge apart from team workspaces"
---

# Individual users

One person, Dana, keeps the memory of a few agents in OML: Claude Code on a laptop, and claude.ai and the Claude app on a phone. Everything Dana and those agents write lives in Dana's own root, `universe.dana`, which no other person's key reads.

The control in this flow is one review per fact, by Dana, marked `same_user` so every reader can tell. Nothing waits on a second person while the work stays in the root. The pair, an agent writes and the same person approves, is one row of [producers and reviewers](../concepts/producers-and-reviewers.md), whose reach view draws a user root apart from team workspaces.

A personal root is a development scope for good. It is capped at `development` and never becomes `protected` or `frozen`; work meant for production moves to a shared workspace, where releases are cut; a personal root never publishes a release ([ADR-0085](../adr/ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md) d17, amending [ADR-0046](../adr/ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md) d1). Today the code still lets a user root climb to `protected`; lowering the ceiling is designed, not built.

## 1. Sign in and connect

1. Dana signs in at `<server>/web` with Google or GitHub. The first sign-in creates `universe.dana` and the console's own keys for the person: `dana-web` for the pages and `dana-review` for review ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)).
2. On the **Keys** page Dana issues a `writer` key named `dana-laptop` and stores it in a file only the owner reads. On a self-run server, an admin issues the same key with `oml keys issue-agent dana-laptop --principal people/dana/laptop --user dana --out ~/.config/oml/keys/laptop` ([get an agent key](../guides/get-an-agent-key.md)).
3. In a terminal: `oml setup claude-code --url <server> --key-file ~/.config/oml/keys/laptop` installs the plugin with that key ([Claude Code](../guides/claude-code.md)).
4. On **Start**, Dana chooses "claude.ai, Desktop or phone". The page issues a connector key and shows its connect code once. The server goes into claude.ai as a custom connector, and the code is entered once. The phone app, signed in to the same account, has the same three tools ([claude.ai and a phone](../guides/claude-ai-and-mobile.md)).

Each agent has its own key. Each key reads and writes in `universe.dana` and nowhere else. Designed (ADR-0085 d7): every key scoped in a user root is bound to `tip` by default, so release binding never hides the person's own development work.

## 2. Capture

- **Explicit.** In Claude Code, the agent stores a fact with `oml_write`. The answer says the fact is held.
- **From chat.** `oml chat init` opens `universe.dana.chat`, puts it in progress and prints `OML_CHAT_WORKSPACE`. With `OML_CHAT_CAPTURE=1` set, the plugin's hooks ask the agent every few turns to write what the conversation established, at most 6 facts a time, each anchored to its turn ([capture chat memory](../use/chat-memory.md)).
- **On the phone.** claude.ai calls the same `oml_write` through the connector.

Every write is scanned in the same call. A leaked key is refused and nothing is stored.

## 3. Recall held work before review

- The agent that wrote a fact recalls it at once: `oml_read` with `include_unreviewed` serves the key's own held rows, marked `unreviewed`.
- Dana's other agents see each other's held work only inside an in-progress workspace. `chat` is one already. For a project, Dana chooses "put it in progress" on the **Workspaces** page ([workspaces](../use/workspaces.md#in-progress-workspaces)).
- A recall from claude.ai on the phone that names the workspace `chat` reads the same held facts, marked. A recall scoped at the root reads only approved ones: today an in-progress subtree serves its held work only to reads scoped inside it, and its ancestors join that audience once [ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d16 is built.

## 4. Approve the agents' work

- On the console's **Review** page, `dana-review` lists the held items in the root and records a verdict on each. The agents' writes appear marked `same_user`.
- From a terminal, with a reviewer key an admin issued in the root (`oml keys issue dana-cli-review --role reviewer --used-by person --user dana --grant project:reviewer --grant project:reader`): `oml review list --workspace project --ids-only | oml review approve --ids -`.
- An approved fact carries `approver.same_user: true` wherever it is served, so a reader never mistakes it for another person's review ([audit and lineage](../architecture/audit-and-lineage.md)).

The rule is the constitution's second bounded exception: same-person approval counts only in the development parts of the person's own root, which with the cap is the whole root. Production scopes, where another person approves, are shared workspaces ([ADR-0001](../adr/ADR-0001-the-constitution.md) d5).

## 5. Correct and retire

- The agent finds the fact and writes `oml_write` with `replaces` and the `expected_version` it read. The new version is held; until Dana approves it, every other reader keeps the old one.
- A retirement is the same call with `retirement: true` and the reason in `text`. Every version stays readable as history.

## 6. Withdraw a held draft

Designed, not built ([ADR-0083](../adr/ADR-0083-the-memories-page-is-one-cockpit-with-approved-to-review-and-submitted-views-and-held-rows-never-appear-as-approved.md) d13). The author key of a held version, an agent's included, withdraws it with `oml_withdraw` or `POST /v1/memories/{artifact_id}/withdraw`, and Dana withdraws any held version written by the person's own keys from the console's Submitted view. The version is kept as `withdrawn`, leaves every review queue and is never served. No verdict is recorded, so R1 is untouched.

Today, a held version leaves the queue only by a verdict: Dana's review key rejects it with a reason, and the bytes are kept.

## 7. Keep personal knowledge apart from team work

- A key with `--user dana` reaches `universe.dana` and the `universe` node, nothing else. It never reads another person's root or a shared workspace ([workspaces](../architecture/workspaces.md)).
- Team work happens in a shared workspace, such as `payments.dev`, under tenant keys an admin issues to Dana ([engineering teams](engineering-teams.md)). Those keys never write into the root: a new memory there from a key that is not the person's is refused `submission.user_root_refused`.
- Moving a personal fact into a team workspace is a new write there, reviewed by the team. The console's export, `GET /v2/web/export`, downloads everything in the root as one JSON file.
- Designed ([ADR-0084](../adr/ADR-0084-a-workspace-moves-between-servers-as-a-signed-bundle-that-lands-held-or-in-progress-and-is-reviewed-again-unless-its-source-server-is-pinned.md) d2, d14): a personal workspace moves as a whole. On the same server, `oml workspaces import --from dana.project --into payments.dev` streams its approved versions from its history into an in-progress team workspace, where they land held and are reviewed again by the team. To another server, `oml workspaces export dana.project --recipient age1… --out dana-project.oml` writes a bundle in `history` mode, the default in a user root; release mode and `--since` are refused there, since a personal root never has a release ([import and export](import-export.md)).
- Designed: in a shared workspace the console acts with a tenant key an admin links to Dana's sign-in, named on the page for each act (ADR-0085 d16).

## What it costs

- One review per fact, by Dana. A fact never approved stays author-only outside in-progress workspaces.
- An agent in an in-progress workspace can act on a fact nobody has judged; the mark says so.
- Personal facts never reach the team by themselves, and the root never becomes a production scope. That is the point of the separate root.
