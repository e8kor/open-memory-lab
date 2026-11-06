---
status: Stable
verified: 2026-11-06
answers: "the questions users and operators ask most, with a short answer and the page that holds the detail"
---

# FAQ

Short answers, each linked to the page that owns it. For a symptom, see [troubleshoot the client](help/troubleshooting-client.md) or [troubleshoot the server](help/troubleshooting-server.md).

## Install and connect

### Which platforms can I install on?

Linux x86_64 and arm64, and Windows x86_64. There is no macOS build, so build from source on a Mac. See [install](start/install.md).

### The installer says the signature was not checked. Is that a failure?

No. Without `cosign` it checks the SHA-256 against `SHA256SUMS`, which catches a damaged download but not a tampered release. Install cosign and rerun with `--require-signature`.

### Why does `oml` say "command not found" after install?

The script writes one file to `$HOME/.local/bin` and edits no profile. Add the `PATH` line it printed. See [install](start/install.md).

### Why does the plugin show no tools?

Set `api_key` and `url` in the plugin settings, not `OML_API_KEY` in the shell. See [troubleshoot the client](help/troubleshooting-client.md#the-claude-code-plugin-shows-no-tools).

### Can claude.ai or my phone reach a server on my laptop?

No. It needs a public HTTPS address with `oauth.issuer` set to it. See [claude.ai and mobile](guides/claude-ai-and-mobile.md).

## Write, review and recall

### I wrote a memory and my agent cannot see it. Why?

A new memory is held until a different reviewer approves it. Its author can see it with `include_unreviewed: true`. See [remember and recall](use/remember-and-recall.md).

### It was approved but recall finds nothing. Why?

Recall returns memories that hold any of your words, best first. Check that your key has a reader grant on the workarea, or you get `access.not_found`.

### Can I read my own unreviewed work while I develop?

Yes, in an in-progress workarea. Readers see unreviewed memories, labeled as such, until a person commits the workarea. See [workareas](use/workareas.md).

### Why was my write refused with `submission.scan_veto`?

The scan found something it blocks, such as a leaked key. `details.finding` names the class. Remove it and write again. See [errors](api/errors.md).

### Can I approve my own memory?

No. The author, and the author of anything the memory derives from, cannot approve it. A development home with one person is the exception: the approval counts once and is marked `self_signed`.

### How do I review many memories at once?

List them with `oml review list`, then approve or reject the set. Each item gets its own verdict. See [review held memories](guides/reviewing.md).

### What can an agent not do?

An agent can write and read. It can never approve, sign, demote or delete. See [MCP tools](api/mcp-plugin.md).

## Run a server

### Why does `oml quickstart` fail?

Port `8080` may be in use, so pass `--port`. Docker must be installed. See [run your own server](start/run-your-own.md).

### Why does `/readyz` answer 503?

One of `postgres`, `fork_check` or `names_assigned` is not `ok`. See [troubleshoot the server](help/troubleshooting-server.md#readyz-answers-503).

### Why do writers get `503 system.unavailable`?

Writes in one tenant serialize on the journal, so a lead takeover or a failed fork check refuses them for a time. Retry after `Retry-After`. For the limits, see [limits](reference/limits.md).

### Is my data sent anywhere, and can I delete it?

On the public server, its operator can read stored memories, and logs are kept 30 days. OML does not delete memories; rejected and retired ones stay as history. See [commitments](product/commitments.md).
