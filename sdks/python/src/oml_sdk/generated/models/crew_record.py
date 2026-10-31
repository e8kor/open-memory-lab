from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="CrewRecord")


@_attrs_define
class CrewRecord:
    """Recorded, not operated — OML never contacts the external manager.

    Attributes:
        name (str | Unset):
        charter_uri (str | Unset):
        external_manager (str | Unset):
    """

    name: str | Unset = UNSET
    charter_uri: str | Unset = UNSET
    external_manager: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        charter_uri = self.charter_uri

        external_manager = self.external_manager

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if name is not UNSET:
            field_dict["name"] = name
        if charter_uri is not UNSET:
            field_dict["charter_uri"] = charter_uri
        if external_manager is not UNSET:
            field_dict["external_manager"] = external_manager

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        name = d.pop("name", UNSET)

        charter_uri = d.pop("charter_uri", UNSET)

        external_manager = d.pop("external_manager", UNSET)

        crew_record = cls(
            name=name,
            charter_uri=charter_uri,
            external_manager=external_manager,
        )

        crew_record.additional_properties = d
        return crew_record

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
