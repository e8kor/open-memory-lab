from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.approver import Approver


T = TypeVar("T", bound="ServedSkill")


@_attrs_define
class ServedSkill:
    """
    Attributes:
        artifact_id (str): GET /v1/memories/{artifact_id} serves the body.
        claim_key (str): skill/<name>.
        name (str): The command: the skill's name, or <name>-unreviewed for a held edit of an approved skill (ADR-0080
            d10).
        description (str):
        version (int):
        workspace_path (str):
        depth (int): The tree distance from the scope node, up or down: 0 on the scope node.
        overrides (list[str]): The farther workspaces' paths whose skill of this name it shadows, nearest first.
        approved_at (datetime.datetime | None): Null for a held version (ADR-0077 d5).
        approver (Approver | None):
        uri (str): The skill's MCP resource, oml://workspace/<workspace>/skill/<name> (ADR-0080 d7).
        unreviewed (bool): A held version, offered to its author key or inside an in-progress workspace (ADR-0080 d10,
            d14).
    """

    artifact_id: str
    claim_key: str
    name: str
    description: str
    version: int
    workspace_path: str
    depth: int
    overrides: list[str]
    approved_at: datetime.datetime | None
    approver: Approver | None
    uri: str
    unreviewed: bool

    def to_dict(self) -> dict[str, Any]:
        from ..models.approver import Approver

        artifact_id = self.artifact_id

        claim_key = self.claim_key

        name = self.name

        description = self.description

        version = self.version

        workspace_path = self.workspace_path

        depth = self.depth

        overrides = self.overrides

        approved_at: None | str
        if isinstance(self.approved_at, datetime.datetime):
            approved_at = self.approved_at.isoformat()
        else:
            approved_at = self.approved_at

        approver: dict[str, Any] | None
        if isinstance(self.approver, Approver):
            approver = self.approver.to_dict()
        else:
            approver = self.approver

        uri = self.uri

        unreviewed = self.unreviewed

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "artifact_id": artifact_id,
                "claim_key": claim_key,
                "name": name,
                "description": description,
                "version": version,
                "workspace_path": workspace_path,
                "depth": depth,
                "overrides": overrides,
                "approved_at": approved_at,
                "approver": approver,
                "uri": uri,
                "unreviewed": unreviewed,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.approver import Approver

        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        claim_key = d.pop("claim_key")

        name = d.pop("name")

        description = d.pop("description")

        version = d.pop("version")

        workspace_path = d.pop("workspace_path")

        depth = d.pop("depth")

        overrides = cast(list[str], d.pop("overrides"))

        def _parse_approved_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                approved_at_type_0 = datetime.datetime.fromisoformat(data)

                return approved_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        approved_at = _parse_approved_at(d.pop("approved_at"))

        def _parse_approver(data: object) -> Approver | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                approver_type_0 = Approver.from_dict(data)

                return approver_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Approver | None, data)

        approver = _parse_approver(d.pop("approver"))

        uri = d.pop("uri")

        unreviewed = d.pop("unreviewed")

        served_skill = cls(
            artifact_id=artifact_id,
            claim_key=claim_key,
            name=name,
            description=description,
            version=version,
            workspace_path=workspace_path,
            depth=depth,
            overrides=overrides,
            approved_at=approved_at,
            approver=approver,
            uri=uri,
            unreviewed=unreviewed,
        )

        return served_skill
