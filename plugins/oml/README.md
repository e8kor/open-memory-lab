# The `oml` agent plugin

This plugin connects Claude Code to an Open Memory Lab server over MCP. It adds four
tools, `oml_remember`, `oml_learn`, `oml_recall` and `oml_events_tail`, plus a skill that
explains when to use them. Every write is held until a different identity approves it.
That is the point of the server, and it means a fact you store is not shared the moment
you store it.

## Install

```sh
export OML_URL=http://127.0.0.1:8080        # the server, without /mcp
export OML_API_KEY=oml_...                  # this agent's writer key
export OML_RECALL_KEY=oml_...               # a reader key on the same workarea
claude plugin marketplace add e8kor/open-memory-lab
claude plugin install oml@open-memory-lab
```

The plugin registers two MCP connections to the same endpoint: `oml` with the writer key
for remembering and `oml-recall` with the reader key for recall. This release gives each
key a single role, so one key cannot do both. When a key can carry writer and reader
grants together, the second connection goes away. If `OML_RECALL_KEY` is unset, only
`oml-recall` fails to connect.

Give each agent its own key. Attribution, and the rule that a reviewer never reviews
their own work, both depend on it. Keep the key in the environment, not in a file you
commit. To issue keys and run a server, see "Quickstart" in the
[repository README](../../README.md#quickstart).

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
This release serves the four above. The other eight are not served yet: `oml_attach`,
`oml_relate_propose`, `oml_my_work`, `oml_workarea_info`, `oml_graph` and `oml_vocab` answer
with an error saying they are not built yet, and `oml_get` and `oml_supersede`, added to the
catalog by ADR-0025, are not yet named by the server. They arrive with the milestones in the
[critical path](../../docs/product/critical-path.md).
