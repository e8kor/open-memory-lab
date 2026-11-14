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
Workspaces view at `/web/workspaces` on the user page and `/web/admin/workspaces` on the admin page;
the Memories list at `GET /web/memories` and `GET /web/admin/memories`; the Atlas at
`GET /web/atlas` and `GET /web/admin/atlas`, with the list's JSON at `GET /web/data/memories` and
`GET /web/admin/data/memories`; the Review view at `/web/review` and `/web/admin/review`; and the
Keys view at `/web/keys` and `/web/admin/keys`. Every act on them runs through the API as the
person's session keys, and a form sent twice runs once. Putting a workspace in progress, committing
it, proposing or ratifying a status change, and a bulk verdict are confirmed first. Every page of
the admin list is one journalled admin read, as before. The Review view holds review next, and on
the admin page the queue of held memories per workspace and bulk review; the reviewer is never the
author, and a verdict the API refuses is shown with its code. On the Keys view a key's value or a
connect code is shown once, on the page the act lands on: it travels there sealed in a cookie that
page clears, never in an address or a log line, and `oml-web` keeps no copy. The admin's Keys view
lists one person's keys, named by `user`, with a cut-off beside each. A signed-in `GET /web` goes
on to the Keys view. One memory opened by its id is at
`/web/memory?id=<id>`, and on the admin page, with its attribution and the form that proposes its
next version, at `/web/admin/memory?id=<id>`; the admin page's Curate view is at
`/web/admin/curate`. The onboarding page is at `/web/start`, with its status poll at `/web/start/status` and the
export at `/web/start/export`; the setup view at `/web/setup`, which, like a new key's panel, shows the address an agent
sets beside its key: the API's issuer, else `web.public_url`, else a `no_server` line and no address (no release
names a server, ADR-0076 d6); the admin overview (the allowlist and
the review and curation grants) at `/web/admin/people`; and the users and quotas page at
`/web/admin/users`. Removing an address, disabling a person and closing a deletion request each show a
confirmation first. The server's only web pages are now sign-in, the sign-up terms page and
sign-out. The header of every page a signed-in person is shown, a refusal page included, carries
**Sign out**: its form posts to the server's `POST /web`, which ends the session, so no credential
`oml-web` derives from it works afterwards, clears the session cookie and returns to the sign-in
page. The sign-in page, the sign-up terms page and a refusal the server answers before it reads
the session carry none. A Mermaid diagram is drawn beside its source on every page that shows
one, the Review card included, in a sandboxed frame `oml-web` serves at `/web/render/` with its
pinned bundle at `/web/assets/`; route both prefixes to `oml-web`.

Since v0.7.1 the server no longer redirects the addresses of v0.6 (ADR-0070, "The old URLs
deleted"). A bookmark of `GET /web?view=...` answers not found once signed in, `GET /web/admin` and
anything below it that `oml-web` does not serve answer not found, and a form from an old page that
posts an act other than sign-out to `POST /web` or `POST /web/admin` is refused and runs nothing.
Use the paths above. Run
`oml-web` behind the same proxy as the server before an upgrade: a deployment that sends these
paths to the server shows not found.

`oml-web` ships everywhere the server does:

| Where | How it runs |
|---|---|
| Release archives | `oml-web-X.Y.Z-<target>.tar.gz` for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, summed in `SHA256SUMS` and signed like the server's ([install the server archive](deployment.md#install-the-server-archive)). |
| The image | `/usr/local/bin/oml-web` in `ghcr.io/e8kor/open-memory-lab`; run it with `--entrypoint /usr/local/bin/oml-web`. The image sets `OML_WEB__LISTEN=0.0.0.0:8091`. |
| `docker-compose.yml` and `oml quickstart` | A service `oml-web` and a Caddy router, the one published port, in front of it and the server. |
| The Helm chart | A second Deployment, `oml-web`, with its Service and the web client Secret ([Kubernetes](../guides/kubernetes.md)). |

## Requirements

- The server with `oauth.issuer` set and the pages on, as for the [user and admin pages](deployment.md).
- One secret, set as `web.client_secret` on the server and on `oml-web`: at least 32 visible
  ASCII characters, at least 8 of them distinct, as `openssl rand -hex 32` writes. Both processes
  refuse to start with a shorter or repetitive one, naming the setting. The server accepts a
  person's capacity credential (`omls_`) only from a request that carries it
  ([Store the client secret](#store-the-client-secret)).
- A reverse proxy in front of both processes on one public origin. The session cookie is a
  `__Host-` cookie, so the browser sends it only to that one host.

## Start it

```sh
export OML_WEB__API_URL=http://127.0.0.1:8090        # the server's listener, not the public proxy
export OML_WEB__CLIENT_SECRET_FILE=<secret-file>     # owner-only; the server's web.client_secret
export OML_WEB__LISTEN=127.0.0.1:8091                # the default
oml-web
```

| Setting | Environment variable | Default | What it does |
|---|---|---|---|
| `web.api_url` | `OML_WEB__API_URL` | required | The server's base URL, `http://` or `https://`, a host and no path. Plain `http://` is accepted only to a loopback host. |
| `web.client_secret` | `OML_WEB__CLIENT_SECRET` | required, or the file below | Sent as `OML-Web-Client` on every API call. Never logged. |
| `web.client_secret_file` | `OML_WEB__CLIENT_SECRET_FILE` | unset | A file holding the secret, read at start, one trailing line ending dropped. On Unix a file its group or others can read stops the start, naming its mode, and so does a file owned by a user other than the process's or root. Setting both stops the start. |
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

## Store the client secret

The secret is the one credential that lets a process present a person's session to the API, so
it lives where the process's other secrets live and nowhere else:

| Shape | Where it lives |
|---|---|
| A host under systemd | An owner-only file each unit reads: `LoadCredential=web-client:/etc/oml/web-client` and `OML_WEB__CLIENT_SECRET_FILE=%d/web-client` on `oml-web`, the same with `OML_WEB__CLIENT_SECRET_FILE` on the server. systemd hands the unit a copy readable by the unit alone. |
| `oml quickstart` | `OML_WEB_CLIENT_SECRET` in the stack's owner-only `.env`, which compose hands to both services. |
| `docker-compose.yml` | A fixed development value unless `OML_WEB_CLIENT_SECRET` is set; for development only. |
| The Helm chart | A Secret: the one `web.clientSecret.existingSecret` names, the recommended path and the only one for GitOps or `helm template`, or one the chart generates once and keeps ([Kubernetes](../guides/kubernetes.md)). `values.yaml` never holds the value. |
| Kubernetes without the chart | An environment variable from the Secret (`valueFrom.secretKeyRef`), as the chart sets it. A Secret mounted as a volume is owned by root, and with `fsGroup` it is group-readable, so `web.client_secret_file` refuses it; with `defaultMode: 0400` and no `fsGroup` only root can read it. |

Neither process logs the secret, prints it in `--doctor`, or puts it in a debug form; a refusal
names the setting and, for a file, its path and mode, never the value.

### Rotating the client secret

The server accepts the old secret beside the new one for a bounded window, so `oml-web` keeps
working while it restarts onto the new value:

1. On the server, set the new value as `web.client_secret`, the old one as
   `web.client_secret_previous` (or `_previous_file`), and `web.client_secret_previous_until` to an
   RFC 3339 instant at most 7 days ahead, such as `2026-11-12T09:00:00Z`. Restart the server. Each
   start logs `web_client_previous_accepted` with the instant.
2. Set the new value on `oml-web` and restart it. Until it has, it still sends the old secret,
   which the server accepts.
3. Unset `web.client_secret_previous` and `web.client_secret_previous_until` on the server and
   restart it.

At the instant, the server stops accepting the old secret on its own, without a restart. A start
with a closed window logs `web_client_previous_closed` until step 3 is done. A window more than 7
days after the start, a previous secret without its instant, or one equal to the current secret
stops the server at start, naming the setting. `oml-web` sends one secret and never reads the
previous one. On Kubernetes the same three steps edit the Secret's keys and restart the two
Deployments ([Kubernetes](../guides/kubernetes.md)).

### Mutual TLS instead of the secret

ADR-0070 d2 names mutual TLS as the alternative for a hop that leaves the host: the server
terminates TLS on the listener `oml-web` reaches and accepts the client certificate whose subject
is `web.client_subject`, issued under `web.client_ca`. This build does not terminate TLS, so it
cannot verify a certificate, and setting `web.client_ca` or `web.client_subject` stops the server
at start. A client certificate a gateway forwards in a header is never accepted: a request that
carries one is refused as outside `oml-web`. Across a network the hop uses `https://` to a TLS
proxy in front of the server, with the secret.

## Route the pages

The proxy sends each page `oml-web` serves to it, and every other path to the server, sign-in
included. For Caddy (`deploy/caddy/Caddyfile` is the full route table):

```caddyfile
memory.example.com {
	@oml_web path /web/guardrails /web/workspaces /web/admin/workspaces /web/memories /web/atlas /web/admin/memories /web/admin/atlas /web/data/memories /web/admin/data/memories /web/review /web/admin/review /web/keys /web/admin/keys
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
page's request id. When a secondary part fails, such as the workspace list on the Guardrails view,
the page renders without it and names that call's request id.

A list the page reads a page at a time is read whole or refused, never shown cut short:

| List | Read by | Bound |
|---|---|---|
| a person's keys | the Memories view and its JSON | 10 pages of 200 |
| a person's keys | the Keys views, and the confirmation before a person's last address is removed, which lists their agent keys with a cut-off beside each | 50 pages of 200 |
| the allowlist | the People page and its acts | 50 pages of 200 |

Past its bound the list is refused with `limit.list_too_long` (422); the API and the command
line read it whole. A cursor the server repeats, at once or in a cycle, is refused with
`unavailable` (502): the fault is the server's, so load the page again.

A key's value or a connect code reaches the page it is shown on in a sealed `__Host-oml_once`
cookie that lives one minute. The cookie's path is `/`, so it rides every request to the site
until that page clears it. Neither process logs `Cookie`; keep the proxy in front from logging
it too (Caddy blanks it unless `log_credentials` is set).

## Not built yet

- The cap on API calls one session has in flight.
- `oml doctor` reading `oml-web`'s environment. Run `oml-web --doctor` instead.
- Mutual TLS between `oml-web` and the server ([Mutual TLS instead of the secret](#mutual-tls-instead-of-the-secret)).
