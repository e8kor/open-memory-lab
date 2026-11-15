from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.workspace_view_children_item_status import WorkspaceViewChildrenItemStatus

T = TypeVar("T", bound="WorkspaceViewChildrenItem")


@_attrs_define
class WorkspaceViewChildrenItem:
    """
    Attributes:
        id (str):
        path_leaf (str):
        status (WorkspaceViewChildrenItemStatus):
    """

    id: str
    path_leaf: str
    status: WorkspaceViewChildrenItemStatus

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        path_leaf = self.path_leaf

        status = self.status.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "id": id,
                "path_leaf": path_leaf,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        id = d.pop("id")

        path_leaf = d.pop("path_leaf")

        status = WorkspaceViewChildrenItemStatus(d.pop("status"))

        workspace_view_children_item = cls(
            id=id,
            path_leaf=path_leaf,
            status=status,
        )

        return workspace_view_children_item
