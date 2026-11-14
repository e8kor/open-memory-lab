from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.workspace_view_edges_item_label import WorkspaceViewEdgesItemLabel
from ..models.workspace_view_edges_item_state import WorkspaceViewEdgesItemState

T = TypeVar("T", bound="WorkspaceViewEdgesItem")


@_attrs_define
class WorkspaceViewEdgesItem:
    """
    Attributes:
        edge_id (str):
        from_ (str):
        to (str):
        label (WorkspaceViewEdgesItemLabel):
        state (WorkspaceViewEdgesItemState):
    """

    edge_id: str
    from_: str
    to: str
    label: WorkspaceViewEdgesItemLabel
    state: WorkspaceViewEdgesItemState

    def to_dict(self) -> dict[str, Any]:
        edge_id = self.edge_id

        from_ = self.from_

        to = self.to

        label = self.label.value

        state = self.state.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "edge_id": edge_id,
                "from": from_,
                "to": to,
                "label": label,
                "state": state,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        edge_id = d.pop("edge_id")

        from_ = d.pop("from")

        to = d.pop("to")

        label = WorkspaceViewEdgesItemLabel(d.pop("label"))

        state = WorkspaceViewEdgesItemState(d.pop("state"))

        workspace_view_edges_item = cls(
            edge_id=edge_id,
            from_=from_,
            to=to,
            label=label,
            state=state,
        )

        return workspace_view_edges_item
