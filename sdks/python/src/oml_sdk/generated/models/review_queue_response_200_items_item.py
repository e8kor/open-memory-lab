from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ReviewQueueResponse200ItemsItem")


@_attrs_define
class ReviewQueueResponse200ItemsItem:
    """
    Attributes:
        workarea_id (UUID):
        path (str):
        held (int):
        oldest_at (datetime.datetime):
    """

    workarea_id: UUID
    path: str
    held: int
    oldest_at: datetime.datetime
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workarea_id = str(self.workarea_id)

        path = self.path

        held = self.held

        oldest_at = self.oldest_at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workarea_id": workarea_id,
                "path": path,
                "held": held,
                "oldest_at": oldest_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workarea_id = UUID(d.pop("workarea_id"))

        path = d.pop("path")

        held = d.pop("held")

        oldest_at = datetime.datetime.fromisoformat(d.pop("oldest_at"))

        review_queue_response_200_items_item = cls(
            workarea_id=workarea_id,
            path=path,
            held=held,
            oldest_at=oldest_at,
        )

        review_queue_response_200_items_item.additional_properties = d
        return review_queue_response_200_items_item

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
