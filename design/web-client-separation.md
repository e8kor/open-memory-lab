# Design and plan: completing the web/API separation (ADR-0070)

Status: design, not implementation.  
Source of truth: [ADR-0070](../docs/adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md),
as amended on 2026-11-10: `oml-web` is a separate process only (d4, d11), and the landing is a
separate deployable built from `site/` (d12).

## Goal

Make the web console a first-party client of the same HTTP contract used by the
CLI, SDKs and agents. It stops calling `oml-app` in process and becomes a
Rust crate and its own binary, `oml-web`, a separate process that:

- depends on `oml-page`, `oml-sdk`, `axum`, `tower`, `http`, `serde` and
  `serde_json` only;
- cannot reach `oml-app`, `oml-domain`, `oml-guardrail`, `oml-scan`,
  `oml-pg`, `oml-lake`, `oml-fetch`, `oml-rerank`, `oml-transport-http` or the
  lead (ADR-0070 d1);
- presents the same pages, forms and outcomes as today;
- calls only documented operations with the `omls_` credential of the
  authenticated person’s session, over HTTP at `web.api_url`, which is
  required; there is no in-process mode.

## Crate layout

```text
crates/
  clients/
    oml-web/
      Cargo.toml
      src/
        main.rs            # the oml-web binary: settings, listener, shutdown
        config.rs          # web.api_url (required), web.listen, client identity, web.public_url
        app.rs             # composition of pages and middleware
        credentials.rs     # derives the per-capacity omls_ credential
        error.rs           # API envelope to page outcome
        http.rs             # Transport over a pooled keep-alive HTTP client
        middleware.rs       # layer stack, request id, CSRF, headers
        pages/
          keys.rs
          keys_inventory.rs
          memories.rs
          workareas.rs
          review.rs
          guardrails.rs
          start.rs
          users.rs        # (open sign-up only if enabled)
        templates/          # generated from oml-page
      tests/
        parity.rs           # web_pages.rs moved, assertions untouched
        dependencies.rs     # the cargo metadata wall (ADR-0070 d1)
  shared/
    oml-page/            # new shared presentation crate
  transports/
    oml-transport-http/    # API, auth context, traffic layer
```

Rules:

- `oml-web` never formats an app-domain string from inside the application.
  Every field is copied from the wire.
- All no-op page state is HTML, cookie state, form nonce state or CSRF state.
- `oml-transport-http` keeps only pages that cannot leave the binary:
  `/oauth/*`, providers, sign-in/sign-up terms and discovery documents.
- The lead serves the API and the auth routes only; it does not link `oml-web`.
  `oml-web` reaches it at `web.api_url`.

## Deployment shapes

```text
production
  browser ── Caddy, the public origin
               ├─ / /teams /personal /privacy /terms ──> site/ static files
               ├─ /web/sign-in /web/callback/* /web/sign-up ──> lead :8090
               ├─ /web /web/* ──> oml-web :8091 ──HTTP keep-alive──> lead :8090
               └─ /v1/* /v2/* /mcp /oauth/* /.well-known/* ──> lead :8090

local development (oml quickstart starts all three)
  browser ── Caddy router, 127.0.0.1:8080, the one published port
               ├─ the pages oml-web serves ──> oml-web :8091 ──HTTP──> lead :8080
               └─ everything else (sign-in, /v1, /v2, /mcp, /oauth, /readyz) ──> lead :8080
```

- Production routes by path behind one public origin, since `__Host-` cookies
  bind one host. `oml-web` reaches the lead directly on loopback or the LAN,
  never back through the public proxy.
- Local development runs the lead, `oml-web` and a Caddy router as compose
  services; `oml quickstart` and `docker-compose.yml` start all three, and the
  router routes exactly as production does, so the browser sees one origin and
  there is no `web.page_url` setting (ADR-0070 d4).
- Kubernetes: two Deployments; the Ingress routes each path in
  `web.ingressPaths` (`/web/guardrails`, `/web/workareas` and
  `/web/admin/workareas` today) to `oml-web` and every other
  path to the lead.
- `deploy/caddy/Caddyfile` is the production route table; a test in `oml-web`
  holds it and both compose files to `oml_web::PAGES`.
- The landing is static files from the `oml-site-<version>.tar.gz` release
  archive, built from `site/`; neither the lead nor `oml-web` serves it.

## Dependency boundary

The dependency wall is a structural test, not a code review habit:

```bash
cargo metadata --format-version 1 --locked
# walk oml-web's resolve graph over normal/build edges and fail if it reaches
# oml-app, oml-domain, oml-guardrail, oml-scan, oml-pg, oml-lake, oml-fetch,
# oml-rerank, oml-transport-http, oml-server-lead or sqlx.
```

Dev-dependencies remain outside the walk so the parity suite can compose the
lead as the current page tests do.

## Identity

A person signs in and the browser receives `__Host-oml_session`. That cookie is
not sent to the API directly.

At session-open, the auth context stores, for every capacity the person reaches,
HMAC-SHA256 of the session token over:

```text
oml/web-capacity/v1/<capacity>
```

The `web` capacity becomes:

```text
omls_ + base62(HMAC-SHA256(session_token, "oml/web-capacity/v1/web"))
```

The API resolves it to:

- the session row;
- the capacity (`web`, `review`, `curate`, `admin`);
- the `<user>-web`, `<user>-review` etc. keys that already hold the person’s
  grants.

The credential is never stored as a decryption key; only its digest is held.
It dies with the session. `omls_` is refused on `/mcp`, and on `/v1`/`/v2`
unless the request proves `oml-web`'s client identity: either
`OML-Web-Client` equal to `web.client_secret`, set on both processes (the
default), or mutual TLS where the lead terminates TLS and `web.client_ca` and
`web.client_subject` name `oml-web`'s certificate. A client certificate a
gateway forwards in a header is not accepted.

## Layer stack

The stack is `tower` layers of the `oml-web` binary, outermost first. An
external gateway in front (Caddy, an ingress, an API gateway) is optional: it
may add TLS, its own rate limit or a WAF, and replaces none of the layers;
`oml-web` trusts no header it sets for identity, session or CSRF.

1. Request id and traffic log (the page-level audit line).
2. Human-only gate: an `Authorization` header on a page route is refused
   before any other processing.
3. Session load: read the cookie, derive the `web` credential, then call
   `GET /v2/whoami`. `oml-web` does not decide whether a session is valid;
   with none, it answers `303` to the lead's `/web/sign-in`.
4. Same-origin and CSRF check.
5. Header and CSP rendering.
6. Fan-out rate limit using the page's own limit; acts remain under API limits.
7. Error mapping from API envelopes into page outcomes.

All journal writes are API-side. `oml-web` does not write to the journal.

## Performance over the hop

- At most two sequential API calls per page: `whoami`, then the rest
  concurrently.
- One pooled keep-alive connection pool per process (HTTP/2 when the hop is
  TLS); no per-request handshake.
- A timeout per call, `web.api_timeout`; the value is unmeasured (#170).
- Partial-page failure states: a failed secondary call (counts, offers,
  usage) renders the page with that part marked unavailable and the request
  id; a failed primary call renders the error page; an act never half-runs.
- Budget: p95 page time at most 5 ms above today's with the API on loopback,
  plus two round trips on a LAN; unmeasured until #170.

## Mockups

```text
/web
+-------------------------------------------------------------+
| OML · quickstart  Keys  Memories  Review  Guardrails  Start |
| korniichuk-web                                             |
+-------------------------------------------------------------+
| Usage   ## memories / writes / tokens                       |
|                                                             |
| Recent memories                                              |
| - promoted  oml-domain::ports · modified 2d ago · open       |
| - held      capture route · review pending · review          |
|                                                             |
| Actions: [ Remember ] [ Recall ] [ Copy connect code ]       |
+-------------------------------------------------------------+

/web/sign-in (rendered only by the auth context on the lead; oml-web redirects here)
+-------------------------------------------------------------+
| Open Memory Lab                                              |
| Sign in to continue                                          |
| [ Google ]  [ GitHub ]                                       |
| No analytics, no third-party scripts                         |
+-------------------------------------------------------------+
```

```text
/admin users
+-------------------------------------------------------------+
| Web · Review · Users                                         |
| alice@example.com  exports  quotas  disable  close deletion  |
| alice-review is the acting key                                |
+-------------------------------------------------------------+

/review
+-------------------------------------------------------------+
| Hold : 12 · oldest 3d · workarea tree with counts             |
| Candidate row [approve] [reject] [replace reasoning]          |
| nothing of held code is promoted unless policy admits          |
+-------------------------------------------------------------+
```

The page layout is deliberately ordinary: no richer client-side behavior is
required for the migration itself.

## API call plan

Existing unchanged calls stay as designed. Ten candidate new operations are
still required for pages:

1. list keys for a user,
2. review held queue summary,
3. grant a person's session key review or curation capacity (the `omls_` credentials themselves are derived from the session token, not issued by an operation),
4. workarea offers,
5. session end under the session credential,
6. account export,
7. deletion request,
8. quota update,
9. disable user,
10. close deletion.

The design refuses any operation that has no documented use case.

## Auth and capacity flow

```text
browser -> POST sign-in form
auth context creates session row + session key(s)
browser receives cookie containing session token only
oml-web uses cookie to derive the capacity credential
oml-web -> API with Authorization: Bearer omls_...
auth context resolves the omls_ digest to session, capacity, person key
API returns the same JSON as today, but with no page syntax
```

## Migration phases and tests

1. **Rules audit and OpenAPI changes**
   - Add operations and changes to `openapi.yaml`, `http-api.md`, `errors.md`.
   - Update `check_contract_sync.py`, SDK generation, and `use_cases.md`.
   - Add `web_*` use-case freeze list.
   - Acceptance: docs and contracts agree.

2. **Transport web delegation**
   - Move `POST /v2/web/emails`, `POST /v2/web/emails/remove` and related pages into lab ring handlers.
   - Remove `web_*` from `/web` rendering paths as they move.
   - Acceptance: `web_pages.rs` still passes against the lead's pages.

3. **`oml-page` shared crate**
   - Extract escaping, CSS/header helpers, constants and compile-time digests.
   - Both transport connect and future oml-web use it.

4. **`oml-web` scaffolding**
   - Add crate and the `oml-web` binary with config loading (`web.api_url`
     required), a health endpoint and the dependency wall test.
   - Add the middleware skeleton: request id, traffic log, CSP, CSRF, error
     mapping; then transport and forms.
   - First page lands only after parity test.
   - Acceptance: build passes with web parity test; `oml_web_reaches_no_application_crate` fails if wall broken.

5. **Pages move one at a time**
   - Start route with no state: the Guardrails view, now `/web/guardrails`.
   - Then workareas, now `/web/workareas` and `/web/admin/workareas`, with the
     first acts: the confirmed dropdown's transitions, ratifications and mode
     changes, each through its documented operation under the confirmation's
     idempotency key; the lead answers the old acts with a `303` to the view
     and runs none of them.
   - Then memories, review and users.
   - Each page gets a parity marker before its old handler is deleted.
   - Each moved page gets its own path under `/web/`, and the lead answers
     the old `?view=` form with a `303` to that fixed path, so every proxy,
     compose router and Ingress routes by path alone (ADR-0070, amended
     2026-11-11).
   - Acceptance: unchanged web_pages assertions over the loopback hop to a
     separate `oml-web` process.

6. **Sign-in and session stays in auth**
   - Keep all routes beginning with `/web` in the transport's auth context.
   - Only the `omls_` credential may call API endpoints.

7. **Phase out old page use cases**
   - Rename moved logic to the operation use case.
   - Remove `web_*` from `oml-app` outside auth.
   - Acceptance: `grep -rn 'pub async fn web_' crates/oml-app` matches only auth.

8. **Walls and docs**
   - Required CI checks.
   - Route coverage tests.
   - `the_lead_serves_no_page_route_outside_oml_web_and_auth`.

## Mockup principles

- Keep the same commands, state names and outcomes.
- Use only route templates to support aggregate request instrumentation.
- No per-person analytics labels per ADR-0042 and existing metrics rules.
- All page act failures return the same mapped message and request id.
- Both pages carry the guardrail/action trace.

## Test gates

- `check_contract_sync.py`
- `generate_sdks.py --check`
- `cargo build -p oml-server-lead` with web crate and dev parity dependency
- `cargo test -p oml-transport-http --test web_pages`
- #167 `open_signup_pg.rs`
- Chromium frame/Atlas scripts
- dependency wall
- route wall
- idempotency replay on each `POST`
- parity over the loopback hop, `oml-web` as its own process
- `site/build.py --check` (the landing's `site/dist/` is reproducible)
- manual UAT for sessions, CSRF, CSP, and cookie boundaries

## Rollback strategy

Each page moves in its own commit. If one page fails after cutover, revert that page's adapter
and route its URL back to the lead's handler in the proxy, without changing the rest. Because the
API contract is the same for `oml-web` and every other client, failures are localized to asset
and form handling.

## Open risks

1. CSP digest and script placement errors.
2. Idempotency key derivation collisions.
3. Fan-in pagination across the API.
4. Admin capacity table mistakes.
5. Per-request latency of several API calls over a network hop versus the prior
   in-process invocation (d8; #170).
6. Precompiled digests and generated token file drift.
7. Version skew between `oml-web` and the lead during an upgrade.

## Next implementation slice

Start with read-only pages:

- `oml-page`
- `oml-web` skeleton
- dependency and route walls (initially permissive)
- `GET /v2/keys` and `GET /v2/whoami` contract (implemented or documented by phase a)
- move `guardrails_view` first because it is read-only and maps directly onto `GET /v1/guardrails`.

Do not start with `memories` or `review`; they have the highest parity risk.
