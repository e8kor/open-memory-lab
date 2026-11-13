---
status: Stable
verified: 2026-11-06
answers: "what the connect page shows a person who adds the claude.ai connector, word for word"
---

# The connect page

When claude.ai adds the connector, the person's browser opens `/oauth/authorize` on the server. This page lists what that page says. The server tests its own sentences against this text, so the two cannot drift. The routes themselves are on the [HTTP API reference](../api/http-api.md#oauth).

**The authorization page.** It sets no cookie and runs no script. Every page carries
`Content-Security-Policy: default-src 'none'; style-src 'sha256-…'; form-action 'self' <redirect
origin>; frame-ancestors 'none'; base-uri 'none'`, `Cache-Control: no-store`,
`Referrer-Policy: same-origin` and `X-Content-Type-Options: nosniff`; `same-origin`, not
`no-referrer`, because `no-referrer` makes a browser send `Origin: null` on the page's own `POST`
(the Fetch Standard, "append a request `Origin` header", fetched 2026-10-31). It names the client —
a document client's `client_name` "verified by" the document URL's whole host, in lower-case ASCII
(the punycode A-label form), never shortened and rendered apart from `client_name`; a registered
client's `client_name` marked "name not verified" — and the redirect URI's host. Its sentences
are fixed, word for word:

- Above the code field: "Enter the code only if you opened this page yourself,
  by adding the connector in Claude, in the last few minutes. If someone sent you this link, close
  it."
- Below it: "Get a connect code from the account page: Connect Claude on the web or a phone."
  with the phrase linked to `/web` on the same origin, then "Codes are shown once; use New connect
  code on the key's row if it was lost."
- A wrong code — unknown, expired, voided, spent, or a revoked or expired key's alike — with the attempts left: "This code
  is not valid. Check it, or ask an administrator for a new one."

After a correct code, the confirmation shows the tenant the key belongs to, by the name the
operator gave it at `POST /v2/tenants`, which no tenant admin can set; the key's
name, principal and role; every workarea it holds a grant on, by its name, with
the grant's role; and one sentence per kind of verb
the key's role and effective access give it:

- where it holds writer: "Claude can remember and learn as this agent in `<workareas>`."
- where it holds reader: "Claude can recall and read as this agent in `<workareas>`."
- where it holds dreamer(propose): "Claude can propose edits and edges as this agent in
  `<workareas>`."
- where it holds writer or dreamer(propose): "Everything it writes is held until someone other
  than its author reviews it."
- for each granted workarea whose tenant policy permits `self_signed` rows: "In `<workarea>`, its
  writes may be self-signed and readable there without review."
- last, always: "It cannot approve, demote, delete or administer anything."

The buttons are Connect and Cancel. Client-supplied strings are HTML-escaped and shortened;
`logo_uri` is never loaded.
.

**Behind a proxy.** A reverse proxy in front of `oml-server` forwards `/mcp`, the two discovery documents and `/oauth/*` unchanged, with no path rewriting and no caching. A proxy that writes access logs must not record the `Location` response header of `POST /oauth/authorize`, whose query carries the authorization code, nor the request bodies of `POST /oauth/authorize` and `POST /oauth/token`.
