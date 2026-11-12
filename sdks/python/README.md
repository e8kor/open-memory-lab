# oml-sdk — the Python client

A client for the Open Memory Lab HTTP API: a layer generated from
[`docs/api/openapi.yaml`](https://e8kor.github.io/open-memory-lab/api/openapi.yaml) by a pinned `openapi-python-client`, and a
thin hand-written wrapper over it. What the wrapper covers, the pins and the regeneration rule are
decided in [docs/engineering/sdks.md](https://e8kor.github.io/open-memory-lab/engineering/sdks/).

Install it from PyPI, Python 3.11 or later:

```sh
pip install oml-sdk             # or: uv add oml-sdk
```

From a checkout of the repository, which is how to install it until the first release is on
PyPI and how to try a change that is not released:

```sh
uv add ./sdks/python            # or: pip install ./sdks/python
```

```python
from oml_sdk import OmlClient

# One pair: url= and api_key=, else OML_URL and OML_API_KEY; one set alone is refused.
with OmlClient() as oml:
    written = oml.write("The canary stage must drain before the next one starts.")
    print(written.value.state)          # held: invisible to other keys until a reviewer's verdict
    print(written.value.claim_key)      # derived by the server when none is sent
    print(written.idempotency_key)      # the key every retry of this call sent

    for hit in oml.read(query="canary stage", include_unreviewed=True).hits:
        print(hit.memory.state, hit.memory.text)

    memory = oml.read(written.value.artifact_id)   # the key's own row, in any state
    print(memory.state, memory.text)

    # replaces must name the served tip, so a reviewer's approve promotes the version first:
    # superseding it while it is held raises SubmissionError (submission.tip_conflict).
    oml.write("Drain, then wait five minutes.", replaces=memory.artifact_id)

    for row in oml.iter_memories(limit=50):   # the key's own rows; follows next_cursor to the end
        print(row.artifact_id, row.state)
```

Seven verbs: `write`, `read`, `workarea`, `events`, `review_next`, `approve` and `reject`.
`write`'s arguments choose a submission or, with `replaces`, a supersession; `read`'s choose a
fetch (`artifact_id`), a recall (`query`) or the listing (neither), as `oml_write` and
`oml_read` do. Every refusal raises `oml_sdk.ApiError` carrying the envelope of
[errors.md](https://e8kor.github.io/open-memory-lab/api/errors/) — `code`, `message`, `rule`, `retryable`, `details`,
`request_id` — and `status`, as the subclass of the code's family; an argument outside its mode
is refused the same way before anything is sent.

`oml_sdk.generated` is generated and never edited by hand; `python3 scripts/generate_sdks.py`
regenerates it. The tests run against a real `oml-server-lead` on a throwaway database
(`tests/conftest.py` names the environment they read). The harness mints an owner and a
serving role for that database, so its admin URL names a superuser that can mint roles;
under a role that cannot, the server tests skip with `did not run: <role> cannot create roles`:

```sh
cd sdks/python && uv run --locked pytest
```
