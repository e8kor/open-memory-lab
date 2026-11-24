"""OmlStore's mapping of LangGraph's BaseStore onto the verbs, against a fake
client (ADR-0094 d4-d9); tests/test_the_langchain_store_writes_only_inside_an_in_progress_workspace.py
proves the same against a server."""

import asyncio
import json

import pytest
from langchain_fake import FakeOml

from oml_sdk.langchain import STORE_PREFIX, OmlStore, StoreRefused, claim_key_for

NS = ("memories", "user-1")


def test_the_store_refuses_a_workspace_outside_every_in_progress_subtree():
    with pytest.raises(StoreRefused) as refused:
        OmlStore(FakeOml(in_progress=False))
    assert refused.value.reason == "not_in_progress"


def test_a_put_that_finds_the_workspace_committed_meanwhile_is_refused_and_writes_nothing():
    oml = FakeOml()
    store = OmlStore(oml)
    oml.in_progress = False
    with pytest.raises(StoreRefused) as refused:
        store.put(NS, "a", {"content": "x"})
    assert refused.value.reason == "not_in_progress"
    assert "write" not in oml.verbs()


def test_a_namespace_and_key_map_to_one_percent_encoded_claim_key_under_the_store_prefix():
    assert claim_key_for(("memories", "user/1"), "50%") == "store/memories/user%2F1/50%25"
    assert claim_key_for(NS, "a").startswith(STORE_PREFIX)


def test_a_put_of_a_new_key_writes_its_value_as_a_json_claim_and_a_get_reads_it_back():
    oml = FakeOml()
    store = OmlStore(oml)
    store.put(NS, "a", {"content": "prefers tea"})
    (write,) = [args for name, args in oml.calls if name == "write"]
    assert write["claim_key"] == "store/memories/user-1/a"
    assert json.loads(write["text"]) == {"content": "prefers tea"}
    assert (write["format"], write["title"]) == ("application/json", "memories/user-1/a")
    item = store.get(NS, "a")
    assert (item.namespace, item.key, item.value) == (NS, "a", {"content": "prefers tea"})


def test_a_put_over_the_keys_own_held_version_withdraws_it_and_writes_the_claim_again():
    oml = FakeOml()
    store = OmlStore(oml)
    store.put(NS, "a", {"content": "first"})
    store.put(NS, "a", {"content": "second"})
    assert [r["state"] for r in oml.rows] == ["withdrawn", "held"]
    assert store.get(NS, "a").value == {"content": "second"}


def test_a_put_over_an_approved_tip_supersedes_it_naming_the_latest_version_the_key_read():
    oml = FakeOml()
    tip = oml.seed(
        claim_key_for(NS, "a"), '{"content": "approved"}', state="promoted", author="agent"
    )
    store = OmlStore(oml)
    store.put(NS, "a", {"content": "proposed"})
    store.put(NS, "a", {"content": "proposed again"})
    writes = [args for name, args in oml.calls if name == "write"]
    assert [(w["replaces"], w["expected_version"]) for w in writes] == [
        (tip["artifact_id"], 1),
        (tip["artifact_id"], 2),
    ]
    # The second successor replaced the first, which is withdrawn after it lands.
    assert [r["state"] for r in oml.rows] == ["promoted", "withdrawn", "held"]
    assert store.get(NS, "a").value == {"content": "proposed again"}


def test_a_delete_withdraws_the_keys_own_held_version_and_the_key_then_reads_as_absent():
    oml = FakeOml()
    store = OmlStore(oml)
    store.put(NS, "a", {"content": "mistaken"})
    store.delete(NS, "a")
    assert [r["state"] for r in oml.rows] == ["withdrawn"]
    assert store.get(NS, "a") is None


def test_a_delete_of_another_authors_held_version_is_refused_not_author_and_withdraws_nothing():
    oml = FakeOml()
    oml.seed(claim_key_for(NS, "a"), '{"content": "theirs"}', state="held", author="other")
    with pytest.raises(StoreRefused) as refused:
        OmlStore(oml).delete(NS, "a")
    assert refused.value.reason == "not_author"
    assert "withdraw" not in oml.verbs()


def test_a_delete_of_a_key_with_an_approved_version_is_refused_approved():
    oml = FakeOml()
    oml.seed(claim_key_for(NS, "a"), '{"content": "approved"}', state="promoted", author="agent")
    with pytest.raises(StoreRefused) as refused:
        OmlStore(oml).delete(NS, "a")
    assert refused.value.reason == "approved"


def test_a_delete_of_a_key_nobody_wrote_is_refused_not_found_never_a_silent_no_op():
    with pytest.raises(StoreRefused) as refused:
        OmlStore(FakeOml()).delete(NS, "missing")
    assert refused.value.reason == "not_found"


def test_a_ttl_is_refused_by_name():
    with pytest.raises(NotImplementedError):  # BaseStore.put, as supports_ttl is False
        OmlStore(FakeOml()).put(NS, "a", {"content": "x"}, ttl=5)


def test_a_get_reads_another_authors_held_version_inside_the_workspace():
    oml = FakeOml()
    oml.seed(claim_key_for(NS, "a"), '{"content": "theirs"}', state="held", author="other")
    assert OmlStore(oml).get(NS, "a").value == {"content": "theirs"}


def test_a_version_whose_text_is_not_a_json_object_is_refused_not_json():
    oml = FakeOml()
    oml.seed(claim_key_for(NS, "a"), "plain prose", state="held", author="agent")
    with pytest.raises(StoreRefused) as refused:
        OmlStore(oml).get(NS, "a")
    assert refused.value.reason == "not_json"


def test_a_search_without_a_query_lists_the_newest_version_of_each_key_under_the_prefix():
    oml = FakeOml()
    store = OmlStore(oml)
    store.put(NS, "a", {"content": "tea", "topic": "drinks"})
    store.put(NS, "b", {"content": "chess", "topic": "games"})
    store.put(("memories", "user-2"), "c", {"content": "coffee", "topic": "drinks"})
    store.put(NS, "a", {"content": "green tea", "topic": "drinks"})
    found = store.search(NS)
    assert [(i.key, i.value["content"]) for i in found] == [("a", "green tea"), ("b", "chess")]
    assert [i.key for i in store.search(("memories",), filter={"topic": "drinks"})] == ["a", "c"]
    assert [i.key for i in store.search(("memories",), limit=1, offset=1)] == ["b"]


def test_a_search_with_a_query_recalls_held_rows_too_and_keeps_only_the_prefix():
    oml = FakeOml()
    oml.seed("elsewhere", '{"content": "tea outside the store"}', state="promoted", author="x")
    store = OmlStore(oml)
    store.put(NS, "a", {"content": "tea"})
    found = store.search(NS, query="tea")
    assert [(i.key, i.score) for i in found] == [("a", 1.0)]
    (recall,) = [args for name, args in oml.calls if name == "read"]
    assert (recall["include_unreviewed"], recall["workspace"]) == (True, "ws_progress")


def test_list_namespaces_answers_each_namespace_once_under_its_conditions():
    oml = FakeOml()
    store = OmlStore(oml)
    store.put(NS, "a", {"content": "x"})
    store.put(NS, "b", {"content": "y"})
    store.put(("notes", "user-1"), "c", {"content": "z"})
    assert store.list_namespaces() == [NS, ("notes", "user-1")]
    assert store.list_namespaces(prefix=("memories",)) == [NS]
    assert store.list_namespaces(suffix=("user-1",), max_depth=1) == [("memories",), ("notes",)]


def test_the_async_variants_answer_as_the_sync_ones():
    oml = FakeOml()
    store = OmlStore(oml)

    async def run():
        await store.aput(NS, "a", {"content": "x"})
        got = await store.aget(NS, "a")
        found = await store.asearch(NS)
        await store.adelete(NS, "a")
        return got, found, await store.aget(NS, "a")

    got, found, after = asyncio.run(run())
    assert got.value == {"content": "x"}
    assert [i.key for i in found] == ["a"]
    assert after is None
