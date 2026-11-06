---
status: Stable
verified: 2026-11-06
answers: "how to run an OML server: oml quickstart on a laptop, one host with TLS for a team, and Kubernetes; what each gives and what it needs"
---

# Run your own server

Three steps, each a larger trust domain: a laptop, one host that people reach over TLS, and a
Kubernetes cluster. The settings are on [configuration](../operations/configuration.md) and the
profiles on [deployment](../operations/deployment.md); this page is the order.

## 1. A laptop: `oml quickstart`

You need Docker with compose v2 and the `oml` CLI ([install](install.md)).

```sh
oml quickstart
```

In order, it:

1. writes `oml/compose.yaml` with the server image pinned to the CLI's version, and an owner-only
   `oml/.env` with generated passwords for the database's superuser, owner role and serving role;
2. runs `docker compose up --detach --wait`, published on `127.0.0.1:8080` only (`--port`
   changes it);
3. reads the enrollment token from the server's log and claims the server, storing the admin
   pair in `oml/credentials`, not in your own credentials file;
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
reference](../cli/reference.md#oml-setup-claude-code-and-oml-quickstart-the-quick-start)).

The containers are separated from each other, but anyone with root on the host reads every
volume, password and the signing keyring ([deployment](../operations/deployment.md#the-docker-profile)).
Use this for trying OML and for one person's memory, not as a team's server.

## 2. One host for a team, over TLS

The quickstart gives no TLS, public address, backups or signing key held apart from the data.
For a team on one host:

1. Run PostgreSQL 18 with pgvector and the two roles the server needs: an owner that migrates and
   a serving role that owns nothing. The server
   refuses to start when both URLs log in as one role.
2. Install `oml-server-lead` from the [server archive](../operations/deployment.md#the-server-archive),
   verified with cosign, and set `OML_POSTGRES__OWNER_URL` and `OML_POSTGRES__URL`.
3. Put a reverse proxy with TLS in front of it and claim the server before the proxy exposes it,
   so nobody else presents the enrollment token first
   ([the bootstrap race](../operations/deployment.md#claiming-the-first-admin-key-the-bootstrap-race)).
4. For claude.ai and the phone apps, set `oauth.issuer` to the public origin and pass the
   `/.well-known/oauth-*` paths through ([claude.ai and mobile](../guides/claude-ai-and-mobile.md)).
5. Back up the database and the keyring together; losing the keyring while the database survives
   loses the deployment's identity
   ([backups](../operations/deployment.md#backups-retention-windows-and-the-erasure-slo)).
6. Check it: `oml doctor` and `/readyz`.

## 3. Kubernetes

The chart in `deploy/helm/oml` runs one server instance under `Recreate` against an external
PostgreSQL, with its keyring on a claim that `helm uninstall` keeps
([the chart](../operations/deployment.md#the-interim-chart-the-current-build-on-kubernetes),); the [Kubernetes guide](../guides/kubernetes.md)
walks the install. CI installs it on kind. Every upgrade and restart stops the API until the new
instance is ready. Read followers and cluster mode are open work
([](https://github.com/e8kor/open-memory-lab/issues/117)).

## Next

Give each agent its own key with `oml keys issue-agent --out`
([first workarea and keys](../guides/administering.md)). Against the quickstart's stack, run it as
`OML_CLI_CREDENTIALS=./oml/credentials oml keys issue-agent …`, because the quickstart keeps its
admin pair there and not in the file `oml login` writes.
