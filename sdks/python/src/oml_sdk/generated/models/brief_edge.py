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

from ..models.brief_edge_state import BriefEdgeState

T = TypeVar("T", bound="BriefEdge")


@_attrs_define
class BriefEdge:
    """
    Attributes:
        kind (Literal['brief']):
        edge_id (str):
        from_artifact_id (str):
        to_workarea_id (str):
        label (Literal['brief']):
        state (BriefEdgeState):
        proposer_key_id (str):
        ratifier_key_id (None | str):
    """

    kind: Literal["brief"]
    edge_id: str
    from_artifact_id: str
    to_workarea_id: str
    label: Literal["brief"]
    state: BriefEdgeState
    proposer_key_id: str
    ratifier_key_id: None | str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        edge_id = self.edge_id

        from_artifact_id = self.from_artifact_id

        to_workarea_id = self.to_workarea_id

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
                "to_workarea_id": to_workarea_id,
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
        kind = cast(Literal["brief"], d.pop("kind"))
        if kind != "brief":
            raise ValueError(f"kind must match const 'brief', got '{kind}'")

        edge_id = d.pop("edge_id")

        from_artifact_id = d.pop("from_artifact_id")

        to_workarea_id = d.pop("to_workarea_id")

        label = cast(Literal["brief"], d.pop("label"))
        if label != "brief":
            raise ValueError(f"label must match const 'brief', got '{label}'")

        state = BriefEdgeState(d.pop("state"))

        proposer_key_id = d.pop("proposer_key_id")

        def _parse_ratifier_key_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        ratifier_key_id = _parse_ratifier_key_id(d.pop("ratifier_key_id"))

        brief_edge = cls(
            kind=kind,
            edge_id=edge_id,
            from_artifact_id=from_artifact_id,
            to_workarea_id=to_workarea_id,
            label=label,
            state=state,
            proposer_key_id=proposer_key_id,
            ratifier_key_id=ratifier_key_id,
        )

        brief_edge.additional_properties = d
        return brief_edge

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
