"""LangMem's manage and search tools over OmlStore, against a real server
(ADR-0094 d10): create, update, search and delete, each a verb of the store."""

import json

from langmem import create_manage_memory_tool, create_search_memory_tool

from oml_sdk.langchain import OmlStore


def test_langmems_memory_tools_create_update_search_and_delete_through_the_store(
    oml_server, agent, marker
):
    store = OmlStore(agent, oml_server.progress_workspace)
    namespace = ("memories", marker)
    manage = create_manage_memory_tool(namespace, store=store)
    search = create_search_memory_tool(namespace, store=store)

    created = manage.invoke({"content": f"prefers green tea {marker}", "action": "create"})
    memory_id = created.rsplit(" ", 1)[1]
    found = json.loads(search.invoke({"query": marker}))
    assert [(m["key"], m["value"]["content"]) for m in found] == [
        (memory_id, f"prefers green tea {marker}")
    ]

    manage.invoke({"content": f"prefers black tea {marker}", "action": "update", "id": memory_id})
    assert store.get(namespace, memory_id).value == {"content": f"prefers black tea {marker}"}

    manage.invoke({"action": "delete", "id": memory_id})
    assert store.get(namespace, memory_id) is None
    assert json.loads(search.invoke({"query": marker})) == []
