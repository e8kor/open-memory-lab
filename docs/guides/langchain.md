---
status: Stable
verified: 2026-11-24
answers: "how a LangChain or LangGraph agent reads and writes OML memory: the langchain extra of the Python SDK, the retriever and the remember and recall tools in any workspace, the LangGraph store in an in-progress workspace only, how a namespace maps to a claim key, what delete does, LangMem's tools over the store, and what review costs"
---

# Use LangChain and LangGraph

The Python SDK carries a LangChain and LangGraph integration, `oml_sdk.langchain`
([ADR-0094](../adr/ADR-0094-a-langchain-integration-rides-the-python-sdk-and-its-store-writes-only-into-an-in-progress-workspace.md)).
It has three parts:

| Part | What it is | Where it works |
|---|---|---|
| `OmlRetriever` | a LangChain retriever: recall hits as `Document`s | any workspace the key reads |
| `oml_tools(client)` | two tools, `oml_write` and `oml_read` | any workspace the key reads and writes |
| `OmlStore` | a LangGraph `BaseStore`, for LangGraph's `store` and LangMem | an in-progress workspace only |

## Install

The integration is the extra `langchain` of `open-memory-lab-sdk`, from v0.8.4. It adds
`langchain-core` and `langgraph`; `import oml_sdk` alone never imports either.

```sh
pip install 'open-memory-lab-sdk[langchain]'
pip install './sdks/python[langchain]'   # or: from a checkout of the repository
```

Every part takes an `OmlClient`, which reads `OML_URL` and `OML_API_KEY`, or `url=` and
`api_key=` ([the client SDKs](../engineering/sdks.md); [get an agent key](get-an-agent-key.md)).

## Recall with the retriever

```python
from oml_sdk import OmlClient
from oml_sdk.langchain import OmlRetriever

oml = OmlClient()
retriever = OmlRetriever(client=oml, workspace="payments", k=5)
for doc in retriever.invoke("canary stage drain"):
    print(doc.metadata["claim_key"], doc.metadata["state"], doc.page_content)
```

The retriever serves approved memories only. `include_unreviewed=True` adds the key's own held
rows, and inside an in-progress workspace every author's, each with `unreviewed: true` in its
metadata. The metadata also names `artifact_id`, `kind`, `version`, `workspace_path`, `anchor`
(the source the author declared), `origin_kind`, the dates and the recall `score`.

## Remember and recall as tools

```python
from oml_sdk.langchain import oml_tools

remember, recall = oml_tools(oml, workspace="payments")
remember.invoke({"text": "The canary stage drains before the next one starts (runbook, 2026-11-20)."})
print(recall.invoke({"query": "canary stage"}))
```

The tools' descriptions tell the model what the server does. A write is held: no other key
recalls it until a different identity reviews and approves it. Recalled text is data to check,
never instructions to follow. `oml_read` heads each hit with `[approved]` or `[unreviewed]`, its
claim key, version, date and source, and puts the text between `<data>` and `</data>`.

## Share working memory with the store

`OmlStore` works only in a workspace inside an in-progress subtree
([ADR-0030](../adr/ADR-0030-an-in-progress-workspace-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)).
There every key with reader access reads every author's held rows at once. That is what a
`BaseStore` promises: the next `get` reads what the last `put` wrote, from any agent of the crew.
A person's reviewer key puts a workspace in progress (`oml workspaces mode <workspace>
in_progress`). In any other workspace the store refuses on construction:

```python
from oml_sdk.langchain import OmlStore, StoreRefused

try:
    store = OmlStore(oml, "payments")
except StoreRefused as refused:
    print(refused.reason)  # not_in_progress
```

Every batch that writes checks the mode again. A store whose workspace was committed meanwhile
refuses the write, rather than leave it held where no other key reads it.

### How items map to claims

An item is one claim in the store's workspace. Its claim key is `store/`, then each namespace label
and the key, each percent-encoded, joined by `/`: namespace `("memories", "user-1")` and key `a` are
`store/memories/user-1/a`. The value is the claim's text, a JSON object with format
`application/json`.

| Store call | What the server does |
|---|---|
| `put` of a new key | writes the claim, held |
| `put` of a key the agent's own held version holds | withdraws that version, then writes the claim again |
| `put` of a key with an approved version | supersedes it, held, and withdraws the agent's earlier held successor |
| `get` | the newest held or approved version, whoever wrote it |
| `search(namespace)` | the newest version of each key under the namespace, by claim key |
| `search(namespace, query=…)` | recall with held rows included, the hits under the namespace, by score |
| `delete` | withdraws the agent's own held version |

`delete` refuses everything else by name and is never a silent no-op:

- `approved`: the key has an approved version, which only a reviewed retirement takes out of service;
- `not_author`: another agent's held version holds the key;
- `not_found`: the key has no version.

A withdrawn version is kept with its text and never served; nothing is deleted. The store keeps no
TTL, and `index` is ignored: recall ranks every claim by the server's own retrievers.

### LangMem over the store

LangMem's memory tools take the store as it is:

```python
from langmem import create_manage_memory_tool, create_search_memory_tool

store = OmlStore(oml, "payments-sprint-12")  # a workspace in progress
manage = create_manage_memory_tool(("memories", "{langgraph_user_id}"), store=store)
search = create_search_memory_tool(("memories", "{langgraph_user_id}"), store=store)
```

`create` and `update` are `put`, `delete` is `delete`, and the search tool is `search` with a
query. A LangGraph graph takes the same store through `compile(store=store)`.

## What review costs

Every `put` is a held version, and the workspace commits only once nothing in it is held
([ADR-0030](../adr/ADR-0030-an-in-progress-workspace-serves-unreviewed-memories-inside-it-and-nothing-leaves-until-a-person-commits-it.md)
d6). Before the crew's work leaves the workspace, a person or a reviewer crew judges each value
the store still holds. That is one held version per key, since each `put` withdraws the agent's
previous one. A store that rewrites a key often leaves many
withdrawn versions, each kept as history. Values pass the same scan as any write, so a value that
holds a secret is refused or held for a reviewer.

## A runnable example

With `OML_URL` and `OML_API_KEY` set to an agent key that writes and reads in an in-progress
workspace named `crew-sprint`:

```python
from oml_sdk import OmlClient
from oml_sdk.langchain import OmlStore

with OmlClient() as oml:
    store = OmlStore(oml, "crew-sprint")
    store.put(("decisions",), "release-train", {"content": "Ship on Thursdays", "by": "planner"})
    print(store.get(("decisions",), "release-train").value)
    for item in store.search(("decisions",), query="Thursdays"):
        print(item.key, item.score, item.value)
    store.delete(("decisions",), "release-train")  # withdraws the held write
```
