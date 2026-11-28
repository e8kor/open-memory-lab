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
        reason (None | str | Unset): The admin's reason. A raise above the person's effective limit, or a cleared
            override whose default is higher, is a widening admin act (raise_limits) that waits on a second admin person
            outside sole_admin and requires a reason, else request.validation with details.field /reason; a lowering is
            narrowing (lower_limits) and applies at once (ADR-0097 d15, d26).
    """

    memories: int | None | Unset = UNSET
    keys: int | None | Unset = UNSET
    connections: int | None | Unset = UNSET
    requests_per_minute: int | None | Unset = UNSET
    reason: None | str | Unset = UNSET

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

        reason: None | str | Unset
        if isinstance(self.reason, Unset):
            reason = UNSET
        else:
            reason = self.reason

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
        if reason is not UNSET:
            field_dict["reason"] = reason

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

        def _parse_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reason = _parse_reason(d.pop("reason", UNSET))

        set_web_user_quota_body = cls(
            memories=memories,
            keys=keys,
            connections=connections,
            requests_per_minute=requests_per_minute,
            reason=reason,
        )

        return set_web_user_quota_body
