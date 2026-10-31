from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.effective_access_entries_item import EffectiveAccessEntriesItem


T = TypeVar("T", bound="EffectiveAccess")


@_attrs_define
class EffectiveAccess:
    """
    Attributes:
        key_id (str):
        epoch (int):
        entries (list[EffectiveAccessEntriesItem]):
    """

    key_id: str
    epoch: int
    entries: list[EffectiveAccessEntriesItem]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        key_id = self.key_id

        epoch = self.epoch

        entries = []
        for entries_item_data in self.entries:
            entries_item = entries_item_data.to_dict()
            entries.append(entries_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "key_id": key_id,
                "epoch": epoch,
                "entries": entries,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.effective_access_entries_item import EffectiveAccessEntriesItem

        d = dict(src_dict)
        key_id = d.pop("key_id")

        epoch = d.pop("epoch")

        entries = []
        _entries = d.pop("entries")
        for entries_item_data in _entries:
            entries_item = EffectiveAccessEntriesItem.from_dict(entries_item_data)

            entries.append(entries_item)

        effective_access = cls(
            key_id=key_id,
            epoch=epoch,
            entries=entries,
        )

        effective_access.additional_properties = d
        return effective_access

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
