from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.account_profile_request_overrides import (
        AccountProfileRequestOverrides,
    )


T = TypeVar("T", bound="AccountProfileRequest")


@_attrs_define
class AccountProfileRequest:
    """
    Attributes:
        profile (str):
        overrides (AccountProfileRequestOverrides | Unset): A quota's limit for this account alone: memories,
            text_bytes, writes_per_month, keys, connections or requests_per_minute.
    """

    profile: str
    overrides: AccountProfileRequestOverrides | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        profile = self.profile

        overrides: dict[str, Any] | Unset = UNSET
        if not isinstance(self.overrides, Unset):
            overrides = self.overrides.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "profile": profile,
            }
        )
        if overrides is not UNSET:
            field_dict["overrides"] = overrides

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.account_profile_request_overrides import (
            AccountProfileRequestOverrides,
        )

        d = dict(src_dict)
        profile = d.pop("profile")

        _overrides = d.pop("overrides", UNSET)
        overrides: AccountProfileRequestOverrides | Unset
        if isinstance(_overrides, Unset):
            overrides = UNSET
        else:
            overrides = AccountProfileRequestOverrides.from_dict(_overrides)

        account_profile_request = cls(
            profile=profile,
            overrides=overrides,
        )

        return account_profile_request
