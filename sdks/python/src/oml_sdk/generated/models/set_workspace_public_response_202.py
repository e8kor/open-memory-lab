from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define

from ..models.set_workspace_public_response_202_state import (
    SetWorkspacePublicResponse202State,
)

T = TypeVar("T", bound="SetWorkspacePublicResponse202")


@_attrs_define
class SetWorkspacePublicResponse202:
    """
    Attributes:
        workspace (UUID):
        act_id (UUID):
        state (SetWorkspacePublicResponse202State):
    """

    workspace: UUID
    act_id: UUID
    state: SetWorkspacePublicResponse202State

    def to_dict(self) -> dict[str, Any]:
        workspace = str(self.workspace)

        act_id = str(self.act_id)

        state = self.state.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "workspace": workspace,
                "act_id": act_id,
                "state": state,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workspace = UUID(d.pop("workspace"))

        act_id = UUID(d.pop("act_id"))

        state = SetWorkspacePublicResponse202State(d.pop("state"))

        set_workspace_public_response_202 = cls(
            workspace=workspace,
            act_id=act_id,
            state=state,
        )

        return set_workspace_public_response_202
