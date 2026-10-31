from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="BanPreview")


@_attrs_define
class BanPreview:
    """Reach preview — nothing journalled.

    Attributes:
        keys_affected (list[str] | Unset):
        held_refusals (int | Unset):
        workareas_touched (list[str] | Unset):
    """

    keys_affected: list[str] | Unset = UNSET
    held_refusals: int | Unset = UNSET
    workareas_touched: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        keys_affected: list[str] | Unset = UNSET
        if not isinstance(self.keys_affected, Unset):
            keys_affected = self.keys_affected

        held_refusals = self.held_refusals

        workareas_touched: list[str] | Unset = UNSET
        if not isinstance(self.workareas_touched, Unset):
            workareas_touched = self.workareas_touched

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if keys_affected is not UNSET:
            field_dict["keys_affected"] = keys_affected
        if held_refusals is not UNSET:
            field_dict["held_refusals"] = held_refusals
        if workareas_touched is not UNSET:
            field_dict["workareas_touched"] = workareas_touched

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        keys_affected = cast(list[str], d.pop("keys_affected", UNSET))

        held_refusals = d.pop("held_refusals", UNSET)

        workareas_touched = cast(list[str], d.pop("workareas_touched", UNSET))

        ban_preview = cls(
            keys_affected=keys_affected,
            held_refusals=held_refusals,
            workareas_touched=workareas_touched,
        )

        ban_preview.additional_properties = d
        return ban_preview

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
