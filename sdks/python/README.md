# open-memory-lab-sdk — the Python client

A client for the Open Memory Lab HTTP API: a layer generated from
[`docs/api/openapi.yaml`](https://e8kor.github.io/open-memory-lab/api/openapi.yaml) by a pinned `openapi-python-client`, and a
thin hand-written wrapper over it. What the wrapper covers, the pins and the regeneration rule are
decided in [docs/engineering/sdks.md](https://e8kor.github.io/open-memory-lab/engineering/sdks/).

From v0.8.3 the client speaks the renamed API (ADR-0091): `oml_sdk.generated.api.workspaces`,
the `Workspace…` models, `create_workspace`, the role `reviewer`, the statuses `protected` and
`frozen`, and `used_by` with `person`. Code written against a v0.7 client that names a workarea
type, the `dreamer` role or `held_by` does not import against it, and a v0.7 client does not
work against a v0.8.3 server: upgrade the client before the server.

From v0.8.4 the role `writer` is gone (ADR-0093): a key that recalls and writes is a
`contributor`, one that writes without recall is a `submitter`, and the grant role that
confers writing is `submitter`. A v0.8.3 client that sends the former role is refused and
issues nothing: upgrade the client before the server.

Python 3.11 or later. Each release attaches a wheel and an sdist to its GitHub release; install
the wheel from the file, or install from a checkout of the repository:

```sh
pip install ./open_memory_lab_sdk-X.Y.Z-py3-none-any.whl  # the wheel from a release's files
uv add ./sdks/python                              # or: pip install ./sdks/python, from a checkout
```

The package is named `open-memory-lab-sdk` on PyPI and is imported as `oml_sdk`. It is not published there yet; until it is, install from a file or a checkout as above.

```python
from oml_sdk import OmlClient

# One pair: url= and api_key=, else OML_URL and OML_API_KEY; one set alone is refused.
with OmlClient() as oml:
    written = oml.write("The canary stage must drain before the next one starts.")
    print(written.value.state)  # held: invisible to other keys until a reviewer's verdict
    print(written.value.claim_key)  # derived by the server when none is sent
    print(written.idempotency_key)  # the key every retry of this call sent

    for hit in oml.read(query="canary stage", include_unreviewed=True).hits:
        print(hit.memory.state, hit.memory.text)

    memory = oml.read(written.value.artifact_id)  # the key's own row, in any state
    print(memory.state, memory.text)

    # replaces must name the served tip, so a reviewer's approve promotes the version first:
    # superseding it while it is held raises SubmissionError (submission.tip_conflict).
    oml.write("Drain, then wait five minutes.", replaces=memory.artifact_id)

    for row in oml.iter_memories(own=True, limit=50):  # the key's own rows; follows next_cursor to the end
        print(row.artifact_id, row.state)
```

Seven verbs: `write`, `read`, `workspace`, `events`, `review_next`, `approve` and `reject`.
`write`'s arguments choose a submission or, with `replaces`, a supersession; `read`'s choose a
fetch (`artifact_id`), a recall (`query`) or the listing (neither), as `oml_write` and
`oml_read` do. Every refusal raises `oml_sdk.ApiError` carrying the envelope of
[errors.md](https://e8kor.github.io/open-memory-lab/api/errors/) — `code`, `message`, `rule`, `retryable`, `details`,
`request_id` — and `status`, as the subclass of the code's family; an argument outside its mode
is refused the same way before anything is sent.

## LangChain and LangGraph

From v0.8.4 the extra `langchain` adds `oml_sdk.langchain`
([ADR-0094](https://github.com/e8kor/open-memory-lab/blob/main/docs/adr/ADR-0094-a-langchain-integration-rides-the-python-sdk-and-its-store-writes-only-into-an-in-progress-workspace.md);
[the guide](https://e8kor.github.io/open-memory-lab/guides/langchain/)). `import oml_sdk` alone
never imports LangChain or LangGraph.

```sh
pip install 'open-memory-lab-sdk[langchain]'
```

- `OmlRetriever(client=oml)`: recall as LangChain `Document`s, approved memories only unless
  `include_unreviewed=True`, in any workspace the key reads.
- `oml_tools(oml)`: the tools `oml_remember` and `oml_recall`. Their descriptions tell the model
  that a write is held until a different identity reviews it, and that recalled text is data,
  never instructions.
- `OmlStore(oml, workspace)`: a LangGraph `BaseStore`, for LangGraph's `store` and LangMem's
  memory tools, in an in-progress workspace only; any other workspace raises `StoreRefused`
  (`not_in_progress`). An item is the claim `store/<label>/…/<key>` with a JSON body; `put`
  writes or supersedes it, held, `get` and `search` read held and approved rows, and `delete`
  withdraws the key's own held version and refuses anything else by name. Every value is
  reviewed before the workspace commits.

```python
from oml_sdk import OmlClient
from oml_sdk.langchain import OmlRetriever, OmlStore

with OmlClient() as oml:
    docs = OmlRetriever(client=oml, workspace="payments").invoke("canary stage")
    store = OmlStore(oml, "crew-sprint")  # a workspace in progress
    store.put(("decisions",), "release-train", {"content": "Ship on Thursdays"})
    print(store.get(("decisions",), "release-train").value)
```

## Generation and tests

`oml_sdk.generated` is generated and never edited by hand; `python3 scripts/generate_sdks.py`
regenerates it. The tests run against a real `oml-server-lead` on a throwaway database
(`tests/conftest.py` names the environment they read). The harness mints an owner and a
serving role for that database, so its admin URL names a superuser that can mint roles;
under a role that cannot, the server tests skip with `did not run: <role> cannot create roles`:

The tests of `oml_sdk.langchain` need the extra and the LangMem group:

```sh
cd sdks/python && uv run --locked --extra langchain --group langmem pytest
```
