<!--
id: adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin
tier: engineering
status: accepted design · 2026-11-04
answers: why OML now serves a web console after ADR-0015 declined one, and how each of ADR-0015's reasons is answered; which pages and routes the lead serves and why so few; how a person signs in with Google or GitHub and what the server checks; what the email allowlist holds, who maintains it, how the first admin gets in and what removing an email ends; how a signed-in person maps to ADR-0044's user; which identity signs each act a page performs and how R1 keeps the pages out of every agent's reach; what the user page and the admin page do; how review, curation, demotion and supersession run from a page without the admin key; what the admin reads and how that read is journalled; the session, CSRF, clickjacking, CSP, rate-limit and logging rules; the request-line fields for web requests; the doctor lines; how the Memories view lists, filters and pages, what each page's list may show, how the Atlas view draws the same page from a JSON route and what its script changes in the CSP and in risk, and how a workarea's acts become a dropdown of what the key may do; what it amends in ADR-0010, ADR-0015, ADR-0025, ADR-0026, ADR-0031, ADR-0042 and ADR-0044
-->

# ADR-0045 — People sign in with Google or GitHub to a user page and an admin page that the lead serves on its own origin, and every act there is a session key's

**Status:** accepted design · 2026-11-04 · revised the same day with the maintainer's answers to its open choices (d20) · amended by [ADR-0037](ADR-0037-a-guardrail-is-a-reviewed-rule-loaded-at-session-start-and-checked-at-the-tool-call.md) · amended 2026-11-06 with the Memories list, the Atlas view, the workarea dropdowns and the pages' copy (d22–d32)

**Release:** introduced in v0.5.0 — sign-in, sessions, the email allowlist, the user page with review of the person's own agents' work, and on the admin page review, curation, supersession and the admin read of every memory's text and attribution; demotion is planned for v0.6.0, since it rests on a verb not built ([releases](../product/releases.md#decisions))

**Amends:** ADR-0015 (the decline of a web console in its context, its consequences and its rejected alternatives; d2 gains a second human surface), ADR-0010 (d1, d7), ADR-0025 (d3), ADR-0026 (d15: the admin key reads content through the admin read, d21), ADR-0031 (d7), ADR-0042 (d2), ADR-0044 (d6, d9 for the admin read, d11)

## Context

**ADR-0015 declined a web console, and its reasons have to be answered, not stepped around.**
[ADR-0015](ADR-0015-cli-administration.md) makes the `oml` CLI the whole administration surface
and declines a web console "whole", for five reasons: a minimal console does not stay minimal,
because duties accrete once the surface exists; a console buys back origin policy, sessions, a
CSP and a second presentation of every state to keep truthful; nothing load-bearing needs a
browser, since every duty has its CLI home; the threat model never has to answer for a browser
origin, a session cookie or a frame; and a review console would have to earn the serve binding's
evidence against a browser's cache and a stale tab. The population it reasoned about was
operators, tenant admins and external review crews ([ADR-0015](ADR-0015-cli-administration.md),
context). The register's D14 row records the decline as owned by
[cli/reference.md](../cli/reference.md).

**Two things have changed since.** First, v0.4.0 already serves an HTML page:
`GET` and `POST /oauth/authorize`, the connect-code page of
[ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) d9. It is server-rendered by
`crates/transports/oml-transport-http/src/oauth/page.rs`: no script, one inline stylesheet whose
SHA-256 the CSP names (computed at compile time), `default-src 'none'`,
`frame-ancestors 'none'`, `form-action 'self'` plus the one redirect origin, `Cache-Control:
no-store`, `Referrer-Policy: same-origin`, `nosniff`, every client string escaped and cut, and no
cookie. Its forgery defence is a request handle sealed with ChaCha20-Poly1305 under a key the
process holds only in memory (`oauth/seal.rs`) and a same-origin check on every `POST` that reads
`Sec-Fetch-Site`, or `Origin` when that header is absent (`same_origin` in `oauth/mod.rs`). Its
log line drops the `Location` query (`logged` in `oauth/mod.rs`; [ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md)
d2). The origin policy, the CSP and the frame rule ADR-0015 wanted to avoid are therefore
already designed, built and running on the maintainer's deployment; a session cookie is not.
Second, [ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
gives every person a root of their own, so the population now includes people who are neither
operators nor admins, and who today get a key, a connect code and a workarea only by asking an
admin to run the CLI for them.

**What exists today, from the code.** The lead routes `POST /v2/keys`, `GET /v2/keys/{key}`,
`POST /v2/keys/{key}/denials`, `POST /v2/keys/{key}/connect-codes`,
`POST /v2/keys/{key}/connections/{connection_id}/revoke`, `GET /v2/whoami`,
`POST /v2/review/next`, `POST /v2/review/verdicts`, `POST /v1/memories/{artifact_id}/supersede`,
the workarea read, tree and create, and the events tail (`ROUTES` in
`crates/transports/oml-transport-http/src/lib.rs`). It routes no key listing, no revocation, no
grant change, no demotion and no review queue read: those are designed in
[openapi-designed.yaml](../api/openapi-designed.yaml). `api_keys.credential` is `bearer` or
`connection`, and a connector key's `hash` is null ([ADR-0031](ADR-0031-oauth-connections-by-connect-code.md)
d7). No table records when a key was last used; the one `last_used_at` column is the OAuth client
cache's eviction order (`migrations/0013_oauth_clients.sql`). A key is cut off today only by a
global denial on `universe`, since revocation is not built.

**The maintainer's decisions of 2026-11-04,** which this record designs around and does not
reopen: a simple user page and an admin page, served on the public origin by `oml-server-lead`
itself as server-rendered forms on the same origin, reusing the connect page's approach, with a
`Secure`, `HttpOnly`, `SameSite` session cookie, a CSRF token on every form and sessions held on
the server with an expiry; sign-in with Google or GitHub, the provider's verified email checked
against an allowlist the admin keeps, no password stored and no mail sent; a signed-in person maps
to ADR-0044's `user`; review only through the person's own reviewer identity, never the admin
key, with R2 enforced by the server on the reviewing key and the lineage closure; curation into
`universe` under ADR-0044 d7; demotion and supersession, never deletion; and the pages human-only,
out of reach of every agent key and every MCP connection, with identity read off the session and
never off a request body. A later decision of the same day drops ADR-0044 d8: R2's exclusion set
stays the keys and principals of the lineage closure, and a person may review their own agents'
work through a separate reviewer key. This record does not rely on d8.

The constitution holds over all of it ([ADR-0001](ADR-0001-the-constitution.md)): the pages are
not a surface an agent can call (R1), the reviewer key is never in the row's lineage (R2), review
changes visibility and not ranking (R3), and nothing is deleted (R4).

Decided; introduced in v0.5.0 but demotion (d12), planned for v0.6.0 with its verb.

## Decision

- **d1 — The lead serves two pages, the user page and the admin page, over six routes and no
  more, and the route set is closed.** Where the deployment sets `oauth.issuer` and at least one
  sign-in provider (d17), `oml-server-lead` routes, beside `ROUTES` and `OAUTH_ROUTES`, a third
  table, `WEB_ROUTES`:

    | Route | Answers |
    |---|---|
    | `GET /web` | the sign-in page without a session; the user page with one (d9) |
    | `POST /web` | the user page's acts, one `action` per form, sign-out among them |
    | `POST /web/sign-in` | starts a provider's flow (d3) |
    | `GET /web/callback/{provider}` | the provider's redirect, `google` or `github` (d3, d4) |
    | `GET /web/admin` | the admin page (d10), for a session whose allowlist entry is an admin's |
    | `POST /web/admin` | the admin page's acts, one `action` per form |

    Each page is one `GET` that renders views selected by query parameters (`view`, `q`,
    `memory`, `workarea`) and one `POST` whose form names exactly one `action` from a closed set,
    the shape `POST /oauth/authorize` already has (two submissions, told apart by their fields).
    A follower routes none of them: every web act writes, and sessions are written on the lead.
    Unset, every web route answers `404` as an unknown path does. A router test asserts that the
    lead serves exactly `WEB_ROUTES` and that each `action` value of each page is one of the
    closed set this record lists; a new page, route or action is a new decision amending this
    one. This is the answer to ADR-0015's first reason: the duties below are enumerated, and
    the ones a console attracts — recall views, graph pictures, dashboards, an assistant — are
    declined here by name. The cost is a page whose views are selected by query parameters
    rather than by paths, which the request log cannot tell apart (d14).

- **d2 — The email allowlist is per tenant, kept by its admins, and the first entry comes from
  the CLI under an admin key.** An **allowlist entry** is `{email, user, admin}`: an email
  address a person may sign in with, the ADR-0044 user that person is, and whether the entry
  opens the admin page. An email is unique in the deployment, so a sign-in, which happens before
  any tenant is known, finds at most one entry, as a connect code's lookup does
  ([ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) d8). It is compared whole, ASCII
  case-insensitive, with no dot or plus-tag folding. `user` is a name under the
  [creation grammar](../api/http-api.md#names-and-ids) of at most 56 characters, so every key
  name d6 composes from it fits the 63-character label; it is given by the admin and never
  derived from the email, as ADR-0044 d2 never derives it from a principal. Several entries may
  name one user, which is how one person signs in with a Google address and a GitHub address;
  no separate person record exists. Three lab routes carry the list for the CLI and every other
  client, admin key role: `GET /v2/web/emails`, `POST /v2/web/emails` (`{email, user, admin}`,
  adding or changing an entry) and `POST /v2/web/emails/remove` (`{email}`); the email travels
  in a body, never in a path, so no proxy log records it. Each act is journalled as
  `web.email_allowed`, `web.email_changed` or `web.email_removed` with the acting key, the user
  and the admin flag, and an `email_digest` in place of the address, since the journal is
  append-only and an address is personal data; the list table holds the address itself and loses
  it when the entry is removed. **Bootstrapping:** the tenant's existing admin key adds the first
  entry, `oml web allow alice@example.com --user alice --admin`; there is no other way in, and no
  boot setting names an email. Thereafter the admin page keeps the list (d10), and the CLI stays
  the recovery path when the last admin loses their provider account. A user name a shared
  workarea already holds is refused `name.taken` (kind `workarea`), as ADR-0044 d2 refuses it at
  issuance; an email another tenant's entry holds is `name.taken` (kind `email`), which tells an
  admin that the address is on another tenant's list and nothing more; a user whose composed
  key names (d6) another key already holds is `name.taken` (kind `key`). The cost is one new
  table, three routes, three verbs and three journal events, for a list a deployment with one
  person holds one row of.

- **d3 — Sign-in is the authorization code flow with S256 PKCE, `state` and, for Google, a
  `nonce`, carried in a sealed cookie that writes no row.** `POST /web/sign-in` takes one field,
  `provider`, after the same-origin check of `POST /oauth/authorize`. It draws a 256-bit `state`,
  a PKCE verifier and, for Google, a `nonce`; seals `{provider, state, verifier, nonce,
  started_at}` with the process's in-memory AEAD under its own purpose string, so a connect
  page's handle never opens as a sign-in and the reverse; sets the result as the cookie
  `__Host-oml_signin` (`Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, `Max-Age=600`); and
  answers `303` to the provider's authorization endpoint with `client_id`, `redirect_uri`
  `<oauth.issuer>/web/callback/<provider>`, `state`, `code_challenge`,
  `code_challenge_method=S256`, the scope (`openid email` for Google, `read:user user:email` for
  GitHub) and the `nonce`. `GET /web/callback/{provider}` opens the cookie, clears it in every
  answer, and refuses before any outbound call unless the cookie opens, is younger than ten
  minutes, names the path's provider and carries the query's `state` byte for byte; the path
  carries the provider so that a code from one provider is never sent to the other's token
  endpoint (the mix-up the path and the sealed provider together close). Like the connect page,
  sign-ins in progress are free to open and survive no restart. The callback then redeems the
  code at the provider's token endpoint with the client secret and the verifier, over TLS, and
  discards the provider's access token as soon as the email is read: OML stores no provider
  token. The cost is the lead's outbound HTTPS calls to two fixed hosts per sign-in, which the
  OAuth page's metadata fetches already made a lead property (bounded in d15).

- **d4 — The server admits only a verified email it read from the provider over TLS, and pins
  the provider account of each entry at its first sign-in.**
    - **Google.** The ID token comes from Google's token endpoint over the TLS connection the
      lead opened, so its issuer is checked by that connection and its signature is not
      verified against Google's keys, as OpenID Connect Core 1.0 §3.1.3.7 item 6 permits for a
      token received directly from the token endpoint (to be fetched and quoted with its date
      when built). The lead checks `iss` is Google's, `aud` is the configured client id, `exp`
      is in the future, `nonce` equals the sealed one, and `email_verified` is `true`; the
      email is the token's `email`. No JWKS fetch exists.
    - **GitHub.** GitHub's OAuth apps are not OpenID Connect providers, so no ID token exists.
      The lead calls `GET https://api.github.com/user` for the account's numeric `id`, then
      `GET https://api.github.com/user/emails`, and takes the one entry with `primary: true` and
      `verified: true`; the profile's public `email`, which may be unset or unverified, is never
      read. A secondary verified address does not sign in, even when it is on the list: one
      address per GitHub account, the one its owner chose as primary, keeps the rule
      predictable. Whether GitHub enforces the PKCE challenge it is sent is unverified (FR-76).
    - **Pinning.** The first successful sign-in of an entry records the provider's subject, the
      ID token's `sub` or the GitHub `id`, on the entry, per provider. A later sign-in with the
      same email and another subject is refused, `auth.invalid` with `details.reason:
      identity_changed`, because an address that moved to another account — a lapsed domain, a
      recycled workspace user — would otherwise sign in as the person. The admin clears the pin
      by removing and re-adding the entry.
    - **Refusals** are pages, never redirects, each `auth.invalid` with one `details.reason`:
      `not_allowed` (no entry holds the email), `email_unverified`, `state_mismatch` (the
      cookie absent, unopened, expired, for the other provider or with another `state`),
      `identity_changed`, and `provider_refused` (the token or email call failed or answered
      `error`). The page names the reason and never echoes the email: whoever signed in already
      knows it.

    The cost is that a provider's own word on an address is the whole proof of it: OML sends
    no mail and checks nothing more, so an address a provider calls verified is trusted as far
    as the allowlist reaches, and the pin is what limits a moved address.

- **d5 — A session is a server-side row under a fresh random token, fixed at sign-in, ended at
  sign-out, at expiry or when its entry changes, and never renewed in place.** On an admitted
  callback the lead mints a 256-bit session token, stores its SHA-256 with the entry, the user,
  the tenant, `created_at` and `expires_at = created_at + web.session_ttl` (d17), and answers
  `303 /web` setting `__Host-oml_session` to the token (`Secure`, `HttpOnly`, `SameSite=Lax`,
  `Path=/`, `Max-Age` the remaining life). No session identifier exists before the callback
  admits the person, so an attacker has no value to plant (session fixation); a browser that
  arrives with an old session cookie gets a new token, and the old row is ended in the same
  commit. Sign-out is the user page's `sign_out` action: it ends the row and clears the cookie.
  Changing an entry ends its sessions; removing an email ends them, and so does any change of
  its `admin` flag, so authority never outlives the list. `SameSite=Lax` and not `Strict`,
  because the callback is a cross-site navigation from the provider and the redirect to `/web`
  that follows it would arrive without a `Strict` cookie; `Lax` withholds the cookie from every
  cross-site `POST`, which is where it matters, and every `GET` is free of effect. There is no
  idle timeout: it would make every page view a write. Session rows are operational state, like
  the authorization requests of [ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) d9:
  not journalled, ended rows removed once past expiry, and every act a session performs is
  journalled under its key (d6). The cost: a stolen session cookie acts as the person until the
  session expires, the person signs out, or an admin changes or removes the entry.

- **d6 — Every act a page performs is performed by a session key: a key the server holds for the
  person, with credential `session` and no value, and the journal names it as the actor.**
  `api_keys.credential` gains a third kind, `session`, beside `bearer` and `connection`
  ([ADR-0031](ADR-0031-oauth-connections-by-connect-code.md) d7). A session key is issued with
  `held_by: human`, has a null `hash` and no connection, records the allowlist user it serves
  in its own column, `session_user`, apart from ADR-0044's `user` field (which only `<user>-web`
  carries, since `user` is refused on admin keys and would confine a reviewer's grants to the
  root), and is never shown: no value exists, so
  it cannot be presented on `/v1`, `/v2` or `/mcp`, and minting a connect code for it is
  refused, `oauth.connect_refused` with `details.reason: session_key`. The server reaches a
  session key only from a live session whose entry names that key's user, re-checked on every
  request, so removing the entry, or its admin flag, makes the key unreachable at once with no
  revocation, which is not built. The admin key that added the entry is recorded on it
  (`added_by`), and the session keys are issued under it. Each person holds at most four, named from their user, each
  created by the act that first needs it and journalled as `key.issued` with the creating key as
  actor; the first sign-in creates two, so a person can review their own agents' work without
  any admin step (d20 e):

    | Key | Role and grants | Created when | Does |
    |---|---|---|---|
    | `<user>-web` | a user key (`user: <user>`), writer and reader on `universe.<user>`, and the `universe` node read ADR-0044 d3 adds | the entry's first sign-in, by the server under the admin key that added the entry | the user page (d9) |
    | `<user>-review` | a tenant dreamer key with dreamer and reader on `universe.<user>`, and any further dreamer and reader grants an admin gives it | the entry's first sign-in, beside `<user>-web`, by the same admin key | review on the user page and the admin page, supersession proposals |
    | `<user>-curate` | a tenant writer key with writer `covers: node` on `universe` and the reader grants on the user roots an admin chooses (ADR-0044 d7) | an admin designates the person a curator (d10) | curation (d12) |
    | `<user>-admin` | a tenant admin key, no grants | the entry gains `admin: true` | the allowlist, the admin acts and the admin read of every memory's text and attribution (d10, d21) |

    The first sign-in therefore creates the person's root by ADR-0044 d2's rule: the first key
    naming the user creates `universe.<user>`, and `<user>-review` is issued after it in the same
    act, with its grants on the root just made. `<user>-review` is a tenant key, so an admin may
    extend it beyond the root (d10). Every session key's principal is its own name
    (`alice-review`), so R2, which excludes the keys and principals of the row's lineage closure
    ([ADR-0044](ADR-0044-every-user-works-under-a-root-of-their-own-and-universe-serves-curated-claims-without-their-source.md)
    d8), lets it approve the person's agents' writes, and still refuses it any row whose closure
    holds `<user>-review` itself, such as a supersession it proposed; an agent key an admin issues
    under the principal `alice-review` would share the exclusion, which is the admin's to avoid. Which key acts is decided by the server from the
    session and the `action`, never by a form field: each action defines its fields, none of
    which names who acts, and a field the action does not define is refused, `request.validation`
    with that field. A field such as `allow_email`'s `user` names what the act is about, never the
    actor, so identity is read off the session and never off the request body. Keeping one key per capacity is
    [ADR-0010](ADR-0010-roles-and-keys.md) d6's two-key rule on the web: the admin key never
    reviews, the reviewer never administers, and the journal says which capacity acted. This
    answers ADR-0015's "one authorization story, one audit story": a page act is a key's act
    under the same grants, verb ceilings and journal as a CLI act. Signing is unchanged: the lead
    signs every event, as it signs every act of every key. The cost is up to four key rows per
    person, which an admin's key reads list beside the bearer keys, marked `session`.

- **d7 — The pages are reachable by a person's browser and by nothing an agent holds (R1).**
    - A web route that receives an `Authorization` header answers `access.verb_ceiling` with
      `details.reason: web_is_human_only` before anything else is read: no bearer key, no
      connection token and no future credential reaches a page.
    - The API and `/mcp` never read the `Cookie` header: a request to `/v1`, `/v2` or `/mcp`
      carrying only a session cookie answers `auth.missing`, as one carrying nothing does, and
      a test asserts it on every route of `ROUTES`.
    - Session keys have no value (d6); no MCP tool, tool argument or resource names a web route,
      a session or an allowlist entry.
    - A session exists only after a provider sign-in of a verified, listed address (d4).

    The honest limit: "human-only" means "a person's signed-in browser". An agent that drives
    that browser — through a browser extension, computer use or a remote-control tool the person
    granted — acts as the person, as an agent with shell access to an admin's CLI already can.
    OML cannot tell the two apart and does not claim to; the pages carry no script and no
    machine interface that would make it easier, and every act still lands in the journal under
    the person's session key.

- **d8 — Every page carries the connect page's headers, a hashed stylesheet and no script, and
  every form a CSRF token and the same-origin check.** Each answer carries `Content-Security-Policy:
  default-src 'none'; style-src 'sha256-…'; form-action 'self' https://accounts.google.com
  https://github.com; frame-ancestors 'none'; base-uri 'none'` (the two provider origins only
  where that provider is configured, since `form-action` also governs the `303` that answers
  `POST /web/sign-in`), `Cache-Control: no-store`, `Referrer-Policy: same-origin`,
  `X-Content-Type-Options: nosniff`, with the stylesheet's digest computed at compile time as
  `page.rs` computes it. Every `POST` to a web route passes the `same_origin` check of
  `POST /oauth/authorize` (`Sec-Fetch-Site: same-origin`, or an `Origin` equal to the issuer when
  that header is absent, `Origin: null` refused), answering `oauth.origin_refused` on the page.
  Every form of a signed-in page carries `csrf_token`, the lower-case hex of SHA-256 over a fixed
  purpose string and the session token, so it needs no column and is worthless without the
  cookie; a `POST` whose token is absent or unequal (compared in constant time) is
  `request.validation` with `details.field: csrf_token`, and journals nothing. `POST
  /web/sign-in` has no session and relies on the origin check, while the sealed `state` binds
  the callback to the browser that started it, which closes login forgery. Every string a page
  renders that came from a person, an agent or a provider — memory text, names, principals,
  labels — is escaped and stripped of control and bidirectional formatting characters by the
  connect page's functions, moved to one module both use. Clickjacking is answered by
  `frame-ancestors 'none'`; injected markup by escaping and `default-src 'none'`. This answers
  ADR-0015's fourth reason: of the browser threats it named, only the session cookie is new
  (d5); the rest are the connect page's, already built.

- **d9 — The user page shows the person's keys, setup, memories and workarea, and reviews the
  person's own agents' work; it acts through `<user>-web`, and through `<user>-review` for
  review alone.** Its views and acts:
    - **Keys.** Every key whose `user` is the session's: name, role, `held_by`, credential,
      issuance time, grants, whether a global denial cuts it off, and a connector key's open
      connections. When the key was last used is not shown: nothing records it, and recording it
      is a write per request whose cost is unmeasured (FR-75). Acts: `issue_key` issues an
      agent key naming the session's user, `{name, role}` with role `writer` (with the reader
      grant beside it, the CLI's default, [ADR-0028](ADR-0028-agent-key-issuance.md) d2) or
      `reader`, granted on the user's root and nothing else; its name is
      `<user>-<name>`, the suffixes `web`, `review`, `curate` and `admin` reserved. The answer
      is a page, not a redirect, that shows the value once under `no-store` with the lines to
      paste, and nothing keeps it. `connect_key` issues a connector key the same way and shows
      its connect code and connector URL once; `connect_code` mints a fresh code for one of the
      user's connector keys; `cut_off` denies one of the user's keys on `universe`, the only
      cut-off that exists while revocation is not built, journalled as `key.denied`. A user key
      can therefore issue keys inside its own root and never above it: issuance never amplifies
      authority (ADR-0010 d3), since `<user>-web` holds writer there and the key it issues holds
      no more.
    - **Setup.** For Claude Code, claude.ai and mobile, and the other harnesses, the steps of
      [Claude Code](../guides/claude-code.md), [claude.ai and mobile](../guides/claude-ai-and-mobile.md)
      and [other agent tools](../guides/other-harnesses.md), with the server's URL filled in
      from `oauth.issuer`, linked to those guides rather than restated, so the guides stay the
      one declaration site. The key value appears in them only on the issuance answer.
    - **Memories.** A search box and a listing over the user's root, with each row's state —
      held, promoted, rejected — and `universe`'s own claims under ADR-0044 d9's served view:
      origin `universe`, no author, no lineage below it. Held and rejected rows are those of
      every key naming the session's user (d16); a single memory opens by `memory=<artifact_id>`.
      This view reads and writes no memory.
    - **Review.** `review_next` and `verdict`, the served ceremony of d11 as the person's own
      `<user>-review`, over whatever that key's dreamer grants reach: `universe.<user>` from the
      first sign-in, so the held writes of the person's agents, and more where an admin extended
      it. The person reviews without an admin, and an admin is no longer the bottleneck for one
      person's agents (d20 e).
    - **Workarea.** The tree of `universe.<user>` and `create_workarea` (`{name, parent?}`),
      ADR-0044 d6's create under `<user>-web`.
    - **Sign-out.** `sign_out` (d5).

- **d10 — The admin page shows the allowlist, the review queue's depth and age, every memory's
  text and attribution, and the review, curation and correction tools, each through the capacity
  it needs.** It opens for a session
  whose entry has `admin: true`; any other session gets `access.not_found`, so the page's
  existence confirms nothing to a non-admin.
    - **The allowlist**, through `<user>-admin`: list, `allow_email` (`{email, user, admin}`),
      `remove_email`, `set_admin`, and `cut_off` (`{key}`), the removal confirmation's act (d13). Removing or un-flagging the tenant's last admin entry from the
      page is refused, `request.validation` with `details.field: admin`, since the page cannot
      then be reopened; the CLI under an admin key can still do it. The removal confirmation
      lists the user's keys (d13).
    - **People's capacities**, through `<user>-admin`: `grant_review` (`{user, workarea}`)
      extends that user's `<user>-review`, which the user's first sign-in created, with dreamer
      and reader on the workarea;
      `grant_curation` (`{user, workareas}`) creates or extends `<user>-curate` with ADR-0044 d7's
      grants. Extending a key means adding a grant, which is designed and not built
      (`POST /v2/keys/{key}/grants`); until it is, each grant is fixed when the key is created.
    - **The review queue's depth and age**, through `<user>-admin`: held-item counts and the
      oldest held item's age per workarea, tenant-wide, with no content, the numbers the designed
      `GET /v2/review/queue` serves. An admin key reads them by role as structure, like the tree
      (d16).
    - **Memories and attribution**, through `<user>-admin` and the admin read of d21: a search
      over every memory of the tenant, every state, every workarea and every user root, with its
      text, and for one memory its attribution — the authoring key, its principal and user, the
      workarea path, `derives_from`, `replaces`, the verdicts with their reviewing keys and
      times, and the digests. A universe claim is shown with its source: ADR-0044 d9's served
      view is for the keys that use memories, and the admin read is not one of them.
    - **Review**, through the signed-in admin's own `<user>-review` alone (d11).
    - **Curation** and **supersession**, through `<user>-curate` and `<user>-review` (d12).
    - **Demotion**, planned for v0.6.0 (d12).

    Reading is not acting on what is read: the admin key reads every memory and still records no
    verdict, writes no memory and is never served a held row for review (d11). Every admin read
    is journalled (d21).

- **d11 — Review from either page is the served ceremony under the person's reviewer key, and
  R2 is the server's check on that key and the row's lineage closure.** `review_next` calls the
  use case behind `POST /v2/review/next` as `<user>-review` and renders everything it serves, the
  text, the version a supersession replaces and, once attachments are built, every attachment;
  the form carries the `serve_id`. `verdict` (`{serve_id, verdict, reason}`) calls the verdict use
  case as the same key, with an idempotency key the page mints per served item so a resubmitted
  form replays rather than repeats. The server refuses, as for every reviewer: a row whose
  lineage closure holds the reviewing key or its principal (`review.author_in_lineage`, never
  served to it), a serve that is stale, consumed or another key's (`review.serve_invalid`), and a
  row outside the key's dreamer access. Since ADR-0044 d8 is dropped, the person behind the
  key is not in the exclusion set: a person may approve their own agents' writes through
  `<user>-review`, and the journal shows the reviewing key, which names no user, beside the
  authoring key, which does. That is R2 as the constitution states it — the reviewing key is
  never in the row's closure — and it is the cost the maintainer chose. The admin key is never
  offered a verdict: it holds no dreamer grant (`access.role_exclusive` at issuance) and the
  page has no path that would use it. A page that is opened again shows the item the key holds,
  served again; the serve binding, not the browser, decides what a verdict judges, and
  `no-store` keeps the item out of the browser cache, which answers ADR-0015's fifth reason.
  The development exception of ADR-0001 d5 is untouched: the page offers no self-signature.
  The cost is that a reviewer reaches beyond their own root only where an admin granted it, per
  workarea.

- **d12 — Curation, supersession and demotion run as ordinary acts of the person's own keys, and
  none of them deletes.**
    - **Curation** (`curate`, `{source, text, claim_key?}`), through `<user>-curate`, is
      ADR-0044 d7's held write into `universe` with `derives_from` naming the source, a memory
      someone other than its author has approved, since a held row is readable by its author
      alone, the text
      prefilled from the source with a line that a curated memory whose text or name repeats its
      source's can be linked to it (FR-73). The second approval is an ordinary review of the held
      universe version by a dreamer key with dreamer access on `universe` whose key and principal
      lie outside its lineage closure, which holds the curator key and the source's author: the
      person's own `<user>-review` qualifies, being neither, which lets a deployment of one
      person curate. The page shows the curated version as held until that review.
    - **Supersession** (`supersede`, `{memory, text, retirement?}`): a curator supersedes its own
      universe claims through `<user>-curate` (writer, own claims); any other memory the person's
      reviewer key reads is superseded through `<user>-review` as dreamer(propose). Each is the
      served `POST /v1/memories/{artifact_id}/supersede` use case: a held version that another
      key reviews, the old version staying the tip until then ([ADR-0025](ADR-0025-the-agent-only-memory-surface.md)
      d10). A retirement is the same act with `retirement: true` (ADR-0025 d11).
    - **Attribution, in v0.5.0.** The attribution view is the admin read of one memory (d21):
      its text beside the journalled authoring key, principal, user, workarea path,
      `derives_from`, `replaces`, the verdicts with their reviewing keys and times, and the
      digests, through `<user>-admin`. It is the per-memory decision trail of the evidence export
      ([ADR-0015](ADR-0015-cli-administration.md) d4) served for one memory, which the export,
      not scheduled, does not wait on.
    - **Demotion, planned for v0.6.0.** `demote` (`{memory, reason}`) runs the designed
      `POST /v2/memories/{artifact_id}/demote` as `<user>-admin`: journalled, bytes kept (R4). It
      does not ride the admin read: a read adds a route and an event, while a demotion changes
      which version is the tip, what recall serves and what the lineage of every successor
      reads, a use case of its own with its own refusals. The page does not ship it before its
      verb.

    No page has a delete, an erase, a reassignment, a tip override or a ban: those stay CLI
    ceremonies with their dry-run previews ([ADR-0015](ADR-0015-cli-administration.md) d3).

- **d13 — Removing an email ends its sessions and nothing else; an admin cuts off the person's
  agents by name.** The removal commit ends every session of the entry (d5). When it was the
  user's last entry, the user's session keys become unreachable, because no session can name
  them (d6). The user's agent keys — the ones the person issued and the ones an admin issued for
  them — keep working: the allowlist governs who signs in to the pages, not what the person's
  agents may do, and coupling them would let a mistyped or replaced address stop a person's
  agents with no warning. The removal confirmation lists those keys with a `cut_off` beside each,
  run as `<user>-admin`'s global denial on `universe`. When revocation is built, the same list
  offers it. The cost is a second deliberate step to offboard a person completely, which the
  confirmation puts in front of the admin.

- **d14 — A web request writes the `http_request` line with credential `session` and the acting
  key, and no token, code, cookie, `state` or email ever reaches a log.** The line keeps every
  field of [ADR-0042](ADR-0042-every-request-logs-one-line-under-an-id-the-caller-may-name-and-metrics-stay-off-the-public-port.md)
  d2; for a web route `credential` is `session` once a session resolved and null before, the key
  fields name the acting session key (`<user>-web` on `GET /web`, `<user>-admin` on
  `GET /web/admin`, and on a `POST` the key the `action` used), `connection_id` is null, and two
  fields join: `web_action`, the `action` from the closed set of d1, and `web_provider`, `google`
  or `github` on the sign-in and callback routes. `route` is the template, so
  `/web/callback/{provider}`, never a query: the callback's `code` and `state`, and the
  `Location` of `POST /web/sign-in`, which carries `state` and the PKCE challenge, are never
  logged, as the connect page's `Location` is not. A callback also writes one `web_sign_in`
  line under the request's ids with `provider`, `outcome` (`signed_in` or d4's reason),
  `tenant_id` and `user` when admitted, and never the email or the provider subject. The proxy
  rule of the connect page extends to `/web/callback/*`: its access log records no query
  ([deployment](../operations/deployment.md#connections-behind-a-reverse-proxy)), stated, not
  checkable. The cost: the log cannot say which email failed a sign-in, and the admin learns
  that from the person.

- **d15 — Rate limits: sign-ins open nothing, provider calls are capped, and page acts are the
  acting key's limits.** A sign-in start writes no row and a callback with a cookie that does not
  open, or with the wrong `state`, makes no outbound call (d3). An admitted callback makes at
  most one token call and, for GitHub, two API calls, each under a per-provider cap on concurrent
  calls and a time limit, beyond which the callback answers `limit.rate` with `Retry-After` on
  the page; the cap and the time limit are stated by the build and unmeasured (FR-76). An
  attacker who starts flows of their own can make the lead call the providers' token endpoints
  with codes the providers refuse, at the rate the cap admits. No limit keys on a source address:
  behind a proxy it is the proxy's, the rule the connect page follows. A page act is a key's act
  and falls under that key's rate-limit grant attributes when those are built
  ([FR-8](../product/roadmap.md)); until then a session's acts are bounded only by the request
  rate the deployment serves, and a person can issue keys in their own root without a count
  limit.

- **d16 — The surface grows by six web routes, five lab routes, one credential kind, one
  journal event for reads, and two readings of existing verbs; no MCP tool changes.**
    - `WEB_ROUTES` (d1), the three `/v2/web/emails` routes (d2) and the two admin read routes
      (d21), in the designed annex until built; each web route's answers are `text/html` pages or `303` redirects, as
      `/oauth/authorize`'s are.
    - `credential: session` on the key record, on `GET /v2/keys/{key}`, on `GET /v2/whoami` (which
      no session key can call, having no value) and on the request line.
    - **Amending ADR-0010 d1 and ADR-0044 d6:** a session user key (`<user>-web`, writer) may
      issue reader and writer keys and connector keys naming its own user with grants inside its
      root, mint connect codes for them, and deny them on `universe`. No bearer user key gains
      these verbs, so an agent still never mints a key.
    - **Amending ADR-0025 d3:** for a session user key, own means every key naming its user, so
      the user page shows the held and rejected rows of the person's agents. A bearer key's own
      stays its authoring key.
    - **Amending ADR-0026 d15:** an admin key reads, by role and as structure, the review queue's
      held counts and oldest ages per workarea, tenant-wide; and it reads every memory's text and
      attribution through the admin read of d21, journalled. "The admin key never reads content"
      no longer holds for the admin key on those two routes; every other route keeps it, so an
      admin key still recalls nothing, is served nothing by `GET /v1/memories/{artifact_id}`, and
      reads no content over MCP, where it has no tool.
    - **Amending ADR-0044 d9 for the admin read alone:** the anonymous served view of a universe
      claim is the view of the keys that use memories; the admin read of d21 shows the claim's
      authoring key, principal, user, source workarea and `derives_from` in the clear, as the
      journal already holds them.
    - **Amending ADR-0031 d7:** a connector key may be issued by a session user key (above), and
      a session key is never a connector key.
    - **Amending ADR-0042 d2:** `credential` takes `session`; `web_action` and `web_provider` join
      the line on web routes.
    - **Amending ADR-0044 d11:** its "no HTTP route is added" holds for ADR-0044's own fields;
      this record adds the routes above.
    - **Amending ADR-0010 d7:** human sign-in is built for the pages, through Google and GitHub;
      the CLI keeps bearer keys.
    - The CLI gains `oml web allow <email> --user <user> [--admin]`, `oml web list` and
      `oml web remove <email>`, over the three lab routes, and `oml admin read [<memory>]
      [--query <text>] [--workarea <workarea>]` over the two admin read routes, so the CLI stays
      complete for administration.

- **d17 — Settings: two providers' client credentials at boot, the public origin from
  `oauth.issuer`, and one journalled session lifetime.** Boot configuration, read at process
  start: `web.google.client_id` (`OML_WEB__GOOGLE__CLIENT_ID`), `web.google.client_secret`
  (`OML_WEB__GOOGLE__CLIENT_SECRET`), `web.github.client_id` (`OML_WEB__GITHUB__CLIENT_ID`) and
  `web.github.client_secret` (`OML_WEB__GITHUB__CLIENT_SECRET`). A provider is on when both of its
  values are set; one without the other stops the process at start, as does a provider set while
  `oauth.issuer` is unset, because the callback URL is `<oauth.issuer>/web/callback/<provider>` and
  the page origin is the issuer's: one public origin, already validated, with no second setting
  to disagree with it. The secrets are read from the environment or the file like every boot
  key, never logged, and `oml settings show` prints them as set or unset. Each provider's
  application is registered by the operator with that callback URL exactly. The journalled
  deployment setting `web.session_ttl` bounds a session's life: shipped default 8 hours, from 1 to
  24 hours, a value outside the range `settings.invalid`. The provider endpoints are constants,
  not settings: a setting that pointed sign-in at another host would let a misconfiguration
  admit any address that host calls verified.

- **d18 — Every failure answers a named refusal and has a doctor line.**

    | Failure | Refusal | Doctor line |
    |---|---|---|
    | a provider's id without its secret, or a provider without `oauth.issuer` | the process stops at start, naming the setting | — |
    | no admin entry on the tenant's allowlist while a provider is on | — | `oml doctor` (admin key): `warn web: no admin email; add one with oml web allow <email> --user <user> --admin` |
    | an email not on the list, unverified, with a stale or foreign `state`, or a moved account | `auth.invalid`, `details.reason` per d4, on the page | — (the person reads it; d14 logs the outcome) |
    | the provider's token or email call fails, or the cap is reached | `auth.invalid` `provider_refused` / `limit.rate`, on the page | `oml doctor` (operator key): `warn web_provider_<p>: <n> failed exchanges since start, last <reason>` from the lead's counters, read through `/readyz`'s web section |
    | a `POST` from another origin, or without a valid `csrf_token` | `oauth.origin_refused` / `request.validation` (`csrf_token`), journalling nothing | — |
    | an `Authorization` header on a web route | `access.verb_ceiling`, `web_is_human_only` | — |
    | a session expired or ended | the sign-in page, `auth.invalid` `session_expired` on a `POST` | — |
    | a non-admin session on the admin page | `access.not_found` | — |
    | a composed key name already taken, at entry creation | `name.taken`, kind `key` | — |
    | a curated claim with no reviewer key outside its lineage on `universe` | — (held, failing closed) | `oml doctor` review availability, as ADR-0044 d14 states for `universe` |
    | an allowlist entry whose user has no `<user>-web` (its first sign-in failed part-way) | — | `oml doctor` (admin key): `warn web: <user> has no session key; it is created at the next sign-in` |
    | a session key used outside a session (a defect) | the lead refuses it, `auth.invalid` `session_key` | `oml doctor` names the key |

- **d19 — The vocabulary and the wire are held here until built, and the build is one change set
  in four steps.** [ADR-0018](ADR-0018-the-documentation-set.md) d4 gains three rows:

    | Term | Means | Never |
    |---|---|---|
    | user page / admin page | the two pages of d1 | "dashboard", "portal", "account page" |
    | allowlist entry | an email, the user it signs in as and its admin flag (d2) | "whitelist", "account" |
    | session key | a key with credential `session` that the server holds for a person and acts with from their session (d6) | "web token", "service account" |

    The new `details.reason` values and the extension of `oauth.origin_refused` to the web routes
    enter [errors](../api/errors.md) with their step. The steps, each closed by its tests:

    | Step | Builds | Tests, named after the sentence each proves |
    |---|---|---|
    | W1 | d2, d17: the allowlist, its routes and verbs, the settings | `the_first_admin_email_is_added_by_the_cli_under_an_admin_key`, `an_email_is_unique_in_the_deployment`, `a_user_named_like_a_shared_workarea_is_name_taken`, `a_provider_without_its_secret_stops_the_process`, `the_journal_holds_an_email_digest_and_never_the_address` |
    | W2 | d3, d4, d5, d7, d8, d14, d15: sign-in, sessions, the headers, the log | `a_callback_without_the_sealed_state_makes_no_outbound_call`, `a_google_token_with_email_verified_false_is_refused`, `github_signs_in_only_the_primary_verified_email`, `a_moved_account_is_refused_as_identity_changed`, `no_session_exists_before_the_callback_admits`, `removing_an_email_ends_its_sessions`, `a_post_without_the_csrf_token_journals_nothing`, `an_authorization_header_on_a_web_route_is_verb_ceiling`, `a_session_cookie_on_the_api_is_auth_missing`, `no_log_line_holds_a_code_state_cookie_or_email`, `the_lead_serves_exactly_web_routes` |
    | W3 | d6, d9, d13, d16: session keys and the user page, review of one's own agents | `the_first_sign_in_creates_the_users_root_and_web_key`, `the_first_sign_in_issues_a_review_key_on_the_users_root`, `a_person_approves_their_own_agents_write_on_the_user_page`, `a_session_key_has_no_value_and_no_connect_code`, `a_user_issues_a_key_only_inside_its_root`, `a_field_the_action_does_not_define_is_refused`, `the_user_page_shows_its_agents_held_rows`, `removing_the_last_email_leaves_the_agents_keys_working` |
    | W4 | d10, d11, d12 (v0.5.0 part), d21: review, curation, supersession, the admin read and attribution | `the_admin_read_serves_text_and_attribution_and_journals_the_read`, `the_admin_read_shows_a_universe_claims_source`, `an_admin_key_still_recalls_nothing`, `review_on_the_admin_page_acts_as_the_persons_reviewer_key`, `the_admin_key_is_never_offered_a_verdict`, `a_reviewer_key_in_the_lineage_is_never_served_the_row`, `a_curated_claim_is_held_until_a_key_outside_its_lineage_approves`, `a_non_admin_session_gets_not_found_on_the_admin_page` |

    Demotion follows in v0.6.0 with its verb.

- **d20 — The maintainer's answers of 2026-11-04 to this record's open choices.** Each is folded
  into the decision it governs; this list is where each answer is found.
    - **(a) An admin reads everything.** The admin session reads every memory's text and full
      attribution directly, from v0.5.0, through `<user>-admin` and the admin read (d10, d21).
      This reverses the first draft's narrowing, amends ADR-0026 d15 for the admin read (d16),
      and sets ADR-0044 d9's anonymous view aside for it (d16). The attribution view moves into
      v0.5.0; demotion stays in v0.6.0 (d12).
    - **(b) Removing an email ends its sessions; agent keys keep working;** the confirmation
      lists their keys with a cut-off beside each (d13), as drafted.
    - **(c) A person issues their own keys and connect codes inside their own root** (d9), as
      drafted.
    - **(d) Sessions last 8 hours absolute, with no idle timeout, under `SameSite=Lax`** (d5,
      d17), as drafted.
    - **(e) A person reviews their own agents' work.** Every signed-in person holds
      `<user>-review` with dreamer and reader on their own root from their first sign-in, with
      no admin step (d6, d9); R2 stays per key and per lineage closure (d11).
    - **(f) Tenancy.** The first clients sign in to the existing `oml` tenant, a root each; no
      tenant is created per client, since organisations beyond the first are not scheduled.
    - **(g) Scope.** v0.5.0 is this record and ADR-0044 alone. Guardrails (ADR-0037) move to
      v0.5.1; the workarea lifecycle (ADR-0009), links between memories (ADR-0008, ADR-0021)
      and the workarea brief, which rests on those links, move to v0.5.2
      ([releases](../product/releases.md)).

- **d21 — The admin read is two lab routes under the admin key role, and every admin read is
  journalled as `memory.admin_read`.**
    - `POST /v2/admin/memories/search` takes `{query?, workarea?, state?, limit?, cursor?}` and
      answers a page of memories of the key's tenant — every state (held, promoted, rejected,
      superseded), every workarea, every user root and `universe` — each with its text and its
      attribution summary: `artifact_id`, `claim_key`, `version`, `state`, `workarea_path`, the
      authoring key's name, principal and user, and `created_at`. `query` is the full-text
      search recall uses, without the visibility filter; `state` and `workarea` narrow it. It is
      a `POST` so that the query text travels in a body, never in a path or query string a proxy
      logs.
    - `GET /v2/admin/memories/{artifact_id}` answers one memory with its text and full
      attribution: the summary's fields and `derives_from`, `replaces`, the submission's
      `idempotency_key` digest, every verdict with its reviewing key, principal, reason and time,
      and the `plaintext_digest`.
    - **Who:** the admin key role alone. The operator, which is provider-wide and owns no
      tenant's content, is refused `access.verb_ceiling`; every other role is refused
      `access.verb_ceiling`, so no agent key reaches it (R1). An admin key reads only its own
      tenant (ADR-0007).
    - **Journalled, not only logged.** Each answer writes one `memory.admin_read` event, actor
      the admin key, payload the `artifact_ids` served (a search page's ids, or the one id) and
      for a search a `query_digest`, never the text or the query; the event joins the events
      tail for admin keys alone. It is journalled because the journal is the signed, append-only
      record an author or an auditor reads later, and an admin reading content no grant gave is
      the act most likely to be questioned; the request log is unsigned, kept 30 days, and names
      the route and not the memories served. The cost is a journal write per admin read, so the
      routes run on the lead alone, and an admin who browses a lot grows the journal by one
      event per page.
    - **What it never does:** it never records a verdict, never consumes or creates a serve
      binding, never changes a tip, and never reaches `oml_read`, recall or any MCP tool. The
      admin page renders it; the CLI's `oml admin read` calls it.

    The cost: an admin of a tenant reads every person's memories, held ones included, and the
    people whose agents write into the tenant must be told so; the pages and the security model
    say it plainly, and the journal shows every read.

- **d22 — The Memories view on both pages is a list of cards, server-rendered, paged by keyset
  and filtered by five fields; the maintainer's decision of 2026-11-06.** The view `memories` of
  `GET /web` and of `GET /web/admin` renders one page of memories as cards in the review card's
  form (d11), compact as d32 draws it: the state, the version, the text and the author, with
  the provenance and the facts (workarea, claim key, anchor, format, valid time) behind Details.
  A held row says so in its state. The page carries no script (d8).
    - **Paging.** Rows run newest first by `(created_at, artifact_id)` descending, the order every
      listing of http-api.md already uses, and a page resumes strictly after the last row shown,
      never at an offset. A row written while a person pages lands before the first page and is
      never shown twice or pushed off a later one. The page size is `limit`, 50 by default and
      at most 200, `api.page_limit_max`'s shipped value; a value outside 1 to 200 is
      `request.validation` with that field. The next page's link carries the opaque `cursor`
      the listing issues, bound to the acting key and to `api.cursor_ttl` (ADR-0025 d13); the
      view shows no count of matching rows, since a count would be a second read and, on the
      user page, a number the person's keys could not otherwise learn.
    - **Filters,** each a query parameter kept across pages: `state` (`held`, `promoted`,
      `rejected`, `superseded`, `demoted`), `workarea` (by name or id, the subtree it roots),
      `origin` (one `origin.kind` of [ADR-0063](ADR-0063-a-chat-turn-becomes-memory-only-when-the-agent-in-it-writes-the-fact-and-every-memory-says-what-kind-of-source-it-came-from.md)
      d1), `author` (a key name) and `q` (text). A filter value outside its grammar is
      `request.validation` naming the field; a workarea or key the acting key may not name is
      `access.not_found`, as on the API.
    - **Text.** On the user page `q` runs the recall path itself (`POST /v1/recall`'s use case
      as `<user>-web`, unreviewed rows included, where "own" is every key naming the person's
      user as d16 reads it for a session user key), so the view finds what the person's agents
      would find and nothing more; its answer is one ranked page of at most 100 rows, recall's
      own bound, with no next page, and the other filters narrow that page. On the admin page
      `q` is the admin read's full-text match (d21), which keeps keyset order and pages.
    - **One memory** still opens by `memory=<artifact_id>`, as d9 and d10 state.

- **d23 — The user page lists what the person's session key may read and the held rows of the
  person's own keys, and nothing else.** The page runs as `<user>-web` (d6) and merges two
  listings the API already serves to that key, each re-applying its scope on every page:
    - the scoped listing at `universe` (ADR-0039 d4): the promoted tips on the nodes where the
      key's reader access is effective, which for `<user>-web` is the person's root and the
      `universe` node ADR-0044 d3 adds, with ADR-0044 d9's anonymous view of a universe claim;
      inside an in-progress workarea it also holds every author's held rows there, as
      [ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
      d2 and d3 serve them to a reader inside;
    - the own-work listing, every state, of every key naming the person's user (d16's reading
      of ADR-0025 d3 for a session user key).

    Both are read after the same position and merged in the listing order, a row in both shown
    once, so one cursor pages the union with no gap and no repeat. A held row of another
    author outside an in-progress workarea is in neither listing, and neither is any row of
    another tenant ([ADR-0001](ADR-0001-the-constitution.md) R3, ADR-0007). `author` narrows the
    own-work listing to one of the person's keys and drops the scoped listing, so it never
    names an author the anonymous view hides; a key name that is not the person's is
    `access.not_found`. The view journals nothing: it is a read of the person's own keys, as
    the API's listing is.

- **d24 — The admin page lists through the admin read, and every page it serves is one
  `memory.admin_read` event.** The view runs `POST /v2/admin/memories/search`'s use case as
  `<user>-admin` (d21) with the filters of d22, `origin` and `author` joining `query`,
  `workarea` and `state` in the use case; the lab route and the CLI's `oml admin read` keep
  their fields, and gain the two when a client asks for them.
  Each page served, a list page or a JSON page (d25), writes exactly one `memory.admin_read`
  event naming the page's `artifact_ids` and, for a text query, its `query_digest`; an empty
  page included, since the search was the admin's act, and never one per row. Opening one memory stays d21's single read.
  The admin page's list is the tenant's every memory, every state and every root, as d10 says.

- **d25 — The Atlas view draws the workareas of a scope as a treemap on the server, and the
  same page of memories is served as JSON from a same-origin route under `/web` with the list's
  scope and journal.** Beside the list, a link `display=atlas` opens the Atlas of the page's
  `workarea` (the person's root on the user page, `universe` on the admin page, when none): the
  design system's AtlasScreen, its workareas as one squarified treemap (Bruls, Huizing and van
  Wijk) whose areas are readable counts (d30), drawn by the server as SVG. Each region is a
  plain link that zooms the Atlas to it; a breadcrumb leads back out; the scope's panel shows
  its counts, a link to its memories and d28's dropdown. Two routes join `WEB_ROUTES`:

    | Route | Answers |
    |---|---|
    | `GET /web/data/memories` | the user page's page of d22 and d23 as JSON |
    | `GET /web/admin/data/memories` | the admin page's page of d22 and d24 as JSON |

    Each takes the view's query parameters and runs the same use case as the list with the same
    session key: the user route journals nothing, and the admin route writes the one
    `memory.admin_read` of d24 for the page it serves. Each answers `{items, next_cursor,
    workareas, base}`, every item holding only fields the card shows (`artifact_id`, `state`,
    `workarea`, `version`, `title`, `text`, `claim_key`, `author`, `author_kind`, `anchor`,
    `format`, `valid`), with `author` null wherever the card shows the shared area, and
    `workareas` the regions of d30. The Atlas page itself reads no memory, only counts, so it
    journals nothing. The routes take the session cookie alone: an `Authorization` header is
    `access.verb_ceiling` `web_is_human_only` (d7), no session is `access.not_found` as JSON,
    and every answer carries `Cache-Control: no-store` and `nosniff`. A `GET` changes nothing,
    so it needs no CSRF token; a cross-site page cannot read the answer without CORS, which the
    routes never send, and `SameSite=Lax` withholds the cookie from a cross-site `fetch` (d5).

- **d26 — No view runs script: the CSP of d8 is unchanged.** The treemap, its labels and its
  state bars are SVG elements and attributes the server writes, styled by classes of the one
  hashed stylesheet; positions are SVG geometry, never inline `style`, so `default-src 'none'`
  holds on every view, the Atlas included, and nothing is added to `script-src`, `connect-src`
  or any other directive. The size of the map follows the viewport by CSS alone: a 16:9 map,
  and a 4:3 one below 600px. **Risk:** none added; injected markup still runs nothing anywhere,
  and the session's origin serves no script but the Mermaid frame's sandboxed one (ADR-0056 d9).

- **d27 — The list is the accessible default, and the Atlas has a table twin.** The view opens
  as the list; the Atlas is reached only by its link. Every region carries a `title` and an
  `aria-label` that spell its name, count and states, and keeps at least a cut name when it is
  too small for its whole name; below the map a table holds the same rows (workarea, status,
  readable count right-aligned in tabular figures, and the counts by state as a bar and as
  words). Each state is a word, a glyph and a colour, never colour alone.

- **d28 — On both pages a workarea's acts are one dropdown of exactly what the acting key may do
  there now, applied through a confirmation; the operator's decision of 2026-11-06.** The
  Workareas view of the user page, and a new Workareas view of the admin page over the tenant's
  tree, which an admin key reads as structure (ADR-0026 d15), give each workarea one `select`
  and one Apply button. The options are computed by the server from the same checks the verbs
  run, factored so that the verb and the offer call one function, and an option is offered
  only where that function admits it now:
    - **status** ([ADR-0009](ADR-0009-status-ladder.md)): "propose committed" or "propose
      validated", only the one legal next rung, where `propose_transition` admits it; and for
      each live proposal "ratify development → committed", only where `ratify_transition` admits
      the acting key, so never to the proposing key or principal (R2,
      `review.ratifier_is_proposer`). On the user page they run as `<user>-review`, the person's
      dreamer key; on the admin page as `<user>-admin`, which ADR-0009 admits by role. A
      proposal is shown as proposed until another key ratifies it.
    - **in progress** ([ADR-0030](ADR-0030-an-in-progress-workarea-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
      d1): "put in progress" where `set_workarea_mode` admits it, as `<user>-review` on both
      pages, a dreamer key a person holds; never offered to another key.
    - **commit** (ADR-0030 d6): "commit" on an in-progress root, as `<user>-review`. While held
      rows remain it is shown disabled with their count, "commit — 3 memories still held", and
      cannot be submitted.
    - **code parent** ([ADR-0064](https://github.com/e8kor/open-memory-lab/pull/97) d3, open):
      marking or unmarking a code parent joins the dropdown as `<user>-review` once ADR-0064's
      mark lands on main; this build has no such verb and offers nothing for it.

    Apply is a `GET` of the view with `confirm=<option>`: it changes nothing, recomputes the
    options and, if the chosen one is still offered, shows what it will do and one `POST` form
    with the CSRF token that runs it; if not, it shows the named refusal the verb would answer.
    The `POST`s are web actions over existing use cases, so no verb is new:
    `workarea_transition` (`{workarea, to}`, `propose_transition`), `workarea_ratify`
    (`{workarea, transition}`, `ratify_transition`) and the existing `workarea_mode`; each joins
    both pages' closed action sets. A refusal at the `POST`, from a race since the confirmation,
    is the verb's named refusal on the page. `propose_transition`, `ratify_transition` and
    `workarea_status` take a session key's caller as every other use case the pages call does;
    their checks and refusals are unchanged.

- **d29 — What this amends and adds.** `WEB_ROUTES` grows from six to eight (d25), and their JSON route also answers the Atlas's workareas (d30); the user
  page's actions gain `workarea_transition` and `workarea_ratify`, and the admin page's gain
  those two and `workarea_mode` (d28); the admin page gains a Workareas view; the admin read's use case gains `origin` and
  `author` (d24), its lab route unchanged. d1's decline of "graph pictures"
  is narrowed to everything but the Atlas of d25; d7's and d8's "no script" stands on every
  view (d26). No MCP tool, verb, journal event or table is added, and no migration.
  No new failure is the operator's: the map is drawn by the server with its table twin (d27),
  and every refusal is an existing one, so no doctor line is added. Built in the same change, tested
  by `the_web_memories_list_pages_by_keyset_with_no_duplicate_or_gap_under_inserts`,
  `the_user_page_never_lists_another_authors_held_row_outside_in_progress_or_another_tenants_row`,
  `the_json_route_answers_exactly_the_lists_rows`, `every_admin_page_of_memories_is_one_admin_read_event`,
  `the_atlas_view_is_a_treemap_drawn_on_the_server_with_no_script`, `the_filters_narrow_the_list`,
  `the_atlas_treemap_draws_and_zooms_with_no_script_in_chromium`,
  `the_workarea_options_offered_are_exactly_the_acts_allowed`,
  `a_workarea_act_is_confirmed_before_it_runs` and
  `the_atlas_draws_and_counts_only_the_workareas_and_rows_the_viewer_may_read` (d30).

- **d30 — The Atlas draws the workareas the viewer may read as nested regions, and only those;
  the operator's decision of 2026-11-06.** The JSON route of d25 also answers `workareas`: the
  nodes the tree read serves the page's reading key under the page's `workarea` filter
  (`universe` when none), so `<user>-web`'s root, its children and the `universe` node on the
  user page, and the tenant's tree, which an admin key reads as structure, on the admin page.
  Each carries its id, parent, name, path, status, whether it is in progress, and its rows by
  state: approved (promoted tips), held, in progress (held inside an in-progress scope) and
  other. The counts are taken from the same listing the list pages through, as the same key,
  so a region and a count exist only where the list would show the row: a held row of another
  author is counted only inside an in-progress scope (ADR-0001 R3; ADR-0030 d3), and a node
  the key may not read is neither drawn nor counted, and filtering to it is `access.not_found`.
  On the admin page the counts are the admin read's scope and no wider; they carry no content
  and journal nothing, as the queue's depth does (d16), while the page of rows beside them is
  d24's one `memory.admin_read`. Counting stops after 2,000 rows a view (chosen, not measured)
  and says the counts are at least those shown. The map shows the scope's children, each
  sized by its subtree's readable rows, and the scope's own rows as one more region; zooming
  into a region makes it the scope. Labels drop by priority as a region shrinks: name and count,
  then its state bar, then its in-progress mark; the table twin of d27 holds every row,
  regions with no readable row included. An in-progress workarea is drawn with its mark and
  counted from a read scoped inside it, where the person reads every author's held rows
  (ADR-0030 d3). An `ontology` mark joins when a workarea can say it is an ontology crew's
  (ADR-0027, not on main yet).

- **d31 — Each section of both pages is a short heading, its control and at most one muted line;
  the operator's decision of 2026-11-06.** Explanations that change no decision move behind a
  `?` disclosure or to the guides. The lines kept are the ones a person acts on: shown once,
  held until someone else reviews it, works once until a time, what removing an address ends.
  Buttons are verbs, headings nouns. The pages' words fall by about a third (counted on the
  views of the Chromium test).

- **d32 — The pages take the design system's forms; the operator's decision of 2026-11-06.** The
  Memories card is the design system's Round 3 compact row ([ADR-0069](ADR-0069-oml-s-design-system-is-the-source-of-truth-for-every-surface.md)
  d3): its state as glyph and one word and its text, clamped, then its author's kind (`agent`,
  `person`) and key and its version bar; the provenance timeline (written, held, reviewed,
  served, a step not yet taken marked waiting) and the facts (id, workarea, claim key, source,
  format, valid time) fold inside it, closed by default. The filters are labelled in plain
  words (Text, State, Workarea, Source, Author). The Atlas is AtlasScreen's treemap (d25, d30)
  in the shared rail layout. The shared stylesheet (`tokens.css`, `web.css`) styles the row,
  the rail, the panel, the lifebar and the legend; the filters and the treemap have their own
  `memories.css` after it, `var(--…)` only. The user page's nav is the design system's (Start,
  Memories, Review, Atlas, Keys), then Workareas and Guardrails. The design system
  places no lifecycle acts on the Atlas; the operator's d30 does, so the scope's panel carries
  d28's dropdown.

## Consequences

- **ADR-0015's reasons, answered one by one.** *Accretion:* the route set and each page's actions
  are closed and tested (d1), and the console's usual accretions are declined here by name.
  *Origin policy, CSP, frames:* built in v0.4.0 for the connect page, reused (d8). *Sessions:* new,
  paid for in d5, with their residual stated. *A second presentation to keep truthful:* each page
  renders what an existing use case returns, through the same keys, and holds no state of its
  own but sessions and the allowlist; every administrative ceremony keeps its CLI home, and the
  CLI remains complete for administration, the self-service acts of d9 aside, which exist only
  for session keys. *Nothing load-bearing needs a browser:* still true for operators and admins;
  ADR-0044's people are the new population, who otherwise depend on an admin for every key, code
  and workarea. *Review against a cache and a stale tab:* the serve binding is server-side and the
  pages are `no-store` (d11). ADR-0015's rejected alternatives on an admin API and on
  administration through MCP stand unchanged: the pages call the same use cases, and MCP reaches
  none of them.
- [cli/reference.md](../cli/reference.md), which owns D14, the [HTTP API](../api/http-api.md),
  [configuration](../operations/configuration.md), [security](../architecture/security.md),
  [releases](../product/releases.md), [llms.txt](../llms.txt), the
  [architecture overview](../architecture/overview.md), [positioning](../product/positioning.md),
  the [review guide](../review-guide.md), the [critical path](../product/critical-path.md) and the
  [home page](../index.md) mark the pages planned and not built. The register's D14 row cites this
  record. The event catalog, the Postgres schema and openapi.yaml change only with the build;
  the designed annex holds the eleven routes now, as it holds every designed operation.
- ADR-0010 d1 and d7, ADR-0015, ADR-0025 d3, ADR-0026 d15, ADR-0031 d7, ADR-0042 d2 and ADR-0044 d6,
  d9 and d11 keep their text; each Status line points here.
- The roadmap opens **FR-75**, recording when each key was last used, with its cost measured, and
  **FR-76**, the pages unverified against the live providers and unmeasured: no sign-in has run,
  whether GitHub enforces PKCE is unverified, the provider-call cap and time limit are unmeasured,
  and no penetration test has run. Human review throughput through the admin page joins the
  unmeasured review throughput of [FR-2](../product/roadmap.md).
- The lead's outbound reach grows by Google's and GitHub's token and API hosts. A deployment
  whose egress policy blocks them has the pages and no sign-in, which the d18 doctor line names.
- A tenant of one person can run every page: the person is an admin, a user, a reviewer and a
  curator through four keys, and every act shows which.
- An admin reads every memory of the tenant, and the first clients share the maintainer's `oml`
  tenant, a root each (d20 f): the maintainer, as that tenant's admin, can read every client's
  memories, and every such read is journalled (d21). A client who needs memories no other person
  can read needs a tenant of their own, which waits on organisations beyond the first.
- [ADR-0026](ADR-0026-names-are-keys.md) d15's sentence that admin keys never read content
  is narrowed by d21 to every route but the admin read; the guides, the security page and
  positioning say so wherever they repeat it.

- **The Memories list, the Atlas view and the workarea dropdowns (d22–d32).** The pages gain
  two JSON routes and the Atlas, which runs no script; the user page's scope is unchanged, being
  two listings the API already serves to `<user>-web`, and the admin page's list journals per
  page as d21 does. The
  workarea dropdowns add no verb: they offer what the verbs would admit, and the verbs still
  decide.

## Rejected alternatives

| Alternative | Why not |
|---|---|
| **Keep the decline; leave people to the CLI and an admin** | ADR-0044 makes every person a user with a root of their own. Without a page, every key, connect code and workarea a person needs is an admin's CLI act on their behalf, and a person on a phone has no CLI. |
| **A console as its own service or a backend-for-frontend** | A second process holding its own state between a person and the server, which ADR-0015 rejected for good reason; the lead already serves an HTML page with the controls a console needs. |
| **A single-page application with a JSON client** | Script on the origin that holds the signing key, a bundle to build and audit, and a token the script can read; server-rendered forms need `default-src 'none'`. |
| **Passwords, or sign-in links by mail** | A password store to protect, and outbound mail to run; the maintainer ruled both out. |
| **No session: a sealed handle per form, as on the connect page** | Every page view would need a fresh provider sign-in; a session row is the cost of browsing at all. |
| **One session credential acting with the person's combined authority, no keys** | A second authorization story beside keys and grants, and an actor the journal cannot name as a key; it would also merge admin and reviewer, against ADR-0010 d6. |
| **Session keys with a bearer value the server keeps** | A stored usable secret where every other key is a digest; a valueless key cannot leak a value. |
| **Reviewing through the admin key, or a page-wide reviewer** | The admin key is never a reviewer (ADR-0010 d6); one shared reviewer would put every person's verdict under one key and make R2 per key meaningless. |
| **Content on the admin page read only through the admin's own reviewer key, granted reader on `universe` (this record's first draft)** | The maintainer chose otherwise on 2026-11-04 (d20 a): an admin answers for every memory in the tenant, and a reader grant an admin gives their own key is the same authority with one more step, while a reviewer key that reads every root would also be served every root's held rows for review. The admin read keeps reading apart from reviewing. |
| **Logging the admin read in the request log only** | The log is unsigned, kept 30 days and names a route, not the memories served; the act it records is the one an author is most likely to question later. |
| **Serving the admin read through recall or `GET /v1/memories/{artifact_id}` with the admin key** | It would put content on routes agents use and make "the admin key reads nothing on `/v1`" false; two routes of its own keep the exception where the journal sees it. |
| **Demotion in v0.5.0 beside the admin read** | A demotion changes the tip, recall and every successor's lineage, a use case of its own; the read adds one route pair and one event (d12). |
| **`<user>-review` only when an admin grants review (this record's first draft)** | A person would wait on an admin before reviewing their own agents' work; the maintainer chose a reviewer key on the person's own root at first sign-in (d20 e). |
| **A session key's principal shared with the person's agent keys** | R2 excludes principals as well as keys, so the person's reviewer key could never approve their agents' writes, which d20 e asks for. |
| **`SameSite=Strict` on the session cookie** | The redirect after a provider's cross-site callback would arrive without it, and the person would land signed out; `Lax` already withholds it from cross-site `POST`s. |
| **An idle timeout** | A write on every page view to extend it; an absolute lifetime and sign-out bound a session without one. |
| **Any verified GitHub address, or the profile's public email** | The public email may be unverified or unset, and accepting every verified address makes which one signed in depend on the order GitHub lists them. |
| **Verifying Google's ID token signature against its published keys** | A key fetch and a cache for no gain over the TLS connection the token arrived on, which OpenID Connect Core allows for a token-endpoint response. |
| **One callback route for both providers** | The provider would come only from the cookie; carrying it in the path as well means a code is never sent to the wrong token endpoint even when the cookie and the path disagree. |
| **Deriving the user name from the email** | Two people can share a local part across domains, and an address change would rename a user; the admin names the user once, as ADR-0044 d2 does. |
| **A person record beside the allowlist** | ADR-0044 rejected a user record beside the root; entries that name a user are enough for one person with several addresses. |
| **The allowlist, or the first admin email, in a boot setting** | A setting is deployment-wide and unjournalled; the list is a tenant's and every change to it is an act the journal should hold. |
| **Removing an email also cutting off the person's agent keys** | A mistyped or replaced address would stop a person's agents without warning; the removal shows the keys and the admin cuts them off deliberately. |
| **Journalling each sign-in and sign-out** | The acts a session performs are journalled under its key; a sign-in event would put a login history in an append-only record forever, for what the request log already shows for 30 days. |
| **Per-address rate limits** | Behind a proxy every request has the proxy's address; the connect page made the same choice. |
| **A web route or MCP tool for agents to drive the pages** | The pages exist for people; an agent's surface is the three MCP tools and the API under its own key (R1). |
| **A dashboard, a recall view, graph pictures or an assistant on the pages** | Each is the accretion ADR-0015 warned of; none is a duty a person cannot do without, and each would need its own record. |
| **Rendering the whole list in the browser from one JSON answer (d22)** | It would put every row the person may read into one script-readable answer and make script the default view, against d8; the server-rendered page keeps the list scriptless and pages it. |
| **A separate single-page application for the Memories view and the Atlas (d25)** | A bundle with its own build, router and token handling, and script on every view; the Atlas needs no script at all. |
| **The docs atlas's three.js orbit view over real data (this amendment's first build), vendored or from a CDN (d25, d26)** | Script on the origin that holds the session, a 600 KB library to pin and audit, and circles that encode nothing; the operator chose the design system's treemap, whose areas are counts and which the server draws with no script. |
| **`script-src 'self'`, or a nonce on an inline script (d26)** | Nothing on the pages needs script once the map is SVG. |
| **Offset pagination, `page=3` (d22)** | A row written while the person pages shifts every later page by one, showing a row twice or skipping one; the keyset resumes after the last row shown. |
| **The treemap laid out by a script in the browser, as the design system's preview does (d26)** | It would bring script back for a layout the server computes once; SVG geometry needs no inline style. |
| **A dropdown of every act, refused by the server when not allowed (d28)** | A person would pick acts the page knew would fail; computing the offer from the verb's own check costs one read per workarea and offers only what runs. |
| **A count of matching rows above the list (d22)** | A second query per page, and on the user page a number about rows the person's keys may not otherwise count. |
