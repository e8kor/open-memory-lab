---
status: Stable
verified: 2026-11-06
answers: "what to do when the CLI, installer, login, Claude Code plugin, claude.ai connection, hooks, recall or review does not work, by symptom"
---

# Troubleshoot the client

Run this first:

```bash
oml doctor
```

It prints each check with `ok`, `warn` or `fail`, and where each value came from, never the key. It exits `0` on `ok` or `warn`, `1` on `fail` and `2` when the server cannot be reached. For server-side symptoms, see [troubleshoot the server](troubleshooting-server.md).

## `url` reads `fail no_server`, or `api key` reads `unset`

No source sets the value. No release names a server, so the address always comes from the caller or from the server that issued the key. The pair comes from flags, then `OML_URL` and `OML_API_KEY`, then the credentials file, whole from the first source that sets either.

```bash
oml login <url>
```

If `next` says a source sets one value without the other, set both in that source or unset the one there. See [CLI reference](../cli/reference.md).

The other lines that name a missing or shared server:

- **`plugin : warn plugin_no_server`**: the plugin has a key and no `url`, so its tools reach no server. Set `url` with `oml setup claude-code --url <server>` or `claude plugin configure oml@open-memory-lab`.
- **`oauth : warn server_origin`** (an admin key): the server sets no `oauth.issuer`, so its pages cannot show agents the URL to set beside their keys. Set `oauth.issuer` ([configuration](../operations/configuration.md)).
- **`sync : warn sync_shared_server`**: the server says it is shared and this clone holds no consent for it. Run `oml sync` once with `--allow-public-server` ([sync](../use/sync.md)).
- **`sync : warn sync_consent_unbound`**: the clone holds the old `oml.allowPublicServer=true`, which names no server and is no longer read. Run `oml sync` once with `--allow-public-server` against the server in question: it records that server's origin and removes the old value, and the line goes.
- **`error: refused setup.no_server`** from `oml setup claude-code`: a key came without a URL off a terminal. Pass `--url` beside `--key-file`, or export `OML_URL` with `OML_API_KEY`.

## `oml login` fails

`oml login` writes nothing unless the server accepts the key, so a failed login leaves the old file as it was.

- **`refused (401): auth.invalid`**: the server does not know the key. Paste the key issued by this server.
- **`no API key … on the terminal; nothing was sent`** (exit `2`): the paste was empty. Piped input must put the key on the first line.
- **`holds a character outside printable ASCII`**: a stray character came with the paste.
- **`cannot reach the server`**: give the address without `/mcp`.

## `whoami` fails with `401 auth.invalid`

The key is a typo, comes from another server, or was denied. Log in again with the right key.

When the answer's `details.reason` is `expired`, the key reached the end date its administrator set, and `oml doctor` prints `fail expired` with the date. The key and its memories are still there: the administrator moves the date with `oml keys limit <key> --expires <instant>`, or issues another key.

```bash
oml login <url>
```

## The Claude Code plugin shows no tools

Work down this list. Each step assumes the ones above it hold.

1. Set the key and the address inside Claude Code, or from a shell.

   ```bash
   oml setup claude-code
   ```

   Exported `OML_URL` and `OML_API_KEY` do not reach the plugin. See [Claude Code](../guides/claude-code.md).
2. Use the server's address. `url` has no default: left unset, the plugin reaches no server and the session-start hook prints `no_server: CLAUDE_PLUGIN_OPTION_URL is not set`. A key sent to a server that did not issue it is refused with `401 auth.invalid`. See [install](../start/install.md).
3. Give the address without `/mcp`. The plugin appends it.
4. Use an agent key. `oml setup claude-code` refuses an `admin` or `operator` key with `setup.admin_key`.

   ```bash
   oml keys issue-agent --out <file>
   ```

5. Use Claude Code 2.1.285 or later. An older one is refused with `setup.claude_code_version`.
6. On `setup.marketplace`, remove the other marketplace, then set up again.

   ```bash
   claude plugin marketplace remove open-memory-lab
   ```

7. Run `/reload-plugins` or restart Claude Code.
8. Allow `mcp__plugin_oml_oml__oml_read` in `permissions.allow` instead of any older tool name.

## A command prints `refused (<status>): <code>`

The CLI prints the code, a message and a `rule:` line, then exits `1`. Branch on the code. The full list is in [errors](../api/errors.md).

| Code | Cause | Fix |
|---|---|---|
| `auth.missing` | The client has no key. | Set `api_key`, `OML_API_KEY` or run `oml login`. |
| `access.not_found` | The thing does not exist or the key may not read it. | Ask for a reader grant on the workspace. |
| `access.verb_ceiling` | The key's role never calls this verb. | Use a key of the right role. |
| `request.workspace_unresolved` | The call names no workspace and the key has no default. | Pass `workspace`. |
| `submission.scan_veto` | The scan refused the write. `details.finding` names the rule class. | Remove what it names. Nothing was stored. |
| `review.author_in_lineage` | The reviewer wrote the memory or one it derives from. | Review with a different identity. |
| `review.serve_invalid` | The served item expired, was used, or changed. | Run `oml review next` again. |
| `limit.rate` | A cap was reached: unused client registrations, recall feedback labels for the day, or the key's requests per minute. | Retry after `Retry-After` seconds. |

## A memory does not appear in recall

A new memory is held. Nobody but its author can recall it until a different reviewer approves it.

- **If the key wrote it**, find it with `include_unreviewed: true` on `oml_read`. Or list the writes with their `state`.
- **If someone else wrote it**, a reviewer key approves it. See [review held memories](../guides/reviewing.md).
- **If it is approved but missing**, check that the key has a reader grant on the workspace. Without one the answer is `access.not_found`.

Recall matches any of the words by default. Try other words. `--match all` requires every word, so remove it if it was used.

```bash
oml read --query "<words>"
oml keys show <key>
```

## Chat capture does nothing

Capture runs from three hooks: `oml hook user-prompt`, `oml hook stop` and `oml hook pre-compact`. Each exits `0` and never blocks.

1. Set `OML_CHAT_CAPTURE=1` and `OML_CHAT_WORKSPACE` in the environment Claude Code runs from.
2. Confirm the plugin is loaded and `oml` is on the `PATH` Claude Code uses for hooks.
3. Check the key has a writer grant on the workspace named by `OML_CHAT_WORKSPACE`.

Capture fires every `OML_CHAT_CAPTURE_EVERY` turns (default 5) and after a compaction. See [chat memory](../use/chat-memory.md).

## The session-start hook prints nothing

The hook fails open. Every failure ends with no output and exit `0`. Likely causes:

- `oml` is not on the hook's `PATH`.
- `OML_SESSION_CONTEXT` is set to something other than `on`, `off` or empty.
- Only one of the plugin's two settings is set. With the key alone, the hook prints `no_server: CLAUDE_PLUGIN_OPTION_URL is not set`.
- The server took longer than 3 seconds, or its certificate does not chain to a public root.
- The key is write-only, so recall refuses it.

Run it by hand to see the reason:

```bash
printf '{"source":"startup","cwd":"%s"}\n' "$PWD" | oml hook session-start
```

## Failure recall stays silent

`oml hook tool-failure` is off unless `OML_FAILURE_RECALL=1` is set. When on, it sends nothing for a non-Bash tool, an interrupted command, an error with no distinctive word left, or a query this session already sent. One failed request stops it for the rest of the session. See [CLI reference](../cli/reference.md).

## The installer refuses

Every refusal starts `oml install:`, names a reason and installs nothing.

- **`SHA-256 … SHA256SUMS says …`**: the download is damaged or altered. Rerun. If it repeats, do not install that archive.
- **`cosign did not verify SHA256SUMS`**: the signature is not the release workflow's at that tag. Do not work around it.
- **`--require-signature was given and cosign is not on PATH`**: install cosign, or drop the flag and accept a checksum-only check.
- **`lists no <archive>`**: there is no build for this platform at that release.

To check by hand, see [install](../start/install.md).

## claude.ai or the phone app does not connect

- **`oauth.connect_code_invalid`**: the code is unknown, expired, used, or its key is denied. A code lives ten minutes and works once. The administrator issues another.

  ```bash
  oml keys connect-code <key>
  ```

- **`oauth.authorization_expired`**: the request outlived ten minutes or five attempts. Start again from the connector.
- **`oauth.connect_refused`**: `details.reason` says why. `not_configured` means `oauth.issuer` is unset.
- **Claude cannot reach the server**: a server on `127.0.0.1` is unreachable from claude.ai. A public HTTPS address is required, `oauth.issuer` set to it, and `oml doctor` showing `oauth` as `ok`. See [claude.ai and mobile](../guides/claude-ai-and-mobile.md).
