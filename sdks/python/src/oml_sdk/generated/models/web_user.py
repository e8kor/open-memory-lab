from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

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
    """

    user: str
    signed_up: bool
    disabled: bool
    under_quotas: bool
    usage: WebUserUsage
    quotas: WebUserQuotas
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        user = self.user

        signed_up = self.signed_up

        disabled = self.disabled

        under_quotas = self.under_quotas

        usage = self.usage.to_dict()

        quotas = self.quotas.to_dict()

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

        web_user = cls(
            user=user,
            signed_up=signed_up,
            disabled=disabled,
            under_quotas=under_quotas,
            usage=usage,
            quotas=quotas,
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
