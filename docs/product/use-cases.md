<!--
id: product/use-cases
tier: client
status: design draft for review · decided, not built
answers: who uses Open Memory Lab and for what; each use case's goal, how it plays out, what its user sees and whether it is available now, in the next release, designed or planned; grouped by journey, with one walkthrough of a day of an agent and a reviewer
-->

# Use cases

**Status:** design draft for review · decided, not built

This page describes what each kind of user does with Open Memory Lab (OML), one use case at a
time: the goal, how it plays out, what the user sees, and whether it works today. It explains
what happens, not how; each use case links to the page that explains how. The
[use-case call map](../engineering/use-case-call-map.md) is the engineering companion: it lists
the calls each use case needs.

## How to read the status

Every use case has one of four statuses. A status describes the software, never an intention.

| Status | Means |
|---|---|
| **Available now** | Works in the current release, 0.3.0. |
| **In the next release** | Built, and waiting for the release being prepared, 0.3.1. Not released yet. |
| **Designed** | Decided, not built. How it works is written down; some of these designs are still under review, and none is in a release. A few designed use cases have a working version that is not in any release; they say so. |
| **Planned** | Decided, not yet designed. The project has decided to do it, and how it works is still open. |

Nothing on this page has been measured. Where a use case depends on how fast or how well
something works, such as how good search results are, the [roadmap](roadmap.md) holds the
open question.

## Who uses OML

| Who | What they do with OML | Where they appear |
|---|---|---|
| **An agent in Claude Code**, and the person working with it | Remembers, searches and corrects shared memory through the OML plugin, and starts every session already knowing what its team knows. | [Everyday memory](#everyday-memory), [Sessions and agents](#sessions-and-agents), [Files and deliverables](#files-and-deliverables) |
| **An agent in another tool** | The same memory from any other tool that speaks MCP, the standard way AI assistants connect to outside tools; later from claude.ai and the Claude apps. | [Sessions and agents](#sessions-and-agents) |
| **A reviewer, person or agent** | Looks at each new memory someone else wrote and approves or rejects it. Reviewers work from the command line or over HTTP, never through the agent's own tools. | [Review](#review) |
| **An operator or admin** | Sets up the server, gives out keys, organizes the workareas and keeps the system healthy, from the command-line tool. | [Getting started](#getting-started), [Teams and access](#teams-and-access), [Operations](#operations) |
| **A vocabulary crew** | Keeps a shared glossary: proposes terms, tags memories with them and reviews both. | [Shared vocabularies](#shared-vocabularies) |
| **A developer** | Uses OML from an application through the Python or TypeScript library, or over plain HTTP. | [Getting started](#getting-started), [Everyday memory](#everyday-memory), [Review](#review) |
| **An auditor, compliance reader or analyst** | Asks who wrote, approved and read a memory, checks that the history was not changed afterwards, and analyzes memory in bulk. | [Audit](#audit), [Time](#time) |
| **A platform team** | Runs OML on Kubernetes or on its own hosts, upgrades it and keeps its files and backups. | [Operations](#operations), [Files and deliverables](#files-and-deliverables) |

## A day with OML

One agent and one reviewer, a working day, using only what is available now.

**Morning.** A developer opens Claude Code in the `procurement-reports` project. Before the
first prompt, OML has already added three things to the session: the team's standing
instructions for its workarea ("quote every price in euros, with its source"), the agent's own
ten most recent memories with where each stands, and the memories that match the project's
name. The developer asks for this week's metals price summary. The agent searches memory first
and finds an approved note from last week: the copper supplier changed its invoice format.

**Mid-morning.** While working, the agent finds that one supplier now quotes in dollars. It
saves that as a new memory. OML answers that the memory is waiting for review. The agent can
already find it when it asks a search to include its own waiting memories, marked as
unreviewed; no other agent sees it yet. It also
records a conclusion, "metals prices rose this week", and names the three memories it drew
it from.

**Noon.** A reviewer, here a person on the procurement team, runs the review command. OML hands
them the next waiting memory written by someone else: the dollar-quote finding. They approve
it. The next item is the price conclusion; one of the memories behind it is a week old, so they
reject it with a reason: "use this week's figures". The reviewer is never shown their own
writes.

**Afternoon.** The agent checks its own memories and sees one approved and one rejected. OML
does not show the agent the reviewer's reason, so the reviewer tells the developer, who asks the
agent for a corrected conclusion from this week's figures. The developer also notices
that last week's note about the copper supplier is now wrong and asks the agent to correct it;
the agent writes a new version, and until a reviewer approves it, everyone keeps seeing the old
one.

**End of day.** The reviewer approves the corrected conclusion and the new version of the
copper note. Every agent with access to the workarea now finds them; the old version of the
note no longer comes back in searches. Every step is in OML's tamper-evident history: who wrote
each memory, who approved or rejected it, and when.

**The next morning,** a second agent on the same team searches for "supplier currency" before
its first task and finds the approved dollar-quote finding, which it had never seen.

What the same day will add as designed use cases are built: the price chart attached to the
conclusion as a file, the team's rules loaded and checked before each command, and an
in-progress workarea where teammates see each other's unreviewed work at once.

## Getting started

### Try OML on a laptop

**Available now** · an operator

- **Goal:** see the whole loop working in minutes before deciding anything.
- **How it plays out:** the operator starts the server and its database with one command and
  runs the quickstart script. It creates a workarea, an agent key and a reviewer key, writes a
  first memory and stops at the review.
- **What they see:** a memory waiting for review; one review command later, the same memory
  found by a search.
- **How it works:** the [README's quickstart](https://github.com/e8kor/open-memory-lab#quickstart).

### Claim the first admin key

**Available now** · an operator

- **Goal:** take control of a new deployment, once.
- **How it plays out:** a new server prints a one-time enrollment code at its first start. The
  operator exchanges it for the first admin key. The code works once; a second attempt is
  refused.
- **What they see:** the key, shown once and stored in a private credentials file.
- **How it works:** [CLI reference](../cli/reference.md#oml-bootstrap-claiming-the-root-operator-key),
  [deployment](../operations/deployment.md#claiming-the-first-admin-key-the-bootstrap-race).

### Give an agent its key without showing it

**Available now** · an operator

- **Goal:** give an agent the one key it needs to save and search in its workarea, without the
  key ever appearing in a chat transcript.
- **How it plays out:** the operator issues the agent's key in one command, naming the agent and
  its workarea. From inside Claude Code, the admin plugin's skill runs the same command and
  writes the key to a private file instead of the screen.
- **What they see:** two lines to paste into a shell; the key itself goes only to the file.
- **How it works:** [CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation),
  [the oml-admin plugin](../cli/reference.md#the-oml-admin-plugin).

### Install the plugin in Claude Code

**Available now** · an agent in Claude Code

- **Goal:** connect Claude Code to OML.
- **How it plays out:** the user sets the server address and the agent's key in the environment,
  adds the project's plugin marketplace and installs the plugin.
- **What they see:** the memory tools in Claude Code, and the session context at the next start.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#wiring-it-into-your-agent).

### Enter the server and key once, kept in the keychain

**Designed** · an agent in Claude Code

- **Goal:** set up the plugin without editing a shell profile.
- **How it plays out:** Claude Code asks for the server address and the key when the plugin is
  enabled, and keeps the key in the system keychain. An operator who issued the key to a private
  file can hand that file to Claude Code from a shell instead, without the key appearing on
  screen. The plugin's tools then use only what Claude Code stored; the start-of-session context
  still falls back to the environment.
- **What they see:** one prompt at install, and no key in any settings file.

### Install the command-line tool from a verified download

**Available now** on 64-bit Intel and AMD Linux · macOS, Windows and ARM Linux **in the next
release** · anyone who runs the `oml` command

- **Goal:** install the `oml` tool from a signed release archive.
- **How it plays out:** download the archive for the platform, check its signature, extract it
  and put `oml` on the path. Today's one archive holds the tool beside the server; the next
  release gives the tool an archive of its own per platform. On Windows the session hook needs
  Git Bash.
- **What they see:** a signed archive on the releases page.
- **How it works:** [CLI reference](../cli/reference.md#installing-oml-and-what-differs-on-windows).

### Call OML from any language

**Available now** · a developer

- **Goal:** use OML without either library, from any language.
- **How it plays out:** the developer reads the published API description and calls the
  calls the server answers. The description also lists calls that are designed and not built,
  so a call can still answer "not found"; separating the two is planned.
- **What they see:** plain JSON over HTTP, with typed errors.
- **How it works:** [HTTP API](../api/http-api.md), [OpenAPI document](../api/openapi.yaml).

### One obvious call for each task

**Designed** · every developer and agent

- **Goal:** a smaller client surface where each task has one obvious call, its common case
  needs the fewest inputs, and the server works out what it can.
- **How it plays out:** the agent's tools, the libraries and the command line are simplified
  together in a coming release: the agent gets one tool to save, one to read and one to see its
  workarea. Existing users migrate once, with each change listed, and the old tool names keep
  working for one release.
- **What they see:** fewer tools to learn, and no invented labels to supply.
- **How it works:** [use-case call map](../engineering/use-case-call-map.md), the input to that
  design.

## Everyday memory

### Save something worth keeping

**Available now** · an agent, a developer

- **Goal:** keep a fact, decision or finding so that later sessions and other agents can use it,
  once someone else approves it.
- **How it plays out:** the agent saves the memory with a short label that groups its future
  versions. OML scans it and holds it for review.
- **What they see:** "waiting for review". Nobody else sees the memory until a reviewer who did
  not write it approves it.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_remember),
  [concepts](../concepts.md#memory-as-claim).

### Record a conclusion and what it came from

**Available now** · an agent

- **Goal:** save a conclusion drawn from memories it already has, naming those memories.
- **How it plays out:** the agent saves the conclusion and lists its sources. The reviewer sees
  the sources beside it.
- **What they see:** the same "waiting for review", and later a conclusion anyone can trace back.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_learn).

### Search before acting

**Available now** · an agent, a developer

- **Goal:** find what the team already knows before starting work, within a size limit.
- **How it plays out:** the agent searches with a few exact words. The workarea's standing
  instructions come first and are never cut; the matching memories follow, as many as fit the
  limit. Search matches words, not meaning: search by meaning is on the [roadmap](roadmap.md).
- **What they see:** standing instructions, then approved memories, best match first, and how
  much the limit cut. Approval never changes the order.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_recall),
  [retrieval](../architecture/retrieval.md).

### See its own work while it waits

**Available now** · an agent

- **Goal:** see its own memories that still wait for review, and find them in searches.
- **How it plays out:** the agent lists its own memories, newest first, or asks a search to
  include its own unreviewed ones.
- **What they see:** each of its memories with where it stands: waiting, approved, rejected,
  withdrawn or erased. Its unreviewed memories are marked; no other key sees them.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_my_work).

### Read one memory back

**Available now** · an agent

- **Goal:** open one memory it has a reference to.
- **What they see:** the memory, what sort of memory it is, its workarea, its sources and where it
  stands.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_get).

### Correct a memory

**Available now** · an agent

- **Goal:** replace a memory with a corrected version.
- **How it plays out:** the agent writes the new version. It is reviewed like any memory, and
  until it is approved everyone keeps seeing the old one.
- **What they see:** the new version waiting for review, then the new version in place of the
  old.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_supersede).

### Propose retiring a memory

**Available now** · an agent

- **Goal:** take a memory that is no longer true out of use, without deleting anything.
- **How it plays out:** the agent proposes the retirement with a reason. A reviewer approves or
  rejects it; until then the memory stays in use. Approved, it stops coming back in searches,
  and its history stays.
- **What they see:** the proposal waiting for review, then the memory gone from searches.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_supersede). The command-line tool has
  no retirement option yet.

### Learn when a memory was approved or rejected

**Available now** · an agent

- **Goal:** find out what happened to its waiting memories.
- **How it plays out:** the agent lists its own memories, or follows the stream of recorded
  events it may see.
- **What they see:** each memory's new state. The reviewer's reason for a rejection is not shown
  to the agent.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_events_tail).

### Check where it works and what it may do

**Available now** · an agent

- **Goal:** know its workarea and what its key may do there.
- **What they see:** the workarea's name and place in the tree, its stage, and the acts the key
  may perform.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_workarea_info).

### Link two memories

**Designed** · an agent

- **Goal:** connect two memories with a labelled link, and browse memories through their links
  and sources.
- **How it plays out:** the agent proposes the link; like a memory, it counts only once someone
  else approves it.
- **How it works:** [HTTP API](../api/http-api.md#claim-edges).

### List what a workarea holds

**Available now** for a developer · **Designed** for an agent

- **Goal:** page through a workarea's approved memories without a search, newest first, by kind
  or by label.
- **How it plays out:** a developer lists them over HTTP or through the libraries. The agent's
  tools list only its own memories today; listing the workarea's is part of the simpler tool set.
- **How it works:** [HTTP API](../api/http-api.md).

### Save many memories at once

**Designed** · a developer

- **Goal:** submit a batch of memories in one request instead of one request each.
- **How it works:** [HTTP API](../api/http-api.md#two-rings).

### Store and search from an application

**Available now** · a developer

- **Goal:** use OML from a Python or TypeScript program with typed calls.
- **How it plays out:** the library reads the server address and key from the environment and
  offers save, search, read, list and correct. It is installed from the source repository; it
  is not published to a package registry yet.
- **What they see:** typed results, and one error type per kind of refusal.
- **How it works:** [client SDKs](../engineering/sdks.md).

### When a request is refused

**Available now** · every user

- **Goal:** know which rule refused a request and the one step that fixes it.
- **What they see:** a refusal that names the rule, says whether retrying can help, and points at
  the next step.
- **How it works:** [errors](../api/errors.md).

### Retry without creating a duplicate

**Available now** · a developer, an agent

- **Goal:** retry a save after a timeout without saving it twice.
- **How it plays out:** every save carries a one-time request reference; a repeat with the same
  reference returns the first answer. The libraries do this for you.
- **How it works:** [HTTP API](../api/http-api.md#idempotency).

### Reuse a label that is taken

**Available now** · an agent

- **Goal:** start a new memory under a label another live memory in the workarea already has.
- **How it plays out:** OML refuses the save and says the label is taken; the agent picks
  another, or finds its own memory under that label and corrects it.
- **How it works:** [errors](../api/errors.md).

## Review

### Approve or reject the next memory

**Available now** · a reviewer, a developer building a reviewer bot

- **Goal:** look at the next memory someone else wrote and decide.
- **How it plays out:** the reviewer asks for the next item. OML never hands a reviewer their
  own writes. They approve it, or reject it with a reason. A reviewer bot does the same through
  the libraries. Agents cannot review through their own memory tools, by design.
- **What they see:** the memory, its sources, who wrote it and when; after approval, the memory
  in other agents' searches.
- **How it works:** [reviewer crews](../api/reviewer-crews.md#the-loop),
  [CLI reference](../cli/reference.md#oml-review-the-review-ceremony).

### See how much waits

**Designed** · a reviewer

- **Goal:** see how many memories wait for review, and for how long, per workarea.
- **How it works:** [reviewer crews](../api/reviewer-crews.md#1-queue-what-waits-and-how-old-it-is).

### Reopen a wrong rejection

**Designed** · a reviewer

- **Goal:** give a memory that was rejected by mistake a second review, by someone other than
  its author.

### Review a memory with its files

**Designed** · a reviewer

- **Goal:** see every file attached to a memory, each checked against its fingerprint, before
  deciding.
- **How it works:** [reviewer crews](../api/reviewer-crews.md#3-fetch-every-attachment).

### Require more than one approval, or a person's

**Designed** · an admin

- **Goal:** for some kinds of memory in a workarea, require two approvals, or one from a person.
- **How it works:** [reviewer crews](../api/reviewer-crews.md#k-1-and-human-required-verdicts).

### Approve your own work when you work alone

**Designed** · one person in a development workarea

- **Goal:** let one person working alone approve their own memory, marked as self-approved so
  nobody mistakes it for a review.

### A rubric beside the item

**Planned** · a reviewer

- **Goal:** see the area's agreed review rubric when an item is put in front of the reviewer.

## Teams and access

### Organize work into workareas

**Available now** · an admin

- **Goal:** give each crew a workarea under a parent, by name.
- **How it plays out:** the admin creates the workarea, sees the tree and one workarea's details,
  records crews by name and points each workarea at its crew. Workareas, crews and keys are
  addressed by readable names everywhere.
- **What they see:** the tree from the organization's root down, with each workarea's stage and
  crew.
- **How it works:** [CLI reference](../cli/reference.md#oml-workareas-tree-edges-status),
  [workareas](../architecture/workareas.md).

### Issue a key with a chosen role

**Available now** · an admin

- **Goal:** issue a key that reviews, administers or only reads, with access to chosen
  workareas.
- **What they see:** the key, shown once.
- **How it works:** [CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation),
  [roles and keys](../architecture/roles-and-keys.md).

### Let an agent write without reading its colleagues' work

**Available now** · an admin

- **Goal:** for a crew whose writers must not read each other's approved work, issue a key that
  saves into the workarea and cannot search it.
- **How it plays out:** the admin issues the key with the write-only option. By default an agent's
  key both saves and searches. A write-only agent still sees its own memories, in every state.
- **How it works:** [CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation).

### Look up a key

**Available now** · an admin

- **Goal:** see one key's access, its exclusions and whether it is live. Listing all keys,
  including those about to expire, is **designed**.

### Keep a key out of part of the tree

**Available now** · an admin

- **Goal:** keep a key out of one branch of the workarea tree whatever its other access says.
  Lifting the exclusion later is **designed**.
- **How it works:** [CLI reference](../cli/reference.md#oml-grants-oml-deny-oml-bans-scope-and-refusal).

### Cut a key off

**Designed** · an admin

- **Goal:** stop a key from doing anything, at once. Today the admin can exclude the key from the
  whole tree, which has the same effect on what it reaches.

### Replace a key

**Designed** · an admin

- **Goal:** replace a key with a successor, with a grace period, alone or for a whole crew.
  Today the path is to issue a new key and exclude the old one.

### Change a key's access

**Designed** · an admin

- **Goal:** add or remove access on an existing key and see what it can reach. Today access is
  fixed when the key is issued.

### Stop an author or source

**Designed** · an admin

- **Goal:** stop an author or a source from getting anything approved, after previewing what the
  ban would reach.

### Host another organization

**Designed** · an operator

- **Goal:** host a second organization on the same deployment, with its own first admin key.
  Today a deployment hosts one organization.

### Switch between servers and keys

**Designed** · an admin

- **Goal:** move the command-line tool between servers and keys by name. Today it takes options,
  environment variables or the one credentials file written when the first admin key is claimed.

### A workarea grows up

**Designed** · an admin

- **Goal:** move a workarea from development to committed to validated, proposed by one person
  and confirmed by another. Validated freezes it.
- **How it works:** [workareas](../architecture/workareas.md).

### Relate, move and tidy workareas

**Designed** · an admin

- **Goal:** record how two workareas relate (one continues, mirrors or derives from another);
  move a workarea under a new parent after previewing the change; move a memory to the workarea
  it belongs in; or serve an earlier approved version as current.

### Work together before review

**Designed** · an admin, the agents of a project

- **Goal:** open an in-progress workarea for a project, where teammates find each other's
  unreviewed memories at once, clearly marked as unreviewed, and nothing leaves the workarea
  until it is committed.
- **How it plays out:** the admin opens the workarea for the project. Inside it, every agent with
  access finds every memory, reviewed or not. A particular key can be kept from unreviewed
  memories. When the project is done and nothing waits for review, the admin proposes the
  commit and a second person confirms it; only then do the memories reach the rest of the
  organization. Review still happens for every memory; it is deferred, never skipped.

### Shared vocabularies

A vocabulary crew keeps the shared terms that make "active customer" mean the same thing in every
crew. Every use case here is **designed**.

- **Look up a term:** its suggestions, its place in the hierarchy and its mappings to other
  vocabularies.
- **Propose terms** as a term creator: new terms, mappings and versioned releases, and a
  deprecation that names the successor.
- **Tag memories** as a classifier, with the terms they are about.
- **Review** as a curator: proposed terms, and the classifiers' tags.
- **Set up the crew**, giving each member a key shaped for its role, without anyone seeing a key.
- **Search by term**, including its narrower terms.
- **Require valid terms** in every save in a workarea.

How it works: [ontologies](../architecture/ontologies.md).

## Sessions and agents

### Start every session from memory

**Available now** · an agent in Claude Code

- **Goal:** start, resume, clear, compact or fork a session already holding the workarea's
  standing instructions, its own recent memories and the memories that match the project.
- **How it plays out:** the plugin's start hook asks OML before the first prompt and adds the
  answer as data to check, not as instructions. If the server does not answer, the session
  starts normally with nothing added. The user can turn it off or change its size limit.
- **What they see:** a short block of context at the top of the session.
- **How it works:** [CLI reference](../cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin).
  That Claude Code adds this context is checked by tests outside Claude Code; a recorded Claude
  Code session is still owed ([roadmap](roadmap.md)).

### Save what a compaction summarized

**Available now** · an agent in Claude Code

- **Goal:** keep the durable facts from a session summary when Claude Code compacts the
  conversation.
- **How it plays out:** after a compaction, the start hook asks the agent to save the summary's
  durable facts as memories, each held for review.

### Allow reads once, keep asking on writes

**Available now** · the person working with an agent in Claude Code

- **Goal:** stop Claude Code asking before every search, and keep it asking before every save.
- **How it works:** [the plugin README](https://github.com/e8kor/open-memory-lab/blob/main/plugins/oml/README.md).

### Be told when to search

**Designed** · an agent in any tool

- **Goal:** have the server itself tell the agent to search memory before non-trivial work, on an
  unknown term and after a failure, in any tool, with no plugin. A working version exists and is
  not in any release.

### Recall known fixes when a command fails

**Designed** · an agent in Claude Code

- **Goal:** when a shell command fails, get the approved pitfalls that match the error, only if
  the user turned it on. A working version exists and is not in any release.

### Use OML from another tool

**Available now** · an agent in another tool

- **Goal:** use the same memory from any tool that connects to MCP servers over HTTP and can send
  the agent's key with each request.
- **What they see:** the same memory tools as in Claude Code. The session-start context is a
  Claude Code feature and is not offered elsewhere. No tool other than Claude Code has been
  checked with OML yet.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#transport-streamable-http-stateless).

### Connect claude.ai and the Claude apps with a one-time code

**Designed** · an agent in claude.ai, Claude Desktop or the mobile apps

- **Goal:** connect by typing a one-time code, with no key pasted anywhere.
- **How it plays out:** the admin issues the agent's key with a connect code instead of a value.
  The user adds OML as a connector and types the code once. The admin can see open connections
  and cut one off.

### Hand lookups to a librarian

**Planned** · an agent in Claude Code

- **Goal:** let a helper agent do memory lookups so the main agent's context stays small.

### Briefs, summaries and handoff notes

**Planned** · an agent in Claude Code

- **Goal:** on entering an area, get its short brief: what it is for, key decisions, conventions
  and where things are; read an approved summary of a whole area that names every memory it
  draws on; leave a note for the next session on what was done and what is open; be asked to
  meet the area's agreed definition of done before ending a turn; and load instructions for
  particular files only when touching them. Today the nearest is the standing instructions and
  the session-start context.

## Files and deliverables

Every use case here is **designed**.

### Hand in finished work

- **Who:** an agent.
- **Goal:** hand in a finished piece of work, such as a report with its figures, reviewed like any
  memory and traceable to the memories behind it.
- **How it plays out:** the agent saves the work as a deliverable with its files and names the
  memories it came from. A new version is a correction of the deliverable. Deliverables suit
  reports, analyses, diagrams and evidence; code belongs in git.

### Attach files to a memory

- **Who:** an agent; from claude.ai or mobile, short text files written into the request.
- **Goal:** attach a file, image or PDF to a memory. Files are named by their content, so they
  cannot be swapped afterwards, and are stored encrypted.
- **How it works:** [HTTP API](../api/http-api.md#attachments), [concepts](../concepts.md#attachments).

### Read and download a deliverable

- **Who:** an agent.
- **Goal:** see a deliverable's file list, read its small text files inline and download any file;
  list the deliverables in a workarea; and trace a result back to its sources, or forward to what
  was built from a memory.

### Choose where files are kept

- **Who:** a platform team.
- **Goal:** keep files on local disk by default, and move them to cloud object storage: copy them
  with one command while the server is stopped, then change one setting.

## Time

Every use case here is **designed**.

- **Say when a memory is true:** an agent states the period over which a memory holds in the
  world, such as one quarter.
- **Ask what OML held at an earlier date:** an auditor reads memory as it stood then, under the
  access they have today.
- **Keep only what was true at a time:** an agent filters searches and listings to memories true
  at a date or within a range.
- **Browse memories as read-only resources:** an agent in an MCP client opens memories and their
  files, present and past, without a tool call.
- **List, print and export from a terminal:** a workarea's memories at a point in time, under the
  agent's own key.

## Guardrails

### Standing instructions

**Available now** · an admin, every agent in the workarea

- **Goal:** give every agent in a workarea and below it instructions that come first in every
  search and are never cut.
- **How it plays out:** someone saves the instruction as a standing instruction; a reviewer
  approves it like any memory. From then on, every agent searching that workarea, or one below
  it, receives it first, as does the session-start context.
- **How it works:** [concepts](../concepts.md#steering).

### Team rules

**Designed** · an admin, every agent in the workarea

- **Goal:** set an approved team rule ("never", "must" or "should"), optionally checked by
  machine, that applies to every agent in a workarea and below.
- **How it plays out:** the rule is saved and approved like a memory. Each session and helper
  agent loads the rules at its start, kept apart from the rest of its context. A tool call that
  breaks a checked
  rule is denied, or the user is asked, before it runs. The admin decides per key whether it
  receives the rules of its own workarea only or of every workarea above it too. Tools with no
  hooks get the rules as a section of their instructions file. A change freeze is a rule that
  starts and ends on its own dates.

## Operations

### Check health

**Available now** · an operator

- **Goal:** know whether the deployment is healthy and what to do next.
- **What they see:** each health signal, whether it is healthy, and the next step. A fuller set of
  signals, and a runbook for each named condition, is **designed**.
- **How it works:** [CLI reference](../cli/reference.md#oml-status-and-oml-doctor-the-named-signals),
  [runbooks](../operations/runbooks.md).

### Install on Kubernetes

**Available now** · a platform team

- **Goal:** install OML on a cluster from the chart, and claim the first key before exposing it
  to the network.
- **How it plays out:** create the database secret, install the chart, read the enrollment code
  from the log, claim the key through a local tunnel, then turn on outside access.
- **How it works:** [deployment](../operations/deployment.md#the-interim-chart-the-current-build-on-kubernetes).

### Upgrade without losing identity

**Available now** · a platform team

- **Goal:** upgrade or restart the release without losing the deployment's signing identity.
  The service is down during an upgrade until the new version is ready.
- **How it works:** [deployment](../operations/deployment.md#upgrades-restarts-and-uninstall).

### Install or upgrade on one host

**Available now** on 64-bit Intel and AMD Linux · ARM Linux and a server-only archive **in the
next release** · an operator

- **Goal:** install or upgrade the server on one host from a signed archive.
- **How it works:** [deployment](../operations/deployment.md#the-server-archive).

### Run the server with least privilege

**In the next release** · an operator

- **Goal:** run the server under a database account that owns none of the tables it reads and
  writes, with a second account that owns them and upgrades them; and move an existing
  installation that uses one account to the two once.
- **What they see:** a server that refuses to start, with a named reason, until the two accounts
  are in place.
- **How it works:** [deployment](../operations/deployment.md).

### Apply settings and see what is in effect

**Designed** · an operator

- **Goal:** check and apply deployment settings, and see what the running deployment actually
  uses. Today settings come from one file and the environment at start.
- **How it works:** [configuration](../operations/configuration.md).

### Rehearse with invented facts

**Designed** · an admin

- **Goal:** run a crew against invented facts in a fenced scenario that real memory never sees,
  then end it.
- **How it works:** [simulations](../architecture/simulations.md).

### Import an existing data set

**Designed** · an admin

- **Goal:** bring a snapshot of an existing table or file set into memory, checked as one batch.

### Run in production

**Designed** · a platform team

- **Goal:** a production cluster where several servers answer reads and one at a time accepts
  writes, beside the services that fill the analytics store and keep checking the system, on a
  managed database and cloud object storage; back up,
  restore and recover from disaster without bringing erased content back; rotate the encryption
  keys and respond to a signing-key compromise; move from one host to Kubernetes without losing
  history; and monitor every part through metrics, logs and alerts that never carry memory
  content. Today the health and readiness checks and the start-up log lines exist.
- **How it works:** [deployment](../operations/deployment.md),
  [restore](../operations/restore.md), [disaster recovery](../operations/disaster-recovery.md),
  [observability](../operations/observability.md).

## Audit

### Follow the record

**Available now** · an auditor

- **Goal:** follow the recorded events a key is allowed to see, from any point onwards.
- **How it works:** [HTTP API](../api/http-api.md#events-tail).

### Check that history was not changed

**Designed** · an auditor

- **Goal:** verify that the signed history was not edited afterwards. The checking command is
  built; the server does not yet hand out the history it checks.
- **How it works:** [CLI reference](../cli/reference.md#oml-audit-evidence-export-and-chain-verification).

### One bundle of evidence

**Designed** · an auditor

- **Goal:** get one signed bundle answering who wrote, approved and read a memory, and when, with
  the history of who had access and how often the same people approved each other's work. Who
  read what is kept for a retention window the organization sets.
- **How it works:** [compliance evidence](compliance.md),
  [audit and lineage](../architecture/audit-and-lineage.md).

### Withdraw or erase a memory

**Designed** · an admin

- **Goal:** withdraw an approved memory from use while keeping its content; or erase its content
  permanently, with a record of every copy and what happened to it.
- **How it works:** [compliance evidence](compliance.md), [commitments](commitments.md).

### Retention and legal hold

**Planned** · an auditor, an admin

- **Goal:** age memories out on a schedule, and keep some under a legal hold.

### Offboard an organization

**Designed** · an operator

- **Goal:** suspend an organization's writes and erase its content, in one recorded ceremony.
- **How it works:** [runbooks](../operations/runbooks.md#tenant-offboarding).

### Analyze memory in an analytics store

**Designed** · an auditor, an analyst

- **Goal:** analyze memory and where it came from in an analytics store, where every figure traces
  back to its source.
- **How it works:** [storage](../architecture/storage.md).
