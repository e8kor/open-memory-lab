from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ListWebUsersResponse200UsersItemUsage")


@_attrs_define
class ListWebUsersResponse200UsersItemUsage:
    """
    Attributes:
        memories (int | Unset):
        text_bytes (int | Unset):
        writes (int | Unset): Versions submitted this UTC month (ADR-0071 d2).
        keys (int | Unset):
        connections (int | Unset):
    """

    memories: int | Unset = UNSET
    text_bytes: int | Unset = UNSET
    writes: int | Unset = UNSET
    keys: int | Unset = UNSET
    connections: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        memories = self.memories

        text_bytes = self.text_bytes

        writes = self.writes

        keys = self.keys

        connections = self.connections

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if memories is not UNSET:
            field_dict["memories"] = memories
        if text_bytes is not UNSET:
            field_dict["text_bytes"] = text_bytes
        if writes is not UNSET:
            field_dict["writes"] = writes
        if keys is not UNSET:
            field_dict["keys"] = keys
        if connections is not UNSET:
            field_dict["connections"] = connections

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        memories = d.pop("memories", UNSET)

        text_bytes = d.pop("text_bytes", UNSET)

        writes = d.pop("writes", UNSET)

        keys = d.pop("keys", UNSET)

        connections = d.pop("connections", UNSET)

        list_web_users_response_200_users_item_usage = cls(
            memories=memories,
            text_bytes=text_bytes,
            writes=writes,
            keys=keys,
            connections=connections,
        )

        list_web_users_response_200_users_item_usage.additional_properties = d
        return list_web_users_response_200_users_item_usage

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
