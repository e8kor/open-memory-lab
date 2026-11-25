from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.web_user_quotas import WebUserQuotas
    from ..models.web_user_usage import WebUserUsage


T = TypeVar("T", bound="WebUser")


@_attrs_define
class WebUser:
    """One person of the tenant as GET /v2/web/users lists them and the quota update answers (ADR-0068 d5).

    Attributes:
        user (str):
        signed_up (bool):
        disabled (bool):
        under_quotas (bool):
        usage (WebUserUsage):
        quotas (WebUserQuotas):
        profile (None | str | Unset): The account's quota profile, null for none (ADR-0096 d9). Since v0.8.5.
        period (str | Unset): The UTC month the usage counts, YYYY-MM (ADR-0096 d9). Since v0.8.5.
        admin_keys (list[str] | Unset): Each live admin key whose row names the person's user root, by name: a row no
            verb makes, which refuses the person's disable with workspace.admin_key_under_root and which oml doctor warns
            about (ADR-0068 d8, d11). Since v0.7.1.
    """

    user: str
    signed_up: bool
    disabled: bool
    under_quotas: bool
    usage: WebUserUsage
    quotas: WebUserQuotas
    profile: None | str | Unset = UNSET
    period: str | Unset = UNSET
    admin_keys: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        user = self.user

        signed_up = self.signed_up

        disabled = self.disabled

        under_quotas = self.under_quotas

        usage = self.usage.to_dict()

        quotas = self.quotas.to_dict()

        profile: None | str | Unset
        if isinstance(self.profile, Unset):
            profile = UNSET
        else:
            profile = self.profile

        period = self.period

        admin_keys: list[str] | Unset = UNSET
        if not isinstance(self.admin_keys, Unset):
            admin_keys = self.admin_keys

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "user": user,
                "signed_up": signed_up,
                "disabled": disabled,
                "under_quotas": under_quotas,
                "usage": usage,
                "quotas": quotas,
            }
        )
        if profile is not UNSET:
            field_dict["profile"] = profile
        if period is not UNSET:
            field_dict["period"] = period
        if admin_keys is not UNSET:
            field_dict["admin_keys"] = admin_keys

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.web_user_quotas import WebUserQuotas
        from ..models.web_user_usage import WebUserUsage

        d = dict(src_dict)
        user = d.pop("user")

        signed_up = d.pop("signed_up")

        disabled = d.pop("disabled")

        under_quotas = d.pop("under_quotas")

        usage = WebUserUsage.from_dict(d.pop("usage"))

        quotas = WebUserQuotas.from_dict(d.pop("quotas"))

        def _parse_profile(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        profile = _parse_profile(d.pop("profile", UNSET))

        period = d.pop("period", UNSET)

        admin_keys = cast(list[str], d.pop("admin_keys", UNSET))

        web_user = cls(
            user=user,
            signed_up=signed_up,
            disabled=disabled,
            under_quotas=under_quotas,
            usage=usage,
            quotas=quotas,
            profile=profile,
            period=period,
            admin_keys=admin_keys,
        )

        web_user.additional_properties = d
        return web_user

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
