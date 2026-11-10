from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.grant_web_capacities_body_capacity import GrantWebCapacitiesBodyCapacity

T = TypeVar("T", bound="GrantWebCapacitiesBody")


@_attrs_define
class GrantWebCapacitiesBody:
    """
    Attributes:
        capacity (GrantWebCapacitiesBodyCapacity):
        workareas (list[str]): Each by id or name.
    """

    capacity: GrantWebCapacitiesBodyCapacity
    workareas: list[str]

    def to_dict(self) -> dict[str, Any]:
        capacity = self.capacity.value

        workareas = self.workareas

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "capacity": capacity,
                "workareas": workareas,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        capacity = GrantWebCapacitiesBodyCapacity(d.pop("capacity"))

        workareas = cast(list[str], d.pop("workareas"))

        grant_web_capacities_body = cls(
            capacity=capacity,
            workareas=workareas,
        )

        return grant_web_capacities_body
