from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="MemorySummaryReplacesType0")


@_attrs_define
class MemorySummaryReplacesType0:
    """For a supersession, the version it replaces; null for a version 1. It obeys the same readability test as
    derives_from on every read, so it is also null when the caller cannot read the replaced version — after an admin
    moved that version into another workarea or back into held, say (ADR-0025 d3).

        Attributes:
            artifact_id (str | Unset):
            version (int | Unset):
    """

    artifact_id: str | Unset = UNSET
    version: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        artifact_id = self.artifact_id

        version = self.version

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if artifact_id is not UNSET:
            field_dict["artifact_id"] = artifact_id
        if version is not UNSET:
            field_dict["version"] = version

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        artifact_id = d.pop("artifact_id", UNSET)

        version = d.pop("version", UNSET)

        memory_summary_replaces_type_0 = cls(
            artifact_id=artifact_id,
            version=version,
        )

        memory_summary_replaces_type_0.additional_properties = d
        return memory_summary_replaces_type_0

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
