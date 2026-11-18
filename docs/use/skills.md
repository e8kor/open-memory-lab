---
status: Stable
verified: 2026-11-18
answers: "what a skill is in OML, how one is written, approved and offered, how a session runs or reads one, and what the plugin asks before a skill file is written on disk"
---

# Keep agent skills in OML

A skill is a reviewed set of instructions an agent runs as a command, such as "cut a release" or
"triage a failing test". It is a memory of kind `skill`: a name, a one-line description and a
markdown body, the shape of a `SKILL.md` without its front matter. Agents and people write one;
every version applies only after a person approves it, and an approval from an agent's key is
refused `review.person_required`.

- **Writing one.** `oml_write` with `kind: "skill"`, a `skill` object `{name, description}` and the
  instructions as markdown text, or `POST /v1/memories` with the same fields. The claim key is
  `skill/<name>`, set by the server. A name is a lowercase label of at most 52 characters with no
  double hyphen; `guardrails` and any name ending in `-unreviewed` are taken. A description is one
  line of at most 1,024 characters with no control, bidi override or zero-width character. A body
  is at most 32,768 bytes and never opens with a `---` line.
- **Changing one.** `oml_write` with `replaces` naming the skill and the `skill` object again: the
  description may change, the name may not. The approved version stays in use until the new one
  is approved.
- **Where it is offered.** In the agent's own workspace and, when the key's guardrail scope is
  `ancestors`, the readable workspaces above it. A skill in a child workspace is not offered at
  its parent, except inside a workspace a person put [in progress](workspaces.md#in-progress-workspaces),
  where sessions above it are offered it too. When two workspaces hold one name, the nearer one is
  offered and its description ends "(overrides <path>)"; the farther one stays readable by its
  address.
- **Before approval.** The key that wrote a skill is offered it at once, marked unreviewed, and so
  is every session inside an in-progress workspace that holds it. An edit waiting for review is a
  second command, `<name>-unreviewed`, beside the approved `<name>`. An unreviewed skill never
  takes the name of an approved one: while any offered workspace holds `<name>` approved, a
  workspace that holds it only unreviewed offers `<name>-unreviewed`. At most 50 unreviewed
  skills are offered at once, and never in place of an approved one.
- **Running and reading.** Claude Code lists each offered skill as `/oml:<name> (MCP)`; running it
  sends a line naming its workspace, version and approval, then the body, then any input. The
  model can read the same text without a person running it, at
  `oml://workspace/<workspace>/skill/<name>`. The session-start hook lists up to 20 offered skills,
  names and descriptions only, each description cut to 160 characters and an unreviewed one
  marked at both ends; it lists fewer when the session's context would pass Claude Code's
  10,000-character limit, and counts the rest in "and N more".
- **Listing them.** `oml skills list` prints what a session would be offered, and
  `GET /v1/skills` serves the same list, 50 a page, without bodies.

**Kept in OML, not on disk.** A skill stored in OML reaches every session in scope once approved,
and an edit or a retirement reaches all of them at once; a file under `.claude/skills/` reaches one
machine. The plugin's tool-call hook therefore asks before a `Write` or an edit under the
project's or the home's `.claude/skills/`, in any letter case, with the reason "OML keeps skills";
the person at the session may allow the call. A shell command that writes there is not matched,
nor a `.claude/skills/` nested deeper in the project or under a directory added with `--add-dir`,
and a guardrail that refuses the call still refuses it. `oml doctor` counts the skills kept on disk.
