---
status: Stable
verified: 2026-11-06
answers: "the size, rate, page and capacity bounds OML enforces or has measured, with units, who sets them and what error you get"
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
| Brief size | 400 tokens | `brief.max_tokens` | none; the brief is cut | Raise the setting |
| Rerank concurrency | 2 at a time; 1,000 ms deadline | `search.rerank.max_concurrent`, `search.rerank.timeout_ms` | none; recall serves the unreranked order | Raise concurrency or the deadline |
| Web session life | 8 hours | build constant | sign in again | none |

Settings are on the [settings page](../operations/configuration.md). The request and error shapes are in the [HTTP API](../api/http-api.md) and [error codes](../api/errors.md).

## Availability

- One lead, no follower. Every upgrade or restart stops the API until the new instance is ready. The public server's availability target is 99.5% over 30 days, an error budget of 3 h 36 min ([capacity and SLOs](../operations/operating.md)).
- Writes are signed one after another on a single chain per tenant, so write throughput has a ceiling that we have not measured.

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
- Lake writer lag under sustained load ([run the lake writer](../operations/lake.md)).

## Not verified against a live client

The Claude Code plugin, installed from the plugin marketplace with its URL and key set in the plugin's options, is in daily use against memory.doublegate.io. The CI suites run the rest against test servers only. As of 2026-11-06 we have not run these against a live client, so their steps follow the vendor's documentation:

- the `oml setup claude-code` command, which writes the same plugin options for you;
- the claude.ai and mobile connector flow, where the steps that name a claude.ai screen follow Anthropic's documentation;
- the agent kit's configuration for Codex CLI, Cursor, Gemini CLI, VS Code and Claude Desktop, where the kit's README marks what a tool's documentation does not settle.

[Connect Claude Code](../guides/claude-code.md), [claude.ai and your phone](../guides/claude-ai-and-mobile.md) and [other agent tools](../guides/other-harnesses.md) link here.

## Not available in this build

macOS server builds, read followers and cluster mode, key expiry, key revoke and rotate verbs, attachments and simulations. Install and platform support is on the [install page](../start/install.md).
