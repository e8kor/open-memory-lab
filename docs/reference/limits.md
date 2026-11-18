---
status: Stable
verified: 2026-11-18
answers: "the size, rate, page and capacity bounds OML enforces or has measured, with units, who sets them and what error is returned"
---

# Limits

Every number has a unit and a source. Where no figure exists, the page says so in a sentence and lists the gap under [not measured](#not-measured-yet).

## Enforced bounds

| Limit | Value | Set by | Error | What to do |
|---|---|---|---|---|
| Memory text size | 1 MiB of UTF-8 bytes after NFC normalization | build constant (`submission.max_body_size` shipped default) | `413 request.payload_too_large` | Split the text into several memories |
| List page size | 1 to 200; default 50 | build constant | `request.validation` | Page with the cursor |
| Recall `k` (MCP) | 1 to 100 | tool schema | `request.validation` | Ask for fewer hits |
| Unused OAuth client registrations | 10,000 | build constant | `429 limit.rate`, with `Retry-After` | Retry after the delay |
| Recall feedback labels | 1,000 a UTC day | build constant | `429 limit.rate`, with `Retry-After` | Retry after the delay |
| A key's own request rate | 1 to 1,000,000 requests a UTC minute; none unless set, and never on an admin key | `oml keys issue --requests-per-minute`, `oml keys limit` | `429 limit.rate`, with `Retry-After` | Retry in the next minute, or have an admin raise it |
| A key's expiry | an instant later than now; none unless set, and never on an admin key | `oml keys issue --expires`, `oml keys limit`; a person's own key, the Keys page's date at issuance | `401 auth.invalid`, `details.reason: expired` | An admin moves the date with `oml keys limit`, or issues another key |
| Brief size | 400 tokens | `brief.max_tokens` | none; the brief is cut | Raise the setting |
| Rerank concurrency | 2 at a time; 1,000 ms deadline | `search.rerank.max_concurrent`, `search.rerank.timeout_ms` | none; recall serves the unreranked order | Raise concurrency or the deadline |
| Web session life | 8 hours | build constant | sign in again | none |
| Upload size | 1 GiB | `uploads.max_bytes` | `413 upload.too_large` | Raise the setting |
| Upload part | 8 MiB exactly, the last part the remainder | build constant | `422 upload.part_size` | Send the length the part number takes |
| Upload sessions a key holds open or unbound | 4 | build constant | `409 upload.session_limit` | Complete, abandon or let one expire |
| Upload bytes a tenant holds open or unbound | 4 GiB | `uploads.tenant_max_bytes` | `409 upload.tenant_limit` | Raise the setting, or abandon sessions |
| Upload parts a Helm-installed lead holds | 8 GiB, the objects emptyDir's `sizeLimit`; the kubelet evicts the lead past it | `objects.sizeLimit`, or `objects.persistence` for a claim | the pod is evicted and replaced, and the parts in flight are named `409 upload.incomplete` at completion | Raise the value, or send the named parts again |
| Upload session life | 24 hours after the last part, at most 72 hours after opening; 24 hours after an unbound completion | build constant | `410 upload.expired` | Open another session |
| Workspace exports running at once | 1 a tenant, 2 in the process | build constant | `409 export.busy`, retryable | Retry once the running export ends |
| Workspace export bundle size | `uploads.max_bytes`, 1 GiB by default | `uploads.max_bytes` | `413 export.too_large` | Export a smaller subtree, or raise the setting |
| Workspace imports and dry runs running at once | 1 a tenant, 2 in the process | build constant | `409 import.busy`, retryable | Retry once a running import ends |
| Workspace import bundle size | `uploads.max_bytes`, 1 GiB by default | `uploads.max_bytes` | `413 upload.too_large` | Split the export, or raise the setting |

Settings are on the [settings page](../operations/configuration.md). The request and error shapes are in the [HTTP API](../api/http-api.md) and [error codes](../api/errors.md).

## Availability

- One lead, no follower. Every upgrade or restart stops the API until the new instance is ready. A single-lead deployment's worked availability target is 99.5% over 30 days, an error budget of 3 h 36 min ([capacity and SLOs](../operations/operating.md)).
- Writes are signed one after another on a single chain per tenant, so write throughput has a ceiling that has not been measured.

## Measured capacity

From the maintainers' own runs on one 8-vCPU host with PostgreSQL:

- Recall at 99,721 readable memories, universe scope, default pipeline `bm25-postings`: p95 25.5 ms measured at the database. The run held 165,000 memories in all, of which 99,721 were readable in that scope. Source: `bench/scale/REPORT.md`.
- Reranking: a recall that waits longer than `search.rerank.timeout_ms` for one of the `search.rerank.max_concurrent` slots serves the unreranked order and says why.
- Details and method: [benchmarks](../benchmarks/index.md).

## Not measured yet

- 1,000,000 memories.
- Cold caches and an idle host.
- Review throughput.
- Per-tenant write ceiling under the chain lock.
- Upload storage: part write and completion throughput at `storage.url`, and the disk 4 GiB of unbound uploads per tenant takes across tenants. A completion reads every part back and hashes it, so it takes time in step with the upload's size.
- Workspace export and import (ADR-0084 I13, d15): an export refuses `export.too_large` before it reads a body when the stored bytes of what it would carry pass `uploads.max_bytes`, but that sum leaves out the lines, verdicts and events, so the bundle is measured again once built; an export that passes the estimate builds its whole bundle in memory before it is encrypted, and memory is bounded only by that final check, up to the tar and the age file together. Two exports and two imports run in the process at most; an import holds the verified bundle, every body included, in memory. An export walks the tenant's whole journal once for its versions' submissions and verdicts, so its time grows with the chain, not with the subtree. The per-day export limit of d15 is not built: one export per tenant at a time is the only bound on how often a tenant exports. The busy gates, and the list of imports the upload sweep leaves running, are held in the lead's process: they bound one lead, and two leads serving one database would each admit their own (as ADR-0072 d12 says of its counters).
- Lake writer lag under sustained load ([run the lake writer](../operations/lake.md)).
- The admin queue read's person-reviewer count (`GET /v2/review/queue`, #435): one pass over the tenant's held rows, one over its person's reviewer keys with their grants and denials, then a match per workspace and key, so it grows with workspaces holding rows times those keys.
- The offered skill set (`GET /v1/skills`, and every MCP `prompts/list`, `prompts/get`, `resources/list` and `resources/read`): each call recomputes the whole set at the key's scope, one ancestry read, a walk of the in-progress subtree below it and one read of every skill row on those workspaces, without bodies, and nothing is cached between calls, so its cost grows with the skill rows in scope and is paid again by each prompt or resource read ([ADR-0080](../adr/ADR-0080-a-skill-is-a-reviewed-claim-the-mcp-server-lists-as-a-prompt-and-serves-as-a-resource-loaded-live-and-never-copied.md) d11).

## Not verified against a live client

The Claude Code plugin, installed from the plugin marketplace with its URL and key set in the plugin's options, is in daily use against a single-lead production server. The CI suites run the rest against test servers only. As of 2026-11-06 no run of these has been completed against a live client, so their steps follow the vendor's documentation:

- the `oml setup claude-code` command, which writes the same plugin options;
- the claude.ai and mobile connector flow: the operator verified it up to connecting and listing the three tools (operator, 2026-11-06), with Claude Code and claude.ai connected through a connect code. A remember and recall round trip from claude.ai has not been reported yet, so the steps after the connection follow Anthropic's documentation;
- the agent kit's configuration for Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop, where the kit's README marks what a tool's documentation does not settle.

[Connect Claude Code](../guides/claude-code.md), [claude.ai and the phone](../guides/claude-ai-and-mobile.md) and [other agent tools](../guides/other-harnesses.md) link here.

## Not available in this build

macOS server builds, read followers and cluster mode, key revoke and rotate verbs, attachments and simulations. Install and platform support is on the [install page](../start/install.md).
