from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ReviewQueuePageItemsItem")


@_attrs_define
class ReviewQueuePageItemsItem:
    """
    Attributes:
        workarea_id (str | Unset):
        workarea_path (str | Unset):
        held (int | Unset):
        oldest_age_seconds (int | Unset):
    """

    workarea_id: str | Unset = UNSET
    workarea_path: str | Unset = UNSET
    held: int | Unset = UNSET
    oldest_age_seconds: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workarea_id = self.workarea_id

        workarea_path = self.workarea_path

        held = self.held

        oldest_age_seconds = self.oldest_age_seconds

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if workarea_id is not UNSET:
            field_dict["workarea_id"] = workarea_id
        if workarea_path is not UNSET:
            field_dict["workarea_path"] = workarea_path
        if held is not UNSET:
            field_dict["held"] = held
        if oldest_age_seconds is not UNSET:
            field_dict["oldest_age_seconds"] = oldest_age_seconds

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workarea_id = d.pop("workarea_id", UNSET)

        workarea_path = d.pop("workarea_path", UNSET)

        held = d.pop("held", UNSET)

        oldest_age_seconds = d.pop("oldest_age_seconds", UNSET)

        review_queue_page_items_item = cls(
            workarea_id=workarea_id,
            workarea_path=workarea_path,
            held=held,
            oldest_age_seconds=oldest_age_seconds,
        )

        review_queue_page_items_item.additional_properties = d
        return review_queue_page_items_item

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
