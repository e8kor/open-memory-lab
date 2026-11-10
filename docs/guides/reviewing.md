---
status: Stable
verified: 2026-11-06
answers: "how a person reviews held memories: getting and storing a reviewer key, taking the next held memory, reading it and its sources, what to check, approving and rejecting, supersessions and steering, and a worked session"
---

# Review held memories

Every write lands held and becomes readable to others when an identity that did not write it
approves it. Why OML works this way is in [why a write is held](../concepts/why-a-write-is-held.md).
This page is the reviewer's routine. Agent crews review over HTTP instead: see
[Integrate a reviewer crew over HTTP](../api/reviewer-crews.md).

## Before you start

You need a reviewer key (role `dreamer`). A tenant admin issues it. You cannot issue your own. The
key needs two grants on the workarea you review: `dreamer`, to take and judge held memories, and
`reader`, to read their sources. Without the reader grant, a held memory is served with an empty
`derives_from` and no replaced version.

```sh
# the admin, with the admin pair stored
oml keys issue alice-review --role dreamer --held-by human --principal people/alice \
  --grant crew:dreamer --grant crew:reader --default-workarea crew
```

The command prints `key issued — SHOWN ONCE` and the value once. The server keeps only its hash.
Give each reviewer their own key and principal, because a shared key makes two people one. An admin
key is never a reviewer key, and a `held_by: service` key is refused every review verb with
`access.verb_ceiling`. `oml quickstart` issues one reviewer key, `quickstart-reviewer`, into
`<dir>/keys/reviewer` ([run your own server](../start/run-your-own.md)); it holds the `dreamer` grant only, so a held memory it serves has no sources. The admin's side is in
[Administer an OML server](administering.md).

Store the key once. `oml login` reads it without echo, checks it, and writes the URL and key to the
owner-only credentials file:

```sh
oml login https://oml.example.com
oml doctor
```

If you also run an agent key on this machine, keep the reviewer pair in a second credentials file
and select it with `OML_CLI_CREDENTIALS`.

## Take the next held memory

```sh
oml review next
```

The command serves one held memory and claims it for your key, oldest first. It serves only
workareas where your key holds `dreamer`. It skips any memory whose lineage names your key or
principal. An empty queue prints `nothing in scope waits — the queue is empty (204).`

The output shows the artifact id and version, kind, writer, claim key and text, then the
`serve_id`. Add `--json` to see `derives_from`, `replaces`, `retirement` and, for a supersession,
the full version it replaces. Read each source with `oml read <artifact_id>`. A source you cannot
read is a reason to reject, not to guess.

To read the queue without claiming anything, run `oml review list` (a reviewer key held by a person).

## Reviewing on the user page

Signed in at `/web`, the **Review** tab serves the next held memory to your own reviewer key,
`<user>-review`, as `oml review next` does. **Memories** lists what your agents wrote, every
state, newest first, 50 a page: filter by state, workarea, origin, author key or text. A held
card says it is held until someone else reviews it. **Atlas** shows your workareas as a map
sized by readable count, with the same numbers in a table below it. **Workareas** gives each workarea one dropdown
of what your reviewer key may do there now: propose the next status, ratify another key's
proposal, put it in progress, or commit it, which waits while any memory in it is held. Apply
shows what will happen before anything is recorded
([ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d22–d30).

## Check the memory

Approve only what you would want another agent to act on.

- **One checkable claim.** One fact or decision per memory, specific enough to be wrong. A
  conclusion names its sources in `derives_from`, and each source supports it.
- **A source and a date.** A quote reference, a commit, a document or an "as of". A memory with no
  anchor ages without anyone noticing.
- **No secrets.** No keys, passwords, tokens or personal data. A rejected memory keeps its bytes,
  so tell an admin about a secret instead of only rejecting it.
- **Scanner findings.** The `oml-scan/1.1` rule set vetoes leaked keys before anything is stored and
  holds what a reviewer should judge. `oml admin rescan` reports what it finds in memories already
  stored. It does not replace reading.

## Approve or reject

```sh
oml review approve --note "matches quote ref AM-7741"
oml review reject --reason "no source and no date; resubmit with the quote reference"
```

Without a `serve_id`, the verdict applies to the one memory your key has claimed. If you ran
`oml review next` more than once, name it: `oml review approve <serve_id>`. Otherwise the verdict
is refused with `request.validation` naming `serve_id`. `--reason` is required to reject. `--note`
is optional and is journaled as the verdict's reason. Both answer
`verdict recorded — <artifact_id> is <state> (journal seq <n>)`.

A rejection is journaled, never silent. The author sees `rejected` in its own listing
(`oml read --state rejected`). The reason is in the journal, and `oml read` does not show it.
Reopening a rejection is not available.

After approval the memory is `promoted`. Any key with `reader` on its workarea or an ancestor finds
it with `oml read --query` or `oml_read`. Its rank does not change.

## Review a supersession or steering

A **supersession** is a new version of a promoted memory, written with `--replaces`. The old
version stays the one everyone reads until you approve the new one. Read the two together with
`--json`. With `retirement: true`, the write proposes retiring the memory, and its text says why.
Approving it stops recall serving the memory, and every version stays readable as history. If
another version won the tip first, the approval is refused with `submission.tip_conflict` and
nothing is journaled. The serve can still carry a reject.

A **steering** memory has kind `steering`. Once approved, every recall in that workarea and below
returns it first, unranked, and no budget cuts it. Read
steering more strictly than a fact, because it shapes every later session in the subtree.

## A worked session

```sh
$ oml review next
serving 1 held item

  artifact  4f9c2a7e-0b31-4d8e-9a55-1c6f3e2d8b10 @ v1   kind: memory   writer: crew-procure/agent-4
  claim_key supplier-quote-am-7741
  body      | Accra Metals quoted 412 USD/t for Q1-2027 delivery, valid until 2026-11-15. Quote ref AM-7741.

  serve_id: srv_4f9c2a7e0b314d8e9a551c6f3e2d8b10  (expires 2026-11-04T10:15:00Z)
  verdict with: oml review approve [--note …] | oml review reject --reason …

$ oml review next --json          # derives_from and replaces of the same claimed item
$ oml read 7a1d0c3b-5e62-4f19-8b07-2d4e6f8a9c31   # each source it names
$ oml review approve --note "dated, sourced, one claim"
verdict recorded — 4f9c2a7e-0b31-4d8e-9a55-1c6f3e2d8b10 is promoted (journal seq 212)

$ oml review next
nothing in scope waits — the queue is empty (204).
```

The identifiers are illustrative. Every workarea has the same policy: one approving verdict
promotes. If a step fails, see [client troubleshooting](../help/troubleshooting-client.md).
