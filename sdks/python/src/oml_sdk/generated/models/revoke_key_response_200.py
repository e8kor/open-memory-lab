from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="RevokeKeyResponse200")


@_attrs_define
class RevokeKeyResponse200:
    """
    Attributes:
        key_id (str):
        name (None | str):
        revoked_at (datetime.datetime):
        act_id (None | str): The admin act the revocation rode; null for a person's own.
        journal_seq (int):
    """

    key_id: str
    name: None | str
    revoked_at: datetime.datetime
    act_id: None | str
    journal_seq: int

    def to_dict(self) -> dict[str, Any]:
        key_id = self.key_id

        name: None | str
        name = self.name

        revoked_at = self.revoked_at.isoformat()

        act_id: None | str
        act_id = self.act_id

        journal_seq = self.journal_seq

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "name": name,
                "revoked_at": revoked_at,
                "act_id": act_id,
                "journal_seq": journal_seq,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        key_id = d.pop("key_id")

        def _parse_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        name = _parse_name(d.pop("name"))

        revoked_at = datetime.datetime.fromisoformat(d.pop("revoked_at"))

        def _parse_act_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        act_id = _parse_act_id(d.pop("act_id"))

        journal_seq = d.pop("journal_seq")

        revoke_key_response_200 = cls(
            key_id=key_id,
            name=name,
            revoked_at=revoked_at,
            act_id=act_id,
            journal_seq=journal_seq,
        )

        return revoke_key_response_200
