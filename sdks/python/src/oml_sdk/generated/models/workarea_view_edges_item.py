from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.workarea_view_edges_item_label import WorkareaViewEdgesItemLabel
from ..models.workarea_view_edges_item_state import WorkareaViewEdgesItemState

T = TypeVar("T", bound="WorkareaViewEdgesItem")


@_attrs_define
class WorkareaViewEdgesItem:
    """
    Attributes:
        edge_id (str):
        from_ (str):
        to (str):
        label (WorkareaViewEdgesItemLabel):
        state (WorkareaViewEdgesItemState):
    """

    edge_id: str
    from_: str
    to: str
    label: WorkareaViewEdgesItemLabel
    state: WorkareaViewEdgesItemState

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

        label = WorkareaViewEdgesItemLabel(d.pop("label"))

        state = WorkareaViewEdgesItemState(d.pop("state"))

        workarea_view_edges_item = cls(
            edge_id=edge_id,
            from_=from_,
            to=to,
            label=label,
            state=state,
        )

        return workarea_view_edges_item
