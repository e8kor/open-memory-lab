---
status: Stable
verified: 2026-11-06
answers: "what to do when something does not work, by symptom: the plugin serving no tools, each oml doctor line, oml login refusals, the error codes a user meets, a memory that does not appear, the session-start and failure-recall hooks staying silent, installer refusals, connect-code failures, and on the server /readyz at 503, postgres.role_split boot refusals, migrations and the proxy's access log; then a short FAQ"
---

# Troubleshooting by symptom

Each entry is a symptom, its cause and the command that fixes it. The behavior is declared on
the pages linked from each entry; this page only orders it by what you see. Common questions
are in the [FAQ](../faq.md). Whether Claude Code runs the plugin's hooks, and a live connection
from claude.ai, are open verifications ([](https://github.com/e8kor/open-memory-lab/issues/114),
[](https://github.com/e8kor/open-memory-lab/issues/115)).

## Start with `oml doctor`

`oml doctor` checks the URL and key the CLI starts from and refuses nothing. It prints where
each value came from, never the key
([CLI reference](../cli/reference.md#oml-login-and-oml-doctor-storing-and-checking-the-pair)).
It exits `0` on `ok` or `warn` and `1` on `fail`. Line by line:

| Line | Reads | Means, and the fix |
|---|---|---|
| `url`, `api key` | `unset` | No source sets the value. `next` names what to set: `--url` and `--api-key`, `OML_URL` and `OML_API_KEY`, or `oml login <url>`. |
| `next` | `… sets url without api_key`, `OML_URL is set without OML_API_KEY` | The first source that sets either value sets only one. The pair always comes from one source, so a key is never sent to a URL from another: set both in that source, or unset the one there. |
| `next` | `… is not read since v0.4.0: rename it OML_URL` | A v0.3 variable is set. Rename both v0.3 variables to `OML_URL` and `OML_API_KEY` ([upgrading](../operations/upgrades.md)). |
| `healthz` | `fail` | The process answered but is not healthy. |
| `readyz` | `fail`, with `postgres`, `fork_check`, `names_assigned` below | The server is up and refuses traffic. See [`/readyz` answers 503](#server-readyz-answers-503). |
| `whoami` | `fail (401 auth.invalid)` | The server does not know the key: a typo, a key from another server, or an expired or revoked key. Run `oml login <url>` again with the right key. |
| `whoami` | `not called (no key with this URL)` | A `warn`: the source holds a URL and no key. |
| `chain head` | `unavailable (needs a key with events-tail access)` | Information, not a failure: an agent key cannot read the events tail. |
| `oauth` | `off (no oauth.issuer: /mcp takes keys only)` | Connections are off. Only claude.ai and the phone apps need them. |
| `oauth` | `failing (<url>: <reason>)` | The server names an issuer in `/readyz`, but a discovery document at that URL did not load or describes another issuer: usually a proxy that does not pass `/.well-known/oauth-*` ([reverse proxy](../operations/deployment.md#connections-behind-a-reverse-proxy)). |

When the server cannot be reached at all, doctor prints `error: cannot reach the server: …` and
exits `2`. Over `https` behind an internal certificate authority the CLI also cannot connect: it
carries public roots and does not read the system's trust store
([CLI reference](../cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin)).

## `oml login` fails

`oml login <url>` writes nothing unless `GET /v2/whoami` accepts the key, so a failed login
leaves the old file as it was.

- **`refused (401): auth.invalid — …`**, exit `1`: the server does not know the key. Check you
  pasted the agent key for this server.
- **`no API key for <url> on the terminal; nothing was sent and nothing written`**, exit `2`: an
  empty paste. Piped, the message says `on stdin`: the key must be the first line.
- **`… holds a character outside printable ASCII`**: a stray character in the paste.
  No key holds one.
- **`error: cannot reach the server: …`**: the URL is wrong or the host is down. Use the server's
  address without `/mcp`.

## The Claude Code plugin shows no tools or does not connect

Work down the list; each step assumes the ones above it hold.

1. **The key set.** The plugin serves no tools until `api_key` is set, and `url` names
   your server; exported `OML_URL` and `OML_API_KEY` do not reach it since v0.4.0. Inside Claude
   Code run `/plugin configure oml@open-memory-lab`, or from a shell `oml setup claude-code`,
   which hands the pair to `claude plugin configure oml@open-memory-lab --values-stdin`
   ([Claude Code](claude-code.md)).
2. **The right server.** Since v0.4.1 a `url` left unset is the maintainer's public server,
   `https://memory.doublegate.io`. A key from another server is refused there with `401
   auth.invalid`, and that server's operator has seen it: set `url` to your server and ask your
   administrator to replace the key. `oml setup claude-code` given no URL prints that it used the
   public server, and on a refusal that the key went there.
3. **`url` without `/mcp`.** The plugin appends `/mcp` itself; a `url` ending in `/mcp` points at
   `/mcp/mcp`.
4. **An agent key.** `oml setup claude-code` refuses an `admin` or `operator` key as
   `setup.admin_key`. Issue an agent key with `oml keys issue-agent --out`
   ([CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation)).
5. **Claude Code 2.1.285 or later.** `claude plugin configure --values-stdin` needs it; an
   older `claude --version` is refused as `setup.claude_code_version`, nothing installed.
6. **The marketplace.** `refused: setup.marketplace — Claude Code already holds a marketplace
   named open-memory-lab from <source>` means one from a local checkout or another source.
   `claude plugin marketplace remove open-memory-lab` removes it and the plugins installed from
   it; then run `oml setup claude-code` again.
7. **Reloaded.** Run `/reload-plugins` or restart Claude Code after configuring.
8. **The permission rules.** A `permissions.allow` rule naming a v0.3 tool allows
   nothing. Allow `mcp__plugin_oml_oml__oml_read` and
   `mcp__plugin_oml_oml__oml_workarea` ([upgrading](../operations/upgrades.md)).

With no `claude` on `PATH`, or with `--print`, `oml setup claude-code` runs nothing after
`whoami` and prints the commands to run yourself.

## The error codes you are likely to meet

The CLI prints a refusal as `refused (<status>): <code> — <message>` and then `rule: …`, and
exits `1`. Branch on the code, never the message. The full list is the
[error taxonomy](../api/errors.md).

| Code | Usual cause | Fix |
|---|---|---|
| `auth.missing` (401) | No `Authorization: Bearer` header: the client has no key. | Set the plugin's `api_key`, or `--api-key`, `OML_API_KEY` or `oml login`. |
| `auth.invalid` (401) | Unknown, expired or revoked key; `details.reason` is `unknown` as built. | Use the key this server issued. |
| `access.not_found` (404) | The thing does not exist or the key may not read it, indistinguishably. Also a key with a writer grant and no reader grant, whose recall is refused this way. | Ask the administrator for a reader grant on the workarea; check the workarea's name. |
| `access.verb_ceiling` (403) | The key's role never calls this verb: a writer key reviewing, an admin key posting a verdict or reading content. | Use a key of the right role. No grant lifts it. |
| `request.workarea_unresolved` (422) | No `workarea` in the call and no default workarea on the key. | Pass `workarea`, or ask the administrator for a key with a default workarea. |
| `request.validation` (400) | A malformed body, or an argument outside the mode the others chose. | The message names the field. |
| `submission.scan_veto` (422) | The scan refused the write; `details.finding` names the rule class. | Remove what it names. Nothing was stored. |
| `review.author_in_lineage` (403) | The reviewer wrote the memory, or one it derives from. | Review with a different identity. |
| `review.serve_invalid` (409) | The serve expired, was used, or its content changed. | Run `oml review next` again. |
| `limit.rate` (429) | The key or tenant is over its rate limit. | Retry after `Retry-After` seconds. |
| `system.unavailable` (503) | The server cannot serve safely; `details.reason` says why, such as `fork_check_failed`. | Retryable; the operator looks at `/readyz`. |

## My memory does not appear when I recall

That is the design, not a fault. Every write is held until a reviewer who is not its author, and
not the author of anything it derives from, approves it (R2), and a held memory is invisible to
every other key on every read path (R3;
[invariant map](../architecture/overview.md#5-the-invariant-map)).

- **The author** finds its own held write with `oml_read` and `include_unreviewed: true`, and
  lists its own writes with their `state` with `oml_read` and no `query`.
- **Everyone else** finds it once a reviewer with a `dreamer` key runs `oml review next` and
  `oml review approve` ([reviewing memories](reviewing.md);
  [CLI reference](../cli/reference.md#oml-review-the-review-ceremony)).
- **Still missing after approval:** in this build a hit names every word of the `query`, so
  try fewer or other words; and the key needs a reader grant on that workarea, else
  `access.not_found`.

## The session-start hook prints nothing

The hook fails open: every failure ends with no output and exit `0`, so a session always starts.
It prints at most one stderr line naming why
([CLI reference](../cli/reference.md#oml-hook-session-start-session-context-for-the-agent-plugin)).
The usual causes:

- `oml` is not on the `PATH` Claude Code runs hooks with, or is older than v0.2.2: the hook's
  command exits `0` with nothing.
- `OML_SESSION_CONTEXT=off`, or a value other than `on`, `off` or empty.
- Only one of the plugin's two settings is set.
- The server took longer than 3 seconds, the whole run longer than 8, or its certificate does
  not chain to a public root.
- The key is write-only: recall refuses it, and the hook then prints no context at all.

Run it by hand to see the reason. Outside Claude Code it reads `OML_URL` and `OML_API_KEY`, never
the stored credentials:

```sh
printf '{"source":"startup","cwd":"%s"}\n' "$PWD" | oml hook session-start
```

## Failure recall stays silent

`oml hook tool-failure` is off unless `OML_FAILURE_RECALL=1` is set in the environment Claude
Code runs from. Even on, it sends nothing for a non-Bash tool, an interrupted command, `grep`,
`test`, `[` or `diff`, an error with no distinctive word left after redaction, a query this
session already sent, or any session in which one earlier request failed: one failure stops it
for the rest of the session. It recalls promoted memories only, never held ones
([CLI reference](../cli/reference.md#oml-hook-tool-failure-recall-after-a-failed-command)).

## The installer refuses

Every refusal starts `oml install:`, names its reason and installs nothing
([install](../start/install.md#what-it-checks-and-its-two-outcomes)).

- **`… has SHA-256 <a>, SHA256SUMS says <b>; nothing was installed`**: a damaged or altered
  download. Rerun; if it repeats, do not install that archive.
- **`cosign did not verify SHA256SUMS of v<version> under the release workflow's identity`**:
  the signature is not the release workflow's at that tag. Do not work around it.
- **`--require-signature was given and cosign is not on PATH`**: install cosign, or drop the
  flag and accept a checksum-only check, which the script then says it did.
- **`SHA256SUMS of v<version> lists no <archive>`**: no build for this platform at that release.
- **`<dir>/oml printed '…', expected 'oml <version>'`**: the binary copied into place is not
  the requested version.

To check by hand, follow [by hand, with cosign](../start/install.md#by-hand-with-cosign).

## claude.ai, Claude Desktop and mobile do not connect

- **The connect page says `oauth.connect_code_invalid`** and shows the attempts left: the code
  is unknown, expired, voided, already used, or its key is revoked, one answer for all. A code
  lives ten minutes and works once; the administrator issues another with `oml keys
  connect-code <key>`.
- **Refused as `oauth.authorization_expired`**: the authorization request outlived
  its ten minutes or used its five attempts. Start again from the connector.
- **`oml keys issue-agent … --connect-code` is refused as `oauth.connect_refused`**:
  `details.reason` names why, `not_configured` when `oauth.issuer` is unset and `role` for an
  admin or operator key.
- **Claude cannot reach the server at all**: a server on `127.0.0.1` is unreachable from
  claude.ai. The path needs a public HTTPS origin, `oauth.issuer` set to it, and `oml doctor`
  reporting `oauth` as `ok` ([claude.ai and mobile](claude-ai-and-mobile.md)).

## Server: `/readyz` answers 503

`/readyz` names three checks, and any one not `ok` keeps it at 503:

- **`postgres: unreachable`**: the serving pool cannot reach the database. Fix the database or
  `OML_POSTGRES__URL`.
- **`fork_check: failed`**: the stored chain head did not verify against the last signed head.
  Writes answer `system.unavailable` and reads keep flowing. Staying down is deliberate: follow
  [fork check failing](../operations/runbooks.md#fork-check-failing) and never force readiness.
- **`names_assigned: unassigned`**: the start could not name a row that predates names, and
  every route but the two probes answers `system.unavailable`.

## Server: the lead exits with `postgres.role_split`

The lead checks its two database roles before and after migrating, and exits before binding a
listener when the split is wrong, logging `event` `role_split_refused` with the reason. A good boot logs
`role_split_booted` with `owner_role` and `serving_role`.

| Reason | Means | Fix |
|---|---|---|
| `owner_url_missing` | `postgres.owner_url` is unset; no single-URL mode exists. | Set `OML_POSTGRES__OWNER_URL` to the owner role. |
| `same_role` | Both URLs log in as one role. | Create a separate owner role and split the install ([splitting](../operations/upgrades.md#splitting-a-single-role-install-the-upgrade-to-v031)). |
| `different_databases` | The two logins see different databases. | Point both URLs at the same database. |
| `serving_role_privileged` | The serving role is a superuser, has `CREATEDB`, `CREATEROLE`, `REPLICATION` or `BYPASSRLS`, is a member of a role, or can create in the database or `public`. | `ALTER ROLE <role> NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;` and `REVOKE CREATE ON DATABASE <database> FROM <role>;` |
| `serving_role_owns` | The serving role owns the database or an object in it, as every unsplit install does. | Move ownership to the owner (`REASSIGN OWNED`, step 4 of the split). |
| `serving_role_reads_migrations` | The serving role holds a privilege on `_sqlx_migrations`. | Revoke it; the serving role holds none there. |
| `serving_role_ungranted` | The serving role has no `SELECT` on `journal`: the migrations did not grant the role `postgres.url` names. | declares the grant. |
| `table_unfenced`, `definer_function` | A table but `_sqlx_migrations` is not the owner's, lacks forced row-level security or has no policy; or a function in `public` is `SECURITY DEFINER`. | No setting fixes these; declares each rule. |

`postgres.role_split: the check could not read the catalog: …` proves nothing and refuses as
well: the owner URL could not read the catalog.

## Server: migrations

The lead runs the migrations itself at boot, under `postgres.owner_url`, before serving under
`postgres.url`; there is no separate migrate step. They are forward-only, so rolling a binary back
never rolls the schema back. A boot that stops with `postgres owner connect: …` could not log in
as the owner; one with `migrate on boot: …` failed while migrating
([migrations](../operations/upgrades.md#migrations-forward-only-owner-role-bootstrap-run)).

## Server: the proxy's access log records codes

With `oauth.issuer` set, the `Location` header of `POST /oauth/authorize` and the bodies of
`POST /oauth/authorize` and `POST /oauth/token` carry codes and tokens. An access log that
records them keeps live credentials. Filter the header out, as the Caddy example on
[deployment](../operations/deployment.md#connections-behind-a-reverse-proxy) does. `oml doctor`
cannot read a proxy's configuration, so it cannot check this for you
([observability](../operations/observability.md#per-profile-checks)).

## FAQ

**Why can't my agent approve its own memory?** No agent tool promotes, signs, demotes or deletes
(R1; [the verb ceiling](../api/mcp-plugin.md#the-verb-ceiling-what-this-surface-can-never-do-r1)),
and a verdict counts only from an identity that wrote neither the memory nor anything it derives
from (R2). Two keys with one principal are one identity for this check. The one exception, a
`development` workarea's `self_signed` rows, is declared in.

**Where are credentials stored?** The CLI keeps `url` and `api_key` in the owner-only file
`~/.config/oml/credentials`, under `%USERPROFILE%` on Windows when `HOME` is unset;
`OML_CLI_CREDENTIALS` names another file. The plugin's key is in the system keychain, never in
`settings.json`. `oml keys issue-agent --out` writes a key to a new owner-only file. `oml
quickstart` keeps its admin pair in `oml/credentials`, not in yours.

**Which setting wins?** For the CLI: flags, then the environment, then the credentials file,
taken as a whole pair from the first source that sets either value
([start settings](../cli/reference.md#start-settings)). The hooks read the plugin's two settings
first and `OML_URL` with `OML_API_KEY` second, and never the credentials file.

**Why is my admin verb refused in the shell my agent runs in?** An environment holding the
agent's `OML_URL` and `OML_API_KEY` wins over the stored admin pair, so the verb ran under the
agent's key. Unset both variables to run an admin verb there.

**Does removing the plugin delete my memories?** No. The memories stay on the server; the
administrator keeps a key out of every workarea with `oml deny set <key> universe`
([Claude Code](claude-code.md#removing-it)).
