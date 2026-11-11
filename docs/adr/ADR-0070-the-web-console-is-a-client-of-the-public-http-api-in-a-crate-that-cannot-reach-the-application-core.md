<!--
id: adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core
tier: engineering
status: accepted design · 2026-11-07, amended 2026-11-10 and 2026-11-11
answers: why the web pages stop calling oml-app's use cases and call the public HTTP API instead; which crate serves the pages and what it may depend on; how a signed-in person's session becomes an API credential without a key value at rest or in the browser; which middleware layer owns the request id, the session, CSRF, CSP, rate limits, audit and error mapping; how oml-web, always its own process, reaches the API; which existing operation serves each web call and which operations are new; what stays in the auth context; the bounded contexts; the migration phases and the test that proves parity; the overhead budget; what it amends in ADR-0042 and ADR-0045
-->

# ADR-0070 — The web console is a client of the public HTTP API, in a crate that cannot reach the application core

**Status:** accepted design · 2026-11-07, amended 2026-11-10 and 2026-11-11

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
each moved page gets its own path under `/web/`, and the lead answers the old `?view=` form with
a `303` to it; see the second table of Phase (b) as built. Amended by the third change of phase
(b), the Workareas view and the first acts through `oml-web` (d3, d5 and d8 revised): see the
third table of Phase (b) as built. Amended by the fourth change of phase (b), the Memories list, the
Atlas and the creation of a workarea through `oml-web` (d3, d5, d7 and d8 revised): see the fourth
table of Phase (b) as built. Amended by the fifth change of phase (b), the Review and Keys views and
their acts through `oml-web` (d3, d5, d7 (b) and d8 revised): see the fifth table of Phase (b) as
built. Amended by the sixth change of phase (b), one memory opened by its id, the admin's Curate
view and the Mermaid frame through `oml-web` (d1, d3, d4, d5 and d7 (b) revised): see the sixth
table of Phase (b) as built.

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

Decided, not built. The build follows #163, #166 and #167.

## Decision

- **d1 — `oml-web` is its own crate and its own binary, depends on the Rust SDK, axum, tower and
  one shared page crate, and a test fails when its dependency graph reaches the application
  core.** The crate lives at `crates/clients/oml-web`, builds the `oml-web` binary, the one
  process that serves the pages (d4), and holds the pages, their forms, their templates,
  the Mermaid frame route of ADR-0056 d9 (`web/render.rs`) and the middleware of d3. Its normal dependencies are `oml-sdk` (d9), `axum`, `tower`,
  `http`, `serde`, `serde_json` and `oml-page`, a new crate with no I/O that holds what the
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
  HTTP API stays in `oml-transport-http`; the allowlist and admin-read handlers move out of its
  `web/` directory into the lab ring's modules, since they are API, not pages. The cost is one
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
  then re-checks on every request what `web_caller` checks today: the session is live and
  unexpired, its entry still names the user, and for `admin` the entry is still an admin's
  (and, with #167, not disabled). The caller is then that session key, with credential
  `session`, under its own grants and verb ceiling; every act lands in the journal under it,
  as today. The credential lives exactly as long as the session and ends with it (sign-out,
  expiry, an entry change), so no revocation is needed; nothing at rest can be decrypted into
  it. Two gates keep [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
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
  still reaches no API with it, and an agent's bearer key still reaches no page. Which
  capacity an action uses is `oml-web`'s compiled table (`issue_key` → `web`, `verdict` →
  `review`, `curate` → `curate`, `allow_email` → `admin`, and so on), never a form field.
  Admin acts use the `admin` capacity under the same rule: `<user>-admin` is an admin key,
  holds no dreamer grant, and is refused every review verb by the API whatever the table says
  (R1, [ADR-0001](ADR-0001-the-constitution.md)). The cost is stated: a defect in the table
  can make an act run as another of the same person's capacities, never as anyone else, and
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
  session_capacity`), so the admin capacity mints no plain key. The admin capacity keeps the
  admin key role's other verbs, under the API's rules, as any admin key does.

- **d3 — Seven layers wrap every page request, outermost first, each concern has one owner,
  and the stack lives in `oml-web`; an external gateway in front is optional.** The layers are
  `tower` layers of the `oml-web` binary, so a deployment with nothing in front of it (local
  development, d4) has all of them. A gateway the operator puts in front (Caddy, an ingress, an
  API gateway) may add TLS, its own rate limit, a web application firewall or access logs; it
  replaces none of the layers below and `oml-web` never trusts a header the gateway sets for
  identity, the session or CSRF. The one setting a gateway touches is `server.trusted_proxies`
  of [ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md),
  read the same way by `oml-web` for the client address on its line.

    | # | Layer | Owner | What it does, and why it sits there |
    |---|---|---|---|
    | 1 | Request id and traffic log | `oml-web`, the [ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) layer reused from `oml-page`'s line type | Honours or mints `X-Request-Id`, writes the page's `http_request` line with `credential: session`, the acting key, `web_action` and the route template. Each inner API call carries `X-Correlation-Id` set to the page's request id and `X-Request-Id` `<page id>.<n>`, so one `jq` filter finds the page line and its API lines. Outermost, so a refusal by any inner layer is still logged. |
    | 2 | Human-only gate | `oml-web` | An `Authorization` header on a page route answers `access.verb_ceiling` `web_is_human_only` before anything is read (ADR-0045 d7). |
    | 3 | Session load | `oml-web` reads the cookie; the API decides | Reads `__Host-oml_session` and derives the `web` credential; the first API call of the page, `GET /v2/whoami`, is the check. No session, or `auth.invalid` from the API, sends a `303` to the lead's sign-in page (d4, d6). `oml-web` never judges a session itself. |
    | 4 | Same-origin and CSRF | `oml-web` | Every `POST` passes the existing `same_origin` check against the public origin and carries `csrf_token` = SHA-256 over the fixed purpose and the session token, compared in constant time (ADR-0045 d8, unchanged). Every form also carries a random nonce, and the `Idempotency-Key` of the act's API call is the hex SHA-256 over the session token and that nonce, so a resubmitted form replays instead of repeating. The API never sees a form, so CSRF belongs to the layer that renders forms. |
    | 5 | Response headers and CSP | `oml-web` | `default-src 'none'`, `style-src` naming the one stylesheet's digest, `script-src` naming only the hashes of the scripts that page carries (none today; #167's copy script on `/web/start`), `form-action 'self'`, `frame-ancestors 'none'`, `base-uri 'none'`, `no-store`, `nosniff`, `Referrer-Policy: same-origin`; the frame route keeps its own policy. Digests computed at compile time from `oml-page`. |
    | 6 | Rate limit | `oml-web` for fan-out, the API for acts | A page view becomes several API calls, so `oml-web` caps the requests one session has in flight and answers `limit.rate` with `Retry-After` past it; the cap is a constant of the build, unmeasured ([#170](https://github.com/e8kor/open-memory-lab/issues/170)). Every act stays under its key's limits at the API, as for any client ([FR-8](../product/roadmap.md)). Provider-call caps stay with sign-in in the auth context (ADR-0045 d15). |
    | 7 | Error mapping | `oml-web` | The API's envelope (`code`, `details`) becomes a page outcome: `access.not_found` the not-found page, `auth.invalid` the sign-in redirect, `request.validation` the form again with `details.field` marked, `limit.*` the retry page, `review.*` and `workarea.*` the message the page shows today. A code the table does not name renders the generic refusal page with the request id. |

    **Audit is two records with two owners.** The page-level audit is `oml-web`'s line
    (layer 1): who acted, as which capacity, which `web_action`, and the request ids of the API
    calls it made. The record of what happened is the API's journal: every act and every admin
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
  other host `oml-web` refuses to start with `web.api_url: plain http to a non-loopback host sends
  session credentials in clear; use https://, or set web.allow_plain_http_api = true for a private
  network`; with `web.allow_plain_http_api = true` it starts and logs one warning naming the host
  at every start. `web.api_url` carries no userinfo, query or fragment. Across a LAN the hop is
  `https://` or mutual TLS (d2); `oml quickstart`'s compose network reaches the lead at a service
  name, so it sets the opt-in explicitly. The doctor line lives in `oml-web`, whose settings the
  lead's `oml doctor` cannot read: `oml-web --doctor` prints `web.api_url` as `ok`, `warn` (plain
  http allowed by the opt-in) or `fail` (plain http refused), and `oml doctor` run with `oml-web`'s
  environment gives the same line. Test:
  `a_plain_http_api_url_to_a_non_loopback_host_is_refused_unless_allowed`.

    | Shape | Browser reaches | `oml-web` reaches the API at |
    |---|---|---|
    | Production | Caddy (or another proxy) on the public origin: `/web` and `/web/*` to `oml-web`, except the auth routes of d6 (`/web/sign-in`, `/web/callback/*`, `/web/sign-up` and the sign-up terms page); those, `/v1/*`, `/v2/*`, `/mcp`, `/oauth/*` and `/.well-known/*` to the lead; the landing paths to static files (d12) | the lead's listener on loopback or the LAN, not back through the public proxy |
    | Local development | three services started by `oml quickstart` (its compose file gains `oml-web` and the router): the lead, `oml-web`, and a path router (Caddy, as a third compose service) in front of both, routing exactly as production does, so the browser sees one origin, `web.public_url` keeps its default and there is no `web.page_url` setting | the lead's address on the host or on the compose network |
    | Kubernetes | the chart's Ingress routes as production does, to two Deployments: each path in `web.ingressPaths` (`Exact`) to `oml-web`, every other path to the lead | the lead's Service |

    Routing is by path alone in every shape (amended 2026-11-11): each moved page has its own
    path under `/web/`, `oml_web::PAGES` lists them, and the proxy, the compose router and the
    Ingress each name exactly those paths. No shape routes on a query.

    `__Host-` cookies bind one host, so production puts both behind the same public origin.
    A person with no live session who opens a page is sent with a `303` to the lead's
    `/web/sign-in`, carrying the page as `next`; the redirect is relative, which works because
    every shape has one public origin. `GET /web/sign-in` is a new route: sign-in rendered at
    `GET /web` until now, and the lead's route table moves with it (phase d). The lead accepts
    `next` only as a same-origin relative path: it starts with `/web`, holds no `//`, no `\`, no
    `@` and no scheme, is percent-decoded once and checked again, and anything else is dropped to
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
    | `web_path`, `workarea_status`, the Atlas breadcrumb (main, #163) | `GET /v2/workareas/{workarea}` | web or admin |
    | `set_workarea_mode`, commit of an in-progress root (main, #163) | `POST /v2/workareas/{workarea}/mode` | review |
    | `propose_transition`, `ratify_transition` (#163) | `POST /v2/workareas/{workarea}/transitions`, `…/{transition_id}/ratify` | review |
    | `web_issue_key` for `issue_key`, `connect_key`, #167's `issue_claude_*` (main, #167) | `POST /v2/keys` | web |
    | `web_connect_code` (main) | `POST /v2/keys/{key}/connect-codes` | web |
    | `web_cut_off` (main, both pages) | `POST /v2/keys/{key}/denials` | web or admin |
    | `web_all_entries` (main) | `GET /v2/web/emails` | admin |
    | `allow_web_email` for `allow_email`, `set_admin` (main) | `POST /v2/web/emails` | admin |
    | `remove_web_email` for `remove_email` (main) | `POST /v2/web/emails/remove` | admin |
    | `admin_read` (main) | `GET /v2/admin/memories/{artifact_id}` | admin |
    | `admin_search` (main) | `POST /v2/admin/memories/search` | admin |
    | `web_users`, `web_deletions`, `signup_overview` (#167) | `GET /v2/web/users` (served by #167) | admin |
    | `web_start_status`, `web_has_agent_key` (#167) | `GET /v2/keys` (new, below) and `GET /v1/memories?with_own=true&limit=1` | web |

    **Changed operations: a parameter or a field joins an existing operation.**

    | Operation | Change | Serves (source) | Traces to | Admin |
    |---|---|---|---|---|
    | `GET /v1/memories` | `with_own=true`: the scoped listing and the caller's own rows merged newest first, a row in both once, one cursor; `author`: one key naming the caller's user, by name or id, which drops the scoped part | `web_memories` (#163) | ADR-0045 d22, d23 | no |
    | `GET /v2/workareas/{workarea}/tree` | `counts=true`: the whole subtree in one answer, each node carrying `{promoted, held, in_progress, other}` as the caller's `with_own` listing would show them, the counts the Atlas draws, and `truncated` past `REGION_COUNT_MAX`; for the admin role every row counted as structure, no content | `web_regions`, `admin_regions` (#163) | ADR-0045 d30 | for the admin reading |
    | `POST /v2/admin/memories/search` | `origin_kind` and `author` filters | `admin_search_filtered` (#163) | ADR-0045 d22, d24 | yes |
    | `GET /v2/review/held` | `ids`: the named held rows only, each with its row or `null` when no longer held or no longer the caller's to judge | `review_named`, the bulk confirmation (main) | ADR-0055 d10 | no |
    | `POST /v2/web/emails`, `POST /v2/web/emails/remove` | `keep_an_admin: true` refuses removing or un-flagging the tenant's last admin entry, `request.validation` `details.field: admin`, in the same transaction | `web_other_admin` (main) | ADR-0045 d10 | yes |
    | `GET /v2/whoami` | `person`: for a key naming a user, `{user, under_quotas, usage, quotas, deletion_requested_at}` | `web_own_limits`, `web_deletion_pending` (#167) | ADR-0068 d5, d6, d10 | no |

    **New operations.**

    | # | Method and path | Request | Response | Serves (source) | Traces to | Who |
    |---|---|---|---|---|---|---|
    | 1 | `GET /v2/keys` | query `user?`, `limit?`, `cursor?` | `{items: [{id, name, role, held_by, credential, user_root_id, revoked, cut_off, grants: [{role, workarea_id, path}]}], next_cursor}` | `web_own_keys`, `web_keys_of_user` (main), `web_has_agent_key` (#167) | ADR-0045 d9, d13; the designed `listKeys` | a session user key, its own user's keys only; the admin role, one `user`'s keys, `user` required |
    | 2 | `GET /v2/review/queue` | none | `{items: [{workarea: {id, name}, path, held, oldest_at}]}`, no content | `web_queue` (main) | ADR-0045 d10, the designed `reviewQueue` | admin |
    | 3 | `POST /v2/web/users/{user}/capacities` | `{capacity: review \| curate, workareas: [NameRef]}` | `{key: {id, name}, grants_added: [...], journal_seq}` | `web_grant_review`, `web_grant_curation` (main) | ADR-0045 d10, ADR-0044 d7 | admin |
    | 4 | `GET /v2/workareas/{workarea}/offers` | none | `{workarea_id, status, mode, transitions: [{op, to, transition_id?, from?}], mode_change: {to} \| null, commit: {offered, held}}`, each offered only where the verb's own check admits the caller now; an unreadable workarea `access.not_found` | `workarea_offers`, `workarea_offer_check` (#163) | ADR-0045 d28 | no |
    | 5 | `POST /v2/web/session/end` | none | `204` | `web_sign_out` (main), the `sign_out` action | ADR-0045 d5 | an `omls_` credential only; it ends the session it derives from |
    | 6 | `GET /v2/web/export` | none | `application/json` attachment of everything the person holds | `web_export` (#167) | ADR-0068 d6 | session user key only |
    | 7 | `POST /v2/web/deletion-requests` | none | `{requested_at, journal_seq}` | `web_request_deletion` (#167) | ADR-0068 d6 | session user key only |
    | 8 | `POST /v2/web/users/{user}/quota` | `{memories?, keys?, connections?}` | the user's row as `GET /v2/web/users` serves it | `web_set_quota` (#167) | ADR-0068 d5 | admin |
    | 9 | `POST /v2/web/users/{user}/disable` | none | `{keys_cut_off, journal_seq}` | `web_disable_user` (#167) | ADR-0068 d8 | admin |
    | 10 | `POST /v2/web/users/{user}/deletion/close` | `{outcome}` | `{journal_seq}` | `web_close_deletion` (#167) | ADR-0068 d6 | admin |

    Every state-creating `POST` takes `Idempotency-Key`, and every refusal is the envelope of
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
    or `<user>-admin` is `web_cut_off`, on `universe` alone. Six of the ten need the admin key role; #1 needs it for any
    user but the caller's own. #3 is one operation for two acts because both are one job: an
    admin gives a person's session key grants, creating `<user>-curate` at the first
    designation; it creates a session key only by this path, so `POST /v2/keys` never mints
    one. #6 and #7 refuse a bearer user key, as d16 of ADR-0045 refuses it key issuance, so an
    agent cannot export or delete its person. `POST /v2/keys/{key}/grants`, designed, is not
    needed by any page and stays designed.

    **Calls that stay in the auth context and become no operation.** `web_sign_in_start`,
    `web_admit`, `web_ensure_keys`, `web_session_open`, `web_session` (now the API's
    resolution of `omls_`), `web_caller` (the same), `web_sign_up` and `check_signup_policy`
    (#167), `oauth_issuer`.

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
  d2 and the provider exchanges. It stays in `oml-transport-http`, in one `auth` module that
  gathers today's `oauth/` and the sign-in half of `web/`. The auth route's renderer
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
    | Auth | sign-in, the OAuth authorization server, sessions and their credentials, the browser-facing identity pages | `oml-transport-http::auth`, `oml-app` auth use cases |
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
    | (b) | `oml-sdk` (d9), whose one consumer is `oml-web`; `oml-page`, the `oml-web` binary with the layers of d3; the proxy routes of d4 send each moved page's path to it; pages move one at a time, each moved page gets its own path under `/web/` and the lead answers the old `?view=` form with a `303` to that fixed path, the old handler deleted in the commit that moves its page (amended 2026-11-11) | the page suites, unchanged (below), green after each page moves; `oml_web_reaches_no_application_crate` |
    | (c) | the `web_*` use cases deleted: the logic behind a new operation is renamed to that operation's use case, the sign-in ones move to `oml-app`'s auth module, the rest go | `grep -rn 'pub async fn web_' crates/oml-app --include='*.rs' | grep -v 'crates/oml-app/src/app/auth.rs'` is empty; the workspace builds and every suite passes |
    | (d) | the walls: the dependency test is a required CI check, and `the_lead_serves_no_page_route_outside_oml_web_and_auth` asserts the route table | both tests required on `main` |

    **Parity** is `crates/transports/oml-transport-http/tests/web_pages.rs` (51 tests on
    `main` before the `web_pages.rs` additions in this change), #167's `open_signup_pg.rs` page cases and the Chromium
    scripts (`tests/chromium/atlas.py`, `render_frame_chromium.rs`), moved to
    `crates/clients/oml-web/tests/` with their assertions untouched. Only the rig changes: it
    starts the lead on a loopback listener and `oml-web` with `web.api_url` set to it, as two
    servers in the test process, and seeds through the app as today (a dev-dependency). The
    suite runs once, over that loopback hop, which is the one transport there is (d4). A test that has to change its assertions is a behaviour change, named in the
    change that makes it.

- **d8 — The budget is p95 page time no more than 5 ms above today's with the API on loopback,
  and 5 ms plus two round trips on a LAN; a page makes at most two sequential calls, over pooled
  keep-alive connections, each under a timeout, and a page whose secondary call fails renders
  without that part.** A page view becomes one to four API calls (the Memories view: `whoami`,
  the listing, the tree with counts, the offers), and each adds serialization both ways, a route
  match, an `omls_` digest lookup (an indexed read; today one `web_caller` per request already
  does it once), on a `POST` an idempotency row, and now a network hop. Four rules hold the hop
  down:
    - **Two sequential calls at most.** `whoami` first, since it decides the session, then the
      page's other calls concurrently; so a LAN adds two round trips, not four.
    - **Pooled keep-alive connections.** One connection pool per `oml-web` process, HTTP/1.1
      keep-alive (HTTP/2 when the hop is TLS), bounded in size, so no call pays a TCP or TLS
      handshake in the steady state.
    - **A timeout per call.** `web.api_timeout`, a constant of the build until measured; a call
      past it is a failure of that call, with the request id of d3 layer 1.
    - **Partial-page failure states.** A page names its primary call (the listing, the memory,
      the queue) and its secondary ones (counts, offers, usage). A failed or timed-out secondary
      call renders the page with that part marked unavailable and the request id beside it; a
      failed primary call renders the error page of d3 layer 7; a failed `whoami` the sign-in
      redirect or the error page. An act (`POST`) is never partial: it fails whole, and its
      idempotency key makes the resubmit a replay.

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
  the pages need fields. The check: `oml-sdk`'s test reads `openapi.yaml` and fails when an
  operation's method, path, path parameters or request field names differ from the SDK's
  table, and `generate_sdks.py --check` runs it, so a contract change that forgets the Rust
  client fails where the Python and TypeScript layers already fail. The cost is hand-written
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
| d1 | `oml-web` depends on `oml-sdk`, `oml-page` and the HTTP stack | also `oml-web-credential`, a leaf of `sha2` and `hex` that `oml-domain` re-exports | the capacity credential and the CSRF token are derived on both sides of the hop; one implementation, outside the application core, keeps the two equal |
| d1 | `oml-page` holds the stylesheet and the helpers | also the page chrome (`shell`, `document`, the tabs), the marks, the same-origin rule, and `gen_design_tokens.py` writes into it | a moved page draws the same header as the lead's pages, asserted equal by `a_moved_page_draws_the_header_and_tabs_the_leads_pages_draw`; #230's identical check is one function both call |
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
| d4 | (silent) | `oml-web` answers `GET /readyz` on its own listener: one unauthenticated call to the lead's `/readyz`, `200` only while the lead is ready and names a `web_client` other than `none`; a lead that names none (`unknown`) is not ready, and the doctor fails it | a probe of the process alone cannot see the hop; the public `/readyz` stays the lead's; a lead that proves no identity fails closed |
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
| d3 layer 6, d8 | the per-session cap on calls in flight; two sequential calls | not built; the view bounds its own fan-out to eight calls in flight (`oml_web::workareas::FAN_OUT`, unmeasured, #170) after the tree, each row's offers and live proposals a secondary read that marks only its row unavailable when it fails | a page of N workareas makes N to 3N calls, where the lead made the same reads in process, and each offers call now runs up to four more verb checks for `refused`; both belong in #170's measurement; the cap across a session's pages lands with the memories view, which fans out further |
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
| d3 layer 4 | Post/Redirect/Get for every act | a key's value or a connect code is shown once on the redirect target: the act seals it, ChaCha20-Poly1305 under a key derived from the session token, into a `__Host-oml_once` cookie (`HttpOnly`, `Secure`, `SameSite=Strict`, five minutes) that the target opens, shows under `no-store` and clears in the same answer; it is never in a URL, a line or `oml-web`'s memory past the request | the API shows the secret exactly once, so the target cannot read it again; the session-derived key lets any replica open it and no other session; the cookie, not a server-side store, keeps `oml-web` stateless (rejected: a short-lived store in `oml-web`, which replicas would have to share; the value in the redirect's query, which proxies log) |
| d3 layer 4 | the bulk verdict's page shows each item's outcome | the bulk verdict redirects to the view with its counts, approved, rejected, skipped, by the same user and replayed, as numbers only; the confirmation, which records nothing, is answered as a page, not a redirect | per-item outcomes do not fit a URL and are not stored; the items left are still listed on the view; a crafted link can put only numbers on the page |
| d3 layer 4, d5 | the verdict's idempotency key minted from the serve | from the form's nonce, as every act's; review next is a `POST` under its own key that redirects to the view with the serve id and the served memory's id, and the view draws the served card by that memory id from `GET /v2/review/held?ids=`, which claims nothing, never reading the serve id, which the contract leaves opaque; the claim that follows a verdict runs under a key derived from the verdict's, so a resubmitted verdict replays its claim too, and a verdict recorded before that claim fails lands on the view with its notice | a `GET` that served the next item would claim one on every reload; a recorded verdict is not an error |
| d3 layer 4, d5 | `Idempotency-Key` taken by `POST /v2/keys`, `POST /v2/keys/{key}/connect-codes` and `POST /v2/keys/{key}/denials` | as a session key each replays its recorded outcome under the same key and body with `Idempotency-Replay: true` and acts nothing again; the recorded body leaves out the key's value and the connect code, which are shown once; `oml-web`, given an answer without the value or the code, redirects to the view and sets no cookie, so the first answer's sealed secret stands | a double click would otherwise issue twice under one name, which is refused, or mint a second code that voids the first; storing the secret to replay it would keep what is shown once |
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
| d1, d4 | the frame route `web/render.rs` belongs to `oml-web`; the routes are exact paths | `oml-web` serves `GET /web/render/{token}` and `GET /web/assets/{file}`, listed as `oml_web::PREFIXES`; Caddy and both compose routers match `/web/render/*` and `/web/assets/*`, the chart's `web.ingressPaths` lists `/web/render` and `/web/assets` as `Prefix`, and `check_chart.py` holds each listed `pathType`. The token is sealed under a key `oml-web` draws at start (ChaCha20-Poly1305), bound to the session's CSRF token, and the source is held in its memory, bounded as before (`RENDER_SOURCES_MAX`); the frame answers only after `GET /v2/whoami` resolves the session's `web` credential, and every failure is `404` | a token names a source, so the route cannot be one exact path; the process that showed the source holds it, which d4 already names as the reason replicas need affinity for this route; the session is the API's to judge (d3 layer 3) |
| d3 layer 5 | every page under the one policy with no `frame-src` | a page that holds a drawing, decided by whether one was drawn and never by the page's text, names `frame-src <public origin>/web/render/`; the frame keeps its own policy and `Referrer-Policy: no-referrer`, and the bundle its `immutable` cache rule. The headers layer keeps a handler's policy, cache rule and referrer policy only on an answer marked `OwnHeaders`, which `oml_web::render` alone sets; every other answer gets the pages' headers whatever its handler set | the lead framed only where a page showed a diagram, and the frame's policy is ADR-0056 d9's; an explicit mark, not the header's presence, so a future handler cannot weaken the pages' policy by setting one |
| d1, d4 | 256 held sources, one pool | at most `RENDER_SOURCES_PER_SESSION` (32) of them per session, which gives up its own oldest first, inside the pool of 256 | one session opening many diagrams would otherwise evict every other session's, whose frames then answer `404` (#159; chosen, not measured) |
| d5 | (the fifth change) the Review card shows a Mermaid claim's source and says the drawing shows on the memory's own page once promoted, linking nothing | the Review card draws the frame from the source `GET /v2/review/held?ids=` served the reviewer, bound to the reviewer's session; still no link to the held memory's own page, which opens only for its author and the agent's owner | the exit criterion named on #189: `oml-web` now mints the frame's token over a source it was served, so no API operation is needed for it |
| d5 | the admin read serves the fields of the search plus attribution | it also names `kind`, on every item of the search and on the read; the page refuses a supersession unless the read names a kind other than `guardrail`, so a server that names none (one before v0.7.0, during a rolling upgrade) is refused too | the admin page refuses to supersede or retire a guardrail (ADR-0037 d17) and `oml-web` cannot otherwise tell one; the API does not refuse a guardrail's retirement without a rule, so the page's check fails closed |
| d5 | `submit_memory` for `curate` and `supersede`, as the capacity the act needs | the supersession runs as `<user>-curate` when `GET /v2/whoami` as that capacity names the key the admin read says wrote the memory, else as `<user>-review`, and only `auth.invalid` from that `whoami` reads as no curator key: any other failure fails the act, so the acting key never changes because a call failed; curation runs as `<user>-curate`, and a session whose curate credential the API does not resolve gets `access.not_found` naming the next sign-in, never the sign-in redirect; both under the form's nonce as `Idempotency-Key`, so a resubmitted form replays the recorded answer (the operations' own record and replay) | the lead compared key ids in process; the key's name is what the wire serves. Capacities are fixed when the session opens (phase (a) as built), so a designation given during the session reaches the next one; the moved curation test signs in again after it |
| d3 layer 4 | the lead's curate and supersede keys were digests of the source and the text | the act's key is the form's nonce's, as every moved act's | a person who curates the same text twice on purpose writes twice, as the API does for any client |

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
| d3 layer 4 | (silent) | a send that finds the key claimed waits up to 5 s, claiming again every 25 ms, and replays the outcome once recorded; past the wait it answers `409 idempotency.in_progress` with `Retry-After: 1`, which [errors](../api/errors.md) defines | 5 s sits under `oml-web`'s 10 s call timeout (d8), so a double click is answered the first send's outcome, never a timeout; the second answer is the replay, so a key's value or connect code is in the first answer alone and never in the record |
| d3 layer 4 | (silent) | an act whose outcome cannot be written after it committed still answers that outcome; its claim stays, so a resubmission answers `idempotency.in_progress` and acts nothing; the lead logs `idempotency_unsettled` with the act, the key id and the claim id, counts `oml_idempotency_unsettled_total`, and `oml doctor` warns on any | the act happened and the person should see it, a key's value included; a held claim fails closed: nothing is repeated, and a fresh form acts under a fresh key. A claim whose process died is not reclaimed after a time, because whether its act committed is unknown; the operator clears one by hand after checking the journal ([runbook](../operations/runbooks.md#a-form-answers-idempotencyin_progress)), as no sweeper removes them yet |
| d3 layer 4 | (silent) | the act and its record run in a task of their own, so a request dropped mid-act (`oml-web`'s call timeout, a closed connection, a lead shutting down gracefully) cannot stop the act between its commit and its record: the act records its outcome and the resubmission replays it. Only a refusal (`4xx`) of the act's first commit, or a refusal before the act, releases the claim; a failure that may have committed (a `5xx`, a lost connection), a failing step after the commit, a later step of a multi-commit act refused after an earlier one committed, and a panic all leave it | a release after a commit lets the resubmission act twice, which is the failure #233 names; the first review of the claims found that dropping the request, a read after the commit failing, and a panic each released it (rejected: releasing on every drop of the claim, which repeats the act in exactly those cases) |
| d3 layer 4 | (silent) | each attempt draws a claim id, kept in the claim row's `response` beside the time it was claimed; settle and release name it, so a late release of one attempt never drops another's claim; the wait between claims backs off from 25 ms to 250 ms; the serving role may update a row's `response` and `committed_at` alone | an operator removing a stuck claim must not let an old attempt's release drop the next attempt's; about twenty-five claim transactions per five-second wait instead of two hundred; no statement of the serving role can rewrite a row's key or digest |

## Consequences

- `oml-web`, `oml-page` and `oml-sdk` join the workspace; `oml-transport-http` loses its page
  code and keeps the API, the auth context and the traffic layer.
- [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d6's "a session key has no value, so it cannot be presented on `/v1`, `/v2` or `/mcp`"
  becomes: it is presented on `/v1` and `/v2` only by its per-session credential, only from
  `oml-web`'s client identity (the secret or mutual TLS), and never on `/mcp`. d7's tests stay and gain the two of phase (a).
  d18 gains a row: `auth.invalid` `session_outside_web`, with the doctor line
  `warn web: no web client identity set; oml-web cannot reach the API` when the lead has
  neither `web.client_secret` nor `web.client_ca`, and `oml-web` refuses to start without its
  own half, naming the setting.
- Two processes ship and run where one did: the release adds the `oml-web` binary and the
  landing archive (d12), the Helm chart a second Deployment, the compose file a second
  service, and `oml quickstart` starts both. `oml doctor` gains a line for an unreachable
  `web.api_url` from `oml-web`'s side.
- The `http_request` line of the API now records `credential: session` for the inner calls,
  correlated to the page line (ADR-0042 d2, d6). Log volume per page view grows with its calls.
- `docs/api/http-api.md`, `openapi.yaml`, `errors.md` and the SDKs gain the ten operations and
  six changes in phase (a); the CLI may use them (`oml keys list`, `oml review queue`) in a
  change of its own.
- [#170](https://github.com/e8kor/open-memory-lab/issues/170) tracks it: the web hop's overhead is unmeasured against its 5 ms p95 budget, and the
  per-session in-flight cap of d3 is a constant chosen without measurement.
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
