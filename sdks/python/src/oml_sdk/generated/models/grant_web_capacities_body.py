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
        workspaces (list[str]): Each by id or name.
    """

    capacity: GrantWebCapacitiesBodyCapacity
    workspaces: list[str]

    def to_dict(self) -> dict[str, Any]:
        capacity = self.capacity.value

        workspaces = self.workspaces

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "capacity": capacity,
                "workspaces": workspaces,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        capacity = GrantWebCapacitiesBodyCapacity(d.pop("capacity"))

        workspaces = cast(list[str], d.pop("workspaces"))

        grant_web_capacities_body = cls(
            capacity=capacity,
            workspaces=workspaces,
        )

        return grant_web_capacities_body
