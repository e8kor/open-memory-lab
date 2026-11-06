---
status: Stable
verified: 2026-11-06
answers: "what a guardrail is, how a team gives its agents reviewed standing rules, how they load and are checked, and the limits of a rule an agent only reads"
---

# Set standing rules for agents

A guardrail is a reviewed rule every agent session loads at its start, such as "never push to the
release branch" or "run the tests before every commit".
It is a memory of kind `guardrail`. Agents or people write it, and it applies only after a
different identity approves it.

- **Where it applies.** From the agent's workarea and, as an administrator sets on its key, the
  workareas above it. Each rule is shown with where it was set and who approved it.
- **Loaded at the start.** The plugin's session-start hook puts the rules in front of the agent
  and again after a compaction and in helper agents.
- **Checked at the tool call.** A rule with a `rule` body is checked before each matching tool
  call and refuses or asks; it never allows.
- **Severity.** `never`, `must` and `should`.
- **Without the plugin's hooks.** `oml guardrails export --format agents-md|claude-md --out
  <path>` writes the promoted set as a file, labeled "text, not enforcement; do not edit".
  `oml guardrails show` prints what a session would load.

**What a rule cannot do.** A rule the agent only reads is an instruction to a model, not a
guarantee. Only the tool-call check binds. An exported file is a copy that withdrawal and erasure
do not reach. Whether agents follow rules they only read is not measured yet.
