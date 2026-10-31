from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.claim_edge_state import ClaimEdgeState
from ..types import UNSET, Unset

T = TypeVar("T", bound="ClaimEdge")


@_attrs_define
class ClaimEdge:
    """
    Attributes:
        edge_id (str):
        from_artifact_id (str):
        to_artifact_id (str):
        state (ClaimEdgeState):
        label (str | Unset):
        proposer_key_id (str | Unset):
        ratifier_key_id (None | str | Unset):
    """

    edge_id: str
    from_artifact_id: str
    to_artifact_id: str
    state: ClaimEdgeState
    label: str | Unset = UNSET
    proposer_key_id: str | Unset = UNSET
    ratifier_key_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        edge_id = self.edge_id

        from_artifact_id = self.from_artifact_id

        to_artifact_id = self.to_artifact_id

        state = self.state.value

        label = self.label

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
                "from_artifact_id": from_artifact_id,
                "to_artifact_id": to_artifact_id,
                "state": state,
            }
        )
        if label is not UNSET:
            field_dict["label"] = label
        if proposer_key_id is not UNSET:
            field_dict["proposer_key_id"] = proposer_key_id
        if ratifier_key_id is not UNSET:
            field_dict["ratifier_key_id"] = ratifier_key_id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        edge_id = d.pop("edge_id")

        from_artifact_id = d.pop("from_artifact_id")

        to_artifact_id = d.pop("to_artifact_id")

        state = ClaimEdgeState(d.pop("state"))

        label = d.pop("label", UNSET)

        proposer_key_id = d.pop("proposer_key_id", UNSET)

        def _parse_ratifier_key_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ratifier_key_id = _parse_ratifier_key_id(d.pop("ratifier_key_id", UNSET))

        claim_edge = cls(
            edge_id=edge_id,
            from_artifact_id=from_artifact_id,
            to_artifact_id=to_artifact_id,
            state=state,
            label=label,
            proposer_key_id=proposer_key_id,
            ratifier_key_id=ratifier_key_id,
        )

        claim_edge.additional_properties = d
        return claim_edge

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
