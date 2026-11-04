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

T = TypeVar("T", bound="ClaimEdgeProposal")


@_attrs_define
class ClaimEdgeProposal:
    """
    Attributes:
        kind (Literal['claim']):
        from_artifact_id (str):
        to_artifact_id (str):
        label (str): Open; supports, contradicts, relates_to and continues are recommended, and oml: labels join terms
            only (ADR-0046 d6).
    """

    kind: Literal["claim"]
    from_artifact_id: str
    to_artifact_id: str
    label: str

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        from_artifact_id = self.from_artifact_id

        to_artifact_id = self.to_artifact_id

        label = self.label

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "kind": kind,
                "from_artifact_id": from_artifact_id,
                "to_artifact_id": to_artifact_id,
                "label": label,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        kind = cast(Literal["claim"], d.pop("kind"))
        if kind != "claim":
            raise ValueError(f"kind must match const 'claim', got '{kind}'")

        from_artifact_id = d.pop("from_artifact_id")

        to_artifact_id = d.pop("to_artifact_id")

        label = d.pop("label")

        claim_edge_proposal = cls(
            kind=kind,
            from_artifact_id=from_artifact_id,
            to_artifact_id=to_artifact_id,
            label=label,
        )

        return claim_edge_proposal
