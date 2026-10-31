from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workarea_edge_proposal_label import WorkareaEdgeProposalLabel

T = TypeVar("T", bound="WorkareaEdgeProposal")


@_attrs_define
class WorkareaEdgeProposal:
    """
    Attributes:
        to_workarea_id (str):
        label (WorkareaEdgeProposalLabel):
    """

    to_workarea_id: str
    label: WorkareaEdgeProposalLabel
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        to_workarea_id = self.to_workarea_id

        label = self.label.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "to_workarea_id": to_workarea_id,
                "label": label,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        to_workarea_id = d.pop("to_workarea_id")

        label = WorkareaEdgeProposalLabel(d.pop("label"))

        workarea_edge_proposal = cls(
            to_workarea_id=to_workarea_id,
            label=label,
        )

        workarea_edge_proposal.additional_properties = d
        return workarea_edge_proposal

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
