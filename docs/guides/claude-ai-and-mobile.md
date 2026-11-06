<!--
id: guides/claude-ai-and-mobile
tier: engineering
status: draft for review · since v0.4.0; no live claude.ai connection made
answers: how claude.ai, Claude Desktop's connectors and the Claude mobile apps reach an OML server through a one-time connect code on the server's own origin; what the server needs; the administrator's and the user's steps; what chat and mobile get and do not get; how a connection ends; and the open question of claude.ai's request headers
-->

# Guide: claude.ai and mobile

**Status:** draft for review · since v0.4.0; no live claude.ai connection made

**Release:** since v0.4.0 ([releases](../product/releases.md))

Claude on the web, Claude Desktop's connectors and the Claude apps for iOS and Android cannot run
`oml login`, so OML links them to one agent key through a one-time connect code, over OAuth 2.1,
on the same origin that serves `/mcp`: there is no separate host, port or service
([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md)). Claude never
receives the key, only a connection the server can withdraw.

**Unverified.** `oml-server-lead`'s `oauth_flow` suite and `oml-cli`'s `connect_flow` suite run
the connect flow end to end over HTTP against the real server, but they run only where the database
role can create roles, as CI's does, and no green run of either is recorded yet. No connection from
claude.ai itself has been made either, so every step below that names a claude.ai screen is
Anthropic's documentation, not a recorded run.

## On the public server: sign up, then follow /web/start

Where the operator has opened sign-up ([ADR-0068](../adr/ADR-0068-anyone-signs-in-and-gets-a-private-root-under-quotas-when-the-operator-opens-sign-up.md), proposed), no administrator is needed.
Sign up at `https://memory.doublegate.io/web` with Google or GitHub, accept the terms, and on
`/web/start` choose **claude.ai, Desktop or phone**. One button issues a connector key and shows
its connect code once, with the exact connector URL and copy buttons. Then in claude.ai:

1. Open Customize, then Connectors.
2. Add a custom connector and paste the URL.
3. Connect, and enter the code on the page that opens.

The page turns from "Waiting for your first connection…" to "Connected ✓", and then to "First
memory held — review it". Behind a copy button the page also offers a line for claude.ai's
personal preferences, so Claude uses the connector without being asked each time:

> I use the Open Memory Lab connector as my long-term memory. Before answering about my projects,
> decisions or preferences, search it with oml_read. When we settle a fact, a decision or a
> finding worth keeping, save it with oml_write in one or two plain sentences. What you write
> waits for my review before anyone else can read it.

The rest of this guide is for a server you run, where an administrator issues the code.

## What the server needs

1. A public HTTPS origin, with `oauth.issuer` set to it (`OML_OAUTH__ISSUER=https://oml.example.com`).
   Unset, connections are off and a connect code is refused.
2. A reverse proxy that passes `/mcp`, `/.well-known/oauth-*` and `/oauth/*` through unchanged
   ([deployment](../operations/deployment.md#connections-behind-a-reverse-proxy)).
3. `oml doctor` reporting `oauth` as passing, which fetches both discovery documents from the
   issuer the server names in `/readyz`.

A laptop on `127.0.0.1` cannot be reached by claude.ai; this path needs a server people reach over
the internet ([self-hosting](self-hosting.md)).

## The administrator: issue a connector key and its code

```sh
oml keys issue-agent claude-mobile --principal crew/claude-mobile --workarea <workarea> --connect-code
```

It prints two lines: the connector address, such as `https://oml.example.com/mcp`, and a code
such as `7KQ2-M9XD-4TPA`, which works once, for ten minutes. The key has no value at all. A code
that ran out before use is replaced with `oml keys connect-code <key>`
([CLI reference](../cli/reference.md#oml-keys-issuance-revocation-rotation)).

## The user: add the connector and enter the code once

1. On claude.ai, open Customize > Connectors and add a custom connector with exactly the printed
   address.
2. Claude opens OML's connect page. Enter the code only if you reached the page this way yourself;
   if someone sent you a link to it, close it.
3. The page shows the organisation, the agent identity and the workareas the connection will use,
   and what it may do there. Where it may write, it says that nothing written reaches anyone else
   until someone else reviews it. Confirm.
4. The connector is then available in every Claude app signed in to that account, the phone apps
   included.

Anthropic offers custom connectors on every plan, one on the Free plan ([custom
connectors](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp),
fetched 2026-11-04: "Free users are limited to one custom connector.").

## What chat and mobile get

The three tools, `oml_write`, `oml_read` and `oml_workarea`, under the connector key's grants.
They get no skill, no session-start hook, no failure recall and no librarian: those are parts of
the Claude Code plugin ([ADR-0040](../adr/ADR-0040-plugin-setup-in-claude-code.md),
consequences). The agent searches when the server's instructions and the tools' descriptions lead
it to. Every write is held, as from any client.

## Ending a connection

The administrator ends one connection with `oml keys disconnect <key> <connection>`, after
`oml keys show <key>` names it; withdrawing the key's access with `oml deny set <key> universe`
ends every connection's reach, since a connection's token is exactly its key
([ADR-0031](../adr/ADR-0031-oauth-connections-by-connect-code.md) d11). Changing `oauth.issuer`
stops every connection at its next request.

## Open: claude.ai's request headers

Anthropic's help page also describes a Request headers setting for a connector, to "add fixed
credentials such as API keys that Claude sends on every request" (fetched 2026-11-04 through a
summarising fetch, so the quote is reported, not checked against the page's source). If it holds,
claude.ai could reach OML with a bearer key and no OAuth, which ADR-0031's premise does not
expect. OML has not tested that route, it would put the key value in claude.ai's settings, and
this guide does not recommend it. The connect code is the documented path.
