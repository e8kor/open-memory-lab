<!--
id: adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core
tier: engineering
status: proposed
answers: why the web pages stop calling oml-app's use cases and call the public HTTP API instead; which crate serves the pages and what it may depend on; how a signed-in person's session becomes an API credential without a key value at rest or in the browser; which middleware layer owns the request id, the session, CSRF, CSP, rate limits, audit and error mapping; how oml-web reaches the API in the lead and when it runs on its own; which existing operation serves each web call and which operations are new; what stays in the auth context; the bounded contexts; the migration phases and the test that proves parity; the overhead budget; what it amends in ADR-0042 and ADR-0045
-->

# ADR-0070 — The web console is a client of the public HTTP API, in a crate that cannot reach the application core

**Status:** proposed · 2026-11-06

**Amends:** ADR-0045 (d1: the page routes move to `oml-web` and the sign-in routes to the auth
context; d6: a session key gains a per-session credential; d7: that credential, never the
cookie, reaches the API, and only through the web client; d14: the page's line and the API's
lines share a correlation id; d16: the new and changed operations below), ADR-0042 (d2:
`credential: session` is now written on the API's own lines as well as on the page's)

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

Decided, not built. The build follows #163, #166 and #167.

## Decision

- **d1 — `oml-web` is its own crate, depends on the Rust SDK, axum, tower and one shared page
  crate, and a test fails when its dependency graph reaches the application core.** The
  crate lives at `crates/clients/oml-web` and holds the pages, their forms, their templates,
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
  request carries `OML-Web-Client` equal to the web client secret (`auth.invalid`,
  `details.reason: session_outside_web`). The secret is a boot key of the lead,
  `web.client_secret`; in-process the composition mints one per boot and hands it to both
  sides, so both modes pass the same check (d4). An agent that reads the cookie therefore
  still reaches no API with it, and an agent's bearer key still reaches no page. Which
  capacity an action uses is `oml-web`'s compiled table (`issue_key` → `web`, `verdict` →
  `review`, `curate` → `curate`, `allow_email` → `admin`, and so on), never a form field.
  Admin acts use the `admin` capacity under the same rule: `<user>-admin` is an admin key,
  holds no dreamer grant, and is refused every review verb by the API whatever the table says
  (R1, [ADR-0001](ADR-0001-the-constitution.md)). The cost is stated: a defect in the table
  can make an act run as another of the same person's capacities, never as anyone else, and
  the API's verb rules still bind it; and whoever holds both the cookie and the client secret
  acts as the person, which is the operator's process boundary.

- **d3 — Seven layers wrap every page request, outermost first, and each concern has one
  owner.**

    | # | Layer | Owner | What it does, and why it sits there |
    |---|---|---|---|
    | 1 | Request id and traffic log | `oml-web`, the [ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md) layer reused from `oml-page`'s line type | Honours or mints `X-Request-Id`, writes the page's `http_request` line with `credential: session`, the acting key, `web_action` and the route template. Each inner API call carries `X-Correlation-Id` set to the page's request id and `X-Request-Id` `<page id>.<n>`, so one `jq` filter finds the page line and its API lines. Outermost, so a refusal by any inner layer is still logged. |
    | 2 | Human-only gate | `oml-web` | An `Authorization` header on a page route answers `access.verb_ceiling` `web_is_human_only` before anything is read (ADR-0045 d7). |
    | 3 | Session load | `oml-web` reads the cookie; the API decides | Reads `__Host-oml_session` and derives the `web` credential; the first API call of the page, `GET /v2/whoami`, is the check. No session, or `auth.invalid` from the API, renders the sign-in page (d6). `oml-web` never judges a session itself. |
    | 4 | Same-origin and CSRF | `oml-web` | Every `POST` passes the existing `same_origin` check against the public origin and carries `csrf_token` = SHA-256 over the fixed purpose and the session token, compared in constant time (ADR-0045 d8, unchanged). Every form also carries a random nonce, and the `Idempotency-Key` of the act's API call is the hex SHA-256 over the session token and that nonce, so a resubmitted form replays instead of repeating. The API never sees a form, so CSRF belongs to the layer that renders forms. |
    | 5 | Response headers and CSP | `oml-web` | `default-src 'none'`, `style-src` naming the one stylesheet's digest, `script-src` naming only the hashes of the scripts that page carries (none today; #167's copy script on `/web/start`), `form-action 'self'`, `frame-ancestors 'none'`, `base-uri 'none'`, `no-store`, `nosniff`, `Referrer-Policy: same-origin`; the frame route keeps its own policy. Digests computed at compile time from `oml-page`. |
    | 6 | Rate limit | `oml-web` for fan-out, the API for acts | A page view becomes several API calls, so `oml-web` caps the requests one session has in flight and answers `limit.rate` with `Retry-After` past it; the cap is a constant of the build, unmeasured ([#170](https://github.com/e8kor/open-memory-lab/issues/170)). Every act stays under its key's limits at the API, as for any client ([FR-8](../product/roadmap.md)). Provider-call caps stay with sign-in in the auth context (ADR-0045 d15). |
    | 7 | Error mapping | `oml-web` | The API's envelope (`code`, `details`) becomes a page outcome: `access.not_found` the not-found page, `auth.invalid` the sign-in page, `request.validation` the form again with `details.field` marked, `limit.*` the retry page, `review.*` and `workarea.*` the message the page shows today. A code the table does not name renders the generic refusal page with the request id. |

    **Audit is the API's journal, not a layer of `oml-web`.** Every act and every admin read
    is journalled by the use case behind its operation, under the session key, as today;
    `oml-web` writes no journal event and keeps no store, so a page cannot record anything the
    API would not. Its line (layer 1) is the page-level trace beside it.

- **d4 — In the lead, `oml-web` calls the API in-process through the same router the listener
  serves; on its own, over HTTP to `web.api_url`; both pass the same bytes through the same
  stack.** The SDK sends through one trait, `Transport`, taking an `http::Request` with a
  serialized body and answering an `http::Response`. In the lead, the composition builds the
  API router once, serves it on the listener, and hands `oml-web` a clone of the same
  `Router` as a `tower` service: each call is `oneshot`, so it passes the traffic guard, the
  route table, authentication, the idempotency store and the body limits exactly as a
  network request does, with no socket. Standalone, `oml-web` is the `oml-web` binary with
  `web.api_url` and `web.client_secret` set, and its transport is an HTTP client; the public
  origin, for the same-origin check, is read from the API's
  `/.well-known/oauth-authorization-server` `issuer` at start, so it is not a third setting.
  A standalone `oml-web` sits behind the same public origin as the lead, routed by path, since
  `__Host-` cookies bind one host; the auth routes of d6 stay on the lead. In-process is
  preferred over the loopback hop the operator named, because a loopback listener adds a
  socket, a second accept path and a peer address of `127.0.0.1` on every API line without
  making the boundary any more real: the boundary is the `Transport` trait and the serialized
  request, which are identical in both modes. The cost: the in-process API line shows no
  client address (the page line holds the person's), and a standalone deployment of more
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
    | `GET /v2/workareas/{workarea}/tree` | `counts=true`: each node carries `{held, promoted, rejected}` as the caller's listing would show them, and `truncated` past `REGION_COUNT_MAX`; for the admin role every row counted as structure, no content | `web_regions`, `admin_regions` (#163) | ADR-0045 d30 | for the admin reading |
    | `POST /v2/admin/memories/search` | `origin_kind` and `author` filters | `admin_search_filtered` (#163) | ADR-0045 d22, d24 | yes |
    | `GET /v2/review/held` | `ids`: the named held rows only, each with its row or `null` when no longer held or no longer the caller's to judge | `review_named`, the bulk confirmation (main) | ADR-0055 d10 | no |
    | `POST /v2/web/emails`, `POST /v2/web/emails/remove` | `keep_an_admin: true` refuses removing or un-flagging the tenant's last admin entry, `request.validation` `details.field: admin`, in the same transaction | `web_other_admin` (main) | ADR-0045 d10 | yes |
    | `GET /v2/whoami` | `person`: for a key naming a user, `{user, under_quotas, usage, quotas, deletion_requested_at}` | `web_own_limits`, `web_deletion_pending` (#167) | ADR-0068 d5, d6, d10 | no |

    **New operations.**

    | # | Method and path | Request | Response | Serves (source) | Traces to | Who |
    |---|---|---|---|---|---|---|
    | 1 | `GET /v2/keys` | query `user?`, `limit?`, `cursor?` | `{items: [{id, name, role, held_by, credential, user, created_at, grants: [{role, workarea: {id, name}, path}], cut_off, connections: [{id, client_name, opened_at}]}], next_cursor}` | `web_own_keys`, `web_keys_of_user` (main), `web_has_agent_key` (#167) | ADR-0045 d9, d13; the designed `listKeys` | a session user key, its own user's keys only; the admin role, the tenant's, `user` narrowing |
    | 2 | `GET /v2/review/queue` | none | `{items: [{workarea: {id, name}, path, held, oldest_at}]}`, no content | `web_queue` (main) | ADR-0045 d10, the designed `reviewQueue` | admin |
    | 3 | `POST /v2/web/users/{user}/capacities` | `{capacity: review \| curate, workareas: [NameRef]}` | `{key: {id, name}, grants_added: [...], journal_seq}` | `web_grant_review`, `web_grant_curation` (main) | ADR-0045 d10, ADR-0044 d7 | admin |
    | 4 | `GET /v2/workareas/{workarea}/offers` | none | `{transitions: [{to}], mode: {to} \| null, commit: {offered, held}}`, each offered only where the verb's own check admits the caller now | `workarea_offers`, `workarea_offer_check` (#163) | ADR-0045 d28 | no |
    | 5 | `POST /v2/web/session/end` | none | `204` | `web_sign_out` (main), the `sign_out` action | ADR-0045 d5 | an `omls_` credential only; it ends the session it derives from |
    | 6 | `GET /v2/web/export` | none | `application/json` attachment of everything the person holds | `web_export` (#167) | ADR-0068 d6 | session user key only |
    | 7 | `POST /v2/web/deletion-requests` | none | `{requested_at, journal_seq}` | `web_request_deletion` (#167) | ADR-0068 d6 | session user key only |
    | 8 | `POST /v2/web/users/{user}/quota` | `{memories?, keys?, connections?}` | the user's row as `GET /v2/web/users` serves it | `web_set_quota` (#167) | ADR-0068 d5 | admin |
    | 9 | `POST /v2/web/users/{user}/disable` | `{reason?}` | `{sessions_ended, journal_seq}` | `web_disable_user` (#167) | ADR-0068 d8 | admin |
    | 10 | `POST /v2/web/users/{user}/deletion/close` | `{outcome}` | `{journal_seq}` | `web_close_deletion` (#167) | ADR-0068 d6 | admin |

    Every state-creating `POST` takes `Idempotency-Key`, and every refusal is the envelope of
    [errors](../api/errors.md). Six of the ten need the admin key role; #1 needs it for any
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
  produces the sign-in page under the fixed URL `/web`; `oml-web` does not render it and
  has no dependency on the auth module, so the wall holds even for the unauthenticated page.

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
    | (a) | `omls_` credentials and the web client secret (d2); the ten new operations and six changes (d5), each with its use case, its contract entries and its tests; `oml-sdk` (d9) | `check_contract_sync.py`, `generate_sdks.py --check`, and per operation a test named after its sentence, among them `an_omls_credential_without_the_web_client_is_auth_invalid`, `an_omls_credential_on_mcp_is_refused`, `a_capacity_credential_ends_with_its_session`, `the_admin_capacity_ends_when_the_entry_loses_admin`, `a_bearer_user_key_cannot_export_or_request_deletion` |
    | (b) | `oml-page`, `oml-web` with the layers of d3, mounted by the lead in front of the old handlers; pages move one at a time behind the same URLs, the old handler deleted in the commit that moves its page | the page suites, unchanged (below), green after each page moves; `oml_web_reaches_no_application_crate` |
    | (c) | the `web_*` use cases deleted: the logic behind a new operation is renamed to that operation's use case, the sign-in ones move to `oml-app`'s auth module, the rest go | `grep -rn 'pub async fn web_' crates/oml-app --include='*.rs' | grep -v 'crates/oml-app/src/app/auth.rs'` is empty; the workspace builds and every suite passes |
    | (d) | the walls: the dependency test is a required CI check, and `the_lead_serves_no_page_route_outside_oml_web_and_auth` asserts the route table | both tests required on `main` |

    **Parity** is `crates/transports/oml-transport-http/tests/web_pages.rs` (51 tests on
    `main` before the `web_pages.rs` additions in this change), #167's `open_signup_pg.rs` page cases and the Chromium
    scripts (`tests/chromium/atlas.py`, `render_frame_chromium.rs`), moved to
    `crates/clients/oml-web/tests/` with their assertions untouched. Only the rig changes: it
    builds the lead's composed router, `oml-web` included, and seeds through the app as today
    (a dev-dependency). The suite runs twice, once over the in-process transport and once over
    a loopback listener with `web.api_url` set, so the two modes of d4 are asserted equal on
    every page. A test that has to change its assertions is a behaviour change, named in the
    change that makes it.

- **d8 — The budget is p95 page time plus API time no more than 5 ms above today's, in-process;
  the overhead is unmeasured until phase (b) measures it.** A page view becomes one to four
  API calls (the Memories view: `whoami`, the listing, the tree with counts, the offers), and
  each adds serialization both ways, a route match, an `omls_` digest lookup (an indexed read;
  today one `web_caller` per request already does it once) and, on a `POST`, an idempotency
  row. The 5 ms p95 is a budget the operator can hold the build to, not a measurement: the
  benchmark of each page, before and after, on the [FR-25](../product/roadmap.md) harness is
  [#170](https://github.com/e8kor/open-memory-lab/issues/170)'s exit. Over budget, the remedy is fewer calls per page (a field on an existing
  operation), never a cache of resolved sessions, which would let authority outlive the list.
  Standalone, the network adds its own time; no budget is set for it.

- **d9 — The Rust SDK is a hand-written thin typed client whose operation table a check holds
  to `openapi.yaml`.** `sdks/rust`, crate `oml-sdk`: one method per served operation, request
  and response types with `serde`, the error envelope as one error type, and the `Transport`
  trait of d4 with two implementations. No Rust generator is pinned today:
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
      the web client secret and never on `/mcp`: accepted (d2).
    - **(b) Transport.** In the lead, the in-process `oneshot` through the same router the
      listener serves, in place of a loopback hop: accepted (d4).
    - **(c) SDK.** A hand-written thin client held to `openapi.yaml` by a contract check:
      accepted (d9).
    - **(d) Ring.** The new operations go on `/v2`, the current API version: confirmed (d5).
    - **(e) Sign-in page.** `GET /web` with no live session passes through to the auth
      context's sign-in page, with no redirect: accepted (d6).

## Consequences

- `oml-web`, `oml-page` and `oml-sdk` join the workspace; `oml-transport-http` loses its page
  code and keeps the API, the auth context and the traffic layer.
- [ADR-0045](ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md)
  d6's "a session key has no value, so it cannot be presented on `/v1`, `/v2` or `/mcp`"
  becomes: it is presented on `/v1` and `/v2` only by its per-session credential, only with
  the web client secret, and never on `/mcp`. d7's tests stay and gain the two of phase (a).
  d18 gains a row: `auth.invalid` `session_outside_web`, with the doctor line
  `warn web: web.client_secret unset; oml-web cannot reach the API` when `oml-web` runs
  standalone without it.
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
| **A loopback TCP hop in-process** | The operator's first wording. It adds a socket, a listener and a peer address without making the boundary more real than the in-process `Transport` call through the same router; the loopback mode is kept as the second run of the parity suite (d7). |
| **A generated Rust SDK** | No pinned Rust generator reads the 3.1 document into useful types today, and a generated layer full of untyped objects would push field names back into page code (d9). Reopen when the schemas are typed and a generator is pinned. |
