from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ConnectCode")


@_attrs_define
class ConnectCode:
    """
    Attributes:
        code_id (str):
        expires_at (datetime.datetime):
        connector_url (str): The URL to enter as the custom connector: <issuer>/mcp, from oauth.issuer, not from the
            address the CLI used.
        code (str | Unset): Twelve Crockford base32 symbols (60 bits) in three groups of four — shown exactly once,
            never journalled, stored only as the SHA-256 of the normalized symbols (ADR-0031 d8).
    """

    code_id: str
    expires_at: datetime.datetime
    connector_url: str
    code: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        code_id = self.code_id

        expires_at = self.expires_at.isoformat()

        connector_url = self.connector_url

        code = self.code

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "code_id": code_id,
                "expires_at": expires_at,
                "connector_url": connector_url,
            }
        )
        if code is not UNSET:
            field_dict["code"] = code

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        code_id = d.pop("code_id")

        expires_at = datetime.datetime.fromisoformat(d.pop("expires_at"))

        connector_url = d.pop("connector_url")

        code = d.pop("code", UNSET)

        connect_code = cls(
            code_id=code_id,
            expires_at=expires_at,
            connector_url=connector_url,
            code=code,
        )

        connect_code.additional_properties = d
        return connect_code

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
