---
status: Beta
verified: 2026-11-14
answers: "how one person uses OML end to end with a root of their own, Claude Code, claude.ai and a phone: connecting, capturing, recalling their own held work, approving their own agents' work, correcting, withdrawing, and keeping personal knowledge apart from team workareas"
---

# Individual users

One person, Dana, keeps her agents' memory in OML. Claude Code runs on her laptop, and claude.ai and the Claude app on her phone. Everything she and her agents write lives in her own root, `universe.dana`, which no other person's key reads.

The control in this flow is one review per fact, by Dana herself, marked `same_user` so every reader can tell. Nothing waits on a second person while the work stays in her root. The pair, an agent writes and the same person approves, is one row of [producers and reviewers](../concepts/producers-and-reviewers.md), whose reach view draws a user root apart from team workareas.

## 1. Sign in and connect

1. Dana signs in at `<server>/web` with Google or GitHub. Her first sign-in creates `universe.dana` and the console's own keys for her: `dana-web` for the pages and `dana-review` for review ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)).
2. On the **Keys** page she issues a `writer` key named `dana-laptop` and stores it in a file only she reads. On a server she runs herself, an admin issues the same key with `oml keys issue-agent dana-laptop --principal people/dana/laptop --user dana --out ~/.config/oml/keys/laptop` ([get an agent key](../guides/get-an-agent-key.md)).
3. In a terminal: `oml setup claude-code --url <server> --key-file ~/.config/oml/keys/laptop` installs the plugin with that key ([Claude Code](../guides/claude-code.md)).
4. On **Start** she chooses "claude.ai, Desktop or phone". The page issues a connector key and shows its connect code once. She adds the server as a custom connector in claude.ai and enters the code. The phone app, signed in to the same account, has the same three tools ([claude.ai and a phone](../guides/claude-ai-and-mobile.md)).

Each agent has its own key. Each key reads and writes in `universe.dana` and nowhere else.

## 2. Capture

- **Explicit.** In Claude Code, the agent stores a fact with `oml_write`. The answer says the fact is held.
- **From chat.** `oml chat init` opens `universe.dana.chat`, puts it in progress and prints `OML_CHAT_WORKAREA`. With `OML_CHAT_CAPTURE=1` set, the plugin's hooks ask the agent every few turns to write what the conversation established, at most 6 facts a time, each anchored to its turn ([capture chat memory](../use/chat-memory.md)).
- **On the phone.** claude.ai calls the same `oml_write` through the connector.

Every write is scanned in the same call. A leaked key is refused and nothing is stored.

## 3. Recall her own work before review

- The agent that wrote a fact recalls it at once: `oml_read` with `include_unreviewed` serves the key's own held rows, marked `unreviewed`.
- Her other agents see each other's held work only inside an in-progress workarea. `chat` is one already. For a project, Dana chooses "put it in progress" on the **Workareas** page ([workareas](../use/workareas.md#in-progress-workareas)).
- A recall from claude.ai on the phone that names the workarea `chat` reads the same held facts, marked. A recall scoped at her root reads only approved ones: today an in-progress subtree serves its held work only to reads scoped inside it, and its ancestors join that audience once ADR-0080 d16 is built ([#375](https://github.com/e8kor/open-memory-lab/pull/375)).

## 4. Approve her own agents' work

- On the console's **Review** page, `dana-review` lists the held items in her root and records a verdict on each. Her agents' writes appear marked `same_user`.
- From a terminal, with a reviewer key an admin issued to her in her root (`oml keys issue dana-cli-review --role dreamer --held-by human --user dana --grant project:dreamer --grant project:reader`): `oml review list --workarea project --ids-only | oml review approve --ids -`.
- An approved fact carries `approver.same_user: true` wherever it is served, so a reader never mistakes it for another person's review ([audit and lineage](../architecture/audit-and-lineage.md)).

The rule is the constitution's second bounded exception: same-person approval counts only in the development parts of her own root. A `committed` workarea in her root is a production scope, where another person approves (ADR-0001 d5 as amended in [#377](https://github.com/e8kor/open-memory-lab/pull/377)).

## 5. Correct and retire

- The agent finds the fact and writes `oml_write` with `replaces` and the `expected_version` it read. The new version is held; until Dana approves it, every other reader keeps the old one.
- A retirement is the same call with `retirement: true` and the reason in `text`. Every version stays readable as history.

## 6. Withdraw a held draft

Designed, not built ([ADR-0083](https://github.com/e8kor/open-memory-lab/pull/375) d13). The author key of a held version, an agent's included, withdraws it with `oml_withdraw` or `POST /v1/memories/{artifact_id}/withdraw`, and Dana withdraws any held version her own keys wrote from the console's Submitted view. The version is kept as `withdrawn`, leaves every review queue and is never served. No verdict is recorded, so R1 is untouched.

Today, a held version leaves the queue only by a verdict: Dana's review key rejects it with a reason, and the bytes are kept.

## 7. Keep personal knowledge apart from team work

- A key with `--user dana` reaches `universe.dana` and the `universe` node, nothing else. It never reads another person's root or a shared workarea ([workareas](../architecture/workareas.md)).
- Team work happens in a shared workarea, such as `payments.dev`, under tenant keys an admin issues to her ([engineering teams](engineering-teams.md)). Those keys never write into her root: a new memory there from a key that is not hers is refused `submission.user_root_refused`.
- Moving a personal fact into a team workarea is a new write there, reviewed by the team. The console's export, `GET /v2/web/export`, downloads everything in her root as one JSON file.
- Designed: in a shared workarea the console acts with a tenant key an admin links to her sign-in, named on the page for each act (ADR-0085 d16, [#377](https://github.com/e8kor/open-memory-lab/pull/377)).

## What it costs

- One review per fact, by herself. A fact she never approves stays author-only outside in-progress workareas.
- An agent in an in-progress workarea can act on a fact nobody has judged; the mark says so.
- Personal facts never reach the team by themselves. That is the point of the separate root.
