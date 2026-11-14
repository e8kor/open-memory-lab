<!--
id: adr/ADR-0022-two-ring-api
tier: engineering
status: accepted design · amended by ADR-0025
answers: why the one API surface gains two stability rings — /v1 memory provider, /v2 memory lab — and what each ring promises its callers
-->

# ADR-0022 — The two-ring API: v1 memory provider, v2 memory lab

**Status:** accepted design · 2026-10-29 · amended by [ADR-0025](ADR-0025-the-agent-only-memory-surface.md), [ADR-0026](ADR-0026-names-are-keys.md), [ADR-0039](ADR-0039-the-api-rewritten-for-v0-4-0.md), [ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md)

**Release:** introduced in v0.1.0 ([releases](../changelog.md))

Everything this record decides is design — no code exists. It refines
[ADR-0014](ADR-0014-http-api-and-mcp-facade.md) (one transport, one keyed listener, one OpenAPI
document) without superseding it: the one surface gains two **stability rings**, not a second
surface.

## Context

OML must be usable as a plain external memory provider by an agent that knows nothing about
review ceremonies, workspace trees or evidence exports — and it must keep that foundation
compatible while the memory-lab capabilities evolve on top. Those are two different change
velocities living on one API: the provider verbs an agent's loop depends on must never break;
the governance surface grows with every capability this register adds (simulations, datasets,
vocabularies). A single undifferentiated version number forces a choice between freezing the lab
and breaking the loop.

The same pressure applies to request ergonomics. A traditional memory provider scopes a client
by its credential — the client says *remember this*, not *remember this in area X*. OML derives
tenant from the key already ([security](../architecture/security.md)); the foundation should
derive the **target workspace** the same way, so the provider ring has the ergonomics of the
category while the workspace machinery keeps working underneath, by default, for every write.

## Decision

- **d1 — One listener, two rings.** The HTTP surface stays one keyed listener and one OpenAPI
  document; paths split into `/v1` (the **memory provider** — the foundation) and `/v2` (the
  **memory lab** — the steroids). The split is a stability contract, not a trust boundary: the
  constitution's verb ceiling (R1), admission, RLS and every guarantee in
  [commitments](../product/commitments.md) apply identically on both rings.

- **d2 — `/v1` is the provider foundation, under a compatibility covenant.** Its verbs are what
  an agent needs to use OML as its only memory and nothing else: submit
  (`POST /v1/memories`, the reserved `POST /v1/memories:batch` of #122), read own and promoted
  (`GET /v1/memories/{artifact_id}`, the own-work listing), recall, supersede, and the
  attachment upload/fetch flow; `/healthz` and `/readyz` stay unversioned. **The covenant:**
  once #150 declares v1 stable, v1 changes are additive only — no field removed, no semantic
  changed, no status code repurposed; anything that would break a v1 client is, by definition, a
  v2 capability. The gate still applies on v1 — a v1 write is held until review like any write;
  what v1 hides is the machinery, not the constitution.

- **d3 — `/v2` is the memory lab, and it evolves.** Review and ratification ceremonies, claim
  edges, workspace and key administration (grants, denials, bans), claims administration
  (reassign, demote, erase, tip-override), simulations, exports, settings, tenants, bootstrap,
  the events tail, effective-access, vocabularies ([ADR-0021](ADR-0021-ai-human-ontologies.md))
  and dataset onboarding ([ADR-0020](ADR-0020-dataset-onboarding.md)) live on `/v2`. v2 prefers
  additive change but may re-version to `/v3` without ever touching `/v1` — the natural
  evolution the product stance demands: the provider is the trunk, the lab rings around it.

- **d4 — The target workspace comes from the key.** A key may carry a journalled
  `default_workspace` (an issuance attribute, like `used_by`). A `/v1` request names no workspace:
  the target is the key's default; recall's default scope is that workspace's subtree. The
  explicit override is the **`X-OML-Workspace` header** (a node id), honored only within the
  key's grants — plain-passable because it is authorization-checked data, never a secret. A v1
  request from a key with neither a default nor an override is refused with the typed
  `request.workspace_unresolved`. `/v2` keeps explicit workspace ids in paths and bodies — an
  administration surface names what it administers.

- **d5 — The attribute placement rule.** Three carriers, by sensitivity:
  **the key, by reference** — every identity- and authorization-bearing attribute (tenant,
  role, grants, `default_workspace`, `used_by`, simulation audience) is resolved server-side from
  the key's journalled issuance and grant records; nothing sensitive ever crosses the wire in
  cleartext, and the bearer-hash key model of
  [roles-and-keys](../architecture/roles-and-keys.md) is unchanged — "in the key" means
  *derivable from the key*, not *embedded in it*;
  **plain headers** — non-secret per-request modifiers that the server authorization-checks:
  `Idempotency-Key`, `X-OML-Workspace`;
  **bodies** — domain content. An attribute that requires secure transfer is, by this rule, a
  key-resolved issuance attribute — never a header.

- **d6 — The MCP facade fronts both rings; the ring split is invisible to tools.** The ten
  agent-safe tools keep their shapes; `workspace_id` arguments become optional (key default,
  explicit value as override). Which ring a tool rides is an implementation mapping noted per
  tool, not a tool property: the facade's contract remains ADR-0014's — the verb ceiling, not
  the version path, is what bounds an agent.

## Consequences

- The OpenAPI document gains `/v2` servers/paths; provider-ring schemas drop required
  `workspace_id`; `X-OML-Workspace` becomes a shared header parameter;
  `request.workspace_unresolved` (422) joins the taxonomy.
- `api_keys` gains `default_workspace_id`; `key.issued` journals it; the CLI issuance ceremony
  takes `--default-workspace`.
- The CLI is a `/v2` client for every ceremony and a `/v1` client for nothing — administration
  is the lab ([ADR-0015](ADR-0015-cli-administration.md) wording unchanged in substance: one
  API family, no verb the API lacks).
- #150 owns the v1-stable declaration: the covenant binds from the moment that row
  closes; until then v1 is as unstable as everything else in a design with no code.
- The docs name the rings exactly: "the provider ring (`/v1`)" and "the lab ring (`/v2`)";
  "memory provider on steroids" is the product sentence this record makes structural.

## Rejected alternatives

- **Two listeners or a separate lab service.** A second keyed surface doubles the auth path,
  the audit story and the drift risk ADR-0014 exists to prevent — and the commit point must
  stay one transaction (ADR-0002).
- **Workspace in every request body (status quo).** Correct for administration, hostile for the
  provider loop: every competing provider scopes by credential; carrying the scope in each call
  invites cross-area mistakes the key can simply make impossible.
- **Structured self-describing keys (JWT-style embedded claims).** Embedding tenant, role or
  workspace in key cleartext moves authorization data onto the wire and into client custody;
  the bearer-hash + server-side-resolution model keeps the key a pure capability reference.
  Revisit trigger: a federation design (#151) that needs cross-deployment key verification.
- **Versioning the whole API as one ring.** Freezing the lab to protect the loop, or breaking
  the loop to grow the lab.
