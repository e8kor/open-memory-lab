"""The generated layer sends a list of kinds as repeated `kinds` parameters,
and the `mine` listing answers every kind the list names (ADR-0092 d6;
ADR-0082 d8 item 3; ADR-0083 d3)."""

import secrets

from oml_sdk.generated.api.memories import list_memories
from oml_sdk.generated.models.claim_kind import ClaimKind


def test_the_mine_listing_answers_every_kind_a_list_of_kinds_names(agent):
    tag = secrets.token_hex(4)
    note = agent.write("A note kept for the kinds list.", claim_key=f"kinds-note-{tag}")
    steer = agent.write(
        "Every note names its source.", claim_key=f"kinds-steer-{tag}", kind="steering"
    )

    response = list_memories.sync_detailed(
        client=agent._api,
        mine=True,
        kinds=[ClaimKind.MEMORY, ClaimKind.STEERING],
        limit=200,
    )

    assert response.status_code == 200, response.content
    ids = {row.artifact_id for row in response.parsed.items}
    assert {note.value.artifact_id, steer.value.artifact_id} <= ids
    assert {row.kind for row in response.parsed.items} <= {"memory", "steering"}
