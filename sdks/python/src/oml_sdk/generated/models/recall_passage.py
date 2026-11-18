from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RecallPassage")


@_attrs_define
class RecallPassage:
    """A document's hit only: the passage it serves (ADR-0081 d3, d4). A document is ranked by its passages and contributes
    one hit, its best passage by score, then ordinal; the hit's memory.text is that passage's lines byte for byte and
    memory.tokens its budget, so k counts claims and max_tokens counts the passage.

        Attributes:
            fragment (str): The heading's slug when the passage is a whole section, else L<n>-L<m>.
            ordinal (int): The passage's place in the document, from 0.
            start_line (int): Its first line, 1-based.
            end_line (int): Its last line, inclusive.
            heading_path (list[str]): The headings of level 1 to 4 above it, outermost first, as plain text.
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

        recall_passage = cls(
            fragment=fragment,
            ordinal=ordinal,
            start_line=start_line,
            end_line=end_line,
            heading_path=heading_path,
        )

        recall_passage.additional_properties = d
        return recall_passage

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
