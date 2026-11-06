---
status: Stable
verified: 2026-11-06
answers: "who OML is for, the problem it answers, what it gives and costs, and what it deliberately does not do"
---

# Positioning

## Who this is for

OML is for teams that run external AI crews, agents built and managed elsewhere on whatever
framework the team chose, and that must answer for what those agents learned. When an agent acts
on a remembered "fact" and the action is wrong, someone is asked who wrote the fact, who checked
it and why it was served as true. If nobody in your organisation is asked that, a plain memory
store is cheaper and faster.

## The problem

A crew's memory is the one part of its output nobody reviews. Code goes through review and a
deployment through a gate, but the conclusions an agent writes down become the input of every
later agent the moment they are written: unreviewed, unattributed beyond a log line, and
removable only by deleting them silently. The store cannot tell a verified finding from a
hallucination or a planted instruction.

## What you get, and what it costs

OML treats every memory as a claim under test. A new memory, text or files, arrives held,
is scanned, and becomes visible to others only when an identity that did not write it approves.
Every write, verdict, removal and key issuance is a signed entry in a tamper-evident record, and
an export produces the per-memory decision trail on demand. Memory is organised in workareas that
mirror your crews.

The cost is a real gate. A memory is not visible to others until a reviewer has acted, and your
organisation supplies that review capacity, normally a reviewing crew with a person as the
exception path. A team that wants writes visible instantly is asking for what OML exists to
refuse. [In-progress workareas](../use/workareas.md) narrow the delay inside a development
workarea, with unreviewed memories marked.

## AI-first, human in the loop

Every role is designed to be held by an agent; people enter through the same roles and the same
pipeline, and the human verdict sits where a mistake is expensive. Every extension is on top of
the memory provider, never instead of it: with all extensions off OML is a full provider (own-held
recall, get and supersede on MCP, the journalled retire-proposal and token-budget recall). The
API makes the layering structural ([ADR-0022](../adr/ADR-0022-two-ring-api.md)): `/v1` is the
provider ring, `/v2` the lab ring. One host, one compose file or one chart runs the same
binaries and journal ([ADR-0023](../adr/ADR-0023-install-profiles-and-cluster-mode.md)).

## What the record is for

A deployment's journal and its export answer an auditor's questions: who wrote this, who
reviewed it, who approved it, under what policy, and who read it within the retention window.
That is a statement about OML's output; no page calls the system compliant or certified
([compliance](compliance.md)). As of late 2026 no memory provider we know of ships structural,
multi-party, evidence-bearing admission control: a gate the system enforces, that requires a
reviewer distinct from the writer, and that leaves a signed decision record.

## Deliberate boundaries

- **No management console beyond two pages.** Administration is the `oml` command; every command
  has machine-readable output. People sign in with Google or GitHub to a user page for their own
  keys and memories and an admin page for the allowlist
  ([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)).
- **No automatic extraction.** OML never watches a conversation and decides what to remember. A
  memory exists because an agent submitted a claim and stands as its author; extraction in the
  provider would put it in the author's seat. Chat memory keeps only facts the agent writes
  ([chat memory](../use/chat-memory.md)).
- **No profiles.** OML does not synthesise user or entity profiles; crews write derived-fact
  claims themselves.
- **No question-answering service.** OML returns memories with their provenance; clients reason
  for themselves.
- **No crew management.** Workareas mirror crews managed elsewhere; OML governs what crews
  remember, never what they do.
