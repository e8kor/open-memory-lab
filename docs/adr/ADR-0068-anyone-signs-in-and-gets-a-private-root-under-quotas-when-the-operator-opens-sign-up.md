<!--
id: adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up
tier: engineering
status: proposed · amended 2026-11-13 (d10: the Start page's heading and step wording follow the voice rule and ADR-0045's header amendment, accepted by the operator on 2026-11-13); the ADR remains proposed
answers: how a deployment opens sign-up to anyone with a Google or GitHub account and keeps self-hosters unchanged; how a self-signed-up person accepts the terms and which version is recorded; which tenant and which admin key a sign-up lands under; why a person's space is a user root and not a tenant of their own, and what proves no second person can see, count or recall it; how a person reviews their agents' writes without breaking R2; which quotas and rate limits hold, what they refuse and how an admin raises them; how a person exports their memories and asks for their account to be deleted, and what deletion does while erasure is unbuilt; the per-address sign-up limit, the disable switch and the absence of anonymous access; the onboarding page and its status poll; what it amends in ADR-0045 and ADR-0058
-->

# ADR-0068 — When the operator opens sign-up, anyone signs in with Google or GitHub and gets a private root under quotas, and leaves through a journalled request

**Status:** proposed · 2026-11-06 · amended by ADR-0071, ADR-0079 · amended 2026-11-13 by [the amendment](#the-amendment-of-2026-11-13-the-start-pages-heading-is-start) (d10: the Start page's heading and step wording, which follow the voice rule and ADR-0045's header amendment, both accepted by the operator on 2026-11-13); the ADR itself remains proposed · the sign-up limits kept by the [operator's decision of 2026-11-14](#operator-decision-2026-11-14-180-the-sign-up-limits-are-kept) (#180)

**Amends:** ADR-0045 (d1: three routes join `WEB_ROUTES`; d2: in open mode a sign-in adds its own entry; d8: one page carries one hashed script; d15: sign-ups are limited per client address), ADR-0058 (d5 is the gate that opening sign-up on the public server waits for)

## Context

The operator wants anyone to start in minutes: sign in with Google or GitHub, get an account and
a key, and connect claude.ai, Claude Desktop, the phone app or Claude Code. The operator chose
"open, with quotas" on 2026-11-06.

Today a person reaches the pages only through an allowlist entry an admin adds
([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
d2). A refused sign-in answers `auth.invalid` `not_allowed`. The first sign-in of an entry issues
`<user>-web` and `<user>-review` under the admin key that added it, which creates the person's
root `universe.<user>` (ADR-0045 d6;
[ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
d2). A user key's grants stay inside its root, plus one reader grant on the `universe` node alone
(ADR-0044 d3). Nothing counts what one person holds. The only rate limits are the provider-call
cap of ADR-0045 d15 and the feedback cap of ADR-0054 d6. Export (`POST /v2/exports`) and erasure
(`POST /v2/memories/{id}/erase`) are designed and not built
([http-api](../api/http-api.md)). Attachments are refused on every write.

[ADR-0058](ADR-0058-the-public-server-runs-to-stated-slos-with-an-error-budget-and-without-backups-until-a-second-person-stores-data.md)
d5 says backups, a rehearsed restore and the backup-failed alert come before the first sign-in
allowlist entry for anyone but the operator. Open sign-up creates such entries, so it is gated on
that work.

The constitution holds over all of it ([ADR-0001](ADR-0001-the-constitution.md)). The pages
are not a surface an agent can call (R1). The reviewing key is never in the row's lineage (R2).
Review changes visibility, not ranking (R3). Nothing is deleted silently (R4).

## Decision

- **d1 — `signup.mode` is `allowlist` by default and `open` only where the operator sets it, with
  the tenant, the issuing admin key and the terms named beside it.** Boot settings:

    | Setting | Default | Means |
    |---|---|---|
    | `signup.mode` | `allowlist` | `allowlist`: ADR-0045 unchanged. `open`: a verified email no entry holds may sign up (d2). |
    | `signup.tenant` | — | the tenant id sign-ups land in; required in `open` |
    | `signup.issuer` | — | the name of an admin key of that tenant, the actor of every sign-up and the issuer of its session keys; required in `open` |
    | `signup.terms_version` | — | the version a person accepts, recorded on their entry; required in `open` |
    | `signup.terms_url`, `signup.privacy_url` | — | the pages the terms checkbox links; required in `open` |
    | `signup.per_ip_per_hour` | 5 | sign-ups admitted from one client address in a rolling hour (d9) |
    | `quota.memories`, `quota.keys`, `quota.connections`, `quota.requests_per_minute` | 1000, 5, 5, 120 | the per-person defaults of d6 |

    `open` without a sign-in provider (`web.google.*` or `web.github.*`) stops the start too, since
    the pages that carry sign-up would be off.

    `open` without one of the required values stops the process at start, naming the setting,
    as a provider without `oauth.issuer` does (ADR-0045 d17). The tenant and the key are checked
    at start: an unknown tenant, or a key that is not a live admin key there, stops the start.
    A self-hoster who sets none of these keeps an allowlist deployment: the same sign-in, no quotas and no
    rate limits. The pages gain a Start tab, and the landing route sends an allowlisted person who has no
    agent key to `/web/start`. The
    quota defaults are the operator's choice of 2026-11-06. Whether they fit real use is
    unmeasured ([#164](https://github.com/e8kor/open-memory-lab/issues/164)).

- **d2 — In open mode, a provider-verified email that no entry holds reaches a terms page, and
  accepting it creates the entry, the root and the keys in the sign-up tenant.** The callback
  admits the provider's answer exactly as ADR-0045 d3 and d4 do: the sealed `state`, a verified
  email, GitHub's primary verified address. Where no entry holds the email and the mode is
  `open`, the lead does not answer `not_allowed`. It seals `{provider, subject, email,
  started_at}` under its own purpose string into `__Host-oml_signup` (`Secure`, `HttpOnly`,
  `SameSite=Lax`, ten minutes). It then renders one page: the terms and privacy links, the terms
  version, one unchecked checkbox `accept_terms`, and one button. `POST /web/sign-up` passes the
  same-origin check, opens the cookie, and requires `accept_terms=yes` and a `terms_version`
  equal to the current one. A changed version is refused `request.validation` with
  `details.field: terms_version`, so a person never accepts a version they were not shown. It
  then takes one sign-up slot for the client address (d9), and in one act:
    - adds the entry `{email, user, admin: false}` under `signup.issuer`, journalled as
      `web.signed_up` with the email digest, the user, the provider, the terms version and
      `self_signup: true`, never the address;
    - pins the provider subject (ADR-0045 d4);
    - issues `<user>-web` and `<user>-review` by the ADR-0045 d6 rule, which creates
      `universe.<user>`;
    - opens a session and answers `303 /web/start`.

    **The user is never derived from the email.** It is `u` followed by ten Crockford base32
    symbols drawn from the process's random source, so ADR-0045 d2's rule holds. No workarea or
    key name carries the address. A drawn name that is already taken is drawn again, at most
    three times, and then refused `name.taken`. The cost is a name a person does not choose.
    Choosing one is a later decision, since a chosen name is a namespace claim another person
    can collide with.

    The sign-up never takes an address an entry holds, in any tenant and with any flag. An entry
    created after the cookie was issued, by another sign-up or by an admin, makes the `POST`
    answer `auth.invalid` (`state_mismatch`) and clear the cookie, and the person signs in
    through the callback, which pins the provider subject (ADR-0045 d4). The `POST` opens no
    session for an existing entry, so a cookie issued for an address while it was unlisted
    cannot be replayed into the session of whoever holds the address afterwards.

    A refused or abandoned terms page writes nothing. A sign-up never sets `admin`. An admin
    entry still comes only from the CLI or the admin page.

- **d3 — A self-signed-up person's space is their user root, and no second person can read,
  write, count or recall anything in it.** ADR-0044 d3 already confines every user key, and every
  key ADR-0045 d6 issues for a person, to that person's root and the `universe` node. A tenant per
  person was weighed and rejected (rejected alternatives). Isolation is proved by one test over
  every surface that exists today. A second person signed up the same way, through their own
  agent key and their own session:
    - recalls nothing of the first person's (`POST /v1/memories/search` and the MCP read tool);
    - gets `access.not_found` for the first person's memory by id;
    - sees no node of the first person's root in the workarea tree;
    - lists none of the first person's memories, keys or counts on `/web` and `/web/start`;
    - is never served the first person's held rows for review.

    The Atlas of the web-memories lane (ADR-0045 d22–d31) draws only the workareas its viewer
    may read. Its counts join this test when that lane lands.

    The sign-up tenant's admins are the operator. They read every memory through the journalled
    admin read (ADR-0045 d21). The terms say so, and the `/web/start` page links them.

- **d4 — A person reviews their agents' writes through `<user>-review`, a key that is not the
  author, and every such verdict is marked `same_user`.** An agent key a person issues is
  `<user>-<name>`, with principal `<user>-<name>`. Their reviewer key is `<user>-review`, with
  principal `<user>-review`. R2 excludes the keys and principals of the row's lineage closure
  (ADR-0044 d8), so `<user>-review` may approve an agent's write and is refused any row it
  wrote or proposed. The person's own direct writes from the pages run as `<user>-web`, also a
  different key from the reviewer, under the same rule. Every promotion, transition and
  ratification where author and reviewer name one person carries `same_user: true`, and
  `self_signed: false`
  ([ADR-0046](ADR-0046-the-lifecycle-claim-edges-and-the-brief-ship-in-v0-5-2-inside-user-roots-with-no-new-tool.md)
  d2; ADR-0055 d7). An auditor reading the export sees a person approving their own agents'
  work, and the pages say "(you)" beside such a verdict. Nothing here self-signs. ADR-0001 d5's
  development exception is untouched. The cost is the one ADR-0044 d8 states: R2 holds per
  credential, not per person, inside a user root.

- **d5 — Every person of the sign-up tenant is under quotas, and an admin raises a person's
  quota as a journalled act.** A quota applies to a user, the root a user key names, in the
  tenant `signup.tenant` names. Tenant keys and every other tenant are unaffected. An allowlist
  deployment has no sign-up tenant and so no quotas.

    | Quota | Counts | Checked at |
    |---|---|---|
    | `memories` | distinct memories (artifacts) whose version lies in the root or below it, every state | each new memory written by a user key of the root; a new version of an existing memory is not a new memory |
    | `keys` | the root's live bearer and connector keys; session keys are not counted | each key issued for the user (`issue_key`, `connect_key`, `/web/start`) |
    | `connections` | open OAuth connections of the root's connector keys | a token exchange that opens a connection on a key with none open; a replacement replaces |
    | `requests_per_minute` | requests one key of the root makes in a fixed UTC minute | every authenticated request of that key, on HTTP and MCP |

    The `memories` check runs after the idempotency lookup and after the live `claim_key`
    refusal, and before the commit. A retry of a write that succeeded is its stored receipt, never
    `limit.quota`. A `remember` under a claim key that is live in the workarea is the
    `submission.claim_key_conflict` it always was, not a quota refusal. A new version is made by
    `supersede`, which extends the chain of an existing memory and adds none, so it is not metered:
    the meter counts distinct `(workarea, claim_key)` chains, and a supersede, a rejection or a
    retirement leaves that count where it was. A person at the limit can still revise and retire
    what they hold. Quotas are read before the write commits and are not held across it, so
    concurrent writes at `limit - 1` can each pass: the memories count is exact to within the
    requests in flight, at most the person's keys times `requests_per_minute`.

    Attachments are refused on every write, so no attachment quota exists. One joins with
    attachments. Past a quota the act is refused `429 limit.quota` with
    `details: {quota, limit, used}` and journals nothing. Past the rate it is `429 limit.rate`
    with `details: {quota: "requests_per_minute", limit}` and `Retry-After` set to the seconds
    left in the minute. Rate counters live in the process, one per replica, the approximation of
    FR-8.

    An admin raises or lowers a person's quota on `/web/admin/users` (`set_quota`,
    `{user, quota, limit}`, `limit` from 0 to 1,000,000, empty to return to the default). The act
    is journalled as `web.quota_set` by `<admin>-admin` and folded into `user_quotas`. The new
    limit holds from the next check. A rate counter reads it at its next minute.

    Every refusal increments `oml_quota_refused_total{quota}` on the metrics listener. Labels
    are the closed set of the four quotas (ADR-0042 d12). `oml doctor` under an admin key prints
    `warn quota: <n> users at or above 90% of a quota (<quota>: <n>, …)` from
    `GET /v2/web/users`, and nothing when none is.

- **d6 — A person exports their memories as JSON from `/web/start`, and deletion is a
  journalled request the admin sees and acts on, never a silent removal.**
    - **Export.** `GET /web/start/export` answers `application/json` with
      `Content-Disposition: attachment`. It holds every memory in the person's root and below:
      text, workarea path, state, claim key, version and timestamps, plus the person's keys
      (name, role, credential, created, cut off) and their entry's terms version. It reads as
      `<user>-web`, own set every key naming the user, so held and rejected rows are included
      (ADR-0045 d16). Nothing a second person wrote is in it, since nothing a second person
      wrote is in the root. It is the person's subset of the designed export
      (`POST /v2/exports`), served before that lands. The designed bundle, with its signatures
      and the chain, stays FR-29's.
    - **Deletion.** `request_deletion` on `/web/start`, with the confirmation field
      `confirm=delete`, journals `web.deletion_requested` under `<user>-web`. It folds a row
      into `web_deletion_requests`, ends nothing and changes no access. The person's agents keep
      working until an admin acts, and the page says so. `/web/admin/users` lists pending
      requests with their age. The admin's `close_deletion` disables the person (d8) and
      journals `web.deletion_closed`. The bytes stay until erasure is built
      ([ADR-0005](ADR-0005-erasure-by-crypto-shredding.md); `POST /v2/memories/{id}/erase` is
      designed). The operator then erases the person's memories with `oml erase`. Until then the
      closure is a cut-off with the content kept, and the terms say so. That is R4: never
      silent, and never a delete. `oml doctor` under an admin key warns
      `warn signup: <n> deletion requests pending, oldest requested <time>`.

- **d7 — There is no anonymous access.** Every page but the sign-in page and the terms page needs
  a live session. Every API and MCP route needs a key or a connection, as before. The status
  poll (d10) answers only a session. The sign-in page shows no count, no name and no memory. A
  test asserts that `/web/start`, its status, its export and `/web/admin/users` answer the
  sign-in page or `access.not_found` without a session.

- **d8 — An admin disables a person with one act, and a disabled entry signs in to nothing and
  cannot sign up again.** `disable_user` on `/web/admin/users` (`{user}`) runs as
  `<admin>-admin`:
    - it journals `web.user_disabled` for the user;
    - it marks every entry of that user `disabled_at`, which ends their sessions in the same
      commit;
    - it cuts off every live bearer and connector key of the root by the existing global denial
      on `universe` (ADR-0045 d13), one `key.denied` each.

    A disabled entry's sign-in is refused `auth.invalid` with `details.reason: disabled`.
    Because the entry and its email digest stay, an open-mode sign-up with the same address
    meets the entry and is refused the same way, and the person cannot sign up again under a new
    root. An admin re-enables only by removing the entry, which ADR-0045 d2 already journals; a
    later sign-up then creates a new root, and the old one stays cut off. The cost is that
    "disabled" is a cut-off and not a revocation. Revocation is designed and not built.

    Each `key.denied`, and the closing commit of `close_deletion` (d6), retries alone on a
    chain-head conflict, so one request journals each of its events at most once
    ([#266](https://github.com/e8kor/open-memory-lab/issues/266)). No refusal follows the first
    commit. An admin key whose row names the root is refused before it: no verb issues one and
    no denial cuts one off, so the row is corrupt, and the disable answers 409
    `workarea.admin_key_under_root` naming the key and journals nothing until the row's
    `user_root_id` is cleared ([#277](https://github.com/e8kor/open-memory-lab/issues/277)).
    A key gone between the listing and its denial, or made an admin key by a row edit in that
    window, is skipped and the rest are denied; the answer's `keys_cut_off` leaves it out and
    names it nowhere else. A conflict that outlasts the retries answers `system.unavailable`. The disable it
    journalled stands, and the act's claim on its `Idempotency-Key` is kept, so the same key
    answers `idempotency.in_progress` and a new key finishes the cut-off. So a 4xx from
    `disable_user` or `close_deletion` always means nothing was journalled.

- **d9 — Sign-ups are limited per client address, at the terms page's `POST`.** The address is
  the request line's `client_ip` under `server.trusted_proxies` (ADR-0042 d7). ADR-0045 d15
  declined address limits because behind a proxy every request shares the proxy's address. With
  trusted proxies set, the address is the client's; unset, every sign-up shares one bucket,
  which fails closed, and `oml doctor` says so: `warn signup: open with no
  server.trusted_proxies; every sign-up shares one address limit`. Past
  `signup.per_ip_per_hour` the page answers `429 limit.rate` with `Retry-After` and journals
  nothing. Counters live in the process. A client is an IPv4 address or the /64 of an IPv6 address, since
  a host holds a whole /64 and a per-address bucket would not bound it; an IPv4-mapped IPv6
  address counts as its IPv4 one. A sign-in of an existing entry is never counted, since
  only a new account is the thing abused.

- **d10 — `/web/start` is the first page after a sign-up: three steps, one hashed script for copy
  and the status poll, and the person's account acts.** Routes added to `WEB_ROUTES`, amending
  ADR-0045 d1:

    | Route | Answers |
    |---|---|
    | `POST /web/sign-up` | the terms page's act (d2) |
    | `GET /web/start` | the onboarding page; the sign-in page without a session |
    | `POST /web/start` | its acts: `issue_claude_ai`, `issue_claude_code`, `issue_other`, `request_deletion` |
    | `GET /web/start/status` | `{connected, first_memory}` as JSON, for the session's user |
    | `GET /web/start/export` | the export of d6 |
    | `GET /web/admin/users` | the users, their quota use, pending deletion requests |
    | `POST /web/admin/users` | `set_quota`, `disable_user`, `close_deletion` |

    The page has one heading and one line per step *(amended 2026-11-13: the heading's word; see [the amendment](#the-amendment-of-2026-11-13-the-start-pages-heading-is-start))*:
    1. **Choose your client**: claude.ai, Desktop or phone; Claude Code; other MCP tools.
    2. **Get your credential**: one button per client.
        - claude.ai: issues a connector key `<user>-claude-ai` and shows its connect code once,
          the exact MCP URL `<oauth.issuer>/mcp`, copy buttons, the three clicks in claude.ai
          (Settings → Connectors → Add custom connector), and the preferences snippet of
          [claude.ai and mobile](../guides/claude-ai-and-mobile.md) behind a copy button.
        - Claude Code: issues `<user>-claude-code` and shows the value once with the plugin
          install commands of [Claude Code](../guides/claude-code.md), the key going into the
          plugin's options.
        - Other tools: issues `<user>-agent` and shows the URL and key with a link to the agent
          kit.
       A name already taken gains `-2`, `-3`.
    3. **Wait for the first connection**: "Waiting for your first connection…", then
       "Connected ✓", then "First memory held — review it", linking `/web?view=review`.

    The page polls `GET /web/start/status` every five seconds. Its answer is `connected: true`
    once any of the person's keys has authenticated a request since this process started, or
    once a connector key has an open connection. It is `first_memory: "held" | "promoted" | null`
    from the person's newest memory. Without a script the page still works and shows the state
    at load. The poll and the copy buttons need script, which ADR-0045 d8 kept off the pages.
    This page alone carries one inline script, its SHA-256 named in its own CSP
    (`script-src 'sha256-…'`, `connect-src 'self'`). The script reads one same-origin JSON
    route that a session cookie opens, and it posts nothing. It adds no machine interface an
    agent could not already reach by driving the browser (ADR-0045 d7). The "connected" signal
    lives in the process and is lost on restart, where the first memory still shows. Recording
    when a key was last used is FR-75's.

    Every page's tabs gain "Start" (`/web/start`), the one change to the shared page chrome.
    After a sign-up, and after any sign-in whose user holds no bearer or connector key yet, the
    callback answers `303 /web/start` in place of `/web`.

- **d11 — The new failures, their refusals and their doctor lines.**

    | Failure | Refusal | Doctor line |
    |---|---|---|
    | `open` without `signup.tenant`, `signup.issuer`, `signup.terms_version`, `signup.terms_url` or `signup.privacy_url`, or with an unknown tenant or a key that is not a live admin key there | the process stops at start, naming the setting | — |
    | the terms unaccepted, or a changed terms version | `request.validation`, `accept_terms` / `terms_version` | — |
    | the sign-up cookie absent, unopened or expired | `auth.invalid`, `state_mismatch` | — |
    | the per-address limit | `limit.rate` with `Retry-After` | `warn signup` when no trusted proxy is set (d9) |
    | a quota reached | `limit.quota` with `{quota, limit, used}` | `warn quota` (d5) |
    | a key's request rate | `limit.rate` with `Retry-After` | — (metrics) |
    | a disabled entry | `auth.invalid`, `disabled` | — |
    | deletion requests pending | — | `warn signup` with the count and the oldest request's time (d6) |
    | an admin key whose row names a person's root | `workarea.admin_key_under_root` on that person's disable or deletion close, nothing journalled (d8) | `warn keys` naming the key and the person, from `GET /v2/web/users`' `admin_keys` |
    | open sign-up on a deployment with no backup | — | — (an ADR-0058 d5 rule; the production page checklist) |

## The amendment of 2026-11-13: the Start page's heading is Start

**Status of this amendment:** accepted design · 2026-11-13, by the operator, before it is built · built in v0.7.3 with the console redesign

The mockups of the console redesign draw the Start page under the header of
[ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)'s
header amendment of the same date, whose rule is that the `<h1>` is the nav item's own word. The
operator decided on 2026-11-13, after seeing them, that the build ships in v0.7.3, and, on
2026-11-12 and 2026-11-13, that UI copy is impersonal and neutral ("dont use you … neutral tone"),
with the rule "everywhere". This amendment records what that does to d10, in its heading's word
and its step headings only.

| Decision | Was | Is | Why |
|---|---|---|---|
| d10 | the page's one heading is "Connect an agent" | the `<h1>` is "Start", and the one line under it reads "Connect an agent: three steps, about two minutes." The page still has one heading and one line per step | the heading is the nav item's word, as on every page of both sides |
| d10 | step 1 is "Choose your client" and step 2 "Get your credential" | the step headings and their buttons carry no second person, under the voice rule of [design/README.md](../../design/README.md). The words are the console copy the build writes, and are the build's: this record does not fix them | the voice rule applies to the console everywhere; ADR-0068's other decisions are unchanged |

Three things are not changed: the three steps, one button per client and what each issues, the
status poll and the account section's acts. The documentation's rewrite to the same voice is
[#357](https://github.com/e8kor/open-memory-lab/issues/357)'s, not this amendment's.

## Operator decision 2026-11-14 (#180): the sign-up limits are kept

**Status of this record:** the operator's decision of 2026-11-14, on
[#180](https://github.com/e8kor/open-memory-lab/issues/180), for opening sign-up on the public
server ([#186](https://github.com/e8kor/open-memory-lab/issues/186)). It changes no decision above.

The operator decided to open sign-up on d1's defaults: `quota.memories` 1000, `quota.keys` 5,
`quota.connections` 5, `quota.requests_per_minute` 120 a minute per key, and
`signup.per_ip_per_hour` 5 sign-ups an hour from one client address (d9). The two meters
ADR-0071 added are capped beside them, `text_bytes` at 50 MB and `writes` at 10,000 a month. Recalls
have no cap, since reading never blocks; 100,000 a month is the threshold #164 watches. ADR-0071's
[record of the same date](ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md#operator-decision-2026-11-14-180-the-limits-open-sign-up-launches-with)
gives the settings and the reasons. Every value is recalibrated after 30 days of live data under
[#164](https://github.com/e8kor/open-memory-lab/issues/164). Turning `open` on stays the
operator's act under ADR-0058 d5 (Consequences).

## Consequences

- Migration 0037 adds `web_emails.terms_version` and `web_emails.disabled_at`, the tables
  `user_quotas` and `web_deletion_requests` under the tenant RLS of 0015, and the serving grants.
- Five journal events join the [event catalog](../data/event-catalog.md): `web.signed_up`,
  `web.quota_set`, `web.user_disabled`, `web.deletion_requested`, `web.deletion_closed`.
- `limit.quota` is served for the first time ([errors](../api/errors.md)), with
  `details.limit` and `details.used` beside `details.quota`.
- The quickstart and [claude.ai and mobile](../guides/claude-ai-and-mobile.md) open with "Sign
  up at memory.doublegate.io, then follow /web/start" for the public server. The self-hosting
  guide documents the settings.
- Turning `open` on for the public server is the operator's act. It follows ADR-0058 d5's
  backups and restore rehearsal, and the terms and privacy pages drafted in
  `docs/legal/` on the connector-directory lane.
- [#164](https://github.com/e8kor/open-memory-lab/issues/164) tracks whether the default quotas, the per-key rate and the per-address sign-up limit
  fit real use is unmeasured.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **A tenant per person** | Row security would isolate each person by construction. But a tenant is created by bootstrap with its own admin key, its own chain and its own partitions, so every sign-up would mint an admin key nobody holds, a chain the operator signs, and partition DDL under the owner role (ADR-0034 d11), all from an anonymous request. ADR-0044 d3 already confines a user key to its root, and the isolation test proves it on every surface. Reopen if a person needs their own admins or crews. |
| **Derive the user from the email's local part** | Readable, but it puts personal data into workarea and key names the journal keeps forever, collides across domains, and breaks ADR-0045 d2's "never derived from the email". |
| **No terms step: sign-in is consent** | Consent would not be recorded and no version would be pinned. When the terms change, nothing would show who accepted which. |
| **Open sign-up without quotas, relying on the disable switch** | One script with a GitHub account could fill the store before anyone looks. Quotas fail closed per person, and the switch is for what quotas miss. |
| **Quotas as journalled grant attributes on each key (errors.md D6)** | The designed home, but a person's memories are written by several keys, so a per-key budget does not bound a person. Grant attributes are not built. The per-user override is journalled, so every decision stays reproducible from the journal. |
| **Delete the account on request, immediately** | R4 forbids silent removal. Erasure is designed and not built. A request the admin sees and closes keeps the act on the record and leaves the bytes to the one erasure ceremony. |
| **A meta refresh in place of a script for the status** | It reloads the page every few seconds and wipes a key shown once before the person copies it. |
| **WebSockets or server-sent events for the status** | A long-lived connection per open tab for a state that changes twice. A five-second poll of a small JSON route costs less and survives proxies. |
| **Limit sign-ups by provider account age or a CAPTCHA** | Neither provider states account age in what OML reads, and a CAPTCHA loads a third party's script into the page. The per-address limit and quotas come first. A CAPTCHA can follow on evidence. |
