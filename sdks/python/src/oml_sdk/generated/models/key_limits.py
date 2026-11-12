from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="KeyLimits")


@_attrs_define
class KeyLimits:
    """A key's limits as they stand (ADR-0079 d5).

    Attributes:
        key_id (str):
        name (None | str):
        expiry (datetime.datetime | None):
        expired (bool): Whether the expiry has come on the server's clock.
        requests_per_minute (int | None):
    """

    key_id: str
    name: None | str
    expiry: datetime.datetime | None
    expired: bool
    requests_per_minute: int | None

    def to_dict(self) -> dict[str, Any]:
        key_id = self.key_id

        name: None | str
        name = self.name

        expiry: None | str
        if isinstance(self.expiry, datetime.datetime):
            expiry = self.expiry.isoformat()
        else:
            expiry = self.expiry

        expired = self.expired

        requests_per_minute: int | None
        requests_per_minute = self.requests_per_minute

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "name": name,
                "expiry": expiry,
                "expired": expired,
                "requests_per_minute": requests_per_minute,
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

        def _parse_expiry(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                expiry_type_0 = datetime.datetime.fromisoformat(data)

                return expiry_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        expiry = _parse_expiry(d.pop("expiry"))

        expired = d.pop("expired")

        def _parse_requests_per_minute(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        requests_per_minute = _parse_requests_per_minute(d.pop("requests_per_minute"))

        key_limits = cls(
            key_id=key_id,
            name=name,
            expiry=expiry,
            expired=expired,
            requests_per_minute=requests_per_minute,
        )

        return key_limits
