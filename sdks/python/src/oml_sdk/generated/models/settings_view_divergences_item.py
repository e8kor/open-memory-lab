from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="SettingsViewDivergencesItem")


@_attrs_define
class SettingsViewDivergencesItem:
    """
    Attributes:
        key (str | Unset):
        configured (Any | Unset):
        effective (Any | Unset):
        cause (str | Unset): Defaulted value, not-yet-picked-up change, or per-replica variance.
    """

    key: str | Unset = UNSET
    configured: Any | Unset = UNSET
    effective: Any | Unset = UNSET
    cause: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        key = self.key

        configured = self.configured

        effective = self.effective

        cause = self.cause

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if key is not UNSET:
            field_dict["key"] = key
        if configured is not UNSET:
            field_dict["configured"] = configured
        if effective is not UNSET:
            field_dict["effective"] = effective
        if cause is not UNSET:
            field_dict["cause"] = cause

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        key = d.pop("key", UNSET)

        configured = d.pop("configured", UNSET)

        effective = d.pop("effective", UNSET)

        cause = d.pop("cause", UNSET)

        settings_view_divergences_item = cls(
            key=key,
            configured=configured,
            effective=effective,
            cause=cause,
        )

        settings_view_divergences_item.additional_properties = d
        return settings_view_divergences_item

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
