from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.account_usage_meters_item_meter import AccountUsageMetersItemMeter
from ..models.account_usage_meters_item_state import AccountUsageMetersItemState

T = TypeVar("T", bound="AccountUsageMetersItem")


@_attrs_define
class AccountUsageMetersItem:
    """
    Attributes:
        meter (AccountUsageMetersItemMeter):
        used (int):
        limit (int | None):
        percent (int | None):
        state (AccountUsageMetersItemState):
    """

    meter: AccountUsageMetersItemMeter
    used: int
    limit: int | None
    percent: int | None
    state: AccountUsageMetersItemState

    def to_dict(self) -> dict[str, Any]:
        meter = self.meter.value

        used = self.used

        limit: int | None
        limit = self.limit

        percent: int | None
        percent = self.percent

        state = self.state.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "meter": meter,
                "used": used,
                "limit": limit,
                "percent": percent,
                "state": state,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        meter = AccountUsageMetersItemMeter(d.pop("meter"))

        used = d.pop("used")

        def _parse_limit(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        limit = _parse_limit(d.pop("limit"))

        def _parse_percent(data: object) -> int | None:
            if data is None:
                return data
            return cast(int | None, data)

        percent = _parse_percent(d.pop("percent"))

        state = AccountUsageMetersItemState(d.pop("state"))

        account_usage_meters_item = cls(
            meter=meter,
            used=used,
            limit=limit,
            percent=percent,
            state=state,
        )

        return account_usage_meters_item
