from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.transition_from import TransitionFrom
from ..models.transition_state import TransitionState
from ..models.transition_to import TransitionTo

T = TypeVar("T", bound="Transition")


@_attrs_define
class Transition:
    """
    Attributes:
        transition_id (str):
        workarea_id (str):
        from_ (TransitionFrom):
        to (TransitionTo):
        state (TransitionState):
        proposed_by_key_id (None | str): The proposing key, served only to that key; null to every other reader, an
            admin key included (ADR-0025 d15).
        ratified_by_key_id (None | str): Null on this build's answers; the journal's workarea.status_transition names
            the ratifier.
    """

    transition_id: str
    workarea_id: str
    from_: TransitionFrom
    to: TransitionTo
    state: TransitionState
    proposed_by_key_id: None | str
    ratified_by_key_id: None | str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        transition_id = self.transition_id

        workarea_id = self.workarea_id

        from_ = self.from_.value

        to = self.to.value

        state = self.state.value

        proposed_by_key_id: None | str
        proposed_by_key_id = self.proposed_by_key_id

        ratified_by_key_id: None | str
        ratified_by_key_id = self.ratified_by_key_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "transition_id": transition_id,
                "workarea_id": workarea_id,
                "from": from_,
                "to": to,
                "state": state,
                "proposed_by_key_id": proposed_by_key_id,
                "ratified_by_key_id": ratified_by_key_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        transition_id = d.pop("transition_id")

        workarea_id = d.pop("workarea_id")

        from_ = TransitionFrom(d.pop("from"))

        to = TransitionTo(d.pop("to"))

        state = TransitionState(d.pop("state"))

        def _parse_proposed_by_key_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        proposed_by_key_id = _parse_proposed_by_key_id(d.pop("proposed_by_key_id"))

        def _parse_ratified_by_key_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        ratified_by_key_id = _parse_ratified_by_key_id(d.pop("ratified_by_key_id"))

        transition = cls(
            transition_id=transition_id,
            workarea_id=workarea_id,
            from_=from_,
            to=to,
            state=state,
            proposed_by_key_id=proposed_by_key_id,
            ratified_by_key_id=ratified_by_key_id,
        )

        transition.additional_properties = d
        return transition

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
