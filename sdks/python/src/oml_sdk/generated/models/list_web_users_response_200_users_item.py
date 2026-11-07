from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.list_web_users_response_200_users_item_quotas import (
        ListWebUsersResponse200UsersItemQuotas,
    )
    from ..models.list_web_users_response_200_users_item_usage import (
        ListWebUsersResponse200UsersItemUsage,
    )


T = TypeVar("T", bound="ListWebUsersResponse200UsersItem")


@_attrs_define
class ListWebUsersResponse200UsersItem:
    """
    Attributes:
        user (str):
        signed_up (bool):
        disabled (bool):
        under_quotas (bool):
        usage (ListWebUsersResponse200UsersItemUsage):
        quotas (ListWebUsersResponse200UsersItemQuotas):
    """

    user: str
    signed_up: bool
    disabled: bool
    under_quotas: bool
    usage: ListWebUsersResponse200UsersItemUsage
    quotas: ListWebUsersResponse200UsersItemQuotas
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
        from ..models.list_web_users_response_200_users_item_quotas import (
            ListWebUsersResponse200UsersItemQuotas,
        )
        from ..models.list_web_users_response_200_users_item_usage import (
            ListWebUsersResponse200UsersItemUsage,
        )

        d = dict(src_dict)
        user = d.pop("user")

        signed_up = d.pop("signed_up")

        disabled = d.pop("disabled")

        under_quotas = d.pop("under_quotas")

        usage = ListWebUsersResponse200UsersItemUsage.from_dict(d.pop("usage"))

        quotas = ListWebUsersResponse200UsersItemQuotas.from_dict(d.pop("quotas"))

        list_web_users_response_200_users_item = cls(
            user=user,
            signed_up=signed_up,
            disabled=disabled,
            under_quotas=under_quotas,
            usage=usage,
            quotas=quotas,
        )

        list_web_users_response_200_users_item.additional_properties = d
        return list_web_users_response_200_users_item

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
