<!--
id: adr/ADR-0015-cli-administration
tier: engineering
status: accepted design · amended by ADR-0028
answers: why the CLI is the whole administration surface, why a web console is declined, and what that refusal costs
-->

# ADR-0015 — The CLI is the administration surface; a web console is declined

**Status:** accepted design · 2026-10-29 · amended by [ADR-0028](ADR-0028-agent-key-issuance.md)

Everything this record decides is design — no code exists. The verb-by-verb contract is owned by
[cli/reference.md](../cli/reference.md) and the ceremony transcripts by
[cli/sessions.md](../cli/sessions.md); this record decides that the CLI is the whole
administration surface, declines a web console, and gives the reasons.

## Context

OML commits to CLI administration: there is no web UI. That is not a
deferral dressed as a decision — the deployment (ADR-0016) ships no console workload, the API
(ADR-0014) reserves no console routes, and the threat model never has to answer
for a browser origin, a session cookie, a CSP, or a sandboxed frame. The people who administer OML
are operators and tenant admins; the reviewers are mostly external crews over the API (ADR-0012),
with human CLI review as the exception path. For that population, a terminal with `--json` is the
honest fit, and everything a browser would add is attack surface plus a second presentation layer
to keep truthful.

A web console was considered and declined — a product decision whose reasons stand in the
rejected alternatives below. `oml` is one binary talking to the
same HTTP API as every agent (ADR-0014 d1) — on its lab ring, `/v2`
([ADR-0022](ADR-0022-two-ring-api.md)), since every ceremony is administration — with operator, admin or
dreamer keys; it holds no verb the API lacks, and no service stands between it and `oml-server` —
no backend-for-frontend, no admin service holding its own state between a person and the
service.

## Decision

- **d1 — Show-once key issuance.** `oml key issue` prints a key value exactly once and never
  again; the journal records the issuance as issued-without-value (the value itself touches no
  journal, no log, no shell history the tool can avoid). Re-obtaining a key is a new issuance and
  a revocation, both journalled. The bootstrap ceremony (first start mints a show-once root
  operator key, ADR-0010) is the same rule at tenant zero.

- **d2 — Review is a served ceremony, not a query.** `oml review next` serves one held item with
  **all** its attachments and, for a supersession, the version it replaces, under a `serve_id`
  bound server-side to (reviewer key, artifact version, attachment digest set), expiring, consumed
  by its first verdict, and invalidated by any change to the served content. A verdict without a
  valid serve is refused server-side; the journalled verdict records the served digests. The point
  is evidentiary: "the reviewer was served everything they judged, exactly as it was when they
  judged it" is a sentence the journal can prove, not a UI promise. Non-author reopen of a
  rejection is its own journalled verb. (The serve binding itself is ADR-0012's rule; this record
  gives it its human surface.)

- **d3 — Mass operations default to `--dry-run`.** Any verb that can touch many rows — bans,
  `keys revoke --matching`, re-parent, `simulations end` — previews its reach by default and acts
  only under an explicit flag, and single-target verbs with wide consequence (reassign, erase)
  offer the same `--dry-run` preview. The preview-every-mass-act rule binds every CLI verb with
  plural reach.

- **d4 — `oml audit export` produces the signed evidence bundle.** Per-memory decision trail,
  per-tenant chain verification, grants history, the read log scoped to its retention window
  (ADR-0013 d2), reciprocal-approval statistics per key pair
  ([ADR-0012](ADR-0012-admission-and-review.md) d4's collusion-visibility rule), and simulation rows separated by `simulation_id` (ADR-0011). The
  bundle is the artifact the product's "evidence an auditor asks for" sentence rests on; the CLI
  is how it leaves the building.

- **d5 — `oml settings show/validate/apply`.** Configuration is inspected as
  configured-versus-effective, validated before apply, and retired keys are refused **by name**
  rather than ignored — a setting that no longer does anything must say so, not rot silently.

- **d6 — `oml status` and `oml doctor` are the operational read surface.** Queue depth per
  admission step, review availability, outbox frontier lag, dead-letter count
  (ADR-0003, ADR-0017) — doctor's job is to say why the system
  waits, as a chain ending in the act that resolves it, each stalled step named with the act
  that unblocks it. `--json` on every verb makes the CLI
  scriptable and makes its human output a formatting layer, never a second source of truth.

## Consequences

- One administration surface means one authorization story (API keys, ADR-0010), one audit story
  (every admin act is an API call, journalled like any other), and zero browser attack surface.
  The cost is real: no at-a-glance dashboard, no graph picture, no point-and-click review. Humans
  who want visuals pipe `--json` into their own tooling.
- The review ceremony's evidence quality goes up, not down, without a UI: the serve binding (d2)
  is server-enforced, so the CLI cannot show less than everything, and the journal records what
  was shown. A web console would have had to earn the same property against a browser's cache and
  a tab's staleness.
- Human review throughput through a terminal is unmeasured and plausibly lower than it would be
  through a purpose-built review UI; since external crews are the primary review capacity (ADR-0012)
  and human CLI review is the exception path, this is accepted and rides the review-throughput FR
  on the [roadmap](../product/roadmap.md).
- Ceremony transcripts in [cli/sessions.md](../cli/sessions.md) are mockups of a tool that does
  not exist yet — decided, not built, like everything here.
- Anyone needing programmatic administration scripts the same API the CLI uses; the CLI's
  existence proves the API is sufficient, which is the cheapest conformance test the admin surface
  can have.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A TUI dashboard as a v1 deliverable** | A terminal UI re-creates the console's presentation burden (layout, state vocabulary rendered as marks, live refresh) without the browser's reach, and it would sit on exactly the verbs `oml status`/`oml doctor --json` already expose. If demand shows — operators repeatedly building the same watch scripts — it becomes an FR on the [roadmap](../product/roadmap.md) with that demand as its evidence, layered over the same API. |
| **A web console, even a minimal one** | "Minimal" does not stay minimal: a console accretes duties — recall views, graph pictures, dashboards, assistants — because the surface exists and each addition looks small on its own. A web console also buys back everything the decline avoids — origin policy, sessions, CSP, a second presentation of every state to keep truthful. Declined whole: every administration duty — key ceremonies, review, mass-act previews, evidence export, settings, status — has its home in d1–d6 or in the API's own read verbs, and nothing load-bearing needs a browser. |
| **An admin API separate from the agent API** | Two contracts to version and audit, and an invitation for admin verbs to escape the taxonomy and the journal. ADR-0014 d1 already settles it: one API, and the CLI is a client like any other — the verb ceiling lives in roles, not in network topology. (ADR-0022's `/v2` lab ring is not this: same listener, same contract document, same taxonomy — a stability ring, not a second API.) |
| **Driving administration through MCP tools** | Admin verbs on an agent-facing protocol blur the verb ceiling R1 exists to enforce; evidence bundles and chunked uploads are not tool calls (ADR-0014, rejected alternatives); and the admin population gains nothing from MCP framing over plain HTTP plus a purpose-built CLI. |
