from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast
from uuid import UUID

from attrs import define as _attrs_define

T = TypeVar("T", bound="RescanReportPublicWorkspacesItem")


@_attrs_define
class RescanReportPublicWorkspacesItem:
    """
    Attributes:
        workspace_id (UUID):
        name (str):
        since (None | str):
        act_id (None | str):
        sole_admin (bool):
        served_rows (int):
    """

    workspace_id: UUID
    name: str
    since: None | str
    act_id: None | str
    sole_admin: bool
    served_rows: int

    def to_dict(self) -> dict[str, Any]:
        workspace_id = str(self.workspace_id)

        name = self.name

        since: None | str
        since = self.since

        act_id: None | str
        act_id = self.act_id

        sole_admin = self.sole_admin

        served_rows = self.served_rows

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "workspace_id": workspace_id,
                "name": name,
                "since": since,
                "act_id": act_id,
                "sole_admin": sole_admin,
                "served_rows": served_rows,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workspace_id = UUID(d.pop("workspace_id"))

        name = d.pop("name")

        def _parse_since(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        since = _parse_since(d.pop("since"))

        def _parse_act_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        act_id = _parse_act_id(d.pop("act_id"))

        sole_admin = d.pop("sole_admin")

        served_rows = d.pop("served_rows")

        rescan_report_public_workspaces_item = cls(
            workspace_id=workspace_id,
            name=name,
            since=since,
            act_id=act_id,
            sole_admin=sole_admin,
            served_rows=served_rows,
        )

        return rescan_report_public_workspaces_item
