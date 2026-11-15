from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="DocumentFields")


@_attrs_define
class DocumentFields:
    """A document's title, signed with the version (ADR-0081 d1).

    Attributes:
        title (str | Unset): One line. Omitted, the first level-1 heading of a markdown body, else the claim key's last
            segment.
    """

    title: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        title = self.title

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if title is not UNSET:
            field_dict["title"] = title

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        title = d.pop("title", UNSET)

        document_fields = cls(
            title=title,
        )

        return document_fields
