---
status: Beta
verified: 2026-11-14
answers: "the experience OML aims for, frictionless in development and controlled toward production; the three audiences it serves; and which scenario page walks each one through a full flow"
---

# Scenarios

OML aims for one experience: **frictionless in development, controlled toward production.** While work is being built, the people and agents building it see it at once, marked unreviewed. Once it is published toward production, a different person always decides what agents receive. An agent never promotes, signs, demotes or deletes, anywhere.

Each page below walks one audience through one flow, step by step, with the commands, MCP tools and console pages that exist. A step that is designed and not built says so and names its decision record.

| Audience | Flow | Page |
|---|---|---|
| One person, with Claude Code, claude.ai and a phone | capture, recall, approve in their own root, withdraw, keep personal knowledge apart | [Individual users](individual.md) |
| An engineering team and its agent crew | a shared development workarea, skills and guardrails as claims, tests, a release, production, admin fixes, rollback | [Engineering teams](engineering-teams.md) |
| A business-analyst team | documents and vocabulary prepared on staging, reviewed, released, exported and imported to production | [Business-analyst teams](analyst-teams.md) |
| Any team moving work between servers | what a bundle carries, how trust is rebuilt, rollback | [Import and export](import-export.md) |

## Where the controls sit

| Stage | Who decides | What it costs the person |
|---|---|---|
| Write | nobody: every write lands held | nothing; the author reads its own held work at once |
| Develop, in progress | nobody yet; the team reads held work, marked | one switch, `oml workareas mode <w> in_progress`, by a person |
| Approve in a person's own root | that person's review key, marked `same_user` | one click on the console's Review page |
| Approve in a shared workarea | a key outside the version's lineage | a second identity's time |
| Publish a release (designed, ADR-0085) | one person cuts, another publishes | a second person's time per release, not per item |
| Fix production at once (designed, ADR-0089) | an admin, confirmed afterwards by another person | nothing waits; the fix is marked until confirmed |

[Producers and reviewers](../concepts/producers-and-reviewers.md) lists every producer and reviewer pair by class and scope, with Euler views of what one key sees. The [workarea lifecycle](../architecture/lifecycle.md) draws these stages and where loose turns strict. The [architecture views](../architecture/views.md) draw what runs today.
