---
status: Stable
verified: 2026-11-06
answers: "the questions users and operators ask most, with the answer and the page that holds the detail"
---

# FAQ

## Install and connect

**Which platforms can I install on?** Linux x86_64 and arm64 (static musl) and Windows x86_64;
Windows on ARM64 runs the x86_64 build under emulation. There is no macOS build after v0.5.2 while
GitHub's macOS runners are unreliable; build from source on a Mac ([install](start/install.md)).

**The installer printed "NOT checked" about the signature. Is that a failure?** No. Without
`cosign` it checks the SHA-256 against `SHA256SUMS`, which catches a damaged download but not a
tampered release. Install cosign and rerun with `--require-signature`
([](https://github.com/e8kor/open-memory-lab/issues/132) tracks checking without it).

**`oml: command not found` after install.** The script writes one file to `$HOME/.local/bin`
(Windows: `%LOCALAPPDATA%\Programs\oml\bin`) and edits no profile. Add the PATH line it printed.

**The plugin shows no tools.** Set `api_key` and `url` in the plugin settings (`/plugin configure
oml@open-memory-lab`), not `OML_API_KEY` in the shell, then reload plugins. You need Claude Code
2.1.285 or later, no `/mcp` suffix on the `url`, and an agent key rather than an admin key
([troubleshooting](help/troubleshooting-client.md)).

**I get `401 auth.invalid` right after setup.** The key is for another server. With an empty `url`
the plugin uses `https://memory.doublegate.io`; set your own server's URL and use a key issued
there.

**`oml login` says "cannot reach the server" behind our proxy or internal CA.** The CLI carries
public roots and does not read the system trust store. Use a public certificate, or a proxy that
terminates with one.

**Can claude.ai or my phone reach a server on my laptop?** No. It needs a public HTTPS origin,
`oauth.issuer` set to it and `/.well-known/oauth-*` passed through. The connect code lives 10
minutes and works once ([claude.ai and mobile](guides/claude-ai-and-mobile.md)).

## Writing, reviewing and recall

**I wrote a memory and my agent cannot see it.** A held memory is visible to its author with
`include_unreviewed: true` or when listing without a query. Everyone else sees it after a
non-author approves it ([your first memory](start/quickstart.md)).

**It was approved but recall finds nothing.** Recall matches memories that contain every word of
the query unless any-word search is on, so try fewer words. The key also needs a reader grant on
that workarea, otherwise the answer is `access.not_found`.

**Can I read my own unreviewed work while I develop?** In a development workarea that opts in,
readers recall memories still waiting for review, marked as such. Nothing leaves until a person
commits it with none waiting ([in-progress workareas](use/workareas.md)).

**Why was my write refused with `submission.scan_veto`?** The deterministic scan found a leaked
key or similar. `details.finding` names the class and the rule, and nothing was stored. Remove it
and write again.

**Why `submission.version_conflict`, or "expected_version required"?** A supersession names the
claim's latest version you read (`expected_version`). Re-read the tip and retry. The SDKs and the
CLI send it; hand-written HTTP calls from before v0.5.x do not.

**Can I approve my own memory?** No. `review.author_in_lineage` refuses the author and the author
of anything it derives from. The one exception is a development home with a single person, where
the approval counts once and is marked `self_signed`.

**How do I review 100 memories at once?** Review a listed set in one act: each item is judged and
signed as its own verdict. Use `oml review list --ids-only | oml review approve --ids -` or the
user page ([reviewing](guides/reviewing.md);).

**Approving says `review.serve_invalid`.** The serve expired, was already used, or its content
changed. Run `oml review next` again.

**`oml sync` refused my run.** The usual refusals are a source that rewound (`--allow-rollback`),
items already present without the sync's anchor (`--adopt`), and the public server
(`--allow-public-server`). Run with `--dry-run --json` first ([sync](use/sync.md)).

**Which agent tools exist, and what can they not do?** `oml_write`, `oml_read` and
`oml_workarea`. An agent may never promote, sign, demote or delete ([MCP tools](api/mcp-plugin.md)).

## Running a server

**`oml quickstart` fails with "port already in use" or "docker missing".** It publishes on
`127.0.0.1:8080` only; pass `--port`. A second run in the same directory is refused as
`quickstart.exists`; without compose the error is `quickstart.docker`. `docker compose
--project-directory oml down -v` resets ([run your own server](start/run-your-own.md)).

**`/readyz` answers 503.** Read which of `postgres`, `fork_check` and `names_assigned` is not `ok`.
The server also refuses to start when the owner and serving database URLs log in as one role
([troubleshooting](help/troubleshooting-client.md#server-readyz-answers-503)).

**Many writers get `503 system.unavailable` "chain head contention".** Writes in one organization
serialize on the chain head. Retry after `Retry-After`; the ceiling is not yet measured
([](https://github.com/e8kor/open-memory-lab/issues/100)).

**Is my data sent anywhere, and can I delete it?** On the public server its operator can read
stored memories, and request logs are kept 30 days. Erasure by crypto-shredding is designed and
not built, so deletion there is a manual operator act. A self-hosted server keeps its data with
you ([](https://github.com/e8kor/open-memory-lab/issues/146)).
