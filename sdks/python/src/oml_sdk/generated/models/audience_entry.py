from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="AudienceEntry")


@_attrs_define
class AudienceEntry:
    """Exactly one of key_id / grant_ref names the audience member; key_id takes a key's id or name.

    Attributes:
        key_id (str | Unset): A key, by its id or its name, resolved server-side like every reference; a revoked or
            expired key's name still resolves (ADR-0026 d4, d10).
        grant_ref (str | Unset):
    """

    key_id: str | Unset = UNSET
    grant_ref: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        key_id: str | Unset
        if isinstance(self.key_id, Unset):
            key_id = UNSET
        else:
            key_id = self.key_id

        grant_ref = self.grant_ref

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if key_id is not UNSET:
            field_dict["key_id"] = key_id
        if grant_ref is not UNSET:
            field_dict["grant_ref"] = grant_ref

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_key_id(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        key_id = _parse_key_id(d.pop("key_id", UNSET))

        grant_ref = d.pop("grant_ref", UNSET)

        audience_entry = cls(
            key_id=key_id,
            grant_ref=grant_ref,
        )

        audience_entry.additional_properties = d
        return audience_entry

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
