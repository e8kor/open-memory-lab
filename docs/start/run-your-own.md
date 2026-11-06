---
status: Stable
verified: 2026-11-06
answers: "how to run an OML server: oml quickstart on a laptop, one host with TLS for a team, and Kubernetes; what each gives and what it needs"
---

# Run your own server

Three options, each a larger trust domain: a laptop, one host that people reach over TLS, and a
Kubernetes cluster. The settings are in [configuration](../operations/configuration.md) and the
profiles in [deployment](../operations/deployment.md).

## 1. Start a server on your laptop

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
reference](../cli/reference.md)).

The containers are separated from each other, but anyone with root on the host reads every
volume, password and the signing keyring ([deployment](../operations/deployment.md)). Use this
for trying OML and for one person's memory, not as a team's server.

## 2. Run a server for a team over TLS

The quickstart gives no TLS, public address, backups or signing key held apart from the data.
For a team on one host:

1. Run PostgreSQL 18 with pgvector and the two roles the server needs: an owner that migrates and
   a serving role that owns nothing. The server refuses to start when both URLs log in as one role.
2. Install `oml-server-lead` from the [server archive](../operations/deployment.md),
   verified with cosign, and set `OML_POSTGRES__OWNER_URL` and `OML_POSTGRES__URL`.
3. Put a reverse proxy with TLS in front of it and claim the server before the proxy exposes it,
   so nobody else presents the enrollment token first
   ([the bootstrap race](../operations/deployment.md)).
4. For claude.ai and the phone apps, set `oauth.issuer` to the public origin and pass the
   `/.well-known/oauth-*` paths through ([claude.ai and your phone](../guides/claude-ai-and-mobile.md)).
5. Back up the database and the keyring together; losing the keyring while the database survives
   loses the deployment's identity
   ([backups](../operations/deployment.md)).
6. Check it with `oml doctor` and `/readyz`.

## 3. Install on Kubernetes

The chart in `deploy/helm/oml` runs one server instance under `Recreate` against an external
PostgreSQL, with its keyring on a claim that `helm uninstall` keeps. The
[Kubernetes guide](../guides/kubernetes.md) walks the install.
Every upgrade and restart stops the API until the new instance is ready. The chart runs one
instance: there are no read followers and no cluster mode.

## Next steps

Give each agent its own key with `oml keys issue-agent --out`
([administer a server](../guides/administering.md)). Against the quickstart's stack, run it as
`OML_CLI_CREDENTIALS=./oml/credentials oml keys issue-agent …`, because the quickstart keeps its
admin pair there and not in the file `oml login` writes.
