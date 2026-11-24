"""oml_sdk.langchain against a real server (ADR-0094; conftest.py's
`sdk-progress` is in progress, `sdk` and `sdk-review` are standard)."""

import secrets

import httpx
import pytest

from oml_sdk.langchain import OmlRetriever, OmlStore, StoreRefused


def test_a_put_is_read_back_at_once_by_the_writer_and_by_another_agent_in_the_workspace(
    oml_server, agent, other_agent, marker
):
    namespace = ("memories", marker)
    OmlStore(agent, oml_server.progress_workspace).put(namespace, "a", {"content": "tea"})
    for client in (agent, other_agent):
        item = OmlStore(client, oml_server.progress_workspace).get(namespace, "a")
        assert (item.namespace, item.key, item.value) == (namespace, "a", {"content": "tea"})


def test_a_second_put_of_a_held_key_replaces_it_and_search_finds_the_newest_value(
    oml_server, agent, marker
):
    store = OmlStore(agent, oml_server.progress_workspace)
    namespace = ("memories", marker)
    store.put(namespace, "a", {"content": f"tea {marker}"})
    store.put(namespace, "a", {"content": f"green tea {marker}"})
    assert store.get(namespace, "a").value == {"content": f"green tea {marker}"}
    assert [i.value["content"] for i in store.search(namespace)] == [f"green tea {marker}"]
    found = store.search(("memories",), query=marker)
    assert [(i.namespace, i.key) for i in found] == [(namespace, "a")]
    assert found[0].score is not None
    states = sorted(str(r.state) for r in agent.iter_memories(own=True, prefix="store/"))
    assert states.count("withdrawn") >= 1


def test_the_store_refuses_a_standard_workspace_by_name(oml_server, agent):
    with pytest.raises(StoreRefused) as refused:
        OmlStore(agent, oml_server.workspace)
    assert refused.value.reason == "not_in_progress"


def test_a_delete_withdraws_the_keys_own_held_write(oml_server, agent, marker):
    store = OmlStore(agent, oml_server.progress_workspace)
    namespace = ("memories", marker)
    store.put(namespace, "a", {"content": "mistaken"})
    store.delete(namespace, "a")
    assert store.get(namespace, "a") is None
    (row,) = [r for r in agent.iter_memories(own=True) if marker in r.claim_key]
    assert str(row.state) == "withdrawn"


def test_a_delete_of_another_keys_write_is_refused_not_author(
    oml_server, agent, other_agent, marker
):
    namespace = ("memories", marker)
    OmlStore(other_agent, oml_server.progress_workspace).put(namespace, "a", {"content": "theirs"})
    with pytest.raises(StoreRefused) as refused:
        OmlStore(agent, oml_server.progress_workspace).delete(namespace, "a")
    assert refused.value.reason == "not_author"
    item = OmlStore(other_agent, oml_server.progress_workspace).get(namespace, "a")
    assert item.value == {"content": "theirs"}


def test_the_retriever_returns_approved_documents_only_by_default(
    oml_server, agent, reviewer, marker
):
    review = oml_server.review_workspace
    approved = agent.write(f"The canary drains first, {marker}.", workspace=review).value
    assert reviewer.review_next().item.artifact_id == approved.artifact_id
    reviewer.approve()
    held = agent.write(f"The canary waits five minutes, {marker}.", workspace=review).value

    docs = OmlRetriever(client=agent, workspace=review).invoke(marker)
    assert [d.metadata["artifact_id"] for d in docs] == [approved.artifact_id]
    assert docs[0].metadata["state"] == "promoted"
    wider = OmlRetriever(client=agent, workspace=review, include_unreviewed=True).invoke(marker)
    assert {d.metadata["artifact_id"] for d in wider} == {approved.artifact_id, held.artifact_id}

    # Nothing is left in the review queue the other tests read.
    assert reviewer.review_next().item.artifact_id == held.artifact_id
    reviewer.reject(reason="the retriever test's held row; nothing more waits")
    assert reviewer.review_next() is None


def _approve(oml_server, artifact_id: str) -> None:
    """The in-progress workspace's person reviewer approves one named held row (ADR-0055)."""
    answer = httpx.post(
        f"{oml_server.url}/v2/review/verdicts/bulk",
        json={"verdict": "approve", "artifact_ids": [artifact_id]},
        headers={
            "Authorization": f"Bearer {oml_server.progress_reviewer_key}",
            "Idempotency-Key": secrets.token_hex(16),
        },
        timeout=10,
    )
    assert answer.status_code == 200, answer.text
    assert answer.json()["items"][0]["state"] == "promoted", answer.text


def test_a_put_over_an_approved_value_supersedes_it_and_a_delete_of_it_is_refused_approved(
    oml_server, agent, marker
):
    store = OmlStore(agent, oml_server.progress_workspace)
    namespace = ("memories", marker)
    store.put(namespace, "a", {"content": "approved"})
    (first,) = [r for r in agent.iter_memories(own=True) if marker in r.claim_key]
    _approve(oml_server, first.artifact_id)

    store.put(namespace, "a", {"content": "proposed"})
    store.put(namespace, "a", {"content": "proposed again"})
    assert store.get(namespace, "a").value == {"content": "proposed again"}
    rows = sorted(
        (r.version, str(r.state)) for r in agent.iter_memories(own=True) if marker in r.claim_key
    )
    assert rows == [(1, "promoted"), (2, "withdrawn"), (3, "held")]

    with pytest.raises(StoreRefused) as refused:
        store.delete(namespace, "a")
    assert refused.value.reason == "approved"


def _own_row(client, marker: str, version: int):
    (row,) = [
        r for r in client.iter_memories(own=True) if marker in r.claim_key and r.version == version
    ]
    return row


def test_a_key_whose_approved_tip_is_a_retirement_reads_as_absent_and_a_put_returns_it_to_service(
    oml_server, agent, marker
):
    store = OmlStore(agent, oml_server.progress_workspace)
    namespace = ("memories", marker)
    store.put(namespace, "a", {"content": "approved"})
    _approve(oml_server, _own_row(agent, marker, 1).artifact_id)
    retirement = agent.write(
        "no longer true",
        replaces=_own_row(agent, marker, 1).artifact_id,
        retirement=True,
        expected_version=1,
        workspace=oml_server.progress_workspace,
    ).value
    _approve(oml_server, retirement.artifact_id)

    assert store.get(namespace, "a") is None
    assert store.search(namespace) == []
    assert all(i.namespace != namespace for i in store.search(("memories",), limit=1000))
    assert namespace not in store.list_namespaces(prefix=("memories",), limit=1000)
    with pytest.raises(StoreRefused) as refused:
        store.delete(namespace, "a")
    assert refused.value.reason == "not_found"

    store.put(namespace, "a", {"content": "back"})
    assert store.get(namespace, "a").value == {"content": "back"}


def test_a_search_with_a_query_serves_the_newest_value_never_a_superseded_one(
    oml_server, agent, marker
):
    store = OmlStore(agent, oml_server.progress_workspace)
    namespace = ("memories", marker)
    store.put(namespace, "a", {"content": f"oolongzz {marker}"})
    _approve(oml_server, _own_row(agent, marker, 1).artifact_id)
    store.put(namespace, "a", {"content": f"espressozz {marker}"})

    assert store.search(namespace, query="oolongzz") == []
    found = store.search(namespace, query="espressozz")
    assert [i.value for i in found] == [{"content": f"espressozz {marker}"}]
    assert found[0].score is not None


def test_a_put_over_another_keys_held_write_is_refused_not_author(
    oml_server, agent, other_agent, marker
):
    namespace = ("memories", marker)
    OmlStore(other_agent, oml_server.progress_workspace).put(namespace, "a", {"content": "theirs"})
    with pytest.raises(StoreRefused) as refused:
        OmlStore(agent, oml_server.progress_workspace).put(namespace, "a", {"content": "mine"})
    assert refused.value.reason == "not_author"
    item = OmlStore(agent, oml_server.progress_workspace).get(namespace, "a")
    assert item.value == {"content": "theirs"}


def test_a_put_over_another_keys_approved_value_is_refused_not_author(
    oml_server, agent, other_agent, marker
):
    namespace = ("memories", marker)
    OmlStore(other_agent, oml_server.progress_workspace).put(namespace, "a", {"content": "v1"})
    _approve(oml_server, _own_row(other_agent, marker, 1).artifact_id)
    with pytest.raises(StoreRefused) as refused:
        OmlStore(agent, oml_server.progress_workspace).put(namespace, "a", {"content": "mine"})
    assert refused.value.reason == "not_author"
