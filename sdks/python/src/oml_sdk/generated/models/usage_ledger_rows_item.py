from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="UsageLedgerRowsItem")


@_attrs_define
class UsageLedgerRowsItem:
    """
    Attributes:
        day (datetime.date):
        account (str):
        user (None | str):
        profile (None | str): The profile the account was on when the day closed; null for an account under no limits
            (ADR-0071 d4, d5).
        memories (int):
        text_bytes (int):
        writes (int):
        recalls (int):
        keys (int):
        connections (int):
        workspaces (int | Unset): Live workspaces under the account's root when the day closed (ADR-0095 d4). Since
            v0.8.5.
    """

    day: datetime.date
    account: str
    user: None | str
    profile: None | str
    memories: int
    text_bytes: int
    writes: int
    recalls: int
    keys: int
    connections: int
    workspaces: int | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        day = self.day.isoformat()

        account = self.account

        user: None | str
        user = self.user

        profile: None | str
        profile = self.profile

        memories = self.memories

        text_bytes = self.text_bytes

        writes = self.writes

        recalls = self.recalls

        keys = self.keys

        connections = self.connections

        workspaces = self.workspaces

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "day": day,
                "account": account,
                "user": user,
                "profile": profile,
                "memories": memories,
                "text_bytes": text_bytes,
                "writes": writes,
                "recalls": recalls,
                "keys": keys,
                "connections": connections,
            }
        )
        if workspaces is not UNSET:
            field_dict["workspaces"] = workspaces

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        day = datetime.date.fromisoformat(d.pop("day"))

        account = d.pop("account")

        def _parse_user(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        user = _parse_user(d.pop("user"))

        def _parse_profile(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        profile = _parse_profile(d.pop("profile"))

        memories = d.pop("memories")

        text_bytes = d.pop("text_bytes")

        writes = d.pop("writes")

        recalls = d.pop("recalls")

        keys = d.pop("keys")

        connections = d.pop("connections")

        workspaces = d.pop("workspaces", UNSET)

        usage_ledger_rows_item = cls(
            day=day,
            account=account,
            user=user,
            profile=profile,
            memories=memories,
            text_bytes=text_bytes,
            writes=writes,
            recalls=recalls,
            keys=keys,
            connections=connections,
            workspaces=workspaces,
        )

        return usage_ledger_rows_item
