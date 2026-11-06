---
status: Stable
verified: 2026-11-06
answers: "what to do when a server's readiness check fails, the database role split refuses boot, migrations stop, a proxy logs credentials, search model files are missing or the lake writer stalls"
---

# Troubleshoot the server

Run this first, with an admin or operator key:

```bash
oml doctor
```

It prints each check with `ok`, `warn` or `fail`. For client-side symptoms, see [troubleshoot the client](troubleshooting-client.md).

## `/readyz` answers 503

`/readyz` names three checks. Any one that is not `ok` keeps it at 503.

- **`postgres: unreachable`**: the serving pool cannot reach the database. Fix the database or `OML_POSTGRES__URL`.
- **`fork_check: failed`**: the stored chain head did not verify. Writes return `system.unavailable` and reads continue. Do not force readiness. See [fork check failing](../operations/runbooks.md#fork-check-failing).
- **`names_assigned: unassigned`**: the start could not name a row that predates names. Every route except the two probes returns `system.unavailable`. Restart the lead after the upgrade finishes. See [upgrades](../operations/upgrades.md).

## The lead exits with `postgres.role_split`

The lead checks its two database roles before and after migrating. It exits before binding a listener when the split is wrong, and logs `role_split_refused` with the reason.

| Reason | Cause | Fix |
|---|---|---|
| `owner_url_missing` | `postgres.owner_url` is unset. | Set `OML_POSTGRES__OWNER_URL` to the owner role. |
| `same_role` | Both URLs log in as one role. | Create a separate owner role. See [upgrades](../operations/upgrades.md). |
| `different_databases` | The two logins see different databases. | Point both URLs at one database. |
| `serving_role_privileged` | The serving role is a superuser or holds `CREATEDB`, `CREATEROLE`, `REPLICATION`, `BYPASSRLS`, or can create in the database. | `ALTER ROLE <role> NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;` then `REVOKE CREATE ON DATABASE <database> FROM <role>;` |
| `serving_role_owns` | The serving role owns the database or an object in it. | Move ownership to the owner role with `REASSIGN OWNED`. |
| `serving_role_reads_migrations` | The serving role holds a privilege on `_sqlx_migrations`. | Revoke it. |
| `serving_role_ungranted` | The serving role has no `SELECT` on `journal`. | Let the migrations run under the owner role, then restart. |
| `table_unfenced`, `definer_function` | A table lacks forced row-level security or a policy, or a function in `public` is `SECURITY DEFINER`. | No setting fixes these. Restore the schema from the migrations. |

If the log says `the check could not read the catalog`, the owner URL could not read the catalog. Fix the owner role's access.

## The lead stops during migrations

The lead runs migrations at boot under `postgres.owner_url`, then serves under `postgres.url`. Migrations only move forward, so rolling the binary back never rolls the schema back.

- **`postgres owner connect: …`**: the owner login failed. Fix the owner URL or password.
- **`migrate on boot: …`**: a migration failed. Read the message, fix the cause and restart. Restore from backup if the schema is half-applied. See [restore](../operations/restore.md).

## The proxy's access log records credentials

With `oauth.issuer` set, the `Location` header of `POST /oauth/authorize` and the bodies of `POST /oauth/authorize` and `POST /oauth/token` carry codes and tokens. An access log that records them keeps live credentials.

1. Configure the proxy to omit that header and those bodies. See [deployment](../operations/deployment.md).
2. Delete the log lines already written.

`oml doctor` cannot read a proxy's configuration, so it cannot check this.

## Search model files are missing

The reranker needs `config.json`, `tokenizer.json` and `model.safetensors` in `search.rerank.model_dir`. The server checks each file against a pinned SHA-256 and stops at start on a mismatch. `oml doctor` warns when reranks are skipped.

Either place the files, or let the server download them once.

```bash
OML_SEARCH__RERANK__FETCH=true oml-server-lead
```

Set it back to `false` afterward. `false` never uses the network. See [configuration](../operations/configuration.md).

## The lake writer is behind or stopped

Check the writer's own health first.

```bash
oml-lakewriter doctor
curl -s http://127.0.0.1:9466/metrics | grep -E 'oml_lake_(lag|commit_failures|dead_letters)'
```

`lake.writer` fails when no writer holds the lease. Start it again. For lag that keeps growing, see [the lake falls behind](../operations/runbooks.md#the-lake-falls-behind) and [the lake](../operations/lake.md).
