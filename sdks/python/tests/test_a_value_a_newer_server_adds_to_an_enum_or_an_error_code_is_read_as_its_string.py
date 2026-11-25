"""An SDK from v0.8.5 on reads a newer server's answers: an enum value the
spec it was generated from does not list, such as a new verb in
`my_effective_verbs` or a new transition state, is kept as its string, and
an error code it has never seen still lands in its family's class (ADR-0096,
upgrade notes for v0.8.5)."""

import json

from oml_sdk.errors import ApiError, error_from_response
from oml_sdk.generated.models.transition import Transition
from oml_sdk.generated.models.workspace_view_my_effective_verbs_item import (
    WorkspaceViewMyEffectiveVerbsItem,
)


def test_an_unknown_verb_is_read_as_its_string():
    verb = WorkspaceViewMyEffectiveVerbsItem("widen_scope")
    assert verb == "widen_scope"
    assert str(verb) == "widen_scope"
    assert verb.value == "widen_scope"
    assert WorkspaceViewMyEffectiveVerbsItem("recall") is WorkspaceViewMyEffectiveVerbsItem.RECALL


def test_a_model_with_an_unknown_enum_value_parses_and_round_trips():
    sent = {
        "transition_id": "t-1",
        "workspace_id": "w-1",
        "from": "protected",
        "to": "archived",
        "state": "withdrawn",
        "proposed_by_key_id": None,
        "ratified_by_key_id": None,
    }
    transition = Transition.from_dict(sent)
    assert transition.to == "archived"
    assert transition.state == "withdrawn"
    assert transition.to_dict()["to"] == "archived"
    assert transition.to_dict()["state"] == "withdrawn"


def test_an_unknown_error_code_lands_in_its_family_as_its_string():
    body = {"error": {"code": "proposal.superseded_later", "message": "new", "retryable": False}}
    error = error_from_response(409, json.dumps(body).encode(), {})
    assert isinstance(error, ApiError)
    assert error.code == "proposal.superseded_later"
