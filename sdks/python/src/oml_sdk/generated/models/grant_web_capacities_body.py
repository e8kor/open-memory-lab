from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.grant_web_capacities_body_capacity import GrantWebCapacitiesBodyCapacity
from ..types import UNSET, Unset

T = TypeVar("T", bound="GrantWebCapacitiesBody")


@_attrs_define
class GrantWebCapacitiesBody:
    """
    Attributes:
        capacity (GrantWebCapacitiesBodyCapacity):
        workspaces (list[str]): Each by id or name.
        reason (str | Unset): Why the capacity is given; scanned and journalled in each grant's admin.act. Required when
            the grant waits on a second admin person, outside sole_admin (ADR-0097 d5, d26). Since v0.8.6.
    """

    capacity: GrantWebCapacitiesBodyCapacity
    workspaces: list[str]
    reason: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        capacity = self.capacity.value

        workspaces = self.workspaces

        reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "capacity": capacity,
                "workspaces": workspaces,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        capacity = GrantWebCapacitiesBodyCapacity(d.pop("capacity"))

        workspaces = cast(list[str], d.pop("workspaces"))

        reason = d.pop("reason", UNSET)

        grant_web_capacities_body = cls(
            capacity=capacity,
            workspaces=workspaces,
            reason=reason,
        )

        return grant_web_capacities_body
