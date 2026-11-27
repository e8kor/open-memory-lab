<!--
id: adr/ADR-0097-every-admin-act-is-one-journalled-record-narrowing-takes-effect-at-once-widening-waits-on-a-second-admin-person-and-every-admin-key-belongs-to-a-person
tier: engineering
status: accepted design · 2026-11-25
answers: how every break-glass and key-authority act of an admin is recorded and confirmed; which acts take effect at once and which wait on a second admin person; how a one-admin tenant acts; which verdicts need a counted person; whether a time-boxed widening exists; how an act against another admin takes effect and is settled; how any key is revoked; whether raising a limit waits; how erase shreds; how an admin reaches into a person's own root; who counts as a second admin person; who may use the admin read; who ratifies a ladder move; what services hold instead of admin keys and who issues session keys; how the host recovers a dormant tenant and settles a frozen dispute; how notices are sent; which pieces land in v0.8.5; whether an admin key may exist unlinked; and for each, the events, refusals, doctor lines, bottlenecks, tests and the production migration
-->

# ADR-0097 — Every admin act is one journalled record: narrowing takes effect at once, widening waits on a second admin person, and every admin key belongs to a person

**Status:** accepted design · 2026-11-25, decided by the operator, who answered all seventeen
questions of the break-glass and key-access brief on 2026-11-25. Q9, Q12 and Q17 differ from the
brief's recommendations, and this record re-derives every rule that rested on those
recommendations (marked **re-derived** where it appears). The operator answered the five tensions
those answers created, and three readings, the same day; each answer is recorded where it applies
and marked **decided by the operator, 2026-11-25**. Readings this record takes beyond the answers
were marked **reading, for the operator to confirm**; the operator decided the last two on 2026-11-26. Amended by [ADR-0098](ADR-0098-a-protected-or-frozen-workspace-can-be-marked-public-and-then-every-reading-key-in-the-tenant-reads-its-approved-items.md) (d3, d5, 2026-11-26, decided by the operator).

**Release:** v0.8.6, not built. Four pieces are built in v0.8.5 (d27).

**Amends:** [ADR-0001](ADR-0001-the-constitution.md) (d5, kept: exception 3 reads with two
changes), [ADR-0009](ADR-0009-status-ladder.md) (d3),
[ADR-0010](ADR-0010-roles-and-keys.md) (d1, d3),
[ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) (d7, d8),
[ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
(d3, d7; d6 by d31),
[ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
(d6, d10, and the decisions of 2026-11-19 and 2026-11-20),
[ADR-0070](ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md) (d2: the admin console creates workspaces and issues keys, d29),
[ADR-0079](ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md)
(d5),
[ADR-0085](ADR-0085-a-workspace-publishes-immutable-releases-that-one-person-cuts-and-another-ratifies-and-each-key-binds-to-the-tip-the-latest-release-or-a-pinned-one.md)
(d16),
[ADR-0089](ADR-0089-an-admin-fixes-a-production-workspace-at-once-by-a-marked-break-glass-act-that-a-person-confirms-or-reverts-afterwards.md)
(d2, d14, d16, and the decisions of 2026-11-19 and 2026-11-20),
[ADR-0093](ADR-0093-contributor-and-submitter-replace-writer.md) (the key-role set),
[ADR-0095](ADR-0095-an-agent-creates-its-own-child-workspaces-writes-guardrails-and-proposes-status-and-brief-changes-through-oml-write-and-every-verdict-and-setting-stays-with-people.md)
(d5; d3, d4 and the answer Q5 by d30 and d31) and
[ADR-0096](ADR-0096-a-person-builds-a-key-in-the-console-with-grants-inside-the-own-root-and-a-rate-and-the-admin-reads-one-key-and-sets-its-limits.md)
(d4, d12; d12's deferral of the admin builder, d29). Each carries an amendment note dated
2026-11-25. It supersedes nothing whole.

**Depends on:** ADR-0001 (R1 to R4), ADR-0068 (open sign-up, proposed, built), ADR-0083 d8 (the
To do flow), ADR-0090 d18 (the lake's twin tables).

## Context

The operator asked on 2026-11-25 to "improve our break the glass strategy, unify approach and make
it in seemless non conflicting with main approach way, lets also align our key strategy, its top
down strategy for who have access where". The brief that answered it (revision 4, reviewed over
three security and consistency rounds and a confirmation) read `origin/main` at `e3a3f252`,
`bulk085/console` at `3928c0e5` and `bulk085/mcp` at `6f1def4d`. Production was not read; the
counts of d27 item 1 come first.

**The inventory.** Today there are fourteen families of break-glass path under six confirmation
rules. ADR-0089's fix is the only one that rides the review flow: another person settles it. Every
act that decides who may act is one admin's act, at once and unsettled: keys, grants, connect
codes, denials, limits, allowlist entries, capacities, quotas and the ladder.

**The top inconsistencies.** Line numbers are `origin/main`'s.

1. **A bearer admin key cannot be ended.** No route denies, expires, rate-limits or revokes one
   (`crates/oml-app/src/app.rs:3947`; `docs/architecture/roles-and-keys.md:84,95`). The deployer
   holds such keys in two files (`oml-devops`, `ops/deployer/README.md:190-202,230`).
2. **Minted key pairs approve anything, instructions included.** An admin issues a contributor and
   a reviewer key `used_by: person`, each with a principal of its choosing, writes with one and
   approves with the other. R2 by person is skipped for a key that counts as no one
   (`app.rs:5171-5178`), which defeats ADR-0001 d5's "another person for instructions".
3. **An admin can act as a person.** It mints `user: alice` keys, a reviewer among them, whose
   writes are approved as alice's own `same_user` work (`app.rs:7453-7460`). It mints a connect
   code for alice's own connector key and holds `/mcp` as her agent (`app.rs:11799-11902`). Neither
   path journals a `memory.admin_read`.
4. **An admin reaches into a person's root alone.** `grant_review` and `grant_curation` grant on any
   workspace through `add_grants`, a private helper with no root check
   (`crates/oml-app/src/app/pages.rs:703-793,815-858`).
5. **The ladder's two identities are two keys, not two persons.** An admin key proposes and ratifies
   (`app.rs:5374,5490`), and `same_user` reads `false` for keys that count as no one
   (`app.rs:5518`). Two keys of one admin freeze a workspace, and frozen is final.
6. **The last-admin guard is a field only the console sends**
   (`crates/transports/oml-transport-http/src/web/emails.rs:257`; `allow_web_email` passes `false`,
   `crates/oml-app/src/app/emails.rs:38-44`).
7. **The second person who settles a fix can be one an admin invented.** The operator accepted this
   on 2026-11-19 and 2026-11-20 (ADR-0089's third decision of 2026-11-19; ADR-0045's limits table of
   2026-11-20).
8. **The runbooks forbid hand edits and prescribe them** (`docs/operations/runbooks.md:11` against
   `154-202`). The deny reason is accepted and dropped
   (`crates/transports/oml-transport-http/src/lib.rs:4246-4253`).

Three models were weighed: patch each hole in place (A), one admin-act record under a fail-closed
rule (B′), and urgent proposals that revert unless ratified within N hours (C). The operator chose
B′ (d1).

**Trust boundary, once.** Anyone with host or database access, or control of the sign-in provider
or the mail domain, can forge consent and settlement under every rule here: the serving role
rotates `api_keys.hash`, sets and accepts a pending link and clears `web_emails.pending_since`
(ADR-0045's limits table). The platform operator is trusted (`docs/architecture/security.md:103`).
In-tenant two-person rules bind tenant admins, never the host. Against the host the controls are
journal tamper evidence and out-of-band notice.

Decided, not built.

## Decision

- **d1 — One model, B′: every act an admin takes outside content review is an admin act under one
  fail-closed rule (Q1).** Narrowing takes effect at once and never reverts by itself. Widening
  takes effect only after a second admin person confirms it, except in a `sole_admin` tenant (d10).
  Erase demotes at once and shreds after a second admin person (d4). An act against another admin
  is contestable (d8). Persons' and agents' own acts are not admin acts and keep their own rules: a
  person's issuance in their own root (ADR-0096 d1), the in-progress mode and commit (ADR-0030),
  `create_workspace` and proposals by agents (ADR-0095), and verdicts. It ships in v0.8.6.
  *Rejected:* model A (patch in place) and model C (urgent proposals with auto-revert), in the
  table at the end; C′, C's fail-closed variant, is B′'s rule under another name.

- **d2 — One envelope, `admin.act`, and the act decides its class.** Every admin act journals
  `admin.act {act_id, act, class, target, reason, actor_person, state, confirmed_by_person,
  sole_admin, cause}` in the commit of its effect. The existing event types (`key.issued`,
  `grant.issued`, `key.denied`, `admin.fix` and the rest) stay as the effect and name the `act_id`;
  ADR-0089 already does this for `admin.fix`. `class` is one of `narrowing`, `erase`, `widening`,
  `consent`, `against_admin`, `neutral` and is fixed per act in code, never sent by the caller; no
  act sits in two classes. `state` is `effective`, `pending_confirmation`, `confirmed`,
  `declined`, `expired`, `contested`, `settled`. `actor_person` is always known, since every admin
  key belongs to a person (d21). `cause` names a non-interactive origin (`upgrade`,
  `deactivation`, `settlement`, `recovery`) and is null for an act an admin took. Settlement rides
  ADR-0083 d8's To do flow: "Admin fixes to confirm" becomes "Admin acts to confirm", judged by
  ADR-0089's predicate (`crates/oml-app/src/app/fixes.rs:1031-1140`). A widening item carries
  "confirm" and "decline" and no revert, because nothing took effect. The lake's twin tables
  (ADR-0090 d18) gain the envelope; older journals replay unchanged. *Rejected:* a class sent by
  the caller, which lets a caller pick the lighter rule; a new event per act with no shared
  envelope, which is today's fourteen paths.

- **d3 — Narrowing takes effect at once and never reverts by itself.** The class: demote, retire,
  release and import rollback (ADR-0089); every deny on any workspace, user roots included
  (ADR-0096 d4, kept); `cut_off`; `disable_user` of a non-admin person; `close_deletion`, where the
  person's request is the consent; key revocation of a non-admin key (d14); connection revocation;
  ending sessions; lowering a limit or quota (d15); narrowing a binding or the federation policy;
  declining a pending or `sole_admin` act; lifting a denial (decided by the operator,
  2026-11-27). Content fixes keep ADR-0089's settlement by another person, unchanged; every other
  narrowing act is listed in the admin's To do as done and counted by doctor. *Rejected:* a timer that reverts narrowing (model C), which re-serves a leaked key the
  moment nobody ratifies its demotion.

- **d4 — Erase demotes at once, and the shred waits on a second admin person (Q8).** Erase becomes
  two effects. The act demotes every version of the claim at once, which contains the leak, as an
  ADR-0089 fix settled as fixes are. The shred runs only after a second admin person who counts
  (d11) confirms it; until then the bytes are kept and `admin.erase_shred_pending` reports the
  oldest. In a `sole_admin` tenant the shred runs at once, marked. A declined shred leaves the
  claim demoted and its bytes kept. This reverses ADR-0089 d2 and d16's erase at once (2026-11-14,
  2026-11-17). The operator's answer names a second **admin** person, where the brief named any
  second person. *Rejected:* shred at once, as today, which lets one admin destroy evidence alone;
  any reviewer person as the shred's confirmer, which the operator did not take.

- **d5 — Widening takes effect only after a second admin person confirms it.** The class:
  - issuing any key or grant that can read, write, approve or ratify, admin and `maintenance` keys
    included;
  - adding a grant anywhere, a person's root included (d6) and an in-progress subtree included,
    where a person holding reviewer in that subtree must also confirm;
  - a capacity (`grant_review`, `grant_curation`), on any workspace;
  - a new admin person, or an admin flag on an existing person (then also d7);
  - minting a connect code for any connector key, one in a person's root included (d6);
  - raising a limit or quota, clearing one, a later expiry, or a looser quota profile (d15);
  - accepting federation for a workspace, or widening the tenant's federation policy;
  - an export allowance, once built (ADR-0084 d10).

  A pending act's key value or connect code is printed once at issuance and authenticates or
  redeems nothing until confirmed (d23). Under `sole_admin` it takes effect at once, marked (d10);
  under a time box it takes effect at once for at most 24 hours (d13). *Rejected:* widening at
  once with a later review, which leaves every minted pair live until someone looks.

- **d6 — Reaching into a person's own root is widening: a second admin person confirms it, and the
  person's consent is not asked (Q9, re-derived).** Three acts reach a root: an admin-issued key
  naming `user: alice` in alice's root, any role the issuer ceiling admits (ADR-0044 d3); a grant
  or capacity on a node in alice's root (ADR-0044 d7, ADR-0045 d10); a connect code for a
  connector key in alice's root (ADR-0031 d7, d8). Each is widening under d5 and nothing more. The
  confirm screen names the root's person. A pending connect code is shown once at the mint, voids
  none of the person's own codes and redeems nothing until confirmed; the void that
  `mint_connect_code` performs today (`app.rs:11884-11890`) happens at confirmation, and ADR-0031
  d8's ten-minute life runs from the confirmation (decided by the operator, 2026-11-26: from
  the mint, a code would nearly always expire before a second admin saw it). This reverses ADR-0044 d3 and d7, ADR-0045 d10
  and ADR-0031 d7 and d8 only as far as adding the second admin's confirmation: an admin still
  issues a reviewer user key in another person's root once confirmed, and no consent class covers
  these acts. A person's own issuance and own codes in their own root never wait (ADR-0096 d1).
  - **Under `sole_admin`** (decided by the operator, 2026-11-25): a reach into a person's root
    takes effect at once, marked `sole_admin` (d10), and the root's person is notified in-product,
    as a To do item, in the act's own commit (d20).
  - **What a confirmed reach acts as** (decided by the operator, 2026-11-25, as is): keys a second
    admin confirmed in a person's root act like that person's own keys. Their approvals keep the
    own-root allowance, marked `same_user` (`app.rs:7453-7460`), and their reads are not counted
    as admin reads (d16). The operator accepted this residual; the security analysis states it.
  *Rejected:* the brief's recommendation, the person's acceptance for every reach and no
  admin-issued reviewer key or admin connect code in another's root at all, which the operator did
  not take; consent for grants only; as today.

- **d7 — Consent shrinks to the acts that bind a key or an address to a person (re-derived from
  Q9).** With reach moved to widening (d6), a person's acceptance is still needed only where an
  act makes something act **as** that person, as the operator's decisions of 2026-11-19 already
  require:
  - a link of a key to a person (ADR-0085 d16, as amended 2026-11-19), pending until the person
    accepts from their own console session;
  - a sign-in address for an existing person (ADR-0045's decision of 2026-11-19), pending until
    the person accepts;
  - an admin flag for an existing person: a second admin person confirms first (d5), then the
    person accepts the flag shown (ADR-0045's decision of 2026-11-20).

  Acceptance events record the provider subject and the session's age, so a forged acceptance can
  be told apart afterwards. *Rejected:* the brief's consent class, which also covered `--user` keys,
  grants, capacities and connect codes in a person's root; the operator moved those to d6.

- **d8 — An act against another admin takes effect at once, is contestable, and suspends the
  target's admin keys (Q5).** The class: revoking a key of another admin person; un-flagging,
  removing, disabling, or changing the entry of another admin person.
  - **Revocation is final.** No value comes back, and that is the containment.
  - **Flag, entry and disable acts take effect at once and are contestable** through a
    contest-only sign-in: the target's provider sign-in still opens it when their entry is
    disabled or un-flagged, it matches the entry as it was before the act, and it does nothing
    but contest and read held notices. A contest restores the flag and sign-in only; it never
    restores a revoked key, and ended sessions stay ended.
  - **The target's admin keys are suspended at once** and stay suspended while the act is open; a
    contest does not lift the suspension (the brief's R4-B1 finding). When the act is settled
    confirmed, they are revoked (d21); when it is settled reversed, the suspension lifts.
  - **With three or more admin persons**, a third admin person who counts (d11) settles.
  - **With fewer than three, a freeze:** both parties lose widening; the initiator loses acts
    against an admin; both lose the admin read; both keep the other narrowing acts. The target may
    revoke the initiator's keys and end the initiator's sessions, both final, only from the
    contest-only sign-in, never through an admin key. Doctor raises `critical`. The operator settles
    from the host with `oml operator settle` (d19).
  - **A stolen key.** An attacker holding A's key un-flags B. B contests from the contest-only
    sign-in, regains the flag, revokes A's keys and ends A's sessions; the freeze already took the
    attacker's admin read. An attacker holding B's session who contests a fair act by A gets B's
    flag back and nothing more. The real B recovers the provider account, and the operator settles.

  *Rejected:* acts against an admin only after a second admin confirms (Q5(c)), which leaves a
  two-admin tenant unable to contain the other's stolen key; no operator settlement (Q5(b)), which
  makes a two-admin freeze permanent, since adding a third admin is widening and the freeze
  refuses it.

- **d9 — Neutral acts take effect at once with nothing to settle.** The class: a rescan (it
  triggers the server's scanner and never takes findings as input); doctor and count reads; the
  admin read, under d16's gate; creating a shared workspace; creating a crew; `set_workspace_crew`;
  crew metadata, the manager included while no served check reads it (`satisfies_manager_inequality`
  is called nowhere in `oml-app`); an allowlist entry for a new non-admin person, which reaches only
  that person's new root and the `universe` node, as an open sign-up does. *Rejected:* making a new
  person widening, which would make every open-sign-up-shaped invitation wait.

- **d10 — `sole_admin`: a tenant that has never had two live admin persons acts at once, marked,
  and leaves the state for good once a second admin person exists (Q2, re-derived).**
  - **When.** Computed by the fold from the journal's admin-entry events (`web.email_allowed`,
    `web.email_changed`, `web.email_accepted` with the admin flag), never from the state at
    migration. A bootstrap always names a person (d21), so the zero-person reading of the brief's
    recommendation does not exist: `sole_admin` means exactly one admin person ever.
  - **Effect.** Widening and the shred take effect at once, marked `sole_admin` on the act, the key
    and every grant, with a doctor line naming the tenant; a reach into a person's root also
    notifies that person (d6). A ladder ratification by the proposer's own person is admitted,
    marked (d17). Fixes stay unconfirmed and excluded from cuts, as today.
  - **Sticky.** Once two live admin persons have existed, the tenant never returns to
    `sole_admin`, except after a drop to one that the departing person confirmed, or a drop settled
    under d8. A recovery is not such a drop (d19).
  - **While any act against an admin is open**, widening is confirm-first even with one live admin
    person.
  - **When a second admin person arrives**, every marked act appears in their To do once their
    entry is seven days old (d11). A declined `sole_admin` act is revoked, which is narrowing and
    takes effect at once.

  *Rejected:* no `sole_admin`, which refuses widening until a second admin exists and breaks every
  one-operator server; a zero-person `sole_admin` with an unlinked bootstrap key, which d21 removes.

- **d11 — A second admin person counts only if their entry was accepted at least seven days before
  the act (Q10).** Seven days is a placeholder, chosen, not measured. The age is read from the
  acceptance event of the entry with the admin flag. A person created on the spot cannot confirm.
  The same age rule counts the shred's confirmer (d4), the third admin who settles a contest (d8)
  and the confirmer of a time box (d13). This reverses the acceptance of invented persons recorded
  on 2026-11-19 and 2026-11-20. An invented admin person confirms nothing for seven days, and
  making one is itself widening. An invented non-admin person settles a production fix only
  through a reviewer grant on the fix's workspace, which is widening that a second admin person
  confirms (d5). In a `sole_admin` tenant the founder can still invent a settler, marked (accepted
  by the operator, 2026-11-25). *Rejected:* anyone else (Q10(b)), which lets an invented person confirm; a
  lineage rule, which freezes the founding admin's widening for good.

- **d12 — A counted person is required for instruction verdicts, fix settlements and ladder
  ratifications; knowledge verdicts keep key-level R2 (Q3).** A key that counts as no person takes
  none of the three. Counting does not change: a session key counts as its user, a user key by the
  issuer walk of 2026-11-20, a tenant key only through an accepted link (`counted_walk` stops at a
  tenant key, `app.rs:4970`). A crew's agent reviewer key still approves knowledge in production
  (ADR-0001 d5, "Agents approve knowledge", 2026-11-14). The minted-pair attack is closed twice:
  the instruction verdict refuses a key that counts as no person, and issuing that reviewer key is
  widening. *Rejected:* every production verdict by a person (Q3(b)), which stops one admin's
  agents from reviewing each other; a tenant key counting as its crew (Q3(c)).

- **d13 — A narrow time box admits emergency read and write grants for at most 24 hours (Q4).** An
  admin person may issue, at once, `reader` and `submitter` grants in shared workspaces only, never
  into a user root or an in-progress subtree, with a mandatory `until` of at most 24 hours (a
  placeholder) that anything issued under the grant inherits. The time-boxed authority issues
  nothing. One open time box per actor person per workspace; one renewal, after which renewals are
  refused until a second admin person settles the first. No untimed twin exists, because untimed
  widening is confirm-first. Refused while any act against an admin is open and during a freeze.
  In a `sole_admin` tenant it adds nothing. *Rejected:* no time box (Q4(b)), which leaves a night
  incident in a multi-admin tenant waiting for a second admin to wake.

- **d14 — One plain revoke verb ends any key, and nothing more (Q6).** `POST
  /v2/keys/{key}/revoke {reason}` revokes one key: it authenticates nothing afterwards, on every
  path (bearer, OAuth token and refresh, connect-code redemption), and its open connections close.
  It journals `key.revoked {key_id, revoked_by_key_id, actor_person, reason, act_id, cause}`. No
  cascade: keys the revoked key issued keep working, and session keys come from the server issuer
  (d18), so revoking an admin key breaks no sign-in. The console offers one button on the admin
  one-key page and on the person's Keys page, and nothing else. Who: an admin person (a linked
  admin key or `<user>-admin`) on any key of the tenant, against another admin person's key d8's
  act and otherwise narrowing; and a person's `<user>-web` on the person's own keys, those that
  count as the person, as a person's own act and not an admin act (decided by the operator,
  2026-11-25).
  Refused: a session key (`session_key`; a session ends with its entry), a reserved key (it
  resolves on no route), an already revoked key (`revoked`). The body admits only `reason`, and
  unknown fields are `request.validation`, so `cascade` and rotation can be added later as new
  members without changing the verb. This reverses ADR-0079 d5 and its answers of 2026-11-12 ("no
  admin key ends another") and ADR-0096 d12's deferral of revocation (2026-11-24). *Rejected:* admin keys only (Q6(b)); no revocation (Q6(c)), which leaves a leaked admin
  key permanent; a cascade, which the operator deferred.
  *Note of 2026-11-25:* rotation is built in v0.8.5 as its own verb, `POST /v2/keys/{key}/rotate
  {grace_seconds?}` ([ADR-0096](ADR-0096-a-person-builds-a-key-in-the-console-with-grants-inside-the-own-root-and-a-rate-and-the-admin-reads-one-key-and-sets-its-limits.md) d13), not as a member of this verb. Under this record's model
  rotation is neutral (d9): it grants nothing new, a key keeping exactly the access it held, so it
  takes effect at once and waits on no second admin person. Its refusals stay as built: a session
  key, a connector key, a revoked, cut-off or expired key. *Amended by the operator's decision
  of 2026-11-25 (ADR-0096 d13, the review round):* in v0.8.5 an admin rotates only its own admin
  key and tenant keys that name no person, and is refused another admin's key
  (`not_admins_key`) and a key in a person's root (`in_persons_root`). In v0.8.6 an admin's
  rotation of a key it would then hold the new value of, another admin person's key or a
  person's, falls under d6 and d8: it waits on a second admin person, as revoking another admin
  person's key does, since the rotating admin alone would then hold the key's only working value.

- **d15 — Raising a limit is widening; lowering it takes effect at once (Q7).** A raise, a cleared
  limit, a later expiry (which can revive a cut-off key) or a looser quota profile waits on a
  second admin person, and the console's limits form says "waits on another admin". A lower rate,
  an earlier expiry or a tighter profile is narrowing. This reverses ADR-0096 d4's limits form
  acting at once (2026-11-24) and ADR-0079 d5's "an admin key that is not the key itself" as the
  only condition (2026-11-12). Raising ADR-0095 d4's `workspaces` quota is a raise. *Rejected:*
  limits at once as today (Q7(b)).

- **d16 — The admin read needs a person-linked admin key or a session, counts its reads per
  person, and is suspended in a freeze (Q11).** A person sees on their own page a count of admin
  reads of their root, by month. Doctor warns past N pages per investigation (a placeholder).
  The explicit close of 2026-11-17, with no idle expiry, is kept. A `maintenance` key never reads.
  Reads through keys or connect codes a second admin confirmed in a person's root are that root's
  own reads and are not counted here (d6, decided by the operator, 2026-11-25).
  This reverses ADR-0089 d14's "no `used_by: person` requirement" (2026-11-14). *Rejected:* as
  today, with or without the per-person count (Q11(b), (c)).

- **d17 — Admins stay on the status ladder, and the ratifier must count as a person other than the
  proposer (Q12, re-derived).** ADR-0009 d3's two identities become two persons at ratification:
  the ratifier counts as a person (d12), and that person differs from the proposer's. Proposers
  stay as ADR-0095 d5 has them: an admin, a reviewer, or an agent reviewer key over MCP. **Decided by
  the operator, 2026-11-26:** when the proposing key counts as no person, the ratifier must also
  differ from the `actor_person` of the admin act that issued the proposing key, so one admin
  cannot propose through an agent key they issued and ratify through their own session. A ladder
  move is not an admin act: it keeps its own two-person flow whoever takes it, and ADR-0089 d2's
  "a ladder move is not a fix" stands. **Under `sole_admin`** (decided by the operator,
  2026-11-25), a ratification by the proposer's own person is admitted and marked `sole_admin` on
  the transition, so a one-person tenant still moves the ladder; the ratifier still counts as a
  person (d12). *Rejected:* the brief's recommendation, admins leave the
  ladder (Q12(a)), which the operator did not take; proposers limited to persons (Q12(b)); as
  today (Q12(c)), where two keys of one admin move it.

- **d18 — No service holds an admin key: a `maintenance` key reads counts, and a reserved server
  issuer issues session keys (Q13).**
  - **The server session issuer** is a reserved per-tenant key like ADR-0089 d17's revert key: no
    value, no credential, refused by `authenticate`, resolvable on no route. Its fixed issuable set
    is the four session-key shapes (`<user>-web`, `<user>-review`, `<user>-curate`,
    `<user>-admin`), not an admin role, and it issues them only for an accepted, unpending,
    undisabled entry, `<user>-admin` only for a confirmed admin flag or under `sole_admin`. It
    replaces `entry.added_by` (`crates/transports/oml-transport-http/src/web/auth.rs:372-377`) and
    open sign-up's `signup.issuer` (`auth.rs:626,676-705`), which is read and ignored for one
    release with a doctor note. Doctor flags any `key.issued` by it that is not one of the four
    shapes.
  - **The `maintenance` key role** reaches, in counts-only form, the reads `oml doctor` makes under
    an admin key today (`run_doctor`, `crates/bins/oml-cli/src/main.rs:6132+`): `/v2/web/users`,
    the `/v2/events` head, `/v2/review/queue`, `/v2/search`, `/v2/federation`,
    `/v2/workspaces/universe/tree`, `/v2/admin/usage/drift` and `/v2/admin/rescan`; plus `POST
    /v2/admin/rescan`, `/healthz`, `/readyz` and `/v2/whoami`. It never reaches `/v2/keys*`,
    `/v2/admin/memories/*`, `/v2/admin/investigations*`, `/v2/admin/fixes`, the full `/v2/events`
    tail, `/v1` or `/mcp`, and never serves claim text, key values, emails or key lists. It carries
    a request rate (placeholder 60 a minute). Issuing it is widening; it is revoked by d14. Doctor
    learns the role in the same release, so the drift, sign-up and rescan lines stay alive on the
    deployer; lines that list keys or addresses stay admin-only, run by a person.
  - **Conversion in place.** `POST /v2/keys/self/convert {to: maintenance}` admits only `admin →
    maintenance` under the key itself, keeps the hash, and journals a sealed `key.converted {from:
    admin, to: maintenance}` that the fold and the lake replay; it is never a migration `UPDATE`.

  This reverses ADR-0045 d6 (2026-11-04: session keys issued "by the server under the admin key
  that added the entry") and retires `signup.issuer` from ADR-0068 d1, a proposed record.
  *Rejected:* service admin keys refused only the admin read (Q13(b)); as today (Q13(c)).

- **d19 — Two host verbs, `recover` and `settle`, whose tokens go to files only (Q14).** Both write
  their token to a 0600 root-only file bound to the tenant id and one process start, never to
  stdout or journald, and notify per d20.
  - **`oml operator recover <tenant> --person <name> --email <address>`** runs only when no admin
    person of the tenant has signed in, and no admin key has authenticated, for 30 days (a
    placeholder). Every admin key belongs to a person (d21), so no bearer key outside a person can
    pin the tenant active. A 72-hour waiting period (a placeholder) follows, cancelled by any
    sign-in of an admin person of the tenant. It then revokes every admin key of the tenant,
    creates the named admin person, and journals `operator.recovery` with the window and the
    revoked keys. **It keeps the dormant admin entries' flags** (decided by the operator,
    2026-11-25). The consequence, stated: the recovered tenant has had two admin persons, so it
    leaves `sole_admin` (d10), and the new admin's widening needs the confirmation of a dormant
    admin who counts (d11). If the dormant admins never return, the new admin still narrows, takes
    neutral acts, fixes and reads, but cannot widen alone.
    - **The way out with one dormant admin.** The new admin un-flags the dormant admin, an act
      against an admin (d8). With fewer than three admin persons it freezes, and after 24 hours the
      operator confirms it with `oml operator settle`; the drop is then settled under d8, and the
      tenant is `sole_admin` again for the new admin (d10). The dormant admin is notified, by email
      at the settlement (d20), and may contest from the contest-only sign-in until then.
    - **With two or more dormant admins, an accepted limit.** The tenant then has three or more
      admin persons, so an un-flag is settled by a third admin who counts (d8), and no dormant one
      will; no freeze opens, so `oml operator settle` does not apply, and a second recovery never
      runs while the new admin signs in. The new admin's widening waits until a dormant admin
      returns. The operator accepted this limit on 2026-11-25.
  - **`oml operator settle <tenant> <act> --confirm|--reverse`** runs only on a freeze open for at
    least 24 hours (a placeholder). No sign-in cancels it; both parties are notified. Its only
    effect is to confirm or reverse the contested act; it issues no key and reads no content. It
    journals `operator.settlement`.

  *Rejected:* no host verbs (Q14(b)), which leaves SQL or a restore and drops d8's settlement.

- **d20 — Notices are in-product for everything, and email goes out only for recovery and
  settlement (Q15).** An act against an admin, offensive narrowing (d24), a recovery and a
  settlement each write a To do item and a `critical` doctor line. A reach into a person's root
  under `sole_admin` writes a To do item for that person (d6). A recovery or settlement notice
  is also emailed to every admin person of the tenant, since a dormant admin sees nothing
  in-product. The server sends no mail today, so the build adds settings for one outbound mail
  relay and its sender, one egress host, and the doctor lines `notice.mail_unconfigured` and
  `notice.mail_delivery_failed`. *Rejected:* email for every notice (Q15(b)); in-product only
  (Q15(c)), under which the recovery notice reaches no dormant admin and the waiting period cancels
  nothing.

- **d21 — Every admin key belongs to a person; when the person goes, the keys go (Q17, re-derived;
  stronger than the recommendation).**
  - **Issuance.** An admin key is issued only with `--person`, `used_by: person`, and a link to
    that person pending until the person accepts it (d7); it authenticates nothing until the link
    is accepted and, where widening applies, a second admin person confirms. `used_by: service` and
    an admin key with no person are refused (`admin_key_requires_person`). Services hold
    `maintenance` keys (d18).
  - **Deactivation and removal.** When an admin person deactivates or removes their own account,
    or asks for deletion and it is closed, their admin keys are revoked at once (`cause:
    deactivation`). When another admin disables, removes or un-flags them (d8), the keys are
    suspended at once and revoked when the act is settled confirmed; a reversed act lifts the
    suspension. Un-flagging counts as removal of the admin standing the keys depend on, so an admin
    key never outlives its person's admin flag (accepted by the operator, 2026-11-25); a person's
    own un-flag revokes their admin keys at once. The
    person's non-admin keys keep today's `disable_user` behaviour.
  - **Bootstrap requires a person.** `POST /v2/bootstrap/claim` requires `person` and `email`, and
    `oml bootstrap claim` and `oml quickstart` require `--person` and `--email`; a claim without
    them is `request.validation` naming the missing field and leaves no tenant. `scripts/quickstart.sh`
    and `scripts/poc-demo.sh` already pass both (`quickstart.sh:85`, `poc-demo.sh:153`); `oml
    quickstart` (`crates/bins/oml-cli/src/main.rs:5305-5318`) and the CI stack and compose jobs
    (`.github/workflows/ci.yml:288,330`) gain them. This reverses ADR-0089's decision of 2026-11-19
    that a bootstrap without `--person` yields an unlinked key, and ADR-0010 d3's issuance of any
    role, admin included, by any admin key (2026-10-29).
  - **What it removes.** The brief's rule 5 ("an unlinked bearer admin key keeps deny, revoking
    non-admin keys and neutral acts") has nothing left to govern: unlinked admin keys do not exist.
    `admin.fix_requires_person` stays only for a key whose link is pending.

  *Rejected:* the brief's recommendation, unlinked admin keys kept for narrowing and neutral acts
  (Q17(a)); unlinked keys confirm-first like any admin's (Q17(b)), under which no person could ever
  confirm as their second.

- **d22 — An admin key may be linked to its own holder's person by possession, in v0.8.6 (decided
  by the operator, 2026-11-25; the precondition of d21).** ADR-0085 d16 refuses a self-link (`self_link`,
  the operator, 2026-11-17) and admits links only from a console admin session. A one-admin tenant
  whose CLI key is not the ceremony's linked key then has no way to link it: the only admin is the
  one who would self-link. This record admits one case: `POST /v2/keys/self/link {person}`,
  called under the admin key itself (possession), names a person and sets a pending link that
  takes effect only when that person accepts from their own console session (consent, d7). Both
  proofs together give no reach the holder lacks: the holder already holds the key, and another
  person's acceptance is still theirs to give. It journals `key.linked_person` with
  `actor: self`. Every other link stays console-only with no self-link. It ships in v0.8.6 only,
  not in v0.8.5, so in a one-admin tenant the CLI key is linked in the upgrade window (Production
  migration): a key suspended as unlinked at the upgrade still reaches this route.

- **d23 — Pending widening is final when declined or expired.** A `pending_confirmation` key
  authenticates nothing on any path (bearer, OAuth token and refresh, connect-code redemption). A
  pending key takes no added grant, no limit change and no connect code. Pending grants live in a
  table of their own, so that no recall, review-queue, count or row-security query folds them in by
  forgetting a filter. Declined and expired are terminal; a late confirm never revives the act.
  Unconfirmed widening expires as declined after seven days, a placeholder that must exceed the
  notice escalation hours. The confirm screen shows role, grants, `used_by`, principal, the root's
  person (d6), the issuing person and the reason, and flags a principal equal to an existing
  person's name.

- **d24 — Offensive narrowing is loud.** Four acts raise a notice and doctor `critical` at once: a
  production fix with `loosens_guardrail`; disabling a person; revoking a person's key; denying a
  reviewer person's key. The notice is written in the act's own commit to every admin person and
  every reviewer person of the workspace concerned, so disabling the reviewers first cannot
  suppress it, and a notice to a disabled person waits for their contest-only sign-in. Doctor
  counts all narrowing acts by one actor person over a rolling 24 hours and flags more than N (a
  placeholder). In a `sole_admin` tenant the only admin recipient is the actor; reviewer persons
  alone see the notices, the price of the exception.

- **d25 — The last live admin person cannot be removed, on any surface.** An act that would leave
  the tenant with no live admin person is refused whatever the caller sends
  (`request.validation`, `details.field: admin`, `details.reason: last_admin`). The guard counts
  admin persons, and with d21 an admin key cannot outlive one. `keep_an_admin` stops being a
  field: v0.8.5 makes the guard unconditional over admin entries (d27 item 4), and v0.8.6 counts
  live admin persons. This reverses ADR-0045 d10's "the CLI under an admin key can still do it"
  (2026-11-04).

- **d26 — Every admin act carries a reason, scanned, in one field shape.** The field follows
  `bulk085/mcp`'s "`reason` when sent" shape (`5c4ce974`), scanned as a verdict's reason. In v0.8.6
  the wire takes a missing reason on narrowing and neutral acts as `reason: null`, counted by
  `admin.act_without_reason`. Widening, consent and against-an-admin acts require one, and the CLI
  prompts on a terminal; a `sole_admin` widening act without one is taken as `null`, so the
  quickstart and CI need no flag. The reason becomes required everywhere in a later named release,
  after the `oml-admin:issue-agent-key` skill and `oml-devops` pass one.

- **d27 — Four pieces land in v0.8.5, in this order (Q16).**
  1. **The read-only production counts** (Consequences, "Production migration"). No code; first,
     because they size everything else.
  2. **The deny's reason is stored** in `key.denied`, scanned like other admin reasons, in
     `bulk085/mcp`'s field shape. This completes ADR-0096 d4, which admits the reason without
     journalling it. It lands on `bulk085/console`, or after both lanes merge, since both change
     `crates/transports/oml-transport-http/src/lib.rs` and `bulk085/mcp` changes
     `docs/reference/event-catalog.md`.
  3. **The docs drift is fixed:** `roles-and-keys.md:84` (it means key revocation; connection
     revocation is built) and `:148` (demotion is built by ADR-0089), and a superseded note on
     ADR-0045 d12's demote route. It rebases after `bulk085/mcp`, which edits `roles-and-keys.md`.
  4. **The last-admin guard is unconditional** on every surface (d25), in `app/emails.rs` and
     `web/emails.rs`, in functions neither lane edits.

- **d28 — Kept, with no question.** ADR-0096 d4's deny on any workspace, gaining a stored reason;
  "an unconfirmed fix never expires" (2026-11-14); the explicit close of an investigation with no
  idle expiry (2026-11-17); ADR-0096 d1, whose admissible set (`reader`, `contributor`,
  `submitter`) the build gives `Issuance::PersonsOwn` as its own instead of the admin ceiling it
  borrows today (`app.rs:3624-3626`); ADR-0095 d3, d5, d6 and d7; ADR-0001 d5 as a whole, its
  exception 3 reading with d4's split shred and d16's person gate.

- **d29 — The admin console reaches parity with the CLI for creating workspaces and issuing keys
  in v0.8.6 (decided by the operator, 2026-11-25).** `<user>-admin` may:
  - **create a workspace anywhere in the tenant**, under `universe` included, which the admin
    Workspaces page refuses today. Creating a workspace is neutral (d9): at once, journalled as an
    `admin.act`, nothing to settle.
  - **issue keys with an admin key builder** that mirrors `oml keys issue`: role, grants
    (workspace, role, node or subtree, deny), default workspace, crew, principal, `used_by`, expiry
    and rate. An admin role takes the person d21 requires. Issuing any key or grant that can read,
    write, approve or ratify is widening (d5): it waits on a second admin person's confirmation,
    takes effect at once marked under `sole_admin` (d10), and a pending key answers `auth.invalid`
    with `details.reason: pending_confirmation` until confirmed (d23). The value is shown once on
    the review step's result, as the personal builder shows it.

  The builder reuses the approved personal KeyBuilder flow (template, form, review), adds the
  admin-only fields and a confirm-pending state; its mockups and design are part of the v0.8.6
  build. The CLI does not change: it already covers both. This reverses ADR-0070 d2's "the admin
  capacity mints no plain key" and ADR-0096 d12's deferral of the admin builder. *Rejected:* keep
  the console issuing nothing and send admins to the CLI, which the operator reversed; an admin
  builder that issues at once, which would bypass d5.

- **d30 — A key can start with its workspaces: existing ones picked at issuance, or new ones
  created in the same act (decided by the operator, 2026-11-25; v0.8.6, CLI and console).**
  Issuing a key through `oml keys issue`, `oml keys issue-agent`, the admin builder (d29) or the
  personal builder (ADR-0096 d1) can:
  - **pick existing workspaces**, each with its grant, and the default workspace. The CLI has this
    today; the admin builder gains it, and the personal builder keeps it inside the own root.
  - **create the key's new workspaces in the same step.** `--create-workspace <parent>/<name>`,
    repeatable, creates the workspace and writes on it the grants the key's role takes by default
    (a contributor reader and submitter, a submitter submitter, a reader reader, a reviewer
    reviewer), covering the subtree. The builders offer the same as a "new workspace" row.

  **The rule: the issuance is one act, and its strictest part decides when it applies.** The
  creations, the key and its grants commit together or not at all. Creating a workspace alone is
  neutral (d9), but inside an issuance it takes the issuance's state: when the key or a grant is
  widening (d5), nothing is created until a second admin person confirms, and the confirmation
  creates the workspaces, issues the key and writes the grants in one commit; under `sole_admin`
  it all applies at once, marked. A pending issuance reserves no name; a confirmation that meets a
  taken name answers `409 name.taken`, applies nothing, and the act waits until it is declined or
  expires (d23). A person's own issuance in their own root creates at once, as ADR-0044 d6 allows
  that person's keys, and is no admin act. Each creation journals `workspace.create` naming the
  `act_id`, and counts against the `workspaces` meter as d31 says. *Rejected:* creating at once and
  granting later, which leaves empty nodes behind every declined key; a creation that applies
  while the grant waits, which splits one act in two; a separate confirmation for the creation,
  which asks a second admin about a neutral act.

- **d31 — A grant may carry `create`, which lets its key create child workspaces under that node
  (decided by the operator, 2026-11-25; v0.8.6, CLI, console and MCP).** `--grant
  <workspace>:<role>:create` on the CLI, a checkbox per grant in both builders, and `create: true`
  on the grant in `POST /v2/keys`.
  - **What it admits.** A key whose effective access at the parent comes from a grant carrying
    `create` creates a child there, on HTTP, the CLI, the console and over MCP (`oml_write
    {create_workspace}`), beside ADR-0095 d3's own-root rule, which is unchanged. The child starts
    as ADR-0095 d3 says: `development`, mode `standard`, no crew.
  - **The key's grant on the child.** Under a grant covering the subtree, the child is already
    covered and no grant is written. Under a node grant, the create writes on the child the same
    grant, role, coverage and `create` included, journalled in the create's commit.
  - **Classes.** Granting `create` is widening (d5), whoever's key it is; a person's own issuance
    sets it only inside the own root, as a person's own act. A child created under the flag is
    neutral and needs no confirmation.
  - **Refusals.** Under a `frozen` parent the create is refused, `access.verb_ceiling` with
    `details.reason: parent_frozen`; under a denied node it is refused as any act there is. An
    admin key never carries a grant and so never the flag; the admin keeps its own create (d29).
  - **The quota.** Each child counts against ADR-0095 d4's `workspaces` meter. A child inside a
    person's root counts against that person's account, as today. **Decided by the operator,
    2026-11-25:** a child in the shared tree counts against the account of the person the creating
    key counts as (d12), and, for a key that counts as no person (a tenant agent key belongs to no
    account, ADR-0071 d1), against the account of the admin person whose act granted the flag.
    **Amended by the operator, 2026-11-27 (answer 8 of the second amendment of that date):** a
    flagged child in the shared tree is charged to no account; only the key's request rate bounds
    it, and doctor's `workspace.flagged_creates` counts it.
  - **Rate.** The key's `requests_per_minute` bounds how fast it creates, as for every create.

  This reverses ADR-0095 d3's "own root only" for flagged grants and its answer Q5 (2026-11-24),
  "no key creates a child in a shared workspace in v0.8.5", and ADR-0044 d6's rule that tenant
  keys create nothing. *Rejected:* creation wherever a key holds submitter (ADR-0095's rejected
  alternative to d3), which needs no grant of the right and lets every writer add permanent nodes;
  a role of its own for creating, which would split every grant table; a flag that is neutral to
  grant, which lets one admin hand an agent unbounded structure in shared trees; letting the child
  carry a wider grant than its parent's, which is escalation by creation.

## Consequences

### Answers that go together

- **Q2 with Q17.** A bootstrap names a person, so `sole_admin` starts at one admin person and the
  zero-person case is gone. The quickstart and CI pass a person (d21).
- **Q2's sticky rule and the confirm-first rule while an act against an admin is open** stop an
  un-flag from turning two admins into a `sole_admin` tenant.
- **Q5 needs Q6.** Without revocation, ending a key is impossible, and un-flagging does not stop a
  bearer admin key. With Q17, suspension and revocation follow the person.
- **Q5(a) needs Q14's settlement verb.** Recovery's dormancy gate never opens during a live
  dispute.
- **Q14 needs Q15's recovery and settlement email**; in-product alone reaches no dormant admin.
- **Q8 uses Q10:** the shred's second admin person counts after seven days.
- **Q4 is refused while an act against an admin is open and during a freeze**, and adds nothing
  under `sole_admin`.
- **Q9 with Q2:** in a `sole_admin` tenant, reach into a person's root takes effect at once,
  marked. In an open-sign-up tenant with one admin, ADR-0068 d3's "no second person can read" a
  root holds only against non-admins; the person is notified in-product (d6).
- **Q12 with Q3:** the ladder ratifier is a counted person, so minted pairs cannot ratify; with
  Q2, a one-person tenant ratifies its own proposal, marked (d17).
- **Q17 with Q13:** service admin keys become `maintenance` keys, so dormancy counts persons only.
- **Q17 with ADR-0085 d16's "no self-link":** a one-admin tenant links its CLI key only by d22, in
  the v0.8.6 upgrade window.
- **Q14 with Q17 and Q2:** recovery keeps the dormant flags, so the recovered tenant leaves
  `sole_admin`; with one dormant admin the way out is an un-flag that the operator settles, and
  with two or more it is an accepted limit (d19).

### Security analysis

- **Escalation.** Every widening path needs a second admin person who counts: minted key pairs,
  reach into a root, connect codes, new admins and limit raises. What remains: two colluding admin
  persons; a patient insider who waits out the seven days (bounded, not prevented); `used_by`
  attested by two admins instead of one.
- **Accepted residual: two admins can act as a person inside that person's root.** Once a second
  admin confirms, keys an admin issued in alice's root, or a connect code for her connector key,
  act like alice's own keys: a contributor and reviewer pair writes and approves there, marked
  `same_user` as own-root work, and alice's agents are served it as her reviewed steering. Reads
  through those keys or that code are not admin reads, so the count alice sees (d16) does not
  include them. The operator accepted this on 2026-11-25 and left it as is (d6). What remains
  visible is the confirmed `admin.act` naming both admin persons and the root; under `sole_admin`
  one admin does it alone, and the person gets an in-product notice.
- **A single admin's unilateral power.** Narrowing stays unilateral, since containment cannot
  wait, and d24 makes it loud. Erase alone destroys, and its shred now waits on a second admin
  person. Acts against an admin are contestable and freeze both sides below three admins.
  `sole_admin` is unilateral by definition, marked, and sticky.
- **Audit gaps.** Closed: unreasoned admin acts, dropped deny reasons, unmarked minted approvals,
  unkillable service admin keys. Remaining: host, database and lake reads below the journal
  (`docs/architecture/storage.md:41`; the lake KEK, `security.md:29`), and owner SQL in the
  runbooks until repair verbs replace it.
- **Lockout and recovery.** The last admin person cannot be removed (d25). Recovery needs 30 days
  of dormancy, a 72-hour wait that any admin sign-in cancels, and email notice (d19, d20). A
  recovered tenant keeps the dormant admins' flags, so its new admin cannot widen alone until a
  dormant admin returns or, with one dormant admin, the operator settles an un-flag (d19). A
  two-admin dispute freezes widening until the operator settles it.
- **Creating children (d31).** The escalation bound: a key creates only under nodes where its
  effective access comes from a grant carrying `create`, and the child carries the same grant or
  is covered by the same subtree grant, so the key never gains a wider role than its grant and
  never grants another key anything. Granting the flag is widening, so one admin cannot hand it out
  alone outside `sole_admin`. Flood limits: the `workspaces` quota, which also bounds depth, and
  the key's request rate; a workspace cannot be deleted, so a child stays for good. In-progress
  parents: a child lies inside the in-progress subtree and gains no audience of its own (ADR-0095
  d3), and granting the flag there is widening that a person holding reviewer there also confirms
  (d5). Frozen parents refuse the create; a `protected` parent admits it, and the child starts in
  `development`.
- **Liveness.** In a multi-admin tenant widening waits on a second admin, who counts only after
  seven days; for the first seven days after a second admin arrives, the founder's widening waits.
  A person's own issuance in their own root never waits.

### Production migration

**Read-only counts first** (d27 item 1), against production by the operator, never by a lane:

1. The server's version, whether open sign-up is on, and which key `signup.issuer` names, with its
   `used_by` and link state.
2. Every admin key: `used_by`, link state, credential, issuer, and which file holds it, both
   deployer files included; whether the operator's CLI key is the bootstrap key and linked; which
   admin keys authenticated in the last 30 days, persons and bearer keys apart.
3. Admin persons per tenant now, and whether a tenant has ever had two, from the journal (this
   decides `sole_admin`).
4. Admin-issued keys naming a `user` other than the issuer's person, reviewer role first.
5. Capacity grants by `<user>-admin` onto `<user>-review` or `<user>-curate` on a node in another
   person's root.
6. Connections opened from codes an admin minted for keys in a person's root.
7. Instruction verdicts and ladder ratifications by keys that count as no one; knowledge verdicts
   in `protected` and `frozen` workspaces by tenant agent reviewer keys, by issuer.
8. Open investigations, their ages, page counts and opening keys; unconfirmed fixes.
9. Live ladder proposals by admin keys, and by keys that count as no one.
10. Every caller of `oml keys issue`, `oml deny set`, `oml keys limit`, `oml bootstrap claim`,
    `oml quickstart` and `oml web allow` in `oml-devops`, the scripts, CI and the operator's skills.
11. Who runs `oml web allow`, how often, and how many entries are admin entries.

**Pre-upgrade operator steps**, before the v0.8.6 server reaches production:

1. **Check how the CLI admin key will be linked** (measurement 2). If it is the bootstrap key,
   linked at the ceremony, nothing is needed. If a second admin person exists, their console links
   it before the upgrade and the operator accepts. In a one-admin tenant neither path exists before
   v0.8.6, since ADR-0085 d16 refuses a self-link, so the key is linked in the upgrade window below.
2. **Ship the deployer step first.** The B′ release carries a new marker, `<!-- oml-deployer:
   post-step maintenance-key -->`. `scripts/release_markers.py` gains it beside
   `admin-rescan-record-held`, appended when the server's key-role set (a constant the build adds
   to the domain crate, read as `RULESET` is) differs from the previous stable tag's;
   `scripts/check_release.py` checks the marker rule in the publish job and a unit test reads the
   real constant. A deployer that does not know the name refuses the release (deployer README
   `:149-150`), so the server cannot reach production before `oml-devops` ships the step.
3. **Configure the mail relay** for recovery and settlement notices (d20), or accept
   `notice.mail_unconfigured` as a known warning.

**Upgrade-window step: link the CLI admin key** that the `oml-admin:issue-agent-key` skill uses
(Q17, d22; decided by the operator, 2026-11-25). After v0.8.6 starts, the operator runs `oml keys
link-self --person <operator>` under that key, which the upgrade suspended as unlinked and which
still reaches `POST /v2/keys/self/link`, then signs in to the console and accepts the link. The
step cannot lock the operator out of a one-admin tenant:

- **The console is the way in.** The operator's console sign-in does not depend on any admin key:
  the server issuer issues `<user>-admin` (d18), and an admin entry accepted before the upgrade
  counts as a confirmed admin flag. Every admin act the console serves, the acceptance included,
  works while the CLI key is suspended.
- **The guard counts persons.** Suspending admin keys removes no admin person, so d25 is untouched.
- **The suspended key keeps its one route** for 30 days (a placeholder), and doctor's
  `admin.key_unlinked_suspended` names it with the days left. Until the link is accepted, the
  issue-agent-key skill answers `auth.invalid` with `details.reason: suspended`, and keys are
  issued from no CLI. A key revoked after the 30 days is replaced by a new admin key issued to the
  operator's own person; that needs a linked admin key, so the 30 days are the window that matters.

**Converted in place, each journalled** at the first start of v0.8.6 or by the deployer's step,
never by a migration `UPDATE` that writes no event:

| What | Becomes | Event | Who writes it |
|---|---|---|---|
| The key in `/etc/oml-deployer/admin.env`, and the key in `/etc/oml/admin.env` | `maintenance`, same hash, so both files keep working through the cut | `key.converted {from: admin, to: maintenance}` | the `maintenance-key` post-step, under each file's key (d18) |
| Any other admin key `used_by: service`, `signup.issuer`'s included | `maintenance`, same hash | `key.converted`, `cause: upgrade` | the server's reserved key at first start |
| An admin key with no accepted link, not converted above | suspended at first start: it authenticates only `/v2/whoami`, `POST /v2/keys/self/link` (d22) and `POST /v2/keys/self/convert` (d18, so the deployer's post-step still converts a file key the first start suspended); revoked after 30 days (a placeholder) if still unlinked | `key.suspended {cause: unlinked}`, then `key.revoked {cause: upgrade}` | the server's reserved key |
| Admin-issued keys in another person's root, capacity grants there, and connections from admin-minted codes for keys there | in a `sole_admin` tenant: `confirmed`, marked `sole_admin`, no change in effect; otherwise `pending_confirmation`, on migration 0064's precedent, until a second admin person confirms (d6) | `admin.act {cause: upgrade}` | the server's reserved key |
| Open investigations opened by a key d16 refuses | closed | `admin.investigation_closed {cause: upgrade}` | the server's reserved key |
| Session keys issued under `entry.added_by` | unchanged until their session ends; the next sign-in issues under the server issuer | none | — |
| Admin entries accepted before the upgrade | count as confirmed admin flags, so the server issuer issues their `<user>-admin` | none | — |
| Live ladder proposals | kept; ratification applies d17 | none | — |
| Earlier `key.denied` events | read with `reason: null` | none | — |

Old journals replay unchanged; the fold computes `sole_admin` from the admin-entry events.

### Doctor lines

`admin.act_pending_confirmation` (count, oldest age); `admin.act_sole_admin` (the tenant and its
marked acts); `admin.act_without_reason` (count); `admin.act_expired` (declined by expiry, last 7
days); `admin.dispute_open` (`critical`, a freeze); `operator.settlement_due` (a freeze open at
least 24 hours); `admin.offensive_narrowing` (`critical`); `admin.narrowing_burst` (more than N
narrowing acts by one person in 24 hours); `admin.key_suspended` (count, by cause);
`admin.key_unlinked_suspended` (from the upgrade, with days left); `admin.erase_shred_pending`
(oldest); `admin.timebox_open` (open boxes and renewals); `admin.investigation_pages` (an
investigation past N pages); `admin.recovery_pending` (`critical`, a waiting period running);
`key.issuer_unexpected_shape` (`critical`, the server issuer issued another shape);
`signup.issuer_ignored` (one release); `notice.mail_unconfigured`; `notice.mail_delivery_failed`.
`oml doctor` learns the `maintenance` role and runs its counts-only reads under it.

### Bottleneck register

Rows the build adds to [limits](../reference/limits.md):

- **`sole_admin`** is folded per tenant from the admin-entry events and kept in a column; a check
  reads one row.
- **To do's "Admin acts to confirm"** reads `admin_acts` by `(tenant, state)`, indexed; one call
  within ADR-0083 d14's counts budget.
- **The narrowing burst** counts by `(tenant, actor_person, at)` over 24 hours, indexed.
- **Authenticate** reads the suspension and pending state from the key row it already reads: no
  added store call.
- **The per-person admin-read count** adds one counter write per admin-read page.
- **The seven-day age** adds one entry read per confirmation.
- **The time box** adds a timer over open boxes, one sweep a minute.

### Tests by sentence

Each red first, named after the sentence it proves:
`every_admin_act_journals_one_envelope_whose_class_the_act_decides`,
`a_widening_act_takes_effect_only_after_a_second_admin_person_confirms_it`,
`a_pending_key_authenticates_on_no_path_until_confirmed`,
`a_declined_or_expired_widening_never_takes_effect`,
`pending_grants_are_served_by_no_recall_review_or_count_query`,
`a_narrowing_act_takes_effect_at_once_and_never_reverts_by_itself`,
`erase_demotes_at_once_and_shreds_only_after_a_second_admin_person`,
`an_admin_key_in_another_persons_root_waits_on_a_second_admin_person`,
`an_admin_connect_code_for_a_key_in_a_persons_root_voids_nothing_until_confirmed`,
`a_link_and_an_address_for_an_existing_person_wait_on_that_person`,
`an_act_against_another_admin_suspends_the_targets_admin_keys_until_settled`,
`a_contest_never_restores_a_revoked_key`,
`below_three_admins_an_act_against_an_admin_freezes_both_sides`,
`the_target_revokes_the_initiator_only_from_the_contest_only_sign_in`,
`a_tenant_that_never_had_two_admin_persons_acts_at_once_marked_sole_admin`,
`a_tenant_that_had_two_admin_persons_never_returns_to_sole_admin_on_an_unflag`,
`a_declined_sole_admin_act_is_revoked_at_once`,
`an_admin_person_accepted_less_than_seven_days_ago_confirms_nothing`,
`an_instruction_verdict_from_a_key_that_counts_as_no_person_is_refused`,
`a_crew_agent_reviewer_key_still_approves_knowledge_in_production`,
`a_time_box_admits_reader_and_submitter_grants_in_shared_workspaces_for_at_most_a_day`,
`a_time_box_is_refused_while_an_act_against_an_admin_is_open`,
`one_revoke_verb_ends_any_key_and_cascades_to_none`,
`revoking_an_admin_key_breaks_no_sign_in`,
`raising_a_limit_waits_on_a_second_admin_and_lowering_it_takes_effect_at_once`,
`the_admin_read_is_refused_to_a_maintenance_key_and_counted_per_person`,
`a_ladder_ratifier_must_count_as_a_person_other_than_the_proposer`,
`an_admin_cannot_ratify_a_proposal_from_an_agent_key_they_issued`,
`the_server_issuer_issues_only_the_four_session_key_shapes`,
`a_maintenance_key_reads_counts_and_never_claim_text_keys_or_emails`,
`an_admin_key_converts_itself_only_to_maintenance_and_keeps_its_hash`,
`recovery_runs_only_after_thirty_days_without_an_admin_person_and_any_sign_in_cancels_it`,
`settlement_confirms_or_reverses_the_contested_act_and_nothing_else`,
`an_admin_key_is_refused_without_a_person`,
`an_admin_persons_own_deactivation_revokes_their_admin_keys`,
`a_bootstrap_claim_without_a_person_is_refused_and_leaves_no_tenant`,
`an_admin_key_links_to_its_holders_person_only_once_that_person_accepts`,
`removing_the_last_live_admin_person_is_refused_on_every_surface`,
`the_maintenance_key_marker_is_appended_when_the_key_role_set_changes`,
`a_reach_into_a_persons_root_under_sole_admin_takes_effect_at_once_and_notifies_the_person`,
`a_sole_admin_ratification_by_the_proposers_own_person_is_admitted_and_marked`,
`a_persons_web_session_revokes_the_persons_own_keys`,
`a_suspended_unlinked_admin_key_still_links_itself_and_converts_itself`,
`an_admin_entry_accepted_before_the_upgrade_still_opens_the_admin_console`,
`recovery_keeps_the_dormant_admins_flags`,
`the_admin_session_creates_a_workspace_under_universe_as_a_neutral_act`,
`a_key_from_the_admin_builder_answers_pending_confirmation_until_a_second_admin_confirms`,
`a_pending_issuance_creates_no_workspace_until_it_is_confirmed`,
`an_issuance_with_new_workspaces_creates_them_and_grants_the_key_in_one_commit_or_not_at_all`,
`a_key_creates_a_child_only_under_a_grant_that_carries_create`,
`a_child_under_a_node_grant_carries_the_same_grant_and_no_wider_role`,
`creating_a_child_under_a_frozen_parent_is_refused`,
`a_flagged_child_counts_against_the_workspaces_quota`,
`oml_write_create_workspace_reaches_a_shared_parent_only_through_the_create_flag`.

### The operator's answers of 2026-11-25 to the tensions

The five tensions the first answers created, and three readings, were put to the operator, who
answered each on 2026-11-25. Each answer is recorded where it applies.

1. **Reach into a root under `sole_admin`:** at once, marked `sole_admin`, and the person is
   notified in-product (d6, d10, d20).
2. **Confirmed keys in a person's root:** left as is. They act like the person's own keys, their
   approvals keep the own-root allowance, and their reads are not admin reads; the operator
   accepted the residual (d6, d16, Security analysis).
3. **The ladder under `sole_admin`:** a same-person ratification is admitted and marked (d17).
4. **Self-link:** confirmed, in v0.8.6 only; the CLI key is linked in the upgrade window, and
   Q16's list stays at four items (d22, d27, Production migration).
5. **Recovery keeps the dormant admins' flags:** the recovered tenant leaves `sole_admin`; the way
   out with one dormant admin is a settled un-flag, and with two or more it is an accepted limit
   (d19).
6. **Readings accepted:** un-flagging revokes or suspends that person's admin keys (d21); a person
   revokes their own keys (d14); under `sole_admin` the founder can still invent a fix settler,
   marked (d11).

7. **Readings decided on 2026-11-26:** the connect code's life runs from the confirmation (d6), and
   the ratifier differs from the admin who issued an agent proposer (d17).

### Pages, API and issues

- **OpenAPI:** `issueKey` and `createWorkspace` admit `<user>-admin` (d29); `issueKey` takes
  `create_workspaces[]` and a grant's `create` (d30, d31); the `oml_write` schema's
  `create_workspace` description names the flag (d31); `revokeKey` (d14), `convertSelf` and `linkSelf` (d18, d22), `confirmAdminAct` and
  `declineAdminAct`, `listAdminActs`, `contest` (d8); `issueKey` requires `person` for an admin key
  and answers `pending_confirmation`; `claimBootstrap` requires `person` and `email`;
  `auth.invalid` gains `details.reason: pending_confirmation` and `suspended`. The Rust and Python
  SDKs are regenerated.
- **Pages:** the admin key builder and the admin Workspaces page's create form (d29); both
  builders' workspace picker, "new workspace" rows and per-grant `create` box (d30, d31); To do's
  "Admin acts to confirm" with the confirm screen of d23; the one-key page's
  revoke button and the person's Keys page's; the contest-only sign-in page; the person's To do
  notice of a reach into their root under `sole_admin`; the limits form's "waits on another admin"; the
  person's count of admin reads of their root.
- **Docs:** `docs/architecture/roles-and-keys.md` (the `maintenance` role, the server issuer, the
  classes), `docs/operations/runbooks.md` (the host verbs), `docs/operations/upgrades.md` (the
  migration above), `docs/reference/event-catalog.md` (`admin.act`, `key.revoked`,
  `key.converted`, `key.suspended`, `operator.recovery`, `operator.settlement`).
- **The CLI and the skill:** `oml keys issue` prints the value once and then "waits on another
  admin" for a pending act; `oml bootstrap claim` and `oml quickstart` require `--person` and
  `--email`; `oml keys issue` and `issue-agent` take `--create-workspace <parent>/<name>` and
  `--grant <workspace>:<role>:create` (d30, d31).
- **Issues:** the build is tracked in milestone v0.8.6; the v0.8.5 pieces in milestone v0.8.5.
- **The cost:** a multi-admin tenant's widening waits on a second person, who counts only after
  seven days; erase no longer destroys at once; a recovered tenant whose dormant admins never
  return cannot widen without the operator; every deployer and bootstrap caller changes once.

### Build notes for v0.8.6, from the v0.8.5 review (2026-11-25)

The three reviews of PR #518 found these, each left for the v0.8.6 build since each changes a
rule this record makes:

- **Disabled admin entries count toward the last-admin guard.** The v0.8.5 guard (d25, d27
  item 4) counts every admin entry that is not pending, a disabled one included, so a tenant whose
  other admin is disabled can still un-flag or remove its one working admin. v0.8.6 counts the
  admin persons d25 names, which leaves a disabled entry out.
- **`GET /v2/review/briefs` serves other persons' proposer key names.** Each proposal carries the
  proposing key's name, which tells a reviewer which key of another person proposed it; ADR-0095's
  `proposed_by` serves the key only to the same person. v0.8.6 aligns the two: the listing names
  another person's key no more than `proposed_by` does.
- **A same-person brief ratification outside a user root.** A person may ratify a brief proposal
  their own other key made when the brief lies outside their root, since the ratify route checks
  the key, not the person. d17's ladder makes the ratifier a person; v0.8.6 refuses the same
  person outside the own root, as a verdict's `same_person` does, or marks it under `sole_admin`.
- **Rotation by an admin of another admin's key or a person's** waits on d6 and d8, as d14's note
  says; v0.8.5 refuses both by name.

## Amendment of 2026-11-27: the core lane's questions (decided by the operator)

Decided by the operator on 2026-11-27, answering the questions the v0.8.6 core lane (PR #524)
left. Each answer is recorded here before the code that takes it.

- **(a) Lifting a denial is narrowing (d3).** It takes effect at once and waits on no one, as the
  denial it lifts did. The class table carries it as `lift_denial`; no admin verb lifts a denial
  yet (the start's rule 5 lift of ADR-0026 d15 is the server's, not an admin act).
- **(b) Declining a `sole_admin` widening act undoes it (d10, d23).** The decline is its own
  narrowing act (`decline_act`), and in its commit it applies the matching narrowing effect, each
  event naming the decline's `act_id`: an issued key is revoked (`key.revoked`); an added grant or
  capacity is revoked (`grant.revoked`, new); a connect code is voided (`connection.code_voided`,
  new) unless already redeemed, when the connection it opened is revoked; a raised limit is set
  back to the limits recorded in the act's target at the act (`key.limits_set`); an admin flag is
  taken off the entry (`web.email_changed` with `admin: false`), under the last-admin guard
  (d25). `not_declinable` is gone: every widening act is declinable while pending or taken at once
  under `sole_admin`.
- **(c) The core lane's six readings are accepted:**
  1. a state change re-journals `admin.act` with the same `act_id`, and the `admin_acts` row is
     the one record of the act;
  2. a decline is its own narrowing act, `decline_act`, journalled before the declined act's
     envelope in one commit;
  3. expiry is computed on read (`state_at`) and not journalled;
  4. pending grants live in `admin_acts.effect`, the table of their own d23 asks for;
  5. an admin's issuance (the act, the key, its grants and its connect code) is one atomic chain;
  6. `<user>-curate` acts count as admin acts.

## Amendment of 2026-11-27 (second): the core2 and issue lanes' questions (decided by the operator)

Decided by the operator on 2026-11-27, answering the questions the v0.8.6 core2 lane (PR #524) and
issue lane (PR #526) left. Each answer is recorded here before the code that takes it. Answers 1 to
7 and 9 are the lanes' recommendations; answer 8 is not, and amends d31's quota sentence.

1. **d8's "three or more admin persons" counts the live admin persons plus every open act's
   target.** A person un-flagged, removed or disabled by an act still open counts toward the three,
   so the act against them does not drop the tenant into the freeze by itself.
2. **Revoking another admin's key is final (d8, d14).** It suspends nothing and opens no dispute or
   freeze: the revocation is the containment, and the target keeps their flag, their entry and their
   other keys.
3. **A removed admin entry keeps its identity in the act's effect (d8).** The `remove_admin` act
   records the entry as it was (email digest, user, admin flag) in its effect, and the contest-only
   sign-in looks the act up by the signing-in address's email digest when no entry matches, so a
   removed admin can contest. A contest of a removal restores the entry with its flag and sign-in,
   as a contest of an un-flag or a disable does.
4. **An admin person disabling themselves is still refused (d8, d21).** A person ends their own
   admin standing by un-flagging or removing their own entry, which revokes their admin keys at
   once; a self-disable is refused, as before.
5. **d4's erase of a target that is not a promoted version records only the pending shred act,
   with no fix record.** The demotion half applies only to a promoted version; anything else has
   nothing to demote, and the act waits on its shred alone.
6. **d21: the person on an admin key must hold an admin flag.** An admin issuance names a person
   whose web entry is live and carries the admin flag; any other person is refused
   `request.validation` with `details.reason: admin_key_requires_admin_person`, naming the person.
   This holds at issuance and at the link's acceptance: a person whose flag came off before they
   accept the link cannot accept it.
7. **A pending key whose default is a workspace the act creates gets the default at confirmation
   (d30).** The pending issuance names no default it creates (the key is read with no default
   while it waits). The confirmation's commit journals a new effect, `key.default_set {key_id,
   default_workspace_id, act_id}`, after the creations, so the key starts with the default its
   issuance asked for. A `sole_admin` issuance sets the default in its own commit, as before.
8. **A flagged (d31 `create`) child in the shared tree is not charged to any account's
   `workspaces` meter; only the request rate bounds it (not the lanes' recommendation).** A child
   inside a person's root counts on that person's meter as before. This replaces d31's quota
   sentence of 2026-11-25 for the shared tree. **Cost:** a tenant agent key holding the flag on a
   shared node is bounded only by its `requests_per_minute` (ADR-0079): at the default rate it can
   add a node a request, without limit over time, and the meter that bounds a person's own tree
   does not see it. The admin who grants the flag accepts that bound, and the doctor counts what
   flagged grants created: `workspace.flagged_creates` (info) lists, per creating key, the
   workspaces created in the shared tree under a `create` grant, so an admin sees the growth the
   meter does not.
9. **Every `create_workspaces[]` parent must already exist (d30).** No creation in one request
   nests under another creation of the same request; a parent the tenant does not hold is
   `request.validation` naming the entry, before anything is journalled.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Model A: patch each hole in place** | It leaves about fourteen paths under about six rules. Issuance stays one admin's act at once, so minted key pairs and new admin persons stay unilateral, and the next admin verb is the next hole. |
| **Model C: every emergency act is an urgent proposal that reverts unless ratified within N hours** | The timer fails open for the acts break glass exists for: an unratified demotion of a leaked key re-serves it, erase cannot join, a one-person tenant loses break glass every N hours, and it reverses "an unconfirmed fix never expires" (2026-11-14). It rewrites v0.8.4's fix build. |
| **C′: C with narrowing never reverting** | It is B′'s rule; B′ adds the envelope and the classes. |
| **The person's consent for reach into their root (Q9(a))** | The operator chose a second admin's confirmation instead (d6). |
| **Admins leave the ladder (Q12(a))** | The operator kept them, with a person as ratifier (d17). |
| **Unlinked admin keys kept for narrowing and neutral acts (Q17(a))** | The operator removed unlinked admin keys altogether (d21). |
| **A zero-person `sole_admin` with an unlinked bootstrap key (Q2(a) as recommended)** | A bootstrap now names a person (d21). |
| **Cascading revocation now** | The operator asked for one plain verb with room to add it later (d14). |
| **A timer that settles a two-admin dispute** | It reverts a fair containment when nobody acts; a freeze with host settlement fails closed (d8, d19). |

## Amendment of 2026-11-26: the public flag joins the widening and narrowing lists (amends d3, d5; ADR-0098)

Decided by the operator on 2026-11-26; recorded in [ADR-0098](ADR-0098-a-protected-or-frozen-workspace-can-be-marked-public-and-then-every-reading-key-in-the-tenant-reads-its-approved-items.md). Designed, not built; it ships in v0.8.6.

- **d5.** Marking a `protected` or `frozen` workspace public (`workspace_public_set`) is widening:
  it takes effect only when a second admin person who counts confirms it, and at once, marked,
  under `sole_admin` (ADR-0098 d6).
- **d3.** Clearing the flag (`workspace_public_clear`) is narrowing: at once, never reverting by
  itself, and it declines a pending set.
