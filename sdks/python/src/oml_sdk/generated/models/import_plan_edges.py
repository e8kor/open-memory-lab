from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

T = TypeVar("T", bound="ImportPlanEdges")


@_attrs_define
class ImportPlanEdges:
    """
    Attributes:
        proposed (int):
        reported (int):
    """

    proposed: int
    reported: int

    def to_dict(self) -> dict[str, Any]:
        proposed = self.proposed

        reported = self.reported

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "proposed": proposed,
                "reported": reported,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        proposed = d.pop("proposed")

        reported = d.pop("reported")

        import_plan_edges = cls(
            proposed=proposed,
            reported=reported,
        )

        return import_plan_edges
