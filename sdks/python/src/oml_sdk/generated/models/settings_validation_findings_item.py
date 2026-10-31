from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="SettingsValidationFindingsItem")


@_attrs_define
class SettingsValidationFindingsItem:
    """
    Attributes:
        code (str | Unset): settings.retired_key or settings.invalid.
        key (str | Unset):
        replacement (None | str | Unset): For a retired key, the setting that replaced it.
        message (str | Unset):
    """

    code: str | Unset = UNSET
    key: str | Unset = UNSET
    replacement: None | str | Unset = UNSET
    message: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        code = self.code

        key = self.key

        replacement: None | str | Unset
        if isinstance(self.replacement, Unset):
            replacement = UNSET
        else:
            replacement = self.replacement

        message = self.message

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if code is not UNSET:
            field_dict["code"] = code
        if key is not UNSET:
            field_dict["key"] = key
        if replacement is not UNSET:
            field_dict["replacement"] = replacement
        if message is not UNSET:
            field_dict["message"] = message

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        code = d.pop("code", UNSET)

        key = d.pop("key", UNSET)

        def _parse_replacement(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        replacement = _parse_replacement(d.pop("replacement", UNSET))

        message = d.pop("message", UNSET)

        settings_validation_findings_item = cls(
            code=code,
            key=key,
            replacement=replacement,
            message=message,
        )

        settings_validation_findings_item.additional_properties = d
        return settings_validation_findings_item

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
