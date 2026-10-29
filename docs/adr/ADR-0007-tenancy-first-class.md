# ADR-0007 — Tenancy is first-class

**Status:** accepted design · 2026-10-29

## Context

A single-operator deployment can treat the machine as its privacy boundary:
content stays where it was written, and every key holder is someone the operator
admitted. OML has no such boundary to lean on. It is a shared service: one
deployment serves many tenants, each an organization whose crews hold valid API
keys on the same Postgres cluster, the same object store, the same `oml-server`
replicas. The
attacker the design is written against is therefore not an outsider but a
**hostile co-tenant** — a party with working credentials who wants to read,
corrupt, impersonate or starve a neighbour. Tenancy cannot be a column added
later or a convention the application code promises to respect; it has to be
first-class, enforced below the application, and designed into every privileged
service. This record makes that commitment; the full control set, custody map
and the STRIDE analysis written against hostile co-tenants are declared in
[security](../architecture/security.md). Decided, not built.

## Decision

- **d1 — Tenant is resolved server-side from the API key, never from the
  request.** No header, path segment, parameter or MCP tool argument names a
  tenant; `oml-server` resolves the bearer key to its `api_keys` row and takes
  `tenant_id` from there. A request has no vocabulary in which to claim someone
  else's tenant, which removes the confused-deputy class where a valid key is
  combined with a forged tenant identifier.

- **d2 — Row-level security keyed by `tenant_id` on every Postgres table.** No
  table is exempted for convenience, and no role used by any OML workload holds
  `BYPASSRLS`. RLS is the second fence behind application scoping: a `WHERE`
  clause bug stops at the policy, which Postgres evaluates, not our code. The
  cost is carried openly — RLS predicates ride every query plan, and their
  performance under per-tenant partitioned indexes is unmeasured
  ([FR-4](../product/roadmap.md) and [FR-7](../product/roadmap.md) carry the
  measurements).

- **d3 — Privileged services are designed, not waved through.** Three workloads
  touch many tenants' data by nature, and each gets the narrowest grant that
  does its one job ([security](../architecture/security.md) owns the full map):
  the **lake-writer** iterates per tenant under a `SET`-tenant, RLS-scoped
  session role (no `BYPASSRLS`), and its KMS grant is
  generate-data-key/encrypt only — never decrypt, so its compromise cannot read
  a single body, its own writes included; the **reconciler** runs read-only with
  no KMS grant at all (it compares ciphertext digests keylessly) and emits every
  repair through the outbox via `oml-server` as a journalled act; the
  **indexer** runs under its own per-tenant credentials, never the signing key,
  with its decrypt-grant shape bound to the embedding decision
  ([RS-1](../product/roadmap.md)). None of the three loads the signing key.

- **d4 — Tenant isolation extends through every storage layer.** Per-tenant
  journal chains (declared in [storage](../architecture/storage.md)), per-tenant
  lake table prefixes, per-tenant storage credentials when the object store is
  externalized, per-tenant object paths for quarantine and bodies, and
  list-partitioned embeddings with per-partition indexes so a nearest-neighbour
  query physically cannot return another tenant's row
  ([retrieval](../architecture/retrieval.md)). What stays shared is the
  deployment itself — replicas, signing key, KMS, lake-writer — which is exactly
  the surface d3 constrains.

- **d5 — Quotas and rate limits are journalled grant attributes, enforced in v1
  as a stated approximation.** Each tenant's limits are attributes of its
  journalled grants, so the limit in force is reproducible from the journal like
  any other authority question. v1 enforcement is **per-replica token buckets at
  limit/N**: with N replicas and uneven balancing, a tenant can transiently
  exceed, or be unfairly denied, its global limit, and the error grows with
  replica churn. That approximation is documented wherever quotas are, not
  hidden; exceeding a limit returns a typed 429 naming its rule in the
  [error taxonomy](../api/errors.md). Exact distributed limiting is
  [FR-8](../product/roadmap.md).

- **d6 — The threat model names the hostile co-tenant first.** The STRIDE
  analysis in [security](../architecture/security.md) is written against the
  hostile co-tenant, and every control in this record is judged against that
  attacker before any other.

## Consequences

- Every table, every query plan and every privileged service pays a tenancy
  tax: RLS evaluation, per-tenant session switching in the lake-writer,
  per-tenant credentials to provision and rotate. The design accepts the tax as
  the price of selling a shared service whose isolation claim survives an
  application bug.
- A tenant's evidence export verifies its own chain without touching anyone
  else's, and one tenant's write rate never serializes against another's chain
  lock — isolation buys audit locality and removes a cross-tenant denial
  channel at the same stroke ([storage](../architecture/storage.md)).
- The per-replica quota approximation (d5) is a known, bounded dishonesty in
  enforcement, stated so an operator sizing limits knows the error term;
  [FR-8](../product/roadmap.md) is its exit.
- Shared CPU and IO contention on replicas remains even with chains, RLS and
  buckets in place — the residual is named in
  [security](../architecture/security.md) rather than claimed away.

## Rejected alternatives

- **Separate deployment per tenant as the only isolation.** One Helm release
  per customer makes isolation trivial and kills the shared-service product:
  no shared review capacity, no shared operations, a fleet to upgrade instead
  of a service, and the per-tenant cost floor of a full Postgres + lake stack.
  A dedicated deployment remains available to any tenant who wants it — the
  chart deploys per-tenant fine ([deployment](../operations/deployment.md)) —
  but it is a deployment choice, not the isolation story. The isolation story
  must hold inside one deployment, or the product is a hosting arrangement.
- **Trust-the-client tenant headers.** Letting the request name its tenant
  (header, path prefix, tool argument) and validating it against the key is the
  conventional shortcut, and it manufactures the exact bug class d1 removes:
  every endpoint becomes a place where validation can be forgotten, and every
  proxy a place where the header can be injected. Deriving tenant from the key
  costs one join on a row the server already reads for authentication.
