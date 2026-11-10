from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="WorkareaCountsItemsItemCounts")


@_attrs_define
class WorkareaCountsItemsItemCounts:
    """
    Attributes:
        promoted (int): Promoted tips.
        held (int): Held rows outside an in-progress scope.
        in_progress (int): Held rows inside an in-progress scope.
        other (int): Every other state.
    """

    promoted: int
    held: int
    in_progress: int
    other: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        promoted = self.promoted

        held = self.held

        in_progress = self.in_progress

        other = self.other

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "promoted": promoted,
                "held": held,
                "in_progress": in_progress,
                "other": other,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        promoted = d.pop("promoted")

        held = d.pop("held")

        in_progress = d.pop("in_progress")

        other = d.pop("other")

        workarea_counts_items_item_counts = cls(
            promoted=promoted,
            held=held,
            in_progress=in_progress,
            other=other,
        )

        workarea_counts_items_item_counts.additional_properties = d
        return workarea_counts_items_item_counts

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
