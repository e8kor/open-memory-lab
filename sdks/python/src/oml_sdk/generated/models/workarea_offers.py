from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workarea_offers_mode import WorkareaOffersMode

if TYPE_CHECKING:
    from ..models.workarea_offers_commit import WorkareaOffersCommit
    from ..models.workarea_offers_mode_change_type_0 import (
        WorkareaOffersModeChangeType0,
    )
    from ..models.workarea_offers_transitions_item import WorkareaOffersTransitionsItem


T = TypeVar("T", bound="WorkareaOffers")


@_attrs_define
class WorkareaOffers:
    """
    Attributes:
        workarea_id (UUID):
        status (str):
        mode (WorkareaOffersMode):
        transitions (list[WorkareaOffersTransitionsItem]):
        mode_change (None | WorkareaOffersModeChangeType0): Putting the workarea in progress, where offered; null
            otherwise.
        commit (WorkareaOffersCommit):
    """

    workarea_id: UUID
    status: str
    mode: WorkareaOffersMode
    transitions: list[WorkareaOffersTransitionsItem]
    mode_change: None | WorkareaOffersModeChangeType0
    commit: WorkareaOffersCommit
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.workarea_offers_mode_change_type_0 import (
            WorkareaOffersModeChangeType0,
        )

        workarea_id = str(self.workarea_id)

        status = self.status

        mode = self.mode.value

        transitions = []
        for transitions_item_data in self.transitions:
            transitions_item = transitions_item_data.to_dict()
            transitions.append(transitions_item)

        mode_change: dict[str, Any] | None
        if isinstance(self.mode_change, WorkareaOffersModeChangeType0):
            mode_change = self.mode_change.to_dict()
        else:
            mode_change = self.mode_change

        commit = self.commit.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workarea_id": workarea_id,
                "status": status,
                "mode": mode,
                "transitions": transitions,
                "mode_change": mode_change,
                "commit": commit,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.workarea_offers_commit import WorkareaOffersCommit
        from ..models.workarea_offers_mode_change_type_0 import (
            WorkareaOffersModeChangeType0,
        )
        from ..models.workarea_offers_transitions_item import (
            WorkareaOffersTransitionsItem,
        )

        d = dict(src_dict)
        workarea_id = UUID(d.pop("workarea_id"))

        status = d.pop("status")

        mode = WorkareaOffersMode(d.pop("mode"))

        transitions = []
        _transitions = d.pop("transitions")
        for transitions_item_data in _transitions:
            transitions_item = WorkareaOffersTransitionsItem.from_dict(
                transitions_item_data
            )

            transitions.append(transitions_item)

        def _parse_mode_change(data: object) -> None | WorkareaOffersModeChangeType0:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                mode_change_type_0 = WorkareaOffersModeChangeType0.from_dict(data)

                return mode_change_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | WorkareaOffersModeChangeType0, data)

        mode_change = _parse_mode_change(d.pop("mode_change"))

        commit = WorkareaOffersCommit.from_dict(d.pop("commit"))

        workarea_offers = cls(
            workarea_id=workarea_id,
            status=status,
            mode=mode,
            transitions=transitions,
            mode_change=mode_change,
            commit=commit,
        )

        workarea_offers.additional_properties = d
        return workarea_offers

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
