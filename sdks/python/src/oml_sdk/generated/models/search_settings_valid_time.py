from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="SearchSettingsValidTime")


@_attrs_define
class SearchSettingsValidTime:
    """The tenant's served memories that declare valid_from, and those whose valid_until has passed, which recall hides by
    default (ADR-0059 d12).

        Attributes:
            declared (int):
            expired (int):
    """

    declared: int
    expired: int

    def to_dict(self) -> dict[str, Any]:
        declared = self.declared

        expired = self.expired

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "declared": declared,
                "expired": expired,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        declared = d.pop("declared")

        expired = d.pop("expired")

        search_settings_valid_time = cls(
            declared=declared,
            expired=expired,
        )

        return search_settings_valid_time
