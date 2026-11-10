from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="RecallSourcesCut")


@_attrs_define
class RecallSourcesCut:
    """The workareas holding a query word that wide did not search because max_sources was reached; their statistics still
    scored every hit.

        Attributes:
            count (int):
            matches_at_least (int):
    """

    count: int
    matches_at_least: int

    def to_dict(self) -> dict[str, Any]:
        count = self.count

        matches_at_least = self.matches_at_least

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "count": count,
                "matches_at_least": matches_at_least,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        count = d.pop("count")

        matches_at_least = d.pop("matches_at_least")

        recall_sources_cut = cls(
            count=count,
            matches_at_least=matches_at_least,
        )

        return recall_sources_cut
