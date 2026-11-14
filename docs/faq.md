---
status: Stable
verified: 2026-11-14
answers: "the questions users and operators ask most, with a short answer and the page that holds the detail"
---

# FAQ

Short answers, each linked to the page that owns it. For a symptom, see [troubleshoot the client](help/troubleshooting-client.md) or [troubleshoot the server](help/troubleshooting-server.md).

## Choosing OML

### How is OML different from other agent memory products?

OML governs who can read a write and when: a write is held until a different identity approves it. It does not try to extract memories on its own or answer questions. See [why a write is held](concepts/why-a-write-is-held.md) and [Evaluate OML](product/evaluate.md).

### Does it use embeddings or a language model?

Not in the default path. The default pipeline, `bm25-postings`, is keyword scoring, and the scan is fixed rules. Two optional pipelines add a cross-encoder reranker or a learned sparse model. See [retrieval](architecture/retrieval.md).

### What does it cost?

The software is Apache-2.0 and can be run on PostgreSQL by anyone; a server someone else runs sets its own terms. See [Is OML a fit?](product/is-oml-for-you.md) and [install](start/install.md).

### How long until a write is readable by others?

Until a reviewer approves it. How long that takes depends on the reviewers, and there is no measured figure. The author reads its own write at once with `include_unreviewed`.

### Can review be turned off?

No. In an [in-progress workarea](use/workareas.md#in-progress-workareas), readers there recall held memories marked unreviewed before review, and nothing leaves it until a person commits it with nothing held; approval still takes a reviewer who did not write the memory. A development workarea where one person's approval of their own memory counts once, marked `self_signed`, is designed ([ADR-0001](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md) d5) and not built. See [why a write is held](concepts/why-a-write-is-held.md).

### Which agents does it work with?

Claude Code through a plugin, claude.ai and the Claude phone apps through a connector, and Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop through the agent kit. See [integrations at a glance](start/index.md#integrations-at-a-glance).

## Install and connect

### Which platforms are supported?

Linux x86_64 and arm64, and Windows x86_64. There is no macOS build, so build from source on a Mac. See [install](start/install.md).

### The installer says the signature was not checked. Is that a failure?

No. Without `cosign` it checks the SHA-256 against `SHA256SUMS`, which catches a damaged download but not a tampered release. Install cosign and rerun with `--require-signature`.

### Why does `oml` say "command not found" after install?

The script writes one file to `$HOME/.local/bin` and edits no profile. Add the `PATH` line it printed. See [install](start/install.md).

### Why does the plugin show no tools?

Set `api_key` and `url` in the plugin settings, not `OML_API_KEY` in the shell. See [troubleshoot the client](help/troubleshooting-client.md#the-claude-code-plugin-shows-no-tools).

### Can claude.ai or a phone reach a server on a laptop?

No. It needs a public HTTPS address with `oauth.issuer` set to it. See [claude.ai and mobile](guides/claude-ai-and-mobile.md).

### Where does the connect code come from?

From the account page, before adding the connector. Open `/web` on the server and sign in, find **Connect Claude on the web or a phone**, type a name, keep **writer** and click **Issue connector key**. Copy the connect code it shows once, then paste it on the `/oauth/authorize` page Claude opens. If it is lost or expired, click **New connect code** on that key's row (connector keys only). See [claude.ai and mobile](guides/claude-ai-and-mobile.md#1-get-a-connect-code-first).

## Write, review and recall

### A memory was written and the agent cannot see it. Why?

A new memory is held until a different reviewer approves it. Its author can see it with `include_unreviewed: true`. See [write and read memories](use/remember-and-recall.md).

### It was approved but recall finds nothing. Why?

Recall returns memories that hold any of the words, best first. Check that the key has a reader grant on the workarea; without one the answer is `access.not_found`.

### Can unreviewed work be read during development?

Yes, in an in-progress workarea. Readers see unreviewed memories, labeled as such, until a person commits the workarea. See [workareas](use/workareas.md).

### Why was a write refused with `submission.scan_veto`?

The scan found something it blocks, such as a leaked key. `details.finding` names the class. Remove it and write again. See [errors](api/errors.md).

### Can an author approve their own memory?

No. The author, and the author of anything the memory derives from, cannot approve it. A `self_signed` approval in a development workarea is designed and not built.

### How are many memories reviewed at once?

List them with `oml review list`, then approve or reject the set. Each item gets its own verdict. See [review held memories](guides/reviewing.md).

### What can an agent not do?

An agent can write and read. Its tools reach no verb that approves, signs, demotes or deletes ([R1](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md)). See [MCP tools](api/mcp-plugin.md).

## Run a server

### Why does `oml quickstart` fail?

Port `8080` may be in use, so pass `--port`. Docker must be installed. See [run a server](start/run-your-own.md).

### Why does `/readyz` answer 503?

One of `postgres`, `fork_check` or `names_assigned` is not `ok`. See [troubleshoot the server](help/troubleshooting-server.md#readyz-answers-503).

### Why do writers get `503 system.unavailable`?

Writes in one tenant serialize on the journal, so a lead takeover or a failed fork check refuses them for a time. Retry after `Retry-After`. For the limits, see [limits](reference/limits.md).

### Is data sent anywhere, and can it be deleted?

On any server, its operator can read stored memories, and a tenant admin reads every memory's text through the journaled [admin read](architecture/roles-and-keys.md#the-admin-read). Request logs are kept 30 days unless the operator changes that. No verb deletes or erases a memory in this build: rejected and retired memories stay as history, and erasure by crypto-shredding is designed and not built. A person downloads their own memories from the web console and can file a deletion request; an admin who closes it disables the person, and the bytes stay until erasure exists. See [compliance](product/compliance.md) and [commitments](product/commitments.md).
