"""The agent loop through the SDK, against a real server (docs/engineering/sdks.md, "Tests")."""

import pytest

from oml_sdk import SubmissionError


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
    # Nobody has approved a held memory (ADR-0077 d5).
    assert (own.hits[0].memory.approver, own.hits[0].memory.approved_at) == (None, None)
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
    # The hit says a person approved it, and when; the read by id says the
    # same (ADR-0077 d1, d2). The fixture's reviewer key is held by a human
    # and acts for no user.
    approved = recalled.hits[0].memory
    assert (approved.approver.held_by.value, approved.approver.same_user) == ("human", False)
    assert approved.approved_at <= approved.promoted_at
    fetched = agent.read(held.artifact_id)
    assert (fetched.approver, fetched.approved_at) == (approved.approver, approved.approved_at)
    assert fetched.state == "promoted"
    assert reviewer.review_next() is None


def test_a_held_version_cannot_be_superseded_and_the_promoted_one_can(
    oml_server, agent, reviewer, marker
):
    review = oml_server.review_workarea
    held = agent.write(
        f"The SDK README's canary rule, {marker}.", claim_key=f"tip-{marker}", workarea=review
    ).value
    with pytest.raises(SubmissionError) as refused:
        agent.write("Drain, then wait five minutes.", replaces=held.artifact_id, expected_version=1)
    assert refused.value.code == "submission.tip_conflict"

    assert reviewer.review_next().item.artifact_id == held.artifact_id
    assert reviewer.approve().value.state == "promoted"
    successor = agent.write(
        "Drain, then wait five minutes.", replaces=held.artifact_id, expected_version=1
    ).value
    assert (successor.state, successor.replaces.artifact_id) == ("held", held.artifact_id)

    assert reviewer.review_next().item.artifact_id == successor.artifact_id
    reviewer.reject(reason="the test's successor; nothing more waits")
    assert reviewer.review_next() is None
