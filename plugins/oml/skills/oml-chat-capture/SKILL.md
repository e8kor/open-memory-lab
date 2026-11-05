---
name: oml-chat-capture
description: Use when an OML chat capture request asks you, before you stop, to write what turns of this conversation established. Covers what counts as a fact worth keeping, one oml_write per fact with origin kind chat and the turn's anchor from the request, idempotency keys, the per-request budget, and when to write nothing.
---

# Capturing chat facts into Open Memory Lab

The plugin's `Stop` hook sends a capture request when chat capture is switched on
(`OML_CHAT_CAPTURE=1`) and enough turns have passed. The request names the turns, the
workarea, your budget of facts, and each turn's anchor. You extract the facts yourself,
from the conversation you already hold: no other model reads it, and the OML server only
stores what you write (ADR-0063 d6).

Every write is held for review, as every `oml_write` is. A person approves what reaches
their team; nothing you write here is visible to anyone else until then.

## What a fact is

Write one claim per fact, in the third person, that will still hold in a later session:

- a preference the person stated ("The person prefers squash merges.");
- a decision taken in these turns, and why, in one sentence;
- a constraint of their project or environment ("The staging database is PostgreSQL 18.");
- a correction of something earlier, stated as the corrected fact.

Do not write:

- a summary of the session, or of what you did;
- your own reasoning, plans or guesses;
- instructions to future agents;
- secrets, keys, passwords, tokens, or anything copied from a credential file;
- facts about people other than the person you are working with.

**Zero facts is a fine answer.** Most turns settle nothing that outlives the session. If
nothing qualifies, write nothing and stop.

## How to write each fact

One `oml_write` per fact, with:

- `text`: the fact, one or two sentences;
- `workarea`: the one the request names;
- `kind`: `memory`;
- `claim_key`: `chat/<topic>`, a short lower-case slug of what the fact is about, such as
  `chat/merge-style`. Use the same key when a later fact replaces an earlier one, and
  send `replaces` with that claim's latest version, so the old preference is superseded,
  not repeated;
- `origin`: `{"kind": "chat"}`;
- `anchor`: the turn the fact came from, exactly as the request gives it:
  `{"uri": "chat://claude-code/<session>", "fragment": "turn=<n>", "hash": "<that turn's hash>", "hasher": "oml-turn/1"}`.
  For a fact from older turns the request lists as a span, use that span and the hash
  it names;
- `idempotency_key`: `chat-<session>-t<n>-<i>`, where `<i>` counts the facts from turn
  `<n>` from 1, so a repeated request replays instead of writing twice.

Stay within the number of facts the request allows. When it says excerpts are allowed
(`OML_CHAT_EXCERPT=1`), you may add one line under the fact starting with `> `, quoting at
most 280 characters of what the person said. Never quote a secret, a credential or anything
private about someone else: the server runs its scanner over the excerpt as over any text, but
what that scanner catches depends on its rules, so do not rely on it.

## After writing

Stop. Do not report the writes to the person unless they asked about memory; the
request came from a hook, not from them.
