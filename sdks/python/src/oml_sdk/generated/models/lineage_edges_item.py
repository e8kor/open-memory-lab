from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.lineage_edges_item_kind import LineageEdgesItemKind
from ..models.lineage_edges_item_state import LineageEdgesItemState
from ..types import UNSET, Unset

T = TypeVar("T", bound="LineageEdgesItem")


@_attrs_define
class LineageEdgesItem:
    """
    Attributes:
        from_artifact_id (str | Unset):
        to_artifact_id (str | Unset):
        kind (LineageEdgesItemKind | Unset):
        state (LineageEdgesItemState | Unset):
    """

    from_artifact_id: str | Unset = UNSET
    to_artifact_id: str | Unset = UNSET
    kind: LineageEdgesItemKind | Unset = UNSET
    state: LineageEdgesItemState | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from_artifact_id = self.from_artifact_id

        to_artifact_id = self.to_artifact_id

        kind: str | Unset = UNSET
        if not isinstance(self.kind, Unset):
            kind = self.kind.value

        state: str | Unset = UNSET
        if not isinstance(self.state, Unset):
            state = self.state.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if from_artifact_id is not UNSET:
            field_dict["from_artifact_id"] = from_artifact_id
        if to_artifact_id is not UNSET:
            field_dict["to_artifact_id"] = to_artifact_id
        if kind is not UNSET:
            field_dict["kind"] = kind
        if state is not UNSET:
            field_dict["state"] = state

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        from_artifact_id = d.pop("from_artifact_id", UNSET)

        to_artifact_id = d.pop("to_artifact_id", UNSET)

        _kind = d.pop("kind", UNSET)
        kind: LineageEdgesItemKind | Unset
        if isinstance(_kind, Unset):
            kind = UNSET
        else:
            kind = LineageEdgesItemKind(_kind)

        _state = d.pop("state", UNSET)
        state: LineageEdgesItemState | Unset
        if isinstance(_state, Unset):
            state = UNSET
        else:
            state = LineageEdgesItemState(_state)

        lineage_edges_item = cls(
            from_artifact_id=from_artifact_id,
            to_artifact_id=to_artifact_id,
            kind=kind,
            state=state,
        )

        lineage_edges_item.additional_properties = d
        return lineage_edges_item

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
