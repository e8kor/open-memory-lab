# CLI sessions

**Status:** design draft for review · decided, not built · **every transcript below is a mockup**

Nothing on this page has run. These are design mockups: hand-written transcripts showing what the
ceremonies in the [CLI reference](reference.md) are intended to feel like, so reviewers can judge
the ceremonies before any code exists. Key values, ids, hashes, timestamps and exact output layout
are illustrative; the *rules* each transcript exercises — show-once issuance, serve binding,
two-party ratification, dry-run previews — are the decided design, and each transcript names the
page that owns its rules. The human layout shown here is not a contract; `--json` output is
([conventions](reference.md#conventions)).

The tenant in every transcript is `aurora`; its crews and key values are invented.

## 1. Bootstrap: the operator key and the first tenant

At first start, a Helm job mints a show-once root **operator** key and journals the issuance
*without the value* ([roles and keys](../architecture/roles-and-keys.md)). The operator creates
tenants and issues each tenant's first admin key — and can do nothing else: no reads, no writes,
no verdicts.

```console
$ kubectl logs job/oml-bootstrap -n oml
oml-server: journal genesis written (deployment key dk_5a21, chain algorithm ed25519)
oml-server: operator key minted — SHOWN ONCE, NOT RECOVERABLE, NOT JOURNALLED AS VALUE

    operator key id : key_op_000001
    operator key    : oml_op_EXAMPLEVALUE_zj4Qm9tW8xKpN2rLcVbH

oml-server: journal seq 1: key.issued {id: key_op_000001, role: operator, value: not recorded}
oml-server: store this value now; the server keeps only its hash.

$ oml context add root --url https://oml.internal.example --key-stdin
reading key from stdin… ok
context "root" added (key id key_op_000001, role operator)

$ oml tenants create aurora
journal seq 2: tenant.created {tenant: aurora}
tenant aurora created — universe workarea wa_000001 (status: committed, permanent)

$ oml tenants issue-admin aurora --label "aurora founding admin"
issuing first admin key for tenant aurora — SHOWN ONCE

    key id : key_a1f3e2
    key    : oml_sk_EXAMPLEVALUE_aurora_Hq7dPw3nXsYvK0mBtRj5

journal seq 3 (tenant aurora, seq 1): key.issued {id: key_a1f3e2, role: admin, value: not recorded}

$ oml tenants issue-admin aurora --label "second attempt"
refused: tenant.admin_exists — aurora already has an admin key; further keys are
issued by that admin (issuer ceiling), not by the operator.
```

The last refusal is the shape of the operator role: a bootstrap channel, not a standing back door.

## 2. Issuing a crew's keys

The aurora admin provisions the `corrosion` crew: a workarea, writer keys for its working agents,
and dreamer keys for the crew that reviews it. Grants bind to node ids; paths are a convenience
([workareas](../architecture/workareas.md)).

```console
$ oml context use aurora-admin
$ oml workareas create universe/research corrosion \
    --crew-name "Corrosion Crew" --external-manager "crewd.example/corrosion"
created wa_00004e universe/research/corrosion (status: development)
journal seq 812: workarea.created

$ oml keys issue --role writer \
    --grant wa_00004e:writer --grant wa_00004e:reader \
    --expires 90d --label corrosion-agent-1
key issued — SHOWN ONCE

    key id : key_9c44b7
    key    : oml_sk_EXAMPLEVALUE_aurora_Wn2gLx8cQe5rTv0zYhDk

journal seq 813: key.issued {role: writer, grants: 2, value: not recorded}

$ oml keys issue --role dreamer \
    --grant wa_00004e:dreamer --expires 90d --label corrosion-reviewer-1
key issued — SHOWN ONCE

    key id : key_51d0ac
    key    : oml_sk_EXAMPLEVALUE_aurora_Fb6mZq1jUs4xNc9pKtRv

journal seq 814: key.issued {role: dreamer, grants: 1, value: not recorded}

$ oml context use corrosion-reviewer-1
$ oml keys issue --role writer --grant wa_00004e:writer --label corrosion-agent-2
refused: keys.issuer_ceiling — key_51d0ac (dreamer) may not issue keys;
a key is issued with a role no higher than the issuer's, and issuance itself
is an admin/operator verb.

$ oml context use aurora-admin
$ oml keys list --expiring 120d
KEY ID      ROLE     LABEL                  EXPIRES     GRANTS
key_9c44b7  writer   corrosion-agent-1      2027-01-27  wa_00004e:writer, wa_00004e:reader
key_51d0ac  dreamer  corrosion-reviewer-1   2027-01-27  wa_00004e:dreamer
```

A writer that must also recall holds explicit reader scope — roles never imply one another. The
key values above appeared exactly once; `oml keys list` and the journal know only ids and hashes.

## 3. A review session

A human dreamer drains the corrosion queue. The serve binds (reviewer key, artifact version,
attachment digest set); the verdict consumes it
([serve mechanics](reference.md#oml-review-the-review-ceremony)). The second item shows the
lineage-closure refusal: the reviewer proposed an edit in this row's ancestry, so the server will
not serve it to them.

```console
$ oml context use corrosion-reviewer-1
$ oml review next --workarea universe/research/corrosion
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
art_02b8e1@v1 promoted; serve srv_f3a91c consumed.

$ oml review next --workarea universe/research/corrosion
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

Status transitions are forward-only and two-party: proposed by one admin or dreamer, ratified by a
second, non-proposing one, journalled with the policy snapshot
([workareas](../architecture/workareas.md)). The first attempt here is refused because a child is
not yet validated; the self-ratification attempt is refused by R2.

```console
$ oml context use aurora-admin
$ oml workareas status propose universe/research/corrosion --to validated
refused: workareas.children_not_validated
  universe/research/corrosion/scratch (wa_000061) is development
an area may only become validated when all its children are validated.

$ oml workareas status propose universe/research/corrosion/scratch --to committed
transition trn_0a44 proposed (development -> committed); awaiting a second ratifier.
$ oml workareas status ratify trn_0a44
refused: workareas.ratifier_is_proposer — a transition is ratified by a second,
non-proposing dreamer or admin (R2 extends to transitions).

$ oml context use corrosion-reviewer-1
$ oml workareas status ratify trn_0a44
journal seq 1203: workarea.status_changed {wa_000061: development -> committed,
  proposer: key_a1f3e2, ratifier: key_51d0ac, policy_snapshot: pol_77e2}

$ # …scratch later ratified to validated the same way (trn_0a51), then:
$ oml context use aurora-admin
$ oml workareas status propose universe/research/corrosion --to validated
transition trn_0a58 proposed (committed -> validated); awaiting a second ratifier.
$ oml context use corrosion-reviewer-1
$ oml workareas status ratify trn_0a58
journal seq 1240: workarea.status_changed {wa_00004e: committed -> validated, …}

$ oml context use aurora-admin
$ oml workareas create universe/research/corrosion post-mortem
refused: workareas.parent_validated — a validated area accepts no new children.
remaining mutations on wa_00004e: demote, erase, crew-metadata update.
```

## 5. Running and ending a simulation

Simulation writes are ordinary journalled events carrying indelible `provenance: simulation`,
visible only to the audience, reviewed only by dreamers in it; ending the simulation demotes its
rows and re-reviews tainted claims ([simulations](../architecture/simulations.md)). `end` is a
mass-reach act, so it defaults to `--dry-run`.

```console
$ oml context use aurora-admin
$ oml simulations open --audience key_9c44b7,key_51d0ac --label "what-if: 40 °C baseline"
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
journal seq 1388: simulation.ended {sim_0007, demoted: 38, re_review: 2}
sim_0007 ended. tainted claims re-enter held; their reviewers see the taint.
```

## 6. Evidence export and chain verification

The export produces the evidence an auditor asks for; the chain verification proves the journal
was not rewritten — and, because journal payloads are digest-only, it still verifies after
crypto-shredding ([audit and lineage](../architecture/audit-and-lineage.md)).

```console
$ oml audit export --out ./evidence-2026-10-29
exporting tenant aurora…
  decision trails    1,482 memories (per-memory: submission, scan, serves, verdicts, policy snapshots)
  chain verification seq 1..1389 — recomputed, signatures valid, head hash 7c19…40be
  grants history     61 grant/deny/epoch events
  read log           retention window 90d (tenant policy): 12,044 events, batch digests anchored at journal seqs
  reciprocal approvals  per key pair; top pair (key_51d0ac, key_60b311): 9 mutual — surfaced, not judged
  simulations        excluded by default; 1 ended simulation listed by id only (sim_0007)
bundle written: ./evidence-2026-10-29/aurora.oml-evidence.tar (signed, 18.3 MiB)
journal seq 1390: audit.exported {scope: aurora, from_seq: 1}

$ oml audit verify-chain --genesis
verifying tenant aurora from genesis…
  seq 1..1389: prev_hash links ok, ed25519 signatures ok (deployment key dk_5a21)
  erased artifacts encountered: 4 — digest envelopes verify; bodies unrecoverable (as designed)
chain verified. head: seq 1389, hash 7c19…40be
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
journal seq 1401: ban.added {source: connector:rss/feedpulse.example, by: key_a1f3e2}
ban ban_0012 active; admission now refuses this source.

$ oml bans list
BAN ID    TARGET                                 BY          STATE   LIFTED BY
ban_0012  source connector:rss/feedpulse.example key_a1f3e2  active  —
```

The preview's sharpest line is the one about promoted claims: a ban is an admission gate, not a
retroactive purge. What was already promoted stays visible until someone demotes it, claim by
claim, in the journal — no silent deletion (R4).
