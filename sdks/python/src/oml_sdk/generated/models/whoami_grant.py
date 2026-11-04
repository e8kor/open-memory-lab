from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.whoami_grant_role import WhoamiGrantRole

T = TypeVar("T", bound="WhoamiGrant")


@_attrs_define
class WhoamiGrant:
    """A stored grant as whoami serves it, without its per-grant deny masks, which name subtrees the key may not read
    through it (ADR-0039 d15).

        Attributes:
            grant_id (str):
            workarea_node_id (str): Node id, never a path.
            role (WhoamiGrantRole):
            epoch (int): The journal seq of this grant's grant.issued event.
    """

    grant_id: str
    workarea_node_id: str
    role: WhoamiGrantRole
    epoch: int

    def to_dict(self) -> dict[str, Any]:
        grant_id = self.grant_id

        workarea_node_id = self.workarea_node_id

        role = self.role.value

        epoch = self.epoch

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "grant_id": grant_id,
                "workarea_node_id": workarea_node_id,
                "role": role,
                "epoch": epoch,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        grant_id = d.pop("grant_id")

        workarea_node_id = d.pop("workarea_node_id")

        role = WhoamiGrantRole(d.pop("role"))

        epoch = d.pop("epoch")

        whoami_grant = cls(
            grant_id=grant_id,
            workarea_node_id=workarea_node_id,
            role=role,
            epoch=epoch,
        )

        return whoami_grant
