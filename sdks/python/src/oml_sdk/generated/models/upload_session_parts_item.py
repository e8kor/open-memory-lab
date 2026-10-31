from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="UploadSessionPartsItem")


@_attrs_define
class UploadSessionPartsItem:
    """
    Attributes:
        part_number (int | Unset):
        size_bytes (int | Unset):
    """

    part_number: int | Unset = UNSET
    size_bytes: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        part_number = self.part_number

        size_bytes = self.size_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if part_number is not UNSET:
            field_dict["part_number"] = part_number
        if size_bytes is not UNSET:
            field_dict["size_bytes"] = size_bytes

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        part_number = d.pop("part_number", UNSET)

        size_bytes = d.pop("size_bytes", UNSET)

        upload_session_parts_item = cls(
            part_number=part_number,
            size_bytes=size_bytes,
        )

        upload_session_parts_item.additional_properties = d
        return upload_session_parts_item

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
