# The `oml` agent plugin

This plugin connects Claude Code to an Open Memory Lab server over MCP. It adds eight
tools, `oml_remember`, `oml_learn`, `oml_recall`, `oml_get`, `oml_supersede`,
`oml_my_work`, `oml_workarea_info` and `oml_events_tail`, plus a skill that explains when
to use them. Every write is held until a different identity approves it.
That is the point of the server, and it means a fact you store is not shared the moment
you store it.

## Install

```sh
export OML_URL=http://127.0.0.1:8080        # the server, without /mcp
export OML_API_KEY=oml_...                  # this agent's one key
claude plugin marketplace add e8kor/open-memory-lab
claude plugin install oml@open-memory-lab
```

The plugin registers one MCP connection, `oml`, and every tool runs on the one key behind
it. Issue that key with the writer role, a writer grant and a reader grant on the agent's
workarea, and that workarea as its default. A key without the reader grant still writes,
but `oml_recall` refuses it with `access.not_found`.

## The loop

On that one key the agent:

1. checks what it may do in its workarea with `oml_workarea_info`;
2. recalls before it acts with `oml_recall`, reading the `steering` rows first;
3. writes with `oml_remember`, or `oml_learn` for a conclusion drawn from memories it
   holds, and gets `state: "held"` back;
4. sees its held writes in `oml_my_work`, reads one back with `oml_get`, and searches them
   beside the approved memories by passing `include_own_held: true` to `oml_recall`; no
   other key reads or recalls them before approval;
5. waits for a different identity to approve them, following `oml_events_tail`, after
   which every key that reads the workarea recalls them;
6. revises or retires a memory with `oml_supersede`, which is held and approved like any
   write while recall keeps serving the version it replaces.

`oml_recall` takes `max_tokens` to bound its answer: the steering rows are never cut, the
hits are the longest run from the top of the ranking that fits, and `budget` reports the cut.

Give each agent its own key. Attribution, and the rule that a reviewer never reviews
their own work, both depend on it. Keep the key in the environment, not in a file you
commit. To issue keys and run a server, see "Quickstart" in the
[repository README](../../README.md#quickstart).

`oml keys issue-agent <key-name> --principal <principal> --workarea <workarea-id>` issues an
agent's key in one call: a writer key named `<key-name>`, with a writer and a reader grant on the
workarea, which is also its default. It prints the `export OML_URL=…` and `export OML_API_KEY=…`
lines to run before you start Claude Code. With `--out <path>` the value goes to a new owner-only
file and the second line reads it from there; the `oml-admin` plugin in this marketplace runs it
that way from inside Claude Code, so the value never enters the conversation. See the
[CLI reference](../../docs/cli/reference.md#oml-keys-issuance-revocation-rotation).

## Other MCP clients

The server speaks stateless streamable HTTP at `/mcp`. Any client that accepts this shape
works:

```json
{
  "mcpServers": {
    "oml": {
      "type": "http",
      "url": "http://127.0.0.1:8080/mcp",
      "headers": { "Authorization": "Bearer ${OML_API_KEY}" }
    }
  }
}
```

## What is served today

The catalog in [docs/api/mcp-tools.json](../../docs/api/mcp-tools.json) declares twelve tools.
This release serves the eight above. The other four are not served yet: `oml_attach`,
`oml_relate_propose`, `oml_graph` and `oml_vocab` answer with an error saying they are not
built yet. They arrive with the milestones in the
[critical path](../../docs/product/critical-path.md).
