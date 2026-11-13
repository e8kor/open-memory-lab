---
status: Stable
verified: 2026-11-06
answers: "how OML fits together: the clients and what each starts from, the actors, the processes and stores, the code layout, how a write, a review, a read and a freeze move through them, and where each of the four rules is enforced"
---

# How OML is put together

Open Memory Lab (OML) is an external memory provider for AI agents and crews. Every memory is held on arrival, scanned, reviewed by an identity that did not write it, and only then visible to others. This page maps the pieces. Contracts live on the pages that own them, and this page links to them.

## Clients and how they start

Every client starts from one pair: the server's URL without `/mcp`, and a key. It reads everything else from `GET /v2/whoami`.

| Client | Reaches | Its pair comes from |
|---|---|---|
| `oml` CLI | `/v1`, `/v2` | `--url` and `--api-key`, then `OML_URL` and `OML_API_KEY`, then the file `oml login` writes |
| Claude Code plugin | `/mcp` | the plugin's `url` and `api_key` options; `oml setup claude-code` sets them |
| Agent kit (Codex CLI, Cursor, Gemini CLI, VS Code, Claude Desktop) | `/mcp` | `OML_API_KEY`, and `OML_URL` where the tool expands it |
| claude.ai and the Claude apps | `/mcp` | no pair; an OAuth connection bound to one key through a connect code |
| Python and TypeScript SDKs | `/v1`, `/v2` | `OML_URL` and `OML_API_KEY` ([SDKs](../engineering/sdks.md)) |

## Who talks to OML

Three kinds of actor use the same versioned HTTP API ([API](../api/http-api.md)). None runs inside the server. OML runs no model, so review capacity comes from reviewer crews.

```mermaid
flowchart LR
    subgraph outside ["Outside the deployment"]
        AG["Agent crews<br/>(writer / reader keys)"]
        RC["Reviewer crews<br/>(reviewer keys)"]
        AD["Admins and the operator"]
    end
    OML["Open Memory Lab"]
    AG -- "MCP: oml_write, oml_read, oml_workarea" --> OML
    RC -- "HTTP: review next, verdict, ratify" --> OML
    AD -- "oml CLI or web pages:<br/>keys, workareas, search settings" --> OML
```

- **Agent crews** call three MCP tools. None of them can promote, sign, demote or delete. See [MCP plugin](../api/mcp-plugin.md).
- **Reviewer crews** hold reviewer keys (role `dreamer`) and work the review over HTTP. See [roles and keys](roles-and-keys.md) and [admission](admission.md).
- **Admins and the operator** use the `oml` CLI with admin or operator keys. The server also serves a sign-in page, a user page and an admin page for people who sign in with Google or GitHub.

The server derives the tenant from the API key, never from the request.

## Processes and stores

Two server binaries run beside PostgreSQL. Only `oml-server-lead` loads the signing key.

```mermaid
flowchart TB
    CL["Agents / reviewer crews / CLI"]
    S["oml-server-lead<br/>serves the API, signs the journal"]
    LW["oml-lakewriter<br/>no signing key"]
    PG[("PostgreSQL<br/>journal, metadata, outbox,<br/>search indexes")]
    DL[("Delta lake<br/>bronze / silver / gold<br/>derived, never serves a read")]
    CL --> S
    S --> PG
    LW -- "drains the outbox in order" --> PG
    LW -- "writes tables" --> DL
```

| Part | Role |
|---|---|
| `oml-server-lead` | The only API surface. It commits each write in one PostgreSQL transaction. |
| `oml-lakewriter` | Drains the outbox into the Delta lake. It cannot sign journal events. |
| PostgreSQL | The system of record: journal, metadata, outbox and search indexes. |
| Delta lake | A derived copy for analytics. See [storage](storage.md) and [Delta tables](../data/delta-tables.md). |

A compromised lead is a compromised key. The signature attests the deployment, not a unique process. See [security](security.md).

## How the code is laid out

Each binary has one shape. A pure core (`oml-domain`) holds the rules: the review states, the workarea tree, the role limits and the lineage checks. It does no I/O. An application layer (`oml-app`) declares what the core needs from the world. Adapters implement those needs for PostgreSQL, the lake and the clock. The HTTP transport and the MCP facade call the application layer. The `oml` CLI reaches it only over HTTP.

## Four scenarios

### A write

```mermaid
sequenceDiagram
    participant A as Agent (writer key)
    participant S as oml-server-lead
    participant P as PostgreSQL
    A->>S: oml_write (text, idempotency key)
    S->>S: scan (same call, can veto)
    S->>P: one transaction: signed event (held),<br/>metadata row, outbox row
    P-->>S: commit
    S-->>A: acknowledged: held, visible to the author only
```

The acknowledgment means one thing: the event, the metadata and the outbox row committed together. See [admission](admission.md).

### A review

```mermaid
sequenceDiagram
    participant R as Reviewer (reviewer key)
    participant S as oml-server-lead
    participant P as PostgreSQL
    participant LW as oml-lakewriter
    R->>S: review next
    S-->>R: one held memory, under a serve id
    R->>S: verdict under the serve id
    S->>S: check: reviewer is not in the author's lineage
    S->>P: one transaction: approval event, state promoted, outbox row
    S-->>R: verdict recorded
    LW->>P: drain the outbox, write the lake
```

### A read

```mermaid
sequenceDiagram
    participant A as Agent (reader key)
    participant S as oml-server-lead
    participant P as PostgreSQL
    A->>S: oml_read (query, workarea)
    S->>P: search promoted rows within access
    S-->>A: steering first, then ranked hits
```

Reads never touch the lake. See [retrieval](retrieval.md).

### A freeze

```mermaid
sequenceDiagram
    participant P1 as Proposer
    participant P2 as Ratifier (not the proposer)
    participant S as oml-server-lead
    P1->>S: propose committed to validated
    S->>S: refuse unless every child is validated
    P2->>S: ratify
    S->>S: journal the transition
```

The workarea then refuses new writes and approvals. There is no unfreeze. See [workareas](workareas.md).

## The four rules and where they are enforced

| Rule | Enforced at |
|---|---|
| An agent's surface never promotes, signs, demotes or deletes. | The API itself: the MCP tool list omits those verbs, and every HTTP route checks the key's role. See [roles and keys](roles-and-keys.md). |
| The reviewer is never the author. | Approval time, in the server. It checks the reviewer's key and person against the author and the memory's lineage. A workarea status change and a link need a second identity. See [admission](admission.md). |
| The gate changes visibility, not ranking. | The read path. Held memories are excluded from every read, and steering comes first and unranked. See [retrieval](retrieval.md). |
| Nothing is silently deleted. | The journal is append-only. A demotion keeps the bytes. See [audit and lineage](audit-and-lineage.md). |

## What OML does not do

OML returns memories, not answers, and runs no model. Review capacity comes from the reviewers, and the scan is deterministic rules. The Delta lake serves no read. Web pages cover sign-in, a user page and an admin page, with no dashboards. Memory does not cross tenants.
