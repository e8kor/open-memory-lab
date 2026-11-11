---
status: Beta
verified: 2026-11-10
answers: "how to run oml-web, the web console's own process, beside the server; where it ships; which settings it reads; how the proxy routes the pages it serves; how to check it; what it does not do yet"
---

# Run the web console

The web console is moving out of the server into its own process, `oml-web` (ADR-0070). `oml-web`
serves pages and calls the server's public HTTP API for everything it shows, as the CLI and the
SDKs do. It holds no store and writes no journal. The server keeps the API, sign-in and every page
that has not moved yet.

The pages move one at a time, and each moved page gets its own path under `/web/`. In this build
`oml-web` serves four views: the Guardrails view of the user page at `GET /web/guardrails`; the
Workareas view at `/web/workareas` on the user page and `/web/admin/workareas` on the admin page;
the Memories list at `GET /web/memories` and `GET /web/admin/memories`; the Atlas at
`GET /web/atlas` and `GET /web/admin/atlas`, with the list's JSON at `GET /web/data/memories` and
`GET /web/admin/data/memories`; the Review view at `/web/review` and `/web/admin/review`; and the
Keys view at `/web/keys` and `/web/admin/keys`. Every act on them runs through the API as the
person's session keys, and a form sent twice runs once. Putting a workarea in progress, committing
it, proposing or ratifying a status change, and a bulk verdict are confirmed first. Every page of
the admin list is one journalled admin read, as before. The Review view holds review next, and on
the admin page the queue of held memories per workarea and bulk review; the reviewer is never the
author, and a verdict the API refuses is shown with its code. On the Keys view a key's value or a
connect code is shown once, on the page the act lands on: it travels there sealed in a cookie that
page clears, never in an address or a log line, and `oml-web` keeps no copy. The admin's Keys view
lists one person's keys, named by `user`, with a cut-off beside each. The server answers the old
addresses, `GET /web` (signed in), `GET /web?view=guardrails`, `?view=workarea`,
`?view=memories` (with `display=atlas`, the Atlas), `?view=review`, `?view=keys`, and
`GET /web/admin?view=workarea`, `?view=memories`, `?view=review`, `?view=held` and `?view=queue`,
with a redirect to the new paths, so bookmarks keep working. One memory opened by its id is at
`/web/memory?id=<id>`, and on the admin page, with its attribution and the form that proposes its
next version, at `/web/admin/memory?id=<id>`; the admin page's Curate view is at
`/web/admin/curate`. The server answers `GET /web?view=memories&memory=<id>`,
`GET /web/admin?view=memories&memory=<id>` and `GET /web/admin?view=curate` with a redirect there,
carrying only the memory's id. The onboarding page is at `/web/start`, with its status poll at `/web/start/status` and the
export at `/web/start/export`; the setup view at `/web/setup`; the admin overview (the allowlist and
the review and curation grants) at `/web/admin/people`; and the users and quotas page at
`/web/admin/users`. The server answers `GET /web?view=setup` and every `GET /web/admin` with a
redirect there; removing an address, disabling a person and closing a deletion request each show a
confirmation first. The server's only web pages are now sign-in, the sign-up terms page and
sign-out. A Mermaid diagram is drawn beside its source on every page that shows
one, the Review card included, in a sandboxed frame `oml-web` serves at `/web/render/` with its
pinned bundle at `/web/assets/`; route both prefixes to `oml-web`. A form from an old page that posts
one of these views' acts, or a creation, a supersession or a curation, to the server runs nothing
and lands on the view. Run
`oml-web` behind the same proxy as the server before you upgrade: a deployment that sends these
paths to the server shows not found.

`oml-web` ships everywhere the server does:

| Where | How it runs |
|---|---|
| Release archives | `oml-web-X.Y.Z-<target>.tar.gz` for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, summed in `SHA256SUMS` and signed like the server's ([install the server archive](deployment.md#install-the-server-archive)). |
| The image | `/usr/local/bin/oml-web` in `ghcr.io/e8kor/open-memory-lab`; run it with `--entrypoint /usr/local/bin/oml-web`. The image sets `OML_WEB__LISTEN=0.0.0.0:8091`. |
| `docker-compose.yml` and `oml quickstart` | A service `oml-web` and a Caddy router, the one published port, in front of it and the server. |
| The Helm chart | A second Deployment, `oml-web`, with its Service and the web client Secret ([Kubernetes](../guides/kubernetes.md)). |

## What you need

- The server with `oauth.issuer` set and the pages on, as for the [user and admin pages](deployment.md).
- One secret of at least 32 characters, set as `web.client_secret` on the server and on `oml-web`.
  The server accepts a person's capacity credential (`omls_`) only from a request that carries it.
- A reverse proxy in front of both processes on one public origin. The session cookie is a
  `__Host-` cookie, so the browser sends it only to that one host.

## Start it

```sh
export OML_WEB__API_URL=http://127.0.0.1:8090        # the server's listener, not the public proxy
export OML_WEB__CLIENT_SECRET="$(cat <secret-file>)" # the server's web.client_secret
export OML_WEB__LISTEN=127.0.0.1:8091                # the default
oml-web
```

| Setting | Environment variable | Default | What it does |
|---|---|---|---|
| `web.api_url` | `OML_WEB__API_URL` | required | The server's base URL, `http://` or `https://`, a host and no path. Plain `http://` is accepted only to a loopback host. |
| `web.client_secret` | `OML_WEB__CLIENT_SECRET` | required | Sent as `OML-Web-Client` on every API call. Never logged. |
| `web.listen` | `OML_WEB__LISTEN` | `127.0.0.1:8091` | Where the pages listen. |
| `web.public_url` | `OML_WEB__PUBLIC_URL` | the server's `oauth.issuer` | The origin a form's `Origin` must equal. Unset, `oml-web` reads it from the server's `/.well-known/oauth-authorization-server` at start, for up to 30 seconds, and refuses to start without it. |
| `web.allow_plain_http_api` | `OML_WEB__ALLOW_PLAIN_HTTP_API` | `false` | `true` accepts a plain `http://` `web.api_url` to a host that is not loopback, for a private network. `oml-web` warns at every start. |

`oml-web` refuses to start when a required setting is missing or invalid, and names it.
`GET /web/healthz` answers without calling the server: it says the process is up, not that the
server is. `GET /readyz` on `oml-web`'s own listener calls the server's `/readyz` once, with no
credential, and answers 200 only when the server is ready and names a web client identity
(`web_client` other than `none`); otherwise 503, naming the check that failed. The public
`/readyz` behind the proxy stays the server's.

## Check it

```sh
oml-web --doctor
```

Run it with `oml-web`'s environment. It prints one line per check and exits 1 on any `fail`:

- `web.api_url`: `ok`; `warn` when it is plain `http://` to a host other than loopback, allowed by
  `web.allow_plain_http_api`, because session credentials cross that hop in clear; `fail` when it
  is missing or refused, with the reason the start would give.
- any other setting the start would refuse, as `fail`, naming it. No line prints the secret.
- `api`: whether the server answers ready, and the web client identity it accepts. A server
  without `web.client_secret` is a `fail`: it refuses every capacity credential `oml-web` sends.

The server's `oml doctor` cannot read `oml-web`'s settings; it warns when the server itself has
no `web.client_secret`.

## Route the pages

The proxy sends each page `oml-web` serves to it, and every other path to the server, sign-in
included. For Caddy (`deploy/caddy/Caddyfile` adds the landing pages):

```caddyfile
memory.example.com {
	@oml_web path /web/guardrails /web/workareas /web/admin/workareas /web/memories /web/atlas /web/admin/memories /web/admin/atlas /web/data/memories /web/admin/data/memories /web/review /web/admin/review /web/keys /web/admin/keys
	reverse_proxy @oml_web 127.0.0.1:8091
	reverse_proxy 127.0.0.1:8090
}
```

The list of pages `oml-web` serves is `PAGES` in `crates/clients/oml-web/src/lib.rs`. It grows
with each page that moves, and this route grows with it; a test holds the Caddyfile, the
compose file, `oml quickstart`'s compose file and the Helm chart's `web.ingressPaths` to it.
Every route is a path, so any proxy or Ingress controller can express it
([Kubernetes](../guides/kubernetes.md)).

## What crosses the hop

For each page, `oml-web` reads the session cookie and derives the person's capacity credential
from it. The cookie never reaches the API. The first call, `GET /v2/whoami`, checks the session.
With no session, or one the server no longer accepts, `oml-web` answers `303` to the server's
`/web/sign-in`, carrying the page as `next`. The server keeps `next` only when it is a path under
`/web` with no `//`, no `\`, no `@` and no scheme, checked again after one more percent-decoding;
anything else returns to `/web`. After sign-in the server sends the person to `next` on its
configured `oauth.issuer`, never on an address a request names.
Beside it, `oml-web` presents the session's admin credential to `GET /v2/whoami`. That
credential resolves only for an admin's entry, and it decides whether the page draws the
Administration tab. For everyone else, each page load adds one refused call (`auth.invalid`) to
the server's request log and its metrics. Do not alert on those
refusals as failed sign-ins.

Each API call carries `X-Correlation-Id` set to the page's request id and `X-Request-Id`
`<page id>.<n>`. The page's own log line, `"service": "oml-web"`, names the acting key and the
ids of the API calls it made, so one filter on the page's id finds the page line and the
server's lines for it. Each call has a 10-second timeout, a constant of the build that has not
been measured ([#170](https://github.com/e8kor/open-memory-lab/issues/170)). Calls share one pool
of kept-alive connections.

When the call a page needs fails, `oml-web` shows the server's error code and message with the
page's request id. When a secondary part fails, such as the workarea list on the Guardrails view,
the page renders without it and names that call's request id.

## Not built yet

- The cap on API calls one session has in flight.
- `oml doctor` reading `oml-web`'s environment. Run `oml-web --doctor` instead.
