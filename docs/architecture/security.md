---
status: Stable
verified: 2026-11-14
answers: "what the tenancy model isolates; who holds which key in the custody map; what the threat model defends against, hostile co-tenants included; what the connection and web surfaces add to the threat model and what they leave"
---

# Security and tenancy

The attacker is a **hostile co-tenant**: a party with valid API keys on the same Postgres cluster and the same lead, `oml-server-lead`. See also [storage](storage.md) (signing chain), [roles and keys](roles-and-keys.md), [workareas](workareas.md), [admission](admission.md).

Nobody has penetration-tested, load-tested or measured these controls; they are stated by the code and its tests, not audited.

## Tenancy model

The server derives the tenant from the API key and never from the request: no parameter, header, path segment or MCP argument names a tenant.

- **Row-level security covers every table** (`FORCE ROW LEVEL SECURITY`, keyed by tenant). The one exception is `_sqlx_migrations`, which holds no tenant data and only the owner reaches. The lead refuses to start on a serving role that is a superuser or holds `BYPASSRLS`, and the lake writer refuses the same of its own role. Nothing checks the owner role's attributes.
- **Two database roles for the lead, and one for the lake writer.** The owner migrates at boot and creates each new tenant's partitions, because `FORCE` does not bind the owner's `pg_stats` view or its DDL. The lake writer, given `OML_LAKE_OWNER_URL`, connects as the owner at its start to grant its own role's privileges. Requests run under a serving role that owns nothing. The lead refuses to start when both URLs log in as the same role, reach different databases, or the serving role owns anything or holds a privileged attribute.
- **Storage is per tenant:** journal chains, lake prefixes (bodies are sealed ciphertext, [the lake](../operations/lake.md)), and per-tenant partitions of memories, search postings and recall telemetry. RLS stays the isolation authority if partition pruning fails. Memory bodies are stored unencrypted in PostgreSQL; only the lake's copies are sealed.

Tenants share the Postgres cluster, the lead process with its signing key, and the key store.

## Key custody

| Material | Held by |
|---|---|
| Ed25519 deployment signing key | The lead only. |
| The keyring file (`kms.keyring_path`; the `file` adapter is the only key store) | The lead. It holds the signing key alone, as 64 hex characters in a file of mode `0600`; the lead refuses a looser mode. |
| Lake sealing key (KEK file) | The lake writer, which holds no signing key. It wraps one data key per lake body version (`artifact_keys`). |
| API keys, connect codes, connection tokens | The lead stores hashes or digests only. |
| Postgres access | The lead: an RLS-scoped serving role. The lake writer: its own role. It refuses to start on a superuser or `BYPASSRLS` role; keeping it apart from the owner and the serving role is the operator's duty. |

**Lead compromise is signing-key compromise.** A compromised lead can sign arbitrary events and read any body, held or promoted. Holders of an earlier chain head can detect forged history, and the startup fork check refuses to extend a chain that does not verify. Nothing detects quiet reads.

**The keyring is a file on the lead's host.** Whoever reads it can sign as the deployment. Back it up with the database: a lead started without it mints a new key, the startup fork check then fails under that key, and the lead refuses writes. The data stays readable.

**A compromised lake writer cannot sign or alter the journal.** It can corrupt the lake, which a comparison of the decrypted bodies against the journal's plaintext digests exposes.

## If the signing key is compromised

Rotation is designed and not built. There is no rotate command, and no code journals `system.signing_key_rotated`: a lead started with a replaced key fails the startup fork check, because every stored chain verifies only under the old key, and refuses writes. Stop writes and verify each tenant's chain against heads kept outside the deployment; the [runbooks](../operations/runbooks.md) give the steps. Events the old key signed in the compromise window still verify, and separating them from attacker events is an audit against those heads.

## Transport

The lead and `oml-web` listen on plain HTTP; TLS terminates at the reverse proxy, whose configuration sets the protocol floor. Plaintext HTTP only on loopback or a private compose network: the bearer key is the whole capability. [Deployment](../operations/deployment.md) shows where TLS terminates per install path. Across hosts to Postgres, require `sslmode=verify-full`.

## Hostile content after the gate

Approval changes visibility, not truth or safety: an approved memory can still carry a prompt injection or a malicious URL.

- **Treat recalled content as data.** Delimit recalled bodies as untrusted in the agent's prompt, keep the served provenance (artifact id, author, workarea) beside them, and never auto-execute instructions found in a body.
- **Steering rows get no stricter default review.** A steering row precedes every hit in its workarea's recalls, so it is the highest-leverage injection surface. It is held and reviewed under the workarea's policy; set a stricter policy where steering matters.

## Connections: the OAuth surface

A connection lets Claude's hosted apps reach `/mcp` through an OAuth client bound to one connector key by a connect code ([roles and keys](roles-and-keys.md#connections)). Without a key, an attacker reaches two discovery documents, three OAuth endpoints and one HTML page.

| Threat | Control | What remains |
|---|---|---|
| A connect code is entered first by someone else | Single use; 10 minutes, or up to 30 days when an admin mints one for a key confined to one workarea; one live code per key; `oml keys show` lists connections so an admin can end an unexpected one. | For its minutes the code is a bearer secret: its first enterer connects as the key, at `/mcp` only. The admin sees that a connection opened, not whose account. |
| Consent phishing | The page names the client and redirect host and says to enter a code only on a page opened by the user. Authorization codes go only to `https://claude.ai/api/mcp/auth_callback`. | The page cannot tell which claude.ai account started the request; the warning works only if read. |
| A co-tenant hands over a code for its own key | The confirmation names the tenant, using the name the operator set. | A person who does not read the tenant name connects into the other tenant. |
| CSRF, clickjacking, injected content, open redirect | No cookie; same-origin check (`Origin: null` is `oauth.origin_refused`); `frame-ancestors 'none'`; exact-match redirect allowlist. | None known. |
| Authorization code interception | S256 PKCE only; a code lives 60 seconds and works once. | The reverse proxy must not log the authorize redirect or OAuth request bodies. |
| Access-token theft | Opaque, digest-stored, `/mcp` only, `Authorization` header only, 1 hour, key re-checked each request. | A stolen token acts as the key at `/mcp` until it expires or the connection ends. |
| Refresh-token theft | Never authenticates `/mcp`; rotates on use; a rotated token presented again revokes the connection. | A thief who refreshes first holds the connection until the client's next refresh trips the reuse check. |
| SSRF through a client metadata URL | Public unicast addresses only; no proxy or redirect; caps on size, time and concurrency. | The lead makes outbound HTTPS requests to hosts an unauthenticated client names. The egress policy must refuse internal ranges, NAT64 included. |

Connection rows carry their key's tenant and fall under its RLS. Deployment-level rows such as OAuth clients belong to a reserved provider tenant and hold no tenant content.

## Web pages: sign-in and sessions

A user page and an admin page are served by `oml-web` on the lead's public origin; the lead serves sign-in, the provider callback and sign-out. People sign in through Google or GitHub against a per-tenant email allowlist, and every act runs as a [session key](roles-and-keys.md#session-keys), so roles, grants and the journal are the same as for any key.

| Threat | Control | What remains |
|---|---|---|
| An agent uses the pages | An `Authorization` header on a web route answers `access.verb_ceiling`; `/v1`, `/v2` and `/mcp` never read `Cookie`. | An agent driving a person's signed-in browser acts as that person. |
| Session cookie theft | `__Host-`, `Secure`, `HttpOnly`, `SameSite=Lax`; the server holds a SHA-256; fixed lifetime; sign-out or an allowlist change ends it. | A stolen cookie acts as the person until it expires. No idle timeout. |
| CSRF, session fixation, login forgery | `SameSite=Lax`, same-origin check and `csrf_token` on every `POST`; the session token is minted at the callback; the callback admits only the `state` sealed in the starting browser's cookie. | None known. |
| A provider's code taken from another person's callback and presented through a sign-in the taker started | S256 PKCE on both providers; for Google also the sealed `nonce`, checked in the ID token. | On GitHub, PKCE is the only control: GitHub documents the verifier as required once a challenge is sent but does not state that it refuses a wrong one, and no live sign-in has tested it (#119). |
| A forged Google ID token | The ID token is read only from Google's token endpoint, over a TLS connection the lead opens with the client secret; its signature is not checked, as OpenID Connect Core §3.1.3.7 item 6 permits for that channel. `iss`, `aud`, `exp`, `nonce` and `email_verified` are checked ([evidence, 2026-11-13](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md#the-two-provider-points-as-checked-against-the-providers-documentation-2026-11-13-119)). | The issuer's authenticity rests on TLS and the web PKI roots alone. |
| A moved or recycled email address | Only a provider-verified address counts; the provider account is pinned at first sign-in (`identity_changed` otherwise). | Before an entry's first sign-in, whoever controls the address at the provider holds it. |
| Mermaid claim drawn in a review card | Drawn only in a sandboxed frame ([ADR-0056](../adr/ADR-0056-a-claim-version-names-its-format-the-signature-covers-its-source-and-adapters-compiled-into-the-server-derive-what-search-reads.md) d9): opaque origin with no cookie, storage or network, one pinned bundle and one hashed initialiser, a token sealed to the session for 5 minutes. Text is escaped and stripped of control and bidirectional characters. | Mermaid has had cross-site scripting advisories; a compromised frame can draw a misleading picture of the one claim's source the session was already shown, which is always beside it. |
| Secrets in logs | Route template only, never the query; the journal holds an email's digest. | The reverse proxy's access log must drop the query on `/web/callback/*`. |
| A person outliving their access | Removing an email ends its sessions. | Their agent keys keep working until an admin adds a global denial to each. |
| Admin authority through the pages | The admin key never reviews; it reads content only on two admin read routes, journaled as `memory.admin_read`. | A tenant admin can read every person's memories, held ones included. Privacy from the admin needs its own tenant. |

## Review binding

A verdict counts only under a `serve_id` that `oml-server` bound to the reviewer key, memory version and content. It expires, the first verdict consumes it, and changed content invalidates it. That stops replay, bait and switch, transplant to another reviewer key, and blind approval. It proves what the reviewer was shown, not what they read.

## Trust boundaries

The [deployment view](views.md#deployment-a-request-from-the-client-to-the-disk) draws them: the reverse proxy, `oml-web`, the lead inside the one trust boundary, its signing key file, PostgreSQL, and the lake writer with its own role and sealing key.

Every client path crosses the reverse proxy and then the lead's API. A browser's console pages go through `oml-web`, which holds no database access and no signing key; its sign-in goes to the lead. No client path reaches Postgres, the lake or a key file.

## Residual risks

1. Undeclared derivation is undetectable; lineage exclusion is as good as the declared `derives_from` graph.
2. Two keys approving each other's work pass every per-row check.
3. Lead compromise is signing-key and read-everything compromise, and quiet reads go undetected.
4. The platform operator is trusted: a Postgres superuser, key store administrator or Kubernetes Secrets holder can read attribution metadata, destroy keys and rewrite unprotected state. Tenant-held chain heads make some of this detectable.
5. Counts leak: a key sees tenant-wide journal positions for events it cannot read, a `claim_key` collision tells a writer a name is taken, and the lineage read names unreadable ancestors as redacted ids. Never content.
6. A connect code is a bearer secret for its minutes, and the lead fetches client-chosen URLs.
7. A key's rate, its own or its account's, is counted by each server process alone, so n processes admit up to n times the rate; a key with no rate of its own and no account quota is unlimited, and skewed load is untested ([ADR-0079](../adr/ADR-0079-a-key-ends-on-a-date-and-is-held-to-a-request-rate-both-set-at-issuance-and-changed-only-by-an-admin-key-that-is-not-the-key-itself.md) d4, d6).
8. The widen hint reads word counts the lead holds in memory, per key and scope, for up to 300 seconds. An erasure or a demotion (both designed, not built), or a change to a workarea's status, mode or federation setting drops them at once. A promotion, a supersession or a retirement reaches them within those 300 seconds. They hold counts of hashed words of rows the key could read, never text ([ADR-0072](../adr/ADR-0072-recall-searches-the-trusted-workareas-a-key-already-reads-chosen-per-query-by-reviewed-descriptors-and-never-widens-what-it-may-read.md#the-widen-hint-as-built-2026-11-11)).

Design record: [ADR-0007](../adr/ADR-0007-tenancy-first-class.md), [ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md), [ADR-0045](../adr/ADR-0045-people-sign-in-to-a-user-page-and-an-admin-page-on-the-servers-own-origin.md).
