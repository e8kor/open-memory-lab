<!--
id: adr/ADR-0031-oauth-connections-by-connect-code
tier: engineering
status: proposed
answers: how Claude's hosted apps reach the agent plugin through a claude.ai custom connector; OML as an OAuth 2.1 protected resource with its own authorization server; the 401 challenge and the discovery documents; client registration by client ID metadata document with dynamic registration as the fallback, and the redirect allowlist; the connector key and its one-time connect code; the authorization page; tokens, rotation and reuse detection; why a token is never more than its key; the connection journal events, the oauth error family, operations, the version move and the build order
-->

# ADR-0031 — Claude's hosted apps reach the agent plugin over OAuth, through a connection that a one-time connect code binds to one agent key

**Status:** proposed · 2026-10-31

**Amends:** ADR-0010 (d3, d7), ADR-0014 (d1, d2, d4), ADR-0015 (d1), ADR-0018 (d4), ADR-0028 (d1), and the roadmap's RS-2

## Context

**The clients that cannot hold a key.** Claude's hosted apps — claude.ai on the web, Claude
Desktop, and the Claude iOS and Android apps — reach a remote MCP server as a **custom
connector**: a person adds the server's URL at Customize > Connectors on claude.ai, and the
connector is then usable from every Claude client on that account, mobile included. Custom
connectors are offered on the Free plan (one connector), Pro, Max, Ultra, Team and Enterprise
([Get started with custom connectors using remote MCP](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp),
fetched 2026-10-31). Claude authenticates to a connector by the MCP authorization
specification, revisions 2025-03-26, 2025-06-18 and 2025-11-25
([Building connectors](https://claude.com/docs/connectors/building),
[Authentication](https://claude.com/docs/connectors/building/authentication.md),
[Adding an unlisted connector](https://claude.com/docs/connectors/custom/add-unlisted), all
fetched 2026-10-31). A static bearer header exists only as a beta for a limited set of
organizations; an Owner enters it and every member shares it (same sources). One header shared by
a whole organization is one key behind many agents, which the
[agent plugin](../api/mcp-plugin.md) rules out — attribution, R2 and the reciprocal-approval
statistics key on one key per agent identity — so OML does not rely on it.

**What OML offers today.** `/mcp` takes `Authorization: Bearer <key>` and nothing else, and a call
without a key answers `401 auth.missing` with no challenge
([agent plugin](../api/mcp-plugin.md), [errors](../api/errors.md)). Whether OML adopts OAuth is
[RS-2](../product/roadmap.md), which [ADR-0018](ADR-0018-the-documentation-set.md) d3 lets close
only in a fixed form: the spec revision with its fetch date, the client support verified with a
date, and the trigger recorded as a decision. [ADR-0014](ADR-0014-http-api-and-mcp-facade.md) d2
and [ADR-0010](ADR-0010-roles-and-keys.md) d7 ship bearer keys until that trigger fires. ADR-0010
rejected "OAuth-only identity, including for agents" because agents cannot hold an interactive
sign-in and a third-party identity provider would sit in the write path's availability budget.
Neither objection reaches this record: keys stay the agent credential, the person signs in once
with a code rather than an agent signing in, and the authorization server is `oml-server` itself.

**What the specification asks.** The MCP authorization specification, revision 2025-11-25
([Authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization),
fetched 2026-10-31), builds on OAuth 2.1 (draft-ietf-oauth-v2-1-13). An MCP server **MUST**
publish OAuth 2.0 Protected Resource Metadata (RFC 9728), and its authorization server publishes
RFC 8414 metadata or OpenID Connect Discovery. Authorization servers **SHOULD** support Client ID
Metadata Documents and **MAY** support Dynamic Client Registration (RFC 7591), "included for
backwards compatibility with earlier versions of the MCP authorization spec". Clients send the
RFC 8707 `resource` parameter and S256 PKCE; servers validate exact redirect URIs, validate that a
token was issued for them, never pass a token through, refuse tokens in the query string, and, for
public clients, **MUST** rotate refresh tokens. Where a client ID metadata document names a
`localhost` redirect, the authorization server **MUST** display the redirect URI's host.

**What Claude expects** (the Anthropic sources above, fetched 2026-10-31):

- An unauthenticated request gets `401` with `WWW-Authenticate: Bearer resource_metadata="<url>"`;
  Claude does not honor that header on a `200`. Without a pointer it probes
  `/.well-known/oauth-protected-resource/<mcp-path>`, then `/.well-known/oauth-protected-resource`.
- The metadata's `resource` equals the URL as the person enters it, path included. When
  `authorization_servers` lists more than one, Claude uses the first and never falls back.
- Claude identifies itself by a client ID metadata document when the authorization server
  advertises `client_id_metadata_document_supported: true` and `none` among
  `token_endpoint_auth_methods_supported`; otherwise it registers by dynamic registration. A
  manually entered client id and secret are also possible.
- The authorization server serves RFC 8414 or OpenID Connect discovery, supports S256 PKCE, takes
  `application/x-www-form-urlencoded` at its token endpoint and `application/json` at
  registration, answers a failed refresh with `invalid_grant`, and is reachable from Anthropic's
  egress range, `160.79.104.0/21`.
- Claude waits up to 10 seconds for discovery, registration and token calls, and 30 seconds for a
  refresh.
- The hosted apps redirect to exactly `https://claude.ai/api/mcp/auth_callback`; Claude Code uses a
  loopback redirect on any port.
- No secret travels in a URL.

**The maintainer's three decisions, which this record designs around and does not reopen.**
(1) OML becomes an OAuth 2.1 protected MCP resource with its own authorization server, served by
`oml-server` on the same origin, and bearer keys keep working unchanged. (2) The binding is a
one-time connect code: the admin issues the agent key and a connect code together, building on
[ADR-0028](ADR-0028-agent-key-issuance.md)'s `issue-agent`; the authorization page asks for the
code and, on a correct one, opens the authorization bound to that key, so claude.ai holds only
tokens bound to that one key and no key value is pasted anywhere. Pasting the key value on OML's
sign-in page is rejected, because the value would pass through the person's clipboard and browser.
(3) The build follows iteration 3, names as keys (the proposed ADR-0026, on another branch); the
implementation plan marks the pure modules that can land earlier.

**One word to keep apart.** The fixed vocabulary gives "grant" to a key's access to a workarea
node (ADR-0018 d4). OAuth's "authorization grant" is therefore never called a grant here: the
standing authority of one OAuth client to act as one key is a **connection**, and the short secret
that opens it is a **connect code** (d7 adds their vocabulary rows).

**What this record holds itself to.** It adds no MCP tool and no verb an agent can call: R1 holds
because a connection reaches only `/mcp`, and `/mcp` serves only the agent-safe subset. A token is
never more than its key (d11). Held-by-default admission (R3) is untouched: what Claude writes
through a connection is held like any write. Every number below is a shipped default or a
constant, a policy choice; nothing about OML is measured, and what would need measuring opens
[FR-34](../product/roadmap.md). Decided, not built.

## Decision

- **d1 — OML is an OAuth 2.1 protected resource at `/mcp`, with its own authorization server on
  the same origin, beside bearer keys, and only where `oauth.issuer` names the public origin.**
  `oml-server` serves both roles. The issuer is the configured public origin, for example
  `https://oml.doublegate.io`, and the one protected resource is `<issuer>/mcp`. A key value in the
  `Authorization` header at `/mcp` works exactly as today for every existing client — the Claude
  Code plugin, scripts, reviewer crews over HTTP — and every other route stays key-only (d10). With
  `oauth.issuer` unset, the discovery documents and the `/oauth/*` endpoints answer `404`, `/mcp`
  is bearer-only, and issuing a connect code is refused: a deployment that has not named its public
  origin cannot advertise one, which also keeps a standalone laptop install free of the surface.

    The authorization server is OML's own because the binding (d8) needs the key store, because no
    third-party identity provider then enters the request path, and because one origin is one TLS
    name for the person to recognize on the page. The cost is that `oml-server` grows an OAuth
    implementation — unauthenticated endpoints, an HTML page, an outbound fetch (d5) — inside the
    process that holds the signing key. This amends ADR-0014 d2's closing sentence and ADR-0010
    d7's "v1 ships bearer keys everywhere": the trigger fired, and `/mcp` also accepts a
    connection's access token. [RS-2](../product/roadmap.md) is restated in ADR-0018 d3's form.

- **d2 — A call to `/mcp` that fails authentication keeps its typed `401` body and adds the RFC
  9728 challenge.** With no `Authorization` header the answer is `auth.missing` with
  `WWW-Authenticate: Bearer resource_metadata="<issuer>/.well-known/oauth-protected-resource/mcp",
  scope="mcp"`. When a credential was presented and failed — an unknown key, an unknown or expired
  token, a revoked connection, a revoked or expired key — the answer is `auth.invalid` with its
  `details.reason` as today, and the challenge adds `error="invalid_token"` (RFC 6750 §3.1), which
  tells a client to refresh or reconnect. The challenge rides only the `401`, since Claude does not
  read it on a `200`. With `oauth.issuer` unset, `/mcp` answers `WWW-Authenticate: Bearer
  realm="oml"`. The `/v1` and `/v2` routes are unchanged: their `401`s carry no challenge, which
  RFC 9110 §15.5.2 asks of every `401`, and correcting that is outside this record. The cost: a
  bearer client whose key is refused at `/mcp` now sees a pointer to an authorization server and
  may offer the person a sign-in; that sign-in needs a connect code the person does not have, and
  the typed body still says `auth.invalid`. Declared on the
  [HTTP API](../api/http-api.md#connections-oauth-for-hosted-mcp-clients).

- **d3 — Discovery is one protected-resource document at the path-inserted URL, one RFC 8414
  document, and one authorization server.** `GET /.well-known/oauth-protected-resource/mcp`
  answers `resource` (exactly `<issuer>/mcp`), `authorization_servers` (`[<issuer>]`, one entry,
  since Claude uses the first and never falls back), `scopes_supported` (`["mcp"]`),
  `bearer_methods_supported` (`["header"]`) and `resource_name`. RFC 9728 §3.3 requires the
  returned `resource` to be identical to the identifier the URL was built from, so the root form,
  `/.well-known/oauth-protected-resource`, could only describe the bare origin, which Claude would
  reject; it is not served. `GET /.well-known/oauth-authorization-server` answers the RFC 8414
  document: `issuer`, the three endpoints of d4, `response_types_supported` `["code"]`,
  `grant_types_supported` `["authorization_code", "refresh_token"]`,
  `code_challenge_methods_supported` `["S256"]`, `token_endpoint_auth_methods_supported`
  `["none"]`, `scopes_supported` `["mcp"]` and `client_id_metadata_document_supported: true`. No
  OpenID Connect discovery document is served: OML issues no ID token. Both documents are built
  from `oauth.issuer` alone, never from the request's `Host` header, so a forged host cannot point
  a client at another server.

- **d4 — The authorization server serves the authorization code flow with S256 PKCE to public
  clients, at three unversioned endpoints, in the RFCs' wire shapes, under one scope.** The
  endpoints are `GET /oauth/authorize` and `POST /oauth/authorize` (the page, d9),
  `POST /oauth/token` (`application/x-www-form-urlencoded`; `grant_type` `authorization_code` or
  `refresh_token`) and `POST /oauth/register` (RFC 7591, `application/json`, d5). Nothing else is
  served: no implicit, password, client-credentials or device flow, no `plain` PKCE and no request
  without a `code_challenge`, no client authentication method but `none`. The scope `mcp` means
  "act as the connected key at `/mcp`" and narrows nothing: what the connection may do is the key's
  role and grants (d11). A request with no `scope` receives `mcp`; one naming any other scope
  answers `invalid_scope`. A `resource` parameter, when sent, must equal `<issuer>/mcp`, else
  `invalid_target` (RFC 8707 §2); when it is absent — the 2025-03-26 revision predates it — the
  token is bound to the one resource anyway. The token response is `{access_token, token_type:
  "Bearer", expires_in, refresh_token, scope: "mcp"}` with `Cache-Control: no-store`.

    The endpoints are unversioned because their contract is the RFCs', reached by the URLs the
    discovery documents name, and a ring's re-version would move URLs that connected clients
    stored; like `/mcp`, `/healthz` and `/readyz` they sit outside both rings. This amends ADR-0014
    d1 (one versioned JSON API is the contract): these endpoints take form bodies and serve HTML,
    no CLI verb fronts them, and they are the only part of the listener a ring does not version.
    It amends ADR-0014 d4: their refusals travel inside RFC 6749 and RFC 7591 error bodies, naming
    the OML code (d13). The cost is a second error shape on one listener, paid because an OAuth
    client reads `error`, not `code`.

- **d5 — A client registers by a client ID metadata document, preferred, or by dynamic
  registration as the fallback; OML holds no pre-registered and no confidential client.**

    - **Client ID metadata documents.** A `client_id` that is an `https` URL with a path is a
      document URL. At the authorize request, before anything is shown, `oml-server` fetches it
      under fixed bounds: `https` only, on the default port; the host must resolve only to public
      unicast addresses — loopback, private (RFC 1918), shared (100.64.0.0/10), link-local,
      unique-local, multicast and unspecified addresses are refused — and the TCP connection is made
      to the address that was checked, with no second resolution; no redirect is followed; the
      fetch is cut at 5 seconds and 16 KiB; the body must be JSON. The document's `client_id` must
      equal the URL exactly, `client_name` and `redirect_uris` must be present,
      `token_endpoint_auth_method` must be absent or `none`, and every redirect URI must be inside
      the allowlist of d6. The document is cached as its `Cache-Control` allows, for at most 24
      hours, and the token endpoint never fetches: its time budget (10 seconds for Claude) is
      spent on OML's own lookups only.
    - **Dynamic client registration.** `POST /oauth/register` admits `redirect_uris` (each inside
      the allowlist), `client_name`, `token_endpoint_auth_method: "none"`, `grant_types` within
      `["authorization_code", "refresh_token"]` and `response_types` `["code"]`, and answers `201`
      with a server-minted `client_id`, no secret and no registration access token (RFC 7592
      client management is not served). A registered client that opens no connection within 24
      hours is removed. At most 10 000 such unused registrations exist at once; past that,
      registration answers `limit.rate` until some lapse, so a flood can stall new dynamic
      registrations and never a metadata-document client or an open connection.
    - **Neither path journals anything** (d12).

    The metadata document is preferred because its identity is verified by the host that serves
    it, so the page can name that host as verified, while a dynamically registered name is
    self-asserted and the page says so (d9). Dynamic registration stays because Claude uses it
    when metadata documents are not advertised and two of the three revisions Claude follows
    predate them. Pre-registration is declined: a client secret typed into claude.ai would be a
    second shared secret per connector, the kind the connect code exists to avoid, and every
    client here is public. The cost is that the lead fetches client-chosen URLs — a server-side
    request forgery surface bounded by the rules above and carried as a residual in
    [security](../architecture/security.md#connections-the-oauth-surface) — and that a deployment
    with no outbound HTTPS cannot verify a metadata-document client. What Claude does after a
    failed document fetch is not in the cited sources (FR-34).

- **d6 — A redirect URI is admitted only by exact match against a fixed allowlist:
  `https://claude.ai/api/mcp/auth_callback`, and loopback for Claude Code.** A loopback URI is
  `http` with host `127.0.0.1`, `[::1]` or `localhost`, any port (RFC 8252 §7.3), and its path and
  query compared exactly against the client's registered URI, the port ignored. No other `https`
  URI, no custom scheme, no wildcard and no fragment is admitted, at registration, in a fetched
  document or in an authorize request. An authorize request's `redirect_uri` must be exactly one of
  the client's registered URIs; a client with exactly one may omit it. When the client or the
  redirect URI is refused, the page shows the refusal and never redirects (RFC 6749 §4.1.2.1).

    The list is fixed, not a setting, because each entry decides where an authorization code may
    be delivered: admitting another assistant's connector is a recorded decision, not a
    configuration change. `localhost` is admitted although RFC 8252 §8.3 calls it NOT RECOMMENDED,
    because which loopback form Claude Code sends is not in the cited sources. The cost is that a
    loopback redirect cannot prove which program listens, so a document naming a real client can
    front any local program; the page names the redirect host and warns (d9), as the specification
    requires.

- **d7 — A connector key is an agent key issued with a connect code instead of a value; it has no
  bearer value and holds at most one live connection.** `POST /v2/keys` with `connect_code: true`
  runs the [issuance order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)
  unchanged and then refuses, with `oauth.connect_refused`, a key role of `admin` or `operator`
  (`details.reason: role`), `held_by: service` (`held_by_service`) and a deployment without
  `oauth.issuer` (`not_configured`). The server mints no value for the key: `api_keys.hash` is
  null, and the `201` carries `connect_code` — `{code_id, code, expires_at, connector_url}` — in
  place of `value`, returned only after the last `grant.issued` and the `connection.code_issued`
  commit. The key authenticates nothing until a connection opens, and then only at `/mcp`.

    `oml keys issue-agent --principal crew/claude-mobile --workarea <workarea-id> --connect-code`
    sends that issuance and prints exactly two lines on stdout, the connector URL and the code; the
    key id, the expiry and the next step go to stderr. `--out` with `--connect-code` is refused
    (exit `2`): there is no value to write, and a code is for a person to read and type within
    minutes. `--json` prints the `201`, as every verb does. The key's name, as a positional
    argument, arrives with names (ADR-0026 d12, proposed).

    A key is a bearer key or a connector key, never both: a connect code for a key that has a
    value answers `oauth.connect_refused` (`bearer_key`). With one live connection per key —
    opening one revokes the key's earlier connection in the same commit (d12) — the key id on
    every journal event names exactly one credential path, so attribution stays exact without
    any change to the signed envelope ([chain format](../data/chain-format.md)). This amends
    ADR-0010 d3 (key values are shown once and stored only as hashes: a connector key has neither
    value nor hash), and ADR-0015 d1 as ADR-0028 d1 amended it (a connector key's value is never
    shown because none exists; its connect code is shown exactly once; `--connect-code` prints
    its own two lines). It amends ADR-0018 d4 with three vocabulary rows:

    | Term | Means | Never |
    |---|---|---|
    | connection | one OAuth client's standing authority to act as one connector key at `/mcp`, opened by redeeming a connect code and ended by revocation | "grant" or "OAuth grant" (a grant is a workarea grant), "session", "authorization" as the object's name; a database or network connection is always qualified, as in "Postgres connection" |
    | connect code | the short, single-use secret, alive for minutes, that binds one connection to one connector key | "pairing code", "PIN", "OTP", "token" |
    | access token, refresh token | a connection's two OAuth credentials, `oml_at_…` and `oml_rt_…`, stored as digests; on the pages that declare connections, "token" alone means one of them | "key" for either; "token" alone for an API key or a budget token |

    The cost: one agent identity cannot act both from claude.ai and from a header-configured
    client such as Claude Code; each gets its own key and principal, which is what one key per
    agent identity already asks for. And a code that lapses before use needs a new one (d8).

- **d8 — The connect code is twelve Crockford base32 symbols, shown once, stored only as a
  digest, journalled by id, spent by its first correct entry, and alive ten minutes by
  default.** The code is 12 symbols of Crockford's base32 alphabet (`0`–`9` and `A`–`Z` without
  `I`, `L`, `O`, `U`), 60 bits from a cryptographically secure generator, printed as three groups
  of four: `7KQ2-M9XD-4TPA`. An entry is normalized by Crockford's decoding rule — case ignored,
  hyphens and spaces ignored, `O` read as `0`, `I` and `L` as `1` — so a misread symbol is not a
  wasted attempt. The code appears in the `201` and on the CLI's stdout and nowhere else: never in
  a log line, never in a URL, never in the journal. `connect_codes.hash` holds the SHA-256 of the
  normalized symbols; `connection.code_issued` carries the code's id and expiry and no digest,
  because the journal is kept forever and exported, and a digest of a 60-bit secret is a
  brute-force target that a secret living minutes never needs to outlast.

    The lifetime is `oauth.connect_code_ttl`, shipped 10 minutes, settable from 1 to 60. The first
    correct entry spends the code, whether or not a connection then opens (d9). A key has at most
    one live code: minting one voids every unredeemed code of that key in the same commit.
    `POST /v2/keys/{key_id}/connect-codes` (`oml keys connect-code <key-id>`) mints a fresh code
    for a connector key, refused with `oauth.connect_refused` for a bearer key, a revoked or expired
    key, or a deployment without `oauth.issuer`.

    The guessing bound is the code's entropy, not a throttle: with L live codes and G wrong
    entries made during their lifetime, the chance that any entry hits one is at most L·G / 2⁶⁰ —
    a formula, not a measurement. d9 adds a per-page attempt limit, and doctor shows the wrong
    entries an operator should look at (d14). The cost: a code is a bearer secret for its
    lifetime, so whoever enters it first connects; the admin sees which client did
    (`connection.opened`) and can revoke it.

- **d9 — The authorization page asks for the code, then shows what the connection will act as
  before it opens; it sets no cookie and acts only on its own form.**

    1. **The request.** `GET /oauth/authorize` checks the client (d5), the redirect URI (d6),
       `response_type=code`, an S256 `code_challenge`, `resource` and `scope` (d4). A refusal of
       the client or the redirect URI is a page and never a redirect; any later refusal redirects
       with the RFC error and the `state`. A valid request becomes an authorization request,
       alive 10 minutes, named by a 256-bit handle that the form carries. The page shows the
       client — a metadata-document client as its `client_name` "verified by <host>", a
       registered one as its `client_name` marked "name not verified" — and the redirect URI's
       host; for a loopback redirect it adds: "The result goes to a program on this computer.
       Continue only if you started this from Claude Code here." Then it asks for the code.
    2. **The code.** `POST /oauth/authorize` with a correct code spends it and shows the key's
       principal, its role, every workarea it holds a grant on with the grant's role, and: "Claude
       will remember and recall as this agent. Everything it writes is held until someone other
       than its author reviews it. It cannot approve, demote, delete or administer anything." The
       buttons are Connect and Cancel. A wrong code answers the same form with one sentence for
       an unknown, expired or spent code alike — "This code is not valid. Check it, or ask your
       administrator for a new one." — and the attempts left. The fifth wrong entry ends the
       request and redirects with `access_denied`. Once names exist (ADR-0026, proposed), the
       page names the key too.
    3. **The confirmation.** Connect issues an authorization code — 256 bits, alive 60 seconds,
       single use — on the redirect with the `state` echoed verbatim; Cancel redirects with
       `access_denied`. The connect code stays spent either way.

    **Cross-site forgery.** The page sets no cookie, so a cross-site form has no ambient credential
    to ride. Each `POST` must carry the handle and an `Origin` equal to the issuer's, else it is
    refused. `state` is echoed and not required: PKCE closes the client's own forgery exposure, and
    the specification asks clients, not servers, to check `state`. **Headers on every page:**
    `Content-Security-Policy: default-src 'none'; style-src 'sha256-…'; form-action 'self'
    <redirect origin>; frame-ancestors 'none'; base-uri 'none'`, `Cache-Control: no-store`,
    `Referrer-Policy: no-referrer` and `X-Content-Type-Options: nosniff`. The page runs no script.
    `form-action` names the redirect origin because a browser may apply it to the redirect that
    follows a form submission; which browsers do is unverified (FR-34). Client-supplied strings
    are HTML-escaped and shortened, and `logo_uri` is never loaded. The cost: two submissions where
    one would do, and a person who cancels needs a new code.

- **d10 — Tokens are opaque 256-bit strings stored as digests; access tokens live an hour,
  refresh tokens rotate on every use, and a reused refresh token or authorization code ends the
  connection.** An access token is `oml_at_` and a refresh token `oml_rt_`, each followed by 43
  base62 characters under the generation rule of the
  [key format](../architecture/roles-and-keys.md#key-format): the shared `oml_` prefix keeps
  secret scanners matching, and the second underscore keeps a token from ever parsing as a key.
  `oauth_tokens` stores the SHA-256 of each, the same defense as a key's hash. An access token
  lives `oauth.access_token_ttl`, shipped 1 hour, at most 24. Each refresh answers a new pair and
  marks the presented refresh token rotated; a refresh token unused for `oauth.refresh_token_ttl`,
  shipped 30 days, expires, and the connection with it. Presenting a rotated refresh token, or
  redeeming an authorization code a second time, revokes the connection and every token it holds
  and answers `invalid_grant` (RFC 6749 §4.1.2; the specification requires rotation for public
  clients).

    Every token is bound to `<issuer>/mcp` and accepted only at `/mcp`. On `/v1` and `/v2` a
    token is not a credential: it matches no key and answers `auth.invalid` with `reason:
    unknown`, as any other non-key does, so the ring routes do not change. Tokens are read from the
    `Authorization` header only; the form-body and query methods of RFC 6750 §2.2 and §2.3 are not
    served. Opaque tokens are chosen because OML is both the authorization server and the resource,
    so a self-contained token saves no lookup it needs, revocation must bite at the next request
    (d11), and a JSON Web Token would carry the tenant and key id in the clear, where the key
    format rule says a credential encodes nothing. The cost: under strict rotation a refresh whose
    answer is lost, so that the client retries with the old token, ends the connection, and the
    person reconnects with a new code. How often that happens with Claude is unmeasured (FR-34).

- **d11 — A connection's token is exactly its key: every request re-resolves it to the key and
  applies every rule to that key, and nothing a token carries can widen it.** At `/mcp` an access
  token resolves to its connection, which must be open, and to the connection's key, which must be
  neither revoked nor expired, and from there to the same caller a key value resolves to. R1–R3,
  the verb ceiling, grant admissibility, the refusal order, grants, denials and the permission
  epoch then apply exactly as for the key, on every request, with no cache. The scope adds and
  narrows nothing; because the token reaches only `/mcp`, it reaches only the agent-safe subset, so
  a connection can do at most what its key can and never more. The facade never forwards a token to
  a ring route: it dispatches its ring call under the key it already resolved, as the specification
  forbids token passthrough.

    `POST /v2/keys/{key_id}/connections/{connection_id}/revoke` (`oml keys disconnect <key-id>
    <connection-id>`) ends a connection: the lead refuses every token of it at the next request,
    and a follower after replication lag, as for a revoked key. A revoked or expired key refuses
    every token of every connection at the next request with no event of its own, because each
    lookup reads the key's state and `key.revoked` already records when. A revoked connection
    never reopens; the key needs a new code. The cost: one more lookup per call than a key, for the
    connection — unmeasured, counted against [FR-25](../product/roadmap.md).

- **d12 — The journal records a connect code's issuance, a connection's opening and its
  revocation, and nothing about refreshes.** Three events join the keys and grants family —
  `connection.code_issued`, `connection.opened` and `connection.revoked` — and `key.issued` gains
  `credential` (`bearer` or `connection`); their payloads are declared on the
  [event catalog](../data/event-catalog.md#connectioncode_issued). `connection.opened` commits at
  the token exchange, when tokens first exist, and carries the client id, the digest of its
  metadata and the redirect URI, so the admin learns which client used a code; when it replaces a
  live connection it names it, and that revocation needs no second event. Not journalled: issuing
  an access token and refreshing — they change no authority, and one event per hour per connection
  would grow the chain with events that decide nothing and tie a 30-second refresh budget to the
  per-tenant chain lock; authorization requests, wrong code entries, client registrations and
  document fetches — unauthenticated input, which would otherwise let anyone grow the signed
  chain, the reasoning that keeps a scan veto out of it; and a code spent with no token exchange,
  which `connect_codes` shows and from which no authority came. A reuse revocation is triggered by
  a token request, but only a holder of a past token can trigger it, and only once per connection.

- **d13 — The `oauth` family joins the closed taxonomy; the OAuth endpoints carry its codes inside
  their RFCs' error shapes, and the JSON surfaces carry them in the envelope.** Nine codes:
  `oauth.client_invalid`, `oauth.redirect_refused`, `oauth.pkce_required`,
  `oauth.resource_mismatch`, `oauth.authorization_expired`, `oauth.connect_code_invalid`,
  `oauth.authorization_code_invalid`, `oauth.refresh_invalid` (each `400`) and
  `oauth.connect_refused` (`422`, on `/v2`). At `/oauth/token` and `/oauth/register` the body is
  `{error, error_description}`: `error` is the RFC code a client branches on, and
  `error_description` begins with the OML code and its rule. At `/oauth/authorize` a refusal
  redirects with `error`, `error_description` and `state`, or, when the client or the redirect URI
  is the problem, is a `text/html` page that names the code. Malformed parameters are
  `request.validation`, carried as `invalid_request` or the RFC's own name for that parameter's
  fault. `/mcp` keeps `auth.missing` and `auth.invalid` with the challenge of d2. The codes are
  declared on [errors](../api/errors.md#oauth-connections) and mirrored in the `ErrorCode` enum.

    OML codes exist on these endpoints because the taxonomy is closed — every refusal the service
    makes is one of its codes — and an operator joining a log line to a doctor count needs one
    name; the RFC shape stays because Claude reads `invalid_grant` to learn that a refresh failed.
    The cost: an OAuth client sees the OML code only as text.

- **d14 — Operations: one boot key and three journalled lifetimes, a doctor check run from
  outside, a proxy that only routes, and every OAuth write on the lead.**

    - **Configuration** ([configuration](../operations/configuration.md)): `oauth.issuer` is
      boot configuration, unset by default; `oauth.access_token_ttl`, `oauth.refresh_token_ttl` and
      `oauth.connect_code_ttl` are journalled settings. The other bounds — the 10-minute
      authorization request, the 60-second authorization code, five attempts per page, the fetch
      bounds of d5, the 10 000 unused registrations — are constants of this design, changed by a
      design change.
    - **Doctor** ([observability](../operations/observability.md#the-doctor-contract)):
      `oauth_discovery` — the CLI fetches both discovery documents from the issuer URL, from
      wherever it runs, and checks that `resource` is `<issuer>/mcp` and `issuer` is the issuer, so
      a proxy that does not route the paths shows as `failing` with the URL; `connect_codes` — live
      codes, codes that lapsed unredeemed, and wrong entries in the window; `connections` — open
      connections, and those revoked for reuse in the window, which is `attention` because it means
      a stolen token or a client that lost a refresh answer.
    - **Proxies.** A reverse proxy in front of `oml-server`, the hosted deployment's Caddy
      included, needs nothing beyond forwarding `/mcp`, `/.well-known/oauth-protected-resource/mcp`,
      `/.well-known/oauth-authorization-server` and `/oauth/*` unchanged, with no path rewriting and
      no caching. The page's headers are `oml-server`'s own (d9), so the proxy adds none.
    - **Egress and addresses.** The hosted apps' calls come from `160.79.104.0/21`; a firewall
      that admits by address admits that range on `/mcp`, the two discovery documents,
      `/oauth/token` and `/oauth/register`, while `/oauth/authorize` is reached by the person's
      browser. No limit keys on the source address: behind a proxy it is the proxy's, OML has no
      trusted-forwarding rule, and all hosted-app token calls share one range anyway.
    - **Cluster mode.** The OAuth endpoints write, so a follower forwards them to the lead; a
      follower validates access tokens from its replica, with the revocation lag a revoked key has
      there ([workareas](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)).

- **d15 — `info.version` moves from `0.2.0-design` to `0.3.0-design`.** The changelog's rule names
  the error taxonomy among the provider ring's contract, and the `ErrorCode` enum that `/v1`
  responses share gains the `oauth` family; a change carrying a proposed record that changes the
  OpenAPI document bumps the version (`CONTRIBUTING.md`, "Spec versioning and tags").
  No `/v1` operation changes: no path, request or response shape, idempotency or pagination rule,
  and no `/v1` operation answers an `oauth` code. The proposed ADR-0026, on another branch, moves
  to `0.3.0-design` as well: if its revision is still untagged when this record lands, the two
  share `0.3.0-design`; if `spec-v0.3` has been cut, this record takes `0.4.0-design` when it is
  rebased.

- **d16 — The build follows iteration 3; the pure modules may land before it.** What touches key
  issuance — the `POST /v2/keys` body, `issue-agent`'s arguments, `api_keys` and the migration
  sequence, the page's description of the key — waits for names, which change all of them. The
  pure modules — the connect code's format and normalization, PKCE verification, the redirect
  matcher, document and registration validation, the discovery documents and the challenge, the
  refresh state machine, the page's rendering and headers — depend on none of that and may land
  earlier, behind no route.

## Consequences

**Pages this record changes, each the single declaration site of its part.**

- [HTTP API](../api/http-api.md): `POST /mcp` on the surface; the section
  [Connections: OAuth for hosted MCP clients](../api/http-api.md#connections-oauth-for-hosted-mcp-clients)
  (the challenge, the two documents, the three endpoints, the page, proxies);
  `POST /v2/keys/{key_id}/connect-codes` and
  `POST /v2/keys/{key_id}/connections/{connection_id}/revoke` in the keys table; the content-type
  convention's exceptions. [openapi.yaml](../api/openapi.yaml) mirrors all of it, with the
  `WWW-Authenticate` header on the `/mcp` `401`.
- [Errors](../api/errors.md): the `oauth` family with its RFC mapping, the `auth` rows' challenge
  note, the status index.
- [Roles and keys](../architecture/roles-and-keys.md#connections): connector keys, the connect
  code, the connection lifecycle, tokens and the token-is-its-key rule; the matrix row; the key
  format names the two token prefixes.
- [Agent plugin](../api/mcp-plugin.md): authentication by key or connection, and the claude.ai
  path in "Wiring it into your agent".
- [Security](../architecture/security.md#connections-the-oauth-surface): the threat-model delta
  and residual risk 9; the custody map and the trust-boundary diagram.
- [Configuration](../operations/configuration.md), [observability](../operations/observability.md),
  [event catalog](../data/event-catalog.md), [Postgres schema](../data/postgres-schema.md) (four
  tables, `api_keys.hash` nullable), [CLI reference](../cli/reference.md) (`--connect-code`,
  `oml keys connect-code`, `oml keys disconnect`, connections in `oml keys show`).
- The [roadmap](../product/roadmap.md): RS-2 restated in ADR-0018 d3's form, its exit criterion met
  by this record and the row leaving the register when the record is accepted; FR-34 opened.
- `README.md`: how to add OML on claude.ai and use it from a phone, marked decided, not built.

**Opened.** [FR-34](../product/roadmap.md): the connection flow is designed, not built, and no live
connection has run. It closes on the end-to-end test of the in-process lead passing in CI and a
dated live connection from a claude.ai custom connector used from a Claude mobile app, with the
token endpoint's latency measured against Claude's 10-second and 30-second limits; it also holds
what the sources do not say — Claude Code's registration method and loopback form, Claude's
behavior after a failed document fetch, `form-action` on redirects across browsers, and how often
strict rotation ends a connection.

**Costs carried here.** An OAuth implementation, an HTML page and an outbound fetch inside the
lead; a second error shape on one listener; one more lookup per `/mcp` call for a connection; a
lost refresh answer ends a connection; a person who cancels or lets a code lapse needs a new one
from the admin; a bearer client refused at `/mcp` now sees a sign-in pointer. None is measured.

**On acceptance, and not before.** This record's Status line and frontmatter `status` become
`accepted design` with the date, and its register row follows; the Status lines of ADR-0010,
ADR-0014, ADR-0015, ADR-0018 and ADR-0028 gain "· amended by ADR-0031"; RS-2 leaves the register;
[commitments](../product/commitments.md) drops RS-2 from its open questions and lists FR-34; the
`spec-vX.Y` tag of d15 is cut. With the proposal, the record registers at its three points, the
D9, D13 and D14 cells of the D-register name it, `llms.txt` names it, and one
[changelog](../changelog.md) entry records it.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — A third-party identity provider as the authorization server** | The code-to-key binding needs OML's key store, and the provider would enter every connection's path and availability budget — ADR-0010's objection. |
| **d1 — The static bearer header claude.ai offers in beta** | Limited to some organizations, entered by an Owner and shared by every member: one key behind many agents. |
| **d1 — OAuth on by default** | The documents would advertise whatever origin a request named, and a laptop install would grow an unauthenticated surface it does not use. |
| **d2 — The challenge on every route's `401`** | Not this record's to change: `/v1` and `/v2` take keys only, and a pointer there would invite an OAuth flow no ring route accepts. |
| **d2 — The challenge on a `200` as well** | Claude does not read it there. |
| **d3 — Serve the `/mcp` document at the root path too** | RFC 9728 §3.3 ties `resource` to the URL the document was fetched from; the root can only describe the origin, which Claude rejects. |
| **d3 — List a second authorization server** | Claude uses the first and never falls back, so a second entry is never used. |
| **d4 — Endpoints under `/v2/oauth/`** | Their shapes are the RFCs', not the ring's, and a `/v3` re-version would move URLs that connected clients stored. |
| **d4 — Scopes per tool or per verb** | They would duplicate the verb ceiling with a second vocabulary that can disagree with it; the key's role and grants already decide. |
| **d4 — No scope at all** | The specification's scope selection reads `scope` from the challenge or `scopes_supported`; one declared scope gives every client the same answer. |
| **d5 — Client ID metadata documents only** | Clients on the two earlier revisions Claude follows register dynamically. |
| **d5 — Dynamic registration only** | A registered name is self-asserted, so the page could never name a verified client, and claude.ai uses documents when they are advertised. |
| **d5 — Pre-registered confidential clients** | A secret typed into claude.ai would be a second shared secret per connector. |
| **d5 — An allowlist of document hosts** | The host serving Claude's document is not in the cited sources, so the shipped list would be a guess; the redirect allowlist bounds where codes go. |
| **d6 — A configurable redirect allowlist** | Each entry decides where an authorization code may go; that is a decision to record, not a value to set. |
| **d6 — Refuse `localhost`** | Claude Code's loopback form is not in the sources; refusing it could leave Claude Code with no connection. |
| **d7 — The maintainer's rejected reading: paste the key value on OML's page** | The value would pass through the person's clipboard and browser. |
| **d7 — Print the key value beside the code** | A second, unneeded credential for the same key, resting on a terminal. |
| **d7 — Codes for bearer keys too** | One key would act through two credential paths, and the key id on a journal event could no longer say which acted without changing the signed envelope. |
| **d7 — Several live connections per key** | Several clients would share one identity, against one key per agent. |
| **d7 — Connect codes for admin keys** | `/mcp` serves no admin verb, so the connection could do nothing an admin key exists for, and the page would show an admin identity. |
| **d8 — The code's digest in the journal** | The journal is kept forever and exported; a 60-bit digest there is a brute-force target that outlives the code by years. |
| **d8 — A link that carries the code** | A secret in a URL lands in history, logs and referrers. |
| **d8 — A longer code, or words** | Twelve symbols already put guessing at L·G / 2⁶⁰; every added symbol is one more for a person to type. |
| **d8 — Different messages for an expired, spent or unknown code** | It would confirm to a guesser that a code existed. |
| **d9 — Open the connection as soon as the code is right, with no confirmation** | The person would never see which agent and which workarea the connection acts as. |
| **d9 — Ask for the code with no prior authorization request** | The code would arrive with no client, redirect or PKCE challenge bound to it. |
| **d9 — A session cookie and a CSRF token** | A cookie is an ambient credential to protect; with none, a forged form has nothing to ride. |
| **d9 — Require `state`** | PKCE already closes the client's exposure, and refusing a conforming client that omits it buys OML nothing. |
| **d9 — Per-address throttling of wrong codes** | Behind a proxy the address is the proxy's, and OML has no trusted-forwarding rule; the entropy bound holds without it. |
| **d10 — JSON Web Tokens** | The resource is also the issuer, so self-containment saves nothing; revocation would wait for expiry, and the token would carry tenant and key id in the clear. |
| **d10 — A grace window for a rotated refresh token** | A stolen refresh token raced inside the window would pass reuse detection. |
| **d10 — Long-lived access tokens with no refresh** | A stolen token would be the key's full capability for its whole life; the specification asks for short-lived access tokens. |
| **d11 — Let a token reach `/v1` and `/v2`** | A connection would become a general credential for the lab ring's verbs, beyond what the hosted apps need, and the facade boundary would stop bounding it. |
| **d11 — Revoke a key's connections by event when the key is revoked** | The lookup already reads the key's state on every request; extra events would record nothing `key.revoked` does not. |
| **d12 — Journal every refresh** | It decides nothing and would tie refresh latency to the per-tenant chain lock. |
| **d12 — Journal client registrations and document fetches** | Unauthenticated input would grow the signed chain. |
| **d13 — The OML envelope on the OAuth endpoints** | OAuth clients parse `error`; Claude reads `invalid_grant` on a failed refresh. |
| **d13 — RFC codes only, with no OML codes** | The taxonomy would stop being closed, and logs and doctor would have no stable name for these refusals. |
| **d14 — Every limit a setting** | A value with one caller is a constant, and these bounds are security properties a deployment should not loosen by editing a file. |
| **d15 — Leave `info.version` at `0.2.0-design`** | The shared error taxonomy changes, which the changelog's rule names, after `0.2.0-design` was tagged. |
| **d16 — Build now, before names** | Issuance, `issue-agent` and the key tables all change again when names land. |
