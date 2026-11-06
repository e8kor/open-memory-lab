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

## `url` or `api key` reads `unset`

No source sets the value. The pair comes from flags, then `OML_URL` and `OML_API_KEY`, then the credentials file, whole from the first source that sets either.

```bash
oml login <url>
```

If `next` says a source sets one value without the other, set both in that source or unset the one there. See [CLI reference](../cli/reference.md).

## `oml login` fails

`oml login` writes nothing unless the server accepts the key, so a failed login leaves the old file as it was.

- **`refused (401): auth.invalid`**: the server does not know the key. Paste the key issued by this server.
- **`no API key … on the terminal; nothing was sent`** (exit `2`): the paste was empty. Piped input must put the key on the first line.
- **`holds a character outside printable ASCII`**: a stray character came with the paste.
- **`cannot reach the server`**: give the address without `/mcp`.

## `whoami` fails with `401 auth.invalid`

The key is a typo, comes from another server, or was denied. Log in again with the right key.

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
2. Use your server's address. A `url` left unset is the public server, which refuses a key it did not issue. See [install](../start/install.md).
3. Give the address without `/mcp`. The plugin appends it.
4. Use an agent key. `oml setup claude-code` refuses an `admin` or `operator` key with `setup.admin_key`.

   ```bash
   oml keys issue-agent --out <file>
   ```

5. Use Claude Code 2.1.285 or later. An older one is refused with `setup.claude_code_version`.
6. If you see `setup.marketplace`, remove the other marketplace, then set up again.

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
| `access.not_found` | The thing does not exist or the key may not read it. | Ask for a reader grant on the workarea. |
| `access.verb_ceiling` | The key's role never calls this verb. | Use a key of the right role. |
| `request.workarea_unresolved` | The call names no workarea and the key has no default. | Pass `workarea`. |
| `submission.scan_veto` | The scan refused the write. `details.finding` names the rule class. | Remove what it names. Nothing was stored. |
| `review.author_in_lineage` | The reviewer wrote the memory or one it derives from. | Review with a different identity. |
| `review.serve_invalid` | The served item expired, was used, or changed. | Run `oml review next` again. |
| `limit.rate` | Too many requests. | Retry after `Retry-After` seconds. |

## My memory does not appear when I recall

A new memory is held. Nobody but its author can recall it until a different reviewer approves it.

- **If you wrote it**, find it with `include_unreviewed: true` on `oml_read`. Or list your writes with their `state`.
- **If someone else wrote it**, a reviewer key approves it. See [review held memories](../guides/reviewing.md).
- **If it is approved but missing**, check that your key has a reader grant on the workarea. Without one you get `access.not_found`.

Recall matches any of your words by default. Try other words. `--match all` requires every word, so remove it if you used it.

```bash
oml read --query "<words>"
oml keys show <key>
```

## Chat capture does nothing

Capture runs from three hooks: `oml hook user-prompt`, `oml hook stop` and `oml hook pre-compact`. Each exits `0` and never blocks.

1. Set `OML_CHAT_CAPTURE=1` and `OML_CHAT_WORKAREA` in the environment Claude Code runs from.
2. Confirm the plugin is loaded and `oml` is on the `PATH` Claude Code uses for hooks.
3. Check the key has a writer grant on the workarea named by `OML_CHAT_WORKAREA`.

Capture fires every `OML_CHAT_CAPTURE_EVERY` turns (default 5) and after a compaction. See [chat memory](../use/chat-memory.md).

## The session-start hook prints nothing

The hook fails open. Every failure ends with no output and exit `0`. Likely causes:

- `oml` is not on the hook's `PATH`.
- `OML_SESSION_CONTEXT` is set to something other than `on`, `off` or empty.
- Only one of the plugin's two settings is set.
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

- **`oauth.connect_code_invalid`**: the code is unknown, expired, used, or its key is denied. A code lives ten minutes and works once. Your administrator issues another.

  ```bash
  oml keys connect-code <key>
  ```

- **`oauth.authorization_expired`**: the request outlived ten minutes or five attempts. Start again from the connector.
- **`oauth.connect_refused`**: `details.reason` says why. `not_configured` means `oauth.issuer` is unset.
- **Claude cannot reach the server**: a server on `127.0.0.1` is unreachable from claude.ai. You need a public HTTPS address, `oauth.issuer` set to it, and `oml doctor` showing `oauth` as `ok`. See [claude.ai and mobile](../guides/claude-ai-and-mobile.md).
