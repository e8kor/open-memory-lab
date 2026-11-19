from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="CatalogCountType1")


@_attrs_define
class CatalogCountType1:
    """
    Attributes:
        at_least (int):
    """

    at_least: int

    def to_dict(self) -> dict[str, Any]:
        at_least = self.at_least

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "at_least": at_least,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        at_least = d.pop("at_least")

        catalog_count_type_1 = cls(
            at_least=at_least,
        )

        return catalog_count_type_1
