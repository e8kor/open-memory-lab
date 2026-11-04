<!--
id: product/use-cases
tier: client
status: draft for review · statuses read v0.3.1 and v0.4.0 as released
answers: who uses Open Memory Lab and for what; each use case's goal, how it plays out in v0.4.0, what its user sees and whether it is available now in v0.3.1, in v0.4.0, designed with its planned release, parked or not scheduled, or planned; grouped by journey, with one walkthrough of a day of an agent and a reviewer
-->

# Use cases

**Status:** draft for review · statuses read v0.3.1 and v0.4.0 as released

This page describes what each kind of user does with Open Memory Lab (OML), one use case at a
time: the goal, how it plays out, what the user sees, and whether it works today. It explains
what happens, not how; each use case links to the page that explains how.

- **For the quick read**, the [quick scenarios](scenarios.md) walk eight of these use cases in
  five steps or fewer each.
- **For when each feature arrived or is planned**, [releases](releases.md) is the one record;
  this page follows it.
- **For the calls behind each use case**, the
  [use-case call map](../engineering/use-case-call-map.md) is the engineering companion.

The stories below tell how each use case works in v0.4.0. In v0.4.0 an agent has three tools:
`oml_write` saves, `oml_read` finds, and `oml_workarea` says where the agent works and what it may
do there. People use the `oml` command. v0.3.1 offers the same use cases under older tool names,
which keep answering through the 0.4 releases ([upgrading from v0.3](../guides/upgrading.md)).

## How to read the status

Every use case has one of these statuses. A status describes the software, never an intention.

| Status | Means |
|---|---|
| **Available now** | Works since v0.3.1 or earlier. Where v0.4.0 changes how it works, the story tells the v0.4.0 way and a note says what v0.3.1 does. |
| **In v0.4.0** | Introduced in v0.4.0, the current release. |
| **Designed** | Decided, not built, and in no release. Each says what [releases](releases.md) assigns it: **planned for** a named release, **parked** (deliberately set aside, with no release) or **not scheduled** (no release assigned yet). |
| **Planned** | Decided, not yet designed: the project has decided to do it, and how it works is still open. |

Nothing on this page has been measured. Where a use case depends on how fast or how well
something works, such as how good search results are, the [roadmap](roadmap.md) holds the
open question.

## Who uses OML

| Who | What they do with OML | Where they appear |
|---|---|---|
| **An agent in Claude Code**, and the person working with it | Remembers, searches and corrects shared memory through the OML plugin, and starts every session already knowing what its team knows. | [Everyday memory](#everyday-memory), [Sessions and agents](#sessions-and-agents), [Files and deliverables](#files-and-deliverables) |
| **An agent in another tool** | The same memory from any other tool that speaks MCP, the standard way AI assistants connect to outside tools; in v0.4.0, from claude.ai and the Claude apps too. | [Sessions and agents](#sessions-and-agents) |
| **A reviewer, person or agent** | Looks at each new memory someone else wrote and approves or rejects it. Reviewers work from the command line or over HTTP, never through the agent's own tools. | [Review](#review) |
| **An operator or admin** | Sets up the server, gives out keys, organizes the workareas and keeps the system healthy, from the command-line tool. | [Getting started](#getting-started), [Teams and access](#teams-and-access), [Operations](#operations) |
| **A vocabulary crew** | Keeps a shared glossary: proposes terms, tags memories with them and reviews both. Parked. | [Shared vocabularies](#shared-vocabularies) |
| **A developer** | Uses OML from an application through the Python or TypeScript library, or over plain HTTP. | [Getting started](#getting-started), [Everyday memory](#everyday-memory), [Review](#review) |
| **An auditor, compliance reader or analyst** | Asks who wrote, approved and read a memory, checks that the history was not changed afterwards, and analyzes memory in bulk. | [Audit](#audit), [Time](#time) |
| **A platform team** | Runs OML on Kubernetes or on its own hosts, upgrades it and keeps its files and backups. | [Operations](#operations), [Files and deliverables](#files-and-deliverables) |

## A day with OML

One agent and one reviewer, a working day, told as it runs in v0.4.0. Every step also works in
v0.3.1 under the older tool names.

**Morning.** A developer opens Claude Code in the `procurement-reports` project. Before the
first prompt, OML has already added three things to the session: the team's standing
instructions for its workarea ("quote every price in euros, with its source"), the agent's own
ten most recent memories with where each stands, and the memories that match the project's
name. The developer asks for this week's metals price summary. OML has told the agent to search
memory before non-trivial work, so it searches first with `oml_read` and finds an approved note
from last week: the copper supplier changed its invoice format.

**Mid-morning.** While working, the agent finds that one supplier now quotes in dollars. It
saves that as a new memory with `oml_write`. OML answers that the memory is held for review. The
agent can already find it when it asks a search to include its own unreviewed memories, marked
as unreviewed; no other agent sees it yet. It also records a conclusion, "metals prices rose this
week", and names the three memories it drew it from.

**Noon.** A reviewer, here a person on the procurement team, runs `oml review next`. OML hands
them the next held memory written by someone else: the dollar-quote finding. They approve it
with `oml review approve`. The next item is the price conclusion; one of the memories behind it
is a week old, so they reject it with `oml review reject` and a reason: "use this week's
figures". The reviewer is never shown their own writes.

**Afternoon.** The agent lists its own memories by state and sees one approved and one rejected.
OML does not show the agent the reviewer's reason, so the reviewer tells the developer, who asks
the agent for a corrected conclusion from this week's figures. The developer also notices that
last week's note about the copper supplier is now wrong and asks the agent to correct it; the
agent writes a new version, naming the note it replaces, and until a reviewer approves it,
everyone keeps seeing the old one.

**End of day.** The reviewer approves the corrected conclusion and the new version of the
copper note. Every agent with access to the workarea now finds them; the old version of the
note no longer comes back in searches. Every step is in OML's tamper-evident history: who wrote
each memory, who approved or rejected it, and when.

**The next morning,** a second agent on the same team searches for "supplier currency" before
its first task and finds the approved dollar-quote finding, which it had never seen.

What the same day will add as designed use cases are built: the team's rules loaded and checked
before each command (planned for v0.5.1), an in-progress workarea where teammates see each
other's unreviewed work at once (planned for v0.6.0), and the price chart attached to the
conclusion as a file (not scheduled).

## Getting started

### Try OML on a laptop

**Available now** with the quickstart script · **In v0.4.0** as one command · anyone with Docker

- **Goal:** see the whole loop working in minutes before deciding anything.
- **How it plays out:** `oml quickstart` starts the server and its database with Docker, claims
  the first admin key, creates a workarea, issues an agent key and a reviewer key into private
  files, and checks one save end to end: it saves a memory as the agent, approves it as the
  reviewer and finds it as the agent. It prints the line that connects Claude Code on the same
  machine.
- **What they see:** each step's result, the key files, and the next two commands. A step that
  fails is named.
- **Before v0.4.0:** the operator starts the stack with Docker Compose and runs the quickstart
  script, which stops at the review.
- **Not yet shown working:** the command's own tests pass, and a recorded run on a real Docker
  host is still owed ([self-hosting](../guides/self-hosting.md)).
- **How it works:** [self-hosting](../guides/self-hosting.md),
  [CLI reference](../cli/reference.md#oml-setup-claude-code-and-oml-quickstart-the-quick-start).

### Claim the first admin key

**Available now** · changed in v0.4.0 · an operator

- **Goal:** take control of a new deployment, once.
- **How it plays out:** a new server prints a one-time enrollment code at its first start. The
  operator exchanges it for the first admin key. The code works once; a second attempt is
  refused. In v0.4.0 the code is read from the terminal without echo, or from a pipe, and never
  typed on the command line, so it stays out of the shell's history.
- **What they see:** the key, shown once and stored in a private credentials file.
- **How it works:** [CLI reference](../cli/reference.md#oml-bootstrap-claiming-the-root-operator-key),
  [deployment](../operations/deployment.md#claiming-the-first-admin-key-the-bootstrap-race).

### Give an agent its key without showing it

**Available now** · changed in v0.4.0 · an operator

- **Goal:** give an agent the one key it needs to save and search in its workarea, without the
  key ever appearing in a chat transcript.
- **How it plays out:** the operator issues the agent's key in one command, naming the agent and
  its workarea, and writes it to a private file. In v0.4.0 the command also prints the one line
  the teammate runs, `oml setup claude-code` with that file, which installs the plugin and hands
  it the key. From inside Claude Code, the admin plugin's skill runs the same command.
- **What they see:** the line to send; the key itself goes only to the file.
- **How it works:** [CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation),
  [the oml-admin plugin](../cli/reference.md#the-oml-admin-plugin).

### Install the plugin in Claude Code

**Available now** · changed in v0.4.0 · an agent in Claude Code

- **Goal:** connect Claude Code to OML.
- **How it plays out:** in v0.4.0, `oml setup claude-code` does it in one command: it checks the
  key with the server, refuses an admin key, adds the project's plugin marketplace at the CLI's
  own release, installs the plugin and gives it the server address and the key, without the key
  appearing on screen. Without the command-line tool, the user installs the plugin from Claude
  Code and fills in its settings.
- **What they see:** the three memory tools in Claude Code after a reload, and the session
  context at the next start.
- **Before v0.4.0:** the user sets the server address and the key in the environment, then adds
  the marketplace and installs the plugin.
- **Not yet shown working:** the command has been run against a stand-in for Claude Code in its
  tests and its steps by hand, not yet end to end against a real Claude Code.
- **How it works:** [Claude Code guide](../guides/claude-code.md).

### Enter the server and key once, kept in the keychain

**In v0.4.0** · an agent in Claude Code

- **Goal:** set up the plugin without editing a shell profile.
- **How it plays out:** Claude Code asks for the server address and the key when the plugin is
  installed, and keeps the key in the system keychain. An operator who issued the key to a
  private file hands that file to `oml setup claude-code` instead, without the key appearing on
  screen. The plugin's tools and its start-of-session context read only what Claude Code stored.
- **What they see:** one prompt at install, with the key field masked, and no key in any
  settings file.
- **How it works:** [Claude Code guide](../guides/claude-code.md#without-the-cli),
  [MCP plugin](../api/mcp-plugin.md#wiring-it-into-your-agent).

### Install the command-line tool from a verified download

**Available now** for Linux, macOS and Windows · the one-line installer **in v0.4.0** · anyone
who runs the `oml` command

- **Goal:** install the `oml` tool from a signed release, checked before it runs.
- **How it plays out:** in v0.4.0 one line installs it for the current user: the installer
  downloads the archive for the platform, checks its signature and checksum, and writes one
  file. Windows has a PowerShell installer. Checking the signature by hand stays possible, and
  is the only way in v0.3.1. On Windows the session hook needs Git Bash.
- **What they see:** what the installer checked, and where it put `oml`.
- **How it works:** [install](../start/install.md),
  [CLI reference](../cli/reference.md#installing-oml-and-what-differs-on-windows).

### Log in once, and switch between servers

**In v0.4.0** · anyone who runs the `oml` command

- **Goal:** give the `oml` command a server and a key once, check them, and move between
  servers.
- **How it plays out:** `oml login` with the server's address asks for the key without echo,
  checks it with the server, and only then stores the address and the key in a private
  credentials file. It prints the key's name, role, organization and default workarea.
  `oml doctor` later shows where each setting came from, never the key itself. A second server
  is a second credentials file, chosen by one setting. The same two settings, the server address
  and the key, are the only ones any client needs.
- **What they see:** who the key is, at login and on every check.
- **Before v0.4.0:** the tool takes options, environment variables or the one credentials file
  written when the first admin key is claimed.
- **How it works:** [CLI reference](../cli/reference.md#oml-login-and-oml-doctor-storing-and-checking-the-pair).

### Call OML from any language

**Available now** · changed in v0.4.0 · a developer

- **Goal:** use OML without either library, from any language.
- **How it plays out:** the developer reads the published API description and calls what the
  server answers. In v0.4.0 the description lists only the calls the server answers; the
  designed ones sit in a separate document, so no listed call answers "not found".
- **What they see:** plain JSON over HTTP, with typed errors.
- **How it works:** [HTTP API](../api/http-api.md), [OpenAPI document](../api/openapi.yaml).

### One obvious call for each task

**In v0.4.0** · every developer and agent

- **Goal:** a smaller client surface where each task has one obvious call, its common case
  needs the fewest inputs, and the server works out what it can.
- **How it plays out:** the agent gets one tool to save, one to read and one to see its
  workarea, in place of eight. The libraries and the command line follow the same shape:
  `write`, `read`, and the review calls. Existing users migrate once, with each change listed;
  the old tool names and commands keep answering, unlisted, through the 0.4 releases.
- **What they see:** fewer tools to learn, and no invented labels to supply.
- **How it works:** [MCP plugin](../api/mcp-plugin.md), [upgrading from v0.3](../guides/upgrading.md).

## Everyday memory

### Save something worth keeping

**Available now** · in v0.4.0 through `oml_write` · an agent, a developer

- **Goal:** keep a fact, decision or finding so that later sessions and other agents can use it,
  once someone else approves it.
- **How it plays out:** the agent saves the memory with `oml_write`. OML scans it and holds it
  for review, and gives it a label that groups its future versions.
- **What they see:** "held for review". Nobody else sees the memory until a reviewer who did not
  write it approves it.
- **Before v0.4.0:** the agent also chooses the label itself.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_write),
  [concepts](../concepts.md#memory-as-claim).

### Record a conclusion and what it came from

**Available now** · in v0.4.0 through `oml_write` · an agent

- **Goal:** save a conclusion drawn from memories it already has, naming those memories.
- **How it plays out:** the agent saves the conclusion with `oml_write` and lists its sources.
  The reviewer sees the sources beside it.
- **What they see:** the same "held for review", and later a conclusion anyone can trace back.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_write).

### Search before acting

**Available now** · in v0.4.0 through `oml_read` · an agent, a developer

- **Goal:** find what the team already knows before starting work, within a size limit.
- **How it plays out:** the agent searches with `oml_read` and a few exact words. The
  workarea's standing instructions come first and are never cut; the matching memories follow,
  as many as fit the limit. Search matches words, not meaning: search by meaning is on the
  [roadmap](roadmap.md), not scheduled. When nothing is found, the agent tries other words, or,
  in Claude Code, asks the librarian.
- **What they see:** standing instructions, then approved memories, best match first, and how
  much the limit cut. Approval never changes the order.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_read),
  [retrieval](../architecture/retrieval.md).

### See its own work while it waits

**Available now** · in v0.4.0 through `oml_read` · an agent

- **Goal:** see its own memories that still wait for review, and find them in searches.
- **How it plays out:** the agent lists its own memories, newest first, by calling `oml_read`
  with no search, or asks a search to include its own unreviewed ones.
- **What they see:** each of its memories with where it stands: held, approved, rejected,
  withdrawn or erased. Its unreviewed memories are marked; no other key sees them.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_read).

### Read one memory back

**Available now** · in v0.4.0 through `oml_read` · an agent

- **Goal:** open one memory it has a reference to.
- **What they see:** the memory, what sort of memory it is, its workarea, its sources and where it
  stands.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_read).

### Correct a memory

**Available now** · in v0.4.0 through `oml_write` · an agent

- **Goal:** replace a memory with a corrected version.
- **How it plays out:** the agent saves the new version with `oml_write`, naming the memory it
  replaces. It is reviewed like any memory, and until it is approved everyone keeps seeing the
  old one.
- **What they see:** the new version held for review, then the new version in place of the old.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_write).

### Propose retiring a memory

**Available now** · in v0.4.0 through `oml_write`, and from the command line · an agent

- **Goal:** take a memory that is no longer true out of use, without deleting anything.
- **How it plays out:** the agent saves, with `oml_write`, a retirement of the memory, its text
  saying why. A reviewer approves or rejects it; until then the memory stays in use. Approved,
  it stops coming back in searches, and its history stays.
- **What they see:** the proposal held for review, then the memory gone from searches.
- **Before v0.4.0:** the command-line tool has no retirement option.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_write),
  [CLI reference](../cli/reference.md#oml-write-oml-read-oml-events-memories-and-the-journal).

### Learn when a memory was approved or rejected

**Available now** · changed in v0.4.0 · an agent

- **Goal:** find out what happened to its held memories.
- **How it plays out:** the agent lists its own memories with `oml_read`, asking for one state:
  held, approved or rejected. There is no position in a stream to keep.
- **What they see:** each memory's new state. The reviewer's reason for a rejection is not shown
  to the agent.
- **Before v0.4.0:** the agent can also follow the stream of recorded events it may see; in
  v0.4.0 that stream is for auditors and no longer one of the agent's tools.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_read).

### Check where it works and what it may do

**Available now** · in v0.4.0 through `oml_workarea` · an agent

- **Goal:** know its workarea and what its key may do there.
- **What they see:** the workarea's name and place in the tree, its stage, and the acts the key
  may perform.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_workarea).

### Link two memories

**Designed** · planned for v0.5.2 · an agent

- **Goal:** connect two memories with a labelled link, and browse memories through their links
  and sources.
- **How it plays out:** the agent proposes the link; like a memory, it counts only once someone
  else approves it.
- **How it works:** [HTTP API](../api/http-api.md#edges).

### List what a workarea holds

**Available now** for a developer · **In v0.4.0** for an agent

- **Goal:** page through a workarea's approved memories without a search, newest first, by kind
  or by label.
- **How it plays out:** a developer lists them over HTTP or through the libraries. In v0.4.0 the
  agent lists them with `oml_read`, asking for the workarea's memories instead of its own.
- **How it works:** [MCP plugin](../api/mcp-plugin.md#oml_read), [HTTP API](../api/http-api.md).

### Save many memories at once

**Designed** · not scheduled · a developer

- **Goal:** submit a batch of memories in one request instead of one request each.
- **How it works:** [HTTP API](../api/http-api.md#two-rings).

### Store and search from an application

**Available now** · changed in v0.4.0 · a developer

- **Goal:** use OML from a Python or TypeScript program with typed calls.
- **How it plays out:** the library reads the server address and key from the environment. In
  v0.4.0 it offers the agent's calls, `write`, `read` and `workarea`, beside the event feed and
  the review calls. It is installed from the source repository; it is not published to a
  package registry yet.
- **What they see:** typed results, and one error type per kind of refusal.
- **How it works:** [client SDKs](../engineering/sdks.md).

### When a request is refused

**Available now** · changed in v0.4.0 · every user

- **Goal:** know which rule refused a request and the one step that fixes it.
- **What they see:** a refusal that names the rule, says whether retrying can help, and points at
  the next step. In v0.4.0 it has one shape everywhere: over HTTP, in the command line, in the
  libraries and in the agent's tools.
- **How it works:** [errors](../api/errors.md).

### Retry without creating a duplicate

**Available now** · a developer, an agent

- **Goal:** retry a save after a timeout without saving it twice.
- **How it plays out:** every save carries a one-time request reference; a repeat with the same
  reference returns the first answer. The libraries and the command line do this for you.
- **How it works:** [HTTP API](../api/http-api.md#idempotency).

### Reuse a label that is taken

**Available now** · changed in v0.4.0 · an agent

- **Goal:** start a new memory under a label another live memory in the workarea already has.
- **How it plays out:** in v0.4.0 this arises only when the agent chooses a label itself, or
  saves the same text twice, since OML makes the label otherwise. OML refuses the save and says
  the label is taken; the agent picks another, or finds its own memory under that label and
  corrects it.
- **How it works:** [errors](../api/errors.md).

## Review

### Approve or reject the next memory

**Available now** · changed in v0.4.0 · a reviewer, a developer building a reviewer bot

- **Goal:** look at the next memory someone else wrote and decide.
- **How it plays out:** the reviewer runs `oml review next`. OML never hands a reviewer their
  own writes. They approve it with `oml review approve`, or reject it with `oml review reject`
  and a reason. A reviewer bot does the same through the libraries. Agents cannot review through
  their own memory tools, by design.
- **What they see:** the memory, its sources, who wrote it and when; after approval, the memory
  in other agents' searches.
- **Before v0.4.0:** the verdict names the memory it decides; in v0.4.0 it applies to the one
  item the reviewer holds.
- **How it works:** [reviewer crews](../api/reviewer-crews.md#the-loop),
  [CLI reference](../cli/reference.md#oml-review-the-review-ceremony).

### See how much waits

**Designed** · not scheduled · a reviewer

- **Goal:** see how many memories wait for review, and for how long, per workarea.
- **How it works:** [reviewer crews](../api/reviewer-crews.md#1-queue-what-waits-and-how-old-it-is).

### Reopen a wrong rejection

**Designed** · not scheduled · a reviewer

- **Goal:** give a memory that was rejected by mistake a second review, by someone other than
  its author.

### Review a memory with its files

**Designed** · not scheduled · a reviewer

- **Goal:** see every file attached to a memory, each checked against its fingerprint, before
  deciding.
- **How it works:** [reviewer crews](../api/reviewer-crews.md#3-fetch-every-attachment).

### Require more than one approval, or a person's

**Designed** · not scheduled · an admin

- **Goal:** for some kinds of memory in a workarea, require two approvals, or one from a person.
- **How it works:** [reviewer crews](../api/reviewer-crews.md#k-1-and-human-required-verdicts).

### Approve your own work when you work alone

**Designed** · not scheduled; the in-progress workareas planned for v0.6.0 revise it · one
person in a development workarea

- **Goal:** let one person working alone approve their own memory, marked as self-approved so
  nobody mistakes it for a review.

### A rubric beside the item

**Planned** · not scheduled · a reviewer

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
  including those about to expire, is **designed**, not scheduled.

### Keep a key out of part of the tree

**Available now** · an admin

- **Goal:** keep a key out of one branch of the workarea tree whatever its other access says.
  Lifting the exclusion later is **designed**, not scheduled.
- **How it works:** [CLI reference](../cli/reference.md#oml-grants-oml-deny-oml-bans-scope-and-refusal).

### Cut a key off

**Designed** · not scheduled · an admin

- **Goal:** stop a key from doing anything, at once. Today the admin can exclude the key from the
  whole tree, which has the same effect on what it reaches.

### Replace a key

**Designed** · not scheduled · an admin

- **Goal:** replace a key with a successor, with a grace period, alone or for a whole crew.
  Today the path is to issue a new key and exclude the old one.

### Change a key's access

**Designed** · not scheduled · an admin

- **Goal:** add or remove access on an existing key and see what it can reach. Today access is
  fixed when the key is issued.

### Stop an author or source

**Designed** · not scheduled · an admin

- **Goal:** stop an author or a source from getting anything approved, after previewing what the
  ban would reach.

### Host another organization

**Designed** · not scheduled · an operator

- **Goal:** host a second organization on the same deployment, with its own first admin key.
  Today a deployment hosts one organization.

### A workarea grows up

**Designed** · planned for v0.5.2 · an admin

- **Goal:** move a workarea from development to committed to validated, proposed by one person
  and confirmed by another. Validated freezes it.
- **How it works:** [workareas](../architecture/workareas.md).

### Relate, move and tidy workareas

**Designed** · not scheduled · an admin

- **Goal:** record how two workareas relate (one continues, mirrors or derives from another);
  move a workarea under a new parent after previewing the change; move a memory to the workarea
  it belongs in; or serve an earlier approved version as current.

### Work together before review

**Designed** · planned for v0.6.0 · an admin, the agents of a project

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
crew. Every use case here is **designed** and **parked**.

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
  starts normally with nothing added. The user can turn it off or change its size limit. In
  v0.4.0 it reads the server address and key from the plugin's own settings.
- **What they see:** a short block of context at the top of the session.
- **Not yet shown working:** that Claude Code adds this context is checked by tests outside
  Claude Code; a recorded Claude Code session is still owed ([roadmap](roadmap.md)).
- **How it works:** [Claude Code guide](../guides/claude-code.md#5-every-session-starts-from-memory),
  [CLI reference](../cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin).

### Save what a compaction summarized

**Available now** · an agent in Claude Code

- **Goal:** keep the durable facts from a session summary when Claude Code compacts the
  conversation.
- **How it plays out:** after a compaction, the start hook asks the agent to save the summary's
  durable facts as memories with `oml_write`, each held for review.

### Allow reads once, keep asking on writes

**Available now** · changed in v0.4.0 · the person working with an agent in Claude Code

- **Goal:** stop Claude Code asking before every search, and keep it asking before every save.
- **How it plays out:** in v0.4.0 two permission rules allow the two reading tools, `oml_read`
  and `oml_workarea`, and Claude Code keeps asking before `oml_write`. Rules written for the
  older read tools allow nothing in v0.4.0 and are replaced once.
- **How it works:** [Claude Code guide](../guides/claude-code.md#5-every-session-starts-from-memory),
  [the plugin README](https://github.com/e8kor/open-memory-lab/blob/main/plugins/oml/README.md).

### Be told when to search

**In v0.4.0** · an agent in any tool

- **Goal:** have the server itself tell the agent to search memory before non-trivial work, on an
  unknown term and after a failure, in any tool, with no plugin.
- **How it plays out:** when a tool connects, OML hands it short instructions on when to search,
  which the tool passes to its agent. The agent then searches with `oml_read` on its own.
- **How it works:** [MCP plugin](../api/mcp-plugin.md).

### Recall known fixes when a command fails

**In v0.4.0** · an agent in Claude Code, when the user turns it on

- **Goal:** when a shell command fails, get the approved pitfalls that match the error.
- **How it plays out:** off by default. Turned on, a failed command searches OML for up to three
  distinctive words of its error and adds what it finds to the session. The cost: those words
  go to the user's server.
- **How it works:** [CLI reference](../cli/reference.md#oml-hook-tool-failure-recall-after-a-failed-command).

### Use OML from another tool

**Available now** · changed in v0.4.0 · an agent in another tool

- **Goal:** use the same memory from any tool that connects to MCP servers over HTTP and can send
  the agent's key with each request.
- **How it plays out:** each release ships a kit of ready configuration files for OpenAI Codex
  CLI, Cursor, Gemini CLI, VS Code and Claude Desktop. The user logs in once and gives the tool
  the server address and the key. In v0.4.0 every tool gets the three agent tools.
- **What they see:** the same memory tools as in Claude Code. The session-start context is a
  Claude Code feature, with one exception: the Gemini CLI configuration wires it too.
- **Not yet shown working:** some steps rest on each tool's own documentation and are marked
  unverified in the kit.
- **How it works:** [other agent tools](../guides/other-harnesses.md),
  [MCP plugin](../api/mcp-plugin.md#transport-streamable-http-stateless).

### Connect claude.ai and the Claude apps with a one-time code

**In v0.4.0** · an agent in claude.ai, Claude Desktop or the mobile apps

- **Goal:** connect by typing a one-time code, with no key pasted anywhere.
- **How it plays out:** the admin issues the agent's key with a connect code instead of a value.
  The user adds the server's address as a custom connector on claude.ai and types the code once
  on OML's own page, which shows what the connection may do. Every Claude app signed in to that
  account, the phone apps included, then has the three tools. The admin can see open
  connections and cut one off.
- **What they see:** the organization, the agent and the workareas the connection will use,
  before they confirm.
- **Needs:** a server on a public HTTPS address.
- **Not yet shown working:** no connection from claude.ai itself has been made, and no green run
  of the connection's end-to-end tests is recorded yet.
- **How it works:** [claude.ai and mobile](../guides/claude-ai-and-mobile.md).

### Hand lookups to a librarian

**In v0.4.0** · an agent in Claude Code

- **Goal:** let a helper agent do memory lookups so the main agent's context stays small.
- **How it plays out:** the plugin ships a librarian, a helper agent that can only read. Asked
  what OML holds about something, it runs up to eight searches and answers with what it found.
- **How it works:** [Claude Code guide](../guides/claude-code.md#6-optional-the-librarian-and-failure-recall).

### Briefs, summaries and handoff notes

**Planned** · an agent in Claude Code

- **Goal:** on entering an area, get its short brief: what it is for, key decisions, conventions
  and where things are (planned for v0.5.2); read an approved summary of a whole area that names
  every memory it draws on (parked); leave a note for the next session on what was done and what
  is open; be asked to meet the area's agreed definition of done before ending a turn; and load
  instructions for particular files only when touching them (these three not scheduled). Today
  the nearest is the standing instructions and the session-start context.
- **How it plays out:** a reviewer proposes a reviewed memory into the area's brief and a second
  reviewer confirms it. Each session that starts in the area then sees the brief first, before
  the standing instructions, and an agent that moves to another area reads that area's brief
  with `oml_workarea`. The brief is kept short by a size limit the server sets.

## Files and deliverables

Every use case here is **designed**: deliverables and storage are parked, and files on a memory
are not scheduled.

### Hand in finished work

- **Who:** an agent. **Parked.**
- **Goal:** hand in a finished piece of work, such as a report with its figures, reviewed like any
  memory and traceable to the memories behind it.
- **How it plays out:** the agent saves the work as a deliverable with its files and names the
  memories it came from. A new version is a correction of the deliverable. Deliverables suit
  reports, analyses, diagrams and evidence; code belongs in git.

### Attach files to a memory

- **Who:** an agent; from claude.ai or mobile, short text files written into the request.
  **Not scheduled.**
- **Goal:** attach a file, image or PDF to a memory. Files are named by their content, so they
  cannot be swapped afterwards, and are stored encrypted. The agent's tools carry no file bytes:
  a file goes up from a terminal or a library, and the save names it.
- **How it works:** [HTTP API](../api/http-api.md#attachments), [concepts](../concepts.md#attachments).

### Read and download a deliverable

- **Who:** an agent. **Parked.**
- **Goal:** see a deliverable's file list, read its small text files inline and download any file;
  list the deliverables in a workarea; and trace a result back to its sources, or forward to what
  was built from a memory.

### Choose where files are kept

- **Who:** a platform team. **Parked.**
- **Goal:** keep files on local disk by default, and move them to cloud object storage: copy them
  with one command while the server is stopped, then change one setting.

## Time

Every use case here is **designed** and **parked**.

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
- **How it plays out:** someone saves the instruction as a standing instruction with
  `oml_write`; a reviewer approves it like any memory. From then on, every agent searching that
  workarea, or one below it, receives it first, as does the session-start context.
- **How it works:** [concepts](../concepts.md#steering).

### Team rules

**Designed** · planned for v0.5.1 · an admin, every agent in the workarea

- **Goal:** set an approved team rule ("never", "must" or "should"), optionally checked by
  machine, that applies to every agent in a workarea and below.
- **How it plays out:** the rule is saved and approved like a memory. Each session and helper
  agent loads the rules at its start, kept apart from the rest of its context. A tool call that
  breaks a checked rule is denied, or the user is asked, before it runs. The admin decides per
  key whether it receives the rules of its own workarea only or of every workarea above it too.
  Tools with no hooks get the rules as a section of their instructions file. A change freeze is
  a rule that starts and ends on its own dates.

## Operations

### Check health

**Available now** · changed in v0.4.0 · an operator, anyone who runs the `oml` command

- **Goal:** know whether the deployment is healthy and what to do next.
- **How it plays out:** `oml doctor` checks the server's health and readiness. In v0.4.0 it also
  shows where the server address and the key came from, never the key itself, which key it is,
  and, on a server that accepts claude.ai connections, whether that setup answers.
- **What they see:** each health signal, whether it is healthy, and the next step. A fuller set of
  signals, and a runbook for each named condition, is **designed**, not scheduled.
- **How it works:** [CLI reference](../cli/reference.md#oml-status-and-oml-doctor-the-named-signals),
  [runbooks](../operations/runbooks.md).

### Install on Kubernetes

**Available now** · a platform team

- **Goal:** install OML on a cluster from the chart, and claim the first key before exposing it
  to the network.
- **How it plays out:** create the database secret, install the chart, read the enrollment code
  from the log, claim the key through a local tunnel, then turn on outside access.
- **How it works:** [deployment](../operations/deployment.md#the-interim-chart-the-current-build-on-kubernetes),
  [self-hosting](../guides/self-hosting.md).

### Upgrade without losing identity

**Available now** · a platform team

- **Goal:** upgrade or restart the release without losing the deployment's signing identity.
  The service is down during an upgrade until the new version is ready. The upgrade to v0.4.0
  moves the server, the command-line tool and the plugin together and changes no stored data.
- **How it works:** [deployment](../operations/deployment.md#upgrades-restarts-and-uninstall),
  [upgrading from v0.3](../guides/upgrading.md).

### Install or upgrade on one host

**Available now** on 64-bit Intel and Arm Linux · an operator

- **Goal:** install or upgrade the server on one host from a signed archive of its own.
- **How it works:** [deployment](../operations/deployment.md#the-server-archive),
  [self-hosting](../guides/self-hosting.md).

### Run the server with least privilege

**Available now** · an operator

- **Goal:** run the server under a database account that owns none of the tables it reads and
  writes, with a second account that owns them and upgrades them; and move an existing
  installation that uses one account to the two once.
- **What they see:** a server that refuses to start, with a named reason, until the two accounts
  are in place.
- **How it works:** [deployment](../operations/deployment.md).

### Apply settings and see what is in effect

**Designed** · not scheduled · an operator

- **Goal:** check and apply deployment settings, and see what the running deployment actually
  uses. Today settings come from one file and the environment at start.
- **How it works:** [configuration](../operations/configuration.md).

### Rehearse with invented facts

**Designed** · not scheduled · an admin

- **Goal:** run a crew against invented facts in a fenced scenario that real memory never sees,
  then end it.
- **How it works:** [simulations](../architecture/simulations.md).

### Import an existing data set

**Designed** · not scheduled · an admin

- **Goal:** bring a snapshot of an existing table or file set into memory, checked as one batch.

### Run in production

**Designed** · not scheduled · a platform team

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

**Available now** · changed in v0.4.0 · an auditor

- **Goal:** follow the recorded events a key is allowed to see, from any point onwards.
- **How it plays out:** over HTTP, through the libraries, and in v0.4.0 with `oml events`. In
  v0.4.0 the feed is no longer one of the agent's tools: an agent learns about its own memories
  by listing them.
- **How it works:** [HTTP API](../api/http-api.md#events-tail),
  [CLI reference](../cli/reference.md#oml-write-oml-read-oml-events-memories-and-the-journal).

### Check that history was not changed

**Designed** · not scheduled · an auditor

- **Goal:** verify that the signed history was not edited afterwards. The checking command is
  built; the server does not yet hand out the history it checks.
- **How it works:** [CLI reference](../cli/reference.md#oml-audit-evidence-export-and-chain-verification).

### One bundle of evidence

**Designed** · not scheduled · an auditor

- **Goal:** get one signed bundle answering who wrote, approved and read a memory, and when, with
  the history of who had access and how often the same people approved each other's work. Who
  read what is kept for a retention window the organization sets.
- **How it works:** [compliance evidence](compliance.md),
  [audit and lineage](../architecture/audit-and-lineage.md).

### Withdraw or erase a memory

**Designed** · not scheduled · an admin

- **Goal:** withdraw an approved memory from use while keeping its content; or erase its content
  permanently, with a record of every copy and what happened to it.
- **How it works:** [compliance evidence](compliance.md), [commitments](commitments.md).

### Retention and legal hold

**Planned** · not scheduled · an auditor, an admin

- **Goal:** age memories out on a schedule, and keep some under a legal hold.

### Offboard an organization

**Designed** · not scheduled · an operator

- **Goal:** suspend an organization's writes and erase its content, in one recorded ceremony.
- **How it works:** [runbooks](../operations/runbooks.md#tenant-offboarding).

### Analyze memory in an analytics store

**Designed** · not scheduled · an auditor, an analyst

- **Goal:** analyze memory and where it came from in an analytics store, where every figure traces
  back to its source.
- **How it works:** [storage](../architecture/storage.md).
