<!--
id: use/workareas
tier: client
status: current
answers: how memory is organised in workareas, what a person's own workarea and the shared universe are, how a workarea matures from development to validated, and what links and briefs do
-->

# Workareas

A workarea is a node in one tree per organisation, rooted at `universe`. Keys read and write
through grants on nodes; a grant covers the node and its subtree, and a denial on a node always
wins. `universe` holds only what curators place there, each claim reviewed by a second identity;
no user key writes to it directly ([ADR-0044](../adr/ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)).

## Your own workarea

When a person's first key is issued, OML creates a workarea of their own under `universe`. Their
keys write, read and make workareas there, so two people can each have a `project`: under alice's
key, `oml workareas create alice project` creates `universe.alice.project`. A search inside a
person's workarea also searches the shared area, so a curated memory reaches everyone below
without copying. A memory a curator copies into the shared area is shown without its author or
its source. Workareas and keys made before v0.5.0 stay where they are and belong to no person.

## From development to validated

A new workarea is in `development`. It moves to `committed` and then to `validated`, each step
proposed by one identity and confirmed by another:

```sh
oml workareas status propose project --to committed
oml workareas status ratify project <transition-id>
```

A validated workarea stays readable and takes no new writes; the changes still allowed (removing a
memory, updating the crew's recorded details) each leave a record. A person's own root climbs to
`committed` at most. There are no backward transitions.

## Links and the brief

Labelled claim links (`supports`, `contradicts`, `amends`) connect two memories. A **brief** is
the few reviewed memories an agent most needs on entering a workarea: purpose, key decisions,
conventions, where things are. A memory joins the brief when one identity marks it and a second,
neither the first nor the author, confirms it; the session-start hook loads it within
`brief.max_tokens` (400 budget tokens unless set)
([ADR-0046](../adr/ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md)).

The structure and the algebra behind it are on [Workareas, roles and keys](../architecture/workareas.md).
