---
status: Stable
verified: 2026-11-06
answers: "how to start a server, write a memory as an agent, see it held, approve it as a second identity and recall it from another key, with the output of each command and a measured time"
---

# Quickstart: see a memory held, then approved

You start a server on your machine, write a memory as an agent and watch it stay invisible until a different key approves it. You need the `oml` CLI ([install](install.md)) and either PostgreSQL 17 or later or Docker.

**Time.** Measured on 2026-11-06 on Linux x86_64 with the v0.6.0 release binaries and PostgreSQL 18.6, from the download to the last recall: 1.8 s to download both archives, 1.5 s to verify and unpack them, and 2.9 s for everything after that, about 6 s of command time in all. Typing and reading take longer, so allow five minutes. The Docker tab, measured in CI on a GitHub-hosted ubuntu runner on 2026-11-06, took 7 s from `oml quickstart` (start, claim and both keys) through the first held write to the first recall of it, with the image already on the runner. It excludes the image pull and the `colleague` key, so a first run takes longer by the pull.

## 1. Start a server and issue three keys

You get a server, a workarea `crew`, an agent key, a second agent key called `colleague` and a reviewer key.

=== "Binaries, no Docker"

    Install PostgreSQL 17 or later with `ltree` and `pg_trgm` (the contrib package). The first block makes a throwaway cluster on port 54329. Use your own database if you have one.

    ```sh
    initdb -D oml/pg -A trust -U postgres
    pg_ctl -D oml/pg -o "-p 54329 -k $PWD/oml -c listen_addresses=127.0.0.1" -l oml/pg.log -w start
    psql -h 127.0.0.1 -p 54329 -U postgres \
      -c "CREATE ROLE oml_owner LOGIN PASSWORD 'ownerpw'" \
      -c "CREATE ROLE oml_server LOGIN PASSWORD 'serverpw'" \
      -c "CREATE DATABASE oml OWNER oml_owner"
    ```

    Download the server and the CLI, verify both with cosign and unpack them, as in [install the server archive](../operations/deployment.md#install-the-server-archive) (`oml-server-lead-0.6.0-x86_64-unknown-linux-gnu` and `oml-cli-0.6.0-x86_64-unknown-linux-musl`). Then start the server. It prints a single-use enrollment token in its log.

    ```sh
    mkdir -p oml/srv oml/keys && chmod 700 oml/keys && umask 077
    cd oml/srv
    OML_POSTGRES__OWNER_URL=postgres://oml_owner:ownerpw@127.0.0.1:54329/oml \
      OML_POSTGRES__URL=postgres://oml_server:serverpw@127.0.0.1:54329/oml \
      ../../oml-server-lead-0.6.0-x86_64-unknown-linux-gnu/oml-server-lead > ../server.log 2>&1 &
    echo $! > ../server.pid; cd ../..
    ```

    Claim the server with the token, create the workarea and issue the keys. The claim keeps the admin key in `oml/credentials`, apart from your own login file.

    ```sh
    until curl -fs http://127.0.0.1:8080/readyz >/dev/null; do sleep 1; done
    TOKEN=$(grep -o 'enr_[0-9a-f]\{48\}' oml/server.log | tail -1)
    export OML_CLI_CREDENTIALS=$PWD/oml/credentials
    printf '%s\n' "$TOKEN" | oml --url http://127.0.0.1:8080 bootstrap claim admin \
      --token-stdin --tenant-name demo --principal platform/admin
    oml workareas create universe crew
    oml keys issue-agent agent --principal crew/agent --workarea crew --out oml/keys/agent
    oml keys issue-agent colleague --principal crew/colleague --workarea crew --out oml/keys/colleague
    oml keys issue reviewer --role dreamer --held-by human --principal crew/reviewer \
      --default-workarea crew --grant crew:dreamer --json | jq -r .value > oml/keys/reviewer
    unset OML_CLI_CREDENTIALS
    ```

    ```text
    enrollment token accepted — consumed (single-use; a second claim is refused)
    first admin key — SHOWN ONCE, NOT RECOVERABLE, NOT JOURNALLED AS VALUE

        key name: admin
        key     : oml_…
        tenant  : 41733dd0-efd0-4d34-835c-896ba82f0e1c

    credentials stored in /…/oml/credentials (owner-only); the server keeps only the key hash.
    created crew (2e82c1a8-7505-44ec-97b1-1f4bde4dc2b2) universe.crew (status: development)
    key agent (c773a866-0619-4a9a-9c94-58c67d6689a4) issued to crew/agent (writer, held by agent): …
    ```

=== "Docker"

    You need Docker with compose v2. One command starts the database and server, claims the server and issues the agent and reviewer keys.

    ```sh
    oml quickstart
    OML_CLI_CREDENTIALS=$PWD/oml/credentials \
      oml keys issue-agent colleague --principal crew/colleague --workarea crew --out oml/keys/colleague
    ```

    The key files are `oml/keys/agent`, `oml/keys/reviewer` and `oml/keys/colleague`.

Both paths end the same way. Name the keys in your shell and point the CLI at the server:

```sh
export OML_URL=http://127.0.0.1:8080
A=$(cat oml/keys/agent); R=$(cat oml/keys/reviewer); C=$(cat oml/keys/colleague)
```

If `OML_URL` or `OML_API_KEY` were already set for another server, these lines replace them for this shell.

## 2. Write a memory as the agent

```sh
OML_API_KEY=$A oml write "We deploy on Tuesdays; Friday deploys need two approvals."
```

```text
accepted — held (journal seq 13)
  artifact  : 80a08d01-11b8-483a-ba9c-cd6d9af2bac1
  claim_key : auto-fa13d72b8435838a
  workarea  : 2e82c1a8-7505-44ec-97b1-1f4bde4dc2b2
```

The memory is `held`: stored and signed, readable by no one yet.

## 3. Recall it: nothing comes back

```sh
OML_API_KEY=$A oml read --query "deploy Friday"
```

```text
0 hit(s) — query digest sha256:8d376cdf…
```

A held memory is invisible to every read, even its author's.

## 4. Ask for your own unreviewed memories

```sh
OML_API_KEY=$A oml read --query "deploy Friday" --include-unreviewed
```

```text
1 hit(s) — query digest sha256:ad5dc872…

  80a08d01-11b8-483a-ba9c-cd6d9af2bac1  auto-fa13d72b8435838a  (held)
    | We deploy on Tuesdays; Friday deploys need two approvals.
```

The author sees its own write, marked `held`. The second agent, `colleague`, still finds nothing:

```sh
OML_API_KEY=$C oml read --query "deploy Friday"
```

```text
0 hit(s) — query digest sha256:8d376cdf…
```

## 5. Approve it as a different identity

Take the artifact id from step 2. The agent's own key cannot approve:

```sh
ID=$(OML_API_KEY=$A oml read --query "deploy Friday" --include-unreviewed --json | jq -r '.. | .artifact_id? // empty' | head -1)
OML_API_KEY=$A oml review approve --ids $ID
```

```text
refused (403): access.verb_ceiling — bulk review needs a dreamer key held by a human, and this is a writer key held by agent
  rule: R1: the surface an agent can call never promotes, signs, demotes or deletes; bulk review is a person's act.
```

The reviewer key can:

```sh
OML_API_KEY=$R oml review approve --ids $ID
```

```text
  80a08d01-11b8-483a-ba9c-cd6d9af2bac1  approved  journal seq 15
1 approved, 0 rejected, 0 skipped
```

## 6. Recall it with another key

```sh
OML_API_KEY=$C oml read --query "deploy Friday"
```

```text
1 hit(s) — query digest sha256:8d376cdf…

  80a08d01-11b8-483a-ba9c-cd6d9af2bac1  auto-fa13d72b8435838a  (promoted)
    | We deploy on Tuesdays; Friday deploys need two approvals.
```

The memory is now `promoted`, and every key with read access to `crew` finds it, with no flag.

## What you saw

The write waited for a reviewer, and the reviewer had to be someone other than the author. Approval changed who could read the memory, not how it ranks. The cost is a delay between writing and recall; [why a write is held](../concepts/why-a-write-is-held.md) explains what that buys.

## Clean up

Binaries: stop the server and the database, then delete the directory.

```sh
kill $(cat oml/server.pid); pg_ctl -D oml/pg -m fast stop; rm -rf oml
```

Docker: `docker compose --project-directory oml down -v`.

## Next

- [Get an agent key](../guides/get-an-agent-key.md) for a real server, then connect [Claude Code](../guides/claude-code.md), [claude.ai and your phone](../guides/claude-ai-and-mobile.md) or [another tool](../guides/other-harnesses.md). For Claude Code on this machine: `oml setup claude-code --url http://127.0.0.1:8080 --key-file oml/keys/agent`.
- Run the server for a team: [run your own server](run-your-own.md).
- Review a queue: [review held memories](../guides/reviewing.md).
