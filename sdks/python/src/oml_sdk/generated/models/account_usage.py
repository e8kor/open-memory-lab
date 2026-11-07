from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.account_usage_meters_item import AccountUsageMetersItem


T = TypeVar("T", bound="AccountUsage")


@_attrs_define
class AccountUsage:
    """An account's six meters against its limits in one UTC month (ADR-0071 d2, d5, d6). Counts only.

    Attributes:
        account (str): The id of the root workarea the account is.
        user (None | str):
        profile (None | str): The quota profile the limits come from; null for an account under no limits.
        profile_declared (bool): false when the configuration no longer declares the profile, which holds every limit at
            zero.
        period (str): The UTC month the flows count, YYYY-MM.
        meters (list[AccountUsageMetersItem]):
    """

    account: str
    user: None | str
    profile: None | str
    profile_declared: bool
    period: str
    meters: list[AccountUsageMetersItem]

    def to_dict(self) -> dict[str, Any]:
        account = self.account

        user: None | str
        user = self.user

        profile: None | str
        profile = self.profile

        profile_declared = self.profile_declared

        period = self.period

        meters = []
        for meters_item_data in self.meters:
            meters_item = meters_item_data.to_dict()
            meters.append(meters_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "account": account,
                "user": user,
                "profile": profile,
                "profile_declared": profile_declared,
                "period": period,
                "meters": meters,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.account_usage_meters_item import AccountUsageMetersItem

        d = dict(src_dict)
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

        profile_declared = d.pop("profile_declared")

        period = d.pop("period")

        meters = []
        _meters = d.pop("meters")
        for meters_item_data in _meters:
            meters_item = AccountUsageMetersItem.from_dict(meters_item_data)

            meters.append(meters_item)

        account_usage = cls(
            account=account,
            user=user,
            profile=profile,
            profile_declared=profile_declared,
            period=period,
            meters=meters,
        )

        return account_usage
