---
status: Stable
verified: 2026-11-06
answers: "how claude.ai, Claude Desktop's connectors and the Claude mobile apps reach an OML server through a one-time connect code on the server's own origin; what the server needs; the administrator's and the user's steps; what chat and mobile get and do not get; and how a connection ends"
---

# Connect claude.ai and your phone

Claude on the web, Claude Desktop's connectors and the Claude apps for iOS and Android cannot run
`oml login`. OML links them to one agent key through a one-time connect code, over OAuth 2.1, on
the same origin that serves `/mcp`. There is no separate host, port or service. Claude never
receives the key, only a connection the server can withdraw.

The server's `oauth_flow` and the CLI's `connect_flow` test suites run this flow end to end in CI.
No live connection from claude.ai has been tested yet, so the steps that name a claude.ai screen
follow Anthropic's documentation.

## Before you start

The server needs three things.

1. A public HTTPS origin, with `oauth.issuer` set to it (`OML_OAUTH__ISSUER=https://oml.example.com`).
   Unset, connections are off and a connect code is refused.
2. A reverse proxy that passes `/mcp`, `/.well-known/oauth-*` and `/oauth/*` through unchanged
   ([deployment](../operations/deployment.md)).
3. `oml doctor` reporting `oauth` as passing, which fetches both discovery documents from the
   issuer the server names in `/readyz`.

Claude cannot reach a laptop on `127.0.0.1`. This path needs a server people reach over the
internet ([run your own server](../start/run-your-own.md)).

## 1. Issue a connector key and its code (administrator)

```sh
oml keys issue-agent claude-mobile --principal crew/claude-mobile --workarea <workarea> --connect-code
```

It prints two lines: the connector address, such as `https://oml.example.com/mcp`, and a code
such as `7KQ2-M9XD-4TPA`. The code works once, for ten minutes. The key has no value. Replace a
code that ran out with `oml keys connect-code <key>`
([CLI reference](../cli/reference.md)).

## 2. Add the connector and enter the code once (user)

1. On claude.ai, open Customize > Connectors and add a custom connector with the printed
   address.
2. Claude opens OML's connect page. Enter the code only if you reached the page this way yourself;
   if someone sent you a link to it, close it.
3. The page shows the organization, the agent identity and the workareas the connection will use,
   and what it may do there. Where it may write, it says that nothing written reaches anyone else
   until someone else reviews it. Confirm.
4. The connector is then available in every Claude app signed in to that account, the phone apps
   included.

Anthropic offers custom connectors on every plan, one on the Free plan ([custom
connectors](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp),
fetched 2026-11-04: "Free users are limited to one custom connector.").

## What chat and mobile get

The three tools, `oml_write`, `oml_read` and `oml_workarea`, under the connector key's grants.
They get no skill, no session-start hook, no failure recall and no librarian, because those are
parts of the Claude Code plugin. The agent searches when the server's instructions and the tools'
descriptions lead it to. Every write is held, as from any client.

## End a connection

The administrator ends one connection with `oml keys disconnect <key> <connection>`, after
`oml keys show <key>` names it. To end every connection's reach, run `oml deny set <key> universe`,
because a connection's token is its key. Changing `oauth.issuer` stops every connection at its
next request.

If a step fails, see [client troubleshooting](../help/troubleshooting-client.md).
