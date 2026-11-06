<!--
id: guides/reviewing
tier: engineering
status: current
answers: how a person reviews held memories: what held means and why the reviewer is never the author, getting and storing a reviewer key, taking the next held memory, reading it and its sources, approving and rejecting, what to check, supersessions and steering, crews and review policy, what approval changes, and what a reviewer can never do
-->

# Guide: reviewing memories

This page is about reviewing what agents write into OML. Reviewing the design documents is a
different task, covered by the review guide.

## What a held memory is

Every write lands **held**: journalled, scanned in the same call, and readable by its author
alone. It becomes readable to others exactly once, when an identity that did not write it
approves it. Two rules of the constitution
([ADR-0001](../adr/ADR-0001-the-constitution.md);
[invariant map](../architecture/overview.md#5-the-invariant-map)) make that work:

- **R2, the reviewer is never the author.** The server checks the reviewer's key and principal
  against the author and the memory's whole lineage closure, at verdict time. `oml review next`
  never serves you a memory you wrote or one derived from yours.
- **R3, held rows are visible to their author only.** No search, fetch or listing returns a held
  row to any other key. Your approval is the one act that changes who can read it; it never
  changes how it ranks.

## Getting a reviewer key

A tenant admin issues it; you cannot issue your own. The key's role is `dreamer`, and it needs two
grants on the workarea you review: `dreamer` to take and judge held memories, and `reader` to read
their sources. Without the reader grant a held memory is served with an empty `derives_from` and
no replaced version, because the server shows a reviewer only what that key may read.

```sh
# the admin, with the admin pair stored
oml keys issue alice-review --role dreamer --held-by human --principal people/alice \
  --grant crew:dreamer --grant crew:reader --default-workarea crew
```

It prints `key issued — SHOWN ONCE` and the value once; the server keeps only its hash. Give each
reviewer their own key and principal: R2 and the reciprocal-approval statistics key on the
principal, and a shared key makes two people one. An admin key is never a reviewer key: an admin
role holds no dreamer grant (`access.role_exclusive`), and a `held_by: service` key is refused
every review verb with `access.verb_ceiling`. The admin's side is in
[administering a server](administering.md) and the
[CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation); `oml quickstart`
issues one reviewer key, `quickstart-reviewer`, into `<dir>/keys/reviewer`, with the `dreamer`
grant alone ([self-hosting](../start/run-your-own.md)).

Store the key once. `oml login` reads it without echo, checks it, and writes the URL and key to
the owner-only credentials file
([login](../cli/reference.md#oml-login-and-oml-doctor-storing-and-checking-the-pair)):

```sh
oml login https://oml.example.com
oml doctor
```

If you also run an agent key on this machine, keep the reviewer pair in a second credentials file
and select it with `OML_CLI_CREDENTIALS`.

## Taking the next held memory

`oml review next` serves one held memory and claims it for your key; the oldest eligible memory
comes first. It serves only memories in workareas where your key holds `dreamer`, and skips any
whose lineage closure names your key or principal. An empty queue prints
`nothing in scope waits — the queue is empty (204).`

The plain output shows the artifact id and version, kind, writer, claim key and text, then the
`serve_id`. Add `--json` to see everything served: `derives_from`, `replaces`, `retirement` and,
for a supersession, the full version it replaces under `replaces` at the top level. Read each
source with `oml read <artifact_id>`. A source you cannot read is a reason to reject, not to guess.

## What to check

Approve only what you would want another agent to act on:

- **One checkable claim.** One fact or decision per memory, specific enough to be wrong. A
  conclusion names what it came from in `derives_from`, and each source supports it.
- **A source and a date.** Where it came from and when it was true: a quote reference, a commit,
  a document, an "as of". A claim with no anchor ages without anyone noticing.
- **No secrets.** No keys, passwords, tokens or personal data. A rejected memory keeps its bytes;
  tell an admin about a secret instead of only rejecting it.
- **Scanner findings.** Since v0.6.0 the scan is the `oml-scan/1` rule set
  ([ADR-0065](../adr/ADR-0065-the-scan-is-a-versioned-compiled-in-rule-set-that-vetoes-leaked-keys-and-holds-what-a-reviewer-should-judge.md)):
  it vetoes leaked keys before anything is stored and holds what a reviewer should judge, and
  `oml admin rescan` reports what it finds in memories already stored. It is no substitute for
  reading.

## Approving and rejecting

```sh
oml review approve --note "matches quote ref AM-7741"
oml review reject --reason "no source and no date; resubmit with the quote reference"
```

Without a `serve_id` the verdict applies to the one memory your key has claimed. If you ran
`oml review next` more than once and hold several, name it: `oml review approve <serve_id>`;
otherwise the verdict is refused with `request.validation` naming `serve_id`. `--reason` is
required to reject; `--note` is optional on approve and is journalled as the verdict's reason.
Both answer `verdict recorded — <artifact_id> is <state> (journal seq <n>)`.

A rejection is journalled, never silent. The author sees `rejected` in its own listing
(`oml read --state rejected`); the reason is in the journal, and `oml read` does not show it.
Reopening a rejection is a designed verb, not in v0.4.0
([admission](../architecture/admission.md#reopening-a-rejection)).

## Supersessions and steering

A **supersession** is a new version of a promoted memory, written with `--replaces`. It is held
like any write, and the old version stays the one everyone reads until you approve the new one.
Read it against what it replaces (`--json` shows both). With `retirement: true` it proposes
retiring the claim, and its text says why; approving it stops recall serving the claim, while
every version stays readable as history. If another version won the tip first, the approval is
refused with `submission.tip_conflict`, nothing is journalled, and the serve can still carry a
reject.

A **steering** memory has kind `steering`: a standing instruction. Once approved, every recall in
that workarea and below returns it first, unranked, ahead of all hits, and no budget cuts it
([concepts](../concepts.md#steering)). Read steering more strictly than a fact: it shapes every
later session in the subtree.

## Reviewer crews and review policy

The designed review capacity is crews of agents holding dreamer keys over plain HTTP, the same
`POST /v2/review/next` and `POST /v2/review/verdicts` the CLI calls; the CLI is the human path
([reviewer crews](../api/reviewer-crews.md)). A crew member is an ordinary key: one key per member,
never an admin key.

In v0.4.0 every workarea has the same fixed policy: one approving `dreamer` verdict ratifies,
journalled with the snapshot `{"k": 1, "required_roles": ["dreamer"]}`. Per-workarea policy
objects (`k > 1`, `human_required_classes`, held-item expiry), the queue counts, the `workarea`
filter on `review next`, attachments and serve expiry are designed and not built
([review policy objects](../architecture/admission.md#review-policy-objects);
[k > 1 and human-required verdicts](../api/reviewer-crews.md#k-1-and-human-required-verdicts)).
The serve is the claimed queue row; binding it to a record of what was shown is
#130.

## After approval

The memory is **promoted**: any key with `reader` on its workarea or an ancestor finds it with
`oml read --query` or the agent's `oml_read`, and fetches it by id. The writer sees `promoted` on
its own listing. Nothing about its rank changed; it became visible.

## What a reviewer can never do

A reviewer key promotes or rejects one served memory at a time. It never deletes: there is no
`oml delete` and no delete endpoint, and R4 forbids silent deletion
([verbs that do not exist](../cli/reference.md#verbs-that-do-not-exist)). Taking a promoted
memory out of service is an admin's journalled demotion, and destroying its bytes is erasure by
crypto-shredding; neither is in the v0.4.0 CLI. A reviewer also cannot judge its own writes or approve from the MCP tools: the agent's tools
carry no verdict.

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

The identifiers are illustrative. `oml quickstart`'s self-check runs the same `review next` and
`review approve` end to end ([run your own server](../start/run-your-own.md)).
