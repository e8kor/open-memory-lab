from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.settings_validation_findings_item import (
        SettingsValidationFindingsItem,
    )


T = TypeVar("T", bound="SettingsValidation")


@_attrs_define
class SettingsValidation:
    """
    Attributes:
        valid (bool):
        findings (list[SettingsValidationFindingsItem]):
    """

    valid: bool
    findings: list[SettingsValidationFindingsItem]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        valid = self.valid

        findings = []
        for findings_item_data in self.findings:
            findings_item = findings_item_data.to_dict()
            findings.append(findings_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "valid": valid,
                "findings": findings,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.settings_validation_findings_item import (
            SettingsValidationFindingsItem,
        )

        d = dict(src_dict)
        valid = d.pop("valid")

        findings = []
        _findings = d.pop("findings")
        for findings_item_data in _findings:
            findings_item = SettingsValidationFindingsItem.from_dict(findings_item_data)

            findings.append(findings_item)

        settings_validation = cls(
            valid=valid,
            findings=findings,
        )

        settings_validation.additional_properties = d
        return settings_validation

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
