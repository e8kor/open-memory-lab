# ADR-0014 — One versioned JSON HTTP API; MCP streamable HTTP as a stateless facade

**Status:** accepted design · 2026-10-29

Everything this record decides is design — no code exists. The contract itself — routes, payloads,
pagination, the OpenAPI document — is owned by [api/http-api.md](../api/http-api.md); the MCP tool
list with examples by [api/mcp-plugin.md](../api/mcp-plugin.md); the error taxonomy by
[api/errors.md](../api/errors.md). This record decides the shape and gives the reasons.

## Context

OML has three kinds of caller: agents (the clients the product exists for), the admin CLI
(ADR-0015), and probes/automation (Kubernetes, scripts around evidence export).
They need one truth about what the service does, and `oml-server` runs as N stateless replicas
behind a load balancer (ADR-0016) — so whatever protocol the callers speak must survive any
request landing on any replica, and must let retried requests be safe, because agents retry.

The shape follows from the callers and the topology. A multi-tenant service with an inbound
port and bearer keys needs one keyed HTTP listener, and MCP — the integration protocol the agent
harness ecosystem speaks — rides that listener as one stateless `POST /mcp` under the same
bearer rule rather than opening a second wire. Derived views (like "who approved this") are read
models folded from the journal, never new lifecycle state, and every kind of tooling reaches the
service over the same one transport. The remaining question is which protocol is primary.

## Decision

- **d1 — One versioned JSON HTTP API is the contract; everything else is a client of it.**
  JSON over HTTPS with `Authorization: Bearer <api-key>`; tenant, role and scope derive
  server-side from the key, never from the request (ADR-0007, ADR-0010). The MCP facade (d2), the
  CLI (ADR-0015), and any future surface call the same routes under the same
  authorization — there is no verb that exists only on a facade, and no facade that may say more
  than the API says. Versioning is in the path, in two stability rings
  ([ADR-0022](ADR-0022-two-ring-api.md)): `/v1` is the memory-provider foundation under a
  compatibility covenant (additive-only once FR-21 declares it stable), and `/v2` is the evolving
  memory lab — one listener, one OpenAPI document, two change velocities.

- **d2 — MCP is a stateless streamable-HTTP facade over the agent-safe verb subset.** The server
  declares stateless operation: no server-assigned session id, each tool call independent and
  carrying its own bearer key; SSE resumability and server push are out of scope for v1. The tool
  list is exactly the verb ceiling R1 allows an agent: `remember`, `recall`, `learn`, attachment
  upload, relate-propose, own-work status, workarea info, graph and lineage reads (scoped to the
  key's grants, serialized by the server — serialize-don't-compute), an events tail, and the
  vocabulary reads added under this rule by [ADR-0021](ADR-0021-ai-human-ontologies.md) d9. Never
  promote, sign, demote, delete, or any admin verb — not as hidden tools, not behind flags; the
  facade cannot widen what the wire may say.
  The MCP authorization story beyond bearer keys (the spec's OAuth 2.1 profile) is an RS row on
  the [roadmap](../product/roadmap.md), written per the honesty discipline with spec revision and
  fetch date; OML ships bearer until the named trigger fires.

- **d3 — Idempotency is a record, not a hope.** A mutating request may carry an idempotency key.
  The server claims an idempotency **record** — `(key_id, workarea, idem_key)` → request digest +
  state (`in_progress` | `committed`) + stored response — **in its own transaction, before any
  side effect**. A retry with the same key and the same request digest returns the stored
  response (or a typed in-progress answer while the first attempt runs); the same key with a
  *different* request digest returns a typed error, because silently executing a second,
  different request under a reused key is how duplicate memories are born. Claiming the record in
  its own transaction is what makes this safe across stateless replicas: two replicas racing the
  same retry serialize on the record's uniqueness, not on luck.

- **d4 — Errors are typed, fail closed, and name their rule.** Every refusal carries a stable
  machine-readable code and names the rule that refused it — the constitution rule, the status
  ladder rule, the quota — so an agent can branch on the code and a person can read the sentence.
  Quota and rate-limit refusals (429) are entries in the same taxonomy, not a load balancer's
  anonymous reflex. When the service cannot evaluate a rule (review unavailable, policy
  unresolvable), it refuses rather than defaults open. The taxonomy is one page,
  [api/errors.md](../api/errors.md), and every surface quotes it.

- **d5 — `/healthz` and `/readyz` are part of the contract.** Liveness and readiness endpoints
  exist for Kubernetes probes (ADR-0016) and for `oml doctor`; they are unauthenticated, say
  nothing tenant-scoped, and their semantics (what "ready" asserts about Postgres and the signing
  key's fork check, per ADR-0002 d7) are documented with the API, not improvised per deployment.

## Consequences

- There is exactly one contract to document, test, and keep honest: the OpenAPI document in
  [api/openapi.yaml](../api/openapi.yaml) matches [api/http-api.md](../api/http-api.md), and the
  MCP tool list is derivable from (and checked against) the routes it fronts on both rings. Drift between
  facade and API is a test failure, not a support ticket.
- Stateless MCP means no server push in v1: an agent polls the events tail rather than being
  notified. That is a real limitation, stated here; the revisit trigger is named in the rejected
  alternatives below.
- Idempotency records are rows with tenant RLS like everything else (they appear in the data
  model, ADR-0002's domain), so retry safety costs one extra transaction per mutating call. That
  cost is unmeasured — it rides the same Postgres scale FR as the rest of the write path
  ([roadmap](../product/roadmap.md)).
- Agents in any language integrate with either plain HTTP/JSON or any MCP client; neither path is
  second-class, because one is defined as the other's facade.
- Every refusal an agent will ever see is enumerable in advance from one page, which is what makes
  fail-closed tolerable to build against.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **MCP-only (no plain HTTP API)** | Admin tooling, evidence export, upload sessions and probes need plain HTTP: a signed audit bundle download, a resumable chunked upload (ADR-0004), and a Kubernetes readiness probe are not tool calls, and forcing them through MCP framing adds encoding without adding meaning. The CLI (ADR-0015) would also inherit MCP's client stack for no benefit. The facade direction only works one way: HTTP can carry MCP; MCP cannot absorb HTTP's jobs. |
| **gRPC as the primary contract** | Agent-ecosystem reach. The clients that matter speak HTTP/JSON and MCP today; a gRPC-first contract puts a transcoding layer in front of every real caller and buys streaming and schema rigor that d2's stateless subset and the OpenAPI document already cover well enough for v1. Nothing forbids a gRPC facade later under d1's rule — a client of `/v1`, saying no more than it says. |
| **Server-session MCP (server-assigned sessions, SSE resumability, push)** | Contradicts N stateless replicas (ADR-0016): a server-held session pins a caller to a replica or drags in shared session state — a new store with new failure modes — to support features (push, resumable streams) v1's verbs do not need. Revisit trigger, named now: when a real client needs push (review-available notifications to dreamer crews is the likely first case), session state becomes a designed feature behind an RS row, not a default. |
| **Per-request tenant/workarea headers instead of key-derived scope** | A request that names its own tenant is a confused-deputy invitation; ADR-0007's rule — tenant derives from the key, server-side, always — would have to be re-proven for every route instead of holding by construction. |
| **Idempotency by "the operation is naturally idempotent"** | `remember` is not: two identical submissions are two claims unless something says otherwise, and claim-key uniqueness alone cannot distinguish a retry from a genuine resubmission. The record (d3) makes the distinction explicit and auditable. |
