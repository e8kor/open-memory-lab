from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="SetWebUserQuotaBody")


@_attrs_define
class SetWebUserQuotaBody:
    """
    Attributes:
        memories (int | None | Unset):
        keys (int | None | Unset):
        connections (int | None | Unset):
        requests_per_minute (int | None | Unset):
    """

    memories: int | None | Unset = UNSET
    keys: int | None | Unset = UNSET
    connections: int | None | Unset = UNSET
    requests_per_minute: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        memories: int | None | Unset
        if isinstance(self.memories, Unset):
            memories = UNSET
        else:
            memories = self.memories

        keys: int | None | Unset
        if isinstance(self.keys, Unset):
            keys = UNSET
        else:
            keys = self.keys

        connections: int | None | Unset
        if isinstance(self.connections, Unset):
            connections = UNSET
        else:
            connections = self.connections

        requests_per_minute: int | None | Unset
        if isinstance(self.requests_per_minute, Unset):
            requests_per_minute = UNSET
        else:
            requests_per_minute = self.requests_per_minute

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if memories is not UNSET:
            field_dict["memories"] = memories
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

        def _parse_memories(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        memories = _parse_memories(d.pop("memories", UNSET))

        def _parse_keys(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        keys = _parse_keys(d.pop("keys", UNSET))

        def _parse_connections(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        connections = _parse_connections(d.pop("connections", UNSET))

        def _parse_requests_per_minute(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        requests_per_minute = _parse_requests_per_minute(
            d.pop("requests_per_minute", UNSET)
        )

        set_web_user_quota_body = cls(
            memories=memories,
            keys=keys,
            connections=connections,
            requests_per_minute=requests_per_minute,
        )

        return set_web_user_quota_body
