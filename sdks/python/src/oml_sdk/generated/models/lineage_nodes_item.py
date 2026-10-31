from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="LineageNodesItem")


@_attrs_define
class LineageNodesItem:
    """
    Attributes:
        artifact_id (str | Unset):
        claim_key (str | Unset):
        version (int | Unset):
        redacted (bool | Unset): Outside the key's read scope; bare id only.
    """

    artifact_id: str | Unset = UNSET
    claim_key: str | Unset = UNSET
    version: int | Unset = UNSET
    redacted: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        claim_key = self.claim_key

        version = self.version

        redacted = self.redacted

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if artifact_id is not UNSET:
            field_dict["artifact_id"] = artifact_id
        if claim_key is not UNSET:
            field_dict["claim_key"] = claim_key
        if version is not UNSET:
            field_dict["version"] = version
        if redacted is not UNSET:
            field_dict["redacted"] = redacted

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        artifact_id = d.pop("artifact_id", UNSET)

        claim_key = d.pop("claim_key", UNSET)

        version = d.pop("version", UNSET)

        redacted = d.pop("redacted", UNSET)

        lineage_nodes_item = cls(
            artifact_id=artifact_id,
            claim_key=claim_key,
            version=version,
            redacted=redacted,
        )

        lineage_nodes_item.additional_properties = d
        return lineage_nodes_item

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
