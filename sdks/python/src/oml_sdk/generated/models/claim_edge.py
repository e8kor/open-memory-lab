from __future__ import annotations

from collections.abc import Mapping
from typing import (
    Any,
    Literal,
    Self,
    TypeVar,
    cast,
)

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.claim_edge_state import ClaimEdgeState

T = TypeVar("T", bound="ClaimEdge")


@_attrs_define
class ClaimEdge:
    """
    Attributes:
        kind (Literal['claim']):
        edge_id (str):
        from_artifact_id (str):
        to_artifact_id (str):
        label (str):
        state (ClaimEdgeState):
        proposer_key_id (str):
        ratifier_key_id (None | str):
    """

    kind: Literal["claim"]
    edge_id: str
    from_artifact_id: str
    to_artifact_id: str
    label: str
    state: ClaimEdgeState
    proposer_key_id: str
    ratifier_key_id: None | str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        edge_id = self.edge_id

        from_artifact_id = self.from_artifact_id

        to_artifact_id = self.to_artifact_id

        label = self.label

        state = self.state.value

        proposer_key_id = self.proposer_key_id

        ratifier_key_id: None | str
        ratifier_key_id = self.ratifier_key_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "kind": kind,
                "edge_id": edge_id,
                "from_artifact_id": from_artifact_id,
                "to_artifact_id": to_artifact_id,
                "label": label,
                "state": state,
                "proposer_key_id": proposer_key_id,
                "ratifier_key_id": ratifier_key_id,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = cast(Literal["claim"], d.pop("kind"))
        if kind != "claim":
            raise ValueError(f"kind must match const 'claim', got '{kind}'")

        edge_id = d.pop("edge_id")

        from_artifact_id = d.pop("from_artifact_id")

        to_artifact_id = d.pop("to_artifact_id")

        label = d.pop("label")

        state = ClaimEdgeState(d.pop("state"))

        proposer_key_id = d.pop("proposer_key_id")

        def _parse_ratifier_key_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        ratifier_key_id = _parse_ratifier_key_id(d.pop("ratifier_key_id"))

        claim_edge = cls(
            kind=kind,
            edge_id=edge_id,
            from_artifact_id=from_artifact_id,
            to_artifact_id=to_artifact_id,
            label=label,
            state=state,
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
