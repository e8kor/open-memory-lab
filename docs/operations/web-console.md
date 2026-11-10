---
status: Beta
verified: 2026-11-10
answers: "how to run oml-web, the web console's own process, beside the server; which settings it reads; how the proxy routes the pages it serves; what it does not do yet"
---

# Run the web console

The web console is moving out of the server into its own process, `oml-web` (ADR-0070). `oml-web`
serves pages and calls the server's public HTTP API for everything it shows, as the CLI and the
SDKs do. It holds no store and writes no journal. The server keeps the API, sign-in and every page
that has not moved yet.

The pages move one at a time. In this build `oml-web` serves one page, the Guardrails view of the
user page (`GET /web?view=guardrails`). The server answers that URL as not found, so a deployment
that routes it to the server shows no Guardrails tab content. Run `oml-web` behind the same proxy
as the server before you upgrade.

## What you need

- The server with `oauth.issuer` set and the pages on, as for the [user and admin pages](deployment.md).
- One secret of at least 32 characters, set as `web.client_secret` on the server and on `oml-web`.
  The server accepts a person's capacity credential (`omls_`) only from a request that carries it.
- A reverse proxy in front of both processes on one public origin. The session cookie is a
  `__Host-` cookie, so the browser sends it only to that one host.

`oml-web` is not in the release archives, the image, the compose file, the Helm chart or
`oml quickstart` yet. Build it from the repository:

```sh
cargo build --release -p oml-web
```

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
server is.

## Route the pages

The proxy sends each page `oml-web` serves to it, and every other path to the server. For Caddy:

```caddyfile
memory.example.com {
	@oml_web {
		method GET
		path /web
		query view=guardrails
	}
	reverse_proxy @oml_web 127.0.0.1:8091
	reverse_proxy 127.0.0.1:8090
}
```

The list of pages `oml-web` serves is `PAGES` in `crates/clients/oml-web/src/lib.rs`. It grows
with each page that moves, and this route grows with it.

## What crosses the hop

For each page, `oml-web` reads the session cookie and derives the person's capacity credential
from it. The cookie never reaches the API. The first call, `GET /v2/whoami`, checks the session.
With no session, or one the server no longer accepts, the person is sent to sign in.

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

- `oml quickstart`, the compose file and the Helm chart do not start `oml-web`.
- `oml-web --doctor` and its `oml doctor` line.
- The cap on API calls one session has in flight.
- A sign-in page at `GET /web/sign-in`. A person with no session is sent to `GET /web`, where
  the server shows sign-in.
