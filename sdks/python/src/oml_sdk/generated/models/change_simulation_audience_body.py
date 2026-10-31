from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.audience_entry import AudienceEntry


T = TypeVar("T", bound="ChangeSimulationAudienceBody")


@_attrs_define
class ChangeSimulationAudienceBody:
    """At least one of add/remove must be non-empty (request.validation otherwise).

    Attributes:
        add (list[AudienceEntry] | Unset):
        remove (list[AudienceEntry] | Unset):
        reason (str | Unset):
    """

    add: list[AudienceEntry] | Unset = UNSET
    remove: list[AudienceEntry] | Unset = UNSET
    reason: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        add: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.add, Unset):
            add = []
            for add_item_data in self.add:
                add_item = add_item_data.to_dict()
                add.append(add_item)

        remove: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.remove, Unset):
            remove = []
            for remove_item_data in self.remove:
                remove_item = remove_item_data.to_dict()
                remove.append(remove_item)

        reason = self.reason

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if add is not UNSET:
            field_dict["add"] = add
        if remove is not UNSET:
            field_dict["remove"] = remove
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.audience_entry import AudienceEntry

        d = dict(src_dict)
        _add = d.pop("add", UNSET)
        add: list[AudienceEntry] | Unset = UNSET
        if _add is not UNSET:
            add = []
            for add_item_data in _add:
                add_item = AudienceEntry.from_dict(add_item_data)

                add.append(add_item)

        _remove = d.pop("remove", UNSET)
        remove: list[AudienceEntry] | Unset = UNSET
        if _remove is not UNSET:
            remove = []
            for remove_item_data in _remove:
                remove_item = AudienceEntry.from_dict(remove_item_data)

                remove.append(remove_item)

        reason = d.pop("reason", UNSET)

        change_simulation_audience_body = cls(
            add=add,
            remove=remove,
            reason=reason,
        )

        change_simulation_audience_body.additional_properties = d
        return change_simulation_audience_body

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
