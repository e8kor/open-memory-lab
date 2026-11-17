from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="HeldPageLaneAuthorsType0Item")


@_attrs_define
class HeldPageLaneAuthorsType0Item:
    """
    Attributes:
        author_key_id (UUID):
        held (int):
    """

    author_key_id: UUID
    held: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        author_key_id = str(self.author_key_id)

        held = self.held

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "author_key_id": author_key_id,
                "held": held,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        author_key_id = UUID(d.pop("author_key_id"))

        held = d.pop("held")

        held_page_lane_authors_type_0_item = cls(
            author_key_id=author_key_id,
            held=held,
        )

        held_page_lane_authors_type_0_item.additional_properties = d
        return held_page_lane_authors_type_0_item

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
