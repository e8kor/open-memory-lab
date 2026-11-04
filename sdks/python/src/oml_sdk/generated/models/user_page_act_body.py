from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.user_page_act_body_action import UserPageActBodyAction

T = TypeVar("T", bound="UserPageActBody")


@_attrs_define
class UserPageActBody:
    """
    Attributes:
        action (UserPageActBodyAction):
        csrf_token (str):
    """

    action: UserPageActBodyAction
    csrf_token: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        action = self.action.value

        csrf_token = self.csrf_token

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "action": action,
                "csrf_token": csrf_token,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        action = UserPageActBodyAction(d.pop("action"))

        csrf_token = d.pop("csrf_token")

        user_page_act_body = cls(
            action=action,
            csrf_token=csrf_token,
        )

        user_page_act_body.additional_properties = d
        return user_page_act_body

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
