# oml-sdk — the Python client

A client for the Open Memory Lab HTTP API: a layer generated from
[`docs/api/openapi.yaml`](../../docs/api/openapi.yaml) by a pinned `openapi-python-client`, and a
thin hand-written wrapper over it. What the wrapper covers, the pins and the regeneration rule are
decided in [docs/engineering/sdks.md](../../docs/engineering/sdks.md).

It is not released to PyPI. Install it from a checkout, Python 3.11 or later:

```sh
uv add ./sdks/python            # or: pip install ./sdks/python
```

```python
from oml_sdk import OmlClient

# The key comes from api_key=, else OML_API_KEY; the server from base_url=, else OML_URL.
with OmlClient() as oml:
    written = oml.remember("deploy-runbook", "The canary stage must drain before the next one starts.")
    print(written.value.state)          # held: invisible to other keys until a reviewer's verdict
    print(written.idempotency_key)      # the key every retry of this call sent

    for hit in oml.recall("canary stage", include_own_held=True).hits:
        print(hit.memory.state, hit.memory.body)

    for row in oml.iter_memories(own=True, limit=50):   # follows next_cursor to the end
        print(row.artifact_id, row.state)
```

Refusals raise one exception class per family of [errors.md](../../docs/api/errors.md), all
subclasses of `oml_sdk.ApiError`, each carrying `code`, `rule`, `message`, `retryable`,
`details`, `request_id` and `status`. File uploads raise `NotBuiltError`: the server serves no
upload sessions yet.

`oml_sdk.generated` is generated and never edited by hand; `python3 scripts/generate_sdks.py`
regenerates it. The tests run against a real `oml-server-lead` on a throwaway database
(`tests/conftest.py` names the environment they read):

```sh
cd sdks/python && uv run --locked pytest
```
