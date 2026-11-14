<!--
id: adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself
tier: engineering
status: accepted design · 2026-11-12
answers: how a key is given an end date and a request rate; what a request under an expired key is answered and how that differs from a revoked key; what an expired key's connection does; who may change a key's date or rate and why the key itself never may; why an admin key carries neither; where the rate is counted and what that means with more than one server process; what stays of an expired key; and the operator's answers to its five open questions
-->

# ADR-0079 — A key ends on a date and is held to a request rate, both set at issuance and changed only by an admin key that is not the key itself

**Status:** accepted design · 2026-11-12, by the operator, as built, with its five open questions decided at their recommendations (1: a connection has no date apart from its key's; 2: a rate every server process shares is decided with ADR-0073's nodes; 3: a person sets a date on their own keys from the web console when the keys page is next changed, and changing it afterwards stays with the admin key role, built in v0.7.3 (#352); 4: an expired key stays on its account's `keys` meter; 5: when the operator path is built, a date and a rate are refused on every role that can issue an admin key, before the first such key is issued) · proposed the same day with #323

**Release:** v0.7.3 (#323)

**Amends:** ADR-0025 (d2: the `expiry` it reserved on `POST /v2/keys` is served, and `key.issued` gains `requests_per_minute`), ADR-0031 (d11: a connection whose key expired works again if an admin moves the key's date before its refresh token lapses), ADR-0068 (d5: a key is also held to a rate of its own, under any account's)

## Context

#185 hands a key to a person outside the operator's organisation, the Connectors Directory
reviewer, and asks that it be "rate-limited, revocable and expires after the review". Revoking
works: `oml deny set <key> universe` fences the key and `oml keys disconnect` ends its connection.
The other two do not exist (#323):

- **No key ends on a date.** [ADR-0025](ADR-0025-the-agent-only-memory-surface.md) d2 reserved
  `expiry` on `POST /v2/keys` and in `key.issued`, and the server refuses the field by name,
  because a key minted without the date its issuer set would hold its access longer than meant.
  [ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) d8 and d11 already say what an
  expired key's connect code, authorization code, refresh token and access token answer;
  [ADR-0026](ADR-0026-names-are-keys.md) d10 already says its name is never released. A connect
  code's `--ttl` ([ADR-0067](ADR-0067-a-directory-reviewer-reaches-a-demo-workspace-through-a-connect-code-that-lives-longer-and-the-tools-and-instructions-describe-instead-of-command.md)
  d3) bounds when a connection may open, not how long it lasts.
- **No key has a rate of its own.** [ADR-0068](ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md)
  d5 counts `requests_per_minute` for the keys of a person of the sign-up tenant, from that
  person's quota. A tenant key, which is what an operator issues for a demo workspace, is under no
  rate, and [security](../architecture/security.md) lists that as limit 7.

So this record decides the parts those records left open: where the date and the rate are set,
what a request past the date is told, who may change either, and where the rate is counted. It
is held against R1, the gate is not callable by the thing being gated, and R4, nothing is
deleted silently ([ADR-0001](ADR-0001-the-constitution.md)).

## Decision

- **d1 — A key may carry an expiry, an instant, set when it is issued.** `POST /v2/keys` takes
  `expiry`, an RFC 3339 instant with its offset, later than the lead's clock at the request;
  anything else is `request.validation` naming `/expiry`, and nothing is journalled. `key.issued`
  journals it in the `expiry` member it has always carried as null. A key issued without one does
  not expire, as before. A session key carries none: it ends with its web session
  ([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d6). A person's own issuance from their web session sends `name`, `role` and `connect_code`,
  and `expiry` where the person ends the key on a date, under this rule; it takes no
  `requests_per_minute`, which is `request.validation` naming the member (The operator's
  answers, 3; built in v0.7.3, #352). The cost: a date is an instant, so an operator who means "the end of
  Friday" writes the instant; a bare date is refused, not read as some midnight.

    An admin key carries neither limit, this date or the rate of d4. `POST /v2/keys` for role
    `admin` with `expiry` or `requests_per_minute` is `request.validation` naming the member,
    with `details.reason: admin_key`, and nothing is journalled; the store refuses the row too
    (`api_keys_admin_key_has_no_limits`). The limits are for keys of the other roles. The
    reason is d5: the admin key role is the one that changes limits, so a limit on an admin
    key is one its holder lifts, and a date one admin key may set on another is a way to end
    it that no record decided. The operator decided this on 2026-11-12. The cost: no admin key
    ends on a date, so a temporary admin key is not possible.

- **d2 — From its expiry on a key authenticates nothing, and the refusal says so.** A request
  that presents the key's value at or after its expiry is `401 auth.invalid` with `details:
  {reason: "expired", expiry}`, where an unknown and a revoked key answer `reason: "unknown"`.
  The date is told only to a caller that presented the key's own value, or a live access token
  of its connection, which is the key's holder; a guessed value still learns nothing. The lead
  reads the key's row and compares its clock on every request, so no cache holds a key past its
  date. A connection is exactly its key (ADR-0031 d11), so "a key or connection expiry" is one
  date, the connector key's: its access token answers the same `auth.invalid` at `/mcp`, with
  the challenge of ADR-0031 d2; a code entered for it answers `oauth.connect_code_invalid`; its
  authorization code and refresh token answer `invalid_grant`; and a connect code asked for it
  is `oauth.connect_refused` with `details.reason: expired`. All of those were decided by
  ADR-0031 and are built here.

- **d3 — An expired key is kept and shown as expired.** Its row, grants, denials and name stay
  (R4; ADR-0026 d10). `GET /v2/keys/{key}` serves `expiry` and `expired`, each item of `GET
  /v2/keys` serves the same two, `GET /v2/whoami` serves the calling key's `expiry`, and `oml
  keys show` prints the date and says when it has passed. Each also serves the key's
  `requests_per_minute`. Nothing is journalled at the instant
  itself: the date was journalled when it was set, and a reader of the journal compares it with
  the next event's time. The cost: an expired key of an account still counts on that account's
  `keys` meter ([ADR-0071](ADR-0071-every-account-is-metered-by-counters-the-write-updates-in-its-own-transaction-and-named-quota-profiles-set-its-limits.md)
  d2), because d5 lets an admin give it a later date.

- **d4 — A key may carry a rate of its own, in one unit: requests per minute.** `POST /v2/keys`
  takes `requests_per_minute`, a whole number from 1 to 1,000,000; a quota's limit runs from 0
  to 1,000,000 (ADR-0068 d5), so the two share the upper bound and a key's rate is never 0.
  `key.issued` journals it for a key that carries one. It is counted as ADR-0068
  d5 counts an account's rate: the requests the key makes in a fixed UTC minute, each request
  that reaches a use case as that key, over HTTP and at `/mcp`, by value or by connection. The
  request past the limit is `429 limit.rate` with `details: {quota: "requests_per_minute",
  limit}` and `Retry-After` set to the seconds left in the minute, and it is not counted. A key
  of an account under a quota is held to both rates and the lower one refuses first. The key's
  own rate is counted before the account's, so a request the account's rate refuses still
  consumes one of the key's own. The count is of use cases, not of HTTP requests: a request
  that runs more than one is counted once for each. `GET /v2/keys/{key}` runs three use cases
  and would cost a caller held to a rate three counts (#341); the route takes the admin key
  role, which carries no rate of its own (d1), so no key pays three today, and a key of
  another role is refused by the first use case and counted once. There is
  one unit and one window: no burst allowance, no per-operation limit, no tier. A frame at
  `/mcp` that calls no tool (`initialize`, `tools/list`, `ping`) reaches no use case and is not
  counted, as today; a flood of those, or of requests with no valid key, is the edge's to stop.

- **d5 — The date and the rate change only by an admin key that is not the key itself, and
  never on an admin key.** `POST /v2/keys/{key}/limits` takes `{expiry?, requests_per_minute?}`,
  at least one member: a member sent replaces the value, `null` clears it, and a member not
  sent keeps it. It needs the admin key role, the role that already fences a key and ends its
  connection, in the key's own tenant, and journals `key.limits_set` with `key_id`,
  `set_by_key_id` and both values as they stand after the change. A key of any other role is
  `access.verb_ceiling`; a key the tenant does not hold is `access.not_found`; and four
  targets are `request.validation` naming `key`, with `details.reason`: `own_key` (the calling
  key), `session_key`, `revoked`, and `admin_key` (any admin key, whether the change sets a
  limit or clears one). No MCP tool reaches the route, so an agent's surface cannot ask for it
  at all. No key that can change limits ever carries one: the route takes the admin key role,
  and an admin key is issued with neither limit (d1) and given neither here. So the gate is
  not callable by the thing being gated (R1): a key of any other role, which is every key
  that carries a date or a rate, never reaches the route, and an admin key, whoever holds it,
  has no date to move and no rate to raise, by itself or through a second admin key it
  issues. Nor does one admin key end another by setting a near date on it. A new `expiry` is
  later than the lead's clock, as at issuance; to end a key now, fence it or end its
  connection, which every key that carries a limit admits, since a denial is refused only on
  an admin key. Moving an expired key's date later makes it authenticate again: an expiry is a
  date an admin holds, not an act like a revocation. The rate a key is held to is read from
  its row on each request, so a change holds from the next request. A person who set a date
  at their own issuance (d1) does not change it: `<user>-web` is no admin key, so it is
  `access.verb_ceiling` here like any other key that is not one.

    This amends ADR-0031 d11's "one whose key ended is dead for good" for an expired key: its
    connection stays `open` with no event, as that record says, and works again if the date is
    moved before its newest refresh token lapses. A client that was answered `invalid_grant`
    meanwhile has dropped its tokens and needs a new connect code.

- **d6 — The rate is counted in the memory of the lead process that serves the request.** It is
  counted as ADR-0068 d5 counts an account's rate, per key and UTC minute, in a counter that
  holds only the keys carrying a rate of their own. A restart forgets the minute's count, and
  processes do not share counts, so a deployment of n lead processes behind one address admits
  up to n times a key's rate. One lead serves a tenant today;
  [configuration](../operations/configuration.md#limits-counted-per-process) states the bound
  beside `quota.requests_per_minute`. A count every process shares is a row written on every
  request, which this record does not buy for a reviewer's key; it is decided with ADR-0073's nodes (The
  operator's answers, 2).

- **d7 — The CLI sets both, and doctor names their failures.** `oml keys issue` and `oml keys
  issue-agent` take `--expires <instant>` and `--requests-per-minute <n>`; `oml keys issue --role
  admin` with either is refused by the server (d1). `oml keys limit <key>`
  takes those two, and `--no-expiry` and `--no-rate-limit` to clear each. `oml keys show` and the
  issuing verbs print both. `oml doctor` prints, for the key it runs under: a failure when the
  server refused it as expired, with the date; a warning when the server answered `limit.rate`;
  and otherwise the instant the key ends and the rate it is held to, with the fact that each
  server process counts alone. A key with neither limit adds no line.

## Consequences

- Migration 0046 adds `api_keys.expires_at` and `api_keys.requests_per_minute`, both null for
  every existing key, which therefore neither expires nor is limited, and the constraint
  `api_keys_admin_key_has_no_limits`, which holds both null on an admin key's row.
- A temporary admin key is not possible. An admin key has no expiry, takes no denial and is
  not revoked by any verb, so one issued for a short task stays live.
- `docs/api/openapi.yaml` serves `expiry` and `requests_per_minute` on `KeyIssueRequest`, `Key`,
  `ListedKey` and `Whoami`, `expired` on `Key` and `ListedKey`, and the operation
  `setKeyLimits`; `openapi-designed.yaml` drops `issueKey.expiry`. `docs/data/event-catalog.md` gains `key.limits_set`, and
  `docs/api/errors.md` the `expired` reason of `auth.invalid` and the wider `limit.rate`.
  [Roles and keys](../architecture/roles-and-keys.md), [security](../architecture/security.md)
  limit 7 and [limits](../reference/limits.md) say what is built.
- The events tail shows `key.limits_set` to no key, as it shows no `key.*` event (ADR-0025 d15).
- The web console's keys pages mark an expired key and show each key's date and rate (#337),
  and a person's two issue forms take an optional date (The operator's answers, 3; #352). Both
  were built in v0.7.3, after this record was accepted.
- #185's reviewer key is issued with `--expires` and `--requests-per-minute` in the ops
  repository's seed script; that change is the operator's.

## The operator's answers of 2026-11-12

The operator accepted this record as built on 2026-11-12 and decided each of its five open
questions at the recommendation it carried.

1. **A date for a connection apart from its key's.** Not built. Decided, at the recommendation:
   none. A connector key has no value and at most one open connection, so the key's date is the
   connection's, and one date is one thing to list and to move.
2. **A rate every server process shares.** Not built. Decided, at the recommendation: it is
   decided with
   [ADR-0073](ADR-0073-a-leader-signs-every-write-and-nodes-serve-the-workspaces-placed-on-them-each-read-routed-to-the-closest-less-busy-replica-fresh-enough-for-it.md)'s
   nodes, when a second process serves requests; until then the bound is n times the rate.
3. **A person setting a date or a rate on their own keys from the web console.** Not built
   when this record was accepted: the route takes the admin key role. Decided, at the
   recommendation: `expiry` is added to a person's own issuance when the keys page is next
   changed, and changing it afterwards stays with the admin key role. Built in v0.7.3 (#352),
   on the operator's word of 2026-11-12, "Build it in this bulk": `POST /v2/keys` as
   `<user>-web` admits `expiry` under d1's rule and still refuses `requests_per_minute`. The
   Keys page's two issue forms, for an agent key and for a connector key, each carry one
   optional date input, which works without script and is sent as the last second of that day
   in UTC (`YYYY-MM-DDT23:59:59Z`); a day that has passed, or a value that is not
   `YYYY-MM-DD`, is refused on the page as the API refuses the instant; a day its month does
   not hold, such as `2026-11-31`, is read as the day that many days on (`2026-12-01`), as
   every `date-time` of the API reads it. The Start page's one-button issue sets
   no date. A person's own issuance never issues an admin key, so d1's rule for admin keys is
   untouched, and their session key is still `access.verb_ceiling` on
   `POST /v2/keys/{key}/limits`, so the date moves or clears only by an admin key (d5). An
   allowlist admin's `<user>-admin` is of the admin key role and is admitted there, though no
   page sends the request. A
   rate on a person's own key is still an admin key's to set.
4. **Whether an expired key leaves its account's `keys` meter.** It stays (d3). Decided, at the
   recommendation: it stays, since the key can be given a later date and would then pass a
   quota it never counted against.
5. **The operator role.** The rule and migration 0046's check name the `admin` role alone. No
   path in this build creates the first operator key (an admin asking for one is
   `access.issuer_ceiling`); an operator key could issue another, and the rule would let it
   carry a date and a rate. An operator key could not reach the limits route, which takes the
   admin role exactly, but an admin key could set a date on an operator key, since the route
   refuses an admin target only. Neither scenario of the review applies while no operator key
   exists. Decided, at the recommendation: when the
   operator path is built, a date and a rate are refused on every role that can issue an admin
   key, in the use case, both stores and the check, before the first such key is issued.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Expired is final, like revoked** | A review that slips a week would need a new key, new grants and a new connect code. The admin who could issue those can as well move the date, so finality protects nothing and costs the operator a ceremony. |
| **Answer an expired key `reason: unknown`, as a revoked key** | The holder cannot tell a mistyped key from one that ended, and the operator is asked which. The date is told only to a caller holding the key's value, so no guesser learns that a key existed. |
| **`--expires` on `oml keys connect-code`**, as #323 proposed | The code's `--ttl` already means how long the code can open a connection. A second date on the same verb that changes the key would read as the code's. The date is the key's and is set where the key is issued or with `oml keys limit`. |
| **Read a bare date such as `2026-12-01`** | It names no instant: start or end of the day, in whose zone. The server refuses it instead of choosing (ADR-0025 d16). |
| **Apply `quota.requests_per_minute` to every key** | It would put a rate on every tenant key of every deployment on upgrade, and a demo key needs a much lower one than a person's agents. A rate on the key is set where it is wanted. |
| **A token bucket, a burst, or limits per operation** | #185 needs one sentence an operator can say to a reviewer: this key makes n requests a minute. The fixed UTC minute is the window ADR-0068 d5 already counts and explains. |
| **Count requests in PostgreSQL so every process shares the count** | One row write per request on the read path, for a bound that one lead already gives. Decided with ADR-0073's nodes (The operator's answers, 2). |
| **Limits bind admin keys too**, as this record first proposed | Declined: the operator decided "No limits on admin keys" on 2026-11-12, after an independent review found two scenarios. One: a limited admin key sheds its limits in two calls. Admin key A, issued with an expiry and a rate, issues admin key A2 with neither, and A2 posts `{expiry: null, requests_per_minute: null}` to A's limits. Two: any admin key ends any other by setting a near expiry on it, which nothing could do before: a denial on an admin key is refused, and the administering guide says an admin key cannot be cut off. |
| **Keep limits on admin keys and document both scenarios** | Declined by the same decision of 2026-11-12. This record's reasons: a limit its holder lifts in two calls limits nothing, and writing down that one admin key can end another would make a new power a rule without deciding it. Refusing both limits on an admin key is the smallest change that closes both. |
| **Let a key shorten its own date or lower its own rate** | Harmless in itself, but the rule "a key never sets its own limits" is one sentence with no direction to get wrong, and an admin who wants less holds another admin key or fences the key. |
| **Sweep expired keys into `key.revoked`** | A background writer that signs events no actor asked for, and a revocation that could not be undone when the date was a mistake. Each lookup reads the date, so nothing needs sweeping. |
