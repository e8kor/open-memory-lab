<!--
id: use/chat-memory
tier: client
status: current
answers: what chat memory captures, what it does not, how to switch it on and off, and where captured facts go
-->

# Chat memory

Chat memory keeps the facts an agent writes from your conversation. It does not store the
conversation: a turn becomes a memory only when the agent in it writes the fact, and every
memory says what kind of source it came from (`origin.kind`)
([ADR-0063](../adr/ADR-0063-a-chat-turn-becomes-memory-only-when-the-agent-in-it-writes-the-fact-and-every-memory-says-what-kind-of-source-it-came-from.md)).

1. **Create the workarea.** `oml chat init` creates `chat` under your own user root and puts it in
   progress, then prints the `OML_CHAT_WORKAREA` value. Only a person's key (held by a human)
   may; an agent key is refused.
2. **Switch capture on.** Set `OML_CHAT_CAPTURE=1` and `OML_CHAT_WORKAREA`. The plugin's `Stop`
   hook counts turns and, every `OML_CHAT_CAPTURE_EVERY` turns (default 5) or after a
   compaction, asks the agent to use the `oml:oml-chat-capture` skill: at most 6 facts, each
   anchored to its turn.
3. **Recall in the next prompt (optional).** `OML_TURN_RECALL=1` sends up to eight distinctive
   words of your prompt, with secret shapes, URLs and the key removed, as a recall and adds hits
   marked `[unreviewed]`. It is off by default and falls silent after three misses.
4. **Review.** Captured facts are held like any memory, in your `chat` workarea. Review them on
   the user page or with `oml review`.
5. **Switch off.** Unset `OML_CHAT_CAPTURE`; retire stored facts with a retirement write.

**Privacy.** The cache keeps a keyed hash of each prompt, never the prompt. Sessions are
pseudonymous: the session id is hashed with a salt kept on your machine (`oml/chat-salt`, mode
0600), so one person's chat on two machines yields two sessions
([#139](https://github.com/e8kor/open-memory-lab/issues/139)). On a server whose operators
read memories, such as the public one, treat captured facts as readable by them.
