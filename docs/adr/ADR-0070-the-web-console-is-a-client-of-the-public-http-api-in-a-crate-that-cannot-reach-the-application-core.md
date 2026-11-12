<!--
id: adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core
tier: engineering
status: accepted design · 2026-11-07, amended 2026-11-10, 2026-11-11 and 2026-11-12; two rows await the operator's acceptance: the in-flight cap and the rate share (the measurement of 2026-11-11); the two rows on the session gate were accepted on 2026-11-12; the two rows on a denied session key's acts (#325) and the three on sign-out (#327) are proposed amendments of 2026-11-12 and await it too
answers: why the web pages stop calling oml-app's use cases and call the public HTTP API instead; which crate serves the pages and what it may depend on; how a signed-in person's session becomes an API credential without a key value at rest or in the browser; which middleware layer owns the request id, the session, CSRF, CSP, rate limits, audit and error mapping; how oml-web, always its own process, reaches the API; which existing operation serves each web call and which operations are new; what stays in the auth context; the bounded contexts; the migration phases and the test that proves parity; the overhead budget; what it amends in ADR-0042 and ADR-0045
-->

# ADR-0070 — The web console is a client of the public HTTP API, in a crate that cannot reach the application core

**Status:** accepted design · 2026-11-07, amended 2026-11-10, 2026-11-11 and 2026-11-12; two rows await the operator's acceptance: the in-flight cap and the rate share (the measurement of 2026-11-11); the two rows on the session gate were accepted on 2026-11-12; the two rows on a denied session key's acts (#325) and the three on sign-out (#327) are proposed amendments of 2026-11-12 and await it too

**Amends:** ADR-0045 (d1: the page routes move to `oml-web` and the sign-in routes to the auth
context; d6: a session key gains a per-session credential; d7: that credential, never the
cookie, reaches the API, and only through the web client; d14: the page's line and the API's
lines share a correlation id; d16: the new and changed operations below), ADR-0042 (d2:
`credential: session` is now written on the API's own lines as well as on the page's)

**Amended in place:** 2026-11-10, by the operator's decision that `oml-web` runs only as its own
process (d1, d2, d3, d4, d7, d8, d9 and d10 revised; d11 and d12 new). What changed and why is in
[The amendment of 2026-11-10](#the-amendment-of-2026-11-10); the decisions below read as amended.
Amended again on 2026-11-10 by phase (a) as built (d2, d3, d5 and d7 revised): see
[Phase (a) as built](#phase-a-as-built-2026-11-10). Amended by the first change of phase (b)
(d1, d3, d4, d8 and d9 revised): see [Phase (b) as built](#phase-b-as-built-2026-11-10).
Amended on 2026-11-11 by the operator's decision to route by path (d4 and d7 (b) revised):
each moved page gets its own path under `/web/`, and the lead's `303`s from the old `?view=` form were deleted in v0.7.1 (The old URLs deleted); see the second table of Phase (b) as built. Amended by the third change of phase
(b), the Workareas view and the first acts through `oml-web` (d3, d5 and d8 revised): see the
third table of Phase (b) as built. Amended by the fourth change of phase (b), the Memories list, the
Atlas and the creation of a workarea through `oml-web` (d3, d5, d7 and d8 revised): see the fourth
table of Phase (b) as built. Amended by the fifth change of phase (b), the Review and Keys views and
their acts through `oml-web` (d3, d5, d7 (b) and d8 revised): see the fifth table of Phase (b) as
built. Amended by the sixth change of phase (b), one memory opened by its id, the admin's Curate
view and the Mermaid frame through `oml-web` (d1, d3, d4, d5 and d7 (b) revised): see the sixth
table of Phase (b) as built.
Amended on 2026-11-11 by the web client secret's storage, rule and rotation (#230, #242; d2 and
d4 revised): see [The client secret as built](#the-client-secret-as-built-2026-11-11). Amended by the
second half of the sixth change (6b), Start, setup, the admin overview and the users page through
`oml-web` (d3, d5, d6 and d7 (b) revised): see the seventh table of Phase (b) as built. Amended by
phase (c), the `web_*` use cases out of `oml-app`'s core (d7 (b) and (c) revised): see
[Phase (c) as built](#phase-c-as-built-2026-11-11). Amended by phase (d), the walls as required checks (d7 (d)
revised): see [Phase (d) as built](#phase-d-as-built-2026-11-11). Amended on 2026-11-11 by the
operator's decision to delete the old URLs' `303`s in v0.7.1 (d7 (b) and (d) revised): see
[The old URLs deleted](#the-old-urls-deleted-2026-11-11). Amended on 2026-11-11 by the idempotency
claims (#233; d3 layer 4 and d5 revised): see
[Idempotency claims](#idempotency-claims-2026-11-11-233). Amended by the follow-ups of phase (b)
(#253; d3 layer 4, d3 layer 6, d5 and d8 revised): see
[The follow-ups of phase (b)](#the-follow-ups-of-phase-b-2026-11-11-253). Amended by the measurement
of 2026-11-11 (#170; d2, d3 layer 6 and d8 revised): see
[The measurement of 2026-11-11](#the-measurement-of-2026-11-11-170). Amended on 2026-11-12 by the
re-measurement (#286; d8 revised): see
[The re-measurement of 2026-11-12](#the-re-measurement-of-2026-11-12-286), and by the session gate's
reads (#297; d2 and d8 revised): see
[The session gate's reads](#the-session-gates-reads-2026-11-12-297). Proposed on 2026-11-12 and
awaiting the operator's acceptance: a denied session key's acts (#325; d2 and d8 revised): see
[A denied session key's acts](#a-denied-session-keys-acts-2026-11-12-325); and sign-out on every
page (#327; d2, d3 layer 4 and d5 revised): see
[Sign-out on every page](#sign-out-on-every-page-2026-11-12-327).

## Context

**The pages call the use cases directly, so the API is not the contract every client gets.**
The user page and the admin page of
[ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
live in `crates/transports/oml-transport-http/src/web/`, beside the API's handlers, and call
`oml-app` in-process: 39 distinct `App` method call sites across the web modules
(`admin_read.rs`, `atlas.rs`, `emails.rs`, `memories.rs`, `mod.rs`, `page.rs`,
`start.rs`, `views.rs`, `workareas.rs`), 20 of them from the `web_*` family
(15 in `oml-app/src/app/pages.rs` and 5 in `web.rs`). Three open pull requests add more: #163 adds
`web_memories`, `web_regions`, `admin_regions`, `workarea_offers` and `admin_search_filtered`
and two JSON routes, `/web/data/memories` and `/web/admin/data/memories`; #167 adds fourteen
use cases in `signup.rs` and seven routes; #166 changes the stylesheet. Each page feature
therefore reaches past the contract the CLI, the SDKs and the agents use, and the rules that
make a page act safe (which key acts, which verb it may run) are enforced partly in the app
and partly in page code that no other client exercises. The `oml-cli` is already a thin HTTP
client that depends on no server crate (`crates/bins/oml-cli/Cargo.toml`); the pages are the
one first-party surface that is not.

The precedent is only partial. `oml-cli` neither owns nor links application
crates (`oml-app`, `oml-domain`, `oml-scan` may still satisfy non-application
helpers), and the ADR's wall is specifically against application-domain code.
The benchmark the cargo metadata test should compare against is described below.

**The operator decided on 2026-11-06 that the pages become a client of the public API.** Their
words: "A new crate oml-web depends only on the generated Rust SDK and calls the public /v1
HTTP API (the same contract every client gets). Middleware does session cookie → the person's
own key held server-side, CSRF, CSP, rate limits and audit. Mounted in the same binary by
default (loopback hop) and can run separately. A dependency test forbids oml-web from
importing oml-app/domain. Missing operations (admin ones) become documented API endpoints."
The public API is the two rings of
[ADR-0022](ADR-0022-two-ring-api.md): `/v1`, the provider ring under its covenant, and `/v2`,
the lab ring where review, keys, workareas and administration live. The pages need both, so
"the public /v1 HTTP API" reads here as the public API of both rings; every new operation
below goes on `/v2`, since `/v1` changes only additively once FR-21 closes.

**Three facts constrain the design.** First, a session key has no value
([ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
d6): nothing exists today that the page could present to the API as the person. Second, the
API never reads the `Cookie` header and no machine interface reaches a page (d7), and both
must stay true. Third, the Occam rule of the
[use-case call map](../engineering/use-case-call-map.md) holds for the new operations: each
traces to a documented use case, merges into an existing operation where one job is one
operation, and defaults whatever the server can derive.

**The operator decided on 2026-11-10 that `oml-web` is a separate process only.** Their words:
"`oml-web` is a separate process only: its own binary and deployable, over the public HTTP API.
Middleware (session → `omls_` credential, CSRF, CSP, rate limit, audit) lives in oml-web, with an
optional external gateway in front. The in-process mode is dropped." The same decision gives the
landing page its own folder, build and release archive, served by Caddy (d12).

Built in phases (a) to (d), 2026-11-10 to 2026-11-11; the records below say how.

## Decision

- **d1 — `oml-web` is its own crate and its own binary, depends on the Rust SDK, axum, tower and
  one shared page crate, and a test fails when its dependency graph reaches the application
  core.** The crate lives at `crates/clients/oml-web`, builds the `oml-web` binary, the one
  process that serves the pages (d4), and holds the pages, their forms, their templates,
  the Mermaid frame route of ADR-0056 d9 (`web/render.rs`) and the middleware of d3. Its normal dependencies are `oml-sdk` (d9), `oml-web-credential`, `axum`, `tower`, `http`, `serde`, `serde_json`, `tokio`, `getrandom`, `ring` and `oml-page`, a new crate with no I/O that holds what the
  connect page and the web pages share today through `oauth/page.rs`: `escape`,
  `client_string`, the compile-time `sha256`, `base64_32`, the design system's stylesheet and
  tokens (ADR-0069, #166). `oml-transport-http` depends on `oml-page` too, for the connect page
  and the sign-in page. The test `oml_web_reaches_no_application_crate`, in
  `crates/clients/oml-web/tests/dependencies.rs`, runs `cargo metadata --format-version 1
  --locked`, walks the resolve graph from `oml-web` over normal and build edges, and fails
  naming the path when it reaches `oml-app`, `oml-domain`, `oml-guardrail`, `oml-scan`,
  `oml-pg`, `oml-lake`, `oml-fetch`, `oml-rerank`, `oml-transport-http`, `oml-server-lead` or
  `sqlx`. Dev-dependencies are outside the walk, as they are for `oml-cli`'s
  `the_default_cli_dependency_tree_is_unchanged`, so the page tests may compose the lead. The
  HTTP API stays in `oml-transport-http`; the allowlist and admin-read handlers are API, not pages, and stay in the transport's `web/` directory beside the sign-in routes (not moved). The cost is one
  more crate and one more shared crate, and every page string the domain used to format (a
  name, a path, a state) is now formatted from the wire's fields.

- **d2 — A session reaches the API as one of the person's own session keys, through a
  per-session credential derived from the session token and accepted only from the web
  client; identity is read off that credential at the API.** At session open the auth context
  computes, for each capacity the entry reaches (`web`, `review`, `curate` when the key
  exists, `admin` when the entry is an admin's), the capacity credential
  `omls_` followed by the base62 of HMAC-SHA256 keyed by the session token over
  `oml/web-capacity/v1/<capacity>`, and stores the SHA-256 of each beside the session row, in
  the same commit that opens it. `oml-web` holds no state: it reads the session token from the
  `__Host-oml_session` cookie and derives the credential of the capacity an action needs. The
  API resolves `Authorization: Bearer omls_…` by digest to the session row and the capacity,
  then checks on every request: the session row is live and unexpired, the capacity is one of the row's, and the `admin` capacity is served only where the row's admin flag is set. The gate reads no allowlist entry: the row carries the entry's user and admin flag as they were at sign-in, and every change to an entry (its user, its admin flag, its removal, the person's disabling) deletes that entry's sessions in the commit that makes it, so a live row's entry stands as the row says. The caller is then that session key, with credential `session`, under its own grants and verb ceiling; every act lands in the journal under it, as today. Three key acts read the session key's access again when they run and refuse a key that carries a denial, on any workarea: `POST /v2/keys` as `<user>-web` and the two key acts of a session key, `POST /v2/keys/{key}/connect-codes` and `POST /v2/keys/{key}/denials` (`access.verb_ceiling`, `details.reason: session_key_denied`; proposed amendment, [A denied session key's acts](#a-denied-session-keys-acts-2026-11-12-325)). `POST /v2/web/session/end` acts on the session alone and consults none of the key's grants or denials, so a denied key still ends its session. The credential lives exactly as long as the session and ends with it (sign-out,
  expiry, an entry change), so no revocation is needed; nothing at rest can be decrypted into it. Every page answered to a request that carries a session cookie has a sign-out control in its header, a form that posts `sign_out` to the lead's `POST /web`: each page `oml-web` serves, each refusal page it answers, and the lead's own refusal page once the lead has read a live session from the cookie. An answer that is not a page has none, and neither has a page the lead draws before it reads the session (proposed amendment, [Sign-out on every page](#sign-out-on-every-page-2026-11-12-327)); a session also ends by expiry, an entry change or a new sign-in in the same browser, which replaces it. Two gates keep [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d7: an `omls_` credential is refused on `/mcp` always, and on `/v1` and `/v2` unless the
  request proves `oml-web`'s client identity (`auth.invalid`, `details.reason:
  session_outside_web`). The identity is one of two, and the lead accepts the one it is
  configured for: a web-client secret, `web.client_secret`, set to the same value on the lead
  and on `oml-web` and sent as `OML-Web-Client`, compared in constant time; or mutual TLS, where
  the lead terminates TLS on the listener `oml-web` reaches and `web.client_ca` names the CA
  whose certificate, with the subject in `web.client_subject`, is `oml-web`'s. The secret is the
  default, since it needs no PKI on one host; mTLS is for a hop that leaves the host. A client
  certificate forwarded by a gateway in a header is not an identity: it is a field, and the
  lead refuses it. Neither value is logged, and `oml quickstart` writes one secret to an
  owner-only file that both processes read (d4). An agent that reads the cookie therefore
  still reaches no API with it, and an agent's bearer key still reaches no page. Which capacity an action uses is compiled into its handler (`issue_key` → `web`, `verdict` → `review`, `curate` → `curate`, `allow_email` → `admin`, and so on), never a form field.
  Admin acts use the `admin` capacity under the same rule: `<user>-admin` is an admin key,
  holds no dreamer grant, and is refused every review verb by the API whatever capacity its handler names
  (R1, [ADR-0001](ADR-0001-the-constitution.md)). The cost is stated: a defect in a handler's choice of capacity can make an act run as another of the same person's capacities, never as anyone else, and
  the API's verb rules still bind it; and whoever holds both the cookie and `oml-web`'s client
  identity acts as the person, which is the operator's process boundary.
  As built in phase (a): the identity is the secret alone. Mutual TLS needs the lead to
  terminate TLS on the listener `oml-web` reaches, which it does not, so `web.client_ca` or
  `web.client_subject` stops the lead at start, naming the setting; the gate accepts a client
  certificate only as a request extension such a listener would set, never as a header. The
  capacities are those the entry reaches when the session opens: a curate designation or an
  admin flag gained later takes effect at the next sign-in, and an entry change ends the
  session. Each resolved request passes the person's request rate (ADR-0068 d5), as their agent
  keys do. A session key acts only as the page act its capacity serves where a key operation
  would reach further: `POST /v2/keys` issues as `<user>-web` only, the person's own issuance,
  and refuses every other capacity (`access.verb_ceiling`, `details.reason:
  session_capacity`), so the admin capacity mints no plain key. The admin capacity mints no connect code either (`session_capacity`), and denies only as the page's cut-off does: on `universe`, a key named by name, never a session key or an admin key. It keeps the admin key role's other verbs, under the API's rules.

- **d3 — Seven concerns guard every page request, each with one owner, all in `oml-web`: four are `tower` layers, the session load is a route layer inside them, error mapping is a function each handler calls, and the in-flight cap is not built; an external gateway in front is optional.** Four `tower` layers wrap every request, outermost first: the ids and the line (1), the human-only gate (2), same-origin and CSRF (4), the response headers (5). The session load (3) is a route layer on the page routes, inside them; the data routes and the frame load their own session. Error mapping (7) is `oml_web::outcome`, which each handler calls. Layer 6 is not built. All of them are in the `oml-web` binary, so a deployment with nothing in front of it (local development, d4) has every one that is built. A gateway the operator puts in front (Caddy, an ingress, an
  API gateway) may add TLS, its own rate limit, a web application firewall or access logs; it
  replaces none of the layers below and `oml-web` never trusts a header the gateway sets for
  identity, the session or CSRF. The one setting a gateway touches is `server.trusted_proxies` of [ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md), on the lead. `oml-web` reads no such setting and its line names no client address (not built).

    | # | Layer | Owner | What it does, and why it sits there |
    |---|---|---|---|
    | 1 | Request id and traffic log | `oml-web`, the [ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) layer, written in `oml-web`'s own middleware (`oml-page` holds no line type) | Honours or mints `X-Request-Id`, writes the page's `http_request` line with `credential: session`, the session's web key, `web_action` and the route template. Each inner API call carries `X-Correlation-Id` set to the page's request id and `X-Request-Id` `<page id>.<n>`, so one `jq` filter finds the page line and its API lines. Outermost, so a refusal by any inner layer is still logged. |
    | 2 | Human-only gate | `oml-web` | An `Authorization` header on any route but `/web/healthz` answers `access.verb_ceiling` `web_is_human_only` before anything is read (ADR-0045 d7). |
    | 3 | Session load | `oml-web` reads the cookie; the API decides | Reads `__Host-oml_session` and derives the `web` credential; the first API call of the page, `GET /v2/whoami`, is the check. No session, or `auth.invalid` from the API, sends a `303` to the lead's sign-in page (d4, d6). `oml-web` never judges a session itself; any failure of the `admin` credential's `whoami` (a timeout, `limit.rate`, a 5xx) reads as not an admin, so the admin pages answer not found while the API fails. |
    | 4 | Same-origin and CSRF | `oml-web` | Every `POST` passes the existing `same_origin` check against the public origin and carries `csrf_token` = SHA-256 over the fixed purpose and the session token, compared in constant time (ADR-0045 d8, unchanged). Every form whose post acts carries a random nonce, and the `Idempotency-Key` of the act's API call is the hex SHA-256 over the session token and that nonce, so a resubmitted form replays instead of repeating. The API never sees a form, so CSRF belongs to the layer that renders forms. |
    | 5 | Response headers and CSP | `oml-web` | `default-src 'none'`, `style-src` naming the one stylesheet's digest, no `script-src` on any page but `/web/start`, whose policy adds the hash of its one script and `connect-src 'self'` for its status poll, `form-action 'self'`, `frame-ancestors 'none'`, `base-uri 'none'`, `no-store`, `nosniff`, `Referrer-Policy: same-origin`; the frame route keeps its own policy. The stylesheet's digest is computed at compile time from `oml-page`; the Start script's when its policy is built. |
    | 6 | Rate limit | `oml-web` for fan-out, the API for acts | Not built ([#170](https://github.com/e8kor/open-memory-lab/issues/170)): `oml-web` caps nothing across a session; the Workareas view bounds its own fan-out (`FAN_OUT`, 8). Every act stays under its key's limits at the API, as for any client ([FR-8](../product/roadmap.md)). Provider-call caps stay with sign-in in the auth context (ADR-0045 d15). |
    | 7 | Error mapping | `oml-web` | The API's envelope (`code`, `details`) becomes a page outcome: `auth.invalid` the sign-in redirect; every other code one refusal page naming the code, the message and the page's request id, under the API's status, `limit.*` with its `Retry-After`; a timeout `504`, an unreachable API or an answer out of shape `502`. |

    **Audit is two records with two owners.** The page-level audit is `oml-web`'s line
    (layer 1): the session's web key, which `web_action`, and the request ids of the API calls it made; which capacity ran an act is on the API's line for that call. The record of what happened is the API's journal: every act and every admin
    read is journalled by the use case behind its operation, under the session key, as today.
    `oml-web` writes no journal event and keeps no store, so a page cannot record anything the
    API would not. An admin read means a read of memory content: the admin's listing of a
    person's keys (`GET /v2/keys?user=`) reads no claim and journals nothing, as `whoami`
    does not.

- **d4 — `oml-web` runs only as its own process and reaches the API over HTTP at `web.api_url`,
  which is required; the lead serves no page, and the deployment routes by path.** The SDK
  sends through one trait, `Transport`, taking an `http::Request` with a serialized body and
  answering an `http::Response`; the `oml-web` binary's one implementation is a pooled HTTP
  client to `web.api_url`, and `oml-web` refuses to start without it, naming the setting. There
  is no in-process mode: the lead does not link `oml-web`, and no composition hands it the API
  router. `oml-web`'s settings are `web.api_url`, `web.listen`, its client identity (d2), and
  `web.public_url`, the origin its same-origin check compares a `POST`'s `Origin` against, which
  defaults to the API's `/.well-known/oauth-authorization-server` `issuer`, read at start. The
  `omls_` credential and the client secret cross the hop, so a plain `http://`
  `web.api_url` is accepted only for a loopback host (`127.0.0.0/8`, `::1`, `localhost`). For any
  other host `oml-web` refuses to start with `web.api_url (OML_WEB__API_URL): plain http to a non-loopback host sends
  session credentials in clear; use https://, or set web.allow_plain_http_api = true for a private
  network`; with `web.allow_plain_http_api = true` it starts and logs one warning naming the host
  at every start. `web.api_url` carries no userinfo, query or fragment. Across a LAN the hop is `https://` or mutual TLS (d2; mutual TLS is not built, see Phase (a) as built); `oml quickstart`'s compose network reaches the lead at a service
  name, so it sets the opt-in explicitly. The doctor line lives in `oml-web`, whose settings the
  lead's `oml doctor` cannot read: `oml-web --doctor` prints `web.api_url` as `ok`, `warn` (plain http allowed by the opt-in) or `fail` (missing or refused); `oml doctor` prints no `web.api_url` line. Test:
  `a_plain_http_api_url_to_a_non_loopback_host_is_refused_unless_allowed`.

    | Shape | Browser reaches | `oml-web` reaches the API at |
    |---|---|---|
    | Production | Caddy (or another proxy) on the public origin: each path of `oml_web::PAGES` and each prefix of `oml_web::PREFIXES` to `oml-web`; every other path to the lead by falling through: `/web` (sign-in without a session, sign-out), `/web/sign-in`, `/web/callback/*`, `/web/sign-up`, `/v1/*`, `/v2/*`, `/mcp`, `/oauth/*`, `/.well-known/*`; the landing paths to static files (d12) | the lead's listener on loopback or the LAN, not back through the public proxy |
    | Local development | four services started by `oml quickstart`: Postgres, the lead, `oml-web` and a path router (Caddy) in front of both, routing exactly as production does, so the browser sees one origin; the compose file sets `web.public_url` to the router's published address, and there is no `web.page_url` setting | the lead's address on the host or on the compose network |
    | Kubernetes | the chart's Ingress routes as production does, to two Deployments: each path in `web.ingressPaths` (`Exact`; `Prefix` for `/web/render` and `/web/assets`) to `oml-web`, every other path to the lead; the chart sets `web.public_url` from `web.publicUrl`, else `https://` and the first Ingress host | the lead's Service |

    Routing is by path alone in every shape (amended 2026-11-11): each moved page has its own
    path under `/web/`, `oml_web::PAGES` lists them, and the proxy, the compose router and the
    Ingress each name exactly those paths. No shape routes on a query.

    `__Host-` cookies bind one host, so production puts both behind the same public origin.
    A person with no live session who opens a page is sent with a `303` to the lead's
    `/web/sign-in`, carrying the page as `next`; the redirect is relative, which works because
    every shape has one public origin. `GET /web/sign-in` is a new route: sign-in rendered at
    `GET /web` until now, and the lead's route table moves with it (phase d). The lead accepts
    `next` only as a same-origin relative path: it is `/web` or a path under `/web/`, at most 2048 bytes, with no `:` before its query, holds no `//`, no `\`, no `@` and no scheme, is percent-decoded once and checked again, and anything else is dropped to
    `/web`. The redirect target is built from the lead's configured public origin, never from a
    request header. Test: `a_next_outside_web_or_naming_another_origin_returns_to_web`. This replaces d10 (e)'s pass-through, which
    needed the sign-in renderer in the same process. The boundary is the `Transport` trait and
    the serialized request, as before, and now also a socket, a second accept path and a peer
    address on every API line. The cost: two processes to deploy, start, upgrade and monitor
    instead of one; one network hop per API call (d8); a version skew window during an upgrade,
    in which `oml-web` must tolerate an API one release ahead or behind (the API's rings
    already promise additive change, and `oml-web` reads unknown fields as absent); and more
    than one `oml-web` replica needs session affinity for the frame route, whose sources are
    held in memory.

- **d5 — Every web call maps to an existing operation, a changed one or one of ten new ones;
  the wire is held here until phase (a) builds it.** Following
  [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d19, the new and changed operations enter `openapi.yaml`, `http-api.md` and `errors.md` in
  the change that serves them, and `openapi-designed.yaml` loses `listKeys` and `reviewQueue`
  in that change. ADR-0045 d22–d31 are the decisions #163 adds, and ADR-0068 is #167's record;
  both are cited as their branches hold them. Every operation below accepts the `omls_` credential of d2 under the
  operation's own rules, so "session user key" means `<user>-web` presenting it, and a bearer
  user key gains nothing.

    **Calls served by an existing operation, unchanged.**

    | Web call (source) | Operation | Capacity |
    |---|---|---|
    | `who(session)`, `web_caller` for the page header (main) | `GET /v2/whoami` | web |
    | `list_memories`, the Memories listing (main) | `GET /v1/memories` | web |
    | `get_memory`, a memory opened by `memory=` (main) | `GET /v1/memories/{artifact_id}` | web |
    | `recall`, the search box (main, #163 `q`) | `POST /v1/memories/recall` | web |
    | `submit_memory` for `curate` (main) | `POST /v1/memories` | curate |
    | `supersede` (main) | `POST /v1/memories/{artifact_id}/supersede` | curate or review |
    | `guardrails` (main) | `GET /v1/guardrails` | web |
    | `review_next` (main, both pages) | `POST /v2/review/next` | review |
    | `review_verdict` (main, both pages) | `POST /v2/review/verdicts` | review |
    | `review_held` (main) | `GET /v2/review/held` | review |
    | `review_bulk` (main) | `POST /v2/review/verdicts/bulk` | review |
    | `create_workarea` (main) | `POST /v2/workareas` | web |
    | `workarea_tree` (main) | `GET /v2/workareas/{workarea}/tree` | web or admin |
    | `web_path`, `workarea_status` (main, #163); the Atlas breadcrumb is read off the scope's path, not this call | `GET /v2/workareas/{workarea}` | web or admin; review or admin for a row's live proposals |
    | `set_workarea_mode`, commit of an in-progress root (main, #163) | `POST /v2/workareas/{workarea}/mode` | review |
    | `propose_transition`, `ratify_transition` (#163) | `POST /v2/workareas/{workarea}/transitions`, `…/{transition_id}/ratify` | review, or admin on the admin page |
    | `web_issue_key` for `issue_key`, `connect_key`, #167's `issue_claude_*` (main, #167) | `POST /v2/keys` | web |
    | `web_connect_code` (main) | `POST /v2/keys/{key}/connect-codes` | web |
    | `web_cut_off` (main, both pages) | `POST /v2/keys/{key}/denials` | web or admin |
    | `web_all_entries` (main) | `GET /v2/web/emails` | admin |
    | `allow_web_email` for `allow_email`, `set_admin` (main) | `POST /v2/web/emails` | admin |
    | `remove_web_email` for `remove_email` (main) | `POST /v2/web/emails/remove` | admin |
    | `admin_read` (main) | `GET /v2/admin/memories/{artifact_id}` | admin |
    | `admin_search` (main) | `POST /v2/admin/memories/search` | admin |
    | `web_users`, `web_deletions`, `signup_overview` (#167) | `GET /v2/web/users` (served by #167) | admin |
    | `web_start_status`, `web_has_agent_key` (#167) | `GET /v2/keys` (new, below) and `GET /v1/memories?with_own=true&limit=1` (replaced by `GET /v2/whoami?start=true`, the seventh table) | web |

    **Changed operations: a parameter or a field joins an existing operation.**

    | Operation | Change | Serves (source) | Traces to | Admin |
    |---|---|---|---|---|
    | `GET /v1/memories` | `with_own=true`: the scoped listing and the caller's own rows merged newest first, a row in both once, one cursor; `author`: one key naming the caller's user, by name, which drops the scoped part | `web_memories` (#163) | ADR-0045 d22, d23 | no |
    | `GET /v2/workareas/{workarea}/tree` | `counts=true`: the whole subtree in one answer, each node carrying `{promoted, held, in_progress, other}` as the caller's `with_own` listing would show them, the counts the Atlas draws, and `truncated` past `REGION_COUNT_MAX`; for the admin role every row counted as structure, no content | `web_regions`, `admin_regions` (#163) | ADR-0045 d30 | for the admin reading |
    | `POST /v2/admin/memories/search` | `origin_kind` and `author` filters | `admin_search_filtered` (#163) | ADR-0045 d22, d24 | yes |
    | `GET /v2/review/held` | `ids`: the named held rows only, each with its row or `null` when no longer held or no longer the caller's to judge | `review_named`, the bulk confirmation (main) | ADR-0055 d10 | no |
    | `POST /v2/web/emails`, `POST /v2/web/emails/remove` | `keep_an_admin: true` refuses removing or un-flagging the tenant's last admin entry, `request.validation` `details.field: admin`, in the same transaction | `web_other_admin` (main) | ADR-0045 d10 | yes |
    | `GET /v2/whoami` | `person`: for a key naming a user, `{user, under_quotas, usage, quotas, deletion_requested_at}` | `web_own_limits`, `web_deletion_pending` (#167) | ADR-0068 d5, d6, d10 | no |

    **New operations.**

    | # | Method and path | Request | Response | Serves (source) | Traces to | Who |
    |---|---|---|---|---|---|---|
    | 1 | `GET /v2/keys` | query `user?`, `limit?`, `cursor?` | `{items: [{id, name, role, held_by, credential, user_root_id, revoked, cut_off, grants: [{role, workarea_id, path}]}], next_cursor}` | `web_own_keys`, `web_keys_of_user` (main), `web_has_agent_key` (#167) | ADR-0045 d9, d13; the designed `listKeys` | a session user key, its own user's keys only; the admin role, one `user`'s keys, `user` required |
    | 2 | `GET /v2/review/queue` | none | `{items: [{workarea_id, path, held, oldest_at}]}`, no content | `web_queue` (main) | ADR-0045 d10, the designed `reviewQueue` | admin |
    | 3 | `POST /v2/web/users/{user}/capacities` | `{capacity: review \| curate, workareas: [id or name]}`, 1 to 50 workareas | `{key: {id, name}, grants_added: [...], journal_seq}` | `web_grant_review`, `web_grant_curation` (main) | ADR-0045 d10, ADR-0044 d7 | admin |
    | 4 | `GET /v2/workareas/{workarea}/offers` | none | `{workarea_id, status, mode, transitions: [{op, to, transition_id?, from?}], mode_change: {to} \| null, commit: {offered, held}}`, each offered only where the verb's own check admits the caller now; an unreadable workarea `access.not_found` | `workarea_offers`, `workarea_offer_check` (#163) | ADR-0045 d28 | no |
    | 5 | `POST /v2/web/session/end` | none | `204` | no page: sign-out stays the lead's `POST /web` (the seventh table), which the sign-out control in every page's header posts to (#327) | ADR-0045 d5 | an `omls_` credential only; it ends the session it derives from |
    | 6 | `GET /v2/web/export` | none | `application/json` attachment of what the person holds, at most 100000 memories | `web_export` (#167) | ADR-0068 d6 | session user key only |
    | 7 | `POST /v2/web/deletion-requests` | none | `{requested_at, journal_seq}` | `web_request_deletion` (#167) | ADR-0068 d6 | session user key only |
    | 8 | `POST /v2/web/users/{user}/quota` | `{memories?, keys?, connections?, requests_per_minute?}`, each a limit or `null` | the user's row as `GET /v2/web/users` serves it | `web_set_quota` (#167) | ADR-0068 d5 | admin |
    | 9 | `POST /v2/web/users/{user}/disable` | none | `{keys_cut_off, journal_seq}` | `web_disable_user` (#167) | ADR-0068 d8 | admin |
    | 10 | `POST /v2/web/users/{user}/deletion/close` | `{outcome}` | `{journal_seq}` | `web_close_deletion` (#167) | ADR-0068 d6 | admin |

    Each of the ten that journals takes `Idempotency-Key`; #5 ends a session, journals nothing and takes none. Every refusal is the envelope of
    [errors](../api/errors.md). #3, #7, #8, #9 and #10 replay: a resubmission under the same
    key answers the recorded response with `Idempotency-Replay: true` and acts nothing again,
    each act claiming its key in a transaction of its own before it acts and recording its
    outcome into the claim after its last commit, so a resubmission while the claim is held
    waits for the outcome and replays it, and a process that dies between the act and the
    record leaves the claim, which a resubmission answers `idempotency.in_progress` rather
    than acting again (#233, the table of 2026-11-11 below). The three key operations of the table above serve a
    session key through the page's own use case: `POST /v2/keys` as `<user>-web` is
    `web_issue_key` (name, role and `connect_code` only), `POST /v2/keys/{key}/connect-codes`
    as `<user>-web` is `web_connect_code`, and `POST /v2/keys/{key}/denials` as `<user>-web`
    or `<user>-admin` is `web_cut_off`, on `universe` alone. Five of the ten (#2, #3, #8, #9, #10) need the admin key role; #1 needs it for any
    user but the caller's own. #3 is one operation for two acts because both are one job: an
    admin gives a person's session key grants, creating `<user>-curate` at the first
    designation; it creates a session key only by this path, so `POST /v2/keys` never mints
    one. #6 and #7 refuse a bearer user key, as d16 of ADR-0045 refuses it key issuance, so an
    agent cannot export or delete its person. `POST /v2/keys/{key}/grants`, designed, is not
    needed by any page and stays designed.

    **Calls that stay in the auth context and become no operation.** `web_sign_in_start`,
    `web_admit`, `web_ensure_keys`, `web_session_open`, `web_session` (now the API's
    resolution of `omls_`), `web_caller` (the same), `web_sign_up` and `check_signup_policy` (#167), `web_has_agent_key`, `oauth_issuer`.

    **Page JSON routes.** `/web/data/memories` and `/web/admin/data/memories` (#163) stay page
    routes of `oml-web`, built from `GET /v1/memories` with `with_own`, the tree with
    `counts`, and #4 (user page), or the admin search and the tree with `counts` (admin page).
    They are presentation of the page's own data, behind the session cookie, and not API.

- **d6 — What faces the browser for identity stays in the auth context, and the system has
  seven bounded contexts, the web one presentation only.** The auth context serves the OAuth
  authorization page, `/oauth/*` and the discovery documents
  ([ADR-0031](ADR-0031-oauth-connections-by-connect-code.md)), and the sign-in routes:
  `POST /web/sign-in`, `GET /web/callback/{provider}`, #167's `POST /web/sign-up` and its
  terms page, and the sign-in page itself. It owns session rows, the capacity credentials of
  d2 and the provider exchanges. It stays in `oml-transport-http`, in `oauth/` and the sign-in half of `web/` (one `auth` module is not built); `oml-app` gathers its use cases in `app/auth.rs`. The auth route's renderer
  produces the sign-in page at `/web/sign-in`, where `oml-web` sends a person with no session
  (d4); `oml-web` does not render it and has no dependency on the auth module, so the wall
  holds even for the unauthenticated page.

    ```mermaid
    flowchart LR
      Browser -->|cookie, forms| Web
      Browser -->|sign-in, callback, sign-up, /oauth| Auth
      Web["Web (oml-web)\npages, forms, CSRF, CSP"] -->|SDK, omls_ credential| API
      Agent -->|bearer key, connection token| API
      CLI -->|bearer key| API
      subgraph API["Public API (oml-transport-http)"]
        Memory["Memory\nwrite, read, review, admin read"]
        Workareas["Workareas\ntree, mode, transitions, offers"]
        Keys["Keys and grants\nissue, list, deny, capacities"]
        Admin["Admin\nallowlist, users, quotas, queue, settings"]
      end
      Auth["Auth\nsign-in, OAuth, sessions, credentials"] -.resolves omls_ and tokens.-> API
      API --> App[(oml-app)]
      Auth --> App
      Observability["Observability\nrequest lines, metrics, traces, doctor"] -.reads.- Web
      Observability -.reads.- API
    ```

    | Context | Holds | Code |
    |---|---|---|
    | Auth | sign-in, the OAuth authorization server, sessions and their credentials, the browser-facing identity pages | `oml-transport-http::oauth` and `::web`, `oml-app::app::auth` |
    | Memory | write, read, recall, supersede, review, the admin read | `/v1/memories*`, `/v2/review/*`, `/v2/admin/memories*` |
    | Workareas | the tree, modes, transitions, offers, edges | `/v2/workareas*`, `/v2/edges*` |
    | Keys and grants | issuance, listing, denials, connect codes, connections, capacities | `/v2/keys*`, `/v2/web/users/{user}/capacities` |
    | Admin | the allowlist, users and quotas, the queue, search and usage settings, rescans | `/v2/web/*` except the capacities update, `/v2/review/queue`, `/v2/search*`, `/v2/usage/*`, `/v2/admin/rescan` |
    | Observability | the request line, `/metrics` on its own listener, OTLP, doctor | `oml-transport-http::traffic`, `oml doctor` |
    | Web | presentation: pages, forms, CSRF, CSP, the frame route; no rule, no store | `oml-web` |

- **d7 — The migration runs in four phases, each closed by its check, and parity is the page
  tests running unchanged.**

    | Phase | Builds | Check that closes it |
    |---|---|---|
    | (a) | `omls_` credentials and `oml-web`'s client identity, the secret (d2; mutual TLS is not built and stops the start); the ten new operations and six changes (d5), each with its use case, its contract entries and its tests | `check_contract_sync.py`, `generate_sdks.py --check`, and per operation a test named after its sentence, among them `an_omls_credential_without_the_web_client_is_auth_invalid`, `an_omls_credential_on_mcp_is_refused`, `a_capacity_credential_ends_with_its_session`, `the_admin_capacity_ends_when_the_entry_loses_admin`, `a_bearer_user_key_cannot_export_or_request_deletion` |
    | (b) | `oml-sdk` (d9), whose one consumer is `oml-web`; `oml-page`, the `oml-web` binary with the layers of d3; the proxy routes of d4 send each moved page's path to it; pages move one at a time, each moved page gets its own path under `/web/`; the lead's `303`s from the old `?view=` form were deleted in v0.7.1 (The old URLs deleted) (amended 2026-11-11) | the page suites, unchanged (below), green after each page moves; `oml_web_reaches_no_application_crate` |
    | (c) | the `web_*` use cases deleted: the logic behind a new operation is renamed to that operation's use case, the sign-in ones move to `oml-app`'s auth module, the rest go | `grep -rn 'pub async fn web_' crates/oml-app --include='*.rs' | grep -v 'crates/oml-app/src/app/auth.rs'` was empty when the phase closed (since #297 it prints the one line Phase (c) as built names); the workspace builds and every suite passed |
    | (d) | the walls: the dependency test is a required CI check, and `the_lead_serves_no_page_route_outside_oml_web_and_auth` asserts the route table | both tests required on `main` |

    **Parity** is the page cases of `crates/transports/oml-transport-http/tests/web_pages.rs`, moved to `crates/clients/oml-web/tests/web_pages.rs` with their assertions untouched; the auth context's cases stay in the lead's file, and #167's `open_signup_pg.rs` stays in `crates/bins/oml-server-lead/tests/`, reaching the moved pages through a path router. The Chromium scripts (`tests/chromium/atlas.py`, `render_frame_chromium.rs`) moved to `crates/clients/oml-web/tests/` with them. Only the rig changes: it
    starts the lead on a loopback listener and `oml-web` with `web.api_url` set to it, as two
    servers in the test process, and seeds through the app as today (a dev-dependency). The
    suite runs once, over that loopback hop, which is the one transport there is (d4). A test that has to change its assertions is a behaviour change, named in the
    change that makes it.

- **d8 — The budget is p95 page time no more than 5 ms above today's with the API on loopback,
  and 5 ms plus two round trips on a LAN; a page aims at two sequential rounds, over pooled
  keep-alive connections, each under a timeout, and a page whose secondary call fails renders
  without that part.** A page view becomes one to four API calls (the Memories view: `whoami`,
  the listing, the tree with counts, the offers); measured, it is 1 to 6 on 21 pages and 15 to 150 on the Workareas views. Each adds serialization both ways, a route
  match, an `omls_` digest lookup (an indexed read; today one `web_caller` per request already
  does it once), on a `POST` an idempotency row, and now a network hop. Four rules hold the hop
  down:
    - **Two sequential rounds is the target.** `whoami` first, since it decides the session, then the page's other calls together; so a LAN adds two round trips, not four. As measured on 2026-11-12, 20 of 23 pages meet it; the admin Review view makes three and the Workareas views one per fan-out wave.
    - **Pooled keep-alive connections.** One connection pool per `oml-web` process, HTTP/1.1
      keep-alive (HTTP/2 when the hop is TLS), bounded in the idle connections it keeps (`POOL_IDLE_MAX`, 32), so no call pays a TCP or TLS
      handshake in the steady state.
    - **A timeout per call.** `API_TIMEOUT`, a constant of the build (10 s); a call
      past it is a failure of that call, with the request id of d3 layer 1.
    - **Partial-page failure states.** A page names its primary call (the listing, the memory,
      the queue) and its secondary ones (the Guardrails selector's tree, the Atlas panel's offers, each Workareas row's offers). A failed or timed-out secondary
      call renders the page with that part marked unavailable and the request id beside it; a
      failed primary call renders the error page of d3 layer 7; a failed `whoami` the sign-in
      redirect or the error page. An act (`POST`) is never partial: it fails whole, and its idempotency key makes the resubmit a replay, except a verdict whose follow-on claim fails, which lands on the view with its notice.

    The 5 ms p95 and the LAN allowance are budgets the operator can hold the build to, not
    measurements: the benchmark of each page, before and after, over loopback and over a LAN,
    on the [FR-25](../product/roadmap.md) harness, the pool size and the call timeout are
    [#170](https://github.com/e8kor/open-memory-lab/issues/170)'s exit. Over budget, the remedy
    is fewer calls per page (a field on an existing operation), never a cache of resolved
    sessions, which would let authority outlive the list. A hop over the public internet is
    not a supported shape and has no budget.

- **d9 — The Rust SDK is a hand-written thin typed client whose operation table a check holds
  to `openapi.yaml`.** `sdks/rust`, crate `oml-sdk`: one method per served operation, request
  and response types with `serde`, the error envelope as one error type, and the `Transport`
  trait of d4 with one implementation, the pooled HTTP client; a test may supply its own. No Rust generator is pinned today:
  `scripts/generate_sdks.py` drives `openapi-python-client` and `openapi-typescript`, and the document is OpenAPI 3.1 with
  many responses typed only as `object`, so a generated layer would carry untyped values where
  the pages need fields. The check: `scripts/check_contract_sync.py` reads `OPERATIONS` in `sdks/rust/src/lib.rs` and fails when an entry's method, path, query or body field names, or a field of its response struct at any depth, differ from `openapi.yaml`; `oml-sdk`'s test `every_method_sends_the_method_path_and_type_its_table_entry_names` holds each method to its entry. The cost is hand-written
  response types, kept honest by the check on requests and by the page tests on responses.

- **d10 — The operator's answers of 2026-11-06 to this record's open choices.** Each is folded
  into the decision it governs; this list is where each answer is found.
    - **(a) Identity.** A session-derived `omls_` credential per capacity, accepted only with
      the web client secret and never on `/mcp`: accepted (d2), and widened on 2026-11-10 to
      the secret or mutual TLS (d11).
    - **(b) Transport.** In the lead, the in-process `oneshot` through the same router the
      listener serves, in place of a loopback hop: accepted (d4), and reversed on 2026-11-10
      (d11): `oml-web` is its own process over HTTP only.
    - **(c) SDK.** A hand-written thin client held to `openapi.yaml` by a contract check:
      accepted (d9).
    - **(d) Ring.** The new operations go on `/v2`, the current API version: confirmed (d5).
    - **(e) Sign-in page.** `GET /web` with no live session passes through to the auth
      context's sign-in page, with no redirect: accepted (d6), and revised on 2026-11-10 (d4):
      with the sign-in renderer in another process, `oml-web` answers a `303` to the lead's
      `/web/sign-in`.

- **d11 — The operator's decision of 2026-11-10: `oml-web` is a separate process only.** Folded
  into the decisions it governs, and listed here so each part is found:
    - **(a) Process.** `oml-web` is its own binary and deployable over the public HTTP API, and
      the in-process mode of the earlier d4 is dropped (d1, d4).
    - **(b) Middleware.** Session to `omls_` credential, CSRF, CSP, rate limit and the
      page-level audit line live in `oml-web`; an external gateway in front is optional and
      replaces none of them (d3).
    - **(c) Client identity.** The API accepts an `omls_` credential only from `oml-web`'s
      client identity, a web-client secret or mutual TLS, and never on `/mcp` (d2).
    - **(d) Budget.** The overhead budget is restated for a network hop on loopback or a LAN,
      with pooling, keep-alive, a timeout per call and partial-page failure states (d8).
    - **(e) Landing.** The landing page is a deployable of its own (d12).

- **d12 — The landing page is a separate deployable: static files built from `site/` and
  shipped as their own release archive, served by the proxy, by neither the lead nor
  `oml-web`.** Its sources and its build live in `site/` of this repository; `site/build.py`
  writes `site/dist/` from them and from the design system's generated CSS
  ([ADR-0069](ADR-0069-oml-s-design-system-is-the-source-of-truth-for-every-surface.md), whose
  `scripts/gen_design_tokens.py` still owns the tokens), and CI fails when `site/dist/` differs
  from what the build writes. Each release attaches `oml-site-<version>.tar.gz`, summed in
  `SHA256SUMS` and signed like the other archives. Caddy serves `/`, `/teams`, `/personal`,
  `/privacy` and `/terms` from the extracted archive and routes every other path as d4's table
  says. The cost: one more archive per release and one more deploy step; the landing can lag
  or lead the server by a release, which is why it names a shipped release and links the
  release map rather than describing planned work.

## The amendment of 2026-11-10

The record as first accepted on 2026-11-07 mounted `oml-web` in the lead by default, calling the API
through a `oneshot` on the same `Router`, with a standalone mode as the second option. The
operator reversed that on 2026-11-10 (d11): one process shape is simpler to reason about and to
secure than two, the boundary becomes a socket nobody can bypass with a function call, and the
web console can be deployed, scaled and restarted without the server. What moved:

| Decision | Was | Is |
|---|---|---|
| d1 | a library crate the lead mounts | a library and the `oml-web` binary |
| d2 | `OML-Web-Client` secret, minted per boot in-process | the secret or mutual TLS, configured on both processes |
| d3 | the layers, owner by owner | the same layers, all in the `oml-web` process; a gateway optional and never trusted for identity |
| d4 | in-process `oneshot` by default, `web.api_url` standalone | `web.api_url` required; production, local and Kubernetes shapes routed by path |
| d7 | parity run twice, in-process and loopback | parity run once, over the loopback hop |
| d8 | 5 ms p95 in-process | 5 ms on loopback, plus two round trips on a LAN; pooling, keep-alive, timeouts, partial pages |
| d9 | two `Transport` implementations | one |
| d10 (b), (e) | in-process; sign-in passes through | reversed; a `303` to the lead's sign-in |
| d12 | (none) | the landing as its own deployable |

## Phase (a) as built (2026-11-10)

Phase (a) (#188, #232) built the wire; where a decision named a shape the use case does not
hold, or a promise the build cannot keep yet, the record now says what was built, and why:

| Decision | Was | Is | Why |
|---|---|---|---|
| d2 | the secret or mutual TLS | the secret; `web.client_ca` stops the start | the lead terminates no TLS, so an mTLS identity could never be proven; refusing at start fails closed |
| d2 | (silent) | capacities fixed at session open; the person's request rate applies; the admin capacity mints no plain key | the credential's digests are written in the opening commit; ADR-0068 d5 binds every key of a person; the admin page never minted keys, and a session key acts as its page act |
| d3 | every admin read journalled | every admin read of memory content | listing keys reads no claim |
| d5 #1 | `created_at`, `connections`, the tenant's keys for the admin role | `user_root_id`, `revoked`; the admin role names `user` | the key row holds no creation time, no page shows connections, and no use case reads every key of a tenant |
| d5 #4 | `{transitions: [{to}], mode, commit}` | adds `op`, `transition_id`, `from`, the node's status and mode | the dropdown ratifies a named proposal, so it needs its id |
| d5 #9 | `{reason?}` → `{sessions_ended, …}` | no body → `{keys_cut_off, …}` | the reason is not journalled, so it is refused rather than dropped; the sessions end in the fold and are not counted, while the keys cut off are |
| d5 tree | `{held, promoted, rejected}` | `{promoted, held, in_progress, other}` | the Atlas draws held rows inside an in-progress scope apart, and the use case counts so |
| d5 | `Idempotency-Key` taken | replayed by the console's acts; the key operations serve a session key | ADR-0070 d3 layer 4 depends on replay; d5's table maps the page's key acts to them |
| d7 | `oml-sdk` in (a) | `oml-sdk` in (b) | its one consumer is `oml-web` |

## Phase (b) as built (2026-11-10)

The first change of phase (b) (#189) built `oml-sdk`, `oml-web`'s layers and the hop, and moved
one page, the Guardrails view (`GET /web?view=guardrails`), the one
`design/web-client-separation.md` orders first. Where it reads a decision
differently, the record says so here:

| Decision | Was | Is | Why |
|---|---|---|---|
| d1 | `oml-web` depends on `oml-sdk`, `oml-page` and the HTTP stack | also `oml-web-credential`, a leaf of `sha2` and `hex` (and `rustix` on Unix) that `oml-domain` re-exports, and `tokio`, `getrandom` and `ring` | the capacity credential and the CSRF token are derived on both sides of the hop; one implementation, outside the application core, keeps the two equal |
| d1 | `oml-page` holds the stylesheet and the helpers | also the page chrome (`shell`, `document`, the tabs), the marks, the same-origin rule, and `gen_design_tokens.py` writes into it | a moved page draws the same header as the lead's pages, asserted equal by `a_moved_page_draws_the_header_and_tabs_the_leads_pages_draw`, which now compares two `oml-web` pages and so no longer holds a moved page equal to a lead page; #230's identical check is one function both call |
| d3 layer 3 | a `303` to the lead's `/web/sign-in` | a `303` to `/web` in the first change; the second change builds the record's reading | the lead rendered sign-in at `GET /web` with no session; the second change adds `GET /web/sign-in` before `GET /web` moves |
| d3 layers 3, 4 | the session load, then same-origin and CSRF | same-origin and CSRF first, on the cookie alone | a forged form is refused before any API call |
| d3 layer 1 | the page line names `web_action` | names `credential: session`, the acting key and `api_request_ids` | no moved page acts yet; `web_action` joins with the first act |
| d3 layer 6 | the per-session cap on calls in flight | not built | it is a constant without a measurement (#170); it lands with the pages that fan out |
| d3 layer 7 | `request.validation` the form again, the outcome's own status | each refusal keeps the API's status; a timeout `504`, an unreachable API `502`, each page naming the request id | a page refuses with the status the lead's page did, so the moved assertions hold |
| d8 | two sequential calls at most | the Guardrails view's workarea selector pages the tree, 200 nodes a call | the tree with `counts=true` answers in one call but stops at 2000 nodes and counts rows the selector does not show; past 200 workareas the selector costs one more call per page of them |
| d8 | (silent) | an admin's tab is drawn when the `admin` credential's `whoami` resolves, beside the `web` one in the first round trip | no field of the `web` key's record says the entry is an admin's |
| d9 | `oml-sdk`'s test reads `openapi.yaml`, run by `generate_sdks.py --check` | `OPERATIONS` in `sdks/rust/src/lib.rs`, read by `check_contract_sync.py`, which also checks the query and body field names, and every field of the response struct and of each struct it holds against the 2xx schema at that depth; `oml-sdk`'s test holds each method to its entry | the operator chose the contract script; it already holds the served surfaces to one another |
| (none) | (none) | the guardrail lines are read in `oml-web` from the wire, held equal to `oml-guardrail`'s frame by a test | `oml-web` may not link `oml-guardrail` (d1); the test is the one renderer's guard |

The second change of phase (b) ships and starts `oml-web`: its release archive, the image's second
entrypoint, the compose file's and `oml quickstart`'s `oml-web` and Caddy router, the chart's second
Deployment, `oml-web --doctor`, `oml-web`'s `/readyz` and the lead's `GET /web/sign-in`. Where it
reads a decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d2 | `oml quickstart` writes one secret to an owner-only file both processes read | the secret is `OML_WEB_CLIENT_SECRET` in quickstart's owner-only `.env`, which compose hands to both services as `web.client_secret` | the `.env` is the stack's one owner-only file of generated values; a second file would hold one more copy |
| d4 | `next` dropped to `/web` unless admitted | admitted by `oml_page::admitted_next` on `GET /web/sign-in` and again on `POST /web/sign-in`, sealed with the sign-in state, admitted a third time at the callback; a browser already signed in is sent on at once; onboarding (`/web/start`) still takes the landing of a person with no agent key | the provider round trip is the one place `next` leaves the lead's hands; one shared rule, in `oml-page`, is the one `oml-web` encodes against |
| d4 | the redirect target built from the configured public origin | built from `oauth.issuer` when `next` is not `/web`; `/web` stays the relative redirect it was | the issuer is the lead's configured origin; the relative `/web` is unchanged behaviour |
| d4 | `oml-web --doctor` and `oml doctor` with `oml-web`'s environment give the `web.api_url` line | `oml-web --doctor` alone, adding a `fail` for any other refused setting and an `api` line for the lead's readiness and web client identity; `oml doctor` does not read `oml-web`'s environment | `oml-cli` cannot link `oml-web`'s configuration without the HTTP stack its dependency test refuses; one reading, the start's own, is kept |
| d4 | (silent) | `oml-web` answers `GET /readyz` on its own listener: one unauthenticated call to the lead's `/readyz`, `200` only while the lead is ready and names a `web_client` other than `none`; a lead that names `none`, or names no `web_client` at all (reported as `unknown`), is not ready, and the doctor fails it | a probe of the process alone cannot see the hop; the public `/readyz` stays the lead's; a lead that proves no identity fails closed |
| d7 (b), d4 | pages move behind the same URLs; the proxy matches `GET /web?view=guardrails` by its query | each moved page has its own path under `/web/` (the Guardrails view at `GET /web/guardrails`), and the lead answers the old `GET /web?view=guardrails` with a `303` to the fixed `/web/guardrails`, carrying nothing from the request; Caddy, both compose routers and the chart's `web.ingressPaths` route by path alone | an Ingress, a Gateway `HTTPRoute` and most proxies match paths, not queries; a query route left the chart's Ingress unable to reach the moved page at all, so a release would have served a Guardrails tab that answers not found; a `303`, not a `301` or `308`, because browsers cache those and pages are still moving |
| d4 Kubernetes | (silent) | the in-cluster hop is plain http under `web.allowPlainHttpApi`, on by default; the web client secret comes from `web.clientSecret.existingSecret`, the recommended path, or a Secret the chart generates once with `lookup` and keeps on uninstall | a render without a cluster (GitOps, `helm template`) cannot read a generated value back and would rotate it; a Secret the operator owns never does |
| (none) | (none) | CI signs a person in through the real routes with a build of the lead carrying the `e2e-sign-in` feature, which answers every GitHub exchange with one address (`OML_E2E_SIGN_IN_EMAIL`), and loads `/web/guardrails` through the router in the docker profile, `oml quickstart` and the chart's Ingress | the provider endpoints are constants (ADR-0045 d17) no runner can answer; the feature is off in every release build, and `check_release.py` fails a release workflow or Dockerfile default that turns it on |

The third change of phase (b) moves the Workareas view of the user page and of the admin page,
and with it the first acts `oml-web` runs: the confirmed dropdown's status transitions,
ratifications and mode changes (ADR-0045 d28). Where it reads a decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d7 (b), d4 | (silent on acts) | the view lives at `/web/workareas` and `/web/admin/workareas`, `GET` and `POST` each; the lead answers `GET /web?view=workarea` and `GET /web/admin?view=workarea` with a `303` there, carrying nothing from the request, and answers the old `workarea_mode`, `workarea_transition` and `workarea_ratify` acts of `POST /web` and `POST /web/admin` with a `303` to the view, running nothing; the Atlas panel's dropdown asks the new path directly | a cached page's form must not act twice in two processes; a `303` that runs nothing loses one click and never an act a person did not confirm |
| d5 #4 | the offers name the acts admitted now | they also name, under `refused`, each of proposing either rung, putting the workarea in progress and committing it that is not admitted, with the code and message its verb would answer | the lead's confirmation names a refused act's refusal by code (ADR-0045 d28), and `oml-web` cannot run the verb's check; a field on the existing operation, not a dry-run operation (rejected alternatives: a GraphQL or backend-for-frontend aggregate) |
| d3 layer 4, d5 | `Idempotency-Key` taken by the transition, ratify and mode operations | each replays a recorded outcome under the same key and body, with `Idempotency-Replay: true`, and acts nothing again | the confirmation's nonce makes the key, so a resubmitted form replays; without the replay a second proposal or ratification would be refused or repeated |
| d3 layer 4 | every form carries a nonce | the confirmation's form does; a post without one acts once under a fresh key, and one whose nonce is not 32 lower-case hex characters is `request.validation`; a second send of the same form while the first is in flight waits for it in the process and replays | the old forms and the parity suite's acts carry none; a fresh key keeps such a post from replaying another form's act; a mangled nonce must not lose the replay silently; the API claims the key before it acts, so two sends racing in two processes act once ([#233](https://github.com/e8kor/open-memory-lab/issues/233), the table of 2026-11-11 below) |
| d3 layer 1 | `web_action` joins with the first act | the page line names `web_action`, the form's action | as decided |
| d3 layer 6, d8 | the per-session cap on calls in flight; two sequential calls | not built; the view bounds its own fan-out to eight calls in flight (`oml_web::workareas::FAN_OUT`, unmeasured, #170) after the tree, each row's offers and live proposals a secondary read: a failed offers read marks only its row unavailable, and a failed proposals read shows none, unmarked | a page of N workareas makes 2N calls on the user page and 3N on the admin page after the tree, where the lead made the same reads in process, and each offers call now runs up to four more verb checks for `refused`; both belong in #170's measurement; the cap across a session's pages lands with the memories view, which fans out further |
| (none) | (none) | creating a workarea stays the lead's act of the user page: the view's form posts it to `POST /web`, which answers `303 /web/workareas?done=created` | `POST /v2/workareas` serves it, but the act is not one of the dropdown's; it moves with the keys view, whose other acts it shares |

The fourth change of phase (b) moves the Memories list and the Atlas of both pages, their JSON
routes, and the creation of a workarea. Where it reads a decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d7 (b), d4 | (silent on the list and the Atlas) | the list lives at `/web/memories` and `/web/admin/memories`, the Atlas at `/web/atlas` and `/web/admin/atlas`, the JSON at `/web/data/memories` and `/web/admin/data/memories`, each `GET`; the lead answers `GET /web?view=memories` and `GET /web/admin?view=memories` with a `303` to the list, or with `display=atlas` to the Atlas, carrying nothing from the request; one memory opened by its id, `?view=memories&memory=<id>`, stays the lead's view, and each row links there | the one-memory view carries the admin page's supersede form and the Mermaid frame, which move with the review and curate views; a row's link to it is a plain `GET` across the router |
| d7 (b), (none) | creating a workarea stays the lead's act (third change) | it is the Workareas view's act, `POST /web/workareas` with `action=create_workarea`, through `POST /v2/workareas` as `<user>-web` under the form's nonce; the lead answers the old act with a `303` to the view, running nothing | the view's other acts already run through `oml-web`; leaving one act behind kept a second form path to the same page |
| d3 layer 4, d5 | `POST /v2/workareas` takes `Idempotency-Key` | it replays a recorded creation under the same key and body with `Idempotency-Replay: true`, and refuses the key on another body with `idempotency.digest_mismatch` | a resubmitted create form would otherwise answer `name.taken` for a creation that succeeded |
| d5 | the listing with `with_own`, the admin search and the tree with `counts` serve the list and the Atlas | they do, with fields joining: `include_text` on the `with_own` listing; `author_key_id` and `scan_finding` on every served memory, the key id null where the principal is; `held_by`, `promoted_at`, `valid_from`, `valid_until` and `scan_finding` on the admin read's items, whose page now holds up to 200; the person's keys from `GET /v2/keys` name each own row's kind; the breadcrumb is read off the scope's path | each is a field the row showed and no operation served; a field on an existing operation, as the third change did for the offers, not a page-shaped aggregate |
| d5 | `/web/data/memories` and `/web/admin/data/memories` stay page routes of `oml-web` | they are, and leave `openapi.yaml`, which declares the lead's routes; no session is `access.not_found` as JSON, not a redirect | the document is held equal to the lead's route table; the routes' contract is the page's own |
| d8 | at most two sequential calls | the list makes them (whoami; the listing and the keys, or the admin search); the Atlas reads the counts and the scope's offers together after whoami, the offers a secondary read whose failure marks the panel unavailable; a text search reads the keys before recall, a third sequential call | the author filter of a text search names a key the recall answer does not; #170 measures it |

The fifth change of phase (b) moves the Review view and the Keys view of the user page and of the
admin page, with their acts: review next, the verdict, the bulk confirmation and bulk verdict; issuing
a key or a connector key, a new connect code and the cut-off (ADR-0045 d9, d11, d13; ADR-0055 d10).
Where it reads a decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d7 (b), d4 | the Review view at `GET /web?view=review` and `/web/admin?view=review`, bulk review at `/web/admin?view=held`, the queue at `/web/admin?view=queue`, the Keys view at `GET /web` | the Review view at `/web/review` and `/web/admin/review`, the person's holding review next only and the admin's the queue and bulk review above and below it, bulk review staying the admin page's (ADR-0055 d1), so the person's page refuses its acts as no action of the page; the Keys view at `/web/keys` and `/web/admin/keys`, the admin's one person's keys named by `user`; `GET` and `POST` each. The lead answers the old views, and a signed-in `GET /web` naming no view, with a `303` there, carrying nothing from the request; the old acts of `POST /web` and `POST /web/admin` get a `303` to their view and run nothing. The admin tabs lose Queue and Bulk review and gain Keys | the queue and bulk review are reads and acts of the same reviewer key on the same rows; one admin path keeps the Ingress list short, and the removal confirmation's cut-offs post to the admin Keys view |
| d3 layer 4 | Post/Redirect/Get for every act | a key's value or a connect code is shown once on the redirect target: the act seals it, ChaCha20-Poly1305 under a key derived from the session token, into a `__Host-oml_once` cookie (`HttpOnly`, `Secure`, `SameSite=Strict`, five minutes, see the follow-ups of #253) that the target opens, shows under `no-store` and clears in the same answer; it is never in a URL, a line or `oml-web`'s memory past the request | the API shows the secret exactly once, so the target cannot read it again; the session-derived key lets any replica open it and no other session; the cookie, not a server-side store, keeps `oml-web` stateless (rejected: a short-lived store in `oml-web`, which replicas would have to share; the value in the redirect's query, which proxies log) |
| d3 layer 4 | the bulk verdict's page shows each item's outcome | the bulk verdict redirects to the view with its counts, approved, rejected, skipped, by the same user and replayed, as numbers only; the confirmation, which records nothing, is answered as a page, not a redirect | per-item outcomes do not fit a URL and are not stored; the items left are still listed on the view; a crafted link can put only numbers on the page |
| d3 layer 4, d5 | the verdict's idempotency key minted from the serve | from the form's nonce, as every act's; review next is a `POST` under its own key that redirects to the view with the serve id and the served memory's id, and the view draws the served card by that memory id from `GET /v2/review/held?ids=`, which claims nothing, never reading the serve id, which the contract leaves opaque; the claim that follows a verdict runs under a key derived from the verdict's, so a resubmitted verdict replays its claim too, and a verdict recorded before that claim fails lands on the view with its notice | a `GET` that served the next item would claim one on every reload; a recorded verdict is not an error |
| d3 layer 4, d5 | `Idempotency-Key` taken by `POST /v2/keys`, `POST /v2/keys/{key}/connect-codes` and `POST /v2/keys/{key}/denials` | as a session key each replays its recorded outcome under the same key and body with `Idempotency-Replay: true` and acts nothing again; the recorded body leaves out the key's value and the connect code, which are shown once; `oml-web`, given an answer without the value or the code, redirects to the view and sets no cookie, so the first answer's sealed secret stands (see the follow-ups of #253) | a double click would otherwise issue twice under one name, which is refused, or mint a second code that voids the first; storing the secret to replay it would keep what is shown once |
| d5 | `Memory` names the format and title | it also names `search_text`, what a derived format's claim will be found by, absent for prose | the reviewer's card shows it before the verdict (ADR-0056 d9) and `oml-web` cannot derive it |
| d5 | (silent) | the moved card does not draw a Mermaid claim: it shows the whole source, the format and the title, and says the drawing shows on the memory's own page once the memory is promoted, with no link | the frame's token is sealed by the lead's per-process key over a source it caches (ADR-0056 d9); an API to mint one has no other use case; a held memory's own page opens only for its author and the agent's owner, so a link would answer a reviewer who is neither `access.not_found`. Drawing it on the card is an exit criterion of the change that moves the memory's own page ([#189](https://github.com/e8kor/open-memory-lab/issues/189)) |
| (none) | the person lane (ADR-0074 d19) on the Review view | not built: it is not on `main` | it moves with the view when it lands |

The sixth change of phase (b) moves one memory opened by its id, on the user page and on the
admin page with the admin page's supersede act, the admin page's Curate view with its act, and the
frame that draws a Mermaid claim (ADR-0056 d9). The Start page, the admin overview (the allowlist,
the capacities and the removal confirmation) and the users and quotas page stay the lead's until the
next change. Where it reads a decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d7 (b), d4 | one memory opened by its id stays the lead's view (fourth change); the lead's `303` carries nothing from the request | one memory is at `/web/memory?id=` and, with its attribution and the supersede form, at `/web/admin/memory?id=` (`GET`, and `POST` for the admin's supersede act); Curate is at `/web/admin/curate`, `GET` and `POST`. The lead answers `?view=memories&memory=<id>` on either page with a `303` to the new page carrying the id alone, parsed as a UUID and written anew, and a `memory` that is not one id with no redirect; `?view=curate` with a `303` carrying nothing; the old `supersede` and `curate` acts with a `303` to the admin Memories list and to Curate, running nothing | a link to one memory without its id is a link to nothing; a parsed and re-serialized id carries no byte of the request, so the rule's purpose (no smuggled target) holds |
| d1, d4 | the frame route `web/render.rs` belongs to `oml-web`; the routes are exact paths | `oml-web` serves `GET /web/render/{token}` and `GET /web/assets/{file}`, listed as `oml_web::PREFIXES`; Caddy and both compose routers match `/web/render/*` and `/web/assets/*`, the chart's `web.ingressPaths` lists `/web/render` and `/web/assets` as `Prefix`, and `check_chart.py` holds each listed `pathType`. The token is sealed under a key `oml-web` draws at first use (ChaCha20-Poly1305), bound to the session's CSRF token, and the source is held in its memory, bounded as before (`RENDER_SOURCES_MAX`); the frame answers only after `GET /v2/whoami` resolves the session's `web` credential, and every failure is `404` | a token names a source, so the route cannot be one exact path; the process that showed the source holds it, which d4 already names as the reason replicas need affinity for this route; the session is the API's to judge (d3 layer 3) |
| d3 layer 5 | every page under the one policy with no `frame-src` | a page that holds a drawing, decided by whether one was drawn and never by the page's text, names `frame-src <public origin>/web/render/`; the frame keeps its own policy and `Referrer-Policy: no-referrer`, and the bundle its `immutable` cache rule. The headers layer keeps a handler's policy, cache rule and referrer policy only on an answer marked `OwnHeaders`, which `oml_web::render` and, for the Start page's script, `oml_web::start` set; every other answer gets the pages' headers whatever its handler set | the lead framed only where a page showed a diagram, and the frame's policy is ADR-0056 d9's; an explicit mark, not the header's presence, so a future handler cannot weaken the pages' policy by setting one |
| d1, d4 | 256 held sources, one pool | at most `RENDER_SOURCES_PER_SESSION` (32) of them per session, which gives up its own oldest first, inside the pool of 256 | one session opening many diagrams would otherwise evict every other session's, whose frames then answer `404` (#159; chosen, not measured) |
| d5 | (the fifth change) the Review card shows a Mermaid claim's source and says the drawing shows on the memory's own page once promoted, linking nothing | the Review card draws the frame from the source `GET /v2/review/held?ids=` served the reviewer, bound to the reviewer's session; still no link to the held memory's own page, which opens only for its author and the agent's owner | the exit criterion named on #189: `oml-web` now mints the frame's token over a source it was served, so no API operation is needed for it |
| d5 | the admin read serves the fields of the search plus attribution | it also names `kind`, on every item of the search and on the read; the page refuses a supersession unless the read names a kind other than `guardrail`, so a server that names none (one before v0.7.0, during a rolling upgrade) is refused too | the admin page refuses to supersede or retire a guardrail (ADR-0037 d17) and `oml-web` cannot otherwise tell one; the API does not refuse a guardrail's retirement without a rule, so the page's check fails closed |
| d5 | `submit_memory` for `curate` and `supersede`, as the capacity the act needs | the supersession runs as `<user>-curate` when `GET /v2/whoami` as that capacity names the key the admin read says wrote the memory, else as `<user>-review`, and only `auth.invalid` from that `whoami` reads as no curator key: any other failure fails the act, so the acting key never changes because a call failed; curation runs as `<user>-curate`, and a session whose curate credential the API does not resolve gets `access.not_found` naming the next sign-in, never the sign-in redirect; both under the form's nonce as `Idempotency-Key`, so a resubmitted form replays the recorded answer (the operations' own record and replay) | the lead compared key ids in process; the key's name is what the wire serves. Capacities are fixed when the session opens (phase (a) as built), so a designation given during the session reaches the next one; the moved curation test signs in again after it |
| d3 layer 4 | the lead's curate and supersede keys were digests of the source and the text | the act's key is the form's nonce's, as every moved act's | a person who curates the same text twice on purpose writes twice, as the API does for any client |

The second half of the sixth change (6b) moves the onboarding page `/web/start` with its status
poll and export, the setup view, the admin overview (the allowlist, its admin flag, the removal
confirmation and the review and curation grants) and the users page (quotas, disabling, deletion
requests). After it the lead serves no page but sign-in, the sign-up terms page and sign-out, so
phase (b) is complete. Where it reads a decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d7 (b), d4 | (silent on the onboarding and users pages, which kept their paths) | `oml-web` serves `/web/start` (`GET`, `POST`), `/web/start/status` and `/web/start/export` (`GET`), `/web/setup`, `/web/admin/people` (`GET`, `POST`) and `/web/admin/users` (`GET`, `POST`); the lead's routes for the four old ones are deleted, so the router sends them to `oml-web`; the lead answers `GET /web?view=setup` with a `303` to `/web/setup` and every `GET /web/admin` with a `303` to the view a `view` names or to `/web/admin/people`, before reading a session and carrying nothing from the request, and the overview's old acts with a `303` there, running nothing | the admin page's default view had no path of its own; one fixed path keeps the router's list exact |
| d6 | the sign-in page and the auth routes stay the lead's | so do the sign-up terms page and `POST /web/sign-up`, and sign-out (`POST /web`, `sign_out`), which ends the session in the auth context | they open or end a session, which `oml-web` never does |
| d4 | a person with no session gets a `303` to sign-in | the onboarding and users pages now answer that `303` where the lead answered the sign-in page (`/web/start`) and `404` (`/web/admin/users`); the status poll and the export are data and answer `404` without a session, never a redirect | every moved page loads its session the same way; data a script fetches must not follow a redirect into a page |
| d5 | `web_start_status` is `GET /v2/keys` and the listing with `with_own` | `GET /v2/whoami?start=true` names `person.start`: `connected` and `first_memory` | whether an agent key connected is read off the lead's request windows, which no listing serves; a query on an existing operation, read only when asked so the session check every page makes stays one lookup |
| d3 layer 4, d5 | #3, #7, #8, #9 and #10 replay | `POST /v2/web/emails` and `/v2/web/emails/remove` replay too, claiming their key before they act as every console act does since #259, so two sends racing act once (`two_concurrent_allowlist_adds_under_one_key_add_once_and_answer_alike`, `two_concurrent_removals_under_one_key_remove_once_and_answer_alike`); the page sends `keep_an_admin: true` on both, so the last admin entry is refused by the API in the act's own transaction, not by a read before it | a resubmitted removal would otherwise answer `access.not_found` for a removal that succeeded; the page's own check could race another admin |
| d3 layer 4 | Post/Redirect/Get for every act | the Start page's issue redirects to `/web/start?client=<slug>` and shows the value or the connect code once through the sealed cookie of the fifth change; each of the up to nine names it tries runs under its own key derived from the form's nonce, so a resubmission replays the attempt that issued | the lead answered the credential in the act's own response; the cookie keeps it out of a URL and a line |
| d3 layer 4 | (the lead disabled a person and closed a deletion request on one post) | removing an address, disabling a person and closing a deletion request each answer a confirmation first, which records nothing; only its form, carrying `confirm=yes` and a fresh nonce, acts, and any other post acts nothing (the nonce is not required, since a post without one acts under a fresh key, and the admin acts accept `yes`, `true` or `on`) | each cuts off keys or ends sessions, which no resubmission or stray click should do unconfirmed |
| d2 | each resolved request passes the person's request rate (phase (a) as built) | a person's session keys are held to their `requests_per_minute` or 60, whichever is more (`SESSION_REQUESTS_PER_MINUTE_MIN`); their agent keys to the quota as set. A page view spends one to four of the web key's requests (the session check, then the page's reads), so a quota set below the floor binds the agents and never locks the person out of the console that shows it | the pages issue reads and the person's own acts, never an agent's traffic; exempting the web key outright would leave it unbounded. 60 is chosen, not measured (#170) |
| d5, d8 | `GET /v2/whoami?start=true` | it authenticates once, so it costs one request of the key's rate as a plain `whoami` does; the status poll and the export make one call each, the session checked by that call, where a page spends the session check first; a refusal keeps the API's status and code, `limit.rate` with its `Retry-After`, and only an unresolved credential is `404` | the poll runs every five seconds; a second lookup per poll doubled its cost, and a `404` for a rate refusal named the wrong cause |
| d3 layer 4 | each name the Start page tries runs under its own key derived from the form's nonce | as built; a resubmission meets `name.taken` on the names before the one that issued and replays that one, so it issues nothing (`resubmitting_an_issue_that_landed_on_the_next_free_name_issues_nothing_new`). An issue that committed but whose outcome was never recorded holds its claim, since `POST /v2/keys` claims its key before it issues (#233, #259), so the resubmission meets `idempotency.in_progress` on that name and issues no next one (`an_issue_whose_name_a_stopped_attempt_holds_issues_no_next_name`) | the claim-first record closes the window between the commit and the record |
| d8 | at most two sequential calls | the overview reads the allowlist 200 entries a call and refuses past 50 calls (`limit.list_too_long`) rather than show it cut short | `listWebEmails` pages by digest; a list read partly would hide an admin entry from the page that guards the last one |

## The client secret as built (2026-11-11)

Issues #230 and #242 specified where `web.client_secret` lives, which values either process
refuses, and how it rotates. The same-origin check #230 also named is one function both processes
call, built in the first change of phase (b). Where they read d2 and d4:

| Decision | Was | Is | Why |
|---|---|---|---|
| d2 | the secret is a setting of both processes; quickstart keeps it in its owner-only `.env` | either process also reads it from `web.client_secret_file`, a file its group and others cannot read (a mode with any of `077` stops the start, naming the setting, the path and the mode), one trailing line ending dropped; setting both stops the start | systemd's `LoadCredential=` and a mounted Secret hand a process a file, not a variable; the owner-only rule is the keyring's (ADR-0023 d4), so a secret file is held as the signing key is |
| d2 | at least 32 characters | at least 32 visible ASCII characters, at least 8 of them distinct, checked by `oml_web_credential::client_secret_refusal`, which the lead and `oml-web` both call; a refusal names the setting, never the value | a header carries visible ASCII whole, so a space or line ending would make the two processes compare different bytes; a repeated placeholder passes a length check; one rule in the leaf both link keeps them from refusing different secrets. Eight is chosen, not measured: 32 random hex digits hold fewer with a probability below one in ten million, so `oml quickstart` writes 64 (#262) |
| d2 | (silent on rotation) | the lead accepts `web.client_secret_previous` (or `_previous_file`) beside the current secret until `web.client_secret_previous_until`, an RFC 3339 instant checked on every request and at most 7 days after the lead's start; a previous secret without the current one, equal to it, or without the instant stops the start, naming the setting; an instant already past loads and proves nothing; each start logs `web_client_previous_accepted` or `web_client_previous_closed` until it is unset. `oml-web` sends one secret. `/readyz` still names `secret` | the order is the lead with both, then `oml-web` onto the new one, then the lead with one, so no page is refused while `oml-web` restarts. An absolute instant bounds an old secret's life without a restart and does not reopen when the lead restarts. Rejected: a list of accepted secrets with no bound, which a forgotten entry keeps open; a window counted from the start, which every restart reopens; `oml-web` sending both, which puts two secrets on every call. The `/readyz` enum is unchanged, so no SDK changes for a state the start's log already names. Seven days is chosen, not measured |
| d2, Consequences | the doctor reads the lead's `web_client` from `/readyz` | `oml-web --doctor` also sends `GET /v2/whoami` with a capacity credential no session holds: `session_outside_web` is a `fail` naming `web.client_secret`, any other `auth.invalid` is `ok` | `/readyz` is unauthenticated and cannot say whether the two secrets match, which is the failure a rotation done out of order or a closed window causes; the gate's existing refusal reasons tell the cases apart, so no operation is added |
| d2 | the secret or mutual TLS | unchanged: mutual TLS is not built and `web.client_ca` or `web.client_subject` stops the start; `operations/web-console.md` documents it as the alternative for a hop that leaves the host, and an `https://` hop to a TLS proxy with the secret as what this build offers | the lead still terminates no TLS (phase (a) as built) |
| d4 Kubernetes | `existingSecret`, or a Secret the chart generates once with `lookup` and keeps | also: the lead alone reads `OML_WEB__CLIENT_SECRET_PREVIOUS` and `OML_WEB__CLIENT_SECRET_PREVIOUS_UNTIL` from the same Secret (`web.clientSecret.previousKey`, `previousUntilKey`), each `optional`; an upgrade keeps every key of the generated Secret, not only the current one; `values.yaml` names Secrets and keys and never a value; `check_chart.py` holds all of it and that `existingSecret` renders no Secret of the chart's own, except the `lookup` read-back and that an upgrade keeps every key, which need a cluster and no check holds. A rotation edits the Secret and restarts the lead, then `oml-web` | a pod reads a Secret's keys at start, so the order of the two restarts is the rotation; no `checksum/` annotation, since the chart cannot read an `existingSecret`'s value and restarting both at once is the gap the order avoids |
| (none) | `check_release.py` refuses `e2e-sign-in` in `release.yml` | also refuses `OML_CARGO_FEATURES` there | a workflow variable passed as that build argument would carry the feature past a check of its name (#242) |
| (none) | the remaining minors of #241's review | not built: a check of the published image for the sign-in double (it needs the pushed image, a release step of its own); the SBOM loop's repeated work (slower, not wrong); the old `?view=` URL's dropped query (deliberate); an upgrade path for a directory an older `oml quickstart` wrote, which has no `oml-web` service and no `OML_WEB_CLIENT_SECRET` | each is a follow-up, not a defect of this change; the quickstart upgrade belongs to `oml quickstart`, not to the secret |
| d2 (#262) | a secret file's mode is read by path, then the file is read by path again | the file is opened once, and its owner and mode are read from that handle; a file owned by a user other than the process's or root stops the start, naming the owner | two opens let the file change between the check and the read; a file another user owns is that user's secret to change. Root may own it, since root can rewrite any file and systemd's `LoadCredential=` may hand one over as root's |
| d2 (#262) | a mounted Secret hands the process a file | on Kubernetes the secret is an environment variable from the Secret (`secretKeyRef`), as the chart sets it; a Secret mounted as a volume is refused by `web.client_secret_file` | a mounted Secret is owned by root and, with `fsGroup`, group-readable; with `defaultMode: 0400` and no `fsGroup`, a non-root process cannot read it |
| d2 (#262) | a `web.client_secret` set and empty on the lead counts as unset | it stops the start, naming the setting; empty `_previous` and `_previous_until` still count as unset | a blank value is a template that lost its secret, and before #230 it stopped the start too; a rotation's two keys are unset by blanking them as often as by removing them |
| d2 (#262) | `web.client_secret_previous_until` read as a valid-time period's first instant | a date, month or year alone stops the start | a bare date names a day, not the instant the old secret stops; reading it as midnight UTC would close the window hours before an operator in another zone expects |
| d2, Consequences (#262) | the doctor's fail line names a different secret | it names the three causes of `session_outside_web`: a different secret, none on the lead, or a proxy that adds a client-certificate header | the lead answers all three the same way, and only the operator can tell them apart |
| d2, Consequences (#262) | each `oml-web --doctor` run adds a 401 request line and a histogram entry on the lead | the probe's credential, `oml_web_credential::DOCTOR_PROBE_CREDENTIAL`, proven by the secret, is answered `auth.invalid` as before and writes no line and no histogram entry, as a probe answered 200 writes none (ADR-0042 d1); an unproven probe is logged as any refusal is | a doctor run is not an unknown credential, and an alert on 401s should not fire for it; the credential is fixed and derives from no session, so a caller who sends it hides nothing that could authenticate |

## Idempotency claims (2026-11-11, #233)

Phase (a) recorded a console act's outcome after the act's last commit. Two sends of one form
that reached the API at once, as a double click does through two `oml-web` replicas, both found
no record and both acted: the second issue of a key answered `name.taken`, and its value was
never shown. A process that stopped between the act and the record left an act a resubmission
repeated. d3 layer 4 and d5 promised neither. Recording inside the act's own transaction, which
the memory, supersede and verdict operations already do, cannot cover these acts: granting
capacities, cutting off, disabling and the key operations commit in more than one transaction,
and a key's value exists only in the answer. So each act claims its key first, as the HTTP
contract's `Idempotency-Key` row says (claims a record before side effects):

| Decision | Was | Is | Why |
|---|---|---|---|
| d3 layer 4, d5 | the outcome recorded after the act's last commit | the act claims `(key, act, Idempotency-Key)` in a transaction of its own before it acts, an `idempotency` row with `committed_at` null; it writes its outcome into that row after its last commit, and an act refused before anything committed deletes the row, so its resubmission acts again | of two concurrent claims the primary key admits one; a claim is the only shape that covers an act of several commits; the table's nullable `response` and `committed_at` (0001) already hold it, so migration 0042 only grants the serving role `UPDATE` of `response` and `committed_at`, and `DELETE` (rejected: recording in the act's last transaction, which leaves the first commits of a multi-commit act unguarded and a concurrent second send acting; an advisory lock per key, which a second process holds no knowledge of once the first commits and dies) |
| d3 layer 4 | (silent) | a send that finds the key claimed waits up to 5 s, claiming again after pauses that double from 25 ms to 250 ms, and replays the outcome once recorded; past the wait it answers `409 idempotency.in_progress` with `Retry-After: 1`, which [errors](../api/errors.md) defines | 5 s sits under `oml-web`'s 10 s call timeout (d8), so a double click is answered the first send's outcome, never a timeout; the second answer is the replay, so a key's value or connect code is in the first answer alone and never in the record |
| d3 layer 4 | (silent) | an act whose outcome cannot be written after it committed still answers that outcome; its claim stays, so a resubmission answers `idempotency.in_progress` and acts nothing; the lead logs `idempotency_unsettled` with the act, the key id and the claim id, counts `oml_idempotency_unsettled_total`, and `oml doctor` warns on any | the act happened and the person should see it, a key's value included; a held claim fails closed: nothing is repeated, and a fresh form acts under a fresh key. A claim whose process died is not reclaimed after a time, because whether its act committed is unknown; the operator clears one by hand after checking the journal ([runbook](../operations/runbooks.md#a-form-answers-idempotencyin_progress)), as no sweeper removes them yet |
| d3 layer 4 | (silent) | the act and its record run in a task of their own, so a request dropped mid-act (`oml-web`'s call timeout, a closed connection, a lead shutting down gracefully) cannot stop the act between its commit and its record: the act records its outcome and the resubmission replays it. Only a refusal (`4xx`) of the act's first commit, or a refusal before the act, releases the claim; a failure that may have committed (a `5xx`, a lost connection), a failing step after the commit, a later step of a multi-commit act refused after an earlier one committed, and a panic all leave it | a release after a commit lets the resubmission act twice, which is the failure #233 names; the first review of the claims found that dropping the request, a read after the commit failing, and a panic each released it (rejected: releasing on every drop of the claim, which repeats the act in exactly those cases) |
| d3 layer 4 | (silent) | each attempt draws a claim id, kept in the claim row's `response` beside the time it was claimed; settle and release name it, so a late release of one attempt never drops another's claim; the wait between claims backs off from 25 ms to 250 ms; the serving role may update a row's `response` and `committed_at` alone | an operator removing a stuck claim must not let an old attempt's release drop the next attempt's; about twenty-five claim transactions per five-second wait instead of two hundred; no statement of the serving role can rewrite a row's key or digest |
| d3 layer 4 | (silent) | the act's task carries the request's traffic scope and session beside its span, so what the act observes (a mode change's count, the workarea, the store time and calls) is the request's | a spawned task sees no task-local of the request: `oml_workarea_mode_changes_total` stopped counting mode changes made over HTTP, and the request line lost the act's store time and workarea, until the second review of the claims found it |
| d5; [ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) d7 | `POST /v2/keys` and `POST /v2/keys/{key}/connect-codes` replayed only for a person's session key | every key claims, admin and bearer keys too: a resubmission replays the first answer without the key's value or the connect code, and a request dropped after its commit never issues or mints again | the same double issue a process stopping between the commit and the record caused for the console's acts, under the next name a client chose; ADR-0031 d7 already specified a replay without the code, and no record now holds a key's value, which is what #127 asks for |

## The follow-ups of phase (b) (2026-11-11, #253)

The independent reviews of phase (b)'s fourth, fifth and sixth changes left follow-ups that
blocked no merge ([#253](https://github.com/e8kor/open-memory-lab/issues/253)). Where they read a
decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d3 layer 6, d8 | the cap across a session's pages lands with the memories view (the third change's table) | not built, and promised to no page: the Memories list and the Atlas make at most four calls after `whoami` and bound none of their own, so the cap waits for [#170](https://github.com/e8kor/open-memory-lab/issues/170) to measure the moved pages' fan-out and choose the bound | the fourth change's table was silent on it; a constant chosen before the measurement is the guess d3 layer 6 already names |
| d8 | the list reads up to 10 pages of the person's keys | past 10 pages of 200 the list and its JSON are refused with `limit.list_too_long` (`422`), as the overview refuses a long allowlist; at a cursor the API repeats, at once or in a cycle, they are refused `unavailable` (`502`), the API's fault (#292). The overview's allowlist read answers the same `502` at a repeated cursor (#293). The Keys views and the confirmation before a person's last address is removed read the key list the same way, whole or refused: `limit.list_too_long` past 50 pages of 200, `unavailable` (`502`) at a repeated cursor (#308). The four cursor walks, the workarea tree's with them, share one walker (`oml_web::pages::walk`); where the tree's walk passes its bound or meets a repeated cursor, the Guardrails page draws without its selector and the Workareas views answer `unavailable` (`502`) | a list cut short silently leaves rows without their kind and answers an author search `access.not_found` for a key the person holds |
| d8 | the Atlas's offers a secondary read whose failure marks the panel unavailable | the JSON route reads them the same way: the scope's region carries `offers_unavailable: true` and no `offers`, and the rows and regions still answer; a failed listing or count is the API's refusal as JSON | the route serves the Atlas's data, so it degrades where the Atlas does |
| d5 | `scan_finding` on every served memory | null wherever `principal` is, on a `universe` row served to a key outside its authoring key set | an anonymous row carries its text and nothing admission recorded of its writing ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) d9) |
| d5 | the admin read names the authoring key by its name | it also serves `key_id`, and the admin memory page names a key with no name by its id | "written by key" was empty for a nameless key |
| d3 layer 4 | the shown-once seal under SHA-256 of a purpose and the session token, kept five minutes | under HMAC-SHA256 keyed by the session token over `oml/web-once/v1` (`oml_web_credential::once_seal_key`, beside the session's other derivations), kept one minute (`Max-Age=60`) | `__Host-` forces `Path=/`, so the cookie rides every request of the site until the target clears it; a redirect is followed at once. Neither process's request line reads `Cookie`, and `oml-web` never forwards it; a proxy in front must not log it (Caddy blanks it unless `log_credentials` is set). A seal in flight across the upgrade does not open; the key is listed and the person issues another |
| d3 layer 4 | (accepted in the fifth change's table, unstated) | a seal is cleared by the browser alone: a copy taken out of the browser opens with the session token until it expires | stated so it stays visible: the session token already reaches that session's keys, and a server-side store, which would end the window, is the alternative the fifth change rejected |
| d3 layer 4 | (silent) | one cookie name serves every act: two acts that show a secret, sent from two tabs of one session before either target loads, leave the second seal, and the first secret is not shown; the key is listed, and the person cuts it off and issues another, or asks a new connect code | a cookie per act needs the target to name its act in the URL; a lost value costs one reissue and shows no secret twice (documented in `oml_web::once`) |
| d3 layer 4; [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md) d13 | the cut-off acts on one post | it answers a confirmation first, which records nothing; only its form, carrying `confirm=yes`, acts (a nonce is not required: a post without one acts under a fresh key), and a resubmitted confirmation replays one cut-off | parity with the lead and with the sixth change's acts that cut keys off (`a_cut_off_is_confirmed_first_and_nothing_is_cut_off_until_it_is`) |
| d5 | `oml-web` read an answer with neither the value nor the code as a replay | it reads `Idempotency-Replay: true` (`oml_sdk::KeyIssued::replayed`, `ConnectCode::replayed`); an answer without the secret that is not a replay is refused `unavailable` (`502`), naming it, and sets no cookie | the header is the contract's replay signal; a body missing its secret is a fault, not a replay |
| d5 | the connect code's expiry shown as its instant | "expires in N minutes (HH:MM UTC)", the minutes read on `oml-web`'s clock; an instant that is not UTC is shown as served | the person reads how long they have, not a timestamp |
| d2 (the seventh table) | each console act resolves its caller for the claim, then acts through the request's credential, which resolves it again | the act runs as the caller it resolved for the claim (`Bearer::Session`), whose grants are read again, so creating a workarea, a transition, a ratification, a mode change, issuing a key and the two allowlist acts each cost one request of the key's rate (`a_workarea_create_by_an_agent_key_costs_one_request_of_its_rate`) | a bearer key at its rate was refused by its own act's second charge; a new connect code by a bearer key runs as the resolved caller too since #281 |

## Phase (c) as built (2026-11-11)

Phase (c) (#190) takes the `web_*` use cases out of `oml-app`'s core. It changes no answer: the
operations and their refusals, the lead's routes and redirects, the journal, and every metric and
log name are as phase (b) left them, and the workspace's suites pass before and after with the
same counts, test binary by test binary, against a throwaway PostgreSQL. The exit check,
`grep -rn 'pub async fn web_' crates/oml-app --include='*.rs' | grep -v 'crates/oml-app/src/app/auth.rs'`,
printed nothing when the phase closed. Since #297 it prints one line, `web_credential_rows_read_apart` in `ports.rs`, the store port's default read and no use case.

**Moved to the auth module, under their names.** `crates/oml-app/src/app/auth.rs` holds what d6
keeps in the auth context: `web_sign_in_start`, `web_admit`, `web_sign_up` and
`check_signup_policy`, `web_ensure_keys`, `web_session_open`, `web_session`, `web_sign_out` (which
`POST /v2/web/session/end` and the sign-out act both call), `web_credential_caller` (the API's resolution of an `omls_` credential, which since #297 reads the store's `web_credential_rows`), `web_caller`, and
`web_has_agent_key`, which picks the callback's landing (ADR-0068 d10). `web.rs` keeps the
allowlist alone and is now `emails.rs`.

**Renamed to the operation they serve.**

| Was | Is | Serves |
|---|---|---|
| `web_emails` | `list_web_emails` | `GET /v2/web/emails` |
| `web_users` | `list_web_users` | `GET /v2/web/users` |
| `web_deletions` | `pending_deletions`, private | `GET /v2/web/users` |
| `web_set_quota` | `set_web_user_quota` | `POST /v2/web/users/{user}/quota` |
| `web_disable_user` | `disable_web_user` | `POST /v2/web/users/{user}/disable` |
| `web_close_deletion` | `close_web_deletion` | `POST /v2/web/users/{user}/deletion/close` |
| `web_request_deletion` | `request_web_deletion` | `POST /v2/web/deletion-requests` |
| `web_export` | `export_web_person` | `GET /v2/web/export` |
| `web_grant_review`, `web_grant_curation` | `grant_review_capacity`, `grant_curation_capacity` | `POST /v2/web/users/{user}/capacities` |
| `web_own_keys`, `web_keys_of_user` | `list_own_keys`, `list_keys_of_user` | `GET /v2/keys` |
| `web_path` | `workarea_path` | `GET /v2/keys`, a grant's path |
| `web_issue_key` | `issue_own_key` | `POST /v2/keys` as `<user>-web` |
| `web_connect_code` | `mint_own_connect_code` | `POST /v2/keys/{key}/connect-codes` as `<user>-web` |
| `web_cut_off` | `cut_off_key` | `POST /v2/keys/{key}/denials` as `<user>-web` or `<user>-admin` |
| `web_queue` | `review_queue` | `GET /v2/review/queue` |
| `web_memories` | `list_memories_with_own` | `GET /v1/memories?with_own=true` |
| `web_regions` | `user_regions` | `GET /v2/workareas/{workarea}/tree?counts=true` for a user key |
| `web_start_status` | `whoami_start`, private | `GET /v2/whoami?start=true` |
| `web_own_limits` | `whoami_limits`, private | `GET /v2/whoami`'s `person` |

**Deleted.** `web_other_admin` (the last admin entry is now the API's `keep_an_admin`, checked in
the act's transaction), `web_all_entries` (the overview pages through `GET /v2/web/emails`) and
`web_deletion_pending` (the onboarding page reads `GET /v2/whoami`'s `person`): none had a caller
after phase (b). The lead's `web::page` stops re-exporting `form`, `glyph_svg`, `hidden`, `input`,
`select`, `state_tag`, `text` and `textarea`, which only the moved pages drew; it keeps what the
sign-in page and the terms page draw.

Where it reads a decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d7 (c) | the logic behind a new operation is renamed to that operation's use case | a use case behind a `/v2/web/*` operation takes the operation's id in snake case (`listWebEmails` is `list_web_emails`); one behind `/v2/keys*`, the queue, the capacities or a changed read names what it serves (`issue_own_key`, `review_queue`, `grant_review_capacity`, `list_memories_with_own`), since `issue_key`, `mint_connect_code`, `list_memories` and `workarea_tree` already name the bearer key's path, which the session key's does not share | an operation id is unique on the wire but not in `oml-app`, where the bearer use case already holds it |
| d7 (c) | the sign-in ones move to the auth module | they move under their `web_` names, and the exit check reads every module but `auth.rs` | the auth context owns them and no page calls them; a rename would touch every sign-in test and the oml-web parity rig for no reader |
| d7 (c) | (silent on names on the wire and in metrics) | the refusal's verb names (`access.verb_ceiling` naming `web_emails` and `web_users`), the store port's methods (`web_queue`, `web_session_open`, `web_session` and the rest) and the `store_port` span names the lead's traces carry from them keep their names | a verb name is on the wire and a span name is in trace queries; changing either is a behaviour change, which this phase is not |
| d7 (b), d7 (d) | (silent on when the old URLs stop answering) | the lead's `303`s to `oml-web`'s paths stay through v0.7.0: `views.rs` (`MOVED_VIEWS`, `MOVED_ADMIN_VIEWS`, `moved_action`, `moved_memories`), the moved acts in the two page `action` sets and `GET`/`POST /web/admin`, so a bookmark or a form cached under v0.6 lands on its new path. They are deleted in v0.7.1, leaving the lead `GET /web` without a session (the sign-in page) and the sign-out act; until then phase (d)'s `the_lead_serves_no_page_route_outside_oml_web_and_auth` names them as the lead's redirects. The operator decided on 2026-11-11 that v0.7.1 deletes them: see [The old URLs deleted](#the-old-urls-deleted-2026-11-11) | v0.7.0 is the release that moves the pages, so its upgrade is the one that meets old URLs; a redirect runs nothing and costs one route, and one release of it is chosen, not measured |

## Phase (d) as built (2026-11-11)

Phase (d) (#191) makes the walls checks a pull request cannot merge past. It changes no answer:
the one code change renames the transport's handler of `GET /v2/web/export` from `web_export` to
`export_web_person`, the use case it calls, which phase (c) left under its old name.

**The two tests.** `oml_web_reaches_no_application_crate`, with its two companions in
`crates/clients/oml-web/tests/dependencies.rs`, is the dependency wall of d1, unchanged.
`the_lead_serves_no_page_route_outside_oml_web_and_auth`, in
`crates/clients/oml-web/tests/lead_routes.rs`, is the route wall. It names each of the lead's eight
web routes and asserts that `WEB_ROUTES` is exactly them: `GET` and `POST /web` (the sign-in page
without a session, and the sign-out act), `GET` and `POST /web/sign-in`,
`GET /web/callback/{provider}` (which draws the terms page for a new address), `POST /web/sign-up`,
and `GET` and `POST /web/admin`, the old admin page URL, which answers only `303`s. Six since v0.7.1, without `/web/admin`: see The old URLs deleted. It asserts that
the API table holds only `/v1/`, `/v2/` and the probes and the OAuth table only `/oauth/` and
`/.well-known/`, and that every path of `oml_web::PAGES` and `oml_web::PREFIXES` is, on the lead,
answered as an unknown path is. A page route added to the lead fails it whether it is added to
`WEB_ROUTES`, to another table, or to the router directly at one of `oml-web`'s paths.

**The job.** CI's `crate walls (oml-web)` job in `ci.yml` runs both test targets,
`cargo test --locked -p oml-web --test dependencies --test lead_routes`, without Postgres. It runs
whenever the `cargo` category does, and `ci ok (ci)` waits for it. `cargo test (Postgres)` runs both
again inside the workspace run.

**The required checks.** Branch protection is a repository setting that only the operator can
change. The operator marks these five checks required on `main`, by these exact names:

- `ci ok (ci)`
- `ci ok (docs)`
- `ci ok (spikes)`
- `ci ok (practices)`
- `crate walls (oml-web)`

The first four are #179's roll-ups. A job skipped by its path filter reports success, so each of
the five reports on every pull request. `m0-spikes.yml` now also runs on `merge_group` like the
other three workflows, and `scripts/tests/test_ci_paths.py` fails any workflow with a final check
that does not. A merge queue can therefore be turned on once the checks are required, and each queued
group then runs the same five checks. Whether to turn it on is the operator's decision; this change
does not.

Where it reads a decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d7 (d) | both tests required on `main` | the tests run in one job, `crate walls (oml-web)`, and that job's name is the required check; the route wall lives in `oml-web`'s tests, beside the dependency wall | the lead's router is built by `oml-transport-http`, which `oml-web`'s tests already start for the parity rig, so the route wall reads `oml_web::PAGES` without a new dependency; one job builds both test targets once, without Postgres |
| d7 (d) | (silent on the old URLs' `303`s) | the test names `GET`/`POST /web/admin` as redirects, and `GET`/`POST /web` keep their `?view=` and moved-act `303`s beside the sign-in page and sign-out, until the operator decides whether v0.7.1 deletes them as Phase (c) as built records. The operator decided to delete them: see [The old URLs deleted](#the-old-urls-deleted-2026-11-11) | deleting a route there deletes its row in the test, in the same change |

## The old URLs deleted (2026-11-11)

The operator decided on 2026-11-11 that v0.7.1 deletes the lead's `303`s from the page URLs and
acts of v0.6, and does not keep them longer: "its not important to keep them" (#283). Phase (c)
as built had kept them through v0.7.0 and left the deletion to this decision.

**Deleted.** `crates/transports/oml-transport-http/src/web/views.rs` (`MOVED_VIEWS`,
`MOVED_ADMIN_VIEWS`, `moved_action`, `moved_memories`, `moved_view`), `GET` and `POST /web/admin`
with their handlers, the admin page's `action` set, and every moved act of the user page's set,
which is now `sign_out` alone. `docs/api/openapi.yaml` loses `/web/admin`, and the generated SDKs
lose `admin_page` and `admin_page_act` with their models.

**What the lead's web routes are now.** Six, all the auth context's (d6): `GET /web` (the sign-in
page without a session), `POST /web` (sign-out), `GET` and `POST /web/sign-in`,
`GET /web/callback/{provider}` and `POST /web/sign-up`. Every proxy copy routes `/web` and
`/web/admin` to the lead already, by falling through to it, so no router changes.

Where it reads a decision differently:

| Decision | Was | Is | Why |
|---|---|---|---|
| d7 (b) | the lead answers an old `?view=` URL, an old act and every `GET /web/admin` with a `303` to the moved page, running nothing (Phase (c) as built) | an old `?view=` URL with a session is `access.not_found`, as a view the page never had was; without one it is the sign-in page, whose `next` is `/web`; an old act of `POST /web` is `request.validation` naming `action`, journalling nothing; `GET` and `POST /web/admin` are unknown paths, `404` before a session is read | the operator's decision; a bookmark or a cached form from v0.6 now ends on a not-found or a refusal instead of the moved page. The redirect target no longer needs to be checked against a smuggled path, and the lead holds no table of `oml-web`'s paths |
| d7 (b) | (Phase (b)'s fifth table) the lead answers a signed-in `GET /web` naming no view with a `303` to the Keys view | kept: `GET /web` is the console's address, where the callback, the sign-in page and sign-out send a person, so a signed-in browser still goes on to `/web/keys` | it is the landing of the auth flow, not an old URL; deleting it would leave a person who has just signed in on a not-found |
| d7 (d) | `the_lead_serves_no_page_route_outside_oml_web_and_auth` names eight routes, two of them `OldUrlRedirect`s (Phase (d) as built) | it names the six above and asserts that `GET` and `POST /web/admin` answer as an unknown path does; `the_old_page_urls_and_acts_answer_not_found_or_refused_and_run_nothing` in `oml-transport-http` and the renamed `the_old_*_urls_are_not_found_*` cases in `oml-web` replace the redirect cases | deleting a route deletes its row in the test, in the same change |
| (none) | the user page's Administration tab links `/web/admin`, which the lead sent to the overview | it links `/web/admin/people`, the overview's own path | the tab otherwise leads to a not-found |

## The measurement of 2026-11-11 (#170)

[#170](https://github.com/e8kor/open-memory-lab/issues/170) measured every page a signed-in admin
loads with a `GET`, served by `oml-web` over loopback at `f3857167` and in process by the lead at
`4dedf532`, the last commit before phase (b), on the same seeded data: 6 workareas and 240
memories (500 iterations), and 48 workareas (200). The harness is `bench/web/run.py`, the numbers
are in `bench/web/results/`, the per-page tables in [Web console overhead](../benchmarks/web.md),
and the findings in `bench/web/README.md`. One shared host, PostgreSQL 18.6, so every figure is an
upper bound. No LAN was measured: the rounds below are the round trips a LAN would add.

**The budget is missed, and v0.7.0 ships with the miss.** 16 of the 21 pages with an in-process
counterpart miss it at 6 workareas, by +6.2 to +42.7 ms p95, and 15 of 21 at 48 workareas, by
+10.1 to +31.2 ms. The operator decided on 2026-11-11 to release v0.7.0 with the miss stated
and to fix it in v0.7.1 under [#286](https://github.com/e8kor/open-memory-lab/issues/286), with
fewer calls per page as d8 requires. The rows on the in-flight cap and the rate share are
readings of the measurement, offered for the operator's acceptance; neither is a settled decision.

| Decision | Was | Is | Why |
|---|---|---|---|
| d8 | 5 ms p95 above the in-process page on loopback, unmeasured | missed on 16 of the 21 pages with an in-process counterpart, by +6.2 to +42.7 ms p95 (memories +6.2, admin memory +9.1), and on 15 of 21 at 48 workareas, by +10.1 to +31.2 ms; within it at 6 workareas, the user Workareas view, the Atlas, both Memories JSON routes and the export. The budget stands; v0.7.0 ships with the miss stated, by the operator's decision of 2026-11-11, and v0.7.1 fixes it (#286) | the miss is the session check: every page but the status poll and the export opens with a round of two `GET /v2/whoami` calls, 7.8 to 17.4 ms p50 each at the lead with 8 to 19 store calls, where the in-process lead read the session once; Review, Curate and Setup, which call nothing else, add +9.5 to +11.4 ms. The hop is not the cause: a page's time outside the lead's handler is at most 4.3 ms p95 (1.0 to 1.5 ms p50 for one round), against 0.5 to 1.0 ms in process |
| d8 | over budget, fewer calls per page through a field on an existing operation | the remedies, in the order of what they buy, none built by #170 and all three taken by [#286](https://github.com/e8kor/open-memory-lab/issues/286) for v0.7.1: a `whoami` that resolves its credential once and reads the person block only when asked (the Start page asks), taking every page's first round toward the in-process read; the text search's read of the person's keys moved beside the recall (+32.0 ms p95, a third round); the offers of a subtree as a field of `GET /v2/workareas/{workarea}/tree` | each is a field or a narrower read on an operation the pages already call; a cache of resolved sessions stays refused |
| d8 | at most two sequential calls | 17 of the 23 pages make two rounds or fewer; the text search, the admin Atlas, its JSON and the admin Review view make three; the user Workareas view 3 + 2N calls and the admin view 3 + 3N over N nodes (N is the workareas on the user view, 15 calls at 6 and 99 at 48, and the nodes on the admin view, one more than the workareas, 24 at 7 and 150 at 49), at most 8 in flight, 4 and 6 rounds at 6 workareas and 14 and 21 to 22 at 48 | over loopback the bounded fan-out made the Workareas views faster than in process (377 ms p50 against 612 at 48), but a LAN adds a round trip per round, not two; the subtree's offers in one call is the fix |
| d3 layer 6, d8 | the per-session cap on calls in flight waits for #170 | a reading for the operator's acceptance, not yet a decision: not built, and not needed. No page but the Workareas views has more than four calls in flight (the Atlas JSON's four after the session check), and those are held to `FAN_OUT` (8), which stays | a page's cost is the lead's handler time, which every call's key rate already bounds; a cap across a session's pages would bound a person loading several Workareas views at once, which nothing measured shows; it is the Workareas fan-out that needs removing, not capping |
| d2 (the 2026-11-11 table) | 60 requests a minute for a session key, chosen, not measured; whether pages need a rate share of their own (#279) | a reading for the operator's acceptance, not yet a decision: no share of their own. Every page but the Workareas views spends at most 4 requests of any one session key per view, most 1 or 2, so the floor allows 15 views a minute; the user Workareas view spends 2N requests of the review key (96 at 48 workareas) and the admin view 2N + 2 of the admin key over N nodes, so one view passes the floor | a rate share would hide a page that does not scale; the subtree's offers in one call bring the Workareas views to a constant cost, and 60 stays |
| d8 | `POOL_IDLE_MAX` (32) and `API_TIMEOUT` (10 s), constants of the build, unmeasured | kept: one page keeps at most 8 connections busy, and the slowest call measured, the tree with counts at 240 memories, took 355 to 406 ms p50 | neither bound was reached; both stay constants of the build |

## The re-measurement of 2026-11-12 (#286)

[#286](https://github.com/e8kor/open-memory-lab/issues/286) built the three remedies of the table
above and ran `bench/web/run.py` again on the same seed, harness and host; the in-process side is
the run of 2026-11-11. `oml-web-6` ran at `b3c7a757`, the one-minute load 1.92 at its start and 2.35
at its end; `oml-web-48` at `cdf88fe0`, which adds the admin Atlas's fallback for an API one release behind and two merges of main (#283, #277, #278, #281, #268), none changing a page's calls, at 1.31 and 2.44. A first 48-workarea run, at `b3c7a757`, ended at a load of
8.67 when another job on the host started, and showed the admin Workareas view at +61.9 ms p95 (532
ms p50): that was the load. Run again on a quiet host the view takes 349 ms p50 and 414 p95, -151.5,
with the same 150 calls and the same store calls per call, each of the fan-out calls 25 to 29% faster at the lead.

**The budget still does not hold on every page.** At 6 workareas 7 of the 21 pages with an
in-process counterpart miss it, by +9.1 to +35.7 ms p95, where 16 did; at 48 workareas 11 of 21,
by +5.1 to +15.0 ms, where 15 did. Six pages differ between the sizes: the admin Workareas
view misses at 6 workareas (+35.7) and not at 48 (-151.5), its in-process counterpart growing with
the workareas faster than its bounded fan-out; the guardrails, Curate and the Start status sit
within 1 ms of the line at both sizes; and the admin Memories list and the admin memory page are
within it at 6 (+1.1, +0.8) and over it at 48 (+8.9, +9.5).

| Decision | Was | Is | Why |
|---|---|---|---|
| d8 | `GET /v2/whoami` resolved a session key twice and read its grants, the person's limits and their deletion request: 8 to 19 store calls, 7.8 to 17.4 ms p50 at the lead | it resolves the credential once, and `person=false`, which `oml-web`'s session check sends, reads no person block: 9 store calls for a `web` session key and 6 for an `admin` one, the session gate's included, against 15 for the `web` key and 8 for the `admin` one; 3.2 to 7.0 ms p50 at the lead (the `start=true` call, which reads the block and the status, 12.9 to 13.8; the Start page's three `whoami` calls 7.9 to 8.0 at their median). The default and `start=true` serve the block as before, so the contract only grows; a key value or a connection now spends one request of its rate where a connection spent two | the person block is read by the Start page alone, which asks for it; `crates/bins/oml-server-lead/tests/web_console_reads.rs` held the 9 and the 6 and, since #297, holds the 2 of the table of 2026-11-12, and that the block is refused to every caller refused before |
| d8 | the text search read the person's keys before the recall: three rounds, +32.0 ms p95 | both in one round: two rounds, +3.8 ms at 6 workareas and -2.3 at 48, within the budget | an author no key of the person's holds is still refused by name, after both reads |
| d8 | the admin Atlas read the scope's offers after the tree: three rounds, +20.4 ms p95 | `GET /v2/workareas/{workarea}/tree?counts=true&offers=true` carries the scope's offers as the offers operation serves them to the same caller, beside the reviewer key's offers for the mode: two rounds and four calls, +9.1 ms at 6 workareas. A failure of that one call now fails the regions with the transitions' offers; the mode's offers still fail alone | the counted tree takes 23.0 ms p50 at the lead on its own, the in-process page's whole time; the miss left is that read and the session check's round |
| d8 | missed on 16 of 21 pages at 6 workareas and 15 at 48 | at 6 workareas missed on 7: the admin Workareas view (+35.7), the admin Atlas (+9.1), the memory page (+9.5), Keys (+11.5), Start (+13.4), People (+10.7) and Users (+11.8); Review, Curate, Setup, the guardrails and the text search are within it. At 48 workareas missed on 11: the guardrails (+5.4), the admin Memories list (+8.9), the admin Atlas (+9.1), the memory page (+10.4), the admin memory page (+9.5), Curate (+5.8), Keys (+12.4), Start (+15.0), the Start status (+5.1), People (+11.3) and Users (+12.3); the admin Workareas view is within it (-151.5), as are Review, Setup and the text search. In both, Review, Setup, the export, the text search, the user Memories list, both Memories JSON routes, the user Atlas and the user Workareas view are within it, and the admin Atlas, the memory page, Keys, Start, People and Users miss it | the session check is now the floor of every page: a round of two `whoami` calls whose session gate still reads the credential, the key and its access on each, where the in-process page read the session once. The five pages of three calls in two rounds pay it twice. The admin Workareas view's miss at 6 workareas is its 3 + 3N fan-out, which #286 did not change: the tree's `offers` serves the scope alone |

Not decided here, and named as what the remaining miss needs: a page whose first read answers who
its session is, so the session check is no round of its own, or a session gate that resolves a
capacity credential in fewer reads; and the offers of every node of a subtree in one call for the
Workareas views.

## The session gate's reads (2026-11-12, #297)

[#297](https://github.com/e8kor/open-memory-lab/issues/297) took the second remedy the
re-measurement above names, a session gate that resolves a capacity credential in fewer reads,
and left the first, a page whose own read is its session check, and the Workareas views' offers
undecided. No operation, field or page call changed: the pages make the calls and rounds they made.
The two rows on the gate are as built, and the operator accepted both on 2026-11-12.

| Decision | Was | Is | Why |
|---|---|---|---|
| d2, d8 | the gate resolved a capacity credential in four store calls, and six for `<user>-web`: the credential's session, the key by its name, the key, its access, and for the web key its root and the person's keys, each a transaction of its own, 22 and 30 statements | one store call, `web_credential_rows`: the same rows in one `REPEATABLE READ, READ ONLY` transaction, the key read by its name in one statement where two read it; 12 and 14 statements counting the transaction's own, by reading the code, held by no test. The store returns rows and decides nothing. The gate makes every check it made, on those rows and in the same order: the session unexpired, and ended when it is past; the capacity a capacity; the admin capacity only for an admin's entry; the key the row holding the capacity's name, a session key serving the session's user, not revoked; its access held by its tenant; a user key's root a user root; the web key's own set the keys naming its root; and the request rate last | one snapshot is what the separate reads return when nothing changes between them, and a change landing between two of them was seen by the later read alone. `the_session_gates_rows_read_in_one_transaction_are_the_rows_its_separate_reads_return` holds Postgres's read equal to the separate reads, which stay every other store's |
| d8 | a session key's verb read the key's access, and a user key's root, again straight after the gate read them | on a `GET` or `HEAD` request the request's first resolution of the caller decides on the access the gate read; one such request journals, the admin read of one memory (`memory.admin_read`). Every later resolution in that request, and every resolution of a request of any other method, reads the access when it runs. Which acts resolve is unchanged: creating a workarea, a transition, a ratification, a mode change and the two allowlist acts resolve at the verb and again after the wait on their idempotency claim; granting capacities, a deletion request, a quota, disabling a person and closing a deletion request resolve at the verb and not after the wait; four resolve nothing and act as the caller the gate built: `POST /v2/keys` as `<user>-web`, `POST /v2/keys/{key}/denials` and `POST /v2/keys/{key}/connect-codes` as a session key, and `POST /v2/web/session/end`. No verb of those four consults the key's grants or denials, so a `<user>-web` key denied on `universe` still issues a key (#325; fixed in v0.7.3 by the proposed amendment below, A denied session key's acts). | the two reads were of the same rows in one request with only the handler's dispatch between them. A body can arrive slowly between the gate and the verb, so a request of another method keeps whatever read its verb makes. Nothing outlives the request: a cache of resolved sessions stays refused |
| d8 | `whoami` without the person block read 9 store calls for a `web` session key and 6 for an `admin` one | 2 each: the gate's one and the record's references. A page's own `GET` spends the gate's one where the gate and the verb's read spent 5 (8 for the web key); a `POST` spends the gate's one and its verb's read where the gate spent 4 (6) | `crates/bins/oml-server-lead/tests/web_console_reads.rs` holds the 2 |
| d8 | missed on 7 of 21 pages at 6 workareas and 11 at 48 | measured at each size with the change, at `9a2f4bd2`, and without it, the lead built at `e80efde6`, which no source the lead links separates from `47ef9ffa`, by `bench/web/run.py` on 2026-11-12. `9a2f4bd2` is not this change's head: the shared walker (#308) and later merges are not in the measured build. Other jobs shared the host, so a run started only below a one-minute load of 2.0, by hand (the harness records the load before and after and enforces nothing), and the load was 1.6 to 2.2 at each run's start and end. The 48-workarea runs stayed under 3 throughout; the 6-workarea runs passed it in 23 of 151 samples with the change (highest 3.64) and in 5 of 170 without it (3.23). At 6 workareas main misses on 8 of 21 pages and the change on 1: the admin Workareas view, by +6.2 ms p95 where it missed by +27.9. At 48 workareas main misses on 10 and the change on 1: the admin Memories list, by +5.8 where it missed by +10.5; at 6 workareas that page is within the budget (-3.9). The six pages the re-measurement named at both sizes are within the budget at both: the admin Atlas (+0.6, +1.5), the memory page (-0.9, -0.2), Keys (+1.2, +1.9), Start (+3.5, +3.9), People (+2.3, +2.7) and Users (+3.8, +4.8) | the session check's `whoami` takes 1.5 to 3.3 ms p50 at the lead where it took 3.3 to 7.0, and a page's own call runs 3 to 7 store calls fewer. The admin Workareas view's miss is its 3 + 3N fan-out, 24 calls in 6 to 7 rounds at 6 workareas. The admin Memories list is one search of 119 ms p50 at the lead, and at 48 workareas it is 0.8 ms over the line; Users at 48 is 0.2 ms inside it, so one more run could move either across. Four more 6-workarea runs with the change, three at loads that reached 5.7 to 9.8 and one at 2.35 falling to 1.73, each put the admin Workareas view over the budget (+7.1 to +18.9) and no other page but the admin Memories list in the run at 9.8. The committed results and [Web console overhead](../benchmarks/web.md) are the runs with the change; each run's load and the findings are in `bench/web/README.md` |

Not decided here: the offers of every node of a subtree in one call. The admin Workareas view
still makes 3 + 3N calls, each now 4 store calls lighter; at 6 workareas it misses by +6.2 ms p95
(96.3 against 90.0 in process), and at 48 it is within the budget (-226.6).

## A denied session key's acts (2026-11-12, #325)

**Proposed amendment, awaiting the operator's acceptance.** The operator decided on 2026-11-12
that [#325](https://github.com/e8kor/open-memory-lab/issues/325) is fixed in v0.7.3; the two rows
below are how it is built, and neither is accepted yet.

The audit of this record found four acts of a session key that acted as the caller the gate built
and consulted none of the key's grants or denials (the table of #297 above). A `<user>-web` key an
admin had denied on `universe` therefore still issued a key, and the key it issued held writer and
reader on the person's whole root with no denial: a cut-off session minted a credential that was
not cut off. Three of the four now refuse a denied key, and the fourth, the end of the session,
stays open to it. No operation, field or page call changed, and no key does more than it did.

| Decision | Was | Is | Why |
|---|---|---|---|
| d2 | `POST /v2/keys` as `<user>-web`, `POST /v2/keys/{key}/connect-codes` and `POST /v2/keys/{key}/denials` as a session key, and `POST /v2/web/session/end` acted on the key record alone and consulted none of its grants or denials | the three key acts refuse a session key that carries a denial, on any workarea: `access.verb_ceiling`, `details.reason: session_key_denied`, `details.verb` naming the act, nothing journalled and the act's idempotency claim released; the Keys page and the Start page show that refusal by its code and message under its status, as layer 7 shows any other. `POST /v2/web/session/end` still consults nothing of the key, and neither does the lead's sign-out, so a denied key's session ends when the person signs out. An admin key takes no denial, so of the session keys only `<user>-web` meets the refusal while the rows are written by the verbs | a key `<user>-web` issues is granted the person's whole root, so an issuer denied on `universe`, on the root or on any workarea inside it would hand on access it no longer holds; a connect code opens a connection for a key of the same root; and a cut-off is an act journalled under a key the admin stopped. Any denial refuses, not only one on `universe`, and the refusal is permanent: no route lifts a denial from a key that is not an admin's, and a sign-in finds the session key by its name and issues no other, so after the person's entry is removed, added back and signed in again the key is the same `<user>-web`, still refused. That is the rule's cost, and it falls on a denial that masks nothing: a user key's grants lie in its root, but for the one reader grant issuance adds on the `universe` node alone ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md) d3), so a denial on a workarea outside the root changes nothing the key reads or writes and still ends the person's three key acts in the console for good. An admin key still issues a key naming the person's user, mints a connect code and cuts a key off for them. The operator decided on 2026-11-12 to keep any denial, strict and permanent. Rejected: a narrower rule, refusing only some denials, since one that passes a denial on part of the root lets the denied key issue a fresh key that reads that part. Refusing the sign-out would keep a session an admin wanted stopped. The refusal says that the key is denied and never where: `GET /v2/keys` already serves `cut_off` on the person's own session key for a denial on `universe`, and no answer names a denied workarea |
| d8 | those four acts resolved nothing | the three key acts resolve at the verb, after the wait on their idempotency claim and again on each retry of a chain conflict: one resolution, which is the key's access and, for `<user>-web`, its root, two store calls, and one for `<user>-admin`, by reading the code, held by no test. `POST /v2/web/session/end` resolves nothing. Every one of them is a `POST`: no `GET` or `HEAD` gained a read, so the gate's one store call and the pages the benchmark measures are as #297 left them | the gate reads the access before the body arrives, so an act that mints a credential decides on a read made when it runs, as every other act of another method does |

Tests, each over the lead's router: `a_cut_off_session_key_issues_no_key`, which also denies the
key on the person's root and on a workarea inside it, `a_cut_off_session_key_mints_no_connect_code`,
`a_cut_off_session_key_cuts_off_no_key`, `a_cut_off_session_key_still_ends_its_session` and
`a_denied_session_key_is_still_refused_after_its_entry_is_removed_added_back_and_signed_in_again`,
which denies the key on a workarea outside the person's root, in
`crates/transports/oml-transport-http/tests/web_pages.rs`; and through `oml-web`,
`a_cut_off_persons_key_acts_are_refused_on_the_page_as_the_api_refuses_them`, which holds each
page's status, code and message equal to the API's.

Not decided here: a denied `<user>-web` still asks for the person's deletion
(`POST /v2/web/deletion-requests`) and reads their export (`GET /v2/web/export`), by reading the
code; #325 names neither.

## Sign-out on every page (2026-11-12, #327)

**Proposed amendment, awaiting the operator's acceptance.** The operator decided on 2026-11-12
that [#327](https://github.com/e8kor/open-memory-lab/issues/327) adds sign-out in v0.7.3; the
three rows below are how it is built, and none is accepted yet.

The audit of this record found that no page drew a sign-out control: `oml_page::web::shell` had
none, so a session ended only by expiry, an entry change or a new sign-in. The header of every
page `shell` draws for a signed-in person now carries one. No operation, route or setting changed:
the control is a form, and the act it posts existed.

| Decision | Was | Is | Why |
|---|---|---|---|
| d2 | no page of the console draws a sign-out control | `oml_page::web::shell` takes the session's CSRF token and draws, after who is signed in, a form with `method="post"` and `action="/web"` holding `action=sign_out`, `csrf_token` and one button, Sign out; every page `oml-web` serves passes its session's token. `oml_page::web::refusal_page` takes the same token and draws the same form in a header that otherwise holds the brand alone. `oml-web` passes it on every refusal page it answers once the request carried a session cookie: a page not found, a query or a form refused, an API refusal shown by its code, the refusal of a cut-off key's act, and the page of a session check the API did not answer or refused by its rate, where the token is the cookie's and the lead's act decides whether a session stands behind it. The lead passes it on its own refusal page once the cookie named a live session: a signed-in `GET /web?view=…`, and a `POST /web` refused after the session is read, a form without the session's CSRF token included. That last page is drawn with the control because the cookie names the session, the lead refused another origin's form before it read the session, the token on the control is the session's and not the page's, and the answer is readable by the page's own origin alone. No control: the sign-in page, the sign-up terms page and its refusals, and a refusal the lead answers before it reads the session (an `Authorization` header, a form from another origin, a body that is not a form, a provider's callback), each drawn for no one; and the answers that are not pages: `oml-web`'s envelope for an `Authorization` header, for a form from another origin and for a form without the session's CSRF token (layers 2 and 4, which run before the session load), its data and frame routes, and a path under `/web` that neither process serves a page at. Below 600px the header drops who and keeps the control. The lead's act ends the session row, so every capacity credential derived from it is `auth.invalid` from the next request; it clears `__Host-oml_session` with the attributes the sign-in set (`Path=/; Secure; HttpOnly; SameSite=Lax`, `Max-Age=0`) and answers a `303` to `/web`, which is the sign-in page for no one | a person on a shared machine had no way to end their session |
| d5 | #5, `POST /v2/web/session/end`, serves no page, and no page draws the sign-out form | the control posts to the lead's `POST /web`; #5 still serves no page | the lead set the cookie and is the one process that can clear it under the same attributes without a second copy of them; opening and ending a session is the auth context's, which `oml-web` never does (d6); and a route in `oml-web` would add a path to the proxy's, the compose router's and the Ingress's lists, where a missing entry strands the control. The cost: #5 has no caller in the console; whether it stays served is not decided here |
| d3 layer 4 | every `POST` passes `oml-web`'s same-origin and CSRF check | the sign-out post never reaches `oml-web`: the proxy sends `POST /web` to the lead, whose act makes the same checks itself, the same-origin rule of `oml-page` and the session's `csrf_token` compared in constant time, and refuses a form that carries any other field. It is a `POST` alone: a `GET` naming `sign_out` ends nothing | one rule in `oml-page` for both processes (#230); a link would let any page a person opens sign them out |

Tests: `the_header_draws_the_sign_out_control_only_for_a_signed_in_session` in `oml-page`; in
`crates/clients/oml-web/tests/web_pages.rs`, over the lead and `oml-web` behind the path router,
`every_signed_in_page_draws_the_sign_out_control_in_its_header` (the 18 page routes of
`oml_web::PAGES` and a page an act answers),
`every_refusal_page_of_a_signed_in_person_draws_the_sign_out_control_in_its_header` (a `404` from
each process, a `400` from each, the lead's `400` for a form without the token, a `409`, the `403`
of a cut-off key's act, and the `404` a cut-off person gets on Guardrails, Workareas, Memories and
the Atlas; none on the lead's refusal of another origin's form or of an unknown provider, and no
page for `oml-web`'s refusal of a form without the token),
`the_sign_out_control_ends_the_session_and_no_credential_of_it_works_afterwards`,
`signing_out_clears_the_cookie_as_it_was_set_and_lands_on_the_sign_in_page`,
`sign_out_is_a_same_origin_post_with_the_sessions_csrf_token_and_never_a_get` and
`a_cut_off_person_signs_out_from_the_page`; and over the lead's router alone,
`sign_out_ends_the_session_and_clears_the_cookie` in
`crates/transports/oml-transport-http/tests/web_pages.rs`. The header was looked at in Chromium
as a static render of `shell` under the stylesheet, at 1100px and 390px in both colour schemes;
the running console was not.

## Consequences

- `oml-web`, `oml-page` and `oml-sdk` join the workspace; `oml-transport-http` loses its page
  code and keeps the API, the auth context and the traffic layer.
- [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d6's "a session key has no value, so it cannot be presented on `/v1`, `/v2` or `/mcp`"
  becomes: it is presented on `/v1` and `/v2` only by its per-session credential, only from
  `oml-web`'s client identity (the secret or mutual TLS), and never on `/mcp`. d7's tests stay and gain the two of phase (a).
  d18 gains a row: `auth.invalid` `session_outside_web`, with the doctor line `warn web: no web client identity set; oml-web cannot reach the API (set web.client_secret on the lead and on oml-web)`, printed only when a sign-in provider is configured and `/readyz` names `web_client: none`, and `oml-web` refuses to start without its own half, naming the setting.
- Two processes ship and run where one did: the release adds the `oml-web` binary and the
  landing archive (d12), the Helm chart a second Deployment, the compose file two more services, `oml-web` and the Caddy router, and `oml quickstart` starts both. `oml-web --doctor` gains an `api` line that fails when `web.api_url` is unreachable or the lead is not ready; `oml doctor` gains only the lead's `warn web` line.
- The `http_request` line of the API now records `credential: session` for the inner calls,
  correlated to the page line (ADR-0042 d2, d6). Log volume per page view grows with its calls.
- `docs/api/http-api.md`, `openapi.yaml`, `errors.md` and the SDKs gain the ten operations and
  six changes in phase (a); the CLI may use them (`oml keys list`, `oml review queue`) in a
  change of its own.
- [#170](https://github.com/e8kor/open-memory-lab/issues/170) measured the web hop on loopback (the table of 2026-11-11): the
  hop itself fits the 5 ms p95 budget, but the session check's `whoami` round does not, so 16 of 21 pages at 6 workareas miss it by +6.2 to +42.7 ms p95. v0.7.0 ships with the miss stated, by the operator's
  decision of 2026-11-11, and v0.7.1 fixes it with fewer calls per page
  ([#286](https://github.com/e8kor/open-memory-lab/issues/286)). That the per-session in-flight
  cap of d3 and a page rate share are not needed is a reading awaiting the operator's acceptance.
- [#286](https://github.com/e8kor/open-memory-lab/issues/286) built the three remedies (the table
  of 2026-11-12): `whoami` takes `person=false` and resolves once, the counted tree takes
  `offers=true`, and the text search reads the keys beside the recall. The miss falls from 16 to
  7 of 21 pages at 6 workareas and from 15 to 11 at 48; the session check's round and the Workareas fan-out remain.
- #297 reads the session gate's rows in one store call (the table of 2026-11-12): the miss falls to 1 of 21 pages at each size, the admin Workareas view at 6 workareas (+6.2 ms p95) and the admin Memories list at 48 (+5.8); the Workareas fan-out remains.
- #163, #166 and #167 land first, on the current design; this change then moves what they
  built, so none of them is rebased onto an unbuilt crate.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Same process, same use cases, under a rule** | Today's shape with a lint that pages call only "API-shaped" use cases. A rule is not a wall: the pages keep a path past the contract that no test can close, and each page feature can still add a `web_*` method no other client has. |
| **The person's key value, encrypted, held server-side** | A bearer value per person stored under a key-encryption key outlives the session, needs revocation (not built) when the entry changes, and puts a decryptable credential at rest where today there is none. The derived credential of d2 dies with the session and exists at rest only as digests. |
| **The session cookie presented to the API as is, with a capacity header** | The session token would be one credential for every capacity, and the acting key would be named by a header beside it, which d6 of ADR-0045 forbids in spirit: identity read off a field rather than off the credential. |
| **A browser SPA over the API** | Puts a script and a token in the browser, against ADR-0045 d7 and d8 (no script, nothing an agent can lift), and gives every page a machine interface. |
| **A GraphQL or backend-for-frontend aggregate API** | A second contract shaped for one client, which the CLI, the SDKs and the agents do not get; it is the same-use-cases shape moved one layer out. Fewer calls per page are bought with a field on an existing operation (d8). |
| **A loopback TCP hop in-process** | The operator's first wording. A listener inside the lead's process adds a socket without separating anything that runs: a crash, a restart or an upgrade still takes both. Superseded by d4's separate process. |
| **In-process by default** (this record's d4 as accepted on 2026-11-07) | The lead mounts `oml-web` and calls the API through a `oneshot` on its own router. Cheapest per call, but the wall is a crate graph inside one address space: the lead links the pages, one deploy and one restart carry both, and the per-boot secret lives in memory both sides share. Rejected 2026-11-10 (d11). |
| **Dual mode: in-process by default, standalone as an option** | Two transports, two compositions and two parity runs to keep equal, and the shape most deployments would run is the one that proves the boundary least. Rejected 2026-11-10 (d11). |
| **A generated Rust SDK** | No pinned Rust generator reads the 3.1 document into useful types today, and a generated layer full of untyped objects would push field names back into page code (d9). Reopen when the schemas are typed and a generator is pinned. |
