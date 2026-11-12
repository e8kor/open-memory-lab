from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.usage_drift_drift_item_meter import UsageDriftDriftItemMeter

T = TypeVar("T", bound="UsageDriftDriftItem")


@_attrs_define
class UsageDriftDriftItem:
    """
    Attributes:
        account (str):
        user (None | str):
        meter (UsageDriftDriftItemMeter):
        counter (int):
        rows (int):
    """

    account: str
    user: None | str
    meter: UsageDriftDriftItemMeter
    counter: int
    rows: int

    def to_dict(self) -> dict[str, Any]:
        account = self.account

        user: None | str
        user = self.user

        meter = self.meter.value

        counter = self.counter

        rows = self.rows

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "account": account,
                "user": user,
                "meter": meter,
                "counter": counter,
                "rows": rows,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        account = d.pop("account")

        def _parse_user(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        user = _parse_user(d.pop("user"))

        meter = UsageDriftDriftItemMeter(d.pop("meter"))

        counter = d.pop("counter")

        rows = d.pop("rows")

        usage_drift_drift_item = cls(
            account=account,
            user=user,
            meter=meter,
            counter=counter,
            rows=rows,
        )

        return usage_drift_drift_item
