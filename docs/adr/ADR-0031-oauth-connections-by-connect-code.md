<!--
id: adr/ADR-0031-oauth-connections-by-connect-code
tier: engineering
status: proposed
answers: how Claude's hosted apps reach the agent plugin through a claude.ai custom connector; OML as an OAuth 2.1 protected resource with its own authorization server; the 401 challenge and the discovery documents; client registration by client ID metadata document with dynamic registration as the fallback, and the redirect allowlist; the connector key and its one-time connect code; the authorization page; tokens, rotation and reuse detection; why a token is never more than its key; the connection journal events, the oauth error family, operations, the version move and the build order
-->

# ADR-0031 — Claude's hosted apps reach the agent plugin over OAuth, through a connection that a one-time connect code binds to one agent key

**Status:** proposed · 2026-10-31

**Amends:** ADR-0010 (d3, d7), ADR-0014 (d1, d2, d3, d4), ADR-0015 (d1), ADR-0018 (d4), ADR-0025 (d2, d15), ADR-0028 (d1)

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
public clients, **MUST** rotate refresh tokens. The client ID metadata document draft
(draft-ietf-oauth-client-id-metadata-document-02 §3, fetched 2026-10-31) requires a client_id URL
with the `https` scheme and a path, and with no userinfo, no
fragment and no single-dot or double-dot path segment, and says it SHOULD NOT carry a query. The
OAuth security best current practice (RFC 9700 §4.4.2, fetched 2026-10-31) answers mix-up attacks,
in which a client that talks to several authorization servers sends one server's authorization
code to another, with two defenses: issuer identification (RFC 9207, whose §2 puts `iss` on every
authorization response, error responses included) or a distinct redirect URI per authorization
server. A client that uses one callback for every server, as Claude's hosted apps do (below), can
use only the first, and only if the authorization server sends `iss`.

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
- The hosted apps redirect to exactly `https://claude.ai/api/mcp/auth_callback`.
- No secret travels in a URL.

**The maintainer's three decisions, which this record designs around and does not reopen.**
(1) OML becomes an OAuth 2.1 protected MCP resource with its own authorization server, served by
`oml-server` on the same origin, and bearer keys keep working unchanged. (2) The binding is a
one-time connect code: the admin issues the agent key and a connect code together, building on
[ADR-0028](ADR-0028-agent-key-issuance.md)'s `issue-agent`; the authorization page asks for the
code and, on a correct one, opens the connection bound to that key, so claude.ai holds only
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
  Listing a key's connections and revoking one (d11) are served whether or not `oauth.issuer` is
  set, so an admin can clean up while the surface is off. Every connection records the resource it
  was opened for, `<issuer>/mcp` at that moment, and a token or refresh token whose connection's
  resource is not the current `<issuer>/mcp` is refused (d10, d11). Changing `oauth.issuer` to
  another origin, or unsetting it, therefore stops every connection at its next request, and no
  token works anywhere but at the resource it was issued for. The stop is not an end in the
  journal: no event records it, because no authority changed, and a connection whose refresh token
  is still unexpired when the old origin is set again works again, unless a newer connection of its
  key opened in between and replaced it (d12). A connection whose refresh token lapsed in between is
  dead for good (d11). An admin who wants connections ended revokes them, and revocation works while
  the issuer is unset.

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
  RFC 9110 §15.5.2 requires of every `401`; that nonconformance is
  [FR-35](../product/roadmap.md), opened by this record, and correcting it is not decided here.
  The cost: a
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
  `["none"]`, `scopes_supported` `["mcp"]`, `client_id_metadata_document_supported: true` and
  `authorization_response_iss_parameter_supported: true` (RFC 9207 §3), because every
  authorization response carries `iss` (d9, d13). No OpenID Connect discovery document is served:
  OML issues no ID token. Both documents are built from `oauth.issuer` alone, never from the
  request's `Host` header, so a forged host cannot point a client at another server.

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
  "Bearer", expires_in, refresh_token, scope: "mcp"}` with `Cache-Control: no-store`. An authorize
  request's parameters are bounded before anything else is checked or fetched: `state` at most 512
  bytes, `client_id` at most 2 048 bytes, `redirect_uri` at most 2 048 bytes, a
  `code_challenge` that is present exactly 43 base64url characters (`^[A-Za-z0-9_-]{43}$`, the
  length of an S256 challenge), and no parameter sent more than once (RFC 6749 §3.1). A request
  past a bound, a repeated parameter and a present `code_challenge` that does not match that
  pattern included, is `request.validation`, answered on the page and never redirected, since
  neither its redirect URI nor its `state` has been accepted. At `/oauth/token` a parameter sent
  more than once is `request.validation` as `invalid_request` (RFC 6749 §3.2, §5.2). An absent
  `code_challenge`, or a `code_challenge_method` that is absent (which RFC 7636 §4.3 reads as
  `plain`) or not `S256`, is `oauth.pkce_required`, redirected after the client and the redirect
  URI are accepted.

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
      document URL. Its form is checked first, more strictly than the draft asks: no userinfo, no
      fragment, no query, no `.` or `..` path segment, no percent-encoded `/` or `.`, no port but
      the default, a host that is a domain name and never an IP literal, so that every address
      fetched is a resolver answer the allow-list below checks, and the string must be exactly its
      own WHATWG URL serialization, so that one client has one spelling; anything else is
      `oauth.client_invalid` (`reason: client_id_form`). At the authorize request, before
      anything is shown, `oml-server` fetches it under fixed bounds. The addresses are an
      allow-list: an AAAA answer is admitted only inside `2000::/3`, the global unicast space, and
      never inside `2001::/23`, `2001:db8::/32`, `2002::/16` or `3fff::/20`, the blocks inside it
      that the IANA IPv6 Special-Purpose Address Registry (fetched 2026-10-31, last updated
      2025-10-09) marks not globally reachable or that
      embed an IPv4 address; an A answer is admitted only when it is unicast, outside
      `224.0.0.0/4`, and inside no block the IANA IPv4 Special-Purpose Address Registry marks
      "Globally Reachable: False" (fetched 2026-10-31, last updated 2025-10-09), and outside the
      deprecated 6to4 relay block `192.88.99.0/24`. As fetched, those blocks are `0.0.0.0/8`,
      `10.0.0.0/8`, `100.64.0.0/10`, `127.0.0.0/8`, `169.254.0.0/16`, `172.16.0.0/12`,
      `192.0.0.0/24`, `192.0.2.0/24`, `192.168.0.0/16`, `198.18.0.0/15`, `198.51.100.0/24`,
      `203.0.113.0/24`, `240.0.0.0/4` and `255.255.255.255/32`. Every IPv6 form that embeds an IPv4
      address — `::/96`, `::ffff:0:0/96`, `::ffff:0:0:0/96`, `64:ff9b::/96`, `64:ff9b:1::/48`,
      `2002::/16`, Teredo's `2001::/32` — falls outside what is admitted, so it is refused by
      construction and never unwrapped; so are `100::/64`, `5f00::/16`, `fc00::/7`, `fec0::/10`,
      `fe80::/10` and `ff00::/8`, all outside `2000::/3`. A network-specific NAT64 prefix (RFC 6052)
      on the lead's own network is inside `2000::/3` and cannot be told apart from a public
      address; the deployment's egress policy refuses it
      ([security](../architecture/security.md#connections-the-oauth-surface)). The TCP connection
      is made to the address that was checked, with no second resolution; the HTTP client ignores
      proxy environment variables, since a proxy would resolve the name again; TLS verifies the
      certificate against the web PKI for the URL's host, sent as SNI; no redirect is followed; the
      request sends `Accept-Encoding: identity`, and the fetch is cut at 5 seconds and at 16 KiB of
      bytes received; the body must be JSON. At most 16 fetches run at once on the lead, at most
      one per host name and at most one per resolved address, so many names pointing at one slow
      address hold one slot; a request past any cap, with no cached copy to serve, is
      `oauth.client_invalid` (`reason: fetch_busy`). The document's `client_id` must equal the URL
      exactly, `client_name` and `redirect_uris` must be present, `token_endpoint_auth_method` must
      be absent or `none`, and every redirect URI must be inside the allowlist of d6.
    - **The document cache.** A fetched document is cached as its `Cache-Control` allows, and kept
      at most 24 hours from its fetch; within those 24 hours a copy past its `Cache-Control`
      lifetime is served when a fresh fetch fails or a fetch cap is full. At most 10 000 documents
      are cached at once. A full cache never refuses: it evicts the least recently used document
      whose client has not connected, and when every cached document's client has connected, the
      fetched document serves its request uncached. A client has connected once a Connect on the
      page names it (d9): Connect stamps `oauth_clients.connected_at`, a column of the provider
      tenant's own table, in the transaction that creates the connection, so the cache and the
      removal of registrations read that column and never a tenant's connections. Nothing after
      the authorize request reads the cache — the authorization request carries what the page and
      the token exchange need (d9) — so evicting a document never affects a page in progress or a
      connection.
    - **Dynamic client registration.** `POST /oauth/register` admits `redirect_uris` (each inside
      the allowlist), `client_name`, `token_endpoint_auth_method: "none"`, `grant_types` within
      `["authorization_code", "refresh_token"]` and `response_types` `["code"]`, and answers `201`
      with a server-minted `client_id`, no secret and no registration access token (RFC 7592
      client management is not served). `client_name` is required, because the page shows it; its
      absence is `request.validation` as `invalid_client_metadata`. An absent
      `token_endpoint_auth_method`, which RFC 7591 §2 reads as `client_secret_basic`, is registered
      as `none` and echoed as `none`; any other value is `request.validation` as
      `invalid_client_metadata`. A registered client that has not connected within 24 hours of its
      registration is removed. At most 10 000 such unused registrations exist at once; past that,
      registration answers `limit.rate` until some lapse.
    - **The token endpoint identifies the client by the connection.** At `POST /oauth/token` the
      client is the one stored on the connection the code or the refresh token belongs to
      (`oauth_connections.client_id`), and the client tables are never read there, so a
      metadata-document client whose cached document lapsed after 24 hours still refreshes for
      the refresh token's whole life. A `client_id` that differs from the connection's is
      `invalid_grant` (`oauth.authorization_code_invalid` or `oauth.refresh_invalid`); an unknown
      code or refresh token is `invalid_grant` too, whatever its `client_id`. `invalid_client`
      (`oauth.client_invalid`) answers only a request that sends client authentication, since every
      client is public and none holds a secret: a request that sends an `Authorization` header
      gets `401` with `WWW-Authenticate` naming the scheme it used, for example `Basic
      realm="oml"` (RFC 6749 §5.2), and one that sends a `client_secret` in the body gets `400`.
      The token endpoint never fetches: its time budget (10 seconds for Claude) is spent on
      OML's own lookups only.
    - **Neither path journals anything** (d12).

    What a flood stalls: a flood of registrations stalls new dynamic registrations only. A flood of
    throwaway document URLs only churns the cache, since a full cache evicts and never refuses, and
    the document of a client that has connected is never evicted. A flood of slow document
    hosts on distinct addresses can hold the 16 fetch slots and stall every document client the
    cache holds no copy of, for as long as it lasts; a cached copy, and every open connection, is
    unaffected, and the residual is carried in
    [security](../architecture/security.md#connections-the-oauth-surface).

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

- **d6 — A redirect URI is admitted only when it is exactly
  `https://claude.ai/api/mcp/auth_callback`.** Matching is byte for byte, on the parameter's value
  after exactly one `application/x-www-form-urlencoded` decoding (in an authorize query or a token
  body), or on the JSON string value (in a registration or a document), and never after URL
  parsing or any further decoding. No other URI, no custom scheme, no wildcard and no fragment is
  admitted, at registration, in a fetched document or in an authorize request. An authorize
  request's `redirect_uri` must be exactly one of the client's registered URIs; a client with
  exactly one may omit it, and the token
  request then follows the rule the [HTTP API](../api/http-api.md#connections-oauth-for-hosted-mcp-clients)
  declares for `redirect_uri`. When the client or the
  redirect URI is refused, the page shows the refusal and never redirects (RFC 6749 §4.1.2.1).

    The list is fixed, not a setting, because each entry decides where an authorization code may
    be delivered: admitting another assistant's connector, or a loopback redirect, is a recorded
    decision, not a configuration change. The cost is that a client returning to any other
    address cannot connect; Claude Code keeps its key in the header
    ([agent plugin](../api/mcp-plugin.md)).

- **d7 — A connector key is an agent key issued with a connect code instead of a value; it has no
  bearer value and holds at most one open connection.** `POST /v2/keys` with `connect_code: true`
  runs the [issuance order](../architecture/roles-and-keys.md#grant-admissibility-and-refusal-order)
  unchanged and then refuses, with `oauth.connect_refused`, a key role of `admin` or `operator`
  (`details.reason: role`), `held_by: service` (`held_by_service`) and a deployment without
  `oauth.issuer` (`not_configured`). The server mints no value for the key: `api_keys.credential`
  is `connection`, folded from `key.issued`, `api_keys.hash` is null, and the `201` carries
  `connect_code` — `{code_id, code, expires_at, connector_url}` — in place of `value`, returned
  only after the last `grant.issued` and the `connection.code_issued` commit. The key
  authenticates nothing until a connection opens, and then only at `/mcp`. This amends ADR-0025
  d2: the issuance body gains `connect_code`, the issuance order gains its seventh step,
  `key.issued` gains `credential`, and "the value only after the last grant commits" gains its
  connect-code form.

    `oml keys issue-agent --principal crew/claude-mobile --workarea <workarea-id> --connect-code`
    sends that issuance and prints exactly two lines on stdout, the connector URL and the code; the
    key id, the expiry and the next step go to stderr. `--out` with `--connect-code` is refused
    (exit `2`): there is no value to write, and a code is for a person to read and type within
    minutes. `--json` prints the `201`, as every verb does. The key's name, as a positional
    argument, arrives with names (ADR-0026 d12, proposed).

    **A replay carries no connect code.** The idempotency record of `POST /v2/keys`
    with `connect_code: true` and of `POST /v2/keys/{key_id}/connect-codes` stores its response
    without the code, and a replay returns that body with `Idempotency-Replay: true`,
    so no idempotency record holds a connect code and the first answer is the only one that
    carries it. A bearer key's replay stays as ADR-0014 d3 has it; that its stored response holds
    the value is [FR-36](../product/roadmap.md), opened by this record. This amends ADR-0014 d3
    for the two connect-code responses ("returns the stored response" returns it without the
    code) and ADR-0028 d1 (`issue-agent`'s output). On a replayed answer,
    `oml keys issue-agent --connect-code` and `oml keys connect-code` print no code, name the key
    id on stderr, say that the code went only to the first answer and that
    `oml keys connect-code <key-id>` mints a new one, and exit `2`
    ([CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation)). The cost: a
    code whose first answer is lost is minted again.

    A key is a bearer key or a connector key, never both: a connect code for a key that has a
    value answers `oauth.connect_refused` (`bearer_key`), and a connector key is not rotated —
    `POST /v2/keys/{key_id}/rotate` on one answers `422 oauth.connect_refused` (`connector_key`),
    because rotation mints a successor with a value printed once, a bearer key with the connector
    key's grants. A fresh connection is a new code (`oml keys connect-code`); a fresh identity is
    a new connector key, and the old one is revoked. With one open connection per key —
    opening one revokes the key's earlier open connection, live or not, in the same commit (d12) —
    the key id on
    every journal event names exactly one credential path, so attribution stays exact without
    any change to the signed envelope ([chain format](../data/chain-format.md)). This amends
    ADR-0010 d3 (key values are shown once and stored only as hashes: a connector key has neither
    value nor hash), and ADR-0015 d1 as ADR-0028 d1 amended it (a connector key's value is never
    shown because none exists; its connect code is shown exactly once; `--connect-code` prints
    its own two lines). It amends ADR-0018 d4 with three vocabulary rows:

    | Term | Means | Never |
    |---|---|---|
    | connection | one OAuth client's standing authority to act as one connector key at `/mcp`, opened at the token exchange after a connect code is redeemed, and ended by revocation | "grant", "OAuth grant", "session" or "authorization" as this object's name (a grant is a workarea grant); bare "connection" for a claim↔claim or workarea↔workarea link, which is always "connection edge" or "connection proposal" — the wire value `kind: "connection"` is code |
    | connect code | the short, single-use secret, alive for minutes, that binds one connection to one connector key | "pairing code", "PIN", "OTP" |
    | access token, refresh token | a connection's two OAuth credentials, `oml_at_…` and `oml_rt_…`, stored as digests | "key" for either; "token" alone for an API key or a budget token |

    The cost: one agent identity cannot act both from claude.ai and from a header-configured
    client such as Claude Code; each gets its own key and principal, which is what one key per
    agent identity already asks for. A code that lapses before use needs a new one (d8). And no
    file's bytes travel through an upload session under a connection: `oml_attach` `open` is
    refused there (d11), because an upload's parts travel on `/v1` under a key value, which a
    connector key does not have. The proposed ADR-0035, on another branch, gives a connection
    small text files inline on `oml_remember` instead.

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
    one live code: minting one voids every unredeemed code of that key in the same commit, and
    `key.revoked` voids them in its own commit (`voided_seq` is that seq). Minting a code, or
    revoking a connection of the key (`connection.revoked`, whatever its cause), also ends every
    live authorization request holding a code of that key and every `authorized` connection of
    it, in the same commit: it voids every code of the key from which no connection has opened,
    spent or not, and code entry, Connect and the token exchange refuse a voided code, so a later
    Connect answers `oauth.authorization_expired` and a later exchange `invalid_grant`
    (`oauth.authorization_code_invalid`). The admin's usual reason to mint again is a code someone
    else may have entered; a page that holds that code, or the authorization code it
    produced, then cannot open a connection that would replace the one the admin meant. Code entry
    reads the key's state as `/mcp` does (d11): the code of a revoked or expired key answers the one
    message for a code that is not valid, `oauth.connect_code_invalid`, so the page never shows a
    dead key's grants. `POST /v2/keys/{key_id}/connect-codes` (`oml keys connect-code <key-id>`)
    mints a fresh code for a connector key, refused with `oauth.connect_refused` for a bearer key,
    a revoked or expired key, or a deployment without `oauth.issuer`.

    The guessing bound is the code's entropy, not a throttle: with L live codes and G wrong
    entries made during their lifetime, the chance that any entry hits one is at most L·G / 2⁶⁰ —
    a formula, not a measurement. L counts every tenant's live codes, because the code is looked
    up before any tenant is known, and G is bounded only by the request rate the deployment
    serves, because an authorization request costs one unauthenticated `GET` to open. d9's limit
    of five entries per page bounds one page's typing errors, not guessing; doctor shows the
    wrong entries an operator should look at (d14). The cost: a code is a bearer secret for its
    lifetime, so whoever enters it first connects. The admin sees that a connection opened and
    when (`connection.opened`), and can revoke it; for claude.ai every connection names the same
    client and redirect URI, so the event cannot tell whose claude.ai account holds it.

- **d9 — The authorization page asks for the code, then shows what the connection will act as
  before it opens; it sets no cookie and acts only on its own form.** The page's sentences below
  are declared, word for word, on the
  [HTTP API](../api/http-api.md#connections-oauth-for-hosted-mcp-clients); this item records the
  decision.

    1. **The request.** `GET /oauth/authorize` checks the parameter bounds (d4), then the client
       (d5), the redirect URI (d6), `response_type=code`, an S256 `code_challenge`, `resource` and
       `scope` (d4). A refusal of a bound, the client or the redirect URI is a page and never a
       redirect; any later refusal redirects with the RFC error, the `state` and `iss` (d13). A
       valid request becomes an authorization request, alive 10 minutes, and writes no row: its
       handle, which the form carries, is the request itself, sealed by an AEAD under a key the
       lead generates when it starts and holds only in memory — the `client_id`, the client's kind,
       `client_name`, document host and metadata digest, the `redirect_uri` and whether it was
       sent, the `code_challenge`, the `state`, the resource, the expiry and the attempts left. A
       handle that does not open under the lead's key, or whose expiry has passed, names no live
       request; a restart or a takeover therefore ends every page in progress, which starts again.
       A row is written only when a correct code is entered (step 2), which only an admin-minted
       code can cause, so unauthenticated page opens write no authorization-request row; the only
       table they write is `oauth_clients`, as the document cache of d5, capped at 10 000 documents.
       The page shows the client — a metadata-document client as its `client_name`
       "verified by `<host>`", a registered one as its `client_name` marked "name not verified" —
       and the redirect URI's host. The host after
       "verified by" is the document URL's whole host, in lower-case ASCII (the punycode A-label
       form for an internationalized name), never shortened, and rendered apart from
       `client_name`, so neither a lookalike Unicode name nor a crafted `client_name` can pass for
       another host. Above the code field it says: "Enter the code only if
       you opened this page yourself, by adding the connector in Claude, in the last few minutes.
       If someone sent you this link, close it." Then it asks for the code.
    2. **The code.** `POST /oauth/authorize` with exactly `handle` and `connect_code` and a correct
       code of a key that is neither revoked nor expired (d8) spends the code, writes the
       authorization request's row with the code's id, key and tenant and the sealed fields,
       under a fresh 256-bit handle the confirmation's form carries, and shows what the
       connection will act as: the tenant the key belongs to, by the name the operator gave it
       at `POST /v2/tenants`, which no tenant admin can set; the key's principal and role; every
       workarea it holds a grant on, with the grant's
       role; and sentences built from the key's role and effective access, one per kind of verb
       it holds — "Claude can remember and learn as this agent in `<workareas>`." where it holds
       writer, "Claude can recall and read as this agent in `<workareas>`." where it holds reader,
       and "Claude can propose edits and edges as this agent in `<workareas>`." where it holds
       dreamer(propose). Where it holds writer or dreamer(propose), the page adds: "Everything it
       writes is held until someone other than its author reviews it." Where a granted workarea's
       tenant policy permits `self_signed` rows, it adds, for each such workarea: "In `<workarea>`,
       its writes may be self-signed and readable there without review." Last, always: "It cannot
       approve, demote, delete or administer anything." The buttons are Connect and Cancel. A wrong
       code answers the same form with one sentence for an unknown, expired, voided, spent, or revoked
       or expired key's code alike — "This code is not valid. Check it, or ask your administrator for a new one." —
       and the attempts left, under a new sealed handle with one attempt fewer. The fifth wrong
       entry ends the request and redirects with `access_denied`. An earlier handle sent again
       carries its earlier count, which costs nothing: the five attempts bound one page's typing
       errors, not guessing (d8). Once names exist (ADR-0026, proposed), the page names the key too.
    3. **The confirmation.** `POST /oauth/authorize` with exactly `handle` and `action`, accepted
       only when the handle names a request row, which only a verified code writes, and that
       row's code is not voided (d8). Connect creates the connection, `authorized`, and issues an
       authorization code — 256 bits, alive 60 seconds, single use —
       on the redirect, with the `state` echoed verbatim, percent-encoded as a query parameter, and
       `iss` (RFC 9207 §2); Cancel redirects with `access_denied`, the `state` and `iss`. Connect
       and Cancel each delete the authorization request's row in their own commit, so a later
       step-3 `POST` with that handle — a double click, the back button, Connect after Cancel —
       names no row and answers `oauth.authorization_expired`, and one connect code yields at most
       one connection. The connect code stays spent either way. Any other combination of
       fields — a code and an action together, an action before a verified code, a second code
       after one verified — is `request.validation` on the page.

    **Cross-site forgery.** The page sets no cookie, so a cross-site form has no ambient credential
    to ride. Each `POST` must carry the handle and either `Sec-Fetch-Site: same-origin` or, when
    that header is absent, an `Origin` equal to the issuer's; `Origin: null` is refused, as is
    anything else. A submission that fails this check is `403 oauth.origin_refused` on the page,
    never redirected, and consumes no attempt: nothing in it is read but its headers. `state` is
    echoed and not required: PKCE closes the client's own forgery exposure, and the specification
    asks clients, not servers, to check `state`. **Headers on
    every page:** `Content-Security-Policy: default-src 'none'; style-src 'sha256-…'; form-action
    'self' <redirect origin>; frame-ancestors 'none'; base-uri 'none'`, `Cache-Control: no-store`,
    `Referrer-Policy: same-origin` and `X-Content-Type-Options: nosniff`. The referrer policy is
    `same-origin`, not `no-referrer`, because under the Fetch Standard's "append a request `Origin`
    header" (fetched 2026-10-31) a browser sends `Origin: null` on a non-CORS `POST` whose referrer
    policy is `no-referrer`, which would refuse the page's own form; under `same-origin` the
    page's own `POST` carries its real origin, and the cross-origin `303` to the redirect URI still
    carries no `Referer`. The page runs no script. `form-action` names the redirect origin because
    a browser may apply it to the redirect that follows a form submission; which browsers do is
    unverified (FR-34). Client-supplied strings are HTML-escaped and shortened, and `logo_uri` is
    never loaded.

    **Not a web console.** The page is not the web console ADR-0015 declines: it administers
    nothing, shows one key's identity to the person holding its code, and keeps no session. It
    does end ADR-0015's "the threat model never has to answer for a browser origin, a session
    cookie, a CSP, or a sandboxed frame":
    [security](../architecture/security.md#connections-the-oauth-surface) now answers for a
    browser origin, a CSP and framing, and the page sets no cookie, so there is still no session
    cookie to answer for.

    The cost: two submissions where one would do, a person who cancels needs a new code, and the
    page's defense against a forwarded link is a sentence the person must read (d8).

- **d10 — Tokens are opaque 256-bit strings stored as digests; access tokens live an hour,
  refresh tokens rotate on every use, and a reused refresh token or authorization code ends the
  connection.** An access token is `oml_at_` and a refresh token `oml_rt_`, each followed by 43
  base62 characters under the generation rule of the
  [key format](../architecture/roles-and-keys.md#key-format). The second underscore keeps a
  token from ever parsing as a key, and so from matching the key pattern of the scan; the scan's
  `secret.api_token` pattern list therefore names both token formats beside the key format
  ([admission](../architecture/admission.md)), so a token pasted into a memory is vetoed like a
  key. `oauth_tokens` stores the SHA-256 of each, the same defense as a key's hash. An access
  token lives `oauth.access_token_ttl`, shipped 1 hour, settable from 5 minutes to 24 hours. Each
  refresh answers a new pair and marks the presented refresh token rotated; a refresh token unused
  for `oauth.refresh_token_ttl`, shipped 30 days, settable from 1 to 90 days and never below the
  access token's life, expires, and a connection whose newest refresh token has expired is dead
  (d11). Presenting a rotated refresh token, or redeeming an authorization code a second time,
  revokes the connection and every token it holds and answers `invalid_grant` (RFC 6749 §4.1.2;
  the specification requires rotation for public clients). The evidence for both checks outlives
  its first use: the authorization code's digest is kept after redemption, marked redeemed, until
  the connection is revoked, and a rotated refresh token's row keeps its own expiry and stays until
  that expiry or the revocation, so a reuse inside either window is always recognized.

    Rotation and redemption are each one conditional update, so two racing requests cannot both
    pass: a refresh locks its connection's row, then marks the presented refresh token rotated only
    where it is still unrotated and unexpired, and mints the new pair only when that update matched
    a row; the exchange marks the code redeemed only where it is still unredeemed. A refresh whose
    update matches nothing while the token's row shows it rotated is reuse, and the revocation then
    takes the per-tenant chain lock to commit `connection.revoked`. The statements are declared on
    the [Postgres schema](../data/postgres-schema.md#connections-adr-0031). A presented code whose
    row is already redeemed is reuse only when the request's `client_id` is the connection's and
    the S256 of its `code_verifier` is the stored `code_challenge`: only the holder of the
    verifier can redeem first, which is the case reuse detection exists for. Any other
    presentation of a redeemed code is `invalid_grant` (`oauth.authorization_code_invalid`) and
    revokes nothing, so a redeemed code read from a browser history, a request log or a proxy log
    ends no connection. An exchange that fails — a wrong verifier, client or `redirect_uri` — does
    not consume the code.

    No token, authorization code, PKCE verifier, authorization-request handle or connect code,
    nor a digest of one, appears in a log line, a metric label, an `error_description`, a
    `details` field or a journal payload; the rule is declared on
    [roles and keys](../architecture/roles-and-keys.md#connections), and its logging half on
    [observability](../operations/observability.md#the-plaintext-rule-a-correctness-rule).

    Every token is bound to its connection's resource, `<issuer>/mcp` when the connection was
    created, and is accepted only at `/mcp`, and only while that resource is the current
    `<issuer>/mcp`; a token whose connection's resource is another origin's answers `auth.invalid`
    (`reason: unknown`) at `/mcp` and `invalid_grant` (`oauth.refresh_invalid`) at refresh. On
    `/v1` and `/v2` a
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
  token resolves to its connection, which must be open and whose resource must be the current
  `<issuer>/mcp`, and to the connection's key, which must be neither revoked nor expired, and from
  there to the same caller a key value resolves to. At `/mcp` only an `oauth_tokens` row of kind
  `access` authenticates: a refresh token presented there answers `auth.invalid` (`reason:
  unknown`) and is neither rotated nor counted as reuse, so a refresh token, which lives for days,
  never serves as a bearer credential. At `/oauth/token` only a row of kind `refresh` is a
  `refresh_token`, and any other token sent as one answers `invalid_grant`
  (`oauth.refresh_invalid`). Code entry on the page (d8), the token
  exchange and every refresh read the key's state the same way: a revoked or expired key's code
  answers `oauth.connect_code_invalid`, and its authorization code or refresh token answers
  `invalid_grant` (`oauth.authorization_code_invalid`, `oauth.refresh_invalid`). The token
  exchange reads it in the transaction that commits `connection.opened`, under the per-tenant
  chain lock, so no connection opens after its key's `key.revoked`. R1–R3,
  the verb ceiling, grant admissibility, the refusal order, grants, denials and the permission
  epoch then apply exactly as for the key, on every request, with no cache. The scope adds and
  narrows nothing; because the token reaches only `/mcp`, it reaches only the agent-safe subset, so
  a connection can do at most what its key can and never more. The facade never forwards a token to
  a ring route: it dispatches its ring call under the key it already resolved, as the specification
  forbids token passthrough.

    One tool answers differently under a connection than under the key's value. `oml_attach`
    `open` is refused under a connection with `403 oauth.attach_refused`, before any upload
    session is created, so no session counts against the key's upload quota: an upload's parts
    travel on `PUT /v1/attachments/uploads/{id}/parts/{n}` under a key value, which a connector
    key does not have, and a token is never accepted on the parts route, since that would make it
    a credential beyond `/mcp`. `oml_attach` `status` answers as for the key, which has no session
    to show. Every other tool answers exactly as it would to the key
    ([agent plugin](../api/mcp-plugin.md#oml_attach)).

    `POST /v2/keys/{key_id}/connections/{connection_id}/revoke` (`oml keys disconnect <key-id>
    <connection-id>`) ends a connection: the lead refuses every token of it at the next request,
    and a follower after replication lag, as for a revoked key. The same commit voids the key's
    codes from which no connection has opened, ending every page in progress and every
    `authorized` connection of the key (d8). It names an open connection: an `authorized` one is
    not journalled, is not listed and cannot be named, so the route answers `access.not_found` for
    it as for an unknown id, and an admin who wants one stopped mints a code or revokes the key,
    either of which ends it. A revoked or expired key refuses
    every token of every connection at the next request with no event of its own, because each
    lookup reads the key's state and `key.revoked` already records when. A revoked connection
    never reopens; the key needs a new code. Revocation and the listing of a key's connections are
    served whether or not `oauth.issuer` is set (d1).

    A connection is **live** while it is open, its key is neither revoked nor expired, its newest
    refresh token is unexpired, and its resource is the current `<issuer>/mcp`. An open connection
    that is not live stays `open` in its table, with no event, because nothing an actor did ended
    it. One whose newest refresh token lapsed, or whose key ended, is dead for good: its next
    refresh answers `oauth.refresh_invalid`, and a new code's connection replaces it. One whose
    resource is no longer current works again if that origin is set again before its refresh token
    lapses (d1). An open connection that is not live is still replaced, and so never revives, once
    a newer connection of its key opens (d12). Doctor counts live connections and reports the
    others apart (d14). The cost: one
    more lookup per call than a key, for the connection — unmeasured, counted against
    [FR-25](../product/roadmap.md).

- **d12 — The journal records a connect code's issuance, a connection's opening and its
  revocation, and nothing about refreshes.** Three events join the keys and grants family —
  `connection.code_issued`, `connection.opened` and `connection.revoked`, the `connection.*`
  family — and `key.issued` gains `credential` (`bearer` or `connection`); their payloads are
  declared on the [event catalog](../data/event-catalog.md#connectioncode_issued). Like `key.*`
  and `grant.*`, the `connection.*` family is on no key's events tail and belongs to the evidence
  export ([HTTP API](../api/http-api.md#events-tail)), so a connector key never reads its own
  connection's client and redirect URI there. `connection.opened` commits at the token exchange,
  when tokens first exist, and carries the client id, the digest of its metadata, the redirect URI
  and the resource, so the admin learns which client software used a code — for claude.ai not
  whose account, since every claude.ai connection names the same client (d8); it revokes the key's
  earlier open connection, live or not, in the same commit and names it in
  `replaces_connection_id`, and that revocation needs no second event. `key.revoked` voids the
  key's unredeemed codes in its own commit, with no event of their own. Not journalled: issuing
  an access token and refreshing — they change no authority, and one event per hour per connection
  would grow the chain with events that decide nothing and tie a 30-second refresh budget to the
  per-tenant chain lock; authorization requests, wrong code entries, client registrations and
  document fetches — unauthenticated input, which would otherwise let anyone grow the signed
  chain, the reasoning that keeps a scan veto out of it; and a code spent with no token exchange,
  which `connect_codes` shows and from which no authority came. A reuse revocation is triggered by
  a token request, but only a holder of a past refresh token, or of a redeemed authorization code
  together with its verifier (d10), can trigger it, and only once per connection.

- **d13 — The `oauth` family joins the closed taxonomy; the OAuth endpoints carry its codes inside
  their RFCs' error shapes, and the JSON surfaces carry them in the envelope.** Eleven codes:
  `oauth.client_invalid`, `oauth.redirect_refused`, `oauth.pkce_required`,
  `oauth.resource_mismatch`, `oauth.authorization_expired`, `oauth.connect_code_invalid`,
  `oauth.authorization_code_invalid`, `oauth.refresh_invalid` (each `400`; `oauth.client_invalid`
  is `401` at the token endpoint for a request that sent an `Authorization` header, d5),
  `oauth.origin_refused` (`403`, on the page, d9), `oauth.attach_refused` (`403`, an `/mcp` tool
  result, d11) and `oauth.connect_refused` (`422`, on `/v2`). At `/oauth/token` and
  `/oauth/register` the body is `{error, error_description}`: `error` is the RFC code a client
  branches on, and `error_description` begins with the OML code and its rule. At
  `/oauth/authorize` a refusal redirects with `error`, `error_description`, `state` and `iss`
  (RFC 9207 §2, which puts `iss` on error responses too), or, when a parameter bound, the client,
  the redirect URI or the origin check is the problem, is a `text/html` page that names the code.
  Malformed parameters are
  `request.validation`, carried as `invalid_request` or the RFC's own name for that parameter's
  fault. `/mcp` keeps `auth.missing` and `auth.invalid` with the challenge of d2. The codes are
  declared on [errors](../api/errors.md#oauth-connections) and mirrored in the `ErrorCode` enum.

    OML codes exist on these endpoints because the taxonomy is closed — every refusal the service
    makes is one of its codes — and an operator joining a log line to a doctor count needs one
    name; the RFC shape stays because Claude reads `invalid_grant` to learn that a refresh failed.
    The cost: an OAuth client sees the OML code only as text.

- **d14 — Operations: one boot key and three journalled lifetimes, a doctor check run from
  outside, a proxy that routes and logs no secret, and every OAuth write on the lead.**

    - **Configuration** ([configuration](../operations/configuration.md)): `oauth.issuer` is
      boot configuration, unset by default; `oauth.access_token_ttl`, `oauth.refresh_token_ttl` and
      `oauth.connect_code_ttl` are journalled settings. The other bounds — the 10-minute
      authorization request, the 60-second authorization code, five attempts per page, the
      parameter bounds of d4, the fetch bounds of d5 with its 16 concurrent fetches on the lead,
      one per host name and one per resolved address, the 10 000 cached documents and their
      24 hours, the 10 000 unused registrations — are constants of this design, changed by a
      design change.
    - **Doctor** ([observability](../operations/observability.md#the-doctor-contract)):
      `oauth_discovery` — the CLI fetches both discovery documents from the issuer URL, from
      wherever it runs, and checks that `resource` is `<issuer>/mcp` and `issuer` is the issuer the
      lead recorded, so a proxy that does not route the paths, or a node serving another issuer,
      shows as `failing` with the URL; `connect_codes`,
      per tenant — live codes and codes that lapsed unredeemed; `connections`, per tenant — live
      connections (d11), open ones that are not live reported apart, and those revoked for reuse
      in the window, which is `attention` because it means a stolen token or a client that lost a
      refresh answer. Wrong code entries are deployment-level: a wrong entry matches no code, so
      it names no key and no tenant, and it writes no row, since an authorization request has none
      until its code verifies (d9); the lead counts them in memory from its start. They are shown
      only under an operator key's doctor, as `connect_code_failures`, never under a tenant
      admin's.
    - **Proxies.** A reverse proxy in front of `oml-server`, the hosted deployment's Caddy
      included, forwards `/mcp`, `/.well-known/oauth-protected-resource/mcp`,
      `/.well-known/oauth-authorization-server` and `/oauth/*` unchanged, with no path rewriting and
      no caching. The page's headers are `oml-server`'s own (d9), so the proxy adds none. A proxy
      that writes access logs must not record the `Location` response header of
      `POST /oauth/authorize`, which carries the authorization code, nor the request bodies of
      `POST /oauth/authorize` and `POST /oauth/token`, which carry the connect code, the
      authorization code, the verifier and the refresh token: the plaintext rule binds
      `oml-server`'s own log, and a proxy's log would be another copy of the same secrets. Caddy's
      access-log fields include response headers under `resp_headers` (the `log` directive's
      documentation, fetched 2026-10-31), so the hosted Caddy's `log` directive carries a
      `format filter` with `resp_headers>Location delete`, whatever Caddy records by default, and
      adds nothing that logs a request body. Doctor cannot read a proxy's configuration, so the
      rule is stated, not checkable
      ([observability](../operations/observability.md#per-profile-checks)).
    - **Egress and addresses.** The hosted apps' calls come from `160.79.104.0/21`; a firewall
      that admits by address admits that range on `/mcp`, the two discovery documents,
      `/oauth/token` and `/oauth/register`, while `/oauth/authorize` is reached by the person's
      browser. No limit keys on the source address: behind a proxy it is the proxy's, OML has no
      trusted-forwarding rule, and all hosted-app token calls share one range anyway.
    - **Cluster mode.** A follower's `oauth.issuer` is always the lead's, because a follower has
      none of its own: the lead writes its value, or null when it has none, into the `lead_lease`
      row when it takes the lease, and a follower serves the challenge, the discovery documents
      and token validation from that row in its replica, as it reads the lead's address there. A
      follower whose boot configuration sets `oauth.issuer` therefore ignores it while it is a
      follower, and a lead-eligible node reads its own only when it takes the lease, so every
      lead-eligible node carries the same value: a takeover by a node configured with another
      is an issuer change, with d1's consequences for every node at once. A follower can then lag
      the lead's value by replication lag and no longer. The OAuth endpoints write, so a follower
      forwards them to the lead; a follower validates access tokens from its replica for the
      reads it serves, with the revocation lag a revoked key has there
      ([workareas](../architecture/workareas.md#grants-bind-to-node-ids-paths-are-derived)). A
      follower forwards an `/mcp` request that carries an access token and reaches a write verb as
      the whole `/mcp` `POST` to the lead's `/mcp`, over the enrolled hop, with the token unchanged
      in `Authorization`; the lead re-resolves token → connection → key and authorizes against
      that key, so a revoked connection or key is refused at the lead's next request whatever the
      follower's lag. This is the MCP request forwarded to the same endpoint, not a token passed to
      a ring route (d11), and the follower never asserts a key id it resolved.

- **d15 — `info.version` moves from `0.2.0-design` to `0.3.0-design`.** The changelog's rule names
  the error taxonomy among the provider ring's contract, and the `ErrorCode` enum that `/v1`
  responses share gains the `oauth` family; a change carrying a proposed record that changes the
  OpenAPI document bumps the version (`CONTRIBUTING.md`, "Spec versioning and tags").
  No `/v1` operation changes: no path, request or response shape, idempotency or pagination rule,
  and no `/v1` operation answers an `oauth` code; `oauth.attach_refused` is the facade's, before
  any `/v1` call. On `/v2` one idempotency rule changes: the record of the two connect-code
  responses stores its response without the code (d7). The proposed ADR-0026, on another branch,
  moves
  to `0.3.0-design` as well: if its revision is still untagged when this record lands, the two
  share `0.3.0-design`; if `spec-v0.3` has been cut, this record takes `0.4.0-design` when it is
  rebased.

- **d16 — The build follows iteration 3; the pure modules may land before it.** What touches key
  issuance — the `POST /v2/keys` body, `issue-agent`'s arguments, `api_keys` and the migration
  sequence, the page's description of the key — waits for names, which change all of them. The
  pure modules — the connect code's format and normalization, PKCE verification, the redirect
  matcher, the address allow-list, document and registration validation, the discovery documents
  and the challenge, the sealed authorization-request handle, the refresh state machine, the
  page's rendering and headers — depend on none of that and may land
  earlier, behind no route.

## Consequences

**Pages this record changes, each the single declaration site of its part.**

- [HTTP API](../api/http-api.md): `POST /mcp` on the surface; the section
  [Connections: OAuth for hosted MCP clients](../api/http-api.md#connections-oauth-for-hosted-mcp-clients)
  (the challenge, the two documents, the three endpoints, the page, proxies);
  `POST /v2/keys/{key_id}/connect-codes` and
  `POST /v2/keys/{key_id}/connections/{connection_id}/revoke` in the keys table, and the refusal
  of a connector key on the rotate row; the `connection.*` family on the events-tail table; the
  idempotency section's rule that a connect-code response is stored without the code; the
  content-type convention's exceptions. [openapi.yaml](../api/openapi.yaml) mirrors all of it,
  with the `WWW-Authenticate` header on the `/mcp` `401`.
- [Errors](../api/errors.md): the `oauth` family with its RFC mapping, the `auth` rows' challenge
  note, the status index.
- [Roles and keys](../architecture/roles-and-keys.md#connections): connector keys, the connect
  code, the connection lifecycle and when a connection is live, tokens, where no connection
  secret may appear, and the token-is-its-key rule; the matrix row; the key format names the two
  token prefixes.
- [Agent plugin](../api/mcp-plugin.md): authentication by key or connection, `oml_attach` refused
  under a connection, and the claude.ai path in "Wiring it into your agent".
- [Security](../architecture/security.md#connections-the-oauth-surface): the threat-model delta
  and residual risk 9; the custody map and the trust-boundary diagram.
- [Configuration](../operations/configuration.md); [observability](../operations/observability.md)
  (the doctor fields, the plaintext rule's list of what a log line never carries, and the
  stated, not checkable, line for a proxy's access log);
  [admission](../architecture/admission.md) (the `secret.api_token` row names the two token
  formats); [event catalog](../data/event-catalog.md); [Postgres schema](../data/postgres-schema.md)
  (five tables, `api_keys.credential`, `api_keys.hash` nullable, `lead_lease.oauth_issuer`, the
  rotation and redemption statements); [CLI reference](../cli/reference.md)
  (`--connect-code`, `oml keys connect-code`, `oml keys disconnect`, connections in
  `oml keys show`, the refusal of `oml keys rotate` on a connector key, and what the connect-code
  verbs do with a replayed answer that carries no code).
- The [roadmap](../product/roadmap.md): RS-2 restated in ADR-0018 d3's form, its exit criterion met
  by this record on acceptance and the row leaving the register then; FR-34, FR-35 and FR-36
  opened.
- `README.md`: how to add OML on claude.ai and use it from a phone, marked decided, not built.
- The claim↔claim and workarea↔workarea sense written "connection edge" or "connection proposal"
  (d7's vocabulary row) on [roles and keys](../architecture/roles-and-keys.md) (the dreamer row,
  the matrix and the dreamer split), [security](../architecture/security.md) (residual risk 8),
  [audit and lineage](../architecture/audit-and-lineage.md), [concepts](../concepts.md), the
  [HTTP API](../api/http-api.md) (the `POST /v1/memories` ceiling), `openapi.yaml` and the
  [agent plugin](../api/mcp-plugin.md).

**Opened.** [FR-34](../product/roadmap.md): the connection flow is designed, not built, and no live
connection has run. It closes on the end-to-end test of the in-process lead passing in CI and a
dated live connection from a claude.ai custom connector used from a Claude mobile app, with the
token endpoint's latency measured against Claude's 10-second and 30-second limits; it also holds
what the sources do not say — Claude's behavior after a failed document fetch, `form-action` on
redirects across browsers, whether Claude checks the `iss` of an authorization response, and how
often strict rotation ends a connection.
[FR-35](../product/roadmap.md): the `/v1` and `/v2` `401` answers carry no `WWW-Authenticate`
challenge, which RFC 9110 §15.5.2 requires (d2); it closes on a recorded decision and the header
on every `401`. [FR-36](../product/roadmap.md): the idempotency record of a bearer key's issuance
or rotation stores the response as sent, key value included (d7); it closes on a recorded decision
and no stored response holding a key value.

**Costs carried here.** An OAuth implementation, an HTML page and an outbound fetch inside the
lead; a second error shape on one listener; one more lookup per `/mcp` call for a connection; a
lost refresh answer ends a connection; a person who cancels or lets a code lapse needs a new one
from the admin; a bearer client refused at `/mcp` now sees a sign-in pointer; a person who enters a
code on a link someone else started connects the sender's client, and only the page's warning
stands in the way; a connection opens no upload session (d11); a replayed connect-code issuance
returns no code (d7); a restart or takeover of the lead ends every page in progress (d9); a flood
of slow
document hosts can stall document clients the cache holds no copy of (d5). The page also ends
ADR-0015's claim that the threat model never answers for a
browser origin, a CSP or a frame (d9): it is not the declined web console, but the threat model now
answers for all three. None is measured.

**On acceptance, and not before.** This record's Status line and frontmatter `status` become
`accepted design` with the date, and its register row follows; the Status lines of ADR-0010,
ADR-0014, ADR-0015, ADR-0018, ADR-0025 and ADR-0028 gain "· amended by ADR-0031"; the accepted
records' spellings that d7's rows put in the "never" column are reworded as page edits: the bare
"connection" for a claim↔claim or workarea↔workarea link in ADR-0010 d5 ("connection
ratification"), ADR-0012's context ("connection-establishment") and ADR-0025's d1 rejected
alternative ("ratify connections") becomes "connection edge" or "connection proposal", ADR-0025's
"needs one connection" in its agent-facing results becomes "needs one key", and the "token" that
names a key in ADR-0022 (its context, d4, d5, d6 and rejected alternatives) and ADR-0025 (d13, d14
and d13's rejected alternative) becomes "key"; RS-2 leaves the register;
[commitments](../product/commitments.md) drops RS-2 from its open questions and lists FR-34,
FR-35 and FR-36; the
`spec-vX.Y` tag of d15 is cut. With the proposal, the record registers at its three points, the
D9, D13 and D14 cells of the D-register name it, `llms.txt` names it, and one
[changelog](../changelog.md) entry records it.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **d1 — A third-party identity provider as the authorization server** | The code-to-key binding needs OML's key store, and the provider would enter every connection's path and availability budget — ADR-0010's objection. |
| **d1 — The static bearer header claude.ai offers in beta** | Limited to some organizations, entered by an Owner and shared by every member: one key behind many agents. |
| **d1 — End every connection by event when `oauth.issuer` changes** | `oauth.issuer` is boot configuration, not a journalled act, so the events would have no actor and no act to cite; refusing tokens whose resource is not current stops them without inventing one. |
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
| **d5 — Refuse an uncached document while the cache is full** | Ten thousand throwaway document URLs fill the cache as fast as the fetch caps allow, and Claude's own document would then be refused until an entry lapsed; eviction costs a refetch and refuses no one. |
| **d5 — A refusal list of special-purpose addresses** | A closed list misses blocks the registries mark not globally reachable, and every new registry entry would be a hole until the list caught up; an allow-list of global unicast refuses what it does not name. |
| **d5 — An allowlist of document hosts** | The host serving Claude's document is not in the cited sources, so the shipped list would be a guess; the redirect allowlist bounds where codes go. |
| **d6 — A configurable redirect allowlist** | Each entry decides where an authorization code may go; that is a decision to record, not a value to set. |
| **d6 — Loopback redirects for Claude Code** | Deferred: how Claude Code signs in is not in the cited sources, and it keeps its key in the header. A loopback entry would bring its own grammar, a second page warning and the residual that any local program can listen, for a client this record cannot source; admitting one is a later recorded decision. |
| **d7 — The maintainer's rejected reading: paste the key value on OML's page** | The value would pass through the person's clipboard and browser. |
| **d7 — Print the key value beside the code** | A second, unneeded credential for the same key, resting on a terminal. |
| **d7 — Store every show-once response without its secret, bearer keys included** | It would change bearer-key issuance and rotation, which connections do not need and which stay unchanged; the stored value is FR-36. |
| **d7 — Codes for bearer keys too** | One key would act through two credential paths, and the key id on a journal event could no longer say which acted without changing the signed envelope. |
| **d7 — Rotate a connector key into a successor** | Rotation prints a successor's value, which would make it a bearer key with the connector key's grants; a successor without a value would need a new code anyway, which `oml keys connect-code` already mints. |
| **d7 — Several open connections per key** | Several clients would share one identity, against one key per agent. |
| **d7 — Connect codes for admin keys** | `/mcp` serves no admin verb, so the connection could do nothing an admin key exists for, and the page would show an admin identity. |
| **d8 — The code's digest in the journal** | The journal is kept forever and exported; a 60-bit digest there is a brute-force target that outlives the code by years. |
| **d8 — A link that carries the code** | A secret in a URL lands in history, logs and referrers. |
| **d8 — A longer code, or words** | Twelve symbols already put guessing at L·G / 2⁶⁰; every added symbol is one more for a person to type. |
| **d8 — Different messages for an expired, spent or unknown code** | It would confirm to a guesser that a code existed. |
| **d9 — Open the connection as soon as the code is right, with no confirmation** | The person would never see which agent and which workarea the connection acts as. |
| **d9 — Ask for the code with no prior authorization request** | The code would arrive with no client, redirect or PKCE challenge bound to it. |
| **d9 — A connect flow with no browser page** | The MCP authorization flow sends the person's browser to an authorization endpoint; there is no flow without one. |
| **d9 — One fixed sentence for what the connection may do** | A reader key cannot remember and a write-only key cannot recall; a fixed sentence would misstate the connection's authority on the page that exists to state it. |
| **d9 — `Referrer-Policy: no-referrer`** | A browser then sends `Origin: null` on the page's own `POST`, which the forgery check refuses, so no connection could open. |
| **d9 — A session cookie and a CSRF token** | A cookie is an ambient credential to protect; with none, a forged form has nothing to ride. |
| **d9 — Require `state`** | PKCE already closes the client's exposure, and refusing a conforming client that omits it buys OML nothing. |
| **d9 — A row per authorization request, under a cap** | Ten thousand ten-minute requests are one unauthenticated `GET` every 60 milliseconds, which holds such a cap full and refuse every new page; a sealed handle writes nothing until an admin-minted code verifies. |
| **d9 — Per-address throttling of wrong codes** | Behind a proxy the address is the proxy's, and OML has no trusted-forwarding rule; the entropy bound holds without it. |
| **d10 — JSON Web Tokens** | The resource is also the issuer, so self-containment saves nothing; revocation would wait for expiry, and the token would carry tenant and key id in the clear. |
| **d10 — A grace window for a rotated refresh token** | A stolen refresh token raced inside the window would pass reuse detection. |
| **d10 — Any presentation of a redeemed authorization code as reuse** | A redeemed code sits in browser history and request logs, so anyone who read one could end the connection months later; only the holder of the verifier can redeem first, and the rule still catches that holder. |
| **d10 — Long-lived access tokens with no refresh** | A stolen token would be the key's full capability for its whole life; the specification asks for short-lived access tokens. |
| **d11 — Let a token reach `/v1` and `/v2`** | A connection would become a general credential for the lab ring's verbs, beyond what the hosted apps need, and the facade boundary would stop bounding it. |
| **d11 — Accept a token on the attachment parts route** | The token would become a credential on `/v1`, which the facade boundary exists to prevent. |
| **d11 — Hide `oml_attach` from a connection's `tools/list`** | The catalog would differ by credential path, so one key would see two catalogs; a typed refusal says why. |
| **d11 — Revoke a key's connections by event when the key is revoked** | The lookup already reads the key's state on every request; extra events would record nothing `key.revoked` does not. |
| **d12 — Journal every refresh** | It decides nothing and would tie refresh latency to the per-tenant chain lock. |
| **d12 — Journal client registrations and document fetches** | Unauthenticated input would grow the signed chain. |
| **d13 — The OML envelope on the OAuth endpoints** | OAuth clients parse `error`; Claude reads `invalid_grant` on a failed refresh. |
| **d13 — RFC codes only, with no OML codes** | The taxonomy would stop being closed, and logs and doctor would have no stable name for these refusals. |
| **d14 — A follower's own `oauth.issuer`** | A follower left on another value would honor tokens the lead refuses, or serve documents for endpoints the lead answers `404`, for as long as it ran. |
| **d14 — Every limit a setting** | A value with one caller is a constant, and these bounds are security properties a deployment should not loosen by editing a file. |
| **d15 — Leave `info.version` at `0.2.0-design`** | The shared error taxonomy changes, which the changelog's rule names, after `0.2.0-design` was tagged. |
| **d16 — Build now, before names** | Issuance, `issue-agent` and the key tables all change again when names land. |
