from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

T = TypeVar("T", bound="UsageDriftUndeclaredProfilesItem")


@_attrs_define
class UsageDriftUndeclaredProfilesItem:
    """
    Attributes:
        account (str):
        user (None | str):
        profile (str):
    """

    account: str
    user: None | str
    profile: str

    def to_dict(self) -> dict[str, Any]:
        account = self.account

        user: None | str
        user = self.user

        profile = self.profile

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "account": account,
                "user": user,
                "profile": profile,
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

        profile = d.pop("profile")

        usage_drift_undeclared_profiles_item = cls(
            account=account,
            user=user,
            profile=profile,
        )

        return usage_drift_undeclared_profiles_item
