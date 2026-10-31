from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="SubmitAcceptedScan")


@_attrs_define
class SubmitAcceptedScan:
    """
    Attributes:
        result (str | Unset):
        scanner_version (str | Unset):
    """

    result: str | Unset = UNSET
    scanner_version: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        result = self.result

        scanner_version = self.scanner_version

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if result is not UNSET:
            field_dict["result"] = result
        if scanner_version is not UNSET:
            field_dict["scanner_version"] = scanner_version

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        result = d.pop("result", UNSET)

        scanner_version = d.pop("scanner_version", UNSET)

        submit_accepted_scan = cls(
            result=result,
            scanner_version=scanner_version,
        )

        submit_accepted_scan.additional_properties = d
        return submit_accepted_scan

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
