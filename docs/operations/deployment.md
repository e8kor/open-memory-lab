---
status: Stable
verified: 2026-11-06
answers: "how to deploy an OML server: the three ways to run it, what runs, how to claim the first admin key, how to put it behind a reverse proxy, and what to back up"
---

# Deploy OML

You can run OML three ways. Pick the smallest one that fits, because each step up adds a trust
boundary you must operate.

| Path | For | Needs | Start with |
|---|---|---|---|
| Docker on one machine | A trial, one person's memory | Docker with compose v2, the `oml` CLI | `oml quickstart` |
| One host behind TLS | A team | PostgreSQL, the `oml-server-lead` binary, a reverse proxy | [One host](#run-on-one-host-behind-tls) |
| Kubernetes | A team on a cluster | A cluster, Helm 3, an external PostgreSQL | [Install OML on Kubernetes](../guides/kubernetes.md) |

Every path runs the same server binary, `oml-server-lead`. The lead is the one process that holds
the signing key, writes the journal and serves the API. [Run your own server](../start/run-your-own.md)
walks the first two paths in order.

## What runs

| Part | What it is | State |
|---|---|---|
| `oml-server-lead` | The server. Exactly one runs per deployment. | None in the process. Everything durable is in PostgreSQL and the keyring. |
| PostgreSQL | The system of record: journal, memories, keys, grants. | All of it. |
| The keyring | A file, `kms.keyring_path`, that holds the signing key and the key that wraps each memory's encryption key. | The deployment's identity. |
| `oml-lakewriter` | An optional second process that copies the journal into Delta tables for analysis. | None. See [Run the lake writer](lake.md). |

The server connects to PostgreSQL with two roles. The owner role owns the database and runs the
migrations at start. The serving role owns nothing and serves requests. The server refuses to start,
with `postgres.role_split`, when both URLs log in as one role. Each role has `LOGIN` and no other
attribute.

There is no web console to deploy. Administration is the [`oml` CLI](../cli/reference.md). People
use the server's own web pages once you turn on sign-in ([configuration](configuration.md)).

## Start on one machine with Docker

You need Docker with compose v2 and the `oml` CLI ([install](../start/install.md)).

```sh
oml quickstart
```

The verb writes a compose file with the server and `pgvector/pgvector:pg17`, pinned to the CLI's
version. It writes an owner-only `.env` of generated passwords and brings the stack up on `127.0.0.1`.
Then it claims the server, issues an agent key and a reviewer key into owner-only files, and checks one
write end to end. The flags are in the [CLI reference](../cli/reference.md).

Containers separate the processes, but anyone with root on the host reads every volume, password and
the keyring. Use it for a trial or for one person, not as a team's server.

## Run on one host behind TLS

You need PostgreSQL 17 or later with the `ltree` and `pg_trgm` extensions, a reverse proxy with a TLS
certificate, and the `oml` CLI on your own machine.

1. Create the two PostgreSQL roles and a database owned by the owner role:

    ```sql
    CREATE ROLE oml_owner LOGIN PASSWORD '...';
    CREATE ROLE oml_server LOGIN PASSWORD '...';
    CREATE DATABASE oml OWNER oml_owner;
    ```

    The migrations grant the serving role its privileges. You grant it nothing by hand.
2. [Install the server archive](#install-the-server-archive).
3. Set the two connection strings in the server's environment or in `oml.toml`:
   `OML_POSTGRES__URL` for the serving role and `OML_POSTGRES__OWNER_URL` for the owner role.
   The server listens on `127.0.0.1:8080` by default (`server.listen`), so only the proxy reaches it.
4. Start the server as its own operating-system user. Keep its environment file readable by that user
   alone, and the keyring file at mode `0600`.
5. [Claim the first admin key](#claim-the-first-admin-key) before the proxy exposes the server.
6. Put the reverse proxy in front and run `oml doctor`.

Every setting is on [configuration](configuration.md).

### Install the server archive

Each release attaches `oml-server-lead-X.Y.Z-<target>.tar.gz` for `x86_64-unknown-linux-gnu` and
`aarch64-unknown-linux-gnu`. The archive holds `oml-server-lead`, the README and the license.
Verify it before you stop anything:

```sh
v=X.Y.Z t=x86_64-unknown-linux-gnu           # aarch64-unknown-linux-gnu on arm64
base=https://github.com/e8kor/open-memory-lab/releases/download/v$v
curl -fsSLO "$base/oml-server-lead-$v-$t.tar.gz" -O "$base/oml-server-lead-$v-$t.tar.gz.sigstore.json"
cosign verify-blob \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/e8kor/open-memory-lab/\.github/workflows/release\.yml@refs/tags/.*$' \
  --bundle "oml-server-lead-$v-$t.tar.gz.sigstore.json" "oml-server-lead-$v-$t.tar.gz" \
  && tar -xzf "oml-server-lead-$v-$t.tar.gz"
```

An archive that does not verify is not installed. The `oml` CLI is a separate archive
([install](../start/install.md)).

## Claim the first admin key

While no tenant exists, the server prints a single-use enrollment token on stdout at every start.
The token is `enr_` and 48 hex digits. A restart prints a new one and voids the old one.

Whoever presents the token first receives the first admin key and the tenant. Claim the server before
any proxy or Ingress exposes it. Do it from the host or through a port-forward:

```sh
printf '%s\n' "$TOKEN" | oml --url http://127.0.0.1:8080 bootstrap claim <key-name> \
    --token-stdin --tenant-name <tenant> --principal <you>
```

The token also sits in whatever store collects the server's logs until a claim consumes it. The CLI
prints the key once and stores it in `~/.config/oml/credentials`. The rest of the first hour is on
[Administer an OML server](../guides/administering.md).

## Put a reverse proxy in front

Only the proxy listens publicly. The server, its metrics listener and PostgreSQL stay on loopback or a
private network.

- Terminate TLS at the proxy and forward the whole origin to the server, unchanged. Do not rewrite paths
  or cache responses.
- Keep the server's own security headers. The proxy adds none.
- Keep the `Location` header of `POST /oauth/authorize` out of the proxy's access log. Keep the bodies of
  `POST /oauth/authorize` and `POST /oauth/token` out too. They carry codes and tokens.
- Set `server.trusted_proxies` to the proxy's address, such as `127.0.0.1/32`. Unset, every request
  logs the proxy's address as its client.

The proxy's access log holds client addresses, so it falls under the same 30-day retention as the
server's ([observability](observability.md)).

### Connect claude.ai and the mobile apps

Claude's hosted apps reach OML through a connection opened by a one-time connect code. The server serves
the whole flow on its own origin. Set `oauth.issuer` to the public origin: `https`, no path, no trailing
slash, the address people type. Unset, connections are off, the OAuth endpoints answer `404`, and a connect
code is refused.

The proxy must pass these paths through: `/mcp`, `/.well-known/oauth-protected-resource/mcp`,
`/.well-known/oauth-authorization-server` and `/oauth/*`. A proxy that forwards only `/mcp`, `/v1` and
`/v2` needs the last three added.

`oml doctor` checks the route. It reports `oauth: failing` with the failing URL when the proxy drops a
path or a node serves another issuer. It reports `off` when the server names no issuer. Then issue the connection
with `oml keys issue-agent <key-name> --principal <principal> --workarea <workarea> --connect-code`
([claude.ai and mobile](../guides/claude-ai-and-mobile.md)).

## Check that it is healthy

The server answers `/healthz` and `/readyz` ([HTTP API](../api/http-api.md)).

- `/healthz` answers 200 while the process is up. It stays shallow, so a PostgreSQL outage does not
  restart the server in a loop.
- `/readyz` answers 503 until PostgreSQL is reachable and the **startup fork check** passes. The check
  compares the stored chain head with the last signed head.

If a restore or failover lost committed events, or the server loaded a different signing key, the server
stays down until you resolve it. There is no override that accepts and forks the chain
([runbooks](runbooks.md)).

`oml doctor` prints each signal with a verdict and exits 1 on any failure
([observability](observability.md)).

## Back up the database and the keyring together

A server with no backup loses every memory, journal event and key when its disk fails. Take backups, and
rehearse one restore, before anyone relies on the server ([Back up and restore](restore.md)).

| What | How | What the copy holds |
|---|---|---|
| PostgreSQL | A daily `pg_dump` as a superuser, because forced row security stops the owner role's dump. Or base backups with WAL archiving. | Everything, including content erased after the copy was taken. |
| The keyring file | Copy it with the database, under its own protection. | A usable copy of the signing key and the key-encryption key. |
| The lake | Not a backup. It is derived from the journal. | Ciphertext bodies and digests. |

Losing the keyring while the database survives loses the deployment's identity. The chain stays readable
and verifiable, but the server cannot extend it. Any copy of the keyring is a usable key copy, so guard
copies as you guard the live file.

A backup is also a copy of erased content. Your backup retention window is the upper bound on how long
erased content survives, so state it where your compliance statement needs it
([compliance](../product/compliance.md)).

## Size the server

One server process serves every tenant, and writes in one tenant serialize on that tenant's chain lock.
Resource requests ship as comments, not values, because no sizing number is measured. The measured
figures, and the service levels one host can run to, are on [Capacity and SLOs](operating.md).

## Next

- Claim the server and issue keys: [Administer an OML server](../guides/administering.md).
- Install on a cluster: [Install OML on Kubernetes](../guides/kubernetes.md).
- Analyze usage: [Run the lake writer](lake.md).
- Move to a new release: [Upgrade OML](upgrades.md).
