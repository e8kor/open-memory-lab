# Design and plan: completing the web/API separation (ADR-0070)

Status: design, not implementation.  
Source of truth: [ADR-0070](../docs/adr/ADR-0070-the-web-console-is-a-client-of-the-public-http-api-in-a-crate-that-cannot-reach-the-application-core.md).

## Goal

Make the web console a first-party client of the same HTTP contract used by the
CLI, SDKs and agents. It stops calling `oml-app` in process and becomes a
Rust crate that:

- depends on `oml-page`, `oml-sdk`, `axum`, `tower`, `http`, `serde` and
  `serde_json` only;
- cannot reach `oml-app`, `oml-domain`, `oml-guardrail`, `oml-scan`,
  `oml-pg`, `oml-lake`, `oml-fetch`, `oml-rerank`, `oml-transport-http` or the
  lead (ADR-0070 d1);
- presents the same pages, forms and outcomes as today;
- calls only documented operations with the `omls_` credential of the
  authenticated person’s session.

## Crate layout

```text
crates/
  clients/
    oml-web/
      Cargo.toml
      src/
        app.rs             # composition of pages and middleware
        credentials.rs     # derives the per-capacity omls_ credential
        error.rs           # API envelope to page outcome
        http.rs             # Transport trait implementation
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
- The lead builds one API router and either hands `oml-web` the same clone or
  runs it separately with `web.api_url`.

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
unless the request also carries `OML-Web-Client` equal to `web.client_secret`.

## Layer stack

1. Request id and traffic log.
2. Human-only gate: an `Authorization` header on a page route is refused
   before any other processing.
3. Session load: read the cookie, derive the `web` credential, then call
   `GET /v2/whoami`. `oml-web` does not decide whether a session is valid.
4. Same-origin and CSRF check.
5. Header and CSP rendering.
6. Fan-out rate limit using the page's own limit; acts remain under API limits.
7. Error mapping from API envelopes into page outcomes.

All journal writes are API-side. `oml-web` does not write to the journal.

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

/web (sign-in page, rendered only by the auth context, same middleware route)
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
   - Acceptance: `web_pages.rs` still passes in in-process mode.

3. **`oml-page` shared crate**
   - Extract escaping, CSS/header helpers, constants and compile-time digests.
   - Both transport connect and future oml-web use it.

4. **`oml-web` scaffolding**
   - Add crate with dependency wall test.
   - Add transport, forms, headers, CSRF and error mapping.
   - First page lands only after parity test.
   - Acceptance: build passes with web parity test; `oml_web_reaches_no_application_crate` fails if wall broken.

5. **Pages move one at a time**
   - Start route with no state: `/web?view=guardrails`.
   - Then workareas, memories, review and users.
   - Each page gets a parity marker before its old handler is deleted.
   - Acceptance: unchanged web_pages assertions against in-process and loopback.

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
- in-process and loopback parity
- manual UAT for sessions, CSRF, CSP, and cookie boundaries

## Rollback strategy

Each page moves in its own commit. If one page fails after cutover, revert that page's adapter
without changing the rest. Because the API contract and `oml-web` behavior inherit the same inner
HTTP stack, failures are localized to asset and form handling.

## Open risks

1. CSP digest and script placement errors.
2. Idempotency key derivation collisions.
3. Fan-in pagination across the API.
4. Admin capacity table mistakes.
5. Per-request latency due multiple API calls versus prior in-process invocation.
6. Precompiled digests and generated token file drift.

## Next implementation slice

Start with read-only pages:

- `oml-page`
- `oml-web` skeleton
- dependency and route walls (initially permissive)
- `GET /v2/keys` and `GET /v2/whoami` contract (implemented or documented by phase a)
- move `guardrails_view` first because it is read-only and maps directly onto `GET /v1/guardrails`.

Do not start with `memories` or `review`; they have the highest parity risk.
