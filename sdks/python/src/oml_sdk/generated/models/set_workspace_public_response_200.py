from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar
from uuid import UUID

from attrs import define as _attrs_define

from ..models.set_workspace_public_response_200_state import (
    SetWorkspacePublicResponse200State,
)

T = TypeVar("T", bound="SetWorkspacePublicResponse200")


@_attrs_define
class SetWorkspacePublicResponse200:
    """
    Attributes:
        workspace (UUID):
        public (bool):
        act_id (UUID):
        state (SetWorkspacePublicResponse200State):
    """

    workspace: UUID
    public: bool
    act_id: UUID
    state: SetWorkspacePublicResponse200State

    def to_dict(self) -> dict[str, Any]:
        workspace = str(self.workspace)

        public = self.public

        act_id = str(self.act_id)

        state = self.state.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "workspace": workspace,
                "public": public,
                "act_id": act_id,
                "state": state,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workspace = UUID(d.pop("workspace"))

        public = d.pop("public")

        act_id = UUID(d.pop("act_id"))

        state = SetWorkspacePublicResponse200State(d.pop("state"))

        set_workspace_public_response_200 = cls(
            workspace=workspace,
            public=public,
            act_id=act_id,
            state=state,
        )

        return set_workspace_public_response_200
