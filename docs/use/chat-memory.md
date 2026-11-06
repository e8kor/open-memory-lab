---
status: Stable
verified: 2026-11-06
answers: "what chat memory captures, what it does not, how to switch it on and off, the three hooks it uses, and where captured facts go"
---

# Capture facts from your chats

Chat memory keeps the facts an agent writes from your conversation. It does not store the
conversation. A turn becomes a memory only when the agent in it writes the fact, and every memory
names its source kind (`origin.kind`). You need a person's key and the
[Claude Code plugin](../guides/claude-code.md).

1. **Create the workarea.** `oml chat init` creates `chat` under your user root, puts it in
   progress and prints the `OML_CHAT_WORKAREA` value. An agent key is refused.
2. **Switch capture on.** Set `OML_CHAT_CAPTURE=1` and `OML_CHAT_WORKAREA`. Three hooks run, each
   exiting 0 without blocking:
    - `oml hook user-prompt` counts the turn and records its hash.
    - `oml hook stop` counts turns. Every `OML_CHAT_CAPTURE_EVERY` turns (default 5) it asks the
      agent to use the `oml:oml-chat-capture` skill: at most 6 facts, each anchored to its turn.
    - `oml hook pre-compact` records a compaction, so the next `stop` asks at once.
3. **Recall in the next prompt (optional).** `OML_TURN_RECALL=1` makes `user-prompt` send up to eight
   distinctive words of your prompt as a recall, with secret shapes, URLs and the key removed. Hits
   are marked `[unreviewed]`. It is off by default and falls silent after three misses.
4. **Review.** Captured facts are held, in your `chat` workarea. Review them on the user page or
   with `oml review`.
5. **Switch off.** Unset `OML_CHAT_CAPTURE`. Retire stored facts with a retirement write.

**Privacy.** The cache keeps a keyed hash of each prompt, never the prompt. The session id is
hashed with a salt kept on your machine (`oml/chat-salt`, mode 0600), so one person's chat on two
machines yields two sessions. On a server whose operators read memories, such as
[the public server](../start/install.md), treat captured facts as readable by them.

If capture does nothing, see [client troubleshooting](../help/troubleshooting-client.md).
