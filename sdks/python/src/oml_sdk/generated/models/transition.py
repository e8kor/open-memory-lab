from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.transition_state import TransitionState
from ..models.transition_to import TransitionTo
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.review_policy import ReviewPolicy


T = TypeVar("T", bound="Transition")


@_attrs_define
class Transition:
    """
    Attributes:
        transition_id (str):
        workarea_id (str):
        to (TransitionTo):
        state (TransitionState):
        proposed_by_key_id (str | Unset):
        ratified_by_key_id (None | str | Unset):
        policy_snapshot (ReviewPolicy | Unset): The per-workarea review policy object (journalled record; admission page
            owns it).
    """

    transition_id: str
    workarea_id: str
    to: TransitionTo
    state: TransitionState
    proposed_by_key_id: str | Unset = UNSET
    ratified_by_key_id: None | str | Unset = UNSET
    policy_snapshot: ReviewPolicy | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        transition_id = self.transition_id

        workarea_id = self.workarea_id

        to = self.to.value

        state = self.state.value

        proposed_by_key_id = self.proposed_by_key_id

        ratified_by_key_id: None | str | Unset
        if isinstance(self.ratified_by_key_id, Unset):
            ratified_by_key_id = UNSET
        else:
            ratified_by_key_id = self.ratified_by_key_id

        policy_snapshot: dict[str, Any] | Unset = UNSET
        if not isinstance(self.policy_snapshot, Unset):
            policy_snapshot = self.policy_snapshot.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "transition_id": transition_id,
                "workarea_id": workarea_id,
                "to": to,
                "state": state,
            }
        )
        if proposed_by_key_id is not UNSET:
            field_dict["proposed_by_key_id"] = proposed_by_key_id
        if ratified_by_key_id is not UNSET:
            field_dict["ratified_by_key_id"] = ratified_by_key_id
        if policy_snapshot is not UNSET:
            field_dict["policy_snapshot"] = policy_snapshot

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.review_policy import ReviewPolicy

        d = dict(src_dict)
        transition_id = d.pop("transition_id")

        workarea_id = d.pop("workarea_id")

        to = TransitionTo(d.pop("to"))

        state = TransitionState(d.pop("state"))

        proposed_by_key_id = d.pop("proposed_by_key_id", UNSET)

        def _parse_ratified_by_key_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ratified_by_key_id = _parse_ratified_by_key_id(
            d.pop("ratified_by_key_id", UNSET)
        )

        _policy_snapshot = d.pop("policy_snapshot", UNSET)
        policy_snapshot: ReviewPolicy | Unset
        if isinstance(_policy_snapshot, Unset):
            policy_snapshot = UNSET
        else:
            policy_snapshot = ReviewPolicy.from_dict(_policy_snapshot)

        transition = cls(
            transition_id=transition_id,
            workarea_id=workarea_id,
            to=to,
            state=state,
            proposed_by_key_id=proposed_by_key_id,
            ratified_by_key_id=ratified_by_key_id,
            policy_snapshot=policy_snapshot,
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
