from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ReadyScanType0")


@_attrs_define
class ReadyScanType0:
    """The scanner's rule set, how many rules it loaded and whether the personal-data rules are on, or null for a server
    that pins none; doctor's scan line reads it (ADR-0065 d11). Its findings are counted on the metrics listener, never
    here.

        Attributes:
            version (str | Unset):
            rules (int | Unset):
            pii (bool | Unset):
    """

    version: str | Unset = UNSET
    rules: int | Unset = UNSET
    pii: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        version = self.version

        rules = self.rules

        pii = self.pii

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if version is not UNSET:
            field_dict["version"] = version
        if rules is not UNSET:
            field_dict["rules"] = rules
        if pii is not UNSET:
            field_dict["pii"] = pii

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        version = d.pop("version", UNSET)

        rules = d.pop("rules", UNSET)

        pii = d.pop("pii", UNSET)

        ready_scan_type_0 = cls(
            version=version,
            rules=rules,
            pii=pii,
        )

        ready_scan_type_0.additional_properties = d
        return ready_scan_type_0

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
