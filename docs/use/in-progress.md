<!--
id: use/in-progress
tier: client
status: current
answers: how a development workarea opts in to serving unreviewed memories inside it, and the rule that nothing leaves until a person commits it with none waiting
-->

# In-progress workareas

Review costs a delay. While a team builds something together, that delay hides each agent's work
from the others. An **in-progress** development workarea trades that delay for a marked risk,
inside the workarea only
([ADR-0030](../adr/ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)).

1. **Opt in.** A person's key (a dreamer held by a human) runs
   `oml workareas mode <workarea> in_progress`. The decision is journalled as
   `workarea.mode_set`; an agent's key cannot make it.
2. **Recall marks what is unreviewed.** Inside the workarea, readers recall other agents'
   memories that still wait for review, each marked `unreviewed`. Nothing from the workarea
   reaches a read outside it.
3. **Commit with none waiting.** `oml workareas mode <workarea> standard` commits it, and is
   refused with `workarea.held_remaining` while anything in it is held. Review first:
   `oml review list --workarea <workarea>`, then `oml review approve --ids -`.
4. **Narrow with a curator.** A curator key profile reads and curates inside the workarea
   without wider rights.

The cost is that an agent can act on a memory nobody has judged, so use it for development
workareas where that is the point. The rule that the reviewer is never the author holds
throughout, and a development home with one person marks that person's own approvals
`self_signed`.
