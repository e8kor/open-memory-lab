from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="KeyLimitsRequest")


@_attrs_define
class KeyLimitsRequest:
    """A member sent replaces the value, null clears it, and a member not sent keeps it (ADR-0079 d5).

    Attributes:
        expiry (datetime.datetime | None | Unset): The instant from which the key authenticates nothing, later than the
            server's clock; null: the key does not expire.
        requests_per_minute (int | None | Unset): The key's own requests per UTC minute; null: none of its own.
    """

    expiry: datetime.datetime | None | Unset = UNSET
    requests_per_minute: int | None | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        expiry: None | str | Unset
        if isinstance(self.expiry, Unset):
            expiry = UNSET
        elif isinstance(self.expiry, datetime.datetime):
            expiry = self.expiry.isoformat()
        else:
            expiry = self.expiry

        requests_per_minute: int | None | Unset
        if isinstance(self.requests_per_minute, Unset):
            requests_per_minute = UNSET
        else:
            requests_per_minute = self.requests_per_minute

        field_dict: dict[str, Any] = {}

        field_dict.update({})
        if expiry is not UNSET:
            field_dict["expiry"] = expiry
        if requests_per_minute is not UNSET:
            field_dict["requests_per_minute"] = requests_per_minute

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)

        def _parse_expiry(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                expiry_type_0 = datetime.datetime.fromisoformat(data)

                return expiry_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        expiry = _parse_expiry(d.pop("expiry", UNSET))

        def _parse_requests_per_minute(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        requests_per_minute = _parse_requests_per_minute(
            d.pop("requests_per_minute", UNSET)
        )

        key_limits_request = cls(
            expiry=expiry,
            requests_per_minute=requests_per_minute,
        )

        return key_limits_request
