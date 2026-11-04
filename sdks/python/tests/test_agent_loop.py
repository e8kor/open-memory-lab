"""The agent loop through the SDK, against a real server (docs/engineering/sdks.md, "Tests")."""


def test_a_written_memory_is_held_recalled_by_its_author_alone_and_promoted_by_a_reviewer(
    oml_server, agent, reviewer, marker
):
    review = oml_server.review_workarea
    written = agent.write(
        f"The SDK agent loop wrote this memory, {marker}.",
        claim_key=f"loop-{marker}",
        workarea=review,
    )
    held = written.value
    assert held.state == "held"
    assert written.replayed is False

    own = agent.read(query=marker, include_unreviewed=True, workarea=review)
    assert [(h.memory.artifact_id, h.memory.state) for h in own.hits] == [
        (held.artifact_id, "held")
    ]
    assert agent.read(query=marker, workarea=review).hits == []

    serve = reviewer.review_next()
    assert serve is not None
    assert serve.item.artifact_id == held.artifact_id
    verdict = reviewer.approve()  # the one item this key claimed (ADR-0039 d10)
    assert verdict.value.state == "promoted"

    recalled = agent.read(query=marker, workarea=review)
    assert [(h.memory.artifact_id, h.memory.state) for h in recalled.hits] == [
        (held.artifact_id, "promoted")
    ]
    assert agent.read(held.artifact_id).state == "promoted"
    assert reviewer.review_next() is None
