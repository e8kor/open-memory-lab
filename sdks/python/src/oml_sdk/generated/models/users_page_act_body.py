from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.users_page_act_body_action import UsersPageActBodyAction
from ..models.users_page_act_body_quota import UsersPageActBodyQuota
from ..types import UNSET, Unset

T = TypeVar("T", bound="UsersPageActBody")


@_attrs_define
class UsersPageActBody:
    """
    Attributes:
        action (UsersPageActBodyAction):
        csrf_token (str):
        user (str):
        quota (UsersPageActBodyQuota | Unset):
        limit (str | Unset):
    """

    action: UsersPageActBodyAction
    csrf_token: str
    user: str
    quota: UsersPageActBodyQuota | Unset = UNSET
    limit: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        action = self.action.value

        csrf_token = self.csrf_token

        user = self.user

        quota: str | Unset = UNSET
        if not isinstance(self.quota, Unset):
            quota = self.quota.value

        limit = self.limit

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "action": action,
                "csrf_token": csrf_token,
                "user": user,
            }
        )
        if quota is not UNSET:
            field_dict["quota"] = quota
        if limit is not UNSET:
            field_dict["limit"] = limit

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        action = UsersPageActBodyAction(d.pop("action"))

        csrf_token = d.pop("csrf_token")

        user = d.pop("user")

        _quota = d.pop("quota", UNSET)
        quota: UsersPageActBodyQuota | Unset
        if isinstance(_quota, Unset):
            quota = UNSET
        else:
            quota = UsersPageActBodyQuota(_quota)

        limit = d.pop("limit", UNSET)

        users_page_act_body = cls(
            action=action,
            csrf_token=csrf_token,
            user=user,
            quota=quota,
            limit=limit,
        )

        users_page_act_body.additional_properties = d
        return users_page_act_body

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
