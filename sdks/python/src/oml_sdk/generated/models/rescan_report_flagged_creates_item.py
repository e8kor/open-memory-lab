from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define

T = TypeVar("T", bound="RescanReportFlaggedCreatesItem")


@_attrs_define
class RescanReportFlaggedCreatesItem:
    """
    Attributes:
        key_id (UUID):
        workspaces (int):
    """

    key_id: UUID
    workspaces: int

    def to_dict(self) -> dict[str, Any]:
        key_id = str(self.key_id)

        workspaces = self.workspaces

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "workspaces": workspaces,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        key_id = UUID(d.pop("key_id"))

        workspaces = d.pop("workspaces")

        rescan_report_flagged_creates_item = cls(
            key_id=key_id,
            workspaces=workspaces,
        )

        return rescan_report_flagged_creates_item
