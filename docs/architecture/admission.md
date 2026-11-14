---
status: Stable
verified: 2026-11-14
answers: "how a write moves from capture through the scan, the hold and the review to a readable memory; what the scan vetoes and holds; who reviews; and what happens when no reviewer is available"
---

# How a write becomes readable

The four rules, R1 to R4, are declared once, in [the constitution](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0001-the-constitution.md). This page shows how admission enforces R2 and R3.

Every write crosses one pipeline before recall serves it to keys other than its author's:

> capture, scan, hold, non-author review, approval, journal entry, readable.

Recall serves a memory to other keys from its approval on, and approval does not change its rank. Two read paths reach a held memory before that: readers inside an [in-progress workarea](workareas.md#in-progress-workareas), where it is marked unreviewed, and an admin key's journaled [admin read](roles-and-keys.md#the-admin-read).

## The states of a memory

A memory is in one state at a time.

```mermaid
stateDiagram-v2
    [*] --> held : capture (scan passed)
    held --> promoted : approved by a reviewer who did not write it
    held --> rejected : rejected, with a reason
    promoted --> demoted : demoted, bytes kept (designed, not built)
```

| State | Who can read it |
|---|---|
| `held` | Its author; the reviewer it is served to; readers of an in-progress workarea that holds it, marked unreviewed; admin keys through the admin read |
| `promoted` | Every key with read access to the workarea (state after approval) |
| `rejected` | Its author, in its own listing |
| `demoted` | Not served to recall. Designed, not built: no verb demotes in this build |

The `erased` state exists in the API enum. This build has no erase verb.

## What happens to a write

1. **Capture.** A person runs `oml write`, or an agent calls `oml_write`. The server normalizes the text to Unicode NFC, then scans it. If the scan passes, the server commits one transaction. It holds a signed journal event, the memory row in state `held` and an outbox row. The write is durable once that transaction commits.
2. **Hold.** Recall shows the held memory to its author with `--include-unreviewed`, and to the readers of an in-progress workarea that holds it. Every other read path filters it out, except the review serve and the admin read.
3. **Review.** A reviewer key (role `dreamer`) claims the memory with `oml review next`. The reviewer reads it and approves or rejects it.
4. **Approval.** One approval from a reviewer who is not the author promotes the memory. The journal records the verdict and the review policy it was decided under. A supersession or a retirement passes the same pipeline. It changes the claim's current version only at approval.
5. **Release.** The outbox drains to the lake after approval. A lagging lake delays analytics. It does not delay visibility, because reads come from PostgreSQL.

For R2, the server checks the reviewing key and its person against the author and against every memory the author's `derives_from` names, including links still pending. If a memory's declared sources exclude every eligible reviewer, it stays held.

## The scan

The scan runs inside the write call, before the transaction. It returns before the call does. A veto refuses the write and journals nothing. The scan is a set of rules, not a model. The same input gives the same verdict, which is why a scan result carries no sampling caveat. It catches what its rules name and nothing else. Judgment belongs to review, so the scan can veto but never approve.

The rule set is `oml-scan/1.2` (`oml-scan/1.1` before the letters-only view was anchored to word edges, #226; `oml-scan/1` before the 2026-11-10 amendment of ADR-0065). Its findings fall into two actions:

| Action | Findings | Result |
|---|---|---|
| Veto | `secret.private_key`, `secret.api_token`, `secret.credential_pair`, `secret.assignment`, `format.scan_bound` (one field over 1 MiB) | The write is refused with `submission.scan_veto`. `details.finding` names the class. |
| Hold | `injection.override`, `injection.role`, `injection.tool_call`, `unicode.bidi`, `unicode.invisible`, `encoding.base64_blob` | The memory is held for a reviewer to judge. |

A held version stores every finding behind its verdict, one row each in its findings list: the class, the rule, the rule set, the field (`/body`, `/title`, or `/body/` and a decoded view's path), the line, the view the rule read (`raw`, `clean`, `spaced`, `letters`), the byte range in that view, and who recorded it (`admission`, or a recording `rescan`). Never the matched text. The review card and the admin page's bulk table show the list, and `POST /v2/review/next` and `GET /v2/review/held` serve it as `scan_findings`. Design record: [ADR-0074](../adr/ADR-0074-the-scan-names-a-threat-class-for-every-finding-refuses-only-what-can-never-be-safe-holds-or-annotates-the-rest-and-recall-serves-every-hit-as-data-with-its-findings.md) d23.

The `pii.email` and `pii.phone` rules exist and are off in this build; no setting turns them on. A rule pass takes about 4 µs for a 124-byte chat turn and 26 µs for a 1.5 KB abstract. It takes 13 ms for a mebibyte of base64.

A claim in a format other than prose is scanned twice over, never less than prose. The adapter accepts the source or refuses it by name (`submission.format_invalid`) before the scan. The scan then runs over the source as for prose, and over each decoded string the adapter hands it: the scalars of a JSON or YAML document after escapes, quoting, folding and block scalars are decoded, because a rule that reads only bytes does not see `"\u002d-----BEGIN PRIVATE KEY"`. A finding on a decoded string names its key path and never the value. A value under a key whose last segment is `password`, `passwd`, `secret`, `token`, `api_key`, `apikey`, `private_key`, `client_secret`, `authorization` or `credentials`, or ends `_secret`, `_token` or `_key`, is vetoed as `secret.keyed_value` unless it is empty or a placeholder (`<…>`, `${…}`, `{{…}}`, `***`, `changeme`, `example`, `redacted`). A format can add a finding and never remove one. The title is scanned as free text that reaches the journal. Design record: [ADR-0056](../adr/ADR-0056-a-claim-version-names-its-format-the-signature-covers-its-source-and-adapters-compiled-into-the-server-derive-what-search-reads.md).

`oml admin rescan` reports what the current rule set would refuse or hold in memories already stored. It changes nothing.

## Who reviews

OML runs no model. Reviewers are agents or people that hold reviewer keys and use the same HTTP API as every other caller.

- A person uses `oml review next`, `oml review approve` and `oml review reject`.
- A crew's agent calls the same routes. See [reviewer crews](../api/reviewer-crews.md).

Every workarea uses one fixed policy: one approving verdict from a reviewer key ratifies. A policy with several approvals, a required human verdict or a held-item expiry is not available.

A verdict counts only against a serve. The server hands one held memory to one reviewer key, and the verdict applies to that memory. OML checks independence at approval. It cannot make an external reviewer diligent. The quality and speed of review depend on the reviewers, not on OML.

## When no reviewer is available

Nothing promotes. A workarea with no reviewer key that is allowed to judge its memories accumulates held memories until one exists. The server does not relax the rule, widen the roles or approve on a timeout. An under-staffed review queue looks like an outage for new knowledge. The alternative, failing open, turns a capacity problem into a trust problem.

A failure inside the pipeline is typed and names its rule ([errors](../api/errors.md)). An unclassifiable failure refuses the write rather than admitting it unscanned.

How fast review goes depends on the reviewers; OML has no measured figure.
