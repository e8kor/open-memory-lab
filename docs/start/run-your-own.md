---
status: Stable
verified: 2026-11-10
answers: "how to run an OML server: oml quickstart on a laptop, one host with TLS for a team (the server, the web console and a proxy), and Kubernetes; what each gives and what it needs"
---

# Run a server

Three options, each a larger trust domain: a laptop, one host that people reach over TLS, and a
Kubernetes cluster. The settings are in [configuration](../operations/configuration.md) and the
profiles in [deployment](../operations/deployment.md).

## 1. Start a server on the laptop

Docker with compose v2 and the `oml` CLI ([install](install.md)) are required. Without Docker, the [quickstart](quickstart.md) has a variant that runs the release binaries on the user's own PostgreSQL.

```sh
oml quickstart
```

In order, it:

1. writes `oml/compose.yaml` with the server image pinned to the CLI's version, and an owner-only
   `oml/.env` with generated passwords for the database's superuser, owner role and serving role,
   and the secret the server and the web console share;
2. runs `docker compose up --detach --wait`: PostgreSQL, the server, the web console (`oml-web`)
   and a Caddy router in front of both, published on `127.0.0.1:8080` only (`--port` changes it).
   The router sends each page the web console serves to it and every other path to the server, so
   the browser and the CLI see one address;
3. reads the enrollment token from the server's log and claims the server, storing the admin
   pair in `oml/credentials`, not in the user's own credentials file;
4. creates the workarea `crew` and issues an agent key into `oml/keys/agent` and a reviewer key
   into `oml/keys/reviewer`, both owner-only;
5. saves a memory as the agent, approves it as the reviewer and finds it as the agent, and names
   the step that failed if one does;
6. prints the key files, `oml setup claude-code --url http://127.0.0.1:8080 --key-file
   oml/keys/agent` and `oml login http://127.0.0.1:8080`.

A second run in the same directory is refused as `quickstart.exists`; a machine without compose
is refused as `quickstart.docker`; `--port` frees a taken port. `docker compose
--project-directory oml down -v` removes the stack and its data
([CLI
reference](../cli/reference.md)).

The containers are separated from each other, but anyone with root on the host reads every
volume, password and the signing keyring ([deployment](../operations/deployment.md)). Use this
for trying OML and for one person's memory, not as a team's server.

## 2. Run a server for a team over TLS

The quickstart gives no TLS, public address, backups or signing key held apart from the data.
A team's host runs two processes behind one proxy: the server, `oml-server-lead`, and the web
console, `oml-web`, which calls the server's HTTP API. For a team on one host:

1. Run PostgreSQL 17 or later with the `ltree` and `pg_trgm` extensions and the two roles the server needs: an owner that migrates and a serving role that owns nothing. The server refuses to start when both URLs log in as one role. The SQL is under [run on one host behind TLS](../operations/deployment.md#run-on-one-host-behind-tls).
2. Install `oml-server-lead` from the [server archive](../operations/deployment.md#install-the-server-archive), verified with cosign, and set the two connection strings:

    ```sh
    export OML_POSTGRES__OWNER_URL=postgres://oml_owner:...@127.0.0.1:5432/oml
    export OML_POSTGRES__URL=postgres://oml_server:...@127.0.0.1:5432/oml
    oml-server-lead
    ```

3. Claim the server from the host before a proxy exposes it, so nobody else presents the enrollment token first ([claim the first admin key](../operations/deployment.md#claim-the-first-admin-key)).
4. Start `oml-web` beside the server, from the `oml-web` archive or the image, with the same
   `web.client_secret` on both ([run the web console](../operations/web-console.md)).
5. Put a reverse proxy with TLS in front of both, on one origin: each page `oml-web` serves goes
   to it, every other path to the server. `deploy/caddy/Caddyfile` is a Caddy configuration that
   does this ([the proxy rules](../operations/deployment.md#put-a-reverse-proxy-in-front)).
6. For claude.ai and the phone apps, set `oauth.issuer` to the public origin and pass the `/.well-known/oauth-*` paths through ([connect claude.ai and the mobile apps](../operations/deployment.md#connect-claudeai-and-the-mobile-apps)).
7. Back up the database and the keyring together; losing the keyring while the database survives loses the deployment's identity ([back up](../operations/deployment.md#back-up-the-database-and-the-keyring-together)).
8. Check it with `oml doctor`, `oml-web --doctor` and `/readyz` ([check that it is healthy](../operations/deployment.md#check-that-it-is-healthy)).

## 3. Install on Kubernetes

The chart in `deploy/helm/oml` runs one server instance under `Recreate` against an external
PostgreSQL, with its keyring on a claim that `helm uninstall` keeps, and the web console as a
second Deployment beside it. The
[Kubernetes guide](../guides/kubernetes.md) walks the install.
Every upgrade and restart stops the API until the new instance is ready. The chart runs one
instance: there are no read followers and no cluster mode.

## Next steps

Give each agent its own key with `oml keys issue-agent --out`
([administer a server](../guides/administering.md)). Against the quickstart's stack, run it as
`OML_CLI_CREDENTIALS=./oml/credentials oml keys issue-agent …`, because the quickstart keeps its
admin pair there and not in the file `oml login` writes.
