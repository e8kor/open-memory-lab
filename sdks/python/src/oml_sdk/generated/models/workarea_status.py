from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workarea_status_status import WorkareaStatusStatus
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.review_policy import ReviewPolicy
    from ..models.transition import Transition


T = TypeVar("T", bound="WorkareaStatus")


@_attrs_define
class WorkareaStatus:
    """
    Attributes:
        workarea_id (str):
        status (WorkareaStatusStatus):
        policy (ReviewPolicy | Unset): The per-workarea review policy object (journalled record; admission page owns
            it).
        pending_transitions (list[Transition] | Unset):
    """

    workarea_id: str
    status: WorkareaStatusStatus
    policy: ReviewPolicy | Unset = UNSET
    pending_transitions: list[Transition] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workarea_id = self.workarea_id

        status = self.status.value

        policy: dict[str, Any] | Unset = UNSET
        if not isinstance(self.policy, Unset):
            policy = self.policy.to_dict()

        pending_transitions: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.pending_transitions, Unset):
            pending_transitions = []
            for pending_transitions_item_data in self.pending_transitions:
                pending_transitions_item = pending_transitions_item_data.to_dict()
                pending_transitions.append(pending_transitions_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workarea_id": workarea_id,
                "status": status,
            }
        )
        if policy is not UNSET:
            field_dict["policy"] = policy
        if pending_transitions is not UNSET:
            field_dict["pending_transitions"] = pending_transitions

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.review_policy import ReviewPolicy
        from ..models.transition import Transition

        d = dict(src_dict)
        workarea_id = d.pop("workarea_id")

        status = WorkareaStatusStatus(d.pop("status"))

        _policy = d.pop("policy", UNSET)
        policy: ReviewPolicy | Unset
        if isinstance(_policy, Unset):
            policy = UNSET
        else:
            policy = ReviewPolicy.from_dict(_policy)

        _pending_transitions = d.pop("pending_transitions", UNSET)
        pending_transitions: list[Transition] | Unset = UNSET
        if _pending_transitions is not UNSET:
            pending_transitions = []
            for pending_transitions_item_data in _pending_transitions:
                pending_transitions_item = Transition.from_dict(
                    pending_transitions_item_data
                )

                pending_transitions.append(pending_transitions_item)

        workarea_status = cls(
            workarea_id=workarea_id,
            status=status,
            policy=policy,
            pending_transitions=pending_transitions,
        )

        workarea_status.additional_properties = d
        return workarea_status

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
