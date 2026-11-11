from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="SearchSettingsValidTime")


@_attrs_define
class SearchSettingsValidTime:
    """The tenant's served memories that declare valid_from, and those whose valid_until has passed, which recall hides by
    default (ADR-0059 d12).

        Attributes:
            declared (int):
            expired (int):
            untimed (int | Unset): Promoted versions with no promoted_at, which a row folded before migration 0005 can be:
                an as-of read never serves one and serves the version before it in its place (ADR-0059 d16).
    """

    declared: int
    expired: int
    untimed: int | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        declared = self.declared

        expired = self.expired

        untimed = self.untimed

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "declared": declared,
                "expired": expired,
            }
        )
        if untimed is not UNSET:
            field_dict["untimed"] = untimed

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        declared = d.pop("declared")

        expired = d.pop("expired")

        untimed = d.pop("untimed", UNSET)

        search_settings_valid_time = cls(
            declared=declared,
            expired=expired,
            untimed=untimed,
        )

        return search_settings_valid_time
