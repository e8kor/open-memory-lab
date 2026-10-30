<!--
id: cli/sessions
tier: engineering
status: design draft for review · decided, not built
answers: what the administrative ceremonies are intended to feel like, as hand-written mockup transcripts — nothing on the page has run
-->

# CLI sessions

**Status:** design draft for review · decided, not built · **every transcript below is a mockup**

Nothing on this page has run. These are design mockups: hand-written transcripts showing what the
ceremonies in the [CLI reference](reference.md) are intended to feel like, so reviewers can judge
the ceremonies before any code exists. Key values, ids, hashes, timestamps and exact output layout
are illustrative; the *rules* each transcript exercises — show-once issuance, serve binding,
two-party ratification, dry-run previews — are the decided design, and each transcript names the
page that owns its rules. Every refusal code shown is a typed error from the
[error taxonomy](../api/errors.md) and every `journal seq` line names an event from the
[event catalog](../data/event-catalog.md) — those identifiers are the contract, not illustration.
The human layout shown here is not a contract; `--json` output is
([conventions](reference.md#conventions)).

The tenant in every transcript is `aurora`; its crews and key values are invented.

## 1. Bootstrap: the operator key and the first tenant

At first start, a Helm job mints a single-use, expiring **enrollment secret**, stored as a
Kubernetes Secret; the job's logs carry progress only — no key or secret material ever transits a
pod log ([deployment](../operations/deployment.md)). The operator exchanges that secret over TLS
for the root **operator** key with `oml bootstrap claim`
([reference](reference.md#oml-bootstrap-claiming-the-root-operator-key)): the key value is shown
once in the operator's terminal and journalled as issued-without-value
([roles and keys](../architecture/roles-and-keys.md)). The operator creates tenants and issues
each tenant's first admin key — and can do nothing else: no reads, no writes, no verdicts.

Provider-level acts — operator key issuance, tenant creation — are journalled in the reserved
**provider tenant's** chain, created at deployment genesis; every other chain is per tenant
([storage](../architecture/storage.md)). A new tenant's chain opens with a `tenant.genesis` event
rooting it by reference to the provider-chain `tenant.created` event that created it — a
reference, not a second numbering ([event catalog](../data/event-catalog.md)). The transcripts
label the chain only in this section: from section 2 on, every `journal seq` is tenant aurora's
chain.

```console
$ kubectl logs job/oml-bootstrap -n oml
oml-bootstrap: provider tenant chain created (deployment signing key dk_5a21, ed25519)
oml-bootstrap: enrollment secret minted — stored as Kubernetes Secret oml/oml-enrollment
oml-bootstrap: single-use; expires 2026-10-30T09:00:00Z; consumed on first claim
oml-bootstrap: this log carries progress only — no key or secret material appears in it
oml-bootstrap: next step: oml bootstrap claim

$ kubectl -n oml get secret oml-enrollment -o jsonpath='{.data.token}' | base64 -d \
    | oml bootstrap claim --url https://oml.internal.example --name root-operator --token-stdin
enrollment secret accepted — consumed (single-use; a second claim is refused)
root operator key — SHOWN ONCE, NOT RECOVERABLE, NOT JOURNALLED AS VALUE

    operator key    : root-operator (key id key_op_000001)
    operator value  : oml_zj4Qm9tW8xKpN2rLcVbH7eGd1sYfA0uJn5iXqB3wTk6

journal (provider) seq 1: key.issued {id: key_op_000001, name: root-operator, role: operator, value_disclosed: false}
store this value now; the server keeps only its hash.

$ oml context add root --url https://oml.internal.example --key-stdin
reading key from stdin… ok
context "root" added (key id key_op_000001, role operator)

$ oml tenants create aurora
journal (provider) seq 2: tenant.created {tenant_id: t_9d41, name: aurora, created_by_key_id: key_op_000001}
journal (aurora) seq 1: tenant.genesis {tenant_id: t_9d41, name: aurora, provider_seq: 2, created_event_id: e_30aa}
tenant aurora created — chain opened, rooted at provider seq 2;
universe workarea wa_000001 (status: committed, permanent)

$ oml tenants issue-admin aurora aurora-admin --label "aurora founding admin"
issuing first admin key for tenant aurora — SHOWN ONCE

    key    : aurora-admin (key id key_a1f3e2)
    value  : oml_Hq7dPw3nXsYvK0mBtRj5E8cUa2LgZ1oD6fNyWm4SkP9

journal (aurora) seq 2: key.issued {id: key_a1f3e2, name: aurora-admin, role: admin, value_disclosed: false}

$ oml tenants issue-admin aurora aurora-admin-2 --label "second attempt"
refused: access.issuer_ceiling — aurora already has its first admin key; an operator
key issues nothing beyond a tenant's first admin key. Further keys come from that
admin under the issuer ceiling.
```

The last refusal is the shape of the operator role: a bootstrap channel, not a standing back door.

## 2. Issuing a crew's keys

The aurora admin provisions the `corrosion` crew: a crew record, a workarea, writer keys for its
working agents, and dreamer keys for the crew that reviews it. Grants bind to node ids; names are
how a person types them, and the server resolves each name to its id
([names](../api/http-api.md#names-and-ids)). An issuance journals `key.issued` without the value,
then one `grant.issued` per grant — and the CLI sends a reader grant beside every writer grant
unless told `--write-only` ([CLI reference](reference.md#oml-keys-issuance-revocation-rotation)).

```console
$ oml context use aurora-admin
$ oml crews create corrosion --external-manager "crewd.example/corrosion"
created crew corrosion (crew_3b17)
journal seq 811: crew.created

$ oml workareas create research corrosion --crew corrosion
created corrosion (wa_00004e) universe.research.corrosion (status: development)
journal seq 812: workarea.create

$ oml keys issue corrosion-agent-1 --role writer \
    --grant corrosion:writer --default-workarea corrosion \
    --crew corrosion --expires 90d
adding corrosion:reader beside corrosion:writer (pass --write-only to issue writer grants alone)
key issued — SHOWN ONCE

    key    : corrosion-agent-1 (key id key_9c44b7)
    value  : oml_Wn2gLx8cQe5rTv0zYhDkJ7bMa3SfU1iGp9eC6nRw4Xj
    default workarea : corrosion (wa_00004e, universe.research.corrosion) — provider-ring (/v1)
                       requests from this key target it when they name none

journal seq 813: key.issued {id: key_9c44b7, name: corrosion-agent-1, role: writer, value_disclosed: false, default_workarea_id: wa_00004e, crew_id: crew_3b17}
journal seq 814: grant.issued {key: key_9c44b7, wa_00004e: writer}
journal seq 815: grant.issued {key: key_9c44b7, wa_00004e: reader}

$ oml keys issue corrosion-reviewer-1 --role dreamer \
    --grant corrosion:dreamer --expires 90d
key issued — SHOWN ONCE

    key    : corrosion-reviewer-1 (key id key_51d0ac)
    value  : oml_Fb6mZq1jUs4xNc9pKtRvA3eWd8HgL2oY7sBk5TmQ0Jx

journal seq 816: key.issued {id: key_51d0ac, name: corrosion-reviewer-1, role: dreamer, value_disclosed: false}
journal seq 817: grant.issued {key: key_51d0ac, wa_00004e: dreamer}

$ oml context use corrosion-reviewer-1
$ oml keys issue corrosion-agent-2 --role writer --grant corrosion:writer
refused: access.verb_ceiling — key_51d0ac (dreamer) is an agent key calling an
admin verb (keys issue); issuance belongs to admin and operator keys (R1), and a
key is in any case issued with a role no higher than the issuer's.

$ oml context use aurora-admin
$ oml keys list --expiring 120d
NAME                  KEY ID      ROLE     EXPIRES     GRANTS
corrosion-agent-1     key_9c44b7  writer   2027-01-27  corrosion:writer, corrosion:reader
corrosion-reviewer-1  key_51d0ac  dreamer  2027-01-27  corrosion:dreamer

$ eval "$(oml keys issue-agent corrosion-agent-3 --principal crew-corrosion/agent-3 --workarea corrosion)"
key corrosion-agent-3 (key id key_7e21d0) issued — value exported as OML_API_KEY, printed nowhere else
journal seq 818: key.issued {id: key_7e21d0, name: corrosion-agent-3, role: writer, value_disclosed: false, default_workarea_id: wa_00004e}
journal seq 819: grant.issued {key: key_7e21d0, wa_00004e: writer}
journal seq 820: grant.issued {key: key_7e21d0, wa_00004e: reader}
$ echo "$OML_URL"
https://oml.internal.example
```

A writer that must also recall holds explicit reader scope — roles never imply one another. The
writer key's `--default-workarea` is a journalled issuance attribute like its grants: a
provider-ring (`/v1`) request from key_9c44b7 that names no workarea targets wa_00004e
([ADR-0022](../adr/ADR-0022-two-ring-api.md)). The
key values above appeared exactly once; `oml keys list` and the journal know only names, ids and
hashes. `oml keys issue-agent` printed its value only into the two export lines the `eval`
consumed, which the [agent plugin](../api/mcp-plugin.md#wiring-it-into-your-agent) reads.

## 3. A review session

A human dreamer drains the corrosion queue. The serve binds (reviewer key, artifact version,
attachment digest set); the verdict consumes it
([serve mechanics](reference.md#oml-review-the-review-ceremony)). Promotion is a three-event
ceremony in the journal: the verdict (`review.approve`), the act that finds the policy satisfied
(`review.ratify`), and the visibility change (`lifecycle.promote`) — the
[event catalog](../data/event-catalog.md) owns each shape. The second item shows the
lineage-closure refusal: the reviewer proposed an edit in this row's ancestry, so the server will
not serve it to them.

```console
$ oml context use corrosion-reviewer-1
$ oml review next --workarea corrosion
serving 1 of 3 held items

  artifact  art_02b8e1 @ v1   kind: memory   writer: key_9c44b7 (corrosion-agent-1)
  submitted 2026-10-29T08:14:02Z   scan: pass (deterministic scan v1)
  claim_key corrosion/duplex-2205-pitting

  body (text/markdown, 1.2 KiB):
  | Duplex 2205 coupons at 60 °C seawater showed pitting onset at ~18 d,
  | contradicting the 30 d figure in art_019aa4. Full dataset attached.

  attachments (served in full — a verdict without them does not count):
    pitting-timeseries.csv   sha256:9f27…c4a1   42 KiB   -> ./review/art_02b8e1/pitting-timeseries.csv  [digest verified]
    coupon-photos.pdf        sha256:77b0…e9d3  1.8 MiB   -> ./review/art_02b8e1/coupon-photos.pdf       [digest verified]

  serve_id: srv_f3a91c  (bound to key_51d0ac + art_02b8e1@v1 + 2 attachment digests; expires 00:30:00)

$ oml review approve srv_f3a91c --note "dataset supports onset claim; contradicts art_019aa4, flagged"
verdict recorded — journal seq 901: review.approve
  {artifact: art_02b8e1@v1, serve: srv_f3a91c, served_digests: [9f27…c4a1, 77b0…e9d3]}
journal seq 902: review.ratify {subject: art_02b8e1@v1, verdicts: [seq 901],
  policy_snapshot: {k: 1, required_roles: [dreamer]}, lineage_closure_digest: 41cc…09f7}
journal seq 903: lifecycle.promote {artifact: art_02b8e1@v1, previous_tip_version: none}
art_02b8e1@v1 promoted; serve srv_f3a91c consumed.

$ oml review next --workarea corrosion
refused for next item art_031c77@v2: review.author_in_lineage
  your key key_51d0ac proposed art_031c77@v2 (edit-as-proposal, derives_from art_031c77@v1);
  the reviewer is never the author — computed over the lineage closure at promotion time.
skipping to next eligible item… none eligible for this key.
2 items remain held; both carry key_51d0ac in their lineage closure and need
a reviewer outside it.
```

The refusal is computed per row, not per key pair: `key_51d0ac` is a perfectly good reviewer for
everything it did not touch. The remaining items wait — the gate fails closed rather than lowering
the bar ([admission](../architecture/admission.md)).

## 4. Freezing a workarea

Status transitions are forward-only — development → committed → validated, one rung at a time —
and two-party: proposed by one admin or dreamer (journalled `workarea.status_proposed`), ratified
by a second, non-proposing one (journalled `workarea.status_transition` with the policy snapshot)
([workareas](../architecture/workareas.md)). The self-ratification attempt here is refused by R2;
the `validated` proposal is refused while a child is not yet validated.

```console
$ oml context use aurora-admin
$ oml workareas status propose corrosion --to committed
transition trn_0a40 proposed (development -> committed); awaiting a second ratifier.
journal seq 1198: workarea.status_proposed {wa_00004e: development -> committed, proposer: key_a1f3e2}

$ oml workareas status ratify trn_0a40
refused: review.ratifier_is_proposer — a transition is ratified by a second,
non-proposing dreamer or admin (R2 extends to transitions).

$ oml context use corrosion-reviewer-1
$ oml workareas status ratify trn_0a40
journal seq 1203: workarea.status_transition {wa_00004e: development -> committed,
  proposer: key_a1f3e2, ratifier: key_51d0ac, policy_snapshot: pol_77e2}

$ oml context use aurora-admin
$ oml workareas status propose corrosion --to validated
refused: workarea.transition_refused
  scratch (wa_000061, universe.research.corrosion.scratch) is development
an area may only become validated when all its children are validated.

$ # …scratch walks the same two rungs under distinct principals
$ #   (trn_0a44: development -> committed; trn_0a51: committed -> validated), then:
$ oml workareas status propose corrosion --to validated
transition trn_0a58 proposed (committed -> validated); awaiting a second ratifier.
journal seq 1236: workarea.status_proposed {wa_00004e: committed -> validated, proposer: key_a1f3e2}

$ oml context use corrosion-reviewer-1
$ oml workareas status ratify trn_0a58
journal seq 1240: workarea.status_transition {wa_00004e: committed -> validated, …}

$ oml context use aurora-admin
$ oml workareas create corrosion post-mortem
refused: workarea.frozen — a validated area accepts no new children.
remaining mutations on wa_00004e: demote, erase, crew-metadata update.
```

## 5. Running and ending a simulation

Simulation writes are ordinary journalled events carrying indelible `provenance: simulation`,
visible only to the audience, reviewed only by dreamers in it; ending the simulation demotes its
rows and re-reviews tainted claims ([simulations](../architecture/simulations.md)). `end` is a
mass-reach act, so it defaults to `--dry-run`.

```console
$ oml context use aurora-admin
$ oml simulations open --audience corrosion-agent-1,corrosion-reviewer-1 --label "what-if: 40 °C baseline"
simulation sim_0007 opened (state: open, audience: 2 keys)
note: audience contains 1 dreamer key — promotion inside the simulation is possible.
journal seq 1302: simulation.opened

  …the crew writes and reviews inside the fence over the agent API;
  every such row carries provenance: simulation + sim_0007 indelibly…

$ oml simulations show sim_0007
sim_0007  open  audience: key_9c44b7, key_51d0ac
  rows written: 41 (held 3, promoted 38, all provenance: simulation)
  sim-derived taints outside the simulation: 2 claims carry sim_0007 in lineage
  containment: rows served only to audience keys; excluded from shared FTS/vector
  indexes; refused promotion into validated areas.

$ oml simulations end sim_0007
dry-run (default) — ending sim_0007 would:
  demote 38 promoted simulation rows
  discard 3 held simulation rows
  re-enqueue 2 sim-derived claims for re-review: art_0412aa@v1, art_0415c0@v3
nothing journalled. re-run with --apply to act.

$ oml simulations end sim_0007 --apply
journal seq 1346: simulation.ended {simulation_id: sim_0007, disposition: demote}
demoting 38 promoted rows — journal seqs 1347..1384: lifecycle.demote
  {reason: simulation_ended, cause_event_id: seq 1346} per row
3 held rows discarded at admission; 2 sim-derived claims re-enter held for re-review.
sim_0007 ended. tainted claims re-enter held; their reviewers see the taint.
```

## 6. Evidence export and chain verification

The export produces the evidence an auditor asks for; the chain verification proves the journal
was not rewritten — and, because journal payloads are digest-only, it still verifies after
crypto-shredding ([audit and lineage](../architecture/audit-and-lineage.md)). The export is an
async, journalled job: `export.requested` is written when the job is requested, and the bundle
exists only once its chain verification finishes — fetched earlier it is `export.not_ready`.

```console
$ oml audit export --out ./evidence-2026-10-29
journal seq 1390: export.requested {export_id: exp_0003, kind: evidence, from_seq: 0,
  include_simulations: false, requested_by_key_id: key_a1f3e2}
export exp_0003 queued (async; the bundle exists when chain verification finishes)…
  decision trails    1,482 memories (per-memory: submission, scan, serves, verdicts, policy snapshots)
  chain verification seq 1..1390 — recomputed, signatures valid, head hash 7c19…40be
  grants history     61 grant/deny/epoch events
  read log           retention window 90d (tenant policy): 12,044 events, batch digests anchored at journal seqs
  reciprocal approvals  per key pair; top pair (key_51d0ac, key_60b311): 9 mutual — surfaced, not judged
  simulations        excluded by default; 1 ended simulation listed by id only (sim_0007)
bundle written: ./evidence-2026-10-29/aurora.oml-evidence.tar (signed, 18.3 MiB)

$ oml audit verify-chain --genesis
verifying tenant aurora from genesis…
  seq 1: tenant.genesis — references provider chain seq 2 (tenant.created): ok
  seq 1..1390: prev_hash links ok, ed25519 signatures ok (deployment key dk_5a21)
  erased artifacts encountered: 4 — digest envelopes verify; bodies unrecoverable (as designed)
chain verified. head: seq 1390, hash 7c19…40be
```

## 7. Banning a source

Bans are journalled, checked at admission, and previewed before they act — `--dry-run` is the
default and shows reach ([grants, denials, bans](reference.md#oml-grants-oml-deny-oml-bans-scope-and-refusal)).

```console
$ oml context use aurora-admin
$ oml bans add --source "connector:rss/feedpulse.example" \
    --reason "feed republishing fabricated abstracts"
dry-run (default) — banning source connector:rss/feedpulse.example would:
  refuse at admission    all future submissions naming this source
  held items affected    7 held submissions would be refused at their next admission check
  promoted items         23 promoted claims cite this source — NOT touched by a ban
                         (a ban gates admission; demotion of existing claims is a
                          separate, per-claim journalled act)
  keys affected          0 keys are revoked — bans act on the source, not the key
nothing journalled. re-run with --apply to act.

$ oml bans add --source "connector:rss/feedpulse.example" \
    --reason "feed republishing fabricated abstracts" --apply
journal seq 1401: ban.imposed {ban_id: ban_0012, subject_kind: source,
  subject: connector:rss/feedpulse.example, banned_by: key_a1f3e2}
ban ban_0012 active; admission now refuses this source.

$ oml bans list
BAN ID    TARGET                                 BY          STATE   LIFTED BY
ban_0012  source connector:rss/feedpulse.example key_a1f3e2  active  —
```

The preview's sharpest line is the one about promoted claims: a ban is an admission gate, not a
retroactive purge. What was already promoted stays visible until someone demotes it, claim by
claim, in the journal — no silent deletion (R4).
