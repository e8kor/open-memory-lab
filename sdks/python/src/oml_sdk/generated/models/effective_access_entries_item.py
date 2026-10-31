from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.role import Role
from ..types import UNSET, Unset

T = TypeVar("T", bound="EffectiveAccessEntriesItem")


@_attrs_define
class EffectiveAccessEntriesItem:
    """
    Attributes:
        workarea_node_id (str | Unset):
        path (str | Unset):
        role (Role | Unset):
        masked_by (None | str | Unset): The deny (per-grant or global) that produced a hole, when one does.
    """

    workarea_node_id: str | Unset = UNSET
    path: str | Unset = UNSET
    role: Role | Unset = UNSET
    masked_by: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        workarea_node_id = self.workarea_node_id

        path = self.path

        role: str | Unset = UNSET
        if not isinstance(self.role, Unset):
            role = self.role.value

        masked_by: None | str | Unset
        if isinstance(self.masked_by, Unset):
            masked_by = UNSET
        else:
            masked_by = self.masked_by

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if workarea_node_id is not UNSET:
            field_dict["workarea_node_id"] = workarea_node_id
        if path is not UNSET:
            field_dict["path"] = path
        if role is not UNSET:
            field_dict["role"] = role
        if masked_by is not UNSET:
            field_dict["masked_by"] = masked_by

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        workarea_node_id = d.pop("workarea_node_id", UNSET)

        path = d.pop("path", UNSET)

        _role = d.pop("role", UNSET)
        role: Role | Unset
        if isinstance(_role, Unset):
            role = UNSET
        else:
            role = Role(_role)

        def _parse_masked_by(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        masked_by = _parse_masked_by(d.pop("masked_by", UNSET))

        effective_access_entries_item = cls(
            workarea_node_id=workarea_node_id,
            path=path,
            role=role,
            masked_by=masked_by,
        )

        effective_access_entries_item.additional_properties = d
        return effective_access_entries_item

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
