<!--
id: start/index
tier: client
status: current
answers: which of three paths to take (use a server, run a server, administer a server) and what each needs
-->

# Get started

Pick the row that matches you. Every client needs two settings, the server's address and a key;
the server derives everything else from the key. The Claude Code plugin and the other tools'
configurations default the address to the public server, `https://memory.doublegate.io`, so with
a key from that server the key is the only setting.

| You are | You have | Do this |
|---|---|---|
| A user | an agent key from your administrator | [Install the CLI](install.md), then [connect Claude Code](../guides/claude-code.md), [claude.ai or a phone](../guides/claude-ai-and-mobile.md) or [another tool](../guides/other-harnesses.md) |
| Running a server | a machine with Docker | [Run your own server](run-your-own.md) |
| An administrator | an admin key | [First workarea and keys](first-workarea.md) |

!!! tip "Something failed?"
    Installer refuses or prints "NOT checked": [installer problems](../guides/troubleshooting.md#the-installer-refuses) and the [FAQ](../faq.md#install-and-connect).
    `oml login` fails: [login problems](../guides/troubleshooting.md#oml-login-fails).
    Plugin shows no tools: [plugin problems](../guides/troubleshooting.md#the-claude-code-plugin-shows-no-tools-or-does-not-connect).

## Use a server (about two minutes with a key)

1. Install `oml` (Linux, Windows) with the line on the [install page](install.md).
2. Run `oml login <server url>`; it asks for the key without echo and stores it only after the
   server accepts it. With a key from the public server, skip this step.
3. Run `oml setup claude-code` and reload plugins in Claude Code.
4. Ask your agent to remember something: [your first memory](first-memory.md).

## Run a server (about five minutes with Docker)

1. Install `oml`.
2. Run `oml quickstart`. It starts the database and server on `127.0.0.1:8080`, claims the
   server, creates the workarea `crew`, issues an agent key and a reviewer key, and checks one
   save, approval and read end to end.
3. Run the `oml setup claude-code` line it prints.
4. Go on to [Run your own server](run-your-own.md) for a server a team can reach.

## Administer a server (under a minute per key)

1. Store your admin pair with `oml login`.
2. Run `oml keys issue-agent <name> --principal <p> --workarea <w> --out <file>`; the key goes
   into a private file, never onto the screen.
3. Send the person the `oml setup claude-code --url … --key-file …` line the command prints, and
   the file over a channel you trust.
4. Delete the file once the line has run. Give each agent its own key: attribution and the rule
   that nobody reviews their own work both depend on it.

## What every path has in common

- **Two settings.** The address and the key. The CLI reads flags, then `OML_URL` and
  `OML_API_KEY`, then the file `oml login` wrote. Apart from `oml setup claude-code`, no command
  has a default address, so an administrator's command never reaches the public server unasked.
- **Held first.** What an agent saves reaches nobody else until a different identity approves it.
  A fact your agent just stored does not show up for a teammate yet.
