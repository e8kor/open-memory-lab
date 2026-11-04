<!--
id: guides/self-hosting
tier: engineering
status: draft for review · oml quickstart since v0.4.0; its compose file not yet run under Docker
answers: how to run OML yourself: oml quickstart on a laptop and what it writes, runs and prints; the docker profile's one-trust-domain caveat; then one host with TLS from the server archive behind a reverse proxy; then Kubernetes with the interim chart; and what each step does not give
-->

# Guide: self-hosting

**Status:** draft for review · `oml quickstart` since v0.4.0; its compose
file not yet run under Docker

**Release:** compose since v0.1.0, the server archive since v0.3.1, the chart since v0.3.0;
`oml quickstart` since v0.4.0 ([releases](../product/releases.md))

Three steps, each a larger trust domain: a laptop, one host that people reach over TLS, and a
Kubernetes cluster. The settings are declared on [configuration](../operations/configuration.md)
and the profiles on [deployment](../operations/deployment.md); this page is the order.

## 1. A laptop: `oml quickstart`

You need Docker with compose v2 and the `oml` CLI ([install](../start/install.md)).

```sh
oml quickstart
```

What it does, in order ([ADR-0041](../adr/ADR-0041-a-quick-start-in-one-command-per-role.md) d9;
[CLI reference](../cli/reference.md#oml-setup-claude-code-and-oml-quickstart-the-quick-start)):

1. Writes `oml/compose.yaml`, embedded in the binary, with the server image pinned to the CLI's
   own version, and an owner-only `oml/.env` with generated passwords for the database's
   superuser, its owner role and its serving role.
2. Runs `docker compose up --detach --wait` under a project named from the directory's absolute
   path, published on `127.0.0.1:8080` only (`--port` changes it).
3. Reads the enrollment token from the server's log and claims the server, storing the admin pair
   in `oml/credentials`, not in your own credentials file.
4. Creates the workarea `crew`, and issues an agent key into `oml/keys/agent` and a reviewer key
   into `oml/keys/reviewer`, both owner-only.
5. Saves a memory as the agent, approves it as the reviewer and finds it as the agent, and stops
   naming the step that failed if one does.
6. Prints the key files, `oml setup claude-code --url http://127.0.0.1:8080 --key-file
   oml/keys/agent` and `oml login http://127.0.0.1:8080`.

A second run against the same directory is refused as `quickstart.exists`; a machine without
compose is refused as `quickstart.docker`. `docker compose --project-directory oml down -v`
removes the stack and its data. About five minutes on a machine with Docker, most of it the image
download, is a target, unmeasured ([FR-68](../product/roadmap.md)).

**Unverified.** The verb runs in `oml-cli`'s `quickstart` suite against a fake `docker` and the
real server, and CI's `compose` job runs its compose file once the branch is pushed; no green run
of that job is recorded yet (ADR-0041, "Where each item stands").

**One trust domain.** The containers are separated from each other, but anyone with root on the
host reads every volume, every password and the signing keyring
([deployment](../operations/deployment.md#the-docker-profile)). Use it to try OML and for one
person's memory, not as a team's server.

## 2. One host for a team, over TLS

What `oml quickstart` does not give: TLS, a public address, backups and a signing key held apart
from the data. For a team on one host:

1. Run PostgreSQL with pgvector, with the two roles the server needs: an owner that migrates and
   a serving role that owns nothing
   ([ADR-0034](../adr/ADR-0034-the-lead-serves-as-a-role-that-owns-nothing.md) d2). The server
   refuses to start when both URLs log in as one role.
2. Install `oml-server-lead` from the [server archive](../operations/deployment.md#the-server-archive),
   verified with cosign, and set `OML_POSTGRES__OWNER_URL` and `OML_POSTGRES__URL`.
3. Put a reverse proxy with TLS in front of it, and claim the server before the proxy exposes
   it, so that nobody else presents the enrollment token first
   ([the bootstrap race](../operations/deployment.md#claiming-the-first-admin-key-the-bootstrap-race)).
4. For claude.ai and the phone apps, set `oauth.issuer` to the public origin and pass the OAuth
   paths through ([claude.ai and mobile](claude-ai-and-mobile.md)).
5. Back up the database and the keyring together; losing the keyring while the database is kept
   loses the deployment's identity ([deployment](../operations/deployment.md#backups-retention-windows-and-the-erasure-slo)).

The `oml standalone` supervisor the design describes is not built.

## 3. Kubernetes: the interim chart

`deploy/helm/oml` runs one server instance under `Recreate`, against an external PostgreSQL,
with its keyring on a claim that `helm uninstall` keeps
([the interim chart](../operations/deployment.md#the-interim-chart-the-current-build-on-kubernetes),
[ADR-0033](../adr/ADR-0033-interim-helm-chart.md), proposed). CI installs it on kind; no other
cluster is recorded running it. Every upgrade and restart stops the API until the new instance is
ready, for a window that is unmeasured. Read followers and cluster mode are not scheduled.

## Next

Give each agent its own key with `oml keys issue-agent --out` ([get
started](../start/index.md#add-an-agent-you-administer-a-server)), and give reviewers their own.
Against the quick start's stack, run it as `OML_CLI_CREDENTIALS=./oml/credentials oml keys
issue-agent …`: the quick start keeps its admin pair there, not in the file `oml login` writes.
