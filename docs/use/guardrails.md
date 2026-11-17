---
status: Stable
verified: 2026-11-06
answers: "what a guardrail is, how a team gives its agents reviewed standing rules, how they load and are checked, and the limits of a rule an agent only reads"
---

# Set standing rules for agents

A guardrail is a reviewed rule every agent session loads at its start, such as "never push to the
release branch" or "run the tests before every commit".
It is a memory of kind `guardrail`. Agents or people write it, and it applies only after a
person approves it: an approval from an agent's key, or from a person's key with a crew or a
profile, is refused `review.person_required`, and the rule stays held. A service key reviews
nothing at all (`access.verb_ceiling`).

- **Where it applies.** From the agent's workspace and, as an administrator sets on its key, the
  workspaces above it. Each rule is shown with where it was set, when it was approved and whose key approved it; a rule approved before v0.8.0 may show an agent's key. On the web,
  Guardrails offers a workspace choice and shows whether the key reads that workspace alone or its
  readable parents too.
- **Loaded at the start.** The plugin's session-start hook puts the rules in front of the agent
  and again after a compaction and in helper agents.
- **Checked at the tool call.** A rule with a `rule` body is checked before each matching tool
  call and refuses or asks; it never allows.
- **Severity.** `never`, `must` and `should`.
- **Before approval.** A rule waiting for review loads only in the session of the key that wrote
  it and, in a workspace a person put [in progress](workspaces.md#in-progress-workspaces), in every
  session working there or above it. It loads as a line of its own marked `[unreviewed]`, after
  the approved rules, and its check asks where an approved one would refuse, so a rule nobody has
  judged can make the agent ask but never stops it. An approved rule it would replace stays in
  force with its own effect until a reviewer approves the change; a proposed retirement is not
  loaded at all. Steering waiting for review follows the same rule, after the approved steering,
  marked `[unreviewed]`.
- **Without the plugin's hooks.** `oml guardrails export --format agents-md|claude-md --out
  <path>` writes the promoted set as a file, labeled "text, not enforcement; do not edit".
  `oml guardrails show` prints what a session would load.

**What a rule cannot do.** A rule the agent only reads is an instruction to a model, not a
guarantee. Only the tool-call check binds. An exported file is a copy that withdrawal and erasure
do not reach. Whether agents follow rules they only read has not been measured.

**Finding a rule. Designed, not built** ([ADR-0092](../adr/ADR-0092-search-filters-by-every-claim-kind-and-ranks-skills-and-guardrails-as-data-and-a-read-without-a-query-is-the-catalog-of-what-a-key-reads.md), planned for v0.8.0). Today recall
refuses `kinds: ["guardrail"]` and points at the guardrail read. Once built, recall ranks the
guardrails that apply at the agent's workspace beside memories, each hit showing its text,
severity and rule, and the catalog lists every guardrail the key reads with whether it is in
force there. A hit or a catalog entry only shows a rule: what binds is still the set the hooks
load, whatever a query ranks.
