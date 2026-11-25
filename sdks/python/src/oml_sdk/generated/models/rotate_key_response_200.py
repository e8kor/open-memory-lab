from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="RotateKeyResponse200")


@_attrs_define
class RotateKeyResponse200:
    """
    Attributes:
        key_id (str):
        name (None | str):
        rotated_at (datetime.datetime):
        previous_valid_until (datetime.datetime | None): When the previous value stops authenticating; null where it
            stopped at once.
        journal_seq (int):
        value (str | Unset): The new value, shown once; absent from a replayed answer.
    """

    key_id: str
    name: None | str
    rotated_at: datetime.datetime
    previous_valid_until: datetime.datetime | None
    journal_seq: int
    value: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        key_id = self.key_id

        name: None | str
        name = self.name

        rotated_at = self.rotated_at.isoformat()

        previous_valid_until: None | str
        if isinstance(self.previous_valid_until, datetime.datetime):
            previous_valid_until = self.previous_valid_until.isoformat()
        else:
            previous_valid_until = self.previous_valid_until

        journal_seq = self.journal_seq

        value = self.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "name": name,
                "rotated_at": rotated_at,
                "previous_valid_until": previous_valid_until,
                "journal_seq": journal_seq,
            }
        )
        if value is not UNSET:
            field_dict["value"] = value

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

        rotated_at = datetime.datetime.fromisoformat(d.pop("rotated_at"))

        def _parse_previous_valid_until(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                previous_valid_until_type_0 = datetime.datetime.fromisoformat(data)

                return previous_valid_until_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        previous_valid_until = _parse_previous_valid_until(
            d.pop("previous_valid_until")
        )

        journal_seq = d.pop("journal_seq")

        value = d.pop("value", UNSET)

        rotate_key_response_200 = cls(
            key_id=key_id,
            name=name,
            rotated_at=rotated_at,
            previous_valid_until=previous_valid_until,
            journal_seq=journal_seq,
            value=value,
        )

        return rotate_key_response_200
