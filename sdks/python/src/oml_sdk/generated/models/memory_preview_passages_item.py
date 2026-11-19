from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="MemoryPreviewPassagesItem")


@_attrs_define
class MemoryPreviewPassagesItem:
    """
    Attributes:
        fragment (str): The heading's slug for a whole section, else L<n>-L<m> (ADR-0057 d7, d21).
        ordinal (int):
        start_line (int): 1-based, inclusive.
        end_line (int): 1-based, inclusive.
        heading_path (list[str]): The headings above it, outermost first.
    """

    fragment: str
    ordinal: int
    start_line: int
    end_line: int
    heading_path: list[str]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        fragment = self.fragment

        ordinal = self.ordinal

        start_line = self.start_line

        end_line = self.end_line

        heading_path = self.heading_path

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "fragment": fragment,
                "ordinal": ordinal,
                "start_line": start_line,
                "end_line": end_line,
                "heading_path": heading_path,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        fragment = d.pop("fragment")

        ordinal = d.pop("ordinal")

        start_line = d.pop("start_line")

        end_line = d.pop("end_line")

        heading_path = cast(list[str], d.pop("heading_path"))

        memory_preview_passages_item = cls(
            fragment=fragment,
            ordinal=ordinal,
            start_line=start_line,
            end_line=end_line,
            heading_path=heading_path,
        )

        memory_preview_passages_item.additional_properties = d
        return memory_preview_passages_item

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
