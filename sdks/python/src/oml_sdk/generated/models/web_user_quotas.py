from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="WebUserQuotas")


@_attrs_define
class WebUserQuotas:
    """
    Attributes:
        memories (int | Unset):
        text_bytes (int | None | Unset): Null is unlimited (ADR-0071 d5). Since v0.7.3.
        writes_per_month (int | None | Unset): Null is unlimited (ADR-0071 d5). Since v0.7.3.
        keys (int | Unset):
        connections (int | Unset):
        requests_per_minute (int | Unset):
    """

    memories: int | Unset = UNSET
    text_bytes: int | None | Unset = UNSET
    writes_per_month: int | None | Unset = UNSET
    keys: int | Unset = UNSET
    connections: int | Unset = UNSET
    requests_per_minute: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        memories = self.memories

        text_bytes: int | None | Unset
        if isinstance(self.text_bytes, Unset):
            text_bytes = UNSET
        else:
            text_bytes = self.text_bytes

        writes_per_month: int | None | Unset
        if isinstance(self.writes_per_month, Unset):
            writes_per_month = UNSET
        else:
            writes_per_month = self.writes_per_month

        keys = self.keys

        connections = self.connections

        requests_per_minute = self.requests_per_minute

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if memories is not UNSET:
            field_dict["memories"] = memories
        if text_bytes is not UNSET:
            field_dict["text_bytes"] = text_bytes
        if writes_per_month is not UNSET:
            field_dict["writes_per_month"] = writes_per_month
        if keys is not UNSET:
            field_dict["keys"] = keys
        if connections is not UNSET:
            field_dict["connections"] = connections
        if requests_per_minute is not UNSET:
            field_dict["requests_per_minute"] = requests_per_minute

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        memories = d.pop("memories", UNSET)

        def _parse_text_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        text_bytes = _parse_text_bytes(d.pop("text_bytes", UNSET))

        def _parse_writes_per_month(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        writes_per_month = _parse_writes_per_month(d.pop("writes_per_month", UNSET))

        keys = d.pop("keys", UNSET)

        connections = d.pop("connections", UNSET)

        requests_per_minute = d.pop("requests_per_minute", UNSET)

        web_user_quotas = cls(
            memories=memories,
            text_bytes=text_bytes,
            writes_per_month=writes_per_month,
            keys=keys,
            connections=connections,
            requests_per_minute=requests_per_minute,
        )

        web_user_quotas.additional_properties = d
        return web_user_quotas

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
