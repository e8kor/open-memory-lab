from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workarea_edge_label import WorkareaEdgeLabel
from ..models.workarea_edge_state import WorkareaEdgeState
from ..types import UNSET, Unset

T = TypeVar("T", bound="WorkareaEdge")


@_attrs_define
class WorkareaEdge:
    """
    Attributes:
        edge_id (str):
        from_workarea_id (str):
        to_workarea_id (str):
        label (WorkareaEdgeLabel):
        state (WorkareaEdgeState):
        proposer_key_id (str | Unset):
        ratifier_key_id (None | str | Unset):
    """

    edge_id: str
    from_workarea_id: str
    to_workarea_id: str
    label: WorkareaEdgeLabel
    state: WorkareaEdgeState
    proposer_key_id: str | Unset = UNSET
    ratifier_key_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        edge_id = self.edge_id

        from_workarea_id = self.from_workarea_id

        to_workarea_id = self.to_workarea_id

        label = self.label.value

        state = self.state.value

        proposer_key_id = self.proposer_key_id

        ratifier_key_id: None | str | Unset
        if isinstance(self.ratifier_key_id, Unset):
            ratifier_key_id = UNSET
        else:
            ratifier_key_id = self.ratifier_key_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "edge_id": edge_id,
                "from_workarea_id": from_workarea_id,
                "to_workarea_id": to_workarea_id,
                "label": label,
                "state": state,
            }
        )
        if proposer_key_id is not UNSET:
            field_dict["proposer_key_id"] = proposer_key_id
        if ratifier_key_id is not UNSET:
            field_dict["ratifier_key_id"] = ratifier_key_id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        edge_id = d.pop("edge_id")

        from_workarea_id = d.pop("from_workarea_id")

        to_workarea_id = d.pop("to_workarea_id")

        label = WorkareaEdgeLabel(d.pop("label"))

        state = WorkareaEdgeState(d.pop("state"))

        proposer_key_id = d.pop("proposer_key_id", UNSET)

        def _parse_ratifier_key_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ratifier_key_id = _parse_ratifier_key_id(d.pop("ratifier_key_id", UNSET))

        workarea_edge = cls(
            edge_id=edge_id,
            from_workarea_id=from_workarea_id,
            to_workarea_id=to_workarea_id,
            label=label,
            state=state,
            proposer_key_id=proposer_key_id,
            ratifier_key_id=ratifier_key_id,
        )

        workarea_edge.additional_properties = d
        return workarea_edge

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
