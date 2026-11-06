from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="KeyLiveConnectCodeType1")


@_attrs_define
class KeyLiveConnectCodeType1:
    """
    Attributes:
        code_id (str):
        expires_at (datetime.datetime):
        extended (bool):
    """

    code_id: str
    expires_at: datetime.datetime
    extended: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        code_id = self.code_id

        expires_at = self.expires_at.isoformat()

        extended = self.extended

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "code_id": code_id,
                "expires_at": expires_at,
                "extended": extended,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        code_id = d.pop("code_id")

        expires_at = datetime.datetime.fromisoformat(d.pop("expires_at"))

        extended = d.pop("extended")

        key_live_connect_code_type_1 = cls(
            code_id=code_id,
            expires_at=expires_at,
            extended=extended,
        )

        key_live_connect_code_type_1.additional_properties = d
        return key_live_connect_code_type_1

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
