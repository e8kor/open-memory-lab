from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ClaimEdgeProposal")


@_attrs_define
class ClaimEdgeProposal:
    """
    Attributes:
        from_artifact_id (str):
        to_artifact_id (str):
        label (str): Vocabulary not fixed by the design (open detail).
    """

    from_artifact_id: str
    to_artifact_id: str
    label: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from_artifact_id = self.from_artifact_id

        to_artifact_id = self.to_artifact_id

        label = self.label

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "from_artifact_id": from_artifact_id,
                "to_artifact_id": to_artifact_id,
                "label": label,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        from_artifact_id = d.pop("from_artifact_id")

        to_artifact_id = d.pop("to_artifact_id")

        label = d.pop("label")

        claim_edge_proposal = cls(
            from_artifact_id=from_artifact_id,
            to_artifact_id=to_artifact_id,
            label=label,
        )

        claim_edge_proposal.additional_properties = d
        return claim_edge_proposal

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
