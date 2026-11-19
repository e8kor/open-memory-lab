from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.memory_preview_shadows_item_relation import (
    MemoryPreviewShadowsItemRelation,
)

T = TypeVar("T", bound="MemoryPreviewShadowsItem")


@_attrs_define
class MemoryPreviewShadowsItem:
    """
    Attributes:
        workspace (str): The workspace's path.
        relation (MemoryPreviewShadowsItemRelation):
    """

    workspace: str
    relation: MemoryPreviewShadowsItemRelation
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workspace = self.workspace

        relation = self.relation.value

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "workspace": workspace,
                "relation": relation,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workspace = d.pop("workspace")

        relation = MemoryPreviewShadowsItemRelation(d.pop("relation"))

        memory_preview_shadows_item = cls(
            workspace=workspace,
            relation=relation,
        )

        memory_preview_shadows_item.additional_properties = d
        return memory_preview_shadows_item

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
