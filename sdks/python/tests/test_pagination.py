"""Cursor iteration over the listing (docs/engineering/sdks.md, "What the wrapper covers")."""

import secrets


def test_iteration_follows_next_cursor_across_pages_until_it_is_null(agent):
    tag = secrets.token_hex(4)
    written = [
        agent.remember(f"page-{tag}-{i}", f"Pagination row {i} of five.").value.artifact_id
        for i in range(5)
    ]

    pages = list(agent.pages(own=True, limit=2))
    assert len(pages) >= 3
    assert all(page.next_cursor for page in pages[:-1])
    assert pages[-1].next_cursor is None
    ids = [row.artifact_id for page in pages for row in page.items]
    assert len(ids) == len(set(ids))
    assert set(written) <= set(ids)
    positions = [ids.index(artifact_id) for artifact_id in written]
    assert positions == sorted(positions, reverse=True)  # newest first

    assert [row.artifact_id for row in agent.iter_memories(own=True, limit=2)] == ids


def test_one_page_carries_the_cursor_to_the_next(agent):
    tag = secrets.token_hex(4)
    for i in range(3):
        agent.remember(f"cursor-{tag}-{i}", f"Cursor row {i}.")
    first = agent.list_memories(own=True, limit=2)
    assert len(first.items) == 2
    assert first.next_cursor
    second = agent.list_memories(own=True, limit=2, cursor=first.next_cursor)
    assert {row.artifact_id for row in first.items}.isdisjoint(
        row.artifact_id for row in second.items
    )
