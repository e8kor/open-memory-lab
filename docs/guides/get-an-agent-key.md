---
status: Stable
verified: 2026-11-06
answers: "how to get an agent key: from the web user page of a server with a sign-in, or with oml keys issue-agent as an administrator or on a server run by the user"
---

# Get an agent key

An agent key lets one agent write and recall in one workarea. Every client in the [Claude Code guide](claude-code.md) and the [other tools guide](other-harnesses.md) needs one. Pick the matching row.

| Role | Get the key from |
|---|---|
| Signed up on a server that has the web pages on | [the web user page](#from-the-web-user-page) |
| An administrator, or the person who runs the server | [`oml keys issue-agent`](#with-the-cli) |
| Trying OML alone on the machine | [the quickstart](../start/quickstart.md), which writes `oml/keys/agent` |

## From the web user page

The server's address and an email address on its allowlist are required, unless its operator opened sign-up. The page shows the address beside each key it issues.

1. Open the server's address in a browser and sign in with Google or GitHub.
2. Open the Keys tab. Under "Issue a key for an agent", enter a name and choose `writer`. Choose `reader` for an agent that only recalls. To end the key on a date, set "Key expires": the key stops working when that day ends in UTC, and only an administrator can change the date later.
3. Select "Issue key". The page shows the value once. Copy it into a file only the owner can read, for example `~/.config/oml/keys/agent`, with mode `0600`.

The key reads and writes in the user's own workarea and nowhere else. The Connect tab lists the next step for each client.

## With the CLI

An admin key on the server is required, stored by `oml login` or `oml bootstrap claim`.

```sh
oml keys issue-agent my-agent --principal crew/my-agent --workarea crew --out oml/keys/agent
```

`--out` writes the value to a new file, mode `0600`, and prints the command that reads it. The value reaches no terminal and no log. Give each agent its own name and principal. For a person's own root, use `--user <name>` in place of `--workarea`. The options are in the [CLI reference](../cli/reference.md#oml-keys-issue-agent); the administrator's view is in [administer a server](administering.md).

## Use the key

- Claude Code: `oml setup claude-code --url <server> --key-file oml/keys/agent` ([guide](claude-code.md)).
- Another tool: [other agent tools](other-harnesses.md).
- Claude on the web or a phone takes a connector key instead of a value ([guide](claude-ai-and-mobile.md)).

A key cannot approve memories. Review needs a separate reviewer key ([review held memories](reviewing.md)).
